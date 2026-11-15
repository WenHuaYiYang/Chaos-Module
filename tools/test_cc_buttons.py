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
    # Pixel harness proves the opaque icon-area texture channels never exceed
    # 68. Independently bound white contrast after blending with the two
    # verified native fills. Model only: actual panel/cache remains unverified.
    def linear(v):
        v=v/255
        return v/12.92 if v<=0.04045 else ((v+0.055)/1.055)**2.4
    for fill in ((34,34,34),(39,122,247)):
        rgb=[(68*160+c*95+127)//255 for c in fill]
        luminance=sum(linear(c)*w for c,w in zip(rgb,(0.2126,0.7152,0.0722)))
        ratio=1.05/(luminance+0.05)
        assert ratio>=7, (fill,rgb,ratio)
        print(f'Modeled white-icon contrast on native {fill}: {ratio:.2f}:1')
print('PASS control-center button pixels and lifecycle (not device/GPU acceptance)')
