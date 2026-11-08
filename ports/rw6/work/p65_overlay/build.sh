#!/usr/bin/env bash
# 构建 p65 锚点执行探针（Vela modlib 模块）。
# 与 scripts/build_chaos.ps1 同一套目标与链接口径：thumbv8m.main-none-eabi + 软浮点 +
# -r 部分链接 + 官方式连续布局后处理。入口符号是 module_main，不链 chaos_ctor。
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
lld="$(ls "$HOME"/.rustup/toolchains/nightly*/lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe | head -1)"

feat="${1:-}"                 # 传 min 就编"只打印"那一版
out="${2:-p65_overlay.ko}"
export RUSTFLAGS="-C target-cpu=cortex-m33 -C target-feature=-fpregs"
cd "$here"
cargo +nightly build --release --target thumbv8m.main-none-eabi -Z build-std=core,compiler_builtins     ${feat:+--features "$feat"}
lib="target/thumbv8m.main-none-eabi/release/libp65_overlay.a"
"$lld" -flavor gnu -r --gc-sections -u module_main \
    -T "$root/scripts/merge_sections.ld" -o p65_overlay.ko "$lib"
python -X utf8 "$root/scripts/fix_ko_layout.py" p65_overlay.ko
ls -l p65_overlay.ko
