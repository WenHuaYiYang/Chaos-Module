//! Chaos 在系统应用列表（Launcher）中的可选显示入口。
//!
//! 默认开启，保持原有注册行为。配置在 UI 定时器中读取；首次安装由 Lua
//! 在注册前通过 IPC 写入当前值，避免在 fops.write 上下文里读取文件。

use crate::*;

const CONFIG_PATH: &[u8] = b"/data/chaos/launcher.cfg\0";

static mut ENABLED: bool = true;
static mut LOADED: bool = false;
static mut APPLY_REQUEST: bool = false;

pub(crate) unsafe fn load() {
    if LOADED {
        return;
    }
    LOADED = true;
    let fd = fw_api::open(CONFIG_PATH.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut value = 1u8;
        if fw_api::read(fd, &mut value, 1) == 1 {
            ENABLED = value != 0;
        }
        fw_api::close(fd);
    }
}

pub(crate) unsafe fn enabled() -> bool {
    ENABLED
}

/// Lua 在注册前或更新模块时设置。已注册时留到 UI 定时器中增删桌面图标。
pub(crate) unsafe fn set_enabled(value: bool) {
    ENABLED = value;
    LOADED = true;
    if APP_REGISTERED != 0 {
        APPLY_REQUEST = true;
    }
}

pub(crate) unsafe fn tick() {
    load();
    if !APPLY_REQUEST || APP_REGISTERED == 0 {
        return;
    }
    APPLY_REQUEST = false;
    if ENABLED {
        fw_api::app_launcher_add(APP_ID);
    } else {
        fw_api::app_launcher_del(APP_ID);
    }
}
