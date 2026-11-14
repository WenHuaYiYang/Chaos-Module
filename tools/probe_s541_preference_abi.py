#!/usr/bin/env python3
"""执行3.101.043官方布局getter/setter，模型仅代替KV与通知接口。"""
from pathlib import Path
import struct
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_LR
root=Path(__file__).resolve().parents[1]
data=(root/'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin').read_bytes()
m=Uc(UC_ARCH_ARM,UC_MODE_THUMB);base=0x0C0C0000;ret=0x10000000
m.mem_map(base,(len(data)+4095)&~4095);m.mem_write(base,data)
m.mem_map(0x20000000,0x20000);m.mem_map(0x20100000,0x20000);m.mem_map(ret,4096)
kv=[];events=[]
def hook(u,a,size,user):
    if a==0x0CABE7AC:
        assert u.reg_read(UC_ARM_REG_R3)==1
        kv.append(bytes(u.mem_read(u.reg_read(UC_ARM_REG_R2),1))[0])
        u.reg_write(UC_ARM_REG_R0,1);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
    elif a==0x0CA41584:
        events.append((u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1),u.reg_read(UC_ARM_REG_R2)))
        u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
m.hook_add(UC_HOOK_CODE,hook)
def call(address,r0=0,r1=0):
    m.reg_write(UC_ARM_REG_R0,r0);m.reg_write(UC_ARM_REG_R1,r1)
    m.reg_write(UC_ARM_REG_SP,0x2001F000);m.reg_write(UC_ARM_REG_LR,ret|1)
    m.emu_start(address|1,ret,count=100)
    return m.reg_read(UC_ARM_REG_R0)
for preference in [2,1,0]:
    assert call(0x0C514300,preference,1)==1
    assert call(0x0C514360)==preference
assert kv==[2,1,0] and events==[(0x10000,1,1)]*3
count=len(kv);assert call(0x0C514300,0,1)==1;assert len(kv)==count
print('通过：官方三布局偏好保存、读取与刷新通知；重复选择不重复写入')
