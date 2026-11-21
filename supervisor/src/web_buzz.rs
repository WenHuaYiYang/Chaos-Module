//! 网页同步器上传接口: 把手机分析出的 `.buzz` 序列落到
//! `/data/chaos/music/buzzN.buzz`(N = 1..6), 供「马达音乐」页播放。
//!
//! 调用方是内置 HTTP 服务器(chaos_httpd.c)的 `/api/buzz/save?slot=N`,
//! 数据在请求体内、未落盘; 这里只做校验 + 写文件, 不碰 UI。

use crate::fw_api;

const DIR: &[u8] = b"/data/chaos/music/\0";
const MAX_LEN: u32 = 6144;
const MAX_EVENTS: usize = 2048;

unsafe fn path_for(slot: u32, out: &mut [u8; 40]) -> *const u8 {
    let mut n = 0usize;
    for &b in DIR {
        if b == 0 {
            break;
        }
        out[n] = b;
        n += 1;
    }
    for &b in b"buzz" {
        out[n] = b;
        n += 1;
    }
    out[n] = b'0' + slot as u8;
    n += 1;
    for &b in b".buzz" {
        out[n] = b;
        n += 1;
    }
    out[n] = 0;
    out.as_ptr()
}

/// 0 成功; -22 参数/内容非法; -5 IO 失败。
///
/// # Safety
/// `data` 必须指向 `len` 字节的已映射内存(HTTP 请求体缓冲)。
#[no_mangle]
pub unsafe extern "C" fn chaos_web_buzz_offer(slot: u32, data: *const u8, len: u32) -> i32 {
    if !(1..=6).contains(&slot) || data.is_null() || len < 8 || len > MAX_LEN {
        return -22;
    }
    let b = data;
    if core::ptr::read_volatile(b) != b'B'
        || core::ptr::read_volatile(b.add(1)) != b'Z'
        || core::ptr::read_volatile(b.add(2)) != b'0'
        || core::ptr::read_volatile(b.add(3)) != b'1'
    {
        return -22;
    }
    let count = core::ptr::read_volatile(b.add(4)) as usize
        | (core::ptr::read_volatile(b.add(5)) as usize) << 8;
    if count == 0 || count > MAX_EVENTS || len < (8 + count * 3) as u32 {
        return -22;
    }
    // 目录可能还不存在(首次上传); 失败不致命, 后面的 open 会给出真实错误。
    let _ = crate::fw_api::capture_mkdir_path(DIR.as_ptr());
    let mut path = [0u8; 40];
    let p = path_for(slot, &mut path);
    let fd = fw_api::open(p, fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 {
        return -5;
    }
    let want = (8 + count * 3) as i32;
    let wrote = fw_api::write(fd, data, want as u32);
    fw_api::close(fd);
    if wrote != want {
        return -5;
    }
    0
}
