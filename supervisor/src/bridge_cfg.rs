//! 原生文件桥开关：持久化到 `/data/chaos/bridge.cfg`，由维护定时器落地。
//!
//! 文件格式：`BRG1` + 1 字节 enabled（0/1）。默认关闭；首次安装不会
//! 自动接管私有消息，用户需要ChaosConnect文件桥时在设置页手动打开。
//!
//! 开关回调立即切换收发门禁并作废旧队列，timer/VFS收尾仍交给各自owner。

use crate::state::BRIDGE_STATE;
use crate::{fw_api, connect_log, st_rd, st_wr};

const CFG_PATH: &[u8] = b"/data/chaos/bridge.cfg\0";
const MAGIC: &[u8; 4] = b"BRG1";

static mut LOADED: u32 = 0;
static mut ENABLED: u32 = 0;
// BRIDGE_STATE 的“停止待执行”值：快速 关→开→关 时，最后一步不能把 0
// 写回去，否则 tick 会以为“本来就没运行”而跳过 stop。
const STATE_STOP: u32 = 0xFFFF_FFFE;
/// 启动失败后的重试间隔。失败不是终态: 原来的 0xffffffff 会让桥永久不启动,
/// 而 UI 仍按 ENABLED 显示"已启用", 插件每个操作都被回 ACCESS。
const RETRY_MS: u32 = 5000;
/// 最多重试几次。失败可能是持续性的(固件不匹配/资源耗尽), 无限重试等于定时
/// 反复调 start, 所以这里封顶; 到顶后回到"终态不再试", 但日志说清原因。
const RETRY_MAX: u32 = 3;
static mut RETRY_AT: u32 = 0;
static mut FAILS: u32 = 0;

extern "C" {
    fn chaos_bridge_set_enabled(enabled:u32);
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
    chaos_bridge_set_enabled(ENABLED);
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
        connect_log::line("ChaosConnect 配置: 配置写入失败");
        return;
    }
    let n = fw_api::write(fd, buf.as_ptr(), buf.len() as u32);
    fw_api::close(fd);
    if n != buf.len() as i32 {
        connect_log::line("ChaosConnect 配置: 配置写入不完整");
    }
}

/// UI先关闭/开启原生门禁，再持久化；无需等待后续维护tick才禁止访问。
pub(crate) unsafe fn toggle() -> bool {
    let on = !enabled();
    ENABLED = if on { 1 } else { 0 };
    chaos_bridge_set_enabled(ENABLED);
    if !on { crate::connect_permission::disconnect(); }
    save(on);
    // 打开时清 0，允许从停止/失败状态重新 start。
    // 关闭时写“停止待执行”，即使快速 关→开→关 也能让 tick 最终 stop；
    // 不能清 0，否则 tick 会跳过 stop，桥继续处理私有消息。
    st_wr!(BRIDGE_STATE, if on { 0 } else { STATE_STOP });
    on
}

/// 维护定时器调用：按开关状态启动/停止文件桥。
pub(crate) unsafe fn tick() {
    crate::connect_permission::load();
    crate::connect_radio::load_enabled();
    load();
    let state = st_rd!(BRIDGE_STATE);
    if ENABLED != 0 {
        crate::connect_radio::ensure_worker();
        // 按 RETRY_MS 退避重启; 成功才置 1, 失败保持 0(未运行)等待下一次。
        let now = fw_api::capture_now_ms();
        let retry = st_rd!(RETRY_AT);
        if state == 0 && (retry == 0 || (now.wrapping_sub(retry) as i32) >= 0) {
            let rc = chaos_bridge_start();
            if rc == 0 {
                st_wr!(BRIDGE_STATE, 1);
                st_wr!(RETRY_AT, 0);
                st_wr!(FAILS, 0);
                connect_log::line("ChaosConnect 已启用，等待插件连接");
            } else {
                let fails = st_rd!(FAILS) + 1;
                st_wr!(FAILS, fails);
                if fails >= RETRY_MAX {
                    // 持续性失败: 不再重试, 也让状态与事实一致(不会显示"已启用")。
                    st_wr!(BRIDGE_STATE, 0xffff_ffff);
                    connect_log::line("ChaosConnect 启动失败，已停止重试");
                } else {
                    st_wr!(RETRY_AT, now.wrapping_add(RETRY_MS * fails));
                    connect_log::line("ChaosConnect 启动失败，稍后重试");
                }
            }
        }
    } else if state != 0 {
        // 非 0 都收尾：1=运行，0xffffffff=启动失败，0xfffffffe=停止待执行。
        // stop 本身幂等，失败/待停止状态都顺手清回 0。
        chaos_bridge_stop();
        st_wr!(BRIDGE_STATE, 0);
        st_wr!(RETRY_AT, 0);
        st_wr!(FAILS, 0);
        connect_log::line("ChaosConnect 已停用");
    }
}
