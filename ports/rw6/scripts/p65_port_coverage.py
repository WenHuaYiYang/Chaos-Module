#!/usr/bin/env python3
"""端口覆盖率：Chaos supervisor 用到的 043 固件地址，有多少在 p65 侧已有定案对应。

做两件事：
  1. 从 `Chaos-Module/supervisor/src` 里抽出所有 `0x0Cxxxxxx` 字面量（带文件与行号）；
  2. 用 043 地址表把地址换成名字，再看 p65 地址表里有没有同名行（`p65.<名字>`）。
输出：总数 / 已覆盖 / 缺口清单（按出现次数排序）。

用法：python scripts/p65_port_coverage.py [--json]
"""
import csv
import glob
import io
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, 'Chaos-Module', 'supervisor', 'src')
T043 = os.path.join(ROOT, 'docs', 'fw_address_table_043.csv')
T65 = os.path.join(ROOT, 'docs', 'fw_address_table_p65.csv')


def load_table(path):
    rows = {}
    for r in csv.DictReader(io.open(path, encoding='utf-8')):
        try:
            rows[r['name']] = int(r['address'], 16)
        except (KeyError, ValueError):
            continue
    return rows


def main():
    t43 = load_table(T043)
    t65 = load_table(T65)
    # 显式映射：043 地址 -> p65 地址（人工核对过的，见 docs/p65_anchor_map.csv）
    amap = {}
    amap_path = os.path.join(ROOT, 'docs', 'p65_anchor_map.csv')
    if os.path.exists(amap_path):
        for r in csv.DictReader(io.open(amap_path, encoding='utf-8')):
            try:
                amap[int(r['addr043'], 16)] = (r['name65'], r['grade'])
            except (KeyError, ValueError):
                continue
    by_addr = {}
    for name, a in t43.items():
        by_addr.setdefault(a, []).append(name)
    # p65 已定案的名字集合（p65.lv_label_set_text -> lv_label_set_text；也接受 043.xxx 同名尾巴）
    have = set()
    for name in t65:
        have.add(name)
        have.add(name.replace('p65.', ''))
    uses = {}
    for p in glob.glob(os.path.join(SRC, '**', '*.rs'), recursive=True):
        for ln, line in enumerate(io.open(p, encoding='utf-8', errors='replace'), 1):
            for m in re.finditer(r'0x0C[0-9A-Fa-f]{6}', line):
                a = int(m.group(0), 16)
                uses.setdefault(a, []).append('%s:%d' % (os.path.relpath(p, ROOT), ln))
    total = len(uses)
    covered, missing = [], []
    for a, sites in uses.items():
        if a in amap:
            covered.append((a, amap[a][0], len(sites)))
            continue
        names = by_addr.get(a, [])
        hit = [n for n in names if n in have or n.split('.', 1)[1] in have]
        if hit:
            covered.append((a, hit[0], len(sites)))
        else:
            missing.append((a, names[0] if names else '<无名>', len(sites), sites[0]))
    missing.sort(key=lambda x: -x[2])
    if '--json' in sys.argv:
        print(json.dumps({'total': total, 'covered': len(covered), 'missing': missing}, ensure_ascii=False, indent=1))
        return
    print('supervisor 用到固件地址 %d 个；p65 侧已有对应 %d 个（%.0f%%）'
          % (total, len(covered), 100.0 * len(covered) / max(1, total)))
    print('--- 缺口（按引用次数）---')
    for a, name, n, site in missing[:40]:
        print('  %#010x  %-42s 用 %d 处  例 %s' % (a, name, n, site))


if __name__ == '__main__':
    main()
