# -*- coding: utf-8 -*-
"""blob(PSRAM 侧串池) 与 flash(XIP 侧代码) 之间的跨域引用扫描。

为什么要它：Ghidra 12.1.4 导入裸镜像会 WARN "Skipping unsupported -loader-Binary
Base Address"，工程实际基址是 0，所以 profile 里写的运行地址 0x1C000000 从来没有生效。
后果是 blob 里的串在 flash 工程里不成"字符串"、flash 里的代码在 blob 工程里不成"引用"——
两边互相看不见，"零引用"这种阴性结论就是这么来的（本轮已定案并推翻上一轮）。

真实形态：代码在 flash（文件偏移 F，运行地址 F+0x0C0C0000），字面量是串在 PSRAM 的
运行地址（0x1C/0x3C 别名零偏移同影，低 24 位就是 blob 内偏移）。所以要找"谁用了这条串"，
得拿 0x3C000000+偏移 去 **AP 文件** 里搜字面量，而不是在镜像里搜字符串引用。

用法：python flash_ref.py <ap.bin> <blob.bin> <关键字正则> [最多条数]
"""
import re
import struct
import sys

ROOT = r"C:/Users/YiYang/Documents/xiaomi band 10 pro"
AP = sys.argv[1] if len(sys.argv) > 1 else ROOT + r"/fw_pkg_rw6/vela_ap.bin"
BLOB = sys.argv[2] if len(sys.argv) > 2 else ROOT + r"/build/rw6_measure/rw6_psram.bin"
KEY = re.compile((sys.argv[3] if len(sys.argv) > 3 else
                  r'icon|scale|zoom|slide|scroll|layout|widget').encode(), re.I)
LIMIT = int(sys.argv[4]) if len(sys.argv) > 4 else 60

ap = open(AP, 'rb').read()
blob = open(BLOB, 'rb').read()
BASES = (0x1C000000, 0x3C000000, 0x38000000, 0x18000000)

# AP 文件的字面量窗：只看看起来像代码/常量区的段，按 4 字节步长取词
words = {}
for i in range(0, len(ap) - 4, 4):
    v = struct.unpack_from('<I', ap, i)[0]
    if (v >> 24) in (0x1C, 0x3C, 0x38, 0x18):
        words.setdefault((v & 0x00FFFFFF), []).append(i)

rows = []
seen = set()
for m in re.finditer(rb'[ -~]{8,}', blob[0x118000:0x3CF000]):
    t = m.start() + 0x118000
    s = m.group()
    if not KEY.search(s):
        continue
    # 归并到整条串的起点，避免同一条被反复统计
    a = blob.rfind(b'\x00', 0, t) + 1
    if a in seen:
        continue
    seen.add(a)
    hits = words.get(a, [])
    if hits:
        rows.append((a, s[:52], hits))
rows.sort(key=lambda r: -len(r[2]))
print("跨域引用（blob 串 <- flash 侧字面量）命中 %d 条，按引用数排序前 %d 条：" % (len(rows), LIMIT))
for a, s, hits in rows[:LIMIT]:
    va = [hex(h + 0x0C0C0000) for h in hits[:4]]
    print("  blob串@%07x  flash引用 %d 处  文件偏移 %s  VA %s\n      %r" % (
        a, len(hits), [hex(h) for h in hits[:4]], va, s))
