#!/usr/bin/env python3
"""Execute exact p67/3.101.043 Bluetooth ABI instructions in Unicorn.

Only firmware wrappers/record producers/queue dispatch are executed. IPC, radio,
heap, scheduler and list primitives are explicit host models. This is ABI
evidence, not an on-device connectivity or stability claim.
"""
from __future__ import annotations
import argparse
import hashlib
import re
import struct
from pathlib import Path
from probe_nsh_spawn import (Machine, IMAGE, SHA256, BASE, RETURN, PARAM,
    UC_ARM_REG_PC, UC_ARM_REG_SP, require)

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = 0x20060000
OWNER = 0x20061000
CALLBACK = 0x201E0000
REQUEST = PARAM
LINK = 0x20062000
CHANNEL = 0x20063008
PEER = bytes.fromhex('665544332211')


class Probe(Machine):
    def __init__(self, image):
        super().__init__(image)
        self.bt_next = 0x20070000
        self.bt_allocations = {}
        self.bt_frees = []
        self.hooks[0x0C840C54] = self.bt_alloc
        self.hooks[0x0C840C60] = self.bt_free

    def bt_alloc(self):
        n = self.reg(0)
        address = self.bt_next
        self.bt_next += (n + 31) & ~15
        self.bt_allocations[address] = n
        self.u.mem_write(address, bytes(n))
        return address

    def bt_free(self):
        address = self.reg(0)
        require(address not in self.bt_frees, 'double Bluetooth free')
        self.bt_frees.append(address)
        return 0

    def half(self, address, value=None):
        if value is None:
            return struct.unpack('<H', self.u.mem_read(address, 2))[0]
        self.u.mem_write(address, struct.pack('<H', value))

    def finish_boundary(self):
        # Stop at the explicitly modelled boundary, after the tested stores.
        self.u.reg_write(UC_ARM_REG_PC, RETURN)
        return None


def header_contract():
    text = (ROOT / 'supervisor/network/include/bt_addresses.h').read_text()
    expected = {
        'BT_SOCKET_CLIENT_SENDRECV': 0x0C3AC47D,
        'BT_ADAPTER_REGISTER': 0x0C3AF3E9,
        'BT_ADAPTER_GET_STATE': 0x0C3AF4F5,
        'BT_DISCOVERY_START': 0x0C3AF525,
        'BT_DISCOVERY_STOP': 0x0C3AF551,
        'BT_PAIR_REQUEST_REPLY': 0x0C3B0051,
        'BT_PAIR_DISPLAY_REPLY': 0x0C3B008D,
        'BT_ADAPTER_SLOT': 0x20126738,
        'BT_L2CAP_CONNECT': 0x0C805B0D,
        'BT_L2CAP_DISCONNECT': 0x0C805BBD,
        'BT_L2CAP_SUBMIT_CID': 0x0C805BE9,
        'BT_QUEUE_EXTERNAL': 0x0C7EB9CD,
        'BT_OWNER_SLOT': 0x2013BDBC,
        'BT_ALLOC': 0x0C840C55, 'BT_FREE': 0x0C840C61,
    }
    for name, value in expected.items():
        found = re.findall(r'^#define\s+' + name + r'\s+(0x[0-9a-fA-F]+)u?$', text, re.M)
        require(len(found) == 1 and int(found[0], 16) == value, name + ' production address mismatch')
    print('PASS production header: exact .043 entries, including disconnect != accept')


def adapter_ipc(image):
    m = Probe(image)
    m.put(0x20126738, ADAPTER)
    m.put(0x2013DBE4, ADAPTER + 0x1000)
    require(m.call(0x0C70B180) == ADAPTER, 'stock getter selected wrong adapter')
    records = []
    def ipc():
        adapter, message, command = m.reg(0), m.reg(1), m.reg(2)
        require(adapter == ADAPTER, 'IPC adapter changed')
        records.append((command, bytes(m.u.mem_read(message + 60, 8))))
        m.u.mem_write(message + 12, b'\0')
        if command == 33:
            m.u.mem_write(message + 60, PEER)
        return 0
    m.hooks[0x0C3AC47C] = ipc
    m.u.mem_write(REQUEST, PEER)
    m.call(0x0C701C70, REQUEST + 32)
    require(bytes(m.u.mem_read(REQUEST + 32, 6)) == PEER and records[-1][0] == 33, 'stock MAC IPC33/message+60 layout')
    require(m.call(0x0C3AF524, ADAPTER, 20) == 0, 'scan start failed')
    require(records[-1][0] == 30 and struct.unpack('<I', records[-1][1][:4])[0] == 20, 'scan timeout layout')
    m.call(0x0C3AF550, ADAPTER)
    require(records[-1][0] == 31, 'scan stop command')
    m.call(0x0C3B0050, ADAPTER, REQUEST, 1)
    require(records[-1][0] == 97 and records[-1][1][:7] == PEER + b'\1', 'pair request reply layout')
    m.call(0x0C3B008C, ADAPTER, REQUEST, 1, 1)
    require(records[-1] == (98, PEER + b'\1\1'), 'pair display reply is not IPC98')
    manager, descriptor, token = PARAM + 0x400, PARAM + 0x500, PARAM + 0x600
    m.put(ADAPTER + 120, manager)
    def register():
        require((m.reg(0), m.reg(1), m.reg(2)) == (manager, 0, descriptor), 'registration descriptor ownership')
        return token
    m.hooks[0x0C3A96EC] = register
    require(m.call(0x0C3AF3E8, ADAPTER, descriptor) == token, 'registration token return')
    print('PASS real stock adapter/get-address, discovery, pair replies and callback registration')


def l2cap_wrappers(image):
    m = Probe(image)
    m.put(0x2013BDBC, OWNER)
    seen = []
    def enqueue():
        seen.append((m.reg(0), m.reg(1), m.reg(2), m.reg(3), m.word(m.u.reg_read(UC_ARM_REG_SP))))
        return 1
    m.hooks[0x0C7EB988] = enqueue
    for entry, handler in [(0x0C805B0C, 0x0C801AC5), (0x0C805BBC, 0x0C802F19), (0x0C805B38, 0x0C80279D)]:
        m.call(entry, REQUEST)
        require(seen[-1] == (OWNER, handler, 0x0C7EBD41, REQUEST, 1), 'L2CAP wrapper routed to wrong handler')
    m.u.mem_write(REQUEST, b'\xA5' * 32)
    m.call(0x0C805BE8, REQUEST, 0x56781234)
    require(seen[-1] == (OWNER, 0x0C801D11, 0x0C7EBD41, REQUEST, 1), 'submit handler/owner')
    require(m.word(REQUEST + 4) == 0x56781234, 'submit must write full CID word')
    require(bytes(m.u.mem_read(REQUEST + 8, 24)) == b'\xA5' * 24, 'submit overwrote payload metadata')
    print('PASS real connect/disconnect/accept/send wrappers, owner, five arguments and CID word')


def external_queue(image):
    m = Probe(image)
    run, cancel, argument = CALLBACK, CALLBACK + 4, PARAM + 0x200
    seen = []
    m.hooks[0x0C7EA0F0] = lambda: 0
    def cancelled():
        seen.append((m.reg(0), m.reg(1), m.reg(2)))
        return 0
    m.hooks[cancel] = cancelled
    m.call(0x0C7EB9CC, OWNER, run | 1, cancel | 1, argument, 0x71)
    require(seen == [(0, 221, argument)], 'lock-null path must invoke cancel once')
    lock_slot = m.word(0x0C7EBB00)
    queue_slot = m.word(0x0C7EBAE8)
    page = PARAM + 0x800
    m.put(lock_slot, PARAM + 0x700)
    m.put(queue_slot, page)
    m.u.mem_write(page, bytes(408))
    m.u.mem_write(page + 4, b'\x02\x00')
    m.hooks[0x0C840C98] = lambda: 0
    m.hooks[0x0C7EBA36] = m.finish_boundary
    m.call(0x0C7EB9CC, OWNER, run | 1, cancel | 1, argument, 0x71)
    require([m.word(page + n) for n in (8, 12, 16, 20)] == [OWNER, argument, run | 1, cancel | 1], 'external queue slot layout')
    require(m.byte(page + 24) == 0x71 and m.byte(page + 5) == 1, 'external queue flag/tail')
    for owner, callback in [(OWNER, run), (0, cancel)]:
        m = Probe(image)
        node = PARAM + 0x800
        for offset, value in [(0, owner), (4, argument), (8, run | 1), (12, cancel | 1)]:
            m.put(node + offset, value)
        m.u.mem_write(node + 16, b'\x71')
        m.hooks[0x0C7EAEE8] = lambda: node
        events = []
        def invoked():
            events.append((m.reg(0), m.reg(1), m.reg(2)))
            return 0
        m.hooks[callback] = invoked
        def released():
            require(m.reg(0) == node - 4, 'dispatcher queue-node allocation prefix')
            return m.finish_boundary()
        m.hooks[0x0C840C60] = released
        m.call(0x0C7EB7C4)
        require(events == [(owner, 0x71, argument)], 'dispatcher selected wrong callback/arguments')
    print('PASS real external queue enqueue/cancel and live/withdrawn owner dispatch')


def channel_records(image):
    m = Probe(image)
    m.put(CHANNEL, LINK)
    m.put(CHANNEL + 20, CALLBACK | 1)
    m.half(CHANNEL + 116, 0x0044)
    m.put(LINK + 8, PARAM + 0x1000)
    m.u.mem_write(LINK + 16, PEER + bytes(2))
    m.u.mem_write(REQUEST, bytes(68))
    m.half(REQUEST + 2, 15)
    m.put(REQUEST + 12, CALLBACK | 1)
    m.u.mem_write(REQUEST + 16, PEER)
    m.half(REQUEST + 52, 1691)
    m.half(REQUEST + 54, 1)
    def link():
        require(bytes(m.u.mem_read(m.reg(0), 6)) == PEER and m.reg(1) == 0, 'classic address/transport')
        return LINK
    def channel():
        require((m.reg(0), m.reg(1), m.reg(2), m.reg(3)) == (LINK, CALLBACK | 1, 15, 1), 'PSM/callback connect fields')
        return CHANNEL
    m.hooks[0x0C8004F4] = link
    m.hooks[0x0C7FF93C] = channel
    m.hooks[0x0C7FF1CC] = lambda: 0
    records = []
    def callback():
        event, record = m.reg(0), m.reg(1)
        size = {2: 68, 3: 120, 6: 12}[event]
        records.append((event, bytes(m.u.mem_read(record, size))))
        return 0
    m.hooks[CALLBACK] = callback
    m.call(0x0C801AC4, OWNER, 1, REQUEST)
    require(records[-1][0] == 2 and struct.unpack_from('<H', records[-1][1])[0] == 0x44, 'assigned event CID+0')
    require(m.half(CHANNEL + 52) == 1691 and m.half(CHANNEL + 54) == 1, 'connect MTU/flags configuration')
    m.half(CHANNEL + 96, 1691)
    m.hooks[0x0C801F24] = lambda: 0
    m.call(0x0C801FD4, CHANNEL)
    event, opened = records[-1]
    require(event == 3 and struct.unpack_from('<H', opened, 108)[0] == 0x44 and struct.unpack_from('<H', opened, 72)[0] == 1691, 'open event CID+108/MTU+72')
    m.put(0x2013BDBC, OWNER)
    m.put(OWNER, PARAM + 0x1200)
    for address in (0x0C7EAD28, 0x0C7EB33C, 0x0C7EACD0, 0x0C7EABF0):
        m.hooks[address] = lambda: 0
    m.call(0x0C801124, CHANNEL, 0x409)
    event, closed = records[-1]
    require(event == 6 and struct.unpack_from('<HH', closed) == (0x44, 0x409) and closed[4:10] == PEER, 'close event CID/reason/peer')
    require(CHANNEL - 8 in m.bt_frees, 'channel teardown owns allocation prefix')
    print('PASS real classic 68-byte connect, assigned/open/close callback record producers')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('image', nargs='?', type=Path, default=IMAGE)
    args = parser.parse_args()
    image = args.image.read_bytes()
    require(hashlib.sha256(image).hexdigest() == SHA256, 'wrong firmware image')
    header_contract()
    adapter_ipc(image)
    l2cap_wrappers(image)
    external_queue(image)
    channel_records(image)
    print('HOST ABI EVIDENCE ONLY: radio/IPC/heap/list/scheduler primitives modelled; hardware pending.')


if __name__ == '__main__':
    main()
