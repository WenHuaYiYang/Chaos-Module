//! fw_api 已验证固件 API 封装层（小米手环 10 Pro / 3.101.043）
//!
//! ## 为什么要有这一层
//!
//! 「裸地址 + 手写调用」最容易出错，且集中在四类：
//!
//! 1. **空串指针 ≠ NULL** `lvx_list_item_update` 判空看的是**指针是否为 NULL**；
//! 传 `b"\0".as_ptr()`（指向空串的有效指针）会触发**副标签惰性创建**，凭空多出一行空白。
//! → 本层用 `Option`-风格的显式 `NULL` 常量，并在注释里写死这条规则。
//! 2. **槽号 → 回调的 catch-all 映射** 布局一改就静默错配（"返回"被派发成"更多"）。
//! → 本层提供 `ev::*` 常量并要求显式列出槽号（回调宏 `chaos_row_cb!` 在 crate 根定义）。
//! 3. **容量变更漏改清理循环** 槽数组 10→12 而清理仍写 `< 10`，留下悬空句柄 → UAF。
//! → 本层集中定义容量常量 `MAX_ROWS_PER_PAGE` 等，清理循环一律引用它。
//! 4. **新增页面漏改某个 pid 上界** `on_create` / `on_resume` / `on_destroy` / `dispatch`
//! 都要同步。→ 见 `MAX_PAGE_ID`。
//!
//! ## 使用纪律（务必遵守）
//!
//! - **新界面一律走本层**，不要再在业务代码里写 `transmute(0x0C...)`。
//! - 本文件任何地址改动，都要重新对固件做核对，并在注释里写清来源与验证程度。
//! - 注释里标注验证程度，由强到弱：`已验证` > `实测` > `静态核对` > `推断待验`。
//! - 调用前先确认上下文（哪个线程 / 是否在事件派发中 / 是否需要 UI 线程）。
//! - **函数指针地址必须 bit0=1**（Thumb）：符号查找与反汇编给出的都是偶数入口地址
//!   （反汇编要用偶数），直接当函数指针调用会硬 fault。对比：
//!   `obj_set_size=0x0C588629` / `row_create=0x0C54CA3D` 均 bit0=1。
//!
//! 本层的地址与偏移都来自对固件镜像的静态核对。

// 本层是固件 API 的**镜像面**: 描述符结构体的字段、`align::`/`trailing::`/`ev::` 这类
// 常量表, 成员即使当前没人调用也要整组留着(它们是查表用的对照面, 删成员比删函数更容易
// 造成"半个表"的误导)。所以这里允许 dead_code; **业务代码里不要照抄这一行**。
#![allow(dead_code)]

/// LVGL 对象句柄（固件里就是指针）
pub type Obj = u32;
/// 回调地址（Thumb 函数指针，bit0 = 1）
pub type Cb = u32;
/// C 字符串指针
pub type Cp = *const u8;
/// 表达"该参数不传"的显式空指针 **不要再写 `b"\0".as_ptr()`**
pub const NONE_PTR: Cp = core::ptr::null();

// ===========================================================================
// 1. 常量 / 枚举
// ===========================================================================

/// 页面数上限（page_id 0..=MAX_PAGE_ID）。新增页必须同步：
/// `DESC` 容量、页名数组、注册循环、`install(meta, items, count)` 的 items 与 count、
/// `render_page` / `dispatch` / `chaos_on_create` / `chaos_on_resume` / `chaos_on_destroy` 的 pid 上界。
pub const MAX_PAGE_ID: usize = 9;
/// 每页行槽上限（`PAGES[pid].rows[slot]`）。**清理循环必须用它**，别再写死数字
/// （根因示例：槽位扩到 12 而销毁仍清 0..9 → 悬空句柄 → use-after-free）。
pub const MAX_ROWS_PER_PAGE: usize = crate::state::MAX_ROWS_PER_PAGE;

/// 对齐枚举（LVGL v9 `lv_align_t` 的取值，与固件自带界面用的常量同值）
pub mod align {
    pub const DEFAULT: u32 = 0;
    pub const TOP_LEFT: u32 = 1;
    pub const TOP_MID: u32 = 2;
    pub const TOP_RIGHT: u32 = 3;
    pub const OUT_BOTTOM_LEFT: u32 = 13;
    /// 放在基准下方并水平居中 行/文本纵向串链用的就是这个
    pub const OUT_BOTTOM_MID: u32 = 14;
    pub const OUT_BOTTOM_RIGHT: u32 = 15;
}

/// 事件码（与固件自带界面的 `EVENT_*` 常量同值）
pub mod ev {
    /// 注册过滤器：接收全部事件
    pub const ALL: u32 = 0;
    /// 点击（行/按钮）
    pub const CLICKED: u32 = 7;
    /// 值改变（switch 翻转走这个）
    pub const VALUE_CHANGED: u32 = 30;
}

/// 行右侧控件（trailing）类型 `lvx_list_row_create` 第 4 参
///
/// 依据：`row_set_trailing` 0x0C4C8208 的 `tbb` 跳转表 @0x0C4C8220
/// （表字节 `4a4e 600d 0d77 8e97 9c0d 0da5 0724`），并逐分支核对图标资源串。
pub mod trailing {
    pub const NONE: u32 = 0;
    /// 开关（固件同名常量 `TRAILING_SWITCH`；挂 0xCA5FEE8 自绘 widget）
    pub const SWITCH: u32 = 1;
    /// 复选框 `/resource/app/common/icon/check_btn.bin`
    pub const CHECKBOX: u32 = 2;
    /// 右箭头 `forward.bin`
    pub const FORWARD: u32 = 3;
    /// 单选圆点(30×30) `radio_btn.bin`
    pub const RADIO: u32 = 6;
    /// 齿轮 `setting.bin`
    pub const SETTING: u32 = 7;
    /// 说明 `instruction.bin`
    pub const INSTRUCTION: u32 = 9;
    /// 单选未选 `radio_next.bin`
    pub const RADIO_NEXT: u32 = 14;
    // 4/5/10/11 = 无; 8 = 空对象
}

/// NuttX open(2) 标志 **不是 POSIX**：只读是 1 而不是 0
pub mod oflag {
    pub const RDONLY: u32 = 1;
    pub const WRONLY: u32 = 2;
    pub const RDWR: u32 = 3;
    pub const CREAT: u32 = 4;
}

// NuttX dirent.d_type 的取值定义在 state.rs(DT_UNKNOWN/DT_DIR/DT_REG), 这里不再重复定义

/// `quick_guesture` 手势码（payload+8 u16）
///
/// 依据：系统 handler 0x0C4BD0FC 按码分流（0x1C2 → 分支打印 `TYPE_GUESTURE_SHAKE`，
/// 0x1C3 → `TYPE_GUESTURE_WRIST`）。
pub mod gesture {
    /// 摇一摇 我们要的
    pub const SHAKE: u32 = 0x1C2;
    /// 抬腕亮屏 必须丢弃（否则误触发）
    pub const WRIST: u32 = 0x1C3;
}

/// 表盘节点 `type`（= watchface 目录数组的索引，0x2013FE4C 被 `[node+0x88]` 索引）
pub mod wf_type {
    /// `/data/app/watchface/builtin/` 系统内置
    pub const BUILTIN: u8 = 0;
    /// `/data/app/watchface/market/` 市场/第三方
    pub const MARKET: u8 = 1;
    /// `/data/app/watchface/builtin/aod/` 息屏（固件自身找"当前面"时跳过）
    pub const AOD: u8 = 2;
}

/// 表盘链表节点字段偏移（`watchface_list.json` 解析器 0xCA7DE18 键↔偏移实证）
pub mod wf_node {
    pub const ID: u32 = 0x08;
    pub const NEXT: u32 = 0x04;
    pub const NAME: u32 = 0x48;
    pub const TYPE: u32 = 0x88;
    pub const VERSION: u32 = 0x8C;
    pub const IN_USE: u32 = 0x90;
    pub const IS_DELETE: u32 = 0x91;
    pub const EDITABLE: u32 = 0x92;
    pub const SUPPORT_ALBUM: u32 = 0x93;
    pub const SUPPORT_AOD: u32 = 0x94;
    pub const SUPPORT_VIDEO: u32 = 0x99;
    /// name_translation：条数 + 数组（每条 8B：`[0]`=language u8, `[4]`=char* 字符串）
    pub const NAME_TR_CNT: u32 = 0xB4;
    pub const NAME_TR_ARR: u32 = 0xB8;
}

/// watchface manager 全局（SRAM 指针字）
pub const WF_MGR_PTR: u32 = 0x2013_FE6C;
/// 引擎"当前面"节点指针字（**切换表盘后只有它变**，set_by_id 不更新它）
pub const WF_ENGINE_CUR: u32 = 0x2011_9770;

/// `persist.switch_guestre_state` —— 固件自带摇一摇功能的开关
pub static KEY_SWITCH_GUESTRE: [u8; 29] = *b"persist.switch_guestre_state\0";



/// 事件总线派发进来的 event 结构：`[+4]`=topic desc，`[+0x18]`=payload 指针，`[+0x1C]`=len
pub mod eventbus_event {
    pub const TOPIC_DESC: u32 = 0x04;
    pub const PAYLOAD: u32 = 0x18;
    pub const LEN: u32 = 0x1C;
}

// ===========================================================================
// 子模块（按职责拆分；对外仍是 `fw_api::名字`, 调用点无需改动）
// ===========================================================================

mod event;
mod fs;
mod gfx;
mod page;
mod sys;

pub use event::*;
pub use fs::*;
pub use gfx::*;
pub use page::*;
pub use sys::*;
