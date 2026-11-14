#!/usr/bin/env python3
"""Production geometry/controller tests; host firmware mocks, no device claims."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = ("s541-native", "s541-preserve-rw", "defaults", "config", "invalid", "short", "save-retry", "toggle-restore",
         "tap", "drag", "rebuild", "remove", "destroy", "unsupported", "idle", "offscreen", "capacity",
         "grow-over-capacity", "remove-all", "exclusive-on", "exclusive-rw", "exclusive-lens", "exclusive-inset",
         "exclusive-boot", "layout-callback", "rw-save-retry", "geometry-failed",
         "layout-fault-deferred", "layout-repeat", "gesture-deferred", "zoom-edge", "spacing-edge", "left-edge-tap", "exit-left", "exit-top",
         "inside-no-exit", "entry-sync", "frame-timer", "long-press", "drag-no-editor", "zoom-no-editor", "blank-no-editor",
         "left-edge-long-press", "pre-entry", "pre-end", "pre-unrelated", "pre-disabled",
         "edit-return", "edit-return-fallback", "edit-return-input", "edit-return-change",
         "edit-return-offscreen", "edit-return-disabled", "edit-return-rebuilt",
         "edit-deleted-before-return", "edit-rebuilt-before-return", "edit-disabled-before-return",
         "pre-before-tick", "pre-offscreen", "pre-await-icons", "pre-await-mode",
         "pre-late-position", "entry-before-tick", "pre-rebuild", "watch-repair",
         "snapshot-retry", "pre-unrelated-before-tick", "watch-unsupported-disable",
         "config-off", "bad-magic", "boot-await-mode", "apply-on", "apply-rebuild",
         "apply-rw", "apply-off", "apply-in-layout", "apply-await-mode", "apply-recover")

def main():
    with tempfile.TemporaryDirectory(prefix="chaos-honeycomb-") as d:
        for source, is_test in (("honeycomb_layout_cases.rs", True), ("honeycomb_harness.rs", False)):
            exe = Path(d) / source.replace(".rs", "")
            cmd = ["rustc", "--edition=2021", str(ROOT/"supervisor/tests"/source), "-o", str(exe)]
            if is_test: cmd.append("--test")
            subprocess.run(cmd, check=True, timeout=60)
            if is_test: subprocess.run([str(exe)], check=True, timeout=30)
            else:
                for case in CASES: subprocess.run([str(exe), case], check=True, timeout=15)

if __name__ == "__main__": main()
