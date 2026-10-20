//! 临时调试: 把运行时 LVGL 模块的一段内存 dump 到 /data/chaos/lvgl_dump.bin。
//!
//! 背景: `lv_arc_create` 没有 veneer 导出，静态镜像里只有字符串没有实现；
//! 实现位于启动后加载的 0x1C06xxxx 区域。这里按块读内存写文件，用来离线反编译。
//!
//! 只跑一次: 文件已存在就跳过。分块写，避免长时间占用 UI 定时器。

use crate::fw_api;
use core::ptr;

const DUMP_PATH: &[u8] = b"/data/chaos/lvgl_dump.bin\0";
const DONE_PATH: &[u8] = b"/data/chaos/lvgl_dump.ok\0";
const START: usize = 0x1C06_0000;
const LEN: usize = 0x0004_0000;      // 256 KB
const CHUNK: usize = 16 * 1024;

static mut FD: i32 = -2;             // -2=未开始, -1=已结束
static mut OFF: usize = 0;
static mut BUF: [u8; CHUNK] = [0; CHUNK];

/// 每拍调用一次; 16 拍左右完成 256KB。
pub(crate) unsafe fn tick() {
    if FD == -1 { return; }
    if FD == -2 {
        // 已 dump 过 / 已有完成标记 -> 不再跑
        let ok = fw_api::open(DONE_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
        if ok >= 0 {
            fw_api::close(ok);
            FD = -1;
            return;
        }
        let fd = fw_api::open(DUMP_PATH.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
        if fd < 0 { FD = -1; return; }
        FD = fd;
        OFF = 0;
    }
    let n = if LEN - OFF < CHUNK { LEN - OFF } else { CHUNK };
    let src = (START + OFF) as *const u8;
    let mut i = 0usize;
    while i < n {
        BUF[i] = ptr::read_volatile(src.add(i));
        i += 1;
    }
    let w = fw_api::write(FD, BUF.as_ptr(), n as u32);
    if w <= 0 {
        fw_api::close(FD);
        FD = -1;
        return;
    }
    OFF += w as usize;
    if OFF >= LEN {
        fw_api::close(FD);
        let fd = fw_api::open(DONE_PATH.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
        if fd >= 0 {
            fw_api::write(fd, b"ok\0".as_ptr(), 3);
            fw_api::close(fd);
        }
        FD = -1;
    }
}
