#!/usr/bin/env python3
"""Compile the complete production notification-icon owner with host firmware seams.

The source is included verbatim except for inner-doc-comment spelling required by
Rust include!. The real clear_black_palette body is also tested on synthetic
BGRA data and, when available, the unpacked p67/3.101.043 no_message.bin resource.
This does not establish on-device GPU/cache, pointer-map or visual acceptance.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def production_function(source: str, name: str) -> str:
    match = re.search(r"(?:pub\(crate\) )?unsafe fn " + re.escape(name)
                      + r"\([^)]*\)[^{]*\{", source)
    if match is None:
        raise AssertionError(f"production {name} function is missing")
    depth = 1
    for index in range(match.end(), len(source)):
        depth += (source[index] == "{") - (source[index] == "}")
        if not depth:
            return source[match.start():index + 1] + "\n"
    raise AssertionError(f"production {name} function is unclosed")


def default_resource() -> Path | None:
    firmware = ROOT / "research/firmware-3.101.043"
    candidates = sorted(path for path in firmware.rglob("no_message.bin")
                        if path.parent.name == "notifications") if firmware.is_dir() else []
    if len(candidates) > 1:
        raise AssertionError("multiple no_message.bin fixtures found; choose one with --resource")
    return candidates[0] if candidates else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resource", type=Path, help="unpacked p67/3.101.043 no_message.bin")
    arguments = parser.parse_args()
    resource = arguments.resource or default_resource()
    if resource is not None:
        resource = resource.resolve(strict=True)
        digest = hashlib.sha256(resource.read_bytes()).hexdigest()
        print(f"Firmware resource: {resource}\nSHA-256: {digest}", flush=True)
    else:
        print("NOT VERIFIED: original no_message.bin resource (not available locally)", flush=True)

    source = (ROOT / "supervisor/src/notification_icon.rs").read_text()
    included = "".join("//" + line[3:] if line.startswith("//!") else line
                       for line in source.splitlines(keepends=True))
    assets = (ROOT / "supervisor/src/background_assets.rs").read_text()
    path_constant = re.search(r"^pub\(crate\) const EMPTY_PATH[^\n]+$", assets, re.MULTILINE)
    if path_constant is None:
        raise AssertionError("production EMPTY_PATH constant is missing")
    with tempfile.TemporaryDirectory(prefix="chaos-notification-icon-") as directory:
        temporary = Path(directory)
        owner_path = temporary / "production_notification_icon.rs"
        palette_path = temporary / "production_palette.rs"
        asset_path = temporary / "production_empty_image.rs"
        owner_path.write_text(included)
        palette_path.write_text(production_function(assets, "clear_black_palette"))
        asset_path.write_text(path_constant.group() + "\n" + production_function(assets, "empty_image"))
        environment = dict(os.environ, CHAOS_NOTIFICATION_ICON_SOURCE=str(owner_path),
                           CHAOS_NOTIFICATION_PALETTE_SOURCE=str(palette_path),
                           CHAOS_NOTIFICATION_ASSET_SOURCE=str(asset_path))
        environment.pop("CHAOS_NOTIFICATION_RESOURCE", None)
        if resource is not None:
            environment["CHAOS_NOTIFICATION_RESOURCE"] = str(resource)
        binary = temporary / "notification_icon.test"
        subprocess.run([os.environ.get("RUSTC", "rustc"), "--edition=2021", "--test",
                        str(ROOT / "supervisor/tests/notification_icon_harness.rs"),
                        "-o", str(binary)], env=environment, check=True)
        subprocess.run([str(binary), "--test-threads=1", "--nocapture"], check=True)
    print("PASS: production notification-icon ownership and palette host regressions")
    print("NOT VERIFIED: device GPU/cache, firmware pointer-map acceptance, visual/touch behavior")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
