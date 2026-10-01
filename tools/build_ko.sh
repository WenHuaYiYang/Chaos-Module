#!/bin/sh
# 构建 supervisor 内核模块 -> supervisor/chaos_sup.ko
#
# 用法: sh tools/build_ko.sh [模块目录] [输出 .ko]
#   默认模块目录 = <仓库根>/supervisor, 默认输出 = <模块目录>/chaos_sup.ko
#
# 前置: nightly 工具链 + rust-src 组件(build-std 要现编 core / compiler_builtins)。
#
# 三个 flag 各在救一个坑:
#   * --target thumbv8m.main-none-eabi 与 -C target-feature=-fpregs —— 软浮点, 与固件自带
#     模块的 e_flags / .ARM.attributes 一致; 硬浮点编出来的加载器不认。
#   * -Z build-std=core,compiler_builtins —— 目标平台上没有现成的 std, 只能现编。
#   * --gc-sections 与 -T merge_sections.ld —— 不 GC 会把 core 的整个 .text 拖进来
#     (实测 550KB, 而 insmod 上限是 256KB); 不 merge 则 .text 大小为 0, 加载器会
#     一声不吭地"装成功", 而模块里一条指令都没有。
#
# 链完还要 fix_ko_layout.py 把各节 sh_addr 从全 0 重排成连续布局(rust-lld -r 不会自己排),
# 最后 verify_chaos_ko.py 要求未定义符号为 0 —— 这个模块没有解析外部符号的阶段,
# 留一个就是运行时跳飞。
set -e

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
src=$(cd "${1:-$root/supervisor}" && pwd)

out=${2:-"$src/chaos_sup.ko"}
case $out in
  /*) ;;
  *) out="$(pwd)/$out" ;;
esac

cd "$src"
RUSTFLAGS="-C target-cpu=cortex-m33 -C target-feature=-fpregs" \
  cargo +nightly build --release --target thumbv8m.main-none-eabi \
    -Z build-std=core,compiler_builtins

lib=$(ls target/thumbv8m.main-none-eabi/release/lib*.a | head -n 1)
host=$(rustc +nightly -vV | sed -n 's/^host: //p')
lld="$(rustc +nightly --print sysroot)/lib/rustlib/$host/bin/rust-lld"
echo "rust-lld: $lld"

"$lld" -flavor gnu -r --gc-sections -u module_main -u chaos_ctor \
       -T "$here/merge_sections.ld" -o "$out" "$lib"

python3 "$here/fix_ko_layout.py" "$out"
python3 "$here/verify_chaos_ko.py" "$out"
echo "OK: $out"
