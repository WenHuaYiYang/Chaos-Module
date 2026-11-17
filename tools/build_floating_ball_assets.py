#!/usr/bin/env python3
"""Encode the flat function-ball base, rounded arc masks and OFL glyph atlas.

Pillow is required. NumPy accelerates mask rasterization when available;
the standard-library fallback produces the same four-sample geometry.
The normal firmware build uses the checked-in assets and needs no extra fonts.
"""
from pathlib import Path
import hashlib
import json
import math
import struct
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'supervisor/assets/floating-ball'
SIDE = 128
SAMPLES = 4
CHARS = ''.join(sorted(set('0123456789:mint常亮度截屏结束表盘震动自动录翻转'), key=ord))


def rle(data):
    out = bytearray()
    at = 0
    while at < len(data):
        run = 1
        while at+run < len(data) and data[at+run] == data[at] and run < 128: run += 1
        if run >= 3:
            out.extend((0x80 | (run-1),data[at])); at += run
        else:
            start = at; at += 1
            while at < len(data) and at-start < 128:
                if at+2 < len(data) and data[at] == data[at+1] == data[at+2]: break
                at += 1
            out.append(at-start-1); out.extend(data[start:at])
    return out


def shape(size):
    outer = size / 2 + 106
    inner = outer - 64
    middle = (inner + outer) / 2
    rounding = 10
    n = SIDE * SAMPLES
    try:
        import numpy as np
        v, u = np.mgrid[:n, :n].astype(float)
        u = (u + .5) * outer / n
        v = (v + .5) * outer / n
        ring = np.abs(np.hypot(u, v) - middle) - (32 - rounding)
        a, b = rounding - u, rounding - v
        sdf = np.sqrt(np.maximum(ring, 0)**2 + np.maximum(a, 0)**2 + np.maximum(b, 0)**2)
        sdf += np.minimum(np.maximum(np.maximum(ring, a), b), 0) - rounding
        coverage = ((sdf <= 0) * 255).astype('uint8')
        dividers = np.zeros((n, n), dtype=bool)
        for angle in (math.pi / 6, math.pi / 3):
            ca, sa = math.cos(angle), math.sin(angle)
            x, y = (inner + 12) * ca, (inner + 12) * sa
            dx, dy = 40 * ca, 40 * sa
            t = np.clip(((u-x)*dx + (v-y)*dy) / (dx*dx + dy*dy), 0, 1)
            dividers |= (u-x-t*dx)**2 + (v-y-t*dy)**2 <= 1.5**2
        separators = (dividers * 255).astype('uint8')
        images = [Image.fromarray(coverage), Image.fromarray(separators)]
    except ImportError:
        coverage, separators = bytearray(n*n), bytearray(n*n)
        for y in range(n):
            for x in range(n):
                u, v = (x+.5)*outer/n, (y+.5)*outer/n
                ring = abs(math.hypot(u, v)-middle) - (32-rounding)
                a, b = rounding-u, rounding-v
                sdf = math.sqrt(max(ring,0)**2+max(a,0)**2+max(b,0)**2)+min(max(ring,a,b),0)-rounding
                coverage[y*n+x] = 255 if sdf <= 0 else 0
                for angle in (math.pi/6, math.pi/3):
                    ca, sa = math.cos(angle), math.sin(angle)
                    sx, sy = (inner+12)*ca, (inner+12)*sa
                    dx, dy = 40*ca, 40*sa
                    t = min(1,max(0,((u-sx)*dx+(v-sy)*dy)/(dx*dx+dy*dy)))
                    if (u-sx-t*dx)**2+(v-sy-t*dy)**2 <= 1.5**2: separators[y*n+x] = 255
        images = [Image.frombytes('L',(n,n),bytes(b)) for b in (coverage,separators)]
    return [image.resize((SIDE,SIDE),Image.Resampling.LANCZOS).tobytes() for image in images]


def masks():
    table = bytearray()
    payload = bytearray()
    start = 8 + 41 * 16
    for size in range(32,73):
        row = []
        for mask in shape(size):
            encoded = rle(mask)
            row.extend((start+len(payload),len(encoded)))
            payload.extend(encoded)
        table.extend(struct.pack('<IIII',*row))
    return b'ARB2'+struct.pack('<BBH',32,72,SIDE)+table+payload


def glyphs():
    side = 28
    data = bytearray()
    records = bytearray()
    for c in CHARS:
        size = 16 if ord(c)<128 else 20
        font = ImageFont.truetype(str(ASSETS/'label-font.otf'),size*SAMPLES)
        image = Image.new('L',(side*SAMPLES,side*SAMPLES))
        ImageDraw.Draw(image).text((side*SAMPLES/2,side*SAMPLES/2),c,font=font,fill=255,anchor='mm')
        data.extend(image.resize((side,side),Image.Resampling.LANCZOS).tobytes())
        advance = round(font.getlength(c) / SAMPLES * 256)
        records.extend(struct.pack('<IHH',ord(c),advance,0))
    return b'GAF1'+struct.pack('<HBB',len(CHARS),side,0)+records+data


def rounded_box(x,y,left,top,right,bottom,radius):
    cx = min(right-radius,max(left+radius,x))
    cy = min(bottom-radius,max(top+radius,y))
    return (x-cx)**2+(y-cy)**2 <= radius**2


def sphere():
    """Flat disk and battery glyph: no photographic shading or outer bevel.

    The broad C-shaped battery stroke is composed at runtime from its A8 mask.
    Every interior material uses one solid colour; only edge coverage varies.
    """
    out = bytearray()
    background, foreground = (20,23,28), (228,231,237)
    for y in range(64):
        for x in range(64):
            disk = icon = 0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    u,v = x+(sx+.5)/SAMPLES,y+(sy+.5)/SAMPLES
                    disk += (u-32)**2+(v-32)**2 < 31.5**2
                    outline = rounded_box(u,v,22,27,40,37,2.8) and not rounded_box(u,v,24,29,38,35,1.0)
                    tip = rounded_box(u,v,40,30,42.5,34,0.8)
                    cell = rounded_box(u,v,25.5,30.5,36.5,33.5,0.8)
                    icon += outline or tip or cell
            coverage = round(disk*255/(SAMPLES*SAMPLES))
            ink = icon/(SAMPLES*SAMPLES)
            colour = [round(background[c]*(1-ink)+foreground[c]*ink) for c in range(3)]
            out.extend((colour[2]*coverage//255,colour[1]*coverage//255,colour[0]*coverage//255,coverage))
    return bytes(out)


def battery_stroke():
    """Full 360-degree flat annulus; the moving end marker is drawn at runtime."""
    out = bytearray()
    radius, half_width = 24.0,6.25
    for y in range(64):
        for x in range(64):
            coverage = 0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    dx,dy = x+(sx+.5)/SAMPLES-32,y+(sy+.5)/SAMPLES-32
                    coverage += abs(math.hypot(dx,dy)-radius) <= half_width
            out.append(round(255*coverage/(SAMPLES*SAMPLES)))
    return bytes(out)

def main():
    artwork = sphere()
    generated = {'arc-masks.bin':masks(),'glyphs.bin':glyphs(),'sphere.bgra':artwork,
                 'battery-stroke.a8':battery_stroke()}
    for name, data in generated.items():
        (ASSETS/name).write_bytes(data)
    metadata = {'font_source':'https://github.com/notofonts/noto-cjk/tree/main/Sans',
                'font_license':'OFL.txt','font_subset_family':'Chaos Arc UI','chars':CHARS,
                'mask_sampling':4,'battery_stroke':{'start':'bottom','closed_track':True,'sweep_degrees':360,'moving_end':'hollow dot','style':'flat solid colours','bevel':False,'shadow':False},'sizes':[32,72],
                'sha256':{name:hashlib.sha256(data).hexdigest() for name,data in generated.items()}}
    (ASSETS/'ASSETS.json').write_text(json.dumps(metadata,ensure_ascii=False,indent=2)+'\n')
    print({name:len(data) for name,data in generated.items()})


if __name__ == '__main__':
    main()
