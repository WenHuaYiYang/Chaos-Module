#!/usr/bin/env python3
"""p67 容器打包/解析（纯 Python，无第三方依赖）。

格式来自官方 Canopus 管理器安装包 ref/canopus-mgr-10p-v65539.bin：
  0x000 主头 0xA8
  0x0A8 11 x (u32 count, u32 offset)
  0x100 风格名 72B
  0x148 基础定义表 16B
  0x158 资源定义表 N*16B
  之后  根属性 16B
  之后  预览块（原样字节）
  之后  资源块，首尾相接，无对齐
资源块: u32 (dataLen | nameLen<<24) + 16B 0 + ASCII 名 + 数据。

这个文件只做容器，不编译 Rust；build.py 负责调用它。
"""

from __future__ import annotations

import argparse
import struct
from pathlib import Path

MAGIC = 0x1234A55A
VERSION = 0x00010003
HDR = 0xA8
PAIR_OFF = 0xA8
PAIRS = 11
BASE_OFF = 0x148
SCRIPT_OFF = 0x158
DEF_SIZE = 16


def _u32(v: int) -> bytes:
    return struct.pack("<I", v & 0xFFFFFFFF)


def _u16(v: int) -> bytes:
    return struct.pack("<H", v & 0xFFFF)


def _pad(data: bytes, n: int) -> bytes:
    return data[:n].ljust(n, b"\0")


def _def(index: int, typ: int, offset: int, length: int) -> bytes:
    b = bytearray(DEF_SIZE)
    b[0:2] = _u16(index)
    b[3] = typ
    b[8:12] = _u32(offset)
    b[12:16] = _u32(length)
    return bytes(b)


def read_block(buf: bytes, off: int) -> dict:
    packed = struct.unpack_from("<I", buf, off)[0]
    data_len = packed & 0xFFFFFF
    name_len = packed >> 24
    name = buf[off + 20 : off + 20 + name_len].decode("latin1")
    data_off = off + 20 + name_len
    return {
        "name": name,
        "data": buf[data_off : data_off + data_len],
        "offset": off,
        "length": 20 + name_len + data_len,
    }


def inspect(path: Path) -> dict:
    buf = path.read_bytes()
    if len(buf) < HDR:
        raise ValueError("文件太短")
    pairs = [struct.unpack_from("<II", buf, PAIR_OFF + i * 8) for i in range(PAIRS)]
    script_count, script_off = pairs[6]
    files = []
    for i in range(script_count):
        index = struct.unpack_from("<H", buf, script_off + i * DEF_SIZE)[0]
        typ = buf[script_off + i * DEF_SIZE + 3]
        off = struct.unpack_from("<I", buf, script_off + i * DEF_SIZE + 8)[0]
        length = struct.unpack_from("<I", buf, script_off + i * DEF_SIZE + 12)[0]
        blk = read_block(buf, off)
        files.append({"index": index, "type": typ, "name": blk["name"], "size": len(blk["data"])})
    preview_off = struct.unpack_from("<I", buf, 0x20)[0]
    width, height = struct.unpack_from("<HH", buf, preview_off + 4)
    return {
        "magic": struct.unpack_from("<I", buf, 0)[0],
        "version": struct.unpack_from("<I", buf, 4)[0],
        "id": buf[0x28:0x34],
        "name": buf[0x68:0xA4].split(b"\0", 1)[0].decode("utf-8", "replace"),
        "preview": (width, height),
        "files": files,
        "size": len(buf),
    }


def build(spec: dict, out: Path) -> dict:
    preview = Path(spec["preview"]).read_bytes()
    entries = []
    for item in spec["files"]:
        name = item["name"].encode("latin1")
        if len(name) > 0xFF:
            raise ValueError(f"资源名太长: {item['name']}")
        data = Path(item["path"]).read_bytes()
        if len(data) > 0xFFFFFF:
            raise ValueError(f"资源太大: {item['name']}")
        entries.append((name, data))

    n = len(entries)
    props_off = SCRIPT_OFF + n * DEF_SIZE
    preview_off = props_off + DEF_SIZE
    cursor = preview_off + len(preview)

    blocks = []
    for name, data in entries:
        block = _u32(len(data) | (len(name) << 24)) + b"\0" * 16 + name + data
        blocks.append((name, block, cursor))
        cursor += len(block)

    buf = bytearray(cursor)
    struct.pack_into("<II", buf, 0, MAGIC, VERSION)
    struct.pack_into("<I", buf, 0x10, 0x00000800)
    struct.pack_into("<I", buf, 0x14, 0x00010000)
    struct.pack_into("<H", buf, 0x1C, 1)
    struct.pack_into("<H", buf, 0x1E, 0)
    struct.pack_into("<I", buf, 0x20, preview_off)
    buf[0x28:0x34] = _pad(spec.get("id", "979820260926").encode("latin1"), 12)
    buf[0x68:0xA4] = _pad(spec.get("name", "Chaos 安装器").encode("utf-8"), 60)
    style_name = spec.get("styleName") or spec.get("style_name") or "Chaos 安装器"
    buf[0x100:0x148] = _pad(style_name.encode("utf-8"), 72)

    pairs = [
        (0x80000000, preview_off),
        (1, BASE_OFF),
        (0, SCRIPT_OFF),
        (0, SCRIPT_OFF),
        (0, SCRIPT_OFF),
        (0, SCRIPT_OFF),
        (n, SCRIPT_OFF),
        (0, props_off),
        (0, props_off),
        (0, props_off),
        (0, props_off),
    ]
    for i, (cnt, off) in enumerate(pairs):
        struct.pack_into("<II", buf, PAIR_OFF + i * 8, cnt, off)

    buf[BASE_OFF : BASE_OFF + DEF_SIZE] = _def(0, 0, props_off, DEF_SIZE)
    for i, (_, block, off) in enumerate(blocks):
        buf[SCRIPT_OFF + i * DEF_SIZE : SCRIPT_OFF + (i + 1) * DEF_SIZE] = _def(i, 5, off, len(block))

    entry_index = int(spec.get("entry_index", 0))
    struct.pack_into("<H", buf, props_off, entry_index)
    buf[props_off + 2] = 0
    buf[props_off + 3] = 5

    buf[preview_off : preview_off + len(preview)] = preview
    for _, block, off in blocks:
        buf[off : off + len(block)] = block

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(buf)
    return {"size": len(buf), "preview_off": preview_off, "blocks": [(n.decode("latin1"), len(data)) for n, data in entries]}


def main() -> int:
    ap = argparse.ArgumentParser(description="p67 容器打包/解析")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("inspect")
    p.add_argument("file", type=Path)
    p = sub.add_parser("build")
    p.add_argument("spec", type=Path)
    p.add_argument("out", type=Path)
    args = ap.parse_args()

    import json

    if args.cmd == "inspect":
        info = inspect(args.file)
        info["id"] = info["id"].split(b"\0", 1)[0].decode("latin1")
        print(json.dumps(info, ensure_ascii=False, indent=2))
    else:
        spec = json.loads(args.spec.read_text(encoding="utf-8"))
        print(json.dumps(build(spec, args.out), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
