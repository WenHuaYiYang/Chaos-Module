//! 统一页面骨架: 分组标题 -> 行链 -> 说明/状态 -> 底部安全区。
//!
//! 内容几何(content 尺寸/位置/底部留白)在 ui.rs render_page 按 `Page::geometry()`
//! 先建好; 这里只负责把 MenuSpec 的行模型装进去。所有走骨架的页面不再自建
//! "返回"行 —— 返回主路径是标题栏返回键(title_mode 默认 1)。

use crate::*;
use crate::page::kit::{menu_render, MenuSpec};

/// 装配一页菜单。`anchor` 是行链的第一个锚点(默认传 ctx.title;
/// 主页这类上面有自绘状态区的页传状态区最后一个对象)。
pub(crate) unsafe fn build<T: MenuSpec>(
    spec: &T, ctx: &PageCtx, anchor: u32,
) -> PageResult {
    menu_render(spec, ctx, anchor)
}
