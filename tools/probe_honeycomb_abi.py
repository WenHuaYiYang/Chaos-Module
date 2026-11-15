#!/usr/bin/env python3
"""Execute the exact firmware's event accessors, vector reader and class dispatch.

Only the destination base-class callback is intercepted; this is machine-code
evidence for the ABI, not a Bluetooth/display/touchscreen or device test.
"""
from pathlib import Path
import argparse
import struct
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC
from verify_honeycomb_abi import verify, VALUES

FLASH=0x0C0C0000; DATA=0x2C0C0000
EVENT=0x20010000; INPUT=0x20011000; OUT=0x20012000; DERIVED=0x20013000
RETURN=0x2001F000

class Machine:
    def __init__(self, image):
        self.u=Uc(UC_ARCH_ARM,UC_MODE_THUMB|UC_MODE_MCLASS)
        for base in (FLASH,DATA):
            self.u.mem_map(base,(len(image)+4095)&~4095); self.u.mem_write(base,image)
        self.u.mem_map(0x20000000,0x200000)
        for table,alias in ((0xAA8,0x1FE00000),(0xB20,0x20000000)):
            source,destination,end=struct.unpack_from('<3I',image,table)
            start=destination-alias; length=end-source
            page=start&~4095; size=((start-page+length+4095)&~4095)
            self.u.mem_map(page,size)
            self.u.mem_write(start,image[source-DATA:end-DATA])
        self.hooks={};self.calls=[]
        self.u.hook_add(UC_HOOK_CODE,self.hook)
    def hook(self,u,address,size,data):
        if address==RETURN:u.emu_stop()
        if address in self.hooks:
            self.calls.append((address,u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1)))
            callback=self.hooks[address]
            if callable(callback):
                result=callback(u)
                if result is not None:u.reg_write(UC_ARM_REG_R0,result)
            u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
    def put(self,address,*words):self.u.mem_write(address,struct.pack('<'+'I'*len(words),*words))
    def call(self,address,*args):
        for r,v in zip((UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3),(*args,0,0,0,0)):
            self.u.reg_write(r,v)
        self.u.reg_write(UC_ARM_REG_SP,0x2001E000);self.u.reg_write(UC_ARM_REG_LR,RETURN|1)
        self.u.emu_start(address|1,RETURN,count=50000)
        assert self.u.reg_read(UC_ARM_REG_PC)==RETURN,'instruction budget exhausted'
        return self.u.reg_read(UC_ARM_REG_R0)

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',type=Path)
    image=p.parse_args().image.read_bytes();verify(image);m=Machine(image)
    m.put(EVENT,0x20018000,0x20019000,0x8002,69,INPUT,0,0)
    assert m.call(VALUES['EVENT_CURRENT_TARGET'],EVENT)==0x20018000
    assert m.call(VALUES['EVENT_INPUT_DEVICE'],EVENT)==INPUT
    assert m.call(0x0C588211,EVENT)==2  # event code masks off the preprocess bit
    assert m.call(0x0C588311,EVENT)==69 # app_id slot used by the original callback
    for kind in (1,3):
        for x,y in ((-128,127),(20,0),(0,-40),(0,0)):
            m.put(INPUT,kind);m.put(INPUT+0x50,x&0xffffffff,y&0xffffffff)
            m.put(OUT,0xdeadbeef,0xdeadbeef)
            m.call(VALUES['INPUT_DEVICE_VECTOR'],INPUT,OUT)
            assert struct.unpack('<ii',m.u.mem_read(OUT,8))==(x,y)
    m.put(OUT,1,1);m.call(VALUES['INPUT_DEVICE_VECTOR'],0,OUT)
    assert bytes(m.u.mem_read(OUT,8))==bytes(8)
    for kind in (1,3):
        m.put(INPUT,kind);m.put(INPUT+0x38,330,240)
        m.call(VALUES['INPUT_DEVICE_POINT'],INPUT,OUT)
        assert struct.unpack('<ii',m.u.mem_read(OUT,8))==(330,240)
    m.call(VALUES['INPUT_DEVICE_POINT'],0,OUT)
    assert bytes(m.u.mem_read(OUT,8))==bytes(8)
    print('PASS real 32-bit signed x/y vectors and press points, null input, event target/code/app_id accessors')
    native=VALUES['LAUNCHER_GRID_CLASS']
    parent=struct.unpack_from('<I',image,native-DATA)[0]
    base_cb=struct.unpack_from('<I',image,parent-DATA+12)[0]&~1
    m.hooks[base_cb]=True
    m.call(VALUES['OBJ_EVENT_BASE'],native,EVENT)
    assert m.calls[-1]==(base_cb,parent,EVENT)
    native_cb=VALUES['LAUNCHER_GRID_EVENT']&~1;m.hooks[native_cb]=True
    m.put(DERIVED,native,0,0,0,0,0,336,480,0x108c80)
    m.call(VALUES['OBJ_EVENT_BASE'],DERIVED,EVENT)
    assert m.calls[-1]==(native_cb,native,EVENT)
    print('PASS real class-base dispatcher: native -> generic base; derived -> native (no recursion)')
    # Native app-id lookup scans callback identity, NOT event dispatch filter.
    # Preserve it while the extra descriptor gates drag-induced short clicks.
    icon=0x20019000;grid=0x20018000;spec=0x2001A000;array=0x2001B000
    stock=0x2001C000;extra=0x2001C100
    m.put(VALUES['LAUNCHER_GRID_SLOT'],grid)
    m.put(icon,0,grid,spec)
    m.put(spec+12,array,2,2,4,0)
    m.put(array,stock,extra)
    m.put(stock,VALUES['LAUNCHER_ICON_CLICK'],69,4)
    m.put(extra,0x20017001,69,4)
    assert m.call(0x0C51805C,icon)==69
    m.put(stock+8,0x7fff)
    assert m.call(0x0C51805C,icon)==69
    print('PASS real native app-id lookup before/after disarming the original click filter')
    # Exercise the layout registry/dispatcher independently of class events.
    # Only allocation and style-value lookup are modelled; registration table
    # writes, lookup and callback dispatch are the firmware's real instructions.
    registry=0x20103174;table=0x20070000;callback=0x20017001
    m.put(registry+0x60,3,table)
    m.hooks[0x1C0606BC]=lambda u:table # copied realloc wrapper, source 0xADAFC
    layout=m.call(VALUES['LAYOUT_REGISTER'],callback,123)
    assert layout==3
    assert struct.unpack('<II',m.u.mem_read(table+layout*8,8))==(callback,123)
    style_getter=struct.unpack_from('<I',image,0xC7F1C)[0]&~1
    m.hooks[style_getter]=lambda u:layout
    m.hooks[callback&~1]=True
    m.call(0x1C054E58,grid) # copied layout dispatcher, source 0xA2298
    assert m.calls[-1]==(callback&~1,grid,123)
    print('PASS real registered-layout callback dispatch, separate from class-event dispatch')
    print('HOST MACHINE-CODE EVIDENCE ONLY: actual hardware input/rendering remains untested.')

if __name__=='__main__':main()
