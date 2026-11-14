#!/usr/bin/env python3
"""Test Chaos's vendored Corona p67 implementation (not device/GPU acceptance).

All host harnesses are compiled against corona_vendor in a temporary staging
tree. --upstream additionally compares every vendored implementation and test
byte-for-byte with the selected Corona checkout. No upstream checkout is
required for the normal test run.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "supervisor/corona_vendor"
TESTS = ROOT / "supervisor/tests/corona"
VERBATIM = (
    "LICENSE",
    "src/p67_backgrounds.inc", "src/p67_launcher.inc",
    "src/p67_blur.inc", "src/p67_blur_native.inc",
    "src/p67_battery.inc", "src/p67_battery_diagnostics.inc",
    "include/resource_hook.h", "include/resource_hook_background.h",
    "include/resource_hook_blur.h", "include/resource_hook_p67_blur.h",
    "include/resource_hook_p67_battery.h", "include/resource_hook_target.h",
)
HARNESSES = (
    "p67_launcher_harness.c", "p67_backgrounds_harness.c",
    "p67_blur_harness.c", "p67_blur_provider_harness.c",
    "p67_battery_harness.c", "test_p67_battery_pixels.c",
)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def verify_sources(upstream: Path | None) -> None:
    entries = [(VENDOR / relative, relative) for relative in VERBATIM]
    entries += [(TESTS / name, "tests/" + name) for name in HARNESSES]
    for actual, relative in entries:
        require(actual.is_file(), f"missing vendored dependency: {actual}")
        if upstream:
            expected = upstream / relative
            require(expected.is_file(), f"missing upstream file: {expected}")
            require(relative == "tests/p67_launcher_harness.c" or actual.read_bytes() == expected.read_bytes(),
                    f"vendored file differs from Corona: {relative}")
    pinned = {}
    for line in (TESTS / "upstream.sha256").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        digest, relative = line.split(None, 1)
        pinned[relative] = digest
    require(set(pinned) == {str(path.relative_to(ROOT)) for path, _ in entries},
            "upstream checksum manifest must cover exactly the vendored dependencies and harnesses")
    for relative, digest in pinned.items():
        require(hashlib.sha256((ROOT / relative).read_bytes()).hexdigest() == digest,
                f"pinned upstream checksum mismatch: {relative}")
    print(f"verbatim provenance: {len(entries)} files OK", flush=True)


def rust_function(source: str, name: str) -> str:
    match = re.search(r"\bfn " + re.escape(name) + r"\([^)]*\)[^{]*\{", source)
    require(match is not None, f"missing Rust function: {name}")
    depth = 1
    for index in range(match.end(), len(source)):
        depth += (source[index] == "{") - (source[index] == "}")
        if not depth:
            return source[match.end():index]
    raise AssertionError(f"unclosed Rust function: {name}")


def check_integration() -> None:
    shim = (ROOT / "supervisor/corona_blur.c").read_text()
    require('#include "corona_vendor/src/p67_backgrounds.inc"' in shim,
            "C adapter must include Corona's complete four-surface implementation")
    for symbol in ("chaos_corona_backgrounds_commit",
                   "chaos_corona_backgrounds_poll",
                   "chaos_corona_backgrounds_pending"):
        require(symbol in shim, f"missing C ABI: {symbol}")
    require("chaos_corona_redraw" not in shim,
            "old nullable redraw callback must not replace Corona's redraw seam")
    home = (ROOT / "supervisor/src/background_home.rs").read_text()
    corona = rust_function(home, "corona_tick")
    require("backgrounds_poll()" in corona, "Corona owner must poll all four native hooks")
    for obsolete in ("attach_borrowed", "SCENE.attach(", "masked_targets(",
                     "background_surfaces::enable", "background_surfaces::set_background",
                     "background_home_refresh_dirty_snapshots"):
        require(obsolete not in corona, f"Corona still uses legacy image pipeline: {obsolete}")
    require("attach_borrowed" not in home, "borrowed Corona image path must be removed")
    owner = rust_function(home, "tick")
    require(owner.index("corona_tick()") < owner.index("background_surfaces::tick("),
            "Corona must intercept the tick before legacy surface mutations")
    bridge = (ROOT / "supervisor/src/corona_blur.rs").read_text()
    require("chaos_corona_backgrounds_commit" in bridge,
            "Rust bridge must commit Corona's complete paths")
    print("Chaos four-surface ownership wiring: OK", flush=True)


def check_c_abi_contract() -> None:
    """Check the thin C seam cannot silently configure only one surface."""
    shim = (ROOT / "supervisor/corona_blur.c").read_text()
    required = (
        "int chaos_corona_backgrounds_commit(uint32_t enabled, uint32_t region_mask)",
        "struct rh_backgrounds backgrounds",
        "backgrounds.launcher",
        "backgrounds.control_center",
        "backgrounds.notifications",
        "backgrounds.widgets",
        "region_mask &= 0x0fu;",
        "if (region_mask & 1u)",
        "if (region_mask & 2u)",
        "if (region_mask & 4u)",
        "if (region_mask & 8u)",
        # 忙碌闸门（重入 + native 未空闲）。两段之间插入了滚动标记自愈，
        # 语义不变：任一条件成立都拒绝本次 commit。
        "if (chaos_corona_in_call) return -2145;",
        "chaos_corona_latch_rescue();",
        "if (!p67_blur_native_idle()) return -2145;",
        "result = rh_platform_backgrounds_commit(&backgrounds);",
        "else chaos_corona_enabled = !!enabled;",
        "if (chaos_corona_in_call) return;",
        "chaos_corona_backgrounds_pending",
        "rh_platform_request_full_redraw",
    )
    for marker in required:
        require(marker in shim, f"C ABI contract missing: {marker}")
    require("AGPL-3.0-only" in shim, "C provenance must state AGPL-3.0-only")
    require("chaos_corona_wake" not in shim, "nullable redraw shim regressed")
    print("Corona C ABI: four-path commit/system rollback/busy guard/redraw seam OK", flush=True)


def check_owner_state_contract() -> None:
    """Static guards complement the compiled production owner tests below."""
    home_source = (ROOT / "supervisor/src/background_home.rs").read_text()
    corona = rust_function(home_source, "corona_tick")
    required = (
        "if st_rd!(CORONA_BG_ACTIVE) == 0",
        "crate::corona_blur::backgrounds_idle()",
        "let region_mask = crate::blur_zone::mask();",
        "crate::corona_blur::backgrounds_commit(true, region_mask)",
        "st_rd!(CORONA_BG_MASK) != region_mask",
        "st_wr!(CORONA_BG_MASK, region_mask)",
        "st_wr!(CORONA_BG_ACTIVE, 1)",
        "crate::corona_blur::backgrounds_commit(false, 0)",
        "st_wr!(CORONA_BG_ACTIVE, 2)",
        "crate::corona_blur::backgrounds_pending()",
        "st_wr!(CORONA_BG_ACTIVE, 0)",
        "st_wr!(CORONA_BG_MASK, 0)",
        "st_wr!(REQUEST, 1)",
    )
    for marker in required:
        require(marker in corona, f"Corona owner transition guard missing: {marker}")
    require("if st_rd!(CORONA_BG_ACTIVE) != 0 { crate::corona_blur::backgrounds_poll(); }" in corona,
            "mask/off retries must poll the applied native configuration")
    remount = rust_function(home_source, "remount")
    require("st_wr!(REQUEST, 1)" in remount and "st_wr!(DIRTY, 1)" in remount,
            "Corona toggles must wake even when legacy MODE is 0")
    owner_tick = rust_function(home_source, "tick")
    require(owner_tick.index("corona_tick()") < owner_tick.index("background_surfaces::tick("),
            "legacy surface worker must not run before Corona arbitration")
    zones = (ROOT / "supervisor/src/blur_zone.rs").read_text()
    mask_body = rust_function(zones, "mask")
    require(mask_body.index("ensure_loaded();") < mask_body.index("st_rd!(MASK)"),
            "mask() must load persisted regions before returning the shared mask")
    require("crate::blur_zone::ensure_loaded();" in rust_function(home_source, "load_mode"),
            "legacy startup must also load the shared saved regions")

    print("Corona Rust owner transition guards: OK", flush=True)


def run_harnesses() -> None:
    cc = os.environ.get("CC", "cc")
    with tempfile.TemporaryDirectory(prefix="chaos-corona-host-") as directory:
        temp = Path(directory)
        shutil.copytree(VENDOR / "src", temp / "src")
        shutil.copy2(ROOT / "supervisor/corona_launcher_rebind.inc", temp / "src/corona_launcher_rebind.inc")
        shutil.copytree(VENDOR / "include", temp / "include")
        (temp / "tests").mkdir()
        flags = [cc, "-std=c11", "-Wall", "-Wextra", "-Werror",
                 "-fsanitize=address,undefined", "-I", str(temp / "include")]
        for name in HARNESSES:
            source = temp / "tests" / name
            shutil.copy2(TESTS / name, source)
            binary = temp / (Path(name).stem + ".test")
            subprocess.run(flags + [str(source), "-o", str(binary)], check=True)
            arguments = ((), ("--mask",), ("--charge",)) if name == "p67_battery_harness.c" else ((),)
            for extra in arguments:
                subprocess.run([str(binary), *extra], check=True)
        # The blur lifecycle harness also supports native diagnostics injection.
        binary = temp / "blur_diagnostics.test"
        subprocess.run(flags + ["-DHARNESS_NATIVE_DIAGNOSTICS=1",
                       str(temp / "tests/p67_blur_harness.c"), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)
    print("Corona host harnesses: 9 runs passed (ASan + UBSan)", flush=True)


def run_owner_harness() -> None:
    """Compile production Rust owner functions with host-only ABI stubs."""
    source = (ROOT / "supervisor/src/background_home.rs").read_text()
    signatures = {
        "corona_tick": "() -> bool",
        "pending": "() -> bool",
        "remount": "()",
        "corona_failure": "(step: u32) -> bool",
    }
    generated = "\n".join(
        "unsafe fn " + name + signatures[name] + " {" + rust_function(source, name) + "}\n"
        for name in signatures
    )
    with tempfile.TemporaryDirectory(prefix="chaos-corona-owner-") as directory:
        temp = Path(directory)
        included = temp / "production_owner.rs"
        generated += "\nunsafe fn masked_targets(full: [u32; 4]) -> [u32; 4] {" + rust_function(source, "masked_targets") + "}\n"
        included.write_text(generated)
        region_source = (ROOT / "supervisor/src/blur_zone.rs").read_text()
        region = temp / "production_region.rs"
        region.write_text("pub(crate) unsafe fn ensure_loaded() {"
                          + rust_function(region_source, "ensure_loaded") + "}\n"
                          + "pub(crate) unsafe fn mask() -> u32 {"
                          + rust_function(region_source, "mask") + "}\n")
        binary = temp / "owner.test"
        environment = dict(os.environ, CHAOS_CORONA_OWNER_SOURCE=str(included),
                           CHAOS_CORONA_REGION_SOURCE=str(region))
        subprocess.run([os.environ.get("RUSTC", "rustc"), "--edition=2021",
                        "--test", str(ROOT / "supervisor/tests/corona_owner_harness.rs"),
                        "-o", str(binary)], env=environment, check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


def run_restore_harness() -> None:
    source = (ROOT / "supervisor/src/background_surfaces.rs").read_text()
    signatures = {
        "consume": "unsafe fn consume()",
        "owns_root": "unsafe fn owns_root(root: u32, roots: Option<[u32; 4]>) -> bool",
        "restore": "pub(crate) unsafe fn restore(roots: Option<[u32; 4]>) -> bool",
        "pending": "pub(crate) unsafe fn pending() -> bool",
        "clear_background": "pub(crate) unsafe fn clear_background()",
    }
    generated = "\n".join(
        signatures[name] + " {" + rust_function(source, name) + "}\n"
        for name in signatures
    )
    with tempfile.TemporaryDirectory(prefix="chaos-corona-restore-") as directory:
        temp = Path(directory)
        included = temp / "production_restore.rs"
        included.write_text(generated)
        binary = temp / "restore.test"
        environment = dict(os.environ, CHAOS_CORONA_RESTORE_SOURCE=str(included))
        subprocess.run([os.environ.get("RUSTC", "rustc"), "--edition=2021",
                        "--test", str(ROOT / "supervisor/tests/corona_restore_harness.rs"),
                        "-o", str(binary)], env=environment, check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


def c_function(source: str, name: str) -> str:
    match = re.search(r"\b(?:int|void) " + re.escape(name) + r"\([^)]*\)\s*\{", source)
    require(match is not None, f"missing C function: {name}")
    depth = 1
    for index in range(match.end(), len(source)):
        depth += (source[index] == "{") - (source[index] == "}")
        if not depth:
            return source[match.start():index + 1]
    raise AssertionError(f"unclosed C function: {name}")


def run_abi_harness() -> None:
    source = (ROOT / "supervisor/corona_blur.c").read_text()
    generated = "\n".join(c_function(source, name) for name in
                          ("chaos_corona_backgrounds_commit", "chaos_corona_backgrounds_poll", "chaos_corona_launcher_position"))
    with tempfile.TemporaryDirectory(prefix="chaos-corona-abi-") as directory:
        temp = Path(directory)
        included = temp / "production_abi.c"
        included.write_text(generated)
        binary = temp / "abi.test"
        subprocess.run([os.environ.get("CC", "cc"), "-std=c11", "-Wall", "-Wextra", "-Werror",
                        "-fsanitize=address,undefined", "-I", str(VENDOR / "include"),
                        '-DCHAOS_CORONA_ABI_SOURCE="' + str(included) + '"',
                        str(ROOT / "supervisor/tests/corona_abi_harness.c"), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, help="also compare with this Corona checkout")
    parser.add_argument("--source-only", action="store_true", help="skip compiled host harnesses")
    args = parser.parse_args()
    verify_sources(args.upstream)
    check_integration()
    check_c_abi_contract()
    check_owner_state_contract()
    if not args.source_only:
        run_harnesses()
        run_owner_harness()
        run_restore_harness()
        run_abi_harness()
        subprocess.run([sys.executable, str(ROOT / "tools/test_notification_icon.py")], check=True)
        subprocess.run([sys.executable, str(ROOT / "tools/test_corona_config.py")], check=True)
    print("PASS: vendored Corona p67 complete-background regression checks")
    print("NOT VERIFIED: device GPU/cache acceptance, touch/scroll feel, frame time")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, OSError, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
