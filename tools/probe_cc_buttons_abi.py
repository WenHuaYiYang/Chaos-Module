#!/usr/bin/env python3
"""Execute exact .043 CC button factory, style wrappers and image-draw init.

Object allocation, generic style writes, alignment and ROM memset are modeled
leaves. Parent/class selection, button dimensions/radius, actual draw-init
layout and source-code bridge constants are verified against the pinned AP.
No device is modified. This is ABI evidence, not visual/GPU acceptance.
"""
from pathlib import Path
import argparse
import struct
from probe_honeycomb_abi import Machine,UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3
from verify_keyboard_abi import IMAGE,ROOT,verify_image

def verify(data):
    verify_image(data)
    m=Machine(data)
    obj,parent,image,dsc=0x20050000,0x20051000,0x20052000,0x20053000
    selected=[];styles=[];sizes=[]
    def factory(u):
        selected.append((u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1)))
        return obj
    m.hooks[0x1C028F80]=factory
    m.hooks[0x1C029074]=lambda u:0
    assert m.call(0x0CA48474,parent)==obj
    assert selected==[(0x2CDB9D6C,parent)]
    m.hooks[0x0CA48474]=lambda u:obj
    m.hooks[0x0C4BF6C0]=lambda u:0
    def width(u):sizes.append(('w',u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1)));return 0
    def height(u):sizes.append(('h',u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1)));return 0
    def style(u):
        styles.append(tuple(u.reg_read(r) for r in (UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3)))
        return 0
    m.hooks.update({0x1C02A330:width,0x1C02A368:height,0x002BD924:style,
                    0x1C02A500:lambda u:0,0x1C06CAC0:lambda u:image})
    assert m.call(0x0C5038C4,parent)==obj
    assert ('w',obj,156) in sizes and ('h',obj,128) in sizes
    assert (obj,12,36,0) in styles
    assert any(s[:3]==(obj,28,0x222222) for s in styles)
    # Run the actual background-opacity wrapper into the generic style seam.
    m.call(0x0C5883F9,obj,96,0)
    assert (obj,29,96,0) in styles
    # ROM memset used by the exact 112-byte image draw initializer.
    m.u.mem_map(0x00270000,0x10000)
    def memset(u):
        dst,value,size=[u.reg_read(r) for r in (UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2)]
        u.mem_write(dst,bytes([value&255])*size);return dst
    m.hooks[0x0027676C]=memset
    m.u.mem_write(dsc,b'\xa5'*120)
    m.call(0x0C5882B9,dsc)
    words=struct.unpack('<30I',m.u.mem_read(dsc,120))
    assert words[5]==112 and words[12]==256 and words[13]==256
    assert m.u.mem_read(dsc+76,1)==b'\xff'
    assert words[28:]==(0xa5a5a5a5,0xa5a5a5a5)
    source=(ROOT/'supervisor/control_center_buttons.inc').read_text()
    assert '#define CC_BUTTON_CLASS 0x2cdb9d6cu' in source
    assert 'P67_NATIVE(0x0c1068a9u))(object,0,12)' in source
    assert 'descriptor[12]=descriptor[13]=256u*scale' in source
    assert 'descriptor[7]=image' in source and 'descriptor)[76]=192u' in source
    print('PASS actual .043: LVX button class/parent, 156x128, radius36, native gray fill, image draw size/scale/opacity ABI')
    print('NOT VERIFIED: native display rendering, GPU alpha/cache acceptance, simulator/device visuals or frame time')

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',type=Path,nargs='?',default=IMAGE)
    verify(p.parse_args().image.read_bytes())
