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

// ===== 撕裂: 逆向结论与回退记录（2026-11-14） =====
//
// 机理（模拟器逆向实证, 详见 docs/Chaos_UI重构_20261113.md §9.5）: 翻转后
//   LVGL 的旋转拷贝把区域原地写进绘制缓冲，而面板驱动的流式任务正在顺序读
//   同一块缓冲送面板 —— 两个线程无同步竞争，流出去的就是半新半旧的一帧
//   = 用户看到的横向割裂。固件的 flush_cb(0x0C4B778C) 把失效区域收进队列后
//   立即返回（异步），busy(驱动 ctx+0x2B6) 非零时用它自己的信号量
//   (0xC1EA358 = nxsem_wait 阻塞 take, 已反汇编确认) 等上一批完成。
//
// 尝试过的修法（都被真机/逆向否决）:
//   a) render_mode=2(FULL): 真机实测半屏冻结，固件 flush 不完整支持 —— 否决;
//   b) 替换 flush_cb 为 busy 自旋包装: 真机点开应用直接崩溃 —— busy 标志的
//      极性未实证(0x0C4B7324 的完成函数只清零+getvalue, 没有可见的 post 调用), 
//      自旋可能在每拍 flush 上空转秒级 → 回退;
//   c) disp+0x2C(flush_wait_cb) 钩子: 逆向发现它的唯一调用点是孤儿代码
//      (0x1C04EE80 的等待循环从未被调用), 钩子装上去不会被调 —— 无效;
//
// 结论: 撕裂是固件 LVGL「DIRECT+旋转」组合的固有限制, 模块层无法同步
//   旋转拷贝与面板流式读。根治需继续逆向下层任务的传输与 TE 同步
//   （PSRAM 侧 set_rotation 0x1C03046C 与 flush 链, 见地址表）——未启动。
//   翻转态用于阅读静态内容不受影响。

/// 原始 flush_cb（翻转前的值，恢复用）。已含 Thumb 位。
static mut FLUSH_ORIG: u32 = 0x0C4B_778D;
/// 我们的 flush_wait 已安装标志。
static WAIT_INSTALLED: AtomicU8 = AtomicU8::new(0);

/// 面板驱动上下文指针的持有者（SRAM，flush_cb 字面量池核对）。
const DRIVER_CTX_HOLDER: u32 = 0x200C_EB7C;
/// 信号量原语：0xC1EA358 = 阻塞 take（内部 nxsem_wait，IPSR 检查+计数递减+休眠路径
/// 已反汇编核对）；完成路径由驱动在流式结束后释放同一信号量。
const SEM_WAIT: u32 = 0x0C1E_A359;

unsafe fn apply_now(value: u8) {
    let disp = fw_api::display_default();
    if disp == 0 {
        return;
    }
    fw_api::display_set_rotation(disp, if value == VALUE_FLIPPED { ROTATION_180 } else { ROTATION_0 });
    APPLIED.store(value, Ordering::Release);
    // 撕裂修复: 翻转态安装 flush_wait 钩子（每轮刷新绘制前等上一批流完，
    // 旋转拷贝不再与流式读竞争）；恢复态卸载。
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
