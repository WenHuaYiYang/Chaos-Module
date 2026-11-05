#!/usr/bin/env python3
"""Run the actual no_std NSH backend/queues against host firmware mocks.

No firmware or device calls are executed. This covers lifecycle, queues and
I/O state transitions; machine-code probes separately check the target ABI.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-nsh-tests-") as work:
        binary = Path(work) / "nsh_tests"
        subprocess.run(["rustc", "--edition=2021", "--test",
                        str(ROOT / "supervisor/tests/nsh_harness.rs"),
                        "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True, timeout=60)

if __name__ == "__main__":
    main()
