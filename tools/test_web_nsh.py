#!/usr/bin/env python3
"""Host regression for the independent web NSH source; no device I/O."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-web-nsh-tests-") as work:
        binary = Path(work) / "web_nsh_tests"
        subprocess.run(["rustc", "--edition=2021", "--test",
                        str(ROOT / "supervisor/tests/web_nsh_harness.rs"),
                        "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True, timeout=60)

if __name__ == "__main__":
    main()
