//! 图形：LVGL 对象、文本与样式（第 2、3 节）
//!
//! 本文件是 `fw_api` 的子模块，调用方式仍是 `fw_api::名字`（父模块已 `pub use` 转出）。

use core::mem::transmute;
use super::*;

// ===========================================================================
// 2. LVGL 对象（thunk → 段B）
// ===========================================================================

/// `lv_obj_set_size(obj, w, h)` 已验证
/// fw_addr: 0x004C8628  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_size(obj: Obj, w: i32, h: i32) -> Obj {
    let f: unsafe extern "C" fn(Obj, i32, i32) -> Obj = transmute(0x0C58_8629usize);
    f(obj, w, h)
}


/// `lvx_confirm_btn_create(parent)` 系统闹钟页底部那个确认按钮的工厂
///
/// 依据: `FUN_0049885e` 建它之后 `set_size(0xa0, 0x60)`、
/// `lvx_object_align(btn, 5, 0, -16)`、`btn_set_src(btn, "/resource/app/common/icon/confirm.bin")`、
/// `obj_add_event(btn, 回调, 7, 0)`。工厂本体是
/// `FUN_00988474`: `lv_obj_create(lvx 按钮类, parent)` 再清一个标志, 与 `zoom_picker_create`
/// 同形状; 全镜像有二十多处调用它, 是通用控件不是闹钟私有。
/// fw_addr: 0x00988474  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn confirm_btn_create(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0CA4_8475usize);
    f(parent)
}

/// `btn_set_src(btn, path)` 把同一张图设进 0 / 0x20 / 0x80 三个状态
///
/// 依据: `FUN_009884EC` 反编译为对 `0x00A01258(obj, path, state)` 的三连调。
/// fw_addr: 0x009884EC  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn btn_set_src(btn: Obj, path: Cp) {
    let f: unsafe extern "C" fn(Obj, Cp) = transmute(0x0CA4_84EDusize);
    f(btn, path)
}

/// `lvx_object_align(obj, align, x, y)` 在**父对象内**对齐 已验证
/// fw_addr: 0x004C9188  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_align(obj: Obj, align: u32, x: i32, y: i32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, i32, i32) -> Obj = transmute(0x0C58_9189usize);
    f(obj, align, x, y)
}

/// `lv_obj_align_to(obj, base, align, x, y)` 相对**另一个对象**对齐 已验证
///
/// base 与 obj 处在不同父对象时容易随滚动/内边距漂移；固件自带界面的做法是
/// **首个对象对齐 `content`，其后一律对齐 `previous`**（且 previous 只在"已显示"时推进）。
pub unsafe extern "C" fn obj_align_to(obj: Obj, base: Obj, align: u32, x: i32, y: i32) -> Obj {
    let f: unsafe extern "C" fn(Obj, Obj, u32, i32, i32) -> Obj = transmute(0x0C58_8EF1usize);
    f(obj, base, align, x, y)
}

/// `lv_obj_set_hidden(obj, hidden)` 0=显示 1=隐藏 已验证
///
/// 入口是一段跳转壳（`ldr.w pc,[pc],#0` + 一格目标指针），目标在 PSRAM `0x1C028D38`。
/// 壳自己的地址只能按"槽值 == 目标地址"反查得到 —— 照反编译里 `thunk_EXT_FUN_1c028d38`
/// 的尾数写会写到隔壁的壳上（那一格装的是 `0x00278699`，另一个函数，一调就崩）。
/// fw_addr: 0x004C8518  (文件偏移; VA = +0x0C0C0000, 指针 |=1)  fw_thunk: 0x1C028D39
///
/// 重要: 同一个入口的真身是 `lv_obj_clear_state`（取证在下面那条上），**它不隐藏任何东西**。
/// 这一格的名字与实参语义都不可信，新代码一律用 `obj_clear_state`；真要隐藏对象请走
/// `lv_obj_add_flag`（入口 `0x0C58_8961`，即现在的 `background_image_set_floating` 那个壳），
/// 把 HIDDEN 位(1)当掩码用。这里保留 `obj_set_hidden` 是为了不改动已经上机的其它页调用点，
/// 那几个调用点（页7/页8/页9 的行显隐）实际没有发生隐藏，是这条命名欠账的下游。
pub unsafe extern "C" fn obj_set_hidden(obj: Obj, hidden: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> Obj = transmute(0x0C58_8519usize);
    f(obj, hidden)
}

/// `lv_obj_clear_state(obj, state_mask)` —— 把对象上指定的状态位**清掉**。
///
/// 与 `obj_set_hidden` 是同一个入口（壳 `0x004C8518` → PSRAM `0x1C028D39`）。身份不是推的，
/// 是照着它在 flash 里的那份影子源逐字读出来的（`0x00076178`）：
/// ```text
/// ldrh r3,[r0,#0x30]     ; 取 obj+0x30 那个 16 位状态字段
/// tst  r1,r3             ; 传的掩码与现有状态有交集没有
/// bic  r2,r3,r1          ; new = state & ~mask   <- 是"清位"，不是加位、更不是赋值
/// bne/ bx lr             ; 没改动就直接返回
/// bl   0x00075734        ; 有改动才写回并重算样式
/// ```
/// 三条旁证各指同一件事：调用点里有两处传 `0xF000`（四个用户状态位）和一处传另一个函数
/// 现算出来的掩码（`0x00408242 mov r1,r0`）—— bool 的"隐藏"装不下这种实参；它这条壳在
/// PSRAM 侧的邻居是 `add_flag`(0x1C028BD8) 与 `clear_flag`(0x1C028C85)，同一族里挨着状态位
/// 那组函数；操作的对象偏移是 `+0x30` 状态而不是 `+0x2C` 标志。
///
/// 系统闹钟页在建完两轮之后对**第一个轮**发 `(轮, 2)`（`0x0048C328`，12 小时制分支
/// `0x0048C524` 同一发）= 把时轮的 `FOCUSED` 清掉。它为什么要清：注册进控制器那张组
/// （同一处 `vtable +0x34/+0x38`）会把第一个成员自动置焦，闹钟页不想要开局就有一个轮是焦的。
/// 我们的页面没有那条注册链，开局本来就没有焦点，所以这一发照抄过来大概率是一发空操作；
/// 抄它是为了让"建完滚轮之后状态一致"这条不留变量。
///
/// 状态位：`CHECKED=1` `FOCUSED=2` `FOCUS_KEY=4` `EDITED=8` `PRESSED=0x20`
/// `PRESS_LOST=0x40` `DISABLED=0x80`，用户位 `0x1000..0x8000`。
/// fw_addr: 0x004C8518  (文件偏移; VA = +0x0C0C0000, 指针 |=1)  fw_thunk: 0x1C028D39
pub unsafe extern "C" fn obj_clear_state(obj: Obj, state: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> Obj = transmute(0x0C58_8519usize);
    f(obj, state)
}

/// 滚轮"当前处于焦点态"的那个状态位（闹钟页就是清掉它，让开局没有轮是蓝的）。
pub const LV_STATE_FOCUSED: u32 = 2;

/// `lv_obj_add_event_cb(obj, cb, filter, user_data)` filter 用 `ev::*`（0=ALL） 已验证
pub unsafe extern "C" fn obj_add_event(obj: Obj, cb: Cb, filter: u32, udata: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, Cb, u32, u32) -> Obj = transmute(0x0C58_83F1usize);
    f(obj, cb, filter, udata)
}

/// `lvx_object_delete(obj)` 删除对象**及其全部子对象** 已验证
///
/// 绝不能在"该对象自己"的事件回调里调用（事件派发中 → use-after-free）。
/// 需要重建时请登记请求，交给 `lv_timer` 回调执行。
pub unsafe extern "C" fn obj_delete(obj: Obj) {
    let f: unsafe extern "C" fn(Obj) = transmute(0x0C58_8DE9usize);
    f(obj)
}

/// `lv_event_get_code(event)` 已验证
pub unsafe extern "C" fn event_get_code(event: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C58_8211usize);
    f(event)
}

/// `lv_event_get_user_data(event)` 已验证
///
/// 依据: 闹钟页那个滚轮回调 `FUN_004985E4` 用 `0x004C8310` 的返回值 1/2/3 区分是
/// 哪个轮被拨(宿主注册时把轮号当 user_data 传进去)。
/// fw_addr: 0x004C8310  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn event_user_data(event: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C58_8311usize);
    f(event)
}

// ---------------------------------------------------------------------------
// 2A. 系统消息框 lvx_page_msgbox（系统"确认重启？"那种叉/勾弹框）
// ---------------------------------------------------------------------------

/// lvx_page_msgbox 当前实例单例: 创建时写入, 类析构(0x0C4C9FF8)随对象删除自动清理。
/// 只读它做一件事: 关闭前判活性(单例仍 == 句柄 才值得删)。静态核对
pub const MSGBOX_SINGLETON: u32 = 0x200C_ED8C;

/// `0x0C4CA0D8(parent, mode)` 在 parent 上建 lvx_page_msgbox, 返回消息框对象。
/// parent = 任意容器(重启流程传页对象, 闹钟"连接断开"弹框传运行时容器 —— 两个生产
/// 调用点都不需要注册新页)。mode 传 0(与固件重启确认框那处调用一致)。静态核对
pub unsafe extern "C" fn msgbox_create(parent: Obj, mode: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> Obj = transmute(0x0C4C_A0D9usize);
    f(parent, mode)
}

/// `0x0C4CA130(page, kind, img, text, sub, btns, n)` 填消息框(7 参, 后 3 个走栈)。
/// kind 5 = 叉/勾确认框; text = 我们自己的 UTF-8 静态串(直进 set_text, 与资源系统无关);
/// img/sub 传 0(无图标无副文案); btns 条目 0x14 字节: [+0]=回调(bit0=1 必须置位,
/// 固件自己的调用点存的就是奇数), [+4]=图标路径可空, 其余 0; n = 按钮数(固件上限 4)。
/// 静态核对
pub unsafe extern "C" fn msgbox_fill(page: Obj, kind: u32, img: u32, text: *const u8,
                                     sub: u32, btns: *const u32, n: u32) {
    let f: unsafe extern "C" fn(Obj, u32, u32, *const u8, u32, *const u32, u32) =
        transmute(0x0C4C_A131usize);
    f(page, kind, img, text, sub, btns, n)
}

// ===========================================================================
// 3. 文本 / 样式
// ===========================================================================

/// `lvx_label_create(parent)` 已验证
pub unsafe extern "C" fn label_create(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0C58_8F31usize);
    f(parent)
}

/// `lvx_label_set_text(label, text)` 已验证
pub unsafe extern "C" fn label_set_text(label: Obj, text: Cp) -> Obj {
    let f: unsafe extern "C" fn(Obj, Cp) -> Obj = transmute(0x0C58_9491usize);
    f(label, text)
}

/// `0xC588BF8(obj, 值)`：系统数据行给标签设的另一个属性（心率区间行传 3）。 静态核对
pub unsafe extern "C" fn label_attr2(obj: Obj, v: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> Obj = transmute(0x0C58_8BF9usize);
    f(obj, v)
}

/// `lvx_style_apply(obj, style, part, state)` 例：`style_apply(lb, STYLE_MISANS_24, 255, 0)` 已验证
pub unsafe extern "C" fn style_apply(obj: Obj, style: u32, a: u32, b: u32) -> i32 {
    let f: unsafe extern "C" fn(Obj, u32, u32, u32) -> i32 = transmute(0x0C4B_F899usize);
    f(obj, style, a, b)
}

/// `lv_obj_set_width(obj, w)` 只设宽度，高度留给控件按样式自己算
///
/// 系统闹钟页对每个滚轮传 86。
/// fw_addr: 0x004C8420  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_width(obj: Obj, w: i32) -> Obj {
    let f: unsafe extern "C" fn(Obj, i32) -> Obj = transmute(0x0C58_8421usize);
    f(obj, w)
}

/// 从共享样式对象里读出字体（系统给冒号标签取 face 用的就是它）。读不到返回 0
///
/// 本体是 `lv_style_get_prop(样式, 0x5a, &out)` 的一层包装：属性 0x5a 就是字体，
/// 取不到时它把 out 清 0 再返回，所以返回值可以判空。
/// fw_addr: 0x003FF87C  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn style_get_font(style: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = transmute(0x0C4B_F87Dusize);
    f(style)
}

/// `lv_obj_set_style_text_font(obj, face, sel)` 给单个对象写字体
///
/// 系统闹钟页写完冒号文本紧接着调它。
/// fw_addr: 0x004C8E78  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_style_text_font(obj: Obj, face: u32, sel: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8E79usize);
    f(obj, face, sel)
}

/// 系统闹钟页给**每个滚轮**传 `(p, 2, 0)`、给 12 小时制那个 AM/PM 轮传 `(p, 0x14, 0)`
///
/// LVGL 本体在 PSRAM、没有符号表，具体是哪条样式属性查不到；只照抄字面值。
/// fw_addr: 0x004C8248  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_style_4c8248(obj: Obj, v: u32, sel: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8249usize);
    f(obj, v, sel)
}

/// 系统闹钟页给那个 96x36 标签传 `(l, 2, 0)`，形状与 `obj_set_style_4c8248` 相同但本体不同
/// fw_addr: 0x004C8830  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_style_4c8830(obj: Obj, v: u32, sel: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8831usize);
    f(obj, v, sel)
}

/// 挂完样式/字体之后，系统一律再对同一对象传一次 `(obj, 0xff, 0)`
///
/// `style_apply` 自己最后一步也是它，冒号标签设完字体后也调它。
/// fw_addr: 0x004C8DA0  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn obj_set_style_4c8da0(obj: Obj, v: u32, sel: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8DA1usize);
    f(obj, v, sel)
}

/// 给标签挂上系统默认的两枚样式（系统建那个 96x36 标签时紧接着调它，传 0）
///
/// 本体是两对 `0x004C7EF8`/`0x004C89F0`（成对的移除 + 挂入），样式对象取自 SRAM。
/// fw_addr: 0x003FFFFC  (文件偏移; VA = +0x0C0C0000, 指针 |=1)
pub unsafe extern "C" fn label_apply_defaults(obj: Obj, sel: u32) {
    let f: unsafe extern "C" fn(Obj, u32) = transmute(0x0C4B_FFFDusize);
    f(obj, sel)
}

// ===========================================================================
// 3.1 条形进度条（照系统「压力分布」/「心率区间」的做法）
// ===========================================================================

// --- 3.1.0 系统侧的"条"是怎么造的 ---
//
// 两页（压力四档、心率五区间）用的是**同一套通用对象 API**，逐步取自它们自己的代码：
//
//   obj = 0xC5881E0(parent, 0)                 ; 通用对象(全盘 552 处调用)
//   0xC588628(obj, 宽, 0x200007d1)             ; 尺寸: 第三参是平台统一的主题/样式指针(两页都是它)
//   0xC588CB8(obj, RGB, 0)                     ; 设背景色(心率页给轨道 #262626)
//   0xC5893C0 / 0xC588770 / 0xC5889C0 / 0xC587C60 / 0xC5880A0 (obj, 0, 0)   ; 清属性
//
// 药丸外形来自对象**样式自带的圆角**（不是遮罩、不是圆头线）：
// 真机实测填充两端都是半圆（中心行比上下边多凸 8px = 半径 24 的圆弧矢高）。
// 根因: 用 `0xC588ED0`(obj_set_color) 配自造遮罩画不出这个形状，
// 系统那条路是"通用对象 + `0xC588CB8` 设背景色"。

/// `0xC588628(obj, 宽, 主题指针)`：系统建条时的尺寸调用。 静态核对
///
/// 第三参两页都写死 `0x200007d1`（平台主题/样式单例，运行时 RAM 地址），
/// 所以高度很可能来自主题而不是我们传的数 —— 本层照抄传同一个常量。
pub const BAR_THEME_PTR: u32 = 0x2000_07d1;

/// 建条时用**明确尺寸**（第三参是高度）。
///
/// 真机实测：把 `BAR_THEME_PTR` 当高度传，对象会画成约 75px 高的大块、且背景色不生效；
/// 而系统在需要明确尺寸处都是给实数（如心率区间页的分隔线 `(296, 1)`，其 `#262626` 正常生效）。
pub unsafe extern "C" fn obj_set_size_wh(obj: Obj, w: u32, h: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8629usize);
    f(obj, w, h)
}

/// `0xC588CB8(obj, RGB, 0)`：**设对象背景色**（系统给轨道/填充上色用的就是它）。 静态核对
///
/// 心率区间页实证：`movw r3,#0x2626; strh; movs r3,#0x26; strb` 之后调它 → 轨道深灰 #262626。
pub unsafe extern "C" fn obj_style_color(obj: Obj, rgb: u32, a: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8CB9usize);
    f(obj, rgb, a)
}

// --- 3.1.3 进度条组件（照系统压力页 create_zone / 心率页的做法，逐字抄）---

/// `0xC5880D0(父) -> bar`：平台的进度条组件工厂（全盘 28 处）。 静态核对
///
/// 依据（压力页 create_zone 0x0C62F43C 起、心率页 0x0C66296A 起，两处写法一致）：
/// ```
/// bar = 0xC5880D0(parent)
/// 0xC588628(bar, 296, 20)          ; 明确尺寸
/// 0xC588CB8(bar, #676767, 0)       ; 轨道色(压力页原样)
/// 0xC588CB8(bar, 色表色, 0)         ; 填充色(压力页按档位取色表, 心率页按区间取色表)
/// 0xC5883F8(bar, 0..255, 0)        ; 设值: 压力页 0xff=满, 0=空
/// 0xC588500(bar, 5, 0) ; 0xC588500(bar, 5, 0x20000)
/// ```
/// 轨道与填充由组件自己画，我们只给"尺寸 + 颜色 + 值"。
pub unsafe extern "C" fn bar_new(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x0C58_80D1usize);
    f(parent)
}

/// `0xC5883D8(bar, 值, 量程)`：同族设值（系统别处写 (bar,0,100)）。 静态核对
pub unsafe extern "C" fn bar_set_value_range(bar: Obj, v: i32, max: i32) -> Obj {
    let f: unsafe extern "C" fn(Obj, i32, i32) -> Obj = transmute(0x0C58_83D9usize);
    f(bar, v, max)
}

/// `0xC588500(bar, 值, 标志)`：据系统进度条 `target_set_value` 的用法，
/// `0xC588500(bar, 100, 0)` 是在设量程/最大值（压力页另用它设 5 号属性）。 静态核对
pub unsafe extern "C" fn bar_set_range(bar: Obj, v: u32, flags: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8501usize);
    f(bar, v, flags)
}

/// `0xC588A70(obj, RGB, 0)`：设文字颜色(系统在标签上大量使用, 例如
/// 心率区间名标签 `0xC588A70(lab, #5027FF, 0)`)。注意: 只在"文字标签"上用(在进度条组件上用会崩)。
///
/// 根因: `0xC58A070` 处反汇编出来是字面量(`cdp2` 垃圾指令), 不是函数入口 ——
/// 把它当入口写成 `0x0C58_A071`(`0xC58A070|1`) 跳过去就是开应用即崩。
/// 真正的入口是 `0xC588A70` 的 thunk: `ldr.w pc,[pc]` 跳到 RAM 控件模块 `0x002BE921`。 静态核对
pub unsafe extern "C" fn obj_style_color_a70(obj: Obj, rgb: u32, a: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_8A71usize);
    f(obj, rgb, a)
}

/// 次要文字灰(系统设色统计里出现最多的次要灰, 33 处)
pub const TEXT_GRAY_SUB: u32 = 0x99_99_99;

/// `0xC5890A8(bar, 值, 0)`：把当前值喂给进度条（系统进度条与压力页都用它）。 静态核对
///
/// 依据：`target_set_value`(0x0C5AC558) 把它算出的百分比变量喂给条；
/// 压力页 `create_zone`(0x0C62F4C2) 也是 `mov r1, fp; bl 0xC5890A8`。
/// 真机实测：不调这一句时，条会一直停在初始化时的满值上。
pub unsafe extern "C" fn bar_apply_value(bar: Obj, v: u32, flags: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32, u32) -> Obj = transmute(0x0C58_90A9usize);
    f(bar, v, flags)
}

/// 条尺寸：照系统进度条尺寸（压力页 296x20，别处 288x20 / 304x8）
pub const BAR_W: i32 = 296;
pub const BAR_H: i32 = 20;

/// 轨道色：压力页给自己的轨道设的就是 #676767
pub const BAR_TRACK_RGB: u32 = 0x67_67_67;

/// 填充色：小米经典蓝。取值依据是固件自己用的蓝：`widget_update_v2`(0x0C622E00) 里
/// `movw r3,#0x5381 / movs r3,#0xfe` 后 `0xC588A70(obj, #5381FE, 0)`，
/// 它也是固件里与 MIUI 经典蓝 #3482FF 最接近的常量。
pub const BAR_FILL_RGB: u32 = 0x53_81_FE;

/// 建一条进度条组件（组件自带半透明轨道；真机照片已确认轨道在，只是半透明）。
///
/// 只用已在真机验证过的调用：工厂 -> 明确尺寸 -> 量程 -> 填充色 -> 初始化 -> 设值。
/// 轨道不再单独建对象（单独建反而画出一条实心条，与系统不一致）。
/// 内存行用的填充色(绿), 与存储行的蓝区分开
pub const BAR_FILL_GREEN: u32 = 0x92_DE_00;

/// 建一条指定填充色的进度条组件。
///
/// 轨道统一设灰 `#676767`：组件自带轨道偏蓝，压力页每根条都是先
/// `0xC588CB8(bar, #676767, 0)` 设轨道、再 `0xC588CB8(bar, 填充色, 0x20000)` 设填充，
/// 这里照抄同一步(设色入口已真机验证过，只是标志位取系统轨道那档的 0)。
pub unsafe fn bar_create_rgb(parent: Obj, pct: u32, fill_rgb: u32) -> Obj {
    let bar = bar_new(parent);
    if bar == 0 {
        return 0;
    }
    obj_set_size_wh(bar, BAR_W as u32, BAR_H as u32);
    bar_set_range(bar, 100, 0);
    obj_style_color(bar, BAR_TRACK_RGB, 0);
    obj_style_color(bar, fill_rgb, 0x20000);
    bar_set_value_range(bar, 0, 100);
    bar_apply_value(bar, bar_pct(pct), 0);
    bar
}

/// 更新进度：先按量程归零再喂值（照系统进度条的用法）
pub unsafe fn bar_set_pct(bar: Obj, pct: u32) {
    if bar == 0 {
        return;
    }
    bar_set_value_range(bar, 0, 100);
    bar_apply_value(bar, bar_pct(pct), 0);
}

/// 百分比钳到 0..100（组件量程用 100，与系统自己的进度条一致）
fn bar_pct(pct: u32) -> u32 {
    if pct > 100 { 100 } else { pct }
}


/// `0x0C86037C(name, path)` 字体登记薄封装: fm 单例(0x20103174)->+0x1C 路径管理器
/// 的 add 记录。同 (name,path) 已存在时只加引用计数(幂等)。 静态核对
pub unsafe extern "C" fn fw_font_reg(name: Cp, path: Cp) {
    let f: unsafe extern "C" fn(Cp, Cp) = transmute(0x0C86_037Dusize);
    f(name, path)
}

/// `0x0C587B38(style, face)` 开机代码把字体写进 style 对象用的就是它。
/// 定位依据: 0x0C4C0B40 起 82 条 `lv_style_init(style)` +
/// `miwear_font_create(id,size,0)` + `mov r1,r0` + `bl 本地址` 的成对序列,
/// 序列里的 style 常量与固件自带界面的字体样式逐条吻合(0x2010CE44=MiSans-Regular 24)。
/// 注意: 这个入口曾被猜成 `lv_style_set_bg_color`，上面的成对序列否掉了那个猜测。
/// 静态核对
pub unsafe extern "C" fn fw_style_set_font(style: u32, face: u32) {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = transmute(0x0C58_7B39usize);
    let _ = f(style, face);
}

/// `0x0C8603B0(name, size, flag)` 按【任意字体名】建 face。
/// miwear_font_create(0x0C4B7FBC) 只吃 id 0..9 查名字表, 拿不到我们自己
/// 注册的新名字; 该表查出的名字串正是喂给本函数的, 故直接调用它。
/// 字体管理器按 (名字,尺寸) 缓存: 名字没见过才会真的打开文件建 face。
/// 静态核对(0x0C4B7FBC 反汇编 0x0C4B7FCE 处实证)
pub unsafe extern "C" fn fw_mfont_create_named(name: Cp, size: u32, flag: u32) -> u32 {
    let f: unsafe extern "C" fn(Cp, u32, u32) -> u32 = transmute(0x0C86_03B1usize);
    f(name, size, flag)
}

/// `0x0C589448(obj, selector, prop)` LVGL v9 样式属性 getter(只读)。
/// 依据: row_init(0x0C4C8560) 以 prop=0x5A(text_font)/0x59/0x5C
/// 从行对象读字体再抄给标签 —— 行文字字体的真正来源。
/// 静态核对
pub unsafe extern "C" fn fw_style_get_prop(obj: u32, sel: u32, prop: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32, u32) -> u32 = transmute(0x0C58_9449usize);
    f(obj, sel, prop)
}

/// `0x0C107790(obj, prop, value, selector)` LVGL v9 对象级 local style 写入,
/// 即固件里的 `lv_obj_set_local_style_prop`(R1=prop R2=值 R3=selector)。
/// 用途: 样式对象/记录/缓存三条路都改不到行文字 => 对**活对象**直写 text_font。
/// 重要(字节级): 该入口尾部 0x0c107892 会调 `lv_obj_refresh_style`
/// (0x0C1070AC) 做整棵子树失效重绘, 传播与否由属性标志表 0x2CCE5BC4[prop] 决定;
/// text_font(0x5A) 实测该表值 0x05 = 带传播位 => **写完不需要再补任何重绘调用**。
/// 另: 对象本来没有 local style 时它会自行分配 12B(0x0c1078ce 起), 由 obj_delete 回收。
/// 静态核对
pub unsafe extern "C" fn obj_set_local_style_prop(obj: u32, prop: u32, val: u32, sel: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = transmute(0x0C10_7791usize);
    f(obj, prop, val, sel)
}

/// `LV_STYLE_TEXT_FONT` 属性号。依据(字节级): 固件自己的
/// `style_get_text_font`(0x0C4BF87C) 逐字为
/// `movs r1,#0x5A; bl 0x0C588BB8(style, 0x5A, &out)` —— 即读 text_font 用的就是 0x5A;
/// `row_init` 也以 `movs r2,#0x5A` 调 getter 取行字体。
/// 注意: "local style 写入即崩"的成因是**属性号靠猜**, 不是本入口的问题;
/// 本常量是固件自己用过的号, 属实证。
pub const LV_STYLE_TEXT_FONT: u32 = 0x5A;

/// `0x0C588BB8(style, prop, &out)` lv_style_t **结构体**属性读取(只读)。
/// 依据(字节级): 固件 style_get_text_font(0x0C4BF87C, 全固件 1265 处调用)体内就是
/// `movs r1,#0x5A; bl 0x0C588BB8(style, 0x5A, &out)` —— 同型调用、同样这批开机
/// lv_style_t 实证。返回值按 bool 用(命中非 0); 即便语义有出入, out 初值 0 +
/// 调用方 plausible/safe 门兜底, 读不到就当没读到, 不会写任何东西。
/// 用途: 曾服务 v3 捕获(已退役), 留作样式诊断工具。
#[allow(dead_code)]
pub unsafe extern "C" fn fw_style_get(style: u32, prop: u32, out: *mut u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32, *mut u32) -> u32 = transmute(0x0C58_8BB9usize);
    f(style, prop, out)
}

/// `0x0C5881D8(obj) -> i32` 子对象数。依据(字节级):
/// get_unhidden_child_count(0x0C599394) 体内 `bl 0xc5881d8` 取循环上界。
/// 静态核对(固件自身用法实证)
pub unsafe extern "C" fn obj_child_count(obj: Obj) -> i32 {
    let f: unsafe extern "C" fn(Obj) -> i32 = transmute(0x0C58_81D9usize);
    f(obj)
}

/// `0x0C5886E0(obj, idx) -> child|0` 按下标取子对象。同上体内循环实证。
/// 静态核对
pub unsafe extern "C" fn obj_get_child(obj: Obj, idx: u32) -> Obj {
    let f: unsafe extern "C" fn(Obj, u32) -> u32 = transmute(0x0C58_86E1usize);
    f(obj, idx)
}

/// `obj` 是否仍挂在 `parent` 的子对象表里 —— **句柄存活门**。
///
/// 用在"要不要 obj_delete / 要不要原地改这个对象"这种判定上: 只有仍在子表里的句柄才是
/// 活对象; 不在就是已经被删了(或根本是复用同一块内存的新对象), 碰它 = use-after-free。
///
/// 为什么不能只信单例字(根因): 消息框那类控件登记单例的写法是
/// `bl 0x0C5891A8(0x200CED8C)` 拿**槽指针**再 `str 实例,[r0]`, 而 0x0C5891A8 是指向
/// 已清零 0x1C 段的跳板 —— 静态读不出它返回 holder 本身还是 holder+偏移, 所以
/// `rd32(0x200CED8C) == 句柄` 有可能永远不等, 拿它当删除门会导致对象永远删不掉。
/// 子对象表是固件自己遍历对象用的入口(上面那两个), 不依赖任何猜测的登记布局。
///
/// 上界 24 是护栏: 一个坏返回值(比如负数或天文数字)会把固件调用打成几亿次循环,
/// UI 线程直接卡到看门狗复位。页根/容器上的子对象量级只有个位到十几。
pub unsafe fn obj_is_child_of(parent: Obj, obj: Obj) -> bool {
    if parent == 0 || obj == 0 { return false; }
    let n = obj_child_count(parent);
    if n <= 0 || n > 24 { return false; }
    let mut i = 0u32;
    while i < n as u32 {
        if obj_get_child(parent, i) == obj { return true; }
        i += 1;
    }
    false
}

/// `lv_arc_create` 0x1C0641D0(Thumb 0x1C0641D1): 类描述 0x2CCE5F0C, 内部
/// `lv_obj_class_create_obj` + `lv_obj_class_init_obj`。反汇编自 LVGL 代码段 dump。
pub unsafe extern "C" fn arc_create(parent: Obj) -> Obj {
    let f: unsafe extern "C" fn(Obj) -> Obj = transmute(0x1C06_41D1usize);
    f(parent)
}

/// 写 arc 的 value 字段(+0x2E，i16，0..100)并失效重绘。
/// LVGL 8.4.0 `lv_arc_t`：obj 24B；+0x24 rotation、+0x26 indic_start、+0x28 indic_end、
/// +0x2A bg_start、+0x2C bg_end、+0x2E value、+0x30 min、+0x32 max（角度 u16，值 i16）。
unsafe fn arc_invalidate(arc: Obj) {
    let f: unsafe extern "C" fn(Obj) = core::mem::transmute(0x1C02_B305usize);
    f(arc);
}

/// 写 arc 的 value(i32, +0x34)。dump 里 arc 构造函数把 +0x34/+0x38/+0x3C
/// 当整数初始化(value/min/max)，角度从 +0x40 起是 i16；之前按 f32 写是错的。
pub unsafe fn arc_set_pct(arc: Obj, pct: u32) {
    if arc == 0 { return; }
    let p = if pct > 100 { 100 } else { pct } as i32;
    let a = arc as usize;
    core::ptr::write_volatile((a + 0x34) as *mut i32, p);
    arc_invalidate(arc);
}

/// 整圆、从正底部开始。dump 构造函数：value +0x34 i32；bg_start/bg_end
/// +0x38/+0x3C f32；indic_start/end +0x40/+0x44 f32；min/max +0x4C/+0x50 i32。
pub unsafe fn arc_make_full(arc: Obj) {
    if arc == 0 { return; }
    let a = arc as usize;
    core::ptr::write_volatile((a + 0x34) as *mut i32, 0);
    core::ptr::write_volatile((a + 0x38) as *mut f32, 0.0);
    core::ptr::write_volatile((a + 0x3C) as *mut f32, 360.0);
    core::ptr::write_volatile((a + 0x40) as *mut f32, 0.0);
    core::ptr::write_volatile((a + 0x44) as *mut f32, 360.0);
    core::ptr::write_volatile((a + 0x4C) as *mut i32, 0);
    core::ptr::write_volatile((a + 0x50) as *mut i32, 100);
    arc_invalidate(arc);
}

/// 关掉 arc 的触摸交互：清 obj flags(+0x1C) 的 CLICKABLE(bit1)。
/// 否则 LVGL 的 arc 会把手势变成数值拖拽（用户能直接把环拖大拖小）。
pub unsafe fn arc_disable_input(arc: Obj) {
    obj_clear_flag(arc, 0x2);
}

/// 清掉对象 flags(+0x1C) 的 SCROLLABLE(bit4)，禁止该容器左右/上下滑动。
/// 主页圆环行比屏宽时，父容器会把手势当横向滚动，用户一划整行就跟着动。
pub unsafe fn obj_clear_scrollable(obj: Obj) {
    obj_clear_flag(obj, 0x10);
}

/// 调固件 lv_obj_clear_flag(obj, f) 0x1C028C85，不再手写 flags 偏移。
pub unsafe fn obj_clear_flag(obj: Obj, f: u32) {
    if obj == 0 { return; }
    let func: unsafe extern "C" fn(Obj, u32) = transmute(0x1C02_8C85usize);
    func(obj, f);
}

/// 调固件 lv_obj_add_flag(obj, f) 0x1C028BD8。
pub unsafe fn obj_add_flag(obj: Obj, f: u32) {
    if obj == 0 { return; }
    let func: unsafe extern "C" fn(Obj, u32) = transmute(0x1C02_8BD9usize);
    func(obj, f);
}
