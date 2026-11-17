#!/usr/bin/env python3
"""Verify exact .043 battery and return-to-watchface service ABIs.

No firmware is redistributed. With no image, check production/harness bindings.
With an image, also check identity, real getter and stock return-home consumer.
"""
from pathlib import Path
import argparse,hashlib,re,struct
ROOT=Path(__file__).resolve().parents[1]
SHA256='519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec'
VALUES={'POWER_MANAGER_BATTERY_LEVEL':0x0CA6E6A9,
        'HOME_PREPARE_WATCHFACE':0x0C50FD7D,'APP_RETURN_HOME_SLOT':0x200EB6F0}

def verify_sources():
    for name,value in VALUES.items():
        for path in [ROOT/'supervisor/src/fw_api/fwaddr.rs',ROOT/'supervisor/tests/floating_ball_services_arm.rs']:
            matches=re.findall(r'pub const '+name+r': u32 = (0x[0-9a-fA-F_]+);',path.read_text())
            assert len(matches)==1 and int(matches[0].replace('_',''),16)==value,(path,name)
    text=(ROOT/'supervisor/src/fw_api/navigation.rs').read_text()
    assert not re.search(r'\bpage_(?:goto|back)\(',text),'page navigation is not an app-level home request'
    text=(ROOT/'supervisor/src/fw_api/battery.rs').read_text()
    assert 'POWER_MANAGER_BATTERY_LEVEL' in text and 'ioctl(' not in text and 'opendir(' not in text

def signature(data,off,hexbytes):
    expected=bytes.fromhex(hexbytes)
    assert data[off:off+len(expected)]==expected,f'043 signature changed at {off:#x}'

def verify(data):
    verify_sources()
    assert hashlib.sha256(data).hexdigest()==SHA256,'wrong AP: expected p67/3.101.043'
    signature(data,0x9AE6A8,'80b4044b00afd3f8b000bd465df8047b704700bf10ef1320')
    # Stock UI uses this no-argument getter as its battery percentage.
    signature(data,0x46FF22,'3ef1c1f30646')
    # Native return-home timer: reset panels with 0, then [app_if+0x10](0).
    signature(data,0x564B30,'80b50d4a0d4900af06200d4b')
    signature(data,0x564B46,'002083ed007bebf616f9094b00201b69984780bd')
    assert struct.unpack_from('<I',data,0x564B78)[0]+0x10==VALUES['APP_RETURN_HOME_SLOT']
    signature(data,0x44FD7C,'b0b582b002af044663f1daf0022804d12046fbf79bfa')
    # Confirm actual gauge paths in this image too; previous code omitted charge/.
    assert b'/dev/charge/soft_gauge\0' in data and b'/dev/charge/batt_gauge\0' in data

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('image',nargs='?',type=Path);args=p.parse_args()
    if args.image:verify(args.image.read_bytes());print('PASS exact 043 AP, stock battery getter and return-home sequence')
    else:verify_sources();print('PASS production and ARM harness service bindings')
if __name__=='__main__':main()
