#!/usr/bin/env bash
# 构建 p65 Chaos 模块（Vela modlib）。目标与脚本与主模块一致：
# thumbv8m.main-none-eabi + 软浮点、-r 部分链接、官方式连续布局后处理。
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
lld="$(ls "$HOME"/.rustup/toolchains/nightly*/lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe | head -1)"

export RUSTFLAGS="-C target-cpu=cortex-m33 -C target-feature=-fpregs"
# 编译时间进关于页（与 10P 同形），别让页面写死一个会过期的日子
export CHAOS_BUILD_DATE="$(date '+%Y-%m-%d %H:%M')"
cd "$here"
feat="${1:-}"                 # 传 autoopen 就是"装载后自己打开应用"那一版
cargo +nightly build --release --target thumbv8m.main-none-eabi -Z build-std=core,compiler_builtins ${feat:+--features "$feat"}
lib="target/thumbv8m.main-none-eabi/release/libp65_chaos.a"
"$lld" -flavor gnu -r --gc-sections -u module_main \
    -T "$root/scripts/merge_sections.ld" -o p65_chaos.ko "$lib"
python -X utf8 "$root/scripts/fix_ko_layout.py" p65_chaos.ko
ls -l p65_chaos.ko
