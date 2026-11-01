#!/usr/bin/env python3
"""Static p67/3.101.043 keyboard ABI checks; does NOT execute firmware.

Check the actual image, boot copy tables, stock callers and Rust entrypoints,
not the host UI mocks. Only Python's standard library is required.
"""

import argparse
import hashlib
from pathlib import Path
import re
import struct
import sys

ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / "research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin"
SHA256 = "519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec"
FLASH_EXEC = 0x0C0C0000
FLASH_DATA = 0x2C0C0000


def require(condition, message):
    if not condition:
        raise ValueError(message)


def u32(data, offset):
    return struct.unpack_from("<I", data, offset)[0]


def signature(data, offset, expected):
    expected = bytes.fromhex(expected)
    actual = data[offset:offset + len(expected)]
    require(actual == expected, f"signature mismatch at file 0x{offset:08X}")


def bl_target(data, offset):
    """Decode an observed Thumb-2 BL, returning a file-relative target."""
    hi, lo = struct.unpack_from("<HH", data, offset)
    require(hi & 0xF800 == 0xF000 and lo & 0xD000 == 0xD000,
            f"not a Thumb BL at 0x{offset:X}")
    s = (hi >> 10) & 1
    i1 = 1 ^ (((lo >> 13) & 1) ^ s)
    i2 = 1 ^ (((lo >> 11) & 1) ^ s)
    imm = (s << 24) | (i1 << 23) | (i2 << 22) | ((hi & 0x3FF) << 12) | ((lo & 0x7FF) << 1)
    if s:
        imm -= 1 << 25
    return offset + 4 + imm


def thunk_target(data, offset):
    signature(data, offset, "5ff800f0")
    target = u32(data, offset + 4)
    require(target & 1, f"missing Thumb bit at thunk 0x{offset:X}")
    return target


def copied_entry(data, offset, table, alias_delta):
    """Use this firmware's observed boot-copy triple and instruction alias."""
    source, destination, end = (u32(data, table + n) for n in (0, 4, 8))
    start = source - FLASH_DATA
    require(start <= offset < end - FLASH_DATA, f"0x{offset:X} is not in copy segment")
    return (destination + offset - start - alias_delta) | 1


def verify_image(data):
    require(hashlib.sha256(data).hexdigest() == SHA256,
            "wrong image: only the checked p67/3.101.043 vela_ap.bin is supported")
    # Startup 0xB6E -> 0xA60 copies SRAM; 0xB7A -> 0xAC8 copies PSRAM.
    require(bl_target(data, 0xB6E) == 0xA60, "SRAM copy caller changed")
    require([u32(data, p) for p in (0xAA8, 0xAAC, 0xAB0)] ==
            [0x2C0FC6CC, 0x200B2860, 0x2C10D438], "SRAM copy triple changed")
    require([u32(data, p) for p in (0xB20, 0xB24, 0xB28)] ==
            [0x2C10D440, 0x3C000000, 0x2C18D940], "PSRAM copy triple changed")
    signature(data, 0x47790, "2de9f04385b09846438e02af13f47c7f0e4605467a60c3f3051100f090804ff0")
    signature(data, 0x7A028, "b0b505460c4600affcf706f9aa68538ec3f38311a1421cbf64f389135386b0bd")
    local = copied_entry(data, 0x47790, 0xAA8, 0x1FE00000)
    scroll = copied_entry(data, 0x7A028, 0xB20, 0x20000000)
    require(local == thunk_target(data, 0xA00BB0) == 0x002BD925, "local style mapping changed")
    require(scroll == thunk_target(data, 0x4C84C8) == 0x1C02CBE9, "scroll mapping changed")
    # Stock generic call: r1=prop, r2=value, r0=obj/r3=selector from descriptor.
    signature(data, 0x9E65F8, "0a460146dce90003")
    require(bl_target(data, 0x9E6600) == 0xA00BB0, "generic setter caller changed")
    # Stock scrolling call: r0=obj, r1=0xC (vertical); keyboard uses mask 3.
    signature(data, 0x421E42, "0c212068")
    require(bl_target(data, 0x421E46) == 0x4C84C8, "scroll setter caller changed")
    # Stock pad/border wrappers forward (obj, prop, value, selector) to SAME body.
    for body, thunk, prop, caller in [
        (0x48258, 0x4C80A0, 0x10, 0x4084BE),
        (0x48270, 0x4C93C0, 0x11, 0x4084C8),
        (0x48288, 0x4C89C0, 0x12, 0x4084AA),
        (0x482A0, 0x4C7C60, 0x13, 0x4084B4),
        (0x484A0, 0x4C8770, 0x30, 0x4084EC),
    ]:
        signature(data, body, f"8c4680b582b0134600af{prop:02x}216246")
        require(bl_target(data, body + 14) == 0x47790, "stock style wrapper target changed")
        signature(data, body + 18, "0837bd4680bd")
        require(thunk_target(data, thunk) == copied_entry(data, body, 0xAA8, 0x1FE00000),
                f"style wrapper mapping changed: 0x{thunk:X}")
        require(bl_target(data, caller) == thunk, f"style wrapper caller changed: 0x{caller:X}")
    return local, scroll


def verify_sources(addresses, gfx):
    for name, function, thunk in [
        ("OBJ_SET_LOCAL_STYLE_PROP", "obj_set_local_style_prop", 0xA00BB0),
        ("OBJ_SET_SCROLL_DIR", "obj_set_scroll_dir", 0x4C84C8),
    ]:
        definitions = re.findall(rf"pub const {name}: u32 = (0x[0-9A-Fa-f_]+);", addresses)
        require(len(definitions) == 1, f"expected one verified target for {name}")
        require(int(definitions[0].replace("_", ""), 16) == (FLASH_EXEC + thunk) | 1,
                f"{name} does not use the stock flash thunk")
        body = re.search(rf'pub unsafe extern "C" fn {function}\([^}}]+?\n}}', gfx)
        require(body is not None, f"missing wrapper {function}")
        require(f"transmute(super::fwaddr::{name} as usize)" in body[0],
                f"{function} bypasses verified address table")
        require("->" not in body[0], f"{function} must have void return type")
    require(not re.search(r"transmute\(0x0[Cc]10_?7791", gfx), "load-image setter call reintroduced")
    require("for prop in [0x10, 0x11, 0x12, 0x13, 0x30]" in gfx,
            "box-spacing properties need a new firmware audit")


def verify_alignment(data, source):
    # obj_align -> set alignment property 0xA; then the layout engine dispatches it.
    require(thunk_target(data, 0x4C9188) == copied_entry(data, 0x77940, 0xB20, 0x20000000),
            "obj_align mapping changed")
    signature(data, 0x77940, "f0b5044683b0164600af00221d464ff0f3ff")
    require(bl_target(data, 0x7794E) == 0xC7938, "alignment property setter changed")
    require(thunk_target(data, 0xC7938) == copied_entry(data, 0x48168, 0xAA8, 0x1FE00000),
            "alignment property setter mapping changed")
    signature(data, 0x48168, "8c4680b582b0134600af0a216246fff70bfb0837bd4680bd")
    require(bl_target(data, 0x48176) == 0x47790, "alignment setter no longer writes local style")
    # Read align (0xA), subtract 2, dispatch through TBB. Index 6 is RIGHT_MID, 7 CENTER.
    signature(data, 0x79346, "0a224ef0e6fd10f0ff01c34478611ad0023907291fd8dfe801f0923b9cb5c3cc83a1")
    require(0x79360 + 2 * data[0x79360 + 8 - 2] == 0x79466, "right-middle branch changed")
    require(0x79360 + 2 * data[0x79360 + 9 - 2] == 0x794A2, "center branch changed")
    # 8: full width difference + half height difference; 9: half differences on BOTH axes.
    signature(data, 0x79466, "fb68ba68a9eb03097b6802ebd27203ebd3735b10c3eb6203cb449a448be7")
    signature(data, 0x794A2, "fb687a68b96803ebd37302ebd27209ebd9795b1001ebd1715210c3eb6903c2eb61029b44924468e7")
    values = re.findall(r"pub const CENTER: u32 = (\d+);", source)
    require(values == ["9"], "CENTER must be 9; 8 is right-middle on p67/3.101.043")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", nargs="?", type=Path, default=IMAGE)
    args = parser.parse_args()
    data = args.image.read_bytes()
    local, scroll = verify_image(data)
    verify_sources((ROOT / "supervisor/src/fw_api/fwaddr.rs").read_text(),
                   (ROOT / "supervisor/src/fw_api/gfx.rs").read_text())
    verify_alignment(data, (ROOT / "supervisor/src/fw_api/align.rs").read_text())
    print(f"PASS p67/3.101.043 image SHA-256: {SHA256}")
    print(f"PASS local style: stock thunk -> 0x{local:08X}; SRAM copy + 5 stock wrappers + caller")
    print(f"PASS scroll direction: stock thunk -> 0x{scroll:08X}; PSRAM copy + caller")
    print("PASS Rust entrypoints, void signatures and box-spacing properties")
    print("PASS center alignment = 9 (8 = right-middle): actual constant + firmware layout branches")
    print("STATIC ONLY: no firmware execution, touchscreen simulation or on-device crash verification.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, struct.error) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        sys.exit(1)
