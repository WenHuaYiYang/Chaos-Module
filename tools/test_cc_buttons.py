#!/usr/bin/env python3
"""Production button-pixel and native lifecycle regressions, with host ABI seams."""
from pathlib import Path
import os
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-cc-buttons-') as folder:
    for name in ('cc_button_pixels_test','cc_buttons_harness'):
        binary=Path(folder)/name
        subprocess.run([os.environ.get('CC','cc'),'-std=c11','-Wall','-Wextra','-Werror',
                        '-fsanitize=address,undefined','-O1',str(ROOT/f'supervisor/tests/{name}.c'),
                        '-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)
    binary=Path(folder)/'config'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/cc_buttons_config_harness.rs'),'-o',str(binary)],check=True)
    for scenario in ('default','off','invalid','retry'):
        subprocess.run([str(binary),scenario],check=True)
print('PASS control-center button pixels and lifecycle (not device/GPU acceptance)')
