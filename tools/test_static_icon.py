#!/usr/bin/env python3
"""Validate the shared original 96px icon used by installer and runtime."""
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from PIL import Image
from gen_chaos_icon import render_icon, encode_icon
from build import build_icon

ROOT = Path(__file__).resolve().parents[1]

class StaticIconTests(unittest.TestCase):
    def test_dark_icons_use_the_existing_processing_without_pixel_loss(self):
        for name in ('dark-potato', 'dark-icon'):
            with Image.open(ROOT / f'tools/app-icons/{name}.png') as source:
                expected = encode_icon(render_icon(source, 96))
            self.assertEqual((ROOT / f'supervisor/assets/app-icons/{name}.bin').read_bytes(), expected)
    def test_both_installer_icons_match_the_original_source_at_96px(self):
        with Image.open(ROOT / 'tools/chaos.png') as source:
            expected = encode_icon(render_icon(source, 96))
        with tempfile.TemporaryDirectory() as directory:
            for name in ('chaos_icon.bin', 'chaos_icon_settings.bin'):
                output = Path(directory) / name
                build_icon(ROOT, output, 96, sys.executable)
                data = output.read_bytes()
                self.assertEqual(data, expected)
                self.assertEqual(struct.unpack_from('<HHI', data, 4), (96,96,384))
                self.assertEqual(len(data), 36876)
                alpha=data[15::4]
                for corner in (0,95,96*95,96*96-1): self.assertEqual(alpha[corner],0)

if __name__ == '__main__': unittest.main()
