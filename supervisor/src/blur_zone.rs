// 模糊区域开关 —— "系统模糊"背景替换/透明化的**分区域门控**。
//
// 背景 hook 的四个目标面(fw_api::background_home_targets 的槽位, 顺序固定):
//   0 = 启动器(桌面列表)   1 = 控制中心
//   2 = 小部件             3 = 通知中心(消息根 + 通知聚合页, 注册见 background_pages)
//
// 位图语义: 1 = 正常替换(默认) / 0 = 不替换(该区域保持官方模糊画面)。
// 持久化: /data/chaos/blur_regions.cfg, 1 字节位图; 文件缺失 = 全开(0x0F)。
// 改位不直接动对象树 —— 只写文件, 然后由 background_home::remount() 置 REQUEST,
// background_home::tick 的拆/装流程按新掩码统一重建(与档位切换同一套路径)。
//
// 生效方式(background_home 侧按槽位掩码过滤 targets):
//   * 静态背景(mode 1): 关闭的槽位不 attach SCENE 图像、不 enable 透明卡片;
//   * 完全透明(mode 3): 关闭的槽位不置 BG_OPA=0、不扫子树;
//   * 通知中心(slot 3): 关闭时 background_pages 不接管聚合页 on_create。

use crate::fw_api;
use crate::{st_rd, st_wr};

pub(crate) const REGION_N: usize = 4;

/// 本页的 page_id(系统模糊 -> 模糊区域; 页15, 注册见 ipc.rs/state.rs/page.rs)
pub(crate) const PAGE_PID: u32 = 15;

static mut MASK: u32 = 0x0F;
static mut LOADED: u32 = 0;

const CFG: &[u8] = b"/data/chaos/blur_regions.cfg\0";

/// 只在进设置页和第一次 set 前调一次; 文件缺失/越界值保持默认全开
pub(crate) unsafe fn ensure_loaded() {
    if LOADED != 0 { return; }
    LOADED = 1;
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut v = 0u8;
        if fw_api::read(fd, &mut v as *mut u8, 1) == 1 && v <= 0x0F {
            st_wr!(MASK, v as u32);
        }
        fw_api::close(fd);
    }
}

/// 槽位当前是否启用(st_rd! 是 volatile 读, 调用方都在 unsafe 上下文)
pub(crate) unsafe fn on(slot: usize) -> bool {
    slot < REGION_N && (st_rd!(MASK) >> slot) & 1 != 0
}

/// 写开关并持久化; 返回是否真的变了(变了调用方才需要 remount)
pub(crate) unsafe fn set(slot: usize, on: bool) -> bool {
    if slot >= REGION_N { return false; }
    ensure_loaded();
    let old = st_rd!(MASK);
    let m = if on { old | (1u32 << slot) } else { old & !(1u32 << slot) };
    if m == old { return false; }
    st_wr!(MASK, m);
    let v = m as u8;
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd >= 0 { let _ = fw_api::write(fd, &v as *const u8, 1); fw_api::close(fd); }
    true
}
