#!/usr/bin/env python3
"""Build legacy-compatible CIPK icon deliveries, optionally with DGF1 animations."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import tempfile
from PIL import Image
import build_gif_icon as gif
from gen_chaos_icon import encode_icon, render_icon
import p67_pack

ROOT=Path(__file__).resolve().parents[1]
MAX_ANIMATION=1024*1024
MAX_STATIC=512*1024
MAX_FILES=256
NAME=re.compile(r'[A-Za-z0-9_]+\.bin\Z',re.ASCII)

def launcher_stems():
    source=(ROOT/'supervisor/src/icon_apply.rs').read_text()
    block=source.split('const ICON_STEMS:',1)[1].split('= [',1)[1].split('];',1)[0]
    stems=re.findall(r'b"([a-z0-9_]+)"',block)
    if len(stems)!=38 or len(set(stems))!=38: raise ValueError('launcher stem ABI changed')
    return stems

def system_stems():
    return set(re.findall(r'stem: b"([a-zA-Z0-9_]+)"',(ROOT/'supervisor/src/res_hook.rs').read_text()))

def validate_animation(data):
    if len(data)<1084 or len(data)>MAX_ANIMATION: raise ValueError('animation must fit within 1 MiB')
    magic,w,h,count,reserved,cycle,plays,table,payload,digest=gif.HEADER.unpack_from(data)
    if (magic,w,h,reserved)!=(b'DGF1',96,96,0) or not 1<=count<=256: raise ValueError('invalid 96px DGF1 header')
    if table!=1084 or payload!=table+count*12 or payload>len(data): raise ValueError('invalid frame table')
    end=payload;total=0
    for frame in range(count):
        delay,start,size=struct.unpack_from('<III',data,table+frame*12)
        if not 1<=delay<=60000 or start!=end or not 1<=size<=96*96*2 or start+size>len(data): raise ValueError('invalid frame record')
        total+=delay;end=start+size
        encoded=data[start:end];at=0;written=0
        while at<len(encoded):
            tag=encoded[at];at+=1;n=(tag&127)+1
            at+=1 if tag&128 else n;written+=n
            if at>len(encoded) or written>96*96: raise ValueError('invalid frame compression')
        if written!=96*96: raise ValueError('incomplete frame')
    if end!=len(data) or total!=cycle: raise ValueError('invalid cycle or trailing data')
    return {'frames':count,'cycle_ms':cycle,'plays':plays,'sha256':digest.hex()}

def validate_static(data,animated=False):
    if not 12<len(data)<=MAX_STATIC or data[0]!=0x19: raise ValueError('invalid native static image')
    w,h,stride=struct.unpack_from('<HHI',data,4)
    if not 1<=w<=512 or not 1<=h<=512: raise ValueError('invalid image dimensions')
    if animated and (w,h)!=(96,96): raise ValueError('animated poster must also be 96x96')
    if data[1] in (15,16,10):
        minimum=w*({15:3,16:4,10:1}[data[1]])
        palette=1024 if data[1]==10 else 0
        if not minimum<=stride<=minimum+64 or len(data)!=12+palette+stride*h: raise ValueError('invalid native pixels/stride')
    return w,h

def encode_cipk(entries):
    if not 1<=len(entries)<=MAX_FILES: raise ValueError('CIPK requires 1..256 files')
    seen=set();result=bytearray(b'CIPK'+struct.pack('<I',len(entries)))
    for name,data in entries:
        if not NAME.fullmatch(name) or len(name)>64 or name in seen: raise ValueError('invalid or duplicate flat filename: '+name)
        seen.add(name)
        limit=MAX_ANIMATION if name.endswith('_anim.bin') else MAX_STATIC
        if not 1<=len(data)<=limit: raise ValueError('invalid file length: '+name)
        encoded=name.encode('ascii')
        result.extend(struct.pack('<IB',len(data),len(encoded))+encoded+data)
    return bytes(result)

def decode_cipk(data):
    if len(data)<8 or data[:4]!=b'CIPK': raise ValueError('invalid CIPK magic')
    count=struct.unpack_from('<I',data,4)[0]
    if not 1<=count<=MAX_FILES: raise ValueError('invalid CIPK count')
    at=8;entries=[];seen=set()
    for _ in range(count):
        if at+5>len(data): raise ValueError('truncated CIPK record')
        size,n=struct.unpack_from('<IB',data,at);at+=5
        if not 1<=n<=64 or at+n+size>len(data): raise ValueError('truncated CIPK name/data')
        name=data[at:at+n].decode('ascii');at+=n
        limit=MAX_ANIMATION if name.endswith('_anim.bin') else MAX_STATIC
        if not NAME.fullmatch(name) or name in seen or not 1<=size<=limit: raise ValueError('invalid CIPK record')
        seen.add(name);entries.append((name,data[at:at+size]));at+=size
    if at!=len(data): raise ValueError('trailing CIPK data')
    return entries

def collect(assets):
    stems=launcher_stems();allowed=set(stems)|system_stems()
    posters={};animations={};details={};explicit_posters=set()
    files=sorted(Path(assets).iterdir())
    for path in files:
        if path.is_dir(): raise ValueError('asset directory must be flat: '+path.name)
        if path.name.startswith('.'): continue
        if path.suffix.lower()=='.bin' and path.stem.endswith('_anim'):
            stem=path.stem[:-5]
            if stem not in stems: raise ValueError('only launcher icons support animations: '+stem)
            if stem in animations: raise ValueError('duplicate animation: '+stem)
            animations[stem]=path.read_bytes();details[stem]=validate_animation(animations[stem]);continue
        stem=path.stem
        if stem not in allowed: raise ValueError('unknown asset stem: '+stem)
        if path.suffix.lower()=='.gif':
            if stem not in stems: raise ValueError('system UI resources require static .bin: '+stem)
            if stem in animations: raise ValueError('duplicate animation: '+stem)
            animations[stem]=gif.convert(path);details[stem]=validate_animation(animations[stem])
            if stem not in posters:
                with Image.open(path) as image: posters[stem]=encode_icon(render_icon(image,96))
        elif path.suffix.lower()=='.png':
            if stem not in stems: raise ValueError('system UI resources require native .bin: '+stem)
            if stem in explicit_posters: raise ValueError('duplicate static poster: '+stem)
            explicit_posters.add(stem)
            with Image.open(path) as image: posters[stem]=encode_icon(render_icon(image,96))
        elif path.suffix.lower()=='.bin':
            if stem in explicit_posters: raise ValueError('duplicate static poster: '+stem)
            explicit_posters.add(stem)
            posters[stem]=path.read_bytes()
        else: raise ValueError('unsupported asset: '+path.name)
    for stem,data in animations.items():
        if stem not in posters: raise ValueError('animation needs a static poster: '+stem)
    entries=[]
    for stem,data in sorted(posters.items()):
        validate_static(data,stem in animations);entries.append((stem+'.bin',data))
    mask=0
    for stem,data in sorted(animations.items()):
        mask|=1<<stems.index(stem);entries.append((stem+'_anim.bin',data))
    if not entries: raise ValueError('no icon assets')
    entries.append(('animations.bin',struct.pack('<4s4HQ',b'CIA1',96,96,38,0,mask)))
    return entries,details

def delivery_lua(name,title):
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,12}',name,re.ASCII): raise ValueError('pack name must be 1..12 ASCII letters/digits/_/-')
    source=(ROOT/'installer/icon_pack.lua').read_text()
    for marker,value in [('__PACK_NAME__',name),('__PACK_TITLE__',title)]:
        if source.count('"'+marker+'"')!=1: raise ValueError('invalid Lua template marker')
        source=source.replace('"'+marker+'"',json.dumps(value,ensure_ascii=False))
    return source

def build(assets,name,output,*,title=None,preview=None,ident=None,raw_output=None):
    title=title or '图标投递 '+name
    entries,details=collect(assets);payload=encode_cipk(entries)
    assert decode_cipk(payload)==entries
    if raw_output:
        Path(raw_output).parent.mkdir(parents=True,exist_ok=True);Path(raw_output).write_bytes(payload)
    output=Path(output);output.parent.mkdir(parents=True,exist_ok=True)
    ident=ident or f'{int.from_bytes(hashlib.sha256(name.encode()).digest()[:6],"little")%10**12:012d}'
    if not re.fullmatch(r'\d{12}',ident,re.ASCII): raise ValueError('container ID must be 12 decimal digits')
    with tempfile.TemporaryDirectory(prefix='chaos-icon-delivery-') as directory:
        stage=Path(directory)
        (stage/'main.lua').write_text(delivery_lua(name,title),encoding='utf-8')
        (stage/'pack.bin').write_bytes(payload)
        result=p67_pack.build({'id':ident,'name':title,'styleName':title,
            'preview':str(preview or ROOT/'tools/p67-preview.block'),
            'files':[{'name':'_lua/xiaomi-band-10-pro/'+file,'path':str(stage/file)} for file in ('main.lua','pack.bin')]},output)
    return {'output':str(output),'bytes':output.stat().st_size,'files':len(entries),'animations':details,'container':result}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets',type=Path,required=True,help='flat native .bin / launcher .png / launcher .gif directory')
    parser.add_argument('--name',required=True,help='1..12 ASCII letters/digits/_/-')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--title');parser.add_argument('--preview',type=Path);parser.add_argument('--id')
    parser.add_argument('--raw-output',type=Path,help='also write standalone CIPK pack.bin')
    args=parser.parse_args()
    try:result=build(args.assets,args.name,args.output,title=args.title,preview=args.preview,ident=args.id,raw_output=args.raw_output)
    except (ValueError,OSError) as error: parser.error(str(error))
    print(json.dumps(result,ensure_ascii=False,indent=2))

if __name__=='__main__': main()
