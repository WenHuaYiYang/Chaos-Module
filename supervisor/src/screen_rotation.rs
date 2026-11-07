//! 屏幕翻转：180° 翻转开关（设置 -> 屏幕翻转）。
//!
//! 走 **LVGL 的 display 旋转**（`lv_display_set_rotation`），不是对象的
//! `transform_rotation` —— 后者只转绘制，触摸坐标不跟着转，翻过来点哪都不对。
//! display 级旋转由 LVGL 自己同步分辨率与输入设备映射，180° 下宽高不变。

use crate::*;
use core::sync::atomic::{AtomicU8, Ordering};

const CONFIG: &[u8] = b"/data/chaos/screen-rotation.cfg\0";
/// 配置值：1 = 正常方向，2 = 翻转 180°。0 只表示"还没读过"。
const VALUE_NORMAL: u8 = 1;
const VALUE_FLIPPED: u8 = 2;
const ROTATION_0: u32 = 0;
const ROTATION_180: u32 = 2;

static VALUE: AtomicU8 = AtomicU8::new(0);
/// 最近一次真正下发给 LVGL 的状态，避免每帧重复调用。
static APPLIED: AtomicU8 = AtomicU8::new(0);

fn load() {
    if VALUE.load(Ordering::Acquire) != 0 {
        return;
    }
    let mut value = VALUE_NORMAL;
    unsafe {
        let fd = fw_api::open(CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
        if fd >= 0 {
            let mut byte = VALUE_NORMAL;
            if fw_api::read(fd, &mut byte, 1) == 1 && byte == VALUE_FLIPPED {
                value = VALUE_FLIPPED;
            }
            fw_api::close(fd);
        }
    }
    VALUE.store(value, Ordering::Release);
}

fn persist(value: u8) {
    unsafe {
        let fd = fw_api::open(
            CONFIG.as_ptr(),
            fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
            0o640,
        );
        if fd >= 0 {
            let _ = fw_api::write(fd, &value, 1);
            fw_api::close(fd);
        }
    }
}

pub(crate) fn enabled() -> bool {
    load();
    VALUE.load(Ordering::Acquire) == VALUE_FLIPPED
}

/// 翻转前那一刻的 disp 结构快照，用于在调试页做前后对比。
static mut BEFORE: [u8; 64] = [0; 64];
static mut BEFORE_VALID: bool = false;

/// 取翻转前的快照；没有翻转记录时返回 None。
pub(crate) unsafe fn before() -> Option<&'static [u8; 64]> {
    if BEFORE_VALID {
        Some(&*core::ptr::addr_of!(BEFORE))
    } else {
        None
    }
}

/// 翻转显示方向。返回切换后的状态（true = 已翻转 180°）。
pub(crate) unsafe fn toggle() -> bool {
    load();
    let (_, snapshot) = probe();
    BEFORE = snapshot;
    BEFORE_VALID = true;
    let next = if enabled() { VALUE_NORMAL } else { VALUE_FLIPPED };
    VALUE.store(next, Ordering::Release);
    persist(next);
    apply_now(next);
    next == VALUE_FLIPPED
}

unsafe fn apply_now(value: u8) {
    let disp = fw_api::display_default();
    if disp == 0 {
        return;
    }
    fw_api::display_set_rotation(disp, if value == VALUE_FLIPPED { ROTATION_180 } else { ROTATION_0 });
    APPLIED.store(value, Ordering::Release);
    // 旋转只重排已提交的像素；已建页面的内容树还带着旧方向的缓存，整树重画一次，
    // 免得切页时新旧方向的内容叠在一起（表现为一条横向接缝）。
    invalidate_pages();
}

/// 把所有已建页面的内容根标脏。`obj_invalidate` 只重画这一棵子树，不发样式事件。
unsafe fn invalidate_pages() {
    let pages: &[crate::state::Page] = &*core::ptr::addr_of!(crate::state::PAGES);
    for page in pages.iter() {
        if page.content != 0 {
            fw_api::obj_invalidate(page.content);
        }
    }
}

/// 页面渲染时调用：只在"配置值"与"已下发值"不一致时动 display。
/// display 还没就绪（返回 0）就留到下一次，不动状态。
pub(crate) unsafe fn ensure_applied() {
    load();
    let want = if enabled() { VALUE_FLIPPED } else { VALUE_NORMAL };
    if APPLIED.load(Ordering::Acquire) == want {
        return;
    }
    apply_now(want);
}

/// 只读探测：返回 display 指针与 `disp+0x20..0x5F` 的 64 个字节。
///
/// LVGL 9 的 `lv_display_t` 里 `render_mode`(partial/direct/full) 和 `rotation`
/// 就落在这附近：固件 `0x0C13D8FA` 读 `+0x39` 并断言取值 1..2 —— 而 LVGL 9 的
/// render_mode 枚举正是 0=PARTIAL 1=DIRECT 2=FULL，抗锯齿只在后两种下有效，
/// 所以 `+0x39` 极可能就是 render_mode（实测未翻转时为 1 = DIRECT）。
/// 翻转前后各读一次做对比，就能把 render_mode 和 rotation 分开。
pub(crate) unsafe fn probe() -> (u32, [u8; 64]) {
    let disp = fw_api::display_default();
    let mut bytes = [0u8; 64];
    if disp != 0 {
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = core::ptr::read_volatile((disp as usize + 0x20 + i) as *const u8);
        }
    }
    (disp, bytes)
}
