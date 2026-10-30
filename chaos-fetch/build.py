#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""build.py -- 把 tun_module.c 编成 NuttX modlib 可加载的 ARM ET_REL 模块。

工具链：clang + ld.lld（自动查找环境变量、Android NDK、PATH 和 Homebrew LLVM）。
产物：  build/tun-3.101.043.bin -- 名字叫 .bin，本体是 ELF（NuttX modlib 约定，
        Shell++ / BTMouse 同款）。安装器按 \\x7fELF + e_type==1 + e_machine==40 校验。

用法:
    python build.py                      # 编译 + 验证
    python build.py --output path/to.ko  # 验证通过后复制产物
    python build.py --no-verify-nm       # 跳过 llvm-nm 独立交叉验证

判据（全部硬失败，不是警告）:
    1. e_type == 1 (ET_REL)         -- modlib 只吃可重定位模块
    2. e_machine == 40 (EM_ARM)
    3. 零未定义符号（nm -u 无输出） -- 模块不能有外部依赖
    4. 关键符号存在: module_initialize / tun_ctor / tun_open / tun_close
       / g_data_anchor / __aeabi_unwind_cpp_pr0
    5. .data 段非零                   -- modlib 需要非零 .data 锚点
    6. .init_array 非空               -- 构造函数靠它被调用
    7. 体积 < 65536 (MAX_LOADED_SIZE)
"""
import argparse
import hashlib
import os
import re
import shutil
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "src", "tun_module.c")
DNS_SRC = os.path.join(HERE, "src", "dns_codec.c")
LD_SCRIPT = os.path.join(HERE, "tun_module.ld")
OUT_DIR = os.path.join(HERE, "build")

TARGET_ID = "xiaomi-band-10-pro-3.101.043"
FIRMWARE_VERSION = "3.101.043"
MODULE_NAME = "tun"

# ---- 工具链 -------------------------------------------------------------
# 优先使用显式环境变量，其次 Android NDK，最后 PATH / Homebrew LLVM。
# 这样 Chaos 主构建脚本在 macOS、Linux、Windows 上都能直接调用本脚本。
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
    hosts = ("darwin-x86_64", "linux-x86_64", "windows-x86_64")
    for root in _ndk_roots():
        versions = sorted(
            (d for d in os.listdir(root) if os.path.isdir(os.path.join(root, d))),
            reverse=True,
        )
        for version in versions:
            for host in hosts:
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
NM = _tool("NM", "llvm-nm")
READELF = _tool("READELF", "llvm-readelf")

# ---- 编译参数 -----------------------------------------------------------
# 基准 = BTMouse standalone/build.sh 的 CFLAGS（043 实机已验证的同场景）
#   + Shell++ 的 043 专用 -fno-jump-tables
CPU = "cortex-m33"
FLOAT_ABI = "soft"
MAX_LOADED_SIZE = 65536
MAX_BSS_SIZE = 24576

CFLAGS = [
    "--target=arm-none-eabi",
    "-mcpu=" + CPU,
    "-mthumb",
    "-mfloat-abi=" + FLOAT_ABI,
    "-Oz",                          # Shell++ 用 -Oz（比 BTMouse 的 -Os 更小）
    "-ffreestanding",
    "-fno-builtin",                 # ★ 不生成 memset/memcpy 调用（模块里 libc 不可用）
    "-fno-common",
    "-fno-stack-protector",
    "-fno-unwind-tables",           # ★ 不生成 .ARM.exidx
    "-fno-asynchronous-unwind-tables",
    "-fno-exceptions",
    "-fomit-frame-pointer",
    "-mlong-calls",                 # Shell++ 同款：固件地址 > 16MB 跨区
    "-fno-jump-tables",             # ★ 043 专用（Shell++ target 补丁）
    "-ffunction-sections",
    "-fdata-sections",
    "-Wall", "-Wextra", "-Werror",
]

REQUIRED_SYMS = [
    "module_initialize",
    "tun_ctor",
    "tun_open",
    "tun_close",
    # ★ read/write 桩必须存在：NuttX 的 O_RDWR open 要求设备至少实现 write，
    #   或同时实现 read+ioctl；缺了就是 -EACCES（app 用 O_RDWR）
    "tun_read",
    "tun_write",
    # ★ 第二步：TUNSETIFF → netdev_register（建 bt-net）
    "tun_ioctl",
    "tun_poll",
    "tun_netdev_register_once",
    # 内核回调桩：网卡注册后 ifconfig / 网络栈会 blx 这些槽位，绝不可为 NULL
    "tun_dev_ifup",
    "tun_dev_ifdown",
    "tun_dev_txavail",
    # ★ 第三步（v3.0 数据面）：devif_poll 的回调 + POLLIN 通知
    "tun_txdone",
    "tun_notify_readable",
    "tun_uninit",
    "g_data_anchor",
    "__aeabi_unwind_cpp_pr0",
]


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, errors="replace", **kw)
    if r.returncode != 0:
        sys.stderr.write("!! 失败: %s\n" % " ".join(str(c) for c in cmd))
        sys.stderr.write((r.stdout or "")[-4000:] + "\n" + (r.stderr or "")[-4000:] + "\n")
        raise SystemExit(1)
    return r


# ---- 极简 ELF32 解析（不依赖 pyelftools）--------------------------------
class Elf32:
    def __init__(self, path):
        self.data = open(path, "rb").read()
        d = self.data
        if d[:4] != b"\x7fELF":
            raise ValueError("不是 ELF")
        self.ei_class, self.ei_data, self.ei_version = d[4], d[5], d[6]
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
            if s["type"] != 2:          # SHT_SYMTAB
                continue
            strtab = self.sections[s["link"]]
            base = strtab["offset"]
            n = s["size"] // 16
            for i in range(n):
                off = s["offset"] + i * 16
                (nm, val, size, info, other, shndx) = struct.unpack_from(
                    "<IIIBBH", self.data, off)
                end = self.data.index(b"\0", base + nm)
                name = self.data[base + nm:end].decode("ascii", "replace")
                out.append(dict(name=name, value=val, size=size, info=info,
                                shndx=shndx, defined=(shndx != 0)))
        self._symbols = out
        return out

    def section(self, name):
        for s in self.sections:
            if s["name"] == name:
                return s
        return None


def check_elf(path):
    """返回 (ok, 报告行列表)。"""
    lines = []
    ok = True
    e = Elf32(path)

    lines.append("  e_type    = %d %s" % (e.e_type,
                 "(ET_REL ✓)" if e.e_type == 1 else "(✗ 必须是 1)"))
    ok &= e.e_type == 1

    lines.append("  e_machine = %d %s" % (e.e_machine,
                 "(EM_ARM ✓)" if e.e_machine == 40 else "(✗ 必须是 40)"))
    ok &= e.e_machine == 40

    cls_ok = (e.ei_class, e.ei_data, e.ei_version) == (1, 1, 1)
    lines.append("  EI_CLASS/DATA/VERSION = %d/%d/%d %s"
                 % (e.ei_class, e.ei_data, e.ei_version,
                    "(32 位小端 ✓)" if cls_ok else "(✗)"))
    ok &= cls_ok

    # 段
    for nm in (".text", ".rodata", ".data", ".bss", ".init_array"):
        s = e.section(nm)
        if s is None:
            lines.append("  段 %-12s 缺失" % nm)
        else:
            lines.append("  段 %-12s size=%-6d off=0x%06x" % (nm, s["size"], s["offset"]))
    text = e.section(".text")
    if text is None or text["size"] == 0:
        lines.append("  ✗ .text 为空")
        ok = False
    data = e.section(".data")
    if data is None or data["size"] == 0:
        lines.append("  ✗ .data 为空（modlib 需要非零 .data 锚点）")
        ok = False
    if e.section(".ARM.exidx") is not None:
        lines.append("  ✗ 出现 .ARM.exidx（应被 /DISCARD/ 丢弃）")
        ok = False
    init = e.section(".init_array")
    if init is None or init["size"] == 0:
        lines.append("  ✗ .init_array 为空（构造函数不会被调用）")
        ok = False
    # ★ v36：.bss 上限闸门（装载器给的模块 RAM 只有这么大；v36 曾因新增诊断缓冲
    # 顶到 25,664 才发现这个常量从没被真正检查过 —— 现在它会拦住超限的构建）
    bss = e.section(".bss")
    if bss is not None:
        if bss["size"] > MAX_BSS_SIZE:
            lines.append("  ✗ .bss = %d 超过上限 %d（超 %d B）"
                         % (bss["size"], MAX_BSS_SIZE, bss["size"] - MAX_BSS_SIZE))
            ok = False
        else:
            lines.append("  .bss = %d / %d ✓（余量 %d B）"
                         % (bss["size"], MAX_BSS_SIZE, MAX_BSS_SIZE - bss["size"]))

    # 符号
    syms = e.symbols
    undef = [s["name"] for s in syms if not s["defined"] and s["name"]]
    lines.append("  未定义符号: %s" % (", ".join(undef) if undef else "(无) ✓"))
    ok &= (len(undef) == 0)

    names = set(s["name"] for s in syms if s["defined"])
    missing = [n for n in REQUIRED_SYMS if n not in names]
    if missing:
        lines.append("  ✗ 缺符号: %s" % ", ".join(missing))
        ok = False
    else:
        lines.append("  必需符号齐全 ✓ (%d 个)" % len(REQUIRED_SYMS))

    # 关键符号必须是 Thumb（bit0 = 1）
    for n in ("tun_open", "tun_close", "tun_ctor", "module_initialize"):
        for s in syms:
            if s["name"] == n and s["defined"]:
                # ET_REL 里 st_value 是段内偏移，Thumb 位由重定位处理，
                # 这里只记录，不做硬判据
                lines.append("  %-22s offset=0x%04x size=%d" % (n, s["value"], s["size"]))
                break

    # ★ v71：体积判据改为「实际装载量」，不再用文件大小。
    # 旧判据 os.path.getsize() 把 .symtab/.strtab/.rel.* 这些**装载器不读**的段
    # 也算进去，于是 289112 B 的产物（实测 insmod 成功，.text=31338）被误判超限。
    # 装载器只映射带 SHF_ALLOC 的段：.text/.rodata/.data/.bss/.init_array。
    LOAD_SECTIONS = (".text", ".rodata", ".data", ".bss", ".init_array")
    load_size = 0
    for nm in LOAD_SECTIONS:
        s = e.section(nm)
        if s is not None:
            load_size += s["size"]
    file_size = os.path.getsize(path)
    lines.append("  文件大小  = %d B (含 symtab/strtab/rel，装载器不读)"
                 % file_size)
    lines.append("  装载体积  = %d B (上限 %d)  ← 真实判据"
                 % (load_size, MAX_LOADED_SIZE))
    ok &= load_size < MAX_LOADED_SIZE
    for nm in LOAD_SECTIONS:
        s = e.section(nm)
        if s is not None:
            lines.append("    %-12s %6d B" % (nm, s["size"]))

    return ok, lines


def main():
    # Windows consoles may default to GBK; verification labels contain
    # Unicode check marks. Keep build diagnostics deterministic and avoid a
    # post-link UnicodeEncodeError hiding the actual ELF result.
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except AttributeError:
            pass
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-verify-nm", action="store_true",
                    help="跳过 llvm-nm 的独立交叉验证")
    ap.add_argument("--output", help="验证通过后，额外复制产物到指定路径")
    args = ap.parse_args()

    os.makedirs(OUT_DIR, exist_ok=True)
    obj = os.path.join(OUT_DIR, "tun_module.o")
    dns_obj = os.path.join(OUT_DIR, "dns_codec.o")
    elf = os.path.join(OUT_DIR, "tun_module.elf")
    out = os.path.join(OUT_DIR, "%s-%s.bin" % (MODULE_NAME, FIRMWARE_VERSION))

    for p in (CLANG, LLD):
        if not os.path.exists(p):
            raise SystemExit("工具缺失: %s" % p)

    print("== 工具链 ==")
    print("   clang :", CLANG)
    print("   lld   :", LLD)
    print("   target: %s  cpu=%s float=%s" % (TARGET_ID, CPU, FLOAT_ABI))

    print("\n== [1/3] 编译 ==")
    cmd = [CLANG] + CFLAGS + ["-c", SRC, "-o", obj]
    print("   " + " ".join(cmd))
    run(cmd)
    print("   -> %s  (%d B)" % (os.path.basename(obj), os.path.getsize(obj)))
    cmd = [CLANG] + CFLAGS + ["-c", DNS_SRC, "-o", dns_obj]
    print("   " + " ".join(cmd))
    run(cmd)
    print("   -> %s  (%d B)" % (os.path.basename(dns_obj), os.path.getsize(dns_obj)))

    print("\n== [2/3] 部分链接 (可重定位 ELF) ==")
    # ★ 刻意不用 --gc-sections：
    #   g_data_anchor（modlib 硬要求的非零 .data 锚点）和 __aeabi_unwind_cpp_pr*
    #   都是"没人引用但必须存在"的段，GC 会把它们删掉。模块本体仅 ~1.4 KB，
    #   距 65536 上限极远，没有 GC 的必要。
    cmd = [LLD, "-flavor", "gnu", "-m", "armelf", "-r",
           "-u", "module_initialize",
           "-T", LD_SCRIPT, "-o", elf, obj, dns_obj]
    print("   " + " ".join(cmd))
    run(cmd)
    # 模块本体就是 ELF（.bin 只是文件名，与 Shellpp / BTMouse 约定一致）
    open(out, "wb").write(open(elf, "rb").read())
    print("   -> %s  (%d B)" % (os.path.basename(out), os.path.getsize(out)))

    print("\n== [3/3] 验证 ==")
    ok, lines = check_elf(out)
    for ln in lines:
        print(ln)

    # ---- 重定位类型归一化 + 闸门 ----------------------------------------
    # 只允许 {2 R_ARM_ABS32, 10 R_ARM_THM_CALL, 38 R_ARM_THM_JUMP24}（内核加载器
    # 实测能处理的那几种）。其它类型一律不放行：R_ARM_TARGET1(30) 语义上就是
    # "按静态链接当 ABS32"，直接改写成 ABS32（同样结果、但落在已验证的集合里）。
    r = run([READELF, "-r", out])
    import collections
    hist = collections.Counter()
    for ln in r.stdout.splitlines():
        for tok in ln.split():
            if tok.startswith("R_ARM_"):
                hist[tok] += 1
    print("\n  -- 重定位类型（必须只落在已验证集合里）--")
    for k in sorted(hist):
        print("     %-22s %d" % (k, hist[k]))
    raw = bytearray(open(out, "rb").read())
    ELF_TARGET1, ELF_ABS32 = 38, 2   # ★ ARM ABI: TARGET1=38, ABS32=2（THM_JUMP24=30，别搞反）
    patched = 0
    e_shoff, = struct.unpack_from("<I", raw, 0x20)
    e_shentsize, e_shnum, e_shstrndx = struct.unpack_from("<HHH", raw, 0x2E)
    secs = [struct.unpack_from("<IIIIIIIIII", raw, e_shoff + i * e_shentsize)
            for i in range(e_shnum)]
    for sc in secs:
        if sc[1] != 9:                      # SHT_REL
            continue
        for k in range(sc[5] // 8):
            o = sc[4] + k * 8
            info, = struct.unpack_from("<I", raw, o + 4)
            if (info & 0xFF) == ELF_TARGET1:
                struct.pack_into("<I", raw, o + 4, (info & 0xFFFFFF00) | ELF_ABS32)
                patched += 1
    if patched:
        open(out, "wb").write(bytes(raw))
        print("     ★ 归一化 R_ARM_TARGET1 -> R_ARM_ABS32：%d 处" % patched)
        r = run([READELF, "-r", out])
        hist = collections.Counter()
        for ln in r.stdout.splitlines():
            for tok in ln.split():
                if tok.startswith("R_ARM_"):
                    hist[tok] += 1
        print("     归一化后：%s" % ", ".join("%s=%d" % (k, hist[k]) for k in sorted(hist)))
    ALLOWED = {"R_ARM_ABS32", "R_ARM_THM_CALL", "R_ARM_THM_JUMP24"}
    bad = sorted(set(hist) - ALLOWED)
    if bad:
        print("     !! 出现未验证的重定位类型：%s" % bad)
        ok = False

    if not args.no_verify_nm and os.path.exists(NM):
        print("\n  -- llvm-nm 独立交叉验证 --")
        r = run([NM, "-u", out])
        undef = [l for l in r.stdout.splitlines() if l.strip()]
        print("     nm -u : %s" % (", ".join(undef) if undef else "(无输出 = 零未定义导入) ✓"))
        r = run([NM, out])
        for l in r.stdout.splitlines():
            if re.search(r"\b(module_initialize|tun_ctor|tun_open|tun_close|"
                         r"g_data_anchor|__aeabi_unwind_cpp_pr)\b", l):
                print("     " + l.strip())

    print("\n== 产物 ==")
    print("   %s" % out)
    print("   size %d B" % os.path.getsize(out))
    print("   md5  %s" % hashlib.md5(open(out, "rb").read()).hexdigest())

    if not ok:
        print("\n!! 验证未全过 —— 不要打包")
        raise SystemExit(2)
    if args.output:
        destination = os.path.abspath(args.output)
        os.makedirs(os.path.dirname(destination), exist_ok=True)
        shutil.copyfile(out, destination)
        print("   已复制到 %s" % destination)
    print("\n验证全过 ✓")


if __name__ == "__main__":
    main()
