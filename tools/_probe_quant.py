#!/usr/bin/env python3
"""Temporary probe: is the committed icon asset reproducible across platforms?"""
import hashlib
import struct
import sys

import build_gif_icon as build


def records(data, table, count):
    return [struct.unpack_from("<III", data, table + i * 12) for i in range(count)]


def unpack(block):
    result = bytearray()
    at = 0
    while at < len(block):
        tag = block[at]
        at += 1
        n = (tag & 127) + 1
        if tag & 128:
            result.extend(bytes((block[at],)) * n)
            at += 1
        else:
            result.extend(block[at:at + n])
            at += n
    return bytes(result)


def canonical(palette, pixels):
    order = sorted(range(256), key=lambda i: (palette[i * 4:i * 4 + 4], i))
    remap = [0] * 256
    for new, old in enumerate(order):
        remap[old] = new
    canon = b"".join(palette[old * 4:old * 4 + 4] for old in order)
    return canon, [bytes(remap[b] for b in frame) for frame in pixels]


def main() -> int:
    data = build.OUTPUT.read_bytes()
    header = build.HEADER.unpack_from(data)
    count, table = header[3], header[7]
    shipped_palette = data[build.HEADER.size:table]
    shipped_pixels = [unpack(data[off:off + size]) for _, off, size in records(data, table, count)]
    frames, durations, _ = build.load(build.SOURCE)
    palette, pixels = build.quantize(frames)

    def multiset(blob):
        entries = sorted(blob[i:i + 4] for i in range(0, len(blob), 4))
        return hashlib.sha256(b"".join(entries)).hexdigest()[:32]

    ship_can, _ = canonical(shipped_palette, shipped_pixels)
    fresh_can, _ = canonical(palette, pixels)

    print("python", sys.version.split()[0])
    print("frames", len(frames), "durations_sum", sum(durations))
    print("shipped_sha      ", hashlib.sha256(data).hexdigest()[:32])
    print("fresh_sha        ", hashlib.sha256(build.convert(build.SOURCE)).hexdigest()[:32])
    print("pal_multiset_sh  ", multiset(shipped_palette))
    print("pal_multiset_fr  ", multiset(palette))
    print("pal_seq_sh       ", hashlib.sha256(shipped_palette).hexdigest()[:32])
    print("pal_seq_fr       ", hashlib.sha256(palette).hexdigest()[:32])
    print("canon_pal_sh     ", hashlib.sha256(ship_can).hexdigest()[:32])
    print("canon_pal_fr     ", hashlib.sha256(fresh_can).hexdigest()[:32])
    print("bytes_equal      ", data == build.convert(build.SOURCE))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
