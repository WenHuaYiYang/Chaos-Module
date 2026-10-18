//! 系统日期时间设置: `/dev/rtc0` + ioctl(0x1902)。
//!
//! 固件 rtc.c 的 ioctl 分发(文件 0x0EE8C8)对 0x1902 的动作: 先调驱动 settime, 成功后
//! 调 `clock_synchronize`(0x0C201DAC) 由 RTC 回写系统时钟 —— 所以不需要另找
//! clock_settime。0x1901 是读时间。
//!
//! 参数缓冲是 9 个 int 的 `struct tm`(tm_mon 0 起 / tm_year 从 1900 起):
//! 固件 date 命令把 0x1901 读回的同一块缓冲直接交给 strftime, 布局已证。
//! 注意: 这份固件的 `struct tm` 一共 44 字节(9 个 int 之后还有 gmtoff/zone 两个
//! 字), localtime_r 会写满 44 字节。缓冲必须按 44 字节给, 只给 36 字节会写穿栈。

use crate::fw_api;

const RTC_PATH: &[u8] = b"/dev/rtc0\0";
const RTC_SET_TIME: u32 = 0x1902;

fn leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if leap(y) { 29 } else { 28 },
        _ => 0,
    }
}

/// 0 = 周日(Sakamoto)。
fn weekday(y: u32, m: u32, d: u32) -> i32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let mut yy = y as i32;
    if m < 3 { yy -= 1; }
    (yy + yy / 4 - yy / 100 + yy / 400 + T[(m - 1) as usize] + d as i32) % 7
}

/// 1970-01-01 起的天数(Howard Hinnant 算法, 全是 i64, 2000..2099 不溢出)。
fn days_from_civil(y: i32, m: i32, d: i32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = ((153 * mp + 2) / 5 + d - 1) as i64;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((if m <= 2 { y + 1 } else { y }) as i32, m as i32, d as i32)
}

/// 本地时区偏移(分钟, 东八区 = 480)。用同一时刻的 localtime_r 与 gmtime_r 相减。
unsafe fn local_offset_min() -> Option<i32> {
    let mut ts = [0u64; 2];
    let cg: unsafe extern "C" fn(u32, *mut u64) -> i32 = core::mem::transmute(0x0C20_1FE1usize);
    if cg(0, ts.as_mut_ptr()) != 0 { return None; }
    let mut loc = [0i32; 11];
    let mut utc = [0i32; 11];
    let lt: unsafe extern "C" fn(*const u8, *mut i32) -> *mut i32 =
        core::mem::transmute(0x0C1F_49ADusize);
    let gt: unsafe extern "C" fn(*const u8, *mut i32) -> *mut i32 =
        core::mem::transmute(0x0C1F_4A71usize);
    if lt(ts.as_ptr() as *const u8, loc.as_mut_ptr()).is_null() { return None; }
    if gt(ts.as_ptr() as *const u8, utc.as_mut_ptr()).is_null() { return None; }
    let lm = days_from_civil(loc[5] + 1900, loc[4] + 1, loc[3]) * 1440
        + (loc[2] * 60 + loc[1]) as i64;
    let um = days_from_civil(utc[5] + 1900, utc[4] + 1, utc[3]) * 1440
        + (utc[2] * 60 + utc[1]) as i64;
    Some((lm - um) as i32)
}

pub(crate) unsafe fn set(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i32 {
    if !(2000..=2099).contains(&y) || mo < 1 || mo > 12 || d < 1 || d > days_in_month(y, mo)
        || h > 23 || mi > 59 || s > 59 {
        return -22;
    }
    // 固件 RTC/系统基准是 UTC: clock_synchronize 把 RTC 字段当 UTC 算 epoch。
    // 用户给的是本地字段, 写 RTC 前必须减时区偏移; 否则东八区会多走 8 小时。
    let off = match local_offset_min() { Some(v) => v, None => return -5 };
    let total = days_from_civil(y as i32, mo as i32, d as i32) * 1440
        + (h * 60 + mi) as i64 - off as i64;
    let udays = total.div_euclid(1440);
    let umins = total.rem_euclid(1440);
    let (uy, umo, ud) = civil_from_days(udays);
    let uh = (umins / 60) as u32;
    let umi = (umins % 60) as u32;
    let mut yday = ud - 1;
    let mut k = 1i32;
    while k < umo { yday += days_in_month(uy as u32, k as u32) as i32; k += 1; }
    let mut tm = [0i32; 11]; // 44 字节 = 这份固件的 sizeof(struct tm)
    tm[0] = s as i32;
    tm[1] = umi as i32;
    tm[2] = uh as i32;
    tm[3] = ud;
    tm[4] = umo - 1;
    tm[5] = uy - 1900;
    tm[6] = weekday(uy as u32, umo as u32, ud as u32);
    tm[7] = yday;
    tm[8] = 0;
    // 先按可读写开; 有些设备节点对 VFS 的写标志不感冒(固件 date 命令就用只读开),
    // 失败再退只读 —— ioctl 的 0x1902 分支不看 fd 的读写标志。
    let mut fd = fw_api::open(RTC_PATH.as_ptr(), fw_api::oflag::RDWR, 0);
    if fd < 0 {
        fd = fw_api::open(RTC_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    }
    if fd < 0 { return fd; }
    let rc = fw_api::ioctl(fd, RTC_SET_TIME, tm.as_mut_ptr() as u32);
    fw_api::close(fd);
    if rc < 0 { rc } else { 0 }
}

/// 系统当前本地时间(9 个 int 的 struct tm)。`clock_gettime(0)` + `localtime_r`。
/// 依据: 固件 date 命令 0x0C23CC34 就是这么读的(0x0C201FE0 / 0x0C1F49AC)。
pub(crate) unsafe fn now_tm() -> Option<[i32; 9]> {
    // 8 字节对齐: clock_gettime 用 strd 写 64 位 tv_sec, 缓冲只给 1/4 对齐可能触发对齐异常
    let mut ts = [0u64; 2]; // 16 字节 = 64 位 time_t + nsec
    let cg: unsafe extern "C" fn(u32, *mut u64) -> i32 = core::mem::transmute(0x0C20_1FE1usize);
    if cg(0, ts.as_mut_ptr()) != 0 { return None; }
    let mut tm = [0i32; 11]; // localtime_r 写满 44 字节, 返回时只取前 9 个字
    let lt: unsafe extern "C" fn(*const u8, *mut i32) -> *mut i32 =
        core::mem::transmute(0x0C1F_49ADusize);
    if lt(ts.as_ptr() as *const u8, tm.as_mut_ptr()).is_null() { return None; }
    Some([tm[0], tm[1], tm[2], tm[3], tm[4], tm[5], tm[6], tm[7], tm[8]])
}
