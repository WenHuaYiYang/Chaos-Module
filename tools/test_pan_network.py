#!/usr/bin/env python3
"""Real ARM lwIP/TinyPAN with a virtual Ethernet NAP, plus control regressions.

Requires clang, rust-lld or ld.lld, unicorn and pyelftools. No firmware or
hardware is involved. DHCP/DNS/TCP responses are wire packets, not mocked
success returns. BT scan/SDP doubles are used only by the command tests.
"""
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC
from elftools.elf.elffile import ELFFile

ROOT=Path(__file__).resolve().parents[1]
NW=ROOT/'supervisor/network'
LOCAL=bytes.fromhex('020000000001');REMOTE=bytes.fromhex('020000000002')
CLIENT=bytes([192,168,44,2]);GATEWAY=bytes([192,168,44,1])
HOST=bytes([93,184,216,34]);DNS=bytes([10,23,45,67])
RETURN=0x201FF000;STACK=0x201F0000
REGS=[UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3]

def checksum(data):
    if len(data)&1:data+=b'\0'
    n=sum(struct.unpack('!%dH'%(len(data)//2),data))
    while n>>16:n=(n&65535)+(n>>16)
    return (~n)&65535

def ip(source,destination,protocol,payload):
    h=bytearray(struct.pack('!BBHHHBBH4s4s',0x45,0,20+len(payload),1,0,64,protocol,0,source,destination))
    struct.pack_into('!H',h,10,checksum(bytes(h)));return bytes(h)+payload

def udp(source,destination,sport,dport,data):
    p=bytearray(struct.pack('!HHHH',sport,dport,8+len(data),0)+data)
    c=checksum(source+destination+bytes([0,17])+struct.pack('!H',len(p))+p)
    struct.pack_into('!H',p,6,c or 65535);return ip(source,destination,17,bytes(p))

def tcp(sport,dport,seq,ack,flags,data=b'',source=HOST,destination=CLIENT):
    p=bytearray(struct.pack('!HHIIBBHHH',sport,dport,seq,ack,0x50,flags,8192,0,0)+data)
    struct.pack_into('!H',p,16,checksum(source+destination+bytes([0,6])+struct.pack('!H',len(p))+p))
    return ip(source,destination,6,bytes(p))

def ethernet(payload,kind=0x800,destination=LOCAL):
    return destination+REMOTE+struct.pack('!H',kind)+payload

def build(out):
    linker=shutil.which('ld.lld')
    if not linker:
        sysroot=Path(subprocess.check_output(['rustc','--print','sysroot'],text=True).strip())
        linker=next(sysroot.glob('lib/rustlib/*/bin/rust-lld'),None)
    if not linker:raise RuntimeError('ld.lld or rust-lld is required')
    flags=['--target=thumbv8m.main-none-eabi','-mcpu=cortex-m33','-mthumb','-mfloat-abi=soft',
        '-ffreestanding','-fno-builtin','-fno-stack-protector','-Os','-Wall','-Wextra','-Werror',
        '-I'+str(NW/'include'),'-I'+str(NW/'ports/libc'),'-I'+str(NW/'ports/lwip'),
        '-I'+str(NW/'third_party/lwip/src/include'),'-include',str(NW/'include/tinypan_port.h')]
    inputs=[NW/'src'/f'{n}.c' for n in ('pan_ip','pan_ethernet','pan_libc','nap_sdp','network_sdp')]
    inputs += [NW/'third_party/tinypan/src/tinypan_bnep.c',ROOT/'supervisor/tests/pan_wire_harness.c']
    lwip=NW/'third_party/lwip/src'
    inputs += [lwip/'core'/f'{v}.c' for v in 'init def dns inet_chksum ip mem memp netif pbuf stats sys tcp tcp_in tcp_out timeouts udp'.split()]
    inputs += [lwip/'core/ipv4'/f'{v}.c' for v in 'acd dhcp etharp icmp ip4 ip4_addr'.split()]+[lwip/'netif/ethernet.c']
    objects=[]
    for i,path in enumerate(inputs):
        obj=out/f'{i}.o';objects.append(str(obj))
        # The command tests use scan/SDP doubles, while SdpTests below execute
        # this actual transport state machine against modelled BT ownership.
        rename=['-Dnetwork_sdp_'+name+'=real_network_sdp_'+name for name in ('start','poll','result','cancel')] if path.name=='network_sdp.c' else []
        subprocess.run(['clang',*flags,*rename,'-c',str(path),'-o',str(obj)],check=True)
    target=out/'pan.elf'
    subprocess.run([str(linker),'-flavor','gnu','-Ttext=0x10000000','--entry=test_boot','-o',str(target),*objects],check=True)
    return target

class Network:
    def __init__(self,elf,mode='echo'):
        self.mode=mode;self.now=0;self.sent=[];self.http=b'';self.peer_seq=1001;self.pending=[];self.dhcp=[];self.syns=0;self.queries=0
        self.u=Uc(UC_ARCH_ARM,UC_MODE_THUMB|UC_MODE_MCLASS)
        self.u.mem_map(0x20000000,0x200000);self.u.mem_write(RETURN,b'\x00\xbe')
        self.u.mem_map(0x0C1F5000,4096);self.allow_assert=False;self.fault_sleeps=0
        with elf.open('rb') as f:
            e=ELFFile(f);pages=set()
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD':pages.update(range(s['p_vaddr']&~4095,(s['p_vaddr']+s['p_memsz']+4095)&~4095,4096))
            for p in pages:self.u.mem_map(p,4096)
            for s in e.iter_segments():
                if s['p_type']=='PT_LOAD':self.u.mem_write(s['p_vaddr'],s.data())
            self.syms={s.name:s['st_value'] for s in e.get_section_by_name('.symtab').iter_symbols() if s.name}
        self.u.hook_add(UC_HOOK_CODE,self.hook)
        self.call('test_boot')
    def hook(self,_u,address,_size,_data):
        if address==RETURN:self.u.emu_stop()
        if address==(self.syms['pan_lwip_assert']&~1) and not self.allow_assert:raise AssertionError('lwIP: '+self.string(self.u.reg_read(UC_ARM_REG_R0)))
        if address==0x0C1F5D0C:
            assert self.allow_assert and self.u.reg_read(UC_ARM_REG_R0)==1000000,'assert must yield with audited usleep'
            self.fault_sleeps+=1;self.u.reg_write(UC_ARM_REG_PC,RETURN);self.u.emu_stop()
    def string(self,address):
        data=bytearray()
        while len(data)<8192:
            b=self.u.mem_read(address+len(data),1)[0]
            if not b:return data.decode('utf-8','replace')
            data.append(b)
        raise AssertionError('unterminated string')
    def call(self,name,*args):return self.invoke(self.syms[name],*args)
    def invoke(self,address,*args):
        for reg in REGS:self.u.reg_write(reg,0)
        self.u.reg_write(UC_ARM_REG_SP,STACK);self.u.reg_write(UC_ARM_REG_LR,RETURN|1)
        for i,value in enumerate(args):
            if i<4:self.u.reg_write(REGS[i],value&0xffffffff)
            else:self.u.mem_write(STACK+(i-4)*4,struct.pack('<I',value&0xffffffff))
        self.u.emu_start(address|1,RETURN,count=5000000)
        assert self.u.reg_read(UC_ARM_REG_PC)==RETURN,'instruction budget: '+hex(address)
        n=self.u.reg_read(UC_ARM_REG_R0);return n if n<0x80000000 else n-0x100000000
    def word(self,name,offset=0):return struct.unpack('<I',self.u.mem_read(self.syms[name]+offset,4))[0]
    def setword(self,name,value):self.u.mem_write(self.syms[name],struct.pack('<I',value))
    def inject(self,frame):self.u.mem_write(self.syms['test_wire'],frame);self.call('test_input',len(frame))
    def ip_status(self):self.call('test_ip_status');return self.string(self.syms['test_read_buffer'])
    def step(self,amount=100):
        self.now+=amount
        for _ in range(min(4,len(self.pending))):self.inject(self.pending.pop(0))
        self.call('test_step',self.now)
        for _ in range(64):
            n=self.call('test_pop')
            if not n:break
            self.packet(bytes(self.u.mem_read(self.syms['test_wire'],n)))
    def packet(self,p):
        assert p[0]==0,'bridge must emit general Ethernet BNEP'
        e=p[1:];self.sent.append(e)
        if self.mode=='blackhole':return
        kind=int.from_bytes(e[12:14],'big')
        if kind==0x806:
            if e[38:42]==GATEWAY:
                reply=struct.pack('!HHBBH',1,0x800,6,4,2)+REMOTE+GATEWAY+LOCAL+CLIENT
                self.pending.append(ethernet(reply,0x806))
            return
        if kind!=0x800:return
        v=e[14:];h=(v[0]&15)*4;total=int.from_bytes(v[2:4],'big');assert checksum(v[:h])==0
        source,destination=v[12:16],v[16:20];body=v[h:total]
        if v[9]==17:
            sport,dport,n,_=struct.unpack('!HHHH',body[:8]);data=body[8:n]
            assert checksum(source+destination+b'\0\x11'+struct.pack('!H',n)+body[:n])==0
            if dport==67:
                options=data[240:];index=0;kind=0
                while index<len(options) and options[index]!=255:
                    tag=options[index];index+=1
                    if not tag:continue
                    size=options[index];index+=1
                    if tag==53:kind=options[index]
                    index+=size
                self.dhcp.append(kind);assert data[28:34]==LOCAL
                reply=bytearray(240);reply[:4]=bytes([2,1,6,0]);reply[4:8]=data[4:8];reply[16:20]=CLIENT;reply[20:24]=GATEWAY
                reply[28:34]=LOCAL;reply[236:240]=bytes.fromhex('63825363')
                reply+=bytes([53,1,2 if kind==1 else 5,54,4])+GATEWAY+bytes([51,4])+struct.pack('!I',3600)
                reply+=bytes([1,4,255,255,255,0,3,4])+GATEWAY
                if self.mode!='no-dhcp-dns':reply+=bytes([6,4])+DNS
                reply+=bytes([255])
                self.pending.append(ethernet(udp(GATEWAY,b'\xff'*4,67,68,bytes(reply)),destination=b'\xff'*6))
            elif dport==53:
                self.queries+=1
                reply=bytearray(data);reply[2:4]=bytes.fromhex('8180');reply[6:8]=b'\0\1'
                reply+=bytes.fromhex('c00c000100010000003c0004')+HOST
                packet=bytearray(ethernet(udp(GATEWAY if self.mode=='no-dhcp-dns' else DNS,CLIENT,53,sport,bytes(reply))))
                if self.mode=='bad-dns' and self.queries==1:packet[-1]^=1
                self.pending.append(bytes(packet))
        elif v[9]==6:
            assert checksum(source+destination+b'\0\x06'+struct.pack('!H',len(body))+body)==0
            if self.mode=='listen':return
            sport,dport,seq,ack=struct.unpack('!HHII',body[:12]);flags=body[13];data=body[(body[12]>>4)*4:]
            if flags&2:
                self.syns+=1
                if self.mode=='drop-syn' and self.syns==1:return
                self.pending.append(ethernet(tcp(dport,sport,1000,seq+1,0x14 if self.mode=='refused' else 0x12)))
            elif data:
                self.http+=data
                self.pending.append(ethernet(tcp(dport,sport,self.peer_seq,seq+len(data),0x10)))
                response=b'PONG' if self.mode=='echo' else b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK'
                for pos in range(0,len(response),5):
                    part=response[pos:pos+5];self.pending.append(ethernet(tcp(dport,sport,self.peer_seq,seq+len(data),0x18,part)));self.peer_seq+=len(part)
    def lease(self):
        for _ in range(150):
            self.step()
            if self.call('pan_ip_ready'):return
        raise AssertionError(self.ip_status()+' DHCP '+repr(self.dhcp))
    def connect(self,host=b'example.com',port=80):
        p=self.syms['test_read_buffer'];self.u.mem_write(p,host+b'\0');handle=self.call('pan_tcp_open',p,port)
        assert handle>0
        for _ in range(200):
            self.step()
            if self.call('pan_tcp_state',handle) in (3,5):return handle
        raise AssertionError('TCP handshake timed out')

class Sdp(Network):
    """Actual production SDP C; BT owner queue/radio callbacks are doubles."""
    def __init__(self,elf):
        self.native_hooks={}
        super().__init__(elf)
        self.u.mem_map(0x0C7E0000,0x70000)
        self.u.mem_write(0x2013BDBC,struct.pack('<I',0x200C0000))
        self.allocations={};self.freed=set();self.heap=0x20080000;self.fail_alloc=False
        self.work=[];self.connects=[];self.disconnects=[];self.transmitted=[];self.callback=0
        self.native_hooks={0x0C840C54:lambda:self.alloc(self.reg(0)),0x0C840C60:self.free,
            0x0C7EB9CC:self.queue,0x0C805B0C:self.connect_native,
            0x0C805BBC:self.disconnect_native,0x0C805BE8:self.send_native}
    def reg(self,n):return self.u.reg_read(REGS[n])
    def hook(self,u,address,size,data):
        callback=self.native_hooks.get(address)
        if callback:
            result=callback();self.u.reg_write(UC_ARM_REG_R0,result&0xffffffff);self.u.reg_write(UC_ARM_REG_PC,self.u.reg_read(UC_ARM_REG_LR));return
        super().hook(u,address,size,data)
    def alloc(self,n):
        if self.fail_alloc:return 0
        p=self.heap;self.heap+=(n+31)&~15;self.allocations[p]=n;self.u.mem_write(p,bytes(n)+b'\xa5'*16);return p
    def release(self,p):
        assert p in self.allocations and p not in self.freed,'invalid/double Bluetooth free'
        assert bytes(self.u.mem_read(p+self.allocations[p],16))==b'\xa5'*16,'Bluetooth allocation overflow'
        self.freed.add(p)
    def free(self):self.release(self.reg(0));return 0
    def queue(self):
        assert self.reg(0)==0x200C0000
        flag=struct.unpack('<I',self.u.mem_read(self.u.reg_read(UC_ARM_REG_SP),4))[0];assert flag==0x71
        self.work.append((self.reg(1),self.reg(2),self.reg(3)));return 0
    def drain(self,cancel=False):
        while self.work:
            run,reject,arg=self.work.pop(0);self.invoke(reject if cancel else run,0 if cancel else 0x200C0000,221 if cancel else 0x71,arg)
    def connect_native(self):
        p=self.reg(0);assert self.allocations[p]==68
        record=bytes(self.u.mem_read(p,68));assert struct.unpack_from('<H',record,2)[0]==1
        assert struct.unpack_from('<HH',record,52)==(1691,1) and record[8]==0
        self.callback=struct.unpack_from('<I',record,12)[0];self.connects.append(record);self.release(p);return 1
    def disconnect_native(self):
        p=self.reg(0);assert self.allocations[p]==4
        self.disconnects.append(struct.unpack('<H',self.u.mem_read(p,2))[0]);self.release(p);return 1
    def send_native(self):
        p=self.reg(0);end,offset=struct.unpack('<HH',self.u.mem_read(p,4))
        assert self.allocations[p]==end+6 and offset==16
        self.transmitted.append((self.reg(1),bytes(self.u.mem_read(p+4+offset,end-offset))))
        self.release(p);return 1
    def start(self):
        p=self.syms['test_read_buffer'];self.u.mem_write(p,bytes.fromhex('665544332211'));return self.call('real_network_sdp_start',p)
    def poll(self):return self.call('real_network_sdp_poll')
    def result(self):return self.call('real_network_sdp_result')
    def event(self,event,data):
        p=self.alloc(len(data));self.u.mem_write(p,data);self.invoke(self.callback,event,p);assert p in self.freed
    def opened(self,cid=0x44,mtu=1691):
        self.event(2,struct.pack('<H',cid));record=bytearray(120);struct.pack_into('<H',record,108,cid);struct.pack_into('<H',record,72,mtu);self.event(3,bytes(record))
    def received(self,packet,cid=0x44):
        record=bytearray(20+len(packet));struct.pack_into('<HHI',record,0,16+len(packet),16,cid);record[20:]=packet;self.event(7,bytes(record))
    def closed(self,cid=0x44):self.event(6,struct.pack('<HH',cid,0))

class PanTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.work=tempfile.TemporaryDirectory(prefix='chaos-pan-wire-');cls.elf=build(Path(cls.work.name))
    @classmethod
    def tearDownClass(cls):cls.work.cleanup()
    def network(self,mode='echo'):return Network(self.elf,mode)
    def test_dhcp_snapshot_preserves_real_fields_and_missing_dns(self):
        for mode in ('echo','no-dhcp-dns'):
            with self.subTest(mode=mode):
                m=self.network(mode);m.lease();p=m.syms['test_read_buffer'];m.call('pan_ip_info',p);raw=bytes(m.u.mem_read(p,28))
                self.assertEqual(raw[:12],CLIENT+bytes([255,255,255,0])+GATEWAY)
                self.assertEqual(raw[12:16],bytes(4) if mode=='no-dhcp-dns' else DNS)
                self.assertEqual(raw[16:22],LOCAL);self.assertEqual(struct.unpack_from('<HI',raw,22),(1500,1))
                m.call('test_down',m.now+1);m.call('pan_ip_info',p);self.assertEqual(bytes(m.u.mem_read(p,28)),bytes(28))
    def test_dns_and_tcp_http_roundtrip(self):
        m=self.network('http');m.lease();h=m.connect();p=m.syms['test_read_buffer']
        request=b'GET / HTTP/1.0\r\nHost: example.com\r\n\r\n';m.u.mem_write(p,request)
        self.assertEqual(m.call('pan_tcp_send',h,p,len(request)),len(request))
        for _ in range(20):m.step()
        n=m.call('pan_tcp_receive',h,p,4096)
        self.assertEqual(bytes(m.u.mem_read(p,n)),b'HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK')
        self.assertEqual(m.http,request);self.assertEqual(m.syns,1);self.assertEqual(m.queries,1)
        self.assertIn(1,m.dhcp);self.assertIn(3,m.dhcp)
    def test_echo_and_link_loss_invalidate_handle(self):
        m=self.network();m.lease();h=m.connect(b'93.184.216.34',8080);p=m.syms['test_read_buffer']
        m.u.mem_write(p,b'PING');self.assertEqual(m.call('pan_tcp_send',h,p,4),4)
        for _ in range(10):m.step()
        self.assertEqual(m.call('pan_tcp_receive',h,p,16),4);self.assertEqual(bytes(m.u.mem_read(p,4)),b'PONG')
        m.call('test_down',m.now+1);self.assertEqual(m.call('pan_ip_ready'),0);self.assertEqual(m.call('pan_tcp_send',h,p,1),-9)
    def test_inbound_tcp_accepts_and_reads_real_packets(self):
        m=self.network();m.lease();m.mode='listen';listener=m.call('pan_tcp_listen',80);self.assertEqual(listener,1)
        m.inject(ethernet(tcp(55000,80,1234,0,2,source=GATEWAY)))
        for _ in range(5):m.step()
        responses=[e[34:] for e in m.sent if e[12:14]==b'\x08\x00' and e[23]==6];reply=responses[-1]
        sport,dport,seq,ack=struct.unpack('!HHII',reply[:12]);self.assertEqual((sport,dport,ack,reply[13]&0x12),(80,55000,1235,0x12))
        m.inject(ethernet(tcp(55000,80,1235,seq+1,0x10,source=GATEWAY)));m.step();h=m.call('pan_tcp_accept',listener);self.assertGreater(h,0)
        p=m.syms['test_read_buffer'];self.assertEqual(m.call('pan_tcp_peer',h,p,p+8),0);self.assertEqual(bytes(m.u.mem_read(p,4)),GATEWAY)
        m.inject(ethernet(tcp(55000,80,1235,seq+1,0x18,b'GET ',source=GATEWAY)));m.step()
        self.assertEqual(m.call('pan_tcp_receive',h,p,16),4);self.assertEqual(bytes(m.u.mem_read(p,4)),b'GET ')
        self.assertEqual(m.call('pan_tcp_finish',h),0);m.step();m.call('pan_tcp_unlisten',listener)
    def test_listener_capacity_duplicate_and_reconnect_cleanup(self):
        m=self.network();m.lease()
        for i,port in enumerate((80,8080,443,9000)):self.assertEqual(m.call('pan_tcp_listen',port),i+1)
        self.assertEqual(m.call('pan_tcp_listen',80),-98);self.assertEqual(m.call('pan_tcp_listen',9001),-98)
        m.call('test_down',m.now+1);m.lease();self.assertEqual(m.call('pan_tcp_listen',80),1)
    def test_reconnect_reclaims_dhcp_and_tcp_allocations(self):
        m=self.network()
        for _ in range(24):m.lease();self.assertEqual(m.ip_status(),'IP 192.168.44.2');m.call('test_down',m.now+1)
    def test_lost_syn_and_corrupt_dns_retransmit(self):
        for mode in ('drop-syn','bad-dns'):
            with self.subTest(mode=mode):
                m=self.network(mode);m.lease();h=m.connect();self.assertEqual(m.call('pan_tcp_state',h),3)
                self.assertGreaterEqual(m.syns if mode=='drop-syn' else m.queries,2)
    def test_tcp_refusal_and_no_lease_are_not_success(self):
        m=self.network('refused');m.lease();h=m.connect();self.assertEqual(m.call('pan_tcp_state',h),5);self.assertEqual(m.call('pan_tcp_error',h),104)
        m=self.network('blackhole')
        for _ in range(70):m.step(1000)
        self.assertEqual(m.call('pan_ip_ready'),0);p=m.syms['test_read_buffer'];m.u.mem_write(p,b'example.com\0');self.assertEqual(m.call('pan_tcp_open',p,80),-100)
    def test_six_slots_have_unique_generation_handles(self):
        m=self.network();m.lease();m.mode='blackhole';p=m.syms['test_read_buffer'];m.u.mem_write(p,b'93.184.216.34\0')
        handles=[m.call('pan_tcp_open',p,80) for _ in range(6)];self.assertTrue(all(h>0 for h in handles));self.assertEqual(len(set(handles)),6)
        self.assertEqual(m.call('pan_tcp_open',p,80),-24)
        old=handles[0];m.call('pan_tcp_close',old);new=m.call('pan_tcp_open',p,80);self.assertNotIn(new,handles);self.assertEqual(m.call('pan_tcp_send',old,p,1),-9)
    def test_enable_scan_is_coalesced_and_disable_preempts_commands(self):
        m=self.network();m.call('test_network_poll');self.assertEqual(m.word('test_view',592),0)
        m.call('network_enable',1);m.call('network_scan');m.call('test_network_poll');m.call('test_network_poll')
        self.assertEqual(m.word('test_scans'),1);self.assertEqual(m.word('test_view',28),0)
        for _ in range(30):m.call('network_scan');m.call('network_connect',0);m.call('network_pair_confirm')
        m.call('network_enable',0);m.call('test_network_poll')
        self.assertEqual(m.word('test_view',592),0);self.assertEqual(m.word('test_view'),0);self.assertEqual(m.word('test_disconnects'),1)
        for _ in range(12):m.call('test_network_poll')
        self.assertEqual(m.word('test_connects'),0);self.assertEqual(m.word('test_scans'),1)
    def test_all_sixteen_nap_peers_selection_and_pair_confirmation(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll');m.call('test_scan_done',16)
        for _ in range(18):m.call('test_network_poll')
        self.assertEqual(m.word('test_view',4),16);self.assertEqual(m.word('test_sdp_starts'),16)
        m.call('network_connect',15);m.call('test_network_poll');self.assertEqual(m.word('test_connected_index'),15)
        m.call('test_pairing',42);m.call('test_network_poll');self.assertEqual(m.word('test_view',20),42)
        m.call('network_pair_confirm');m.call('test_network_poll');self.assertEqual(m.word('test_confirms'),1)
    def test_missing_radio_address_reports_97_instead_of_dhcp_timeout(self):
        # 地址没就绪时 pan_ip_poll 的 link 条件不成立, DHCP 一个包都没发过;
        # 报 -110 会误导成"手机没给 IP"。这条钉住它与真 DHCP 超时分开。
        m=self.network();m.call('network_enable',1);m.call('test_network_poll')
        m.call('test_scan_done',1);m.call('test_network_poll');m.call('test_network_poll')
        m.call('network_connect',0);m.call('test_network_poll')
        m.call('test_linked',0x44);m.call('test_network_poll')
        m.setword('test_radio_ready',0)
        for _ in range(245):m.call('test_network_poll')
        self.assertEqual(m.word('test_view',28),(-97)&0xffffffff)
        self.assertEqual(m.word('test_view',32),6)
        self.assertEqual(m.word('test_disconnects'),1)
    def test_saved_peer_connects_without_inquiry(self):
        # 记住的对端直连: 不经过扫描, 因此不要求手机处于"可被发现"状态。
        m=self.network();m.call('network_enable',1);m.call('test_network_poll')
        m.call('test_scan_done',0);m.call('test_network_poll');m.call('test_network_poll')
        self.assertEqual(m.call('test_connect_saved'),0)
        self.assertEqual(m.word('test_address_connects'),0)  # 命令只入队, 由 worker 执行
        m.call('test_network_poll')
        self.assertEqual(m.word('test_address_connects'),1)
        self.assertEqual(m.word('test_connected_address'),0x44332211)
        self.assertEqual(m.word('test_view',72),0x44332211)
        self.assertEqual(m.word('test_view',40),0x6e6f6870)  # "phon"
        self.assertEqual(m.word('test_view',44),0x65)        # "e"
        self.assertEqual(m.word('test_connects'),0)  # 没走"选设备"那条路
    def test_saved_peer_refused_while_scanning(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll')
        m.call('network_scan');m.call('test_network_poll')
        self.assertEqual(m.call('test_connect_saved'),-16)
        for _ in range(4):m.call('test_network_poll')
        self.assertEqual(m.word('test_address_connects'),0)
    def test_disconnect_cancels_sdp_and_pending_selection(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll');m.call('test_scan_done',16);m.call('test_network_poll');m.call('test_network_poll')
        m.setword('test_sdp_hold',1);m.call('network_connect',0);m.call('test_network_poll');m.call('network_disconnect');m.call('test_network_poll')
        for _ in range(10):m.call('test_network_poll')
        self.assertEqual(m.word('test_sdp_cancels'),1);self.assertEqual(m.word('test_sdp_pending'),0);self.assertEqual(m.word('test_connects'),0);self.assertEqual(m.word('test_view'),0)
    def test_disconnect_preserves_latest_unprocessed_enable_target(self):
        for sequence,wanted in [([1],1),([1,0,1],1),([1,0],0),([0,1,0,1],1)]:
            with self.subTest(sequence=sequence):
                m=self.network()
                for target in sequence:m.call('network_enable',target)
                m.call('network_scan');m.call('network_connect',0);m.call('network_disconnect')
                for _ in range(12):m.call('test_network_poll')
                self.assertEqual(m.word('test_view',592),wanted);self.assertEqual(m.word('test_view'),0)
                self.assertEqual(m.word('test_scans'),0);self.assertEqual(m.word('test_connects'),0);self.assertEqual(m.word('test_disconnects'),1)
    def test_disable_then_reenable_survives_pending_disconnect(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll');m.call('network_enable',0)
        m.call('network_enable',1);m.call('network_disconnect');m.call('test_network_poll')
        self.assertEqual(m.word('test_view',592),1);self.assertEqual(m.word('test_view'),0);self.assertEqual(m.word('test_disconnects'),1)
        m.call('network_scan');m.call('test_network_poll');self.assertEqual(m.word('test_scans'),2)
    def test_repeat_enable_does_not_restart_completed_scan(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll');m.call('test_scan_done',0);m.call('test_network_poll')
        m.call('network_enable',1);m.call('test_network_poll');self.assertEqual(m.word('test_scans'),1);self.assertEqual(m.word('test_view'),0)
    def test_full_command_ring_reports_busy_and_disable_still_wins(self):
        m=self.network();m.call('network_enable',1);m.call('test_network_poll');m.call('test_fill_command_queue')
        self.assertEqual(m.call('test_command_queue_count'),8);m.call('network_scan');m.call('test_network_poll')
        self.assertEqual(m.word('test_view',28),0xfffffff0);self.assertEqual(m.word('test_view',24),1)
        m.call('network_enable',0);self.assertEqual(m.call('test_command_queue_count'),1);m.call('test_network_poll')
        self.assertEqual(m.word('test_view',592),0);self.assertEqual(m.word('test_view',28),0);self.assertEqual(m.word('test_disconnects'),1)
    def test_assert_quarantines_worker_yields_cpu_and_is_ui_visible(self):
        m=self.network();m.lease();m.call('network_enable',1);m.call('test_network_poll')
        p=m.syms['test_read_buffer'];m.u.mem_write(p,b'test invariant\0');m.allow_assert=True;m.call('pan_lwip_assert',p)
        self.assertEqual(m.fault_sleeps,1);self.assertEqual(m.call('pan_ip_faulted'),1);self.assertEqual(m.call('pan_ip_ready'),0)
        m.call('pan_ip_info',p);self.assertEqual(struct.unpack('<I',m.u.mem_read(p+24,4))[0],0)
        self.assertEqual(m.ip_status(),'协议栈断言，已停用');m.call('network_service_view',m.syms['test_view'])
        self.assertEqual(m.word('test_view'),0);self.assertEqual(m.word('test_view',28),0xfffffffb);self.assertEqual(m.word('test_view',32),7)
    def test_sdp_nap_uuid_continuation_and_malformed_packets(self):
        m=self.network();p=m.syms['test_read_buffer'];out=p+256;m.call('nap_sdp_init',p)
        self.assertEqual(m.call('nap_sdp_request',p,out,32),13);request=bytes(m.u.mem_read(out,13))
        self.assertEqual(request,bytes.fromhex('02000100083503191116001000'))
        def response(transaction,total,handles,continuation=b''):
            body=struct.pack('!HH',total,len(handles))+b''.join(struct.pack('!I',h) for h in handles)+bytes([len(continuation)])+continuation
            return struct.pack('!BHH',3,transaction,len(body))+body
        def feed(packet):m.u.mem_write(out,packet);return m.call('nap_sdp_response',p,out,len(packet))
        first=response(1,2,[0x10001],b'x');self.assertEqual(feed(first[:-1]),-71);self.assertEqual(feed(first),0)
        self.assertEqual(m.call('nap_sdp_request',p,out,32),14);self.assertEqual(bytes(m.u.mem_read(out+12,2)),b'\x01x')
        self.assertEqual(feed(response(2,2,[0x10001])),-71);self.assertEqual(feed(response(2,2,[0x10002])),1)
        self.assertEqual(m.call('nap_sdp_request',p,out,32),-22)
        m.call('nap_sdp_init',p);m.call('nap_sdp_request',p,out,32);self.assertEqual(feed(response(1,0,[])),1)
        m.call('nap_sdp_init',p);m.call('nap_sdp_request',p,out,32);self.assertEqual(feed(bytes.fromhex('01000100020003')),-65539)
    def test_sdp_transport_owned_request_callback_and_disconnect_lifetime(self):
        m=Sdp(self.elf);self.assertEqual(m.start(),0);self.assertEqual(m.start(),-16);m.drain();m.opened();m.poll();m.drain()
        self.assertEqual(m.transmitted,[(0x44,bytes.fromhex('02000100083503191116001000'))])
        m.received(bytes.fromhex('0300010009000100010001000100'));m.poll();m.drain()
        self.assertEqual(m.disconnects,[0x44]);self.assertEqual(m.result(),-11);self.assertEqual(m.start(),-16)
        m.closed();m.poll();self.assertEqual(m.result(),1);self.assertEqual(set(m.allocations),m.freed);self.assertEqual(m.start(),0)
    def test_sdp_cancel_before_queue_run_never_opens_channel(self):
        m=Sdp(self.elf);m.start();m.call('real_network_sdp_cancel');m.poll();m.drain();m.poll()
        self.assertEqual(m.connects,[]);self.assertEqual(m.result(),-125);self.assertEqual(set(m.allocations),m.freed);self.assertEqual(m.start(),0)
    def test_sdp_late_cid_after_timeout_is_closed_before_reuse(self):
        m=Sdp(self.elf);m.start();m.drain()
        for _ in range(55):m.poll()
        self.assertEqual(m.result(),-110);self.assertEqual(m.start(),-16)
        m.event(2,struct.pack('<H',0x48));m.poll();m.drain();self.assertEqual(m.disconnects,[0x48]);self.assertEqual(m.start(),-16)
        m.closed(0x48);m.poll();self.assertEqual(m.result(),-110);self.assertEqual(set(m.allocations),m.freed);self.assertEqual(m.start(),0)
    def test_sdp_failed_close_queue_is_retried_without_double_free(self):
        m=Sdp(self.elf);m.start();m.drain();m.opened();m.poll();m.drain();m.received(b'bad');m.poll();m.drain(cancel=True)
        m.poll();m.drain();self.assertEqual(m.disconnects,[0x44]);m.closed();m.poll();self.assertEqual(m.result(),-71);self.assertEqual(set(m.allocations),m.freed)
    def test_sdp_invalid_cid_small_mtu_and_allocation_fail_are_not_nap(self):
        for kind in ('cid','mtu','alloc','owner'):
            with self.subTest(kind=kind):
                m=Sdp(self.elf)
                if kind=='alloc':m.fail_alloc=True
                if kind=='owner':m.u.mem_write(0x2013BDBC,bytes(4))
                m.start();m.drain()
                if kind=='cid':m.event(2,struct.pack('<H',0));m.poll()
                elif kind=='mtu':m.opened(mtu=1);m.poll();m.drain();m.closed();m.poll()
                else:m.poll()
                self.assertEqual(m.result(),{'cid':-111,'mtu':-90,'alloc':-12,'owner':-19}[kind]);self.assertEqual(set(m.allocations),m.freed)

if __name__=='__main__':unittest.main(verbosity=2)
