#!/usr/bin/env python3
"""Run production home-stat refresh and resume bodies with host UI/procfs seams."""
from pathlib import Path
import os
import subprocess
import tempfile
import sys
import zipfile
from test_corona_blur import rust_function

ROOT = Path(__file__).resolve().parents[1]
home = (ROOT/'supervisor/src/page/home.rs').read_text(encoding='utf-8')
ui = (ROOT/'supervisor/src/ui.rs').read_text(encoding='utf-8')
if len(sys.argv) > 1:
    # Optional prior source archive proves this test catches the shipped regression.
    with zipfile.ZipFile(sys.argv[1]) as baseline:
        home = baseline.read(next(n for n in baseline.namelist() if n.endswith('/supervisor/src/page/home.rs'))).decode('utf-8')
        ui = baseline.read(next(n for n in baseline.namelist() if n.endswith('/supervisor/src/ui.rs'))).decode('utf-8')
with tempfile.TemporaryDirectory(prefix='chaos-home-stats-') as tmp:
    source = Path(tmp)/'production.rs'
    source.write_text('fn needs_tick()->bool {'+rust_function(home,'needs_tick')+'}\n'
                      +'fn tick(){'+rust_function(home,'tick')+'}\n'
                      +'unsafe fn resume(page:u32)->i32 {'+rust_function(ui,'chaos_on_resume')+'}\n',encoding='utf-8')
    env = dict(os.environ, CHAOS_HOME_SOURCE=str(source))
    exe = Path(tmp)/'home.exe'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/home_stats_harness.rs'),'-o',str(exe)],env=env,check=True)
    subprocess.run([str(exe)],check=True)
