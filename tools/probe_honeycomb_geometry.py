#!/usr/bin/env python3
"""Run production ARM geometry port against the real .043 layout commit.

Models image zoom/invalidation, animation deletion and object memory only.
The rectangle writes and the firmware layout/child/area operations execute real
instructions. No touchscreen/pixels/hardware success is inferred from this.
Requires Rust ARM target, Unicorn and pyelftools, plus the exact AP image.
"""
from pathlib import Path
import argparse
import struct
import subprocess
import tempfile
from elftools.elf.elffile import ELFFile
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1
from probe_honeycomb_abi import Machine, VALUES
from verify_honeycomb_abi import verify

ROOT=Path(__file__).resolve().parents[1]
GRID=0x20030000; SPEC=0x20031000; CHILDREN=0x20032000
ICONS=[0x20034000+0x100*i for i in range(7)]
OFFSETS=[(0,0),(-48,83),(-96,0),(-48,-83),(48,-83),(96,0),(48,83)]

def build(directory):
    archive=directory/'port.a';elf=directory/'port.elf'
    subprocess.run(['rustc','--edition=2021','--target','thumbv8m.main-none-eabi',
        '--crate-type','staticlib','-C','panic=abort','-C','opt-level=s','-C','target-feature=-fpregs',
        str(ROOT/'supervisor/tests/honeycomb_port_arm.rs'),'-o',str(archive)],check=True)
    sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())
    host=next(v.split(': ',1)[1] for v in subprocess.check_output(['rustc','-vV'],text=True).splitlines() if v.startswith('host:'))
    linker=sysroot/'lib/rustlib'/host/'bin/rust-lld'
    subprocess.run([str(linker),'-flavor','gnu','-Ttext=0x10000000','--gc-sections',
        '--entry=test_render','-u','test_frame','-u','test_layout','-u','test_rw_tick',
        '-u','test_bad_layout','-u','test_layout_cycle','-u','test_zoom','-u','test_navigation','-u','test_motion_parent',
        '-u','test_spacing','-u','test_current_frame','-u','test_apply_current_frame','-u','test_stop_return',
        '-u','test_entry_dispatch','-u','test_clear_snapshot',
        '-u','test_entry_source','-u','test_watch_entry','-u','test_stop_watch',
        '-u','test_rw_apply',
        str(archive),'-o',str(elf)],check=True)
    return elf

def machine(image,elf):
    m=Machine(image)
    with elf.open('rb') as f:
        e=ELFFile(f);pages=set()
        for s in e.iter_segments():
            if s['p_type']=='PT_LOAD':pages.update(range(s['p_vaddr']&~4095,(s['p_vaddr']+s['p_memsz']+4095)&~4095,4096))
        for p in pages:m.u.mem_map(p,4096)
        for s in e.iter_segments():
            if s['p_type']=='PT_LOAD' and s['p_filesz']:m.u.mem_write(s['p_vaddr'],s.data())
        symbols={s.name:s['st_value'] for s in e.get_section_by_name('.symtab').iter_symbols() if s.name}
    m.put(GRID+8,SPEC);m.put(SPEC,CHILDREN)
    m.u.mem_write(SPEC+0x30,struct.pack('<H',len(ICONS)))
    m.put(CHILDREN,*ICONS);m.put(GRID+0x1c,0,0,335,479)
    for i,icon in enumerate(ICONS):
        ud=0x20040000+0x40*i
        m.put(icon+4,GRID);m.put(icon+0x18,ud)
        m.put(icon+0x1c,0,0,111,111);m.put(icon+0x44,112,112)
        m.put(ud,10+i*10,20,112,112)
    def zoom(u):
        icon=u.reg_read(UC_ARM_REG_R0);scale=u.reg_read(UC_ARM_REG_R1)
        m.put(icon+0x50,scale,scale)
    def flags(u,add):
        icon=u.reg_read(UC_ARM_REG_R0);v=u.reg_read(UC_ARM_REG_R1)
        old=struct.unpack('<I',u.mem_read(icon+0x2c,4))[0]
        m.put(icon+0x2c,(old|v) if add else (old&~v))
    m.hooks[VALUES['IMAGE_SET_ZOOM']&~1]=zoom
    m.hooks[VALUES['ANIMATION_DELETE']&~1]=lambda u:1
    m.hooks[0x1c028bd8]=lambda u:flags(u,True)
    m.hooks[0x1c028c84]=lambda u:flags(u,False)
    m.hooks[0x0c587e08]=lambda u:0 # enable/disable invalidation only
    m.hooks[0x1c02b304]=lambda u:0 # native lv_obj_invalidate destination
    return m,symbols

def rect(m,icon):return struct.unpack('<4i',m.u.mem_read(icon+0x1c,16))
def centre(r):return ((r[0]+r[2]+1)//2,(r[1]+r[3]+1)//2)

def edit_return_probe(image,elf):
    m,s=machine(image,elf)
    out=0x20075000
    assert m.call(s['test_render'],GRID,0,48,83)==1
    actual=rect(m,ICONS[0])
    zoom=struct.unpack('<I',m.u.mem_read(ICONS[0]+0x50,4))[0]
    # The native writer has already filled a different final destination in
    # metadata. Capture must preserve the CURRENT visible area and transform.
    ud=struct.unpack('<I',m.u.mem_read(ICONS[0]+0x18,4))[0]
    m.put(ud,500,600,112,112)
    before=len(m.calls)
    assert m.call(s['test_current_frame'],GRID,0,out)==1
    assert struct.unpack('<4i',m.u.mem_read(out,16))==(*centre(actual),112*zoom//256,1)
    assert len(m.calls)==before, 'capture must not set geometry or invalidate'
    m.put(ICONS[0]+0x2c,1)
    assert m.call(s['test_current_frame'],GRID,0,out)==1
    assert struct.unpack('<4i',m.u.mem_read(out,16))[-1]==0
    # Mode animations do not depend on ud+0x13; all live icon vars must be
    # stopped once, including those without the native position-animation bit.
    m.put(GRID+0x8b4,7)
    for icon in ICONS:
        p=struct.unpack('<I',m.u.mem_read(icon+0x18,4))[0]
        m.u.mem_write(p+0x13,b'\0')
    m.calls.clear()
    assert m.call(s['test_stop_return'],GRID)==1
    stopped=[obj for a,obj,_ in m.calls if a==VALUES['ANIMATION_DELETE']&~1]
    assert stopped==ICONS
    assert bytes(m.u.mem_read(GRID+0x8b4,4))==bytes(4)
    m.calls.clear()
    for _ in range(16):assert m.call(s['test_render'],GRID,0,0,0)==1
    assert not any(a==VALUES['ANIMATION_DELETE']&~1 for a,_,_ in m.calls)
    for scale in (129,193,254,300):
        size=112*scale//256
        m.put(ICONS[0]+0x50,scale,scale)
        m.put(ICONS[0]+0x1c,0,0,size-1,size-1)
        before=rect(m,ICONS[0])
        assert m.call(s['test_apply_current_frame'],GRID,0)==1
        assert rect(m,ICONS[0])==before, 'takeover must preserve native pixel dimensions'
    print('PASS ARM edit-return captures live bounds instead of final metadata; stops all native icon writers once and avoids per-frame deletion')
    print('PASS ARM takeover preserves native integer image dimensions without a one-pixel shrink')

def layout_loop_probe(image,elf):
    # Real lv_obj_update_layout -> real layout tree -> real callback dispatcher
    # -> compiled ARM callback -> real clear_flag (bad case)/native rect commit.
    # Dimensions, display/event plumbing are modelled for the object fixture.
    from unicorn import UC_HOOK_CODE
    from unicorn.arm_const import UC_ARM_REG_R2
    registry=0x20103174;table=0x20070000
    for bad in (True,False):
        m,s=machine(image,elf)
        del m.hooks[0x1c028bd8];del m.hooks[0x1c028c84]
        m.hooks[0x002bca3c]=lambda u:1 if u.reg_read(UC_ARM_REG_R0)==GRID and u.reg_read(UC_ARM_REG_R2)==0x16 else 0
        for address in (0x1c02f078,0x1c030420,0x1c02b374,0x1c02be50):m.hooks[address]=lambda u:0
        callback=s['test_bad_layout' if bad else 'test_layout_cycle']
        m.put(registry+0x60,2,table);m.put(table+8,callback,0)
        m.u.mem_write(GRID+0x32,b'\x05')
        visits=[0]
        def visit(u,address,size,data):
            if address==callback&~1:visits[0]+=1
        m.u.hook_add(UC_HOOK_CODE,visit)
        if bad:
            try:m.call(0x1c02c760,GRID)
            except AssertionError as e:assert 'budget' in str(e)
            else:raise AssertionError('v1.1.8 layout-dirty cycle unexpectedly settled')
            assert visits[0]>10
            print('REPRODUCED real firmware layout-update loop: old callback does not return within instruction budget')
        else:
            m.call(0x1c02c760,GRID)
            assert visits[0]==1
            assert bytes(m.u.mem_read(registry+0x68,1))==b'\0'
            assert not bytes(m.u.mem_read(GRID+0x32,1))[0]&5
            assert centre(rect(m,ICONS[0]))==(168,240)
            print('PASS real firmware layout-update loop: pure callback returns after one pass and releases layout mutex')

def navigation_probe(image,elf):
    from unicorn.arm_const import UC_ARM_REG_R2
    event=0x20018000;indev=0x20019000;parent=0x20060000
    for chain,delta in ((0x100,(80,0)),(0x200,(0,80))):
        m,s=machine(image,elf)
        m.put(GRID+4,parent);m.put(GRID+0x2c,2)
        m.put(ICONS[0]+0x2c,0x300)
        m.put(parent+0x2c,0x310)
        m.put(indev,1);m.put(indev+0x2c,10)
        m.put(indev+0x50,*delta);m.put(indev+0x70,ICONS[0])
        m.put(event,GRID,ICONS[0],2,0,indev)
        m.hooks[0x002bca3c]=lambda u:256 if u.reg_read(UC_ARM_REG_R2) in (0x6c,0x6d) else 0
        # Parent has scrollable content in either axis. Child/parent walking,
        # chain flags, threshold and final target selection execute real code.
        for src in (0x7a150,0x7a164,0x7aa8c,0x7aac0):
            m.hooks[0x1c000000+src-0x4d440]=lambda u:100
        assert m.call(0x1c052a30,indev)==0
        m.put(indev+0x58,2000,2000)
        m.call(s['test_navigation'],GRID,event,chain)
        assert struct.unpack('<II',m.u.mem_read(indev+0x58,8))==(0,0)
        assert m.call(0x1c052a30,indev)==parent
        assert struct.unpack('<I',m.u.mem_read(GRID+0x2c,4))[0]&0x8300==chain|0x8000
    print('PASS real firmware scroll chooser: closed cloud consumes input; boundary handoff selects native parent on both axes without accumulated jump')

def motion_parent_probe(image,elf):
    m,s=machine(image,elf)
    root,window,container=0x20060000,0x20061000,0x20062000
    spec,array,descriptor=0x20063000,0x20064000,0x20065000
    m.put(GRID+4,root);m.put(root+4,window);m.put(window+4,container)
    m.put(container+8,spec);m.put(spec+12,array,1,1,4,0)
    m.put(array,descriptor);m.put(descriptor,VALUES['LAUNCHER_POSITION_EVENT'],window,0)
    assert m.call(s['test_motion_parent'],GRID)==container
    m.put(descriptor,123,window,0)
    assert m.call(s['test_motion_parent'],GRID)==0
    m.put(descriptor,0x20066001,window,0)
    assert m.call(s['test_motion_parent'],GRID)==container
    event=0x20018000
    for depth in (3,9,12,13):
        m,s=machine(image,elf)
        chain=[0x20060000+i*0x1000 for i in range(depth)]
        spec,array,desc=0x20080000,0x20081000,0x20082000
        m.put(GRID+4,chain[0])
        for child,ancestor in zip(chain,chain[1:]):m.put(child+4,ancestor)
        owner=chain[-1]
        m.put(owner+8,spec);m.put(spec+12,array,1,1,4,0)
        m.put(array,desc);m.put(desc,VALUES['LAUNCHER_POSITION_EVENT'],chain[1],0)
        m.put(event,owner,GRID,9,chain[1],0)
        assert m.call(s['test_motion_parent'],GRID)==(owner if depth<=12 else 0)
        assert m.call(s['test_entry_source'],GRID,event)==int(depth<=12)
        m.put(event,0x20090000,GRID,9,chain[1],0)
        assert m.call(s['test_entry_source'],GRID,event)==0
    print('PASS ARM entry-source lookup accepts live native bindings before attach through 12 ancestors and rejects unrelated/over-limit containers')
    print('PASS production entry observer finds the stock callback on the actual third ancestor; no fixed-parent assumption')

def early_watch_probe(image,elf):
    from unicorn.arm_const import UC_ARM_REG_R2,UC_ARM_REG_R3
    m,s=machine(image,elf)
    root,page,owner=0x20060000,0x20061000,0x20062000
    spec,array,desc=0x20080000,0x20081000,0x20082000
    child_spec,children=0x20083000,0x20084000
    m.put(GRID,VALUES['LAUNCHER_GRID_CLASS']);m.put(GRID+4,root)
    m.put(root+4,page);m.put(page+4,owner)
    m.put(root+8,child_spec);m.put(child_spec,children)
    m.u.mem_write(child_spec+0x30,struct.pack('<H',1));m.put(children,GRID)
    m.put(VALUES['LAUNCHER_GRID_SLOT'],GRID);m.put(VALUES['LAUNCHER_ROOT_SLOT'],root)
    m.put(owner+8,spec);m.put(spec+12,array,1,4,4,0)
    m.put(array,desc);m.put(desc,VALUES['LAUNCHER_POSITION_EVENT'],page,0)
    callbacks=[desc];added=[]
    def sync():
        m.put(spec+16,len(callbacks))
        for i,d in enumerate(callbacks):m.put(array+4*i,d)
    def add(u):
        assert u.reg_read(UC_ARM_REG_R0)==owner
        callback=u.reg_read(UC_ARM_REG_R1);code=u.reg_read(UC_ARM_REG_R2)
        data=u.reg_read(UC_ARM_REG_R3);d=desc+64*len(callbacks)
        m.put(d,callback,data,code);callbacks.append(d);added.append(callback);sync()
        return d
    def remove(u):
        assert u.reg_read(UC_ARM_REG_R0)==owner
        callback=u.reg_read(UC_ARM_REG_R1)
        callbacks[:]=[d for d in callbacks if struct.unpack('<I',m.u.mem_read(d,4))[0]!=callback]
        sync();return 1
    m.hooks[0x0c5883f0]=add;m.hooks[0x0c589570]=remove
    assert m.call(s['test_watch_entry'],GRID)==1
    assert len(added)==1 and len(callbacks)==2
    wrapped=struct.unpack('<I',m.u.mem_read(desc,4))[0]
    assert wrapped!=VALUES['LAUNCHER_POSITION_EVENT']
    assert bytes(m.u.mem_read(GRID+0x89c,4))==bytes(4)
    assert struct.unpack('<I',m.u.mem_read(GRID,4))[0]==VALUES['LAUNCHER_GRID_CLASS']
    # Native rebuild restores its callback and removes the instance observer.
    m.put(desc,VALUES['LAUNCHER_POSITION_EVENT']);callbacks.pop();sync()
    assert m.call(s['test_watch_entry'],GRID)==1
    assert len(added)==2 and len(callbacks)==2
    assert struct.unpack('<I',m.u.mem_read(desc,4))[0]==wrapped
    assert m.call(s['test_watch_entry'],GRID)==1
    assert len(added)==2
    near_array,near_desc=0x20085000,0x20086000
    m.put(child_spec+12,near_array,1,1,4,0)
    m.put(near_array,near_desc);m.put(near_desc,added[0],0,0)
    assert m.call(s['test_motion_parent'],GRID)==owner, 'stale nearer observer must not hide the native callback'
    m.call(s['test_stop_watch'])
    assert callbacks==[desc]
    assert struct.unpack('<III',m.u.mem_read(desc,12))==(VALUES['LAUNCHER_POSITION_EVENT'],page,0)
    print('PASS ARM early watcher leaves unready native grid untouched, repairs lost callbacks without duplicates and restores the original binding on disable')

def rw_reapply_probe(image,elf):
    m,s=machine(image,elf)
    root,spec,children=0x20060000,0x20061000,0x20062000
    m.put(GRID,VALUES['LAUNCHER_GRID_CLASS']);m.put(GRID+4,root)
    m.put(GRID+0x89c,1);m.put(root+8,spec);m.put(spec,children)
    m.u.mem_write(spec+0x30,struct.pack('<H',1));m.put(children,GRID)
    m.put(VALUES['LAUNCHER_GRID_SLOT'],GRID);m.put(VALUES['LAUNCHER_ROOT_SLOT'],root)
    m.put(VALUES['LAUNCHER_SNAPSHOT_SLOT'],0)
    m.u.mem_write(VALUES['LAUNCHER_TRANSITION_SLOT'],b'\1')
    trace=[]
    def layout(u):
        assert u.reg_read(UC_ARM_REG_R0)==GRID and u.reg_read(UC_ARM_REG_R1)==7
        trace.append('layout');return 0
    def refresh(u):
        assert u.reg_read(UC_ARM_REG_R0)==GRID
        # Production RW coefficients must be written BEFORE recomputation.
        assert struct.unpack('<f',m.u.mem_read(GRID+0x1d0,4))[0]!=0
        trace.append('refresh')
        # Model native recomputation/display, as for source/zoom in other probes.
        # The subsequent coordinate commit runs the actual .043 instructions.
        for i,icon in enumerate(ICONS):
            ud=struct.unpack('<I',m.u.mem_read(icon+0x18,4))[0]
            m.put(ud,10+i*20,20,64,64)
        return 0
    m.hooks[VALUES['OBJ_GET_STYLE_VALUE']&~1]=lambda u:7
    m.hooks[VALUES['OBJ_SET_LAYOUT']&~1]=layout
    m.hooks[VALUES['LAUNCHER_GRID_REFRESH']&~1]=refresh
    m.call(s['test_rw_apply'])
    assert trace==[], 'native transition must finish before refreshing'
    m.u.mem_write(VALUES['LAUNCHER_TRANSITION_SLOT'],b'\0')
    m.call(s['test_rw_tick'],0)
    assert trace==['layout','refresh']
    assert rect(m,ICONS[0])==(10,20,73,83)
    assert any(a==0x1c02b304 and obj==GRID for a,obj,_ in m.calls)
    m.call(s['test_rw_tick'],0)
    assert trace==['layout','refresh'], 'stable RW must not refresh each timer turn'
    print('PASS ARM RW reapply writes coefficients before rebinding/recomputation, commits coordinates without scrolling, retries transitions once and idles')

def snapshot_probe(image,elf):
    event,root,page=0x20018000,0x20060000,0x20061000
    for y in (0,480):
        for on in (0,1):
            m,s=machine(image,elf)
            m.put(GRID+4,root);m.put(root+4,page);m.put(page+0x20,y)
            m.put(event,0x20062000,GRID,9,page,0,0,0)
            captures=[];resets=[]
            m.hooks[0x0c1f0d20]=lambda u:0 # native logging
            # Native close-begin resets the live layout before its screenshot;
            # the former post observer can no longer repair those pixels.
            m.call(s['test_render'],GRID,0,0,0)
            def capture(u):
                captures.append((u.reg_read(UC_ARM_REG_R0),centre(rect(m,ICONS[0]))))
            def reset(u):
                resets.append(True);m.put(ICONS[0]+0x1c,0,0,111,111)
            m.hooks[0x0c5176fc]=capture
            m.hooks[0x0c5166f0]=reset
            m.call(s['test_entry_dispatch'],event,on)
            assert len(captures)==1
            assert len(resets)==(1 if not on and y==0 else 0)
            assert captures[0]==(page,(56,56) if not on and y==0 else (168,240))
            assert bytes(m.u.mem_read(VALUES['LAUNCHER_TRANSITION_SLOT'],1))==b'\1'
            if on and y==0:assert bytes(m.u.mem_read(GRID+0x8b8,1))[0]&4
    # Execute the true native SCROLL_END with no snapshot. Time/analytics are
    # modelled; the native path itself must clear its transition latch.
    m,s=machine(image,elf)
    m.put(GRID+4,root);m.put(root+4,page);m.put(page+0x20,0)
    m.put(event,0x20062000,GRID,11,page,0,0,0)
    m.u.mem_write(VALUES['LAUNCHER_TRANSITION_SLOT'],b'\1')
    m.hooks[0x0c1f0d20]=lambda u:0
    m.hooks[0x0c1f1664]=lambda u:0
    m.call(s['test_entry_dispatch'],event,1)
    assert bytes(m.u.mem_read(VALUES['LAUNCHER_TRANSITION_SLOT'],1))==b'\0'
    # A stale native screenshot is a sibling of launcher root under page.
    # Model only deletion/bitmap cleanup, retaining real child membership.
    snapshot,spec,children=0x20067000,0x20068000,0x20069000
    m.put(page+8,spec);m.put(spec,children)
    m.u.mem_write(spec+0x30,struct.pack('<H',2));m.put(children,root,snapshot)
    m.put(VALUES['LAUNCHER_SNAPSHOT_SLOT'],snapshot)
    deleted=[]
    def delete(u):
        deleted.append(u.reg_read(UC_ARM_REG_R0));m.put(VALUES['LAUNCHER_SNAPSHOT_SLOT'],0)
    m.hooks[0x0c588de8]=delete
    assert m.call(s['test_clear_snapshot'],GRID)==1 and deleted==[snapshot]
    m.put(VALUES['LAUNCHER_SNAPSHOT_SLOT'],0x20067100)
    assert m.call(s['test_clear_snapshot'],GRID)==0 and deleted==[snapshot]
    print('PASS old native close captures stock coordinates; production wrapper captures prepared honeycomb without reset at both endpoints, preserves native end/latch and removes only a live owned snapshot')

def native_edit_probe(image):
    from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2
    m=Machine(image)
    event=0x20018000;indev=0x20019000;vtable=0x20072000;meta=0x20073000
    m.put(GRID,VALUES['LAUNCHER_GRID_CLASS']);m.put(GRID+0x89c,1)
    m.put(ICONS[0]+4,GRID);m.put(indev,1)
    m.put(event,GRID,ICONS[0],5,69,indev,0,0)
    base=struct.unpack_from('<I',image,VALUES['LAUNCHER_GRID_CLASS']-0x2c0c0000)[0]
    callback=struct.unpack_from('<I',image,base-0x2c0c0000+12)[0]
    m.hooks[callback&~1]=lambda u:1 # base visuals
    m.hooks[0x0c1f0d20]=lambda u:0 # log
    m.hooks[0x0c4c56cc]=lambda u:0 # haptic
    def memset(u):
        dest=u.reg_read(UC_ARM_REG_R0)
        u.mem_write(dest,bytes([u.reg_read(UC_ARM_REG_R1)&255])*u.reg_read(UC_ARM_REG_R2))
        return dest
    m.hooks[0x0c588980]=memset # ROM memset is not part of the supplied AP image
    entered=[]
    def editor(u):
        assert u.reg_read(UC_ARM_REG_R0)==GRID
        entered.append(True);m.put(GRID+0x89c,0x10001)
    m.hooks[0x0c515de4]=editor # actual edit-layout UI is outside this probe
    m.call(VALUES['LAUNCHER_GRID_EVENT'],VALUES['LAUNCHER_GRID_CLASS'],event)
    assert entered==[True]
    assert bytes(m.u.mem_read(GRID+0x898,1))==b'\1'
    # Execute the real stock app callback in edit mode. Only registry/service
    # dispatch is modelled; the eligibility and delete-request branch is real.
    m.put(VALUES['LAUNCHER_GRID_SLOT'],GRID)
    m.put(event,ICONS[0],ICONS[0],4,69,indev,0,0)
    lookup=0x20071001;dispatch=0x20071101
    m.put(0x200eb640+0x18,vtable);m.put(vtable+4,lookup)
    m.put(0x200eb6e0+4,dispatch)
    m.u.mem_write(meta+0x10,struct.pack('<H',69))
    m.hooks[lookup&~1]=lambda u:meta
    deleted=[]
    def delete_request(u):
        payload=u.reg_read(UC_ARM_REG_R0)
        assert struct.unpack('<I',u.mem_read(payload+4,4))[0]==0x10003
        assert struct.unpack('<H',u.mem_read(payload+12,2))[0]==69
        deleted.append(True)
    m.hooks[dispatch&~1]=delete_request
    m.call(VALUES['LAUNCHER_ICON_CLICK'],event)
    assert deleted==[True]
    m.u.mem_write(meta+0x12,b'\4') # native non-removable app flag
    m.call(VALUES['LAUNCHER_ICON_CLICK'],event)
    assert deleted==[True]
    print('PASS real stock long-press selects editor; app callback requests native deletion only for removable apps')

def probe(image,elf):
    m,s=machine(image,elf)
    for i,(dx,dy) in enumerate(OFFSETS):
        assert m.call(s['test_render'],GRID,i,0,0)==1
        assert centre(rect(m,ICONS[i]))==(168+dx,240+dy)
        assert m.calls[-1][:2]==(0x1c02b304,GRID), 'committed geometry must request redraw'
    initial=[rect(m,icon) for icon in ICONS]
    assert len(set(centre(r) for r in initial))==7
    print('PASS real firmware coords form seven distinct hexagonal app positions')
    for px,py in [(48,83),(-48,-83),(50,0),(0,-60)]:
        assert m.call(s['test_render'],GRID,0,px&0xffffffff,py&0xffffffff)==1
        moved=rect(m,ICONS[0])
        assert centre(moved)==(168+px,240+py)
        # An ordinary subsequent native layout reads the SAME authoritative
        # metadata; changing only an align style cannot satisfy this assertion.
        m.put(ICONS[0]+0x1c,0,0,111,111)
        assert m.call(s['test_frame'],GRID)==1
        assert rect(m,ICONS[0])==moved
    print('PASS production ARM port changes x/y and survives a subsequent native layout pass')
    for zoom in (128,192,256,384):
        assert m.call(s['test_zoom'],GRID,5,zoom)==1
        assert centre(rect(m,ICONS[5]))==(168+96*zoom//256,240)
        assert m.call(s['test_zoom'],GRID,0,zoom)==1
        r=rect(m,ICONS[0])
        assert abs(r[2]-r[0]+1-84*zoom//256)<=1
    print('PASS production ARM zoom scales actual hexagonal spacing and central icon size together from 50% to 150%')
    for zoom in (128,256,384):
        for spacing in (224,256,320,512):
            pitch=zoom*spacing//256
            assert m.call(s['test_spacing'],GRID,5,zoom,spacing)==1
            hidden=struct.unpack('<I',m.u.mem_read(ICONS[5]+0x2c,4))[0]&1
            offset=96*pitch//256
            if offset>=224:
                assert hidden, (zoom,spacing)
            else:
                assert not hidden, (zoom,spacing)
                assert centre(rect(m,ICONS[5]))==(168+offset,240), (zoom,spacing)
            assert m.call(s['test_spacing'],GRID,0,zoom,spacing)==1
            r=rect(m,ICONS[0])
            assert abs(r[2]-r[0]+1-84*zoom//256)<=1
            # A later native layout must retain both drawn and hit-test bounds.
            m.put(ICONS[0]+0x1c,0,0,111,111)
            assert m.call(s['test_frame'],GRID)==1
            assert rect(m,ICONS[0])==r
    print('PASS independent ARM spacing changes actual pitch, preserves central icon size and survives native layout')
    ud=struct.unpack('<I',m.u.mem_read(ICONS[0]+0x18,4))[0]
    m.u.mem_write(ud+0x13,b'\x01');m.put(GRID+0x8b4,5)
    assert m.call(s['test_render'],GRID,0,0,0)==1
    assert bytes(m.u.mem_read(ud+0x13,1))==b'\0'
    assert bytes(m.u.mem_read(GRID+0x8b4,4))==bytes(4)
    assert centre(rect(m,ICONS[0]))==(168,240)
    assert any(a==VALUES['ANIMATION_DELETE']&~1 and obj==ICONS[0] for a,obj,_ in m.calls)
    print('PASS pending native icon animation is cancelled before its geometry gate is cleared')
    assert m.call(s['test_render'],GRID,0,1000,0)==1
    assert struct.unpack('<I',m.u.mem_read(ICONS[0]+0x2c,4))[0]&1
    assert m.call(s['test_render'],GRID,0,0,0)==1
    assert not struct.unpack('<I',m.u.mem_read(ICONS[0]+0x2c,4))[0]&1
    assert centre(rect(m,ICONS[0]))==(168,240)
    print('PASS off-screen icon hides and returns to its actual centre')
    m.put(VALUES['LAUNCHER_GRID_SLOT'],GRID);m.put(GRID+0x89c,1)
    assert m.call(s['test_rw_tick'],1)==1
    x_intercept=GRID+0x84
    assert struct.unpack('<f',m.u.mem_read(x_intercept,4))[0]==54.0
    m.u.mem_write(x_intercept,struct.pack('<f',999.0))
    assert m.call(s['test_rw_tick'],1)==1
    assert struct.unpack('<f',m.u.mem_read(x_intercept,4))[0]==999.0
    assert m.call(s['test_rw_tick'],0)==1
    assert struct.unpack('<f',m.u.mem_read(x_intercept,4))[0]==54.0
    print('PASS actual ARM RW timer resets stock once, then stops repairing tables while honeycomb is selected')
    # Use the REAL visibility-flag helpers this time. Earlier probes replaced
    # these with bitwise models, hiding their parent-layout-dirty side effect.
    del m.hooks[0x1c028bd8]
    del m.hooks[0x1c028c84]
    m.hooks[0x002bca3c]=lambda u:1 # style getter: parent has a registered layout
    m.hooks[0x1c02f078]=lambda u:0 # display lookup, unrelated to layout flags
    m.hooks[0x1c030420]=lambda u:0 # event send, no display in this model
    for _ in range(3):
        m.u.mem_write(GRID+0x32,b'\0')
        m.call(0x1c028c85,ICONS[0],1)
        assert bytes(m.u.mem_read(GRID+0x32,1))[0]&1
    print('REPRODUCED .043: redundant clear-HIDDEN marks parent layout dirty on EVERY pass')
    for _ in range(50):
        m.u.mem_write(GRID+0x32,b'\0')
        assert m.call(s['test_layout'],GRID,0,0,0)==1
        assert not bytes(m.u.mem_read(GRID+0x32,1))[0]&1
    for _ in range(2):
        m.u.mem_write(GRID+0x32,b'\0')
        assert m.call(s['test_render'],GRID,0,0,0)==1
        assert not bytes(m.u.mem_read(GRID+0x32,1))[0]&1
    print('PASS pure geometry callback stays settled; repeated timer preparation avoids redundant visibility setters')
    layout_loop_probe(image,elf)
    navigation_probe(image,elf)
    motion_parent_probe(image,elf)
    early_watch_probe(image,elf)
    rw_reapply_probe(image,elf)
    snapshot_probe(image,elf)
    native_edit_probe(image)
    edit_return_probe(image,elf)
    print('MACHINE-CODE GEOMETRY EVIDENCE ONLY: zoom/invalidation/display are modelled; device acceptance pending.')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',type=Path)
    image=p.parse_args().image.read_bytes();verify(image)
    with tempfile.TemporaryDirectory(prefix='chaos-geometry-') as d:probe(image,build(Path(d)))

if __name__=='__main__':main()
