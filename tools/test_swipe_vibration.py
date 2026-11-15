#!/usr/bin/env python3
"""Run production launcher vibration code independently of either blur owner."""
from pathlib import Path
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
source=(ROOT/'supervisor/src/background_home.rs').read_text()
tick=source[source.index('pub(crate) unsafe fn tick()'):]
assert tick.index('sv_watch_tick();') < tick.index('if corona_tick()')
assert 'swipe_scroll_evt(' not in source[source.index('unsafe extern "C" fn structure_changed'):source.index('unsafe fn watch_structure')]
production=source[source.index('static mut SV_LOADED'):source.index('unsafe fn persist_mode')]
with tempfile.TemporaryDirectory(prefix='chaos-swipe-vib-') as folder:
    included=Path(folder)/'production.rs';included.write_text(production)
    binary=Path(folder)/'vibration'
    subprocess.run(['rustc','--edition=2021',str(ROOT/'supervisor/tests/swipe_vibration_harness.rs'),
                    '--cfg','host_test','-o',str(binary)],env={**__import__('os').environ,'CHAOS_SWIPE_SOURCE':str(included)},check=True)
    subprocess.run([str(binary)],check=True)
print('PASS vibration observer is maintained before V2 handoff, with deferred input and cooldown')
