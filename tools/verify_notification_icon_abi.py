#!/usr/bin/env python3
"""Read-only notification-icon ABI checks for p67 / 3.101.043 only.

Pin the original AP and no_message resource, then verify observed firmware
instructions, stock callers and the existing Rust wrappers. No device code
is executed and no files are written; Python's standard library is enough.
"""

import argparse
import hashlib
from pathlib import Path
import re
import struct
import sys

from verify_keyboard_abi import (
    IMAGE, ROOT, SHA256, bl_target, copied_entry, require, signature,
    thunk_target, u32,
)

RESOURCE_SHA256 = "5c7a938ab677293e148412b27355c2efe6465553927933d9cde23801cafa0949"
EMPTY_PATH = b"/resource/app/notifications/no_message.bin\0"
IMAGE_CLASS = 0x2CCE61EC


def copied_thunk(data, thunk, body, psram=False):
    table, alias = (0xB20, 0x20000000) if psram else (0xAA8, 0x1FE00000)
    require(thunk_target(data, thunk) == copied_entry(data, body, table, alias),
            f"wrong copy mapping: thunk 0x{thunk:X} / body 0x{body:X}")


def verify_firmware(data):
    require(hashlib.sha256(data).hexdigest() == SHA256,
            "wrong AP: only the audited p67 / 3.101.043 image is supported")
    require([u32(data, p) for p in (0xAA8, 0xAAC, 0xAB0)] ==
            [0x2C0FC6CC, 0x200B2860, 0x2C10D438], "SRAM copy table changed")
    require([u32(data, p) for p in (0xB20, 0xB24, 0xB28)] ==
            [0x2C10D440, 0x3C000000, 0x2C18D940], "PSRAM copy table changed")

    # lv_image_create loads the exact class pointer; source lives at obj+0x34.
    copied_thunk(data, 0x4C8D58, 0xB9F00, psram=True)
    signature(data, 0xB9F00, "98b503460c4600af19460448")
    require(u32(data, 0xB9F1C) == IMAGE_CLASS, "image class literal changed")
    require(u32(data, IMAGE_CLASS - 0x2C0C0000) == 0x2CCE0308,
            "image class no longer derives from the stock object class")
    copied_thunk(data, 0x4C7FA0, 0xBA2F4, psram=True)
    signature(data, 0xBA438,
              "d4f83480a8450cd094f86030284603f00303013b012b3fd9")
    # Variable descriptor assignment: free an old file/symbol source first,
    # then store the descriptor. Never retain the old owned string pointer.
    signature(data, 0xBA402,
              "94f8603003f00303013b012b50d9b7f808c079896563aee7")
    signature(data, 0xBA4B2, "606bf3f716fbaae7")
    require(bl_target(data, 0xBA4B4) == 0xADAE4, "old image-source free changed")
    # Path assignment duplicates the caller's string. OOM leaves the current
    # image source untouched; the void wrapper does NOT report success.
    signature(data, 0xBA450, "f3f712fb002882d06063")
    signature(data, 0xBA4D0,
              "f3f7d2fa00283ff442af6063b8f1000fbbd04046f3f7fefab7e7")
    require(bl_target(data, 0xBA450) == bl_target(data, 0xBA4D0) == 0xADA78,
            "image path no longer uses the audited strdup helper")
    signature(data, 0xADA7C,
              "06461af01ff8451c284600f00ff8044618b12a46314619f08dff")

    # Indexed formats 7..10 have 2/4/16/256 palette entries and 4 bytes each.
    require(struct.unpack_from("<8I", data, 0xC24F70) ==
            (2, 4, 16, 256, 8, 16, 64, 1024), "indexed palette tables changed")
    signature(data, 0xA295A, "90f821a0866c02afaaf10703ddb2bb61")
    signature(data, 0xA29EA, "032927d8dfe801f03f024e46")
    require(0xA29F2 + data[0xA29F2 + 3] * 2 == 0xA2A7E,
            "format 10 no longer selects the 8-bit indexed branch")
    signature(data, 0xA2A7E, "002c44d04ff0ff3e00234ff0080cb9e7")
    # Copy the complete 32-bit palette value, including byte 3 alpha, into
    # the format-16 output image. No palette-alpha information is discarded.
    signature(data, 0xA29AC, "1023a18cc4f830b0")
    signature(data, 0xA2A10,
              "3278a3eb0c051a410eea0202d2b258f822206bb2002b40f82120")

    # Event getters: current target +0, original target +4, u16 code +8
    # (only the lower 15 bits), user_data +12. Dispatch rewrites +0 to the
    # parent when bubbling, not +4: these names must not be interchanged.
    signature(data, 0x49BB0, "80b4006800afbd465df8047b704700bf")
    signature(data, 0x49BC0, "80b4406800afbd465df8047b704700bf")
    copied_thunk(data, 0x4C8210, 0x49BD0)
    copied_thunk(data, 0x4C8310, 0x49BF4)
    signature(data, 0x49BD0, "80b4008900afc0f30e00bd465df8047b7047")
    signature(data, 0x49BF4, "80b4c06800afbd465df8047b7047")
    signature(data, 0x49508, "c7e9000023f00703384639813a613b76fc60")
    signature(data, 0x494C2, "2389263b012b04d820462560fff7bfffb0bd")

    # add_event returns the new 12-byte descriptor in r0, not a bool and
    # not an object. The user callback itself is a one-argument void call.
    copied_thunk(data, 0x4C83F0, 0x49618)
    require(bl_target(data, 0x49636) == 0x49A90, "event-registration body changed")
    signature(data, 0x49A90,
              "2de9f04182b0044600af0c201d468846164603f013f97860"
              "c0f800807b68c3e90156")
    signature(data, 0x49ABE, "78680837bd46bde8f081")
    signature(data, 0x49A1C, "5a683046f2601b689847337e13f00200")
    # Important limitation: immediately after allocation at 0x49AA2 the
    # firmware writes [r0], with no NULL test. This does not prove safe OOM.

    # Native DELETE = 0x24 before object destruction, STYLE_CHANGED = 0x2D.
    copied_thunk(data, 0xA008E8, 0x7C11C, psram=True)
    require(bl_target(data, 0x7C12A) == 0x7BFC8, "object-delete body changed")
    signature(data, 0x7C00C, "0022242120464bf0ddfe00285fd02046fff768fe")
    require(bl_target(data, 0x7C012) == 0xC7DD0, "DELETE sender changed")
    copied_thunk(data, 0xC7DD0, 0x494FC)
    signature(data, 0x47100, "2d21284602f0faf9")
    signature(data, 0x47160, "2d21204602f0caf9")
    require(bl_target(data, 0x47104) == bl_target(data, 0x47164) == 0x494FC,
            "style-change sender changed")

    # The stock notification page array explicitly names these RAM entries.
    # Initializer records: key +0x14, current root +0x30, create +0x4C,
    # destroy +0x64. Only aggregation (index 0 in fw_api) belongs in this hook.
    require([u32(data, p) for p in (0xC61638, 0xC6163C)] ==
            [0x200D1134, 0x200D10C0], "notification page RAM table changed")
    for base, key, name, create, destroy in [
        (0xD0D738, 0x00140001, b"page_aggregation\0", 0x0C525AFD, 0x0C525B19),
        (0xD0D7AC, 0x00140000, b"page_detail\0", 0x0C52613D, 0x0C5260A9),
    ]:
        require(u32(data, base + 0x14) == key, "notification key changed")
        require(u32(data, base + 0x30) == 0, "initial root must be empty")
        require(u32(data, base + 0x4C) == create, "notification create slot changed")
        require(u32(data, base + 0x64) == destroy, "notification destroy slot changed")
        name_at = u32(data, base + 0x10) - 0x2C0C0000
        require(data[name_at:name_at + len(name)] == name, "notification page name changed")
    signature(data, 0x465AFC, "98b5044c0846d2e9002300af2160fff761ff98bd")
    require(u32(data, 0x465B10) == 0x20110E88, "aggregation root global changed")

    # Generic create writes +0x30 BEFORE calling create(desc, root, start).
    signature(data, 0x9AAEC4, "dbf800000bf0a4fa28638046")
    signature(data, 0x9AAF60, "28461a68296be36c9847286b")
    signature(data, 0x9AAF8C, "05222b6b85f82820")  # state = 5 after create
    # Synchronous destroy callback, object DELETE, root=0, state=4, in order.
    signature(data, 0x9AABDE, "5a6e002afad0284690472c6b0026204655f07bfe")
    require(bl_target(data, 0x9AABEE) == 0xA008E8, "page delete target changed")
    signature(data, 0x9AABFC, "ec8a2e63")
    signature(data, 0x9AAC0E, "0423043785f82830")
    # +0x24 != 0 uses deferred destruction: state can become 4 BEFORE root=0.
    signature(data, 0x9AABC0, "6b6ae98aac8a002b33d1")
    signature(data, 0x9AAC4C, "1248591c40f8235004230437117085f82830")
    require(bl_target(data, 0x9AB52C) == 0xA008E8, "deferred page delete target changed")
    signature(data, 0x9AB530, "4ff0000cb8f81440c8f830c0")


def verify_resource(data):
    require(hashlib.sha256(data).hexdigest() == RESOURCE_SHA256,
            "wrong no_message resource: expected the original p67 / 3.101.043 file")
    require(data[:12] == bytes.fromhex("190a00006000600060000000"),
            "expected magic25/format10, 96x96, stride96 indexed resource")
    require(len(data) == 12 + 256 * 4 + 96 * 96, "wrong indexed image size")
    palette = [data[12 + i * 4:16 + i * 4] for i in range(256)]
    black = [i for i, color in enumerate(palette) if color[:3] == bytes(3)]
    require(black == [0] and palette[0][3] == 255, "black palette entry changed")
    pixels = data[12 + 1024:]
    require(pixels.count(0) == 5036, "black-pixel usage changed")
    changed = bytearray(data)
    for index in black:
        changed[12 + index * 4 + 3] = 0
    require([i for i, (old, new) in enumerate(zip(data, changed)) if old != new] == [15],
            "the legacy transformation must change only black palette alpha")


def verify_sources():
    assets = (ROOT / "supervisor/src/background_assets.rs").read_text()
    require('b"/resource/app/notifications/no_message.bin\\0"' in assets,
            "legacy replacement resource path changed")
    require("load(EMPTY_PATH, 96, 96, true)" in assets, "legacy resource dimensions changed")
    require(re.search(r"write_volatile\(\s*(?:\(color \+ 3\) as \*mut u8|color\.add\(3\)),\s*0\s*\)", assets),
            "legacy alpha transformation changed")
    for filename, function, address, return_type in [
        ("gfx.rs", "obj_add_event", 0x0C5883F1, "Obj"),
        ("gfx.rs", "event_get_code", 0x0C588211, "u32"),
        ("gfx.rs", "event_user_data", 0x0C588311, "u32"),
        ("glass.rs", "background_image_set_source", 0x0C587FA1, None),
    ]:
        source = (ROOT / "supervisor/src/fw_api" / filename).read_text()
        body = re.search(rf'pub unsafe extern "C" fn {function}\([^}}]+?\n}}', source)
        require(body is not None, f"missing wrapper {function}")
        targets = re.findall(r"transmute\((0x[0-9a-fA-F_]+)usize\)", body[0])
        require(len(targets) == 1 and int(targets[0].replace("_", ""), 16) == address,
                f"{function} does not use its audited stock thunk")
        header = body[0].split("{", 1)[0]
        require((f"-> {return_type}" in header) if return_type else ("->" not in header),
                f"wrong return type for {function}")
    home = (ROOT / "supervisor/src/fw_api/home.rs").read_text().replace("_", "")
    for literal in ["0x200D10C0", "0x200D1134", "0x00140001", "0x00140000",
                    "0x0C525AFD", "0x0C52613D"]:
        require(literal in home, f"notification page ABI constant changed: {literal}")
    require("pub const BACKGROUNDNOTIFYCREATESLOT: u32 = 0x4C;" in home,
            "notification create offset changed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", nargs="?", type=Path, default=IMAGE)
    parser.add_argument("--resource", type=Path)
    args = parser.parse_args()
    resource = args.resource
    if resource is None:
        found = list((ROOT / "research/firmware-3.101.043").glob(
            "**/files/app/notifications/no_message.bin"))
        require(len(found) == 1, "expected one original no_message.bin; pass --resource explicitly")
        resource = found[0]
    verify_firmware(args.image.read_bytes())
    verify_resource(resource.read_bytes())
    verify_sources()
    print(f"PASS p67 / 3.101.043 AP SHA-256: {SHA256}")
    print("PASS no_message: 96x96 indexed-alpha; 256-entry palette; alpha-only change affects 5036 pixels")
    print("PASS IMAGE_CLASS 0x2CCE61EC, obj+0x34 source, copy/thunk mapping, path strdup/free semantics")
    print("PASS event current+0 / target+4 / code16+8 / userdata+12; DELETE=0x24, STYLE_CHANGED=0x2D")
    print("PASS add_event descriptor return, void callback, aggregation key/root/create/destroy lifecycle")
    print("LIMIT: root clears AFTER DELETE; deferred destruction sets state+0x28=4 before root clears.")
    print("LIMIT: add_event allocation is not NULL-checked in this image; OOM safety is NOT proven.")
    print("STATIC ONLY: no firmware execution, thread/race simulation or on-device crash verification.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, struct.error) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        sys.exit(1)
