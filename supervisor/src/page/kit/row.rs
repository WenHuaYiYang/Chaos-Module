//! 声明式行模型: 页面只提供一份 `&'static [RowModel]`, 建行/绑定/派发由 kit 统一做。
//!
//! 行槽位只有 12 个, 分组标题与长说明**不占槽**(它们是普通 label, 见 section.rs);
//! 真正占槽的行按清单顺序从 MenuSpec::first_slot 起分配。静态菜单页的可见行
//! 数量必须 <= 11(留 1 槽余量), 超了就说明该页该走 LongList(窗口+更多)而不是菜单。

use crate::*;

/// 行副标签: 无 / 静态串 / 动态取值(返回 NUL 结尾 C 串指针)。
/// 注意: RowSub::None 建行时传 NULL —— "没有副标签"与"空串副标签"是两回事
/// (空串会触发惰性建标签, 界面多一行空白)。
pub(crate) enum RowSub {
    None,
    Static(&'static [u8]),
    Dyn(unsafe fn() -> *const u8),
}

pub(crate) enum RowKind {
    /// 分组标题: 纯 label, 不占行槽(文本在 RowModel::label)
    Section,
    /// 长说明: 纯 label, 全宽, 高度随换行增长, 不占行槽(文本在 RowModel::label)
    Note,
    /// 导航: page_goto 到目标页(Pid::X as u32)
    Nav(u32),
    /// 自定义动作: 静态 fn(用于"先做准备再跳转"这类入口, 如时间/日期编辑)
    Fn(unsafe fn()),
    /// 开关行: 翻转与持久化都在 ToggleDef 的实现里
    Toggle(&'static ToggleDef),
    /// 不可点的展示行(状态/信息)
    Display,
}

/// 开关行定义: 读态 / 翻转 / 可选的状态副文案。
pub(crate) struct ToggleDef {
    /// 读当前态(非 false = 开)
    pub get: unsafe fn() -> bool,
    /// 翻转(点击回调上下文里直接调; 耗时的持久化自己拆拍)
    pub toggle: unsafe fn(),
    /// 开关的状态副文案(可无)
    pub status: Option<unsafe fn() -> *const u8>,
}

pub(crate) struct RowModel {
    pub label: &'static [u8],
    pub sub: RowSub,
    pub kind: RowKind,
    /// 行右端部件(0=无; 取值与 fw_api::trailing 一致)
    pub trailing: u32,
    /// 点击/开关后的原地刷新钩子(可选): 从行句柄现取, 重画勾选态或状态文字。
    /// 勾选行(系统背景这类"点一下切回官方背景")靠它补写勾选位。
    pub refresh: Option<unsafe fn(slot: usize)>,
}

impl RowModel {
    pub(crate) const fn nav(label: &'static [u8], pid: u32) -> Self {
        RowModel { label, sub: RowSub::None, kind: RowKind::Nav(pid), trailing: 0, refresh: None }
    }
    pub(crate) const fn section(text: &'static [u8]) -> Self {
        RowModel { label: text, sub: RowSub::None, kind: RowKind::Section, trailing: 0, refresh: None }
    }
    pub(crate) const fn note(text: &'static [u8]) -> Self {
        RowModel { label: text, sub: RowSub::None, kind: RowKind::Note, trailing: 0, refresh: None }
    }
    pub(crate) const fn toggle(label: &'static [u8], def: &'static ToggleDef) -> Self {
        RowModel { label, sub: RowSub::None, kind: RowKind::Toggle(def),
                   trailing: fw_api::trailing::SWITCH, refresh: None }
    }
    pub(crate) const fn display(label: &'static [u8]) -> Self {
        RowModel { label, sub: RowSub::None, kind: RowKind::Display, trailing: 0, refresh: None }
    }
    /// 带刷新钩子的变体(勾选行/带状态文字的行)。
    pub(crate) const fn action(label: &'static [u8], f: unsafe fn(),
                               trailing: u32, refresh: Option<unsafe fn(slot: usize)>) -> Self {
        RowModel { label, sub: RowSub::None, kind: RowKind::Fn(f), trailing, refresh }
    }
    pub(crate) const fn display_sub(label: &'static [u8], sub: RowSub) -> Self {
        RowModel { label, sub, kind: RowKind::Display, trailing: 0, refresh: None }
    }
}

/// 该行是否占一个行槽(Section/Note 不占)
pub(crate) fn takes_slot(m: &RowModel) -> bool {
    !matches!(m.kind, RowKind::Section | RowKind::Note)
}

/// 该行是否可点
pub(crate) fn clickable(m: &RowModel) -> bool {
    matches!(m.kind, RowKind::Nav(_) | RowKind::Fn(_) | RowKind::Toggle(_))
}

/// 该行是否是开关行(事件过滤用)
pub(crate) fn is_toggle(m: &RowModel) -> bool {
    matches!(m.kind, RowKind::Toggle(_))
}

