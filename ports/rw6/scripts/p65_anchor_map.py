#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 Chaos 用到的每个 043 固件入口，按"函数体内的字符串键"批量绑到 p65 的实现上。

已定案的前提（都是本轮实测）：
  - 函数序言字节不跨镜像共享（同一段在 043 自己里唯一命中，在 p65 里 0 命中，做过阳性对照）；
  - p65  file 0x1C0000..0x3C0000 这一带是**倒灌进 PSRAM 的 blob**，代码里的串指针用的是
    PSRAM VA：`VA = file + 0x3C000000 - 0x2FDC4`。
    ★重要更正：此前记的"p65 名字池/串池无静态引用"是**搜错别名域**造成的假阴性 ——
      只搜了 0x0C/0x2C/0x28 三个 flash 别名，没搜 0x3C。按 0x3C 口径重搜，
      043 `app_install` 体内那枚 `[%s] %s: [%s] installation failed`
      在 p65 里正好 1 处引用（file 0x00181A6C）。

用法：
  python scripts/p65_anchor_map.py            # 全部 99 处 fw_api 锚点
  python scripts/p65_anchor_map.py --only gfx # 只做一个模块
输出 docs/Chaos_p65_锚点绑定_20261018.csv：每处锚点的键串、p65 引用点、
以及"该键是否唯一"。引用点仍要交 Ghidra 取所在函数入口，才算锚点定下来。
"""
import csv
import json
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, 'Chaos-Module', 'supervisor', 'src')
PACK = os.path.join(ROOT, 'canopus_src', 'targets',
                    'xiaomi-band-10-pro-3.101.043', 'symbols')
FW043 = os.path.join(ROOT, 'fw_pkg_043', 'vela_ap.bin')
P65 = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
OUT = os.path.join(ROOT, 'docs', 'Chaos_p65_锚点绑定_20261018.csv')

A1 = 0x0C0C0000
A2 = 0x2C0C0000
POOL = 0x3C000000 - 0x2FDC4          # p65 串池：VA = file + POOL
BODY = 0x200     # 折中：真边界要靠 FlQuery 的 funcs 模式导出的函数表（--fnbounds）
STR = re.compile(rb'[\x20-\x7e]{6,}\x00')
ADDR = re.compile(r'transmute\(\s*(0x[0-9A-Fa-f_]+)usize')
FN = re.compile(r'fn\s+([A-Za-z0-9_]+)')


def collect():
    out = []
    files = ['fw_api.rs'] + ['fw_api/' + x for x in
                             sorted(os.listdir(os.path.join(SRC, 'fw_api')))
                             if x.endswith('.rs')]
    for rel in files:
        p = os.path.join(SRC, rel)
        if not os.path.isfile(p):
            continue
        cur = '?'
        for ln in open(p, encoding='utf-8').read().splitlines():
            m = FN.search(ln)
            if m:
                cur = m.group(1)
            for a in ADDR.findall(ln):
                v = int(a.replace('_', ''), 16) & ~1
                out.append((rel.split('/')[-1].replace('.rs', ''), cur, v))
    return out


def pack_index():
    by = {}
    for f in os.listdir(PACK):
        s = json.load(open(os.path.join(PACK, f), encoding='utf-8'))
        e = s.get('entry_address')
        if e:
            by[int(e, 16) & ~1] = (s.get('name', '?'), s.get('prototype', ''),
                                   s.get('status', ''))
    return by


def keys_of43(d, entry):
    """043 函数体窗口里的串键（去重，保顺序）。"""
    seen, out = set(), []
    for a in range(entry & ~3, min(entry + BODY, len(d) - 4), 2):
        v = struct.unpack_from('<I', d, a)[0]
        for base in (A1, A2):
            if base <= v < base + len(d):
                m = STR.match(d[v - base:v - base + 96])
                if m:
                    t = m.group()[:-1].decode('latin1')
                    if t not in seen:
                        seen.add(t)
                        out.append((a - entry, t))
    return out


def bind_p65(p65, text):
    """枚串键在 p65 里的引用点：先找串，再按 PSRAM VA / flash 别名找引用。"""
    pat = text.encode('latin1') + b'\x00'
    hits = []
    i = p65.find(pat)
    while i >= 0:
        for va in (i + POOL, A1 + i, A2 + i):
            j = p65.find(struct.pack('<I', va))
            if j >= 0:
                hits.append(j)
        i = p65.find(pat, i + 1)
    return hits


def vote(anchor_keys, p65, win=0x400, need=2):
    """多键投票：把每枚键在 p65 的引用点摊平，找">=need 枚不同的键投到同一邻域"的簇。

    为什么不能只看"某枚键唯一命中"：043 的取体窗口会越进邻居函数，于是两支不相干的
    锚点会共用同一枚别人的串（实测有 6 支锚点都投到 `sport_data_manager_unregister`
    那一处）。所以唯一命中不算证据，**多键同域**才算。
    """
    per_key = []
    for off, text in anchor_keys:
        refs = bind_p65(p65, text)
        if refs:
            per_key.append((text, sorted(refs)))
    if len(per_key) < need:
        return None
    best = None
    for _, seeds in per_key:
        for seed in seeds:
            lo, hi = seed - win, seed + win
            keys_in, sites = set(), []
            for t, rs in per_key:
                hit = [x for x in rs if lo <= x <= hi]
                if hit:
                    keys_in.add(t)
                    sites.extend(hit)
            if len(keys_in) >= need:
                cand = (len(keys_in), min(sites), sorted(keys_in), sorted(set(sites)))
                if best is None or cand[0] > best[0]:
                    best = cand
    if best is None:
        return None
    return best


def main():
    only = None
    if '--only' in sys.argv:
        only = sys.argv[sys.argv.index('--only') + 1]
    f43 = open(FW043, 'rb').read()
    p65 = open(P65, 'rb').read()
    idx = pack_index()
    rows = [['module', 'wrapper', 'addr_043', 'fw_name_043', 'key_string',
             'p65_string_file', 'p65_va_form', 'p65_ref_file', 'n_refs', 'grade']]
    stat = {'唯一': 0, '多候选': 0, '无键': 0, '不在符号包': 0}
    for mod, fn, addr in collect():
        if only and mod != only:
            continue
        pk = idx.get(addr)
        nm = pk[0] if pk else ''
        e = addr - A1
        if not (0 <= e < len(f43)):
            continue
        ks = keys_of43(f43, e)
        v = vote(ks, p65)
        if v is None:
            stat['无键' if pk else '不在符号包'] += 1
            rows.append([mod, fn, f'0x{addr:08X}', nm, ' | '.join(t[:28] for _, t in ks[:3]),
                         '', '', '', 0, '串键不足两枚同域（要换结构对照或调用点上下文）'])
            continue
        sites = v[3][:10]
        rows.append([mod, fn, f'0x{addr:08X}', nm, ' | '.join(x[:28] for x in v[2][:4]),
                     '', '', ' '.join(f'0x{x + A1:08X}' for x in sites), v[0],
                     '候选点待定：要挑出落在函数里的那一个'])
        stat['唯一'] += 1
        print(f"[{v[0]}键同域] {mod:6s} 043 0x{addr:08X} -> 候选 "
              + " ".join(f"0x{x + A1:08X}" for x in sites[:4]) + f"  键={v[2][0][:26]!r}")

    from collections import Counter
    cnt = Counter(r[7] for r in rows[1:] if r[7])
    clash = 0
    for r in rows[1:]:
        if r[7] and cnt[r[7]] > 1:
            r[9] = f'撞车（{cnt[r[7]]} 支投到同一邻域）—— 至少一支错，要硬核'
            clash += 1
    print(f"撞车降级 {clash} 处（同一 p65 邻域被多支锚点投中，说明取体窗口还是太宽）")

    with io_open(OUT, 'w') as fh:
        w = csv.writer(fh, lineterminator='\n')
        w.writerows(rows)
    print(f"\n唯一绑定 {stat['唯一']}、多候选 {stat['多候选']}、"
          f"无键 {stat['无键']}、不在符号包 {stat['不在符号包']}")
    print(f"-> {os.path.relpath(OUT, ROOT)}")


def io_open(p, mode):
    import io
    return io.open(p, mode, encoding='utf-8', newline='')


if __name__ == '__main__':
    main()
