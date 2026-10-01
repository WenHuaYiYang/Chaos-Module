#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""gen_chaos_icon.py — 从一张方图生成手环的应用图标 `chaos_icon.bin`。

画布 112x112, 圆形直径 100px(圆心 56,56), 4 倍超采样再 LANCZOS 缩到 112 —— 四边各留 6px
透明间距, 与固件自带应用图标同规格。格式: 12 字节头
`[magic=0x19, cf=0x10, flags=0, reserved=0, w u16, h u16, stride u32]` + 逐像素 BGRA
(stride = 112*4 = 448, 像素共 50,176 字节)。

用法: python3 tools/gen_chaos_icon.py [源图] [输出]
  默认 <本脚本目录>/chaos.png -> <仓库根>/chaos_icon.bin
"""
import os
import struct
import sys

from PIL import Image, ImageDraw

_HERE = os.path.dirname(os.path.abspath(__file__))
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(_HERE, 'chaos.png')
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(_HERE, '..', 'chaos_icon.bin')

SIZE = 112
SS = 4
DIAMETER = 100


def generate():
    img = Image.open(SRC).convert('RGBA')

    big = SIZE * SS
    center = big // 2
    radius = (DIAMETER // 2) * SS

    img_big = img.resize((big, big), Image.LANCZOS)

    mask_big = Image.new('L', (big, big), 0)
    ImageDraw.Draw(mask_big).ellipse(
        [center - radius, center - radius, center + radius, center + radius],
        fill=255,
    )

    r, g, b, a = img_big.split()
    a = Image.composite(a, mask_big, mask_big)
    img_big = Image.merge('RGBA', (r, g, b, a))

    img_final = img_big.resize((SIZE, SIZE), Image.LANCZOS)

    px = img_final.load()
    for x, y in ((0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)):
        assert px[x, y][3] == 0, '四角必须是透明的: (%d,%d) alpha=%d' % (x, y, px[x, y][3])

    r, g, b, a = img_final.split()
    bgra = b''.join(
        bytes(pixel) for pixel in zip(b.getdata(), g.getdata(), r.getdata(), a.getdata())
    )

    header = bytes([0x19, 0x10, 0x00, 0x00])
    header += struct.pack('<HH', SIZE, SIZE)
    header += struct.pack('<I', SIZE * 4)

    data = header + bgra
    assert len(data) == 12 + SIZE * SIZE * 4, '长度不对: %d' % len(data)

    with open(OUT, 'wb') as f:
        f.write(data)
    print('OK %s: %d 字节' % (OUT, len(data)))
    return len(data)


if __name__ == '__main__':
    generate()
