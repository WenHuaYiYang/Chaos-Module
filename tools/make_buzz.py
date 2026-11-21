#!/usr/bin/env python3
"""把音频分析成 .buzz 振动序列(与网页同步器同一套算法), 供内置示例使用。

用法: python tools/make_buzz.py song.mp3 --limit 1024 --out song.buzz
算法: 10ms 跳能量包络 -> 正差分 onset 强度 -> 局部自适应阈值挑脉冲 ->
      按能量三档映射振动效果(强 0x0D / 中 0x07 / 弱 0x03)。
"""
import argparse
import struct
import sys

import miniaudio
import numpy as np


def analyze(path, sensitivity=1.0, limit=1024, max_seconds=90.0):
    dec = miniaudio.decode_file(path, output_format=miniaudio.SampleFormat.SIGNED16,
                                nchannels=1, sample_rate=44100)
    pcm = np.asarray(dec.samples, dtype=np.int16).astype(np.float32) / 32768.0
    sr = dec.sample_rate
    if max_seconds and len(pcm) > sr * max_seconds:
        pcm = pcm[: int(sr * max_seconds)]

    hop = max(1, int(round(sr * 0.01)))
    win = hop * 2
    frames = 1 + max(0, (len(pcm) - win) // hop)
    env = np.empty(frames, dtype=np.float32)
    for i in range(frames):
        chunk = pcm[i * hop: i * hop + win]
        env[i] = float(np.sqrt(np.mean(chunk * chunk)))

    flux = np.zeros_like(env)
    flux[1:] = np.maximum(0.0, env[1:] - env[:-1])
    peak = float(flux.max())
    if peak <= 0:
        return [], 0.0

    k = 1.8 * max(0.1, min(sensitivity, 3.0))
    w = 8
    events = []
    last = 0
    for i in range(1, len(flux)):
        if len(events) >= limit:
            break
        lo, hi = max(0, i - w), min(len(flux), i + w)
        local = float(flux[lo:hi].mean())
        threshold = local * k + peak * 0.015
        if flux[i] < threshold or flux[i] < peak * 0.05:
            continue
        delta = i - last
        if delta < 2:
            continue
        e = flux[i] / peak
        ty = 0x0D if e > 0.55 else 0x07 if e > 0.25 else 0x03
        events.append((min(max(delta, 1), 255), ty))
        last = i
    return events, len(pcm) / sr


def encode(events):
    out = bytearray()
    out += b"BZ01"
    out += struct.pack("<HH", len(events), 120)
    for delta, ty in events:
        out += bytes((delta, ty, 0))
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("audio")
    ap.add_argument("--limit", type=int, default=1024)
    ap.add_argument("--sensitivity", type=float, default=1.0)
    ap.add_argument("--seconds", type=float, default=90.0)
    ap.add_argument("--out")
    args = ap.parse_args()

    events, dur = analyze(args.audio, args.sensitivity, args.limit, args.seconds)
    if not events:
        sys.exit("没有提取到脉冲")
    data = encode(events)
    span = sum(d for d, _ in events) / 100.0
    print(f"脉冲 {len(events)} 个, 覆盖 {span:.1f}s (音频 {dur:.1f}s), 序列 {len(data)} 字节")
    if args.out:
        with open(args.out, "wb") as f:
            f.write(data)
        print("写出:", args.out)


if __name__ == "__main__":
    main()
