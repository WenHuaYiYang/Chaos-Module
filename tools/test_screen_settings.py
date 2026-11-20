#!/usr/bin/env python3
"""Run the production screen preferences against RAM/VFS/UI-owner seams."""
from pathlib import Path
import subprocess
import tempfile
ROOT = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="chaos-screen-settings-") as temp:
    executable = Path(temp)/"screen-settings"
    subprocess.run(["rustc","--edition=2021","--test",str(ROOT/"supervisor/tests/screen_settings_harness.rs"),"-o",str(executable)],check=True)
    subprocess.run([str(executable),"--test-threads=1"],check=True)
# Validate restoration stays in the deferred UI-owner startup, outside page rendering.
source=(ROOT/"supervisor/src/watchface.rs").read_text()
startup=source[source.index("unsafe fn tick_app()"):source.index("unsafe fn tick_pages(")]
assert startup.index("BOOT_DELAY") < startup.index("if !lite")
for helper in ("screen_rotation","dimmer_timeout","touch_palm"):
    assert startup.index(f"crate::{helper}::ensure_applied();") < startup.index("crate::appmgr::boot_apply();")
assert "screen-rotation.cfg" not in (ROOT/"supervisor/src/screen_rotation.rs").read_text()
print("PASS screen settings: session-only rotation, saved timeout/palm, injection restore, bounded I/O and atomic failure recovery")
