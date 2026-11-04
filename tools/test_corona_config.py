#!/usr/bin/env python3
"""Run configuration tests against the complete production Corona Rust module.

Firmware file I/O and the native C ABI are mocked. Each test runs in a fresh
process to preserve real lazy-load behavior without modifying production
statics. Build products exist only in a temporary directory. This is not
device filesystem, graphics, or CPU acceptance testing.
"""

from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = (
    "missing", "empty", "invalid", "short-read", "read-error",
    "read-open-error", "read-once", "independent-load", "combinations",
    "independent-save", "restart", "unchanged", "native-abi",
    "save-open-error", "save-write-error", "save-short-write",
    "save-invalid-count", "save-disable-retry", "save-latest-value",
)


def main():
    rustc = shutil.which("rustc")
    if rustc is None:
        raise SystemExit("rustc is required for the production Corona config harness")
    source = ROOT / "supervisor/tests/corona_config_harness.rs"
    with tempfile.TemporaryDirectory(prefix="chaos-corona-config-") as temporary:
        executable = Path(temporary) / "corona_config"
        subprocess.run(
            [rustc, "--edition=2021", str(source), "-o", str(executable)],
            cwd=ROOT, check=True, timeout=60,
        )
        for case in CASES:
            subprocess.run([str(executable), case], cwd=ROOT, check=True, timeout=15)
    print(f"Corona configuration: {len(CASES)} scenarios passed")


if __name__ == "__main__":
    main()
