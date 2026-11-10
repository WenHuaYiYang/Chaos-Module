#!/usr/bin/env python3
"""Run the real capture worker against VFS mocks; independently decode GIF/APNG/BMP."""
from pathlib import Path
import os
import subprocess
import tempfile
import struct
import zlib
from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]

def verify_apng_chunks(path):
    data = path.read_bytes(); assert data[:8] == b"\x89PNG\r\n\x1a\n"
    offset = 8; next_sequence = 0; frame_count = None; frames = []; current = None
    while offset < len(data):
        size = int.from_bytes(data[offset:offset + 4], "big"); tag = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + size]
        checksum = int.from_bytes(data[offset + 8 + size:offset + 12 + size], "big")
        assert zlib.crc32(tag + payload) == checksum, (tag, offset)
        offset += 12 + size
        if tag == b"acTL": frame_count, plays = struct.unpack(">II", payload); assert plays == 0
        if tag == b"fcTL":
            sequence, width, height, x, y, num, den, dispose, blend = struct.unpack(">IIIIIHHBB", payload)
            assert sequence == next_sequence; next_sequence += 1
            assert dispose == blend == 0 and num > 0 and den > 0
            current = [width, height, bytearray()]; frames.append(current)
        if tag == b"IDAT": assert len(frames) == 1; current[2].extend(payload)
        if tag == b"fdAT":
            sequence = int.from_bytes(payload[:4], "big"); assert sequence == next_sequence; next_sequence += 1
            current[2].extend(payload[4:])
        if tag == b"IEND": assert offset == len(data)
    assert frame_count == len(frames)
    for width, height, compressed in frames:
        raw = zlib.decompress(compressed)
        assert len(raw) == (width + 1) * height
        assert all(raw[y * (width + 1)] == 0 for y in range(height))

def palette_rgb(r, g, b):
    red, green, blue = r >> 5, g >> 5, b >> 6
    return (red * 255 // 7, green * 255 // 7,
            red * 255 // 7 if red == green and blue == red // 2 else blue * 255 // 3)

def noise_rgb(seed):
    output = bytearray(); value = seed
    for _ in range(336 * 480):
        value = (value * 1664525 + 1013904223) & 0xffffffff
        b, g, r = value.to_bytes(4, "little")[:3]
        output.extend(palette_rgb(r, g, b))
    return bytes(output)

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-capture-") as temp:
        directory = Path(temp)
        binary = directory / "capture-tests"
        subprocess.run(["rustc", "--edition=2021", "--test", str(ROOT / "supervisor/tests/capture_harness.rs"), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True, env={**os.environ, "CAPTURE_TEST_OUTPUT": temp})
        with Image.open(directory / "worker.gif") as gif:
            assert gif.size == (336, 480) and gif.n_frames == 3
            assert gif.info["loop"] == 0
            for frame in range(3):
                gif.seek(frame)
                rgb = gif.convert("RGB")
                value = frame + 1
                for pixel in range(336 * 480):
                    value = (value * 1664525 + 1013904223) & 0xffffffff
                    b, g, r = value.to_bytes(4, "little")[:3]
                    expected = palette_rgb(r, g, b)
                    assert rgb.getpixel((pixel % 336, pixel // 336)) == expected, (frame, pixel)
                assert gif.info["duration"] == (250 if frame == 0 else 2000 if frame == 1 else 10)
        with Image.open(directory / "worker.bmp") as bmp:
            assert bmp.size == (336, 480)
            pixels = bmp.convert("RGB")
            value = 1
            for pixel in range(336 * 480):
                value = (value * 1664525 + 1013904223) & 0xffffffff
                b, g, r = value.to_bytes(4, "little")[:3]
                assert pixels.getpixel((pixel % 336, pixel // 336)) == (r, g, b)
        with Image.open(directory / "size-limit.gif") as gif:
            assert gif.n_frames > 1
            gif.seek(gif.n_frames - 1)
            gif.load()
        assert (directory / "size-limit.gif").stat().st_size <= 16 * 1024 * 1024
        with Image.open(directory / "static.gif") as gif:
            assert gif.n_frames == 1 and gif.info["duration"] == 5020
            assert gif.convert("RGB").getextrema() == ((0, 0),) * 3
        with Image.open(directory / "delta.gif") as gif:
            assert gif.n_frames == 3
            for frame in range(3):
                gif.seek(frame)
                extent = gif.tile[0][1]
                if frame: assert extent == (frame * 30, 100, (frame + 1) * 30 + 20, 112)
                rgb = gif.convert("RGB")
                x = (frame + 1) * 30
                for y in range(480):
                    for col in range(336):
                        assert rgb.getpixel((col, y)) == ((255, 0, 0) if 100 <= y < 112 and x <= col < x + 20 else (0, 0, 0))
        with Image.open(directory / "torn.gif") as gif:
            assert gif.n_frames == 3
            for frame, seed in enumerate((1, 3, 4)):
                gif.seek(frame); rgb = gif.convert("RGB"); value = seed
                for pixel in range(336 * 480):
                    value = (value * 1664525 + 1013904223) & 0xffffffff
                    b, g, r = value.to_bytes(4, "little")[:3]
                    assert rgb.getpixel((pixel % 336, pixel // 336)) == palette_rgb(r, g, b)
        with Image.open(directory / "timing.gif") as gif:
            duration = 0
            assert gif.n_frames == 100
            for frame in range(gif.n_frames):
                gif.seek(frame); gif.load(); duration += gif.info["duration"]
            assert abs(duration - 6700) < 10, duration
        for name in ("sixty.apng", "noise.apng", "static.apng", "slow.apng", "gaps.apng", "flipping.apng", "unsynced.apng", "grays.apng"):
            verify_apng_chunks(directory / name)
        with Image.open(directory / "slow.apng") as animation:
            assert animation.n_frames == 6; duration = 0
            for frame in range(6):
                animation.seek(frame); duration += animation.info["duration"]
                assert animation.convert("RGB").tobytes() == noise_rgb(frame + 1)
            expected = int((directory / "slow-duration.txt").read_text())
            assert abs(duration - expected) < 0.001 and duration > 6 * 17
        with Image.open(directory / "gaps.apng") as animation:
            assert animation.n_frames == 4; duration = 0
            for frame, x in enumerate((30, 60, 120, 150)):
                animation.seek(frame); duration += animation.info["duration"]
                expected = Image.new("RGB", (336, 480))
                ImageDraw.Draw(expected).rectangle((x, 100, x + 19, 111), fill=(255, 0, 0))
                assert animation.convert("RGB").tobytes() == expected.tobytes()
            assert abs(duration - 68) < 0.001
        for suffix in ("gif", "apng"):
            with Image.open(directory / f"unsynced.{suffix}") as animation:
                assert animation.size == (336, 480) and animation.n_frames == 20
                duration = 0
                for frame in range(20):
                    animation.seek(frame); duration += animation.info["duration"]
                    expected = Image.new("RGB", (336, 480)); x = ((frame + 1) * 30) % 300
                    ImageDraw.Draw(expected).rectangle((x, 100, x + 19, 111), fill=(255, 0, 0))
                    assert animation.convert("RGB").tobytes() == expected.tobytes(), (suffix, frame)
                assert abs(duration - (1340 if suffix == "gif" else 340)) < 0.001
        with Image.open(directory / "flipping.apng") as animation:
            assert animation.n_frames == 60; duration = 0
            for frame in range(60):
                animation.seek(frame); duration += animation.info["duration"]
                expected = Image.new("RGB", (336, 480)); x = ((frame + 1) * 60) % 300
                ImageDraw.Draw(expected).rectangle((x, 100, x + 19, 111), fill=(255, 0, 0))
                assert animation.convert("RGB").tobytes() == expected.tobytes()
            assert abs(duration - 1000) < 0.001
        for suffix in ("gif", "apng"):
            with Image.open(directory / f"grays.{suffix}") as animation:
                assert animation.size == (336, 480) and animation.n_frames == 1
                rgb = animation.convert("RGB")
                # Independent goal: neutral source cards must remain neutral,
                # not olive, green or blue; primary/secondary channels stay intact.
                for gray in range(256):
                    for y in (0, 100, 239, 479):
                        r, g, b = rgb.getpixel((gray, y))
                        assert r == g == b and abs(r - gray) <= 31, (suffix, gray, r, g, b)
                assert rgb.getpixel((48, 200)) == (36, 36, 36)
                colors = [(255,0,0), (0,255,0), (0,0,255), (255,255,0), (255,0,255), (0,255,255)]
                for i, color in enumerate(colors): assert rgb.getpixel((256 + i * 12, 200)) == color
        with Image.open(directory / "sixty.apng") as animation:
            assert animation.n_frames == 60
            duration = 0
            for frame in range(60):
                animation.seek(frame); duration += animation.info["duration"]
                expected = Image.new("RGB", (336, 480)); x = ((frame + 1) * 30) % 300
                ImageDraw.Draw(expected).rectangle((x, 100, x + 19, 111), fill=(255, 0, 0))
                assert animation.convert("RGB").tobytes() == expected.tobytes(), frame
            assert abs(duration - 1000) < 0.001, duration
        with Image.open(directory / "noise.apng") as animation:
            assert animation.n_frames == 3
            for frame in range(3):
                animation.seek(frame); assert animation.convert("RGB").tobytes() == noise_rgb(frame + 1)
        with Image.open(directory / "static.apng") as animation:
            assert animation.n_frames == 1 and abs(animation.info["duration"] - 299999) < 10
            assert animation.convert("RGB").getextrema() == ((0, 0),) * 3
        with Image.open(directory / "fifteen.gif") as animation:
            assert animation.n_frames == 15
        print("PASS: GIF/APNG/BMP pixels and timing, neutral gray cards/primary channels, bounded snapshot repair and relaxed fallback")

if __name__ == "__main__":
    main()
