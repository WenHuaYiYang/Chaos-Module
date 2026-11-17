#!/usr/bin/env python3
"""Execute exact .043 input ABI instructions with explicit allocation/UI substitutes."""
from pathlib import Path
import argparse
import struct
from unicorn import UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_PC
from verify_keyboard_abi import copied_entry
from verify_gyro_pointer_abi import verify, VALUES
from probe_honeycomb_abi import Machine, RETURN

def probe(image):
    verify(image)
    m = Machine(image)
    native = lambda off: copied_entry(image, off, 0xB20, 0x20000000)
    input_device, display, layer, timer = 0x20050000, 0x20052000, 0x20054000, 0x20055000
    callback = 0x20056001
    m.put(0x20103174+0x18, display)
    m.put(display+0x2CC, layer)
    m.u.mem_write(input_device-8, b'canary!!'+bytes([0xA5])*204+b'CANARY!!')
    # Heap/list insertion and timer scheduling are boundary models, not firmware boot.
    m.hooks[native(0xA9C1C)&~1] = lambda u: input_device
    def memset(u):
        destination=u.reg_read(UC_ARM_REG_R0); value=u.reg_read(UC_ARM_REG_R1)
        m.u.mem_write(destination,bytes([value&255])*u.reg_read(UC_ARM_REG_R2))
        return destination
    m.hooks[native(0xADA24)&~1] = memset
    timer_args = []
    def new_timer(u):
        args = tuple(u.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2))
        timer_args.append(args); m.put(timer, args[1], 0, args[0], args[2]); return timer
    m.hooks[native(0xAD474)&~1] = new_timer
    assert m.call(VALUES['GYRO_INDEV_CREATE']) == input_device
    assert bytes(m.u.mem_read(input_device-8,8)) == b'canary!!'
    assert bytes(m.u.mem_read(input_device+204,8)) == b'CANARY!!'
    assert struct.unpack('<II',m.u.mem_read(input_device+0x24,8)) == (display,timer)
    assert timer_args[0][1:] == (8,input_device)
    assert m.call(VALUES['GYRO_DISPLAY_SYS_LAYER'],display) == layer
    m.call(VALUES['GYRO_INDEV_SET_TYPE'],input_device,1)
    m.call(VALUES['GYRO_INDEV_SET_READ'],input_device,callback)
    assert bytes(m.u.mem_read(input_device,1)) == b'\x01'
    assert struct.unpack('<I',m.u.mem_read(input_device+4,4))[0] == callback
    assert struct.unpack('<I',m.u.mem_read(input_device+0x1C,4))[0] == 0
    print('PASS native create/type/read callback, 204-byte allocation canaries, timer/display fields and system layer')

    # Execute through native read_cb dispatch and state decoding. Stop at the
    # downstream widget-dispatch boundary: no app objects or physical display model.
    stop = native(0x9E9A4)&~1
    def stop_at_widget_dispatch(u,address,size,data):
        if address == stop: u.reg_write(UC_ARM_REG_PC,RETURN|1)
    m.u.hook_add(UC_HOOK_CODE,stop_at_widget_dispatch)
    frames = [(168,240,1),(168,240,0),(168,360,1),(168,344,1),(168,120,0)]
    delivered = []
    for x,y,state in frames:
        def read(u):
            assert u.reg_read(UC_ARM_REG_R0) == input_device
            out = u.reg_read(UC_ARM_REG_R1)
            m.u.mem_write(out,struct.pack('<iiIIhBB',x,y,0,0,0,state,0))
            delivered.append((x,y,state))
        m.hooks[callback&~1] = read
        m.call(native(0x9E920),input_device)
        assert bytes(m.u.mem_read(input_device+0x0C,1)) == bytes([state])
    assert delivered == frames
    m.put(0x20103174+0x54,input_device)
    m.call(VALUES['GYRO_INDEV_RESET'],input_device,0)
    assert bytes(m.u.mem_read(input_device+0x0E,1))[0] & 2
    assert struct.unpack('<I',m.u.mem_read(0x20103174+0x58,4))[0] == 0
    print('PASS native 20-byte read data, 32-bit coordinates, press/release decoding and reset-query cancellation')

    freed, removed = [], []
    m.hooks[native(0xAD4E8)&~1] = lambda u: freed.append(u.reg_read(UC_ARM_REG_R0)) or 0
    m.hooks[native(0xA9CE8)&~1] = lambda u: removed.append(u.reg_read(UC_ARM_REG_R1)) or 0
    m.hooks[native(0xADAE4)&~1] = lambda u: freed.append(u.reg_read(UC_ARM_REG_R0)) or 0
    m.call(VALUES['GYRO_INDEV_DELETE'],input_device)
    assert removed == [input_device]
    assert timer in freed and input_device in freed
    print('PASS native delete retires its own timer, list node and input allocation')
    print('MACHINE-CODE ABI EVIDENCE: allocation, scheduling and widget dispatch are substitutes; hardware remains untested.')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',type=Path)
    probe(p.parse_args().image.read_bytes())

if __name__ == '__main__':
    main()
