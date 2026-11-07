#!/bin/sh
# 编译网络栈 C 代码 -> network/build/*.o（供 build_ko.sh 链入 chaos_sup.ko）。
# 依赖: clang --target=thumbv8m.main-none-eabi（与 corona_blur.c 同一套软浮点 ABI）。
set -e
here=$(cd "$(dirname "$0")" && pwd)
exec python3 "$here/build_network.py" "$@"
