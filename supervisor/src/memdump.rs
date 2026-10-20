//! 临时调试: dump 运行时 LVGL 相关内存。
//!
//! 1) 0x1C060000..0x1C0A0000 -> /data/chaos/lvgl_dump.bin   (代码段)
//! 2) 0x2CCE0000..0x2CD00000 -> /data/chaos/lvgl_rodata.bin (类描述/只读数据)
//!
//! 每个区各跑一次，分块写避免长时间占用 UI 定时器。

use crate::fw_api;
use core::ptr;

struct Region {
    start: usize,
    len: usize,
    path: &'static [u8],
    done: &'static [u8],
}

const REGIONS: [Region; 2] = [
    Region {
        start: 0x1C06_0000,
        len: 0x0004_0000,
        path: b"/data/chaos/lvgl_dump.bin\0",
        done: b"/data/chaos/lvgl_dump.ok\0",
    },
    Region {
        start: 0x2CCE_0000,
        len: 0x0002_0000,
        path: b"/data/chaos/lvgl_rodata.bin\0",
        done: b"/data/chaos/lvgl_rodata.ok\0",
    },
];

const CHUNK: usize = 16 * 1024;

static mut REGION: usize = 0;
static mut FD: i32 = -2;
static mut OFF: usize = 0;
static mut BUF: [u8; CHUNK] = [0; CHUNK];

unsafe fn exists(path: &[u8]) -> bool {
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        fw_api::close(fd);
        true
    } else {
        false
    }
}

unsafe fn touch(path: &[u8]) {
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 {
        fw_api::write(fd, b"ok\0".as_ptr(), 3);
        fw_api::close(fd);
    }
}

/// 每拍调用一次。
pub(crate) unsafe fn tick() {
    if REGION >= REGIONS.len() { return; }
    let r = &REGIONS[REGION];
    if FD == -2 {
        if exists(r.done) {
            REGION += 1;
            return;
        }
        let fd = fw_api::open(r.path.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
        if fd < 0 {
            REGION += 1;
            return;
        }
        FD = fd;
        OFF = 0;
    }
    let n = if r.len - OFF < CHUNK { r.len - OFF } else { CHUNK };
    let src = (r.start + OFF) as *const u8;
    let mut i = 0usize;
    while i < n {
        BUF[i] = ptr::read_volatile(src.add(i));
        i += 1;
    }
    let w = fw_api::write(FD, BUF.as_ptr(), n as u32);
    if w <= 0 {
        fw_api::close(FD);
        FD = -2;
        REGION += 1;
        return;
    }
    OFF += w as usize;
    if OFF >= r.len {
        fw_api::close(FD);
        touch(r.done);
        FD = -2;
        REGION += 1;
    }
}
