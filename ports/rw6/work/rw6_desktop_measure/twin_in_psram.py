#!/usr/bin/env python3
"""拿 043 flash 里已定名的函数当模板, 在 RW6 的 PSRAM dump 里找同源孪生函数。

为什么这样打: 两代是同一套 launcher 框架(043 = 3.101.043 / RW6 = 3.100.043), 同一支函数
的指令形状几乎不变, 只有 BL 目标与字面量会变。043 侧的函数**已经定名**(find_fn_by_name 能解),
而 RW6 侧的对应实现不在 flash 里、在 PSRAM(见 docs/Chaos_桌面图标缩放_20261024.md §13) ——
所以模板取 043 flash, 靶子取 RW6 PSRAM dump。之前那版 find_twin.py 拿 043 flash 比 RW6 flash,
两边根本不是同一份实现, 命中 16/40 是必然的。

比较时打掩码: 分支族(B/B1/BL/BC/B.cond/CBZ/CBNZ)只留族位, 立即数类(ADD/SUB SP)同理。
用法: python twin_in_psram.py <043文件偏移hex> [窗口半字数] [靶起点hex] [靶终点hex]
"""
import struct
import sys

A43 = 'fw_pkg_043/vela_ap.bin'
TARGET = 'build/rw6_measure/rw6_psram.bin'
BASE = 0x1C000000


def norm(img, off, n):
    """取 n 个半字并打掩码, 返回定长列表。"""
    hw = struct.unpack_from('<%dH' % n, img, off)
    out = []
    i = 0
    while i < n:
        h0 = hw[i]
        h1 = hw[i + 1] if i + 1 < n else 0
        if h0 >= 0xE800:                       # 32 位前缀
            if (h0 & 0xF800) == 0xF000 and (h1 & 0xD000) == 0xD000:
                out += [0xF000, 0xFFFF]        # BL / B.W: 留族位, 掩掉偏移
            elif (h0 & 0xD000) == 0x8000 and (h1 & 0xD000) == 0x8000:
                out += [0xD000, 0xFFFF]        # B(条件) 32 位
            else:
                out += [h0, h1]
            i += 2
        else:
            if (h0 & 0xE000) == 0xE000:
                out.append(0xE000)             # B(短)
            elif (h0 & 0xF000) == 0xD000:
                out.append(0xD000)             # B(条件, 16 位)
            elif (h0 & 0xF800) in (0xB000, 0xB800):
                out.append(h0 & 0xF800)        # ADD/SUB SP 与 CBZ/CBNZ: 掩掉立即数
            elif (h0 & 0xF800) == 0x4800:
                out.append(0x4800)             # LDR(文字池相对): 掩掉偏移
            else:
                out.append(h0)
            i += 1
    return out[:n]


def main():
    base43 = int(sys.argv[1], 16)
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 48
    lo = int(sys.argv[3], 16) if len(sys.argv) > 3 else 0x0
    hi = int(sys.argv[4], 16) if len(sys.argv) > 4 else 0x200000
    a43 = open(A43, 'rb').read()
    tar = open(TARGET, 'rb').read()
    tpl = norm(a43, base43, n)
    if len(tpl) < n:
        print('模板取不满 %d 个半字' % n)
        return 1
    best = []
    for off in range(lo, min(hi, len(tar) - 2 * n), 2):
        h0 = int.from_bytes(tar[off:off + 2], 'little')
        if h0 != tpl[0]:
            continue
        cand = norm(tar, off, n)
        score = sum(1 for x, y in zip(tpl, cand) if x == y)
        best.append((score, off))
    best.sort(reverse=True)
    print('模板: 043 file 0x%X (VA 0x%X)  %d 半字   靶: RW6 PSRAM 0x%X..0x%X'
          % (base43, base43 + 0x0C0C0000, n, lo + BASE, hi + BASE))
    print('候选(满分 %d):' % n)
    for score, off in best[:10]:
        print('   PSRAM 偏移 0x%06X  VA 0x%08X  命中 %d/%d  %.0f%%'
              % (off, off + BASE, score, n, 100.0 * score / n))
    if not best:
        print('   没有同首半字的候选')
    return 0


if __name__ == '__main__':
    sys.exit(main())
