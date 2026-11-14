#!/usr/bin/env python3
"""从用户提供的控制按钮 RLE 素材生成适配 p67 的预乘 BGRA。"""
import argparse
from pathlib import Path
import re

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
args = parser.parse_args()
text = args.source.read_text()
out = Path(__file__).resolve().parents[1] / 'supervisor/assets/s541'
out.mkdir(parents=True, exist_ok=True)
for mode in range(3):
    body = re.search(r'selector_asset_' + str(mode) + r'\[\]\s*=\s*\{([^}]+)', text).group(1)
    rle = [int(x) for x in re.findall(r'\d+', body)]
    alpha = bytearray()
    for n, value in zip(rle[::2], rle[1::2]):
        alpha.extend(bytes([value]) * n)
    assert len(alpha) == 360 * 120
    pixels = bytearray()
    for y in range(96):
        for x in range(288):
            pixels.extend(bytes([alpha[(y * 120 // 96) * 360 + x * 360 // 288]]) * 4)
    (out / f'selector-{mode}.bgra').write_bytes(pixels)
print('已导入三个原版按钮状态，288×96预乘BGRA')
