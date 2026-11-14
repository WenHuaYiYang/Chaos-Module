#!/usr/bin/env python3
"""Audit native GIF frame descriptors on exact p67/.043 firmware.

--probe compiles the production image port for ARM and runs it with the real
firmware source setter. Layout, decoder registration and cache operations use
explicit host seams. This is not a hardware rendering/power claim.
"""
import argparse
import hashlib
from pathlib import Path
import re
import struct
import subprocess
import tempfile
from verify_keyboard_abi import SHA256, require, signature, copied_entry, thunk_target
from verify_notification_icon_abi import verify_firmware

ROOT = Path(__file__).resolve().parents[1]
ORIGINAL_HEADER = bytes.fromhex('191000006000600080010000')

def verify(image):
    require(hashlib.sha256(image).hexdigest() == SHA256, "expected exact p67/3.101.043 AP")
    verify_firmware(image) # variable image assignment, old-string ownership and indexed-alpha decoder
    addresses = (ROOT / "supervisor/src/fw_api/fwaddr.rs").read_text()
    for name, value in (("IMAGE_CLASS", 0x2CCE61EC), ("LAUNCHER_ICON_APP_ID", 0x0C51805D)):
        found = re.findall(r"pub const " + name + r": u32 = (0x[\da-fA-F_]+);", addresses)
        require(len(found) == 1 and int(found[0].replace("_", ""), 16) == value, name)
    require(thunk_target(image, 0x4C90D0) == copied_entry(image, 0xA6B14, 0xB20, 0x20000000)
            == 0x1C0596D5, "image cache-drop boot-copy mapping")
    signature(image, 0xA6B14, "b0b5044686b000af00f08ef8")
    signature(image, 0x4580A6, "03680134")
    signature(image, 0x4580AE, "8088")
    asset = (ROOT / "supervisor/assets/dynamic-icon.frames").read_bytes()
    source = (ROOT / "tools/chaos.gif").read_bytes()
    require(asset[:8] == b"DGF1\x60\0\x60\0" and asset[28:60] == hashlib.sha256(source).digest(), "supplied 96px GIF source hash")
    print("PASS exact AP: native variable/file source setters, path-copy ownership, indexed-alpha format, cache key and app-id lookup")
    print("PASS embedded 96px GIF source hash")

def unpack_frame(asset, index):
    table = struct.unpack_from('<I', asset, 20)[0]
    _, start, size = struct.unpack_from('<III', asset, table + index * 12)
    encoded = asset[start:start + size]; result = bytearray(); at = 0
    while at < len(encoded):
        tag = encoded[at]; at += 1; n = (tag & 127) + 1
        if tag & 128: result.extend(bytes((encoded[at],)) * n); at += 1
        else: result.extend(encoded[at:at + n]); at += n
    return asset[60:1084] + result

def probe(image):
    from elftools.elf.elffile import ELFFile
    from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                                  UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC)
    from probe_honeycomb_abi import Machine, RETURN
    with tempfile.TemporaryDirectory(prefix="chaos-gif-port-") as tmp:
        archive = Path(tmp) / "port.a"; executable = Path(tmp) / "port.elf"
        subprocess.run(['rustc', '--edition=2021', '--target', 'thumbv8m.main-none-eabi',
                        '--crate-type', 'staticlib', '-C', 'panic=abort', '-C', 'opt-level=s',
                        '-C', 'target-feature=-fpregs',
                        str(ROOT / 'supervisor/tests/dynamic_icon_port_arm.rs'), '-o', str(archive)], check=True)
        sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
        host = next(line.split(': ', 1)[1] for line in subprocess.check_output(['rustc', '-vV'], text=True).splitlines() if line.startswith('host:'))
        linker = sysroot / 'lib/rustlib' / host / 'bin/rust-lld'
        subprocess.run([str(linker), '-flavor', 'gnu', '-Ttext=0x10000000', '--gc-sections',
                        '--entry=test_frame', '-u', 'test_static', '-u', 'test_original', '-u', 'test_retire', '-u', 'test_timer', '-u', 'test_pack_timer', str(archive), '-o', str(executable)], check=True)
        m = Machine(image)
        with executable.open('rb') as file:
            elf = ELFFile(file); pages = set()
            for segment in elf.iter_segments():
                if segment['p_type'] == 'PT_LOAD':
                    pages.update(range(segment['p_vaddr'] & ~4095,
                                       (segment['p_vaddr'] + segment['p_memsz'] + 4095) & ~4095, 4096))
            for page in pages: m.u.mem_map(page, 4096)
            for segment in elf.iter_segments():
                if segment['p_type'] == 'PT_LOAD' and segment['p_filesz']:
                    m.u.mem_write(segment['p_vaddr'], segment.data())
            symbols = {symbol.name: symbol['st_value'] for symbol in elf.get_section_by_name('.symtab').iter_symbols() if symbol.name}
        def call(address, *args):
            for register, value in zip((UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3), (*args, 0, 0, 0, 0)):
                m.u.reg_write(register, value)
            m.u.reg_write(UC_ARM_REG_SP, 0x2001E000); m.u.reg_write(UC_ARM_REG_LR, RETURN | 1)
            m.u.emu_start(address | 1, RETURN, count=2_000_000)
            require(m.u.reg_read(UC_ARM_REG_PC) == RETURN, 'ARM GIF port instruction budget exhausted')
            return m.u.reg_read(UC_ARM_REG_R0)
        def word(at): return struct.unpack('<I', m.u.mem_read(at, 4))[0]
        def execute(offset): return copied_entry(image, offset, 0xB20, 0x20000000) & ~1
        object = 0x20040000; path = 0x20060000
        m.put(object, 0x2CCE61EC, 0x20030000, 0, 0, 0, 0, 0x20031000)
        m.put(object + 0x1C, 100, 200, 195, 295)
        m.put(object + 0x34, path, 0, 0, 0, 96, 96, 0, 176, 176)
        m.u.mem_write(object + 0x60, bytes((65, 9))) # native owned file source + indexed mode
        m.u.mem_write(path, b'/data/chaos/chaos_icon.bin\0')
        held = {offset: word(object + offset) for offset in (0, 4, 0x18, 0x1C, 0x20, 0x24, 0x28, 0x4C, 0x50, 0x54)}
        freed = []; dropped = []; alive = [True]; delete = [False]; invalidations = [0]
        original_path = b'/data/chaos/chaos_icon.bin\0'
        original_header = ORIGINAL_HEADER # decoder-registration seam; producer is checked by test_static_icon.py
        allocations = []; fail_strdup = [False]
        def info(u):
            source = u.reg_read(UC_ARM_REG_R0); target = u.reg_read(UC_ARM_REG_R1)
            if u.mem_read(source, 1)[0] == 0x19:
                header = bytes(u.mem_read(source, 12))
            else:
                require(bytes(u.mem_read(source, len(original_path))) == original_path, 'original file decoder path')
                header = original_header
            u.mem_write(target, header); return 1
        def strdup(u):
            source = u.reg_read(UC_ARM_REG_R0)
            require(bytes(u.mem_read(source, len(original_path))) == original_path, 'native setter copies the original path')
            if fail_strdup[0]: return 0
            target = 0x20061000 + len(allocations) * 0x100
            u.mem_write(target, original_path); allocations.append(target); return target
        def free(u): freed.append(u.reg_read(UC_ARM_REG_R0))
        def cache_drop(u):
            source = u.reg_read(UC_ARM_REG_R0)
            if alive[0]: require(source != word(object + 0x34), 'evicted currently bound playback buffer')
            dropped.append(source)
        def invalidate(u):
            invalidations[0] += 1
            if delete[0] and invalidations[0] == 2:
                alive[0] = False; u.mem_unmap(object, 4096)
        m.hooks[execute(0x83798)] = info # decoder registration/header lookup seam
        m.hooks[execute(0xADA78)] = strdup
        m.hooks[execute(0xADAE4)] = free # native heap ownership, not our frame data
        m.hooks[0x0C5890D0] = cache_drop
        m.hooks[execute(0x78744)] = invalidate
        for offset in (0x77DF8, 0xBA27C, 0x772FC): m.hooks[execute(offset)] = True
        timer = 0x20070000; timer_creations = []
        def create_timer(u):
            callback, period = u.reg_read(UC_ARM_REG_R0), u.reg_read(UC_ARM_REG_R1)
            require(period == 1000 and callback & 1, 'initial timer period/Thumb callback')
            timer_creations.append(callback); m.put(timer, period, 0, callback); return timer
        m.hooks[0x0C587ED0] = create_timer
        require(call(symbols['test_timer'], 0, 20) == 1 and not timer_creations, 'static selection must not create a timer')
        require(call(symbols['test_timer'], 1, 20) == 1 and word(timer) == 20, '20ms native period setter')
        require(call(symbols['test_timer'], 0, 20) == 1 and word(timer) == 1000, 'idle native period setter')
        m.put(timer, 999)
        require(call(symbols['test_timer'], 1, 20) == 0 and word(timer) == 999, 'unknown timer layout must remain untouched')
        m.put(timer, 1000)
        require(call(symbols['test_timer'], 1, 20) == 1 and len(timer_creations) == 1, 'reuse one animation timer')
        require(call(symbols['test_pack_timer'], 1, 50) == 1 and word(timer) == 20, 'shared timer uses faster app request')
        require(call(symbols['test_timer'], 0, 20) == 1 and word(timer) == 50, 'pack animation continues when app animation stops')
        require(call(symbols['test_pack_timer'], 0, 50) == 1 and word(timer) == 1000, 'idle only when both animation clients stop')
        require(call(symbols['test_timer'], 1, 20) == 1 and len(timer_creations) == 1, 'both clients reuse the same timer')
        asset = (ROOT / 'supervisor/assets/dynamic-icon.frames').read_bytes()
        def pixels(source):
            header = bytes(m.u.mem_read(source, 28))
            require(header[:12] == bytes.fromhex('190a00006000600060000000'), 'native 96px indexed-alpha descriptor')
            size, pointer = struct.unpack_from('<II', header, 12)
            require(size == 10240 and pointer & 63 == 0, 'payload size/64-byte alignment')
            return bytes(m.u.mem_read(pointer, size))
        source = path
        for index in (0, 1, 96, 192):
            previous = source; invalidations[0] = 0
            source = call(symbols['test_frame'], object, source, index)
            require(source != previous and word(object + 0x34) == source, 'native variable-source assignment')
            require(pixels(source) == unpack_frame(asset, index), 'production ARM GIF pixels and palette')
            require(all(word(object + offset) == value for offset, value in held.items()), 'source update changed geometry/zoom/click data')
        before = len(dropped)
        fail_strdup[0] = True
        require(call(symbols['test_original'], object, source) == 1 and word(object + 0x34) == source, 'file setter OOM keeps current frame')
        require(len(dropped) == before, 'failed restoration must not evict bound playback cache')
        fail_strdup[0] = False
        for index in (17, 31, 45):
            invalidations[0] = 0
            require(call(symbols['test_original'], object, source) == 1, 'restore original file')
            source = word(object + 0x34)
            require(source == allocations[-1], 'native setter binds its owned duplicate, not our static path')
            require(bytes(m.u.mem_read(source, len(original_path))) == original_path, 'restored original path')
            before = len(dropped)
            require(call(symbols['test_retire']) == 1 and len(dropped) == before + 2, 'retire both buffers only after file is bound')
            require(all(word(object + offset) == value for offset, value in held.items()), 'file restore changed geometry/zoom/click data')
            invalidations[0] = 0
            source = call(symbols['test_frame'], object, source, index)
            require(pixels(source) == unpack_frame(asset, index), 'resume GIF after original file')
        require(freed == [path, *allocations], 'only native-owned paths are freed; descriptors and caller path never freed')
        require(dropped, 'bounded playback cache retirement did not run')
        delete[0] = True; invalidations[0] = 0
        require(call(symbols['test_original'], object, source) == 1 and not alive[0], 'DELETE during file setter')
        alive[0] = True; delete[0] = False
        m.u.mem_map(object, 4096)
        for offset, value in held.items(): m.put(object + offset, value)
        m.put(object + 0x34, path, 0, 0, 0, 96, 96, 0, 176, 176)
        m.u.mem_write(object + 0x60, bytes((65, 9)))
        m.hooks[0x0C588F88] = lambda u: 0
        for choice, name in ((2, 'dark-potato'), (3, 'dark-icon')):
            source = call(symbols['test_static'], object, word(object + 0x34), choice)
            require(source != 0 and word(object + 0x34) == source, 'static icon bound by real firmware setter')
            header = bytes(m.u.mem_read(source, 28))
            require(header[:12] == ORIGINAL_HEADER, 'static BGRA header unchanged')
            size, pointer = struct.unpack_from('<II', header, 12)
            require(size == 36864 and pointer & 63 == 0, 'static pixels aligned')
            expected = (ROOT / f'supervisor/assets/app-icons/{name}.bin').read_bytes()[12:]
            require(bytes(m.u.mem_read(pointer, size)) == expected, 'static icon pixels preserved exactly')
            require(all(word(object + offset) == value for offset, value in held.items()), 'static icon changed geometry or click data')
        print('PASS two dark icons: production ARM port, real source setter and exact BGRA pixels')
        print('PASS production ARM port + real setter: GIF pixels, original file restore/resume, OOM preserves current source and cache')
        print('PASS native-owned path lifetime, two aligned playback buffers and one 20ms/1000ms timer')
        print('PASS source swap preserves geometry/zoom/click data; no instance access after setter DELETE')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path); parser.add_argument('--probe', action='store_true')
    args = parser.parse_args(); image = args.image.read_bytes(); verify(image)
    if args.probe: probe(image)
    print('HOST/STATIC ONLY: display/GPU cache and real device playback still need p67 hardware')

if __name__ == '__main__': main()
