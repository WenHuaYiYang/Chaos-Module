#!/usr/bin/env python3
"""Production ARM transport + real .043 crypto wrapper, explicit AES/radio/timer seams.
Not a radio throughput benchmark. Does not claim a new Bluetooth L2CAP PSM.
"""
from pathlib import Path
import argparse,struct,subprocess,tempfile
from elftools.elf.elffile import ELFFile
from unicorn.arm_const import UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3,UC_ARM_REG_SP,UC_ARM_REG_LR,UC_ARM_REG_PC
from verify_honeycomb_abi import verify
from probe_honeycomb_abi import Machine,RETURN
from test_chaos_connect import frame
ROOT=Path(__file__).resolve().parents[1]
def main():
    ap=argparse.ArgumentParser();ap.add_argument('image',type=Path);args=ap.parse_args();data=args.image.read_bytes();verify(data);m=Machine(data)
    with tempfile.TemporaryDirectory() as tmp:
        tmp=Path(tmp);objects=[]
        for src in ['supervisor/bridge/bridge.c','supervisor/bridge/chaos_connect.c','supervisor/tests/chaos_connect_arm.c']:
            out=tmp/(Path(src).stem+'.o');objects.append(str(out))
            subprocess.run(['clang','--target=arm-none-eabi','-mcpu=cortex-m33','-mthumb','-mfloat-abi=soft','-ffreestanding','-fno-builtin','-ffunction-sections','-fdata-sections','-O2','-I'+str(ROOT/'supervisor/bridge'),'-c',str(ROOT/src),'-o',str(out)],check=True)
        elf=tmp/'probe.elf';sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip());host=next(x.split(': ',1)[1] for x in subprocess.check_output(['rustc','-vV'],text=True).splitlines() if x.startswith('host:'));lld=sysroot/'lib/rustlib'/host/'bin/rust-lld'
        subprocess.run([str(lld),'-flavor','gnu','-Ttext=0x10000000','--gc-sections','--entry=chaos_bridge_start','-u','chaos_bridge_stop','-u','chaos_bridge_set_enabled','-u','test_dispatches','-u','test_resets','-u','test_chunk','-u','test_android',*objects,'-o',str(elf)],check=True)
        with elf.open('rb') as stream:
            e=ELFFile(stream);pages=set()
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD':pages.update(range(s['p_vaddr']&~4095,(s['p_vaddr']+s['p_memsz']+4095)&~4095,4096))
            for page in pages:m.u.mem_map(page,4096)
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD' and s['p_filesz']:m.u.mem_write(s['p_vaddr'],s.data())
            syms={s.name:s['st_value'] for s in e.get_section_by_name('.symtab').iter_symbols() if s.name}
        def call(addr,*args):
            for reg,value in zip((UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3),(*args,0,0,0,0)):m.u.reg_write(reg,value)
            m.u.reg_write(UC_ARM_REG_SP,0x2001e000);m.u.reg_write(UC_ARM_REG_LR,RETURN|1)
            m.u.emu_start(addr|1,RETURN,count=4000000);assert m.u.reg_read(UC_ARM_REG_PC)==RETURN,hex(m.u.reg_read(UC_ARM_REG_PC));return m.u.reg_read(UC_ARM_REG_R0)
        word=lambda a:struct.unpack('<I',m.u.mem_read(a,4))[0]
        h=0x123;session=0x20058000;crypto=0x20059000;head=0x200e16b8;node=0x2005a000;conn=0x2005b000;buf=0x20060000
        m.put(head,0,node);m.put(node,head,head,0,conn);m.put(conn+12,0xc7d6e6d)
        m.u.mem_write(conn+6,struct.pack('<H',h));m.u.mem_write(conn+0x52,struct.pack('<H',4096))
        m.put(0x20125700,0x2005c000);m.put(0x201259e4,0x2005d000)
        m.hooks[0xc7d7e94]=lambda u:session if u.reg_read(UC_ARM_REG_R0)==h else 0
        m.hooks[0xc587ed0]=lambda u:0x2005e000
        m.hooks[0xc588128]=lambda u:0
        m.hooks[0xc2e3994]=lambda u:0;m.hooks[0xc2e3b84]=lambda u:0
        # SPP adapter/socket lifecycle is exercised in its separate ABI probe
        # and production host harness; this probe isolates the legacy carrier.
        m.hooks[0xc3af1a8]=lambda u:0
        original=[]
        m.hooks[0xc7d6e6c]=lambda u:original.append((u.reg_read(UC_ARM_REG_R1),bytes(m.u.mem_read(u.reg_read(UC_ARM_REG_R3),word(u.reg_read(UC_ARM_REG_SP)))))) or 0
        # Real L2 crypto wrapper calls real AES-CTR wrapper. Only AES primitive
        # and lock calls are replaced; mutating its nonce proves it is a stack copy.
        m.hooks[0xc7d05b4]=lambda u:crypto
        m.u.mem_write(crypto,bytes(range(64))+b'\x01'+bytes(15))
        before=bytes(m.u.mem_read(crypto,64))
        for a in [0xc702828,0xc702850,0xc240de8,0xc240e4c,0xc240df8]:m.hooks[a]=lambda u:0
        def aes(u):
            n=u.reg_read(UC_ARM_REG_R1);sp=u.reg_read(UC_ARM_REG_SP);src=word(sp+4);dst=word(sp+8)
            m.u.mem_write(dst,bytes(x^0xa5 for x in m.u.mem_read(src,n)));m.u.mem_write(u.reg_read(UC_ARM_REG_R3),bytes(16));return 0
        m.hooks[0xc242650]=aes
        attempts=[];fail=[True]
        def send(u):
            p=u.reg_read(UC_ARM_REG_R1);n=struct.unpack('<H',m.u.mem_read(p+8,2))[0];payload=word(p+4)
            assert bytes(m.u.mem_read(p,2))==b'\x01\x02'
            plain=bytes(m.u.mem_read(payload,n));encrypted=bytes(x^0xa5 for x in plain);m.u.mem_write(payload,encrypted);attempts.append((plain,encrypted))
            if fail[0]:fail[0]=False;return 0xffffffff
            return 0
        m.hooks[0xc7d719c]=send
        assert call(syms['chaos_bridge_start'])==0xffffffff # disabled by default
        call(syms['chaos_bridge_set_enabled'],1)
        assert call(syms['chaos_bridge_start'])==0
        call(syms['chaos_bridge_set_enabled'],0);call(syms['boot_tick'])
        assert word(conn+12)==0xc7d6e6d and call(syms['chaos_bridge_start'])==0xffffffff
        call(syms['chaos_bridge_set_enabled'],1)
        assert call(syms['chaos_bridge_start'])==0;call(syms['boot_tick']);call(syms['owner_tick'])
        receive=word(conn+12);assert receive!=0xc7d6e6d
        def packet(f,pid,kind=0,namespace=b'ChaosConnect'):
            # fifth C argument (id) is on the stack, so use Python's independent encoding.
            def v(x):
                out=bytearray()
                while x>=128:out.append((x&127)|128);x>>=7
                out.append(x);return bytes(out)
            def b(tag,p):return v(tag*8+2)+v(len(p))+p
            basic=b(1,namespace)+b(2,b'');message=b(1,basic)+b(2,f)
            return b'\x08'+v(kind)+b'\x10'+v(pid)+b(22,b(9,message))
        def receive_frame(f,pid,ch=1,kind=0,namespace=b'ChaosConnect'):
            p=packet(f,pid,kind,namespace) if ch==1 else f;encrypted=bytes(x^0xa5 for x in p);m.u.mem_write(buf,encrypted)
            # l2 RX's fifth argument is length; emulate caller stack.
            def dispatch(addr):
                for reg,value in zip((UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3),(h,ch,2,buf)):m.u.reg_write(reg,value)
                m.put(0x2001e000,len(encrypted));m.u.reg_write(UC_ARM_REG_SP,0x2001e000);m.u.reg_write(UC_ARM_REG_LR,RETURN|1)
                m.u.emu_start(addr|1,RETURN,count=4000000);assert m.u.reg_read(UC_ARM_REG_PC)==RETURN
            dispatch(receive);return encrypted
        unrelated=receive_frame(b'normal',1,ch=0x43);assert original[-1]==(0x43,unrelated)
        # An unrelated authenticated protobuf must pass through unchanged too.
        p=b'\x08\x02\x10\x00';encrypted=bytes(x^0xa5 for x in p);m.u.mem_write(buf,encrypted)
        for reg,value in zip((UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3),(h,1,2,buf)):m.u.reg_write(reg,value)
        m.put(0x2001e000,len(p));m.u.reg_write(UC_ARM_REG_SP,0x2001e000);m.u.reg_write(UC_ARM_REG_LR,RETURN|1);m.u.emu_start(receive|1,RETURN,count=4000000)
        assert original[-1]==(1,encrypted) and bytes(m.u.mem_read(crypto,64))==before
        # OTHER is a host subscription wildcard, so namespace/CRC checks are essential.
        for kind,namespace in [(0,b'AnotherApp'),(20,b'ChaosConnect'),(0x4343,b'ChaosConnect')]:
            encrypted=receive_frame(frame(1,1),0x12340001,kind=kind,namespace=namespace)
            assert original[-1]==(1,encrypted)
        corrupt=bytearray(frame(1,1));corrupt[-1]^=1
        count=len(original);receive_frame(bytes(corrupt),0x12340001);assert len(original)==count # corrupt private frame is dropped
        call(syms['owner_tick']);assert word(syms['test_dispatches'])==0
        receive_frame(frame(1,1),0x12340001);call(syms['owner_tick']);assert word(syms['test_dispatches'])==1 and word(syms['test_chunk'])==2048
        call(syms['bt_tick']);call(syms['bt_tick']);assert attempts[0]==attempts[1] # retry uses pristine plaintext
        for i in range(8):receive_frame(frame(0x31,i+2,bytes([i])*2048),0x12340002+i)
        before_batch=word(syms['test_dispatches'])
        call(syms['owner_tick']);assert word(syms['test_dispatches'])==before_batch+2
        for _ in range(3):call(syms['owner_tick'])
        call(syms['bt_tick']);assert word(syms['test_dispatches'])==9
        assert bytes(m.u.mem_read(crypto,64))==before
        # Closing is immediate, even before the maintenance stop callback.
        # Queue an already-accepted request and a reply, then close at the UI gate.
        receive_frame(frame(0x31,19,b'queued-reply'),0x12340013);call(syms['owner_tick'])
        receive_frame(frame(0x31,20,b'queued-request'),0x12340014)
        before_dispatch=word(syms['test_dispatches']);before_send=len(attempts)
        call(syms['chaos_bridge_set_enabled'],0)
        receive_frame(frame(1,21),0x12340015) # cached RX callback still installed
        call(syms['owner_tick']);fail[0]=True;call(syms['bt_tick']);call(syms['bt_tick'])
        assert word(syms['test_dispatches'])==before_dispatch
        # Only the stopped event may pass; its retry also uses pristine plaintext.
        assert len(attempts)==before_send+2 and attempts[-1]==attempts[-2]
        stopped=attempts[-1][0];assert b'CCN1'+bytes([1,2,0xc2,0]) in stopped
        call(syms['bt_tick']);assert len(attempts)==before_send+2
        assert call(syms['chaos_bridge_start'])==0xffffffff
        call(syms['boot_tick']);assert word(conn+12)==0xc7d6e6d # late boot cannot re-enable
        call(syms['chaos_bridge_set_enabled'],1);call(syms['chaos_bridge_start']);call(syms['owner_tick']);call(syms['bt_tick'])
        assert word(syms['test_dispatches'])==before_dispatch
        receive_frame(frame(0x31,22,b'no-hello'),0x56780016);call(syms['owner_tick'])
        assert word(syms['test_dispatches'])==before_dispatch
        receive_frame(frame(1,23),0x56780017);call(syms['owner_tick']);call(syms['bt_tick'])
        assert word(syms['test_dispatches'])==before_dispatch+1
        # Subsequent off/on cycles must also invalidate the old epoch.
        baseline=word(syms['test_dispatches']);receive_frame(frame(0x31,24,b'old'),0x56780018);call(syms['chaos_bridge_stop']);assert word(conn+12)==0xc7d6e6d
        call(syms['chaos_bridge_set_enabled'],1);call(syms['chaos_bridge_start']);call(syms['owner_tick']);call(syms['bt_tick']);assert word(syms['test_dispatches'])==baseline
        call(syms['chaos_bridge_stop'])
        # A third-party handler is never overwritten.
        m.put(conn+12,0x10070001);call(syms['chaos_bridge_set_enabled'],1);call(syms['chaos_bridge_start']);call(syms['owner_tick']);assert word(conn+12)==0x10070001
        # Standard SPP uses the same real ARM stream parser and owner queue.
        # Even an empty legacy HELLO is forced to Android by physical provenance.
        raw_pipe=0x2005f000
        m.put(syms['radio_pipe'],raw_pipe);m.put(syms['radio_ready'],1);m.put(syms['radio_generation'],71)
        raw=packet(frame(1,1),0x9abc0001);stream=struct.pack('<I',len(raw))+raw
        m.u.mem_write(buf,stream);before_raw=word(syms['test_dispatches'])
        for at in range(len(stream)):call(syms['radio_read'],raw_pipe,buf+at,1)
        call(syms['owner_tick'])
        assert word(syms['test_dispatches'])==before_raw+1 and word(syms['test_android'])==1
        raw_writes=[]
        m.hooks[0xc3a9bfc]=lambda u:raw_writes.append(bytes(m.u.mem_read(u.reg_read(UC_ARM_REG_R1),u.reg_read(UC_ARM_REG_R2)))) or 0
        call(syms['bt_tick']);assert len(raw_writes)==1
        record=raw_writes[0];assert struct.unpack_from('<I',record)[0]==len(record)-4 and b'CCN1' in record[4:]
        call(syms['radio_written'],raw_pipe,0,0)
        call(syms['chaos_bridge_set_enabled'],0)
        m.u.mem_write(buf,stream);call(syms['radio_read'],raw_pipe,buf,len(stream));call(syms['owner_tick'])
        assert word(syms['test_dispatches'])==before_raw+1
        print('PASS production ARM SPP stream/owner queue, physical Android provenance, framed reply, immediate disabled gate')
        # Execute the actual .043 msgbox factory/constructor used by the consent
        # overlay. Only lower LVGL allocation/style/list primitives are seams.
        # mode=0 must skip page lifecycle attachment (0xc4c9eb4's null branch),
        # so a display-top container is not interpreted as a page descriptor.
        parent,modal,holder=0x20065000,0x20066000,0x20067000
        canary=bytes(range(128));m.u.mem_write(parent,canary)
        m.hooks[0xc5891f8]=lambda u:modal
        m.hooks[0xc587d40]=lambda u:0
        m.hooks[0xc5891a8]=lambda u:holder
        for a in [0xc588cb8,0xc5883f8,0xc589448]:m.hooks[a]=lambda u:0
        assert call(0xc4ca0d8,parent,0)==modal
        assert word(holder)==modal and bytes(m.u.mem_read(parent,128))==canary
        # Real screen_turn_on(tag, source): exercise its OFF-state dispatch and
        # prove the stock wakelock source 26 + tag reach the display-owner helper.
        screen_global=word(0xca73280);m.put(screen_global+0x1bc,0)
        tag=0x20068000;m.u.mem_write(tag,b'chaos-connect\0');wake=[]
        m.hooks[0xca6ecd8]=lambda u:0
        m.hooks[0xca717b8]=lambda u:wake.append((u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1))) or 0
        call(0xca7323c,tag,26);assert wake==[(tag,26)]
        assert data[0xca6e9fc-0xc0c0000:0xca6e9fe-0xc0c0000]==b'\x1a\x21' # stock wakelock caller movs r1,#26
        # Actual stock row update: a1/primary NULL, secondary populated. Verify
        # a refresh changes only the secondary label, never the main title.
        row,primary,secondary,status=0x20069000,0x2006a000,0x2006b000,0x2006c000
        m.u.mem_write(row,bytes(128));m.put(row+0x3c,primary,secondary)
        m.u.mem_write(status,"已连接\0".encode());updated=[]
        m.hooks[0xc589490]=lambda u:updated.append((u.reg_read(UC_ARM_REG_R0),u.reg_read(UC_ARM_REG_R1))) or 0
        m.hooks[0xc5891b0]=lambda u:0
        m.put(0x2001e000,0,0)
        call(0xc4c8904,row,0,0,status)
        assert updated==[(secondary,status)],updated
        print('PASS real .043 row refresh preserves primary and updates secondary only')
        print('PASS real .043 screen wake dispatcher and stock source-26 caller; display-owner helper is a seam')
        print('PASS real .043 system msgbox mode-0 constructor without page descriptor mutation; LVGL allocation/style primitives are seams')
        print('PASS production ARM hook/codec, ordinary encrypted/OTHER namespace passthrough, corrupt-private rejection, real stateless crypto wrapper, 8-frame window, immutable retry, immediate off gate/late-boot rejection/stopped notice/new-HELLO requirement, epoch cancellation and foreign-hook preservation')
        print('HOST ONLY: AES/radio/timers/file engine are explicit seams; actual Bluetooth throughput requires hardware')
if __name__=='__main__':main()
