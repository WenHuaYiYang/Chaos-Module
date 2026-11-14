//! 页11/12/14/16/19：亮度、缓存、设置、震动与 fetch 恢复。

use crate::page::*;
use crate::page::Page;
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub, ToggleDef};

pub(crate) struct SettingsPage;

pub(crate) struct LogLevelPage;
pub(super) static PAGE_LOG_LEVEL: LogLevelPage = LogLevelPage;

macro_rules! log_level_toggle {
    ($get:ident, $toggle:ident, $def:ident, $bit:expr) => {
        unsafe fn $get() -> bool { fw_api::system_log_mask() & (1 << $bit) != 0 }
        unsafe fn $toggle() {
            fw_api::system_log_mask_set(fw_api::system_log_mask() ^ (1 << $bit));
        }
        static $def: ToggleDef = ToggleDef { get: $get, toggle: $toggle, status: None };
    };
}
log_level_toggle!(log_emerg_get, log_emerg_toggle, LOG_EMERG, 0);
log_level_toggle!(log_alert_get, log_alert_toggle, LOG_ALERT, 1);
log_level_toggle!(log_crit_get, log_crit_toggle, LOG_CRIT, 2);
log_level_toggle!(log_error_get, log_error_toggle, LOG_ERROR, 3);
log_level_toggle!(log_warn_get, log_warn_toggle, LOG_WARN, 4);
log_level_toggle!(log_notice_get, log_notice_toggle, LOG_NOTICE, 5);
log_level_toggle!(log_info_get, log_info_toggle, LOG_INFO, 6);
log_level_toggle!(log_debug_get, log_debug_toggle, LOG_DEBUG, 7);

static LOG_LEVEL_ROWS: &[RowModel] = &[
    RowModel::toggle("紧急 EMERG（r）\0".as_bytes(), &LOG_EMERG),
    RowModel::toggle("警报 ALERT（a）\0".as_bytes(), &LOG_ALERT),
    RowModel::toggle("严重 CRITICAL（c）\0".as_bytes(), &LOG_CRIT),
    RowModel::toggle("错误 ERROR（e）\0".as_bytes(), &LOG_ERROR),
    RowModel::toggle("警告 WARNING（w）\0".as_bytes(), &LOG_WARN),
    RowModel::toggle("通知 NOTICE（n）\0".as_bytes(), &LOG_NOTICE),
    RowModel::toggle("信息 INFO（i）\0".as_bytes(), &LOG_INFO),
    RowModel::toggle("调试 DEBUG（d）\0".as_bytes(), &LOG_DEBUG),
    RowModel::note("说明：防止系统狂写日志导致存储芯片受损（阻止米米的傲娇行为）\n支持多选，仅记录开启的等级，立即生效；重启后恢复系统默认。全部关闭时掩码为0。\n命令 setlogmask 会同时开启所选等级及更严重等级；本页按各等级独立选择。\0".as_bytes()),
];

impl MenuSpec for LogLevelPage {
    fn rows(&self) -> &'static [RowModel] { LOG_LEVEL_ROWS }
}
impl Page for LogLevelPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "日志级别设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }
    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页11: 亮度 =====

pub(crate) struct BrightnessPage;

// 静态行模型 + 保留原 tick 的原地刷新(行槽位不变: 0 状态 / 3 自动开关)。
unsafe fn bright_status_ptr() -> *const u8 { core::ptr::addr_of!(BRIGHT_STATUS) as *const u8 }
unsafe fn bright_set_600() { st_wr!(BRIGHT_REQ_VAL, 600); st_wr!(BRIGHT_REQ, 4); }
unsafe fn bright_set_2000() { st_wr!(BRIGHT_REQ_VAL, 2000); st_wr!(BRIGHT_REQ, 4); }
unsafe fn bright_auto_get() -> bool { fw_api::brightness_get_auto_adjustment() != 0 }
unsafe fn bright_auto_toggle() { st_wr!(BRIGHT_REQ, 2); }

static BRIGHT_AUTO_DEF: ToggleDef = ToggleDef { get: bright_auto_get, toggle: bright_auto_toggle, status: None };
static BRIGHT_ROWS: &[RowModel] = &[
    RowModel::display_sub("当前亮度\0".as_bytes(), RowSub::Dyn(bright_status_ptr)),
    RowModel::action("600nit\0".as_bytes(), bright_set_600, 0, None),
    RowModel::action("2000nit\0".as_bytes(), bright_set_2000, 0, None),
    RowModel::toggle("自动亮度\0".as_bytes(), &BRIGHT_AUTO_DEF),
];

impl MenuSpec for BrightnessPage {
    fn rows(&self) -> &'static [RowModel] { BRIGHT_ROWS }
}

impl Page for BrightnessPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "亮度控制".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            st_wr!(BRIGHT_VAL, fw_api::brightness_get_target());
            bright_fill_status();
            frame::build(self, ctx, ctx.title)
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, _idx: usize) { unsafe { kit::menu_switch(self, _idx) } }

    fn needs_tick(&self) -> bool {
        unsafe { st_rd!(BRIGHT_REQ) != 0 }
    }

    fn tick(&self) {
        unsafe {
            let req = st_rd!(BRIGHT_REQ);
            if req == 0 { return; }
            st_wr!(BRIGHT_REQ, 0);
            if req == 1 {
                let v = st_rd!(BRIGHT_REQ_VAL);
                fw_api::brightness_set_value(v);
            } else if req == 2 {
                let cur = fw_api::brightness_get_auto_adjustment();
                let nv = if cur != 0 { 0u32 } else { 1u32 };
                // 关自动前记住当前亮度: 固件关自动时会 apply(state[+0x0C]=手动值),
                //   我们从没写过手动值 → 0 → 息屏。先记下 target, 关完立刻补上。
                let target = fw_api::brightness_get_target();
                fw_api::brightness_set_auto_adjustment(nv);
                if nv == 0 && target > 0 {
                    fw_api::brightness_apply_raw(target);
                }
            } else if req == 4 {
                let v = st_rd!(BRIGHT_REQ_VAL);
                fw_api::brightness_apply_raw(v);
            }
            // 刷新亮度页(存活门)
            if PAGES[11].built == 0 || !page_is_live(11) { return; }
            bright_fill_status();
            let rows = page_rows_ptr(11);
            let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
                fw_api::row_update;
            let r0 = read_volatile(rows.add(0));
            if r0 != 0 {
                upd(r0, core::ptr::null(), "当前亮度\0".as_bytes().as_ptr(),
                    core::ptr::addr_of!(BRIGHT_STATUS) as *const u8, 0, 0);
            }
            let auto_on = fw_api::brightness_get_auto_adjustment() as u8;
            let r3 = read_volatile(rows.add(3));
            if r3 != 0 {
                upd(r3, core::ptr::null(), "自动亮度\0".as_bytes().as_ptr(),
                    core::ptr::null(), 0, auto_on);
            }
        }
    }
}

// ===== 页12: 缓存清理(核心逻辑移植自 Shell++, 见 cacheclean.rs) =====
//
// (2026-10-15 二版) 页面加卡片: 每个根目录一张(主标签=路径, 副标签=占用),
// 与 shellpp 的清理页同形态。行0=总量状态, 行1..4=四张卡, 行5=清理按钮。
// 布局走 ctx.rows 槽 0..5(<= MAX_ROWS_PER_PAGE, 一屏装得下)。

pub(crate) struct CachePage;

impl Page for CachePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "缓存清理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 首行文本: 刚出清理结果就展示结果(消费一次), 否则现场扫描
            if st_rd!(crate::cacheclean::CC_HAS_RESULT) == 0 {
                crate::cacheclean::scan_msg();
            } else {
                st_wr!(crate::cacheclean::CC_HAS_RESULT, 0);
            }
            let msg = crate::cacheclean::msg();
            let r0 = build_row(ctx, ctx.title, 0, &RowSpec::display(msg, core::ptr::null()));
            let mut last = if r0 != 0 { r0 } else { ctx.title };
            // 四张路径卡片(不可点: 主标签=路径, 副标签=占用)
            let mut r = 1usize;
            while r <= crate::cacheclean::ROOT_N {
                let spec = RowSpec::display(crate::cacheclean::root_path(r - 1),
                                            crate::cacheclean::root_usage(r - 1).as_ptr());
                let card = build_row(ctx, last, r, &spec);
                if card != 0 { last = card; }
                r += 1;
            }
            let label: &[u8] = if crate::cacheclean::CC_ARM.armed() {
                "再次点击确认清理\0".as_bytes()
            } else {
                "开始清理\0".as_bytes()
            };
            let r5 = build_row(ctx, last, 5, &RowSpec::clickable(label));
            if r5 == 0 { return Err(PageError::RowFailed(5)); }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx != 5 { return; }
            if !crate::cacheclean::CC_ARM.click() {
                // 第一次点: 武装确认(只改行文本, 不在事件回调做长 FS 操作)
                let rows = page_rows_ptr(12);
                let r1 = read_volatile(rows.add(5));   // 行5 = 清理按钮(1..4 是路径卡片)
                if r1 != 0 {
                    fw_api::row_update(r1, fw_api::NONE_PTR,
                        "再次点击确认清理\0".as_bytes().as_ptr(), fw_api::NONE_PTR, 0, 0);
                }
            } else {
                st_wr!(crate::cacheclean::CC_REQ, 1);   // 清理由 tick 执行
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { st_rd!(crate::cacheclean::CC_REQ) != 0 }
    }

    fn tick(&self) {
        unsafe {
            if st_rd!(crate::cacheclean::CC_REQ) == 0 { return; }
            st_wr!(crate::cacheclean::CC_REQ, 0);
            crate::cacheclean::clean();
            st_wr!(crate::cacheclean::CC_HAS_RESULT, 1);
            render_req(Pid::Cache as u32);
        }
    }
}


// ===== 页16: 主页 -> 震动 =====
//
// 两个开关独立持久化 /data/chaos/swipe_vib.cfg, 默认全关; "测试震动"是动作行。
// 走统一菜单骨架, 返回靠标题栏返回键。
pub(crate) struct VibPage;

unsafe fn vib_list_get() -> bool { crate::background_home::swipe_list_on() != 0 }
unsafe fn vib_list_toggle() {
    crate::background_home::set_swipe_vib(true,
        if crate::background_home::swipe_list_on() != 0 { 0 } else { 1 });
}
unsafe fn vib_back_get() -> bool { crate::background_home::swipe_back_on() != 0 }
unsafe fn vib_back_toggle() {
    crate::background_home::set_swipe_vib(false,
        if crate::background_home::swipe_back_on() != 0 { 0 } else { 1 });
}
unsafe fn vib_test() { crate::background_home::vib_test_req(); }

static VIB_LIST_DEF: ToggleDef = ToggleDef { get: vib_list_get, toggle: vib_list_toggle, status: None };
static VIB_BACK_DEF: ToggleDef = ToggleDef { get: vib_back_get, toggle: vib_back_toggle, status: None };

static VIB_ROWS: &[RowModel] = &[
    RowModel::toggle("滑动列表震动\0".as_bytes(), &VIB_LIST_DEF),
    RowModel::toggle("右滑返回震动\0".as_bytes(), &VIB_BACK_DEF),
    RowModel::action("测试震动\0".as_bytes(), vib_test, 0, None),
];

impl MenuSpec for VibPage {
    fn rows(&self) -> &'static [RowModel] { VIB_ROWS }
}

impl Page for VibPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "震动\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页14: 设置 =====
//
// 走统一菜单骨架(MenuSpec + RowModel): 分组标题不占行槽, 11 行在预算内。
// 信息架构(只重排, 不增删入口):
//   显示与交互  亮度 / 震动 / 屏幕翻转 / 应用图标
//   时间与外观  时间设置 / 日期设置 / 系统关于页面修改
//   存储与维护  缓存清理 / 运行日志 / 恢复fetch支持 / 文件桥
// 页名字符串沿用 `about`(registry.rs): 它只是 page_goto 的解析键, 与标题无关。

unsafe fn enter_time() {
    crate::page::time::time_edit_enter(crate::page::time::TIME_PID as u32);
}
unsafe fn enter_date() {
    crate::page::time::time_edit_enter(crate::page::time::DATE_MD_PID as u32);
}

unsafe fn rot_get() -> bool { crate::screen_rotation::enabled() }
unsafe fn rot_toggle() { crate::screen_rotation::toggle(); }
unsafe fn bridge_get() -> bool { crate::bridge_cfg::enabled() }
unsafe fn bridge_toggle() {
    let on = crate::bridge_cfg::toggle();
    crate::log::kv("文件桥开关 ", if on { 1 } else { 0 });
}
unsafe fn appicon_sub() -> *const u8 {
    crate::dynamic_icon::name(crate::dynamic_icon::selected()).as_ptr()
}

static ROT_DEF: ToggleDef = ToggleDef { get: rot_get, toggle: rot_toggle, status: None };
static PALM_DEF: ToggleDef = ToggleDef {
    get: crate::touch_palm::enabled, toggle: crate::touch_palm::toggle, status: None,
};
static BRIDGE_DEF: ToggleDef = ToggleDef { get: bridge_get, toggle: bridge_toggle, status: None };

static SETTINGS_ROWS: &[RowModel] = &[
    RowModel::section("显示与交互\0".as_bytes()),
    RowModel::nav("亮度\0".as_bytes(), Pid::Bright as u32),
    RowModel { label: "自动熄屏秒数\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Fn(crate::dimmer_timeout::open), trailing: 0, refresh: None },
    RowModel::nav("震动\0".as_bytes(), Pid::Vibrate as u32),
    RowModel { label: "关闭遮盖熄屏\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Toggle(&PALM_DEF), trailing: fw_api::trailing::SWITCH, refresh: None },
    RowModel { label: "屏幕翻转\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Toggle(&ROT_DEF), trailing: fw_api::trailing::SWITCH, refresh: None },
    RowModel { label: "应用图标\0".as_bytes(),
               sub: RowSub::Dyn(appicon_sub),
               kind: RowKind::Nav(Pid::AppIcon as u32),
               trailing: 0, refresh: None },
    RowModel::section("时间与外观\0".as_bytes()),
    RowModel { label: "时间设置\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Fn(enter_time), trailing: 0, refresh: None },
    RowModel { label: "日期设置\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Fn(enter_date), trailing: 0, refresh: None },
    RowModel::nav("系统关于页面修改\0".as_bytes(), Pid::AboutMod as u32),
    RowModel::section("存储与维护\0".as_bytes()),
    RowModel::nav("缓存清理\0".as_bytes(), Pid::Cache as u32),
    RowModel::nav("运行日志\0".as_bytes(), Pid::Log as u32),
    RowModel::nav("日志级别设置\0".as_bytes(), Pid::LogLevel as u32),
    RowModel::nav("恢复fetch支持\0".as_bytes(), Pid::Fetch as u32),
    RowModel { label: "文件桥\0".as_bytes(), sub: RowSub::None,
               kind: RowKind::Toggle(&BRIDGE_DEF), trailing: fw_api::trailing::SWITCH, refresh: None },
];

impl MenuSpec for SettingsPage {
    fn rows(&self) -> &'static [RowModel] { SETTINGS_ROWS }
}

impl Page for SettingsPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 开机后第一次进设置页补一次方向；已经对齐过就是一次原子读。
            crate::screen_rotation::ensure_applied();
            frame::build(self, ctx, ctx.title)
        }
    }
    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}


// ===== 页19: 设置 -> 恢复 fetch 支持 =====
pub(crate) struct FetchPage;
pub(super) static PAGE_FETCHP: FetchPage = FetchPage;
static FETCH_HELP: &[u8] = "使用：\n1.前提条件：安装了autorun（如果没安装请在AstroBox上搜索或者加群1018886448获取autorun最新版本）\n2.开启方式：在Chaos的注入器表盘中点击注入设置->注册自启动，然后在autorun表盘中按照流程开启自启动，重启即可。\n3.注意事项：此功能只保证在小米运动健康中正常使用。\0".as_bytes();

/// 页19 行模型: 一条长说明(Note), 挂在原生可滚动的 content 上。
static FETCH_ROWS: &[RowModel] = &[RowModel::note(FETCH_HELP)];

impl MenuSpec for FetchPage {
    fn rows(&self) -> &'static [RowModel] { FETCH_ROWS }
}

impl Page for FetchPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "恢复fetch支持\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn on_click(&self, _idx: usize) {}
}


pub(super) static PAGE_BRIGHT: BrightnessPage = BrightnessPage;
pub(super) static PAGE_CACHEP: CachePage = CachePage;
pub(super) static PAGE_VIBP: VibPage = VibPage;
pub(super) static PAGE_SETTINGS: SettingsPage = SettingsPage;
