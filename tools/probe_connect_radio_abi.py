#!/usr/bin/env python3
"""Actual .043 SPP, scan-mode and bond IPC wrappers; radio/service are seams."""
import struct
from probe_nsh_spawn import Machine,IMAGE,PARAM,require,SHA256,UC_ARM_REG_SP
from pathlib import Path
import hashlib
image=IMAGE.read_bytes();require(hashlib.sha256(image).hexdigest()==SHA256,'wrong firmware')
m=Machine(image);adapter=PARAM;uuid=PARAM+0x800;callbacks=PARAM+0x900
records=[];scan_mode=[1];bond_mode=[0]
def ipc():
    require(m.reg(0)==adapter,'adapter changed');message,command=m.reg(1),m.reg(2)
    records.append((command,bytes(m.u.mem_read(message+60,32))))
    m.u.mem_write(message+12,bytes([scan_mode[0] if command==38 else bond_mode[0] if command==92 else 0]));return 0
m.hooks[0xc3ac47c]=ipc
seen=[]
m.hooks[0xc3b1434]=lambda:seen.append((m.reg(0),m.reg(1),m.reg(2))) or 0x20090000
require(m.call(0xc3b14d0,adapter,callbacks)==0x20090000,'SPP app forwarding')
require(seen==[(adapter,0,callbacks)],'callback table signature')
u=struct.pack('<5I',16,0x414f5301,0x80004348,0x00014a10,0x43434e31);m.u.mem_write(uuid,u)
require(m.call(0xc3b152c,adapter,0x20090000,19,uuid,1)==0,'server start')
formatted=[]
def formatter():
    sp=m.u.reg_read(UC_ARM_REG_SP)
    formatted.append((m.reg(3),*[m.word(sp+i*4) for i in range(5)]));return 0
m.hooks[0xc1ea8bc]=formatter
m.call(0xc3a9580,uuid,PARAM+0x1800,128)
require(formatted==[(0x43434e31,1,0x4a10,0x8000,0x4348414f,0x5301)],'UUID canonical field order')
command,payload=records[-1]
require(command==0xf4 and payload[4:24]==u and struct.unpack_from('<H',payload,24)[0]==19 and payload[26]==1,'SPP server UUID/SCN/max-connection IPC')
require(m.call(0xc3b1588,adapter,0x20090000,19)==0,'server stop')
require(records[-1][0]==0xf5 and struct.unpack_from('<H',records[-1][1],4)[0]==19,'SPP stop IPC')
for value in range(3):
    scan_mode[0]=value
    require(m.call(0xc3af680,adapter)==value,'get_scan_mode returns enum, not status')
m.call(0xc3af64c,adapter,2,1);require(records[-1][0]==37 and records[-1][1][:2]==b'\x02\x01','discoverable/bondable mode ABI')
address=PARAM+0xa00;m.u.mem_write(address,b'\x01\x02\x03\x04\x05\x06')
m.call(0xc3b1624,adapter,0x20090000,address,77)
require(records[-1][0]==0xf7 and records[-1][1][4:10]==b'\x01\x02\x03\x04\x05\x06' and struct.unpack_from('<H',records[-1][1],10)[0]==77,'SPP disconnect address/port IPC')
for value in range(3):
    bond_mode[0]=value
    require(m.call(0xc3afda0,adapter,address,1)==value,'bond-state query returns enum')
    require(records[-1][0]==92 and records[-1][1][:7]==b'\x01\x02\x03\x04\x05\x06\x01','bond-state BR/EDR address/transport')
m.call(0xc3affa2,adapter,address,1)
require(records[-1][0]==94 and records[-1][1][:7]==b'\x01\x02\x03\x04\x05\x06\x01','BREDR bond ABI')
# Stock miwear owns channel 5. Our private service leaves it alone on channel 19.
alias=0x2cd4b9c0-0x2c0c0000
require(struct.unpack_from('<4I',image,alias)==(0,0,0xc712295,0xc71222d),'stock SPP callback slots')

# The stock miwear callback really denies an unknown peer when its gate is 1.
# A bounded module pairing window clears only this RAM gate, never its address.
m.hooks[0xc1f0d20]=lambda:0;m.hooks[0xc3a8f98]=lambda:PARAM+0x1900
m.hooks[0xc3a8f5c]=lambda:0 if bytes(m.u.mem_read(m.reg(0),6))==bytes(m.u.mem_read(m.reg(1),6)) else 1
m.put(0x20126738,adapter);m.u.mem_write(0x201264a6,b'ABCDEF')
for gate,accept in [(1,0),(0,1),(1,0)]:
    m.u.mem_write(0x201264a4,bytes([gate]));m.call(0xc709e90,0,address)
    require(records[-1][0]==0x61 and records[-1][1][6]==accept,'stock pair allow-list gate')
    require(bytes(m.u.mem_read(0x201264a6,6))==b'ABCDEF','pair target address was changed')

# Real native pipe wrappers. Heap, libuv registration/submission and callbacks
# are leaves; descriptor layout and callback arguments execute firmware bytes.
next_heap=[0x200a0000];freed=[]
def allocate():
    n=m.reg(0);p=next_heap[0];next_heap[0]+=(n+15)&~15;m.u.mem_write(p,bytes(n));return p
m.hooks[0xc1f903c]=allocate;m.hooks[0xc1f90bc]=allocate
m.hooks[0xc1f8ff8]=lambda:freed.append(m.reg(0)) or 0
m.hooks[0xc3c42a4]=lambda:0
pipe=PARAM+0xc00;payload=PARAM+0x1000;user_callback=0x201df001
submitted=[]
def uv_buf():m.put(m.reg(0),m.reg(1));m.put(m.reg(0)+4,m.reg(2));return m.reg(0)
def uv_write():
    req,handle,buffers,count=m.reg(0),m.reg(1),m.reg(2),m.reg(3)
    require(handle==pipe and count==1,'UV write signature');submitted.append(req)
    require(m.word(buffers)==payload and m.word(buffers+4)==17,'UV buffer ABI')
    return 0
m.hooks[0xc2de1a4]=uv_buf;m.hooks[0xc2e0970]=uv_write
require(m.call(0xc3a9bfc,pipe,payload,17,user_callback)==0,'native pipe write')
req=submitted[-1];calls=[]
m.hooks[user_callback&~1]=lambda:calls.append((m.reg(0),m.reg(1),m.reg(2))) or 0
m.call(0xc3a9850,req,0)
require(calls==[(pipe,payload,0)] and req in freed,'write completion lifetime/signature')
m.hooks[0xc2db98c]=lambda:0;m.hooks[0xc2dbd2c]=lambda:0
read=[]
m.hooks[0xc2de8c8]=lambda:read.append((m.reg(0),m.reg(1),m.reg(2))) or 0
require(m.call(0xc3a9a74,pipe,2400,user_callback,0)==0,'native read start')
require(read[-1]==(pipe,0xc3a9885,0xc3a98c9),'native allocation/read callback wiring')
uvbuf=PARAM+0x1500;m.put(uvbuf,payload);m.put(uvbuf+4,17);calls.clear()
m.call(0xc3a98c8,pipe,17,uvbuf)
require(calls==[(pipe,payload,17)] and payload in freed,'borrowed read buffer/signature')
print('PASS actual .043: SPP registration, callback slots, private UUID, channel19/max1, stop/disconnect, scan-mode enum/status distinction, BREDR bond/query, stock pair gate, UV read/write lifetimes')
print('NOT VERIFIED: radio/service execution, Android pairing or wireless throughput')
