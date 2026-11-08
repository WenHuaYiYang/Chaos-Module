#!/usr/bin/env python3
"""用 043 已定名的 launcher 段抽字符串, 到 RW6 镜像里反查引用点, 钉出 RW6 的 launcher 区。

为什么这么打: RW6 的**标识符**名字表是死的(没有任何 4 字节 VA 引用, 见
docs/Chaos_桌面图标缩放_20261024.md §14), 但日志格式串与 UI 文案是代码里的字面量池项,
一定有引用点。两代是同一套 launcher 框架, 串文本几乎不变 —— 串是比函数形状更稳的锚。

口径: 运行地址 R = 文件偏移 F + 0x0C0C0000; 只读常量另有 0x2C 别名域(F + 0x2C0C0000)。
两个域都要扫, 只扫一个会漏。

用法: python anchor_via_strings.py [043起始hex] [043结束hex]
"""
import re
import struct
import sys

A43 = 'fw_pkg_043/vela_ap.bin'
ARW6 = 'fw_pkg_rw6/vela_ap.bin'
XIP = 0x0C0C0000
ALIAS = 0x2C0C0000
KEYS = ('launcher', 'app_icon', 'slide', 'icon', 'page', 'zoom', 'scale', 'layout',
        'anim', 'scroll', 'widget', 'grid', 'list')


def strings(img, lo, hi, minlen=5):
    """返回 {字符串: 文件偏移}，只取可读 ASCII 段。"""
    out = {}
    for m in re.finditer(rb'[\x20-\x7e]{%d,}' % minlen, img[lo:hi]):
        s = m.group().rstrip(b'\x00').decode('ascii', 'replace')
        if len(s) >= minlen:
            out.setdefault(s, lo + m.start())
    return out


def ref_sites(img, va_targets):
    """扫全镜像找 4 字节小端字 == 目标 VA(两个别名域都算)。返回 {va: [文件偏移...]}"""
    hits = {va: [] for va in va_targets}
    pats = {}
    for va in va_targets:
        for form in (va, va - XIP + ALIAS):
            pats.setdefault(struct.pack('<I', form), []).append(va)
    step = 4
    for off in range(0, len(img) - 4, step):
        p = img[off:off + 4]
        va = pats.get(p)
        if va:
            for v in va:
                hits[v].append(off)
    return hits


def main():
    lo = int(sys.argv[1], 16) if len(sys.argv) > 1 else 0x42F000
    hi = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0x459000
    a43 = open(A43, 'rb').read()
    arw6 = open(ARW6, 'rb').read()
    s43 = strings(a43, lo, hi)
    # 只挑带关键词、且在 RW6 里也存在的串
    cands = []
    for s, off in sorted(s43.items(), key=lambda x: x[1]):
        low = s.lower()
        if not any(k in low for k in KEYS):
            continue
        if len(s) < 6:
            continue
        idx = arw6.find(s.encode())
        if idx >= 0 and arw6.find(s.encode(), idx + 1) == -1:
            cands.append((s, off, idx))
    print('043 launcher 段 file 0x%X..0x%X 里带关键词的串, 且在 RW6 中唯一命中: %d 条'
          % (lo, hi, len(cands)))
    if not cands:
        return 1
    va_map = {}
    rows = []
    for s, o43, rw6off in cands[:400]:
        va_map[XIP + rw6off] = s
        rows.append((s, o43, rw6off))
    hits = ref_sites(arw6, list(va_map))
    got = [(va, off_list) for va, off_list in hits.items() if off_list]
    print('其中在 RW6 代码里有字面量引用的: %d 条' % len(got))
    for va, offs in sorted(got, key=lambda x: x[1][0])[:40]:
        s = va_map[va]
        print('  RW6 file 0x%06X  <- "%s"  引用点 %s' % (
            va - XIP, s[:52], ', '.join('0x%X' % (o - (o % 1)) for o in offs[:4])))
    # 汇总引用点落在哪个文件区间 —— 那就是 RW6 的 launcher 区
    sites = sorted(o for _, offs in got for o in offs)
    if sites:
        print('\n引用点分布(按 0x10000 分桶):')
        buckets = {}
        for o in sites:
            buckets[o >> 16] = buckets.get(o >> 16, 0) + 1
        for b in sorted(buckets, key=lambda x: -buckets[x])[:8]:
            print('   file 0x%04X0000..0x%04XFFFF  %d 处  (VA 0x%08X..)'
                  % (b, b, buckets[b], (b << 16) + XIP))
    return 0


if __name__ == '__main__':
    sys.exit(main())
