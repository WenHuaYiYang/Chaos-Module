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
        native=(ROOT/'supervisor/corona_vendor/src/p67_blur_native.inc').read_text()
        buttons=(ROOT/'supervisor/control_center_buttons.inc').read_text()
        extracted={
            'STRUCT':native[native.index('struct p67_blur_native_output {'):native.index('static struct p67_blur_native_output')],
            'PUBLISH':native[native.index('static uintptr_t p67_blur_native_publish('):],
            'SOURCE':buttons[buttons.index('static int cc_source('):buttons.index('static void cc_submit(')],
        }
        flags=[]
        for name,code in extracted.items():
            part=Path(folder)/(name+'.inc');part.write_text(code)
            macro='CHAOS_BUTTON_SOURCE' if name=='SOURCE' else 'CHAOS_OUTPUT_'+name
            flags.append(f'-D{macro}="{part}"')
        publication=Path(folder)/'publication'
        subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O1','-fsanitize=address,undefined',
                        *flags,str(ROOT/'supervisor/tests/blur_split_publish_harness.c'),'-o',str(publication)],check=True)
        subprocess.run([str(publication)],check=True)
        launcher=(ROOT/'supervisor/corona_vendor/src/p67_launcher.inc').read_text()
        adapter=(ROOT/'supervisor/corona_blur.c').read_text()
        parts={
            'DRAW':launcher[launcher.index('static void p67_draw_background('):launcher.index('static void p67_remove_event(')],
            'REFRESH':adapter[adapter.index('static void chaos_v2_background_refresh('):adapter.index('void chaos_corona_backgrounds_poll(')],
        }
        flags=[]
        for name,code in parts.items():
            part=Path(folder)/(name+'.inc');part.write_text(code)
            flags.append(f'-DCHAOS_BACKGROUND_{name}="{part}"')
        background=Path(folder)/'background'
        subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O1','-fsanitize=address,undefined',
                        *flags,str(ROOT/'supervisor/tests/blur_background_custom_harness.c'),'-o',str(background)],check=True)
        subprocess.run([str(background)],check=True)
        selection = Path(folder) / 'selection'
        subprocess.run(['rustc', '--edition=2021', '--test',
                        str(ROOT / 'supervisor/tests/blur_selection_harness.rs'), '-o', str(selection)], check=True)
        subprocess.run([str(selection)], check=True)
    print('PASS SysPatch source hashes, exact filters, selection/persistence and busy retry')

if __name__ == '__main__':
    main()
