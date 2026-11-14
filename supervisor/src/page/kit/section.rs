//! 分组标题与长说明: 都是普通文本控件, **不占 12 行槽位**。
//!
//! 为什么不用行控件当标题: 行槽只有 12 个, 标题吃掉一格就把"静态菜单页 <= 11 行"
//! 的预算挤爆; 而且行控件自带 trailing 部件与点击语义, 标题都不需要。
//!
//! 顺序与尺寸照抄真机验证过的长文本 label 样板(许可正文/美化页说明):
//! create -> style_apply -> **先限宽** -> set_text -> align_to。
//! 重要: 不要对这类 label 调 no_scroll —— 那是行控件"文字超长时裁剪、不让整行
//! 跟手划动"的语义, 套在 label 上会按创建时刻的旧尺寸裁字(真机: 分组标题与
//! 说明的字形边缘被截)。label 自己不跟手, 不需要它。

use crate::*;

/// 分组标题: 副色小字, 挂在前一个对象下方。
pub(crate) unsafe fn section_label(content: u32, prev: u32, text: &[u8]) -> u32 {
    let lb = fw_api::label_create(content);
    if lb == 0 { return 0; }
    fw_api::style_apply(lb, crate::FW_STYLE_HR_NAME, 255, 0);
    fw_api::obj_set_width(lb, 320);
    fw_api::label_set_text(lb, text.as_ptr());
    fw_api::obj_style_color_a70(lb, T::SECONDARY, 0);
    fw_api::obj_align_to(lb, prev, fw_api::align::OUT_BOTTOM_MID, 0, T::SP_3);
    lb
}

/// 长说明: 全宽 label, 高度随换行增长, 挂在原生可滚动的 content 上。
pub(crate) unsafe fn note_label(content: u32, prev: u32, text: &[u8]) -> u32 {
    let lb = fw_api::label_create(content);
    if lb == 0 { return 0; }
    fw_api::style_apply(lb, crate::FW_STYLE_HR_NAME, 255, 0);
    fw_api::obj_set_width(lb, 320);
    fw_api::label_set_text(lb, text.as_ptr());
    fw_api::obj_align_to(lb, prev, fw_api::align::OUT_BOTTOM_MID, 0, T::SP_2);
    lb
}
