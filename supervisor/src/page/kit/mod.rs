//! 页面组件 kit: 声明式行模型 + 统一菜单 + 分组标题 + 勾选列表 + 滚动视口 + 二次确认 + 反馈。

pub(crate) mod row;
pub(crate) mod section;
pub(crate) mod menu;
pub(crate) mod checklist;
pub(crate) mod scroll;
pub(crate) mod confirm;
pub(crate) mod feedback;

pub(crate) use row::{RowKind, RowModel, RowSub, ToggleDef};
pub(crate) use menu::{menu_click, menu_is_switch, menu_render, menu_switch, MenuSpec};
