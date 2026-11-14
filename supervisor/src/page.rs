//! 页面框架：Page trait、PageCtx、行/按钮构建器与 PAGE_TABLE。
//!
//! 具体页面按功能拆在 `page/` 子模块；本文件只保留跨页面共享的框架代码。
//
// 页面抽象层: 每个页面是一个有状态的对象, 通过 PAGE_TABLE 分发
//
// 架构原则:
//   1. 页面知道自己的 id —— trait 方法不传 pid
//   2. 页面专属状态存在 struct 里(AtomicU32 内部可变性), 不再用全局 static mut
//   3. render 返回 Result, 失败可追溯
//   4. 新增页面 = 实现 Page + 加进 PAGE_TABLE, 不改 ui.rs

use crate::*;

mod home;
mod time;
mod log;
mod watchface;
mod settings;
mod beautify;
mod appmgr;
mod about;
mod system;
mod keyboard;
mod debug;
mod capture;
mod app_icon;
mod quickapp;
// 单一页注册表(Pid / PAGE_DEFS / PAGE_TABLE)与刷新总线(dirty)。
// 页身份三处对齐由 registry 的宏 + 编译期断言兜住; ipc.rs 的页名指针从 PAGE_DEFS 取。
mod registry;
pub(crate) mod dirty;
// 视觉 token / 页面骨架 / 组件 kit / 进入微动效(B1 起)。
pub(crate) mod theme;
pub(crate) mod frame;
pub(crate) mod motion;
pub(crate) mod kit;
pub(crate) use registry::{Pid, PAGE_DEFS, PAGE_TABLE};
pub(crate) use theme::T;

pub(crate) use beautify::BLUR_LEGACY_PID;

// ===== 错误类型 =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageError {
    /// 某个关键行控件创建失败
    RowFailed(usize),
}

pub(crate) type PageResult = Result<(), PageError>;

// ===== 渲染上下文 =====

/// 框架建好 content/title 后传给页面的渲染句柄
pub(crate) struct PageCtx {
    pub content: u32,
    pub title: u32,
    pub rows: *mut u32,
}

// ===== 行构建器 =====

/// 行控件规格。用 builder 模式配置, 避免带一堆位置参数的构造函数。
#[derive(Clone, Copy)]
pub(crate) struct RowSpec<'a> {
    pub text: &'a [u8],
    pub sub: *const u8,
    pub trailing: u32,
    pub clickable: bool,
}

impl<'a> RowSpec<'a> {
    /// 普通可点击行
    pub fn clickable(text: &'a [u8]) -> Self {
        Self { text, sub: core::ptr::null(), trailing: 0, clickable: true }
    }
    /// 不可点击的展示行
    pub fn display(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: 0, clickable: false }
    }
    /// 可点击的展示行: 样子与 `display` 一样(主标签 + 可选的副标签), 但它挂事件。
    /// 用在"点这一行要触发一个动作、但这一行不该有开关/勾选框"的场合
    /// (页5 的"补写一遍"行就是: 点它启动或中断任务, 但行右端不放部件)。
    /// 注意: 这种场合若改用 `display`, 它 `clickable: false`, 点上去没有任何反应。
    pub fn tap(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: 0, clickable: true }
    }
    /// switch 开关行
    pub fn switch(text: &'a [u8]) -> Self {
        Self { text, sub: core::ptr::null(), trailing: fw_api::trailing::SWITCH, clickable: true }
    }
    /// 复选框行(表盘切换列表同款)。`sub` 非 NULL => 双排(主标签 + 副标签), 与表盘页
    /// 的"名称 + ID"同形; 勾选态由调用方用 `row_update` 的第 6 参**原地**写。
    /// 为什么可以用: 复选框是行控件自带的 trailing 部件(`trailing::CHECKBOX` = 2,
    /// 见 ui.rs rows_sync), 内核模块直接调 `row_create` 就能拿到。"原生 checkbox 只有
    /// 原生 ELF 能调"那个限制只针对 Lua 绑定够不到 `lvx_widgets`, 与内核模块无关。
    pub fn check(text: &'a [u8], sub: *const u8) -> Self {
        Self { text, sub, trailing: fw_api::trailing::CHECKBOX, clickable: true }
    }
}

/// 行内副标签(值)的对象偏移。`lvx_list_item_update` 的参数归属已实测核对:
/// primary -> row+0x3c, secondary -> row+0x40(见 fw_api::row_update 注释)。
pub(crate) const ROW_SUB_LB: u32 = 0x40;

/// 按 RowSpec 建一行 + 挂事件 + 对齐。返回行句柄, 0=失败。
/// 重要: slot 必须在 MAX_ROWS_PER_PAGE 之内，ctx.rows 按同一常量分配。
/// 越界写会破坏相邻静态量，因此创建前必须检查上界。
pub(crate) unsafe fn build_row(ctx: &PageCtx, prev: u32, slot: usize, spec: &RowSpec) -> u32 {
    if slot >= fw_api::MAX_ROWS_PER_PAGE { return 0; }
    let r = fw_api::row_create(ctx.content, spec.text.as_ptr(), spec.sub, spec.trailing);
    if r == 0 { return 0; }
    write_volatile(ctx.rows.add(slot), r);
    // 注意: 标签自套不在这里隐式全量执行(28px 统一写会把文件管理/表盘管理等
    // 列表行拉成大字)。改由 InfoPage::render 显式调用, 只作用于主页行。
    if spec.trailing == fw_api::trailing::SWITCH {
        let tobj = fw_api::row_trailing(r);
        if tobj != 0 { fw_api::obj_add_event(tobj, row_cb(), 0, slot as u32); }
        else { fw_api::obj_add_event(r, row_cb(), 7, slot as u32); }
    } else if spec.clickable {
        fw_api::obj_add_event(r, row_cb(), 7, slot as u32);
        // 复选框行: 复选框自己也挂一份, 点到圈上也算点这一行(表盘页只挂行本体,
        // 点到圈上没有反应)。这一行不是 switch 行, 所以复选框发出来的 VALUE_CHANGED
        // 会被 chaos_row_dispatch 丢掉, 只有 CLICKED 会走到 on_click。
        if spec.trailing == fw_api::trailing::CHECKBOX {
            let tobj = fw_api::row_trailing(r);
            if tobj != 0 { fw_api::obj_add_event(tobj, row_cb(), 0, slot as u32); }
        }
    }
    fw_api::obj_align_to(r, prev, 14, 0, T::ROW_GAP);
    r
}

/// 批量建行, 任意一行失败即返回 Err
pub(crate) unsafe fn build_rows(
    ctx: &PageCtx, mut prev: u32, specs: &[(usize, RowSpec)],
) -> Result<u32, PageError> {
    for &(slot, ref spec) in specs {
        let r = build_row(ctx, prev, slot, spec);
        if r == 0 { return Err(PageError::RowFailed(slot)); }
        prev = r;
    }
    Ok(prev)
}

// ===== 紧凑按钮块(一行放多个按钮) =====

/// 单个按钮块的标准尺寸。行控件默认按"双排列表行"占高, 四五个按钮就会把
/// 480 高的屏铺满, 所以按钮一律显式给宽高、走这些常量。
pub(crate) const TILE_W: u32 = 160;
pub(crate) const TILE_H: u32 = 64;
/// 整行(328)减去两个按钮和中间 8px 缝: 160*2+8
pub(crate) const TILE_ROW_W: u32 = 328;
/// 半宽按钮居中到父容器中心需要的横向补偿(左边块的中心比内容中心小 84)
pub(crate) const TILE_CENTER_DX: i32 = 84;

unsafe fn tile_base(ctx: &PageCtx, slot: usize, text: &[u8], w: u32, h: u32) -> u32 {
    if slot >= fw_api::MAX_ROWS_PER_PAGE || text.is_empty() { return 0; }
    let r = fw_api::row_create(ctx.content, text.as_ptr(), fw_api::NONE_PTR, 0);
    if r == 0 { return 0; }
    write_volatile(ctx.rows.add(slot), r);
    fw_api::obj_set_size_wh(r, w, h);
    cardify(r);
    fw_api::obj_set_local_style_prop(r, crate::fw_api::fwaddr::p65_style::RADIUS, h / 2, 0);
    fw_api::obj_add_event(r, row_cb(), 7, slot as u32);
    r
}

/// 把按钮块染成主操作(蓝底白字)或危险操作(深底红字)。
pub(crate) unsafe fn tile_primary(row: u32) {
    use crate::fw_api::fwaddr::p65_style as ps;
    if row == 0 { return; }
    fw_api::obj_set_local_style_prop(row, ps::BG_COLOR, C_ACCENT, 0);
    let lb = fw_api::obj_get_child(row, 0);
    if lb != 0 { fw_api::obj_set_local_style_prop(lb, ps::TEXT_COLOR, 0xFFFFFF, 0); }
}


pub(crate) fn pill_w(txt: &[u8]) -> u32 {
    let mut w = 30u32;
    let mut i = 0usize;
    while i < txt.len() {
        let b = txt[i];
        if b == 0 { break; }
        if b >= 0xE0 { w += 34; i += 3; }
        else if b >= 0xC0 { w += 30; i += 2; }
        else { w += 16; i += 1; }
    }
    if w < 132 { 132 } else if w > 236 { 236 } else { w }
}

pub(crate) unsafe fn pill(lab: u32, col: u32, bg: u32, txt: &[u8]) {
    use crate::fw_api::fwaddr::p65_style as ps;
    if lab == 0 { return; }
    fw_api::obj_style_color_a70(lab, col, 0);
    fw_api::obj_set_local_style_prop(lab, ps::BG_COLOR, bg, 0);
    fw_api::obj_set_local_style_prop(lab, ps::BG_OPA, 255, 0);
    fw_api::obj_set_local_style_prop(lab, ps::RADIUS, T::R_PILL, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_0, 1, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_1, 1, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_3, 0, 0);
    // 上下只留 4: 24px 字体 + 8 内边距 = 32, 能装进 34 高的胶囊, 文字不会被裁/滚动
    fw_api::obj_set_local_style_prop(lab, ps::PAD_2, 4, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_3, 4, 0);
    // TEXT_ALIGN = 94, CENTER = 2 (蓝牙模块验证过的属性号)
    fw_api::obj_set_local_style_prop(lab, 94, 2, 0);
    fw_api::obj_set_size_wh(lab, pill_w(txt), 34);
    fw_api::obj_clear_scrollable(lab);
}

pub(crate) unsafe fn tile_danger(row: u32) {
    use crate::fw_api::fwaddr::p65_style as ps;
    if row == 0 { return; }
    fw_api::obj_set_local_style_prop(row, ps::BG_COLOR, 0x2A2A2A, 0);
    let lb = fw_api::obj_get_child(row, 0);
    if lb != 0 { fw_api::obj_set_local_style_prop(lb, ps::TEXT_COLOR, C_DANGER, 0); }
}

/// 在基准行下方左侧建一个按钮块(必须左对齐, 否则半宽块会被居中而整体右移)
pub(crate) unsafe fn build_tile_below(
    ctx: &PageCtx, prev: u32, slot: usize, text: &[u8], w: u32, h: u32,
) -> u32 {
    let r = tile_base(ctx, slot, text, w, h);
    if r != 0 { fw_api::obj_align_to(r, prev, fw_api::align::OUT_BOTTOM_LEFT, 0, 8); }
    r
}

/// 在基准按钮块右侧建一个按钮块
pub(crate) unsafe fn build_tile_right(
    ctx: &PageCtx, base: u32, slot: usize, text: &[u8], w: u32, h: u32,
) -> u32 {
    let r = tile_base(ctx, slot, text, w, h);
    if r != 0 { fw_api::obj_align_to(r, base, fw_api::align::OUT_RIGHT_MID, 8, 0); }
    r
}

/// 行建立后压高度(用于状态/信息行, 默认双排高度太占屏)
pub(crate) unsafe fn row_shrink(row: u32, h: u32) {
    if row != 0 { fw_api::obj_set_size_wh(row, TILE_ROW_W, h); }
}

/// 卡片样式：圆角 + 深灰底色 + 左右内边距。只改外观，不动滚动与点击。
/// 卡片外观交给固件原生行自己画(Chaos 原本的灰底卡片就是它), 这里只关滚动。
pub(crate) unsafe fn cardify(row: u32) {
    if row == 0 { return; }
    no_scroll(row);
}

/// 关掉对象及其后代(两层)的 SCROLLABLE: 文字超长时裁剪, 不让整行跟手划动。
pub(crate) unsafe fn no_scroll(obj: u32) {
    if obj == 0 { return; }
    fw_api::obj_clear_scrollable(obj);
    let mut i = 0u32;
    while i < 4 {
        let c = fw_api::obj_get_child(obj, i);
        if c == 0 { break; }
        fw_api::obj_clear_scrollable(c);
        i += 1;
    }
}

// ===== 跨页调试状态 =====
pub(crate) static mut DBG_ON: u32 = 0;
pub(crate) static mut DBG_CLICKS: u32 = 0;

// ===== 共享 UI 配色 =====
// 真源在 theme.rs 的 T::*; 这些旧名是别名, 既有调用点不动, 逐步改成 T::。
#[allow(dead_code)] pub(crate) const C_SECONDARY: u32 = T::SECONDARY;
pub(crate) const C_ACCENT: u32 = T::ACCENT;
#[allow(dead_code)] pub(crate) const C_SUCCESS: u32 = T::SUCCESS;
pub(crate) const C_DANGER: u32 = T::DANGER;
pub(crate) const C_HIDDEN: u32 = T::HIDDEN;
pub(crate) const C_HIDDEN_TINT: u32 = T::HIDDEN_TINT;
pub(crate) const C_VISIBLE_TINT: u32 = T::VISIBLE_TINT;
pub(crate) const C_VISIBLE: u32 = T::VISIBLE;

// ===== Page trait =====

/// 页面内容几何: 框架建 content 时按它定尺寸。
/// List = 列表页口径(336x424 摆在 y=56, 底部留白 32); FullBleed = 整页口径
/// (336x480 摆在 y=0, 无留白) —— 时间/日期滚轮页那组字面偏移(滚轮 dy=0、
/// 遮罩 +104/-105、提示标签 +75)是按整页算的, 不走 List 的 56/32 基准。
pub(crate) enum Geom { List, FullBleed }

/// 页面行为接口。页面知道自己是谁, 不需要外部传 pid。
pub(crate) trait Page: Sync {
    /// 标题文本写入 buf(48B), 必须以 \0 结尾
    fn fill_title(&self, buf: *mut u8);
    /// 标题模式: 0=无返回键, 1=有返回键
    fn title_mode(&self) -> u32 { 1 }
    /// 内容几何(默认列表页口径; 时间/日期滚轮页返回 FullBleed)
    fn geometry(&self) -> Geom { Geom::List }
    /// 建页面专属控件(框架已建好 content + title)
    fn render(&self, ctx: &PageCtx) -> PageResult;
    /// 进入页面(on_resume, FG_PAGE 已置位、render 之前)。进页要重读的数据放这里。
    fn on_enter(&self) {}
    /// 离开前台/退后台(on_pause)。只清逻辑旗, 不碰对象树。
    fn on_pause(&self) {}
    /// 息屏回调(on_destroy)。息屏时对象树仍在, 禁止碰树/句柄 —— 只清逻辑旗。
    fn on_screen_off(&self) {}
    /// 对象树即将被固件递归删除(on_ui_destroy)。句柄清理由框架做, 页面只作废自己的缓存。
    fn on_tree_gone(&self) {}
    /// 新页面实例交付(on_create)。旧实例连同对象树已被销毁, 页面在这里作废
    /// 挂在页根上的浮层状态(确认框等)与自己的句柄缓存。
    fn on_instance_reset(&self) {}
    /// 行点击(CLICKED 事件)
    fn on_click(&self, idx: usize);
    /// 开关翻转(VALUE_CHANGED 事件)
    fn on_switch(&self, _idx: usize) {}
    /// 该行是否是 switch 行(事件过滤用)
    fn is_switch_row(&self, _idx: usize) -> bool { false }
    /// 行显隐(渲染后调用)
    fn apply_visibility(&self) {}
    /// 由常驻 lv_timer 调用(忙 50ms / 息屏全空闲 1000ms), 做延迟请求消费 + UI 刷新。
    /// 只在本页是前台页且对象树存活时才被框架调用。
    fn tick(&self) {}
    /// 本页是否有待处理的 tick 逻辑(框架用, 避免空转)
    fn needs_tick(&self) -> bool { false }
}


pub(crate) unsafe fn cstr_len(s: *const u8) -> usize {
    let mut n = 0usize;
    while read_volatile(s.add(n)) != 0 { n += 1; }
    n
}


// PAGE_TABLE 已搬到 page/registry.rs: 与 Pid 枚举、页名一起由 `pages!` 宏一次生成,
// 页号/页名/页实例三处对齐由编译期断言兜住(见 registry.rs 头注释)。
