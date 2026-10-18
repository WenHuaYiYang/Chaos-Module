//! 固件地址的**唯一真源**，按编译目标选择。
//!
//! ## 为什么要有这一层
//!
//! 同一支固件入口在不同机型上是两个不同的地址。代码里写死的 `transmute(0x0C..)`
//! 只对编译时那一个目标成立，而 `0x0C51xxxx` 与 `0x0C7Exxxx` 只差一位数字，
//! 平移时最容易抄错。把"哪个目标用哪个值"收到这一份，其余各层只引用这里的名字，
//! 一个锚点就不会有两半真值。
//!
//! ## 迁入规则
//!
//! - 一条地址进 RW6 那一档之前，必须先有**另一台机器上的独立证据**（对侧固件的函数
//!   入口 + 参数与结构行为逐行对上），等级写进那一行的注释；只有助记符序列相似、
//!   只有串键对上，都不算。
//! - 还没定案的锚点在这一份里**不存在**。不许拿默认目标的值顶替 —— 那会让
//!   RW6 的构建看起来"通了"，实际一点就崩。
//!
//! ## 开关
//!
//! 默认目标是 3.101.043。`--features fw-p65`（Redmi Watch 6 / best1502p）现在被下面
//! 那条 `compile_error!` 挡着：RW6 侧只定案了极少数锚点，半迁状态编出来的模块不可用。
//! 覆盖齐了之后删掉那条 `compile_error!`，这条线才算能编译。

/// 3.101.043（小米手环 10 Pro / best1503）—— 默认目标
mod t043 {
    /// `app_launcher_add(app_id)`：把**注册表在册**的应用立即加回桌面。
    /// 043 官方符号包里这一段叫 `init_buffer`，1 参。
    pub const APP_LAUNCHER_ADD: u32 = 0x0C51_3A45;

    /// `app_launcher_del(app_id)`：把一个应用从桌面摘掉（只删桌面记录与图标对象，
    /// 不卸载、不碰注册表）。
    pub const APP_LAUNCHER_DEL: u32 = 0x0C51_3B2D;

    /// `launcher_refresh_app(app_id)`：换完注册表里的图标路径后让桌面重画这一个应用。
    pub const LAUNCHER_REFRESH_APP: u32 = 0x0C51_3BC1;

    /// `page_goto(key, start_data, param, callbacks)`：带动画压栈跳页。
    pub const PAGE_GOTO: u32 = 0x0CA6_C359;

    /// `lvx_page_title_create(root, title, mode, cb, udata)`：系统同款页标题栏。
    pub const PAGE_TITLE_CREATE: u32 = 0x0C4C_A6C5;

    /// `lvx_zoom_picker_create(parent)`：系统同款滚轮选择器（时间设置那两个轮就是它）。
    pub const ZOOM_PICKER_CREATE: u32 = 0x0CA6_7835;

    /// `lvx_zoom_picker_set_mode(picker, mode)`：滚轮的取词档位（模式 1 走逐条取词）。
    pub const ZOOM_PICKER_SET_MODE: u32 = 0x0CA6_7851;

    /// `brightness_set_value(v)`：手动设亮度。
    pub const BRIGHTNESS_SET_VALUE: u32 = 0x0CA6_F661;

    /// `brightness_set_full_power(on)`：全功率模式。
    pub const BRIGHTNESS_SET_FULL_POWER: u32 = 0x0CA6_F7D9;

    /// `apply_brightness_smoothly(v)` / `brightness_apply_raw(v)`：直接渐变应用，不经手动钳位。
    pub const BRIGHTNESS_APPLY_RAW: u32 = 0x0CA6_EF35;

    /// `fw_mfont_create_named(name, size, flag)`：按任意字体名建 face（换字体那条链的入口）。
    /// 调用点未接表（`gfx.rs` 此刻带着另一条在飞的改动，不能替它提交）；
    /// 接过来时照 `fwaddr` 头部那三步：值进表、调用点引表名、核对有没有门钉着这个 VA。
    pub const MFONT_CREATE_NAMED: u32 = 0x0C86_03B1;
}

/// 3.100.043（Redmi Watch 6 / best1502p）
mod tp65 {
    /// 与 043 的 `app_launcher_add` 逐行对上：先经注册表取回调，再拿图标对象，
    /// 对象为空则早退，然后取 id 进图标链表比较档位位。
    /// 等级 静态核对（双侧反编译逐行比对），模拟器里还没执行到。
    pub const APP_LAUNCHER_ADD: u32 = 0x0C7E_C451;

    /// 与 043 的 `app_launcher_del` 逐行对上：四参日志 + 遍历图标链表，
    /// 命中即释放图标对象并从链表与顺序表上摘除。
    /// 等级 静态核对（双侧反编译逐行比对），模拟器里还没执行到。
    pub const APP_LAUNCHER_DEL: u32 = 0x0C7E_C53D;

    /// 与 043 的 `launcher_refresh_app` 逐行对上：在桌面记录链里按记录首字 == app_id
    /// 找记录，找不到就打四参日志返回；经注册表 `[+0x18]` 槽 +4 取描述符，
    /// 把 `desc+0x0C` 的图标路径抄进记录，再调 `desc+0x1C` 回调，最后释放旧图片对象重画。
    /// 等级 静态核对（双侧反编译逐行比对），模拟器里还没执行到。
    pub const LAUNCHER_REFRESH_APP: u32 = 0x0C7E_C745;

    /// 与 043 的 `page_goto` 逐行对上：四参、按 `+0x14` 比对页键走页面链表、
    /// 页栈取上一项、`+0x24/+0x30` 双非零才隐藏、经 `[ctx+4]+0x20` 取新页对象。
    /// **两处版本差异已核**：这一档把新页的 `+0x1C/+0x20` 清 0（043 填 `0x7FFFFFFF`），
    /// 动画起点口径不同。等级 静态核对（双侧反编译逐行比对），模拟器里还没执行到。
    pub const PAGE_GOTO: u32 = 0x0C22_2D11;

    /// 与 043 的 `lvx_page_title_create` 同形：五参、先建控件再判空返回、
    /// 逐格写尺寸与对齐。两处差异已核：这一档没有 043 那句"额外设 +0x10 属性"，
    /// 且高度常数不同（043 `+0x44=0x38`，这一档 `+0x4C=0x48`）。
    /// 等级 静态核对（同族同形，差异已列），模拟器里还没执行到。
    pub const PAGE_TITLE_CREATE: u32 = 0x0C21_B2FD;

    /// 与 043 的 `lvx_zoom_picker_create` **完全同形**（四参、把类对象指针 prepend 进第一参、
    /// 连"后两参都传 param_4"这个怪写法都一样），建完再初始化一次。
    /// 等级 静态核对（双侧反编译逐行比对），模拟器里还没执行到。
    pub const ZOOM_PICKER_CREATE: u32 = 0x0C24_0411;

    /// 与 043 的 `lvx_zoom_picker_set_mode` 逐行对上：模式 1 才建逐条取词的标签数组，
    /// 条数 ×0x28 分配、按 `i - n*(i/n)` 取模回绕、每条过一遍取词函数、
    /// 取词为空就打等级 3 日志。结构体字段整体偏移 +8（043 `+0x534/+0x538/+0x53C`
    /// 对这一档 `+0x53C/+0x540/+0x544`）。等级 静态核对，模拟器里还没执行到。
    pub const ZOOM_PICKER_SET_MODE: u32 = 0x0C24_0435;

    /// 与 043 的 `brightness_set_value` **逐行相同**：先问一次"是不是手动档"，
    /// 是就把入参过一遍钳位；写状态块 `+0x0C`；`状态块+0x240` 非零时打等级 6 的四参日志；
    /// 最后 `+0x19==2 && +0x18==0` 那一条支路里调 apply。状态块字段偏移整体平移
    /// （043 的 `+0x210` 对这一档 `+0x240`）。等级 静态核对，模拟器里还没执行到。
    pub const BRIGHTNESS_SET_VALUE: u32 = 0x0CA6_5C49;

    /// 与 043 的 `brightness_set_full_power` 逐行对上：比 `+0x18` 相等就返回，
    /// 否则写 `+0x18`，按"是不是手动档"把上限格（043 `+0xDC` 对这一档 `+0xEC`）填 600 或 0xFF，
    /// 再走同一条 `+0x19==2` 支路调 apply。等级 静态核对，模拟器里还没执行到。
    pub const BRIGHTNESS_SET_FULL_POWER: u32 = 0x0CA6_5DB9;

    /// 与 043 的 `apply_brightness_smoothly` 对上：它是上面两支在那个支路里调的那一支，
    /// 两侧调用图位置一致（`set_value` 与 `set_full_power` 都调它，各自四处引用）。
    /// 等级 静态核对，模拟器里还没执行到。
    pub const BRIGHTNESS_APPLY_RAW: u32 = 0x0CA6_54D1;

    /// 与 043 的 `vg_font_create(name,size,flag)` 逐行对上：三参（名字、short 尺寸、short 标志），
    /// 先把 8 字节的本地键清零再填三项，管理器 `+0x1C` 为空时先建缓存并登记两遍，
    /// 名字或尺寸为 0 走等级 3 日志，否则查缓存→建 face→把 face 挂回 `+0x1C`。
    /// **两处版本差异已核**：这一档的管理器要多走一层 `+0x1E0` 间接，且初始化里多一个登记调用。
    /// 等级 静态核对，模拟器里还没执行到。
    pub const MFONT_CREATE_NAMED: u32 = 0x0C21_88ED;
}

#[cfg(not(feature = "fw-p65"))]
pub use t043::*;

/// `state.rs` 的注册链那一头（`FW_INIT_BUFFER`）还留着自己的字面值。在它改从这一份取
/// 之前，先用一条编译期判据钉住两边不许漂移：谁改了其中一边而没改另一边，
/// 默认目标的构建当场失败。
#[cfg(not(feature = "fw-p65"))]
const _: () = assert!(crate::state::FW_INIT_BUFFER == t043::APP_LAUNCHER_ADD);

#[cfg(feature = "fw-p65")]
pub use tp65::*;

#[cfg(feature = "fw-p65")]
compile_error!("fw-p65 的地址表只覆盖了极少数锚点，其余调用点仍是默认目标的值，\
                编出来的模块在 Redmi Watch 6 上必崩。把地址逐条定案并迁入 `tp65` 后删除本条。");
