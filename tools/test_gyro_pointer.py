#!/usr/bin/env python3
"""Exercise the production wrist-cursor controller and owner-thread lifecycle."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-gyro-pointer-") as tmp:
        for name in ("gyro_pointer_cases", "gyro_pointer_harness"):
            source = ROOT / "supervisor/tests" / (name + ".rs")
            if not source.exists():
                raise FileNotFoundError(source)
            output = Path(tmp) / name
            subprocess.run(["rustc", "--edition=2021", "--test", str(source), "-o", str(output)],
                           check=True, timeout=60)
            subprocess.run([str(output), "--test-threads=1"], check=True, timeout=60)

if __name__ == "__main__":
    main()
