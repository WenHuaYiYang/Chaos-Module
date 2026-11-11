#!/usr/bin/env python3
"""Convert the supplied GIF into indexed-alpha frames for the native icon.

Keep every composited frame, duration and repeat count. Runtime only unpacks
one native 96px frame; it does not implement GIF disposal or LZW decoding.
"""
from pathlib import Path
import argparse
import hashlib
import struct
from PIL import Image
from gen_chaos_icon import render_icon

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "tools/chaos.gif"
OUTPUT = ROOT / "supervisor/assets/dynamic-icon.frames"
SIZE = 96
HEADER = struct.Struct("<4s4H4I32s")

def pack(indices: bytes) -> bytes:
    """PackBits: high bit = repeat, otherwise literal; length = low bits + 1."""
    result = bytearray()
    at = 0
    while at < len(indices):
        run = 1
        while run < 128 and at + run < len(indices) and indices[at + run] == indices[at]:
            run += 1
        if run >= 3:
            result.extend((0x80 | (run - 1), indices[at]))
            at += run
            continue
        start = at
        at += run
        while at < len(indices) and at - start < 128:
            if at + 2 < len(indices) and indices[at] == indices[at + 1] == indices[at + 2]:
                break
            at += 1
        result.append(at - start - 1)
        result.extend(indices[start:at])
    return bytes(result)

def load(source: Path):
    frames, durations = [], []
    with Image.open(source) as gif:
        if gif.format != "GIF" or not 1 <= gif.n_frames <= 256:
            raise ValueError("expected a GIF with 1..256 frames")
        repeat = gif.info.get("loop")
        plays = 0 if repeat == 0 else (repeat + 1 if repeat is not None else 1)
        for index in range(gif.n_frames):
            gif.seek(index)
            delay = gif.info.get("duration", 0)
            if not 1 <= delay <= 60_000:
                raise ValueError(f"frame {index}: unsupported GIF duration {delay}ms")
            # Pillow applies disposal, transparency and partial-frame compositing.
            frames.append(render_icon(gif.convert("RGBA"), SIZE))
            durations.append(delay)
    return frames, durations, plays

def quantize(frames):
    atlas = Image.new("RGBA", (SIZE, SIZE * len(frames)))
    for index, frame in enumerate(frames):
        atlas.paste(frame, (0, index * SIZE))
    # One RGBA palette for the whole sequence prevents frame-to-frame palette
    # changes and retains transparency in native indexed-alpha format 0x0A.
    indexed = atlas.quantize(colors=256, method=Image.Quantize.FASTOCTREE,
                             dither=Image.Dither.NONE)
    rgba = bytes(indexed.getpalette("RGBA"))
    palette = bytearray()
    for at in range(0, len(rgba), 4):
        r, g, b, a = rgba[at:at + 4]
        palette.extend((b, g, r, a))
    pixels = [indexed.crop((0, n * SIZE, SIZE, (n + 1) * SIZE)).tobytes()
              for n in range(len(frames))]
    return bytes(palette), pixels

def convert(source: Path) -> bytes:
    frames, durations, plays = load(source)
    palette, pixels = quantize(frames)
    assert len(palette) == 1024
    blocks = [pack(frame) for frame in pixels]
    table = HEADER.size + len(palette)
    payload = table + len(frames) * 12
    header = HEADER.pack(b"DGF1", SIZE, SIZE, len(frames), 0,
                         sum(durations), plays, table, payload,
                         hashlib.sha256(source.read_bytes()).digest())
    records = bytearray()
    offset = payload
    for delay, block in zip(durations, blocks):
        records.extend(struct.pack("<III", delay, offset, len(block)))
        offset += len(block)
    return header + palette + bytes(records) + b"".join(blocks)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    jobs = [(args.source or SOURCE, args.out or OUTPUT)]
    for source, output in jobs:
        data = convert(source)
        if args.check:
            if not output.is_file() or output.read_bytes() != data:
                raise SystemExit(f"GIF frame asset {output.name} is stale: run tools/build_gif_icon.py")
        else:
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_bytes(data)
        print(f"GIF icon {source.name}: 96x96, {len(data)} bytes, {struct.unpack_from('<H', data, 8)[0]} frames, "
              f"{struct.unpack_from('<I', data, 12)[0]}ms/cycle")

if __name__ == "__main__":
    main()
