// 系统 UI 图标替换 —— LVGL 文件层资源重定向。
//
// 为什么要有这一条路: 桌面图标那条线改的是注册表节点 +0x0C(见 icon_apply.rs),
// 而日历磁贴底图 / 控制中心图标 / 设置页图标三处的路径是**固件代码里的常量**
// (不在注册表里), 改注册表那条线够不到它们。这里改成在 LVGL 读文件的入口上
// 换路径: 固件自己每次去读那个常量路径, 读到的就是我们的图。
//
// 机制(逐字反汇编, 静态核对):
//   lv_fs_open 0x0C169624 按首字符挑驱动, 命中盘符 '/' 之后**剥掉前导斜杠**再调
//   驱动的 open 回调(0x0C169684 `ldrb r3,[r5,#1]` / `cmp r3,#0x3a`(':') /
//   0x0C16968C `addne r1,r5,#1`), 回调槽在驱动 +0x0C。
//   驱动实例 0x20103340 由 0x0C1651B0 初始化: `movs r0,#0x2f`('/') 写 +0,
//   `mov.w r6,#0x1000` 写 +4(cache size), 原回调 0x1C057C51 写 +0x0C。
//   回调返回值: 0x0C16969A `subs r3,r0,#1` + `adds r3,#3` + `bhi` =>
//   **大于 0 才算打开成功**, 0 与 -1 都是失败哨兵(跳走那条返回 12)。
//
// 三条硬约束:
//   A. 只在"读 + 盘符 '/' 的驱动"上生效, 其余一律原样透传给原回调。
//   B. 换过去的文件打不开就**回退原路径**。最坏结果是显示固件原图标, 不是崩。
//      这也是"包里缺几张素材"能被容忍的原因。
//   C. 回调里不分配、不递归、不碰对象树。我们的目标路径在 /data/ 下,
//      不会命中任何 /resource/ 规则, 所以调原回调不会绕回自己。
//
// 生效时机: 控制中心与设置页每次进入都会重建页面、重新读图, 所以装好之后
// **退出重进页面即可看到**。已经显示在屏上的图不会自己变(那需要退役图像
// 缓存并让持有者重设 src, 是另一条线, 这里不做)。

use crate::mem::{rd8, rd32};
use crate::*;

// ===== 固件侧常量(逐条静态核对, 见文件头) =====
/// LVGL '/' 驱动实例
const FS_DRV: u32 = 0x2010_3340;
/// 驱动 +4 = cache size, 期望 4096(装 hook 前的门之一)
const FS_CACHE: u32 = 0x2010_3344;
/// 驱动 +0x0C = open 回调槽
const FS_OPEN_SLOT: u32 = 0x2010_334C;
/// 固件原本的 open 回调(PSRAM 执行别名, bit0=1 可直接调)
const FS_OPEN_ORIG: u32 = 0x1C05_7C51;
/// 只读打开(与 lv_fs_open 传给回调的 mode 一致)
const MODE_RD: u32 = 2;

// ===== 素材落点: 与桌面图标同一个包目录(icon_apply::ICON_DIR), 沿用同一套包号 =====
/// 与 icon_apply 的素材根目录同一个值: 系统 UI 素材和桌面图标在同一个包里,
/// 切包时两边一起换。
const ICON_DIR: &[u8] = b"/data/chaos/icons";
/// 拼好的目标路径缓冲: 目录 18 + '/' 1 + 包号 2 + '/' 1 + stem <=20 + ".bin" 4 + NUL
const RH_PATH_CAP: usize = 56;
static mut RH_PATH: [u8; RH_PATH_CAP] = [0; RH_PATH_CAP];
/// 资源缓存退役使用独立缓冲，文件回调不会覆盖它。
static mut RH_CACHE_PATH: [u8; 96] = [0; 96];
static mut RH_CACHE_BUSY: u32 = 0;
static mut RH_CACHE_RULE: u32 = 0;
static mut RH_CACHE_FRAME: u32 = 0;
/// LVGL 的解码缓存与头缓存实例槽。
const IMAGE_CACHE_SLOT: u32 = 0x2010_329C;
const HEADER_CACHE_SLOT: u32 = 0x2010_32A0;

/// 一条规则: 源目录前缀 + 源文件名 -> 包里的 stem。
///
/// 为什么是"目录 + 文件名"两段而不是只看文件名: 同名文件在固件的别处也有
/// (例如 `notify.bin` 在通知栏目录下同样存在), 只看文件名会把无关那一处也换掉。
struct Rule {
    /// 源目录前缀, 含结尾 '/'
    dir: &'static [u8],
    /// 源文件名(路径最后一段, 含 .bin)
    name: &'static [u8],
    /// 包里的素材 stem(与 build_icon_pack.py 出的文件名一致)
    stem: &'static [u8],
}

/// 系统 UI 图标映射表。stem 一律 `ctrl_` / `set_` / `cal_` 前缀, 与桌面那 38 个
/// 扁平 stem 隔开 —— 两者放在同一个包目录里, 名字撞了就等于换错图。
///
/// 素材侧的取名规则: CIPK 容器只接受 `^[A-Za-z0-9_]+\.bin$` 的扁平名(投递脚本
/// icon_pack.lua 按这条正则挡路径穿越), 所以这里不能写目录结构。
const RULES: [Rule; 24] = [
    // ---- 控制中心(快捷开关) ----
    Rule { dir: b"/resource/app/control_center/icon/", name: b"flashlight_all.bin", stem: b"ctrl_flashlight" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"setting_all.bin",    stem: b"ctrl_setting" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"battery_all.bin",    stem: b"ctrl_battery" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"bright_open.bin",    stem: b"ctrl_bright" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"alarm_all.bin",      stem: b"ctrl_alarm" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"findphone_all.bin",  stem: b"ctrl_findphone" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"disturb_open.bin",   stem: b"ctrl_disturb" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"raise_open.bin",     stem: b"ctrl_raise" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"game_mode.bin",      stem: b"ctrl_game" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"phone_conn.bin",     stem: b"ctrl_phone_conn" },
    Rule { dir: b"/resource/app/control_center/icon/", name: b"phone_disconn.bin",  stem: b"ctrl_phone_disconn" },
    // 勿扰动画与终态共用原尺寸画布，避免切帧时换回系统图。
    Rule { dir: b"/resource/app/control_center/icon/", name: b"dnd_0.bin",          stem: b"ctrl_dnd" },
    // ---- 设置页(每行左侧的小图) ----
    Rule { dir: b"/resource/app/settings/icon/", name: b"notify.bin",      stem: b"set_notify" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"desktop.bin",     stem: b"set_desktop" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"display.bin",     stem: b"set_display" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"disturb.bin",     stem: b"set_disturb" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"safe.bin",        stem: b"set_safe" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"battery.bin",     stem: b"set_battery" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"motion.bin",      stem: b"set_motion" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"preference.bin",  stem: b"set_preference" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"my_device.bin",   stem: b"set_mydevice" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"wrist.bin",       stem: b"set_wrist" },
    Rule { dir: b"/resource/app/settings/icon/", name: b"hr_broadcast.bin", stem: b"set_hr" },
    // ---- 日历磁贴底图 ----
    // 换这张**不会**盖掉日期: 星期与日期数字是固件画在图上的两枚文字, 它们照旧画,
    // 只是底图变成我们的。所以这张素材必须挑**浅色底** —— 深色底会让深色日期字糊掉。
    Rule { dir: b"/resource/app/perpetual_calendar/", name: b"calendar_background_icon.bin", stem: b"cal_background" },
];

/// 规则总数与按序取 stem: 删除流水线(icon_apply)要按同一张表清文件,
/// 否则删包会漏掉系统图标那一半, 留下删不掉的非空目录。
pub(crate) const RULE_N: usize = RULES.len();

pub(crate) fn rule_stem(k: usize) -> &'static [u8] {
    RULES[k].stem
}

// ===== 状态 =====
/// 0=未装 1=已装(槽里是我们的回调)
static mut RH_ON: u32 = 0;
/// 装之前从槽里读到的原回调(与 FS_OPEN_ORIG 双记录: 槽值可能因固件版本不同)
static mut RH_ORIG: u32 = 0;
/// 命中规则的次数 / 换路径后打开成功的次数 / 打开失败回退的次数(真机取证用)
static mut RH_HIT: u32 = 0;
static mut RH_OK: u32 = 0;
static mut RH_FALLBACK: u32 = 0;
/// 日历底图这条规则被 open 命中的次数(0 = 固件自装 hook 起没重读过底图)
static mut RH_CAL_HIT: u32 = 0;

// ---------------------------------------------------------------------------
// 字符串小工具(回调里不能用 fmt, 全部手写)
// ---------------------------------------------------------------------------

/// 安全读一个字节: 地址不可信就当字符串到此结束。
unsafe fn rb(p: u32) -> u8 {
    if !plausible_ptr(p) { return 0; }
    rd8(p as *const u8)
}

/// 路径长度(含结尾 NUL 的位置), 触到缓冲区上限就停。
unsafe fn path_len(p: u32, cap: usize) -> usize {
    let mut n = 0usize;
    while n < cap {
        if rb(p + n as u32) == 0 { break; }
        n += 1;
    }
    n
}

/// 从 `start` 起是否正好是 `s`(整段比对, 不要求结尾 NUL)
unsafe fn at_is(p: u32, start: usize, s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if rb(p + (start + i) as u32) != s[i] { return false; }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 查表: 把固件路径换成我们包里的 stem
// ---------------------------------------------------------------------------

/// 在 `abs`(已带前导 '/')里找最后一段 '/', 返回其后那一段的起点。
unsafe fn basename_at(abs: u32, n: usize) -> usize {
    let mut last = 0usize;
    let mut i = 0usize;
    while i < n {
        if rb(abs + i as u32) == b'/' { last = i + 1; }
        i += 1;
    }
    last
}

/// 勿扰动画帧号为 0..62，只接受固件生成的十进制文件名。
unsafe fn dnd_frame(abs: u32, base: usize, len: usize) -> bool {
    if len < 9 || len > 10 || !at_is(abs, base, b"dnd_")
        || !at_is(abs, base + len - 4, b".bin") {
        return false;
    }
    let digits = len - 8;
    let mut value = 0u32;
    let mut i = 0usize;
    while i < digits {
        let c = rb(abs + (base + 4 + i) as u32);
        if c < b'0' || c > b'9' || (digits > 1 && i == 0 && c == b'0') {
            return false;
        }
        value = value * 10 + (c - b'0') as u32;
        i += 1;
    }
    value <= 62
}

/// 命中规则返回 stem 序号; 没命中返回 None。
///
/// 目录前缀与文件名**两段都要对上**才换(见 RULES 上的说明)。
unsafe fn rule_match(abs: u32, n: usize) -> Option<usize> {
    let base = basename_at(abs, n);
    let name_len = n - base;
    let mut k = 0usize;
    while k < RULES.len() {
        let r = &RULES[k];
        // 文件名段: 长度相等且逐字节相同(不要求后面就是 NUL, 调用方已保证完整路径)
        if (name_len == r.name.len() && at_is(abs, base, r.name))
            || (r.stem == b"ctrl_dnd" && dnd_frame(abs, base, name_len)) {
            // 目录段: 从路径头开始比对前缀
            if base == r.dir.len() && at_is(abs, 0, r.dir) {
                return Some(k);
            }
        }
        k += 1;
    }
    None
}

/// 把 `/data/chaos/icons/<NN>/<stem>.bin` 写进 RH_PATH。pack 为 0 时写空串
/// (调用方会先看包号, 这里留空是为了让"忘了看"的代价是打不开文件)。
unsafe fn build_target(rule: usize, pack: u32) {
    let dst = core::ptr::addr_of_mut!(RH_PATH) as *mut u8;
    let mut o = 0usize;
    if pack < 1 || pack > 8 {
        write_volatile(dst, 0);
        return;
    }
    let mut i = 0usize;
    while i < ICON_DIR.len() {
        write_volatile(dst.add(o), ICON_DIR[i]);
        o += 1;
        i += 1;
    }
    write_volatile(dst.add(o), b'/'); o += 1;
    write_volatile(dst.add(o), b'0' + ((pack / 10) % 10) as u8); o += 1;
    write_volatile(dst.add(o), b'0' + (pack % 10) as u8); o += 1;
    write_volatile(dst.add(o), b'/'); o += 1;
    let s = RULES[rule].stem;
    i = 0;
    while i < s.len() && o < RH_PATH_CAP - 6 {
        write_volatile(dst.add(o), s[i]);
        o += 1;
        i += 1;
    }
    for c in b".bin" {
        if o < RH_PATH_CAP - 1 { write_volatile(dst.add(o), *c); o += 1; }
    }
    write_volatile(dst.add(o), 0);
}

// ---------------------------------------------------------------------------
// 回调本体: 装进驱动 +0x0C 的那个函数
// ---------------------------------------------------------------------------

/// LVGL '/' 驱动的 open 回调。
///
/// `path` **没有前导斜杠**(lv_fs_open 剥掉了; 见文件头), 查表前要自己加回去。
/// 返回值: 大于 0 才算成功(固件判据), 0 与 -1 都是失败。
#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_fs_open(drv: u32, path: u32, mode: u32) -> i32 {
    let orig = st_rd!(RH_ORIG);
    if orig == 0 { return -1; }
    let f: unsafe extern "C" fn(u32, u32, u32) -> i32 = core::mem::transmute(orig as usize);
    // 约束 A: 只在这一个驱动的只读打开上生效, 其余原样透传
    if drv != FS_DRV || mode != MODE_RD || path == 0 { return f(drv, path, mode); }

    let n = path_len(path, RH_PATH_CAP - 1);
    if n == 0 || n >= RH_PATH_CAP - 1 { return f(drv, path, mode); }

    // 加回前导斜杠: 规则表按带斜杠的绝对路径写, 这里拼在栈上的缓冲里
    let mut abs = [0u8; RH_PATH_CAP];
    abs[0] = b'/';
    let mut i = 0usize;
    while i < n {
        abs[i + 1] = rb(path + i as u32);
        i += 1;
    }
    abs[n + 1] = 0;

    match rule_match(abs.as_ptr() as u32, n + 1) {
        None => f(drv, path, mode),
        Some(k) => {
            // 缺少日历整图的包保留系统底图，避免接管一半日历。
            if RULES[k].stem == CAL_STEM && !icon_apply::calendar_available() {
                return f(drv, path, mode);
            }
            st_wr!(RH_HIT, st_rd!(RH_HIT).wrapping_add(1));
            // 日历底图单独计一次: 它有没有被读过, 直接区分"固件根本没重画"
            // 与"重画了但没用我们的图" —— 这两种的下一步完全不同。
            if RULES[k].stem.len() == CAL_STEM.len()
                && at_is(abs.as_ptr() as u32, 0, b"/resource/app/perpetual_calendar/") {
                st_wr!(RH_CAL_HIT, st_rd!(RH_CAL_HIT).wrapping_add(1));
            }
            let pack = icon_apply::cur_pack();
            if pack < 1 || pack > 8 { return f(drv, path, mode); }
            build_target(k, pack);
            // 目标路径剥掉前导斜杠再交给原回调 —— 它收到的从来都是这种形态
            let rel = (core::ptr::addr_of!(RH_PATH) as u32) + 1;
            let got = f(drv, rel, mode);
            if got > 0 {
                st_wr!(RH_OK, st_rd!(RH_OK).wrapping_add(1));
                got
            } else {
                // 约束 B: 素材缺失/打不开就回退, 最坏只是显示固件原图标
                st_wr!(RH_FALLBACK, st_rd!(RH_FALLBACK).wrapping_add(1));
                f(drv, path, mode)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 安装 / 卸载
// ---------------------------------------------------------------------------

/// 驱动在不在位。三条门: 盘符是 '/'、cache size 是 4096、槽里有回调。
/// 开机早期槽还是空的, 所以这个门每拍都过一次, 直到装上为止。
unsafe fn drv_ready() -> bool {
    if rd8(FS_DRV as *const u8) != b'/' { return false; }
    if rd32(FS_CACHE as *const u32) != 4096 { return false; }
    let slot = rd32(FS_OPEN_SLOT as *const u32);
    slot != 0
}

/// 装 hook。幂等: 槽里已经是我们自己就不重装(模块被重复加载时不会套两层)。
/// 原回调**以槽里的值为准**, FS_OPEN_ORIG 只是静态核对记下的期望值 ——
/// 真机上两者应当相等, 不等时以槽值为准才不会把调用指到错的地址。
pub(crate) unsafe fn install() {
    if st_rd!(RH_ON) != 0 { return; }
    if !drv_ready() { return; }
    let slot = rd32(FS_OPEN_SLOT as *const u32);
    if slot == chaos_fs_open as *const () as u32 { return; }
    if !plausible_ptr(slot) { return; }
    st_wr!(RH_ORIG, slot);
    write_volatile(FS_OPEN_SLOT as *mut u32, chaos_fs_open as *const () as u32);
    // 写回去读不出来就当没装成: 下一拍再试, 绝不留下"以为装了其实没装"
    if rd32(FS_OPEN_SLOT as *const u32) == chaos_fs_open as *const () as u32 {
        st_wr!(RH_ON, 1);
    }
}

/// 切换素材时登记一次资源缓存退役，实际工作由 UI 节拍执行。
pub(crate) unsafe fn refresh_cache() {
    st_wr!(RH_CACHE_RULE, 0);
    st_wr!(RH_CACHE_FRAME, 0);
    st_wr!(RH_CACHE_BUSY, 1);
}

pub(crate) unsafe fn pending() -> bool { st_rd!(RH_CACHE_BUSY) != 0 }

/// 每拍最多退役一个源路径，动画帧逐个展开。
unsafe fn refresh_cache_step() {
    if !pending() || !fw_api::screen_is_on() || st_rd!(RH_ON) == 0 { return; }
    if !plausible_ptr(rd32(IMAGE_CACHE_SLOT as *const u32))
        || !plausible_ptr(rd32(HEADER_CACHE_SLOT as *const u32)) { return; }
    let mut k = st_rd!(RH_CACHE_RULE) as usize;
    while k < RULES.len() {
        let rule = &RULES[k];
        // 蓝牙保持原图，日历整图由桌面流水线管理。
        if rule.stem == b"ctrl_phone_conn" || rule.stem == b"ctrl_phone_disconn"
            || rule.stem == CAL_STEM {
            k += 1;
            st_wr!(RH_CACHE_RULE, k as u32);
            continue;
        }
        let dst = core::ptr::addr_of_mut!(RH_CACHE_PATH) as *mut u8;
        let mut n = 0usize;
        for c in rule.dir { write_volatile(dst.add(n), *c); n += 1; }
        if rule.stem == b"ctrl_dnd" {
            for c in b"dnd_" { write_volatile(dst.add(n), *c); n += 1; }
            let frame = st_rd!(RH_CACHE_FRAME);
            if frame >= 10 { write_volatile(dst.add(n), b'0' + (frame / 10) as u8); n += 1; }
            write_volatile(dst.add(n), b'0' + (frame % 10) as u8); n += 1;
            for c in b".bin" { write_volatile(dst.add(n), *c); n += 1; }
            if frame < 62 { st_wr!(RH_CACHE_FRAME, frame + 1); }
            else { st_wr!(RH_CACHE_FRAME, 0); st_wr!(RH_CACHE_RULE, (k + 1) as u32); }
        } else {
            for c in rule.name { write_volatile(dst.add(n), *c); n += 1; }
            st_wr!(RH_CACHE_RULE, (k + 1) as u32);
        }
        write_volatile(dst.add(n), 0);
        fw_api::fw_img_free_by_path(dst as u32);
        return;
    }
    st_wr!(RH_CACHE_BUSY, 0);
}

/// 摘 hook, 把原回调写回去。模块卸载路径用; 正常运行时不调。
pub(crate) unsafe fn uninstall() {
    if st_rd!(RH_ON) == 0 { return; }
    let orig = st_rd!(RH_ORIG);
    if orig == 0 { return; }
    write_volatile(FS_OPEN_SLOT as *mut u32, orig);
    st_wr!(RH_ON, 0);
}

// ---------------------------------------------------------------------------
// 日历: 为什么不在这条线上换(2026-10-03 定案, 逐字反汇编)
// ---------------------------------------------------------------------------
//
// 043 上"日程"格的链路(0x0C4EFDE8, 日历 app 注册的 signal=6 回调):
//   sb = *(0x2010F05C); 为 0 时 0x0C587F38 分配 112x112 像素缓冲写回;
//   然后无论如何都 image_set_src(白盘字面量 0x2CB941A4) -> 建 112x112 图像对象
//   -> 挂星期/日期两枚 label -> 0x0CA5FB40 把整块(底图+文字)**光栅化进缓冲**
//   -> 成功则 `str r3,[sl]; str r3,[r4,#0xc]` 把**缓冲指针**写进注册表节点 +0x0C,
//      失败才回退写 `launcher.bin` 路径(0x0C4EFECC)。
// 即: 桌面画日历格读的是节点 +0x0C 上的 RAM 图像描述符(首字节 magic 0x19),
// 不是文件路径。所以:
//   * 底图文件只在 signal=6 重光栅化时才被 open —— 开机建过一次快照后固件不再读,
//     真机读数 `日0.0`(notify 调到了而 open 命中 0 次)与这段完全吻合;
//   * 就算把 open 换到我们的文件, 产物也只是又一块固件缓冲, 还会被下一次
//     signal=6 冲回 —— 这条线是死路;
//   * 正解在 icon_apply: 日历就是注册表里 app_id=69 的普通节点, 把 +0x0C 像另外
//     38 张一样指到我们的素材文件即可(节点的 +0x0C 平时被固件写成缓冲指针,
//     icon_stem_eq 的"可打印 ASCII"检查认不出它, 所以那边按 id 找节点)。
//
// 这条 cal_background 规则仍然保留: 固件哪天真重光栅化(比如系统主动发 signal=6),
// open 会命中它, 底图就换成我们的 —— 但不再依赖它, 也不再加 notify/缓存退役。
/// 日历底图在规则表里的 stem(open 命中计数用)
const CAL_STEM: &[u8] = b"cal_background";

/// 每拍尝试安装文件回调，并推进已登记的资源缓存退役。
pub(crate) unsafe fn tick() {
    if st_rd!(RH_ON) == 0 { install(); }
    refresh_cache_step();
}

/// 取证读数(界面用): 是否已装 / 命中 / 换成功 / 回退
pub(crate) unsafe fn counters() -> (u32, u32, u32, u32) {
    (st_rd!(RH_ON), st_rd!(RH_HIT), st_rd!(RH_OK), st_rd!(RH_FALLBACK))
}

/// 系统 UI 那一路的三个计数(图标页副标签显示, 用来分辨卡在哪一环):
/// (命中规则次数, 换成我们素材的次数, 素材缺失而回退的次数)
pub(crate) unsafe fn sys_counters() -> (u32, u32, u32) {
    (st_rd!(RH_HIT), st_rd!(RH_OK), st_rd!(RH_FALLBACK))
}

/// 日历底图那条规则的命中次数(0 = 固件自装 hook 起没重读过底图 —— 043 上
/// 这就是常态, 见"日历"一节; 只在固件自己发 signal=6 重光栅化时才会 >0)
pub(crate) unsafe fn cal_hit() -> u32 { st_rd!(RH_CAL_HIT) }
