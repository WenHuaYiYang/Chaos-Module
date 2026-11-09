#!/usr/bin/env python3
"""Check exact p67/.043 image, boot-copy mappings, native class/callers and port.

Standard library only. No firmware is distributed. For machine-code probes run
probe_honeycomb_abi.py separately (Unicorn); neither replaces hardware testing.
"""
from pathlib import Path
import argparse
import re
import struct
from verify_keyboard_abi import (SHA256, FLASH_DATA, copied_entry, thunk_target,
                                 signature, bl_target, require)
import hashlib

ROOT = Path(__file__).resolve().parents[1]
VALUES = {
    "LAUNCHER_GRID_SLOT": 0x201102EC, "LAUNCHER_ROOT_SLOT": 0x201102F4,
    "LAUNCHER_GRID_CLASS": 0x2CD2024C, "LAUNCHER_GRID_CTOR": 0x0C514981,
    "LAUNCHER_GRID_DTOR": 0x0C5147C1, "LAUNCHER_GRID_EVENT": 0x0C515ECD,
    "LAUNCHER_ICON_CLICK": 0x0C517419, "LAUNCHER_GRID_REFRESH": 0x0C515CF9,
    "LAUNCHER_POSITION_EVENT": 0x0C517D9D,
    "LAUNCHER_SNAPSHOT_SLOT": 0x201102DC, "LAUNCHER_TRANSITION_SLOT": 0x201102D9,
    "LAUNCHER_CREATE_SNAPSHOT": 0x0C5176FD,
    "LAUNCHER_COMMIT_RECTS": 0x0C514655, "LAYOUT_REGISTER": 0x0C588C99,
    "OBJ_SET_LAYOUT": 0x0C588BA9, "OBJ_GET_STYLE_VALUE": 0x0C187F19,
    "OBJ_EVENT_COUNT": 0x0C588A49, "OBJ_EVENT_DESCRIPTOR": 0x0C587C91,
    "OBJ_REMOVE_EVENT_CALLBACK": 0x0C589571,
    "OBJ_EVENT_BASE": 0x0C588CC9, "EVENT_INPUT_DEVICE": 0x0C588DA9,
    "EVENT_CURRENT_TARGET": 0x0C588CE9, "INPUT_DEVICE_VECTOR": 0x0C588681,
    "INPUT_DEVICE_POINT": 0x0C587B51,
    "IMAGE_SET_ZOOM": 0x0C5881B9, "ANIMATION_DELETE": 0x0C5888C9,
}

def verify(data):
    require(hashlib.sha256(data).hexdigest() == SHA256, "wrong image: expected p67/3.101.043 AP")
    source = (ROOT/"supervisor/src/fw_api/fwaddr.rs").read_text()
    for name, expected in VALUES.items():
        matches = re.findall(r"pub const " + name + r": u32 = (0x[0-9a-fA-F_]+);", source)
        require(len(matches) == 1 and int(matches[0].replace("_", ""),16) == expected,
                "production address mismatch: " + name)
    require(struct.unpack_from("<9I",data,0xC6024C) == (
        0x2CDBA8B4,0x0C514981,0x0C5147C1,0x0C515ECD,0,0,336,480,0x108C80),
        "native 36-byte class descriptor changed")
    for off, expected in {
        0x454980:"2de9f04f2ded048b002303250c4689b0",
        0x4547C0:"d1f8900840b198b500af0c4674f00cfb",
        0x455ECC:"2de9f04fafb0864806af0e4672f0f6fe",
        0x455CF8:"f0b583b000af044672f06afa051e32dd",
        # The registered grid layout consumes user_data rectangles, not align.
        0x454654:"2de9f04f044683b000af786073f0bafdd4f8b4681eb10c37",
        0x45563E:"a3693a69c3e90085a36920469e60da60",
        0x455266:"0a4873f016fd01462860fff7d4bb",
        0x457418:"2de9f0439db000af064670f075ff414b04461868",
        0x49BB0:"80b4006800afbd465df8047b7047", # current target = event[0]
        0x49BE4:"80b4006900afbd465df8047b7047", # input device = event[0x10]
        0x9E100:"002380b4c1e9003300af40b1037803f0fd03012b03d1036d0b60436d4b60",
        0x9E050:"80b400afa8b1037803f0fd03012b08d0",
    }.items(): signature(data,off,expected)
    # Never call the file copies directly: use the audited runnable thunks.
    for thunk, body, table, alias in (
        (0x4C8A48,0x49640,0xAA8,0x1FE00000),
        (0x4C7C90,0x49654,0xAA8,0x1FE00000),
        (0x4C9570,0x4967C,0xAA8,0x1FE00000),
        (0x4C8CC8,0x495E0,0xAA8,0x1FE00000),
        (0x4C8CE8,0x49BB0,0xAA8,0x1FE00000),
        (0x4C8DA8,0x49BE4,0xAA8,0x1FE00000),
        (0x4C8680,0x9E100,0xB20,0x20000000),
        (0x4C7B50,0x9E050,0xB20,0x20000000),
        (0x4C81B8,0xBAB4C,0xB20,0x20000000),
        (0x4C8C98,0xA2264,0xB20,0x20000000),
        (0x4C8BA8,0x77888,0xB20,0x20000000),
    ):
        require(thunk_target(data,thunk) == copied_entry(data,body,table,alias),
                f"boot-copy/thunk mismatch at {thunk:#x}")
    require(bl_target(data,0x455F2E) == 0x4C8680, "stock vector consumer changed")
    require(bl_target(data,0x455612) == 0x4C81B8, "stock image zoom consumer changed")
    require(bl_target(data,0x45782E) == 0x4C83F0, "stock icon callback registration changed")
    signature(data,0x457828,"04222046") # r2=SHORT_CLICKED, r0=icon
    require(struct.unpack_from("<I",data,0x457A68)[0] == VALUES["LAUNCHER_ICON_CLICK"],
            "stock callback literal changed")
    require(struct.unpack_from("<I",data,0x458314)[0] == VALUES["LAUNCHER_POSITION_EVENT"],
            "stock root-position callback literal changed")
    require(bl_target(data,0x45826E) == 0x4C9578, "stock motion-container parent lookup changed")
    # Both SCROLL_BEGIN endpoints enter native screenshot generation; the
    # page is callback user data (+12), and its absolute y is at +32.
    require(bl_target(data,0x457E0A) == 0x4576FC, "native snapshot capture path changed")
    signature(data,0x457DB4,"204670f0abfa0546")
    require(struct.unpack_from('<I',data,0x457ED0)[0] == VALUES['LAUNCHER_TRANSITION_SLOT'],
            "native transition latch changed")
    require(struct.unpack_from('<I',data,0x457EE4)[0] == VALUES['LAUNCHER_SNAPSHOT_SLOT'],
            "native snapshot slot changed")
    # Native get-icon-app-id uses descriptor[0] callback / descriptor[1] user data.
    signature(data,0x4580A6,"03680134")
    signature(data,0x4580AE,"8088")
    # Registered layout dispatcher asks for LV_STYLE_LAYOUT (0x16), then calls
    # table[id].callback(obj, table[id].user_data). It is a separate execution
    # stage from lv_obj_class.event_cb.
    signature(data,0xA229C,"162200af00210446")
    require(bl_target(data,0xA22A4) == 0xC7F18, "layout style getter changed")
    signature(data,0xA22BC,"02ebc301496852f833309847")
    print("PASS exact AP, 36-byte class, native app callback, x/y vector, zoom and runnable boot-copy mappings")

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("image",type=Path)
    verify(p.parse_args().image.read_bytes())

if __name__ == "__main__": main()
