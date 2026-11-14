#!/usr/bin/env python3
"""Chaos 一键构建：Rust 模块 + fetch 模块 -> 图标 -> p67 安装器容器。

所有编译产物都写在 dist/ 下，源目录 installer/ 只保留源码：

  dist/chaos_sup.ko                Chaos 内核模块
  dist/fetch-module.ko             恢复 fetch 支持的 TUN 内核模块
  dist/chaos-installer-p67.bin     可直接安装的 p67 容器
  dist/assets/chaos_sup.bin        容器内使用的 Chaos ko 副本
  dist/assets/fetch-module.bin     容器内使用的 fetch ko 副本
  dist/assets/chaos_icon.bin       96px 应用图标
  dist/assets/chaos_icon_settings.bin  96px 设置图标
  dist/assets/lxgw.ttf             字体槽占位文件
  dist/stage/                      打包临时目录（spec.json 等）
  dist/THIRD-PARTY-NETWORK.txt      网络组件来源、版权与许可（也随容器分发）

依赖：
  * nightly Rust + rust-src + thumbv8m.main-none-eabi（build_ko.sh 使用）
  * clang + ld.lld（chaos-fetch/build.py 自动发现，可用环境变量覆盖）
  * Python 3 + Pillow（生成图标与 GIF 帧）
  * p67_pack.py（本仓库，纯 Python，无额外依赖）

常用参数：
  --skip-ko        不重新编译 Rust，复用 dist/chaos_sup.ko
  --skip-fetch     不重新编译 fetch，复用 dist/fetch-module.ko
  --no-container   只出 ko 和图标，不打容器
  --no-icons       不重新生成图标，复用 dist/assets 里的图标
"""

from __future__ import annotations

import argparse
import hashlib
from datetime import datetime
import json
import os
import shutil
import subprocess
import sys
import struct
from pathlib import Path

from network_notices import network_notices

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
    # Bootstrap the original icon; the module applies the saved selection.
    source = repo / "tools" / "chaos.png"
    run([python, str(repo / "tools" / "gen_chaos_icon.py"), str(source), str(out)], repo, env)
    data = out.read_bytes()
    if len(data) != 12 + size * size * 4 or data[:4] != bytes((0x19, 0x10, 0, 0)):
        raise RuntimeError(f"图标格式错误: {out}")


def build_app_icons(repo: Path, python: str) -> None:
    for name in ("dark-potato", "dark-icon"):
        run([python, str(repo / "tools/gen_chaos_icon.py"),
             str(repo / f"tools/app-icons/{name}.png"),
             str(repo / f"supervisor/assets/app-icons/{name}.bin")], repo)


def module_sections(path: Path) -> dict:
    """区分加载内容、BSS和符号/重定位，避免把.a/.ko/容器大小混为一谈。"""
    data = path.read_bytes()
    if data[:5] != b"\x7fELF\x01":
        raise RuntimeError(f"不是ELF32模块: {path}")
    off = struct.unpack_from('<I', data, 32)[0]
    size, count, names = struct.unpack_from('<HHH', data, 46)
    headers = [struct.unpack_from('<10I', data, off + i * size) for i in range(count)]
    table = headers[names]
    strings = data[table[4]:table[4] + table[5]]
    sections = {}
    for h in headers:
        name = strings[h[0]:].split(b'\0', 1)[0].decode('ascii')
        if name:
            sections[name] = {'bytes': h[5], 'stored_bytes': 0 if h[1] == 8 else h[5],
                              'allocated': bool(h[2] & 2)}
    return sections


def write_build_report(dist: Path, ko: Path, assets: Path, container: Path | None) -> None:
    paths = [ko, dist / 'fetch-module.ko', dist / 'capatch.elf',
             dist / 'THIRD-PARTY-NETWORK.txt', *sorted(assets.iterdir())]
    if container is not None:
        paths.append(container)
    report = {'module_sections': module_sections(ko), 'files': {}}
    for path in paths:
        if path.is_file():
            data = path.read_bytes()
            report['files'][str(path)] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    (dist / 'build-report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print('模块节大小:', {k: v['stored_bytes'] for k, v in report['module_sections'].items() if v['stored_bytes']})
    print(f'构建资源清单与SHA-256: {dist / "build-report.json"}')


def find_objcopy() -> str | None:
    # llvm/rust 的 objcopy 优先: mingw 的 GNU objcopy 不认 ARM ELF, 会整份失败。
    candidates = []
    for extra in ("C:/Program Files/LLVM/bin/llvm-objcopy.exe",
                  os.path.expanduser("~/.cargo/bin/rust-objcopy.exe")):
        if os.path.isfile(extra):
            candidates.append(extra)
    for name in ("llvm-objcopy", "rust-objcopy", "objcopy"):
        found = shutil.which(name)
        if found:
            candidates.append(found)
    return candidates[0] if candidates else None


def strip_ko_file(path: Path) -> None:
    """容器内的 ko 副本就地 --strip-unneeded（旧链 2026-10-24 起的默认口径）。

    局部符号表对固件加载器无用（重定位只认 symtab 下标），strip 只省体积；
    构建产物 supervisor/chaos_sup.ko 本身不 strip，调试与 verify_chaos_ko.py
    要看符号。找不到 objcopy 或 strip 失败都保持原样并告警（不算构建失败）。
    """
    oc = find_objcopy()
    if oc is None:
        print("WARN: 找不到 objcopy, chaos_sup.bin 未 strip (容器会大 ~50KB)")
        return
    before = path.stat().st_size
    tmp = path.parent / (path.name + ".tmp")
    try:
        r = subprocess.run([oc, "--strip-unneeded", str(path), str(tmp)],
                           capture_output=True, text=True)
        if r.returncode != 0:
            raise RuntimeError(r.stderr.strip()[:200] or f"exit {r.returncode}")
    except BaseException as exc:
        if tmp.is_file():
            tmp.unlink()
        print(f"WARN: strip 失败({exc}), chaos_sup.bin 按未 strip 打包")
        return
    os.replace(tmp, path)
    print(f"ko strip (容器内副本): {before} -> {path.stat().st_size} bytes")


def stage_installer(repo: Path, stage: Path, assets: Path) -> list[dict]:
    """把容器要带的文件按 _lua/<设备目录>/<名字> 收集到 dist/stage。"""
    font = assets / "lxgw.ttf"
    if not font.is_file():
        # 与旧包一致：字体槽只放占位说明，安装器按内容判断后跳过。
        font.write_bytes("Chaos 字体占位：字体由独立投递包安装，不是 TTF。\n".encode("utf-8"))

    # 安装器脚本打包时瘦身（旧链 minify_lua 逻辑）：只去注释/行首缩进/空行，
    # token 序列逐字不变（下面用 tokens() 对比断言）。源文件保持可读。
    sys.path.insert(0, str(repo / "tools"))
    import minify_lua  # noqa: PLC0415
    text = (repo / "installer" / "chaos_installer.lua").read_text(encoding="utf-8")
    slim = minify_lua.strip_source(text)
    if minify_lua.tokens(text) != minify_lua.tokens(slim):
        raise RuntimeError("安装器瘦身改变了 token 序列, 中止打包")
    main_min = assets / "main.min.lua"
    main_min.write_text(slim, encoding="utf-8", newline="\n")
    print(f"安装器 lua: {len(text.encode('utf-8'))} -> {len(slim.encode('utf-8'))} bytes (去注释/缩进/空行)")

    # ko 副本按旧链口径 strip：直接覆盖 assets 里的容器副本（旧链同款）。
    strip_ko_file(assets / "chaos_sup.bin")

    sources = [
        ("main.lua", main_min),
        ("chaos_sup.bin", assets / "chaos_sup.bin"),
        ("fetch-module.bin", assets / "fetch-module.bin"),
        ("capatch.bin", assets / "capatch.bin"),
        ("ca.pem", assets / "ca.pem"),
        ("chaos_icon.bin", assets / "chaos_icon.bin"),
        ("chaos_icon_settings.bin", assets / "chaos_icon_settings.bin"),
        ("lxgw.ttf", font),
        ("THIRD-PARTY-NETWORK.txt", assets.parent / "THIRD-PARTY-NETWORK.txt"),
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
    # 逐项验证容器仍完整携带全部资源，而非靠遗漏模块/图片缩包。
    packed = out.read_bytes()
    count, table = struct.unpack_from('<II', packed, p67_pack.PAIR_OFF + 6 * 8)
    if count != len(files):
        raise RuntimeError('容器资源数量不一致')
    for i, item in enumerate(files):
        offset = struct.unpack_from('<I', packed, table + i * p67_pack.DEF_SIZE + 8)[0]
        block = p67_pack.read_block(packed, offset)
        if block['name'] != item['name'] or block['data'] != Path(item['path']).read_bytes():
            raise RuntimeError(f'容器资源完整性校验失败: {item["name"]}')
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
        if icon_python is not None:
            build_app_icons(repo, icon_python)
        # build_ko.sh统一执行无损素材预处理，直接编译也使用同一资源。
        if icon_python is not None and (repo / "tools/chaos.gif").is_file():
            run([icon_python, str(repo / "tools/build_gif_icon.py")], repo)
        build_ko(repo, ko, build_time)
    elif not ko.is_file():
        raise RuntimeError("--skip-ko 但 dist/chaos_sup.ko 不存在")
    else:
        print('注意: --skip-ko复用已有模块，不会把新增源码或内置图片编入旧模块。')
    shutil.copyfile(ko, assets / "chaos_sup.bin")
    print(f"ko: {ko} ({ko.stat().st_size} bytes)")
    (dist / "THIRD-PARTY-NETWORK.txt").write_text(network_notices(repo), encoding="utf-8")

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
        build_icon(repo, assets / "chaos_icon.bin", 96, icon_python)
        build_icon(repo, assets / "chaos_icon_settings.bin", 96, icon_python)
    else:
        # 兼容旧目录：首次迁移时把 installer/ 里的图标搬进 dist/assets。
        for icon in ("chaos_icon.bin", "chaos_icon_settings.bin"):
            dst = assets / icon
            src = repo / "installer" / icon
            if not dst.is_file() and src.is_file():
                shutil.copyfile(src, dst)

    if args.no_container:
        write_build_report(dist, ko, assets, None)
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
    write_build_report(dist, ko, assets, out)
    print(f"完成: {out}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (subprocess.CalledProcessError, RuntimeError, ValueError) as exc:
        print(f"构建失败: {exc}", file=sys.stderr)
        raise SystemExit(getattr(exc, "returncode", 1) or 1)
