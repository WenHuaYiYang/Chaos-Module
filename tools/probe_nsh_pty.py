#!/usr/bin/env python3
"""Execute p67/3.101.043 PTY machine code; only OS primitives/pipes are modeled.

Run .venv/bin/python tools/probe_nsh_pty.py. --static-only requires no dependencies.
This is an ABI/emulator probe, not a scheduler/real-device validation.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import struct
import unittest

ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / 'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin'
SHA = '519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec'
FLASH = 0x0C0C0000
DATA = 0x2C0C0000
PTY_IOCTL = 0x0C1AC2E0
PTY_WRITE = 0x0C1AC484
FILE_IOCTL = 0x0C1CFF88
GETPID = 0x0C2093EC
READLINE = 0x0C240128


def static_checks(raw):
    assert hashlib.sha256(raw).hexdigest() == SHA, 'wrong firmware image'
    def signature(va, expected):
        expected = bytes.fromhex(expected)
        assert raw[va - FLASH:va - FLASH + len(expected)] == expected, hex(va)
    signature(PTY_IOCTL, '2de9f0470369')
    signature(PTY_WRITE, '2de9f04f83b00369')
    signature(0x0C1ACB7A, '40f20342')
    signature(0x0C1AC542, '022148465bf05bfa')
    signature(GETPID, '80b4054b00af186808b183681868')
    fops = [0x0C1AC61D, 0x0C1AC569, 0x0C1AC769, 0x0C1AC485,
            0, 0x0C1AC2E1, 0, 0, 0x0C1AC1F9, 0, 0, 0x0C1AC711]
    assert list(struct.unpack_from('<12I', raw, 0x2CCBEA50 - DATA)) == fops
    assert struct.unpack_from('<I', raw, 0x2CCBEA80 - DATA)[0] == 0x0C1ACADD
    table = 0x0C1AC32C
    cases = {0x101: 0x0C1AC3EA, 0x102: 0x0C1AC3CA,
             0x118: 0x0C1AC3B6, 0x119: 0x0C1AC41A,
             0x12B: 0x0C1AC40A, 0x12C: 0x0C1AC396, 0x12D: 0x0C1AC38A}
    for request, target in cases.items():
        assert table + 2 * raw[table - FLASH + request - 0x101] == target
    print('PASS exact p67 AP, PTY/ptmx fops, ioctl jump table, flags and PID/signal signatures')


def run_emulator(raw):
    try:
        from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE
        from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
                                       UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR,
                                       UC_ARM_REG_PC)
    except ImportError as exc:
        raise SystemExit('Unicorn is required; use .venv/bin/python or --static-only') from exc
    regs = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)

    class Machine:
        STOP = 0x01000000
        PIPE = 0x01000100
        GETC = 0x01000200
        PUTC = 0x01000300
        PUTS = 0x01000400
        F = 0x20030000
        INODE = 0x20030100
        SF = 0x20030200
        SINODE = 0x20030300
        ROOT = 0x20031000
        ARG = 0x20032000
        INPUT = 0x20034000
        def __init__(self):
            self.u = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
            size = (len(raw) + 0xFFF) & ~0xFFF
            for base in (FLASH, DATA):
                self.u.mem_map(base, size)
                self.u.mem_write(base, raw)
            self.u.mem_map(0x20000000, 0x200000)
            self.u.mem_map(self.STOP, 0x1000)
            self.forward = []
            self.pipe_result = 0
            self.signals = []
            self.writes = []
            self.sem = []
            self.stopped = False
            self.input = []
            self.hooks = {
                0x0C1E693C: lambda: 0,   # mutex lock
                0x0C1E697C: lambda: 0,   # mutex unlock
                0x0C206944: lambda: self.semaphore('wait'),
                0x0C2070D0: lambda: self.semaphore('post'),
                0x0C207A00: self.kill,
                0x0C1D25D4: self.write,
                self.PIPE: self.pipe,
                self.GETC: self.getc,
                self.PUTC: lambda: 0,
                self.PUTS: lambda: self.reg(2),
            }
            self.u.hook_add(UC_HOOK_CODE, self.code)
            self.put(self.F + 0x10, self.INODE)
            self.put(self.INODE + 0x18, self.ROOT)
            self.put(self.INODE + 0x10, 0x2CCBEA50)
            self.put(self.SF + 0x10, self.SINODE)
            self.put(self.SINODE + 0x18, self.ROOT + 0x70)
            self.put(self.SINODE + 0x10, 0x2CCBEA50)
            self.put(self.ROOT, self.ROOT)
            self.put(self.ROOT + 0x70, self.ROOT)
            self.u.mem_write(self.ROOT + 0x48, b'\x01')
            self.u.mem_write(self.ROOT + 0xE3, b'\x07')
            self.put(self.ROOT + 0x4C, -1)
            self.put(self.ROOT + 0xBC, -1)
            self.put(self.ROOT + 0x58, 9)   # real pty constructor defaults
            self.put(self.ROOT + 0xC8, 5)
            for index, endpoint in enumerate((8, 0x28, 0x78, 0x98)):
                inode = 0x20033000 + 0x100 * index
                self.put(self.ROOT + endpoint, 0x403)
                self.put(self.ROOT + endpoint + 0x10, inode)
                self.put(inode + 0x10, inode + 0x30)
                self.put(inode + 0x30 + 0x14, self.PIPE | 1)
        def reg(self, i): return self.u.reg_read(regs[i])
        def put(self, addr, value): self.u.mem_write(addr, struct.pack('<I', value & 0xFFFFFFFF))
        def word(self, addr): return struct.unpack('<I', self.u.mem_read(addr, 4))[0]
        def semaphore(self, action):
            self.sem.append((action, self.reg(0))); return 0
        def pipe(self):
            self.forward.append((self.reg(0), self.reg(1), self.reg(2))); return self.pipe_result
        def kill(self):
            self.signals.append((self.reg(0), self.reg(1))); return 0
        def write(self):
            self.writes.append((self.reg(0), bytes(self.u.mem_read(self.reg(1), self.reg(2)))))
            return self.reg(2)
        def getc(self): return self.input.pop(0) if self.input else -1
        def code(self, uc, addr, size, arg):
            if addr == self.STOP:
                self.stopped = True; uc.emu_stop(); return
            if addr == 0x0C1E4180: raise AssertionError('firmware assertion called')
            if addr in self.hooks:
                value = self.hooks[addr]()
                uc.reg_write(UC_ARM_REG_R0, value & 0xFFFFFFFF)
                uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
        def invoke(self, entry, *args):
            for i, arg in enumerate(args): self.u.reg_write(regs[i], arg & 0xFFFFFFFF)
            self.u.reg_write(UC_ARM_REG_SP, 0x201F0000)
            self.u.reg_write(UC_ARM_REG_LR, self.STOP | 1)
            self.stopped = False
            self.u.emu_start(entry | 1, 0, count=20000)
            assert self.stopped, 'instruction budget exhausted (unexpected block/loop)'
            r = self.reg(0)
            return r if r < 0x80000000 else r - 0x100000000

    class Tests(unittest.TestCase):
        def test_getptn_is_32_bit_and_null_is_einval(self):
            m = Machine(); m.put(m.ARG, 0xCCCCCCCC)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12B, m.ARG), 0)
            self.assertEqual(m.word(m.ARG), 7)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12B, 0), -22)
        def test_unlock_takes_value_not_pointer_and_is_idempotent(self):
            m = Machine(); m.u.mem_write(m.ROOT + 0xE1, b'\x01')
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12C, 0), 0)
            self.assertEqual(m.sem, [('post', m.ROOT + 0xE8)])
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12C, 0), 0)
            self.assertEqual(len(m.sem), 1)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12D, m.ARG), 0)
            self.assertEqual(m.word(m.ARG), 0)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12C, 1), 0)
            self.assertEqual(m.sem[-1], ('wait', m.ROOT + 0xE8))
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12D, m.ARG), 0)
            self.assertEqual(m.word(m.ARG), 1)
        def test_fionbio_executes_vfs_and_only_changes_master_directions(self):
            m = Machine(); m.put(m.ARG, 1)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x30A, m.ARG), 0)
            self.assertEqual(m.forward, [(m.ROOT + 8, 0x30A, m.ARG), (m.ROOT + 0x28, 0x30A, m.ARG)])
            self.assertEqual(m.word(m.ROOT + 8), 0x443)
            self.assertEqual(m.word(m.ROOT + 0x28), 0x443)
            self.assertEqual(m.word(m.ROOT + 0x78), 0x403)
            self.assertEqual(m.word(m.ROOT + 0x98), 0x403)
            m.put(m.ARG, 0)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x30A, m.ARG), 0)
            self.assertEqual(m.word(m.ROOT + 8), 0x403)
        def test_readable_query_only_forwards_read_pipe(self):
            m = Machine()
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x306, m.ARG), 0)
            self.assertEqual(m.forward, [(m.ROOT + 8, 0x306, m.ARG)])
        def test_fionbio_vfs_fallback_accepts_enotty_from_pipe(self):
            m = Machine(); m.pipe_result = -25; m.put(m.ARG, 1)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x30A, m.ARG), 0)
            self.assertEqual(m.word(m.ROOT + 8), 0x443)
            self.assertEqual(m.word(m.ROOT + 0x28), 0x443)
        def test_termios_round_trip_uses_this_firmware_layout(self):
            m = Machine()
            m.put(m.ARG, 0x11); m.put(m.ARG + 4, 0x22); m.put(m.ARG + 12, 0x33)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x102, m.ARG), 0)
            self.assertEqual((m.word(m.ROOT + 0x50), m.word(m.ROOT + 0x58), m.word(m.ROOT + 0x54)),
                             (0x11, 0x22, 0x33))
            m.u.mem_write(m.ARG, bytes(16))
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x101, m.ARG), 0)
            self.assertEqual((m.word(m.ARG), m.word(m.ARG + 4), m.word(m.ARG + 12)), (0x11, 0x22, 0x33))
        def test_unknown_ioctl_and_null_getlock_are_rejected(self):
            m = Machine()
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x12D, 0), -22)
            self.assertEqual(m.invoke(PTY_IOCTL, m.F, 0x7FFF, m.ARG), -25)
        def test_ctrl_c_targets_positive_peer_pid_and_clear_releases_slot(self):
            m = Machine(); m.u.mem_write(m.INPUT, b'\x03')
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x118, 123), 0)
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x118, 124), -22)
            self.assertEqual(m.invoke(PTY_WRITE, m.F, m.INPUT, 1), 1)
            self.assertEqual(m.signals, [(123, 2)])
            self.assertFalse(m.writes)
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x119, 0), 0)
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x118, 124), 0)
        def test_zero_pid_does_not_mean_current_pid_and_breaks_subsequent_claim(self):
            m = Machine(); m.u.mem_write(m.INPUT, b'\x03')
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x118, 0), 0)
            self.assertEqual(m.word(m.ROOT + 0xBC), 0)
            self.assertEqual(m.invoke(PTY_IOCTL, m.SF, 0x118, 123), -22)
            self.assertEqual(m.invoke(PTY_WRITE, m.F, m.INPUT, 1), 1)
            self.assertFalse(m.signals)
            self.assertEqual(m.writes, [(m.ROOT + 0x28, b'\x03')])
        def test_ctrl_d_is_data_not_pty_eof_and_is_filtered_by_nsh_line_editor(self):
            m = Machine(); m.u.mem_write(m.INPUT, b'\x04')
            self.assertEqual(m.invoke(PTY_WRITE, m.F, m.INPUT, 1), 1)
            self.assertEqual(m.writes, [(m.ROOT + 0x28, b'\x04')])
            vtable = 0x20035000
            for n, fn in enumerate((m.GETC, m.PUTC, m.PUTS)): m.put(vtable + n * 4, fn | 1)
            m.input = [4, 10]
            self.assertEqual(m.invoke(READLINE, vtable, m.INPUT, 256), 1)
            self.assertEqual(bytes(m.u.mem_read(m.INPUT, 2)), b'\n\0')
            self.assertFalse(m.input)
        def test_getpid_reads_current_task_group_pid(self):
            m = Machine(); tcb = 0x20036000; group = 0x20037000
            m.put(0x200B24D4, tcb); m.put(tcb + 8, group); m.put(group, 123)
            self.assertEqual(m.invoke(GETPID), 123)
            m.put(0x200B24D4, 0)
            self.assertEqual(m.invoke(GETPID), 0)

    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
    if not result.wasSuccessful(): raise SystemExit(1)
    print('HOST ONLY: actual p67 driver/VFS/line-editor executed; pipe internals and scheduler modeled.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--static-only', action='store_true')
    args = parser.parse_args()
    data = IMAGE.read_bytes()
    static_checks(data)
    if not args.static_only:
        run_emulator(data)
