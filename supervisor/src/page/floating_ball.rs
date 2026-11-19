//! Home -> settings -> 灵动点: native third-level settings and radio pickers.
use crate::floating_ball::{
    self as ball,
    model::{Action, Dock, SIZES},
};
use crate::page::kit::checklist::{click as check_click, render as check_render, CheckSpec};
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub, ToggleDef};
use crate::page::*;

pub(crate) struct BallPage;
pub(crate) static PAGE: BallPage = BallPage;
pub(crate) struct SizePage;
pub(crate) static SIZE: SizePage = SizePage;
pub(crate) struct ActionPage;
pub(crate) static ACTION: ActionPage = ActionPage;
static mut EDIT_SLOT: usize = 0;
static mut SIZE_TEXT: [u8; 24] = [0; 24];
static mut BATTERY_TEXT: [u8; 24] = [0; 24];
static mut OPACITY_TEXT: [u8; 24] = [0; 24];

unsafe fn enabled() -> bool {
    ball::config().enabled
}
unsafe fn toggle() {
    let mut c = ball::config();
    c.enabled = !c.enabled;
    ball::update(c);
}
unsafe fn cycle_opacity() {
    let mut c = ball::config();
    c.opacity = (c.opacity + 1) % 5;
    ball::update(c);
    // 原地刷新本行(有副标题的行 update 必须重传文本, 否则副标题被固件洗掉)。
    let pid = st_rd!(FG_PAGE) as usize;
    if pid <= MAX_PID {
        let r = read_volatile(page_rows_ptr(pid).add(2));
        if r != 0 {
            fw_api::row_update(r, fw_api::NONE_PTR, "透明度\0".as_bytes().as_ptr(),
                opacity_text(), 0, 0);
        }
    }
}
/// 透明度行的副标题: "不透明度 83%"(档位百分比, 100/83/67/50/33)。
unsafe fn opacity_text() -> *const u8 {
    let level = (ball::config().opacity as usize) % 5;
    let percent = [100u32, 83, 66, 50, 33][level];
    let p = core::ptr::addr_of_mut!(OPACITY_TEXT).cast();
    let mut w = W::new(p, 24);
    w.s("不透明度 ".as_bytes());
    w.n(percent);
    w.end();
    p
}
unsafe fn size_text() -> *const u8 {
    let p = core::ptr::addr_of_mut!(SIZE_TEXT).cast();
    let mut w = W::new(p, 24);
    w.n(ball::config().size as u32).s(b" px");
    w.end();
    p
}
unsafe fn battery_text() -> *const u8 {
    if !ball::config().enabled {
        return "启用后读取电量\0".as_ptr();
    }
    if let Some(percent) = ball::battery_level() {
        let p = core::ptr::addr_of_mut!(BATTERY_TEXT).cast();
        let mut w = W::new(p, 24);
        w.n(percent as u32).s(b"%");
        w.end();
        p
    } else {
        "电量暂不可用\0".as_ptr()
    }
}
unsafe fn slot_one() -> *const u8 {
    ball::config().slots[0].name().as_ptr()
}
unsafe fn slot_two() -> *const u8 {
    ball::config().slots[1].name().as_ptr()
}
unsafe fn slot_three() -> *const u8 {
    ball::config().slots[2].name().as_ptr()
}
unsafe fn edit(slot: usize) {
    EDIT_SLOT = slot;
    nav_goto(Pid::FloatingBallAction as u32, 0);
}
unsafe fn edit_one() {
    edit(0);
}
unsafe fn edit_two() {
    edit(1);
}
unsafe fn edit_three() {
    edit(2);
}
unsafe fn corner_text() -> *const u8 {
    ball::config().dock.edge.name().as_ptr()
}
unsafe fn reset_corner() {
    let mut c = ball::config();
    c.dock = Dock::DEFAULT;
    ball::update(c);
}
unsafe fn status() -> *const u8 {
    if ball::save_failed() {
        "保存失败，请检查空间后重试\0".as_ptr()
    } else {
        "设置自动保存\0".as_ptr()
    }
}
static ENABLED: ToggleDef = ToggleDef {
    get: enabled,
    toggle,
    status: None,
};
static ROWS: &[RowModel] = &[
    RowModel::toggle("启用灵动点\0".as_bytes(), &ENABLED),
    RowModel { label: "圆点大小\0".as_bytes(), sub: RowSub::Dyn(size_text), kind: RowKind::Nav(Pid::FloatingBallSize as u32), trailing: 0, refresh: None },
    RowModel { label: "透明度\0".as_bytes(), sub: RowSub::Dyn(opacity_text), kind: RowKind::Fn(cycle_opacity), trailing: 0, refresh: None },
    RowModel::display_sub("电量进度\0".as_bytes(), RowSub::Dyn(battery_text)),
    RowModel::section("扇形菜单功能\0".as_bytes()),
    RowModel { label: "功能一\0".as_bytes(), sub: RowSub::Dyn(slot_one), kind: RowKind::Fn(edit_one), trailing: 0, refresh: None },
    RowModel { label: "功能二\0".as_bytes(), sub: RowSub::Dyn(slot_two), kind: RowKind::Fn(edit_two), trailing: 0, refresh: None },
    RowModel { label: "功能三\0".as_bytes(), sub: RowSub::Dyn(slot_three), kind: RowKind::Fn(edit_three), trailing: 0, refresh: None },
    RowModel { label: "恢复右上角位置\0".as_bytes(), sub: RowSub::Dyn(corner_text), kind: RowKind::Fn(reset_corner), trailing: 0, refresh: None },
    RowModel::display_sub("保存状态\0".as_bytes(), RowSub::Dyn(status)),
    RowModel::note("轻点展开，点击空白处收起；按住圆点后拖动，松手自动吸附最近的屏幕边，位置自动保存。时间直接显示在连续弧带上；常亮可选5、10、15分钟，再次选择当前时长可结束。亮度沿弧相对滑动，按下不跳值；长按后慢滑可逐nit微调。轻点圆点从二级返回主菜单；关闭灵动点或主动息屏时恢复原息屏时间。球内整圈粗环按电量显示进度，带小圆点的端头随剩余电量收缩。截屏自动隐藏灵动点，结果提示2秒后收起；保存目录沿用截屏设置。快捷功能还可选表盘、震动、自动亮度、录屏和屏幕翻转。震动开关本次运行有效，重启后跟随系统设置。\0".as_bytes()),
];
impl MenuSpec for BallPage {
    fn rows(&self) -> &'static [RowModel] {
        ROWS
    }
}
impl Page for BallPage {
    fn fill_title(&self, p: *mut u8) {
        unsafe {
            put_str(p, "灵动点\0".as_bytes());
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }
    fn is_switch_row(&self, i: usize) -> bool {
        kit::menu_is_switch(self, i)
    }
    fn on_click(&self, i: usize) {
        unsafe { kit::menu_click(self, i) }
    }
    fn on_switch(&self, i: usize) {
        unsafe { kit::menu_switch(self, i) }
    }
}

struct SizeChoices;
const SIZE_NAMES: [&[u8]; 6] = [
    b"32 px\0", b"40 px\0", b"48 px\0", b"56 px\0", b"64 px\0", b"72 px\0",
];
impl CheckSpec for SizeChoices {
    fn count(&self) -> usize {
        SIZES.len()
    }
    unsafe fn label(&self, i: usize) -> *const u8 {
        SIZE_NAMES[i].as_ptr()
    }
    fn trailing(&self) -> u32 {
        fw_api::trailing::RADIO
    }
    unsafe fn selected(&self, i: usize) -> bool {
        ball::config().size == SIZES[i]
    }
    unsafe fn choose(&self, i: usize) {
        let mut c = ball::config();
        c.size = SIZES[i];
        ball::update(c);
    }
}
impl Page for SizePage {
    fn fill_title(&self, p: *mut u8) {
        unsafe {
            put_str(p, "圆点大小\0".as_bytes());
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { check_render(&SizeChoices, ctx).map(|_| ()) }
    }
    fn on_click(&self, i: usize) {
        unsafe { check_click(&SizeChoices, Pid::FloatingBallSize as u32, i) }
    }
}
struct ActionChoices;
impl CheckSpec for ActionChoices {
    fn count(&self) -> usize {
        Action::ALL.len()
    }
    unsafe fn label(&self, i: usize) -> *const u8 {
        Action::ALL[i].name().as_ptr()
    }
    fn trailing(&self) -> u32 {
        fw_api::trailing::RADIO
    }
    unsafe fn selected(&self, i: usize) -> bool {
        ball::config().slots[EDIT_SLOT] == Action::ALL[i]
    }
    unsafe fn choose(&self, i: usize) {
        let mut c = ball::config();
        c.assign(EDIT_SLOT, Action::ALL[i]);
        ball::update(c);
    }
}
impl Page for ActionPage {
    fn fill_title(&self, p: *mut u8) {
        unsafe {
            put_str(p, "选择快捷功能\0".as_bytes());
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { check_render(&ActionChoices, ctx).map(|_| ()) }
    }
    fn on_click(&self, i: usize) {
        unsafe { check_click(&ActionChoices, Pid::FloatingBallAction as u32, i) }
    }
}
