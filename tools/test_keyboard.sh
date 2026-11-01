#!/bin/sh
# Compile the real keyboard.rs against host UI/save mocks; no ARM runtime needed.
set -eu
keyboard_repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$keyboard_repo/supervisor/target/keyboard-tests"
rustc --edition=2021 --test "$keyboard_repo/supervisor/tests/keyboard_harness.rs" \
  -o "$keyboard_repo/supervisor/target/keyboard-tests/keyboard_tests"
"$keyboard_repo/supervisor/target/keyboard-tests/keyboard_tests"
