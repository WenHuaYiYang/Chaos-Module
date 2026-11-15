#!/usr/bin/env python3
"""Compare capture paths and APNG workloads using the production size optimization level."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="chaos-capture-bench-") as directory:
    binary = Path(directory) / "bench"
    subprocess.run(["rustc", "--edition=2021", "-C", "opt-level=z", str(ROOT / "supervisor/tests/capture_bench.rs"), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
print("HOST SYNTHETIC ENCODER ONLY: not device fps, DMA timing, VFS or flash throughput")
