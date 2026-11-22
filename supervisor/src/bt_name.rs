//! 蓝牙广播名: 通过蓝牙服务的 bttool 通道(cmd 0x22)改 adapter 本地名。
//!
//! **必须由 chaos.net worker 执行**: adapter/registration 只在该线程的 prepare()
//! 里创建, 在 supervisor 线程直接调 bt_probe 会拿到 -601(adapter 为空)。所以这里
//! 只入队(network_set_bt_name), 结果由 tick() 每拍回读 —— 之前直接同步调用正是
//! "已保存但一直未生效"的原因。
//!
//! 固件重启后会按系统"设备名称"重新应用一次广播名, 所以持久化由这里负责:
//! 用户设置成功就写 `/data/chaos/bt_name.txt`, 开机后维护节拍里补下发一次
//! (失败按拍重试), 这样重启后仍是用户设定的名字。

use crate::fw_api;

const CONFIG: &[u8] = b"/data/chaos/bt_name.txt\0";
const CAP: usize = 32;

static mut NAME: [u8; CAP] = [0; CAP];
static mut LEN: usize = 0;
static mut LOADED: bool = false;
/// 本次开机是否已成功下发(成功一次就不再重复, 避免每拍调蓝牙服务)。
static mut APPLIED: bool = false;
/// 已重试次数(以拍计, 约 0.6s/次): 蓝牙服务就绪前会连续失败, 上限后放弃本次开机。
static mut RETRIES: u32 = 0;
/// 已入队、等 worker 回结果。
static mut SUBMITTED: bool = false;
/// 已等待的拍数: 超时视为请求丢失, 重新入队。
static mut WAIT: u32 = 0;
const WAIT_LIMIT: u32 = 8;
/// worker 回了一定结果但页面还没重绘过(页面据此请求刷新一次)。
static mut SETTLED: bool = false;
/// 最近一次下发的结果: 0 成功, 其它为错误码(供页面显示)。
static mut LAST_RC: i32 = 0;
static mut HAS_RESULT: bool = false;
const RETRY_LIMIT: u32 = 120;

extern "C" {
    /// 入队(由 PAN worker 执行): 0 已入队, -22 名字非法。
    fn chaos_net_set_bt_name(name: *const u8) -> i32;
    /// worker 最近一次执行结果: -999 尚无结果。
    fn chaos_net_bt_name_result() -> i32;
}

#[inline]
fn valid(c: u8) -> bool {
    // 与蓝牙服务一致: 允许 UTF-8 多字节(中文名)与常见可见字符, 排除控制字符。
    (c >= 0x20 && c != 0x7f) || c >= 0x80
}

pub(crate) unsafe fn ensure_loaded() {
    if LOADED {
        return;
    }
    LOADED = true;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return;
    }
    let mut buf = [0u8; CAP];
    let got = fw_api::read(fd, buf.as_mut_ptr(), CAP as u32);
    fw_api::close(fd);
    if got <= 0 {
        return;
    }
    let mut n = 0usize;
    while n < got as usize && n < CAP - 1 && buf[n] != 0 {
        NAME[n] = buf[n];
        n += 1;
    }
    NAME[n] = 0;
    LEN = n;
}

/// 当前设定的名字(空串 = 未设置, 保持系统广播名)。
pub(crate) unsafe fn current() -> *const u8 {
    ensure_loaded();
    core::ptr::addr_of!(NAME) as *const u8
}

pub(crate) unsafe fn has_custom() -> bool {
    ensure_loaded();
    LEN != 0
}

/// 是否启用了自定义广播名(有保存的名字即启用)。
pub(crate) unsafe fn enabled() -> bool {
    has_custom()
}

/// 入队一次下发请求(worker 执行, 结果异步回读)。
unsafe fn submit(ptr: *const u8) -> i32 {
    if ptr.is_null() {
        return -22;
    }
    crate::btpan::ensure_worker();
    let rc = chaos_net_set_bt_name(ptr);
    if rc == 0 {
        SUBMITTED = true;
        WAIT = 0;
    }
    rc
}

/// 立即下发当前设定的名字(入队)。0 表示已提交; 结果见 last_result()。
pub(crate) unsafe fn apply() -> i32 {
    ensure_loaded();
    if LEN == 0 {
        return -22;
    }
    if !crate::fw_api::screen_is_on() {
        return -11;
    }
    submit(core::ptr::addr_of!(NAME) as *const u8)
}

/// 保存用户输入并入队下发。返回 true = 已保存(键盘可以关); 下发结果异步。
pub(crate) unsafe fn save(text: *const u8) -> bool {
    ensure_loaded();
    if text.is_null() {
        return false;
    }
    let mut len = 0usize;
    while len < CAP - 1 {
        let c = core::ptr::read_volatile(text.add(len));
        if c == 0 {
            break;
        }
        if !valid(c) {
            return false;
        }
        NAME[len] = c;
        len += 1;
    }
    if len == 0 {
        return false;
    }
    NAME[len] = 0;
    LEN = len;
    persist();
    APPLIED = false;
    RETRIES = 0;
    let rc = apply();
    if rc != 0 {
        // 入队就失败(名称非法/屏幕熄灭): 直接给出结果。
        LAST_RC = rc;
        HAS_RESULT = true;
        SETTLED = true;
    }
    true
}

/// 页面还需要逐拍跟进: 正在等 worker 回结果, 或结果刚到还没重绘过。
pub(crate) unsafe fn pending_result() -> bool {
    SUBMITTED || SETTLED
}

/// 取走"结果刚到"标志(取走即清), 页面据此重绘一次。
pub(crate) unsafe fn take_settled() -> bool {
    let s = SETTLED;
    SETTLED = false;
    s
}

/// 最近一次下发结果: None = 本次开机还没结果(还在等 worker)。
pub(crate) unsafe fn last_result() -> Option<i32> {
    if HAS_RESULT {
        Some(LAST_RC)
    } else {
        None
    }
}

/// 恢复设备默认广播名: 清掉自定义设置, 并立刻把系统「设备名称」入队下发。
pub(crate) unsafe fn restore_default() -> i32 {
    ensure_loaded();
    LEN = 0;
    NAME[0] = 0;
    APPLIED = true; // 本次开机不再按 tick 自动下发
    RETRIES = 0;
    HAS_RESULT = false;
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 {
        return -5;
    }
    let _ = fw_api::write(fd, b"\0".as_ptr(), 1);
    fw_api::close(fd);
    if !crate::fw_api::screen_is_on() {
        return -11;
    }
    submit(crate::device_name::current_text())
}

/// 开关翻转: 关 -> 恢复设备默认; 开 -> 下发已保存的名字。
/// 没有保存过名字时返回 false(页面据此弹键盘让用户先输入)。
pub(crate) unsafe fn set_enabled(on: bool) -> bool {
    ensure_loaded();
    if on {
        if LEN == 0 {
            return false;
        }
        APPLIED = false;
        RETRIES = 0;
        HAS_RESULT = false;
        apply();
        true
    } else {
        restore_default();
        true
    }
}

unsafe fn persist() {
    let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 {
        return;
    }
    let _ = fw_api::write(fd, core::ptr::addr_of!(NAME) as *const u8, LEN as u32);
    fw_api::close(fd);
}

/// 维护节拍: 先收 worker 的结果, 再决定要不要(重新)入队。
pub(crate) unsafe fn tick() {
    ensure_loaded();

    if SUBMITTED {
        let rc = chaos_net_bt_name_result();
        if rc == -999 {
            // worker 还没执行到(它每 250ms 一拍): 等几拍, 超时视为丢失。
            WAIT += 1;
            if WAIT < WAIT_LIMIT {
                return;
            }
            SUBMITTED = false;
        } else {
            SUBMITTED = false;
            LAST_RC = rc;
            HAS_RESULT = true;
            SETTLED = true;
            if rc == 0 {
                APPLIED = true;
                RETRIES = 0;
            } else if LEN != 0 {
                RETRIES += 1;
            }
            return;
        }
    }

    if APPLIED || RETRIES >= RETRY_LIMIT || LEN == 0 {
        return;
    }
    if !crate::fw_api::screen_is_on() {
        return;
    }
    if apply() != 0 {
        RETRIES += 1;
    }
}
