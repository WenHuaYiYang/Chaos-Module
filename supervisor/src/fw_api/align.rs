//! 原生对象对齐参数，由固件 UI 与主机回归测试共用。

pub const DEFAULT: u32 = 0;
pub const TOP_LEFT: u32 = 1;
pub const TOP_MID: u32 = 2;
pub const TOP_RIGHT: u32 = 3;
/// 在父对象内水平、垂直居中；键盘按键文字使用。
/// p67 / 3.101.043 静态核对：布局分派表 file 0x79360 的 9 分支到 0x794A2，
/// 分别取父/子宽高的一半计算 x/y；8 分支到 0x79466，x 用完整宽度差（靠右）。
pub const CENTER: u32 = 9;
pub const OUT_BOTTOM_LEFT: u32 = 13;
/// 放在基准下方并水平居中 行/文本纵向串链用的就是这个
pub const OUT_BOTTOM_MID: u32 = 14;
pub const OUT_BOTTOM_RIGHT: u32 = 15;
/// 放在基准右侧并垂直居中。滚轮并排用的就是这个值 —— 真机可正常渲染的那一版
/// 时/分两轮都靠它串成一行, 所以它是实测值不是手册换算值。
pub const OUT_RIGHT_MID: u32 = 19;
/// 系统页面给 `lvx_object_align` 传的字面值，配 `(对象, 7, x, 0)` 把控件按 x 摆在
/// 父容器里 —— 闹钟页的两个滚轮(60/190)与它们之间的冒号标签(160)用的都是这一个值。
/// 只照抄、不解释: 本仓库这套对齐常量与 LVGL 公开枚举对不上，按手册名反推已经错过一次。
pub const X_ANCHOR: u32 = 7;
/// 底部水平居中。系统闹钟页给确认按钮传的就是这个值，配 `(0, -16)` 表示
/// 水平居中、离父容器底边 16px。值取自闹钟页字面量，不是按手册换算。
pub const BOTTOM_MID: u32 = 5;
