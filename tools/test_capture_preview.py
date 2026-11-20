#!/usr/bin/env python3
"""Focused screenshot preview validation: exact pixels, corrupt inputs and UI flow."""
from pathlib import Path
import json, os, struct, subprocess, tempfile, zlib
from PIL import Image
ROOT=Path(__file__).resolve().parents[1]
def chunk(tag,data):
    return struct.pack('>I',len(data))+tag+data+struct.pack('>I',zlib.crc32(tag+data))
def png(w,h,raw,color=2,depth=8):
    compressed=zlib.compress(raw)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,depth,color,0,0,0))+chunk(b'IDAT',compressed[:5])+chunk(b'IDAT',compressed[5:])+chunk(b'IEND',b'')
def averaged(rgb,w,h,dw,dh):
    out=bytearray()
    for y in range(dh):
        top,bottom=y*h//dh,max(y*h//dh+1,(y+1)*h//dh)
        for x in range(dw):
            left,right=x*w//dw,max(x*w//dw+1,(x+1)*w//dw)
            n=(right-left)*(bottom-top)
            total=[0,0,0]
            for sy in range(top,bottom):
                for sx in range(left,right):
                    p=(sy*w+sx)*3
                    for c in range(3): total[c]+=rgb[p+2-c]
            out.extend((v+n//2)//n for v in total)
    return bytes(out)
def main():
    with tempfile.TemporaryDirectory(prefix='chaos-shot-preview-') as tmp:
        d=Path(tmp);manifest=d/'Cargo.toml'
        manifest.write_text('[package]\nname="chaos-preview-check"\nversion="0.0.0"\nedition="2021"\n[dependencies]\nminiz_oxide={version="=0.8.9",default-features=false}\n[[bin]]\nname="decode"\npath='+json.dumps(str(ROOT/'supervisor/tests/capture_preview_decode.rs'))+'\n[[test]]\nname="lifecycle"\npath='+json.dumps(str(ROOT/'supervisor/tests/capture_preview_harness.rs'))+'\n')
        subprocess.run(['cargo','build','--offline','--manifest-path',str(manifest),'--bin','decode'],check=True)
        exe=d/'target/debug/decode'
        def run(mode,*args):subprocess.run([str(exe),mode,*map(str,args)],check=True)
        w,h=336,480;rgb=bytes((x*17+y*31+c*59)%256 for y in range(h) for x in range(w) for c in range(3))
        bgr=bytes(v for i in range(0,len(rgb),3) for v in rgb[i:i+3][::-1]);(d/'source.bgr').write_bytes(bgr)
        run('encode',d/'source.bgr',d/'module.png')
        assert Image.open(d/'module.png').convert('RGB').tobytes()==rgb
        # All PNG scanline filters, independent zlib dynamic Huffman and split IDAT.
        rows=bytearray();previous=bytes(w*3)
        for y in range(h):
            line=rgb[y*w*3:(y+1)*w*3];f=y%5;rows.append(f)
            for x,v in enumerate(line):
                a=line[x-3] if x>=3 else 0;b=previous[x];c=previous[x-3] if x>=3 else 0
                p=a+b-c;ds=[abs(p-a),abs(p-b),abs(p-c)];paeth=[a,b,c][ds.index(min(ds))]
                rows.append((v-[0,a,b,(a+b)//2,paeth][f])%256)
            previous=line
        (d/'filters.png').write_bytes(png(w,h,rows))
        Image.frombytes('RGB',(w,h),rgb).save(d/'bottom-up.bmp')
        # The actual module BMP encoder uses top-down BGR24.
        header=bytearray(54);header[:2]=b'BM';struct.pack_into('<I',header,2,54+len(bgr));struct.pack_into('<I',header,10,54)
        struct.pack_into('<IiiHHII',header,14,40,w,-h,1,24,0,len(bgr));(d/'top-down.bmp').write_bytes(header+bgr)
        for filename in ('module.png','filters.png','bottom-up.bmp','top-down.bmp'):
            for dw,dh in ((w,h),(84,120),(280,400)):
                expected=d/f'expected-{dw}-{dh}.bgr'
                if not expected.exists():expected.write_bytes(averaged(rgb,w,h,dw,dh))
                run('decode',d/filename,expected,dw,dh)
        # Padded BMP and common non-RGB PNG colour modes.
        for mode in ('RGBA','L','P'):
            im=Image.frombytes('RGB',(w,h),rgb).convert(mode);im.save(d/f'{mode}.png')
            converted=im.convert('RGBA');bg=Image.new('RGBA',im.size,(0,0,0,255));bg.alpha_composite(converted)
            expected=d/f'{mode}.bgr';expected.write_bytes(averaged(bg.convert('RGB').tobytes(),w,h,84,120))
            run('decode',d/f'{mode}.png',expected,84,120)
        im=Image.new('RGB',(5,3),(255,128,7));im.save(d/'padded.bmp');(d/'padded.bgr').write_bytes(bytes([7,128,255])*15)
        run('decode',d/'padded.bmp',d/'padded.bgr',5,3)
        valid=(d/'module.png').read_bytes();bad_crc=bytearray(valid);bad_crc[-1]^=1
        for i,bad in enumerate((valid[:40],bad_crc,png(20000,20000,b'\0'),png(w,h,b'\x05'+bytes(w*3)),bytes(header)+bgr[:-1],png(w,h,rows[:-1]))):
            path=d/f'bad-{i}';path.write_bytes(bad);run('reject',path)
        subprocess.run(['cargo','test','--offline','--manifest-path',str(manifest),'--test','lifecycle','--','--test-threads=1'],check=True)
        page=d/'page';subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'supervisor/tests/capture_harness.rs'),'-o',str(page)],check=True)
        subprocess.run([str(page),'screenshot_preview','--test-threads=1'],check=True)
    print('PASS BMP/PNG original pixels, thumbnails, filters/deflate, malformed files, image retirement and confirmed-delete UI')
if __name__=='__main__':main()
