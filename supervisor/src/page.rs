// 页面抽象层: 每个页面是一个有状态的对象, 通过 PAGE_TABLE 分发
//
// 架构原则:
//   1. 页面知道自己的 id —— trait 方法不传 pid
//   2. 页面专属状态存在 struct 里(AtomicU32 内部可变性), 不再用全局 static mut
//   3. render 返回 Result, 失败可追溯
//   4. 新增页面 = 实现 Page + 加进 PAGE_TABLE, 不改 ui.rs

use core::sync::atomic::{AtomicU32, Ordering};
use crate::*;

// ===== 错误类型 =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageError {
    /// content_create 返回 0
    ContentFailed,
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
const ROW_SUB_LB: u32 = 0x40;

/// 按 RowSpec 建一行 + 挂事件 + 对齐。返回行句柄, 0=失败。
/// 重要: slot 必须在 MAX_ROWS_PER_PAGE(=12) 之内 —— ctx.rows 只有 12 个元素,
/// 越界写会破坏相邻静态量, 实测表现为 slot 用到 12/13/14 时点任意一行都崩。
pub(crate) unsafe fn build_row(ctx: &PageCtx, prev: u32, slot: usize, spec: &RowSpec) -> u32 {
    if slot >= fw_api::MAX_ROWS_PER_PAGE { return 0; }
    let r = fw_api::row_create(ctx.content, spec.text.as_ptr(), spec.sub, spec.trailing);
    if r == 0 { return 0; }
    write_volatile(ctx.rows.add(slot), r);
    // 注意: 标签自套不在这里隐式全量执行(28px 统一写会把文件管理/表盘管理等
    // 列表行拉成大字)。改由 InfoPage::render 显式调用, 只作用于主页行。
    if spec.trailing == fw_api::trailing::SWITCH {
        let tobj = fw_api::row_trailing(r);
        if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(slot), 0, 0); }
        else { fw_api::obj_add_event(r, row_ev_fn(slot), 7, 0); }
    } else if spec.clickable {
        fw_api::obj_add_event(r, row_ev_fn(slot), 7, 0);
        // 复选框行: 复选框自己也挂一份, 点到圈上也算点这一行(表盘页只挂行本体,
        // 点到圈上没有反应)。这一行不是 switch 行, 所以复选框发出来的 VALUE_CHANGED
        // 会被 chaos_row_dispatch 丢掉, 只有 CLICKED 会走到 on_click。
        if spec.trailing == fw_api::trailing::CHECKBOX {
            let tobj = fw_api::row_trailing(r);
            if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(slot), 0, 0); }
        }
    }
    fw_api::obj_align_to(r, prev, 14, 0, 8);
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
const TILE_ROW_W: u32 = 328;
/// 半宽按钮居中到父容器中心需要的横向补偿(左边块的中心比内容中心小 84)
const TILE_CENTER_DX: i32 = 84;

unsafe fn tile_base(ctx: &PageCtx, slot: usize, text: &[u8], w: u32, h: u32) -> u32 {
    if slot >= fw_api::MAX_ROWS_PER_PAGE || text.is_empty() { return 0; }
    let r = fw_api::row_create(ctx.content, text.as_ptr(), fw_api::NONE_PTR, 0);
    if r == 0 { return 0; }
    write_volatile(ctx.rows.add(slot), r);
    fw_api::obj_set_size_wh(r, w, h);
    cardify(r);
    fw_api::obj_set_local_style_prop(r, crate::fw_api::fwaddr::p65_style::RADIUS, h / 2, 0);
    fw_api::obj_add_event(r, row_ev_fn(slot), 7, 0);
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

static mut P_EXCL_LB: [u32; 40] = [0; 40];
static mut P_STATE_LB: [u32; 40] = [0; 40];
static mut LAST_MSG: [u8; 64] = [0; 64];
static mut LAST_HDR: [u8; 64] = [0; 64];

/// 文本是否变化; 变化时把新文本存进 cache(供下次比较)。避免每 tick 重设同一段文字。
pub(crate) unsafe fn text_changed(cache: *mut u8, txt: *const u8, cap: usize) -> bool {
    let mut i = 0usize;
    let mut diff = false;
    loop {
        let b = core::ptr::read_volatile(txt.add(i));
        let a = core::ptr::read_volatile(cache.add(i));
        if a != b { diff = true; }
        if b == 0 || i + 1 >= cap { break; }
        i += 1;
    }
    if diff {
        let mut j = 0usize;
        while j <= i && j + 1 < cap {
            core::ptr::write_volatile(cache.add(j), core::ptr::read_volatile(txt.add(j)));
            j += 1;
        }
        if j + 1 < cap { core::ptr::write_volatile(cache.add(j), 0); }
    }
    diff
}

/// 在副标题胶囊右侧追加“排除状态”胶囊: 已排除=红, 未排除=灰。
pub(crate) unsafe fn add_excl_pill(row: u32, sub_lab: u32, i: usize, skip: bool) -> u32 {
    if skip || sub_lab == 0 { return 0; }
    let p = fw_api::label_create(row);
    if p == 0 { return 0; }
    let (txt, col, bg): (&[u8], u32, u32) = if crate::appmgr::excl_on(i) {
        ("已排除\0".as_bytes(), C_DANGER, 0x4D1613)
    } else {
        ("未排除\0".as_bytes(), C_SECONDARY, 0x3A3A3E)
    };
    fw_api::label_set_text(p, txt.as_ptr());
    pill(p, col, bg, txt);
    // 与第一个胶囊垂直居中对齐
    fw_api::obj_align_to(p, sub_lab, fw_api::align::OUT_RIGHT_MID, 8, 0);
    p
}

pub(crate) unsafe fn set_excl_pill(p: u32, i: usize) {
    if p == 0 { return; }
    let (txt, col, bg): (&[u8], u32, u32) = if crate::appmgr::excl_on(i) {
        ("已排除\0".as_bytes(), C_DANGER, 0x4D1613)
    } else {
        ("未排除\0".as_bytes(), C_SECONDARY, 0x3A3A3E)
    };
    fw_api::label_set_text(p, txt.as_ptr());
    pill(p, col, bg, txt);
}

/// 把副标题标签做成彩色小胶囊(状态药丸): 角色 tint 底色 + 角色色文字。
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
    fw_api::obj_set_local_style_prop(lab, ps::RADIUS, 100, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_0, 1, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_1, 1, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_3, 0, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_2, 8, 0);
    fw_api::obj_set_local_style_prop(lab, ps::PAD_3, 8, 0);
    // TEXT_ALIGN = 94, CENTER = 2 (蓝牙模块验证过的属性号)
    fw_api::obj_set_local_style_prop(lab, 94, 2, 0);
    fw_api::obj_set_size_wh(lab, pill_w(txt), 32);
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

/// 把整行宽度(328)的控件挂到"左边半宽按钮块"下方时, 需要向右补
/// (内容中心 - 左块中心)=84, 否则会整体偏左半个按钮。
pub(crate) unsafe fn build_row_under_left(
    ctx: &PageCtx, left_tile: u32, slot: usize, spec: &RowSpec,
) -> u32 {
    let r = build_row(ctx, left_tile, slot, spec);
    if r != 0 {
        fw_api::obj_align_to(r, left_tile, fw_api::align::OUT_BOTTOM_MID,
            TILE_CENTER_DX, 8);
    }
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

// ===== Page trait =====

/// 页面行为接口。页面知道自己是谁, 不需要外部传 pid。
pub(crate) trait Page: Sync {
    /// 标题文本写入 buf(48B), 必须以 \0 结尾
    fn fill_title(&self, buf: *mut u8);
    /// 标题模式: 0=无返回键, 1=有返回键
    fn title_mode(&self) -> u32 { 1 }
    /// 建页面专属控件(框架已建好 content + title)
    fn render(&self, ctx: &PageCtx) -> PageResult;
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

// ===== 页0: 信息/主菜单 =====

// ===== 页2: 时间/日期设置(原生滚轮, 入口在关于页) =====
// 页2 原来是文件管理的第 2 级目录, 文件管理入口退役后这一格再没人进得去,
// 所以时间/日期编辑器占用它 —— 二级界面必须有独立 page_id, 只有真 page_goto/page_back
// 才有固件的压栈动画, 页内切层级没有。
// 滚轮整段逐字抄系统闹钟页 `FUN_0048C1E4`(两列分支 + 公共尾): 分隔标签、两个滚轮的
// 样式挂接、上下两条渐隐遮罩、底部那条 96x36 标签, 对象先后次序与字面值都不改。
// 取值走选中项文字 getter; 初值从 clock_gettime + localtime_r 取;
// 保存走 tick 调 time_set(写 RTC, 驱动同步系统时钟)。
pub(crate) const TIME_PID: usize = 2;
/// 日期设置(月+日)的独立 page_id。用页 3 —— 它原来是文件管理第三级，
/// 入口退役后一直不可达但保留注册，是 AGENTS 指定的首选可腾货源；注册页数不变(17)，
/// 不碰真机那条"原生应用最多 14 页"的上限。
pub(crate) const DATE_MD_PID: usize = 3;
static mut TJ_SAVE_REQ: u32 = 0;
static mut TJ_P1: u32 = 0;   // 页2 的两个轮句柄
static mut TJ_P2: u32 = 0;
static mut TJ_M1: u32 = 0;   // 页3 的两个轮句柄(与页2 分开: on_resume 只在 built==0 时重建,
static mut TJ_M2: u32 = 0;   // 从页3 返回页2 不会重跑 render, 共用槽会拿着已回收的句柄去刷)
static mut TJ_TM: [i32; 9] = [0; 9];
static mut TJ_MSG: [u8; 40] = [0; 40];
static mut TJ_HINT: u32 = 0;   // 页2 底部那条 96x36 提示标签
static mut TJ_HINT2: u32 = 0;  // 页3 的同一条标签
static mut TJ_V: [i32; 3] = [-1; 3];   // 每个轮上一次拍看到的选中值, 判"又滚过一格"用
static mut TJ_FB_UD: u32 = 0;          // 事件回调登记的轮号(1..3), 0 = 这拍没有
/// 当前蓝着的轮号(锁存), 0 = 还没人碰过、两列都是白。
/// **只由"哪一列被拨"改写, 不存在任何超时/松手把它清零的路径** —— 见 `tj_feedback`。
static mut TJ_ACTIVE: u32 = 0;

fn tj_leap(y: i32) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

fn tj_days(y: i32, m: i32) -> i32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if tj_leap(y) { 29 } else { 28 },
        _ => 31,
    }
}

unsafe fn tj_msg(s: &[u8]) {
    let m = core::ptr::addr_of_mut!(TJ_MSG) as *mut u8;
    let n = s.len().min(39);
    let mut i = 0usize;
    while i < n { write_volatile(m.add(i), s[i]); i += 1; }
    write_volatile(m.add(n), 0);
}

unsafe fn tj_fail(rc: i32) {
    let m = core::ptr::addr_of_mut!(TJ_MSG) as *mut u8;
    let pre = "失败 ".as_bytes();
    for (i, b) in pre.iter().enumerate() { write_volatile(m.add(i), *b); }
    let mut n = pre.len();
    let mut v = if rc < 0 { -rc } else { rc };
    let neg = rc < 0;
    let mut tmp = [b'0'; 10];
    let mut k = 0usize;
    loop {
        tmp[k] = b'0' + (v % 10) as u8;
        v /= 10;
        k += 1;
        if v == 0 || k == tmp.len() { break; }
    }
    if neg && n < 38 { write_volatile(m.add(n), b'-'); n += 1; }
    while k > 0 && n < 39 { k -= 1; write_volatile(m.add(n), tmp[k]); n += 1; }
    write_volatile(m.add(n), 0);
}

/// 当前前台那一屏有哪几个轮、哪条提示标签。
///
/// 时间/年份(页2)与日期(页3)是三个并列入口，同一时刻只有一页在被操作，句柄存在不同槽里，
/// 所以所有"碰控件"的动作都要先问这一句拿到的是哪一屏。判据用 `FG_PAGE`
/// (on_resume 设，页3 一旦前台就恒是 3)，不用一个全局"步骤"变量 —— 那样
/// 从页3 按返回键回到页2 时没人把它改回来，页2 会拿着页3 的轮去刷颜色。
unsafe fn tj_scr() -> ([u32; 3], u32) {
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID {
        ([st_rd!(TJ_M1), st_rd!(TJ_M2), 0], st_rd!(TJ_HINT2))
    } else {
        ([st_rd!(TJ_P1), st_rd!(TJ_P2), 0], st_rd!(TJ_HINT))
    }
}

/// 把当前前台那一屏的滚轮值写回 TJ_TM：时间屏动时/分，年份屏动年，月日屏动月/日。
unsafe fn tj_pick_values() {
    let (wheels, _) = tj_scr();
    let mut tm = core::ptr::read_volatile(core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
    let md = st_rd!(FG_PAGE) as usize == DATE_MD_PID;
    if md {
        if wheels[0] != 0 { let v = fw_api::zoom_picker_value(wheels[0]); if v >= 1 && v <= 12 { tm[4] = v - 1; } }
        if wheels[1] != 0 { let v = fw_api::zoom_picker_value(wheels[1]); if v >= 1 && v <= 31 { tm[3] = v; } }
    } else {
        // 页2 只有时分两轮(年份设置已删)
        if wheels[0] != 0 { let v = fw_api::zoom_picker_value(wheels[0]); if v >= 0 && v <= 23 { tm[2] = v; } }
        if wheels[1] != 0 { let v = fw_api::zoom_picker_value(wheels[1]); if v >= 0 && v <= 59 { tm[1] = v; } }
    }
    if tm[4] < 0 { tm[4] = 0; }
    if tm[4] > 11 { tm[4] = 11; }
    if tm[3] < 1 { tm[3] = 1; }
    let dmax = tj_days(tm[5] + 1900, tm[4] + 1);
    if tm[3] > dmax { tm[3] = dmax; }
    core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
}

/// 反馈状态归零：每建一屏都要走一遍。
/// 值缓存留 -1 表示"还没读到过"，第一次读到只登记不震 —— 建完页那一拍可视项可能还没挂齐。
unsafe fn tj_reset_feedback() {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_V), [-1; 3]);
    st_wr!(TJ_ACTIVE, 0);
    st_wr!(TJ_FB_UD, 0);
    st_wr!(TJ_SAVE_REQ, 0);
}

unsafe fn tj_save_now() {
    tj_pick_values();
    let tm = core::ptr::read_volatile(core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
    let y = tm[5] + 1900;
    let mo = tm[4] + 1;
    let rc = crate::time_set::set(
        y as u32, mo as u32, tm[3] as u32, tm[2] as u32, tm[1] as u32, 0);
    if rc != 0 {
        tj_fail(rc);
    } else {
        // 立刻回读核对: ioctl 说成功但系统时钟没跟上的话, 这里会报出来
        let ok = match crate::time_set::now_tm() {
            Some(g) => g[5] + 1900 == y && g[4] + 1 == mo && g[3] == tm[3]
                && g[2] == tm[2] && g[1] == tm[1],
            None => false,
        };
        tj_msg(if ok { "已存".as_bytes() } else { "失败".as_bytes() });
    }
    crate::render_req(st_rd!(FG_PAGE) as usize);
}

/// 设置页那两行入口：各走自己的压栈跳转。
///
/// `target` 是这次要进的页号：时间 → 页2(时分)、日期 → 页3(月日)。两页平级，
/// 都直接从设置页进来。年份设置已按用户要求删除，不再有"先年再月日"那一步。
unsafe fn time_edit_enter(target: u32) {
    st_wr!(TJ_P1, 0); st_wr!(TJ_P2, 0);
    st_wr!(TJ_M1, 0); st_wr!(TJ_M2, 0);
    st_wr!(TJ_HINT, 0); st_wr!(TJ_HINT2, 0);
    tj_reset_feedback();
    match crate::time_set::now_tm() {
        Some(tm) => {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
            // 提示标签初始就是一个空格(闹钟页 24 小时制下填的正是空格), 拨哪个轮才写哪个字段名
            tj_msg(" ".as_bytes());
        }
        None => {
            let mut tm = [0i32; 9];
            tm[3] = 1; tm[4] = 0; tm[5] = 126; // 2026-01-01, 年 2000..2099 内
            core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
            tj_msg(" ".as_bytes());
        }
    }
    crate::nav_goto(target, 0);
}

/// 数字之间的分隔标签：逐字抄闹钟页 `FUN_0048C1E4` 冒号那一段。
///
/// 建标签 -> 设文本 -> `align(7, dx, 0)` -> 从共享样式 `0x2010D2E8` 取 face ->
/// 写进这个对象 -> 最后一步 `(obj, 0xff, 0)`。face 取自系统开机样式表，换过系统
/// 字体的设备上取到的就是那张脸，所以这里不写死任何字体名。取回 0 就只跳过写
/// 字体那一步，其余照抄。
unsafe fn tj_sep(parent: u32, text: *const u8, dx: i32) -> u32 {
    let lb = fw_api::label_create(parent);
    if lb == 0 { return 0; }
    fw_api::label_set_text(lb, text);
    fw_api::obj_align(lb, fw_api::align::X_ANCHOR, dx, 0);
    let face = fw_api::style_get_font(fw_api::COLON_FACE_STYLE);
    if face != 0 { fw_api::obj_set_style_text_font(lb, face, 0); }
    fw_api::obj_set_style_4c8da0(lb, 0xff, 0);
    lb
}

/// 建一个滚轮：逐字抄 `FUN_0048C1E4` 里两轮共用的那一段，只有宽度按列数给。
///
/// 建 -> 挂样式 `0x2010D300` -> `(p, 2, 0)` -> 设宽度 -> 3 行可见 -> 范围 -> 初值 ->
/// 循环模式 1 -> `align(7, dx, 0)`。
/// `val` 是**标签值**（年 2026、月 10、日 8），减 min 换算成控件要的偏移在函数里做。
/// 不设高度：闹钟页没设，高度由那条样式里的字体行高算出来。之前在这里硬写
/// 86x226，226 取的是闹钟页 AM/PM 那一支的值，两列分支用不上，结果是控件按
/// 我们给的高度排版、字体按默认主题算，选中行放大后就挤成一串四位数。
unsafe fn tj_picker(parent: u32, min: i32, max: i32, val: i32, w: i32, dx: i32) -> u32 {
    let p = fw_api::zoom_picker_create(parent);
    if p == 0 { return 0; }
    fw_api::style_apply(p, fw_api::PICKER_STYLE, 0xff, 0);
    fw_api::obj_set_style_4c8248(p, 2, 0);
    fw_api::obj_set_width(p, w);
    fw_api::zoom_picker_set_visible(p, 3);
    fw_api::zoom_picker_set_range(p, min, max, 1);
    // **这里必须减 min**：`set_value` 收的是范围内的**偏移**(0..项数-1)，不是标签值。
    // 时/分从 0 起，两种口径看不出差别；年(2000 起)、月/日(1 起)一减就错位 ——
    // 把 2026 传进去等于偏移 2026，而年轮只有 100 项，控件内部索引越界之后的表现就是
    // "乱跳、最后存下来的不是拨的那个数"（2026-10-18 真机，文档 §1 早已定案过这一条）。
    // 读值走 `zoom_picker_value`(读选中项文字)拿到的是**标签值**，所以写偏移、读标签，
    // 两边本来就不对称 —— 不要为了"对称"把这里改回标签值。
    let off = if val < min { 0 } else if val > max { max - min } else { val - min };
    fw_api::zoom_picker_set_value(p, off);
    fw_api::zoom_picker_set_mode(p, 1);
    fw_api::obj_align(p, fw_api::align::X_ANCHOR, dx, 0);
    p
}

/// 滚轮上下那两条渐隐遮罩：逐字抄 `FUN_0048C1E4` 尾部。
///
/// 建图 -> 设资源路径 -> 加 `0x40000` 那个标志 -> 304x32 -> 上条 `align(2, 0, 104)`、
/// 下条 `align(5, 0, -105)`。它们在滚轮之后创建，所以压在两轮上面，把上下两行淡出。
unsafe fn tj_mask(parent: u32, path: *const u8, top: bool) {
    let im = fw_api::background_image_create(parent);
    if im == 0 { return; }
    fw_api::background_image_set_source(im, path as u32);
    fw_api::background_image_set_floating(im);
    fw_api::obj_set_size(im, 0x130, 0x20);
    if top {
        fw_api::obj_align(im, fw_api::align::TOP_MID, 0, 0x68);
    } else {
        fw_api::obj_align(im, fw_api::align::BOTTOM_MID, 0, -0x69);
    }
}

/// 底部那条 96x36 标签：逐字抄 `FUN_0048C1E4` 尾部。
///
/// 建 -> 96x36 -> 挂系统默认样式 -> `(l, 2, 0)` -> 设文本 -> `align(2, 0, 75)` ->
/// 文字色 `0x277af7`。闹钟页在 24 小时制下给它填一个空格（那位置是 12 小时制放
/// AM/PM 的），我们把保存结果显示在同一条标签上，不再另加标签，保持对象数一致。
/// 句柄交给调用方存进自己那一屏的槽(页2 存 `TJ_HINT`、页3 存 `TJ_HINT2`)。
unsafe fn tj_hint(parent: u32, text: *const u8) -> u32 {
    let lb = fw_api::label_create(parent);
    if lb == 0 { return 0; }
    fw_api::obj_set_size(lb, 0x60, 0x24);
    fw_api::label_apply_defaults(lb, 0);
    fw_api::obj_set_style_4c8830(lb, 2, 0);
    fw_api::label_set_text(lb, text);
    fw_api::obj_align(lb, fw_api::align::TOP_MID, 0, 0x4b);
    fw_api::obj_style_color_a70(lb, 0x277af7, 0);
    lb
}

/// 把滚轮接进事件回调：与闹钟页宿主同一形状 —— 过滤器 0(收全部事件)，
/// user_data 放轮号(1/2/3)，回调体里再读回来区分是哪个轮被拨。
/// 注册一律放在设完初值之后，这样建页时的 set_value 不会触发反馈。
unsafe fn tj_bind_wheel(p: u32, no: u32) {
    if p == 0 { return; }
    let f: u32 = {
        let g: unsafe extern "C" fn(u32) -> u32 = chaos_tj_wheel_ev;
        g as u32
    };
    fw_api::obj_add_event(p, f, fw_api::ev::ALL, no);
}

/// 把滚轮的染色与底部标签写成"哪个轮在被拨"。
///
/// 三行动作(被拨的轮 `0x277af7`、其余 `0xffffff`、**每个轮**都重挂一遍文字)与
/// 闹钟页回调体 `FUN_004985E4` 一致；差别只在**调用它的上下文**：
/// 闹钟在自己的应用进程里、在事件派发链上直接做，我们这是内核模块挂在 LVGL 派发上，
/// 派发链里做这些实测会把系统打崩，所以放到拍上(lv_timer 上下文)做。
/// 只刷 `tj_scr()` 给的那一屏 —— 另一屏的轮这时可能已经不在对象树上了。
unsafe fn tj_paint(act: u32) {
    let (wheels, hint) = tj_scr();
    let mut i = 0usize;
    while i < 3 {
        let p = wheels[i];
        if p != 0 {
            fw_api::obj_style_color_a70(p, if act == (i as u32) + 1 {
                fw_api::WHEEL_COLOR_ACTIVE
            } else {
                fw_api::WHEEL_COLOR_IDLE
            }, 0);
            fw_api::zoom_picker_refresh(p);
        }
        i += 1;
    }
    if act != 0 && hint != 0 {
        let txt = tj_field_name(act - 1);
        if !txt.is_null() { fw_api::label_set_text(hint, txt); }
    }
}

/// 滚轮的反馈：在**拍上下文**里做，不在事件派发链里做。
///
/// 只有两条线：
/// - **蓝 = 最后被拨过的那一列**。系统闹钟页 `FUN_004985E4` 逐字读过：只认 7/9 两个码，
///   把被拨那一轮染 `0x277af7`、其余染 `0xffffff`，**整个函数没有任何回白分支**。
///   系统页的蓝就是"最后碰过的那一列留蓝"，直到拨另一列才换 —— 松手、停住、惯性滚完
///   都不变白。所以这里也不做回白：`TJ_ACTIVE` 只被"哪一列被拨"改写。
/// - **震 = 这一拍取值真的变了一格**，只有正在操作的那一个轮会被读。
///
/// 上一版那套"回白"已经删掉，它是蓝白闪烁的根因（真机两轮回报）：上一版每拍拿控件
/// 状态字 `obj+0x30` 与建屏基线比，想用它判"手指还在不在"。但那个字不是"按着"的稳定
/// 指示 —— 滚动过程中 LVGL 自己会动它，于是判据在 50ms 的拍之间反复翻面，
/// `TJ_ACTIVE` 被清零又立刻被下一发交互码拉回，屏幕就白一下蓝一下地闪。
/// 而"回白"这个需求本身在系统页里并不存在（见上），1:1 的做法就是不实现它。
unsafe fn tj_feedback() {
    let (wheels, _) = tj_scr();
    let cache = core::ptr::addr_of_mut!(TJ_V) as *mut i32;
    let touched = st_rd!(TJ_FB_UD) as usize;
    st_wr!(TJ_FB_UD, 0);
    // 这一拍没有交互事件：什么都不做。颜色保持不变，一个控件都不读。
    if touched == 0 { return; }
    if touched != st_rd!(TJ_ACTIVE) as usize {
        st_wr!(TJ_ACTIVE, touched as u32);
        let p = wheels[touched - 1];
        if p != 0 {
            let v = fw_api::zoom_picker_value(p);
            if v >= 0 { write_volatile(cache.add(touched - 1), v); }
        }
        tj_paint(touched as u32);
        return;
    }
    let p = wheels[touched - 1];
    if p == 0 { return; }
    let v = fw_api::zoom_picker_value(p);
    if v < 0 { return; }          // 这一拍没读到有效文字(正在重挂可视项): 不震
    let prev = read_volatile(cache.add(touched - 1));
    if prev < 0 {
        write_volatile(cache.add(touched - 1), v);   // 只补基准, 不震
        return;
    }
    if v != prev {
        write_volatile(cache.add(touched - 1), v);
        // 震动只在"这个轮真的又滚过一格"这一发：滚三格震三次，停住不滚就不震。
        // 固件那一拍由闹钟 app 把自己那个私有 ctx 注册进控制器之后由控制器发
        // (`(*(控制器 vtable +0x34/+0x38))(*0x20112520 的宿主, 轮)`)，内核模块没有等价 ctx，
        // 函数本体又在 PSRAM 侧，静态读不到，所以这里用平台自己的触觉入口做等效。
        // 该入口内部先读系统震动开关(0x2011942E)，用户关掉震动时自然不响。
        fw_api::vibrate(0x0d);
    }
}

/// 滚轮事件回调：只登记，一个固件调用都不做。
///
/// 只登记交互族 `{1 按下, 2 按住, 7 点击, 9 滚动开始, 11 滚动中, 12, 26, 30 取值变了}`
/// → `TJ_FB_UD`(轮号)，**其余事件码一概不记**（DRAW 系那一堆每帧都来，混进来就等于一直重绘）。
///
/// 抬手码 `{3 PRESS_LOST, 8 RELEASED}` 现在**不登记**：颜色不再有"松手收白"这一步
/// （系统闹钟页也没有，见 `tj_feedback`），抬手码没有任何消费者。
/// `SCROLL_END(10)` 同理两不沾 —— 甩动的惯性滚完就发一次，而手指往往还按在屏上；
/// 按住期间反复发的 `LONG_PRESSED(5)`/`LONG_PRESSED_REPEAT(6)` 也不是抬手。
/// 染色、重挂文字、换标签、震动一律由 `tj_feedback` 在拍(lv_timer)上下文里做。
#[no_mangle]
pub unsafe extern "C" fn chaos_tj_wheel_ev(event: u32) -> u32 {
    let code = fw_api::event_get_code(event);
    let ud = fw_api::event_user_data(event);
    if ud < 1 || ud > 3 { return 0; }
    if code == 1 || code == 2 || code == 7 || code == 9
        || code == 11 || code == 12 || code == 26 || code == 30 {
        st_wr!(TJ_FB_UD, ud);
    }
    0
}

/// 拨动那个轮时的字段名：时间屏用固件自己的消息号取当前语言的词(小时/分钟)，
/// 日期两屏没有对应的消息号，只能用字面的"年/月/日"。
/// 哪一屏看 `FG_PAGE`：页3 是月/日，页2 是时/分。
unsafe fn tj_field_name(idx: u32) -> *const u8 {
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID {
        if idx == 0 { "月\0".as_ptr() } else { "日\0".as_ptr() }
    } else {
        fw_api::translate(if idx == 0 { fw_api::HINT_KEY_HOUR } else { fw_api::HINT_KEY_MIN })
    }
}

/// 这一屏的基准盒：content 铺满整页、顶对齐 0、不留白。
///
/// 框架给每页固定的是 `336x424` 摆在 `y=56`(标题栏固有高 `0x38=56`)再加底部留白 32 ——
/// 那是列表页的口径。闹钟页那一串字面偏移(滚轮 `dy=0`、遮罩 `+104/-105`、提示标签 `+75`、
/// 确认按钮 `-16`)是按"content 铺满整页"算的: 滚轮在 `dy=0` 时正好落在整页中心 = 屏幕中心，
/// 按钮底边离屏幕底 16。照抄偏移而不照抄基准，整组就会比系统低 28px、按钮也跟着抬 16。
/// LVGL 的这些偏移都是相对"父对象内边距之后的可用区"量的，所以可用区错了整组跟着错。
unsafe fn tj_base_box(content: u32) {
    fw_api::obj_set_size(content, 336, 480);
    fw_api::obj_align(content, fw_api::align::TOP_MID, 0, 0);
    fw_api::content_pad_bottom(content, 0, 0);
}

/// 建完轮之后的公共尾部：遮罩、提示标签、反馈回调挂接、确认按钮。
///
/// 次序与闹钟页一致: 轮 -> 遮罩(压在轮上做渐隐) -> 底部标签 -> 挂反馈回调 -> 确认按钮。
/// 反馈回调必须挂在设完初值之后，否则建页时那发 `set_value` 会当成"用户拨了一格"。
/// `nh` 是这一屏的轮数(1 或 2)，取 `tj_scr()` 那一屏的槽。
unsafe fn tj_screen_tail(ctx: &PageCtx, nh: usize) -> PageResult {
    // 闹钟页宿主在建完两轮之后，对**第一个轮**清一次焦点态
    // (`0x0048C328`/`0x0048C524` 的 `(轮, 2)` = `clear_state(轮, FOCUSED)`)：
    // 它把轮注册进控制器那张组，组会把第一个成员自动置焦，所以建完要把它清掉，
    // 开局才有"没有一个轮是蓝的"。我们没有那条注册链，这一发按理是空操作，
    // 照抄是为了建轮之后的状态与闹钟页不留差别。
    let (wheels, _) = tj_scr();
    if wheels[0] != 0 {
        fw_api::obj_clear_state(wheels[0], fw_api::LV_STATE_FOCUSED);
    }
    // 基准值不在这里读: 建页时控件的可视项标签还没挂齐, 读回来是 -1,
    // 于是第一拍就把 -1 当"变了一格" —— 凭空震一下并染色(实测过), 交给第一拍去填。
    // 颜色也不在这里定: 开局两列都是白(`TJ_ACTIVE = 0`), 第一次拨哪一列才上蓝。
    tj_reset_feedback();
    tj_mask(ctx.content,
            b"/resource/app/settings/icon/time_top2.bin\0".as_ptr(), true);
    tj_mask(ctx.content,
            b"/resource/app/settings/icon/time_bottom2.bin\0".as_ptr(), false);
    let hint = tj_hint(ctx.content, core::ptr::addr_of!(TJ_MSG) as *const u8);
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID { st_wr!(TJ_HINT2, hint); }
    else { st_wr!(TJ_HINT, hint); }
    let mut no = 1usize;
    while no <= nh {
        tj_bind_wheel(wheels[no - 1], no as u32);
        no += 1;
    }
    // 确认按钮抄宿主 `FUN_0049885E`: 工厂 -> 160x96 -> `align(5, 0, -16)` ->
    // confirm 图标 -> 底色 `0x222222` -> 挂 CLICKED。
    let btn = fw_api::confirm_btn_create(ctx.content);
    if btn == 0 { return Err(PageError::RowFailed(0)); }
    fw_api::obj_set_size(btn, 0xa0, 0x60);
    fw_api::obj_align(btn, fw_api::align::BOTTOM_MID, 0, -16);
    fw_api::btn_set_src(btn, b"/resource/app/common/icon/confirm.bin\0".as_ptr());
    fw_api::obj_style_color(btn, 0x222222, 0);
    fw_api::obj_add_event(btn, row_ev_fn(0), fw_api::ev::CLICKED, 0);
    Ok(())
}

/// 确认按钮的派发：三屏都是当场写入系统时钟。
///
/// 这里只做"登记"，真正的写 RTC 在拍上做 —— 那条链 open/read 设备节点，
/// 放在 UI 线程的事件回调里会阻塞到看门狗复位(早就定过的规矩)。
/// 三个入口各自只改自己那一组字段，其余字段取自进页时读到的 `TJ_TM`，
/// 所以"只改年份"不会顺手把月日改掉。
unsafe fn tj_confirm() {
    st_wr!(TJ_SAVE_REQ, 1);
}

/// 圆环中心文本: 只写百分比数字, 如 "45%"
unsafe fn ring_fill(i: usize) {
    let dst = (core::ptr::addr_of_mut!(RING_TXT) as *mut u8).add(i * 48);
    let pct = if i == 0 { storage_pct() } else { mem_pct() };
    if pct >= 100 {
        write_volatile(dst.add(0), b'1');
        write_volatile(dst.add(1), b'0');
        write_volatile(dst.add(2), b'0');
        write_volatile(dst.add(3), b'%');
        write_volatile(dst.add(4), 0);
    } else if pct >= 10 {
        write_volatile(dst.add(0), b'0' + (pct / 10) as u8);
        write_volatile(dst.add(1), b'0' + (pct % 10) as u8);
        write_volatile(dst.add(2), b'%');
        write_volatile(dst.add(3), 0);
    } else {
        write_volatile(dst.add(0), b'0' + pct as u8);
        write_volatile(dst.add(1), b'%');
        write_volatile(dst.add(2), 0);
    }
}

pub(crate) struct InfoPage;

/// 读缓存里的占用百分比(0=存储 1=内存)。render 建条时用, 不碰 procfs。
unsafe fn stat_pct(i: usize) -> u32 {
    read_volatile((core::ptr::addr_of!(STAT_PCT) as *const u32).add(i))
}

impl Page for InfoPage {
    fn fill_title(&self, buf: *mut u8) {
        // 标题栏文本 = 应用名 "Chaos"。
        // 缓冲 48B, 5 字节写入无越界
        unsafe { put_str(buf, "Chaos\0".as_bytes()); }
    }
    fn title_mode(&self) -> u32 { 0 }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            DBG_ON = 0; DBG_CLICKS = 0;
            // 展示顺序: CPU(标准行) -> 存储 -> 内存 -> 导航。
            // 句柄槽位不变(数据行仍 0/1, CPU 仍 2), 只改悬挂位置。
            // 进页面不读 procfs: 原先这一屏要 5 次 open/read/close(存储与内存各读两遍 ——
            // stat_fill 一次、storage_pct/mem_pct 又一次), 是"每次进入主页都顿一下"的主要来源。
            // 现在用上一轮的缓存建行: 文本在 STAT_VAL、百分比在 STAT_PCT; 建完把 STAT_TICK
            // 置 19, 下一拍 tick 立刻补一次真值(50ms 后数字就是新的), 进页这一下不碰文件系统。
            let nm2 = stat_name(2);
            let val2 = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
            let cpu = build_row(ctx, ctx.title, 2, &RowSpec::display(
                core::slice::from_raw_parts(nm2, cstr_len(nm2)), val2));
            let anchor = if cpu != 0 { cpu } else { ctx.title };
            // 数据行组件(存储=小米蓝, 内存=绿), 封装在 fw_api::data_row_create 等
            let d0 = fw_api::data_row_create(ctx.content, stat_name(0),
                fw_api::BAR_FILL_RGB, stat_pct(0));
            fw_api::data_row_place(&d0, anchor);
            if d0.value != 0 {
                fw_api::label_set_text(d0.value, core::ptr::addr_of!(STAT_VAL) as *const u8);
            }
            let d1 = fw_api::data_row_create(ctx.content, stat_name(1),
                fw_api::BAR_FILL_GREEN, stat_pct(1));
            fw_api::data_row_place(&d1, d0.row);
            if d1.value != 0 {
                fw_api::label_set_text(d1.value,
                    (core::ptr::addr_of!(STAT_VAL) as *const u8).add(STAT_STRIDE));
            }
            // 句柄登记(tick 用): [行, 数值标签, 条] x2
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            write_volatile(hp.add(0), d0.row);
            write_volatile(hp.add(1), d0.value);
            write_volatile(hp.add(2), d0.bar);
            write_volatile(hp.add(3), d1.row);
            write_volatile(hp.add(4), d1.value);
            write_volatile(hp.add(5), d1.bar);
            // 导航入口
            build_rows(ctx, d1.row, &[
                (3, RowSpec::clickable(b"\xe5\xba\x94\xe7\x94\xa8\xe7\xae\xa1\xe7\x90\x86\0")), // 应用管理(页1, Shell++ 移植)
                (4, RowSpec::clickable(b"\xe8\xa1\xa8\xe7\x9b\x98\xe7\xae\xa1\xe7\x90\x86\0")), // 表盘管理(页10)
                (5, RowSpec::clickable(b"\xe7\xb3\xbb\xe7\xbb\x9f\xe7\xbe\x8e\xe5\x8c\x96\0")), // 系统美化(页13: 字体/图标/模糊)
                (6, RowSpec::clickable("设置\0".as_bytes())),
                (7, RowSpec::clickable("关于\0".as_bytes())),   // 时间/日期/亮度/震动/缓存/版本(页14)
            ])?;
            // 主页行的字体不再在这里特殊处理: 逐对象补写(font_tree)会按每个标签
            // 自己的行盒选脸, 覆盖所有页面。这里再无条件写 28px 基准 face 反而
            // 会和它打架(实测: 主页导航行 28px 观感正常, 但同一规则套到列表行就偏大)。
            // 让下一拍 tick 立即刷(周期是 20 拍, 置 19 后 +1 正好命中), 见上面那段注释
            st_wr!(STAT_TICK, 19);
            let _ = cpu;
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                // 3 = 应用管理(页1, Shell++ 移植, 见 appmgr.rs)。
                // (2026-10-15) 原文件管理入口已注释退役:
                //   3 => {
                //       let rp = core::ptr::addr_of_mut!(PATH_STACK) as *mut u8;
                //       write_volatile(rp.add(0), b'/');
                //       write_volatile(rp.add(1), 0u8);
                //       write_volatile(core::ptr::addr_of_mut!(DIR_WIN_SAVE).cast::<u32>(), 0);
                //       nav_goto(1, 0);
                //   }
                3 => nav_goto(1, 0),   // 应用管理
                4 => nav_goto(10, 0),  // 表盘管理
                5 => nav_goto(13, 0),  // 系统美化
                6 => nav_goto(14, 0),  // 设置(时间/日期/亮度/震动/缓存/版本)
                7 => nav_goto(18, 0), // 关于(页18, 调试入口)
                _ => {} // 行0-2 是数据行
            }
        }
    }

    fn needs_tick(&self) -> bool { true }

    fn tick(&self) {
        unsafe {
            // 每 20 拍(亮屏 1s)刷新三行实时数据。原先 10 拍(0.5s)一轮要读 5 次 procfs,
            // 现在一轮 3 次、频率减半 —— UI 线程上的文件系统开销降到约 1/3。
            // 存储/内存这类占用率 1 秒一次足够; 跟手靠的是滑动本身, 不是这三个数。
            let tk = st_rd!(STAT_TICK).wrapping_add(1);
            st_wr!(STAT_TICK, tk);
            if tk % 20 != 0 { return; }
            // 息屏 / AOD 一律不刷(续航): 这三行数值只有亮屏时才有人看,
            // 而每 0.5 秒一次的刷新里含三次 /proc 读 + 若干 label 重绘。少了这道门,
            // 息屏期间自愈重建还会把本页拉起来 => 整段息屏时间都在 2Hz 读 /proc 并重画。
            if !fw_api::screen_is_on() { return; }
            if PAGES[0].built == 0 || !page_is_live(0) { return; }
            // 值真变了才写: label_set_text / bar_set_pct 每次都要失效重绘, 而存储、内存
            // 这类值绝大多数拍根本没变。STAT_PREV/BAR_PREV 只在本页 tick 里用。
            static mut STAT_PREV: [[u8; STAT_STRIDE]; 3] = [[0; STAT_STRIDE]; 3];
            static mut BAR_PREV: [i32; 2] = [-1; 2];
            // 返回"文本与上一份不同"(并把新值抄进 prev)。0 终止符之外的字节全比。
            unsafe fn stat_changed(row: usize) -> bool {
                let cur = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(row * STAT_STRIDE);
                let prev = core::ptr::addr_of_mut!(STAT_PREV[row]) as *mut u8;
                let mut i = 0usize;
                let mut diff = false;
                while i < STAT_STRIDE {
                    let c = read_volatile(cur.add(i));
                    if read_volatile(prev.add(i)) != c { diff = true; }
                    write_volatile(prev.add(i), c);
                    if c == 0 { break; }
                    i += 1;
                }
                diff
            }
            let rows = page_rows_ptr(0);
            let upd: unsafe extern "C" fn(u32, *const u8, *const u8, *const u8, i32, u8) -> i32 =
                fw_api::row_update;
            // 行2(CPU): 数值只写值标签, 不走 row_update。row_update 每次都会把
            // "行级联当前解析到的字体"重烘进行内值标签的 local style
            // (0x0C4C89D6 读属性 0x5A -> 0x0C4C8A34 `bl 0xc588e78`), 而本行每秒刷一次
            // => 烘进去的文楷被盖回行级联字体, 逐对象补写 10s 才追回一次。这正是实测
            // "只有 CPU 这类周期刷新的行闪变, 标题与数据行不闪"的根因。
            // 值标签句柄每次从行现取(行的副标签在 +0x40, 已实测核对), 不缓存。
            let r2 = read_volatile(rows.add(2));
            if r2 != 0 {
                stat_fill_pct(2);
                let ch2 = stat_changed(2);
                let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(2 * STAT_STRIDE);
                let vl = read_volatile((r2 + ROW_SUB_LB) as *const u32);
                if vl != 0 {
                    if ch2 { fw_api::label_set_text(vl, val); }
                } else {
                    // 值标签还没建出来: 走一次 row_update, 它会顺手把标签建好
                    upd(r2, core::ptr::null(), stat_name(2), val, 0, 0);
                }
            }
            // 行0/行1(数据行): 数值标签与条
            let hp = core::ptr::addr_of_mut!(DR_HANDLES) as *mut u32;
            let bp = core::ptr::addr_of_mut!(BAR_PREV) as *mut i32;
            // 每个文件只读一次: stat_fill_pct 同时产出文本(写进 STAT_VAL)与百分比,
            // 百分比顺手存进 STAT_PCT —— 下次 render 建条就不用再读文件了。
            let pct_pp = core::ptr::addr_of_mut!(STAT_PCT) as *mut u32;
            for i in 0..2usize {
                let p = stat_fill_pct(i);
                write_volatile(pct_pp.add(i), p);
                let ch = stat_changed(i);
                let vl = read_volatile(hp.add(i * 3 + 1));
                if vl != 0 && ch {
                    let val = (core::ptr::addr_of!(STAT_VAL) as *const u8).add(i * STAT_STRIDE);
                    fw_api::label_set_text(vl, val);
                }
                let bar = read_volatile(hp.add(i * 3 + 2));
                let pv = p as i32;
                if bar != 0 && read_volatile(bp.add(i)) != pv {
                    write_volatile(bp.add(i), pv);
                    fw_api::bar_set_pct(bar, p);
                }
            }

        }
    }
}


/// 算 C 字符串长度(不含 \0)
unsafe fn cstr_len(s: *const u8) -> usize {
    let mut n = 0usize;
    while read_volatile(s.add(n)) != 0 { n += 1; }
    n
}

// ===== 页7: 排除项管理 =====
//
// 从页1 行3 进入。列表显示**全部**应用(含已隐藏), 勾选 = 加入排除名单;
// 排除项不参与页1 的一键隐藏/显示, 且在页1 列表里不出现。
// 行0=返回 行1=说明 行2=状态; 条目事件槽 = 条目下标 + 12(与页1 错开 ctx.rows)。

// (旧版 refresh_appmgr_rows 已由 refresh_list_rows 取代)

static mut EX_SUB: [u8; 64] = [0; 64];

unsafe fn ex_sub_cp(dst: *mut u8, mut o: usize, s: &[u8]) -> usize {
    for &c in s { core::ptr::write_volatile(dst.add(o), c); o += 1; }
    o
}

/// 排除项行的副标题: "已排除·显示中" / "未排除·已隐藏" (7 个字符)
unsafe fn excl_sub(i: usize) -> *const u8 {
    let d = core::ptr::addr_of_mut!(EX_SUB) as *mut u8;
    let mut o = 0usize;
    o = ex_sub_cp(d, o, if crate::appmgr::excl_on(i) { "已排除".as_bytes() } else { "未排除".as_bytes() });
    o = ex_sub_cp(d, o, "·".as_bytes());
    o = ex_sub_cp(d, o, if crate::appmgr::visible(i) { "显示中".as_bytes() } else { "已隐藏".as_bytes() });
    core::ptr::write_volatile(d.add(o), 0);
    d as *const u8
}

unsafe fn refresh_excl_rows() {
    let rows = crate::appmgr::rows_ptr();
    let mode = crate::appmgr::excl_mode();
    let n = crate::appmgr::count();
    let mut i = 0usize;
    while i < n {
        let r = read_volatile(rows.add(i));
        if r != 0 {
            fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR,
                excl_sub(i), 0, 0);
            let lab = crate::mem::rd32((r + ROW_SUB_LB) as *const u32);
            if lab != 0 {
                if crate::appmgr::visible(i) { pill(lab, C_VISIBLE, C_VISIBLE_TINT, "显示中\0".as_bytes()); }
                else { pill(lab, C_HIDDEN, C_HIDDEN_TINT, "已隐藏\0".as_bytes()); }
            }
        }
        i += 1;
    }
    if EX_HINT_ROW != 0 {
        fw_api::row_update(EX_HINT_ROW, fw_api::NONE_PTR,
            crate::appmgr::excl_hint().as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
    if EX_MSG_ROW != 0 {
        let m = if mode == 0 { crate::appmgr::excl_msg() } else { crate::appmgr::msg() };
        fw_api::row_update(EX_MSG_ROW, fw_api::NONE_PTR,
            m.as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
}

pub(crate) struct ExclPage;

static mut AM_MSG_ROW: u32 = 0;
static mut AM_HDR_ROW: u32 = 0;
static mut EX_HINT_ROW: u32 = 0;
static mut EX_MSG_ROW: u32 = 0;

impl Page for ExclPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "排除项管理\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::ensure();
            crate::appmgr::rows_reset();
            let s1 = build_row(ctx, ctx.title, 0,
                &RowSpec::display(crate::appmgr::excl_hint(), core::ptr::null()));
            if s1 == 0 { return Err(PageError::RowFailed(0)); }
            row_shrink(s1, 56);
            EX_HINT_ROW = read_volatile(ctx.rows.add(0));
            EX_MSG_ROW = 0;
            let mut prev = s1;
            if prev == 0 { prev = ctx.title; }
            let rows = crate::appmgr::rows_ptr();
            // 排除项在前, 稳定排序; 单模式: 列出全部快应用
            crate::appmgr::build_excl_view();
            let n = crate::appmgr::excl_view_count();
            let mut v = 0usize;
            while v < n {
                let i = crate::appmgr::excl_view_at(v);
                v += 1;
                if i == usize::MAX { break; }
                let sub = excl_sub(i);
                let r = fw_api::row_create(ctx.content,
                    crate::appmgr::entry_primary(i), sub, 0);
                if r == 0 { return Err(PageError::RowFailed(i)); }
                write_volatile(rows.add(i), r);
                fw_api::obj_add_event(r, row_ev_fn(i + 12), 7, 0);
                fw_api::obj_align_to(r, prev, 14, 0, 10);
                prev = r;
                fw_api::obj_set_size_wh(r, 320, 100);
                cardify(r);
                fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR, sub, 0, 0);
                let lab = crate::mem::rd32((r + ROW_SUB_LB) as *const u32);
                if lab != 0 {
                    if crate::appmgr::visible(i) { pill(lab, C_VISIBLE, C_VISIBLE_TINT, "显示中\0".as_bytes()); }
                    else { pill(lab, C_HIDDEN, C_HIDDEN_TINT, "已隐藏\0".as_bytes()); }
                }
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx >= 12 {
                let i = idx - 12;
                crate::appmgr::detail_open_ctx(i, 1);
                nav_goto(29, 0);
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { crate::appmgr::excl_has_req() || crate::appmgr::has_req() }
    }

    fn tick(&self) {
        unsafe {
            let mut dirty = false;
            if crate::appmgr::excl_has_req() {
                crate::appmgr::do_excl();
                dirty = true;
            }
            if crate::appmgr::has_req() {
                crate::appmgr::do_op();
                dirty = true;
            }
            if dirty { refresh_excl_rows(); }
        }
    }
}


// ===== 页17: 运行日志 =====
// 直接显示一段文字，不再逐行建列表。只读尾部 2KB，截到最后一个换行后的 512B。
pub(crate) struct LogPage;
static PAGE_LOG: LogPage = LogPage;

const LOG_VIEW_CAP: usize = 48 * 1024;
const LOG_PAGE_LINES: usize = 8;
static mut LOG_VIEW: [u8; LOG_VIEW_CAP] = [0; LOG_VIEW_CAP];
static mut LOG_TEXT: [u8; 4096] = [0; 4096];
static mut LOG_LINE_OFF: [u32; 1024] = [0; 1024];
static mut LOG_LINES: usize = 0;
static mut LOG_PAGE: usize = 0;
static mut LOG_MSG: [u8; 48] = [0; 48];
static mut LOG_CLEAR_ARMED: u32 = 0;

unsafe fn log_page_load() {
    let buf = core::ptr::addr_of_mut!(LOG_VIEW) as *mut u8;
    let n = crate::log::tail(buf, LOG_VIEW_CAP - 1);
    if n + 1 < LOG_VIEW_CAP { write_volatile(buf.add(n), 0); }

    let offs = core::ptr::addr_of_mut!(LOG_LINE_OFF) as *mut u32;
    let mut count = 0usize;
    let mut i = 0usize;
    while i < n && count < 1024 {
        write_volatile(offs.add(count), i as u32);
        count += 1;
        while i < n && read_volatile(buf.add(i)) != b'\n' { i += 1; }
        if i < n { i += 1; }
    }
    LOG_LINES = count;
    let maxp = if count == 0 { 0 } else { (count - 1) / LOG_PAGE_LINES };
    if LOG_PAGE > maxp { LOG_PAGE = maxp; }

    let out = core::ptr::addr_of_mut!(LOG_TEXT) as *mut u8;
    let mut o = 0usize;
    let start_line = if count > (LOG_PAGE + 1) * LOG_PAGE_LINES {
        count - (LOG_PAGE + 1) * LOG_PAGE_LINES
    } else { 0 };
    let end_line = if count > LOG_PAGE * LOG_PAGE_LINES {
        count - LOG_PAGE * LOG_PAGE_LINES
    } else { 0 };
    let mut li = start_line;
    while li < end_line {
        let s = read_volatile(offs.add(li)) as usize;
        let e = if li + 1 < count { read_volatile(offs.add(li + 1)) as usize } else { n };
        let mut j = s;
        while j < e && o + 1 < 4096 {
            let mut c = read_volatile(buf.add(j));
            if c == b'\r' { c = b' '; }
            write_volatile(out.add(o), c);
            o += 1;
            j += 1;
        }
        if o + 1 < 4096 { write_volatile(out.add(o), b'\n'); o += 1; }
        li += 1;
    }
    if o == 0 {
        put_str(out, "暂无日志\0".as_bytes());
    } else {
        write_volatile(out.add(o), 0);
    }

    let msg = core::ptr::addr_of_mut!(LOG_MSG) as *mut u8;
    if count == 0 {
        put_str(msg, "暂无日志\0".as_bytes());
    } else {
        let mut b = [0u8; 48];
        let mut w = crate::text::W::new(b.as_mut_ptr(), 48);
        w.s("共 ".as_bytes());
        w.n(count as u32);
        w.s(" 行 第 ".as_bytes());
        w.n((LOG_PAGE + 1) as u32);
        w.s("/".as_bytes());
        w.n((maxp + 1) as u32);
        w.s(" 页".as_bytes());
        w.end();
        let len = w.len();
        let mut k = 0usize;
        while k < len && k + 1 < 48 { write_volatile(msg.add(k), b[k]); k += 1; }
        write_volatile(msg.add(k), 0);
    }
}

impl Page for LogPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "运行日志\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            log_page_load();
            let maxp = if LOG_LINES == 0 { 0 } else { (LOG_LINES - 1) / LOG_PAGE_LINES };
            let specs: [(usize, RowSpec); 6] = [
                (0, RowSpec::clickable("返回\0".as_bytes())),
                (1, RowSpec::clickable(if LOG_PAGE < maxp { "更早一页\0".as_bytes() } else { "已到最早\0".as_bytes() })),
                (2, RowSpec::clickable(if LOG_PAGE > 0 { "更新一页\0".as_bytes() } else { "已是最新\0".as_bytes() })),
                (3, RowSpec::clickable("刷新\0".as_bytes())),
                (4, RowSpec::clickable(if LOG_CLEAR_ARMED != 0 { "再点一次确认清空\0".as_bytes() } else { "清空日志\0".as_bytes() })),
                (5, RowSpec::display(
                    core::slice::from_raw_parts(core::ptr::addr_of!(LOG_MSG) as *const u8,
                        cstr_len(core::ptr::addr_of!(LOG_MSG) as *const u8) + 1),
                    core::ptr::null())),
            ];
            build_rows(ctx, ctx.title, &specs)?;
            let rp = page_rows_ptr(17);
            let prev = if rp.is_null() { 0 } else { read_volatile(rp.add(5)) };
            let lab = fw_api::label_create(ctx.content);
            if lab != 0 {
                fw_api::style_apply(lab, crate::FW_STYLE_HR_NAME, 255, 0);
                fw_api::label_set_text(lab, core::ptr::addr_of!(LOG_TEXT) as *const u8);
                fw_api::obj_set_size_wh(lab, 320, 320);
                fw_api::obj_align_to(lab, if prev != 0 { prev } else { ctx.title },
                    fw_api::align::OUT_BOTTOM_MID, 0, 8);
            }
            Ok(())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                LOG_CLEAR_ARMED = 0;
                nav_back();
            } else if idx == 1 {
                let maxp = if LOG_LINES == 0 { 0 } else { (LOG_LINES - 1) / LOG_PAGE_LINES };
                if LOG_PAGE < maxp { LOG_PAGE += 1; }
                render_req(17);
            } else if idx == 2 {
                if LOG_PAGE > 0 { LOG_PAGE -= 1; }
                render_req(17);
            } else if idx == 3 {
                render_req(17);
            } else if idx == 4 {
                if LOG_CLEAR_ARMED == 0 {
                    LOG_CLEAR_ARMED = 1;
                    render_req(17);
                } else {
                    let ok = crate::log::clear();
                    LOG_CLEAR_ARMED = 0;
                    put_str(core::ptr::addr_of_mut!(LOG_MSG) as *mut u8,
                        if ok { "日志已清空\0".as_bytes() } else { "清空失败\0".as_bytes() });
                    LOG_PAGE = 0;
                    render_req(17);
                }
            }
        }
    }
}

// ===== 页8/9: 表盘列表(切换/轮换) =====

pub(crate) struct WatchfaceListPage {
    /// 本页对应的 page_id (8 或 9)
    pub pid: usize,
}

impl Page for WatchfaceListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if self.pid == 8 { put_str(buf, "表盘切换".as_bytes()); }
            else { put_str(buf, "摇一摇轮换".as_bytes()); }
        }
    }

    fn render(&self, _ctx: &PageCtx) -> PageResult {
        unsafe {
            refresh_watchface_lines();
            rows_layout(self.pid);
            wf_apply_selection(self.pid);
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        unsafe {
            if self.pid == 8 {
                let v = st_rd!(WF_SHOW_BUILTIN);
                st_wr!(WF_SHOW_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                render_req(8);
            } else {
                let v = st_rd!(WF_SEL_BUILTIN);
                st_wr!(WF_SEL_BUILTIN, if v != 0 { 0 } else { 1 });
                load_watchfaces(st_rd!(WF_SEL_BUILTIN));
                render_req(9);
            }
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 { return; }
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            if idx <= 8 {
                let e = win * 8 + (idx - 1);
                if e < cnt {
                    if self.pid == 8 {
                        let sw: unsafe extern "C" fn(*const u8) -> i32 =
                            core::mem::transmute(FW_SET_WATCHFACE as usize);
                        let rc = sw((core::ptr::addr_of!(WF_IDS) as *const u8).add(e * 16));
                        st_wr!(WF_SW_RC, rc);
                        if rc == 0 {
                            let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                            let rf: unsafe extern "C" fn(u32, u32) -> i32 =
                                core::mem::transmute(FW_REFRESH_CUR_WF as usize);
                            let rfc = rf(1, node);
                            st_wr!(WF_RF_RC, rfc);
                        }
                        load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                        st_wr!(WF_WIN, win as u32);
                        wf_apply_selection(8);
                    } else {
                        let node = read_volatile((core::ptr::addr_of!(WF_NODES) as *const u32).add(e));
                        let off = !desel_has(node);
                        desel_set(node, off);
                        wf_apply_selection(9);
                    }
                }
            } else if idx == 9 {
                if (win + 1) * 8 < cnt {
                    st_wr!(WF_WIN, (win + 1) as u32);
                    render_req(self.pid);
                }
            } else {
                nav_back();
            }
        }
    }

    fn apply_visibility(&self) {
        unsafe {
            let sh = fw_api::obj_set_hidden;
            let rows = page_rows_ptr(self.pid);
            let cnt = st_rd!(WF_COUNT) as usize;
            let win = st_rd!(WF_WIN) as usize;
            let sw0 = read_volatile(rows.add(0));
            if sw0 != 0 { sh(sw0, 0); }
            for r in 0..8usize {
                let op = read_volatile(rows.add(r + 1));
                if op != 0 {
                    let vis = if cnt == 0 { if r == 0 { 0u32 } else { 1u32 } }
                              else if win * 8 + r < cnt { 0u32 } else { 1u32 };
                    sh(op, vis);
                }
            }
            let op9 = read_volatile(rows.add(9));
            if op9 != 0 { sh(op9, if (win + 1) * 8 < cnt { 0 } else { 1 }); }
            let op10 = read_volatile(rows.add(10));
            if op10 != 0 { sh(op10, 0); }
        }
    }
}

// ===== 页10: 表盘管理 =====

pub(crate) struct WfMgmtPage;

impl Page for WfMgmtPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "表盘管理".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let mut tbuf: [u8; 88] = [0; 88];
            shake_toggle_text(tbuf.as_mut_ptr());
            let tlen = cstr_len(tbuf.as_ptr());
            let text = core::slice::from_raw_parts(tbuf.as_ptr(), tlen + 1);
            let prev = build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch(text)),
                (1, RowSpec::clickable("摇一摇轮换\0".as_bytes())),
                (2, RowSpec::clickable("表盘切换\0".as_bytes())),
            ])?;
            // 补写开关初态
            let rows = page_rows_ptr(10);
            let s0 = read_volatile(rows.add(0));
            if s0 != 0 {
                let upd = fw_api::row_update;
                let en = st_rd!(SHAKE_EN);   // 开关真源 = 全局(与开机恢复/定时器门控同源)
                upd(s0, core::ptr::null(), tbuf.as_ptr(), core::ptr::null(), 0, en as u8);
            }
            let _ = prev;
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 }

    fn on_switch(&self, _idx: usize) {
        // 开关唯一真源 = 全局 SHAKE_EN(摇一摇定时器门控与开机恢复都读它)。
        // 页面不再持本地副本 —— 本地副本会让开关初态与持久化状态脱钩
        // (旧实现: 本地默认 1, 重启后永远显示"开", 且第一次拨动写的值与所见相反)。
        unsafe {
            let v = if st_rd!(SHAKE_EN) != 0 { 0u32 } else { 1u32 };
            st_wr!(SHAKE_EN, v);
            crate::watchface::persist_shake_enabled(v);
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                1 => nav_goto(9, 0),
                2 => {
                    load_watchfaces(st_rd!(WF_SHOW_BUILTIN));
                    nav_goto(8, 0);
                }
                _ => {}
            }
        }
    }
}

// ===== 页11: 亮度 =====

pub(crate) struct BrightnessPage {
    pub req: AtomicU32,
    pub req_val: AtomicU32,
}

impl Page for BrightnessPage {

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "亮度控制".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let cur = fw_api::brightness_get_target();
            st_wr!(BRIGHT_VAL, cur);
            let auto_on = fw_api::brightness_get_auto_adjustment();
            bright_fill_status();
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::display("当前亮度\0".as_bytes(), core::ptr::addr_of!(BRIGHT_STATUS) as *const u8)),
                (1, RowSpec::clickable(b"600nit\0")),
                (2, RowSpec::clickable(b"2000nit\0")),
                (3, RowSpec::switch("自动亮度\0".as_bytes())),
            ])?;
            // 补写开关初态
            let rows = page_rows_ptr(11);
            let r3 = read_volatile(rows.add(3));
            if r3 != 0 {
                let upd = fw_api::row_update;
                upd(r3, core::ptr::null(), "自动亮度\0".as_bytes().as_ptr(),
                    core::ptr::null(), 0, auto_on as u8);
            }
            Ok(())
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 3 }

    fn on_click(&self, idx: usize) {
        match idx {
            1 => {
                self.req_val.store(600, Ordering::Relaxed);
                self.req.store(4, Ordering::Relaxed);
                unsafe {
                    st_wr!(BRIGHT_REQ_VAL, 600);
                    st_wr!(BRIGHT_REQ, 4);
                }
            }
            2 => {
                self.req_val.store(2000, Ordering::Relaxed);
                self.req.store(4, Ordering::Relaxed);
                unsafe {
                    st_wr!(BRIGHT_REQ_VAL, 2000);
                    st_wr!(BRIGHT_REQ, 4);
                }
            }
            _ => {}
        }
    }

    fn on_switch(&self, _idx: usize) {
        self.req.store(2, Ordering::Relaxed);
        unsafe { st_wr!(BRIGHT_REQ, 2); }
    }

    fn needs_tick(&self) -> bool {
        self.req.load(Ordering::Relaxed) != 0
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
            let label: &[u8] = if st_rd!(crate::cacheclean::CC_ARM) != 0 {
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
            if st_rd!(crate::cacheclean::CC_ARM) == 0 {
                // 第一次点: 武装确认(只改行文本, 不在事件回调做长 FS 操作)
                st_wr!(crate::cacheclean::CC_ARM, 1);
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
            render_req(12);
        }
    }
}

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

/// 页28: 系统美化 -> 应用列表设置(二级)。桌面图标三条排版律的开关都放在那里。
pub(crate) const APP_LIST_PID: u32 = 28;

pub(crate) struct BeautifyPage;

impl Page for BeautifyPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统美化\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable("更换字体\0".as_bytes())),
                (1, RowSpec::clickable("桌面图标\0".as_bytes())),
                (2, RowSpec::clickable("系统模糊\0".as_bytes())),
                (3, RowSpec::switch("重启恢复字体和图标\0".as_bytes())),
                (4, RowSpec::switch("重启恢复模糊\0".as_bytes())),
                (5, RowSpec::clickable("应用列表设置\0".as_bytes())),
                (6, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(3)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, crate::beautify_restore::status(), 0,
                crate::beautify_restore::enabled() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(4)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, crate::res_hook::status(), 0,
                crate::res_hook::held() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            // 二级页一律 nav_goto(真页面跳转 => 固件带压栈动画, 返回键也是动画 pop);
            // 页内切层级(只 render_req)没有动画, 不许再用这种做法。
            match idx {
                0 => nav_goto(5, 0),
                1 => nav_goto(6, 0),
                2 => nav_goto(crate::background_home::BLUR_PID, 0),
                5 => nav_goto(APP_LIST_PID, 0),
                6 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 3 || idx == 4 }

    fn on_switch(&self, idx: usize) {
        match idx {
            3 => unsafe { crate::beautify_restore::request_toggle(); },
            // 关掉 = 让出 LVGL 资源钩子。Corona 那类模块要求槽里仍是出厂回调才肯装,
            // 先让位、让它装好、再打开这一格, 两条重定向就能串在一起。
            4 => unsafe { crate::res_hook::request_toggle(); },
            _ => {}
        }
    }
}

// ===== 页28: 系统美化 -> 应用列表设置(二级) =====
//
// 桌面图标的三条排版参数都在 desktop_scale 那张写表层: 行0 = 缩放折线(同 RW6),
// 行1 = 排距锚点(同 RW6), 行2 = 外列内缩(同 RW6)。三个开关都默认开, 关掉就把
// 对应的表写回固件原值 —— 实现路径只有一条, 开关只改参数。
pub(crate) struct AppListPage;

impl Page for AppListPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用列表设置\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("缩放同 RW6\0".as_bytes())),
                (1, RowSpec::switch("排距同 RW6\0".as_bytes())),
                (2, RowSpec::switch("内缩同 RW6\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::lens_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::pitch_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(2)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR, 0,
                crate::desktop_scale::inset_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe { if idx == 11 { nav_back(); } }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx < 3 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                crate::desktop_scale::set_lens(!crate::desktop_scale::lens_on());
            } else if idx == 1 {
                crate::desktop_scale::set_pitch(!crate::desktop_scale::pitch_on());
            } else if idx == 2 {
                crate::desktop_scale::set_inset(!crate::desktop_scale::inset_on());
            } else {
                return;
            }
            crate::render_req(APP_LIST_PID as usize);
        }
    }
}

// ===== 页4: 系统美化 -> 系统模糊(二级) =====
//
// 背景档位与状态文案都在 background_home(0 关 / 1 复用静态背景 / 3 完全透明),
// 这里只建行与派发点击。返回固定走行 11 —— 与"更换字体""桌面图标"两页同一套。
// 原"固定图片背景"那一档已删: 它只读 /data/chaos/background.bin, 而这文件从没有生成过。
// 行5 = "模糊区域"入口(页15, blur_zone): 分区域启停背景替换(启动器/控制中心/通知中心/小部件)。
pub(crate) struct BlurPage;

impl Page for BlurPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统模糊\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("系统背景\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::switch("静态背景图(表盘)\0".as_bytes())),
                (2, RowSpec::switch("自动截取替换图片\0".as_bytes())),
                (3, RowSpec::clickable("手动生成或更新表盘背景\0".as_bytes())),
                (4, RowSpec::switch("完全透明背景（实验性）\0".as_bytes())),
                (5, RowSpec::display(crate::background_home::status(), core::ptr::null())),
                (6, RowSpec::clickable("模糊区域\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            // 勾选态原地写(不重建整页): 行 0/1 是两档单选(档位), 行 2/4 是开关
            let sel = crate::background_home::mode() as usize;
            for i in 0..2 {
                fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(i)), fw_api::NONE_PTR,
                    fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (i == sel) as u8);
            }
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(4)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, (sel == 3) as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(2)), fw_api::NONE_PTR,
                fw_api::NONE_PTR, fw_api::NONE_PTR, 0, crate::background_home::realtime_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => crate::background_home::request(0),
                3 => crate::background_home::generate(),
                6 => nav_goto(crate::blur_zone::PAGE_PID, 0),
                11 => nav_back(),
                _ => {}
            }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 1 || idx == 2 || idx == 4 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 1 {
                crate::background_home::request(
                    if crate::background_home::mode() == 1 { 0 } else { 1 });
            }
            if idx == 2 {
                crate::background_home::set_realtime(
                    if crate::background_home::realtime_on() != 0 { 0 } else { 1 });
            }
            if idx == 4 {
                crate::background_home::request(
                    if crate::background_home::mode() == 3 { 0 } else { 3 });
            }
        }
    }
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
                let r = build_row(ctx, last, FONT_ROW0, &RowSpec::display(
                    "还没有投递过字体\0".as_bytes(), FONT_EMPTY_SUB.as_ptr()));
                if r != 0 { last = r; }
            }
            build_rows(ctx, last, &[
                (9, RowSpec::clickable("返回\0".as_bytes()))])?;
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
                9 => nav_back(),
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
            build_rows(ctx, last, &[(11, RowSpec::clickable("返回\0".as_bytes()))])?;
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
                11 => nav_back(),
                _ => {}
            }
        }
    }
}

// ===== 页1: 应用管理(Shell++ shellpp-ii 核心逻辑移植, 见 appmgr.rs) =====
//
// (2026-10-15 二版) 列表形态改为**全量条目**: 去掉"更多"分页, 注册表里的每个应用
// 一行复选框(勾 = 当前可见), 行数超过一屏靠 content 滚动(lvx_page_content 本就是
// 可滚容器, content_pad_bottom 就是给滚动收尾用的)。
// 行句柄/事件槽位约束: 条目行**不走 ctx.rows**(只有 12 格) —— 句柄自管在
// appmgr::APP_ROWS, 事件槽号 = 条目下标 + 12(12..=51; 0..11 是 ctx.rows 的
// 批量按钮/状态行, 条目复用会被派发成点按钮), 回调是 lib.rs 的 chaos_row_ev12..51。
// 布局: 行0=一键隐藏全部 行1=一键显示全部 行2=状态 行3=排除项管理入口 —— 放最上面,
// 列表再长也不怕按钮滚出屏幕; 之后是应用条目(已排除的条目不在本页显示)。

pub(crate) struct AppMgrPage;

// UI 令牌: 与蓝牙音频模块同一套配色(白 / 次要灰 / 强调蓝 / 成功绿 / 警告琥珀 / 危险红)。
#[allow(dead_code)]
const C_SECONDARY: u32 = 0x8E_8E93;
const C_ACCENT: u32 = 0x2D_7CF6;
#[allow(dead_code)]
const C_SUCCESS: u32 = 0x30_D158;
const C_WARNING: u32 = 0xFF_9F0A;
const C_DANGER: u32 = 0xFF_453A;

/// 状态文字: 已隐藏=警告琥珀, 显示中=强调蓝。
const C_HIDDEN: u32 = C_WARNING;
const C_HIDDEN_TINT: u32 = 0x4A3008;
const C_VISIBLE_TINT: u32 = 0x10284D;
const C_VISIBLE: u32 = C_ACCENT;

static mut AM_STATUS_HID: [u8; 12] = [0; 12];
static mut AM_STATUS_VIS: [u8; 12] = [0; 12];

/// 列表行原地刷新(页1/页30 共用): 行数没变就逐行改副标题与颜色, 不重建整页,
/// 这样列表滚动位置不会跳回顶部, 也不会在快速连点时销毁/重建行对象。
/// 返回 false = 行数或句柄对不上, 调用方需要整页重建。
static mut ROW_STATE: [u8; 40] = [0xFF; 40];

unsafe fn refresh_list_rows(selected_mode: bool) -> bool {
    if list_building() { return true; } // 还在分批建行, 先不刷
    let n = crate::appmgr::view_count();
    if n as u32 != crate::appmgr::ui_rows() { return false; }
    let rows = crate::appmgr::rows_ptr();
    for v in 0..n {
        let i = crate::appmgr::view_at(v);
        if i == usize::MAX { return false; }
        let r = core::ptr::read_volatile(rows.add(v));
        if r == 0 { return false; }
        let vis = crate::appmgr::visible(i);
        let sub = if vis {
            core::ptr::addr_of!(AM_STATUS_VIS) as *const u8
        } else {
            core::ptr::addr_of!(AM_STATUS_HID) as *const u8
        };
        let sel = if selected_mode && crate::appmgr::sel_has(i) { 1u8 } else { 0u8 };
        let st = (vis as u8) | (sel << 1);
        let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
        if core::ptr::read_volatile(cache.add(v)) == st { continue; }
        core::ptr::write_volatile(cache.add(v), st);
        fw_api::row_update(r, fw_api::NONE_PTR,
            crate::appmgr::entry_primary(i), fw_api::NONE_PTR, 0, sel);
        let sp = core::ptr::read_volatile((core::ptr::addr_of!(P_STATE_LB) as *const u32).add(v));
        let stxt: &[u8] = if vis { "显示中\0".as_bytes() } else { "已隐藏\0".as_bytes() };
        if sp != 0 {
            fw_api::label_set_text(sp, stxt.as_ptr());
            if vis { pill(sp, C_VISIBLE, C_VISIBLE_TINT, stxt); }
            else { pill(sp, C_HIDDEN, C_HIDDEN_TINT, stxt); }
        }
        if !selected_mode {
            let p = core::ptr::read_volatile((core::ptr::addr_of!(P_EXCL_LB) as *const u32).add(v));
            set_excl_pill(p, i);
        }
    }
    true
}

/// 状态行原地刷新(页1 的"已隐藏 N / 显示中 M"提示)
unsafe fn refresh_msg_row() {
    let r = core::ptr::read_volatile(core::ptr::addr_of!(AM_MSG_ROW));
    if r != 0 {
        fw_api::row_update(r, fw_api::NONE_PTR,
            crate::appmgr::msg().as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
}


// ---- 列表分批建立 ----
// 打开页面时一次建几十行会长时间占住 UI 线程；这里先建首屏几行, 其余的每次 tick 续几行。
// kind: 1 = 页1(无复选框), 2 = 页30(带复选框)。副标题用调用方传进来的状态文案。
static mut LB_KIND: u32 = 0;
static mut LB_V: u32 = 0;
static mut LB_N: u32 = 0;
static mut LB_PREV: u32 = 0;
static mut LB_CONTENT: u32 = 0;
static mut LB_FIRST: u32 = 0;
static mut LB_DELAY: u32 = 0;
static mut LB_SUB_VIS: u32 = 0;
static mut LB_SUB_HID: u32 = 0;

static mut LIST_REBUILD: u8 = 0;

const LB_FIRST_CHUNK: u32 = 0;
const LB_TICK_CHUNK: u32 = 5;

pub(crate) fn list_building() -> bool { unsafe { LB_V < LB_N } }

unsafe fn list_begin(kind: u32, prev: u32, content: u32, sub_vis: *const u8, sub_hid: *const u8) {
    LB_KIND = kind;
    LB_V = 0;
    LB_N = crate::appmgr::view_count() as u32;
    LB_PREV = prev;
    LB_CONTENT = content;
    LB_FIRST = 1;
    LB_SUB_VIS = sub_vis as u32;
    LB_SUB_HID = sub_hid as u32;
    {
        let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
        let mut i = 0usize;
        while i < 40 { core::ptr::write_volatile(cache.add(i), 0xFF); i += 1; }
        let pl = core::ptr::addr_of_mut!(P_EXCL_LB) as *mut u32;
        let mut i = 0usize;
        while i < 40 { core::ptr::write_volatile(pl.add(i), 0); i += 1; }
        let lm = core::ptr::addr_of_mut!(LAST_MSG) as *mut u8;
        let lh = core::ptr::addr_of_mut!(LAST_HDR) as *mut u8;
        let mut i = 0usize;
        while i < 64 { core::ptr::write_volatile(lm.add(i), 0xFF); core::ptr::write_volatile(lh.add(i), 0xFF); i += 1; }
        let mut z = 0usize;
        let sp_ = core::ptr::addr_of_mut!(P_STATE_LB) as *mut u32;
        let ep_ = core::ptr::addr_of_mut!(P_EXCL_LB) as *mut u32;
        while z < 40 { core::ptr::write_volatile(sp_.add(z), 0); core::ptr::write_volatile(ep_.add(z), 0); z += 1; }
    }
    LIST_REBUILD = 0;
    LB_DELAY = 3;
    crate::appmgr::set_ui_rows(LB_N);
    crate::log::kv("应用总数 ", crate::appmgr::count() as u32);
    crate::log::kv("可见列表条目数 ", LB_N);
    list_build_chunk(LB_FIRST_CHUNK);
}

unsafe fn list_build_chunk(limit: u32) {
    let rows = crate::appmgr::rows_ptr();
    let mut k = 0u32;
    while k < limit && LB_V < LB_N {
        let v = LB_V as usize;
        let i = crate::appmgr::view_at(v);
        if i == usize::MAX { LB_V = LB_N; break; }
        let vis = crate::appmgr::visible(i);
        let trailing = if LB_KIND == 2 { fw_api::trailing::CHECKBOX } else { 0 };
        let r = fw_api::row_create(LB_CONTENT, crate::appmgr::entry_primary(i),
            b"\xc2\xa0\0".as_ptr(), trailing);
        if r == 0 { crate::log::kv("列表行创建失败, 已建 ", LB_V); LB_V = LB_N; break; }
        write_volatile(rows.add(v), r);
        fw_api::obj_add_event(r, row_ev_fn(v + 12), 7, 0);
        if LB_KIND == 2 {
            let tobj = fw_api::row_trailing(r);
            if tobj != 0 { fw_api::obj_add_event(tobj, row_ev_fn(v + 12), 0, 0); }
        }
        fw_api::obj_set_size_wh(r, 320, 100);
        fw_api::obj_set_local_style_prop(r, crate::fw_api::fwaddr::p65_style::PAD_3, 10, 0);
        cardify(r);
        let sp = fw_api::label_create(r);
        let stxt: &[u8] = if vis { "显示中\0".as_bytes() } else { "已隐藏\0".as_bytes() };
        if sp != 0 {
            fw_api::label_set_text(sp, stxt.as_ptr());
            if vis { pill(sp, C_VISIBLE, C_VISIBLE_TINT, stxt); }
            else { pill(sp, C_HIDDEN, C_HIDDEN_TINT, stxt); }
            // 锚在固件自己排出来的第二行(副标签)上: 垂直居中于该行, 水平用 dx 拉回左边距。
            // 第二行一定在卡片内部, 所以胶囊不会再掉到卡片外面。
            let sub = crate::mem::rd32((r + 0x40) as *const u32);
            let base = if sub != 0 { sub } else { r };
            fw_api::obj_align_to(sp, base, fw_api::align::OUT_BOTTOM_LEFT, 18, 0);
        }
        P_STATE_LB[v] = sp;
        let exp = add_excl_pill(r, sp, i, LB_KIND == 2);
        if LB_KIND == 1 { P_EXCL_LB[v] = exp; }
        if LB_KIND == 1 {
            let cache = core::ptr::addr_of_mut!(ROW_STATE) as *mut u8;
            core::ptr::write_volatile(cache.add(v), (vis as u8) | 0);
        }
        let ddx = if LB_KIND == 2 && LB_FIRST != 0 { TILE_CENTER_DX } else { 0 };
        fw_api::obj_align_to(r, LB_PREV, 14, ddx, 10);
        LB_FIRST = 0;
        LB_PREV = r;
        if LB_KIND == 2 {
            fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR,
                fw_api::NONE_PTR, 0, crate::appmgr::sel_has(i) as u8);
        }
        LB_V += 1;
        k += 1;
    }
}

impl Page for AppMgrPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用管理\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::ensure();
            crate::appmgr::rows_reset();
            // 顶部: 批量操作(整宽) + 状态行 + 重新应用隐藏
            let t0 = build_tile_below(ctx, ctx.title, 0,
                "批量操作 >\0".as_bytes(), TILE_ROW_W, TILE_H);
            if t0 == 0 { return Err(PageError::RowFailed(0)); }
            let s2 = build_row(ctx, t0, 2,
                &RowSpec::display(crate::appmgr::msg(), core::ptr::null()));
            row_shrink(s2, 56);
            AM_MSG_ROW = read_volatile(ctx.rows.add(2));
            if s2 != 0 {
                let t3 = build_tile_below(ctx, s2, 3,
                    "重新应用隐藏\0".as_bytes(), 328, 56);
                tile_primary(t3);
            }
            crate::appmgr::sort_view_visible_first();

            // 状态文案(静态缓冲, 供每行的副标签使用)
            put_str(core::ptr::addr_of_mut!(AM_STATUS_HID) as *mut u8, "已隐藏\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(AM_STATUS_VIS) as *mut u8, "显示中\0".as_bytes());

            // 条目行: 先建首屏, 其余由 tick 分批续建
            let mut prev = read_volatile(ctx.rows.add(3));
            if prev == 0 { prev = ctx.title; }

            // 分组说明行: "快应用 N / 已隐藏 M"
            let hdr = fw_api::row_create(ctx.content,
                crate::appmgr::header_text().as_ptr(), fw_api::NONE_PTR, 0);
            if hdr != 0 {
                row_shrink(hdr, 44);
                fw_api::obj_align_to(hdr, prev, 14, 0, 14);
                let hl = crate::mem::rd32((hdr + ROW_SUB_LB) as *const u32);
                if hl != 0 { fw_api::obj_style_color_a70(hl, C_SECONDARY, 0); }
                prev = hdr;
                AM_HDR_ROW = hdr;
            }
            list_begin(1, prev, ctx.content,
                core::ptr::addr_of!(AM_STATUS_VIS) as *const u8,
                core::ptr::addr_of!(AM_STATUS_HID) as *const u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        // 页1 顶部按钮槽位不变: 0=批量, 1=排除项, 3=重新应用隐藏
        unsafe {
            if idx == 0 {
                nav_goto(30, 0);            // 批量操作
            } else if idx == 3 {
                crate::appmgr::request_reapply();
                render_req(1);
            } else if idx >= 12 {
                let v = idx - 12;
                if v < crate::appmgr::view_count() {
                    let e = crate::appmgr::view_at(v);
                    if e != usize::MAX {
                        crate::appmgr::detail_open(e);
                        nav_goto(29, 0);    // 应用详情
                    }
                }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe {
            list_building() || crate::appmgr::has_req() || crate::appmgr::batch_pending()
                || crate::appmgr::reapply_pending()
        }
    }

    fn tick(&self) {
        unsafe {
            if list_building() {
                if LB_DELAY > 0 { LB_DELAY -= 1; }
                else { list_build_chunk(LB_TICK_CHUNK); }
            }
            if crate::appmgr::reapply_pending() {
                crate::appmgr::do_reapply();
            } else if crate::appmgr::batch_pending() {
                crate::appmgr::do_batch();
            } else if crate::appmgr::has_req() {
                crate::appmgr::do_op();
            }
            // 只改了隐藏位/名单时逐行原地刷新, 行数变了(卸载、重扫)才整页重建。
            // 整页重建会销毁行对象, 列表滚动位置会跳回顶部。
            if text_changed(core::ptr::addr_of_mut!(LAST_MSG) as *mut u8,
                crate::appmgr::msg().as_ptr(), 64) {
                refresh_msg_row();
            }
            let h = core::ptr::read_volatile(core::ptr::addr_of!(AM_HDR_ROW));
            if h != 0 && text_changed(core::ptr::addr_of_mut!(LAST_HDR) as *mut u8,
                crate::appmgr::header_text().as_ptr(), 64) {
                fw_api::row_update(h, fw_api::NONE_PTR,
                    crate::appmgr::header_text().as_ptr(), fw_api::NONE_PTR, 0, 0);
            }
            // 行句柄失效时不再整页重建: 重建会销毁 40 个对象并再次触发本 tick, 形成每帧重建的卡顿。
            // 行数真的变了(卸载/重扫)由对应操作自己 render_req。
            let _ = refresh_list_rows(false);
        }
    }
}

// ===== 页16: 主页 -> 震动 =====
//
// 行0 = "滑动列表震动"(launcher 根 SCROLL_END + 拖拽距离过滤, 见 background_home.rs);
// 行1 = "右滑返回震动"(前台 app 切回 HOME 震一次); 行2 = "测试震动";
// 行11 = 返回。两个开关独立持久化 /data/chaos/swipe_vib.cfg, 默认全关。
pub(crate) struct VibPage;

impl Page for VibPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "震动\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("滑动列表震动\0".as_bytes())),
                (1, RowSpec::switch("右滑返回震动\0".as_bytes())),
                (2, RowSpec::clickable("测试震动\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(0)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::background_home::swipe_list_on() as u8);
            fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(1)),
                fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                0, crate::background_home::swipe_back_on() as u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 2 { crate::background_home::vib_test_req(); }
            else if idx == 11 { nav_back(); }
        }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx == 0 || idx == 1 }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                crate::background_home::set_swipe_vib(true,
                    if crate::background_home::swipe_list_on() != 0 { 0 } else { 1 });
            } else if idx == 1 {
                crate::background_home::set_swipe_vib(false,
                    if crate::background_home::swipe_back_on() != 0 { 0 } else { 1 });
            }
        }
    }
}

// ===== 分发表 =====

static PAGE_INFO: InfoPage = InfoPage;
static PAGE_APPMGR: AppMgrPage = AppMgrPage;
static PAGE_EXCL: ExclPage = ExclPage;
static PAGE_WF_LIST_8: WatchfaceListPage = WatchfaceListPage { pid: 8 };
static PAGE_WF_LIST_9: WatchfaceListPage = WatchfaceListPage { pid: 9 };
static PAGE_WF_MGMT: WfMgmtPage = WfMgmtPage;
static PAGE_BRIGHT: BrightnessPage = BrightnessPage { req: AtomicU32::new(0), req_val: AtomicU32::new(0) };
static PAGE_CACHEP: CachePage = CachePage;
static PAGE_BEAUTIFY: BeautifyPage = BeautifyPage;
static PAGE_APPLIST: AppListPage = AppListPage;
static PAGE_BLURP: BlurPage = BlurPage;
static PAGE_ZONEP: BlurZonePage = BlurZonePage;
static PAGE_VIBP: VibPage = VibPage;
static PAGE_FONTLIST: FontListPage = FontListPage;
static PAGE_ICONSUB: IconSubPage = IconSubPage;
static PAGE_SETTINGS: SettingsPage = SettingsPage;

pub(crate) struct SettingsPage;
static mut ABOUT_FW: [u8; 32] = [0; 32];
static mut ABOUT_BT: [u8; 40] = [0; 40];

unsafe fn about_firmware() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_FW) as *mut u8;
    let fd = fw_api::open(b"/etc/build.prop\0".as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd >= 0 {
        let mut b = [0u8; 512];
        let n = fw_api::read(fd, b.as_mut_ptr(), 511);
        fw_api::close(fd);
        let key = b"ro.build.version=";
        let mut i = 0usize;
        while i + key.len() < n.max(0) as usize {
            let mut same = true;
            for j in 0..key.len() { if b[i + j] != key[j] { same = false; break; } }
            if same {
                let start = i + key.len();
                let mut end = start;
                while end < n as usize && b[end] != b'\n' && b[end] != b'\r' && b[end] != 0 { end += 1; }
                let len = core::cmp::min(end - start, 31);
                for j in 0..len { write_volatile(out.add(j), b[start + j]); }
                write_volatile(out.add(len), 0);
                return core::slice::from_raw_parts(out, len + 1);
            }
            i += 1;
        }
    }
    put_str(out, "未知\0".as_bytes());
    core::slice::from_raw_parts(out, 7)
}

unsafe fn about_build_time() -> &'static [u8] {
    let out = core::ptr::addr_of_mut!(ABOUT_BT) as *mut u8;
    let src = option_env!("CHAOS_BUILD_TIME").unwrap_or("unknown").as_bytes();
    let len = core::cmp::min(src.len(), 39);
    for i in 0..len { write_volatile(out.add(i), src[i]); }
    write_volatile(out.add(len), 0);
    core::slice::from_raw_parts(out, len + 1)
}

impl Page for SettingsPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设置\0".as_bytes()); }
    }
    /// 页14 原来叫"关于"，只放版本与开发者。2026-10-18 重排：主页只留四个一级入口
    /// (应用管理 / 表盘管理 / 系统美化 / 设置)，常用的小开关与时间日期都收进这里，
    /// 版本信息压到底部当展示行 —— 用户要"清楚"，就不要在主页堆十个平级行。
    /// 页名字符串沿用 `about`(state/ipc 没动)：它只是 page_goto 的解析键，与标题无关。
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable("时间设置\0".as_bytes())),
                (1, RowSpec::clickable("日期设置\0".as_bytes())),
                (2, RowSpec::clickable("亮度\0".as_bytes())),
                (3, RowSpec::clickable("震动\0".as_bytes())),
                (4, RowSpec::clickable("缓存清理\0".as_bytes())),
                (5, RowSpec::clickable("运行日志\0".as_bytes())),
                (6, RowSpec::clickable("恢复fetch支持\0".as_bytes())),
                (7, RowSpec::clickable("系统关于页面修改\0".as_bytes())),
            ]).map(|_| ())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => time_edit_enter(TIME_PID as u32),
                1 => time_edit_enter(DATE_MD_PID as u32),
                2 => nav_goto(11, 0),   // 亮度
                3 => nav_goto(16, 0),   // 震动
                4 => nav_goto(12, 0),   // 缓存清理
                5 => nav_goto(17, 0),   // 运行日志
                6 => nav_goto(19, 0),   // 恢复 fetch 支持
                7 => nav_goto(24, 0),   // 系统关于页面修改
                _ => {}
            }
        }
    }
}

// ===== 页18: 关于 =====
pub(crate) struct AboutPage;
static PAGE_ABOUT_PAGE: AboutPage = AboutPage;
static mut DBG_ON: u32 = 0;
static mut DBG_CLICKS: u32 = 0;
static mut DBG_VAL: u32 = 50;
static mut DBG_TXT: [u8; 8] = [0; 8];
static mut DBG_DUMP_MSG: [u8; 32] = [0; 32];
static mut DBG_DUMP_ARM: u32 = 0;
static mut DBG_TICKS: u32 = 0;

unsafe fn dbg_text() {
    let v = DBG_VAL;
    let d = core::ptr::addr_of_mut!(DBG_TXT) as *mut u8;
    if v >= 100 { write_volatile(d.add(0), b'1'); write_volatile(d.add(1), b'0'); write_volatile(d.add(2), b'0'); write_volatile(d.add(3), b'%'); write_volatile(d.add(4), 0); }
    else if v >= 10 { write_volatile(d.add(0), b'0' + (v / 10) as u8); write_volatile(d.add(1), b'0' + (v % 10) as u8); write_volatile(d.add(2), b'%'); write_volatile(d.add(3), 0); }
    else { write_volatile(d.add(0), b'0' + v as u8); write_volatile(d.add(1), b'%'); write_volatile(d.add(2), 0); }
}

unsafe fn dbg_fields(a: fw_api::Obj) {
    if a == 0 { return; }
    let p = a as usize;
    crate::log::kv("dbg34 ", core::ptr::read_volatile((p + 0x34) as *const u32));
    crate::log::kv("dbg38 ", core::ptr::read_volatile((p + 0x38) as *const u32));
    crate::log::kv("dbg3C ", core::ptr::read_volatile((p + 0x3C) as *const u32));
    crate::log::kv("dbg40 ", core::ptr::read_volatile((p + 0x40) as *const u32));
    crate::log::kv("dbg44 ", core::ptr::read_volatile((p + 0x44) as *const u32));
    crate::log::kv("dbg48 ", core::ptr::read_volatile((p + 0x48) as *const u32));
    crate::log::kv("dbg4C ", core::ptr::read_volatile((p + 0x4C) as *const u32));
    crate::log::kv("dbg50 ", core::ptr::read_volatile((p + 0x50) as *const u32));
}

unsafe fn dbg_dump() {
    const BASE: usize = 0x1C06_0000;
    const LEN: usize = 512 * 1024;
    let path = b"/data/chaos/lvgl_dump.bin\0";
    let fd = fw_api::open(path.as_ptr(), fw_api::oflag::WRONLY | fw_api::oflag::CREAT, 0o640);
    let mut ok = fd >= 0;
    if ok {
        let mut off = 0usize;
        while off < LEN {
            let got = fw_api::write(fd, (BASE + off) as *const u8, (LEN - off) as u32);
            if got <= 0 { ok = false; break; }
            off += got as usize;
        }
        fw_api::close(fd);
    }
    let m = core::ptr::addr_of_mut!(DBG_DUMP_MSG) as *mut u8;
    let t: &[u8] = if ok { "dump 完成 512KB\0".as_bytes() } else { "dump 失败\0".as_bytes() };
    for i in 0..t.len() { write_volatile(m.add(i), t[i]); }
}

impl Page for AboutPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            if DBG_ON != 0 { put_str(buf, "调试\0".as_bytes()); }
            else { put_str(buf, "关于\0".as_bytes()); }
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if DBG_ON != 0 {
                dbg_text();
                // 4 环 = 2x2 验证矩阵（全部只写角度字段，不碰样式 API）：
                //   A/B 灰底环(MAIN 整圈) + 蓝进度(INDICATOR 90..90+pct*3.6) 25%/75%
                //   C   只有 INDICATOR 整圈 90..450，验证整圈特例
                //   D   对照：一个字段都不写，看构造函数默认值到底画出什么
                let specs: [(i32, i32, u32, bool, bool); 4] = [(52, 96, 25, true, true), (196, 96, 75, true, true), (52, 268, 100, false, true), (196, 268, 0, false, false)];
                let caps: [&[u8]; 4] = [b"A 25%\0" as &[u8], b"B 75%\0", b"C 100%\0", b"D def\0"];
                for (i, &(x, y, pct, track, progress)) in specs.iter().enumerate() {
                    if track {
                        let t = fw_api::arc_create(ctx.content);
                        if t != 0 {
                            fw_api::obj_set_size_wh(t, 120, 120);
                            fw_api::obj_align(t, fw_api::align::TOP_LEFT, x, y);
                            fw_api::arc_set_bg_full(t);
                            fw_api::arc_disable_input(t);
                        }
                    }
                    if progress {
                        let arc = fw_api::arc_create(ctx.content);
                        if arc != 0 {
                            fw_api::obj_set_size_wh(arc, 120, 120);
                            fw_api::obj_align(arc, fw_api::align::TOP_LEFT, x, y);
                            fw_api::arc_set_progress(arc, pct);
                            fw_api::arc_disable_input(arc);
                        }
                    }
                    let cl = fw_api::label_create(ctx.content);
                    if cl != 0 {
                        fw_api::style_apply(cl, crate::FW_STYLE_HR_NAME, 255, 0);
                        fw_api::label_set_text(cl, caps[i].as_ptr());
                        fw_api::obj_align(cl, fw_api::align::TOP_LEFT, x + 34, y + 122);
                    }
                }
                let _ = build_rows(ctx, ctx.content, &[
                    (0, RowSpec::tap("① 重绘四个对比环\0".as_bytes(), core::ptr::null())),
                    (1, RowSpec::tap("② 导出 LVGL dump\0".as_bytes(), core::ptr::null())),
                ]);
                let msg = fw_api::label_create(ctx.content);
                if msg != 0 {
                    fw_api::style_apply(msg, crate::FW_STYLE_HR_NAME, 255, 0);
                    fw_api::label_set_text(msg, core::ptr::addr_of!(DBG_DUMP_MSG) as *const u8);
                    fw_api::obj_align(msg, fw_api::align::TOP_MID, 0, 420);
                }
                return Ok(());
            }
            let bt = about_build_time();
            let fw = about_firmware();
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::tap("Chaos\0".as_bytes(), "v1.1.29\0".as_bytes().as_ptr())),
                (1, RowSpec::display("编译时间\0".as_bytes(), bt.as_ptr())),
                (2, RowSpec::display("运行固件\0".as_bytes(), fw.as_ptr())),
                (3, RowSpec::display("文华逸洋\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (4, RowSpec::display("IKUN_CXKPRO\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (5, RowSpec::display("谦虚的海鸥\0".as_bytes(), "开发者\0".as_bytes().as_ptr())),
                (6, RowSpec::display("@wzx\0".as_bytes(), "贡献者(fetch实现)\0".as_bytes().as_ptr())),
                (7, RowSpec::display("许可证\0".as_bytes(), "AGPL-3.0\0".as_bytes().as_ptr())),
                (8, RowSpec::clickable("开放源代码许可\0".as_bytes())),
            ])?;
            Ok(())
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe {
            if DBG_ON != 0 {
                if DBG_TICKS < 5 { return; }
                if idx == 1 {
                    if DBG_DUMP_ARM == 0 {
                        DBG_DUMP_ARM = 1;
                        let m = core::ptr::addr_of_mut!(DBG_DUMP_MSG) as *mut u8;
                        for (i, c) in "再点一次确认导出\0".as_bytes().iter().enumerate() { write_volatile(m.add(i), *c); }
                    } else {
                        DBG_DUMP_ARM = 0;
                        dbg_dump();
                    }
                    crate::ui::render_req(18);
                    return;
                }
                crate::ui::render_req(18);
                return;
            }
            if idx == 0 {
                DBG_CLICKS += 1;
                if DBG_CLICKS >= 5 { DBG_ON = 1; DBG_CLICKS = 0; DBG_TICKS = 0; DBG_DUMP_ARM = 0; crate::ui::render_req(18); }
            } else if idx == 8 {
                // 行 8 才是"开放源代码许可"(行 6 是 @wzx, 行 7 是不可点的许可证展示行)。
                // 判据按 build_rows 的 slot 走, 插行时必须跟着挪。
                nav_goto(20, 0);
            }
        }
    }
    fn needs_tick(&self) -> bool { unsafe { DBG_ON != 0 } }
    fn tick(&self) { unsafe { if DBG_ON != 0 { DBG_TICKS = DBG_TICKS.wrapping_add(1); } } }
}

// ===== 页2: 时间设置(时 + 分) =====
//
// 只做时分两轮, 与系统闹钟页同形。日期(月+日)是页3 的独立入口。
// 年份设置 2026-10-18 按用户要求删掉: 年轮那一整套(2000..2099 单列、TJ_MODE 分派)整段移除,
// 年份保持原值, 日期屏只改月日。
// 返回不走行控件 —— title_mode() 默认 1, 标题栏自带返回键, 与其它二级页一致。
pub(crate) struct TimeSetPage;
static PAGE_TIMESET: TimeSetPage = TimeSetPage;

impl Page for TimeSetPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "时间设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            tj_base_box(ctx.content);
            let tm = core::ptr::read_volatile(
                core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
            // 横向位置按闹钟页的字面值抄。它的 7 号对齐把 x 当**左边缘**偏移算，
            // 所以两列(宽 86)落在 60..146 与 190..276，右边距同样是 60，
            // 冒号 160 正好处在 146..190 那条缝里。
            tj_sep(ctx.content, ":\0".as_ptr(), 160);
            let p1 = tj_picker(ctx.content, 0, 23, tm[2], 86, 60);
            let p2 = tj_picker(ctx.content, 0, 59, tm[1], 86, 190);
            st_wr!(TJ_P1, p1); st_wr!(TJ_P2, p2);
            if p2 == 0 { tj_msg("无轮".as_bytes()); }
            tj_screen_tail(ctx, 2)
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe { if idx == 0 { tj_confirm(); } }
    }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) {
        unsafe {
            // 反馈排在保存之前: tj_save_now 会登记整页重建, 登记完再碰控件句柄没意义
            tj_feedback();
            if st_rd!(TJ_SAVE_REQ) != 0 {
                st_wr!(TJ_SAVE_REQ, 0);
                tj_save_now();
            }
        }
    }
}

// ===== 页3: 日期设置(月 + 日) =====
//
// 这一格原来是文件管理第三级目录，入口退役后一直不可达。二级界面必须有独立 page_id
// (只有真 page_goto/page_back 才有固件的压栈动画)，所以日期设置占它 —— 它是与页2 平级
// 的独立入口(关于页第三行), 不再由年屏压栈进入。
// 两列几何逐字用闹钟页那一套: 86 宽、左沿 60/190、分隔符 x=160。
// 句柄存 TJ_M1/TJ_M2，与页2 分开 —— 从页3 返回页2 不会重跑 render(框的 built 还置着)，
// 共用槽会让页2 拿着已经回收的轮去刷颜色。
pub(crate) struct DateMDPage;
static PAGE_DATE_MD: DateMDPage = DateMDPage;

impl Page for DateMDPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "日期设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            tj_base_box(ctx.content);
            let tm = core::ptr::read_volatile(
                core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
            tj_sep(ctx.content, "/\0".as_ptr(), 160);
            // 轮上的取值是 1 基(月 1..12、日 1..31)，而 tm 里 tm[4] 是 0 基、tm[3] 是 1 基，
            // 所以初值各加对应偏移 —— 上一版直接把 tm[4] 塞进一个 min=1 的轮，月份偏一格。
            // 日的上限固定 31，跨月多出来的那几天在写回时按年月夹掉(tj_pick_values)。
            let p1 = tj_picker(ctx.content, 1, 12, tm[4] + 1, 86, 60);
            let p2 = tj_picker(ctx.content, 1, 31, tm[3], 86, 190);
            st_wr!(TJ_M1, p1); st_wr!(TJ_M2, p2);
            if p2 == 0 { tj_msg("无轮".as_bytes()); }
            tj_screen_tail(ctx, 2)
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe { if idx == 0 { tj_confirm(); } }
    }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) {
        unsafe {
            tj_feedback();
            if st_rd!(TJ_SAVE_REQ) != 0 {
                st_wr!(TJ_SAVE_REQ, 0);
                tj_save_now();
            }
        }
    }
}

// ===== 页15: 系统模糊 -> 模糊区域(三级) =====
//
// 四个背景替换区域的开关(数据与持久化全在 blur_zone.rs, 这里只建行与派发):
//   行0=启动器 行1=控制中心 行2=通知中心 行3=小部件 行11=返回。
// 开 = 正常替换(默认); 关 = 该区域保持官方模糊画面。
// 翻位后 blur_zone::set 持久化 + background_home::remount() 置 REQUEST,
// 由背景 tick 的拆/装流程按新掩码统一重建(页面退出前就能看到拆装, 不用重启)。
// 开关初态: 单排行, row_update 第 6 参原地补写(secondary 传 NONE 是安全的 ——
// 这些行没有副标签, 详见 fw_api::row_update 注释)。
pub(crate) struct BlurZonePage;

impl Page for BlurZonePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "模糊区域\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::blur_zone::ensure_loaded();
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::switch("启动器\0".as_bytes())),
                (1, RowSpec::switch("控制中心\0".as_bytes())),
                (2, RowSpec::switch("小部件\0".as_bytes())),
                (3, RowSpec::switch("通知中心\0".as_bytes())),
                (11, RowSpec::clickable("返回\0".as_bytes()))])?;
            for i in 0..crate::blur_zone::REGION_N {
                fw_api::row_update(core::ptr::read_volatile(ctx.rows.add(i)),
                    fw_api::NONE_PTR, fw_api::NONE_PTR, fw_api::NONE_PTR,
                    0, crate::blur_zone::on(i) as u8);
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        if idx == 11 { unsafe { nav_back(); } }
    }

    fn is_switch_row(&self, idx: usize) -> bool { idx < crate::blur_zone::REGION_N }

    fn on_switch(&self, idx: usize) {
        unsafe {
            if idx >= crate::blur_zone::REGION_N { return; }
            if crate::blur_zone::set(idx, !crate::blur_zone::on(idx)) {
                crate::background_home::remount();
            }
        }
    }
}

// ===== 页20: 关于 -> 开放源代码许可 =====
pub(crate) struct LicensesPage;
static PAGE_LICENSESP: LicensesPage = LicensesPage;

impl Page for LicensesPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "开放源代码许可\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::tap("MIBAND10PRO-TUNMODULE\0".as_bytes(),
                    "MPL-2.0 license\0".as_bytes().as_ptr())),
            ]).map(|_| ())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 0 {
                MPL_PAGE = 0;
                nav_goto(21, 0);
            }
        }
    }
}

// ===== 页21: MIBAND10PRO-TUNMODULE / MPL-2.0 许可正文 =====
pub(crate) struct MplLicensePage;
static PAGE_LICENSE_MPLP: MplLicensePage = MplLicensePage;
static MPL_TEXT: &[u8] = include_bytes!("../../licenses/MIBAND10PRO-TUNMODULE-MPL-2.0.txt");
const MPL_PAGE_LINES: usize = 8;
static mut MPL_PAGE: usize = 0;
static mut MPL_VIEW: [u8; 4096] = [0; 4096];
static mut MPL_STATUS: [u8; 48] = [0; 48];

fn mpl_line_count() -> usize {
    let mut lines = 1usize;
    for &c in MPL_TEXT {
        if c == b'\n' { lines += 1; }
    }
    lines
}

fn mpl_page_count() -> usize {
    (mpl_line_count() + MPL_PAGE_LINES - 1) / MPL_PAGE_LINES
}

unsafe fn mpl_render_page() {
    let pages = mpl_page_count();
    if MPL_PAGE >= pages { MPL_PAGE = pages - 1; }
    let target_line = MPL_PAGE * MPL_PAGE_LINES;
    let mut line = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i < MPL_TEXT.len() && line < target_line {
        if MPL_TEXT[i] == b'\n' { line += 1; }
        i += 1;
        start = i;
    }
    let mut end = start;
    line = 0;
    while end < MPL_TEXT.len() && line < MPL_PAGE_LINES {
        if MPL_TEXT[end] == b'\n' { line += 1; }
        end += 1;
    }
    let out = core::ptr::addr_of_mut!(MPL_VIEW) as *mut u8;
    let count = core::cmp::min(end - start, 4095);
    for i in 0..count { write_volatile(out.add(i), MPL_TEXT[start + i]); }
    write_volatile(out.add(count), 0);

    let status = core::ptr::addr_of_mut!(MPL_STATUS) as *mut u8;
    let mut buf = [0u8; 48];
    let mut w = crate::text::W::new(buf.as_mut_ptr(), buf.len());
    w.s("第 ".as_bytes());
    w.n((MPL_PAGE + 1) as u32);
    w.s("/".as_bytes());
    w.n(pages as u32);
    w.s(" 页".as_bytes());
    w.end();
    let len = w.len();
    for i in 0..len { write_volatile(status.add(i), buf[i]); }
    write_volatile(status.add(len), 0);
}

impl Page for MplLicensePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "MPL-2.0 license\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            mpl_render_page();
            let pages = mpl_page_count();
            let specs: [(usize, RowSpec); 3] = [
                (0, RowSpec::clickable(if MPL_PAGE > 0 { "上一页\0".as_bytes() } else { "已到首页\0".as_bytes() })),
                (1, RowSpec::clickable(if MPL_PAGE + 1 < pages { "下一页\0".as_bytes() } else { "已到末页\0".as_bytes() })),
                (2, RowSpec::display(
                    core::slice::from_raw_parts(core::ptr::addr_of!(MPL_STATUS) as *const u8,
                        cstr_len(core::ptr::addr_of!(MPL_STATUS) as *const u8) + 1),
                    core::ptr::null())),
            ];
            let prev = build_rows(ctx, ctx.title, &specs)?;
            let label = fw_api::label_create(ctx.content);
            if label != 0 {
                fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
                fw_api::label_set_text(label, core::ptr::addr_of!(MPL_VIEW) as *const u8);
                fw_api::obj_set_size_wh(label, 320, 320);
                fw_api::obj_align_to(label, prev, fw_api::align::OUT_BOTTOM_MID, 0, 8);
            }
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            let pages = mpl_page_count();
            if idx == 0 && MPL_PAGE > 0 {
                MPL_PAGE -= 1;
                render_req(21);
            } else if idx == 1 && MPL_PAGE + 1 < pages {
                MPL_PAGE += 1;
                render_req(21);
            }
        }
    }
}

// ===== 页19: 设置 -> 恢复 fetch 支持 =====
pub(crate) struct FetchPage;
static PAGE_FETCHP: FetchPage = FetchPage;
static FETCH_HELP: &[u8] = "使用：\n1.前提条件：安装了autorun（如果没安装请在AstroBox上搜索或者加群1018886448获取autorun最新版本）\n2.开启方式：在Chaos的注入器表盘中点击注入设置->注册自启动，然后在autorun表盘中按照流程开启自启动，重启即可。\n3.注意事项：此功能只保证在小米运动健康中正常使用。\0".as_bytes();

impl Page for FetchPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "恢复fetch支持\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let label = fw_api::label_create(ctx.content);
            if label != 0 {
                fw_api::style_apply(label, crate::FW_STYLE_HR_NAME, 255, 0);
                fw_api::label_set_text(label, FETCH_HELP.as_ptr());
                // 说明页没有开关；完整文本超出首屏后由 content 原生滚动。
                fw_api::obj_set_size_wh(label, 320, 640);
                fw_api::obj_align(label, fw_api::align::TOP_MID, 0, 12);
            }
            Ok(())
        }
    }

    fn on_click(&self, _idx: usize) {}
}

// ===== 页22: 设置 -> 系统版本显示 =====
pub(crate) struct SystemVersionPage;
static PAGE_SYSVERP: SystemVersionPage = SystemVersionPage;

unsafe fn system_version_checks(rows: *mut u32) {
    let selected = crate::system_version::selected();
    for i in 0..4 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for SystemVersionPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统版本显示\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("3.101.043\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("4.103.022\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("91.78.13\0".as_bytes(), core::ptr::null())),
                (3, RowSpec::check("自定义\0".as_bytes(), crate::system_version::custom_display())),
            ])?;
            system_version_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 3 {
                crate::system_version::choose(idx);
                if page_is_live(crate::system_version::PAGE_PID as usize) {
                    system_version_checks(page_rows_ptr(crate::system_version::PAGE_PID as usize));
                }
            } else if idx == 3 {
                crate::system_version::edit_custom();
            }
        }
    }
}

// ===== 页24: 设置 -> 系统关于页面修改 =====
pub(crate) struct SystemAboutPage;
static PAGE_ABOUT_MODP: SystemAboutPage = SystemAboutPage;

impl Page for SystemAboutPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统关于页面修改\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable("系统版本显示\0".as_bytes())),
                (1, RowSpec::clickable("手环系统文本\0".as_bytes())),
                (2, RowSpec::clickable("设备名称\0".as_bytes())),
                (3, RowSpec::clickable("手环型号\0".as_bytes())),
            ]).map(|_| ())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => nav_goto(crate::system_version::PAGE_PID, 0),
                1 => nav_goto(crate::system_text::PAGE_PID, 0),
                2 => nav_goto(crate::device_name::PAGE_PID, 0),
                3 => nav_goto(crate::device_model::PAGE_PID, 0),
                _ => {}
            }
        }
    }
}

// ===== 页25: 系统关于页面修改 -> 手环系统文本 =====
pub(crate) struct SystemTextPage;
static PAGE_SYSTEXTP: SystemTextPage = SystemTextPage;

unsafe fn system_text_checks(rows: *mut u32) {
    let selected = crate::system_text::selected();
    for i in 0..3 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for SystemTextPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环系统文本\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("Xiaomi HyperOS\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("HarmonyOS\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("自定义\0".as_bytes(), crate::system_text::custom_display())),
            ])?;
            system_text_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 2 {
                crate::system_text::choose(idx);
                if page_is_live(crate::system_text::PAGE_PID as usize) {
                    system_text_checks(page_rows_ptr(crate::system_text::PAGE_PID as usize));
                }
            } else if idx == 2 {
                crate::system_text::edit_custom();
            }
        }
    }
}

// ===== 页26: 系统关于页面修改 -> 设备名称 =====
pub(crate) struct DeviceNamePage;
static PAGE_DEVNAMEP: DeviceNamePage = DeviceNamePage;

unsafe fn device_name_checks(rows: *mut u32) {
    let selected = crate::device_name::selected();
    for i in 0..=crate::device_name::preset_count() {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for DeviceNamePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设备名称\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 文本和数量都以 device_name::PRESETS 为准，新增预设后这里自动建行。
            let mut previous = ctx.title;
            for i in 0..crate::device_name::preset_count() {
                let row = build_row(ctx, previous, i, &RowSpec::check(
                    crate::device_name::preset_text(i), core::ptr::null()));
                if row == 0 { return Err(PageError::RowFailed(i)); }
                previous = row;
            }
            let custom = crate::device_name::preset_count();
            let row = build_row(ctx, previous, custom, &RowSpec::check(
                "自定义\0".as_bytes(), crate::device_name::custom_display()));
            if row == 0 { return Err(PageError::RowFailed(custom)); }
            device_name_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            let custom = crate::device_name::preset_count();
            if idx < custom {
                crate::device_name::choose(idx);
                if page_is_live(crate::device_name::PAGE_PID as usize) {
                    device_name_checks(page_rows_ptr(crate::device_name::PAGE_PID as usize));
                }
            } else if idx == custom {
                crate::device_name::edit_custom();
            }
        }
    }
}

// ===== 页27: 系统关于页面修改 -> 手环型号 =====
pub(crate) struct DeviceModelPage;
static PAGE_DEVMODELP: DeviceModelPage = DeviceModelPage;

unsafe fn device_model_checks(rows: *mut u32) {
    let selected = crate::device_model::selected();
    for i in 0..4 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for DeviceModelPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环型号\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("M2551B1\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("M2553B1\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("M9178B13\0".as_bytes(), core::ptr::null())),
                (3, RowSpec::check("自定义\0".as_bytes(), crate::device_model::custom_display())),
            ])?;
            device_model_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 3 {
                crate::device_model::choose(idx);
                if page_is_live(crate::device_model::PAGE_PID as usize) {
                    device_model_checks(page_rows_ptr(crate::device_model::PAGE_PID as usize));
                }
            } else if idx == 3 {
                crate::device_model::edit_custom();
            }
        }
    }
}

// ===== 页23: 通用英文数字键盘 =====
pub(crate) struct KeyboardPage;
static PAGE_KEYBOARDP: KeyboardPage = KeyboardPage;

impl Page for KeyboardPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, crate::keyboard::title()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if crate::keyboard::render(ctx.content) {
                Ok(())
            } else {
                Err(PageError::RowFailed(0))
            }
        }
    }

    fn on_click(&self, _idx: usize) {}
}

// ===== 页29: 应用管理 -> 应用详情(P0 占位) =====
/// 把以 NUL 结尾的静态缓冲当 &[u8] 用(RowSpec 吃切片, 不收裸指针)。
unsafe fn cstr_static(p: *const u8) -> &'static [u8] {
    let mut n = 0usize;
    while read_volatile(p.add(n)) != 0 { n += 1; }
    core::slice::from_raw_parts(p, n + 1)
}

pub(crate) struct AppDetailPage;
static PAGE_DETAILP: AppDetailPage = AppDetailPage;

static mut DT_LBL_SHOW: [u8; 24] = [0; 24];
static mut DT_LBL_HIDE: [u8; 24] = [0; 24];
static mut DT_LBL_UN: [u8; 32] = [0; 32];
static mut DT_LBL_UN2: [u8; 32] = [0; 32];

impl Page for AppDetailPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "应用详情\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            if !crate::appmgr::detail_valid() {
                let r = build_row(ctx, ctx.title, 0,
                    &RowSpec::tap("应用不存在, 返回\0".as_bytes(), core::ptr::null()));
                if r == 0 { return Err(PageError::RowFailed(0)); }
                return Ok(());
            }
            put_str(core::ptr::addr_of_mut!(DT_LBL_SHOW) as *mut u8, "显示该应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_HIDE) as *mut u8, "隐藏该应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_UN) as *mut u8, "卸载应用\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(DT_LBL_UN2) as *mut u8, "确认卸载?\0".as_bytes());

            let toggle: &[u8] = crate::appmgr::detail_action_label();
            let un: &[u8] = if crate::appmgr::detail_confirm() {
                cstr_static(core::ptr::addr_of!(DT_LBL_UN2) as *const u8)
            } else {
                cstr_static(core::ptr::addr_of!(DT_LBL_UN) as *const u8)
            };
            let r0 = build_row(ctx, ctx.title, 0,
                &RowSpec::display(crate::appmgr::detail_name(),
                    crate::appmgr::detail_pkg().as_ptr()));
            if r0 == 0 { return Err(PageError::RowFailed(0)); }
            fw_api::obj_set_size_wh(r0, 320, 88);
            fw_api::obj_align_to(r0, ctx.title, 14, 0, 10);
            cardify(r0);
            let r1 = build_row(ctx, r0, 1,
                &RowSpec::display(cstr_static(crate::appmgr::detail_fmt()),
                    core::ptr::null()));
            if r1 == 0 { return Err(PageError::RowFailed(1)); }
            fw_api::obj_set_size_wh(r1, 320, 56);
            fw_api::obj_align_to(r1, r0, 14, 0, 10);
            cardify(r1);
            // 隐藏/显示 | 排除/取消排除 并排, 卸载整宽一行
            let t2 = build_tile_below(ctx, r1, 2, toggle, TILE_W, TILE_H);
            if t2 == 0 { return Err(PageError::RowFailed(2)); }
            tile_primary(t2);
            let xt = crate::appmgr::detail_excl_label();
            let t3 = build_tile_right(ctx, t2, 3, xt, TILE_W, TILE_H);
            let t4 = build_tile_below(ctx, t2, 4, un, TILE_ROW_W, 56);
            tile_danger(t4);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if !crate::appmgr::detail_valid() {
                if idx == 0 { nav_back(); }
                return;
            }
            if idx == 2 {
                crate::appmgr::detail_toggle_hide();
                render_req(29);
            } else if idx == 3 {
                crate::appmgr::detail_toggle_excl();
                render_req(29);
            } else if idx == 4 {
                if crate::appmgr::detail_uninstall_click() {
                    // 已卸载: 回到应用管理列表
                    nav_back();
                    render_req(1);
                } else {
                    render_req(29);
                }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe { crate::appmgr::has_req() || crate::appmgr::excl_has_req() }
    }

    fn tick(&self) {
        unsafe {
            let mut dirty = false;
            if crate::appmgr::has_req() {
                crate::appmgr::do_op();
                dirty = true;
            }
            if crate::appmgr::excl_has_req() {
                crate::appmgr::do_excl();
                dirty = true;
            }
            if dirty {
                crate::appmgr::detail_refresh();
                render_req(29);
            }
        }
    }
}

// ===== 页30: 应用管理 -> 批量操作 =====
pub(crate) struct BatchPage;
static PAGE_BATCHP: BatchPage = BatchPage;

static mut B_SEL_TXT: [u8; 48] = [0; 48];
static mut B_ST_HID: [u8; 12] = [0; 12];
static mut B_ST_VIS: [u8; 12] = [0; 12];
static mut B_LBL_ALL: [u8; 32] = [0; 32];
static mut B_LBL_UNSEL: [u8; 32] = [0; 32];
static mut B_LBL_DEL: [u8; 32] = [0; 32];
static mut B_LBL_DEL2: [u8; 32] = [0; 32];
static mut B_COUNT_ROW: u32 = 0;

unsafe fn b_sel_text() -> *const u8 {
    let d = core::ptr::addr_of_mut!(B_SEL_TXT) as *mut u8;
    let mut w = W::new(d, 48);
    w.s("已选 ".as_bytes()).n(crate::appmgr::sel_count() as u32)
        .s(" / 共 ".as_bytes()).n(crate::appmgr::batch_total() as u32)
        .s("\0".as_bytes());
    w.end();
    d
}

/// 批量页的"已选 N / 共 M"行原地刷新
unsafe fn refresh_batch_count_row() {
    let r = core::ptr::read_volatile(core::ptr::addr_of!(B_COUNT_ROW));
    if r != 0 {
        fw_api::row_update(r, fw_api::NONE_PTR,
            cstr_static(b_sel_text()).as_ptr(), fw_api::NONE_PTR, 0, 0);
    }
}

impl Page for BatchPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "批量操作\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            crate::appmgr::rows_reset();
            put_str(core::ptr::addr_of_mut!(B_ST_HID) as *mut u8, "已隐藏\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_ST_VIS) as *mut u8, "显示中\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_ALL) as *mut u8, "全选\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_DEL) as *mut u8, "删除选中\0".as_bytes());
            put_str(core::ptr::addr_of_mut!(B_LBL_DEL2) as *mut u8, "确认删除?\0".as_bytes());
            let _ = &B_LBL_UNSEL;

            let all: &[u8] = if crate::appmgr::sel_count() == 0 {
                cstr_static(core::ptr::addr_of!(B_LBL_ALL) as *const u8)
            } else {
                // "取消全选"
                put_str(core::ptr::addr_of_mut!(B_LBL_UNSEL) as *mut u8, "取消全选\0".as_bytes());
                cstr_static(core::ptr::addr_of!(B_LBL_UNSEL) as *const u8)
            };
            let del: &[u8] = if crate::appmgr::sel_del_confirmed() {
                cstr_static(core::ptr::addr_of!(B_LBL_DEL2) as *const u8)
            } else {
                cstr_static(core::ptr::addr_of!(B_LBL_DEL) as *const u8)
            };
            // 顶部: 已选统计 + 四个操作按钮并排两行, 返回交给标题栏返回键
            let s0 = build_row(ctx, ctx.title, 0,
                &RowSpec::display(cstr_static(b_sel_text()), core::ptr::null()));
            if s0 == 0 { return Err(PageError::RowFailed(0)); }
            row_shrink(s0, 56);
            B_COUNT_ROW = s0;
            let t1 = build_tile_below(ctx, s0, 1, all, TILE_W, TILE_H);
            if t1 == 0 { return Err(PageError::RowFailed(1)); }
            let t2 = build_tile_right(ctx, t1, 2, "隐藏选中\0".as_bytes(), TILE_W, TILE_H);
            tile_primary(t2);
            let t3 = build_tile_below(ctx, t1, 3, "显示选中\0".as_bytes(), TILE_W, TILE_H);
            if t3 == 0 { return Err(PageError::RowFailed(3)); }
            let t4 = build_tile_right(ctx, t3, 4, del, TILE_W, TILE_H);
            tile_danger(t4);

            // 勾选行: 先建首屏, 其余由 tick 分批续建
            crate::appmgr::sort_view_visible_first();
            list_begin(2, t3, ctx.content,
                core::ptr::addr_of!(B_ST_VIS) as *const u8,
                core::ptr::addr_of!(B_ST_HID) as *const u8);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx == 1 {
                crate::appmgr::sel_toggle_all();
                if !refresh_list_rows(true) { render_req(30); }
                refresh_batch_count_row();
            } else if idx == 2 {
                crate::appmgr::batch_request_sel(true);
                render_req(30);
            } else if idx == 3 {
                crate::appmgr::batch_request_sel(false);
                render_req(30);
            } else if idx == 4 {
                if crate::appmgr::sel_del_confirmed() {
                    crate::appmgr::batch_request_del();
                } else {
                    crate::appmgr::sel_del_arm();
                }
                render_req(30);
            } else if idx == 5 {
                crate::appmgr::sel_del_disarm();
                nav_back();
            } else if idx >= 12 {
                let v = idx - 12;
                if v < crate::appmgr::view_count() {
                    let e = crate::appmgr::view_at(v);
                    if e != usize::MAX { crate::appmgr::sel_toggle(e); }
                    if !refresh_list_rows(true) { render_req(30); }
                    refresh_batch_count_row();
                }
            }
        }
    }

    fn needs_tick(&self) -> bool {
        unsafe {
            list_building() || crate::appmgr::batch_pending() || crate::appmgr::del_pending()
                || crate::appmgr::has_req()
        }
    }

    fn tick(&self) {
        unsafe {
            if list_building() {
                if LB_DELAY > 0 { LB_DELAY -= 1; }
                else { list_build_chunk(LB_TICK_CHUNK); }
            }
            if crate::appmgr::has_req() {
                crate::appmgr::do_op();
            } else if crate::appmgr::batch_pending() {
                crate::appmgr::do_batch();
            } else if crate::appmgr::del_pending() {
                crate::appmgr::do_del_batch();
            }
            // 同上: 勾选/状态变化只原地刷新, 不再自动整页重建
            let _ = refresh_list_rows(true);
            refresh_batch_count_row();
        }
    }
}

pub(crate) static PAGE_TABLE: [&dyn Page; PAGE_COUNT] = [
    &PAGE_INFO,       // 0
    &PAGE_APPMGR,     // 1  应用管理(Shell++ 移植; 文件管理入口已注释退役)
    &PAGE_TIMESET,      // 2  时间设置 / 日期第一步(年)(原目录 2 级, 文件管理退役后空出)
    &PAGE_DATE_MD,      // 3  日期设置(月日)(原目录 3 级, 一直不可达)
    &PAGE_BLURP,      // 4  系统美化 -> 系统模糊(原来是目录 4 级, 为腾独立 page_id 重分配)
    &PAGE_FONTLIST,   // 5  系统美化 -> 更换字体(字体清单 + 自由切换)
    &PAGE_ICONSUB,    // 6  系统美化 -> 桌面图标
    &PAGE_EXCL,       // 7  应用管理 -> 排除项管理(原文件查看退役)
    &PAGE_WF_LIST_8,  // 8
    &PAGE_WF_LIST_9,  // 9
    &PAGE_WF_MGMT,    // 10
    &PAGE_BRIGHT,     // 11
    &PAGE_CACHEP,     // 12
    &PAGE_BEAUTIFY,     // 13
    &PAGE_SETTINGS,     // 14  设置(时间/日期/亮度/震动/缓存/版本)
    &PAGE_ZONEP,        // 15 系统模糊 -> 模糊区域
    &PAGE_VIBP,         // 16 主页 -> 震动
    &PAGE_LOG,          // 17 设置 -> 运行日志
    &PAGE_ABOUT_PAGE,   // 18 主页 -> 关于
    &PAGE_FETCHP,       // 19 设置 -> 恢复 fetch 支持
    &PAGE_LICENSESP,    // 20 关于 -> 开放源代码许可
    &PAGE_LICENSE_MPLP, // 21 MIBAND10PRO-TUNMODULE / MPL-2.0
    &PAGE_SYSVERP,      // 22 系统关于页面修改 -> 系统版本显示
    &PAGE_KEYBOARDP,    // 23 通用英文数字键盘
    &PAGE_ABOUT_MODP,   // 24 设置 -> 系统关于页面修改
    &PAGE_SYSTEXTP,     // 25 系统关于页面修改 -> 手环系统文本
    &PAGE_DEVNAMEP,     // 26 系统关于页面修改 -> 设备名称
    &PAGE_DEVMODELP,    // 27 系统关于页面修改 -> 手环型号
    &PAGE_APPLIST,      // 28 系统美化 -> 应用列表设置(桌面缩放/排距/内缩)
    &PAGE_DETAILP,      // 29 应用管理 -> 应用详情(P0 占位)
    &PAGE_BATCHP,       // 30 应用管理 -> 批量操作(P0 占位)
];
