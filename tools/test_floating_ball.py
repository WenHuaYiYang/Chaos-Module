#!/usr/bin/env python3
"""Exercise the production ball controller and native overlay against a host VFS/display."""
from pathlib import Path
import subprocess
import tempfile
from verify_floating_ball_services import verify_sources

ROOT = Path(__file__).resolve().parents[1]

def main():
    verify_sources()
    with tempfile.TemporaryDirectory(prefix="chaos-floating-ball-") as temp:
        binary = Path(temp) / "floating-ball-tests"
        subprocess.run(["rustc", "--edition=2021", "--test",
                        str(ROOT / "supervisor/tests/floating_ball_harness.rs"),
                        "-o", str(binary)], check=True, timeout=60)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)

if __name__ == "__main__":
    main()
