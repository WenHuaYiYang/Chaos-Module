#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把"串键命中点"解析成 p65 的函数入口，产出可用的锚点表。

为什么还要这一步：p65 里按 PSRAM VA 搜到的串引用点，绝大多数落在**文字池/描述表**上而不是
函数体内（`who` 的 CONTAIN 全为 NONE）。Thumb 的文字池通常挨在自己函数尾巴后面，
所以要问"这格之前最近的那支函数是谁、隔多远"：距离小 = 这池子就是它的；
距离大 = 那是另一张表，不能算。

用法：
  python scripts/ghidra_fw.py who <目标> $(python scripts/p65_anchor_resolve.py --sites) \
      > _probe/who.txt
  python scripts/p65_anchor_resolve.py _probe/who.txt

输入 docs/Chaos_p65_锚点绑定_20261018.csv（p65_anchor_map.py 产出），
输出 docs/Chaos_p65_锚点解析_20261018.csv。
距离上限 0x200 字节：实测 14 处落在这个范围内，最远的一处 0x1154 明显是另一张表，直接筛掉。
"""
import csv
import io
import os
import re
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIND = os.path.join(ROOT, 'docs', 'Chaos_p65_锚点绑定_20261018.csv')
OUT = os.path.join(ROOT, 'docs', 'Chaos_p65_锚点解析_20261018.csv')
A1 = 0x0C0C0000
MAX_GAP = 0x200
OWN_CACHE = {}


def own_site(p65, text):
    """这枚键自己的池位置：先找串，再找按 PSRAM VA / XIP / 缓存别名三种形态引用它的那一格。
    不能用绑定表里的整串候选点 —— 邻域窗 ±0x400 会把隔壁函数的池也圈进来
    （实测 add 与 del 因此撞到同一支函数）。同一枚键被两处引用时取第一处，需要人工分开。
    同一枚键会在多行锚点里重复出现，而每次都要在 11MB 镜像里扫四遍，所以带缓存。"""
    POOL = 0x3C000000 - 0x2FDC4
    if text in OWN_CACHE:
        return OWN_CACHE[text]
    i = p65.find(text.encode('latin1') + bytes([0]))
    got = None
    while i >= 0:
        for va in (i + POOL, A1 + i, 0x2C0C0000 + i):
            j = p65.find(struct.pack('<I', va))
            if j >= 0:
                got = j
                break
        if got is not None:
            break
        i = p65.find(text.encode('latin1') + bytes([0]), i + 1)
    OWN_CACHE[text] = got
    return got


def image():
    return open(os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin'), 'rb').read()


def keylist(r):
    """这支锚点要问的键，**按优先级排好**：自己名字键在最前，其余是同体里的日志串。
    空串必须剔掉 —— 拿 b'\\0' 去镜像里找会命中第一处填充字节，然后按那个假下标算出
    一个"引用点"，实测把几十行都推到了同一支毫不相干的函数上。"""
    keys = [k.strip().rstrip('…') for k in r[4].split(' | ')]
    keys = [k for k in keys if k]
    name = r[3].strip()
    return ([name] if name else []) + [k for k in keys if k != name]


def sites():
    """需要问 `who` 的站点 = 每枚键**自己**的池位置（与解析时查的是同一批，共用 `keylist`）。
    以前这里打的是绑定表第 7 列的邻域候选点，与解析时查的那批不是同一批地址，
    结果 97 支里 83 支被记成"命中点不在紧邻函数上"—— 那是工具口径不符，不是真阴性。"""
    p65 = image()
    rows = list(csv.reader(io.open(BIND, encoding='utf-8')))
    s = set()
    for r in rows[1:]:
        for key in keylist(r):
            j = own_site(p65, key)
            if j is not None:
                s.add(j)
    print(' '.join('0x%08X' % v for v in sorted(s)))
    sys.stderr.write('键自己的池位置 %d 处（来自 %d 支锚点）\n' % (len(s), len(rows) - 1))


def parse_who(path):
    out = {}
    for ln in io.open(path, encoding='utf-8', errors='replace'):
        m = re.search(r'WHO,(0x[0-9A-Fa-f]+),(CONTAIN|BEFORE),([0-9a-f]+),(\S+),(\d+)', ln)
        if m:
            out[int(m.group(1), 16)] = (m.group(2), int(m.group(3), 16), int(m.group(5)))
            continue
        m = re.search(r'WHO,(0x[0-9A-Fa-f]+),NONE', ln)
        if m:
            out[int(m.group(1), 16)] = ('NONE', 0, 0)
    return out


def main():
    argv = sys.argv[1:]
    if argv and argv[0] == '--sites':
        sites()
        return 0
    if not argv:
        sys.exit(__doc__)
    who = parse_who(argv[0])
    p65 = image()

    rows = list(csv.reader(io.open(BIND, encoding='utf-8')))
    out = [['module', 'addr_043', 'fw_name_043', 'p65_entry_run', 'p65_entry_file',
            'how', 'gap', 'n_keys', 'decided_by', 'other_votes', 'key_string', 'grade']]
    ok = 0
    for r in rows[1:]:
        # 键的顺序就是优先级：**这支锚点自己的名字键排在最前**，其余是同体里的日志串。
        # 以前这里按"距离最小"投票，结果 add 那一行被隔壁 del 的池赢走
        # （del 的池距函数入口 0x84，add 自己的池距 0xE0）—— 距离小不等于"是我的池"。
        ordered = keylist(r)
        votes = []
        for key in ordered:
            site = own_site(p65, key)
            if site is None:
                continue
            f = who.get(site)
            if not f or f[0] == 'NONE' or f[2] >= MAX_GAP:
                continue
            votes.append((key, f))
        if not ordered:
            out.append([r[0], r[2], r[3], '', '', '', '', r[8], '', '', '',
                        '没有串键（要换结构对照或调用点上下文）'])
            continue
        if not votes:
            out.append([r[0], r[2], r[3], '', '', '', '', r[8], '', '', r[4][:40],
                        '命中点不在紧邻函数上（是别的表/池），待定'])
            continue
        k0, best = votes[0]
        others = [k for k, f in votes[1:] if f[1] != best[1]]
        how = '池在函数尾巴' if best[0] == 'CONTAIN' else '池在函数之后'
        out.append([r[0], r[2], r[3], f'0x{best[1] + A1:08X}', f'0x{best[1]:08X}',
                    how, f'0x{best[2]:X}', r[8], k0, ' | '.join(others), r[4][:40],
                    '候选：还要 Ghidra 核参数 + 模拟器核被执行' +
                    ('（其余键指向别处，只信首键）' if others else '')])
        ok += 1
    with io.open(OUT, 'w', encoding='utf-8', newline='') as fh:
        w = csv.writer(fh, lineterminator='\n')
        w.writerows(out)
    print(f"解析出函数入口 {ok} 处 / 共 {len(out)-1} 处锚点")
    for r in out[1:]:
        if r[3]:
            print(f"   {r[0]:6s} 043 {r[1]} {r[2] or '-':26s} -> p65 run {r[3]} "
                  f"({r[5]} 距 {r[6]}, 首键 {r[8][:26]}"
                  f"{', 分歧 ' + r[9][:26] if r[9] else ''})")
    print(f"-> {os.path.relpath(OUT, ROOT)}")


if __name__ == '__main__':
    main()
