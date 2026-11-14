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

// ===== 撕裂修复（2026-11-14 模拟器+真机逆向定案） =====
//
// 机理（逆向实证，见 docs/Chaos_UI重构_20261113.md §9.5）:
//   翻转后 LVGL 的旋转拷贝把区域原地写进绘制缓冲，而面板驱动的流式任务正在
//   **顺序读同一块缓冲**送面板 —— 旋转拷贝与流式读是两个线程的无同步竞争，
//   流出去的就是"半新半旧"的一帧 = 用户看到的横向割裂。
//   固件的 flush_cb(0x0C4B778C) 把失效区域收进队列后立即返回（异步），它自己
//   在 busy(驱动 ctx+0x2B6) 非零时会用 ctx+0x2B8 的信号量**阻塞等待**上一批
//   完成 —— 说明信号量的语义就是"驱动持有 = 正在流式，释放 = 本批完成"。
// 修法(第一版 disp+0x2C 钩子被证伪): 逆向发现 flush_wait_cb 的唯一调用点是
//   孤儿代码(小米移植用 flash 版替换了 LVGL 默认的等待实现, 连调用点一起没了)——
//   装在 disp+0x2C 的钩子根本不会被调。改为**替换 flush_cb(disp+0x28) 本体**:
//   our_flush_cb 先调原始 flush(收队列+提交), 然后有界自旋等 busy(驱动ctx+0x2B6)
//   清零 —— busy 由驱动完成函数在拷贝/送面板结束时清零, 清零 = 本批已离开绘制
//   缓冲, 下一帧的旋转拷贝落在安静窗口, 竞争消除。

/// 原始 flush_cb（翻转前的值，恢复用）。已含 Thumb 位。
static mut FLUSH_ORIG: u32 = 0x0C4B_778D;
/// 我们的 flush_wait 已安装标志。
static WAIT_INSTALLED: AtomicU8 = AtomicU8::new(0);

/// 面板驱动上下文指针的持有者（SRAM，flush_cb 字面量池核对）。
const DRIVER_CTX_HOLDER: u32 = 0x200C_EB7C;
/// 信号量原语：0xC1EA358 = 阻塞 take（内部 nxsem_wait，IPSR 检查+计数递减+休眠路径
/// 已反汇编核对）；完成路径由驱动在流式结束后释放同一信号量。
const SEM_WAIT: u32 = 0x0C1E_A359;

/// 我们的 flush_cb：先调原始 flush(收失效区域进队列+提交)，然后有界自旋
/// 等 busy(驱动完成标志)清零 —— 本批已离开绘制缓冲，下一帧的旋转拷贝落点安全。
unsafe extern "C" fn our_flush_cb(disp: u32) {
    let orig: unsafe extern "C" fn(u32) = core::mem::transmute((read_volatile(core::ptr::addr_of!(FLUSH_ORIG)) as usize) | 1);
    orig(disp);
    let ctx = read_volatile(DRIVER_CTX_HOLDER as *const u32);
    if ctx == 0 {
        return;
    }
    // 有界自旋: 驱动完成函数(0x0C4B7324)在流式结束后清 busy; 超时放行(宁撕不挂)。
    let mut spins = 0usize;
    while read_volatile((ctx + 0x2B6) as *const u8) != 0 {
        spins += 1;
        if spins > 200_000 {
            break;
        }
    }
}

/// 安装/恢复 flush_cb 钩子。翻转态安装我们的包装，正常态恢复原始 flush_cb。
unsafe fn flush_hook_install(install: bool) {
    let disp = fw_api::display_default();
    if disp == 0 {
        return;
    }
    if install {
        if WAIT_INSTALLED.swap(1, Ordering::AcqRel) == 1 {
            return;
        }
        // 原始 flush_cb(disp+0x28, 已含 Thumb 位) 存起来恢复用
        let orig = read_volatile((disp + 0x28) as *const u32);
        write_volatile(core::ptr::addr_of_mut!(FLUSH_ORIG), orig);
        let ours = our_flush_cb as *const () as u32 | 1;
        write_volatile((disp + 0x28) as *mut u32, ours);
    } else {
        if WAIT_INSTALLED.swap(0, Ordering::AcqRel) == 0 {
            return;
        }
        let orig = read_volatile(core::ptr::addr_of!(FLUSH_ORIG));
        if orig != 0 {
            write_volatile((disp + 0x28) as *mut u32, orig);
        }
    }
}

unsafe fn apply_now(value: u8) {
    let disp = fw_api::display_default();
    if disp == 0 {
        return;
    }
    fw_api::display_set_rotation(disp, if value == VALUE_FLIPPED { ROTATION_180 } else { ROTATION_0 });
    APPLIED.store(value, Ordering::Release);
    // 撕裂修复: 翻转态安装 flush_wait 钩子（每轮刷新绘制前等上一批流完，
    // 旋转拷贝不再与流式读竞争）；恢复态卸载。
    flush_hook_install(value == VALUE_FLIPPED);
    // 旋转只重排已提交的像素；已建页面的内容树还带着旧方向的缓存，整树重画一次，
    // 免得切页时新旧方向的内容叠在一起（表现为一条横向接缝）。
    invalidate_pages();
}

/// 把所有已建页面的内容根标脏。`obj_invalidate` 只重画这一棵子树，不发样式事件。
unsafe fn invalidate_pages() {
    let pages: &[crate::state::PageState] = &*core::ptr::addr_of!(crate::state::PAGES);
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
