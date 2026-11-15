//! MenuPage: 页面实现 MenuSpec(给出 &'static [RowModel] 与首槽位),
//! render/点击/开关分发全部委托给 kit —— 页面里不再手写 build_rows + match 长链。

use crate::*;
use super::row::{clickable, is_toggle, takes_slot, RowKind, RowModel, RowSub, ToggleDef};
use super::section::{note_label, section_label};

/// 菜单页规格。实现方只提供数据; 槽位预算: 占槽行数 + first_slot 必须 <= 11。
pub(crate) trait MenuSpec {
    /// 行模型(Section/Note 不占槽)
    fn rows(&self) -> &'static [RowModel];
    /// 第一个行槽号(默认 0; 主页这类"上面有自绘状态区"的页给 3)
    fn first_slot(&self) -> usize { 0 }
}

/// 副标签取值: 建行用。
unsafe fn sub_ptr(sub: &RowSub) -> *const u8 {
    match sub {
        RowSub::None => core::ptr::null(),
        RowSub::Static(s) => s.as_ptr(),
        RowSub::Dyn(f) => (f)(),
    }
}

/// 建整页菜单: 分组标题 -> 行链(开关行建完即补写初态) -> 说明。
/// 返回最后一个对象(供调用方接自绘内容或对齐检查)。
pub(crate) unsafe fn menu_render<T: MenuSpec>(
    spec: &T, ctx: &PageCtx, anchor: u32,
) -> PageResult {
    let mut prev = anchor;
    let mut slot = spec.first_slot();
    for m in spec.rows() {
        match &m.kind {
            RowKind::Section => {
                let lb = section_label(ctx.content, prev, m.label);
                if lb != 0 { prev = lb; }
            }
            RowKind::Note => {
                let lb = note_label(ctx.content, prev, m.label);
                if lb != 0 { prev = lb; }
            }
            _ => {
                let row_spec = RowSpec {
                    text: m.label,
                    sub: sub_ptr(&m.sub),
                    trailing: m.trailing,
                    clickable: clickable(m),
                };
                let r = build_row(ctx, prev, slot, &row_spec);
                if r == 0 { return Err(PageError::RowFailed(slot)); }
                if let RowKind::Toggle(d) = &m.kind {
                    // 开关初态原地补写(与旧页面建行后的做法一致: 只写勾选位)
                    let sp = match d.status {
                        Some(f) => (f)(),
                        None => fw_api::NONE_PTR,
                    };
                    fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR, sp, 0,
                                       (d.get)() as u8);
                } else if m.trailing == fw_api::trailing::CHECKBOX
                    || m.trailing == fw_api::trailing::RADIO {
                    // 勾选/单选行的初始态: 没有专门的 Choice 定义, 由 refresh 钩子给。
                    if let Some(f) = m.refresh { (f)(slot); }
                }
                prev = r;
                slot += 1;
            }
        }
    }
    Ok(())
}

/// 槽号 -> (行模型, 槽号)。派发用。
fn spec_row<T: MenuSpec>(spec: &T, idx: usize) -> Option<(&'static RowModel, usize)> {
    let rows = spec.rows();
    let first = spec.first_slot();
    if idx < first { return None; }
    let mut s = first;
    for m in rows {
        if !takes_slot(m) { continue; }
        if s == idx { return Some((m, s)); }
        s += 1;
    }
    None
}

/// 开关行原地刷新: 只写勾选位与状态副文案(值从 ToggleDef::get 现读)。
unsafe fn refresh_toggle(slot: usize, d: &ToggleDef) {
    let pid = st_rd!(FG_PAGE) as usize;
    if pid > MAX_PID { return; }
    let r = read_volatile(page_rows_ptr(pid).add(slot));
    if r == 0 { return; }
    let sp = match d.status { Some(f) => (f)(), None => fw_api::NONE_PTR };
    fw_api::row_update(r, fw_api::NONE_PTR, fw_api::NONE_PTR, sp, 0, (d.get)() as u8);
}

/// 点击派发: Nav 跳页 / Fn 调用 / Toggle 翻转并原地刷新; 之后走行的刷新钩子。
pub(crate) unsafe fn menu_click<T: MenuSpec>(spec: &T, idx: usize) {
    if let Some((m, slot)) = spec_row(spec, idx) {
        match &m.kind {
            RowKind::Nav(pid) => nav_goto(*pid, 0),
            RowKind::Fn(f) => (f)(),
            RowKind::Toggle(d) => {
                (d.toggle)();
                refresh_toggle(slot, d);
            }
            _ => {}
        }
        if let Some(f) = m.refresh { (f)(slot); }
    }
}

/// 开关翻转派发(VALUE_CHANGED)。
pub(crate) unsafe fn menu_switch<T: MenuSpec>(spec: &T, idx: usize) {
    if let Some((m, slot)) = spec_row(spec, idx) {
        if let RowKind::Toggle(d) = &m.kind {
            (d.toggle)();
            refresh_toggle(slot, d);
        }
    }
}

/// 该槽是否开关行(事件过滤)。
pub(crate) fn menu_is_switch<T: MenuSpec>(spec: &T, idx: usize) -> bool {
    match spec_row(spec, idx) {
        Some((m, _)) => is_toggle(m),
        None => false,
    }
}
