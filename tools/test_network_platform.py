#!/usr/bin/env python3
"""Host regressions for real p67 platform wrapper and credential/status glue."""
from pathlib import Path
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[1]
def main():
    with tempfile.TemporaryDirectory(prefix="chaos-net-platform-") as work:
        work = Path(work)
        rust = work / "platform_tests"
        subprocess.run(["rustc", "--edition=2021", "--test", str(ROOT / "supervisor/tests/network_platform_harness.rs"), "-o", str(rust)], check=True)
        subprocess.run([str(rust)], check=True, timeout=60)
        c = work / "net_glue_tests"
        subprocess.run(["clang", "-std=c11", "-g", "-fsanitize=address,undefined", "-fno-omit-frame-pointer", "-I" + str(ROOT / "supervisor/network/include"), str(ROOT / "supervisor/tests/net_glue_harness.c"), "-o", str(c)], check=True)
        subprocess.run([str(c)], check=True, timeout=60)
if __name__ == "__main__":
    main()
