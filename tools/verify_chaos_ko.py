# -*- coding: utf-8 -*-
"""verify_chaos_ko.py — 校验自研 .ko: ET_REL/ARM/e_entry/零未定义/.init_array/重定位类型"""
import sys, struct
# 注意: 不要用 io.TextIOWrapper 换掉 sys.stdout —— 那个 wrapper 在解释器退出时被 GC,
# 会对已关闭的 buffer 再 flush 一次并抛异常, 结果是"校验全部通过但退出码 1",
# 还会把调用它的 build 脚本一起带成失败。reconfigure 改的是同一个对象, 没有这个问题。
sys.stdout.reconfigure(encoding="utf-8")

path = sys.argv[1]
data = open(path, "rb").read()
assert data[:4] == b"\x7fELF", "not ELF"
assert data[4:7] == b"\x01\x01\x01", "expected ELF32 little-endian v1"
assert struct.unpack_from("<H", data, 16)[0] == 1, "not ET_REL"
assert struct.unpack_from("<H", data, 18)[0] == 40, "not ARM"
e_entry = struct.unpack_from("<I", data, 24)[0]
print(f"ELF OK: {path} ({len(data)}B) e_entry=0x{e_entry:X}")

shoff = struct.unpack_from("<I", data, 32)[0]
shentsize = struct.unpack_from("<H", data, 46)[0]
shnum = struct.unpack_from("<H", data, 48)[0]
shstrndx = struct.unpack_from("<H", data, 50)[0]
assert shentsize == 40 and 0 < shstrndx < shnum, "invalid ELF32 section table"
assert shoff + shnum * shentsize <= len(data), "truncated section table"
shstr_off = struct.unpack_from("<I", data, shoff + shstrndx * shentsize + 16)[0]
shstr = data[shstr_off:]

secs = {}
for i in range(shnum):
    e = shoff + i * shentsize
    nm = struct.unpack_from("<I", data, e)[0]
    name = shstr[nm:shstr.find(b"\x00", nm)].decode(errors="replace")
    typ, flags, off, sz = (struct.unpack_from("<I", data, e + x)[0] for x in (4, 8, 16, 20))
    secs[name] = (typ, flags, off, sz)

symtab = secs.get(".symtab")
stroff = secs.get(".strtab", (0, 0, 0, 0))[2]
strtab = data[stroff:] if stroff else b""
undef = []
if symtab:
    typ, _, off, sz = symtab
    for j in range(sz // 16):
        se = off + j * 16
        sn = struct.unpack_from("<I", data, se)[0]
        shndx = struct.unpack_from("<H", data, se + 14)[0]
        if shndx == 0:
            nm2 = strtab[sn:strtab.find(b"\x00", sn)].decode(errors="replace")
            if nm2:
                undef.append(nm2)
print("未定义符号:", undef if undef else "无 (OK)")
assert symtab and not undef, "unresolved symbols cannot be loaded by Chaos"
assert secs.get(".text", (0, 0, 0, 0))[3] > 0, "missing merged .text"

init = [n for n in secs if n.startswith(".init_array")]
print("init_array:", [(n, secs[n][3]) for n in init] if init else "无!")
assert any(secs[n][3] >= 4 for n in init), "missing module constructor"

rel = [n for n in secs if n.startswith(".rel")]
types = {}
for n in rel:
    _, _, off, sz = secs[n]
    for j in range(sz // 8):
        r_info = struct.unpack_from("<I", data, off + j * 8 + 4)[0]
        t = r_info & 0xFF
        types[t] = types.get(t, 0) + 1
print("重定位类型:", {t: c for t, c in sorted(types.items())})
assert types and set(types) <= {2, 10, 30, 42, 47, 48}, "unexpected ARM relocation type"
allocated = {name: sec[3] for name, sec in secs.items() if sec[1] & 2}
print("静态加载内存（不含任务栈、动态堆、加载器开销）:", sum(allocated.values()), "bytes", allocated)
