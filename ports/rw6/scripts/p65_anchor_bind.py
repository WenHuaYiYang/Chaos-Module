#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 043 的官方符号（名字+入口）批量绑到 p65 的对应实现上。

为什么可行（两条都是本轮实测的）：
  - 函数序言字节**不能**跨镜像对拷（同一段 8/16 字节在 p65 全镜像 0 命中，
    而它在 043 自己里唯一命中 —— 阳性对照做过，所以这个阴性算数）；
  - 但**普通字符串**（日志格式串、断言文本、路径）在 p65 里是被代码引用的，
    只有调试名字池那批没人引用。所以用"同一枚字符串"当两侧的连接键。

分工：本脚本只做批处理（取字面量、搜串、找引用点），
候选实现一律再交给 Ghidra 反编译核对，不许拿这里的输出直接当定论。

用法：
  python scripts/p65_anchor_bind.py                 # 全部 145 条官方符号
  python scripts/p65_anchor_bind.py page_goto ...   # 只绑指定的名字
输出：docs/Chaos_p65_锚点绑定_20261018.csv（候选 + 用到的串 + 引用点数），
     stdout 打每条的候选数与最可信那一条。
"""
import csv
import io
import json
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PACK = os.path.join(ROOT, 'canopus_src', 'targets',
                    'xiaomi-band-10-pro-3.101.043', 'symbols')
FW043 = os.path.join(ROOT, 'fw_pkg_043', 'vela_ap.bin')
P65 = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
OUT = os.path.join(ROOT, 'docs', 'Chaos_p65_锚点绑定_20261018.csv')

A1 = 0x0C0C0000          # XIP 运行地址
A2 = 0x2C0C0000          # flash 的 cache 别名
BODY = 0x600             # 一条函数体最多取这么长去找字面量（宁可宽一点）
MIN_STR = 5              # 太短的串到处都有，当键没用

PRINTABLE = re.compile(rb'[\x20-\x7e]{5,}\x00')


def is_string_ptr(d, v, n):
    """v 像不像一枚指向可读 ASCII 串的指针（两个别名域都认）。"""
    for base in (A1, A2):
        if base <= v < base + n:
            f = v - base
            if f + 2 >= n:
                continue
            seg = d[f:f + 64]
            m = PRINTABLE.match(seg)
            if m and len(m.group()) >= MIN_STR:
                return f, m.group()[:-1].decode('latin1')
    return None


def literals_in_body(d, entry_file, n):
    """取函数体窗口里所有"看起来是串指针"的字面量值（批处理，不解释指令）。"""
    out = []
    end = min(entry_file + BODY, n - 4)
    for a in range(entry_file & ~3, end, 2):
        v = struct.unpack_from('<I', d, a)[0]
        got = is_string_ptr(d, v, n)
        if got:
            out.append((v, a, got[1]))
    return out


def find_in_p65(p, text, n):
    """同一枚串在 p65 里的位置（可能多处；返回 [(file, 两个候选 VA)]）。"""
    pat = text.encode('latin1') + b'\x00'
    res = []
    i = p.find(pat)
    while i >= 0:
        res.append((i, [A1 + i, A2 + i]))
        i = p.find(pat, i + 1)
    return res


def referrers(p, va, n, limit=64):
    """p65 代码里把这枚 VA 当字面量用的位置（半字对齐扫，4 字节精确匹配）。"""
    hits = []
    pat = struct.pack('<I', va)
    i = p.find(pat)
    while i >= 0 and len(hits) < limit:
        if A1 <= i + A1 < A1 + n:          # 只要落在代码域里的
            hits.append(i)
        i = p.find(pat, i + 1)
    return hits


def load_symbols(want):
    syms = []
    for fn in sorted(os.listdir(PACK)):
        if not fn.endswith('.json'):
            continue
        s = json.load(io.open(os.path.join(PACK, fn), encoding='utf-8'))
        if want and s['name'] not in want:
            continue
        entry = int(s['entry_address'], 16)
        syms.append((s['name'], entry, s.get('prototype', '')))
    return syms


def cluster(addrs):
    """把同一支函数附近的引用点并成一簇，返回按簇大小排序的 (代表地址, 成员数, 跨度)。"""
    if not addrs:
        return []
    a = sorted(addrs)
    groups, cur = [], [a[0]]
    for x in a[1:]:
        if x - cur[-1] <= 0x400:
            cur.append(x)
        else:
            groups.append(cur)
            cur = [x]
    groups.append(cur)
    out = []
    for g in groups:
        out.append((min(g), len(g), max(g) - min(g)))
    out.sort(key=lambda t: (-t[1], t[2]))
    return out


def main():
    want = set(sys.argv[1:])
    f43 = open(FW043, 'rb').read()
    p65 = open(P65, 'rb').read()
    n43, np = len(f43), len(p65)
    syms = load_symbols(want)
    if not syms:
        sys.exit('符号包里没匹配到名字（want=%s）' % (','.join(sorted(want)) or '全部'))

    rows = [['symbol', 'addr_043', 'p65_candidate_file', 'p65_candidate_run',
             'n_refs', 'span', 'key_string', 'note']]
    bound, weak, miss = 0, 0, 0
    for name, entry, proto in syms:
        ef = entry - A1
        if not (0 <= ef < n43):
            print(f"[跳过] {name}: 043 入口 0x{entry:08X} 越出镜像")
            continue
        lits = literals_in_body(f43, ef, n43)
        # 同一支函数可能引用多枚串：每枚都试一遍，取"p65 里引用点最少但非零"的那枚当键
        # （引用点越少 = 这枚串越独特 = 绑到的函数越可能只有一支）
        cands = []
        for va, _, text in lits:
            for off, vas in find_in_p65(p65, text, np):
                for v in vas:
                    refs = referrers(p65, v, np)
                    for rep, cnt, span in cluster(refs):
                        if cnt:
                            cands.append((cnt, -span, rep, text, cnt, span))
        if not cands:
            best = None
        else:
            cands.sort()
            cnt_neg, _, rep, text, cnt, span = cands[0]
            best = (0, text, (rep, cnt, span), len(lits))
        if best is None:
            miss += 1
            rows.append([name, f'0x{entry:08X}', '', '', 0, 0, '', '无可用串键'])
            print(f"[无键] {name:34s} 体内串 {len(lits)} 枚，p65 里没找到引用点")
            continue
        off, text, cand, nrefs = best
        cf, run = off, A1 + cand[0]
        rows.append([name, f'0x{entry:08X}', f'0x{cand[0]:08X}', f'0x{run:08X}',
                     cand[1], f'0x{cand[2]:X}', text[:48],
                     f'串在 p65 file 0x{off:08X}，全镜像引用 {nrefs} 处'])
        if cand[1] == 1:
            bound += 1
        else:
            weak += 1
        print(f"[{'唯一' if cand[1]==1 else '簇'+str(cand[1])}] {name:34s} "
              f"候选 run 0x{run:08X} (file 0x{cand[0]:08X})  键={text[:40]!r}")

    with io.open(OUT, 'w', encoding='utf-8', newline='') as fh:
        csv.writer(fh, lineterminator='\n').writerows(rows)
    print(f"\n合计 {len(syms)} 条：唯一簇 {bound}、多簇待定 {weak}、无键 {miss}")
    print(f"候选表 -> {os.path.relpath(OUT, ROOT)}（一律要再交 Ghidra 反编译核对才算数）")


if __name__ == '__main__':
    main()
