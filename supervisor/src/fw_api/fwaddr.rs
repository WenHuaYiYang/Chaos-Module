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

/// **原型漂移清单**：同一支功能在两端**参数表不同**时，只有地址不够 —— 包装函数必须按目标给两套签名。
/// 目前已知的一处：`lvx_list_item_update` —— 043 是六参、p65 是四参
/// `(obj, icon, p3, p4)`，体内语义也不同（p65 是"空标签早退 → 设图标 → 取主标签文本 →
/// 四项样式写回并重排"）。接这类锚点时，先在两档里各写一份包装，再由上层按语义调用，
/// **不许**拿 043 的原型直接调 p65 的地址。
///
/// 检查办法：接一支锚点之前，先比两端反编译的参数个数与字段读写集合；
/// 个数或集合不同就进这张清单。
pub mod proto_drift {}

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

    /// `lv_timer_create(cb, period, ud)`：建一个在固件主循环里跑的定时器。
    /// 本档走 LVGL 跳转壳：file 0x4C7ED0 处是 `ldr.w pc,[pc],#0`，壳里装的槽值
    /// `0x1C060035` 是运行期落在 PSRAM 侧的实现真身。等级 已验证 —— 本模块的主节拍
    /// 与安装队列都由它建，界面上的读数、滑动震动、页面 tick 全依赖它。
    /// 重要: p65 那一档是**另一个地址**，两档的值不许互抄。
    pub const LV_TIMER_CREATE: u32 = 0x0C58_7ED1;
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

    // ---- 建控件与样式：这一族的目标是 SRAM 里的真身，不是 flash 里的跳转壳 ----

    /// `lv_obj_class_create_obj(class, parent)`。建控件第一步。
    /// 走 flash 跳转壳（同功能的 `0x0C25858D`）会返回一个小整数而不是对象指针，必须用这个 SRAM 真身；
    /// `parent` 传空会建出屏幕对象，这是合法用法（不是崩因）。等级 执行确认（模拟器）。
    pub const LV_OBJ_CLASS_CREATE_OBJ: u32 = 0x0037_75BD;

    /// `lv_obj_class_init_obj(obj, ctx)`。建控件第二步，`ctx` 传 0。等级 执行确认（模拟器）。
    pub const LV_OBJ_CLASS_INIT_OBJ: u32 = 0x0037_76B1;

    /// `lv_label_class`：标签类指针。等级 执行确认（建出的对象头一格就是它）。
    pub const LV_LABEL_CLASS: u32 = 0x0C31_4608;

    /// `lv_label_set_text(obj, text)`：文本指针在 `obj+0x34`，非静态串会被复制一份。
    /// 等级 执行确认（写入后该格指向设进去的串）。
    pub const LV_LABEL_SET_TEXT: u32 = 0x0C78_C879;

    /// `lv_timer_create(cb, period, ud)`：回调在固件主循环里跑。定时器结构
    /// `+0x00 period / +0x04 last_run / +0x08 cb / +0x0C user_data / +0x10 repeat / +0x14 flags`。
    /// 等级 执行确认（回调 5 秒内被调百余次）。
    pub const LV_TIMER_CREATE: u32 = 0x0C74_14D9;

    /// `lv_obj_set_local_style_prop(obj, prop, value, selector)`：样式与几何的统一入口。
    /// 影子源 file 0x18420（活体 SRAM 0x2017E408 起 24 字节在镜像里唯一命中）。
    /// 注意 `0x00378965` **不是**它：那一支只写属性 0xA/8/9（align/x/y），是 `lv_obj_align`。
    /// 等级 执行确认（宽高/坐标/底色/不透明度/字色/字体全部按给定值生效）。
    pub const LV_OBJ_SET_LOCAL_STYLE_PROP: u32 = 0x0037_E409;

    /// `lv_obj_get_local_style_prop(obj, prop, out, selector)`：返回 1 = 命中并把值写进 `out[0]`。
    /// 等级 执行确认（写进去的值逐个读回一致）。
    pub const LV_OBJ_GET_LOCAL_STYLE_PROP: u32 = 0x0037_D629;

    /// `lv_obj_align(obj, align, x_ofs, y_ofs)`：内部依次写 align/x/y 三个属性。
    /// 等级 执行确认（三支属性包装的反编译逐个对上）。
    pub const LV_OBJ_ALIGN: u32 = 0x0037_8965;

    /// `lv_obj_delete(obj)`：发 `LV_EVENT_DELETE(0x23)` 后从父链摘除。
    /// **必须用 SRAM 真身**：file 0x19E56 在早期段（< 0x2FDC4），XIP 别名调不动。
    /// 等级 执行确认（删完对象的类指针被清、父对象子表清空）。
    pub const LV_OBJ_DELETE: u32 = 0x0037_FE3F;

    /// `lv_obj_add_flag(obj, flag)`：对象标志字在 `obj+0x2C`。等级 执行确认（模拟器）。
    pub const LV_OBJ_ADD_FLAG: u32 = 0x0038_7B99;

    /// `lv_obj_add_event_cb(obj, cb, filter, user_data)`：描述符
    /// `+0x0C = cb / +0x10 = user_data / +0x14 = filter`。等级 执行确认（模拟器）。
    pub const LV_OBJ_ADD_EVENT_CB: u32 = 0x0C25_70BD;

    /// `lv_timer_delete(timer)`。等级 静态核对（ll_remove + free）。
    pub const LV_TIMER_DELETE: u32 = 0x0C74_154D;

    /// `lv_image_set_src(img, src)`。等级 静态核对（走图像解码器）。
    pub const LV_IMAGE_SET_SRC: u32 = 0x0C78_ACF1;
}

/// p65 侧的样式属性号（`set_local_style_prop` 的第二个参数）。
/// 出处是固件自己的 setter 调用点与逐属性对照实验；**不要按 LVGL 版本或枚举顺序猜**：
/// 043 的组 0 是 `W/H/X/Y/ALIGN = 1/4/7/8/9`，这一档实测是 `1/2/8/9/0xA`，
/// 高度按 043 写 4 完全不生效（对象保持主题默认的 130x130）。
pub mod p65_style {
    pub const WIDTH: u32 = 0x01;
    pub const HEIGHT: u32 = 0x02;
    /// 0x05 / 0x07 会让宽/高收缩到给定值（夹取的上界那一侧），语义未定名，先不要用。
    pub const X: u32 = 0x08;
    pub const Y: u32 = 0x09;
    pub const ALIGN: u32 = 0x0A;
    pub const RADIUS: u32 = 0x0C;
    /// 内边距一组 0x10..0x15：主题对 `lv_obj` 设的是前四个 20、后两个 11。
    /// 单个方向对应哪一个没逐个取证，清零时 0x10..0x13 四个一起清。
    pub const PAD_0: u32 = 0x10;
    pub const PAD_1: u32 = 0x11;
    pub const PAD_2: u32 = 0x12;
    pub const PAD_3: u32 = 0x13;
    pub const BG_COLOR: u32 = 0x1C;
    pub const BG_OPA: u32 = 0x1D;
    /// 文本块 0x58..0x5E：0x58 颜色 / 0x59 不透明度 / 0x5A 字体。
    pub const TEXT_COLOR: u32 = 0x58;
    pub const TEXT_OPA: u32 = 0x59;
    pub const TEXT_FONT: u32 = 0x5A;
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
