#!/usr/bin/env python3
"""从原始PNG（或旧RLE）生成抗锯齿、预乘BGRA控制条及透明轮廓。"""
import argparse
from pathlib import Path
import re
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
args = parser.parse_args()
out = Path(__file__).resolve().parents[1] / 'supervisor/assets/s541'
out.mkdir(parents=True, exist_ok=True)
if args.source.is_dir():
    background = Image.open(args.source / 'btn_bg.png').convert('RGBA')
    assert background.size == (360, 120)
    background.getchannel('A').resize((288, 96), Image.Resampling.LANCZOS).save(out / 'background-mask.png')
    (out / 'background.alpha').write_bytes(background.getchannel('A').resize((288, 96), Image.Resampling.LANCZOS).tobytes())
    names = ['grid', 'grid_text', 'list']
    for mode in range(3):
        combined = background.copy()
        for i, name in enumerate(names):
            # 白色圆片镂空图标作当前项，其余为半透明图标；保留原PNG的alpha边缘。
            part = Image.open(args.source / (name + ('_deselected.png' if i == mode else '_selected.png'))).convert('RGBA')
            combined.alpha_composite(part, (4 + i * 120, 4))
        scaled = combined.resize((288, 96), Image.Resampling.LANCZOS)
        pixels = bytearray()
        raw = scaled.tobytes()
        for r, g, b, a in zip(raw[0::4], raw[1::4], raw[2::4], raw[3::4]):
            pixels.extend((b * a // 255, g * a // 255, r * a // 255, a))
        # 运行时保留原偏好索引0/2/1，视觉顺序仍为图标/图标文字/列表。
        (out / f'selector-{[0,2,1][mode]}.bgra').write_bytes(pixels)
    print('已从原版PNG合成透明控制条，使用Lanczos抗锯齿与预乘BGRA')
    raise SystemExit(0)
text = args.source.read_text()
for mode in range(3):
    body = re.search(r'selector_asset_' + str(mode) + r'\[\]\s*=\s*\{([^}]+)', text).group(1)
    rle = [int(x) for x in re.findall(r'\d+', body)]
    alpha = bytearray()
    for n, value in zip(rle[::2], rle[1::2]):
        alpha.extend(bytes([value]) * n)
    assert len(alpha) == 360 * 120
    alpha = Image.frombytes('L', (360, 120), bytes(alpha)).resize((288, 96), Image.Resampling.LANCZOS).tobytes()
    pixels = bytearray()
    for a in alpha:
        pixels.extend(bytes([a]) * 4)
    (out / f'selector-{mode}.bgra').write_bytes(pixels)
print('已导入三个原版按钮状态，288×96预乘BGRA')
