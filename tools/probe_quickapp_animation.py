#!/usr/bin/env python3
"""执行生产ARM适配与043构造/动画指令，系统引擎与分配器为显式替身。"""
from pathlib import Path
import argparse,struct,subprocess,tempfile
from verify_honeycomb_abi import verify
from probe_honeycomb_abi import Machine
from elftools.elf.elffile import ELFFile
from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3
ROOT=Path(__file__).resolve().parents[1]
def main():
    p=argparse.ArgumentParser();p.add_argument('image',type=Path);args=p.parse_args()
    image=args.image.read_bytes();verify(image);m=Machine(image)
    with tempfile.TemporaryDirectory() as folder:
        archive=Path(folder)/'port.a';exe=Path(folder)/'port.elf'
        subprocess.run(['rustc','--edition=2021','--target','thumbv8m.main-none-eabi','--crate-type','staticlib',
            '-C','panic=abort','-C','opt-level=s',str(ROOT/'supervisor/tests/quickapp_animation_arm.rs'),'-o',str(archive)],check=True)
        sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())
        host=next(x.split(': ',1)[1] for x in subprocess.check_output(['rustc','-vV'],text=True).splitlines() if x.startswith('host:'))
        subprocess.run([str(sysroot/'lib/rustlib'/host/'bin/rust-lld'),'-flavor','gnu','-Ttext=0x10000000','--gc-sections',
            '--entry=test_toggle','-u','test_tick',str(archive),'-o',str(exe)],check=True)
        with exe.open('rb') as f:
            e=ELFFile(f);pages=set()
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD':pages.update(range(s['p_vaddr']&~4095,(s['p_vaddr']+s['p_memsz']+4095)&~4095,4096))
            for page in pages:m.u.mem_map(page,4096)
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD' and s['p_filesz']:m.u.mem_write(s['p_vaddr'],s.data())
            syms={s.name:s['st_value'] for s in e.get_section_by_name('.symtab').iter_symbols() if s.name}
        word=lambda a:struct.unpack('<I',m.u.mem_read(a,4))[0]
        app=0x20040000;proxy=0x20041000;root=0x20042000;vt=0x20043000;head=0x20044000;node=0x20044100
        # Run the REAL constructor: establishes +4c/+50 and dynamic app id ABI.
        m.put(app+0x18,0);m.call(0x0c5478d1,0x123,app,proxy)
        assert word(app+0x4c)==0xc547699 and word(app+0x50)==0xc547765
        m.put(app+4,proxy);m.put(proxy+0x2c,root);m.put(root+0x2c,0)
        m.put(0x200eb640,head);m.put(0x200eb658,vt);m.put(vt+0x4c,0x0dff0001)
        m.put(head+4,node);m.put(node+4,head);m.u.mem_write(node+0x10,struct.pack('<H',0x123))
        m.hooks[0x0dff0000]=lambda u:app if u.reg_read(UC_ARM_REG_R0)==0x123 else 0
        m.u.mem_map(0x0dff0000,4096)
        calls=[];animations=[]
        m.hooks[0xca6cc98]=lambda u:calls.append(('show',u.reg_read(UC_ARM_REG_R0))) or 7
        m.hooks[0xc547698]=lambda u:calls.append(('create',u.reg_read(UC_ARM_REG_R0))) or 0
        m.hooks[0xc547764]=lambda u:calls.append(('foreground',u.reg_read(UC_ARM_REG_R0))) or 0
        # Context init uses native memset; animation builder runs to native driver seam.
        def memset(u):
            dst=u.reg_read(UC_ARM_REG_R0);m.u.mem_write(dst,bytes([u.reg_read(UC_ARM_REG_R1)&255])*u.reg_read(UC_ARM_REG_R2));return dst
        m.hooks[0xcac0c40]=memset
        m.hooks[0xca45c6c]=lambda u:animations.append(u.reg_read(UC_ARM_REG_R0)) or 0
        m.put(0x200eb6f8,0xca6cc99);m.call(syms['test_toggle'])
        hook=word(0x200eb6f8);assert hook!=0xca6cc99
        assert m.call(hook,0x10,0,0,0)==7
        assert word(app+0x4c)==0xc547699 and word(app+0x50)==0xc547765
        assert m.call(hook,0x123,0,0,0)==7
        create=word(app+0x4c);show=word(app+0x50)
        assert create!=0xc547699 and show!=0xc547765
        m.call(create,app,0,0);m.put(0x20050000,0x123);m.call(syms['test_tick']);assert animations==[root]
        m.call(show,app);m.call(syms['test_tick']);assert animations==[root,root]
        m.call(syms['test_tick']);assert len(animations)==2
        m.call(show,app);m.put(root+0x2c,1);m.call(syms['test_tick']);assert len(animations)==2
        m.put(root+0x2c,0);m.call(syms['test_tick']);assert len(animations)==3
        m.call(syms['test_toggle']);assert word(0x200eb6f8)==0xca6cc99
        assert word(app+0x4c)==0xc547699 and word(app+0x50)==0xc547765
        assert [c[0] for c in calls]==['show','show','create','foreground','foreground']
        m.put(0x200eb6f8,0x0dff0001)
        m.call(syms['test_toggle']);assert word(0x200eb6f8)==0x0dff0001
        m.call(syms['test_toggle']);assert word(0x200eb6f8)==0x0dff0001
        print('PASS native constructor, production launch hook, cold/warm callback forwarding, native animation builder, once-only and restore')
        print('HOST ONLY: actual engine timing and visible cold launch require device testing')
if __name__=='__main__':main()
