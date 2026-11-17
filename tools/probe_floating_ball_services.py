#!/usr/bin/env python3
"""Execute production ARM adapters and exact .043 machine code.

Application dispatch and LVGL hardware are explicit seams, not a device test.
"""
from pathlib import Path
import argparse,struct,subprocess,tempfile
from verify_floating_ball_services import verify
from probe_honeycomb_abi import Machine
from elftools.elf.elffile import ELFFile
from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_FPEXC,UC_ARM_REG_C1_C0_2
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser();p.add_argument('image',type=Path);args=p.parse_args()
    image=args.image.read_bytes();verify(image);m=Machine(image)
    m.u.reg_write(UC_ARM_REG_C1_C0_2,0x00F00000);m.u.reg_write(UC_ARM_REG_FPEXC,0x40000000)
    with tempfile.TemporaryDirectory()as temp:
        archive=Path(temp)/'services.a';exe=Path(temp)/'services.elf'
        subprocess.run(['rustc','--edition=2021','--target','thumbv8m.main-none-eabi','--crate-type','staticlib',
                        '-C','panic=abort','-C','opt-level=s',str(ROOT/'supervisor/tests/floating_ball_services_arm.rs'),'-o',str(archive)],check=True)
        sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())
        host=next(x.split(': ',1)[1]for x in subprocess.check_output(['rustc','-vV'],text=True).splitlines()if x.startswith('host:'))
        subprocess.run([str(sysroot/'lib/rustlib'/host/'bin/rust-lld'),'-flavor','gnu','-Ttext=0x10000000','--gc-sections',
                        '--entry=test_battery','-u','test_home',str(archive),'-o',str(exe)],check=True)
        with exe.open('rb')as f:
            elf=ELFFile(f);pages=set()
            for s in elf.iter_segments():
                if s['p_type']=='PT_LOAD':pages.update(range(s['p_vaddr']&~4095,(s['p_vaddr']+s['p_memsz']+4095)&~4095,4096))
            for page in pages:m.u.mem_map(page,4096)
            for s in elf.iter_segments():
                if s['p_type']=='PT_LOAD'and s['p_filesz']:m.u.mem_write(s['p_vaddr'],s.data())
            syms={s.name:s['st_value']for s in elf.get_section_by_name('.symtab').iter_symbols()if s.name}
        for value in range(101):
            m.put(0x2013EFC0,value)
            assert m.call(syms['test_battery'])==value
        for value in [101,255,0xFFFFFFFF,0xFFFFFFFE]:
            m.put(0x2013EFC0,value);assert m.call(syms['test_battery'])==0xFFFFFFFF
        print('PASS actual native battery getter: all 0..100 percentages; invalid values rejected')
        events=[];request=0x0DFF0001;m.u.mem_map(request&~4095,4096)
        m.hooks[0x0DFF0000]=lambda u:events.append(('home',u.reg_read(UC_ARM_REG_R0)))or 0
        m.hooks[0x0C50FD7C]=lambda u:events.append(('prepare',u.reg_read(UC_ARM_REG_R0)))or 0
        m.hooks[0x0C1F0D20]=lambda u:0
        m.put(0x200EB6F0,request)
        assert m.call(syms['test_home'])==1
        production=list(events);assert production==[('prepare',0),('home',0)]
        events.clear()
        m.call(0x0C624B31,0)
        assert events==production,'adapter must match real stock return-home callback'
        print('PASS production home adapter matches the stock return-home callback and argument/order ABI')
        for invalid in [0,2,0xDEADBEEF,request&~1]:
            events.clear();m.put(0x200EB6F0,invalid)
            assert m.call(syms['test_home'])==0 and not events
        print('PASS invalid native service pointers fail without changing UI')
        # Run the real home-panel reset too. Only lower LVGL geometry/draw and
        # notification/log services are replaced; the firmware's guard/order/
        # target-coordinate decisions execute unchanged.
        del m.hooks[0x0C50FD7C]
        m.hooks[0x0CA72F3C]=lambda u:2
        for i,record in enumerate([0x20110168,0x20110154,0x20110140,0x2011012C]):
            panel=0x20060000+i*0x100;content=panel+0x40
            m.put(record,panel,0,0,content)
        m.hooks[0x0C588768]=lambda u:0
        m.hooks[0x0C588AC8]=lambda u:1
        moves=[]
        def move(axis):
            def capture(u):
                moves.append((axis,u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1),u.reg_read(UC_ARM_REG_R2)));return 0
            return capture
        m.hooks[0x0C589420]=move('y');m.hooks[0x0C589508]=move('x')
        m.hooks[0x0C504BE8]=lambda u:0;m.hooks[0x0C518340]=lambda u:0
        m.hooks[0x0C201FE0]=lambda u:0;m.hooks[0x0CAA4368]=lambda u:0
        m.call(0x0C50FD7D,0)
        assert moves==[('y',0x20060000,480,0),('y',0x20060100,0,0),('x',0x20060200,336,0),('x',0x20060300,0,0)],moves
        print('PASS real stock home reset places all four panels at canonical watchface coordinates')
if __name__=='__main__':main()
