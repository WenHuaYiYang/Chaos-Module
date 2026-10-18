//! 原生滚轮选择器 `lvx_zoom_picker`(系统闹钟时间选择同款)。
//!
//! 完整调用序列抄自固件闹钟页 `FUN_0048C1E4`(文件偏移 0x0048C1E4，运行地址 0x0C5481E4)：
//! 建控件 -> 挂样式 -> 那条 `(p, 2, 0)` 样式写入 -> 只设宽度 86 -> 3 行可见
//! -> 范围 0..23 / 0..59 步长 1 -> 初值 -> 循环模式 1 -> `align(7, x, 0)`。
//!
//! 选中值不另找 getter,直接读控件字段(静态核对):
//! - `+0x94` 可视区起点索引(u32)
//! - `+0xF8` 选中项相对起点的有符号偏移(s8)
//! - `+0x538` 范围项数(i32)
//! - `+0x545` 模式(u8, 1=循环)
//! 证据: `set_value`(0x0C9A7CB8 对应的固件函数 0x0CA67CB8)调
//! `FUN_0099C430(picker, value, total)`,后者把起点写 `+0x94`、把
//! 选中偏移写 `+0xF8`;`set_range`(0x0CA67A80)写 `+0x538`/`+0x53C`。
//! 循环模式把范围复制 5 份并对齐到中间一份,所以选中索引要减 `2 * 范围项数`。

use core::mem::transmute;
use super::*;

/// 滚轮那两支的入口按目标取自 `fwaddr`。默认目标的那两个值在这里留一份**带判据的**副本：
/// 发版门按文本核这两个 VA，而调用点引的是表 —— 一条编译期判据把两边钉成同一个值，
/// 谁改了其中一边而没改另一边，构建当场失败。
#[cfg(not(feature = "fw-p65"))]
const _: () = assert!(super::fwaddr::ZOOM_PICKER_CREATE == 0x0CA6_7835
                      && super::fwaddr::ZOOM_PICKER_SET_MODE == 0x0CA6_7851);

/// `lvx_zoom_picker_create(parent)`; 0 = 失败。
/// 入口按目标取（043 文件偏移 0x009A7834）
pub unsafe extern "C" fn zoom_picker_create(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(super::fwaddr::ZOOM_PICKER_CREATE as usize);
    f(parent)
}

/// 系统闹钟页给每个滚轮挂的样式对象（开机样式表里的条目，换系统字体时这条会被改写）。
/// 它自带的字体字号是 70，滚轮的行高与选中带尺寸都由它算。
pub const PICKER_STYLE: u32 = 0x2010_D300;

/// 冒号标签的字体取自这条样式（同一张表，字号 64）。
pub const COLON_FACE_STYLE: u32 = 0x2010_D2E8;

/// 被拨的那个轮用这个色，其余轮恢复白。
///
/// 依据: 闹钟页滚轮回调 `FUN_004985E4` 里成对出现的 `0x004C8A70` 调参 ——
/// 一个传 `0x277af7`、另一个传 `0xffffff`；`0x277af7` 与那条底部提示标签用的是同一个蓝。
pub const WHEEL_COLOR_ACTIVE: u32 = 0x27_7a_f7;
pub const WHEEL_COLOR_IDLE: u32 = 0xff_ff_ff;

/// 底部提示标签在拨"时/分"时显示的字段名消息号（固件 i18n 键，不是字面文本）。
///
/// 依据: 回调体里 `uVar3 = *DAT_00498748` 得到 `0x2cb8aa4c`("hour.common")、
/// `*DAT_0049874c` 得到 `0x2cb8aa74`("min.common"，存储在池里是 "cnt/min.common" 的尾部)，
/// 再交给翻译函数取当前语言的词。设备没有这两个键的年份/日期版本，所以日期页只能用
/// 自己的字面文本。
pub const HINT_KEY_HOUR: u32 = 0x2cB8_AA4C;
pub const HINT_KEY_MIN: u32 = 0x2cB8_AA74;

/// 把滚轮的每一项文字按对象当前的样式重挂一遍（改完颜色必须调它，否则颜色不落上去）
///
/// 依据: `FUN_009A7CDC` 反编译为遍历 `+0xC0` 个可视项，逐个 `label_set_text` +
/// 写样式属性 0x58，并把 `+0x18` 指向该项的标签串。闹钟回调对**每个**轮都调一次。
/// fw_addr: 0x009A7CDC  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn zoom_picker_refresh(p: Obj) {
    let f: unsafe extern "C" fn(Obj) = transmute(0x0CA6_7CDDusize);
    f(p)
}

/// 可见行数（闹钟页传 3）。
/// fw_addr: 0x009A7A08  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn zoom_picker_set_visible(p: Obj, rows: i32) {
    let f: unsafe extern "C" fn(Obj, i32) = transmute(0x0CA6_7A09usize);
    f(p, rows)
}

/// 范围 [min, max], 步长 1。
/// fw_addr: 0x009A7A80  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn zoom_picker_set_range(p: Obj, min: i32, max: i32, step: i32) {
    let f: unsafe extern "C" fn(Obj, i32, i32, i32) = transmute(0x0CA6_7A81usize);
    f(p, min, max, step)
}

/// 设初值。value 是范围内的偏移(0..项数-1), 不是标签值:
/// 闹钟页的时/分从 0 起, 所以看不出差别; 年/月/日要先减 min。
/// fw_addr: 0x009A7CB8  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn zoom_picker_set_value(p: Obj, value: i32) {
    let f: unsafe extern "C" fn(Obj, i32) = transmute(0x0CA6_7CB9usize);
    f(p, value)
}

/// 模式: 1 = 循环(闹钟页同款)。
/// 入口按目标取（043 文件偏移 0x009A7850）
pub unsafe extern "C" fn zoom_picker_set_mode(p: Obj, mode: i32) {
    let f: unsafe extern "C" fn(Obj, i32) = transmute(super::fwaddr::ZOOM_PICKER_SET_MODE as usize);
    f(p, mode)
}

/// 取当前选中项的文本指针(0 = 没找到)。
///
/// 这是闹钟确认回调 `0x0C5535AC` 真正用的取值入口: 它对两份 picker 都调
/// `FUN_009a7a18(picker)` 拿选中项文字, 再用 `atoi` 转数字; 不是读 +0x94/+0xF8
/// 那些滚动字段(那些字段只描述可视窗口, 不是逻辑值)。
/// fw_addr: 0x009A7A18  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn zoom_picker_text(p: Obj) -> *const u8 {
    let f: unsafe extern "C" fn(Obj) -> *const u8 = transmute(0x0CA6_7A19usize);
    f(p)
}

/// 读当前选中值(解析选中项文字的前导数字)。
/// 返回 -1 表示没取到文字/不是数字。标签是**实际显示值**(年 2026、月 10、时 21),
/// 不需要再按范围偏移或循环份数换算。
pub unsafe fn zoom_picker_value(p: Obj) -> i32 {
    let s = zoom_picker_text(p);
    if s.is_null() {
        return -1;
    }
    let mut v: i32 = 0;
    let mut any = false;
    let mut i = 0usize;
    loop {
        let c = core::ptr::read_volatile(s.add(i));
        if c >= b'0' && c <= b'9' {
            v = v.saturating_mul(10).saturating_add((c - b'0') as i32);
            any = true;
            i += 1;
        } else {
            break;
        }
    }
    if any { v } else { -1 }
}
