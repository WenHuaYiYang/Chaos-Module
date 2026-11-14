#!/usr/bin/env python3
"""用3.101.043原始名称回调复现缺失R0崩溃并验证应用节点参数。"""
from pathlib import Path
from unicorn import Uc, UcError, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_LR
import struct

root = Path(__file__).resolve().parents[1]
image = (root / 'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin').read_bytes()
source = (root / 'supervisor/src/fw_api/honeycomb.rs').read_text()
assert 'Some((id, source, name(node)))' in source
base, entry, resolve, finish = 0x0C0C0000, 0x0C549634, 0x0C54A584, 0x10000000

def run(argument):
    machine = Uc(UC_ARCH_ARM, UC_MODE_THUMB)
    machine.mem_map(base, (len(image) + 4095) & ~4095)
    machine.mem_write(base, image)
    machine.mem_map(0x20000000, 0x10000)
    machine.mem_map(finish, 0x1000)
    def word(address, value):
        machine.mem_write(address, struct.pack('<I', value))
    word(0x20001030, 0x20002000)
    word(0x20002008, 0x20003000)
    word(0x20003004, 0x20004000)
    observed = []
    def hook(uc, address, size, user):
        if address == resolve:
            observed.append((uc.reg_read(UC_ARM_REG_R0), uc.reg_read(UC_ARM_REG_R1)))
            uc.reg_write(UC_ARM_REG_R0, 0x20005000)
            uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))
    machine.hook_add(UC_HOOK_CODE, hook)
    machine.reg_write(UC_ARM_REG_R0, argument)
    machine.reg_write(UC_ARM_REG_SP, 0x2000F000)
    machine.reg_write(UC_ARM_REG_LR, finish | 1)
    machine.emu_start(entry | 1, finish, count=30)
    assert observed == [(0, 0x20004004)]
    assert machine.reg_read(UC_ARM_REG_R0) == 0x20005000

try:
    run(0)
except UcError:
    print('复现：缺少应用节点时，原始名称回调读取空指针+0x30失败')
else:
    raise AssertionError('缺失参数未能复现崩溃')
run(0x20001000)
print('通过：传入应用节点后，原始固件回调正确解析并返回名称')
