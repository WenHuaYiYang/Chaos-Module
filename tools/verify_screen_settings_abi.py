#!/usr/bin/env python3
"""Verify the exact .043 screen-service RAM fields and refresh call chain."""
from pathlib import Path
import argparse
import hashlib
import re
import struct
from verify_keyboard_abi import SHA256, signature, bl_target
ROOT=Path(__file__).resolve().parents[1]
def verify(data):
    assert hashlib.sha256(data).hexdigest()==SHA256, "expected exact p67/.043 AP"
    base=struct.unpack_from('<I',data,0x9b22cc)[0]
    assert base==struct.unpack_from('<I',data,0x9b2948)[0]==0x2013f368
    signature(data,0x9b22aa,'c4f8d430') # str r3,[r4,#0xd4]
    signature(data,0x9b2926,'84f8d230') # strb r3,[r4,#0xd2]
    assert bl_target(data,0x9b22c0)==0x9b0360
    assert bl_target(data,0x9b293c)==0x9b14b4
    source=(ROOT/'supervisor/src/fw_api/fwaddr.rs').read_text()
    for name,value in {'SCREEN_DIMMER_TIMEOUT':base+0xd4,'SCREEN_TIMEOUT_REFRESH':0x0ca70361,
                       'SCREEN_TOUCH_PALM':base+0xd2,'SCREEN_TOUCH_PALM_REFRESH':0x0ca714b5}.items():
        matches=re.findall(r'pub const '+name+r': u32 = (0x[\da-fA-F_]+);',source)
        assert len(matches)==1 and int(matches[0].replace('_',''),16)==value,name
    print('PASS exact .043 timeout/palm RAM fields, stock refresh calls and production bindings')
if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('image',type=Path)
    verify(parser.parse_args().image.read_bytes())
