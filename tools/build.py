#!/usr/bin/env python3
"""编译 Chaos 的两个 Rust 模块并输出为 bin。

输出规则：
  installer/chaos-install/_Lua 存在时，输出到该目录；
  否则输出到 installer/。

实际编译、链接、布局修正和 ELF 校验统一复用 build_ko.sh。
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


DEFAULT_ROOT = Path(__file__).resolve().parent.parent


def run_build(repo: Path, build: Path, source: str, output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    command = ["sh", str(build), source, str(output)]
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=repo, check=True)
    if not output.is_file() or output.stat().st_size == 0:
        raise RuntimeError(f"构建完成但没有生成有效文件: {output}")
    print(f"已生成: {output} ({output.stat().st_size} bytes)", flush=True)


def build_icon(repo: Path, output: Path) -> None:
    """生成真实的 LVGL/固件图像资源；不能把 module.ko 当作图标。"""
    generator = repo / "tools" / "gen_chaos_icon.py"
    source = repo / "tools" / "chaos.png"
    command = [sys.executable, str(generator), str(source), str(output)]
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=repo, check=True)
    data = output.read_bytes()
    if len(data) != 12 + 112 * 112 * 4 or data[:4] != bytes((0x19, 0x10, 0, 0)):
        raise RuntimeError(f"生成的图标格式错误: {output}")
    print(f"已生成: {output} ({len(data)} bytes)", flush=True)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="编译 supervisor/module，并将结果输出为 chaos_sup.bin 和 chaos_icon.bin。"
    )
    parser.add_argument(
        "--repo",
        type=Path,
        default=DEFAULT_ROOT,
        help="项目根目录，默认使用本脚本所在项目。",
    )
    args = parser.parse_args()

    repo = args.repo.expanduser().resolve()
    build = repo / "tools" / "build_ko.sh"
    if not build.is_file():
        parser.error(f"找不到构建脚本: {build}")

    lua_dir = repo / "installer" / "chaos-install" / "_Lua"
    destination = lua_dir if lua_dir.is_dir() else repo / "installer"
    destination.mkdir(parents=True, exist_ok=True)

    # 使用绝对路径，避免从其他目录调用时输出位置错误。
    run_build(repo, build, "supervisor", destination / "chaos_sup.bin")
    build_icon(repo, destination / "chaos_icon.bin")

    print(f"完成：{destination}", flush=True)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (subprocess.CalledProcessError, RuntimeError) as exc:
        print(f"构建失败，退出码 {exc.returncode}", file=sys.stderr)
        raise SystemExit(getattr(exc, "returncode", 1))
