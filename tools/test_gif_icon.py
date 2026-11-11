#!/usr/bin/env python3
"""Check that the shipped native frames correspond to the supplied GIF."""
from pathlib import Path
import hashlib
import random
import struct
import unittest
from PIL import Image
import build_gif_icon as build

def unpack(encoded):
    result = bytearray(); at = 0
    while at < len(encoded):
        tag = encoded[at]; at += 1; n = (tag & 127) + 1
        if tag & 128:
            result.extend(bytes((encoded[at],)) * n); at += 1
        else:
            result.extend(encoded[at:at + n]); at += n
    return bytes(result)

class GifIconTests(unittest.TestCase):
    def test_packbits_literal_repeat_and_packet_boundaries(self):
        rng = random.Random(7642)
        for raw in [b'', b'a', b'aa', b'a' * 128, b'a' * 129, bytes(range(256)),
                    bytes(rng.randrange(256) for _ in range(96 * 96)),
                    bytes(range(127)) + b'x' * 130 + bytes(range(129))]:
            self.assertEqual(unpack(build.pack(raw)), raw)

    def check_uploaded_gif(self, source, output, expected_count, expected_cycle):
        data = output.read_bytes()
        magic, width, height, count, _, cycle, plays, table, _, digest = build.HEADER.unpack_from(data)
        self.assertEqual((magic, width, height, count, cycle, plays), (b'DGF1', 96, 96, expected_count, expected_cycle, 0))
        self.assertEqual(digest, hashlib.sha256(source.read_bytes()).digest())
        frames, durations, _ = build.load(source)
        palette, expected = build.quantize(frames)
        self.assertEqual(data[build.HEADER.size:table], palette)
        distinct = set()
        for index in range(count):
            delay, offset, size = struct.unpack_from('<III', data, table + index * 12)
            self.assertEqual(delay, durations[index])
            pixels = unpack(data[offset:offset + size])
            self.assertEqual(pixels, expected[index], f'original GIF frame {index}')
            self.assertEqual(len(pixels), 96 * 96)
            distinct.add(hashlib.sha256(pixels).digest())
            for corner in (0, 95, 96 * 95, 96 * 96 - 1):
                self.assertEqual(palette[pixels[corner] * 4 + 3], 0)
        self.assertGreater(len(distinct), 20)
        with Image.open(source) as gif:
            self.assertEqual(frames[0].tobytes(), build.render_icon(gif, 96).tobytes())

    def test_new_dynamic_gif_hash_frames_delays_and_alpha(self):
        self.check_uploaded_gif(build.SOURCE, build.OUTPUT, 193, 5200)

if __name__ == '__main__':
    unittest.main()
