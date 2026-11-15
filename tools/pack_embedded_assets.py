#!/usr/bin/env python3
"""无损压缩内置BGRA素材；不改分辨率、颜色、透明度或任何像素。"""
from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]

def encode(data: bytes) -> bytes:
    if len(data) % 4:
        raise ValueError('BGRA长度必须是4的倍数')
    out = bytearray()
    at = 0
    while at < len(data):
        pixel = data[at:at + 4]
        count = 1
        while count < 65535 and at + count * 4 < len(data) and data[at + count * 4:at + count * 4 + 4] == pixel:
            count += 1
        out.extend(struct.pack('<H', count) + pixel)
        at += count * 4
    return bytes(out)

def decode(data: bytes) -> bytes:
    if len(data) % 6:
        raise ValueError('RLE记录不完整')
    out = bytearray()
    for at in range(0, len(data), 6):
        count = struct.unpack_from('<H', data, at)[0]
        if count == 0:
            raise ValueError('空RLE记录')
        out.extend(data[at + 2:at + 6] * count)
    return bytes(out)

def build(repo: Path = ROOT) -> tuple[int, int]:
    before = after = 0
    for i in range(3):
        source = repo / f'supervisor/assets/s541/selector-{i}.bgra'
        raw = source.read_bytes()
        if len(raw) != 288 * 96 * 4:
            raise ValueError(f'素材尺寸错误: {source}')
        packed = encode(raw)
        if decode(packed) != raw:
            raise ValueError(f'无损校验失败: {source}')
        output = source.with_suffix('.rle')
        if not output.exists() or output.read_bytes() != packed:
            output.write_bytes(packed)
        before += len(raw); after += len(packed)
    print(f'内置控制条无损压缩: {before} -> {after} B，逐字节还原校验通过')
    return before, after

if __name__ == '__main__':
    import sys
    build(Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT)
