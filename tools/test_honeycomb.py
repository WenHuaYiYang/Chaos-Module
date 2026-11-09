#!/usr/bin/env python3
"""Production geometry/controller tests; host firmware mocks, no device claims."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = ("defaults", "config", "invalid", "short", "save-retry", "toggle-restore",
         "tap", "drag", "rebuild", "remove", "destroy", "unsupported", "idle", "offscreen", "capacity",
         "grow-over-capacity", "remove-all", "exclusive-on", "exclusive-rw", "exclusive-lens", "exclusive-inset",
         "exclusive-boot", "layout-callback", "rw-save-retry", "geometry-failed",
         "layout-fault-deferred", "layout-repeat", "gesture-deferred", "zoom-edge", "exit-left", "exit-top",
         "inside-no-exit", "entry-sync", "frame-timer", "long-press", "drag-no-editor", "zoom-no-editor", "blank-no-editor",
         "pre-entry", "pre-end", "pre-unrelated", "pre-disabled")

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
