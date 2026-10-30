// 恢复 fetch 支持开关。
//
// 这里只持久化配置，不在设置页里热加载内核模块。安装器在下次重新注入时读取
// /data/chaos/fetch.cfg，部署并加载 chaos-fetch，再执行完整的 1→2→3→4 布防。

use crate::fw_api;
use crate::{st_rd, st_wr};

static mut LOADED: u32 = 0;
static mut ENABLED: u32 = 1;

const CFG: &[u8] = b"/data/chaos/fetch.cfg\0";

pub(crate) unsafe fn ensure_loaded() {
    if st_rd!(LOADED) != 0 { return; }
    st_wr!(LOADED, 1);
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut value = 1u8;
        if fw_api::read(fd, &mut value as *mut u8, 1) == 1 {
            st_wr!(ENABLED, if value == 0 { 0 } else { 1 });
        }
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn on() -> bool {
    ensure_loaded();
    st_rd!(ENABLED) != 0
}

/// 写成功后才更新界面状态；返回 false 时调用方刷新为原状态。
pub(crate) unsafe fn set(enabled: bool) -> bool {
    ensure_loaded();
    let value = if enabled { 1u8 } else { 0u8 };
    let fd = fw_api::open(CFG.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    if fd < 0 { return false; }
    let ok = fw_api::write(fd, &value as *const u8, 1) == 1;
    fw_api::close(fd);
    if ok { st_wr!(ENABLED, value as u32); }
    ok
}
