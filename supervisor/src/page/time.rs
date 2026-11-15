//! 页2/3：时间与日期编辑，使用固件原生滚轮控件。

use crate::page::*;
use crate::page::Page;

// ===== 页2: 时间/日期设置(原生滚轮, 入口在关于页) =====
// 页2 原来是文件管理的第 2 级目录, 文件管理入口退役后这一格再没人进得去,
// 所以时间/日期编辑器占用它 —— 二级界面必须有独立 page_id, 只有真 page_goto/page_back
// 才有固件的压栈动画, 页内切层级没有。
// 滚轮整段逐字抄系统闹钟页 `FUN_0048C1E4`(两列分支 + 公共尾): 分隔标签、两个滚轮的
// 样式挂接、上下两条渐隐遮罩、底部那条 96x36 标签, 对象先后次序与字面值都不改。
// 取值走选中项文字 getter; 初值从 clock_gettime + localtime_r 取;
// 保存走 tick 调 time_set(写 RTC, 驱动同步系统时钟)。
pub(crate) const TIME_PID: usize = 2;
/// 日期设置(月+日)的独立 page_id。用页 3 —— 它原来是文件管理第三级，
/// 入口退役后一直不可达但保留注册，是 AGENTS 指定的首选可腾货源；注册页数不变(17)，
/// 不碰真机那条"原生应用最多 14 页"的上限。
pub(crate) const DATE_MD_PID: usize = 3;
static mut TJ_SAVE_REQ: u32 = 0;
static mut TJ_P1: u32 = 0;   // 页2 的两个轮句柄
static mut TJ_P2: u32 = 0;
static mut TJ_M1: u32 = 0;   // 页3 的两个轮句柄(与页2 分开: on_resume 只在 built==0 时重建,
static mut TJ_M2: u32 = 0;   // 从页3 返回页2 不会重跑 render, 共用槽会拿着已回收的句柄去刷)
static mut TJ_TM: [i32; 9] = [0; 9];
static mut TJ_MSG: [u8; 40] = [0; 40];
static mut TJ_HINT: u32 = 0;   // 页2 底部那条 96x36 提示标签
static mut TJ_HINT2: u32 = 0;  // 页3 的同一条标签
static mut TJ_V: [i32; 3] = [-1; 3];   // 每个轮上一次拍看到的选中值, 判"又滚过一格"用
static mut TJ_FB_UD: u32 = 0;          // 事件回调登记的轮号(1..3), 0 = 这拍没有
/// 当前蓝着的轮号(锁存), 0 = 还没人碰过、两列都是白。
/// **只由"哪一列被拨"改写, 不存在任何超时/松手把它清零的路径** —— 见 `tj_feedback`。
static mut TJ_ACTIVE: u32 = 0;

fn tj_leap(y: i32) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

fn tj_days(y: i32, m: i32) -> i32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if tj_leap(y) { 29 } else { 28 },
        _ => 31,
    }
}

unsafe fn tj_msg(s: &[u8]) {
    let m = core::ptr::addr_of_mut!(TJ_MSG) as *mut u8;
    let n = s.len().min(39);
    let mut i = 0usize;
    while i < n { write_volatile(m.add(i), s[i]); i += 1; }
    write_volatile(m.add(n), 0);
}

unsafe fn tj_fail(rc: i32) {
    let m = core::ptr::addr_of_mut!(TJ_MSG) as *mut u8;
    let pre = "失败 ".as_bytes();
    for (i, b) in pre.iter().enumerate() { write_volatile(m.add(i), *b); }
    let mut n = pre.len();
    let mut v = if rc < 0 { -rc } else { rc };
    let neg = rc < 0;
    let mut tmp = [b'0'; 10];
    let mut k = 0usize;
    loop {
        tmp[k] = b'0' + (v % 10) as u8;
        v /= 10;
        k += 1;
        if v == 0 || k == tmp.len() { break; }
    }
    if neg && n < 38 { write_volatile(m.add(n), b'-'); n += 1; }
    while k > 0 && n < 39 { k -= 1; write_volatile(m.add(n), tmp[k]); n += 1; }
    write_volatile(m.add(n), 0);
}

/// 当前前台那一屏有哪几个轮、哪条提示标签。
///
/// 时间/年份(页2)与日期(页3)是三个并列入口，同一时刻只有一页在被操作，句柄存在不同槽里，
/// 所以所有"碰控件"的动作都要先问这一句拿到的是哪一屏。判据用 `FG_PAGE`
/// (on_resume 设，页3 一旦前台就恒是 3)，不用一个全局"步骤"变量 —— 那样
/// 从页3 按返回键回到页2 时没人把它改回来，页2 会拿着页3 的轮去刷颜色。
unsafe fn tj_scr() -> ([u32; 3], u32) {
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID {
        ([st_rd!(TJ_M1), st_rd!(TJ_M2), 0], st_rd!(TJ_HINT2))
    } else {
        ([st_rd!(TJ_P1), st_rd!(TJ_P2), 0], st_rd!(TJ_HINT))
    }
}

/// 把当前前台那一屏的滚轮值写回 TJ_TM：时间屏动时/分，年份屏动年，月日屏动月/日。
unsafe fn tj_pick_values() {
    let (wheels, _) = tj_scr();
    let mut tm = core::ptr::read_volatile(core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
    let md = st_rd!(FG_PAGE) as usize == DATE_MD_PID;
    if md {
        if wheels[0] != 0 { let v = fw_api::zoom_picker_value(wheels[0]); if v >= 1 && v <= 12 { tm[4] = v - 1; } }
        if wheels[1] != 0 { let v = fw_api::zoom_picker_value(wheels[1]); if v >= 1 && v <= 31 { tm[3] = v; } }
    } else {
        // 页2 只有时分两轮(年份设置已删)
        if wheels[0] != 0 { let v = fw_api::zoom_picker_value(wheels[0]); if v >= 0 && v <= 23 { tm[2] = v; } }
        if wheels[1] != 0 { let v = fw_api::zoom_picker_value(wheels[1]); if v >= 0 && v <= 59 { tm[1] = v; } }
    }
    if tm[4] < 0 { tm[4] = 0; }
    if tm[4] > 11 { tm[4] = 11; }
    if tm[3] < 1 { tm[3] = 1; }
    let dmax = tj_days(tm[5] + 1900, tm[4] + 1);
    if tm[3] > dmax { tm[3] = dmax; }
    core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
}

/// 反馈状态归零：每建一屏都要走一遍。
/// 值缓存留 -1 表示"还没读到过"，第一次读到只登记不震 —— 建完页那一拍可视项可能还没挂齐。
unsafe fn tj_reset_feedback() {
    core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_V), [-1; 3]);
    st_wr!(TJ_ACTIVE, 0);
    st_wr!(TJ_FB_UD, 0);
    st_wr!(TJ_SAVE_REQ, 0);
}

unsafe fn tj_save_now() {
    tj_pick_values();
    let tm = core::ptr::read_volatile(core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
    let y = tm[5] + 1900;
    let mo = tm[4] + 1;
    let rc = crate::time_set::set(
        y as u32, mo as u32, tm[3] as u32, tm[2] as u32, tm[1] as u32, 0);
    if rc != 0 {
        tj_fail(rc);
    } else {
        // 立刻回读核对: ioctl 说成功但系统时钟没跟上的话, 这里会报出来
        let ok = match crate::time_set::now_tm() {
            Some(g) => g[5] + 1900 == y && g[4] + 1 == mo && g[3] == tm[3]
                && g[2] == tm[2] && g[1] == tm[1],
            None => false,
        };
        tj_msg(if ok { "已存".as_bytes() } else { "失败".as_bytes() });
    }
    crate::render_req(st_rd!(FG_PAGE));
}

/// 设置页那两行入口：各走自己的压栈跳转。
///
/// `target` 是这次要进的页号：时间 → 页2(时分)、日期 → 页3(月日)。两页平级，
/// 都直接从设置页进来。年份设置已按用户要求删除，不再有"先年再月日"那一步。
pub(crate) unsafe fn time_edit_enter(target: u32) {
    st_wr!(TJ_P1, 0); st_wr!(TJ_P2, 0);
    st_wr!(TJ_M1, 0); st_wr!(TJ_M2, 0);
    st_wr!(TJ_HINT, 0); st_wr!(TJ_HINT2, 0);
    tj_reset_feedback();
    match crate::time_set::now_tm() {
        Some(tm) => {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
            // 提示标签初始就是一个空格(闹钟页 24 小时制下填的正是空格), 拨哪个轮才写哪个字段名
            tj_msg(" ".as_bytes());
        }
        None => {
            let mut tm = [0i32; 9];
            tm[3] = 1; tm[4] = 0; tm[5] = 126; // 2026-01-01, 年 2000..2099 内
            core::ptr::write_volatile(core::ptr::addr_of_mut!(TJ_TM) as *mut [i32; 9], tm);
            tj_msg(" ".as_bytes());
        }
    }
    crate::nav_goto(target, 0);
}

/// 数字之间的分隔标签：逐字抄闹钟页 `FUN_0048C1E4` 冒号那一段。
///
/// 建标签 -> 设文本 -> `align(7, dx, 0)` -> 从共享样式 `0x2010D2E8` 取 face ->
/// 写进这个对象 -> 最后一步 `(obj, 0xff, 0)`。face 取自系统开机样式表，换过系统
/// 字体的设备上取到的就是那张脸，所以这里不写死任何字体名。取回 0 就只跳过写
/// 字体那一步，其余照抄。
unsafe fn tj_sep(parent: u32, text: *const u8, dx: i32) -> u32 {
    let lb = fw_api::label_create(parent);
    if lb == 0 { return 0; }
    fw_api::label_set_text(lb, text);
    fw_api::obj_align(lb, fw_api::align::X_ANCHOR, dx, 0);
    let face = fw_api::style_get_font(fw_api::COLON_FACE_STYLE);
    if face != 0 { fw_api::obj_set_style_text_font(lb, face, 0); }
    fw_api::obj_set_style_4c8da0(lb, 0xff, 0);
    lb
}

/// 建一个滚轮：逐字抄 `FUN_0048C1E4` 里两轮共用的那一段，只有宽度按列数给。
///
/// 建 -> 挂样式 `0x2010D300` -> `(p, 2, 0)` -> 设宽度 -> 3 行可见 -> 范围 -> 初值 ->
/// 循环模式 1 -> `align(7, dx, 0)`。
/// `val` 是**标签值**（年 2026、月 10、日 8），减 min 换算成控件要的偏移在函数里做。
/// 不设高度：闹钟页没设，高度由那条样式里的字体行高算出来。之前在这里硬写
/// 86x226，226 取的是闹钟页 AM/PM 那一支的值，两列分支用不上，结果是控件按
/// 我们给的高度排版、字体按默认主题算，选中行放大后就挤成一串四位数。
unsafe fn tj_picker(parent: u32, min: i32, max: i32, val: i32, w: i32, dx: i32) -> u32 {
    let p = fw_api::zoom_picker_create(parent);
    if p == 0 { return 0; }
    fw_api::style_apply(p, fw_api::PICKER_STYLE, 0xff, 0);
    fw_api::obj_set_style_4c8248(p, 2, 0);
    fw_api::obj_set_width(p, w);
    fw_api::zoom_picker_set_visible(p, 3);
    fw_api::zoom_picker_set_range(p, min, max, 1);
    // **这里必须减 min**：`set_value` 收的是范围内的**偏移**(0..项数-1)，不是标签值。
    // 时/分从 0 起，两种口径看不出差别；年(2000 起)、月/日(1 起)一减就错位 ——
    // 把 2026 传进去等于偏移 2026，而年轮只有 100 项，控件内部索引越界之后的表现就是
    // "乱跳、最后存下来的不是拨的那个数"（2026-10-18 真机，文档 §1 早已定案过这一条）。
    // 读值走 `zoom_picker_value`(读选中项文字)拿到的是**标签值**，所以写偏移、读标签，
    // 两边本来就不对称 —— 不要为了"对称"把这里改回标签值。
    let off = if val < min { 0 } else if val > max { max - min } else { val - min };
    fw_api::zoom_picker_set_value(p, off);
    fw_api::zoom_picker_set_mode(p, 1);
    fw_api::obj_align(p, fw_api::align::X_ANCHOR, dx, 0);
    p
}

/// 滚轮上下那两条渐隐遮罩：逐字抄 `FUN_0048C1E4` 尾部。
///
/// 建图 -> 设资源路径 -> 加 `0x40000` 那个标志 -> 304x32 -> 上条 `align(2, 0, 104)`、
/// 下条 `align(5, 0, -105)`。它们在滚轮之后创建，所以压在两轮上面，把上下两行淡出。
unsafe fn tj_mask(parent: u32, path: *const u8, top: bool) {
    let im = fw_api::background_image_create(parent);
    if im == 0 { return; }
    fw_api::background_image_set_source(im, path as u32);
    fw_api::background_image_set_floating(im);
    fw_api::obj_set_size(im, 0x130, 0x20);
    if top {
        fw_api::obj_align(im, fw_api::align::TOP_MID, 0, 0x68);
    } else {
        fw_api::obj_align(im, fw_api::align::BOTTOM_MID, 0, -0x69);
    }
}

/// 底部那条 96x36 标签：逐字抄 `FUN_0048C1E4` 尾部。
///
/// 建 -> 96x36 -> 挂系统默认样式 -> `(l, 2, 0)` -> 设文本 -> `align(2, 0, 75)` ->
/// 文字色 `0x277af7`。闹钟页在 24 小时制下给它填一个空格（那位置是 12 小时制放
/// AM/PM 的），我们把保存结果显示在同一条标签上，不再另加标签，保持对象数一致。
/// 句柄交给调用方存进自己那一屏的槽(页2 存 `TJ_HINT`、页3 存 `TJ_HINT2`)。
unsafe fn tj_hint(parent: u32, text: *const u8) -> u32 {
    let lb = fw_api::label_create(parent);
    if lb == 0 { return 0; }
    fw_api::obj_set_size(lb, 0x60, 0x24);
    fw_api::label_apply_defaults(lb, 0);
    fw_api::obj_set_style_4c8830(lb, 2, 0);
    fw_api::label_set_text(lb, text);
    fw_api::obj_align(lb, fw_api::align::TOP_MID, 0, 0x4b);
    fw_api::obj_style_color_a70(lb, 0x277af7, 0);
    lb
}

/// 把滚轮接进事件回调：与闹钟页宿主同一形状 —— 过滤器 0(收全部事件)，
/// user_data 放轮号(1/2/3)，回调体里再读回来区分是哪个轮被拨。
/// 注册一律放在设完初值之后，这样建页时的 set_value 不会触发反馈。
unsafe fn tj_bind_wheel(p: u32, no: u32) {
    if p == 0 { return; }
    let f: u32 = {
        let g: unsafe extern "C" fn(u32) -> u32 = chaos_tj_wheel_ev;
        g as u32
    };
    fw_api::obj_add_event(p, f, fw_api::ev::ALL, no);
}

/// 把滚轮的染色与底部标签写成"哪个轮在被拨"。
///
/// 三行动作(被拨的轮 `0x277af7`、其余 `0xffffff`、**每个轮**都重挂一遍文字)与
/// 闹钟页回调体 `FUN_004985E4` 一致；差别只在**调用它的上下文**：
/// 闹钟在自己的应用进程里、在事件派发链上直接做，我们这是内核模块挂在 LVGL 派发上，
/// 派发链里做这些实测会把系统打崩，所以放到拍上(lv_timer 上下文)做。
/// 只刷 `tj_scr()` 给的那一屏 —— 另一屏的轮这时可能已经不在对象树上了。
unsafe fn tj_paint(act: u32) {
    let (wheels, hint) = tj_scr();
    let mut i = 0usize;
    while i < 3 {
        let p = wheels[i];
        if p != 0 {
            fw_api::obj_style_color_a70(p, if act == (i as u32) + 1 {
                fw_api::WHEEL_COLOR_ACTIVE
            } else {
                fw_api::WHEEL_COLOR_IDLE
            }, 0);
            fw_api::zoom_picker_refresh(p);
        }
        i += 1;
    }
    if act != 0 && hint != 0 {
        let txt = tj_field_name(act - 1);
        if !txt.is_null() { fw_api::label_set_text(hint, txt); }
    }
}

/// 滚轮的反馈：在**拍上下文**里做，不在事件派发链里做。
///
/// 只有两条线：
/// - **蓝 = 最后被拨过的那一列**。系统闹钟页 `FUN_004985E4` 逐字读过：只认 7/9 两个码，
///   把被拨那一轮染 `0x277af7`、其余染 `0xffffff`，**整个函数没有任何回白分支**。
///   系统页的蓝就是"最后碰过的那一列留蓝"，直到拨另一列才换 —— 松手、停住、惯性滚完
///   都不变白。所以这里也不做回白：`TJ_ACTIVE` 只被"哪一列被拨"改写。
/// - **震 = 这一拍取值真的变了一格**，只有正在操作的那一个轮会被读。
///
/// 上一版那套"回白"已经删掉，它是蓝白闪烁的根因（真机两轮回报）：上一版每拍拿控件
/// 状态字 `obj+0x30` 与建屏基线比，想用它判"手指还在不在"。但那个字不是"按着"的稳定
/// 指示 —— 滚动过程中 LVGL 自己会动它，于是判据在 50ms 的拍之间反复翻面，
/// `TJ_ACTIVE` 被清零又立刻被下一发交互码拉回，屏幕就白一下蓝一下地闪。
/// 而"回白"这个需求本身在系统页里并不存在（见上），1:1 的做法就是不实现它。
unsafe fn tj_feedback() {
    let (wheels, _) = tj_scr();
    let cache = core::ptr::addr_of_mut!(TJ_V) as *mut i32;
    let touched = st_rd!(TJ_FB_UD) as usize;
    st_wr!(TJ_FB_UD, 0);
    // 这一拍没有交互事件：什么都不做。颜色保持不变，一个控件都不读。
    if touched == 0 { return; }
    if touched != st_rd!(TJ_ACTIVE) as usize {
        st_wr!(TJ_ACTIVE, touched as u32);
        let p = wheels[touched - 1];
        if p != 0 {
            let v = fw_api::zoom_picker_value(p);
            if v >= 0 { write_volatile(cache.add(touched - 1), v); }
        }
        tj_paint(touched as u32);
        return;
    }
    let p = wheels[touched - 1];
    if p == 0 { return; }
    let v = fw_api::zoom_picker_value(p);
    if v < 0 { return; }          // 这一拍没读到有效文字(正在重挂可视项): 不震
    let prev = read_volatile(cache.add(touched - 1));
    if prev < 0 {
        write_volatile(cache.add(touched - 1), v);   // 只补基准, 不震
        return;
    }
    if v != prev {
        write_volatile(cache.add(touched - 1), v);
        // 震动只在"这个轮真的又滚过一格"这一发：滚三格震三次，停住不滚就不震。
        // 固件那一拍由闹钟 app 把自己那个私有 ctx 注册进控制器之后由控制器发
        // (`(*(控制器 vtable +0x34/+0x38))(*0x20112520 的宿主, 轮)`)，内核模块没有等价 ctx，
        // 函数本体又在 PSRAM 侧，静态读不到，所以这里用平台自己的触觉入口做等效。
        // 该入口内部先读系统震动开关(0x2011942E)，用户关掉震动时自然不响。
        fw_api::vibrate(0x0d);
    }
}

/// 滚轮事件回调：只登记，一个固件调用都不做。
///
/// 只登记交互族 `{1 按下, 2 按住, 7 点击, 9 滚动开始, 11 滚动中, 12, 26, 30 取值变了}`
/// → `TJ_FB_UD`(轮号)，**其余事件码一概不记**（DRAW 系那一堆每帧都来，混进来就等于一直重绘）。
///
/// 抬手码 `{3 PRESS_LOST, 8 RELEASED}` 现在**不登记**：颜色不再有"松手收白"这一步
/// （系统闹钟页也没有，见 `tj_feedback`），抬手码没有任何消费者。
/// `SCROLL_END(10)` 同理两不沾 —— 甩动的惯性滚完就发一次，而手指往往还按在屏上；
/// 按住期间反复发的 `LONG_PRESSED(5)`/`LONG_PRESSED_REPEAT(6)` 也不是抬手。
/// 染色、重挂文字、换标签、震动一律由 `tj_feedback` 在拍(lv_timer)上下文里做。
#[no_mangle]
pub unsafe extern "C" fn chaos_tj_wheel_ev(event: u32) -> u32 {
    let code = fw_api::event_get_code(event);
    let ud = fw_api::event_user_data(event);
    if ud < 1 || ud > 3 { return 0; }
    if code == 1 || code == 2 || code == 7 || code == 9
        || code == 11 || code == 12 || code == 26 || code == 30 {
        st_wr!(TJ_FB_UD, ud);
    }
    0
}

/// 拨动那个轮时的字段名：时间屏用固件自己的消息号取当前语言的词(小时/分钟)，
/// 日期两屏没有对应的消息号，只能用字面的"年/月/日"。
/// 哪一屏看 `FG_PAGE`：页3 是月/日，页2 是时/分。
unsafe fn tj_field_name(idx: u32) -> *const u8 {
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID {
        if idx == 0 { "月\0".as_ptr() } else { "日\0".as_ptr() }
    } else {
        fw_api::translate(if idx == 0 { fw_api::HINT_KEY_HOUR } else { fw_api::HINT_KEY_MIN })
    }
}

/// 建完轮之后的公共尾部：遮罩、提示标签、反馈回调挂接、确认按钮。
///
/// 次序与闹钟页一致: 轮 -> 遮罩(压在轮上做渐隐) -> 底部标签 -> 挂反馈回调 -> 确认按钮。
/// 反馈回调必须挂在设完初值之后，否则建页时那发 `set_value` 会当成"用户拨了一格"。
/// `nh` 是这一屏的轮数(1 或 2)，取 `tj_scr()` 那一屏的槽。
unsafe fn tj_screen_tail(ctx: &PageCtx, nh: usize) -> PageResult {
    // 闹钟页宿主在建完两轮之后，对**第一个轮**清一次焦点态
    // (`0x0048C328`/`0x0048C524` 的 `(轮, 2)` = `clear_state(轮, FOCUSED)`)：
    // 它把轮注册进控制器那张组，组会把第一个成员自动置焦，所以建完要把它清掉，
    // 开局才有"没有一个轮是蓝的"。我们没有那条注册链，这一发按理是空操作，
    // 照抄是为了建轮之后的状态与闹钟页不留差别。
    let (wheels, _) = tj_scr();
    if wheels[0] != 0 {
        fw_api::obj_clear_state(wheels[0], fw_api::LV_STATE_FOCUSED);
    }
    // 基准值不在这里读: 建页时控件的可视项标签还没挂齐, 读回来是 -1,
    // 于是第一拍就把 -1 当"变了一格" —— 凭空震一下并染色(实测过), 交给第一拍去填。
    // 颜色也不在这里定: 开局两列都是白(`TJ_ACTIVE = 0`), 第一次拨哪一列才上蓝。
    tj_reset_feedback();
    tj_mask(ctx.content,
            b"/resource/app/settings/icon/time_top2.bin\0".as_ptr(), true);
    tj_mask(ctx.content,
            b"/resource/app/settings/icon/time_bottom2.bin\0".as_ptr(), false);
    let hint = tj_hint(ctx.content, core::ptr::addr_of!(TJ_MSG) as *const u8);
    if st_rd!(FG_PAGE) as usize == DATE_MD_PID { st_wr!(TJ_HINT2, hint); }
    else { st_wr!(TJ_HINT, hint); }
    let mut no = 1usize;
    while no <= nh {
        tj_bind_wheel(wheels[no - 1], no as u32);
        no += 1;
    }
    // 确认按钮抄宿主 `FUN_0049885E`: 工厂 -> 160x96 -> `align(5, 0, -16)` ->
    // confirm 图标 -> 底色 `0x222222` -> 挂 CLICKED。
    let btn = fw_api::confirm_btn_create(ctx.content);
    if btn == 0 { return Err(PageError::RowFailed(0)); }
    fw_api::obj_set_size(btn, 0xa0, 0x60);
    fw_api::obj_align(btn, fw_api::align::BOTTOM_MID, 0, -16);
    fw_api::btn_set_src(btn, b"/resource/app/common/icon/confirm.bin\0".as_ptr());
    fw_api::obj_style_color(btn, 0x222222, 0);
    fw_api::obj_add_event(btn, row_cb(), fw_api::ev::CLICKED, 0);
    Ok(())
}

/// 确认按钮的派发：三屏都是当场写入系统时钟。
///
/// 这里只做"登记"，真正的写 RTC 在拍上做 —— 那条链 open/read 设备节点，
/// 放在 UI 线程的事件回调里会阻塞到看门狗复位(早就定过的规矩)。
/// 三个入口各自只改自己那一组字段，其余字段取自进页时读到的 `TJ_TM`，
/// 所以"只改年份"不会顺手把月日改掉。
unsafe fn tj_confirm() {
    st_wr!(TJ_SAVE_REQ, 1);
}

/// 圆环中心文本: 只写百分比数字, 如 "45%"

// ===== 页2: 时间设置(时 + 分) =====
//
// 只做时分两轮, 与系统闹钟页同形。日期(月+日)是页3 的独立入口。
// 年份设置 2026-10-18 按用户要求删掉: 年轮那一整套(2000..2099 单列、TJ_MODE 分派)整段移除,
// 年份保持原值, 日期屏只改月日。
// 返回不走行控件 —— title_mode() 默认 1, 标题栏自带返回键, 与其它二级页一致。
pub(crate) struct TimeSetPage;
pub(super) static PAGE_TIMESET: TimeSetPage = TimeSetPage;

impl Page for TimeSetPage {
    /// 整页口径: 336x480 摆在 y=0、底部不留白。滚轮那一串字面偏移(滚轮 dy=0、
    /// 遮罩 +104/-105、提示标签 +75、确认按钮 -16)是按"content 铺满整页"算的,
    /// 照抄偏移就必须照抄基准 —— 详见 page.rs 的 enum Geom 注释。
    fn geometry(&self) -> Geom { Geom::FullBleed }

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "时间设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let tm = core::ptr::read_volatile(
                core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
            // 横向位置按闹钟页的字面值抄。它的 7 号对齐把 x 当**左边缘**偏移算，
            // 所以两列(宽 86)落在 60..146 与 190..276，右边距同样是 60，
            // 冒号 160 正好处在 146..190 那条缝里。
            tj_sep(ctx.content, ":\0".as_ptr(), 160);
            let p1 = tj_picker(ctx.content, 0, 23, tm[2], 86, 60);
            let p2 = tj_picker(ctx.content, 0, 59, tm[1], 86, 190);
            st_wr!(TJ_P1, p1); st_wr!(TJ_P2, p2);
            if p2 == 0 { tj_msg("无轮".as_bytes()); }
            tj_screen_tail(ctx, 2)
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe { if idx == 0 { tj_confirm(); } }
    }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) {
        unsafe {
            // 反馈排在保存之前: tj_save_now 会登记整页重建, 登记完再碰控件句柄没意义
            tj_feedback();
            if st_rd!(TJ_SAVE_REQ) != 0 {
                st_wr!(TJ_SAVE_REQ, 0);
                tj_save_now();
            }
        }
    }
}

// ===== 页3: 日期设置(月 + 日) =====
//
// 这一格原来是文件管理第三级目录，入口退役后一直不可达。二级界面必须有独立 page_id
// (只有真 page_goto/page_back 才有固件的压栈动画)，所以日期设置占它 —— 它是与页2 平级
// 的独立入口(关于页第三行), 不再由年屏压栈进入。
// 两列几何逐字用闹钟页那一套: 86 宽、左沿 60/190、分隔符 x=160。
// 句柄存 TJ_M1/TJ_M2，与页2 分开 —— 从页3 返回页2 不会重跑 render(框的 built 还置着)，
// 共用槽会让页2 拿着已经回收的轮去刷颜色。
pub(crate) struct DateMDPage;
pub(super) static PAGE_DATE_MD: DateMDPage = DateMDPage;

impl Page for DateMDPage {
    /// 与页2 同口径: 闹钟页那组两列几何按整页可用区算。
    fn geometry(&self) -> Geom { Geom::FullBleed }

    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "日期设置\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            let tm = core::ptr::read_volatile(
                core::ptr::addr_of!(TJ_TM) as *const [i32; 9]);
            tj_sep(ctx.content, "/\0".as_ptr(), 160);
            // 轮上的取值是 1 基(月 1..12、日 1..31)，而 tm 里 tm[4] 是 0 基、tm[3] 是 1 基，
            // 所以初值各加对应偏移 —— 上一版直接把 tm[4] 塞进一个 min=1 的轮，月份偏一格。
            // 日的上限固定 31，跨月多出来的那几天在写回时按年月夹掉(tj_pick_values)。
            let p1 = tj_picker(ctx.content, 1, 12, tm[4] + 1, 86, 60);
            let p2 = tj_picker(ctx.content, 1, 31, tm[3], 86, 190);
            st_wr!(TJ_M1, p1); st_wr!(TJ_M2, p2);
            if p2 == 0 { tj_msg("无轮".as_bytes()); }
            tj_screen_tail(ctx, 2)
        }
    }
    fn on_click(&self, idx: usize) {
        unsafe { if idx == 0 { tj_confirm(); } }
    }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) {
        unsafe {
            tj_feedback();
            if st_rd!(TJ_SAVE_REQ) != 0 {
                st_wr!(TJ_SAVE_REQ, 0);
                tj_save_now();
            }
        }
    }
}
