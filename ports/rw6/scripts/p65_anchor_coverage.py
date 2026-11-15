#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Chaos 用到的每个固件入口在 p65 上"有没有名字可对"的覆盖率表。

为什么要先算这个：移植的全部工作量都压在"给每个 fw_api 锚点找到 p65 的实现"上，
而可行的批量办法只有一条 —— 拿 043 官方符号包的名字去 p65 的字符串/名字段里找同名的格子。
名字在 p65 里根本不存在的那些（LVGL 核心 API 就是这种），只能逐支靠结构+动态核，
成本高一个量级。先分清这两类，才知道该先做什么、哪批只能往后放。

fw_api 里的地址一律现读现算：`transmute(0x0C54_CA3Dusize)` 这种带下划线的写法要先去下划线。
输出按模块分组的三档统计：有名字 / 名字成对可搜 / 无名字（要逐支硬啃）。
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, 'Chaos-Module', 'supervisor', 'src')
PACK = os.path.join(ROOT, 'canopus_src', 'targets',
                    'xiaomi-band-10-pro-3.101.043', 'symbols')
P65 = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
A1 = 0x0C0C0000

ADDR = re.compile(r'transmute\(\s*(0x[0-9A-Fa-f_]+)usize')
FN = re.compile(r'pub (?:unsafe )?fn ([a-z0-9_]+)')


def pack_index():
    """043 入口地址 -> 官方符号（名字 + 原型 + 证据等级）。"""
    by_addr = {}
    for f in sorted(os.listdir(PACK)):
        if not f.endswith('.json'):
            continue
        s = json.load(open(os.path.join(PACK, f), encoding='utf-8'))
        try:
            e = int(s['entry_address'], 16)
        except (KeyError, ValueError):
            continue
        by_addr[e & ~1] = (s['name'], s.get('prototype', ''), s.get('status', ''))
    return by_addr


def collect():
    """扫 fw_api.rs 与 fw_api/*.rs：每个 transmute 地址配上它所在 pub fn 的名字。"""
    out = []
    names = ['fw_api.rs'] + ['fw_api/' + x for x in
                             sorted(os.listdir(os.path.join(SRC, 'fw_api')))]
    for name in names:
        p = os.path.join(SRC, name)
        if not os.path.isfile(p):
            continue
        lines = open(p, encoding='utf-8').read().splitlines()
        cur = '?'
        for ln in lines:
            m = FN.search(ln)
            if m:
                cur = m.group(1)
            for a in ADDR.findall(ln):
                v = int(a.replace('_', ''), 16) & ~1
                out.append((name.split('/')[-1].replace('.rs', ''), cur, v))
    return out


def main():
    idx = pack_index()
    p65 = open(P65, 'rb').read()
    anchors = collect()
    print(f"043 官方符号包 {len(idx)} 条；Chaos 侧用到 {len(anchors)} 处 transmute 地址\n")

    mods = {}
    named = unnamed = 0
    rows = []
    for mod, fn, addr in anchors:
        pk = idx.get(addr)
        if pk:
            nm, proto, status = pk
            innm = p65.count(nm.encode() + b'\x00')
            kind = '有名可搜' if innm else '有名无串'
            named += 1
        else:
            nm, proto, status, innm, kind = '', '', '', 0, '不在符号包'
            unnamed += 1
        mods.setdefault(mod, [0, 0, 0])
        mods[mod][0] += 1
        if nm:
            mods[mod][1] += 1
        if innm:
            mods[mod][2] += 1
        rows.append((mod, fn, f'0x{addr:08X}', nm, kind, innm, proto[:44], status))

    print(f"{'模块':10s} {'锚点数':>6} {'有官方名':>8} {'名字串在p65里':>12}")
    for m, (t, n, s) in sorted(mods.items()):
        print(f"{m:10s} {t:6d} {n:8d} {s:12d}")
    print(f"\n合计：官方符号覆盖 {named} 处，未覆盖 {unnamed} 处；"
          f"其中名字串能在 p65 镜像里搜到的只有 {sum(r[5] > 0 for r in rows)} 处")

    out = os.path.join(ROOT, 'docs', 'Chaos_p65_锚点覆盖_20261018.csv')
    with open(out, 'w', encoding='utf-8', newline='') as fh:
        w = __import__('csv').writer(fh, lineterminator='\n')
        w.writerow(['module', 'wrapper', 'addr_043', 'fw_name_043', 'kind',
                    'name_hits_in_p65', 'prototype', 'proof'])
        w.writerows(rows)
    print(f"明细 -> {os.path.relpath(out, ROOT)}")
    print("读法：name_hits_in_p65=0 的那批没有名字可对，只能靠结构+动态逐支核；"
          "这一列就是移植的真实成本所在。")


if __name__ == '__main__':
    sys.exit(main())
