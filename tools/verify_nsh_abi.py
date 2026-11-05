#!/usr/bin/env python3
'''Static p67/3.101.043 NSH ABI checks.

Checks the exact 043 image, stock callsites and the production address table.
PTY and spawn semantics are exercised separately by probe_nsh_pty.py and
probe_nsh_spawn.py. None of these checks is an on-device validation claim.
'''

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / 'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin'
SHA256 = '519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec'
FLASH_BASE = 0x0C0C0000
SYSTEM_VA = 0x0C240398
NSH_SCRIPT_VA = 0x0C238EAC
NSH_PARSE_VA = 0x0C238A84
SPAWN_VA = 0x0C209770
WAITPID_VA = 0x0C2063C4
ATTR_INIT_VA = 0x0C1EA724
USLEEP_VA = 0x0C1F5D0C
CONSOLE_NEW_VA = 0x0C238E28
NSH_SESSION_VA = 0x0C23DDAC


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def offset(va: int) -> int:
    require(va >= FLASH_BASE, f'VA below AP flash base: 0x{va:08X}')
    return va - FLASH_BASE


def signature(data: bytes, va: int, expected_hex: str) -> None:
    expected = bytes.fromhex(expected_hex)
    off = offset(va)
    require(data[off:off + len(expected)] == expected,
            f'signature mismatch at VA 0x{va:08X} / file 0x{off:X}')


def thumb_bl_target(data: bytes, va: int) -> int:
    off = offset(va)
    hi = int.from_bytes(data[off:off + 2], 'little')
    lo = int.from_bytes(data[off + 2:off + 4], 'little')
    require(hi & 0xF800 == 0xF000 and lo & 0xD000 == 0xD000,
            f'not a Thumb BL at VA 0x{va:08X}')
    s = (hi >> 10) & 1
    i1 = 1 ^ (((lo >> 13) & 1) ^ s)
    i2 = 1 ^ (((lo >> 11) & 1) ^ s)
    imm = ((s << 24) | (i1 << 23) | (i2 << 22) |
           ((hi & 0x3FF) << 12) | ((lo & 0x7FF) << 1))
    if s:
        imm -= 1 << 25
    return va + 4 + imm


def verify_image(data: bytes) -> None:
    require(hashlib.sha256(data).hexdigest() == SHA256,
            'wrong image: only the audited p67/3.101.043 AP is supported')

    signature(data, SYSTEM_VA,
              'b0b58eb0054602af002845d007f11c00aaf7bcf90446002847d1642307f11c00')
    signature(data, NSH_SCRIPT_VA,
              '2de9f04f83b08a4602af11460446994604f01af8002800f0b1800646236a2046')
    signature(data, NSH_PARSE_VA,
              '00232de9f0470e4605460c46884682b0dff8a49000af80f88a34a31b49462046')

    signature(data, SPAWN_VA,
              '2de9f04f8bb08146884616461c4606af')
    signature(data, WAITPID_VA,
              '98b500affff7dcfe041e01db204698bd')
    signature(data, ATTR_INIT_VA,
              'b0b582b0044600af00b30025391d2846')
    signature(data, CONSOLE_NEW_VA,
              'f8b5044600af4ff4b760c0f743f910b3')
    signature(data, NSH_SESSION_VA,
              '2de9f04182b0044602af00286cd01546')

    require(thumb_bl_target(data, 0x0C2403FE) == SPAWN_VA,
            'system() spawn target changed')
    require(thumb_bl_target(data, 0x0C240410) == WAITPID_VA,
            'system() waitpid target changed')
    require(thumb_bl_target(data, 0x0C2390B8) == CONSOLE_NEW_VA,
            'stock console constructor caller changed')
    require(thumb_bl_target(data, 0x0C2390C6) == NSH_SESSION_VA,
            'stock nsh_session caller changed at 0x0C2390C6')
    require(data[offset(0x0C2390CA):offset(0x0C2390CA) + 8] == bytes.fromhex('e36a014620469847'),
            'console exit callback load sequence changed')

    literals = {
        'nsh_session': (0x2CB2E634, b'nsh_session\0'),
        'nsh_main': (0x2CB2B7C8, b'nsh_main\0'),
        'waitpid': (0x2CB2E6A4, b'waitpid\0'),
        'usleep': (0x2CB2DDA0, b'usleep\0'),
        '/dev/ptmx': (0x2CB252B4, b'/dev/ptmx\0'),
        '/dev/pts/%d': (0x2CB25278, b'/dev/pts/%d\0'),
    }
    for name, (va, literal) in literals.items():
        off = va - 0x2C0C0000
        require(0 <= off < len(data) and data[off:off + len(literal)] == literal,
                f'043 literal changed: {name}')


def verify_sources() -> None:
    source_files = list((ROOT / 'supervisor/src').rglob('*.rs'))
    forbidden = (
        '0xc3754b0', '0xc3754b1', '0xc8c2dbc', '0xc8c2dbd',
        '0xc350e8c', '0xc350e8d', '0xc204a78', '0xc204a79',
        'termnshsession', 'terminalnative188',
    )
    for path in source_files:
        text = path.read_text(errors='replace').lower().replace('_', '')
        text = re.sub(r'0x0+([0-9a-f])', r'0x\1', text)
        for token in forbidden:
            require(token not in text, f'foreign Band 11/q66 terminal ABI in {path}: {token}')

    expected = {
        'NSH_SYSTEM': 0x0C240399, 'NSH_TASK_SPAWN': 0x0C209771,
        'NSH_ATTR_INIT': 0x0C1EA725, 'NSH_ATTR_FLAGS': 0x0C1EA781,
        'NSH_ATTR_PRIORITY': 0x0C1EA7A1, 'NSH_ATTR_STACK': 0x0C1EA7C5,
        'NSH_ATTR_DESTROY': 0x0C1EA7E1, 'NSH_WAITPID': 0x0C2063C5,
        'NSH_ACTIONS_INIT': 0x0C1EA711, 'NSH_ACTIONS_ADDDUP2': 0x0C1EA63D,
        'NSH_ACTIONS_DESTROY': 0x0C1EA6E5, 'NSH_CONSOLE_NEW': 0x0C238E29,
        'NSH_SESSION': 0x0C23DDAD, 'NSH_USLEEP': 0x0C1F5D0D,
        'NSH_ERRNO_LOCATION': 0x0C1E45BD, 'NSH_USER_FREE': 0x0C1F8FF9,
    }
    addresses = (ROOT / 'supervisor/src/fw_api/fwaddr.rs').read_text()
    for name, value in expected.items():
        matches = re.findall(rf'pub const {name}: u32 = (0x[0-9a-fA-F_]+);', addresses)
        require(len(matches) == 1 and int(matches[0].replace('_', ''), 16) == value,
                f'production {name} must use exact p67 Thumb entry 0x{value:08X}')
        require(value & 1 == 1, f'{name} is not a callable Thumb address')

    notice = (ROOT / 'docs/NSH-ABI.md').read_text(errors='replace')
    require('持久交互式' in notice and 'NSH-PTY.md' in notice and 'NSH-SPAWN.md' in notice,
            'NSH ABI notice must describe the PTY backend and its evidence')
    require('尚未真机验证' in notice, 'NSH ABI notice must distinguish host evidence from device testing')
    require('0x0C3754B0' in notice and '禁止复制' in notice,
            'notice must explicitly reject the Band 11 session address')
    require('0x0C238E29' in notice and '0x0C23DDAD' in notice,
            'notice must list the verified 043 console/session entries')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', nargs='?', type=Path, default=IMAGE)
    args = parser.parse_args()
    data = args.image.read_bytes()
    verify_image(data)
    verify_sources()
    print(f'PASS p67/3.101.043 AP SHA-256: {SHA256}')
    print('PASS verified system() Thumb entry: 0x0C240399')
    print('PASS 043 spawn/waitpid call graph and console+nsh_session lifecycle')
    print('PASS nsh_script/nsh_parse prologues and NSH/PTY literals')
    print('PASS production NSH entries match p67; no foreign terminal or forbidden pthread entries')
    print('STATIC ONLY: run both machine-code probes separately; on-device behavior remains unverified.')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        sys.exit(1)
