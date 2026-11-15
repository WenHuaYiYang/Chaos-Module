//! 页面进入微动效: content 容器从 +8px 在 3 拍(50ms/拍)内滑到 0。
//!
//! 约束(都是硬性的):
//! - 只动 content 容器的 y, 不碰任何行控件;
//! - 不新建定时器 —— 走常驻主节拍, 息屏空闲 1000ms 时它也自然放慢;
//! - 只在前台页、亮屏、对象树存活时推进, 任何一条不满足立即归零;
//! - 重建/退页/息屏立即归零(render_page 重建路径与 on_pause/on_destroy 调 reset);
//! - 一键关闭: MOTION_ENABLED = false 重新构建即可。
//!
//! 重要: 当前**默认关闭**。真机实测任何非零 y 偏移都会在标题栏与 content 之间
//! 露出一条背景缝(第一拍 8px, 之后 4/2px 逐拍收回), 用户报"点开界面顶部空出
//! 一两个像素一帧然后恢复" —— 不做透明度/缩放的前提下, 整体 y 滑入没有
//! 无缝的画法; 要重新启用必须先解决这条缝(例如改成不越出基准盒的方案)。

use crate::*;

pub(crate) const MOTION_ENABLED: bool = false;
/// 每拍的 y 偏移, 走完归 0(3 拍 = 150ms)。
const STEPS: [i32; 3] = [8, 4, 2];

static mut MOT_PID: u32 = 0xFFFF_FFFF;
static mut MOT_BASE: i32 = 56;   // 该页 content 的基准 y(列表页 56 / 整页 0)
static mut MOT_STEP: usize = 0;

unsafe fn apply(base: i32, off: i32) {
    let pid = st_rd!(MOT_PID) as usize;
    if pid > MAX_PID { return; }
    let c = PAGES[pid].content;
    if c == 0 { return; }
    fw_api::obj_align(c, fw_api::align::TOP_MID, 0, base + off);
}

/// 页面(重)进入后武装。只在真重建过对象树的那次进入生效(唤醒复用旧树不抖)。
pub(crate) unsafe fn arm(pid: u32) {
    if !MOTION_ENABLED { return; }
    let pid = pid as usize;
    if pid > MAX_PID { return; }
    if PAGES[pid].built == 0 || !page_is_live(pid) || !fw_api::screen_is_on() { return; }
    let base = match PAGE_TABLE[pid].geometry() {
        Geom::List => crate::ui::CONTENT_BASE_Y,
        Geom::FullBleed => 0,
    };
    st_wr!(MOT_PID, pid as u32);
    st_wr!(MOT_BASE, base);
    st_wr!(MOT_STEP, 0);
    apply(base, STEPS[0]);
}

/// 立即归零(退页/息屏/重建)。
pub(crate) unsafe fn reset() {
    if st_rd!(MOT_PID) == 0xFFFF_FFFF { return; }
    let base = st_rd!(MOT_BASE);
    apply(base, 0);
    st_wr!(MOT_PID, 0xFFFF_FFFF);
}

/// 主节拍推进一步。
pub(crate) unsafe fn tick() {
    if st_rd!(MOT_PID) == 0xFFFF_FFFF { return; }
    let pid = st_rd!(MOT_PID) as usize;
    // 前台页 + 亮屏 + 对象树存活, 否则立即归零
    if pid > MAX_PID
        || st_rd!(FG_PAGE) as usize != pid
        || PAGES[pid].built == 0
        || !page_is_live(pid)
        || !fw_api::screen_is_on()
    {
        reset();
        return;
    }
    let step = st_rd!(MOT_STEP) as usize;
    let base = st_rd!(MOT_BASE);
    if step + 1 >= STEPS.len() {
        apply(base, 0);
        st_wr!(MOT_PID, 0xFFFF_FFFF);
        return;
    }
    st_wr!(MOT_STEP, step + 1);
    apply(base, STEPS[step + 1]);
}
