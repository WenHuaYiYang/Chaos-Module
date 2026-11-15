#!/usr/bin/env python3
"""Run the production bounded directory walker with an in-memory host VFS."""
import os
from pathlib import Path
import subprocess
import tempfile
from test_corona_blur import rust_function

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / 'supervisor/src/appmgr/detail.rs').read_text(encoding='utf-8')
constants = source[source.index('const APP_DIRS:'):source.index('pub(crate) unsafe fn mk_path3')]
scanner = source[source.index('static mut D_SIZE:'):source.index('static mut D_VERBUF:')]
helpers = '\n'.join('unsafe fn '+name+signature+' {'+rust_function(source, name)+'}' for name,signature in (
    ('mk_path3','(root: &[u8], mid: &[u8], pkg: *const u8, suffix: &[u8]) -> usize'),
    ('file_bytes','(path: *const u8) -> u32')))
with tempfile.TemporaryDirectory(prefix='chaos-detail-scan-') as tmp:
    generated = Path(tmp)/'scanner.rs'
    generated.write_text('use core::ptr::{read_volatile,write_volatile};\n'+constants+scanner+helpers,encoding='utf-8')
    env = os.environ.copy()
    env['CHAOS_SCAN_SOURCE'] = str(generated)
    exe = Path(tmp)/'scanner.exe'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/detail_scan_harness.rs'),'-o',str(exe)],env=env,check=True)
    subprocess.run([str(exe)],check=True)
