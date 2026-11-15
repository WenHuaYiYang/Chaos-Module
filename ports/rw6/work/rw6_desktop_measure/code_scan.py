# -*- coding: utf-8 -*-
"""按"函数指针候选"给 RW6 的 blob 定函数边界，再在里面找缩放的写入点。

为什么不用 Ghidra 的函数表：Ghidra 对 0x1C1695F0 以上那段没有流分析入口，
把它当成一支 879032 字节的巨函数，所以"按 Ghidra 函数逐条反汇编"的扫描
只覆盖了真正的记账段，桌面渲染段一条没判（这是本脚本要补的洞）。

候选入口的判据：某个字面量的低 24 位落在代码窗内且 bit0=1（Thumb），
就是有人拿它当函数指针用 —— 表项、回调注册、vtable 都算。
每条从入口往前解到 `bx lr` 后接对齐填充为止，长度上限 0x800。

用法：python code_scan.py <blob.bin> [--lo 0x118000] [--hi 0x3cf000]
"""
import re
import struct
import sys

from capstone import Cs, CS_ARCH_ARM, CS_MODE_MCLASS, CS_MODE_THUMB

ROOT = r"C:/Users/YiYang/Documents/xiaomi band 10 pro"
path = sys.argv[1] if len(sys.argv) > 1 else ROOT + r"/build/rw6_measure/rw6_psram_rel.bin"
buf = open(path, 'rb').read()
LO = 0x118000
HI = 0x3CF000

# 1) 收集入口候选
ent = set()
for o in range(0, min(len(buf), 0x800000) - 4, 4):
    v = struct.unpack_from('<I', buf, o)[0]
    if (v >> 24) in (0x1C, 0x3C) and (v & 1):
        t = (v & 0x00FFFFFF) & ~1
        if LO <= t < HI:
            ent.add(t)
print("入口候选 %d 个（窗 %#x..%#x）" % (len(ent), LO, HI))

md = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_MCLASS)
md.detail = False
funcs = {}
for a in sorted(ent):
    n = 0
    for i in md.disasm(buf[a:a + 0x800], a | 1):
        n += 1
        if i.mnemonic in ('bx', 'pop') and ('lr' in i.op_str):
            if i.mnemonic == 'bx':
                break
    funcs[a] = n
print("解出 %d 支，总指令 %d 条" % (len(funcs), sum(funcs.values())))

# 2) 在真边界里找 scale 相关写入与几何常量
GEO = {0x100: '256', 0x101: '257', 0x69: '105', 0x60: '96', 0x84: '132'}
rows = []
for a in sorted(funcs):
    s = min(0x800, max(4, funcs[a] * 4 + 8))
    code = buf[a:a + s]
    w50 = []
    geos = set()
    calls = set()
    for i in md.disasm(code, a):
        m = re.search(r'#0x(4[0-9a-f]|5[0-9a-f]|6[0-9a-f])\]', i.op_str)
        if i.mnemonic.startswith(('str', 'strh', 'strb')) and m:
            w50.append((i.address, i.mnemonic, m.group(1)))
        for h in re.findall(r'#0x([0-9a-f]+)', i.op_str):
            v = int(h, 16)
            if v in GEO:
                geos.add(v)
        if i.mnemonic in ('bl', 'bl.w'):
            calls.add(i.op_str)
    if (any(x[2] in ('50', '54') for x in w50) and geos) or len(geos) >= 3:
        rows.append((a, len(w50), sorted(GEO[g] for g in geos), len(calls)))
print("\n写 +0x50/+0x54 且含几何常量，或含 3 个以上几何常量的函数：%d 支" % len(rows))
for a, nw, g, nc in rows[:60]:
    print("  %07x  写入%-2d  常量%s  调用%d" % (a, nw, g, nc))
