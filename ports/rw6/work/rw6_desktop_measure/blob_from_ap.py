# -*- coding: utf-8 -*-
"""从 RW6 的 AP 镜像里直接还原 launcher 那块 PSRAM 内容，不碰模拟器。

依据（本轮实测，逐 4KB 块比对 8MB dump 与 fw_pkg_rw6/vela_ap.bin）：
  dump[0x000000:0x118000] == ap[0x2FDC4 : 0x2FDC4+0x118000]   # 代码半段
  dump[0x118000:0x800000] == ap[0x2F5C4+0x118000 : ...]        # 串池/资源半段
两块在 flash 里差 0x800（搬运时两段的源地址各算了一次），拼起来才是运行期看到的那 8MB。
镜像里指针写在 0x3C/0x38 别名窗，Ghidra 一次只认一个段，所以统一改写成 0x1C 基。

用法：python blob_from_ap.py [ap.bin] [out.bin] [--check 活体dump.bin]
"""
import struct
import sys

ROOT = r"C:/Users/YiYang/Documents/xiaomi band 10 pro"
AP = sys.argv[1] if len(sys.argv) > 1 else ROOT + r"/fw_pkg_rw6/vela_ap.bin"
OUT = sys.argv[2] if len(sys.argv) > 2 else ROOT + r"/build/rw6_measure/rw6_blob_ap.bin"
CODE_D = 0x2FDC4      # 代码半段的 flash 偏移差
DATA_D = 0x2F5C4      # 串池半段的 flash 偏移差
SPLIT = 0x118000      # 两半的分界（4KB 粒度实测）
SIZE = 0x800000

ap = open(AP, 'rb').read()
raw = bytearray(ap[CODE_D:CODE_D + SPLIT])
raw += ap[DATA_D + SPLIT:DATA_D + SIZE]
assert len(raw) == SIZE, len(raw)

if '--check' in sys.argv:
    ref = open(sys.argv[sys.argv.index('--check') + 1], 'rb').read()
    g = 0x1000
    nt = min(len(ref), SIZE) // g
    bad = [i * g for i in range(nt) if raw[i * g:i * g + g] != ref[i * g:i * g + g]]
    print("还原块 与 活体 dump 逐 4KB 比对：一致 %d / %d，不一致 %d 块" % (
        nt - len(bad), nt, len(bad)))
    runs = []
    for o in bad:
        if runs and runs[-1][1] == o:
            runs[-1][2] = o + g
        else:
            runs.append([o, o + g, o + g])
    for s, _, e in runs[:14]:
        print("  不一致 %07x-%07x (%d KB)" % (s, e, (e - s) // 1024))
    if len(runs) > 14:
        print("  ...另 %d 段" % (len(runs) - 14))

buf = raw
# 0x3C / 0x38 / 0x18 别名窗统一成 0x1C 基：只改高字节，低 24 位是同一块物理内存的偏移
n = 0
for o in range(0, SIZE - 4, 4):
    v = struct.unpack_from('<I', buf, o)[0]
    t = v >> 24
    if t in (0x3C, 0x38, 0x18):
        buf[o:o + 4] = struct.pack('<I', (v & 0x00FFFFFF) | 0x1C000000)
        n += 1
open(OUT, 'wb').write(bytes(buf))
print("写出 %s：%d 字节，改写别名指针 %d 处" % (OUT, SIZE, n))
