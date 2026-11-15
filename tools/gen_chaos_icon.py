#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""gen_chaos_icon.py — 从一张方图生成手环的应用图标 `chaos_icon.bin`。

画布 96x96，保留原版圆形外沿的透明间距，4 倍超采样再 LANCZOS 缩到 96。
格式: 12 字节头
`[magic=0x19, cf=0x10, flags=0, reserved=0, w u16, h u16, stride u32]` + 逐像素 BGRA
(stride = 96*4 = 384, 像素共 36,864 字节)。

用法: python3 tools/gen_chaos_icon.py [源图] [输出]
  默认 chaos.png。
"""
import os
import struct
import sys

from PIL import Image, ImageDraw

_HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SRC = os.path.join(_HERE, 'chaos.png')
SRC = sys.argv[1] if len(sys.argv) > 1 else DEFAULT_SRC
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(_HERE, '..', 'chaos_icon.bin')

SIZE = int(os.environ.get('CHAOS_ICON_SIZE', '96'))
SS = 4
DIAMETER = int(round(SIZE * 100 / 112))


def render_icon(img, size=SIZE):
    """Use the same size, circular mask and alpha for static and GIF frames."""
    img = img.convert('RGBA')
    big = size * SS
    center = big // 2
    radius = (int(round(size * 100 / 112)) // 2) * SS

    img_big = img.resize((big, big), Image.LANCZOS)

    mask_big = Image.new('L', (big, big), 0)
    ImageDraw.Draw(mask_big).ellipse(
        [center - radius, center - radius, center + radius, center + radius],
        fill=255,
    )

    r, g, b, a = img_big.split()
    a = Image.composite(a, mask_big, mask_big)
    img_big = Image.merge('RGBA', (r, g, b, a))

    return img_big.resize((size, size), Image.LANCZOS)


def encode_icon(image):
    width, height = image.size
    return bytes((0x19, 0x10, 0, 0)) + struct.pack('<HHI', width, height, width * 4) + image.tobytes('raw', 'BGRA')


def generate():
    with Image.open(SRC) as source:
        img_final = render_icon(source, SIZE)

    px = img_final.load()
    for x, y in ((0, 0), (SIZE - 1, 0), (0, SIZE - 1), (SIZE - 1, SIZE - 1)):
        assert px[x, y][3] == 0, '四角必须是透明的: (%d,%d) alpha=%d' % (x, y, px[x, y][3])

    data = encode_icon(img_final)
    assert len(data) == 12 + SIZE * SIZE * 4, '长度不对: %d' % len(data)

    with open(OUT, 'wb') as f:
        f.write(data)
    print('OK %s: %d 字节' % (OUT, len(data)))
    return len(data)


if __name__ == '__main__':
    generate()
