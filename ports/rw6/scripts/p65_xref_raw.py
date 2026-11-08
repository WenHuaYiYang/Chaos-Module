#!/usr/bin/env python3
"""p65 镜像的原始调用点扫描（批处理，不做"当代码读"）。

用途：Ghidra 的调用图在这个镜像上不全（大量跨段调用经 PLT 跳转壳），
`ghidra_fw.py calls` 报"1 个调用者"时用它补全。

口径：
  * 镜像有两个视图：XIP（run = file + 0x0C0C0000）与 PSRAM（run = file + 0x3BFD023C）。
  * 跳转壳 = 8 字节 `5f f8 00 f0` + 目标字（Thumb 指针 bit0=1）。
  * 一条 BL 的目标若等于「函数 VA」或「跳转壳的 VA」，都算一次调用。

用法：
  python scripts/p65_xref_raw.py 0x115D4            # 目标函数的 file offset
  python scripts/p65_xref_raw.py 0x115D4 --asm      # 附带每个调用点的前后指令
"""
import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
XIP = 0x0C0C0000
PSR = 0x3BFD023C


def load():
    try:
        from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_MCLASS
    except ImportError:
        raise SystemExit('缺 capstone')
    data = open(IMG, 'rb').read()
    return data, Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_MCLASS)


def veneers(data):
    out = {}
    i = data.find(b'\x5f\xf8\x00\xf0')
    while i >= 0:
        if i % 4 == 0 and i + 8 <= len(data):
            t = struct.unpack_from('<I', data, i + 4)[0]
            if t & 1 and (0x0C000000 <= t < 0x0F000000 or 0x3C000000 <= t < 0x3E000000
                          or 0x00200000 <= t < 0x00400000):
                out[i] = t & ~1
        i = data.find(b'\x5f\xf8\x00\xf0', i + 1, )
    return out


def main():
    if len(sys.argv) > 1 and sys.argv[1] == '--top':
        top()
        return
    target = int(sys.argv[1], 0)
    with_asm = '--asm' in sys.argv
    data, cs = load()
    cs.skipdata = True   # 数据段里遇到解不动的字节不中止整趟扫描
    ven = veneers(data)
    # 目标的两个视图
    wants = {XIP + target, PSR + target}
    # 指到目标的跳转壳
    ven_to_target = {off for off, t in ven.items() if t in wants}
    want_sites = wants | {XIP + off for off in ven_to_target} | {PSR + off for off in ven_to_target}
    hits = []
    for ins in cs.disasm(data, XIP):
        m = ins.mnemonic
        if not (m == 'bl' or m.startswith('blx') or m == 'b.w' or m.startswith('ldr.w pc')):
            continue
        op = ins.op_str.strip()
        if not op.startswith('#'):
            continue
        try:
            tgt = int(op[1:], 0)
        except ValueError:
            continue
        if tgt in want_sites:
            hits.append((ins.address, m, op))
    print('目标 file=%#x  XIP=%#x  PSRAM=%#x  跳转壳 %d 个' % (target, XIP + target, PSR + target, len(ven_to_target)))
    for a, m, op in hits:
        extra = ''
        if with_asm:
            ctx = []
            for ins in cs.disasm(data[max(0, a - 8):a + 12], XIP + max(0, a - 8)):
                ctx.append('%s %s' % (ins.mnemonic, ins.op_str))
            extra = '   | ' + ' ; '.join(ctx)
        print('  site file=%#x  run=%#x  %s %s%s' % (a, XIP + a, m, op, extra))
    print('合计 %d 处（含跳转壳间接调用）' % len(hits))




def top(limit=40, lo=0x0E0000, hi=0x240000):
    """统计某个区间内所有 BL 的目标频次（找"被一堆小设置函数共用"的中枢）。"""
    from collections import Counter
    data, cs = load()
    cs.skipdata = True
    c = Counter()
    for ins in cs.disasm(data[lo:hi], XIP + lo):
        if ins.mnemonic != 'bl':
            continue
        op = ins.op_str.strip()
        if not op.startswith('#'):
            continue
        try:
            t = int(op[1:], 0)
        except ValueError:
            continue
        c[t - XIP] += 1
    for t, n in c.most_common(limit):
        print('%8d  file=%#x' % (n, t))

if __name__ == '__main__':
    main()
