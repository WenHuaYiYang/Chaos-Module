#!/usr/bin/env python3
"""Execute exact p67/3.101.043 spawn, actions, console and waitpid bytes.

Requires unicorn (pip install unicorn). No hardware or production firmware is
modified. Heap allocation, scheduler, VFS leaf operations and TLS are explicit
models; the ABI wrappers, action list management/dispatch, group selection,
argument construction, attributes and waitpid decision/cleanup run real AP code.
This is host ABI evidence, not an on-device stability or approval claim.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import struct
import sys

ROOT = Path(__file__).resolve().parent.parent
IMAGE = ROOT / 'research/firmware-3.101.043/043_ABI/executables/raw_arm_thumb/vela_ap.bin'
SHA256 = '519307675665e4866d722a8119a98589c397b614ac3294cb87bfc86de45756ec'
BASE = 0x0C0C0000
DATA_ALIAS = 0x2C0C0000
RAM = 0x20000000
RAM_SIZE = 0x200000
STACK = 0x201F0000
RETURN = 0x201FF000
USER_HEAP_SLOT = 0x200F4C28
USER_HEAP = 0x200D0000
CURRENT = 0x200B24D4
PARENT = 0x20010000
CHILD = 0x20011000
PARENT_GROUP = PARENT + 0xE8
CHILD_GROUP = CHILD + 0xE8
TLS = 0x20018000
CHILD_TLS = 0x20019000
PARAM = 0x20020000
PID = 47
PARENT_PID = 31

try:
    from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE
    from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
        UC_ARM_REG_R3, UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
        UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12,
        UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_BASEPRI)
except ImportError:
    print('SKIP: install unicorn in a task virtualenv to execute the firmware probes', file=sys.stderr)
    raise SystemExit(77)

REGS = [UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
        UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R7,
        UC_ARM_REG_R8, UC_ARM_REG_R9, UC_ARM_REG_R10, UC_ARM_REG_R11, UC_ARM_REG_R12]


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def signed(v):
    return v if v < 0x80000000 else v - 0x100000000


class Machine:
    def __init__(self, image):
        self.u = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
        size = (len(image) + 4095) & ~4095
        self.u.mem_map(BASE, size)
        self.u.mem_write(BASE, image)
        self.u.mem_map(DATA_ALIAS, size)
        self.u.mem_write(DATA_ALIAS, image)
        self.u.mem_map(RAM, RAM_SIZE)
        self.u.mem_write(RETURN, b'\x00\xbe')
        self.hooks = {}
        self.visited = set()
        self.allocations = {}
        self.frees = []
        self.heap_next = 0x20040000
        self.fail_alloc = False
        self.fd_tables = {}
        self.fd_events = []
        self.put(USER_HEAP_SLOT, USER_HEAP)
        self.put(CURRENT, PARENT)
        self.put(PARENT + 8, PARENT_GROUP)
        self.put(PARENT_GROUP, PARENT_PID)
        self.hooks[0x0C1F8B60] = self.zalloc
        self.hooks[0x0C1F7F04] = self.free
        self.hooks[0x0C1E4180] = self.assertion
        self.hooks[0x0C1F5444] = lambda: TLS
        self.hooks[0x0C205D1C] = self.sched_getparam
        self.hooks[0x0C205E0C] = lambda: 1
        self.hooks[0x0C5885B0] = lambda: len(self.cstr(self.reg(0)))
        self.hooks[0x0C588980] = self.memset
        self.hooks[0x0C206008] = lambda: CHILD_TLS
        self.u.hook_add(UC_HOOK_CODE, self.on_code)

    def reg(self, n):
        return self.u.reg_read(REGS[n])

    def word(self, addr):
        return struct.unpack('<I', self.u.mem_read(addr, 4))[0]

    def byte(self, addr):
        return self.u.mem_read(addr, 1)[0]

    def put(self, addr, value):
        self.u.mem_write(addr, struct.pack('<I', value & 0xFFFFFFFF))

    def cstr(self, addr):
        out = bytearray()
        while len(out) < 4096:
            b = self.byte(addr + len(out))
            if b == 0:
                return bytes(out)
            out.append(b)
        raise AssertionError('unterminated firmware string')

    def on_code(self, _u, address, _size, _):
        self.visited.add(address)
        if address == RETURN:
            self.u.emu_stop()
            return
        f = self.hooks.get(address)
        if f is not None:
            result = f()
            if result is None:
                self.u.emu_stop()
                return
            self.u.reg_write(UC_ARM_REG_R0, result & 0xFFFFFFFF)
            self.u.reg_write(UC_ARM_REG_PC, self.u.reg_read(UC_ARM_REG_LR))

    def call(self, addr, *args):
        for reg in REGS:
            self.u.reg_write(reg, 0)
        self.u.reg_write(UC_ARM_REG_SP, STACK)
        self.u.reg_write(UC_ARM_REG_LR, RETURN | 1)
        for i, value in enumerate(args):
            if i < 4:
                self.u.reg_write(REGS[i], value & 0xFFFFFFFF)
            else:
                self.put(STACK + (i - 4) * 4, value)
        self.u.emu_start(addr | 1, RETURN, count=100000)
        require(self.u.reg_read(UC_ARM_REG_PC) == RETURN, f'probe did not return: {addr:#x}')
        return signed(self.reg(0))

    def assertion(self):
        raise AssertionError(f'firmware assert: message={self.reg(0):#x} line={self.reg(1)}')

    def zalloc(self):
        heap, size = self.reg(0), self.reg(1)
        require(heap == USER_HEAP, 'allocator used a different heap')
        if self.fail_alloc:
            return 0
        ptr = self.heap_next
        self.heap_next += (size + 15) & ~15
        self.u.mem_write(ptr, bytes(size))
        self.allocations[ptr] = (heap, size)
        return ptr

    def free(self):
        heap, ptr = self.reg(0), self.reg(1)
        require(self.allocations.get(ptr, (None,))[0] == heap, 'cross-heap/unknown/double free')
        del self.allocations[ptr]
        self.frees.append(ptr)
        return 0

    def sched_getparam(self):
        require(self.reg(0) == 0, 'attr_init should read the current scheduling priority')
        self.put(self.reg(1), 83)
        return 0

    def memset(self):
        dst, value, count = self.reg(0), self.reg(1), self.reg(2)
        self.u.mem_write(dst, bytes([value & 255]) * count)
        return dst

    def add_actions(self, old=6, destinations=(0, 1, 2, 5)):
        head = PARAM + 0x100
        require(self.call(0x0C1EA710, head) == 0, 'actions_init failed')
        for target in destinations:
            require(self.call(0x0C1EA63C, head, old, target) == 0, 'adddup2 failed')
        return head


def probe_attributes(image):
    m = Machine(image)
    attr = PARAM + 16
    m.u.mem_write(attr - 8, b'\xA5' * 36)
    m.u.mem_write(attr, bytes(20))
    require(m.call(0x0C1EA724, attr) == 0, 'attr_init failed')
    initial = bytes(m.u.mem_read(attr, 20))
    require(initial[0] == 0 and initial[1] == 83 and initial[2] == 1, 'initial attr scheduling bytes')
    require(initial[4:12] == bytes(8) and m.word(attr + 12) == 0x800 and m.word(attr + 16) == 0, 'initial attr fields')
    priority = PARAM + 64
    m.put(priority, 100)
    require(m.call(0x0C1EA7A0, attr, priority) == 0, 'schedparam setter')
    require(m.call(0x0C1EA7C4, attr, 32768) == 0, 'stacksize setter')
    require(m.call(0x0C1EA780, attr, 4) == 0, 'flags setter')
    require(m.byte(attr) == 4 and m.byte(attr + 1) == 100, 'flags/priority offsets')
    require(m.word(attr + 12) == 32768 and m.word(attr + 16) == 0, 'stack addr must remain NULL')
    require(bytes(m.u.mem_read(attr - 8, 8)) == b'\xA5' * 8 and bytes(m.u.mem_read(attr + 20, 8)) == b'\xA5' * 8, 'attr overflow')
    require(m.call(0x0C1EA7E0, attr) == 0, 'attr destroy')
    print('PASS actual attr_init/setters: 20B, flags@0=4, priority@1=100, stack@12=32768, stackaddr@16=NULL; guards intact')


def probe_actions(image):
    m = Machine(image)
    head = m.add_actions()
    node = m.word(head)
    nodes = []
    for target in (0, 1, 2, 5):
        require(m.allocations[node] == (USER_HEAP, 16), 'action allocation shape')
        require(m.byte(node + 4) == 2 and m.word(node + 8) == 6 and m.word(node + 12) == target, 'dup2 action layout/order')
        nodes.append(node)
        node = m.word(node)
    require(node == 0, 'actions not NULL terminated')
    # Execute the public allocator wrapper on a failing modeled user heap.
    m.fail_alloc = True
    require(m.call(0x0C1EA63C, head, 6, 8) == 12, 'allocation failure must return positive ENOMEM')
    require(m.word(nodes[-1]) == 0, 'failed adddup2 changed the list')
    require(m.word(TLS + 0x3C) == 12, 'allocator errno')
    require(m.call(0x0C1EA6E4, head) == 0, 'actions_destroy failed')
    require(m.word(head) == 0 and not m.allocations and m.frees == nodes, 'destroy did not pair allocator/free')
    require(m.call(0x0C1EA6E4, head) == 0 and m.frees == nodes, 'empty destroy was not harmless')
    print('PASS actual actions init/adddup2/destroy, order and allocation failure; real wrappers select matching heap slot 0x200F4C28')


def probe_child_isolation(image):
    m = Machine(image)
    head = m.add_actions()
    m.put(CHILD + 0x40, 0x8000)
    # Run group allocation itself: normal task receives its inline own group.
    m.hooks[0x0C1B8E30] = lambda: 0  # fd-list/mutex init
    m.hooks[0x0C209FA0] = lambda: 0  # task-group resources
    m.hooks[0x0C1E69D8] = lambda: 0  # task-group lock init
    require(m.call(0x0C202824, CHILD, 0) == 0, 'group init failed')
    require(m.word(CHILD + 8) == CHILD_GROUP != PARENT_GROUP, 'child shares parent group')
    parent_fds = PARENT_GROUP + 0x2D0
    child_fds = CHILD_GROUP + 0x2D0
    before = {0: 'system-in', 1: 'system-out', 2: 'system-err', 5: 'pty-master', 6: 'pty-slave'}
    m.fd_tables[parent_fds] = dict(before)
    m.fd_tables[child_fds] = {}
    def clone():
        src, dst, actions, cloexec = (m.reg(i) for i in range(4))
        require((src, dst, actions, cloexec) == (parent_fds, child_fds, m.word(head), 1), 'wrong clone direction/context')
        m.fd_tables[dst] = dict(m.fd_tables[src])
        return 0
    def dup2():
        task, old, new = (m.reg(i) for i in range(3))
        require(task == CHILD, 'action applied to the parent task')
        fdtable = m.word(task + 8) + 0x2D0
        m.fd_tables[fdtable][new] = m.fd_tables[fdtable][old]
        m.fd_events.append((task, old, new))
        return new
    m.hooks[0x0C1B932C] = clone  # low-level files/locking are modeled
    m.hooks[0x0C1B97EC] = dup2
    require(m.call(0x0C202A48, CHILD, m.word(head), 1) >= 0, 'file setup failed')
    require(m.fd_tables[parent_fds] == before, 'parent fds changed')
    require(all(m.fd_tables[child_fds][x] == 'pty-slave' for x in (0, 1, 2, 5)), 'child stdin/out/err or inherited master incorrect')
    require(m.fd_events == [(CHILD, 6, x) for x in (0, 1, 2, 5)], 'actual dispatch order')
    require(m.call(0x0C2055D4, CHILD) == child_fds, 'actual task fd-table getter')
    print('PASS actual group selection + file setup + action dispatcher: own child group, child-only fd remap; VFS leaves modeled')


def probe_spawn_and_argv(image):
    m = Machine(image)
    name, attr = PARAM + 0x200, PARAM + 0x240
    m.u.mem_write(name, b'chaos.nsh\0')
    m.call(0x0C1EA724, attr)
    m.u.mem_write(attr, bytes([4, 100]))
    m.put(attr + 12, 32768)
    m.put(attr + 16, 0)
    head = m.add_actions()
    entry, envp = 0x201D0001, PARAM + 0x280
    m.put(envp, 0)
    m.hooks[0x0C202364] = lambda: envp
    observed = []
    def initialize():
        sp = m.u.reg_read(UC_ARM_REG_SP)
        args = [m.reg(i) for i in range(4)] + [m.word(sp + i * 4) for i in range(5)]
        require(args[1:] == [name, 100, 0, 32768, entry, 0, envp, m.word(head)], 'spawn six-argument ABI forwarding')
        require(m.word(args[0] + 0x40) == 0x8000, 'spawn task type is not a normal isolated task')
        m.put(args[0] + 0x2C, PID)
        observed.append(args)
        return 0
    m.hooks[0x0C208C8C] = initialize
    def set_priority():
        require(m.reg(0) == PID and m.word(m.reg(1)) == 100, 'actual attr_apply scheduling')
        return 0
    m.hooks[0x0C205A34] = set_priority
    m.hooks[0x0C209170] = lambda: 0
    require(m.call(0x0C209770, name, entry, head, attr, 0, 0) == PID, 'task_spawn result')
    require(len(observed) == 1, 'initializer not reached once')
    # Execute actual task_args and its real stack-frame allocator and strlcpy.
    m.put(CHILD + 0x60, 32768)
    m.put(CHILD + 0x64, 0x20090000)
    m.put(CHILD + 0x68, 0x20090000)
    require(m.call(0x0C208FEC, CHILD, name, 0) == 0, 'NULL argv construction')
    argv = m.word(CHILD_TLS + 0x40)
    require(m.cstr(m.word(argv)) == b'chaos.nsh' and m.word(argv + 4) == 0, 'NULL argv did not produce argc=1')
    supplied = PARAM + 0x2A0
    m.put(supplied, name)
    m.put(supplied + 4, 0)
    require(m.call(0x0C208FEC, CHILD, name, supplied) == 0, 'explicit argv construction')
    argv = m.word(CHILD_TLS + 0x40)
    require(m.cstr(m.word(argv)) == b'chaos.nsh' and m.cstr(m.word(argv + 4)) == b'chaos.nsh' and m.word(argv + 8) == 0, 'expected task-name prepend behavior')
    print('PASS actual task_spawn six-arg forwarding + actual argv builder: argv=NULL yields argc=1; passing name again wrongly yields argc=2')


def probe_waitpid(image):
    m = Machine(image)
    status, record = PARAM + 0x300, PARAM + 0x340
    m.put(CHILD + 8, CHILD_GROUP)
    m.put(CHILD_GROUP + 4, PARENT_PID)
    m.put(PARENT_GROUP + 0x14, record)
    m.put(record, 0)
    m.put(record + 4, 0)
    m.put(record + 8, PID)
    m.put(record + 12, 7)
    m.hooks[0x0C205910] = lambda: CHILD if m.reg(0) == PID else 0
    def no_wait():
        raise AssertionError('nonblocking wait reached a signal wait')
    m.hooks[0x0C207DE0] = no_wait
    m.put(status, 0xDEADBEEF)
    m.u.reg_write(UC_ARM_REG_BASEPRI, 0x20)
    require(m.call(0x0C2063C4, PID, status, 2) == 0, 'WNOHANG must return 0 for live child')
    require(m.word(status) == 0xDEADBEEF and m.word(PARENT_GROUP + 0x14) == record, 'live child was reaped or status modified')
    require(m.u.reg_read(UC_ARM_REG_BASEPRI) == 0x20, 'waitpid did not restore BASEPRI')
    m.put(record + 4, 4)
    require(m.call(0x0C2063C4, PID, status, 2) == PID, 'exited child result')
    require(m.word(status) == 7 << 8, 'exit status must be shifted 8 bits')
    require(m.word(PARENT_GROUP + 0x14) == 0, 'child record not unlinked')
    require(m.word(0x200F5CB0 + 0x100) == record, 'actual child-record recycle not reached')
    require(m.call(0x0C2063C4, PID, status, 2) == -1 and m.word(TLS + 0x3C) == 10, 'reaped child should return -1/ECHILD')
    print('PASS actual waitpid WNOHANG=2: live=>0; exited=>pid/status<<8 and real record recycle; absent=>-1 errno=ECHILD(10)')


def probe_console_sleep_errno(image):
    m = Machine(image)
    console = m.call(0x0C238E28, 1)
    require(m.allocations[console] == (USER_HEAP, 0x5B8), 'console allocation')
    require([m.word(console + x) for x in (0x4AC, 0x4B0, 0x4B4)] == [0, 1, 2], 'console default fd fields')
    require(m.word(console + 0x2C) == 0x0C238C19, 'console exit callback')
    require(m.byte(console + 0x4A8) == 1, 'console mode field')
    closed = []
    exit_codes = []
    def close_fd():
        closed.append(m.reg(0))
        return 0
    def exit_task():
        exit_codes.append(m.reg(0))
        m.u.reg_write(UC_ARM_REG_PC, RETURN)
        return None
    m.hooks[0x0C1B9E48] = close_fd
    m.hooks[0x0C1EBD08] = exit_task
    # Clones have their own allocation and fresh native callbacks. Releasing
    # one must not free the live session console or copy an instance hook.
    m.put(console + 0x2C, 0x201FE001)
    clone = m.call(m.word(console), console)
    require(clone != console and m.word(clone + 0x2C) == 0x0C238C19, 'clone inherited instance exit hook')
    require(m.byte(clone + 0x4A8) == 1, 'clone did not preserve mode')
    m.call(m.word(clone + 8), clone)
    require(m.frees == [clone] and console in m.allocations and not closed, 'release must free only the clone')
    m.put(console + 0x2C, 0x0C238C19)
    # The actual interactive session returns on input failure without releasing
    # the original console. The prompt and I/O are modeled leaves, not parser
    # or on-device terminal coverage. The owner must delegate +0x2C afterward.
    prompt = PARAM + 0x380
    m.u.mem_write(prompt, b'nsh> \0')
    m.hooks[0x0C23D560] = lambda: prompt
    m.hooks[0x0C1D2640] = lambda: m.reg(2)
    m.hooks[0x0C23FD58] = lambda: -1
    m.hooks[0x0C1EAA78] = lambda: 0
    require(m.call(0x0C23DDAC, console, 0, 1, 0) == 0, 'interactive EOF return')
    require(console in m.allocations and m.frees == [clone], 'session released caller-owned console')
    # The actual callback frees the owned console, closes nonstandard fds
    # without double-closing stdout==stderr, then exits (does not return).
    for off, fd in ((0x4AC, 7), (0x4B0, 8), (0x4B4, 8)):
        m.put(console + off, fd)
    m.call(0x0C238C18, console, 9)
    require(closed == [8, 7] and m.frees == [clone, console] and exit_codes == [9], 'actual console exit ownership/order')
    # Command table 0x2CCDB428 resolves "exit" to this exact builtin; it
    # dispatches the instance +0x2C callback with status 0, not a global exit.
    builtin_console = m.call(0x0C238E28, 1)
    m.call(0x0C239130, builtin_console, 1, 0)
    require(m.frees == [clone, console, builtin_console] and exit_codes == [9, 0], 'exit builtin bypassed instance lifecycle')
    sleeps = []
    def nanosleep():
        require(m.reg(0) == 0 and m.reg(1) == 0, 'usleep clock/flags')
        p = m.reg(2)
        sleeps.append((m.word(p), m.word(p + 4), m.word(p + 8)))
        return 0
    m.hooks[0x0C2089DC] = nanosleep
    require(m.call(0x0C1F5D0C, 20000) == 0, 'usleep failed')
    require(sleeps == [(0, 0, 20000000)], 'usleep microseconds conversion')
    require(m.call(0x0C1E45BC) == TLS + 0x3C, 'TLS errno offset')
    print('PASS actual console ctor/clone/release, session EOF retains ownership, exit builtin/+0x2C paired free; usleep(20000)=>20ms, TLS errno+0x3C')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('image', nargs='?', type=Path, default=IMAGE)
    args = p.parse_args()
    if not args.image.is_file():
        print(f'SKIP: audited firmware image missing: {args.image}', file=sys.stderr)
        return 77
    image = args.image.read_bytes()
    require(hashlib.sha256(image).hexdigest() == SHA256, 'wrong AP: only exact p67/3.101.043 is supported')
    for probe in (probe_attributes, probe_actions, probe_child_isolation, probe_spawn_and_argv, probe_waitpid, probe_console_sleep_errno):
        probe(image)
    print('HOST ABI ONLY: physical scheduling, PTY/GPU, device power and crash stability are NOT verified.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
