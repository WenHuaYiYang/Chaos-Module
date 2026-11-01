#!/usr/bin/env python3
"""Chaos 一键构建：Rust 模块 + fetch 模块 -> 图标 -> p67 安装器容器。

所有编译产物都写在 dist/ 下，源目录 installer/ 只保留源码：

  dist/chaos_sup.ko                Chaos 内核模块
  dist/fetch-module.ko             恢复 fetch 支持的 TUN 内核模块
  dist/chaos-installer-p67.bin     可直接安装的 p67 容器
  dist/assets/chaos_sup.bin        容器内使用的 Chaos ko 副本
  dist/assets/fetch-module.bin     容器内使用的 fetch ko 副本
  dist/assets/chaos_icon.bin       112px 应用图标
  dist/assets/chaos_icon_settings.bin  64px 设置图标
  dist/assets/lxgw.ttf             字体槽占位文件
  dist/stage/                      打包临时目录（spec.json 等）

依赖：
  * nightly Rust + rust-src + thumbv8m.main-none-eabi（build_ko.sh 使用）
  * clang + ld.lld（chaos-fetch/build.py 自动发现，可用环境变量覆盖）
  * Python 3 + Pillow（生成图标）
  * p67_pack.py（本仓库，纯 Python，无额外依赖）

常用参数：
  --skip-ko        不重新编译 Rust，复用 dist/chaos_sup.ko
  --skip-fetch     不重新编译 fetch，复用 dist/fetch-module.ko
  --no-container   只出 ko 和图标，不打容器
  --no-icons       不重新生成图标，复用 dist/assets 里的图标
"""

from __future__ import annotations

import argparse
from datetime import datetime
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parent.parent
DEVICE_DIR = "xiaomi-band-10-pro"
DEFAULT_ID = "979820260926"
DEFAULT_NAME = "Chaos 安装器"


def run(cmd: list[str], cwd: Path, env: dict | None = None) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd, env=env, check=True)


def find_icon_python(repo: Path) -> str:
    """在编译前检查 Pillow；当前 Python 缺依赖时复用项目虚拟环境。"""
    venv_python = repo / ".venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    failures = []
    # 不 resolve() 虚拟环境的符号链接，否则会绕过它的 site-packages。
    for python in dict.fromkeys((Path(sys.executable), venv_python)):
        if not python.is_file():
            continue
        try:
            result = subprocess.run(
                [str(python), "-c", "from PIL import Image, ImageDraw"],
                cwd=repo, capture_output=True, text=True,
            )
        except OSError as exc:
            failures.append(f"{python}: {exc}")
            continue
        if result.returncode == 0:
            return str(python)
        detail = result.stderr.strip() or f"exit {result.returncode}"
        failures.append(f"{python}: {detail}")
    raise RuntimeError(
        "图标生成需要 Pillow（PIL），当前 Python 和项目 .venv 均不可用。\n"
        f"请在 {repo} 中创建 .venv（建议 Python 3.12；已有环境无需重建），再执行：\n"
        f"  {venv_python.relative_to(repo)} -m pip install -r tools/requirements-build.txt\n"
        + "\n".join(failures)
    )


def build_ko(repo: Path, out: Path, build_time: str) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CHAOS_BUILD_TIME"] = build_time
    run(["sh", str(repo / "tools" / "build_ko.sh"), "supervisor", str(out)], repo, env)
    if not out.is_file() or out.stat().st_size == 0:
        raise RuntimeError(f"没有生成有效 ko: {out}")


def build_fetch(repo: Path, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    run([sys.executable, str(repo / "chaos-fetch" / "build.py"),
         "--output", str(out)], repo)
    if not out.is_file() or out.stat().st_size == 0:
        raise RuntimeError(f"没有生成有效 fetch 模块: {out}")


def build_capatch(repo: Path, out: Path) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    run([sys.executable, str(repo / "chaos-fetch" / "capatch" / "build_capatch.py"),
         "--output", str(out)], repo)
    if not out.is_file() or out.stat().st_size == 0:
        raise RuntimeError(f"没有生成有效 capatch 模块: {out}")


def build_icon(repo: Path, out: Path, size: int, python: str) -> None:
    out.parent.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CHAOS_ICON_SIZE"] = str(size)
    run([python, str(repo / "tools" / "gen_chaos_icon.py"),
         str(repo / "tools" / "chaos.png"), str(out)], repo, env)
    data = out.read_bytes()
    if len(data) != 12 + size * size * 4 or data[:4] != bytes((0x19, 0x10, 0, 0)):
        raise RuntimeError(f"图标格式错误: {out}")


def stage_installer(repo: Path, stage: Path, assets: Path) -> list[dict]:
    """把容器要带的文件按 _lua/<设备目录>/<名字> 收集到 dist/stage。"""
    font = assets / "lxgw.ttf"
    if not font.is_file():
        # 与旧包一致：字体槽只放占位说明，安装器按内容判断后跳过。
        font.write_bytes("Chaos 字体占位：字体由独立投递包安装，不是 TTF。\n".encode("utf-8"))

    sources = [
        ("main.lua", repo / "installer" / "chaos_installer.lua"),
        ("chaos_sup.bin", assets / "chaos_sup.bin"),
        ("fetch-module.bin", assets / "fetch-module.bin"),
        ("capatch.bin", assets / "capatch.bin"),
        ("ca.pem", assets / "ca.pem"),
        ("chaos_icon.bin", assets / "chaos_icon.bin"),
        ("chaos_icon_settings.bin", assets / "chaos_icon_settings.bin"),
        ("lxgw.ttf", font),
    ]
    out = []
    for name, path in sources:
        if not path.is_file():
            raise RuntimeError(f"缺少容器资源: {path}")
        staged = stage / name
        shutil.copyfile(path, staged)
        out.append({
            "name": f"_lua/{DEVICE_DIR}/{name}",
            "path": str(staged),
        })
    return out


def build_container(repo: Path, out: Path, preview: Path, stage: Path, assets: Path,
                    ident: str, name: str, style: str) -> dict:
    sys.path.insert(0, str(repo / "tools"))
    import p67_pack  # noqa: PLC0415

    files = stage_installer(repo, stage, assets)
    spec = {
        "id": ident,
        "name": name,
        "styleName": style,
        "preview": str(preview),
        "files": files,
    }
    spec_path = stage / "spec.json"
    spec_path.write_text(json.dumps(spec, ensure_ascii=False, indent=2), encoding="utf-8")
    info = p67_pack.build(spec, out)
    check = p67_pack.inspect(out)
    if check["magic"] != p67_pack.MAGIC or check["version"] != p67_pack.VERSION:
        raise RuntimeError("容器头校验失败")
    w, h = check["preview"]
    if (w, h) != (336, 480):
        raise RuntimeError(f"预览图尺寸必须是 336x480，当前 {w}x{h}")
    return info


def main() -> int:
    ap = argparse.ArgumentParser(description="Chaos 一键构建（双 ko + 图标 + p67 容器）")
    ap.add_argument("--repo", type=Path, default=DEFAULT_ROOT)
    ap.add_argument("--out", type=Path, default=None, help="容器输出路径，默认 dist/chaos-installer-p67.bin")
    ap.add_argument("--skip-ko", action="store_true", help="复用 dist/chaos_sup.ko")
    ap.add_argument("--skip-fetch", action="store_true", help="复用 dist/fetch-module.ko")
    ap.add_argument("--no-container", action="store_true")
    ap.add_argument("--no-icons", action="store_true")
    ap.add_argument("--id", default=DEFAULT_ID)
    ap.add_argument("--name", default=DEFAULT_NAME)
    ap.add_argument("--style", default=DEFAULT_NAME)
    ap.add_argument("--preview", type=Path, default=None, help="预览块，默认 tools/p67-preview.block")
    args = ap.parse_args()

    repo = args.repo.expanduser().resolve()
    icon_python = None if args.no_icons else find_icon_python(repo)
    if icon_python is not None:
        print(f"图标 Python: {icon_python}", flush=True)
    dist = repo / "dist"
    assets = dist / "assets"
    stage = dist / "stage"
    assets.mkdir(parents=True, exist_ok=True)
    stage.mkdir(parents=True, exist_ok=True)
    build_time = datetime.now().astimezone().strftime("%Y-%m-%d %H:%M:%S %z")

    ko = dist / "chaos_sup.ko"
    if not args.skip_ko:
        build_ko(repo, ko, build_time)
    elif not ko.is_file():
        raise RuntimeError("--skip-ko 但 dist/chaos_sup.ko 不存在")
    shutil.copyfile(ko, assets / "chaos_sup.bin")
    print(f"ko: {ko} ({ko.stat().st_size} bytes)")

    fetch = dist / "fetch-module.ko"
    capatch = dist / "capatch.elf"
    if not args.skip_fetch:
        build_fetch(repo, fetch)
        build_capatch(repo, capatch)
    elif not fetch.is_file() or not capatch.is_file():
        raise RuntimeError("--skip-fetch 但 dist/fetch-module.ko 或 dist/capatch.elf 不存在")
    shutil.copyfile(fetch, assets / "fetch-module.bin")
    shutil.copyfile(capatch, assets / "capatch.bin")
    shutil.copyfile(repo / "chaos-fetch" / "assets" / "ca.pem", assets / "ca.pem")
    print(f"fetch: {fetch} ({fetch.stat().st_size} bytes)")
    print(f"capatch: {capatch} ({capatch.stat().st_size} bytes)")

    if icon_python is not None:
        build_icon(repo, assets / "chaos_icon.bin", 112, icon_python)
        build_icon(repo, assets / "chaos_icon_settings.bin", 64, icon_python)
    else:
        # 兼容旧目录：首次迁移时把 installer/ 里的图标搬进 dist/assets。
        for icon in ("chaos_icon.bin", "chaos_icon_settings.bin"):
            dst = assets / icon
            src = repo / "installer" / icon
            if not dst.is_file() and src.is_file():
                shutil.copyfile(src, dst)

    if args.no_container:
        print(f"完成（未打容器）: {dist}")
        return 0

    preview = args.preview or repo / "tools" / "p67-preview.block"
    if not preview.is_file():
        raise RuntimeError(f"缺少预览块: {preview}")
    out = args.out or dist / "chaos-installer-p67.bin"
    info = build_container(repo, out, preview, stage, assets, args.id, args.name, args.style)
    print(f"容器: {out} ({info['size']} bytes)")
    for name, size in info["blocks"]:
        print(f"  {name}  {size}")
    print(f"完成: {out}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (subprocess.CalledProcessError, RuntimeError, ValueError) as exc:
        print(f"构建失败: {exc}", file=sys.stderr)
        raise SystemExit(getattr(exc, "returncode", 1) or 1)
