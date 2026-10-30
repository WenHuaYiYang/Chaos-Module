#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""build_capatch.py — 构建 capatch 模块（改 libcurl CA 路径指针到 /data）

工具链：Android NDK clang + ld.lld（与 tun 模块同款）。
产物：  capatch.elf（ET_REL，NuttX modlib 可加载）

判据：
  1. e_type == 1 (ET_REL)
  2. e_machine == 40 (EM_ARM)
  3. 零未定义符号
  4. 关键符号存在：module_initialize / mm_ctor / p_read / p_write
  5. .data 段非零（modlib 需要非零 .data 锚点）
  6. .init_array 非空（构造函数靠它被调用）
  7. 装载体积 < 65536
"""
import argparse
import os
import shutil
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "capatch.c")
LD_SCRIPT = os.path.join(HERE, "m.ld")
OUT_ELF = os.path.join(HERE, "capatch.elf")
OUT_OBJ = os.path.join(HERE, "capatch.o")


def _ndk_roots():
    roots = [os.environ.get("ANDROID_NDK_ROOT"), os.environ.get("ANDROID_NDK_HOME")]
    sdk = os.environ.get("ANDROID_SDK_ROOT") or os.environ.get("ANDROID_HOME")
    if sdk:
        roots.extend([os.path.join(sdk, "ndk"), os.path.join(sdk, "ndk-bundle")])
    if sys.platform == "darwin":
        roots.append(os.path.expanduser("~/Library/Android/sdk/ndk"))
    elif sys.platform == "win32":
        roots.append(os.path.expandvars(r"%LOCALAPPDATA%\Android\Sdk\ndk"))
    return [r for r in roots if r and os.path.isdir(r)]


def _tool(env_name, executable):
    override = os.environ.get(env_name)
    if override:
        return override
    exe = executable + (".exe" if sys.platform == "win32" else "")
    for root in _ndk_roots():
        versions = sorted((d for d in os.listdir(root)
                           if os.path.isdir(os.path.join(root, d))), reverse=True)
        for version in versions:
            for host in ("darwin-x86_64", "linux-x86_64", "windows-x86_64"):
                candidate = os.path.join(root, version, "toolchains", "llvm",
                                         "prebuilt", host, "bin", exe)
                if os.path.isfile(candidate):
                    return candidate
    found = shutil.which(executable)
    if found:
        return found
    for directory in ("/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin"):
        candidate = os.path.join(directory, executable)
        if os.path.isfile(candidate):
            return candidate
    return executable


CLANG = _tool("CLANG", "clang")
LLD = _tool("LLD", "ld.lld")

CPU = "cortex-m33"
FLOAT_ABI = "soft"
MAX_LOADED_SIZE = 65536
MAX_BSS_SIZE = 24576

CFLAGS = [
    "--target=arm-none-eabi",
    "-mcpu=" + CPU,
    "-mthumb",
    "-mfloat-abi=" + FLOAT_ABI,
    "-Oz",
    "-ffreestanding",
    "-fno-builtin",
    "-fno-common",
    "-fno-stack-protector",
    "-fno-unwind-tables",
    "-fno-asynchronous-unwind-tables",
    "-fno-exceptions",
    "-fomit-frame-pointer",
    "-mlong-calls",
    "-fno-jump-tables",
    "-ffunction-sections",
    "-fdata-sections",
    "-Wall", "-Wextra", "-Werror",
]

REQUIRED_SYMS = ["module_initialize", "mm_ctor", "p_read", "p_write",
                 "do_patch", "__aeabi_unwind_cpp_pr0"]


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    if r.returncode != 0:
        sys.stderr.write("!! 失败: %s\n" % " ".join(cmd))
        sys.stderr.write((r.stdout or "")[-4000:] + "\n")
        sys.stderr.write((r.stderr or "")[-4000:] + "\n")
        raise SystemExit(1)
    return r


class Elf32:
    def __init__(self, path):
        self.data = open(path, "rb").read()
        d = self.data
        if d[:4] != b"\x7fELF":
            raise ValueError("不是 ELF")
        (self.e_type, self.e_machine, self.e_version, self.e_entry, self.e_phoff,
         self.e_shoff, self.e_flags, self.e_ehsize, self.e_phentsize,
         self.e_phnum, self.e_shentsize, self.e_shnum,
         self.e_shstrndx) = struct.unpack_from("<HHIIIIIHHHHHH", d, 16)
        self.sections = []
        for i in range(self.e_shnum):
            off = self.e_shoff + i * self.e_shentsize
            (name, stype, flags, addr, shoff, size, link, info, align,
             entsize) = struct.unpack_from("<IIIIIIIIII", d, off)
            self.sections.append(dict(name_off=name, type=stype, flags=flags,
                                      addr=addr, offset=shoff, size=size,
                                      link=link, info=info, align=align,
                                      entsize=entsize, index=i))
        shstr = self.sections[self.e_shstrndx]
        base = shstr["offset"]
        for s in self.sections:
            end = d.index(b"\0", base + s["name_off"])
            s["name"] = d[base + s["name_off"]:end].decode("ascii", "replace")
        self._symbols = None

    @property
    def symbols(self):
        if self._symbols is not None:
            return self._symbols
        out = []
        for s in self.sections:
            if s["type"] != 2:
                continue
            strtab = self.sections[s["link"]]
            base = strtab["offset"]
            n = s["size"] // 16
            for i in range(n):
                off = s["offset"] + i * 16
                (nm, val, size, info, other, shndx) = struct.unpack_from(
                    "<IIIBBH", self.data, off)
                end = self.data.index(b"\0", base + nm)
                out.append(dict(name=self.data[base + nm:end].decode("ascii", "replace"),
                                value=val, size=size, defined=(shndx != 0)))
        self._symbols = out
        return out

    def section(self, name):
        for s in self.sections:
            if s["name"] == name:
                return s
        return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", help="验证通过后复制 capatch.elf 到指定路径")
    args = parser.parse_args()
    for p in (CLANG, LLD):
        if not os.path.exists(p):
            raise SystemExit("工具缺失: %s" % p)
    print("== 工具链 ==")
    print("   clang :", CLANG)
    print("   lld   :", LLD)

    print("\n== [1/2] 编译 ==")
    cmd = [CLANG] + CFLAGS + ["-c", SRC, "-o", OUT_OBJ]
    run(cmd)
    print("   -> %s (%d B)" % (os.path.basename(OUT_OBJ), os.path.getsize(OUT_OBJ)))

    print("\n== [2/2] 部分链接 ==")
    cmd = [LLD, "-flavor", "gnu", "-m", "armelf", "-r",
           "-u", "module_initialize",
           "-T", LD_SCRIPT, "-o", OUT_ELF, OUT_OBJ]
    run(cmd)
    print("   -> %s (%d B)" % (os.path.basename(OUT_ELF), os.path.getsize(OUT_ELF)))

    print("\n== 校验 ==")
    e = Elf32(OUT_ELF)
    ok = True
    print("  e_type    = %d %s" % (e.e_type, "(ET_REL OK)" if e.e_type == 1 else "(FAIL)"))
    ok &= e.e_type == 1
    print("  e_machine = %d %s" % (e.e_machine, "(EM_ARM OK)" if e.e_machine == 40 else "(FAIL)"))
    ok &= e.e_machine == 40
    for nm in (".text", ".rodata", ".data", ".bss", ".init_array"):
        s = e.section(nm)
        if s is None:
            print("  FAIL 段 %s 缺失" % nm)
            ok = False
        else:
            print("  段 %-12s size=%-6d" % (nm, s["size"]))
    if e.section(".data") is None or e.section(".data")["size"] == 0:
        print("  FAIL .data 为空")
        ok = False
    if e.section(".init_array") is None or e.section(".init_array")["size"] == 0:
        print("  FAIL .init_array 为空")
        ok = False
    bss = e.section(".bss")
    if bss and bss["size"] > MAX_BSS_SIZE:
        print("  FAIL .bss %d > %d" % (bss["size"], MAX_BSS_SIZE))
        ok = False
    syms = e.symbols
    undef = [s["name"] for s in syms if not s["defined"] and s["name"]]
    print("  未定义符号: %s" % (", ".join(undef) if undef else "(无) OK"))
    ok &= not undef
    names = set(s["name"] for s in syms if s["defined"])
    missing = [n for n in REQUIRED_SYMS if n not in names]
    if missing:
        print("  FAIL 缺符号: %s" % ", ".join(missing))
        ok = False
    else:
        print("  必需符号齐全 OK")
    load = 0
    for nm in (".text", ".rodata", ".data", ".bss", ".init_array"):
        s = e.section(nm)
        if s:
            load += s["size"]
    print("  装载体积 = %d B (上限 %d)" % (load, MAX_LOADED_SIZE))
    ok &= load < MAX_LOADED_SIZE
    print("\n%s" % ("BUILD: PASS" if ok else "BUILD: FAIL"))
    if ok and args.output:
        destination = os.path.abspath(args.output)
        os.makedirs(os.path.dirname(destination), exist_ok=True)
        shutil.copyfile(OUT_ELF, destination)
        print("已复制到 %s" % destination)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
