#!/usr/bin/env python3
"""第一阶段V2移植：原始源码摘要、实际滤波器与生产Rust选择器回归。"""
from pathlib import Path
import hashlib
import json
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    source = ROOT / 'supervisor/syspatch_blur'
    manifest = json.loads((source / 'SOURCE.json').read_text())
    for name, digest in manifest['files'].items():
        assert hashlib.sha256((source / name).read_bytes()).hexdigest() == digest, name
    with tempfile.TemporaryDirectory(prefix='chaos-blur-v2-') as folder:
        test = Path(folder) / 'filter'
        subprocess.run([os.environ.get('CC', 'cc'), '-std=c11', '-O2',
                        str(ROOT / 'supervisor/tests/syspatch_filter_test.c'), '-o', str(test)], check=True)
        subprocess.run([str(test)], check=True)
        selection = Path(folder) / 'selection'
        subprocess.run(['rustc', '--edition=2021', '--test',
                        str(ROOT / 'supervisor/tests/blur_selection_harness.rs'), '-o', str(selection)], check=True)
        subprocess.run([str(selection)], check=True)
    print('PASS SysPatch source hashes, exact filters, selection/persistence and busy retry')

if __name__ == '__main__':
    main()
