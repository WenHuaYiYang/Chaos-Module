#!/usr/bin/env python3
"""Exact p67/.043 clock and system() instructions; no physical device I/O.

The monotonic tick reader, task_spawn scheduler, and waitpid are explicit
models. Real firmware dispatcher/conversion/attr/system wrapper bytes run.
Reboot board-control instructions are inspected, never executed.
"""
import argparse
import hashlib
import re
import struct
from pathlib import Path
from probe_nsh_spawn import Machine, IMAGE, SHA256, BASE, PARAM, TLS, UC_ARM_REG_SP, require
from verify_nsh_abi import thumb_bl_target, verify_image
ROOT = Path(__file__).resolve().parents[1]

def static(image):
    verify_image(image)
    def word(address): return struct.unpack_from('<I', image, address-BASE)[0]
    require(thumb_bl_target(image, 0x0C202012)==0x0C202194, 'CLOCK_MONOTONIC conversion path changed')
    require(image[0x0C201FEE-BASE:0x0C201FF4-BASE]==bytes.fromhex('01280c460dd0'), 'clock id 1 dispatch changed')
    require(word(0x0CCDB648)==0x2CB7EA58 and word(0x0CCDB64C)==0x0C23D7D5, 'reboot command table changed')
    require(image[0x2CB7EA58-0x2C0C0000:0x2CB7EA58-0x2C0C0000+7]==b'reboot\0', 'reboot literal changed')
    require(word(0x0C23C6D4)==0x2CB2E2C4, 'stock meminfo consumer changed')
    require(image[0x2CB2E2C4-0x2C0C0000:0x2CB2E2C4-0x2C0C0000+14]==b'/proc/meminfo\0', 'proc path changed')
    addresses=(ROOT/'supervisor/src/fw_api/fwaddr.rs').read_text()
    values=re.findall(r'pub const CLOCK_GETTIME: u32 = (0x[0-9a-fA-F_]+);', addresses)
    require(len(values)==1 and int(values[0].replace('_',''),16)==0x0C201FE1, 'production clock must be exact odd Thumb callable')
    print('PASS exact AP, CLOCK_MONOTONIC dispatcher, reboot command-table row, stock proc/meminfo literal, production odd Thumb address')

def clock_probe(image):
    m=Machine(image)
    ticks=[0]
    def tick_read():
        m.u.mem_write(m.reg(0), struct.pack('<Q',ticks[0]))
        return 0
    m.hooks[0x0C1AE158]=tick_read
    m.hooks[0x0C1E45BC]=lambda:TLS
    target=PARAM+0x100
    for value in [0,1,9999,10000,123456789,0x100000000+1234,50000000000]:
        ticks[0]=value
        m.u.mem_write(target-8,b'\xA5'*32)
        require(m.call(0x0C201FE0,1,target)==0,'clock_gettime failed')
        sec,nsec=struct.unpack('<qi',m.u.mem_read(target,12))
        require((sec,nsec)==(value//10000,(value%10000)*100000),f'timespec conversion mismatch for {value}: {sec},{nsec}')
        require(bytes(m.u.mem_read(target-8,8))==b'\xA5'*8 and bytes(m.u.mem_read(target+12,12))==b'\xA5'*12,'timespec write exceeded 12 bytes')
    require(m.call(0x0C201FE0,5,target)==-1 and m.word(TLS)==22,'invalid clock id error contract')
    require(m.call(0x0C201FE0,1,0)==-1 and m.word(TLS)==22,'null timespec error contract')
    print('PASS actual clock dispatcher/conversion: sec64@0+nsec32@8, 10kHz ticks, guards and EINVAL')

def system_probe(image):
    m=Machine(image)
    command=PARAM+0x100
    m.u.mem_write(command,b'reboot\0')
    calls=[]
    def spawn():
        sp=m.u.reg_read(UC_ARM_REG_SP)
        argv=m.word(sp)
        require(m.cstr(m.reg(0))==b'system','system child name changed')
        require(m.reg(1)==0x0C2390E1 and m.reg(2)==0,'system child entry/actions changed')
        require(m.cstr(m.word(argv))==b'-c' and m.word(argv+4)==command and m.word(argv+8)==0,'system argv is not [-c, command, NULL]')
        attr=m.reg(3)
        require(m.byte(attr)==4 and m.byte(attr+1)==100 and m.word(attr+12)==8192 and m.word(attr+16)==0,'system spawn attr changed')
        calls.append('spawn')
        return 77
    def wait():
        require(m.reg(0)==77 and m.reg(2)==0,'system must use blocking wait in its isolated task')
        m.put(m.reg(1),7<<8)
        calls.append('wait')
        return 77
    m.hooks[0x0C209770]=spawn
    m.hooks[0x0C2063C4]=wait
    require(m.call(0x0C240398,command)==7<<8,'system wait-status return contract')
    require(calls==['spawn','wait'],'system call sequence')
    print('PASS actual system(): fixed command pointer, NSH -c argv, isolated attr, blocking wait and wait-status return')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',nargs='?',type=Path,default=IMAGE);p.add_argument('--static-only',action='store_true');args=p.parse_args()
    image=args.image.read_bytes();require(hashlib.sha256(image).hexdigest()==SHA256,'wrong firmware image')
    static(image)
    if not args.static_only:clock_probe(image);system_probe(image)
    print('HOST EVIDENCE ONLY: hardware ticks/scheduler/wait mocked; no reboot was executed.')
if __name__=='__main__':main()
