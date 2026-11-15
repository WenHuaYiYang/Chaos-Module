#!/usr/bin/env python3
"""Exercise production interpolation/cache and UI text cache, without device FPS claims."""
from pathlib import Path
import os
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-perf-') as tmp:
    exe=Path(tmp)/'blur'
    subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O2','-fsanitize=address,undefined',str(ROOT/'supervisor/tests/blur_performance_harness.c'),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
    exe=Path(tmp)/'text'
    subprocess.run(['rustc','--edition=2021','--test',str(ROOT/'supervisor/src/row_text_cache.rs'),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
