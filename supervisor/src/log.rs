//! 运行日志: 追加到 `/data/chaos/chaos.log`, 由「运行日志」页读取/清空。
//!
//! 每行带本地时间前缀 `[MM-DD HH:MM:SS]`。只记录 Chaos 自己的关键动作
//! (模块加载、隐藏/显示、批量、排除项、模式切换、开机补偿), 不接管固件日志。
//! 文件超过上限就整份重建; 日志页只读尾部, 避免拖慢渲染。

use crate::fw_api;
use crate::text::W;

pub(crate) const LOG_PATH: &[u8] = b"/data/chaos/chaos.log\0";
const LOG_MAX: u32 = 48 * 1024;
const LINE_CAP: usize = 240;

fn pad2(w: &mut W, v: i32) {
    let v = if v < 0 { 0 } else if v > 99 { 99 } else { v };
    unsafe {
        w.c(b'0' + (v / 10) as u8);
        w.c(b'0' + (v % 10) as u8);
    }
}

/// 组装 "时间 + 前缀 + [包名] + [数字]" 后一次性追加。
unsafe fn emit(prefix: &[u8], pkg: *const u8, num: Option<u32>) {
    let mut b = [0u8; LINE_CAP];
    let mut w = W::new(b.as_mut_ptr(), LINE_CAP);
    w.c(b'[');
    if let Some(tm) = crate::time_set::now_tm() {
        pad2(&mut w, tm[4] + 1); w.c(b'-'); pad2(&mut w, tm[3]); w.c(b' ');
        pad2(&mut w, tm[2]); w.c(b':'); pad2(&mut w, tm[1]); w.c(b':'); pad2(&mut w, tm[0]);
    } else {
        w.s(b"??-?? ??:??:??".as_slice());
    }
    w.s(b"] ".as_slice());
    if !prefix.is_empty() { w.s(prefix); }
    if !pkg.is_null() {
        let mut n = 0usize;
        while n < 120 && *pkg.add(n) != 0 { n += 1; }
        if n != 0 { w.s(core::slice::from_raw_parts(pkg, n)); }
    }
    if let Some(v) = num { w.s(b" ".as_slice()); w.n(v); }
    w.end();
    let len = w.len();
    if len == 0 { return; }

    let mut fd = fw_api::open(LOG_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return; }
    let end = fw_api::lseek(fd, 0, fw_api::seek::END);
    if end < 0 || end as u32 >= LOG_MAX {
        fw_api::close(fd);
        let _ = fw_api::fs_remove(LOG_PATH.as_ptr());
        fd = fw_api::open(LOG_PATH.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
        if fd < 0 { return; }
    }
    let _ = fw_api::write(fd, b.as_ptr(), len as u32);
    let _ = fw_api::write(fd, b"\n".as_ptr(), 1);
    fw_api::close(fd);
}

pub(crate) unsafe fn line(s: &str) { emit(s.as_bytes(), core::ptr::null(), None); }
pub(crate) unsafe fn bytes(b: &[u8]) { emit(b, core::ptr::null(), None); }

/// 写一个 NUL 结尾的 C 串(不把 NUL 写进文件)。
pub(crate) unsafe fn cstr(p: *const u8) { emit(b"", p, None); }

/// 前缀 + 包名, 例如 `log::pkg("隐藏 ", pkg)`。
pub(crate) unsafe fn pkg(prefix: &str, p: *const u8) { emit(prefix.as_bytes(), p, None); }

/// 前缀 + 数字, 例如 `log::kv("已处理 ", n)`。
pub(crate) unsafe fn kv(prefix: &str, n: u32) { emit(prefix.as_bytes(), core::ptr::null(), Some(n)); }

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
    let start = if (end as usize) > cap { end as usize - cap } else { 0 };
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
