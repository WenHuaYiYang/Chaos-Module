# -*- coding: utf-8 -*-
"""在 launcher 那块镜像里按"字面量 == 名字串地址"反查函数体。

依据两条本轮实测：
  1. dump 的 8MB 就是 AP 文件的两段拷贝（代码半段 file+0x2FDC4，串池半段 file+0x2F5C4），
     所以这份分析可以完全离线、可复现，不依赖模拟器。
  2. 0x3C 与 0x1C 两个 PSRAM 别名窗零偏移同影（17116 个 0x3C 指针都落在可读串上），
     所以字面量池里的 0x3C23xxxx 与 0x1C23xxxx 是同一个物理位置。

用法：python name_ref.py <blob.bin> <名字> [名字...]
"""
import struct
import sys

ROOT = r"C:/Users/YiYang/Documents/xiaomi band 10 pro"
path = sys.argv[1]
names = sys.argv[2:]
buf = open(path, 'rb').read()
SIZE = len(buf)
words = memoryview(buf).cast('B')

# 预先把整段的 4 字节字取出来一次，避免每个名字重复解包
vals = struct.unpack_from('<%dI' % (SIZE // 4), buf, 0)

for nm in names:
    b = nm.encode()
    offs = []
    i = buf.find(b)
    while 0 <= i:
        if buf[i - 1:i] == b'\x00' or i == 0:
            offs.append(i)
        i = buf.find(b, i + 1)
    print("== %s  串位置 %s" % (nm, [hex(o) for o in offs]))
    for o in offs:
        s = buf[o:o + len(nm) + 1]
        if not (s.endswith(b'\x00') and len(nm) + 1 <= len(s) + 1):
            pass
        targets = set()
        for base in (0x1C000000, 0x3C000000, 0x38000000, 0x18000000):
            targets.add(base + o)
        hits = [k * 4 for k, v in enumerate(vals) if v in targets]
        for h in hits:
            # 往上找函数序言：push {...}, lr 形态
            pro = None
            for back in range(0, 0x60, 2):
                w = struct.unpack_from('<H', buf, h - back)[0]
                if (w & 0b1110100000000000) == 0b1010000000000000 and (w & 8):
                    pro = h - back
                    break
            print("   引用点 %07x  附近序言 %s" % (
                h, hex(pro) if pro else "未找到(半字线性回看 0x60)"))
