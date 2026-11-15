#!/usr/bin/env python3
"""把 p65 的跳转壳解析成"真身地址 + 静态影子源"。

p65 的应用层大量通过 `ldr.w pc,[pc,#0]` + 一字面量的跳转壳调用 LVGL / SRAM 里的代码。
壳很好认（`5f f8 00 f0`），但字面量常常指向 SRAM/RAMX（`0x003xxxxx`），Ghidra 按 file offset
看那里是数据。做法：从活体 guest 读 SRAM 起 32 字节，在 vela_ap.bin 里搜**唯一命中**，
那就是影子源 file offset（"倒灌代码"法）。

用法：
  python scripts/p65_thunk_resolve.py 0x196A1C 0x196EF4 0x1976F4
  python scripts/p65_thunk_resolve.py 0x196A1C --asm     # 附带影子源前几条指令
"""
import os
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
IMG = os.path.join(ROOT, 'fw', 'p65_100_043', 'vela_ap.bin')
sys.path.insert(0, os.path.join(ROOT, 'work', 'probe_p65'))
import qmpmem  # noqa: E402

RAMX_TO_SRAM = 0x1FE00000


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    want_asm = '--asm' in sys.argv
    img = open(IMG, 'rb').read()
    cs = None
    if want_asm:
        from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_MCLASS
        cs = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_MCLASS)
        cs.skipdata = True
    for a in args:
        if a.startswith('file:'):
            # file:<file offset> —— 找这段代码的 SRAM 副本（早期段函数必须用它调）
            off = int(a.split(':', 1)[1], 0)
            pat = img[off:off + 24]
            dump = qmpmem.read(0x20000000, 0x1A0000)
            hits = []
            i = dump.find(pat)
            while i >= 0:
                hits.append(0x20000000 + i)
                i = dump.find(pat, i + 1)
            if len(hits) == 1:
                sram = hits[0]
                print('%#x (file) -> SRAM %#x  RAMX %#x' % (off, sram, (sram - RAMX_TO_SRAM)))
            elif not hits:
                print('%#x (file) 在 SRAM 里没找到副本' % off)
            else:
                print('%#x (file) 在 SRAM 里命中 %d 处（不唯一，不可用）: %s'
                      % (off, len(hits), ' '.join('%#x' % h for h in hits[:6])))
            continue
        off = int(a, 0)
        if img[off:off + 4] != b'\x5f\xf8\x00\xf0':
            print('%#x 不是跳转壳（%s）' % (off, img[off:off + 4].hex()))
            continue
        tgt = struct.unpack_from('<I', img, off + 4)[0]
        line = '%#x -> 真身 %#x' % (off, tgt)
        va = tgt & ~1
        if 0x00200000 <= va < 0x00400000:
            sram = va + RAMX_TO_SRAM
            data = qmpmem.read(sram, 32)
            if len(data) == 32:
                hits = []
                i = img.find(data)
                while i >= 0:
                    hits.append(i)
                    i = img.find(data, i + 1)
                if len(hits) == 1:
                    line += '  SRAM %#x  影子源 file %#x' % (sram, hits[0])
                    if cs:
                        for ins in cs.disasm(img[hits[0]:hits[0] + 0x20], 0x0C0C0000 + hits[0]):
                            line += '\n      %s %s %s' % (hex(ins.address), ins.mnemonic, ins.op_str)
                else:
                    line += '  SRAM %#x  影子源命中 %d 处（不唯一）' % (sram, len(hits))
            else:
                line += '  SRAM %#x 读不到' % sram
        else:
            line += '  （不是 SRAM/RAMX，按 file offset 直接用）'
        print(line)


if __name__ == '__main__':
    main()
