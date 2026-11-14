#!/usr/bin/env python3
"""核对 3.101.043 原始 setlogmask 机器码的所有掩码（需要 unicorn）。"""
from pathlib import Path
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_SP, UC_ARM_REG_LR

ROOT = Path(__file__).resolve().parents[1]
image = (ROOT / 'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin').read_bytes()
start = 0x0C1F0CE0
offset = start - 0x0C0C0000
machine = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
machine.mem_map(start & ~0xFFF, 0x1000)
machine.mem_write(start, image[offset:offset + 24])
machine.mem_map(0x200C8000, 0x1000)
machine.mem_map(0x20000000, 0x1000)
machine.mem_map(0x10000000, 0x1000)
for mask in range(256):
    old = mask ^ 0xFF
    machine.mem_write(0x200C8958, bytes([old]))
    machine.reg_write(UC_ARM_REG_R0, mask)
    machine.reg_write(UC_ARM_REG_SP, 0x20001000)
    machine.reg_write(UC_ARM_REG_LR, 0x10000001)
    machine.emu_start(start | 1, 0x10000000, count=16)
    assert machine.mem_read(0x200C8958, 1) == bytes([mask])
    assert machine.reg_read(UC_ARM_REG_R0) == old
print('PASS: 固件 setlogmask 的256种掩码均直接写入并返回旧值（含0）')
