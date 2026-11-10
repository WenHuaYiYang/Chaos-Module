#!/usr/bin/env python3
"""Run exact AP file_poll -> fb_poll -> POLLPRI notifier -> teardown bytes.

No display, DMA or IRQ scheduling is emulated. The callback is a host model
which acknowledges revents just like the production atomic-counter callback.
"""
from pathlib import Path
import argparse
import hashlib
import struct
from probe_nsh_spawn import Machine, SHA256, require, UC_ARM_REG_PC

def verify(data):
    require(hashlib.sha256(data).hexdigest() == SHA256, "wrong firmware")
    events = []
    class CaptureMachine(Machine):
        def on_code(self, u, address, size, user):
            if address == callback:
                events.append(self.word(poll + 8)); self.put(poll + 8, 0)
                self.put(poll + 24, self.word(poll + 24) + 1)
                # Model the Rust atomic increment, then execute the real native
                # poll callback that coalesces and posts the wake semaphore.
                self.u.reg_write(UC_ARM_REG_PC, 0x0C1D0B15)
                return
            super().on_code(u, address, size, user)
    m = CaptureMachine(data)
    file, inode, dev, vtable, ctx, poll, ring, callback = (
        0x20030000, 0x20030100, 0x20030200, 0x20030300,
        0x20030400, 0x20030500, 0x20030600, 0x201FE000)
    m.put(file + 16, inode); m.put(file + 20, ctx)
    m.put(inode + 12, 0x10000) # inode i_flags u16 at +14: character device=1
    m.put(inode + 16, 0x2CCBEC54) # native read/seek/ioctl/poll operations
    m.put(inode + 24, dev); m.put(dev, vtable); m.put(dev + 16, ctx)
    m.put(vtable + 88, dev); m.put(dev + 24, 1); m.put(dev + 20, ring - 56)
    m.put(ctx + 4, 0xffffffff); m.put(poll, 3); m.put(poll + 4, 2)
    m.put(poll + 16, callback | 1)
    semaphore = poll + 32
    m.put(poll + 12, semaphore); m.put(poll + 44, 0xdecafbad)
    require(m.call(0x0C1EA25C, semaphore, 0, 0) == 0, "native 12-byte semaphore init")
    require(m.word(poll + 44) == 0xdecafbad, "semaphore initializer overwrote storage")
    m.hooks[0x0C1E63C4] = lambda: 1 # ring not empty; no immediate POLLOUT
    posts = []
    def post():
        require(m.reg(0) == semaphore, "wake used a different semaphore")
        value = m.word(semaphore) & 0xffff
        posts.append(value); m.put(semaphore, (value + 1) & 0xffff); return 0
    # Scheduler wakeup remains a model; native default callback/getvalue execute.
    m.hooks[0x0C2070D0] = post
    require(m.call(0x0C1D0BA4, file, poll, 1) == 0, "file_poll registration")
    require(m.word(ctx + 8) == poll and m.word(poll + 20) == ctx + 8, "24-byte pollfd/priv offsets")
    # ISR callback can acknowledge then receive a second event; a held bit
    # would cause the native poll_notify implementation to coalesce it.
    m.call(0x0C1AF91C, vtable); m.call(0x0C1AF91C, vtable)
    require(events == [2, 2], "repeat vsync POLLPRI delivery")
    require(len(posts) == 1 and m.word(semaphore) == 1, "wake tokens were not coalesced")
    m.put(semaphore, 0); m.call(0x0C1AF91C, vtable)
    require(len(posts) == 2 and m.word(poll + 24) == 3, "next consumed token did not wake again")
    m.put(semaphore, 0xffff); m.call(0x0C1AF91C, vtable)
    require(posts[-1] == 0xffff and m.word(semaphore) == 0, "blocked-task wake path")
    require(m.call(0x0C1D0BA4, file, poll, 0) == 0, "teardown")
    require(m.word(ctx + 8) == 0 and m.word(poll + 20) == 0, "driver must release callback slot")
    m.call(0x0C1AF91C, vtable)
    require(events == [2, 2, 2, 2], "callback after teardown")
    verify_snapshot_copy(data)
    print("PASS actual AP: file_poll/POLLPRI, stable storage, native semaphore init/coalescing/wake routing and teardown")
    print("NOT VERIFIED: physical vsync timing, DMA/cache coherency, panel tearing or device frame rate")

def verify_snapshot_copy(data):
    """Execute real fb_read; model video/plane providers and the ROM memcpy leaf."""
    m = Machine(data)
    file, inode, dev, vtable, ctx = (0x20030000, 0x20030100, 0x20030200, 0x20030300, 0x20030400)
    source, destination, size = 0x20050000, 0x20150000, 336 * 480 * 3
    video_provider, plane_provider = 0x201FE100, 0x201FE200
    m.put(file + 16, inode); m.put(file + 20, ctx); m.put(inode + 24, dev)
    m.put(dev, vtable); m.put(vtable, video_provider | 1); m.put(vtable + 4, plane_provider | 1)
    m.put(ctx + 4, 0xffffffff)
    def video():
        m.u.mem_write(m.reg(1), struct.pack("<BBHHBB", 12, 0, 336, 480, 1, 1)); return 0
    def plane():
        m.u.mem_write(m.reg(2), struct.pack("<IIHBBIIII", source, size * 2, 1008, 0, 24, 336, 960, 0, 0)); return 0
    m.hooks[video_provider] = video; m.hooks[plane_provider] = plane
    def copy():
        destination, source, length = m.reg(0), m.reg(1), m.reg(2)
        m.u.mem_write(destination, bytes(m.u.mem_read(source, length))); return destination
    # AP's memcpy thunk jumps to ROM 0x00276661, absent from the AP image.
    m.hooks[0x0C588DB0] = copy
    images = [b"\x11\x22\x33" * (336 * 480), b"\x44\x55\x66" * (336 * 480)]
    m.u.mem_write(source, b"".join(images))
    for index, expected in enumerate(images):
        m.put(file + 8, index * size); m.put(file + 12, 0)
        m.put(destination - 4, 0xdecafbad); m.put(destination + size, 0x12345678)
        require(m.call(0x0C1AF034, file, destination, size) == size, "native fb_read snapshot size")
        require(bytes(m.u.mem_read(destination, size)) == expected, "wrong visible framebuffer window")
        require(m.word(file + 8) == (index + 1) * size, "native read cursor")
        require(m.word(destination - 4) == 0xdecafbad and m.word(destination + size) == 0x12345678, "snapshot copy overflow")
    m.put(file + 8, size * 2 - 17)
    require(m.call(0x0C1AF034, file, destination, size) == 17, "native copy must clamp at framebuffer EOF")
    require(m.call(0x0C1AF034, file, destination, size) == 0, "native framebuffer EOF")
    require(0x0C588DB0 in m.visited, "snapshot did not execute native bulk memcpy")
    print("PASS actual AP: fb_read bulk-copy ABI for both 483840-byte windows, cursor, bounds and EOF (ROM copy leaf modeled)")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    verify(parser.parse_args().image.read_bytes())
