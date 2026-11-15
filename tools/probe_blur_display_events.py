#!/usr/bin/env python3
"""Execute p67/.043 display-event list dispatch/get/remove machine code.

The callback and allocator leaves are models. This proves that DELETE event
dispatch alone retains its record; it does not prove which vitality caller
issued the notification, physical GPU behavior, or acceptance on a device.
Requires the same Unicorn environment as probe_honeycomb_abi.py.
"""
from pathlib import Path
import argparse
import struct

from probe_honeycomb_abi import Machine, UC_ARM_REG_R0
from verify_keyboard_abi import IMAGE, verify_image


def verify(data):
    verify_image(data)
    m = Machine(data)
    display, buffer, record, event, callback = (
        0x20030000, 0x20031000, 0x20031100, 0x20032000, 0x20033001)
    event_list = display + 736
    m.put(event_list, buffer, 1, 4, 4, 0)
    m.put(buffer, record)
    m.put(record, callback, 7, 0x8000)
    m.put(event, display, display, 36, 0, 0, 0, 0)
    seen = []

    def on_event(u):
        address = u.reg_read(UC_ARM_REG_R0)
        seen.append(struct.unpack('<4I', m.u.mem_read(address, 16)))
        return 0

    m.hooks[callback & ~1] = on_event
    assert m.call(0x1C07AB49, event_list) == 1
    assert m.call(0x1C07A4B1, event_list, 0) == record
    # Execute the SRAM lv_event_send body, with the exact native list ABI.
    assert m.call(0x002BFB50, event_list, event, 1) == 1
    assert seen == [(display, display, 36, 7)]
    assert m.call(0x1C07AB49, event_list) == 1
    assert m.call(0x1C07A4B1, event_list, 0) == record
    # Allocator leaves only: free the record, resize the now-empty vector.
    freed = []
    m.hooks[0x002C2D40] = lambda u: freed.append(u.reg_read(UC_ARM_REG_R0)) or 0
    m.hooks[0x1C0606BC] = lambda u: 0
    assert m.call(0x1C07A6F9, event_list, 0) == 1
    assert freed == [record]
    assert m.call(0x1C07AB49, event_list) == 0
    assert m.call(0x1C07A4B1, event_list, 0) == 0
    print('PASS actual .043: DELETE dispatch preserves callback/user_data/filter; native index removal clears it')
    print('NOT VERIFIED: vitality call chain, simulator/device recovery, physical GPU/cache behavior')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', type=Path, nargs='?', default=IMAGE)
    verify(parser.parse_args().image.read_bytes())
