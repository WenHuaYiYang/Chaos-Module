#!/usr/bin/env python3
"""从 p65 镜像里按 LVGL `__func__` 日志串反查函数入口。

口径（本轮定案）：p65 的 LVGL 9 代码带日志，每个会打日志的函数体内有一个
字面量指向它自己的函数名串（`LV_LOG_TRACE` 的 `__func__` 实参），而名字串本身
没有静态引用（Ghidra 的 text 模式查不到）。所以：

  file(串) --+0x3BFD023C--> PSRAM VA --在镜像里找 4 字节字面量--> file(字面量)
  --> Ghidra who(字面量) --> 所在函数入口

输出 CSV：name,str_file_off,ref_file_off,func_entry,delta（都是 file offset；
运行地址 = file + 0x0C0C0000（XIP）或 + 0x3BFD023C（PSRAM））。

用法：
  python scripts/p65_nameref.py                  # 提取 + 查 who（一次 headless）
  python scripts/p65_nameref.py --names lv_      # 只看某个前缀
"""
import os
import re
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
PSRAM_DELTA = 0x3BFD023C  # file + delta = PSRAM VA
OUT = None  # 按前缀分文件：docs/p65_nameref_<prefix>.csv


def literals_for(data, va):
    pat = struct.pack('<I', va & 0xFFFFFFFF)
    out = []
    i = data.find(pat)
    while i >= 0:
        if i % 4 == 0:
            out.append(i)
        i = data.find(pat, i + 1)
    return out


def main():
    prefix = 'lv_'
    if '--names' in sys.argv:
        prefix = sys.argv[sys.argv.index('--names') + 1]
    data = open(IMG, 'rb').read()
    pat = re.compile(rb'(?:%s)[a-zA-Z0-9_]{2,}' % prefix.encode())
    rows = []
    for m in pat.finditer(data):
        # 只认独立成串（前后是 \0 或非标识符）
        st, en = m.start(), m.end()
        if st > 0 and (chr(data[st - 1]).isalnum() or data[st - 1] in b'_'):
            continue
        if en < len(data) and (chr(data[en]).isalnum() or data[en] in b'_'):
            continue
        name = m.group().decode()
        refs = literals_for(data, PSRAM_DELTA + st)
        if not refs:
            rows.append((name, st, None, None, None))
        for r in refs:
            rows.append((name, st, r, None, None))
    out_path = os.path.join(ROOT, 'docs', 'p65_nameref_%s.csv' % prefix.rstrip('_'))
    print('名字 %d 条，字面量命中 %d 条' % (len(rows), sum(1 for r in rows if r[2] is not None)))

    addrs = [r[2] for r in rows if r[2] is not None]
    who = {}
    if addrs:
        cmd = [sys.executable, os.path.join(ROOT, 'scripts', 'ghidra_fw.py'), 'who',
               'firmware-watch6-p65-100043'] + ['0x%X' % a for a in addrs]
        p = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace')
        for line in p.stdout.splitlines():
            if line.startswith('WHO,'):
                parts = line.split(',')
                if len(parts) < 6:
                    continue
                _, a, how, entry = parts[:4]
                tail = parts[4:]
                delta = tail[-1].split()[0]
                fname = ','.join(tail[:-1])
                who[int(a, 16)] = (how, int(entry, 16), fname, int(delta))
        if not who:
            print(p.stdout[-2000:], p.stderr[-1000:])
    out = []
    for name, st, r, _, _ in rows:
        if r is None:
            out.append((name, st, None, '', ''))
        else:
            w = who.get(r)
            out.append((name, st, r, ('' if not w else '0x%X' % w[1]),
                        ('' if not w or w[0] != 'CONTAIN' else str(w[3]))))
    with open(out_path, 'w', encoding='utf-8', newline='') as f:
        f.write('name,str_file_off,ref_file_off,func_entry,delta\n')
        for name, st, r, e, d in out:
            assert isinstance(st, int), (name, st)
            f.write('%s,0x%X,%s,%s,%s\n' % (name, st, '' if r is None else '0x%X' % r, e, d))
    print('->', os.path.relpath(out_path, ROOT))


if __name__ == '__main__':
    main()
