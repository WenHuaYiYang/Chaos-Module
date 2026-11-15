#!/usr/bin/env python3
"""Run the same PTY engine lifecycle suite with the separate CCN1 backend."""
from pathlib import Path
import subprocess,tempfile
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-ccn-nsh-') as tmp:
    src=(ROOT/'supervisor/tests/web_nsh_harness.rs').read_text()
    src=src.replace('../src/web_nsh.rs',str(ROOT/'supervisor/src/connect_nsh.rs'))
    src=src.replace('mod web_nsh;','mod connect_nsh;').replace('crate::web_nsh::chaos_web_terminal_','crate::connect_nsh::chaos_connect_terminal_')
    src=src.replace('b"chaos.web.nsh.worker"','b"chaos.ccn.nsh.worker"')
    p=Path(tmp)/'harness.rs';p.write_text(src);exe=Path(tmp)/'tests'
    subprocess.run(['rustc','--edition=2021','--test',str(p),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
