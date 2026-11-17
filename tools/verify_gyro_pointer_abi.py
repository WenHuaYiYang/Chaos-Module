#!/usr/bin/env python3
"""Static wrist-cursor ABI evidence for the exact p67/3.101.043 AP; no hardware claims."""
from pathlib import Path
import argparse
import hashlib
import re
import struct
from verify_keyboard_abi import SHA256, FLASH_DATA, signature, copied_entry, thunk_target, bl_target, require

ROOT = Path(__file__).resolve().parents[1]
VALUES = {
    'GYRO_INDEV_CREATE': 0x0CAC0C91,
    'GYRO_INDEV_DELETE': 0x1C0506D5,
    'GYRO_INDEV_NEXT': 0x0C587F21,
    'GYRO_INDEV_SET_TYPE': 0x1C050799,
    'GYRO_INDEV_SET_READ': 0x1C0507B1,
    'GYRO_INDEV_RESET': 0x0C587DC9,
    'GYRO_DISPLAY_SYS_LAYER': 0x0C588DC9,
}

def verify(image):
    require(hashlib.sha256(image).hexdigest() == SHA256, 'wrong AP: requires exact p67/3.101.043 image')
    source = (ROOT/'supervisor/src/fw_api/fwaddr.rs').read_text()
    for name, address in VALUES.items():
        matches = re.findall(r'pub const '+name+r': u32 = (0x[0-9A-Fa-f_]+);', source)
        require(len(matches) == 1 and int(matches[0].replace('_', ''), 16) == address,
                'production binding changed: '+name)
    for thunk, body in ((0xA00C90, 0x9DA88),
                        (0x4C7F20, 0x9DB80), (0x4C7DC8, 0x9DC8C), (0x4C8DC8, 0x7CFB4)):
        require(thunk_target(image, thunk) == copied_entry(image, body, 0xB20, 0x20000000),
                f'boot-copy/thunk mismatch at {thunk:#x}')
    for name, body in (('GYRO_INDEV_DELETE', 0x9DB14),
                       ('GYRO_INDEV_SET_TYPE', 0x9DBD8), ('GYRO_INDEV_SET_READ', 0x9DBF0)):
        require(VALUES[name] == copied_entry(image, body, 0xB20, 0x20000000), name+' copy mismatch')
    signature(image, 0x9DBD8, '80b400af20b1837b017043f002038373')
    signature(image, 0x9DBF0, '80b400af00b14160')
    signature(image, 0x9DA9E, 'cc220021') # sizeof(lv_indev_t) == 204
    signature(image, 0x9DABC, 'a062')     # read timer +0x28
    signature(image, 0x9DAD6, '6062')     # display +0x24
    signature(image, 0x9E938, '14220021') # zero sizeof(lv_indev_data_t) == 20
    signature(image, 0x9E95A, '6368')     # read_cb +4
    signature(image, 0x9E964, '07f10c019847') # callback(indev, stack+12)
    signature(image, 0x9E99A, 'ba7f')     # data state = stack+30 => offset 18
    signature(image, 0x9E9A2, '2273')     # indev state +0x0c
    signature(image, 0x7CFBE, 'd4f8cc02') # system layer above top layer, display+0x2cc
    signature(image, 0x229838, '80b50a4600af40f68121') # integer us -> SNIOC_SET_INTERVAL
    node = b'/dev/uorb/sensor_gyro0\0'
    require(image[0xAAD47C:0xAAD47C+len(node)] == node, 'gyro device path changed')
    require(struct.unpack_from('<II', image, 0xC32EFC) == (FLASH_DATA+0xA7FE70, 24),
            'native sensor_gyro metadata/event size changed')
    # Page identity reuses sys.rs rather than adding a new native call. Check the
    # actual stock top-root accessor, manager literal and watch-face visibility load.
    sys_source = (ROOT/'supervisor/src/fw_api/sys.rs').read_text()
    for name, value in {'PAGE_MGR': 0x2013EDE4, 'FW_PAGE_BY_INDEX': 0x0CA6A7CD,
                        'DESC_PAGE_ID': 0x14, 'DESC_APP_ID': 0x16,
                        'DESC_ROOT': 0x30, 'HOME_APP_ID': 16, 'WATCHFACE_ON': 0x2010C980}.items():
        matches = re.findall(r'pub const '+name+r': u32 = ([0-9A-Fa-f_x]+);', sys_source)
        require(len(matches) == 1 and int(matches[0].replace('_', ''), 0) == value,
                'page identity binding changed: '+name)
    signature(image, 0x9ACF4C, '80b50a4800af038843b1418831b1013989b2')
    require(bl_target(image, 0x9ACF5E) == 0x9AA7CC, 'top-page descriptor accessor changed')
    signature(image, 0x9ACF64, '006b80bd') # descriptor root +0x30
    require(struct.unpack_from('<I', image, 0x9ACF78)[0] == 0x2013EDE4,
            'top-page manager literal changed')
    signature(image, 0x3FF564, '064c02af2578')
    require(struct.unpack_from('<I', image, 0x3FF580)[0] == 0x2010C980,
            'watch-face visibility literal changed')
    return True

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path)
    verify(parser.parse_args().image.read_bytes())
    print('PASS exact AP digest, boot-copy/thunks, virtual input lifecycle and packed input layout')
    print('PASS gyro node, 24-byte native event, integer sampling interval and system overlay layer')
    print('PASS existing native page manager/root and watch-face visibility bindings')
    print('STATIC ONLY: physical sensor axes, touch/rendering and hardware behavior need device verification.')

if __name__ == '__main__':
    main()
