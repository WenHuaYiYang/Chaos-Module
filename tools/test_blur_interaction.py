#!/usr/bin/env python3
"""Execute production blur/CC frame paths with interaction gating and native seams."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-blur-interaction-') as tmp:
    temp=Path(tmp)
    shutil.copytree(ROOT/'supervisor/corona_vendor/src',temp/'src')
    shutil.copytree(ROOT/'supervisor/corona_vendor/include',temp/'include')
    tests=temp/'tests';tests.mkdir()
    for name in ('p67_blur_harness.c','p67_blur_interaction_harness.c'):
        shutil.copy2(ROOT/'supervisor/tests/corona'/name,tests/name)
    sources=(tests/'p67_blur_interaction_harness.c',ROOT/'supervisor/tests/cc_buttons_interaction_harness.c')
    for i,source in enumerate(sources):
        binary=temp/f'interaction-{i}.exe'
        subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O1','-fsanitize=address,undefined',
                        '-I',str(ROOT/'supervisor'),'-I',str(temp/'include'),str(source),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)
