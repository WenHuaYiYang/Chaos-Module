//! 原生文件桥开关：持久化到 `/data/chaos/bridge.cfg`，由维护定时器落地。
//!
//! 文件格式：`BRG1` + 1 字节 enabled（0/1）。默认关闭；首次安装不会
//! 自动占用 L2 0x43，用户需要文件桥时在设置页手动打开。
//!
//! 开关只改配置并清 `BRIDGE_STATE`，真正的 start/stop 放到 `chaos_shake_timer`
//! 里做，避免在 LVGL 点击回调里创建/撤销 owner timer。

use crate::state::BRIDGE_STATE;
use crate::{fw_api, log, st_rd, st_wr};

const CFG_PATH: &[u8] = b"/data/chaos/bridge.cfg\0";
const MAGIC: &[u8; 4] = b"BRG1";

static mut LOADED: u32 = 0;
static mut ENABLED: u32 = 0;

extern "C" {
    fn chaos_bridge_start() -> i32;
    fn chaos_bridge_stop();
}

unsafe fn load() {
    if LOADED != 0 {
        return;
    }
    LOADED = 1;
    let fd = fw_api::open(CFG_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return;
    }
    let mut buf = [0u8; 5];
    let n = fw_api::read(fd, buf.as_mut_ptr(), 5);
    fw_api::close(fd);
    if n == 5 && &buf[0..4] == MAGIC && buf[4] <= 1 {
        ENABLED = buf[4] as u32;
    }
}

/// 当前开关状态（默认关）。
pub(crate) unsafe fn enabled() -> bool {
    load();
    ENABLED != 0
}

unsafe fn save(on: bool) {
    let mut buf = [0u8; 5];
    buf[0..4].copy_from_slice(MAGIC);
    buf[4] = if on { 1 } else { 0 };
    let fd = fw_api::open(
        CFG_PATH.as_ptr(),
        fw_api::oflag::WRONLY | fw_api::oflag::CREAT,
        0o640,
    );
    if fd < 0 {
        log::line("文件桥开关: 配置写入失败");
        return;
    }
    let n = fw_api::write(fd, buf.as_ptr(), buf.len() as u32);
    fw_api::close(fd);
    if n != buf.len() as i32 {
        log::line("文件桥开关: 配置写入不完整");
    }
}

/// UI 开关调用：翻转并落盘；实际 start/stop 由下一次维护 tick 执行。
pub(crate) unsafe fn toggle() -> bool {
    let on = !enabled();
    ENABLED = if on { 1 } else { 0 };
    save(on);
    // 打开时清 0，允许从停止/失败状态重新 start。
    // 关闭时不能清 0：tick 需要看到“原先在运行”的 1，才能调用 stop；
    // 清 0 会让 stop 分支永远不成立，桥会继续处理 L2 0x43。
    if on { st_wr!(BRIDGE_STATE, 0); }
    on
}

/// 维护定时器调用：按开关状态启动/停止文件桥。
pub(crate) unsafe fn tick() {
    load();
    let state = st_rd!(BRIDGE_STATE);
    if ENABLED != 0 {
        if state == 0 {
            let rc = chaos_bridge_start();
            st_wr!(BRIDGE_STATE, if rc == 0 { 1 } else { 0xffff_ffff });
            log::line(if rc == 0 {
                "文件桥: CanopusConnect 已启动"
            } else {
                "文件桥: 启动失败"
            });
        }
    } else if state != 0 {
        // 1 = 正在运行；0xffffffff = 上次启动失败。两种非零状态都收尾，
        // stop 本身幂等，失败状态也顺手清回 0。
        chaos_bridge_stop();
        st_wr!(BRIDGE_STATE, 0);
        log::line("文件桥: 已关闭");
    }
}
