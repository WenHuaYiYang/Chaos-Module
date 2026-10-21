//! 运行日志: 追加到 `/data/chaos/chaos.log`, 由「运行日志」页读取/清空。
//!
//! 只记录 Chaos 自己的关键动作(隐藏/显示、批量、开机补偿、模块加载),
//! 不接管固件日志, 也不做后台线程 —— 每次写一行, 文件超过上限就整份重建。
//! 日志页在 UI 线程读文件, 单次最多读尾部 2KB, 避免拖慢渲染。

use crate::fw_api;

pub(crate) const LOG_PATH: &[u8] = b"/data/chaos/chaos.log\0";
const LOG_MAX: u32 = 48 * 1024;

/// 追加一行。失败静默: 日志不能影响主流程。
pub(crate) unsafe fn line(s: &str) {
    bytes(s.as_bytes());
}

pub(crate) unsafe fn bytes(b: &[u8]) {
    if b.is_empty() { return; }
    let mut fd = fw_api::open(LOG_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return; }
    let end = fw_api::lseek(fd, 0, fw_api::seek::END);
    if end < 0 || end as u32 >= LOG_MAX {
        fw_api::close(fd);
        let _ = fw_api::fs_remove(LOG_PATH.as_ptr());
        fd = fw_api::open(LOG_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
        if fd < 0 { return; }
    }
    let n = core::cmp::min(b.len(), 180);
    let _ = fw_api::write(fd, b.as_ptr(), n as u32);
    let _ = fw_api::write(fd, b"\n".as_ptr(), 1);
    fw_api::close(fd);
}

/// 写一个 NUL 结尾的 C 串(不把 NUL 写进文件)。
pub(crate) unsafe fn cstr(p: *const u8) {
    let mut n = 0usize;
    while n < 180 && *p.add(n) != 0 { n += 1; }
    if n != 0 { bytes(core::slice::from_raw_parts(p, n)); }
}

/// 清空日志(删文件, 下次写入时重建)。
pub(crate) unsafe fn clear() -> bool {
    fw_api::fs_remove(LOG_PATH.as_ptr()) == 0
}

/// 读日志尾部到调用方缓冲, 返回字节数。只保留最后 `cap` 字节, 从完整行开始。
pub(crate) unsafe fn tail(buf: *mut u8, cap: usize) -> usize {
    let fd = fw_api::open(LOG_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return 0; }
    let end = fw_api::lseek(fd, 0, fw_api::seek::END);
    if end <= 0 { fw_api::close(fd); return 0; }
    let mut start = if (end as usize) > cap { end as usize - cap } else { 0 };
    let _ = fw_api::lseek(fd, start as i64, fw_api::seek::SET);
    let mut n = 0usize;
    while n < cap {
        let got = fw_api::read(fd, buf.add(n), (cap - n) as u32);
        if got <= 0 { break; }
        n += got as usize;
    }
    fw_api::close(fd);
    // 从行首开始, 丢掉被截断的半行。
    if start != 0 {
        let mut i = 0usize;
        while i < n && *buf.add(i) != b'\n' { i += 1; }
        if i < n { i += 1; }
        if i > 0 && i < n {
            core::ptr::copy(buf.add(i), buf, n - i);
            n -= i;
        } else if i >= n {
            n = 0;
        }
    }
    n
}
