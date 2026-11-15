#!/usr/bin/env python3
"""Production V2 parameter persistence and native setter regression."""
from pathlib import Path
import os
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='chaos-blur-params-') as folder:
    binary=Path(folder)/'params'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/blur_params_harness.rs'),'-o',str(binary)],check=True)
    for case in ('v2','v2_range','legacy','legacy_range','legacy_checksum','legacy_truncated','groups','default','valid','magic','checksum','range','truncated','busy','persist','writefail','closefail','openfail','bounds'):
        subprocess.run([str(binary),case],check=True)
    source=(ROOT/'supervisor/corona_blur.c').read_text()
    start=source.index('int chaos_blur_v2_set_params(')
    end=source.index('\n}',start)+2
    body=Path(folder)/'setter.c';body.write_text(source[start:end])
    binary=Path(folder)/'setter'
    subprocess.run([os.environ.get('CC','cc'),'-std=c11','-Wall','-Wextra','-Werror',
                    '-fsanitize=address,undefined',f'-DCHAOS_PARAMS_SETTER="{body}"',
                    str(ROOT/'supervisor/tests/blur_params_native_harness.c'),'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True)
print('PASS V2 parameter validation, persistence, short I/O, restart, busy retry and native invalidation')
