#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""fix_ko_layout.py — 把 chaos_sup.ko 的节区 sh_addr 重写为连续布局。

背景(逆向固件自带模块的结论):
  - rust-lld -r 产出所有节 sh_addr=0；固件自带(可加载)模块的 .rodata/.data/.bss 是连续非零地址。
  - Vela 加载器/模块枚举路径会读取节地址信息填入模块对象，全 0 布局是切表盘崩溃的头号嫌疑。

规则(对齐固件自带模块的实际形状):
  - .text 从 addr 0 开始(.text addr=0, offset=0x34，ELF 头不占地址)
  - .rodata/.data/.bss 按节头顺序分配连续地址（.rodata 8 对齐，其余 4 对齐）
  - .init_array/.fini_array/.ARM.exidx 保持 addr=0(这些节由加载器按类型单独处理)

用法: python fix_ko_layout.py <in.ko> [out.ko]  (默认原地覆盖)
"""
import struct
import sys

def fix(path, out=None):
    out = out or path
    d = bytearray(open(path, 'rb').read())
    e_shoff, = struct.unpack_from('<I', d, 32)
    e_shnum, e_shstrndx = struct.unpack_from('<HH', d, 0x30)
    secs = []
    for i in range(e_shnum):
        off = e_shoff + i * 40
        v = struct.unpack_from('<10I', d, off)
        secs.append(dict(idx=i, hdr_off=off, name_off=v[0], type=v[1], flags=v[2],
                         addr=v[3], offset=v[4], size=v[5], align=v[8]))
    shstr = secs[e_shstrndx]
    def nm(n):
        e = d.index(b'\x00', shstr['offset'] + n)
        return d[shstr['offset'] + n:e].decode()
    for s in secs:
        s['name'] = nm(s['name_off'])

    # 需要 sh_addr 连续排布的"内容节"
    CONTENT = ('.text', '.rodata', '.data', '.bss')
    cur = 0
    changed = []
    for s in secs:
        if s['name'] not in CONTENT or s['size'] == 0:
            continue
        align = 8 if s['name'] == '.rodata' else 4
        cur = (cur + align - 1) & ~(align - 1)
        new_addr = cur
        cur = new_addr + s['size']
        if s['type'] == 8:  # NOBITS (bss): 地址推进但不占文件
            pass
        if new_addr != s['addr']:
            struct.pack_into('<I', d, s['hdr_off'] + 12, new_addr)
            changed.append((s['name'], s['addr'], new_addr))
        # .rodata 对齐声明同步为 8（对齐 官方）
        if s['name'] == '.rodata' and s['align'] != 8:
            struct.pack_into('<I', d, s['hdr_off'] + 32, 8)
            changed.append((s['name'] + '.align', s['align'], 8))

    # 数组节 flags 对齐 官方: WA(3) 而非 A(2)（官方 .init_array/.fini_array flags=3）
    for s in secs:
        if s['name'] in ('.init_array', '.fini_array') and (s['flags'] & 1) == 0:
            struct.pack_into('<I', d, s['hdr_off'] + 8, s['flags'] | 1)
            changed.append((s['name'] + '.flags', s['flags'], s['flags'] | 1))

    open(out, 'wb').write(bytes(d))
    print(f"fixed {out}: {len(changed)} fields")
    for name, old, new in changed:
        print(f"  {name}: 0x{old:X} -> 0x{new:X}")
    # 打印最终布局
    print(f"\n{'name':<16}{'type':<12}{'addr':>8}{'size':>8}")
    for i in range(e_shnum):
        off = e_shoff + i * 40
        name_off, typ, flags, addr = struct.unpack_from('<4I', d, off)
        size = struct.unpack_from('<I', d, off + 20)[0]
        n = nm(name_off) if i != 0 else ''
        if typ != 0:
            print(f"{n:<16}{typ:<12}0x{addr:06X}0x{size:06X}")

if __name__ == '__main__':
    fix(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
