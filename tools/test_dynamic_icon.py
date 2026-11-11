#!/usr/bin/env python3
"""Run the production icon controller against a host launcher/VFS fixture."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-dynamic-icon-") as directory:
        for name in ("dynamic_icon_harness", "dynamic_icon_frames_cases", "app_icon_page_harness"):
            exe = Path(directory) / name
            subprocess.run(["rustc", "--edition=2021", "--test",
                            str(ROOT / "supervisor/tests" / (name + ".rs")),
                            "-o", str(exe)], check=True, timeout=60)
            subprocess.run([str(exe), "--test-threads=1"], check=True, timeout=30)

if __name__ == "__main__":
    main()
