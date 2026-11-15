//! 勾选/单选列表: 一份 CheckSpec(动态选项 + 选中判据 + 点选动作)替代每页一份的
//! "建复选框行 + 逐行补写勾选 + 点击里手写选中"样板。
//!
//! 与静态菜单(kit/menu.rs)的区别: 选项是运行期算的(条数/文字/选中态都来自 spec),
//! 所以行模型不是 &'static [RowModel], 而是一个按行号回调的 trait。
//! 行右端部件由 spec 给(复选框 CHECKBOX / 单选 RADIO)。
//!
//! 事件语义与旧页面一致: 行本体只认 CLICKED; 复选框 trailing 发出的 VALUE_CHANGED
//! 在派发层被丢掉(点圈上也算点这一行 —— build_row 已给 trailing 挂了 CLICKED)。

use crate::*;

pub(crate) trait CheckSpec {
    /// 选项数(占槽 first_slot..first_slot+count-1; 必须 <= 11 个槽)
    fn count(&self) -> usize;
    /// 选项 i 的主标签(C 串指针)
    unsafe fn label(&self, i: usize) -> *const u8;
    /// 选项 i 的副标签(没有就返回 null; null 不会触发惰性建标签)
    unsafe fn sub(&self, _i: usize) -> *const u8 { core::ptr::null() }
    /// 行右端部件(CHECKBOX=2 / RADIO=6)
    fn trailing(&self) -> u32 { fw_api::trailing::CHECKBOX }
    /// 选项 i 当前是否选中
    unsafe fn selected(&self, i: usize) -> bool;
    /// 点选第 i 项(回调上下文; 轻操作 + 持久化, 长活交给页面 tick)
    unsafe fn choose(&self, i: usize);
    /// 第一个行槽号(默认 0)
    fn first_slot(&self) -> usize { 0 }
}

/// 建整份列表 + 逐行补写勾选态。返回最后一个对象(供页面接说明行/状态行)。
pub(crate) unsafe fn render(spec: &dyn CheckSpec, ctx: &PageCtx) -> Result<u32, PageError> {
    let mut prev = ctx.title;
    let mut slot = spec.first_slot();
    let mut i = 0usize;
    while i < spec.count() {
        let lp = spec.label(i);
        let text = core::slice::from_raw_parts(lp, cstr_len(lp) + 1);
        let row_spec = RowSpec { text, sub: spec.sub(i), trailing: spec.trailing(),
                                 clickable: true };
        let r = build_row(ctx, prev, slot, &row_spec);
        if r == 0 { return Err(PageError::RowFailed(slot)); }
        fw_api::row_update(r, core::ptr::null(), core::ptr::null(), core::ptr::null(), 0,
                           spec.selected(i) as u8);
        // 单选部件(RADIO)与复选框不同: build_row 只给 CHECKBOX 的 trailing 挂事件,
        // RADIO 的圈要在这里补挂 CLICKED(点圈上也算点这一行) —— 与旧页面做法一致。
        if spec.trailing() == fw_api::trailing::RADIO {
            let t = fw_api::row_trailing(r);
            if t != 0 { fw_api::obj_add_event(t, row_cb(), 7, slot as u32); }
        }
        prev = r;
        slot += 1;
        i += 1;
    }
    Ok(prev)
}

/// 行槽号 -> 选项号。
fn spec_index(spec: &dyn CheckSpec, idx: usize) -> Option<usize> {
    let first = spec.first_slot();
    if idx < first { return None; }
    let i = idx - first;
    if i < spec.count() { Some(i) } else { None }
}

/// 点击派发: 点选 + 全列表勾选态原地刷新(不重建整页)。
pub(crate) unsafe fn click(spec: &dyn CheckSpec, pid: u32, idx: usize) {
    if let Some(i) = spec_index(spec, idx) {
        spec.choose(i);
        refresh_all(spec, pid);
    }
}

/// 全列表勾选态原地刷新(选中态变了才写 —— row_update 每次都会失效重绘)。
pub(crate) unsafe fn refresh_all(spec: &dyn CheckSpec, pid: u32) {
    let pid = pid as usize;
    if pid > MAX_PID { return; }
    let rows = page_rows_ptr(pid);
    let mut i = 0usize;
    while i < spec.count() {
        let r = read_volatile(rows.add(spec.first_slot() + i));
        if r != 0 {
            // 勾选位写法与旧页面相同: 只动第 6 参, 文字/副标签都传"保持"。
            fw_api::row_update(r, core::ptr::null(), core::ptr::null(), core::ptr::null(),
                               0, spec.selected(i) as u8);
        }
        i += 1;
    }
}

