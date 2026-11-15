#!/usr/bin/env python3
"""Production PAN connect and poll: SPP required, no duplicate bonding."""
from pathlib import Path
import os,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[1]
s=(ROOT/'supervisor/network/src/bt_probe.c').read_text()
with tempfile.TemporaryDirectory(prefix='chaos-pan-spp-') as d:
 d=Path(d)
 structs=d/'state.inc';structs.write_text(s[s.index('struct state {'):s.index('static int same(')])
 functions=d/'functions.inc';functions.write_text(s[s.index('void bt_probe_connect(void)'):s.index('void bt_probe_disconnect(void)')]+s[s.index('void bt_probe_poll(void)'):s.index('/* 用记住的对端地址直连') if '/* 用记住的对端地址直连' in s else s.index('/* 用记住的地址直连')])
 binary=d/'test'
 subprocess.run([os.environ.get('CC','cc'),'-std=c11','-fsanitize=address,undefined',
  '-I'+str(ROOT/'supervisor/network/include'),f'-DSTATE="{structs}"',f'-DFUNCTIONS="{functions}"',
  str(ROOT/'supervisor/tests/pan_spp_harness.c'),'-o',str(binary)],check=True)
 subprocess.run([str(binary)],check=True)
