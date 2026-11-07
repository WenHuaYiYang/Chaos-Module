#!/usr/bin/env python3
"""Compile and run the real BTPAN page source against host firmware mocks."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main() -> None:
    with tempfile.TemporaryDirectory(prefix="chaos-btpan-ui-") as d:
        exe = Path(d) / "btpan_ui_tests"
        subprocess.run([
            "rustc", "+nightly", "--edition=2021", "--test",
            str(ROOT / "supervisor/tests/btpan_ui_harness.rs"), "-o", str(exe)
        ], check=True)
        subprocess.run([str(exe)], check=True)
    print("BTPAN UI host harness: passed")

if __name__ == "__main__":
    main()
