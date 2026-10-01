//! 页面框架：列表行、内容容器、标题、跳转与返回（第 4、5 节）
//!
//! 本文件是 `fw_api` 的子模块，调用方式仍是 `fw_api::名字`（父模块已 `pub use` 转出）。

use core::mem::transmute;
use super::*;

// ===========================================================================
// 4. 行（stock list row）
// ===========================================================================

/// `lvx_list_row_create(parent, primary, secondary, trailing)` 已验证
///
/// - `secondary` 传 `NONE_PTR` = **单排**（紧凑）；传非 NULL（含空串）会建出副标签
/// - `trailing` 用 `trailing::*`
pub unsafe extern "C" fn row_create(parent: Obj, primary: Cp, secondary: Cp, trailing: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, Cp, Cp, u32) -> Obj = transmute(0x0C54_CA3Dusize);
    f(parent, primary, secondary, trailing)
}

/// `lvx_list_row_trailing(row)` 取行右侧控件（真的就是 `row[+0x50]`） 已验证
///
/// 开关行的事件要挂**它**上（固件自带的开关行就是这么挂的）。
pub unsafe extern "C" fn row_trailing(row: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0C4C_8C61usize);
    f(row)
}

/// `lvx_list_item_update(row, a1, primary, secondary, badge, selected)` 已验证
///
/// 参数归属（0x0C4C8904 反汇编实证，按"三个文本槽"理解是错的）：
/// - `a1` → `row+0x34`：`row_create` 从不创建这条标签（写它无效）
/// - `primary` → `row+0x3c`：**主标签**
/// - `secondary` → `row+0x40`：**副标签**（传 `NONE_PTR` 才不建；传空串会惰性创建出空行）
/// - `badge` → int32，`row+0x54`
/// - `selected` → trailing 选中态：非 0 → 选中（switch 为"开"、复选框为"勾选"）
///
/// **不用的文本槽一律传 `NONE_PTR`。**
pub unsafe extern "C" fn row_update(
    row: Obj,
    a1: Cp,
    primary: Cp,
    secondary: Cp,
    badge: i32,
    selected: u8,
) -> i32 {
    let f: unsafe extern "C" fn(Obj, Cp, Cp, Cp, i32, u8) -> i32 = transmute(0x0C4C_8905usize);
    f(row, a1, primary, secondary, badge, selected)
}

// ===========================================================================
// 5. 页面（固件 page 框架）
// ===========================================================================

/// `lvx_page_content_create(root)` 已验证
pub unsafe extern "C" fn content_create(root: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0CA6_7245usize);
    f(root)
}

/// `lvx_content_pad_bottom(content, pad, flags)` 已验证
pub unsafe extern "C" fn content_pad_bottom(content: Obj, pad: i32, flags: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, i32, u32) -> Obj = transmute(0x0C58_93C1usize);
    f(content, pad, flags)
}

/// `lvx_page_title_create(root, title, mode, cb, udata)` 已验证
///
/// - `mode = 1` → 带返回键；`cb = 0` 时固件装**默认返回回调（带动画）**
/// - 类固有尺寸 0x150 x 0x38(=56, 正是 content 的 y 偏移), 几何稳定
/// - 本函数体内 `str` 只写自己对象的字段与栈槽, **不写页描述符也不登记全局单例**
///   (对比消息框 0x0C4C9F78 会 `bl 0x0C5891A8` 登记) => 标题对象除了调用方的句柄没有
///   第二份引用, 类描述符的 ctor/析构都是 0 => 删除它只走 LVGL 通用删除, 可以安全重建。
///   重要: 页面重建时标题要**删掉再重建** —— 留着不删的话, 标题对象上的 local
///   `text_font` 会指向已经回收掉的字体对象, 整条文字不画, 滚动和重新套样式都救不回来。
pub unsafe extern "C" fn page_title_create(root: Obj, title: Cp, mode: u32, cb: Cb, udata: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, Cp, u32, Cb, u32) -> Obj = transmute(0x0C4C_A6C5usize);
    f(root, title, mode, cb, udata)
}

/// `page_goto(app_id<<16 | page_id, start_data, param, callbacks)` 带动画压栈 已验证
pub unsafe extern "C" fn page_goto(key: u32, start_data: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32, u32) -> i32 = transmute(0x0CA6_C359usize);
    let _ = f(key, start_data, 0, 0);
}

/// 系统动画返回上一级 已验证
///
/// 注：`page_finish(desc)` **无动画**，真机实测也不可用，统一用这个。
pub unsafe extern "C" fn page_back() {
    let f: unsafe extern "C" fn() -> i32 = transmute(0x0CA7_6FB5usize);
    let _ = f();
}



// ===========================================================================
// 4.1 数据行组件(照系统心率区间面板的行)
// ===========================================================================

/// 一行"数据行"的句柄: 行容器 + 名称标签 + 数值标签 + 进度条
#[derive(Clone, Copy)]
pub struct DataRow {
    pub row: Obj,
    pub name: Obj,
    pub value: Obj,
    pub bar: Obj,
}

impl DataRow {
    pub const EMPTY: DataRow = DataRow { row: 0, name: 0, value: 0, bar: 0 };
}

/// 数据行尺寸与行内位置(与心率区间面板一致: 条 296x20 摆在文字下方, 不压字)
pub const DATA_ROW_W: i32 = 328;
pub const DATA_ROW_H: i32 = 112;
const DR_PAD: i32 = 16;
const DR_NAME_Y: i32 = 8;
const DR_VALUE_Y: i32 = 40;
const DR_BAR_Y: i32 = 78;

/// 建一行"数据行"组件。
///
/// 复刻系统心率区间面板每行的做法(建行段 0x0C66288E 内 0x0C662A0C 起):
/// - 行容器用列表行(自带卡片与行距), 文字槽传空格/NONE_PTR, 不用它自带的标签;
/// - 名称标签: 调 `label_attr2(3)` 并套系统样式 `FW_STYLE_HR_NAME`;
///   注意 `0xC588420(lab,106)` 是宽度约束(系统只给两个字的区间名调), 名字更长会裁字, 故不调;
/// - 数值标签: 只套系统样式 `FW_STYLE_HR_VALUE`, 不调那两个属性(与系统一致);
/// - 进度条: 平台组件 296x20, 填充色由调用方给, 摆在文字下方。
pub unsafe fn data_row_create(parent: Obj, name: Cp, fill_rgb: u32, pct: u32) -> DataRow {
    let mut d = DataRow::EMPTY;
    let row = row_create(parent, b" \0".as_ptr(), NONE_PTR, 0);
    if row == 0 {
        return d;
    }
    obj_set_size_wh(row, DATA_ROW_W as u32, DATA_ROW_H as u32);
    let nl = label_create(row);
    if nl != 0 {
        label_attr2(nl, 3);
        style_apply(nl, crate::FW_STYLE_HR_NAME, 255, 0);
        label_set_text(nl, name);
        obj_align(nl, align::TOP_LEFT, DR_PAD, DR_NAME_Y);
    }
    let vl = label_create(row);
    if vl != 0 {
        style_apply(vl, crate::FW_STYLE_HR_VALUE, 255, 0);
        // 数值用常规行副标题同款灰(0xC588A70 是系统给标签设文字色的入口)
        obj_style_color_a70(vl, super::TEXT_GRAY_SUB, 0);
        obj_align(vl, align::TOP_LEFT, DR_PAD, DR_VALUE_Y);
    }
    let bar = bar_create_rgb(row, pct, fill_rgb);
    if bar != 0 {
        obj_align(bar, align::TOP_LEFT, DR_PAD, DR_BAR_Y);
    }
    d.row = row;
    d.name = nl;
    d.value = vl;
    d.bar = bar;
    d
}

/// 把数据行挂到某一行下方(与标准行同样的行距 8px)
pub unsafe fn data_row_place(d: &DataRow, prev: Obj) {
    if d.row != 0 {
        obj_align_to(d.row, prev, align::OUT_BOTTOM_MID, 0, 8);
    }
}
