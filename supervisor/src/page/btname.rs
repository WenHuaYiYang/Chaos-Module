//! 实验性功能 -> 修改蓝牙名: 改手环在手机蓝牙列表里显示的名字。
//!
//! 页面结构: 开关(启用/关闭自定义名) -> 名称(点击输入) -> 恢复设备默认 -> 状态。
//! 底层走蓝牙服务 bttool 通道(cmd 0x22), 固件重启会按系统"设备名称"覆盖,
//! 因此保存后由 bt_name 模块在开机时补下发一次。

use crate::bt_name;
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub, ToggleDef};
use crate::page::*;

pub(crate) struct BtNamePage;
pub(crate) static PAGE: BtNamePage = BtNamePage;

static mut SHOWN: [u8; 40] = [0; 40];

/// 副标题: 当前设定的名字(空 = 使用设备默认名)。
unsafe fn current_text() -> *const u8 {
    let src = bt_name::current();
    let mut n = 0usize;
    while n < SHOWN.len() - 1 {
        let c = core::ptr::read_volatile(src.add(n));
        if c == 0 {
            break;
        }
        SHOWN[n] = c;
        n += 1;
    }
    SHOWN[n] = 0;
    if n == 0 {
        // 「未设置（用设备默认名）」
        let fallback = "未设置, 用设备默认名\0".as_bytes();
        for (i, &b) in fallback.iter().enumerate() {
            SHOWN[i] = b;
        }
    }
    core::ptr::addr_of!(SHOWN) as *const u8
}

static mut STATUS: [u8; 64] = [0; 64];

/// 把蓝牙服务的返回码翻成能看懂的原因(用户看到的错误码要能自解释)。
unsafe fn rc_text(w: &mut W, rc: i32) {
    match rc {
        0 => {
            w.s("已生效\0".as_bytes());
        }
        -11 => {
            w.s("屏幕熄灭, 稍后自动重试\0".as_bytes());
        }
        -22 => {
            w.s("名称不合法\0".as_bytes());
        }
        -601 => {
            w.s("蓝牙服务未就绪, 稍后自动重试\0".as_bytes());
        }
        -618 => {
            w.s("蓝牙通信失败, 稍后自动重试\0".as_bytes());
        }
        -619 => {
            w.s("蓝牙未开启, 请先打开蓝牙\0".as_bytes());
        }
        n if n > 0 => {
            w.s("蓝牙拒绝该名称(".as_bytes());
            w.n(n as u32);
            w.s(")\0".as_bytes());
        }
        n => {
            w.s("失败(".as_bytes());
            w.n((-n) as u32);
            w.s(")\0".as_bytes());
        }
    }
}

unsafe fn status_text() -> *const u8 {
    let mut w = W::new(core::ptr::addr_of_mut!(STATUS) as *mut u8, 64);
    match bt_name::last_result() {
        Some(rc) => rc_text(&mut w, rc),
        None => {
            if bt_name::has_custom() {
                w.s("待下发(蓝牙服务启动后自动生效)\0".as_bytes());
            } else {
                w.s("使用设备默认名\0".as_bytes());
            }
        }
    }
    w.end();
    core::ptr::addr_of!(STATUS) as *const u8
}

unsafe fn edit() {
    bt_name::ensure_loaded();
    let initial = bt_name::current();
    crate::keyboard::open(initial, crate::keyboard::Completion::BtName);
}

unsafe fn switch_on() -> bool {
    bt_name::enabled()
}

/// 开关: 关 -> 恢复设备默认名; 开 -> 下发已保存的名字。
/// 还没设过名字时点开关直接弹键盘(输入保存后开关自然变开)。
unsafe fn switch_flip() {
    if bt_name::enabled() {
        bt_name::set_enabled(false);
    } else if !bt_name::set_enabled(true) {
        edit();
        return;
    }
    crate::render_req(Pid::BtName as u32);
}

static SWITCH: ToggleDef = ToggleDef {
    get: switch_on,
    toggle: switch_flip,
    status: None,
};

unsafe fn restore() {
    bt_name::restore_default();
    crate::render_req(Pid::BtName as u32);
}

static ROWS: &[RowModel] = &[
    RowModel::toggle("自定义蓝牙名\0".as_bytes(), &SWITCH),
    RowModel { label: "蓝牙名称\0".as_bytes(), sub: RowSub::Dyn(current_text),
               kind: RowKind::Fn(edit), trailing: 0, refresh: None },
    RowModel { label: "恢复设备默认\0".as_bytes(),
               sub: RowSub::Static("改回系统「设备名称」\0".as_bytes()),
               kind: RowKind::Fn(restore), trailing: 0, refresh: None },
    RowModel { label: "状态\0".as_bytes(), sub: RowSub::Dyn(status_text),
               kind: RowKind::Display, trailing: 0, refresh: None },
    RowModel::note("开关打开才使用下面输入的名字, 关掉立即恢复设备默认名。改名会立刻改掉手机蓝牙列表里看到的手环名。固件重启时会先按系统「设备名称」广播, Chaos 随后(约几秒内)重新下发这里的设定。名称上限 31 字节(约 10 个汉字)。\0".as_bytes()),
];

impl MenuSpec for BtNamePage {
    fn rows(&self) -> &'static [RowModel] {
        ROWS
    }
}

impl Page for BtNamePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            put_str(buf, "修改蓝牙名\0".as_bytes());
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }
    fn is_switch_row(&self, _idx: usize) -> bool {
        false
    }
    fn on_click(&self, idx: usize) {
        unsafe { kit::menu_click(self, idx) }
    }
    fn needs_tick(&self) -> bool {
        // 只在"正在等 worker 回结果 / 结果刚到"时逐拍跟进(几秒), 之后恢复常规节拍。
        unsafe { bt_name::pending_result() }
    }
    fn tick(&self) {
        unsafe {
            if bt_name::take_settled() {
                crate::render_req(Pid::BtName as u32);
            }
        }
    }
}
