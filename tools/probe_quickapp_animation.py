#!/usr/bin/env python3
"""执行生产ARM适配与043构造/动画指令，系统引擎与分配器为显式替身。"""
from pathlib import Path
import argparse,struct,subprocess,tempfile
from verify_honeycomb_abi import verify
from probe_honeycomb_abi import Machine
from elftools.elf.elffile import ELFFile
from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3,UC_ARM_REG_SP
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
            '--entry=test_toggle','-u','test_tick','-u','test_close_toggle',str(archive),'-o',str(exe)],check=True)
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
        m.put(vt+8,0x0dff0005)
        m.put(head+4,node);m.put(node+4,head);m.u.mem_write(node+0x10,struct.pack('<H',0x123))
        m.hooks[0x0dff0000]=lambda u:app if u.reg_read(UC_ARM_REG_R0)==0x123 else 0
        m.hooks[0x0dff0004]=lambda u:0 # registry startup-domain lookup seam
        m.u.mem_map(0x0dff0000,4096)
        calls=[];animations=[]
        m.hooks[0xca6cc98]=lambda u:calls.append(('show',u.reg_read(UC_ARM_REG_R0))) or 7
        m.hooks[0xc547698]=lambda u:calls.append(('create',u.reg_read(UC_ARM_REG_R0))) or 0
        m.hooks[0xc547764]=lambda u:calls.append(('foreground',u.reg_read(UC_ARM_REG_R0))) or 0
        m.hooks[0xc5475f8]=lambda u:calls.append(('hide',u.reg_read(UC_ARM_REG_R0))) or 0
        m.hooks[0xc54751c]=lambda u:calls.append(('destroy',u.reg_read(UC_ARM_REG_R0))) or 0
        # Context init uses native memset; animation builder runs to native driver seam.
        def memset(u):
            dst=u.reg_read(UC_ARM_REG_R0);m.u.mem_write(dst,bytes([u.reg_read(UC_ARM_REG_R1)&255])*u.reg_read(UC_ARM_REG_R2));return dst
        m.hooks[0xcac0c40]=memset
        m.hooks[0xca6c65c]=lambda u:0 # native launch-mode state setup
        m.hooks[0xca6c678]=lambda u:0 # native post-launch notification
        m.hooks[0xca45c6c]=lambda u:animations.append(u.reg_read(UC_ARM_REG_R0)) or 0
        exits=[];out_callbacks=[]
        def out_animation(u):
            assert (u.reg_read(UC_ARM_REG_R1),u.reg_read(UC_ARM_REG_R2),u.reg_read(UC_ARM_REG_R3))==(255,0,256)
            assert word(u.reg_read(UC_ARM_REG_SP))==204
            assert word(u.reg_read(UC_ARM_REG_SP)+4)==200
            source=u.reg_read(UC_ARM_REG_R0)
            out_callbacks.append(word(u.reg_read(UC_ARM_REG_SP)+12))
            assert out_callbacks[-1]!=0
            # Native driver creates a sibling snapshot, then hides the source.
            m.put(source+0x2c,word(source+0x2c)|1)
            exits.append(source);return 0
        m.hooks[0xca45db0]=out_animation
        m.hooks[0xca69934]=lambda u:0 # no splash icon in the initial forwarding cases
        original_table=bytes(m.u.mem_read(0x2cde7d18,60))
        assert struct.unpack_from('<I',original_table,0x18)[0]==0xca6cc99
        # Actual interface holds a pointer to FLASH vtable, not an inline callback.
        m.put(0x200eb688,0x2cde7d18);m.put(0x200eb6f8,0xdeadbeef)
        m.call(syms['test_toggle'])
        table=word(0x200eb688);assert table!=0x2cde7d18
        hook=word(table+0x18);assert hook!=0xca6cc99
        assert word(0x200eb6f8)==0xdeadbeef
        for offset in range(0,60,4):
            if offset!=0x18:assert word(table+offset)==word(0x2cde7d18+offset)
        request=0x20045000
        def launch(app_id):
            m.u.mem_write(request,bytes(44));m.u.mem_write(request+6,struct.pack('<H',app_id))
            # Execute the REAL launch request -> interface+4 -> vtable+0x18 chain.
            assert m.call(0xca6d4b5,request)==1
        launch(0x10)
        assert word(app+0x4c)==0xc547699 and word(app+0x50)==0xc547765
        launch(0x123)
        create=word(app+0x4c);show=word(app+0x50)
        assert create!=0xc547699 and show!=0xc547765
        m.call(create,app,0,0);m.put(0x20050000,0x123);m.call(syms['test_tick']);assert animations==[root]
        m.call(show,app);m.call(syms['test_tick']);assert animations==[root,root]
        m.call(syms['test_tick']);assert len(animations)==2
        m.call(show,app);m.put(root+0x2c,1);m.call(syms['test_tick']);assert len(animations)==2
        m.put(root+0x2c,0);m.call(syms['test_tick']);assert len(animations)==3
        m.call(syms['test_toggle']);assert word(0x200eb688)==0x2cde7d18
        assert bytes(m.u.mem_read(0x2cde7d18,60))==original_table
        assert word(app+0x4c)==0xc547699 and word(app+0x50)==0xc547765
        assert [c[0] for c in calls]==['show','show','create','foreground','foreground']
        m.put(0x200eb688,0x20046000);m.u.mem_write(0x20046000,original_table)
        m.put(0x20046000+0x18,0x0dff0001) # third-party hook must remain installed
        m.call(syms['test_toggle']);assert word(0x200eb688)==0x20046000
        m.call(syms['test_toggle']);assert word(0x200eb688)==0x20046000
        # Cold launch: actual production cover code must animate before invoking the engine.
        m.put(0x200eb688,0x2cde7d18)
        m.put(0x20046000+0x2c8,0x20049000)
        m.put(node+0xc,0x20048000);m.u.mem_write(0x20048000,b'/data/quickapp/icon.bin\0')
        m.hooks[0xca69934]=lambda u:node
        m.u.mem_map(0x0dfe0000,4096)
        cover=0x20047000;icon_image=0x20047100
        def create_object(u):
            parent=u.reg_read(UC_ARM_REG_R0);object=cover if parent==0x20049000 else icon_image
            m.put(object+4,parent);m.put(object+0x2c,0);return object
        m.hooks[0x0dfe0000]=create_object
        m.put(0x20050000,16);m.put(0x20050004,0)
        m.call(syms['test_toggle'])
        direct=word(word(0x200eb688)+0x18)
        before=len(calls)
        assert m.call(direct,0x123,0x20048000,0,0)==7
        assert len(calls)==before+1 # deep-link arguments remain synchronous and unretained
        before=len(calls);anim_before=len(animations)
        launch(0x123)
        assert len(calls)==before and animations[anim_before:]==[cover]
        m.put(0x20050004,300);m.call(syms['test_tick'])
        assert len(calls)==before # time alone cannot submit before first cover draw
        ev=0x2004a000;m.put(ev,cover,cover,25,0,0,0,0)
        m.call(word(0x20050010),ev)
        m.call(syms['test_tick']);assert len(calls)==before+1 and calls[-1]==('show',0x123)
        assert word(cover+4)==0x20049000 # opaque cover held across stock engine construction
        m.put(0x20050000,0x29);m.put(0x20050008,0x2004b000)
        m.put(0x2004b000+0x30,0x2004c000);m.put(root+4,0x2004c000)
        m.put(root+0x10,0)
        m.put(0x20050004,350);m.call(syms['test_tick']);assert word(cover+4)!=0
        m.put(root+0x10,1)
        for now in [400,450,500,550]:m.put(0x20050004,now);m.call(syms['test_tick'])
        assert 0<word(cover+0x14)<255
        m.put(0x20050004,700);m.call(syms['test_tick']);assert word(cover+4)==0
        assert len(calls)==before+1 # cold launch submitted exactly once
        m.call(syms['test_toggle'])
        m.put(0x20050000,16);m.put(0x20050004,1000);m.call(syms['test_toggle'])
        before=len(calls);launch(0x123)
        m.put(0x20050004,3100);m.call(syms['test_tick'])
        assert len(calls)==before+1 and word(cover+4)==0 # missing draw ack falls back to normal launch
        m.call(syms['test_toggle'])
        # Closing is independently enabled while opening remains off.
        m.put(0x20050000,16);m.put(0x20050004,4000)
        m.call(syms['test_close_toggle']);before=len(calls);anim_before=len(animations)
        launch(0x123);assert len(calls)==before+1 and len(animations)==anim_before
        hide=word(app+0x5c);destroy=word(app+0x64)
        assert hide!=0xc5475f9 and destroy!=0xc54751d
        m.put(0x20050000,0x29);m.put(root+0x2c,0);m.put(0x2005001c,1)
        m.u.mem_write(proxy+0x39,b'\x01')
        snapshot=0x2004e000;allocations=[];captures=[];freed=[];cache_drops=[]
        def allocate(u):
            allocations.append((u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1),u.reg_read(UC_ARM_REG_R2)))
            return snapshot
        def snapshot_root(u):
            captures.append(u.reg_read(UC_ARM_REG_R0));assert calls[-1][0]=='show';return 1
        m.hooks[0xc587f38]=allocate;m.hooks[0xcac0878]=snapshot_root
        m.hooks[0xcac0db8]=lambda u:freed.append(u.reg_read(UC_ARM_REG_R0)) or 0
        m.hooks[0xc5890d0]=lambda u:cache_drops.append(u.reg_read(UC_ARM_REG_R0)) or 0
        def stock_hide(u):
            # No HOME frame can be exposed before the next maintenance tick:
            # the exact captured image is already covering the outgoing app.
            assert word(cover+4)==0x20049000 and word(icon_image+0x18)==snapshot
            assert not exits and word(cover+0x2c)&1==0
            calls.append(('hide',u.reg_read(UC_ARM_REG_R0)));return 0
        m.hooks[0xc5475f8]=stock_hide
        m.call(hide,app);assert captures==[root] and calls[-1]==('hide',app)
        m.call(destroy,app);assert len(captures)==1 and calls[-1]==('destroy',app)
        m.call(syms['test_tick']);assert not exits # No close animation over another foreground app.
        m.put(0x20050000,16);m.put(0x20050004,4100);m.call(syms['test_tick'])
        assert exits==[cover] and allocations==[(336,480,15)]
        assert word(cover+4)==0x20049000 and not freed
        m.put(0x20050004,4400);m.call(syms['test_tick'])
        assert word(cover+4)!=0 and word(cover+0x2c)&1 # no guessed 240ms teardown
        # Run the REAL native OUT completion aggregator for both animation tracks.
        # It deletes its transition image before invoking our callback with lv_anim_t*.
        a1=0x2004f000;a2=0x2004f060;shared=0x2004f100;helper=0x2004f200
        cleaned=[];state_freed=[]
        m.hooks[0xca455e4]=lambda u:cleaned.append(u.reg_read(UC_ARM_REG_R0)) or 0
        m.hooks[0xcac08d0]=lambda u:state_freed.append(u.reg_read(UC_ARM_REG_R0)) or 0
        m.put(shared,out_callbacks[-1],helper,2,0)
        for a in (a1,a2):m.put(a,cover);m.put(a+0x1c,shared)
        m.call(0xca45c21,a1);m.call(syms['test_tick']);assert word(cover+4)!=0 and not cleaned
        m.call(0xca45c21,a2);assert cleaned==[cover] and state_freed==[shared]
        m.call(syms['test_tick']);assert word(cover+4)==0
        m.put(0x2005001c,0);m.call(syms['test_tick']);assert not freed
        m.put(0x2005001c,1);m.call(syms['test_tick']);assert freed==[snapshot] and cache_drops==[snapshot]
        m.call(syms['test_close_toggle'])
        assert word(app+0x5c)==0xc5475f9 and word(app+0x64)==0xc54751d
        # If no faithful image is available, do not flash a black/icon fallback.
        m.hooks[0xc5475f8]=lambda u:calls.append(('hide',u.reg_read(UC_ARM_REG_R0))) or 0
        m.call(syms['test_close_toggle']);m.put(0x20050000,16);launch(0x123)
        m.put(0x20050000,0x29);m.put(0x2005001c,0);m.call(word(app+0x5c),app)
        assert allocations==[(336,480,15)] and calls[-1]==('hide',app)
        m.put(0x20050000,16);m.call(syms['test_tick']);assert exits==[cover] and word(cover+4)==0
        m.call(syms['test_close_toggle']);assert word(cover+4)==0 and freed==[snapshot]
        # Synchronous HOME return starts OUT in the original callback's turn,
        # and switching elsewhere cancels a prepared cover without playing it.
        m.call(syms['test_close_toggle']);m.put(0x2005001c,1);launch(0x123)
        m.put(0x20050000,0x29)
        m.hooks[0xcac0878]=lambda u:captures.append(u.reg_read(UC_ARM_REG_R0)) or 1
        def return_home(u):
            assert word(cover+4)!=0 and word(cover+0x2c)&1==0
            calls.append(('hide',u.reg_read(UC_ARM_REG_R0)));m.put(0x20050000,16);return 0
        m.hooks[0xc5475f8]=return_home
        m.call(word(app+0x5c),app);assert exits==[cover,cover] # no timer advance
        m.call(syms['test_close_toggle']);m.call(syms['test_tick'])
        m.call(syms['test_close_toggle']);launch(0x123);m.put(0x20050000,0x29)
        def switch_other(u):m.put(0x20050000,32);return 0
        m.hooks[0xc5475f8]=switch_other
        m.call(word(app+0x5c),app);assert word(cover+4)==0 and exits==[cover,cover]
        m.call(syms['test_close_toggle']);m.call(syms['test_tick'])
        print('PASS real launch request vtable dispatch, native constructor, cold/warm forwarding, animation builder, restore and foreign-hook rejection')
        print('PASS cold launch: native cover animation -> first draw -> stock engine once -> content gate -> fade/remove')
        print('PASS close: cover before stock hide, immediate HOME handoff, real dual-track completion, no icon flash, cancellation and GPU-idle retirement')
        print('HOST ONLY: actual engine timing and visible cold launch require device testing')
if __name__=='__main__':main()
