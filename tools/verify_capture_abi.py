#!/usr/bin/env python3
"""Static exact-image capture ABI audit. No device or framebuffer execution."""
import argparse
import hashlib
from pathlib import Path
import re
import struct
from verify_keyboard_abi import SHA256, bl_target, require, signature

ROOT = Path(__file__).resolve().parents[1]

def verify(data):
    require(hashlib.sha256(data).hexdigest() == SHA256, "expected exact p67/3.101.043 AP")
    # Actual native AT screenshot uses the same device and one 24-bit frame.
    signature(data, 0x5EDBC0, "90b5a1b0044600afa8b1802200213846")
    require(struct.unpack_from("<I", data, 0x5EDC04)[0] == 0x2CBFAE14, "AT screenshot command literal")
    command = data[0xB3AE14:].split(b"\0", 1)[0]
    require(command == b"dd if=/dev/fb0 of=/data/snapshot/%s.bin bs=483840 count=1", "native framebuffer copy size")
    require(bl_target(data, 0x5EDBE2) == 0x180398, "AT screenshot shell entry")
    # Native display initialization/plane lookup: actual ioctl request and fields.
    signature(data, 0x3F7AA4, "2a4642f6010152f8040b18f579fb")
    require(bl_target(data, 0x3F7AAE) == 0x1101A4, "video info ioctl target")
    signature(data, 0x3F7234, "f0b50c4683b042f60201224602af18f5afff002845db")
    require(bl_target(data, 0x3F7242) == 0x1101A4, "plane info ioctl target")
    signature(data, 0x3F728E, "2589")  # stride u16 +8
    signature(data, 0x3F72BA, "e47a")  # bpp u8 +11
    require(struct.unpack_from("<BBHHBB", data, 0xD053E0) == (12, 0, 336, 480, 1, 1), "native video-info initializer")
    require(struct.unpack_from("<IIHBBIIII", data, 0xD053E8) ==
            (0, 967680, 1008, 0, 24, 336, 960, 0, 0), "native double-buffer plane initializer")
    require(bl_target(data, 0xEF0A6) == 0x4C8DB0, "fb_read uses native bulk memcpy")
    # fmt 12 dispatches to LVGL RGB888=15; fmt 11 is RGB565, NOT this path.
    branch = 0x3F7B48 + 2 * struct.unpack_from("<H", data, 0x3F7B4A)[0]
    require(branch == 0x3F7CDC, "RGB24 format branch")
    signature(data, branch, "0f23bb60")
    # A real display-vsync producer calls the notifier. Native fb_poll owns a
    # per-open file slot; poll_notify's fd/events/revents/arg/cb/priv are 6 words.
    require(bl_target(data, 0x166410) == 0xEF91C, "display vsync producer")
    require(bl_target(data, 0xEF93A) == 0x110B44, "vsync poll_notify dispatch")
    signature(data, 0xEF932, "022204f10800114621f003f92468")
    signature(data, 0xEED60, "b0b582b000af7960d0e9041389690868")
    # Native poll() uses these exact file-reference helpers and dispatcher.
    for site, target in ((0x110EA2, 0xF9700), (0x110EB0, 0x110BA4), (0x110E4A, 0xF979C)):
        require(bl_target(data, site) == target, "native poll file reference ABI")
    signature(data, 0x12A25C, "50b1b2f5004f07d20346002012b2c3e9")
    signature(data, 0x110B14, "90b583b00023c46800af7b60")
    signature(data, 0x14690C, "88b1d0e89f3f1a040bd4")
    signature(data, 0x146CA8, "2de9f04f83b01c4b02af")
    require(bl_target(data, 0x110D38) == 0x12A25C, "native poll semaphore init")
    require(bl_target(data, 0x110E26) == 0x146CA8, "native bounded semaphore wait")
    require(struct.unpack_from("<I", data, 0x111044)[0] == 0x0C1D0B15, "native poll wake callback")
    # mkdir and rename signatures + existing bridge share the same entrypoints.
    signature(data, 0x1105E8, "b0b588b000af0d46044616f0e7f80023")
    signature(data, 0x1112E8, "2de9f04fabb000af002800f074810378")
    addresses = (ROOT / "supervisor/src/fw_api/fwaddr.rs").read_text()
    for name, value in (("CAPTURE_MKDIR", 0x0C1D05E9), ("CAPTURE_RENAME", 0x0C1D12E9),
                        ("CAPTURE_GET_FILE", 0x0C1B9701), ("CAPTURE_PUT_FILE", 0x0C1B979D), ("CAPTURE_FILE_POLL", 0x0C1D0BA5),
                        ("CAPTURE_SEM_INIT", 0x0C1EA25D), ("CAPTURE_SEM_DESTROY", 0x0C20690D),
                        ("CAPTURE_POLL_WAKE", 0x0C1D0B15), ("CAPTURE_SEM_WAIT", 0x0C206CA9)):
        found = re.findall(r"pub const " + name + r": u32 = (0x[\da-fA-F_]+);", addresses)
        require(len(found) == 1 and int(found[0].replace("_", ""), 16) == value, name + " production address")
    # Independent page registry entries must advance together.
    state = (ROOT / "supervisor/src/state.rs").read_text()
    page = (ROOT / "supervisor/src/page.rs").read_text()
    ipc = (ROOT / "supervisor/src/ipc.rs").read_text()
    require("PAGE_COUNT: usize = 44" in state, "registered page count")
    for name in ("PAGE_CAPTURE", "PAGE_RECORDINGS", "PAGE_SCREENSHOTS", "PAGE_RECORDSET", "PAGE_SHOTSET"):
        require("pub static " + name in state and "addr_of!(" + name + ")" in ipc, name + " IPC name registration")
        require("&capture::" + name in page, name + " Page table registration")
    require("addr_of!(PAGE_APP_ICON)" in ipc and "&app_icon::PAGE_APP_ICON" in page, "icon-picker page registration")
    print("PASS exact AP: framebuffer layout, VFS/POLLPRI, semaphore wake ABI and 44-page registry")
    print("STATIC ONLY: actual pixels, active-buffer behavior, deep sleep and capture frame rate require hardware validation")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    verify(parser.parse_args().image.read_bytes())
