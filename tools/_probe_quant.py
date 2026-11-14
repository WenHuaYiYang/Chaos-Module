#!/usr/bin/env python3
"""Temporary probe: does Pillow's FASTOCTREE quantizer agree with the committed asset?"""
import hashlib
import sys

import build_gif_icon as build


def main() -> int:
    data = build.OUTPUT.read_bytes()
    header = build.HEADER.unpack_from(data)
    table = header[7]
    shipped = data[build.HEADER.size:table]
    frames, durations, _ = build.load(build.SOURCE)
    palette, pixels = build.quantize(frames)
    ship_set = {shipped[i:i + 4] for i in range(0, len(shipped), 4)}
    fresh_set = {palette[i:i + 4] for i in range(0, len(palette), 4)}
    print("python", sys.version.split()[0])
    print("frames", len(frames), "durations_sum", sum(durations))
    print("bytes_equal", shipped == palette)
    print("shipped_pal_sha", hashlib.sha256(shipped).hexdigest()[:32])
    print("fresh_pal_sha  ", hashlib.sha256(palette).hexdigest()[:32])
    print("shipped_slice", shipped[500:540].hex())
    print("fresh_slice  ", palette[500:540].hex())
    print("color_set_equal", ship_set == fresh_set,
          "shipped_only", len(ship_set - fresh_set), "fresh_only", len(fresh_set - ship_set))
    print("alpha_hist_shipped", len({c for c in ship_set if c[3] != 255}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
