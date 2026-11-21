//! Corona native blur bridge.
//!
//! The complete four-surface hooks and provider live in the AGPL-3.0-only
//! files under supervisor/corona_vendor. This module owns only Chaos
//! configuration and the serialized UI-owner tick that drives the provider.

use crate::fw_api;

/// 模糊状态: 0=关闭, 1/2/3 = 开启 + 算法(0 省电原生 / 1 V2 / 2 V3)。
/// 单独一个文件, 值 -1 就是算法号 —— 一个字节同时表达开关和算法。
const ALGO_CONFIG: &[u8] = b"/data/chaos/blur_algo.cfg\0";
/// 旧版只有开/关(`blur_v2.cfg`), 仅用于一次性迁移。
const LEGACY_V2_CONFIG: &[u8] = b"/data/chaos/blur_v2.cfg\0";

/// 算法号: 0 = 上游 Canopus 原生滤波(radius 2/84x120, 最省电)
///         1 = V2(SysPatch IIR radius 12, 全分辨率, 清晰但费电)
///         2 = V3(V2 的画质 + 场景门控/卡住兜底, 清晰且省电)
pub(crate) const ALGO_NATIVE: u32 = 0;
pub(crate) const ALGO_V2: u32 = 1;
pub(crate) const ALGO_V3: u32 = 2;

extern "C" {
    fn chaos_blur_select_algorithm(algo: u32) -> i32;
    fn chaos_blur_control_buttons_set(enabled: u32);
    fn chaos_blur_launcher_buttons_set(enabled: u32);
    fn chaos_corona_backgrounds_commit(enabled: u32, region_mask: u32) -> i32;
    fn chaos_corona_backgrounds_poll();
    fn chaos_corona_backgrounds_pending() -> u32;
    fn chaos_corona_backgrounds_idle() -> i32;
    fn chaos_corona_backgrounds_owned() -> u32;
    fn chaos_corona_diagnostics(out: *mut u8, capacity: u32) -> u32;
    fn chaos_corona_set_scene(id: u32);
}

static mut LOADED: bool = false;
static mut ENABLED: bool = false;
static mut ALGO: u32 = ALGO_NATIVE;
static mut SAVE_FAILED: bool = false;

unsafe fn load() {
    if LOADED { return; }
    LOADED = true;
    let fd = fw_api::open(ALGO_CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut value = 0u8;
        if fw_api::read(fd, &mut value as *mut u8, 1) == 1 && value <= 3 {
            ENABLED = value != 0;
            ALGO = if value == 0 { ALGO_NATIVE } else { (value - 1) as u32 };
        }
        fw_api::close(fd);
        return;
    }
    // 迁移: 旧文件只有 0/1, 1 表示当时开着(跑的是改进版 V2) -> 打开并选 V3,
    // 这样升级后观感不变, 但立刻享受场景门控带来的省电。
    let old = fw_api::open(LEGACY_V2_CONFIG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if old < 0 { return; }
    let mut value = 0u8;
    if fw_api::read(old, &mut value as *mut u8, 1) == 1 && value == 1 {
        ENABLED = true;
        ALGO = ALGO_V3;
    }
    fw_api::close(old);
}

unsafe fn persist() -> bool {
    let fd = fw_api::open(ALGO_CONFIG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let value = [if ENABLED { (ALGO + 1) as u8 } else { 0u8 }];
    let saved = fw_api::write(fd, value.as_ptr(), value.len() as u32) == 1;
    fw_api::close(fd);
    saved
}

pub(crate) unsafe fn enabled() -> bool {
    load();
    ENABLED
}

pub(crate) unsafe fn save_failed() -> bool { SAVE_FAILED }

/// 模糊是否启用(与算法号无关)。
pub(crate) unsafe fn active() -> bool { enabled() }

/// 当前算法号(0/1/2); 关闭时返回值无意义, 用 active() 判断开关。
pub(crate) unsafe fn algo() -> u32 {
    load();
    ALGO
}

/// 写盘失败后本次切换仍立即生效, 但同值调用要重试写盘 —— 否则用户改了设置、
/// 那一刻正好 IO 失败, 就会永久"看着开着但重启即丢"。
pub(crate) unsafe fn set_enabled(on: bool) {
    load();
    let changed = ENABLED != on;
    if !changed && !SAVE_FAILED { return; }
    ENABLED = on;
    SAVE_FAILED = !persist();
    if changed { crate::background_home::remount(); }
}

/// 选算法同时打开模糊: 这三个选项本身互斥, 点哪个就切到哪个。
pub(crate) unsafe fn set_algo(next: u32) {
    load();
    let next = next.min(ALGO_V3);
    let changed = !ENABLED || ALGO != next;
    if !changed && !SAVE_FAILED { return; }
    ENABLED = true;
    ALGO = next;
    SAVE_FAILED = !persist();
    if changed { crate::background_home::remount(); }
}

/// Commit Corona's background configuration using the shared region mask:
/// launcher, control center, widgets, notifications (bits 0 through 3).
/// Unselected regions retain the system background. The C
/// implementation is the verbatim p67_backgrounds/p67_launcher state machine;
/// a non-zero return means it is still busy with a native scroll/delete and
/// must be retried on a later UI-owner tick.
pub(crate) unsafe fn backgrounds_commit(enabled: bool, region_mask: u32) -> i32 {
    // 2026-11-19: 改回 README 点名的 Canopus-Module-Corona 原生滤波(algorithm=0)。
    // 之前固定用 V2(SysPatch IIR): radius 12 / PRECISION / 每像素色彩 LUT /
    // 输出全分辨率 336x480 BGR888(约 484 KB 纹理)。原生路径是 radius 2、2 passes、
    // 84x120 RGB565(约 20 KB) 再交给 GPU 4x 放大 —— 单次采集的计算量、纹理上传量
    // 和每帧带宽都小一个数量级。实测用户反馈"十多分钟掉一格 vs 原来半小时一格"。
    let rc = chaos_blur_select_algorithm(algo());
    if rc != 0 { return rc; }
    chaos_corona_backgrounds_commit(enabled as u32, region_mask & 0x0F)
}

/// 把当前"场景身份"(表盘 identity)交给 C 侧, 供周期采集判断是否需要重算。
/// 每次 tick 写一次, 只是一个 u32; 采集判据从"模糊层绘制次数"换成它,
/// 才能断开"采集→整屏重绘→计数+1→再采集"的自激环。
pub(crate) unsafe fn set_scene(id: u32) {
    chaos_corona_set_scene(id);
}

pub(crate) unsafe fn backgrounds_poll() {
    crate::blur_params::apply();
    chaos_blur_control_buttons_set((algo() != 0 && crate::cc_buttons_config::enabled()) as u32);
    chaos_blur_launcher_buttons_set((algo() != 0 && crate::launcher_buttons_config::enabled()) as u32);
    let _ = chaos_blur_select_algorithm(algo());
    chaos_corona_backgrounds_poll();
}

pub(crate) unsafe fn backgrounds_pending() -> bool {
    chaos_corona_backgrounds_pending() != 0
}

/// Reuse the upstream native-idle and gesture gates before removing the old
/// Chaos scene. Neither this bridge nor Backdrop takes ownership of Corona's
/// image descriptors; publication/retirement remain entirely in Corona.
pub(crate) unsafe fn backgrounds_idle() -> bool {
    chaos_corona_backgrounds_idle() != 0
}

/// Still owns the scene? The native owners are torn down by their own DELETE /
/// gesture handling; when that happens without a commit the module would keep
/// believing it owns the scene and simply stop painting (blur gone, page left
/// transparent). Rust re-commits once this reports 0 for a few ticks.
pub(crate) unsafe fn backgrounds_owned() -> bool {
    chaos_corona_backgrounds_owned() != 0
}

/// 只读诊断文本（key=value 逐行，C 侧保证带终止符）。返回写入长度；0 = 未写入。
pub(crate) unsafe fn diagnostics(out: &mut [u8]) -> usize {
    chaos_corona_diagnostics(out.as_mut_ptr(), out.len() as u32) as usize
}

