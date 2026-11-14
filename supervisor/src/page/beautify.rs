//! 页4/5/6/7/13/15/28：字体、图标、模糊与桌面布局美化。

use crate::page::*;
use crate::page::Page;
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub, ToggleDef};

// ===== 页13: 系统美化(菜单) / 页5: 更换字体 / 页6: 桌面图标 =====
//
// 结构(一个菜单页 + 两个二级页, 二级页要带页面跳转动画):
//   页13 菜单    行0 更换字体 -> nav_goto(5) / 行1 桌面图标 -> nav_goto(6) / 行2 返回
//   页5  字体    行0..7 字体条目(动态建行 + 真复选框) / 行8 重新应用字体 /
//                行9 再补写一遍 / 行11 返回(nav_back)
//   页6  图标    行0..9 图标包条目(动态建行 + 真复选框) / 行11 返回(nav_back)
//
// 为什么两个二级页要占独立 page_id: 固件的页面跳转动画只在真正的 page_goto/page_back
// 里有(页面注册见 ipc.rs)。页内切层级(只 render_req)没有动画, 整页内容原地换一批,
// 观感上就不是"进了另一个页面"。文件管理每级目录各占一个 page_id 也是同一个原因。
// 代价: 原生应用最多注册 14 页, 再多会在安装过程中黑屏自重启(机制未验证, 按硬上限处理);
// 所以文件管理从 6 级压到 3 级腾出三个 page_id(页4/5/6) —— 注册总数仍是 14, 只是重新分配。
//
// 行位分配都受 MAX_ROWS_PER_PAGE(=12) 封顶: 条目行从行 0 起排, 页5/页6 的行 11 固定"返回"。

/// 图标列表的第 0 个行槽(页6: 行0..9 共 10 行, 行11 返回)
const ICON_ROW0: usize = 0;

/// 静态 C 串缓冲 -> 带结尾 NUL 的切片。行控件读的是 C 串, 传切片是给 RowSpec 用。
unsafe fn cbuf(p: *const u8) -> &'static [u8] {
    let n = cstr_len(p);
    if n == 0 { return &[]; }
    core::slice::from_raw_parts(p, n + 1)
}

// ===== 页13: 系统美化(菜单, 四条进二级页) =====


pub(crate) struct BeautifyPage;

// ===== 页13 的开关定义 =====
unsafe fn restore_font_get() -> bool { crate::beautify_restore::enabled() }
unsafe fn restore_font_toggle() { crate::beautify_restore::request_toggle(); }
unsafe fn restore_font_status() -> *const u8 { crate::beautify_restore::status() }
unsafe fn restore_blur_get() -> bool { crate::res_hook::held() }
unsafe fn restore_blur_toggle() { crate::res_hook::request_toggle(); }
unsafe fn restore_blur_status() -> *const u8 { crate::res_hook::status() }

static RESTORE_FONT_DEF: ToggleDef = ToggleDef {
    get: restore_font_get, toggle: restore_font_toggle, status: Some(restore_font_status) };
static S541_DEF: ToggleDef = ToggleDef {
    get: crate::s541_launcher::enabled, toggle: crate::s541_launcher::toggle, status: None };
static RESTORE_BLUR_DEF: ToggleDef = ToggleDef {
    get: restore_blur_get, toggle: restore_blur_toggle, status: Some(restore_blur_status) };

/// 页13 行模型(分组标题与说明不占行槽)。信息架构(只重排, 不删入口):
///   字体与图标  更换字体 / 桌面图标 / 应用列表设置
///   模糊与背景  系统模糊 / 模糊区域 + 重启恢复两个开关 + corona 说明
/// 返回主路径 = 标题栏返回键, 不再自建"返回"行。
static BEAUTIFY_ROWS: &[RowModel] = &[
    RowModel::section("字体与图标\0".as_bytes()),
    RowModel::nav("更换字体\0".as_bytes(), Pid::FontList as u32),
    RowModel::nav("桌面图标\0".as_bytes(), Pid::IconSub as u32),
    RowModel::nav("应用列表设置\0".as_bytes(), Pid::AppList as u32),
    RowModel::toggle("s541控制按钮\0".as_bytes(), &S541_DEF),
    RowModel::section("模糊与背景\0".as_bytes()),
    RowModel::nav("系统模糊\0".as_bytes(), Pid::Blur as u32),
    RowModel::nav("模糊区域\0".as_bytes(), Pid::BlurZone as u32),
    RowModel::toggle("重启恢复字体和图标\0".as_bytes(), &RESTORE_FONT_DEF),
    RowModel::toggle("重启恢复模糊\0".as_bytes(), &RESTORE_BLUR_DEF),
    RowModel::note("说明：“corona模糊”功能源码来自https://github.com/AstralSightStudios/Canopus-Module-Corona，修改后源码可见：https://github.com/WenHuaYiYang/Chaos-Module/tree/master/supervisor\0".as_bytes()),
];

impl MenuSpec for BeautifyPage {
    fn rows(&self) -> &'static [RowModel] { BEAUTIFY_ROWS }
}

impl Page for BeautifyPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统美化\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页28: 系统美化 -> 应用列表设置(二级) =====
//
// 桌面图标的三条排版参数都在 desktop_scale 那张写表层: 行0 = 缩放折线(同 RW6),
// 行1 = 排距锚点(同 RW6), 行2 = 外列内缩(同 RW6)。三个开关都默认开, 关掉就把
// 对应的表写回固件原值；开关改参数后在 UI 线程重新应用。
// 蜂窝与这些开关互斥：蜂窝开时清空 RW6 三项；点开任一 RW6 项则关闭蜂窝。
pub(crate) struct AppListPage;

// 页28 开关定义
unsafe fn ds_lens_get() -> bool { crate::desktop_scale::lens_on() }
unsafe fn ds_lens_toggle() { crate::desktop_scale::set_lens(!crate::desktop_scale::lens_on()); }
unsafe fn ds_pitch_get() -> bool { crate::desktop_scale::pitch_on() }
unsafe fn ds_pitch_toggle() { crate::desktop_scale::set_pitch(!crate::desktop_scale::pitch_on()); }
unsafe fn ds_inset_get() -> bool { crate::desktop_scale::inset_on() }
unsafe fn ds_inset_toggle() { crate::desktop_scale::set_inset(!crate::desktop_scale::inset_on()); }
unsafe fn honey_get() -> bool { crate::honeycomb::enabled() }
unsafe fn honey_toggle() { crate::honeycomb::toggle(); }

static DS_LENS_DEF: ToggleDef = ToggleDef { get: ds_lens_get, toggle: ds_lens_toggle, status: None };
static DS_PITCH_DEF: ToggleDef = ToggleDef { get: ds_pitch_get, toggle: ds_pitch_toggle, status: None };
static DS_INSET_DEF: ToggleDef = ToggleDef { get: ds_inset_get, toggle: ds_inset_toggle, status: None };
static HONEY_DEF: ToggleDef = ToggleDef { get: honey_get, toggle: honey_toggle, status: None };

static APPLIST_ROWS: &[RowModel] = &[
    RowModel::toggle("缩放同 RW6\0".as_bytes(), &DS_LENS_DEF),
    RowModel::toggle("排距同 RW6\0".as_bytes(), &DS_PITCH_DEF),
    RowModel::toggle("内缩同 RW6\0".as_bytes(), &DS_INSET_DEF),
    RowModel::toggle("蜂窝视图\0".as_bytes(), &HONEY_DEF),
    RowModel::note("中间区域上下左右拖动图标。\n左侧上滑缩小间距，下滑放大；右侧上滑放大整体，下滑缩小。\n到顶部继续下拉，或到左端继续右滑返回。轻点打开应用，长按进入编辑并按提示删除。\0".as_bytes()),
];

impl MenuSpec for AppListPage {
    fn rows(&self) -> &'static [RowModel] { APPLIST_ROWS }
}

impl Page for AppListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用列表设置\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页4: 系统美化 -> 系统模糊(二级) =====
//
// 背景档位与状态文案都在 background_home(0 关 / 1 复用静态背景 / 3 完全透明),
// 这里只建行与派发点击。返回固定走行 11 —— 与"更换字体""桌面图标"两页同一套。
// 两种算法供对照：Corona原滤波和SysPatch IIR(V2)，四区适配暂时共用。
pub(crate) struct BlurPage;

pub(crate) const BLUR_LEGACY_PID: u32 = 7;

// 页4 开关/刷新定义
unsafe fn blur_bg_off() {
    // 选回官方背景须关闭两开关；引擎和 v2 图标都由背景生命周期安全退役。
    crate::corona_blur::set(false);
    crate::corona_blur::set_v2_enabled(false);
    crate::background_home::request(0);
}
unsafe fn blur_bg_refresh(slot: usize) {
    let pid = st_rd!(FG_PAGE) as usize;
    if pid > MAX_PID { return; }
    let r = read_volatile(page_rows_ptr(pid).add(slot));
    if r != 0 {
        fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
            (crate::background_home::mode() == 0 && !crate::corona_blur::active()) as u8);
    }
}
unsafe fn blur_v2_get() -> bool { crate::corona_blur::v2_enabled() }
unsafe fn blur_corona_get() -> bool { crate::corona_blur::enabled() }
unsafe fn blur_choices_refresh() {
    let pid = st_rd!(FG_PAGE) as usize;
    if pid != Pid::Blur as usize { return; }
    for (slot, on) in [(1, crate::corona_blur::enabled()), (2, crate::corona_blur::v2_enabled())] {
        let row = read_volatile(page_rows_ptr(pid).add(slot));
        if row != 0 { fw_api::row_update(row, fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0, on as u8); }
    }
    blur_bg_refresh(0);
}
unsafe fn blur_corona_toggle() { crate::corona_blur::toggle_corona(); blur_choices_refresh(); }
static BLUR_CORONA_DEF: ToggleDef = ToggleDef { get: blur_corona_get, toggle: blur_corona_toggle, status: None };
unsafe fn blur_v2_toggle() { crate::corona_blur::toggle_v2(); blur_choices_refresh(); }
unsafe fn blur_status_ptr() -> *const u8 { crate::background_home::status().as_ptr() }
static BLUR_V2_DEF: ToggleDef = ToggleDef { get: blur_v2_get, toggle: blur_v2_toggle, status: None };

/// 页4 行模型(菜单骨架; 返回靠标题栏返回键):
///   0 系统背景 / 1 Corona / 2 V2 / 3 状态 / 4 旧版 / 5 区域。
static BLUR_ROWS: &[RowModel] = &[
    RowModel::action("系统背景\0".as_bytes(), blur_bg_off, fw_api::trailing::CHECKBOX, Some(blur_bg_refresh)),
    RowModel::toggle("Corona模糊\0".as_bytes(), &BLUR_CORONA_DEF),
    RowModel::toggle("模糊v2\0".as_bytes(), &BLUR_V2_DEF),
    RowModel { label: "状态\0".as_bytes(), sub: RowSub::Dyn(blur_status_ptr),
               kind: RowKind::Display, trailing: 0, refresh: None },
    RowModel { label: "旧版选项\0".as_bytes(), sub: RowSub::Static("关闭上面的模糊开关后生效\0".as_bytes()),
               kind: RowKind::Nav(BLUR_LEGACY_PID), trailing: 0, refresh: None },
    RowModel { label: "模糊区域\0".as_bytes(), sub: RowSub::Static("适用于所有模糊选项（含通知图标去黑底）\0".as_bytes()),
               kind: RowKind::Nav(Pid::BlurZone as u32), trailing: 0, refresh: None },
    RowModel::note("说明：“corona模糊”功能源码来自https://github.com/AstralSightStudios/Canopus-Module-Corona，修改后源码可见：https://github.com/WenHuaYiYang/Chaos-Module/tree/master/supervisor。\0".as_bytes()),
];

impl MenuSpec for BlurPage {
    fn rows(&self) -> &'static [RowModel] { BLUR_ROWS }
}

impl Page for BlurPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统模糊\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页7: 系统模糊 -> 旧版选项 =====
//
// 这是原系统模糊页的兼容入口。系统背景(主页面)与模糊区域(页15)不在这里重复，
// 其余旧档位/实时截取设置全部集中到本页；corona 开启时保留设置但不应用。
pub(crate) struct BlurLegacyPage;

// 页7 开关定义(初态/翻转都从 background_home 的真源现读)
unsafe fn legacy_static_get() -> bool { crate::background_home::mode() == 1 }
unsafe fn legacy_static_toggle() {
    crate::background_home::request(if crate::background_home::mode() == 1 { 0 } else { 1 });
}
unsafe fn legacy_rt_get() -> bool { crate::background_home::realtime_on() != 0 }
unsafe fn legacy_rt_toggle() {
    crate::background_home::set_realtime(if crate::background_home::realtime_on() != 0 { 0 } else { 1 });
}
unsafe fn legacy_clear_get() -> bool { crate::background_home::mode() == 3 }
unsafe fn legacy_clear_toggle() {
    crate::background_home::request(if crate::background_home::mode() == 3 { 0 } else { 3 });
}
static LEGACY_STATIC_DEF: ToggleDef = ToggleDef { get: legacy_static_get, toggle: legacy_static_toggle, status: None };
static LEGACY_RT_DEF: ToggleDef = ToggleDef { get: legacy_rt_get, toggle: legacy_rt_toggle, status: None };
static LEGACY_CLEAR_DEF: ToggleDef = ToggleDef { get: legacy_clear_get, toggle: legacy_clear_toggle, status: None };
static LEGACY_STATUS_ROWS: &[RowModel] = &[
    RowModel::toggle("静态背景图(表盘)\0".as_bytes(), &LEGACY_STATIC_DEF),
    RowModel::toggle("自动截取替换图片\0".as_bytes(), &LEGACY_RT_DEF),
    RowModel::action("手动生成或更新表盘背景\0".as_bytes(), legacy_generate, 0, None),
    RowModel::toggle("完全透明背景（实验性）\0".as_bytes(), &LEGACY_CLEAR_DEF),
    RowModel { label: "状态\0".as_bytes(), sub: RowSub::Dyn(blur_status_ptr),
               kind: RowKind::Display, trailing: 0, refresh: None },
    RowModel::display("关闭两种模糊后生效\0".as_bytes()),
];

unsafe fn legacy_generate() { crate::background_home::generate(); }

impl MenuSpec for BlurLegacyPage {
    fn rows(&self) -> &'static [RowModel] { LEGACY_STATUS_ROWS }
}

impl Page for BlurLegacyPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "旧版选项\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页5: 系统美化 -> 更换字体(二级) =====
//
// 页面形态: 复选框列表(与"桌面图标"页同款) —— 清单里有几条字体就列几条,
// 点哪条就切到哪条; 已选中的那条再点走两步删除。
// 数据面全在 font_list.rs(清单 / 切换 / 两步删除), 这里只负责建行与派发点击。
//
// 行位: 0..7 = 字体条目(动态, 有几条建几条), 8 = 重新应用字体, 9 = 补写一遍,
// 11 = 返回。条目行数由 font_list::entry_shown() 截断, 与 0..7 这 8 个槽位一致。
// 删除字体的确认浮层叠在本页 content 上, 不占行位。
pub(crate) struct FontListPage;

/// 字体条目行的起始槽位(与 action/返回 隔开: 8 条占 0..7)
const FONT_ROW0: usize = 0;
/// "重新应用字体"那一行的槽位
/// 行 0..7 = 字体条目槽位(FONT_ROW0 起); (v3.6) 原"重新应用字体"(行8)与
/// "补写一遍"(行9)退役: 热替换接管后重应用冗余、逐对象补写不再需要。

impl Page for FontListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            put_str(buf, "更换字体\0".as_bytes());
        }
    }

    /// 进页重读字体清单: 刚投递进去的字体不用重启就该出现在列表里。
    fn on_enter(&self) {
        unsafe { font_list::refresh(); }
    }

    /// 新页面实例交付: 旧页根连同挂在页根上的确认框已被固件销毁, 框状态必须归零,
    /// 否则后续删除会打到复用同一块内存的新对象上(见 confirm_pop.rs)。
    fn on_instance_reset(&self) {
        unsafe { font_list::popup_forget(Pid::FontList as usize); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 确认框挂在页根上, 重建只销毁 content 带不走它 —— 先把它的状态归零。
            font_list::popup_reset();
            font_list::refresh();          // 新投递的字体不用重启就该出现在列表里
            let ent = font_list::entry_shown();
            font_list::lines_fill();
            // 动态建行: 有几条建几行。条目行带真复选框(trailing::CHECKBOX),
            // 勾选态靠 row_update 原地写 —— 与"桌面图标"页、"表盘切换"页同一套。
            let mut last = ctx.title;
            let mut i = 0usize;
            while i < ent as usize {
                let spec = RowSpec::check(cbuf(font_list::line_primary(i)),
                                          font_list::line_secondary(i));
                let r = build_row(ctx, last, FONT_ROW0 + i, &spec);
                if r != 0 { last = r; }
                i += 1;
            }
            // 字体一个都没有时不新增行(复用行 0 那个槽位), 只给一句说明:
            // 空列表页什么都不说会像是界面坏了。副标签指着那句静态文本。
            if ent == 0 {
                let _ = build_row(ctx, last, FONT_ROW0, &RowSpec::display(
                    "还没有投递过字体\0".as_bytes(), FONT_EMPTY_SUB.as_ptr()));
            }
            // 建完行立刻原地补写勾选态, 并把行句柄交给 font_list: 切换与两步删除
            // 的反馈都走原地刷新(不重建整页 —— 重建会把整页字体样式重抄一遍)。
            font_list::bind_rows(ctx.rows, FONT_ROW0, ent as usize);
            font_list::refresh_rows();
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 行0固定为官方字体，行1..8对应八个自定义字体槽位。
                0..=8 => font_list::click_row(idx),
                _ => {}
            }
        }
    }
}
static FONT_EMPTY_SUB: &[u8] = b"\xe5\x85\x88\xe4\xbe\xa7\xe8\xbd\xbd\xe4\xb8\x80\xe4\xb8\xaa\xe5\xad\x97\xe4\xbd\x93\xe6\x8a\x95\xe9\x80\x92\xe5\x8c\x85\0";

// ===== 页6: 系统美化 -> 桌面图标(二级) =====

pub(crate) struct IconSubPage;

impl Page for IconSubPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "桌面图标\0".as_bytes()); }
    }

    /// 进页重读图标包清单: 刚侧载投递完的包不用重启就该出现在列表里。
    fn on_enter(&self) {
        unsafe { icon_apply::refresh_packs(); }
    }

    /// 新页面实例交付: 与字体页同款, 把挂在页根上的确认框状态归零。
    fn on_instance_reset(&self) {
        unsafe { icon_apply::popup_forget(Pid::IconSub as usize); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 确认框挂在本页根对象上: 重建只销毁 content, 带不走它 —— 开头先兜住
            // 前一次没关掉的框, 再把状态归零(与"更换字体"页同款)。
            icon_apply::popup_reset();
            icon_apply::refresh_packs();   // 新投递的包不用重启就该出现在列表里
            let ent = icon_apply::entry_shown();
            icon_apply::lines_fill();
            // 动态建行: 有几条建几行。条目行带真复选框(trailing::CHECKBOX),
            // 勾选态靠 row_update 原地写 —— 与"表盘切换"页同一套做法。
            // 列表第一项是"系统原图标": 这一页的交互就是图标切换(复选框列表), 不是
            // "应用/恢复"两个动作按钮; 做成第 0 项之后切换与还原是同一个动作。
            let mut last = ctx.title;
            let mut i = 0usize;
            while i < ent as usize {
                let spec = RowSpec::check(cbuf(icon_apply::line_primary(i)),
                                          icon_apply::line_secondary(i));
                let r = build_row(ctx, last, ICON_ROW0 + i, &spec);
                if r != 0 { last = r; }
                i += 1;
            }
            // 建完行立刻原地补写勾选态, 并把行句柄交给 icon_apply: 之后点一下切换、
            // 两次确认删除都只走原地刷新(不重建整页 —— 重建会把整页字体样式重抄一遍)。
            icon_apply::bind_rows(ctx.rows, ICON_ROW0, ent as usize);
            icon_apply::refresh_rows();
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 0..9 = 图标包条目。选中语义全在 icon_apply::click_row 里:
                // 点未选中的 = 切过去; 点已选中的包 = 先变成"再点一次删除", 再点才删。
                0..=9 => icon_apply::click_row(idx),
                _ => {}
            }
        }
    }
}


// ===== 页15: 系统模糊 -> 模糊区域(三级) =====
//
// 四个背景替换区域的开关(数据与持久化全在 blur_zone.rs, 这里只建行与派发):
//   行0=启动器 行1=控制中心 行2=小部件 行3=通知中心 行11=返回。
// 开 = 正常替换(默认); 关 = 该区域保持官方模糊画面。
// 对 Corona 开关生效；关闭通知区域也恢复原图标和去黑底 hook。
// 翻位后 blur_zone::set 持久化 + background_home::remount() 置 REQUEST,
// 由背景 tick 的拆/装流程按新掩码统一重建(页面退出前就能看到拆装, 不用重启)。
// 开关初态: 单排行, row_update 第 6 参原地补写(secondary 传 NONE 是安全的 ——
// 这些行没有副标签, 详见 fw_api::row_update 注释)。
pub(crate) struct BlurZonePage;

// 页15 开关定义(四个背景替换区域; 数据与持久化全在 blur_zone.rs)
unsafe fn zone0_get() -> bool { crate::blur_zone::on(0) }
unsafe fn zone0_toggle() {
    if crate::blur_zone::set(0, !crate::blur_zone::on(0)) {
        crate::background_home::remount();
    }
}
static ZONE0_DEF: ToggleDef = ToggleDef { get: zone0_get, toggle: zone0_toggle, status: None };
unsafe fn zone1_get() -> bool { crate::blur_zone::on(1) }
unsafe fn zone1_toggle() {
    if crate::blur_zone::set(1, !crate::blur_zone::on(1)) {
        crate::background_home::remount();
    }
}
static ZONE1_DEF: ToggleDef = ToggleDef { get: zone1_get, toggle: zone1_toggle, status: None };
unsafe fn zone2_get() -> bool { crate::blur_zone::on(2) }
unsafe fn zone2_toggle() {
    if crate::blur_zone::set(2, !crate::blur_zone::on(2)) {
        crate::background_home::remount();
    }
}
static ZONE2_DEF: ToggleDef = ToggleDef { get: zone2_get, toggle: zone2_toggle, status: None };
unsafe fn zone3_get() -> bool { crate::blur_zone::on(3) }
unsafe fn zone3_toggle() {
    if crate::blur_zone::set(3, !crate::blur_zone::on(3)) {
        crate::background_home::remount();
    }
}
static ZONE3_DEF: ToggleDef = ToggleDef { get: zone3_get, toggle: zone3_toggle, status: None };

static ZONE_ROWS: &[RowModel] = &[
    RowModel { label: "适用于两种模糊\0".as_bytes(), sub: RowSub::Static("包括通知图标去黑底\0".as_bytes()),
               kind: RowKind::Display, trailing: 0, refresh: None },
    RowModel::toggle("启动器\0".as_bytes(), &ZONE0_DEF),
    RowModel::toggle("控制中心\0".as_bytes(), &ZONE1_DEF),
    RowModel::toggle("小部件\0".as_bytes(), &ZONE2_DEF),
    RowModel::toggle("通知中心\0".as_bytes(), &ZONE3_DEF),
    RowModel { label: "关闭通知区域恢复原图标\0".as_bytes(), sub: RowSub::Static("同时撤销通知图标去黑底\0".as_bytes()),
               kind: RowKind::Display, trailing: 0, refresh: None },
];

impl MenuSpec for BlurZonePage {
    fn rows(&self) -> &'static [RowModel] { ZONE_ROWS }
}

impl Page for BlurZonePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "模糊区域\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::blur_zone::ensure_loaded();
            frame::build(self, ctx, ctx.title)
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

pub(super) static PAGE_BEAUTIFY: BeautifyPage = BeautifyPage;
pub(super) static PAGE_APPLIST: AppListPage = AppListPage;
pub(super) static PAGE_BLURP: BlurPage = BlurPage;
pub(super) static PAGE_ZONEP: BlurZonePage = BlurZonePage;
pub(super) static PAGE_FONTLIST: FontListPage = FontListPage;
pub(super) static PAGE_ICONSUB: IconSubPage = IconSubPage;
pub(super) static PAGE_BLUR_LEGACYP: BlurLegacyPage = BlurLegacyPage;
