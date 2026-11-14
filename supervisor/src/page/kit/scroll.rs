//! 统一滚动视口: viewport(打开滚动、挡住手势链) + board(内容板, 高度随文本)。
//!
//! 此固件的 label 不会响应 LV_SIZE_CONTENT —— 文字超过对象高度时是直接裁掉
//! 而不是把对象撑开, 所以"文本 + 滚动"必须手算高度: viewport 打开一个方向的
//! 滚动, board 按行数换算像素高, label 铺在板上。
//! 使用方: 运行日志页(纵向) / NSH 终端(横向)。键盘页的外壳同形, 保留自管。

use crate::*;

/// 建一个滚动视口, 挂在 anchor 下方。`hor` = 横向滚动(终端) / false = 纵向(日志)。
/// 滚到头就停: 关掉链式滚动/手势冒泡/弹性, 不把拖动传给 content 的系统手势。
pub(crate) unsafe fn viewport(parent: u32, anchor: u32, w: i32, h: i32, hor: bool) -> u32 {
    let vp = fw_api::obj_create(parent, 0);
    if vp == 0 { return 0; }
    fw_api::obj_clear_box_spacing(vp);
    fw_api::obj_set_size(vp, w, h);
    if anchor != 0 {
        fw_api::obj_align_to(vp, anchor, fw_api::align::OUT_BOTTOM_MID, 0, T::SP_2);
    }
    fw_api::obj_style_color(vp, 0, 0);
    fw_api::obj_add_flag(vp, fw_api::LV_OBJ_FLAG_SCROLLABLE | fw_api::LV_OBJ_FLAG_CLICKABLE);
    fw_api::obj_set_scroll_dir(vp, if hor { fw_api::LV_DIR_HOR } else { fw_api::LV_DIR_VER });
    fw_api::obj_clear_flag(vp,
        fw_api::LV_OBJ_FLAG_SCROLL_CHAIN
            | fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE
            | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC);
    vp
}

/// 绝对定位的滚动视口(终端用): 摆在父容器的 (dx, dy) 处。
/// **不清滚动链** —— 终端是横向滚, 纵向拖动仍要交给页面 content(故意的分工),
/// 只关手势冒泡与弹性。
pub(crate) unsafe fn viewport_abs(parent: u32, dx: i32, dy: i32, w: i32, h: i32, hor: bool) -> u32 {
    let vp = fw_api::obj_create(parent, 0);
    if vp == 0 { return 0; }
    fw_api::obj_clear_box_spacing(vp);
    fw_api::obj_set_size(vp, w, h);
    fw_api::obj_align(vp, fw_api::align::TOP_LEFT, dx, dy);
    fw_api::obj_add_flag(vp, fw_api::LV_OBJ_FLAG_SCROLLABLE | fw_api::LV_OBJ_FLAG_CLICKABLE);
    fw_api::obj_set_scroll_dir(vp, if hor { fw_api::LV_DIR_HOR } else { fw_api::LV_DIR_VER });
    fw_api::obj_clear_flag(vp,
        fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC);
    vp
}

/// 视口里的内容板: 不自身滚动, 高度由调用方按文本行数给(至少撑满视口)。
pub(crate) unsafe fn board(vp: u32, w: i32, h: i32) -> u32 {
    let board = fw_api::obj_create(vp, 0);
    if board == 0 { return 0; }
    fw_api::obj_clear_box_spacing(board);
    fw_api::obj_clear_scrollable(board);
    fw_api::obj_set_size(board, w, h);
    fw_api::obj_align(board, fw_api::align::TOP_LEFT, 0, 0);
    fw_api::obj_style_color(board, 0, 0);
    board
}
