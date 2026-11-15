#![no_std]
//! p65（Redmi Watch 6）Chaos 模块骨架。
//!
//! 只使用已经执行确认过的固件入口：系统原生页标题、系统原生列表行类、
//! 通用样式属性、标签、定时器、对象删除，以及"由固件打开应用"那一发。
//! 触摸回调一律只登记，真正的动作放在定时器拍里做。

use core::panic::PanicInfo;

// ---- 固件入口 ----
/// 系统原生列表行的类：系统运动页里 lv_img 的父对象。

const SET_PROP: usize = 0x0037_E409;
const ADD_FLAG: usize = 0x0038_7B99;
const ADD_EVENT: usize = 0x0C25_70BD;
const LABEL_SET_TEXT: usize = 0x0C78_C879;
const DEL_OBJ: usize = 0x0037_FE3F;
const TIMER_CREATE: usize = 0x0C74_14D9;
const TITLE_CREATE: usize = 0x0C21_B2FD;
const MFONT: usize = 0x0C21_88ED;
const APP_LOOKUP: usize = 0x1C15_1109;
const APP_INSTALL: usize = 0x1C15_1AE5;
/// 把应用挂到桌面：只在桌面记录链已经有图标、且已排版时调用一次。
const LAUNCHER_ADD: usize = 0x0C7E_C451;
/// 重读应用描述符里的图标路径与显示名，并刷新桌面记录。
const LAUNCHER_REFRESH: usize = 0x0C7E_C745;
/// 桌面记录链 {+0x00 计数, +0x04 首节点, +0x08 尾节点}；节点 +0x10 是图标对象。
const DESK_LIST: usize = 0x3C3B_F9DC;
/// 固件"打开应用"那一发：入参结构 `+0x06`(u16) = app_id。
const OPEN_APP: usize = 0x1C13_4189;

// ---- 样式属性号（本机实测，与 043 不同）----
const P_WIDTH: u32 = 0x01;
const P_HEIGHT: u32 = 0x02;
const P_X: u32 = 0x08;
const P_Y: u32 = 0x09;
const P_BG_COLOR: u32 = 0x1C;
const P_BG_OPA: u32 = 0x1D;

// ---- 系统原生控件入口（抄自系统「设置」页的行构建流程）----
// 那条原生行 = 一张 416x120 的卡片 + 系统标签（标题 + 副标题），
// 下面这些是固件自己的入口，调用次序与系统设置页逐条对应。
/// 设尺寸
const FW_SIZE: usize = 0x0C25_5CE5;
/// 设位置
const FW_POS: usize = 0x0C25_68ED;
/// 设底色
const FW_BG: usize = 0x0C25_7B3D;
/// 设内边距
const FW_PAD: usize = 0x0C25_6665;
/// 清标志
const FW_CLR_FLAG: usize = 0x0C25_6B0D;
/// 置标志
const FW_SET_FLAG: usize = 0x0C25_6A1D;
/// 刷新布局
/// 系统标签的主样式
const FW_STYLE_MAIN: usize = 0x0C21_E581;
/// 系统标签的副样式
const FW_STYLE_SUB: usize = 0x0C21_E089;
/// 系统标签类（框架自己的标签类，基类才是通用容器）
/// 系统标签的文字标记
const FW_LABEL_FLAG: usize = 0x0C78_C995;
/// 卡片清样式一
const FW_CLR1: usize = 0x0C25_83B5;
/// 卡片清样式二
const FW_CLR2: usize = 0x0C25_758D;
/// 卡片清样式三
const FW_CLR3: usize = 0x0C25_6455;
/// 卡片属性 0x30 清零
const FW_CLR4: usize = 0x0C25_6765;
/// 卡片建好后清的第一组样式
const FW_CLR0: usize = 0x0C25_8025;
/// 卡片整体样式（系统建完卡片后调的那一发）
const FW_CARD_STYLE: usize = 0x0C92_E9C5;
/// 页面样式一
const FW_PAGE1: usize = 0x0C25_8125;
/// 页面样式二
const FW_PAGE2: usize = 0x0C25_771D;
/// 页面样式三
const FW_PAGE3: usize = 0x0C25_86FD;
/// 页面状态
const FW_PAGE4: usize = 0x0C25_8A5D;
/// 滚动条那一发：系统在置完 SCROLLABLE 之后紧接着调的那一发
const FW_SCROLLBAR: usize = 0x0037_AFD9;
/// 页面压栈跳转（与 10P 的 page_goto 同形，四参）
const FW_PAGE_GOTO: usize = 0x0C22_2D11;


/// 本应用在系统应用表里的 id。桌面网格按 app_id 升序排，id 太大就落到第二页；
/// 系统已用的 id 最���是 3（活力指标 19、运动 15…），2 是空位。
const APP_ID: u32 = 6;
const SCREEN_W: u32 = 432;
const SCREEN_H: u32 = 514;

static PKG: &[u8] = b"chaos\0";
static ICON: &[u8] = b"/data/chaos_icon.bin\0";
static ROW_NAME: &[u8] = b"chaosmain\0";
static FONT_NAME: &[u8] = b"MiSans-Regular\0";
/// 桌面图标下的名字。固件把应用描述符 +0x1C 当函数调、返回值当字符串用，
/// 留 0 会在挂桌面图标时调空指针，表现为整机 UI 卡死。
static APP_NAME: &[u8] = b"Chaos\0";

static T_CHAOS: &[u8] = &[0x43, 0x68, 0x61, 0x6F, 0x73, 0x00];
static T_ABOUT: &[u8] = &[0xE5, 0x85, 0xB3, 0xE4, 0xBA, 0x8E, 0x00];
static K_KERNEL: &[u8] = &[0xE5, 0x86, 0x85, 0xE6, 0xA0, 0xB8, 0x00];
static K_SCREEN: &[u8] = &[0xE5, 0xB1, 0x8F, 0xE5, 0xB9, 0x95, 0x00];
static K_MEM: &[u8] = &[0xE5, 0x86, 0x85, 0xE5, 0xAD, 0x98, 0x00];
static K_CPU: &[u8] = b"CPU\0";
static K_STORE: &[u8] = &[0xE5, 0xAD, 0x98, 0xE5, 0x82, 0xA8, 0x00];
static K_MODULE: &[u8] = &[0xE6, 0xA8, 0xA1, 0xE5, 0x9D, 0x97, 0x00];
/// 关于页（与 10P 同形）：版本 / 编译时间 / 运行固件 / 三位开发者 / 许可证。
static V_APPVER: &[u8] = b"v1.0.3\0";
static K_BUILD: &[u8] = &[0xE7, 0xBC, 0x96, 0xE8, 0xAF, 0x91, 0xE6, 0x97, 0xB6, 0xE9, 0x97, 0xB4, 0x00];
static K_FW: &[u8] = &[0xE8, 0xBF, 0x90, 0xE8, 0xA1, 0x8C, 0xE5, 0x9B, 0xBA, 0xE4, 0xBB, 0xB6, 0x00];
static K_LICENSE: &[u8] = &[0xE8, 0xAE, 0xB8, 0xE5, 0x8F, 0xAF, 0xE8, 0xAF, 0x81, 0x00];
static V_DEV: &[u8] = &[0xE5, 0xBC, 0x80, 0xE5, 0x8F, 0x91, 0xE8, 0x80, 0x85, 0x00];
static V_LICENSE: &[u8] = b"AGPL-3.0\0";
static K_DEV1: &[u8] = &[0xE6, 0x96, 0x87, 0xE5, 0x8D, 0x8E, 0xE9, 0x80, 0xB8, 0xE6, 0xB4, 0x8B, 0x00];
static K_DEV2: &[u8] = b"IKUN_CXKPRO\0";
static K_DEV3: &[u8] = &[0xE8, 0xB0, 0xA6, 0xE8, 0x99, 0x9A, 0xE7, 0x9A, 0x84, 0xE6, 0xB5, 0xB7, 0xE9, 0xB8, 0xA5, 0x00];
const BUILD_DATE: &str = match option_env!("CHAOS_BUILD_DATE") {
    Some(s) => s,
    None => "未知",
};
static mut V_BUILD: [u8; 24] = [0; 24];
unsafe fn build_stamp() {
    let b = BUILD_DATE.as_bytes();
    let n = if b.len() < 23 { b.len() } else { 23 };
    let mut i = 0usize;
    while i < n {
        V_BUILD[i] = b[i];
        i += 1;
    }
    V_BUILD[n] = 0;
}
static V_KERNEL: &[u8] = b"p65 3.100.043\0";
static V_SCREEN: &[u8] = b"432x514\0";
static V_WAIT: &[u8] = b"--\0";
static V_MODULE: &[u8] = b"chaos_p65\0";
static K_NUTTX: &[u8] = b"NuttX\0";
static mut V_NUTTX: [u8; 32] = [0; 32];
// 应用管理：标题、翻页行
static T_APPS: &[u8] = &[0xE5, 0xBA, 0x94, 0xE7, 0x94, 0xA8, 0xE7, 0xAE, 0xA1, 0xE7, 0x90, 0x86, 0x00];

/// 装载后是否自己把应用打开：正式版走桌面图标，调试冒烟用 `autoopen`。
const AUTO_OPEN: bool = cfg!(feature = "autoopen");
/// 调试用：装载后由固件打开哪个应用。默认开本应用；`opensettings` 开系统「设置」(id 22)，
/// 用来取系统原生列表页的视觉基准。
const OPEN_TARGET: u32 = if cfg!(feature = "opensettings") { 22 } else { APP_ID };

const PAGE_MENU: u32 = 0;
const PAGE_ABOUT: u32 = 1;
const PAGE_APPS: u32 = 2;
const PAGE_CACHE: u32 = 3;
const PAGE_BRIGHT: u32 = 4;
const PAGE_FILES: u32 = 5;
const PAGE_DESK: u32 = 6;
const PAGE_VIB: u32 = 7;
const PAGE_STORAGE: u32 = 8;
const PAGE_WATCH: u32 = 9;
const PAGE_SETTINGS: u32 = 10;
const PAGE_BEAUTIFY: u32 = 11;
const PAGE_WFMGMT: u32 = 12;
const PAGE_LOG: u32 = 13;
const PAGE_N: usize = 14;

// ---- 时间/日期页：原生滚轮 + 写时钟（全部执行验证过的锚点）----
const PICKER_CREATE: usize = 0x0C24_0411;
const PICKER_SET_RANGE: usize = 0x0C24_061D;
const PICKER_SET_MODE: usize = 0x0C24_0435;
const PICKER_SET_VALUE: usize = 0x0C23_77A9;
const PICKER_REFRESH: usize = 0x0C24_0893;
const CLOCK_SETTIME: usize = 0x0C4C_1FB5;
const CLOCK_GETTIME: usize = 0x0C4C_205D;

#[repr(C)]
struct Ts {
    sec: u64,
    nsec: i32,
}

static mut TIME_H: u32 = 0;
static mut TIME_M: u32 = 0;
static mut TIME_MSG: [u8; 32] = [0; 32];

/// 建一个原生滚轮：范围 [min,max]、循环、初值 value（实际值）。
unsafe fn picker_new(parent: u32, x: u32, y: u32, w: u32, h: u32, min: i32, max: i32, value: i32) -> u32 {
    let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
    let p = create(parent);
    if p == 0 {
        return 0;
    }
    // 尺寸/位置必须先给：标签的位置是在 refresh 里按当前尺寸算的
    fw_size(p, w, h);
    fw_pos(p, x, y);
    let setr: unsafe extern "C" fn(u32, i32, i32, i32) = core::mem::transmute(PICKER_SET_RANGE);
    setr(p, min, max, 1);
    let setm: unsafe extern "C" fn(u32, i32) = core::mem::transmute(PICKER_SET_MODE);
    setm(p, 1);
    let n = max - min + 1;
    let mut idx = value - min;
    if idx < 0 {
        idx = 0;
    }
    // 核心有 count <= index 的前置门：这里必须传 0..n-1，循环窗由控件自己折算
    let setv: unsafe extern "C" fn(u32, i32, i32) = core::mem::transmute(PICKER_SET_VALUE);
    setv(p, idx, n);
    let refr: unsafe extern "C" fn(u32) = core::mem::transmute(PICKER_REFRESH);
    refr(p);
    p
}

/// 取滚轮当前选中值：+0x94 首可见 + +0xFC 窗内偏移（循环模式再减两份）。
unsafe fn picker_value(p: u32, min: i32, max: i32) -> i32 {
    if p == 0 {
        return min;
    }
    let n = max - min + 1;
    let start = rd32(p as usize + 0x94) as i32;
    let off = (rd32(p as usize + 0xFC) & 0xFF) as i32;
    let sel = start + off;
    let mut v = sel % n;
    if v < 0 {
        v += n;
    }
    min + v
}

unsafe fn clock_now() -> u64 {
    let get: unsafe extern "C" fn(u32, *mut Ts) -> i32 = core::mem::transmute(CLOCK_GETTIME);
    let mut t = Ts { sec: 0, nsec: 0 };
    let _ = get(0, &mut t);
    t.sec
}

unsafe fn clock_set(sec: u64) -> i32 {
    let set: unsafe extern "C" fn(u32, *const Ts) -> i32 = core::mem::transmute(CLOCK_SETTIME);
    let t = Ts { sec, nsec: 0 };
    set(0, &t)
}

/// 1970-01-01 起的天数 -> (年,月,日)。
fn civil_from_days(z0: i64) -> (i32, u32, u32) {
    let z = z0 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + if m <= 2 { 1 } else { 0 }) as i32, m, d)
}

/// (年,月,日) -> 1970-01-01 起的天数。
fn days_from_civil(y0: i32, m: u32, d: u32) -> i64 {
    let y = y0 as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// 本地时区偏移（秒）。这台机器是 UTC+8。
const TZ_OFF: i64 = 8 * 3600;

/// 保存按钮。
static K_SAVE: &[u8] = &[0xE4, 0xBF, 0x9D, 0xE5, 0xAD, 0x98, 0];
/// "返回"（10P 二级页底部那条独立行）。
static K_BACK: &[u8] = &[0xE8, 0xBF, 0x94, 0xE5, 0x9B, 0x9E, 0];
/// "更换字体"。
static K_FONT_MENU: &[u8] = &[0xE6, 0x9B, 0xB4, 0xE6, 0x8D, 0xA2, 0xE5, 0xAD, 0x97, 0xE4, 0xBD, 0x93, 0];
static K_BLUR_MENU: &[u8] = "系统模糊\0".as_bytes();
static K_RESTORE_UI: &[u8] = "重启恢复字体和图标\0".as_bytes();
static K_RESTORE_BLUR: &[u8] = "重启恢复模糊\0".as_bytes();
static K_SYS_BG: &[u8] = "系统背景\0".as_bytes();
static K_STATIC_BG: &[u8] = "静态背景图(表盘)\0".as_bytes();
static K_AUTO_BG: &[u8] = "自动截取替换图片\0".as_bytes();
static K_GEN_BG: &[u8] = "手动生成或更新表盘背景\0".as_bytes();
static K_TRANSPARENT_BG: &[u8] = "完全透明背景（实验性）\0".as_bytes();
static K_ZONE_MENU: &[u8] = "模糊区域\0".as_bytes();
static K_SYS_ICON: &[u8] = "系统原图标\0".as_bytes();
static K_FW_ICON: &[u8] = "固件原图标\0".as_bytes();
static V_IN_USE: &[u8] = "使用中\0".as_bytes();
static V_MISSING: &[u8] = "文件缺失\0".as_bytes();
static V_SLOT: &[u8] = "槽位 \0".as_bytes();
static V_DELETE_AGAIN: &[u8] = "再点一次删除 \0".as_bytes();
static V_SAVE_OK: &[u8] = "记录选择，下次运行自动应用\0".as_bytes();
static V_SAVE_BUSY: &[u8] = "正在保存设置\0".as_bytes();
static T_BLUR: &[u8] = "系统模糊\0".as_bytes();
static T_ZONES: &[u8] = "模糊区域\0".as_bytes();
static K_ZONE0: &[u8] = "启动器\0".as_bytes();
static K_ZONE1: &[u8] = "控制中心\0".as_bytes();
static K_ZONE2: &[u8] = "通知中心\0".as_bytes();
static K_ZONE3: &[u8] = "小部件\0".as_bytes();
const BVIEW_ICON: u32 = 0;
const BVIEW_FONT: u32 = 1;
const BVIEW_BLUR: u32 = 2;
const BVIEW_ZONE: u32 = 3;
/// 系统动画返回上一级：入口 FUN_00162620，参数 0 = 弹当前页（自触发探针 <0.5s 生效）。
const PAGE_BACK: usize = 0x0C22_2621;
/// 上次点"返回"的拍号：2 秒冷却，防连点弹两层。
static mut BACK_TICK: u32 = 0;
/// 给对象写 text_font 的属性号（10P 侧实证 0x5A）。
const P_TEXT_FONT: u32 = 0x5A;
/// 本页建出来的标签（换字体时逐个写 text_font）。
static mut LBL_TAB: [u32; 64] = [0; 64];
static mut LBL_N: u32 = 0;
static mut FONT_FACE: u32 = 0;
/// 换过字体：其它页面在成为当前页时重建一次（新标签创建时就套新脸）。
static mut FONT_DIRTY: u32 = 0;
/// 系统美化的二级视图：0=桌面图标 1=更换字体 2=系统模糊 3=模糊区域。
static mut FONT_VIEW: u32 = 1;
static mut FONT_N: u32 = 0;
static mut FONT_NAMES: [[u8; 24]; 12] = [[0; 24]; 12];
static mut FONT_IDS: [u32; 12] = [0; 12];
static mut FONT_FACES: [u32; 12] = [0; 12];
static mut FONT_SYS_FACE: u32 = 0;
static mut FONT_SUB: [[u8; 24]; 12] = [[0; 24]; 12];
static mut FONT_CUR: u32 = u32::MAX;
static mut FONT_DELCONF: u32 = 0;
static mut FONT_DEL_REQ: u32 = 0;
static mut FONT_GEN: u32 = 0;
static mut FONT_REG_NAME: [u8; 24] = [0; 24];
const FONT_INDEX: &[u8] = b"/data/chaos/font/index.txt\0";
const FONT_DIR: &[u8] = b"/data/chaos/font";
const FONT_REG: usize = 0x0C21_88B9;
const FONT_MANAGER_SYS: &[u8] = b"/resource/font/MiSans-Regular.ttf\0";
/// 系统美化的选择记录：重启后是否恢复字体/图标，以及上次选择的字体名。
/// 10P 的“重启恢复字体和图标”对应这张记录；记录坏了就退回默认值，不猜。
const BEAUTY_CFG: &[u8] = b"/data/chaos/beautify.cfg\0";
static mut BEAUTY_LOADED: u32 = 0;
static mut BEAUTY_RESTORE_ON: u32 = 1;
static mut BEAUTY_BLUR_RESTORE_ON: u32 = 1;
static mut BEAUTY_SAVE_REQ: u32 = 0;
static mut BEAUTY_FONT_NAME: [u8; 24] = [0; 24];
/// 图标包清单（与 10P 同一套定长记录）：包号 1..8，包号 0 是系统原图标。
const ICON_INDEX: &[u8] = b"/data/chaos/icons/index.txt\0";
static mut ICON_PACK_N: u32 = 0;
static mut ICON_PACK_ID: [u32; 8] = [0; 8];
static mut ICON_PACK_NAME: [[u8; 16]; 8] = [[0; 16]; 8];
static mut ICON_PACK_CUR: u32 = 0;
static mut ICON_PACK_DIRTY: u32 = 0;
/// 模糊区域掩码：位 0..3 = 启动器 / 控制中心 / 通知中心 / 小部件。
static mut BLUR_ZONE_MASK: u32 = 0x0F;
static mut BLUR_MODE: u32 = 0;
/// 换图标时保留桌面记录引用的原路径。10P 用堆副本避免固件 free 只读段；
/// RW6 的桌面记录本来就是从描述符 +0x0C 现读，这里每个应用各留一格镜像路径，
/// 换包与恢复都从这一格取。
const B_ICON_MAX: usize = 40;
const B_ICON_PATH_CAP: usize = 80;
static mut B_ICON_ID: [u32; B_ICON_MAX] = [0; B_ICON_MAX];
static mut B_ICON_ORIG: [u32; B_ICON_MAX] = [0; B_ICON_MAX];
static mut B_ICON_PATH: [[u8; B_ICON_PATH_CAP]; B_ICON_MAX] =
    [[0; B_ICON_PATH_CAP]; B_ICON_MAX];
static mut B_ICON_N: u32 = 0;
const B_ICON_NONE: u32 = u32::MAX;
static mut B_ICON_REQ: u32 = B_ICON_NONE;
static mut B_ICON_PLAN: [u32; B_ICON_MAX] = [0; B_ICON_MAX];
static mut B_ICON_PLAN_N: u32 = 0;
static mut B_ICON_CURSOR: u32 = 0;

/// 扫 /resource/font 下 *.ttf，存去掉扩展名的名字（vg_font_create_named 要的是名字）。
unsafe fn font_scan() {
    FONT_N = 0;
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR | 1);
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR | 1);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR | 1);
    let dirp = od(b"/resource/font\0".as_ptr());
    if dirp == 0 {
        return;
    }
    loop {
        let de = rd(dirp);
        if de == 0 {
            break;
        }
        let nm = (de + 1) as *const u8;
        let mut n = 0usize;
        while n < 23 {
            let c = core::ptr::read_volatile(nm.add(n));
            if c == 0 {
                break;
            }
            n += 1;
        }
        let ext_ok = n > 4
            && core::ptr::read_volatile(nm.add(n - 4)) == b'.'
            && (core::ptr::read_volatile(nm.add(n - 3)) | 0x20) == b't'
            && (core::ptr::read_volatile(nm.add(n - 2)) | 0x20) == b't'
            && (core::ptr::read_volatile(nm.add(n - 1)) | 0x20) == b'f';
        // 只列能显示中文的 MiSans 一族：N62SFontVF 没有中文字形，选中后整页中文会变方块
        let mi = n > 6
            && core::ptr::read_volatile(nm.add(0)) == b'M'
            && core::ptr::read_volatile(nm.add(1)) == b'i'
            && core::ptr::read_volatile(nm.add(2)) == b'S'
            && core::ptr::read_volatile(nm.add(3)) == b'a'
            && core::ptr::read_volatile(nm.add(4)) == b'n'
            && core::ptr::read_volatile(nm.add(5)) == b's';
        if ext_ok && mi && (FONT_N as usize) < FONT_NAMES.len() {
            let row = FONT_N as usize;
            let dst = (core::ptr::addr_of_mut!(FONT_NAMES) as *mut u8).add(row * 24);
            let mut k = 0usize;
            while k + 4 < n {
                core::ptr::write_volatile(dst.add(k), core::ptr::read_volatile(nm.add(k)));
                k += 1;
            }
            core::ptr::write_volatile(dst.add(k), 0);
            FONT_IDS[row] = FONT_N + 1;
            FONT_N += 1;
        }
    }
    cd(dirp);
}

unsafe fn font_path(dst: *mut u8, slot: u32) {
    let mut o = 0usize;
    while FONT_DIR[o] != 0 {
        core::ptr::write_volatile(dst.add(o), FONT_DIR[o]);
        o += 1;
    }
    for b in b"/st" {
        core::ptr::write_volatile(dst.add(o), *b);
        o += 1;
    }
    core::ptr::write_volatile(dst.add(o), b'0' + (slot % 10) as u8);
    o += 1;
    for b in b".ttf" {
        core::ptr::write_volatile(dst.add(o), *b);
        o += 1;
    }
    core::ptr::write_volatile(dst.add(o), 0);
}

unsafe fn font_slot_index(slot: u32) -> u32 {
    let mut i = 0u32;
    while i < FONT_N {
        if FONT_IDS[i as usize] == slot {
            return i;
        }
        i += 1;
    }
    u32::MAX
}

/// LVGL 的 `lv_freetype_font_create(path, render_mode, size, style)`。
/// 参数次序不能按名字猜：第一参是文件路径，第二参才是渲染模式。
unsafe fn font_face_from(path: *const u8) -> u32 {
    let mf: unsafe extern "C" fn(*const u8, u32, u32, u32) -> u32 =
        core::mem::transmute(0x0C73_44FDusize);
    mf(path, 0, 22, 0)
}

/// 写清单：整块 128 字节，空槽 16 个空格。失败就保持旧文件，不出现半截状态。
unsafe fn font_index_write() -> bool {
    let mut b = [0x20u8; 128];
    let mut row = 0usize;
    let mut i = 0usize;
    while i < FONT_N as usize {
        let off = row * 16;
        let slot = FONT_IDS[i];
        b[off] = b'0' + ((slot / 10) % 10) as u8;
        b[off + 1] = b'0' + (slot % 10) as u8;
        b[off + 2] = b' ';
        let src = (core::ptr::addr_of!(FONT_NAMES) as *const u8).add(i * 24);
        let mut k = 0usize;
        while k < 12 && core::ptr::read_volatile(src.add(k)) != 0 {
            b[off + 3 + k] = core::ptr::read_volatile(src.add(k));
            k += 1;
        }
        b[off + 15] = b'\n';
        row += 1;
        i += 1;
    }
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 =
        core::mem::transmute(FW_OPEN | 1);
    let wr: unsafe extern "C" fn(i32, *const u8, u32) -> i32 =
        core::mem::transmute(FW_WRITE | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(FONT_INDEX.as_ptr(), 6, 0o640);
    if fd < 0 {
        return false;
    }
    let n = wr(fd, b.as_ptr(), 128);
    let _ = cl(fd);
    n == 128
}

unsafe fn font_name_ptr(i: u32) -> *const u8 {
    (core::ptr::addr_of!(FONT_NAMES) as *const u8).add((i as usize) * 24)
}

unsafe fn beauty_name_copy(dst: *mut u8, src: *const u8, cap: usize) {
    let mut n = 0usize;
    while n + 1 < cap {
        let c = core::ptr::read_volatile(src.add(n));
        if c == 0 {
            break;
        }
        core::ptr::write_volatile(dst.add(n), c);
        n += 1;
    }
    core::ptr::write_volatile(dst.add(n), 0);
}

unsafe fn beauty_save() {
    let mut b = [0u8; 48];
    b[0] = b'C';
    b[1] = b'B';
    b[2] = b'W';
    b[3] = b'1';
    b[4] = BEAUTY_RESTORE_ON as u8;
    b[5] = BEAUTY_BLUR_RESTORE_ON as u8;
    let mut n = 0usize;
    while n < 23 && BEAUTY_FONT_NAME[n] != 0 {
        b[8 + n] = BEAUTY_FONT_NAME[n];
        n += 1;
    }
    b[6] = n as u8;
    b[32] = ICON_PACK_CUR as u8;
    b[33] = BLUR_ZONE_MASK as u8;
    b[34] = BLUR_MODE as u8;
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 =
        core::mem::transmute(FW_OPEN | 1);
    let wr: unsafe extern "C" fn(i32, *const u8, u32) -> i32 =
        core::mem::transmute(FW_WRITE | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(BEAUTY_CFG.as_ptr(), 6, 0o640);
    if fd < 0 {
        return;
    }
    let r = wr(fd, b.as_ptr(), 48);
    let _ = cl(fd);
    DIAG[55] = r as u32;
}

/// 读出上次的选择并恢复字体。10P 的开关关掉时只读记录、不应用。
unsafe fn beauty_load() {
    if BEAUTY_LOADED != 0 {
        return;
    }
    BEAUTY_LOADED = 1;
    let mfsys: unsafe extern "C" fn(*const u8, u32, u32) -> u32 = core::mem::transmute(MFONT);
    FONT_SYS_FACE = mfsys(FONT_NAME.as_ptr(), 22, 0);
    font_scan();
    icon_packs_scan();
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 =
        core::mem::transmute(FW_OPEN | 1);
    let rd: unsafe extern "C" fn(i32, *mut u8, u32) -> i32 =
        core::mem::transmute(FW_READ | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(BEAUTY_CFG.as_ptr(), 1, 0);
    if fd >= 0 {
        let mut b = [0u8; 48];
        let n = rd(fd, b.as_mut_ptr(), 48);
        let _ = cl(fd);
        if n >= 8 && &b[..4] == b"CBW1" {
            BEAUTY_RESTORE_ON = if b[4] != 0 { 1 } else { 0 };
            BEAUTY_BLUR_RESTORE_ON = if b[5] != 0 { 1 } else { 0 };
            let ln = (b[6] as usize).min(23);
            let mut k = 0usize;
            while k < ln {
                BEAUTY_FONT_NAME[k] = b[8 + k];
                k += 1;
            }
            BEAUTY_FONT_NAME[ln] = 0;
            if n >= 48 {
                ICON_PACK_CUR = b[32] as u32;
                BLUR_ZONE_MASK = (b[33] as u32) & 0x0F;
                BLUR_MODE = b[34] as u32;
            }
        }
    }
    let mut i = 0u32;
    while i < FONT_N {
        let a = font_name_ptr(i);
        let b = core::ptr::addr_of!(BEAUTY_FONT_NAME) as *const u8;
        let mut k = 0usize;
        loop {
            let ca = core::ptr::read_volatile(a.add(k));
            let cb = core::ptr::read_volatile(b.add(k));
            if ca != cb {
                break;
            }
            if ca == 0 {
                let slot = FONT_IDS[i as usize];
                FONT_CUR = slot;
                if BEAUTY_RESTORE_ON != 0 {
                    let mf: unsafe extern "C" fn(*const u8, u32, u32) -> u32 =
                        core::mem::transmute(MFONT);
                    let face = mf(a, 22, 0);
                    if face >= 0x3C00_0000 {
                        FACE = face;
                        FONT_FACE = face;
                    } else {
                        FONT_CUR = u32::MAX;
                    }
                }
                return;
            }
            k += 1;
            if k >= 23 {
                break;
            }
        }
        i += 1;
    }
    FONT_CUR = u32::MAX;
    if ICON_PACK_CUR != 0 && BEAUTY_RESTORE_ON != 0 {
        icon_pack_start(ICON_PACK_CUR);
    }
}

/// 读图标包清单。与字体页一样，清单是唯一来源，不遍历目录。
unsafe fn icon_packs_scan() {
    ICON_PACK_N = 0;
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 =
        core::mem::transmute(FW_OPEN | 1);
    let rd: unsafe extern "C" fn(i32, *mut u8, u32) -> i32 =
        core::mem::transmute(FW_READ | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(ICON_INDEX.as_ptr(), 1, 0);
    if fd < 0 {
        return;
    }
    let mut b = [0u8; 128];
    let mut got = 0usize;
    while got < 128 {
        let n = rd(fd, b.as_mut_ptr().add(got), (128 - got) as u32);
        if n <= 0 {
            break;
        }
        got += n as usize;
    }
    let _ = cl(fd);
    let mut i = 0usize;
    while i < 8 && (i + 1) * 16 <= got {
        let off = i * 16;
        i += 1;
        let d0 = b[off];
        let d1 = b[off + 1];
        if !d0.is_ascii_digit() || !d1.is_ascii_digit() {
            continue;
        }
        let id = ((d0 - b'0') as u32) * 10 + (d1 - b'0') as u32;
        if id < 1 || id > 8 {
            continue;
        }
        let row = ICON_PACK_N as usize;
        if row >= ICON_PACK_ID.len() {
            break;
        }
        let mut ln = 0usize;
        while ln < 12 && off + 3 + ln < got {
            let c = b[off + 3 + ln];
            if c < 0x20 || c >= 0x7f || c == b' ' {
                break;
            }
            ICON_PACK_NAME[row][ln] = c;
            ln += 1;
        }
        if ln == 0 {
            continue;
        }
        ICON_PACK_ID[row] = id;
        ICON_PACK_NAME[row][ln] = 0;
        ICON_PACK_N += 1;
    }
}

unsafe fn icon_path_ok(p: u32) -> bool {
    p >= 0x0C00_0000 && p < 0x4000_0000
}

unsafe fn icon_path_has_prefix(p: u32, pre: &[u8]) -> bool {
    if !icon_path_ok(p) {
        return false;
    }
    let mut i = 0usize;
    while i < pre.len() {
        if core::ptr::read_volatile((p as usize + i) as *const u8) != pre[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 从图标的原有路径取 stem：优先最后一段；最后一段叫 launcher[.bin] 时退到父目录名。
unsafe fn icon_stem(path: u32, dst: *mut u8) -> bool {
    if !icon_path_ok(path) {
        return false;
    }
    let mut b = [0u8; 96];
    let mut n = 0usize;
    while n + 1 < b.len() {
        let c = core::ptr::read_volatile((path as usize + n) as *const u8);
        if c == 0 {
            break;
        }
        if c < 0x20 || c >= 0x7f {
            return false;
        }
        b[n] = c;
        n += 1;
    }
    if n == 0 {
        return false;
    }
    let mut last = (0usize, 0usize);
    let mut prev = (0usize, 0usize);
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= n {
        if i == n || b[i] == b'/' {
            if i > start {
                prev = last;
                last = (start, i - start);
            }
            start = i + 1;
        }
        i += 1;
    }
    let (mut off, mut len) = last;
    if len == 12 && &b[off..off + 12] == b"launcher.bin" {
        off = prev.0;
        len = prev.1;
    } else if len == 8 && &b[off..off + 8] == b"launcher" {
        off = prev.0;
        len = prev.1;
    }
    if len > 4 && &b[off + len - 4..off + len] == b".bin" {
        len -= 4;
    }
    if len == 0 || len > 48 {
        return false;
    }
    let mut k = 0usize;
    while k < len {
        core::ptr::write_volatile(dst.add(k), b[off + k]);
        k += 1;
    }
    core::ptr::write_volatile(dst.add(len), 0);
    true
}

unsafe fn icon_pack_path(dst: *mut u8, pack: u32, stem: *const u8) {
    let pre = b"/data/chaos/icons/";
    let mut o = 0usize;
    while o < pre.len() {
        core::ptr::write_volatile(dst.add(o), pre[o]);
        o += 1;
    }
    core::ptr::write_volatile(dst.add(o), b'0' + ((pack / 10) % 10) as u8);
    core::ptr::write_volatile(dst.add(o + 1), b'0' + (pack % 10) as u8);
    core::ptr::write_volatile(dst.add(o + 2), b'/');
    o += 3;
    let mut k = 0usize;
    loop {
        let c = core::ptr::read_volatile(stem.add(k));
        if c == 0 {
            break;
        }
        core::ptr::write_volatile(dst.add(o), c);
        o += 1;
        k += 1;
    }
    for c in b".bin" {
        core::ptr::write_volatile(dst.add(o), *c);
        o += 1;
    }
    core::ptr::write_volatile(dst.add(o), 0);
}

unsafe fn icon_file_ok(path: *const u8) -> bool {
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 =
        core::mem::transmute(FW_OPEN | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(path, 1, 0);
    if fd < 0 {
        false
    } else {
        let _ = cl(fd);
        true
    }
}

unsafe fn icon_slot_for(id: u32, current: u32) -> u32 {
    let mut i = 0usize;
    while i < B_ICON_N as usize {
        if B_ICON_ID[i] == id {
            return i as u32;
        }
        i += 1;
    }
    if i >= B_ICON_MAX {
        return B_ICON_NONE;
    }
    B_ICON_ID[i] = id;
    if icon_path_ok(current) && !icon_path_has_prefix(current, b"/data/chaos/icons/") {
        B_ICON_ORIG[i] = current;
    }
    B_ICON_N += 1;
    i as u32
}

/// 选包后按桌面记录逐拍换一个应用，避免一次重启几十次桌面。
unsafe fn icon_pack_apply_one(pack: u32) -> bool {
    if B_ICON_CURSOR >= B_ICON_PLAN_N {
        return true;
    }
    let id = B_ICON_PLAN[B_ICON_CURSOR as usize];
    B_ICON_CURSOR += 1;
    let look: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(APP_LOOKUP);
    let desc = look(id);
    if desc == 0 {
        return B_ICON_CURSOR >= B_ICON_PLAN_N;
    }
    let cur = rd32((desc + 0x0C) as usize);
    let slot = icon_slot_for(id, cur);
    if slot == B_ICON_NONE {
        return B_ICON_CURSOR >= B_ICON_PLAN_N;
    }
    let orig = B_ICON_ORIG[slot as usize];
    let mut target = orig;
    if pack != 0 && orig != 0 {
        let mut stem = [0u8; 64];
        if icon_stem(orig, stem.as_mut_ptr()) {
            let p = (core::ptr::addr_of_mut!(B_ICON_PATH) as *mut u8)
                .add((slot as usize) * B_ICON_PATH_CAP);
            icon_pack_path(p, pack, stem.as_ptr());
            if icon_file_ok(p) {
                target = p as u32;
            }
        }
    }
    if target != 0 {
        core::ptr::write_volatile((desc + 0x0C) as *mut u32, target);
        let rf: unsafe extern "C" fn(u32) = core::mem::transmute(LAUNCHER_REFRESH);
        rf(id);
    }
    B_ICON_CURSOR >= B_ICON_PLAN_N
}

unsafe fn icon_pack_start(pack: u32) {
    B_ICON_PLAN_N = 0;
    B_ICON_CURSOR = 0;
    let mut p = rd32(DESK_LIST + 4);
    let mut n = 0u32;
    while is_obj(p) && n < 200 && B_ICON_PLAN_N < B_ICON_MAX as u32 {
        let id = rd32(p as usize) & 0xFFFF;
        B_ICON_PLAN[B_ICON_PLAN_N as usize] = id;
        B_ICON_PLAN_N += 1;
        p = rd32(p as usize + DESK_NEXT);
        n += 1;
    }
    B_ICON_REQ = pack;
}

unsafe fn font_set_face(face: u32) {
    FACE = face;
    FONT_FACE = face;
    // 只写本页标签：跨页批量写在实测里会把输入弄失灵
    let mut k = 0u32;
    while k < LBL_N {
        let l = LBL_TAB[k as usize];
        if l >= 0x3C00_0000 {
            prop(l, P_TEXT_FONT, face);
        }
        k += 1;
    }
}

/// 换字体：登记 /data/chaos/font/st<槽>.ttf 为独占名字再建脸。
/// 退回系统字体走同一入口，只是路径换成 /resource/font/MiSans-Regular.ttf。
unsafe fn font_apply(i: u32) {
    if i >= FONT_N {
        return;
    }
    let slot = FONT_IDS[i as usize];
    let mf: unsafe extern "C" fn(*const u8, u32, u32) -> u32 = core::mem::transmute(MFONT);
    let face = mf(font_name_ptr(i), 22, 0);
    DIAG[56] = face;
    DIAG[57] = FONT_CUR;
    DIAG[58] = FONT_N;
    DIAG[59] = slot;
    if face < 0x3C00_0000 {
        return;
    }
    FONT_CUR = slot;
    FONT_DELCONF = 0;
    FONT_DIRTY = 1;
    beauty_name_copy(core::ptr::addr_of_mut!(BEAUTY_FONT_NAME) as *mut u8,
                     font_name_ptr(i), 24);
    BEAUTY_SAVE_REQ = 1;
    font_set_face(face);
}

unsafe fn font_restore_system() {
    let face = FONT_SYS_FACE;
    if face >= 0x3C00_0000 {
        font_set_face(face);
    }
    FONT_CUR = u32::MAX;
    core::ptr::write_volatile(core::ptr::addr_of_mut!(BEAUTY_FONT_NAME) as *mut u8, 0);
}

/// 删除一个字体槽：若正在用则先退回系统字体，再改清单、删文件。
unsafe fn font_delete_requested(slot: u32) {
    let idx = font_slot_index(slot);
    if idx == u32::MAX {
        return;
    }
    if FONT_CUR == slot {
        font_restore_system();
    }
    let i = idx as usize;
    let mut k = i;
    while k + 1 < FONT_N as usize {
        FONT_IDS[k] = FONT_IDS[k + 1];
        let src = (core::ptr::addr_of!(FONT_NAMES) as *const u8).add((k + 1) * 24);
        let dst = (core::ptr::addr_of_mut!(FONT_NAMES) as *mut u8).add(k * 24);
        let mut b = 0usize;
        while b < 24 {
            core::ptr::write_volatile(dst.add(b), core::ptr::read_volatile(src.add(b)));
            b += 1;
        }
        k += 1;
    }
    FONT_N -= 1;
    let _ = font_index_write();
    let un: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_UNLINK | 1);
    let mut p = [0u8; 32];
    font_path(p.as_mut_ptr(), slot);
    let _ = un(p.as_ptr());
    FONT_DELCONF = 0;
    FONT_DIRTY = 1;
    BEAUTY_SAVE_REQ = 1;
}



/// 时间页两个轮之间的冒号。
static K_COLON: &[u8] = &[b':', 0];
/// 日期页标题。
static T_DATE: &[u8] = &[0xE6, 0x97, 0xA5, 0xE6, 0x9C, 0x9F, 0xE8, 0xAE, 0xBE, 0xE7, 0xBD, 0xAE, 0];

static mut DATE_M: u32 = 0;
static mut DATE_D: u32 = 0;
static mut DATE_MSG: [u8; 32] = [0; 32];

/// 把 "已保存 Y-M-D" 写进日期页提示缓冲。
unsafe fn date_msg_saved(y: i32, m: i32, dd: i32) {
    let p = core::ptr::addr_of_mut!(DATE_MSG) as *mut u8;
    let head: &[u8] = &[0xE5, 0xB7, 0xB2, 0xE4, 0xBF, 0x9D, 0xE5, 0xAD, 0x98, 0x20];
    let mut i = 0usize;
    while i < head.len() {
        core::ptr::write_volatile(p.add(i), head[i]);
        i += 1;
    }
    core::ptr::write_volatile(p.add(i), b'0' + (m / 10) as u8);
    core::ptr::write_volatile(p.add(i + 1), b'0' + (m % 10) as u8);
    i += 2;
    core::ptr::write_volatile(p.add(i), b'-');
    i += 1;
    core::ptr::write_volatile(p.add(i), b'0' + (dd / 10) as u8);
    core::ptr::write_volatile(p.add(i + 1), b'0' + (dd % 10) as u8);
    i += 2;
    core::ptr::write_volatile(p.add(i), 0);
}


/// 把 "已保存 HH:MM" 写进提示缓冲。
unsafe fn time_msg_saved(h: i32, m: i32) {
    let p = core::ptr::addr_of_mut!(TIME_MSG) as *mut u8;
    let head: &[u8] = &[0xE5, 0xB7, 0xB2, 0xE4, 0xBF, 0x9D, 0xE5, 0xAD, 0x98, 0x20]; // "已保存 "
    let mut i = 0usize;
    while i < head.len() {
        core::ptr::write_volatile(p.add(i), head[i]);
        i += 1;
    }
    core::ptr::write_volatile(p.add(i), b'0' + (h / 10) as u8);
    core::ptr::write_volatile(p.add(i + 1), b'0' + (h % 10) as u8);
    core::ptr::write_volatile(p.add(i + 2), b':');
    core::ptr::write_volatile(p.add(i + 3), b'0' + (m / 10) as u8);
    core::ptr::write_volatile(p.add(i + 4), b'0' + (m % 10) as u8);
    core::ptr::write_volatile(p.add(i + 5), 0);
}

// 每页一份页名：page_goto 按 (app_id<<16|page_index) 解析，页名只是注册键。
static P_MENU: &[u8] = b"chaosmain\0";
static P_ABOUT: &[u8] = b"chaosabout\0";
static P_APPS: &[u8] = b"chaosapps\0";
static P_CACHE: &[u8] = b"chaoscache\0";
static P_BRIGHT: &[u8] = b"chaosbright\0";
static P_FILES: &[u8] = b"chaosfiles\0";
static P_DESK: &[u8] = b"chaosdesk\0";
static P_VIB: &[u8] = b"chaosvib\0";
static P_STORAGE: &[u8] = b"chaosstore\0";
static P_WATCH: &[u8] = b"chaoswatch\0";
static P_SETTINGS: &[u8] = b"chaossettings\0";
static P_BEAUTIFY: &[u8] = b"chaosbeautify\0";
static P_WFMGMT: &[u8] = b"chaoswfmgmt\0";
static P_LOG: &[u8] = b"chaoslog\0";
static P_WMGR: &[u8] = b"chaoswmgr\0";

// 「缓存清理」页的文案（UTF-8，末尾 0）
static T_CACHE: &[u8] = &[0xE7, 0xBC, 0x93, 0xE5, 0xAD, 0x98, 0xE6, 0xB8, 0x85, 0xE7, 0x90, 0x86, 0x00];
/// 清理按钮（10P 缓存页那一行）。
static K_CACHE_START: &[u8] = &[0xE5, 0xBC, 0x80, 0xE5, 0xA7, 0x8B, 0xE6, 0xB8, 0x85, 0xE7, 0x90, 0x86, 0x00];
/// 二次确认态提示（点第一下之后按钮那条）。
static V_ARM: &[u8] = &[
    0xE5, 0x86, 0x8D, 0xE6, 0xAC, 0xA1, 0xE7, 0x82, 0xB9, 0xE5, 0x87, 0xBB, 0xE7, 0xA1, 0xAE,
    0xE8, 0xAE, 0xA4, 0xE6, 0xB8, 0x85, 0xE7, 0x90, 0x86, 0x00,
];

// ---- 亮度页 ----
// p65 的亮度可设范围是 0-255：设 300 会被固件钳到 255（实测读回 255），
// 系统设置页里传的 600 是它自己的标度，不是这里能设的值。
const BRIGHT_MAX: u32 = 255;
static T_BRIGHT: &[u8] = &[
    0xE4, 0xBA, 0xAE, 0xE5, 0xBA, 0xA6, 0xE6, 0x8E, 0xA7, 0xE5, 0x88, 0xB6, 0x00,
];
static K_BRIGHT_NOW: &[u8] = &[
    0xE5, 0xBD, 0x93, 0xE5, 0x89, 0x8D, 0xE4, 0xBA, 0xAE, 0xE5, 0xBA, 0xA6, 0x00,
];
/// 四档：25% / 50% / 75% / 100%
static K_BRIGHT_LV: [&[u8]; 4] = [
    &[0x32, 0x35, 0x25, 0x00],
    &[0x35, 0x30, 0x25, 0x00],
    &[0x37, 0x35, 0x25, 0x00],
    &[0x31, 0x30, 0x30, 0x25, 0x00],
];
/// 自动亮度行标题。
static K_BRIGHT_AUTO: &[u8] = &[
    0xE8, 0x87, 0xAA, 0xE5, 0x8A, 0xA8, 0xE4, 0xBA, 0xAE, 0xE5, 0xBA, 0xA6, 0x00,
];
/// 当前亮度文案（"300/600"）。
static mut V_BRIGHT: [u8; 16] = [0; 16];
/// 自动亮度行文案（开/关）。
static mut V_AUTO: [u8; 8] = [0; 8];
/// 待设亮度（0 = 不动）。
static mut BRIGHT_REQ: u32 = 0;
/// 待切换自动亮度（0 = 不动；1 = 开，2 = 关）。
static mut BRIGHT_AUTO_REQ: u32 = 0;
/// 本拍动过亮度（要重排那一页刷新读数）。
static mut BRIGHT_DIRTY: u32 = 0;

// ---- 文件查看器 ----
// 只用已验证的三支：opendir / readdir / closedir，加 fstat 取文件大小。
static T_FILES: &[u8] = &[
    0xE6, 0x9F, 0xA5, 0xE7, 0x9C, 0x8B, 0xE5, 0x99, 0xA8, 0x00,
];
/// ".."（上一级）
static K_FILES_UP: &[u8] = b"..\0";
static K_FILES_DIR: &[u8] = &[0xE7, 0x9B, 0xAE, 0xE5, 0xBD, 0x95, 0x00];
static K_FILES_EMPTY: &[u8] = &[
    0xE7, 0xA9, 0xBA, 0xE7, 0x9B, 0xAE, 0xE5, 0xBD, 0x95, 0x00,
];
const FILES_MAX: usize = 24;
static mut F_PATH: [u8; 160] = [0; 160];
static mut F_PATH_LEN: usize = 0;
static mut F_NAME: [[u8; 48]; FILES_MAX] = [[0; 48]; FILES_MAX];
static mut F_ISDIR: [u8; FILES_MAX] = [0; FILES_MAX];
static mut F_SIZE: [u32; FILES_MAX] = [0; FILES_MAX];
static mut F_MSG: [[u8; 20]; FILES_MAX] = [[0; 20]; FILES_MAX];
static mut F_N: u32 = 0;
/// 一页 6 个条目（回调只有 0..7 八个，row0=上一级、row7=下一页）。
static mut F_PAGE: usize = 0;
static mut F_SHOWN: u32 = 0;
/// "下一页"
static K_FILES_NEXT: &[u8] = &[
    0xE4, 0xB8, 0x8B, 0xE4, 0xB8, 0x80, 0xE9, 0xA1, 0xB5, 0x00,
];
/// 进入哪个目录（u32::MAX = 不动；0 = 上一级；其余 = 第 n-1 个条目）。
static mut F_REQ: u32 = u32::MAX;

// ---- 桌面图标页 ----
// 桌面记录链 0x3C3BF9DC：+0x00 计数、+0x04 首、+0x08 尾；
// 节点：+0x00 u16 app_id、+0x04 显示名串、+0x08 包名串、+0x1C 下一条。
static T_DESK: &[u8] = &[
    0xE6, 0xA1, 0x8C, 0xE9, 0x9D, 0xA2, 0xE5, 0x9B, 0xBE, 0xE6, 0xA0, 0x87, 0x00,
];
const DESK_SHOW: usize = 7;
static mut D_IDS: [u32; DESK_SHOW] = [0; DESK_SHOW];
static mut D_N: u32 = 0;

// ---- 缓存清理 ----
// 只清这几个目录**里面**的内容，根目录保留；红线同 10P：不碰 /data/fitness/cache
// （那里是等待同步到手机的健康数据）。
static CC_ROOTS: [&[u8]; 4] = [
    b"/data/cache\0",
    b"/data/quickapp/cache\0",
    b"/data/log\0",
    b"/data/offlinelog\0",
];
const CC_ROOT_N: usize = 4;
const CC_DEPTH_MAX: u32 = 8;
static mut CC_WORK: [u8; 176] = [0; 176];
static mut CC_FILES: u32 = 0;
static mut CC_BYTES: u32 = 0;
static mut CC_FAIL: u32 = 0;
static mut CC_ROOT_FILES: [u32; CC_ROOT_N] = [0; CC_ROOT_N];
static mut CC_ROOT_BYTES: [u32; CC_ROOT_N] = [0; CC_ROOT_N];
static mut CC_ROOT_MSG: [[u8; 48]; CC_ROOT_N] = [[0; 48]; CC_ROOT_N];
/// 根目录的显示名（去掉 /data/ 前缀，页面上只显示后半截）。
static mut CC_MSG: [u8; 96] = [0; 96];
/// 1 = 已武装（点第二下才真清）。
static mut CC_ARM: u32 = 0;
/// 1 = 有清理请求，由拍执行。
static mut CC_REQ: u32 = 0;
/// 本次清理只清哪个根（0xFF = 全清）。
static mut CC_REQ_ROOT: usize = 0xFF;
/// 1 = 本拍要整页重建（文本变化后重排一次）。
static mut CC_REDRAW: u32 = 0;
/// 重建时置 1：跳过重扫，保留刚出的结果文案与已武装态。
static mut CC_KEEP: u32 = 0;

static mut APP_DESC: [u32; 16] = [0; 16];
/// 每页 29 word（116B）描述符，与 10P 的注册布局逐字同形。
static mut PAGE_DESC: [[u32; 29]; PAGE_N] = [[0; 29]; PAGE_N];
/// 每页自己的根对象与内容容器（压栈后上一页还活着，不能共用一个全局）。
static mut ROOTS: [u32; PAGE_N] = [0; PAGE_N];
static mut CONTENTS: [u32; PAGE_N] = [0; PAGE_N];
static mut ROOT: u32 = 0;
static mut CONTENT: u32 = 0;
static mut CUR: u32 = PAGE_MENU;
/// 还要建几行：一行一拍建。
static mut ROW_TODO: u32 = 0;
/// 下一行是第几行。
static mut ROW_IDX: u32 = 0;
/// 每页建出来的行卡片句柄，按创建次序（= 行号）。宿主机导航回归按句柄读屏幕坐标，
/// 不猜滚动量。压栈后上一页仍在，必须按页号分开存。只在 navdbg 构建里编进去。
#[cfg(feature = "navdbg")]
static mut ROW_HS: [[u32; 24]; PAGE_N] = [[0; 24]; PAGE_N];
#[cfg(feature = "navdbg")]
static mut ROW_CNS: [u32; PAGE_N] = [0; PAGE_N];
#[cfg(feature = "navdbg")]
static mut BUILD_PID: usize = 0;
static mut WANT: u32 = u32::MAX;
static mut FACE: u32 = 0;
static mut TICKS: u32 = 0;
static mut ADDED: bool = false;
static mut APPS_OFF: u32 = 0;
static mut V_COUNT: [u8; 24] = [0; 24];
static mut APP_BUF: [[u8; 40]; 48] = [[0; 40]; 48];
static mut APP_VAL: [[u8; 12]; 48] = [[0; 12]; 48];
/// 应用页副标题：桌面记录里的包名（空则退回显示 id）。
static mut APP_PKG: [[u8; 40]; 48] = [[0; 40]; 48];
/// 应用页每一行对应的应用 id：点行就由固件打开这个应用。
static mut APP_IDS: [u32; 48] = [0; 48];
static mut APP_N: u32 = 0;
/// 行里那个副标题标签的句柄：数据行要按拍更新文本。
static mut VAL_LABELS: [u32; 16] = [0; 16];
/// row_w 最近一次建出来的主标签（数据行要把两条文字上移给进度条让位）
static mut LAST_KEY: u32 = 0;
#[cfg(feature = "procfs")]
static mut CPU_PREV: [u8; 16] = [0; 16];
#[cfg(feature = "procfs")]
static mut CPU_PLEN: usize = 0;
#[cfg(feature = "procfs")]
static mut MEM_PREV: [u8; 32] = [0; 32];
#[cfg(feature = "procfs")]
static mut MEM_PLEN: usize = 0;
#[cfg(feature = "procfs")]
static mut ST_PREV: [u8; 32] = [0; 32];
#[cfg(feature = "procfs")]
static mut ST_PLEN: usize = 0;
/// 诊断通道：从 lsmod 拿到本模块 DATA 基址后可直接读。
/// 诊断通道：从 lsmod 拿本模块 DATA 基址后按魔数定位。
/// 必须导出（`no_mangle`），否则链接器的 gc-sections 会把只写不读的 static 丢掉。
#[no_mangle]
pub static mut DIAG: [u32; 64] = [0xC0DE_0034; 64];

/// 导航回归快照（魔数 0xC0DE00E7）：
/// [魔数, CUR, ROOT, 页根存活, 行数, 24*(x1,y1,x2,y2)]。
/// 宿主机在模块 DATA 段里按魔数定位后读；不用固定 scratch 页——那页会被别的东西覆盖。
#[cfg(feature = "navdbg")]
const NAV_EMPTY: [u32; 5 + 24 * 4] = {
    let mut a = [0u32; 5 + 24 * 4];
    a[0] = 0xC0DE_00E7;
    a
};
#[cfg(feature = "navdbg")]
#[no_mangle]
pub static mut NAV_SNAP: [u32; 5 + 24 * 4] = NAV_EMPTY;

/// 一次性调试格：记录"走到挂图标哪一步"，guest 卡住时仍能从 QMP 读回来。
const MARK: usize = 0x3C7F_F000;
/// 探针：page_back(0) 的调用计数（宿主看行为，不看返回值）。
static mut BACK_CALLS: u32 = 0;







unsafe fn mark(v: u32) {
    core::ptr::write_volatile(MARK as *mut u32, v);
}

/// 读当前有效亮度。
unsafe fn bright_get() -> u32 {
    let f: unsafe extern "C" fn() -> u32 = core::mem::transmute(FW_BRIGHT_GET | 1);
    f()
}

/// 写手动亮度（0-600）。
unsafe fn bright_set(v: u32) {
    let f: unsafe extern "C" fn(u32) = core::mem::transmute(FW_BRIGHT_SET | 1);
    f(v);
}

/// 亮度状态块（+0x04 自动开关、+0x08 当前有效值、+0x0C 手动值）。
unsafe fn bright_state() -> *const u8 {
    core::ptr::read_volatile(BRIGHT_LITERAL as *const u32) as *const u8
}

/// 自动亮度开关（1=开）。
unsafe fn bright_auto_get() -> u32 {
    core::ptr::read_volatile(bright_state().add(4) as *const u8) as u32
}

/// 设自动亮度开关。固件自己会按开关决定要不要立刻重算。
unsafe fn bright_auto_set(on: u32) {
    let f: unsafe extern "C" fn(u32) = core::mem::transmute(0x0CA6_5CE0 | 1);
    f(on);
}

/// 把当前亮度写成 "128/255"。
unsafe fn bright_fill() {
    let cur = bright_get();
    let mut w = CW {
        p: core::ptr::addr_of_mut!(V_BRIGHT) as *mut u8,
        n: 0,
        cap: 16,
    };
    w.num(cur);
    w.s(b"/");
    w.num(BRIGHT_MAX);
    w.end();
    // 自动亮度那一行的取值
    let mut a = CW {
        p: core::ptr::addr_of_mut!(V_AUTO) as *mut u8,
        n: 0,
        cap: 8,
    };
    if bright_auto_get() != 0 {
        a.s(&[0xE5, 0xBC, 0x80, 0x00]); // 开
    } else {
        a.s(&[0xE5, 0x85, 0xB3, 0x00]); // 关
    }
    a.end();
}

/// 换表盘：`watchface_config_set_use_watchface(const char *id)`，file 0x9B8988
/// ⇒ 运行地址 0x0CA78988（file + 0x0C0C0000；这两个数字手算错过一次，别凭记忆写）。
const FW_WF_USE: usize = 0x0CA7_8988;

/// 震动三支（运行地址 = file + 0x0C0C0000；这个加法手算错过一次，别再凭记忆写）：
/// run(file 0x9B37A8) / set_mode(file 0x9B3910)；模式值在重定位字面量 0x0C92B6D4 指到的字节里。
const FW_VIB_RUN: usize = 0x0CA7_37A8;
const FW_VIB_SET_MODE: usize = 0x0CA7_3910;
const VIB_MODE_LITERAL: usize = 0x0C92_B6D4;

/// 读当前震动模式（1..25）。
unsafe fn vib_mode() -> u32 {
    let p = core::ptr::read_volatile(VIB_MODE_LITERAL as *const u32) as usize;
    if p == 0 {
        return 0;
    }
    core::ptr::read_volatile(p as *const u8) as u32
}

/// 按当前模式震一次。返回固件的结果码（0 = 交给驱动成功）。
unsafe fn vib_run() -> i32 {
    let mut pat = [0u8; 12];
    pat[0] = vib_mode() as u8;
    let f: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_VIB_RUN | 1);
    f(pat.as_ptr())
}

/// 设震动模式（写 persist.vibration.mode 并更新全局）。
unsafe fn vib_set_mode(m: u32) {
    let f: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_VIB_SET_MODE | 1);
    f(m);
}

#[cfg(feature = "wfprobe")]
unsafe fn probe_watchface() {
    // 表盘删除：watchface_manager_delete_watchface(id)，file 0x9D0B1C ⇒ 0x0CA90B1C
    static ID: &[u8] = b"120917364924\0";
    let f: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(0x0CA9_0B1C | 1);
    DIAG[60] = f(ID.as_ptr()) as u32;
    DIAG[21] = 0x0BEE_000E;
}

// ---- 震动页 ----
// 两个不同的全局：驱动用的"震动类型"（决定波形，0x0C92B6D4 指到）与
// 用户设置里的"震动强度"（0x0CA73950 指到，1=标准 2=增强）。
static T_VIB: &[u8] = &[0xE9, 0x9C, 0x87, 0xE5, 0x8A, 0xA8, 0x00];
static K_VIB_ONE: &[u8] = &[
    0xE9, 0x9C, 0x87, 0xE5, 0x8A, 0xA8, 0xE4, 0xB8, 0x80, 0xE6, 0xAC, 0xA1, 0x00,
];
static K_VIB_LEVEL: &[u8] = &[
    0xE5, 0xBC, 0xBA, 0xE5, 0xBA, 0xA6, 0x00, // 强度
];
static K_VIB_STD: &[u8] = &[
    0xE6, 0xA0, 0x87, 0xE5, 0x87, 0x86, 0x00, // 标准
];
static K_VIB_ENH: &[u8] = &[
    0xE5, 0xA2, 0x9E, 0xE5, 0xBC, 0xBA, 0x00, // 增强
];
const VIB_LEVEL_LITERAL: usize = 0x0CA7_3950;
static mut VIB_LEVEL_MSG: [u8; 12] = [0; 12];
/// 待震动（0 = 不动；1 = 震一次；2 = 改强度后再震）。
static mut VIB_REQ: u32 = 0;
static mut VIB_REQ_MODE: u32 = 0;
/// 10P 震动页第一行「滑动列表震动」：列表滚动事件里只登记，震动交给拍。
static mut VIB_SCROLL_ON: u32 = 0;
static mut VIB_SCROLL_REQ: u32 = 0;
static K_VIB_SCROLL: &[u8] = &[
    0xE6, 0xBB, 0x91, 0xE5, 0x8A, 0xA8, 0xE5, 0x88, 0x97, 0xE8, 0xA1, 0xA8, 0xE9, 0x9C, 0x87,
    0xE5, 0x8A, 0xA8, 0x00,
];
static K_VIB_TEST: &[u8] = &[
    0xE6, 0xB5, 0x8B, 0xE8, 0xAF, 0x95, 0xE9, 0x9C, 0x87, 0xE5, 0x8A, 0xA8, 0x00,
];
static mut VIB_DIRTY: u32 = 0;

/// 用户设置里的震动强度（1 = 标准，2 = 增强）。
unsafe fn vib_level() -> u32 {
    let p = core::ptr::read_volatile(VIB_LEVEL_LITERAL as *const u32) as usize;
    if p == 0 {
        return 0;
    }
    core::ptr::read_volatile(p as *const u8) as u32
}

/// 把强度写进 VIB_LEVEL_MSG（标准/增强/数字）。
unsafe fn vib_fill() {
    let mut w = CW {
        p: core::ptr::addr_of_mut!(VIB_LEVEL_MSG) as *mut u8,
        n: 0,
        cap: 12,
    };
    match vib_level() {
        1 => w.s(&[0xE6, 0xA0, 0x87, 0xE5, 0x87, 0x86]), // 标准
        2 => w.s(&[0xE5, 0xA2, 0x9E, 0xE5, 0xBC, 0xBA]), // 增强
        _ => w.num(vib_level()),
    }
    w.end();
}

// ---- 表盘列表页 ----
// 已装表盘都在 /data/app/watchface/builtin/<id>/；切换走
// watchface_config_set_use_watchface(id)（返回 0 = 受理），**重启后生效**。
static K_SETTINGS: &[u8] = &[0xE8, 0xAE, 0xBE, 0xE7, 0xBD, 0xAE, 0x00]; // 设置

/// 10P 页8 的标题与菜单名都是"表盘切换"
static T_WATCH: &[u8] = &[
    0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98, 0xE5, 0x88, 0x87, 0xE6, 0x8D, 0xA2, 0x00,
];
/// 10P 页17 运行日志：标题 + 环形缓冲（8 条 x 40 字节）
static T_LOG: &[u8] = &[
    0xE8, 0xBF, 0x90, 0xE8, 0xA1, 0x8C, 0xE6, 0x97, 0xA5, 0xE5, 0xBF, 0x97, 0x00,
];
static K_LOG: &[u8] = &[
    0xE8, 0xBF, 0x90, 0xE8, 0xA1, 0x8C, 0xE6, 0x97, 0xA5, 0xE5, 0xBF, 0x97, 0x00,
];
static K_REFRESH: &[u8] = &[0xE5, 0x88, 0xB7, 0xE6, 0x96, 0xB0, 0x00];
static K_LOG_OLDER: &[u8] = &[
    0xE6, 0x9B, 0xB4, 0xE6, 0x97, 0xA9, 0xE4, 0xB8, 0x80, 0xE9, 0xA1, 0xB5, 0x00,
];
static K_LOG_NEWER: &[u8] = &[
    0xE6, 0x9B, 0xB4, 0xE6, 0x96, 0xB0, 0xE4, 0xB8, 0x80, 0xE9, 0xA1, 0xB5, 0x00,
];
static K_TIME_SET: &[u8] = &[
    0xE6, 0x97, 0xB6, 0xE9, 0x97, 0xB4, 0xE8, 0xAE, 0xBE, 0xE7, 0xBD, 0xAE, 0x00,
];
static K_DATE_SET: &[u8] = &[
    0xE6, 0x97, 0xA5, 0xE6, 0x9C, 0x9F, 0xE8, 0xAE, 0xBE, 0xE7, 0xBD, 0xAE, 0x00,
];
static K_CLEAR_LOG: &[u8] = &[
    0xE6, 0xB8, 0x85, 0xE7, 0xA9, 0xBA, 0xE6, 0x97, 0xA5, 0xE5, 0xBF, 0x97, 0x00,
];
static K_CLEAR_ARMED: &[u8] = &[
    0xE5, 0x86, 0x8D, 0xE7, 0x82, 0xB9, 0xE4, 0xB8, 0x80, 0xE6, 0xAC, 0xA1, 0xE7, 0xA1, 0xAE,
    0xE8, 0xAE, 0xA4, 0xE6, 0xB8, 0x85, 0xE7, 0xA9, 0xBA, 0x00,
];
/// 主页存储/内存两行的占用百分比（0=内存 1=存储）与进度条句柄
static mut STAT_PCT: [u32; 2] = [0; 2];
static mut BAR_HANDLES: [u32; 2] = [0; 2];
const LOG_MAX: usize = 8;
static mut LOG_BUF: [[u8; 40]; LOG_MAX] = [[0; 40]; LOG_MAX];
static mut LOG_HEAD: u32 = 0;
static mut LOG_N: u32 = 0;
static mut LOG_CLEAR_ARM: u32 = 0;
/// 日志窗口偏移（0 = 最新一页，每页 4 条）
static mut LOG_OFF: u32 = 0;

/// 记一条日志（只存 ASCII 键值，避免日志页里塞中文长句）
unsafe fn log_add(tag: &[u8], val: u32) {
    let i = (LOG_HEAD as usize) % LOG_MAX;
    let mut w = CW {
        p: core::ptr::addr_of_mut!(LOG_BUF[i]) as *mut u8,
        n: 0,
        cap: 40,
    };
    w.s(tag);
    w.s(b" ");
    w.num(val);
    w.end();
    LOG_HEAD = LOG_HEAD.wrapping_add(1);
    if LOG_N < LOG_MAX as u32 {
        LOG_N += 1;
    }
}

/// 表盘管理 / 系统美化（10P 页10 / 页13 的标题）
static T_WFMGMT: &[u8] = &[
    0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98, 0xE7, 0xAE, 0xA1, 0xE7, 0x90, 0x86, 0x00,
];
static T_BEAUTIFY: &[u8] = &[
    0xE7, 0xB3, 0xBB, 0xE7, 0xBB, 0x9F, 0xE7, 0xBE, 0x8E, 0xE5, 0x8C, 0x96, 0x00,
];
static K_WATCH_SW: &[u8] = &[
    0xE5, 0x88, 0x87, 0xE6, 0x8D, 0xA2, 0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98, 0x00,
];
static WATCH_DIR: &[u8] = b"/data/app/watchface/builtin\0";
/// 表盘管理器：全局指针字 0x3C49762C；mgr+0x48=当前主表盘节点、+0x4C=息屏表盘、
/// +0x74=链表头、+0x70=哨兵。节点: +0x04 next、+0x08 id 串、+0x48 内联名字、
/// +0x88 类型(0 内置/1 市场/2 息屏/3 拒绝)、+0x90 in_use。
const WF_MGR_PTR: usize = 0x3C49_762C;
const WF_MGR_CUR: usize = 0x48;
const WF_LIST_HEAD: usize = 0x74;
const WF_LIST_SENT: usize = 0x70;
const WF_NEXT: usize = 0x04;
const WF_ID: usize = 0x08;
const WF_NAME: usize = 0x48;
const WF_TYPE: usize = 0x88;
const WATCH_MAX: usize = 16;
static mut W_ID: [[u8; 16]; WATCH_MAX] = [[0; 16]; WATCH_MAX];
static mut W_NAME: [[u8; 24]; WATCH_MAX] = [[0; 24]; WATCH_MAX];
static mut W_CUR: [u8; WATCH_MAX] = [0; WATCH_MAX];
static mut W_NODE: [u32; WATCH_MAX] = [0; WATCH_MAX];
static mut W_SHOW_BUILTIN: u32 = 1;
static mut W_WIN: u32 = 0;
static K_WATCH_SHOW: &[u8] = &[
    0xE6, 0x98, 0xBE, 0xE7, 0xA4, 0xBA, 0xE5, 0x86, 0x85, 0xE7, 0xBD, 0xAE, 0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98, 0x00,
];
static K_MORE: &[u8] = &[0xE6, 0x9B, 0xB4, 0xE5, 0xA4, 0x9A, 0x00];
static mut W_N: u32 = 0;
static mut W_MSG: [u8; 48] = [0; 48];
static mut W_REQ: u32 = u32::MAX;

unsafe fn watch_scan() {
    W_N = 0;
    let mgr = rd32(WF_MGR_PTR);
    if mgr & 0xE000_0000 != 0x2000_0000 {
        return;
    }
    let mgr = mgr as usize;
    let cur = rd32(mgr + WF_MGR_CUR) as usize;
    let sent = mgr + WF_LIST_SENT;
    let mut node = rd32(mgr + WF_LIST_HEAD) as usize;
    let mut iter = 0usize;
    while node != 0 && node != sent && iter < 200 && (W_N as usize) < WATCH_MAX {
        iter += 1;
        let ty = core::ptr::read_volatile((node + WF_TYPE) as *const u8) as u32;
        if ty == 3 || ty == 2 || (ty == 0 && W_SHOW_BUILTIN == 0) {
            node = rd32(node + WF_NEXT) as usize;
            continue;
        }
        let mut idbuf = [0u8; 16];
        let idp = (node + WF_ID) as *const u8;
        let mut k = 0usize;
        while k < 15 {
            let c = core::ptr::read_volatile(idp.add(k));
            if c == 0 {
                break;
            }
            idbuf[k] = c;
            k += 1;
        }
        let mut nb = [0u8; 24];
        let nmp = (node + WF_NAME) as *const u8;
        let mut k = 0usize;
        while k < 23 {
            let c = core::ptr::read_volatile(nmp.add(k));
            if c == 0 {
                break;
            }
            nb[k] = c;
            k += 1;
        }
        let i = W_N as usize;
        W_ID[i] = idbuf;
        W_NAME[i] = nb;
        W_CUR[i] = if cur != 0 && node == cur { 1 } else { 0 };
        W_NODE[i] = node as u32;
        W_N += 1;
        node = rd32(node + WF_NEXT) as usize;
    }
}

/// 把选中的表盘设成当前（重启后生效）。
unsafe fn watch_apply(idx: usize) {
    if idx >= W_N as usize {
        return;
    }
    let f: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_WF_USE | 1);
    let r = f(core::ptr::addr_of!(W_ID[idx]) as *const u8);
    let mut w = CW {
        p: core::ptr::addr_of_mut!(W_MSG) as *mut u8,
        n: 0,
        cap: 48,
    };
    if r == 0 {
        // "已切换，重启生效"
        w.s(&[0xE5, 0xB7, 0xB2, 0xE5, 0x88, 0x87, 0xE6, 0x8D, 0xA2, 0xEF, 0xBC, 0x8C]);
        w.s(&[0xE9, 0x87, 0x8D, 0xE5, 0x90, 0xAF, 0xE7, 0x94, 0x9F, 0xE6, 0x95, 0x88]);
    } else {
        // "切换失败"
        w.s(&[0xE5, 0x88, 0x87, 0xE6, 0x8D, 0xA2, 0xE5, 0xA4, 0xB1, 0xE8, 0xB4, 0xA5]);
    }
    w.end();
}

// ---- 存储详情页 ----
// 数据来自 /proc/fs/blocks：每行 "块大小 总块数 已用 可用 挂载点"。
static T_STORE: &[u8] = &[
    0xE5, 0xAD, 0x98, 0xE5, 0x82, 0xA8, 0xE8, 0xAF, 0xA6, 0xE6, 0x83, 0x85, 0x00,
]; // 存储详情
static STORE_PATH: &[u8] = b"/proc/fs/blocks\0";
const ST_MAX: usize = 8;
static mut ST_NAME: [[u8; 24]; ST_MAX] = [[0; 24]; ST_MAX];
static mut ST_MSG: [[u8; 40]; ST_MAX] = [[0; 40]; ST_MAX];
static mut ST_N: u32 = 0;

/// 读一整个 /proc 文本文件到 buf，返回字节数。
unsafe fn read_small(path: *const u8, buf: &mut [u8]) -> usize {
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 = core::mem::transmute(FW_OPEN | 1);
    let rd: unsafe extern "C" fn(i32, *mut u8, usize) -> isize = core::mem::transmute(FW_READ | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(path, 1, 0);
    if fd < 0 {
        return 0;
    }
    let mut n = 0usize;
    while n < buf.len() {
        let r = rd(fd, buf.as_mut_ptr().add(n), buf.len() - n);
        if r <= 0 {
            break;
        }
        n += r as usize;
    }
    cl(fd);
    n
}

/// 解析 /proc/fs/blocks，填 ST_NAME / ST_MSG。
unsafe fn storage_scan() {
    let mut buf = [0u8; 512];
    let n = read_small(STORE_PATH.as_ptr(), &mut buf);
    ST_N = 0;
    if n == 0 {
        return;
    }
    let mut i = 0usize;
    let mut lines = 0u32;
    while i < n && (ST_N as usize) < ST_MAX {
        // 找行尾
        let mut j = i;
        while j < n && buf[j] != b'\n' {
            j += 1;
        }
        let line = &buf[i..j];
        i = j + 1;
        lines += 1;
        if lines <= 2 {
            continue; // 两行表头
        }
        // 4 个数字 + 挂载点
        let mut k = 0usize;
        let mut nums = [0u32; 4];
        let mut ok = true;
        let mut p = 0usize;
        while p < 4 {
            while k < line.len() && line[k] == b' ' {
                k += 1;
            }
            let mut v = 0u32;
            let st = k;
            while k < line.len() && line[k] >= b'0' && line[k] <= b'9' {
                v = v.saturating_mul(10).saturating_add((line[k] - b'0') as u32);
                k += 1;
            }
            if k == st {
                ok = false;
                break;
            }
            nums[p] = v;
            p += 1;
        }
        if !ok {
            continue;
        }
        while k < line.len() && line[k] == b' ' {
            k += 1;
        }
        let idx = ST_N as usize;
        let mut w = CW {
            p: core::ptr::addr_of_mut!(ST_NAME[idx]) as *mut u8,
            n: 0,
            cap: 24,
        };
        while k < line.len() {
            w.s(&[line[k]]);
            k += 1;
        }
        w.end();
        let bs = nums[0] as u64;
        let total = (nums[1] as u64 * bs) as u32;
        let used = (nums[2] as u64 * bs) as u32;
        let mut w2 = CW {
            p: core::ptr::addr_of_mut!(ST_MSG[idx]) as *mut u8,
            n: 0,
            cap: 40,
        };
        w2.size(used);
        w2.s(b"/");
        w2.size(total);
        w2.end();
        ST_N += 1;
    }
}

/// 文件大小：open -> fstat -> close，取 stat 的 +0x18。
unsafe fn stat_size(path: *const u8) -> u32 {
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 = core::mem::transmute(FW_OPEN | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(path, 1, 0);
    if fd < 0 {
        return 0;
    }
    let mut st = [0u32; 24];
    let fs: unsafe extern "C" fn(i32, *mut u8) -> i32 = core::mem::transmute(FW_FSTAT | 1);
    let r = fs(fd, st.as_mut_ptr() as *mut u8);
    cl(fd);
    if r < 0 { 0 } else { st[6] }
}

unsafe fn rd32(a: usize) -> u32 {
    core::ptr::read_volatile(a as *const u32)
}

fn is_obj(a: u32) -> bool {
    (0x3C00_0000..0x3E00_0000).contains(&a)
}

unsafe fn prop(o: u32, p: u32, v: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32, u32) = core::mem::transmute(SET_PROP);
    f(o, p, v, 0);
}

unsafe fn fw_size(o: u32, w: u32, h: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_SIZE);
    f(o, w, h);
}

unsafe fn fw_pos(o: u32, x: u32, y: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_POS);
    f(o, x, y);
}

unsafe fn fw_bg(o: u32, c: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_BG);
    f(o, c, 0);
}

unsafe fn fw_pad(o: u32, p: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_PAD);
    f(o, p, 0);
}

unsafe fn fw_clr_flag(o: u32, bit: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_CLR_FLAG);
    f(o, bit);
}

unsafe fn fw_set_flag(o: u32, bit: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_SET_FLAG);
    f(o, bit);
}

unsafe fn fw_style_main(o: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_STYLE_MAIN);
    f(o, 0);
}

unsafe fn fw_style_sub(o: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_STYLE_SUB);
    f(o, 0);
}

unsafe fn fw_label_flag(o: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_LABEL_FLAG);
    f(o, 1);
}

/// 清卡片上继承来的样式：系统建完卡片后连着清几组样式与属性，
/// 不清就会留下类默认的那圈白边。四发都是通过函数指针派发的间接跳转。
unsafe fn fw_clr0(o: u32) {
    let f: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_CLR0);
    f(o, 0);
}
unsafe fn fw_clr1(o: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_CLR1);
    f(o, 0, 0);
}
unsafe fn fw_clr2(o: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_CLR2);
    f(o, 0, 0);
}
unsafe fn fw_clr3(o: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_CLR3);
    f(o, 0, 0);
}
unsafe fn fw_clr4(o: u32, prop: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_CLR4);
    f(o, prop, 0);
}
/// 卡片整体样式（系统建完卡片后最后调的那一发）
unsafe fn fw_card_style(o: u32) {
    let f: unsafe extern "C" fn(u32) = core::mem::transmute(FW_CARD_STYLE);
    f(o);
}

/// 只创建不初始化：框架的系统标签是「先 create 再 init」两发，
/// 走我们那个 create() 会初始化两次。
/// 把数字按固定宽度写进缓冲，返回写完后的位置。
/// 清掉对象继承来的一整段装饰样式（边框/描边/阴影那一区）。
/// 属性号区间来自框架的 prop 名字表；这里先整段清，定位到具体几项后再收窄。
unsafe fn fw_clear_decor(o: u32) {
    let mut p = 0x14u32;
    while p <= 0x57 {
        prop(o, p, 0);
        p += 1;
    }
}

/// 把对象上继承来的边框/描边/阴影清掉。
///
/// 属性号来自框架自己的 prop 名字表（PSRAM 0x3C34ACB0 起，名字按属性号排开，
/// `text_color=0x58`/`text_opa=0x59`/`text_font=0x5A` 与固件侧实测一致，可当锚）：
/// border_color=0x3C、border_opa=0x3D、border_width=0x3E、outline_width=0x41、
/// outline_color=0x42、outline_opa=0x43、shadow_width=0x45。
///
/// 类默认带一圈浅色描边，系统建完卡片会清掉；不清就多一条白边。
/// 系统标签：直接调框架自己的那个（内部是「按类建对象 + 初始化」两发）。
const FW_LABEL_NEW: usize = 0x0C78_BCB5;

unsafe fn fw_label(parent: u32) -> u32 {
    let f: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_LABEL_NEW);
    let l = f(parent);
    if l >= 0x3C00_0000 {
        if (LBL_N as usize) < LBL_TAB.len() {
            LBL_TAB[LBL_N as usize] = l;
            LBL_N += 1;
        }
        if FONT_FACE != 0 {
            prop(l, P_TEXT_FONT, FONT_FACE);
        }
    }
    l
}

/// 系统默认容器（页面/行卡片）的创建入口：父对象下建一个系统容器。
/// 系统「设置」页的页面与行都是它建的（RAMX 执行别名，SRAM 里的固件代码）。
const FW_CARD_NEW: usize = 0x0038_7B79;
/// 框架的内容容器创建：建一个类 0x3C34DE84 的对象当子容器。
/// 系统页面把行挂在这层里面，这层自带滚动；不挂在它上面就没有滚动。

/// 原生行：一张 416x120 的卡片，标题与副标题都是系统标签。
/// 尺寸、位置、底色、内边距、标志与事件全走固件自己的入口，
/// 抄的是系统「设置」页那条行的构建次序。
///
/// 行/开关共用的点击回调表：编号与 on_clickN 一一对应。
unsafe fn cb_for(idx: u32) -> u32 {
    let cb = match idx {
        0 => on_click0 as *const () as usize,
        1 => on_click1 as *const () as usize,
        2 => on_click2 as *const () as usize,
        3 => on_click3 as *const () as usize,
        4 => on_click4 as *const () as usize,
        5 => on_click5 as *const () as usize,
        6 => on_click6 as *const () as usize,
        7 => on_click7 as *const () as usize,
        8 => on_click8 as *const () as usize,
        9 => on_click9 as *const () as usize,
        20 => on_click20 as *const () as usize,
        21 => on_click21 as *const () as usize,
        30 => on_click30 as *const () as usize,
        31 => on_click31 as *const () as usize,
        22 => on_click22 as *const () as usize,
        23 => on_click23 as *const () as usize,
        24 => on_click24 as *const () as usize,
        25 => on_click25 as *const () as usize,
        26 => on_click26 as *const () as usize,
        27 => on_click27 as *const () as usize,
        _ => on_click_noop as *const () as usize,
    };
    cb as u32
}

/// 系统原生开关：类名实测 "switch"（0x3C34D774，默认 52x92），
/// 点一下自己就变蓝；程序置初态写对象 +0x30 的最低位再重设一次宽度触发重绘。
const FW_SWITCH_CLASS: u32 = 0x3C34_D774;
const SW_X: u32 = 320;
const SW_Y: u32 = 14;

#[allow(dead_code)]
unsafe fn switch_new(parent: u32, x: u32, y: u32, on: bool) -> u32 {
    let mk: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(0x0037_75BCusize | 1);
    let sw = mk(FW_SWITCH_CLASS, parent);
    if sw == 0 {
        return 0;
    }
    let init: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x0037_76B0usize | 1);
    init(sw, 0);
    fw_clr_flag(sw, 0x08); // 真 SCROLLABLE 位(v9 里 0x10 是 SCROLL_ELASTIC)
    fw_pos(sw, x, y);
    if on {
        let st = rd32(sw as usize + 0x30);
        core::ptr::write_volatile((sw as usize + 0x30) as *mut u32, st | 1);
        prop(sw, P_WIDTH, 52);
    }
    sw
}

/// 开关行：主标签 + 右端原生开关；点开关与点整行走同一个回调（只登记请求）。
/// 注意: 该页只要多挂这一个子对象就滚不动（换挂载点、清 SCROLLABLE/CLICKABLE、把类换成普通
/// obj 做对照，全都不解决），不是开关特有；接页面前必须先解掉这条，现在没有调用点。
unsafe fn row_switch(
    parent: u32,
    y: u32,
    h: u32,
    key: *const u8,
    value: *const u8,
    idx: u32,
    slot: u32,
    on: bool,
) -> u32 {
    // 开关占右端时主标签仍要放得下“重启恢复字体和图标”（10P 同一行是 9 个汉字）。
    let card = row_w(parent, y, h, key, value, idx, slot, 280, 280);
    if card == 0 {
        return 0;
    }
    // 开关挂在内容容器上、与卡片同级：挂在卡片里会让整页滚不动（实测）。
    let sw = switch_new(parent, SW_X + 8, y + SW_Y, on);
    if sw != 0 {
        // 只挂 CLICKED(7): 挂 ALL(0) 会把整页的手势吃掉、页面滚不动(实测)
        let reg: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(ADD_EVENT);
        reg(sw, cb_for(idx), 7, 0);
    }
    card
}

/// 系统原生复选框（类名实测 "checkbox"，0x3C224090）。勾选态与开关同一处：对象 +0x30 最低位。
/// 复选框在条目行里只做"当前项"指示，点击行为仍由整行负责。
const FW_CHECKBOX_CLASS: u32 = 0x3C22_4090;
const CK_X: u32 = 340;

#[allow(dead_code)]
unsafe fn checkbox_new(parent: u32, x: u32, y: u32, on: bool) -> u32 {
    let mk: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(0x0037_75BCusize | 1);
    let ck = mk(FW_CHECKBOX_CLASS, parent);
    if ck == 0 {
        return 0;
    }
    let init: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x0037_76B0usize | 1);
    init(ck, 0);
    fw_clr_flag(ck, 0x08);
    fw_pos(ck, x, y);
    if on {
        let st = rd32(ck as usize + 0x30);
        core::ptr::write_volatile((ck as usize + 0x30) as *mut u32, st | 1);
        prop(ck, P_WIDTH, 30);
    }
    ck
}

unsafe fn row_check(
    parent: u32,
    y: u32,
    h: u32,
    key: *const u8,
    value: *const u8,
    idx: u32,
    slot: u32,
    key_w: u32,
    val_w: u32,
    on: bool,
) -> u32 {
    let card = row_w(parent, y, h, key, value, idx, slot, key_w, val_w);
    if card == 0 {
        return 0;
    }
    checkbox_new(card, CK_X, SW_Y, on);
    card
}

/// 原生进度条：类名实测 "bar"（0x3C223B58），lv_bar_set_value = 0x0C784850
/// （夹在 min(+0x38)/max(+0x3C) 之间写 +0x34）。anim 传 0 = 不动画。
const FW_BAR_CLASS: u32 = 0x3C22_3B58;
const FW_BAR_SET_VALUE: usize = 0x0C78_4850;

unsafe fn bar_new(parent: u32, x: u32, y: u32, w: u32, h: u32, pct: u32) -> u32 {
    let mk: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(0x0037_75BCusize | 1);
    let bar = mk(FW_BAR_CLASS, parent);
    if bar == 0 {
        return 0;
    }
    let init: unsafe extern "C" fn(u32, u32) = core::mem::transmute(0x0037_76B0usize | 1);
    init(bar, 0);
    fw_clr_flag(bar, 0x08);
    fw_pos(bar, x, y);
    prop(bar, P_WIDTH, w);
    prop(bar, P_HEIGHT, h);
    let set: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_BAR_SET_VALUE | 1);
    set(bar, if pct > 100 { 100 } else { pct }, 0);
    bar
}

unsafe fn row(parent: u32, y: u32, h: u32, key: *const u8, value: *const u8, idx: u32, slot: u32) -> u32 {
    row_w(parent, y, h, key, value, idx, slot, 224, 224)
}

/// 与 `row` 同形，但两条标签的宽度可调（缓存页的目录名比 224 长）。
unsafe fn row_w(
    parent: u32,
    y: u32,
    h: u32,
    key: *const u8,
    value: *const u8,
    idx: u32,
    slot: u32,
    key_w: u32,
    val_w: u32,
) -> u32 {
    let mk: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(FW_CARD_NEW);
    let card = mk(parent, 0);
    if card == 0 {
        return 0;
    }
    #[cfg(feature = "navdbg")]
    {
        let cns = ROW_CNS[BUILD_PID];
        if (cns as usize) < ROW_HS[BUILD_PID].len() {
            ROW_HS[BUILD_PID][cns as usize] = card;
            ROW_CNS[BUILD_PID] = cns + 1;
        }
    }
    // 次序照抄系统「设置」页那条行：尺寸 -> 底色 -> 内边距 -> 位置 -> 标志 ->
    // 清三组样式 -> 事件 -> 属性 0x30 -> 卡片整体样式
    fw_clear_decor(card);
    fw_size(card, 416, h);
    fw_clr0(card);
    // 清装饰时本地 bg 被清成透明了，这里补回来（本地优先级高于样式表）
    prop(card, P_BG_COLOR, 0x222222);
    prop(card, P_BG_OPA, 0xFF);
    fw_bg(card, 0x222222);
    fw_pad(card, 32);
    fw_pos(card, 8, y);
    fw_clr_flag(card, 0x10);
    fw_set_flag(card, 2);
    fw_clr1(card);
    fw_clr2(card);
    fw_clr3(card);
    let addf: unsafe extern "C" fn(u32, u32) = core::mem::transmute(ADD_FLAG);
    addf(card, 0x2); // CLICKABLE
    let reg: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = core::mem::transmute(ADD_EVENT);
    reg(card, cb_for(idx), 7, 0);
    fw_clr4(card, 0x30);
    fw_card_style(card);
    let l = fw_label(card);
    if l != 0 {
        LAST_KEY = l;
        fw_size(l, key_w, 48);
        // 主标签在上、副标签在下，间距照系统那条行（17 / 67）
        let ly = if h > 83 { (h - 83) / 2 } else { (h - 48) / 2 };
        fw_pos(l, 20, ly);
        let set: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(LABEL_SET_TEXT);
        set(l, key);
        fw_style_main(l);
        fw_label_flag(l);
    }
    if !value.is_null() {
        let l2 = fw_label(card);
        if l2 != 0 {
            fw_size(l2, val_w, 37);
            fw_pos(l2, 20, if h > 83 { (h - 83) / 2 + 50 } else { (h - 37) / 2 });
            let set: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(LABEL_SET_TEXT);
            set(l2, value);
            fw_style_sub(l2);
            fw_label_flag(l2);
            if (slot as usize) < VAL_LABELS.len() {
                VAL_LABELS[slot as usize] = l2;
            }
        }
    }
    card
}

/// 桌面记录链：`+0x00` u16 app_id、`+0x04` 名字串、`+0x18` 上一条、`+0x1C` 下一条。
const DESK_NEXT: usize = 0x1C;
const DESK_PREV: usize = 0x18;

/// 给本应用的桌面图标定位：桌面网格是页面创建时排的，运行期加进来的图标
/// 没有位置（尺寸为 0 且带 HIDDEN）。我们按第一页第一个格子直接给它定位。
unsafe fn desk_place_icon(app_id: u32) {
    let head = rd32(DESK_LIST + 4);
    let mut p = head as usize;
    while p != 0 {
        if (rd32(p) & 0xFFFF) == app_id {
            let icon = rd32(p + 0x10);
            if icon != 0 {
                fw_size(icon, 96, 96);
                fw_pos(icon, 20, 40);
                fw_clr_flag(icon, 1); // 清 HIDDEN
            }
            return;
        }
        p = rd32(p + DESK_NEXT) as usize;
    }
}

/// 桌面图标保持在第一页第一个格子。
///
/// 桌面网格是页面创建时按 launcher.db 排的，运行期加进来的图标没有位置
/// （尺寸为 0 且带 HIDDEN）；桌面页一重建它又会被排掉。所以每几拍查一次，
/// 没位置就重新摆上去。
unsafe fn desk_keep_icon(app_id: u32) {
    let head = rd32(DESK_LIST + 4);
    let mut p = head as usize;
    while p != 0 {
        if (rd32(p) & 0xFFFF) == app_id {
            let icon = rd32(p + 0x10);
            if icon != 0 {
                let x1 = rd32(icon as usize + 0x1C);
                let x2 = rd32(icon as usize + 0x24);
                if x2 <= x1 {
                    fw_size(icon, 96, 96);
                    fw_pos(icon, 20, 40);
                    fw_clr_flag(icon, 1); // 清 HIDDEN
                }
            }
            return;
        }
        p = rd32(p + DESK_NEXT) as usize;
    }
}

/// 把本应用的桌面记录挪到链首。
///
/// 桌面网格按记录链的顺序排，链尾的记录只能落在第二页；挪到链首才进第一页。
/// 链是单向的：head -> ... -> tail，tail 的 next 为 0，head 的 prev 为 0。
unsafe fn desk_promote(app_id: u32) {
    let head = rd32(DESK_LIST + 4);
    if head == 0 {
        return;
    }
    let mut p = head as usize;
    let mut prev = 0usize;
    while p != 0 {
        if (rd32(p) & 0xFFFF) == app_id {
            break;
        }
        prev = p;
        p = rd32(p + DESK_NEXT) as usize;
    }
    if p == 0 || p == head as usize {
        return;
    }
    let next = rd32(p + DESK_NEXT);
    // 摘出：我们的前一个接上后一个，尾指向前一个
    if prev != 0 {
        core::ptr::write_volatile((prev + DESK_NEXT) as *mut u32, next);
    }
    core::ptr::write_volatile((DESK_LIST + 8) as *mut u32, prev as u32);
    // 接到头上
    core::ptr::write_volatile((p + DESK_PREV) as *mut u32, 0);
    core::ptr::write_volatile((p + DESK_NEXT) as *mut u32, head);
    core::ptr::write_volatile((head as usize + DESK_PREV) as *mut u32, p as u32);
    core::ptr::write_volatile((DESK_LIST + 4) as *mut u32, p as u32);
}
const DESK_NAME: usize = 0x04;
const DESK_PKG: usize = 0x08;

unsafe fn app_count() -> u32 {
    let mut p = rd32(DESK_LIST + 4);
    let mut n = 0u32;
    while is_obj(p) && n < 200 {
        n += 1;
        p = rd32(p as usize + DESK_NEXT);
    }
    n
}

/// 取第 `idx` 条桌面记录：返回 app_id，名字拷进 `out`（UTF-8 边界内截断）。
/// 把节点上的一个字符串字段拷进定长缓冲；只有被缓冲长度截断时才回退到多字节边界。
unsafe fn copy_field(p: u32, off: usize, out: *mut u8, cap: usize) {
    let s = rd32(p as usize + off) as usize;
    let mut n = 0usize;
    let mut cut = true;
    if s > 0x0C00_0000 && s < 0x4000_0000 {
        while n + 1 < cap {
            let b = core::ptr::read_volatile((s + n) as *const u8);
            if b == 0 {
                cut = false;
                break;
            }
            *out.add(n) = b;
            n += 1;
        }
    }
    if cut {
        while n > 0 && (*out.add(n - 1) & 0xC0) == 0x80 {
            n -= 1;
        }
    }
    *out.add(n) = 0;
}

unsafe fn app_at(
    idx: u32,
    name_out: *mut u8,
    name_cap: usize,
    pkg_out: *mut u8,
    pkg_cap: usize,
) -> u32 {
    let mut p = rd32(DESK_LIST + 4);
    let mut i = 0u32;
    while is_obj(p) && i < idx {
        p = rd32(p as usize + DESK_NEXT);
        i += 1;
    }
    if !is_obj(p) {
        return 0;
    }
    copy_field(p, DESK_NAME, name_out, name_cap);
    copy_field(p, DESK_PKG, pkg_out, pkg_cap);
    (rd32(p as usize) & 0xFFFF) as u32
}

unsafe fn u32_dec(mut v: u32, out: *mut u8) -> usize {
    let mut tmp = [0u8; 10];
    let mut n = 0;
    loop {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
        if v == 0 {
            break;
        }
    }
    for i in 0..n {
        *out.add(i) = tmp[n - 1 - i];
    }
    *out.add(n) = 0;
    n
}

// ---- 系统数据 ----
// 内存/存储两行的数据来自系统自己的 /proc 文本接口，格式由 NSH 的 `cat` 输出核对过。
// 装载器不导出 libc 符号，这三支是固件自己的 VFS 入口，从 nsh 的 cat 实现
// （file 0x4F0C98）里逐条读出来的：open(path,1) / read(fd,buf,0x400) / close(fd)。
// 只读标志是 1，不是 POSIX 的 0。
const FW_OPEN: usize = 0x1C15_80E4;
const FW_READ: usize = 0x1C15_8720;
/// 写入口：从 nsh 的 VFS 长跳表取（file 0x7FE040 -> 0x1C159A2D）。
const FW_WRITE: usize = 0x1C15_9A2C;
const FW_CLOSE: usize = 0x1C15_4D9C;
/// 目录三支（libc 真实现，镜像 file 0x437884/0x4378EC/0x4378C0）。
/// DIR 0x68 字节：+0x00 fd、+0x04 fd_dir（0x62 字节）；dirent = {d_type@0, d_name@1}。
const FW_OPENDIR: usize = 0x0C4F_7884;
const FW_READDIR: usize = 0x0C4F_78EC;
const FW_CLOSEDIR: usize = 0x0C4F_78C0;
/// 删除三支（PSRAM 里的 VFS，运行地址 = file + 0x1BFD023C）：
/// unlink file 0x1896D8、rmdir file 0x1888CC、fstat file 0x187414（buf+0x18 是大小）。
/// 从 nsh `rm`/`rm -r` 的调用点反查得到，已在 guest 里执行验证。
const FW_UNLINK: usize = 0x1C15_9914;
const FW_RMDIR: usize = 0x1C15_8B08;
const FW_FSTAT: usize = 0x1C15_7650;
/// 亮度两支：读当前有效亮度、写手动亮度（0-600）。
/// 都在 flash XIP 段：名字串 `brightness_set_value` 由 p65_nameref 反查得到，
/// 写的是状态块 +0x0C，量程上限 600（系统设置页就是这么传的）。
const FW_BRIGHT_GET: usize = 0x0CA6_5CA4;
const FW_BRIGHT_SET: usize = 0x0CA6_5C48;
/// 亮度状态块指针是**运行期重定位的字面量**：固件里 `ldr r3,[pc,#0xc]` 取的就是它。
/// 读同一个字面量就等于拿到状态块，不用另找入口。字面量在 0x0CA65CBC（读回 PSRAM 地址）。
const BRIGHT_LITERAL: usize = 0x0CA6_5CBC;

#[cfg(feature = "procfs")]
static MEMINFO_PATH: &[u8] = b"/proc/meminfo\0";
#[cfg(feature = "procfs")]
static VERSION_PATH: &[u8] = b"/proc/version\0";
#[cfg(feature = "procfs")]
static CPULOAD_PATH: &[u8] = b"/proc/cpuload\0";
#[cfg(feature = "procfs")]
static FSBLOCKS_PATH: &[u8] = b"/proc/fs/blocks\0";

/// 把 /proc/version 里 "version " 之后的部分截到 28 字节，给关于页一行用。
#[cfg(feature = "procfs")]
unsafe fn load_nuttx_version() {
    let mut buf = [0u8; 128];
    let n = read_file(VERSION_PATH.as_ptr(), &mut buf);
    if n == 0 {
        return;
    }
    let mut i = 0usize;
    let mut start = 0usize;
    while i + 8 <= n {
        if buf[i..i + 8] == *b"version " {
            start = i + 8;
            break;
        }
        i += 1;
    }
    let mut j = start;
    let mut k = 0usize;
    while j < n && k < 28 && buf[j] >= 0x20 && buf[j] < 0x7f {
        V_NUTTX[k] = buf[j];
        j += 1;
        k += 1;
    }
    V_NUTTX[k] = 0;
}

#[cfg(feature = "procfs")]
unsafe fn read_file(path: *const u8, buf: &mut [u8]) -> usize {
    let op: unsafe extern "C" fn(*const u8, i32, u32) -> i32 = core::mem::transmute(FW_OPEN | 1);
    let rd: unsafe extern "C" fn(i32, *mut u8, usize) -> isize = core::mem::transmute(FW_READ | 1);
    let cl: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_CLOSE | 1);
    let fd = op(path, 1, 0);
    if fd < 0 {
        return 0;
    }
    let mut n = 0usize;
    while n < buf.len() {
        let r = rd(fd, buf.as_mut_ptr().add(n), buf.len() - n);
        if r <= 0 {
            break;
        }
        n += r as usize;
    }
    cl(fd);
    n
}

// ---- 缓存清理实现 ----
// 遍历用 opendir/readdir/closedir；体积用 open+fstat+close；删除用 unlink/rmdir。
// 长操作只在拍里跑（进页那一次扫描也在 on_create，不碰事件派发链）。

struct CW {
    p: *mut u8,
    n: usize,
    cap: usize,
}

impl CW {
    unsafe fn s(&mut self, b: &[u8]) {
        for &c in b {
            if self.n + 1 < self.cap {
                core::ptr::write_volatile(self.p.add(self.n), c);
                self.n += 1;
            }
        }
    }

    unsafe fn num(&mut self, v: u32) {
        let mut t = [0u8; 10];
        let mut k = 0usize;
        let mut x = v;
        if x == 0 {
            t[0] = b'0';
            k = 1;
        }
        while x > 0 {
            t[k] = b'0' + (x % 10) as u8;
            x /= 10;
            k += 1;
        }
        while k > 0 {
            k -= 1;
            self.s(&[t[k]]);
        }
    }

    unsafe fn size(&mut self, bytes: u32) {
        if bytes >= 1048576 {
            let t = (bytes as u64 * 10 + 524288) / 1048576;
            self.num((t / 10) as u32);
            self.s(b".");
            self.num((t % 10) as u32);
            self.s(b"M");
        } else if bytes >= 1024 {
            let t = (bytes as u64 * 10 + 512) / 1024;
            self.num((t / 10) as u32);
            self.s(b".");
            self.num((t % 10) as u32);
            self.s(b"K");
        } else {
            self.num(bytes);
            self.s(b"B");
        }
    }

    unsafe fn end(&mut self) {
        if self.n < self.cap {
            core::ptr::write_volatile(self.p.add(self.n), 0);
        } else {
            core::ptr::write_volatile(self.p.add(self.cap - 1), 0);
        }
    }
}

unsafe fn cc_add_sat(v: &mut u32, amount: u32) {
    if 0xFFFF_FFFF - *v < amount {
        *v = 0xFFFF_FFFF;
    } else {
        *v += amount;
    }
}

/// 递归走 CC_WORK[0..plen]。clean=false 只统计；true 删文件 + 删空子目录。
unsafe fn cc_walk(plen: usize, depth: u32) {
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR | 1);
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR | 1);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR | 1);
    let dirp = od(CC_WORK.as_ptr());
    if dirp == 0 {
        return;
    }
    let mut iter = 0usize;
    loop {
        if iter >= 400 {
            break;
        }
        iter += 1;
        let de = rd(dirp);
        if de == 0 {
            break;
        }
        let dt = core::ptr::read_volatile(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = core::ptr::read_volatile(nm);
        let n2 = core::ptr::read_volatile(nm.add(1));
        let n3 = core::ptr::read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) {
            continue;
        }
        let mut o = plen;
        if o > 0 && core::ptr::read_volatile(CC_WORK.as_ptr().add(o - 1)) != b'/' {
            if o + 1 >= CC_WORK.len() {
                continue;
            }
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), b'/');
            o += 1;
        }
        let mut k = 0usize;
        while o + 1 < CC_WORK.len() {
            let c = core::ptr::read_volatile(nm.add(k));
            if c == 0 {
                break;
            }
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), c);
            o += 1;
            k += 1;
        }
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), 0);
        if dt == 8 {
            // DT_REG
            cc_add_sat(&mut CC_BYTES, stat_size(CC_WORK.as_ptr()));
            CC_FILES = CC_FILES.saturating_add(1);
        } else if dt == 4 {
            // DT_DIR
            if depth < CC_DEPTH_MAX {
                cc_walk(o, depth + 1);
            }
        }
        // 链接/设备等未知类型一律跳过，不打开不跟随
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(plen), 0);
    }
    cd(dirp);
}

unsafe fn cc_walk_roots() {
    CC_FILES = 0;
    CC_BYTES = 0;
    CC_FAIL = 0;
    let mut r = 0usize;
    while r < CC_ROOT_N {
        let root = CC_ROOTS[r];
        let mut i = 0usize;
        while i < root.len() {
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(i), root[i]);
            i += 1;
        }
        let f0 = CC_FILES;
        let b0 = CC_BYTES;
        cc_walk(root.len() - 1, 0);
        CC_ROOT_FILES[r] = CC_FILES - f0;
        CC_ROOT_BYTES[r] = CC_BYTES - b0;
        r += 1;
    }
}

unsafe fn cc_format_roots() {
    let mut r = 0usize;
    while r < CC_ROOT_N {
        let mut w = CW {
            p: (core::ptr::addr_of_mut!(CC_ROOT_MSG) as *mut u8).add(r * 48),
            n: 0,
            cap: 48,
        };
        if CC_ROOT_FILES[r] == 0 {
            w.s(&[0xE7, 0xA9, 0xBA]); // 空
        } else {
            w.num(CC_ROOT_FILES[r]);
            w.s(&[0x20, 0xE4, 0xB8, 0xAA, 0xE6, 0x96, 0x87, 0xE4, 0xBB, 0xB6, 0x20, 0x2F, 0x20]); // " 个文件 / "
            w.size(CC_ROOT_BYTES[r]);
        }
        w.end();
        r += 1;
    }
}

/// 进页扫描：只读统计 + 卡片副标签 + 顶部汇总文案。
unsafe fn cc_scan() {
    cc_walk_roots();
    let files = CC_FILES;
    let bytes = CC_BYTES;
    cc_format_roots();
    let mut w = CW {
        p: core::ptr::addr_of_mut!(CC_MSG) as *mut u8,
        n: 0,
        cap: 96,
    };
    if files == 0 {
        // "无可清理缓存"
        w.s(&[0xE6, 0x97, 0xA0, 0xE5, 0x8F, 0xAF, 0xE6, 0xB8, 0x85, 0xE7, 0x90, 0x86, 0xE7, 0xBC, 0x93, 0xE5, 0xAD, 0x98]);
        w.end();
    } else {
        // "缓存: N 个文件 / X"
        w.s(&[0xE7, 0xBC, 0x93, 0xE5, 0xAD, 0x98, 0x3A, 0x20]);
        w.num(files);
        w.s(&[0x20, 0xE4, 0xB8, 0xAA, 0xE6, 0x96, 0x87, 0xE4, 0xBB, 0xB6, 0x20, 0x2F, 0x20]);
        w.size(bytes);
        w.end();
    }
}

/// 真清的结果文案（写进 CC_MSG）。
unsafe fn cc_result_msg(files: u32, bytes: u32, fail: u32) {
    let mut w = CW {
        p: core::ptr::addr_of_mut!(CC_MSG) as *mut u8,
        n: 0,
        cap: 96,
    };
    w.s(&[0xE5, 0xB7, 0xB2, 0xE6, 0xB8, 0x85, 0xE7, 0x90, 0x86, 0x20]); // "已清理 "
    w.num(files);
    if fail > 0 {
        w.s(&[0x28, 0xE5, 0xA4, 0xB1, 0xE8, 0xB4, 0xA5, 0x20]); // "(失败 "
        w.num(fail);
        w.s(&[0x29]); // ")"
    } else if bytes > 0 {
        w.s(&[0x20, 0x2F, 0x20]); // " / "
        w.size(bytes);
    }
    w.end();
}

// ---- 清理作业：一拍只做一个 VFS 动作 ----
// 连着删十几个文件会把 UI 线程占死（实测第一次跑就出现了"应用退出"，
// 而目录里只少了第一个文件），所以清理拆成增量作业：
// 每拍开一个目录 / 读一个条目 / 删一个文件，路径前缀用栈记着。
static mut CC_JOB: u32 = 0; // 1 = 作业在跑
/// 本次作业清到哪个根为止（不含）。
static mut CC_JOB_END: usize = 0;
static mut CC_ROOT: usize = 0;
static mut CC_DIRP: u32 = 0;
static mut CC_PATH_LEN: usize = 0;
static mut CC_STACK_DIR: [u32; 9] = [0; 9];
static mut CC_STACK_LEN: [u8; 9] = [0; 9];
static mut CC_STACK_N: usize = 0;
static mut CC_ROOT_F0: u32 = 0;
static mut CC_ROOT_B0: u32 = 0;

/// 清理作业：只清某一个根目录（root = 0xFF 表示全清）。
unsafe fn cc_job_start(root: usize) {
    CC_FILES = 0;
    CC_BYTES = 0;
    CC_FAIL = 0;
    CC_ROOT = root;
    CC_JOB_END = if root == 0xFF { CC_ROOT_N } else { root + 1 };
    CC_DIRP = 0;
    CC_STACK_N = 0;
    CC_PATH_LEN = 0;
    CC_JOB = 1;
    let mut r = 0usize;
    while r < CC_ROOT_N {
        CC_ROOT_FILES[r] = 0;
        CC_ROOT_BYTES[r] = 0;
        r += 1;
    }
}

/// 走一步。返回 true 表示整个作业做完了。
unsafe fn cc_job_step() -> bool {
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR | 1);
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR | 1);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR | 1);
    let un: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_UNLINK | 1);
    let rmd: unsafe extern "C" fn(*const u8) -> i32 = core::mem::transmute(FW_RMDIR | 1);
    // 没有打开的目录：开下一个根（打不开的根直接跳过）
    if CC_DIRP == 0 {
        if CC_ROOT >= CC_JOB_END {
            return true;
        }
        let root = CC_ROOTS[CC_ROOT];
        let mut i = 0usize;
        while i < root.len() {
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(i), root[i]);
            i += 1;
        }
        CC_PATH_LEN = root.len() - 1;
        CC_ROOT_F0 = CC_FILES;
        CC_ROOT_B0 = CC_BYTES;
        let d = od(CC_WORK.as_ptr());
        if d == 0 {
            CC_ROOT += 1;
            return false;
        }
        CC_DIRP = d;
        CC_STACK_N = 0;
        return false;
    }
    // 取下一个条目
    let de = rd(CC_DIRP);
    if de == 0 {
        cd(CC_DIRP);
        CC_DIRP = 0;
        if CC_STACK_N > 0 {
            // 当前路径是刚清空的子目录，删掉它，再退回父目录
            if rmd(CC_WORK.as_ptr()) < 0 {
                CC_FAIL = CC_FAIL.saturating_add(1);
            }
            CC_STACK_N -= 1;
            CC_DIRP = CC_STACK_DIR[CC_STACK_N];
            CC_PATH_LEN = CC_STACK_LEN[CC_STACK_N] as usize;
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(CC_PATH_LEN), 0);
        } else {
            CC_ROOT_FILES[CC_ROOT] = CC_FILES - CC_ROOT_F0;
            CC_ROOT_BYTES[CC_ROOT] = CC_BYTES - CC_ROOT_B0;
            CC_ROOT += 1;
        }
        return false;
    }
    let dt = core::ptr::read_volatile(de as *const u8);
    let nm = (de + 1) as *const u8;
    let n1 = core::ptr::read_volatile(nm);
    let n2 = core::ptr::read_volatile(nm.add(1));
    let n3 = core::ptr::read_volatile(nm.add(2));
    if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) {
        return false;
    }
    // 拼子路径
    let mut o = CC_PATH_LEN;
    if o > 0 && core::ptr::read_volatile(CC_WORK.as_ptr().add(o - 1)) != b'/' {
        if o + 1 >= CC_WORK.len() {
            return false;
        }
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), b'/');
        o += 1;
    }
    let mut k = 0usize;
    while o + 1 < CC_WORK.len() {
        let c = core::ptr::read_volatile(nm.add(k));
        if c == 0 {
            break;
        }
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), c);
        o += 1;
        k += 1;
    }
    core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(o), 0);
    if dt == 8 {
        // DT_REG：量大小 + 删掉
        cc_add_sat(&mut CC_BYTES, stat_size(CC_WORK.as_ptr()));
        CC_FILES = CC_FILES.saturating_add(1);
        if un(CC_WORK.as_ptr()) < 0 {
            CC_FAIL = CC_FAIL.saturating_add(1);
        }
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(CC_PATH_LEN), 0);
    } else if dt == 4 {
        // DT_DIR：下降一层（父目录与父路径长度进栈）
        if CC_STACK_N < 9 {
            CC_STACK_DIR[CC_STACK_N] = CC_DIRP;
            CC_STACK_LEN[CC_STACK_N] = CC_PATH_LEN as u8;
            CC_STACK_N += 1;
            let d2 = od(CC_WORK.as_ptr());
            if d2 != 0 {
                CC_DIRP = d2;
                CC_PATH_LEN = o;
            } else {
                CC_STACK_N -= 1;
                core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(CC_PATH_LEN), 0);
            }
        } else {
            core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(CC_PATH_LEN), 0);
        }
    } else {
        // 链接/设备等未知类型：跳过，不打开不跟随
        core::ptr::write_volatile(CC_WORK.as_mut_ptr().add(CC_PATH_LEN), 0);
    }
    false
}

/// 扫描 F_PATH 指向的目录，填 F_NAME/F_ISDIR/F_SIZE/F_MSG。
unsafe fn files_scan() {
    let od: unsafe extern "C" fn(*const u8) -> u32 = core::mem::transmute(FW_OPENDIR | 1);
    let rd: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(FW_READDIR | 1);
    let cd: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(FW_CLOSEDIR | 1);
    F_N = 0;
    let dirp = od(F_PATH.as_ptr());
    if dirp == 0 {
        return;
    }
    let mut iter = 0usize;
    loop {
        if iter >= 400 {
            break;
        }
        iter += 1;
        let de = rd(dirp);
        if de == 0 {
            break;
        }
        let dt = core::ptr::read_volatile(de as *const u8);
        let nm = (de + 1) as *const u8;
        let n1 = core::ptr::read_volatile(nm);
        let n2 = core::ptr::read_volatile(nm.add(1));
        let n3 = core::ptr::read_volatile(nm.add(2));
        if n1 == b'.' && (n2 == 0 || (n2 == b'.' && n3 == 0)) {
            continue;
        }
        if F_N as usize >= FILES_MAX {
            break;
        }
        let i = F_N as usize;
        let mut k = 0usize;
        while k < 47 {
            let c = core::ptr::read_volatile(nm.add(k));
            if c == 0 {
                break;
            }
            F_NAME[i][k] = c;
            k += 1;
        }
        F_NAME[i][k] = 0;
        F_ISDIR[i] = if dt == 4 { 1 } else { 0 };
        let mut w = CW {
            p: core::ptr::addr_of_mut!(F_MSG[i]) as *mut u8,
            n: 0,
            cap: 20,
        };
        if dt == 4 {
            w.s(&[0xE7, 0x9B, 0xAE, 0xE5, 0xBD, 0x95]); // 目录
        } else if dt == 8 {
            // 普通文件：拼子路径量大小
            let mut tmp = [0u8; 176];
            let mut o = F_PATH_LEN;
            let mut j = 0usize;
            while j < F_PATH_LEN && j < 175 {
                tmp[j] = F_PATH[j];
                j += 1;
            }
            if o > 0 && tmp[o - 1] != b'/' {
                tmp[o] = b'/';
                o += 1;
            }
            let mut q = 0usize;
            while o + 1 < 176 {
                let c = F_NAME[i][q];
                if c == 0 {
                    break;
                }
                tmp[o] = c;
                o += 1;
                q += 1;
            }
            tmp[o] = 0;
            let sz = stat_size(tmp.as_ptr());
            F_SIZE[i] = sz;
            w.size(sz);
            w.end();
        } else {
            w.s(b"?");
        }
        w.end();
        F_N += 1;
    }
    cd(dirp);
}

/// 上一级（不越过 /data）。
unsafe fn files_up() {
    if F_PATH_LEN <= 5 {
        return;
    }
    let mut n = F_PATH_LEN;
    while n > 5 && F_PATH[n - 1] != b'/' {
        n -= 1;
    }
    if n > 5 {
        n -= 1;
    }
    F_PATH_LEN = n;
    F_PATH[n] = 0;
}

/// 进某个条目（只有目录才进）。
unsafe fn files_enter(idx: usize) {
    if idx >= F_N as usize || F_ISDIR[idx] == 0 {
        return;
    }
    let mut o = F_PATH_LEN;
    if o > 0 && F_PATH[o - 1] != b'/' {
        if o + 1 >= F_PATH.len() {
            return;
        }
        F_PATH[o] = b'/';
        o += 1;
    }
    let mut k = 0usize;
    while o + 1 < F_PATH.len() {
        let c = F_NAME[idx][k];
        if c == 0 {
            break;
        }
        F_PATH[o] = c;
        o += 1;
        k += 1;
    }
    F_PATH[o] = 0;
    F_PATH_LEN = o;
}

/// 走桌面记录链，取第 i 个应用的 (app_id, 显示名指针, 包名指针)。
unsafe fn desk_at(i: u32) -> (u32, u32, u32) {
    let mut p = rd32(DESK_LIST + 4);
    let mut k = 0u32;
    while p != 0 {
        if k == i {
            let id = rd32(p as usize) & 0xFFFF;
            return (id, rd32(p as usize + 4), rd32(p as usize + 8));
        }
        p = rd32(p as usize + DESK_NEXT);
        k += 1;
    }
    (0, 0, 0)
}

#[cfg(feature = "procfs")]
fn find_bytes(buf: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || buf.len() < needle.len() {
        return None;
    }
    let mut i = 0;
    while i + needle.len() <= buf.len() {
        let mut k = 0;
        while k < needle.len() && buf[i + k] == needle[k] {
            k += 1;
        }
        if k == needle.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 从 `i` 起跳过非数字，读一个十进制数；返回（数值, 结束位置）。
#[cfg(feature = "procfs")]
fn parse_num(buf: &[u8], mut i: usize) -> (u32, usize) {
    while i < buf.len() && !(buf[i] >= b'0' && buf[i] <= b'9') {
        i += 1;
    }
    let mut v = 0u32;
    while i < buf.len() && buf[i] >= b'0' && buf[i] <= b'9' {
        v = v.saturating_mul(10).saturating_add((buf[i] - b'0') as u32);
        i += 1;
    }
    (v, i)
}

/// 把字节数写成 "12.3M"（一位小数，四舍五入）。
#[cfg(feature = "procfs")]
unsafe fn fmt_mb(out: &mut [u8], bytes: u32) -> usize {
    let t = (bytes as u64 * 10 + 1048576 / 2) / 1048576;
    let mut n = u32_dec((t / 10) as u32, out.as_mut_ptr());
    if t % 10 != 0 {
        out[n] = b'.';
        out[n + 1] = b'0' + (t % 10) as u8;
        n += 2;
    }
    out[n] = b'M';
    n + 1
}

/// 只有值真的变了才写控件，避免每拍重排整行。
#[cfg(feature = "procfs")]
unsafe fn set_row_value(slot: usize, s: &[u8], prev: &mut [u8], plen: &mut usize) {
    if slot >= VAL_LABELS.len() || VAL_LABELS[slot] == 0 {
        return;
    }
    if *plen == s.len() && &prev[..s.len()] == s {
        return;
    }
    let mut text = [0u8; 32];
    for i in 0..s.len() {
        text[i] = s[i];
    }
    text[s.len()] = 0;
    let set: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(LABEL_SET_TEXT);
    set(VAL_LABELS[slot], text.as_ptr());
    prev[..s.len()].copy_from_slice(s);
    *plen = s.len();
}

/// 读 /proc/cpuload、/proc/meminfo 的 Umem 行、/proc/fs/blocks 的 /data 行，
/// 刷新主页三条数据行。
#[cfg(feature = "procfs")]
unsafe fn refresh_stats() {
    let mut buf = [0u8; 768];
    let mut s = [0u8; 32];

    // CPU 负载：文本形如 "  6.6%"，去掉前导空格后连 '%' 一起留。
    let n = read_file(CPULOAD_PATH.as_ptr(), &mut buf);
    if n > 0 {
        let mut i = 0usize;
        while i < n && (buf[i] == b' ' || buf[i] == b'\n' || buf[i] == b'\r') {
            i += 1;
        }
        let mut k = 0usize;
        while i < n && k < 12 {
            s[k] = buf[i];
            let stop = buf[i] == b'%';
            i += 1;
            k += 1;
            if stop {
                break;
            }
        }
        if k > 0 {
            set_row_value(0, &s[..k], &mut CPU_PREV, &mut CPU_PLEN);
        }
    }

    let n = read_file(MEMINFO_PATH.as_ptr(), &mut buf);
    if n > 0 {
        if let Some(p) = find_bytes(&buf[..n], b"Umem") {
            let (total, q) = parse_num(&buf[..n], p + 4);
            let (used, _) = parse_num(&buf[..n], q);
            if total > 0 {
                let m = fmt_mb(&mut s, used);
                s[m] = b'/';
                let k = fmt_mb(&mut s[m + 1..], total);
                set_row_value(1, &s[..m + 1 + k], &mut MEM_PREV, &mut MEM_PLEN);
                STAT_PCT[0] = (used.saturating_mul(100) / total) as u32;
            }
        }
    }

    // 百分比变了才写条形（值不变不写）
    let set_bar: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_BAR_SET_VALUE | 1);
    let mut bi = 0usize;
    while bi < 2 {
        let h = BAR_HANDLES[bi];
        if h >= 0x3C00_0000 {
            set_bar(h, if STAT_PCT[bi] > 100 { 100 } else { STAT_PCT[bi] }, 0);
        }
        bi += 1;
    }

    let n = read_file(FSBLOCKS_PATH.as_ptr(), &mut buf);
    if n > 0 {
        if let Some(p) = find_bytes(&buf[..n], b"/data") {
            let mut line = p;
            while line > 0 && buf[line - 1] != b'\n' {
                line -= 1;
            }
            let (bsize, q) = parse_num(&buf[..n], line);
            let (blocks, q) = parse_num(&buf[..n], q);
            let (used, _) = parse_num(&buf[..n], q);
            if bsize > 0 && blocks > 0 {
                let m = fmt_mb(&mut s, used.saturating_mul(bsize));
                s[m] = b'/';
                let k = fmt_mb(&mut s[m + 1..], blocks.saturating_mul(bsize));
                set_row_value(2, &s[..m + 1 + k], &mut ST_PREV, &mut ST_PLEN);
                STAT_PCT[1] = (used.saturating_mul(100) / blocks) as u32;
            }
        }
    }
}

/// 清掉旧内容、按 `which` 重画一整页。切页 = 删旧子树 + 建新子树。
unsafe fn build_page(which: u32) -> u32 {
    if ROOT == 0 {
        return 0;
    }
    // 重建会把内容容器（连同标签）删掉：先清标签表，别留已回收的句柄
    LBL_N = 0;
    #[cfg(feature = "navdbg")]
    {
        BUILD_PID = if (which as usize) < PAGE_N { which as usize } else { 0 };
        ROW_CNS[BUILD_PID] = 0;
    }
    if which == PAGE_MENU {
        for v in VAL_LABELS.iter_mut() {
            *v = 0;
        }
        #[cfg(feature = "procfs")]
        {
            CPU_PLEN = 0;
            MEM_PLEN = 0;
            ST_PLEN = 0;
        }
    }
    if which == PAGE_APPS {
        APP_N = 0;
    }
    if CONTENT >= 0x3C00_0000 {
        let del: unsafe extern "C" fn(u32) = core::mem::transmute(DEL_OBJ);
        del(CONTENT);
        CONTENT = 0;
    }
    // 层级照抄系统「设置」页：框架页面 -> 内容容器 -> 行卡片。
    // 行必须挂进内容容器那一层，那一层才有滚动。
    // 页面对象用系统的那个创建入口建（系统页面/行卡片都是它建的）：
    // 它的类自带滚动条样式，用别的类建就没有滚动条。
    let mk: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(FW_CARD_NEW);
    let page = mk(ROOT, 0);
    if page == 0 {
        return 0;
    }
    fw_clear_decor(page);
    // 页面这一串照抄系统页面（媒体页）：清样式 -> 置状态 -> 置 SCROLLABLE -> 滚动条那一发
    let f1: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_PAGE1);
    let f2: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_PAGE2);
    let f3: unsafe extern "C" fn(u32, u32, u32) = core::mem::transmute(FW_PAGE3);
    let f4: unsafe extern "C" fn(u32, u32, u32, u32) = core::mem::transmute(FW_PAGE4);
    f1(page, 0, 0);
    fw_clr1(page);
    fw_clr2(page);
    fw_clr3(page);
    fw_clr4(page, 0);
    f2(page, 0, 0);
    f3(page, 0, 0);
    f4(page, 1, 0, 0);
    prop(page, P_WIDTH, SCREEN_W);
    prop(page, P_HEIGHT, SCREEN_H);
    prop(page, P_X, 0);
    prop(page, P_Y, 0);
    for p in 0x10..0x14 {
        prop(page, p, 0);
    }
    prop(page, P_BG_COLOR, 0x000000);
    prop(page, P_BG_OPA, 0xFF);
    fw_clr0(page);
    fw_clr1(page);
    fw_clr2(page);
    fw_clr3(page);
    fw_clr4(page, 0);
    fw_clr_flag(page, 0x10); // 外壳不滚：滚的是内容容器
    // 内容容器：系统的行就挂在这层，它自带滚动与滚动条样式
    let content = mk(page, 0);
    if content != 0 {
        fw_clear_decor(content);
        fw_size(content, SCREEN_W, SCREEN_H - 56);
        fw_pos(content, 0, 56);
        prop(content, P_BG_COLOR, 0x000000);
        prop(content, P_BG_OPA, 0xFF);
        fw_clr0(content);
        fw_clr1(content);
        fw_clr2(content);
        fw_clr3(content);
        fw_clr4(content, 0);
        fw_set_flag(content, 0x10); // SCROLLABLE
        let sb: unsafe extern "C" fn(u32, u32) = core::mem::transmute(FW_SCROLLBAR);
        sb(content, 0x0C); // 滚动条那一发，系统在置完 SCROLLABLE 之后紧接着调
        CONTENT = content;
        // 列表滚动监听：只挂 SCROLL(11)，不挂 ALL（挂 ALL 会吃掉整页手势）
        let reg: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            core::mem::transmute(ADD_EVENT);
        reg(content, on_scroll_ev as *const () as usize as u32, 11, 0);
        CONTENTS[which as usize] = content;
    }
    let title: unsafe extern "C" fn(u32, *const u8, u32, u32, u32) -> u32 =
        core::mem::transmute(TITLE_CREATE);
    if which == PAGE_MENU {
        // 10P 的 Chaos 主页标题没有返回键（它是根页），子页才有
        title(page, T_CHAOS.as_ptr(), 0, 0, 0);
        V_COUNT[0] = 0;
        let n = u32_dec(app_count(), V_COUNT.as_mut_ptr());
        V_COUNT[n] = b' ';
        V_COUNT[n + 1] = 0xE4; // 个
        V_COUNT[n + 2] = 0xB8;
        V_COUNT[n + 3] = 0xAA;
        V_COUNT[n + 4] = 0;
        ROW_TODO = if cfg!(feature = "procfs") { 8 } else { 9 };
    } else if which == PAGE_ABOUT {
        title(page, T_ABOUT.as_ptr(), 1, 0, 0);
        #[cfg(feature = "procfs")]
        load_nuttx_version();
        build_stamp();
        ROW_TODO = 7;
    } else if which == PAGE_BRIGHT {
        title(page, T_BRIGHT.as_ptr(), 1, 0, 0);
        bright_fill();
        ROW_TODO = 4;
    } else if which == PAGE_DESK {
        if FONT_VIEW == BVIEW_FONT {
            title(page, K_FONT_MENU.as_ptr(), 1, 0, 0);
            font_scan();
            ROW_TODO = FONT_N + 1;
        } else if FONT_VIEW == BVIEW_BLUR {
            title(page, T_BLUR.as_ptr(), 1, 0, 0);
            ROW_TODO = 8;
        } else if FONT_VIEW == BVIEW_ZONE {
            title(page, T_ZONES.as_ptr(), 1, 0, 0);
            ROW_TODO = 5;
        } else {
            title(page, T_DESK.as_ptr(), 1, 0, 0);
            icon_packs_scan();
            ROW_TODO = ICON_PACK_N + 2;
        }
    } else if which == PAGE_VIB {
        title(page, T_VIB.as_ptr(), 1, 0, 0);
        vib_fill();
        ROW_TODO = 3;
    } else if which == PAGE_STORAGE {
        // 10P 页3：原生滚轮选年/月/日，底部保存。
        title(page, T_DATE.as_ptr(), 1, 0, 0);
        DATE_M = 0;
        DATE_D = 0;
        let now = clock_now() as i64 + TZ_OFF;
        let (_, mo, dd) = civil_from_days(now / 86400);
        // 日期页只有月/日两个轮（年份取当前时钟，和 10P 的日期页一致）
        DATE_M = picker_new(page, 110, 116, 110, 270, 1, 12, mo as i32);
        DATE_D = picker_new(page, 240, 116, 110, 270, 1, 31, dd as i32);
        fw_clr_flag(content, 0x10);
        ROW_TODO = 2;
    } else if which == PAGE_WATCH {
        title(page, T_WATCH.as_ptr(), 1, 0, 0);
        watch_scan();
        if W_MSG[0] == 0 {
            // 初始提示："点一个表盘切换"
            let mut w = CW {
                p: core::ptr::addr_of_mut!(W_MSG) as *mut u8,
                n: 0,
                cap: 48,
            };
            w.s(&[0xE7, 0x82, 0xB9, 0xE4, 0xB8, 0x80, 0xE4, 0xB8, 0xAA]); // 点一个
            w.s(&[0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98]); // 表盘
            w.s(&[0xE5, 0x88, 0x87, 0xE6, 0x8D, 0xA2]); // 切换
            w.end();
        }
        ROW_TODO = 10; // 0=显示内置表盘 1..8=条目 9=更多
    } else if which == PAGE_SETTINGS {
        title(page, K_SETTINGS.as_ptr(), 1, 0, 0);
        ROW_TODO = 6;
    } else if which == PAGE_BEAUTIFY {
        title(page, T_BEAUTIFY.as_ptr(), 1, 0, 0);
        ROW_TODO = 6;
    } else if which == PAGE_WFMGMT {
        title(page, T_WFMGMT.as_ptr(), 1, 0, 0);
        ROW_TODO = 1;
    } else if which == PAGE_LOG {
        title(page, T_LOG.as_ptr(), 1, 0, 0);
        ROW_TODO = 8;
    } else if which == PAGE_CACHE {
        title(page, T_CACHE.as_ptr(), 1, 0, 0);
        if CC_KEEP != 0 {
            CC_KEEP = 0; // 重建：保留刚出的结果文案与武装态
        } else {
            CC_ARM = 0;
            cc_scan();
        }
        ROW_TODO = 1 + CC_ROOT_N as u32 + 1;
    } else if which == PAGE_FILES {
        // 10P 页2：原生滚轮选时/分，底部保存。滚轮建在内容容器上。
        title(page, K_TIME_SET.as_ptr(), 1, 0, 0);
        TIME_H = 0;
        TIME_M = 0;
        let now = clock_now() as i64 + TZ_OFF;
        let sod = (now % 86400) as i32;
        TIME_H = picker_new(page, 84, 116, 110, 270, 0, 23, sod / 3600);
        TIME_M = picker_new(page, 238, 116, 110, 270, 0, 59, (sod % 3600) / 60);
        // 两轮之间的冒号（10P 的闹钟时间页同款分隔）
        let colon = fw_label(page);
        if colon != 0 {
            let set: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(LABEL_SET_TEXT);
            set(colon, K_COLON.as_ptr());
            fw_size(colon, 30, 60);
            fw_pos(colon, 202, 146);
            fw_style_main(colon);
            fw_label_flag(colon);
        }
        // 这一页不滚：滑动要落到滚轮上
        fw_clr_flag(content, 0x10);
        ROW_TODO = 2;
    } else {
        title(page, T_APPS.as_ptr(), 1, 0, 0);
        let total = app_count();
        // 应用页整份列表一次建完，靠内容容器滚动翻到底，不再分页。
        // 上限是行缓冲的容量（APP_BUF / APP_IDS 都是 48 条）。
        ROW_TODO = if total > 48 { 48 } else { total };
    }
    if CONTENT == 0 {
        CONTENT = page;
    }
    // 一次建完全部行：10P 那边是 build_rows 一次建完，拆成多拍会看见行一行行冒出来
    let mut i = 0u32;
    while i < ROW_TODO {
        build_row(which, i);
        i += 1;
    }
    ROW_TODO = 0;
    #[cfg(feature = "procfs")]
    if which == PAGE_MENU {
        refresh_stats();
    }
    page
}

/// 建第 `i` 行。应用页的行内容随页码变，所以每行现读桌面记录链。
unsafe fn build_row(which: u32, i: u32) {
    if CONTENT == 0 {
        return;
    }
    if which == PAGE_MENU {
        #[cfg(feature = "procfs")]
        {
            // 10P 主页：CPU/存储/内存 三行 + 应用管理/表盘管理/系统美化/设置/关于 五入口
            match i {
                0 => row(CONTENT, 14, 120, K_CPU.as_ptr(), V_WAIT.as_ptr(), 10, 0),
                1 => {
                    // 10P 存储行 = 值 + 进度条
                    let c = row(CONTENT, 142, 120, K_STORE.as_ptr(), V_WAIT.as_ptr(), 10, 2);
                    if LAST_KEY >= 0x3C00_0000 {
                        fw_pos(LAST_KEY, 20, 8);
                    }
                    if VAL_LABELS[2] >= 0x3C00_0000 {
                        fw_pos(VAL_LABELS[2], 20, 54);
                    }
                    BAR_HANDLES[1] = bar_new(c, 20, 98, 300, 8, STAT_PCT[1]);
                    c
                }
                2 => {
                    let c = row(CONTENT, 270, 120, K_MEM.as_ptr(), V_WAIT.as_ptr(), 10, 1);
                    if LAST_KEY >= 0x3C00_0000 {
                        fw_pos(LAST_KEY, 20, 8);
                    }
                    if VAL_LABELS[1] >= 0x3C00_0000 {
                        fw_pos(VAL_LABELS[1], 20, 54);
                    }
                    BAR_HANDLES[0] = bar_new(c, 20, 98, 300, 8, STAT_PCT[0]);
                    c
                }
                3 => row(CONTENT, 398, 120, T_APPS.as_ptr(), V_COUNT.as_ptr(), 3, 4),
                4 => row(CONTENT, 526, 120, T_WFMGMT.as_ptr(), core::ptr::null(), 4, 5),
                5 => row(CONTENT, 654, 120, T_BEAUTIFY.as_ptr(), core::ptr::null(), 5, 6),
                6 => row(CONTENT, 782, 120, K_SETTINGS.as_ptr(), core::ptr::null(), 6, 7),
                _ => row(CONTENT, 910, 120, T_ABOUT.as_ptr(), core::ptr::null(), 7, 8),
            };
        }
        #[cfg(not(feature = "procfs"))]
        {
            match i {
                0 => row(CONTENT, 14, 120, K_KERNEL.as_ptr(), V_KERNEL.as_ptr(), 4, 0),
                1 => row(CONTENT, 142, 120, K_SCREEN.as_ptr(), V_SCREEN.as_ptr(), 4, 1),
                2 => row(CONTENT, 270, 120, T_BRIGHT.as_ptr(), core::ptr::null(), 3, 2),
                3 => row(CONTENT, 398, 120, T_APPS.as_ptr(), V_COUNT.as_ptr(), 0, 3),
                4 => row(CONTENT, 526, 120, T_ABOUT.as_ptr(), core::ptr::null(), 1, 4),
                5 => row(CONTENT, 654, 120, T_CACHE.as_ptr(), core::ptr::null(), 2, 5),
                6 => row(CONTENT, 782, 120, T_DESK.as_ptr(), core::ptr::null(), 7, 6),
                7 => row(CONTENT, 910, 120, T_FILES.as_ptr(), core::ptr::null(), 6, 7),
                8 => row(CONTENT, 1038, 120, T_VIB.as_ptr(), core::ptr::null(), 5, 8),
                _ => row(CONTENT, 1166, 120, T_WATCH.as_ptr(), core::ptr::null(), 8, 9),
            };
        }
    } else if which == PAGE_BRIGHT {
        if i == 0 {
            row_w(
                CONTENT,
                14,
                120,
                K_BRIGHT_NOW.as_ptr(),
                core::ptr::addr_of!(V_BRIGHT) as *const u8,
                7,
                12,
                300,
                300,
            );
        } else if i <= 2 {
            // 两档预设（结构与 10P 亮度页一致：当前亮度 + 两档 + 自动亮度开关）
            let lv = (i - 1) as usize + 2;
            row(CONTENT, 14 + i * 128, 120, K_BRIGHT_LV[lv].as_ptr(), core::ptr::null(), i, 13);
        } else {
            // 自动亮度，右端是系统原生开关
            row_switch(
                CONTENT,
                14 + i * 128,
                120,
                K_BRIGHT_AUTO.as_ptr(),
                core::ptr::null(),
                5,
                14,
                bright_auto_get() != 0,
            );
        }
    } else if which == PAGE_SETTINGS {
        // 10P 设置页：时间/日期/亮度/震动/缓存/日志；时间与日期等滚轮页落地，先不出行
        // 10P 设置页六行：时间设置 / 日期设置 / 亮度 / 震动 / 缓存清理 / 运行日志
        match i {
            0 => row(CONTENT, 14, 120, K_TIME_SET.as_ptr(), core::ptr::null(), 30, 0),
            1 => row(CONTENT, 142, 120, K_DATE_SET.as_ptr(), core::ptr::null(), 31, 0),
            2 => row(CONTENT, 270, 120, T_BRIGHT.as_ptr(), core::ptr::null(), 0, 0),
            3 => row(CONTENT, 398, 120, T_VIB.as_ptr(), core::ptr::null(), 1, 0),
            4 => row(CONTENT, 526, 120, T_CACHE.as_ptr(), core::ptr::null(), 2, 0),
            _ => row(CONTENT, 654, 120, K_LOG.as_ptr(), core::ptr::null(), 3, 0),
        };
    } else if which == PAGE_LOG {
        // 10P 页17：更早一页 / 更新一页 / 刷新 / 清空日志(两点确认) / 4 条。
        // 前两行不适用时不占位：按"实际显示的第几个槽"顺排，避免留空档。
        let mut off = 0u32;
        if LOG_OFF + 4 >= LOG_N {
            off += 1;
        }
        if LOG_OFF == 0 {
            off += 1;
        }
        if i < off {
            return;
        }
        let y = 14 + (i - off) * 128;
        match i {
            0 => {
                row(CONTENT, y, 120, K_LOG_OLDER.as_ptr(), core::ptr::null(), 0, 0);
            }
            1 => {
                row(CONTENT, y, 120, K_LOG_NEWER.as_ptr(), core::ptr::null(), 1, 0);
            }
            2 => {
                row(CONTENT, y, 120, K_REFRESH.as_ptr(), core::ptr::null(), 2, 0);
            }
            3 => {
                row(
                    CONTENT,
                    y,
                    120,
                    if LOG_CLEAR_ARM == 0 { K_CLEAR_LOG.as_ptr() } else { K_CLEAR_ARMED.as_ptr() },
                    core::ptr::null(),
                    3,
                    0,
                );
            }
            _ => {
                let k = (i - 4) as u32 + LOG_OFF;
                if k < LOG_N {
                    let idx = (LOG_HEAD + LOG_MAX as u32 - 1 - k) as usize % LOG_MAX;
                    let p = core::ptr::addr_of!(LOG_BUF[idx]) as *const u8;
                    row_w(CONTENT, y, 120, p, core::ptr::null(), 9, 0, 320, 320);
                }
            }
        };
    } else if which == PAGE_BEAUTIFY {
        // 10P 页13：三档美化 + 两条重启恢复开关 + 返回。
        let st = if BEAUTY_SAVE_REQ != 0 { V_SAVE_BUSY.as_ptr() } else { V_SAVE_OK.as_ptr() };
        match i {
            0 => {
                row(CONTENT, 14, 120, K_FONT_MENU.as_ptr(), core::ptr::null(), 0, 0);
            }
            1 => {
                row(CONTENT, 142, 120, T_DESK.as_ptr(), core::ptr::null(), 1, 0);
            }
            2 => {
                row(CONTENT, 270, 120, K_BLUR_MENU.as_ptr(), core::ptr::null(), 2, 0);
            }
            3 => {
                row_switch(CONTENT, 398, 120, K_RESTORE_UI.as_ptr(), st, 24, 0,
                           BEAUTY_RESTORE_ON != 0);
            }
            4 => {
                row_switch(CONTENT, 526, 120, K_RESTORE_BLUR.as_ptr(), st, 25, 0,
                           BEAUTY_BLUR_RESTORE_ON != 0);
            }
            _ => {
                row(CONTENT, 654, 120, K_BACK.as_ptr(), core::ptr::null(), 22, 0);
            }
        }
    } else if which == PAGE_WFMGMT {
        // 10P 表盘管理：摇一摇轮换开关/摇一摇轮换/表盘切换；摇一摇无事件源，先只出表盘切换
        row(CONTENT, 14, 120, T_WATCH.as_ptr(), core::ptr::null(), 0, 0);
    } else if which == PAGE_WATCH {
        if i == 0 {
            // 10P 第一行：显示内置表盘（右端系统原生开关）
            row_switch(
                CONTENT,
                14,
                120,
                K_WATCH_SHOW.as_ptr(),
                core::ptr::null(),
                20,
                0,
                W_SHOW_BUILTIN != 0,
            );
        } else if i >= 9 {
            // 10P 的"更多"：翻到下一窗（每窗 8 条）
            row(CONTENT, 14 + i * 128, 120, K_MORE.as_ptr(), core::ptr::null(), 9, 0);
        } else {
            let k = (W_WIN * 8 + i - 1) as usize;
            if k < W_N as usize {
                // 主标签"表盘 N"，副标签显示包 id
                let mut buf = [0u8; 16];
                let mut w = CW { p: buf.as_mut_ptr(), n: 0, cap: 16 };
                w.s(&[0xE8, 0xA1, 0xA8, 0xE7, 0x9B, 0x98, 0x20]); // "表盘 "
                w.num(k as u32 + 1);
                w.end();
                // 主标签 = 节点上的显示名（空则退回"表盘 N"），副标签 = id，右端勾选=当前表盘
                let nm = core::ptr::addr_of!(W_NAME[k]) as *const u8;
                let mut primary = nm;
                if *nm == 0 {
                    let get: unsafe extern "C" fn(u32) -> *const u8 =
                        core::mem::transmute(0x0CA9_0C7Cusize | 1);
                    let p = get(*core::ptr::addr_of!(W_NODE[k]));
                    if !p.is_null() && *p != 0 {
                        primary = p;
                    } else {
                        primary = buf.as_ptr();
                    }
                }
                let idp = core::ptr::addr_of!(W_ID[k]) as *const u8;
                let cur = *core::ptr::addr_of!(W_CUR[k]) != 0;
                row_check(CONTENT, 14 + i * 128, 120, primary, idp, i, 0, 180, 180, cur);
            }
        }
    } else if which == PAGE_STORAGE {
        if i == 0 {
            row_w(
                CONTENT,
                252,
                80,
                core::ptr::addr_of!(DATE_MSG) as *const u8,
                core::ptr::null(),
                10,
                0,
                300,
                300,
            );
        } else {
            row(CONTENT, 342, 120, K_SAVE.as_ptr(), core::ptr::null(), 9, 0);
        }
    } else if which == PAGE_VIB {
        if i == 0 {
            // 10P 第一行：滑动列表震动（右端系统原生开关）
            row_switch(
                CONTENT,
                14,
                120,
                K_VIB_SCROLL.as_ptr(),
                core::ptr::null(),
                21,
                0,
                VIB_SCROLL_ON != 0,
            );
        } else if i == 1 {
            // 10P 第二行：测试震动
            row(CONTENT, 142, 120, K_VIB_TEST.as_ptr(), core::ptr::null(), 0, 0);
        } else {
            // 强度（只读）
            row_w(
                CONTENT,
                270,
                120,
                K_VIB_LEVEL.as_ptr(),
                core::ptr::addr_of!(VIB_LEVEL_MSG) as *const u8,
                9,
                0,
                300,
                300,
            );
        }
    } else if which == PAGE_DESK {
        if FONT_VIEW == BVIEW_FONT {
            // 10P 页5：动态字体条目（真复选框）+ 末行返回。
            if i < FONT_N {
                let slot = FONT_IDS[i as usize];
                let nm = font_name_ptr(i);
                let lp = (core::ptr::addr_of_mut!(FONT_SUB) as *mut u8)
                    .add((i as usize) * 24);
                let sp = (core::ptr::addr_of_mut!(FONT_SUB) as *mut u8)
                    .add((i as usize) * 24);
                if FONT_CUR == slot {
                    beauty_name_copy(sp, V_IN_USE.as_ptr(), 24);
                } else {
                    beauty_name_copy(sp, V_SLOT.as_ptr(), 24);
                    let mut n = 0usize;
                    while core::ptr::read_volatile(sp.add(n)) != 0 {
                        n += 1;
                    }
                    core::ptr::write_volatile(sp.add(n), b'0' + (slot % 10) as u8);
                    core::ptr::write_volatile(sp.add(n + 1), 0);
                }
                row_check(CONTENT, 14 + i * 128, 120, nm, sp, i, 0, 300, 300,
                          FONT_CUR == slot);
            } else {
                row(CONTENT, 14 + i * 128, 120, K_BACK.as_ptr(), core::ptr::null(), 22, 0);
            }
        } else if FONT_VIEW == BVIEW_BLUR {
            // 与 10P 页4 同序：系统背景 / 静态背景图 / 自动截取 / 手动生成 / 透明 / 状态 / 模糊区域 / 返回。
            match i {
                0 => {
                    row_check(CONTENT, 14, 120, K_SYS_BG.as_ptr(), core::ptr::null(), 0, 0,
                              300, 300, BLUR_MODE == 0);
                }
                1 => {
                    row_switch(CONTENT, 142, 120, K_STATIC_BG.as_ptr(), core::ptr::null(),
                               1, 0, BLUR_MODE == 1);
                }
                2 => {
                    row_switch(CONTENT, 270, 120, K_AUTO_BG.as_ptr(), core::ptr::null(),
                               2, 0, false);
                }
                3 => {
                    row(CONTENT, 398, 120, K_GEN_BG.as_ptr(), core::ptr::null(), 3, 0);
                }
                4 => {
                    row_switch(CONTENT, 526, 120, K_TRANSPARENT_BG.as_ptr(),
                               core::ptr::null(), 4, 0, BLUR_MODE == 3);
                }
                5 => {
                    let msg = if BLUR_MODE == 0 { K_SYS_BG.as_ptr() } else { K_STATIC_BG.as_ptr() };
                    row(CONTENT, 654, 120, msg, core::ptr::null(), 10, 0);
                }
                6 => {
                    row(CONTENT, 782, 120, K_ZONE_MENU.as_ptr(), core::ptr::null(), 6, 0);
                }
                _ => {
                    row(CONTENT, 910, 120, K_BACK.as_ptr(), core::ptr::null(), 22, 0);
                }
            }
        } else if FONT_VIEW == BVIEW_ZONE {
            // 10P 页15：四个区域开关 + 返回。
            if i < 4 {
                let key = match i {
                    0 => K_ZONE0.as_ptr(),
                    1 => K_ZONE1.as_ptr(),
                    2 => K_ZONE2.as_ptr(),
                    _ => K_ZONE3.as_ptr(),
                };
                row_switch(CONTENT, 14 + i * 128, 120, key, core::ptr::null(),
                           24 + i, 0, (BLUR_ZONE_MASK >> i) & 1 != 0);
            } else {
                row(CONTENT, 14 + i * 128, 120, K_BACK.as_ptr(), core::ptr::null(), 22, 0);
            }
        } else {
            // 10P 页6：系统原图标 + 投递包 + 返回。
            if i == 0 {
                row_check(CONTENT, 14, 120, K_SYS_ICON.as_ptr(), K_FW_ICON.as_ptr(),
                          0, 0, 300, 300, ICON_PACK_CUR == 0);
            } else if i <= ICON_PACK_N {
                let r = (i - 1) as usize;
                let sel = ICON_PACK_CUR == ICON_PACK_ID[r];
                row_check(CONTENT, 14 + i * 128, 120, ICON_PACK_NAME[r].as_ptr(),
                          K_FW_ICON.as_ptr(), i, 0, 300, 300, sel);
            } else {
                row(CONTENT, 14 + i * 128, 120, K_BACK.as_ptr(), core::ptr::null(), 22, 0);
            }
        }
    } else if which == PAGE_CACHE {
        // 10P 缓存页：行0 = 汇总/结果文案，行1..4 = 四个根目录卡（路径 + 占用），
        // 行5 = 清理按钮（第一下武装，第二下真删）。卡片不可点。
        if i == 0 {
            row_w(
                CONTENT,
                14,
                120,
                core::ptr::addr_of!(CC_MSG) as *const u8,
                core::ptr::null(),
                10,
                0,
                380,
                380,
            );
        } else if i <= CC_ROOT_N as u32 {
            let r = (i - 1) as usize;
            let root = CC_ROOTS[r];
            let msg = core::ptr::addr_of!(CC_ROOT_MSG[r]) as *const u8;
            row_w(CONTENT, 14 + i * 128, 120, root.as_ptr(), msg, 10, 0, 380, 380);
        } else {
            let label = if CC_ARM != 0 { V_ARM.as_ptr() } else { K_CACHE_START.as_ptr() };
            row_w(CONTENT, 14 + i * 128, 120, label, core::ptr::null(), 3, 0, 380, 380);
        }
    } else if which == PAGE_FILES {
        if i == 0 {
            row_w(
                CONTENT,
                252,
                80,
                core::ptr::addr_of!(TIME_MSG) as *const u8,
                core::ptr::null(),
                10,
                0,
                300,
                300,
            );
        } else {
            row(CONTENT, 342, 120, K_SAVE.as_ptr(), core::ptr::null(), 8, 0);
        }
    } else if which == PAGE_ABOUT {
        // 名字最长的是 IKUN_CXKPRO，标签宽给到 300 才不被截断
        match i {
            0 => row_w(CONTENT, 14, 120, T_CHAOS.as_ptr(), V_APPVER.as_ptr(), 0, 5, 300, 300),
            1 => row_w(CONTENT, 142, 120, K_BUILD.as_ptr(), core::ptr::addr_of!(V_BUILD) as *const u8, 1, 6, 300, 300),
            2 => row_w(CONTENT, 270, 120, K_FW.as_ptr(), if V_NUTTX[0] != 0 { V_NUTTX.as_ptr() } else { V_WAIT.as_ptr() }, 2, 7, 300, 300),
            3 => row_w(CONTENT, 398, 120, K_DEV1.as_ptr(), V_DEV.as_ptr(), 3, 8, 300, 300),
            4 => row_w(CONTENT, 526, 120, K_DEV2.as_ptr(), V_DEV.as_ptr(), 4, 9, 300, 300),
            5 => row_w(CONTENT, 654, 120, K_DEV3.as_ptr(), V_DEV.as_ptr(), 5, 10, 300, 300),
            _ => row_w(CONTENT, 782, 120, K_LICENSE.as_ptr(), V_LICENSE.as_ptr(), 6, 11, 300, 300),
        };
    } else {
        let total = app_count();
        let base = if APPS_OFF < total { APPS_OFF } else { 0 };
        if base + i >= total {
            return;
        }
        let id = app_at(
            base + i,
            APP_BUF[i as usize].as_mut_ptr(),
            40,
            APP_PKG[i as usize].as_mut_ptr(),
            40,
        );
        let mut vb = [0u8; 12];
        let n = u32_dec(id, vb.as_mut_ptr());
        vb[n] = 0;
        for k in 0..n + 1 {
            APP_VAL[i as usize][k] = vb[k];
        }
        APP_IDS[i as usize] = id;
        APP_N = i + 1;
        let val = if APP_PKG[i as usize][0] != 0 {
            APP_PKG[i as usize].as_ptr()
        } else {
            APP_VAL[i as usize].as_ptr()
        };
        row(CONTENT, 14 + i * 128, 120, APP_BUF[i as usize].as_ptr(), val, i, 15);
    }
}

#[no_mangle]
pub extern "C" fn on_click0(_e: u32) -> i32 {
    unsafe { WANT = 0; }
    0
}

#[no_mangle]
pub extern "C" fn on_click1(_e: u32) -> i32 {
    unsafe { WANT = 1; DIAG[60] = DIAG[60].wrapping_add(1); }
    0
}

#[no_mangle]
pub extern "C" fn on_click2(_e: u32) -> i32 {
    unsafe { WANT = 2; }
    0
}

#[no_mangle]
pub extern "C" fn on_click3(_e: u32) -> i32 {
    unsafe { WANT = 3; }
    0
}

#[no_mangle]
pub extern "C" fn on_click4(_e: u32) -> i32 {
    unsafe { WANT = 4; }
    0
}

#[no_mangle]
pub extern "C" fn on_click5(_e: u32) -> i32 {
    unsafe { WANT = 5; }
    0
}

/// 20 号槽：开关行（"显示内置表盘"这类）的整行点击。开关只是状态指示，
/// 点击落点由卡片接管(实测点开关位置命中的是卡片)。
#[no_mangle]
pub extern "C" fn on_click20(_e: u32) -> i32 {
    unsafe { WANT = 20; }
    0
}

/// 30/31 号槽：设置页前两行（时间设置 / 日期设置）。RW6 侧写时钟的入口还没取证，
/// 这两行先按系统设置里那条改时间链走：打开系统设置 App。
#[no_mangle]
pub extern "C" fn on_click30(_e: u32) -> i32 {
    unsafe { WANT = 30; }
    0
}

#[no_mangle]
pub extern "C" fn on_click31(_e: u32) -> i32 {
    unsafe { WANT = 31; }
    0
}

#[no_mangle]
pub extern "C" fn on_click22(_e: u32) -> i32 {
    unsafe { WANT = 22; }
    0
}

#[no_mangle]
pub extern "C" fn on_click23(_e: u32) -> i32 {
    unsafe { WANT = 23; }
    0
}

#[no_mangle]
pub extern "C" fn on_click24(_e: u32) -> i32 {
    unsafe { WANT = 24; }
    0
}

#[no_mangle]
pub extern "C" fn on_click25(_e: u32) -> i32 {
    unsafe { WANT = 25; }
    0
}

#[no_mangle]
pub extern "C" fn on_click26(_e: u32) -> i32 {
    unsafe { WANT = 26; }
    0
}

#[no_mangle]
pub extern "C" fn on_click27(_e: u32) -> i32 {
    unsafe { WANT = 27; }
    0
}

/// 21 号槽：「滑动列表震动」开关行。
#[no_mangle]
pub extern "C" fn on_click21(_e: u32) -> i32 {
    unsafe { WANT = 21; }
    0
}

/// 内容容器的 SCROLL 事件（11）：事件链里只登记，不在派发链里震动。
#[no_mangle]
pub extern "C" fn on_scroll_ev(_e: u32) -> i32 {
    unsafe {
        if VIB_SCROLL_ON != 0 {
            VIB_SCROLL_REQ = 1;
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn on_click6(_e: u32) -> i32 {
    unsafe { WANT = 6; }
    0
}

#[no_mangle]
pub extern "C" fn on_click7(_e: u32) -> i32 {
    unsafe { WANT = 7; }
    0
}

#[no_mangle]
pub extern "C" fn on_click9(_e: u32) -> i32 {
    unsafe { WANT = 9; }
    0
}

#[no_mangle]
pub extern "C" fn on_click8(_e: u32) -> i32 {
    unsafe { WANT = 8; }
    0
}


/// 压栈跳到本应用的某一页：页键 = app_id<<16 | page_index，转场动画由固件给。
unsafe fn goto_page(index: u32) -> i32 {
    log_add(b"page", index);
    let f: unsafe extern "C" fn(u32, u32, u32, u32) -> i32 = core::mem::transmute(FW_PAGE_GOTO);
    f((APP_ID << 16) | index, 0, 0, 0)
}

/// 列表里那些只展示、不跳转的行挂这个。
#[no_mangle]
pub extern "C" fn on_click_noop(_e: u32) -> i32 {
    0
}

/// 注册应用与页（描述符全零后只填必要字段；页数组是指针数组）。
unsafe fn register() -> bool {
    let look: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(APP_LOOKUP);
    if look(APP_ID) != 0 {
        return true;
    }
    for i in 0..16 {
        APP_DESC[i] = 0;
    }
    // 以系统已有的原生应用（运动，id 19）的描述符为底：全零描述符能注册、能打开，
    // 但挂到桌面时固件建出的图标是隐藏且零尺寸的。克隆保留了类型与可见性这些字段。
    let proto = look(0x13);
    if proto != 0 {
        let src = proto as *const u32;
        for i in 0..16 {
            APP_DESC[i] = core::ptr::read_volatile(src.add(i));
        }
    }
    APP_DESC[2] = PKG.as_ptr() as u32;
    APP_DESC[3] = ICON.as_ptr() as u32;
    APP_DESC[4] = APP_ID;
    APP_DESC[7] = on_name as *const () as usize as u32;
    // 每页 29 word；页键低 16 位是页索引、高 16 位是应用 id（10P 的注册布局）。
    let pnames: [*const u8; PAGE_N] = [
        P_MENU.as_ptr(),
        P_ABOUT.as_ptr(),
        P_APPS.as_ptr(),
        P_CACHE.as_ptr(),
        P_BRIGHT.as_ptr(),
        P_FILES.as_ptr(),
        P_DESK.as_ptr(),
        P_VIB.as_ptr(),
        P_STORAGE.as_ptr(),
        P_WATCH.as_ptr(),
        P_SETTINGS.as_ptr(),
        P_BEAUTIFY.as_ptr(),
        P_WFMGMT.as_ptr(),
        P_LOG.as_ptr(),
    ];
    let mut pg = 0usize;
    while pg < PAGE_N {
        let mut i = 0usize;
        while i < 29 {
            PAGE_DESC[pg][i] = 0;
            i += 1;
        }
        PAGE_DESC[pg][4] = pnames[pg] as u32;
        PAGE_DESC[pg][5] = (APP_ID << 16) | pg as u32;
        PAGE_DESC[pg][13] = on_signal as *const () as usize as u32;
        PAGE_DESC[pg][19] = on_create as *const () as usize as u32;
        PAGE_DESC[pg][20] = on_resume as *const () as usize as u32;
        PAGE_DESC[pg][22] = on_pause as *const () as usize as u32;
        PAGE_DESC[pg][23] = on_destroy as *const () as usize as u32;
        PAGE_DESC[pg][24] = on_ui_destroy as *const () as usize as u32;
        pg += 1;
    }
    let base = core::ptr::addr_of!(PAGE_DESC) as u32;
    let mut items: [u32; PAGE_N] = [0; PAGE_N];
    let mut k = 0usize;
    while k < PAGE_N {
        items[k] = base + (k as u32) * 116;
        k += 1;
    }
    let ins: unsafe extern "C" fn(*const u32, *const u32, u32) -> u32 = core::mem::transmute(APP_INSTALL);
    ins(core::ptr::addr_of!(APP_DESC) as *const u32, items.as_ptr(), PAGE_N as u32);
    look(APP_ID) != 0
}

/// 由固件自己打开本应用（带系统转场）。
unsafe fn open_app_id(id: u32) -> u32 {
    let mut req = [0u32; 14];
    req[1] = (id << 16) | 1; // u16@+0x06 = app_id
    req[5] = 0x1C13_75B1;
    req[8] = 0x1C13_7641;
    req[13] = 0x33;
    let f: unsafe extern "C" fn(*const u32) -> u32 = core::mem::transmute(OPEN_APP);
    f(req.as_ptr())
}

unsafe fn open_self() -> u32 {
    open_app_id(OPEN_TARGET)
}

#[no_mangle]
pub extern "C" fn on_name() -> *const u8 {
    APP_NAME.as_ptr()
}

#[no_mangle]
pub extern "C" fn on_signal() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn on_create(desc: u32, root: u32, _data: u32) -> i32 {
    unsafe {
        let pid = core::ptr::read_volatile((desc + 0x14) as *const u16) as u32;
        if pid < PAGE_N as u32 {
            CUR = pid;
        }
        ROOTS[CUR as usize] = root;
        ROOT = root;
        CONTENT = 0;
        beauty_load();
        if FACE == 0 {
            let mf: unsafe extern "C" fn(*const u8, u32, u32) -> u32 = core::mem::transmute(MFONT);
            FACE = mf(FONT_NAME.as_ptr(), 22, 0);
        }
        build_page(CUR);
        DIAG[1] = ROOT;
        DIAG[2] = rd32(ROOT as usize);          // class
        DIAG[3] = rd32(ROOT as usize + 0x2C);   // flags
    }
    0
}

#[no_mangle]
pub extern "C" fn on_resume(desc: u32, _a: u32, _b: u32) -> i32 {
    unsafe {
        let pid = core::ptr::read_volatile((desc + 0x14) as *const u16) as u32;
        if pid < PAGE_N as u32 {
            CUR = pid;
            ROOT = ROOTS[pid as usize];
            CONTENT = CONTENTS[pid as usize];
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn on_pause(_d: u32, _a: u32, _b: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn on_destroy(_d: u32, _a: u32, _b: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn on_ui_destroy(_d: u32, _a: u32, _b: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn chaos_tick(_t: u32) {
    unsafe {
        TICKS = TICKS.wrapping_add(1);
        // 导航回归用：把"当前页号"写进固定 scratch，宿主机直接读，不依赖截图
        #[cfg(feature = "navdbg")]
        {
            core::ptr::write_volatile((MARK + 0x10) as *mut u32, CUR);
            core::ptr::write_volatile((MARK + 0x14) as *mut u32, ROOT);
            // 专用探针（backprobe）：第一次进表盘切换页就调一次 page_back(0)，调用计数写 +0x44
            #[cfg(feature = "backprobe")]
            if CUR == PAGE_WATCH && BACK_CALLS == 0 {
                BACK_CALLS = 1;
                let back: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(0x0C22_2621usize);
                let _ = back(0);
            }
            core::ptr::write_volatile((MARK + 0x44) as *mut u32, BACK_CALLS);
            core::ptr::write_volatile((MARK + 0x30) as *mut u32, DATE_M);
            core::ptr::write_volatile((MARK + 0x34) as *mut u32, DATE_D);
            core::ptr::write_volatile((MARK + 0x38) as *mut u32, TIME_H);
            let alive = if ROOT != 0 && rd32(ROOT as usize + 8) != 0 { 1u32 } else { 0u32 };
            let pid = if (CUR as usize) < PAGE_N { CUR as usize } else { 0 };
            let n = ROW_CNS[pid];
            let sp = core::ptr::addr_of_mut!(NAV_SNAP) as *mut u32;
            core::ptr::write_volatile(sp.add(1), CUR);
            core::ptr::write_volatile(sp.add(2), ROOT);
            core::ptr::write_volatile(sp.add(3), alive);
            core::ptr::write_volatile(sp.add(4), n);
            let mut i = 0u32;
            while i < n && (i as usize) < ROW_HS[pid].len() {
                let h = ROW_HS[pid][i as usize];
                let dst = sp.add(5 + (i as usize) * 4);
                if h != 0 {
                    let src = (h as usize + 0x1C) as *const u32;
                    core::ptr::write_volatile(dst, core::ptr::read_volatile(src));
                    core::ptr::write_volatile(dst.add(1), core::ptr::read_volatile(src.add(1)));
                    core::ptr::write_volatile(dst.add(2), core::ptr::read_volatile(src.add(2)));
                    core::ptr::write_volatile(dst.add(3), core::ptr::read_volatile(src.add(3)));
                } else {
                    core::ptr::write_volatile(dst, 0);
                    core::ptr::write_volatile(dst.add(1), 0);
                    core::ptr::write_volatile(dst.add(2), 0);
                    core::ptr::write_volatile(dst.add(3), 0);
                }
                i += 1;
            }
        }
        match TICKS {
            1 => {
                register();
            }
            20 => {
                if AUTO_OPEN {
                    open_self();
                }
            }
            40 => {
                // 调试：应用打开后直接跳到某一页（跳触摸坐标，验页面用）
                if cfg!(feature = "openvib") {
                    goto_page(PAGE_VIB);
                }

                if cfg!(feature = "openwatch") {
                    goto_page(PAGE_WATCH);
                }
                if cfg!(feature = "openbright") {
                    goto_page(PAGE_BRIGHT);
                }
                if cfg!(feature = "openabout") {
                    goto_page(PAGE_ABOUT);
                }
                if cfg!(feature = "openlog") {
                    goto_page(PAGE_LOG);
                }
            }
            _ => {}
        }
        #[cfg(feature = "wfprobe")]
        if TICKS == 60 {
            probe_watchface();
        }
        if cfg!(feature = "tour") && TICKS >= 60 && (TICKS - 60) % 20 == 0 {
            // 巡览：每 20 拍换一页，1..PAGE_N-1 依次过一遍
            let k = 1 + ((TICKS - 60) / 20) % (PAGE_N as u32 - 1);
            goto_page(k);
        }
        if !ADDED && TICKS == 10 {
            mark(0xADD0_0001);
            let add: unsafe extern "C" fn(u32) = core::mem::transmute(LAUNCHER_ADD);
            add(APP_ID);
            let rf: unsafe extern "C" fn(u32) = core::mem::transmute(LAUNCHER_REFRESH);
            rf(APP_ID);
            mark(0xADD0_0002);
            desk_promote(APP_ID);
            desk_place_icon(APP_ID);
            ADDED = true;
        }
        if TICKS % 10 == 0 && ADDED {
            desk_keep_icon(APP_ID);
        }
        if FONT_DIRTY != 0 && ROOT != 0 && CUR != PAGE_BEAUTIFY {
            FONT_DIRTY = 0;
            build_page(CUR);
        }
        if B_ICON_REQ != B_ICON_NONE {
            let pack = B_ICON_REQ;
            if icon_pack_apply_one(pack) {
                B_ICON_REQ = B_ICON_NONE;
                ICON_PACK_DIRTY = 0;
                if CUR == PAGE_DESK && FONT_VIEW == BVIEW_ICON && ROOT != 0 {
                    build_page(PAGE_DESK);
                }
            }
        }
        if FONT_DEL_REQ != 0 {
            let slot = FONT_DEL_REQ;
            FONT_DEL_REQ = 0;
            font_delete_requested(slot);
            if CUR == PAGE_DESK && FONT_VIEW == BVIEW_FONT && ROOT != 0 {
                build_page(PAGE_DESK);
            }
        }
        if BEAUTY_SAVE_REQ != 0 {
            BEAUTY_SAVE_REQ = 0;
            beauty_save();
            if CUR == PAGE_BEAUTIFY && ROOT != 0 {
                build_page(PAGE_BEAUTIFY);
            }
        }
        if ROW_TODO > 0 && ROOT != 0 {
            build_row(CUR, ROW_IDX);
            ROW_IDX += 1;
            ROW_TODO -= 1;
        }
        if WANT != u32::MAX && ROOT != 0 {
            let want = WANT;
            WANT = u32::MAX;
            if CUR == PAGE_MENU {
                match want {
                    3 => {
                        APPS_OFF = 0;
                        goto_page(PAGE_APPS);
                    }
                    4 => {
                        goto_page(PAGE_WFMGMT);
                    }
                    5 => {
                        goto_page(PAGE_BEAUTIFY);
                    }
                    6 => {
                        goto_page(PAGE_SETTINGS);
                    }
                    7 => {
                        goto_page(PAGE_ABOUT);
                    }
                    _ => {}
                }
            } else if CUR == PAGE_WATCH {
                if want == 20 {
                    // 显示内置表盘 开关：改过滤条件后重扫重排
                    W_SHOW_BUILTIN = if W_SHOW_BUILTIN != 0 { 0 } else { 1 };
                    W_WIN = 0;
                    watch_scan();
                    if CUR == PAGE_WATCH && ROOT != 0 {
                        build_page(PAGE_WATCH);
                    }
                } else if want == 9 {
                    // 更多：翻窗
                    if (W_WIN + 1) * 8 < W_N {
                        W_WIN += 1;
                        if CUR == PAGE_WATCH && ROOT != 0 {
                            build_page(PAGE_WATCH);
                        }
                    }
                } else if want >= 1 && want <= 8 {
                    // 点条目：设成当前（重启后生效）
                    let k = (W_WIN * 8 + want - 1) as usize;
                    if k < W_N as usize {
                        W_REQ = k as u32;
                    }
                }
            } else if CUR == PAGE_SETTINGS {
                match want {
                    30 => {
                        goto_page(PAGE_FILES);
                    }
                    31 => {
                        goto_page(PAGE_STORAGE);
                    }
                    0 => {
                        goto_page(PAGE_BRIGHT);
                    }
                    1 => {
                        goto_page(PAGE_VIB);
                    }
                    2 => {
                        goto_page(PAGE_CACHE);
                    }
                    3 => {
                        goto_page(PAGE_LOG);
                    }
                    _ => {}
                }
            } else if CUR == PAGE_LOG {
                match want {
                    0 => {
                        // 更早一页
                        if LOG_OFF + 4 < LOG_N {
                            LOG_OFF += 4;
                        }
                        if CUR == PAGE_LOG && ROOT != 0 {
                            build_page(PAGE_LOG);
                        }
                    }
                    1 => {
                        // 更新一页
                        LOG_OFF = LOG_OFF.saturating_sub(4);
                        if CUR == PAGE_LOG && ROOT != 0 {
                            build_page(PAGE_LOG);
                        }
                    }
                    2 => {
                        if CUR == PAGE_LOG && ROOT != 0 {
                            build_page(PAGE_LOG);
                        }
                    }
                    3 => {
                        if LOG_CLEAR_ARM == 0 {
                            LOG_CLEAR_ARM = 1;
                        } else {
                            LOG_CLEAR_ARM = 0;
                            LOG_N = 0;
                            LOG_HEAD = 0;
                            let mut i = 0usize;
                            while i < LOG_MAX {
                                LOG_BUF[i] = [0; 40];
                                i += 1;
                            }
                        }
                        if CUR == PAGE_LOG && ROOT != 0 {
                            build_page(PAGE_LOG);
                        }
                    }
                    _ => {}
                }
            } else if CUR == PAGE_BEAUTIFY {
                match want {
                    0 => {
                        FONT_VIEW = BVIEW_FONT;
                        font_scan();
                        goto_page(PAGE_DESK);
                    }
                    1 => {
                        FONT_VIEW = BVIEW_ICON;
                        icon_packs_scan();
                        goto_page(PAGE_DESK);
                    }
                    2 => {
                        FONT_VIEW = BVIEW_BLUR;
                        goto_page(PAGE_DESK);
                    }
                    24 => {
                        BEAUTY_RESTORE_ON = if BEAUTY_RESTORE_ON != 0 { 0 } else { 1 };
                        BEAUTY_SAVE_REQ = 1;
                        if ROOT != 0 {
                            build_page(PAGE_BEAUTIFY);
                        }
                    }
                    25 => {
                        BEAUTY_BLUR_RESTORE_ON =
                            if BEAUTY_BLUR_RESTORE_ON != 0 { 0 } else { 1 };
                        BEAUTY_SAVE_REQ = 1;
                        if ROOT != 0 {
                            build_page(PAGE_BEAUTIFY);
                        }
                    }
                    22 if TICKS.wrapping_sub(BACK_TICK) > 10 => {
                        BACK_TICK = TICKS;
                        let back: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(PAGE_BACK);
                        let _ = back(0);
                    }
                    _ => {}
                }
            } else if CUR == PAGE_WFMGMT {
                if want == 0 {
                    W_WIN = 0;
                    goto_page(PAGE_WATCH);
                }
            } else if CUR == PAGE_VIB {
                if want == 0 {
                    VIB_REQ = 1;
                    VIB_REQ_MODE = 0;
                } else if want == 21 {
                    VIB_SCROLL_ON = if VIB_SCROLL_ON != 0 { 0 } else { 1 };
                    if CUR == PAGE_VIB && ROOT != 0 {
                        build_page(PAGE_VIB);
                    }
                }
            } else if CUR == PAGE_DESK {
                if FONT_VIEW == BVIEW_FONT {
                    DIAG[60] = 0xF000_0000 | want;
                    DIAG[61] = FONT_CUR;
                    DIAG[62] = FONT_N;
                    DIAG[63] = FONT_IDS[0];
                    if want == 22 && TICKS.wrapping_sub(BACK_TICK) > 10 {
                        BACK_TICK = TICKS;
                        let back: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(PAGE_BACK);
                        let _ = back(0);
                    } else if want < FONT_N {
                        let slot = FONT_IDS[want as usize];
                        if FONT_CUR != slot {
                            font_apply(want);
                            if ROOT != 0 {
                                build_page(PAGE_DESK);
                            }
                        }
                    }
                } else if FONT_VIEW == BVIEW_BLUR {
                    match want {
                        0 | 1 | 4 => {
                            BLUR_MODE = if want == 0 { 0 } else if want == 1 { 1 } else { 3 };
                            BEAUTY_SAVE_REQ = 1;
                            if ROOT != 0 {
                                build_page(PAGE_DESK);
                            }
                        }
                        6 => {
                            FONT_VIEW = BVIEW_ZONE;
                            if ROOT != 0 {
                                build_page(PAGE_DESK);
                            }
                        }
                        22 if TICKS.wrapping_sub(BACK_TICK) > 10 => {
                            BACK_TICK = TICKS;
                            let back: unsafe extern "C" fn(u32) -> i32 =
                                core::mem::transmute(PAGE_BACK);
                            let _ = back(0);
                        }
                        _ => {}
                    }
                } else if FONT_VIEW == BVIEW_ZONE {
                    if want >= 24 && want <= 27 {
                        let bit = 1u32 << (want - 24);
                        BLUR_ZONE_MASK ^= bit;
                        BEAUTY_SAVE_REQ = 1;
                        if ROOT != 0 {
                            build_page(PAGE_DESK);
                        }
                    } else if want == 22 && TICKS.wrapping_sub(BACK_TICK) > 10 {
                        BACK_TICK = TICKS;
                        let back: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(PAGE_BACK);
                        let _ = back(0);
                    }
                } else {
                    if want == 22 && TICKS.wrapping_sub(BACK_TICK) > 10 {
                        BACK_TICK = TICKS;
                        let back: unsafe extern "C" fn(u32) -> i32 = core::mem::transmute(PAGE_BACK);
                        let _ = back(0);
                    } else if want == 0 {
                        ICON_PACK_CUR = 0;
                        ICON_PACK_DIRTY = 1;
                        BEAUTY_SAVE_REQ = 1;
                        icon_pack_start(0);
                        if ROOT != 0 {
                            build_page(PAGE_DESK);
                        }
                    } else if want <= ICON_PACK_N {
                        ICON_PACK_CUR = ICON_PACK_ID[(want - 1) as usize];
                        ICON_PACK_DIRTY = 1;
                        BEAUTY_SAVE_REQ = 1;
                        icon_pack_start(ICON_PACK_CUR);
                        if ROOT != 0 {
                            build_page(PAGE_DESK);
                        }
                    }
                }
            } else if CUR == PAGE_CACHE {
                // 点「开始清理」：第一下武装，第二下才真删。卡片行不可点。
                if want == 3 {
                    if CC_ARM == 0 {
                        CC_ARM = 1;
                        CC_KEEP = 1;
                        CC_REDRAW = 1;
                    } else {
                        CC_REQ = 1;
                        CC_REQ_ROOT = 0xFF;
                    }
                }
            } else if CUR == PAGE_FILES {
                // 保存：读两个轮 -> 本地时分 -> UTC epoch -> clock_settime（写时钟入口已执行验证）
                if want == 8 {
                    let h = picker_value(TIME_H, 0, 23) as i64;
                    let m = picker_value(TIME_M, 0, 59) as i64;
                    let now = clock_now() as i64 + TZ_OFF;
                    let days = now / 86400;
                    let secs = days * 86400 + h * 3600 + m * 60 - TZ_OFF;
                    let r = clock_set(if secs > 0 { secs as u64 } else { 0 });
                    log_add(b"timeset", r as u32);
                    time_msg_saved(h as i32, m as i32);
                    if ROOT != 0 {
                        build_page(PAGE_FILES);
                    }
                }
            } else if CUR == PAGE_STORAGE {
                // 保存日期：读三个轮 -> 当天时分不变 -> UTC epoch -> clock_settime
                if want == 9 {
                    let mo = picker_value(DATE_M, 1, 12) as i64;
                    let dd = picker_value(DATE_D, 1, 31) as i64;
                    let now = clock_now() as i64 + TZ_OFF;
                    let (y, _, _) = civil_from_days(now / 86400);
                    let sod = now % 86400;
                    let secs = days_from_civil(y, mo as u32, dd as u32) * 86400 + sod - TZ_OFF;
                    let r = clock_set(if secs > 0 { secs as u64 } else { 0 });
                    log_add(b"dateset", r as u32);
                    date_msg_saved(y, mo as i32, dd as i32);
                    if ROOT != 0 {
                        build_page(PAGE_STORAGE);
                    }
                }
            } else if CUR == PAGE_APPS {
                // 点应用行 = 由固件打开那个应用，页面本身不动
                let idx = want as usize;
                if idx < APP_IDS.len() {
                    let id = APP_IDS[idx];
                    if id != 0 && id != APP_ID {
                        open_app_id(id);
                    }
                }
            }
        }
        // 亮度：设值动作放在拍里做（回调只登记），改完重排那一页刷新读数。
        if BRIGHT_AUTO_REQ != 0 {
            let on = if BRIGHT_AUTO_REQ == 1 { 1 } else { 0 };
            BRIGHT_AUTO_REQ = 0;
            bright_auto_set(on);
            DIAG[41] = on;
            BRIGHT_DIRTY = 1;
        }
        if BRIGHT_REQ != 0 {
            let v = BRIGHT_REQ;
            BRIGHT_REQ = 0;
            bright_set(v);
            log_add(b"bright", v);
            DIAG[40] = v;
            BRIGHT_DIRTY = 1;
        }
        if BRIGHT_DIRTY != 0 {
            BRIGHT_DIRTY = 0;
            bright_fill();
            if CUR == PAGE_BRIGHT && ROOT != 0 {
                build_page(PAGE_BRIGHT);
            }
        }
        // 表盘切换：动作放拍里，改完重排该页把结果文案刷出来。
        if W_REQ != u32::MAX {
            let k = W_REQ as usize;
            W_REQ = u32::MAX;
            watch_apply(k);
            log_add(b"wf", k as u32);
            if CUR == PAGE_WATCH && ROOT != 0 {
                build_page(PAGE_WATCH);
            }
        }
        // 滑动列表震动：滚动事件只登记，震动在这里发（派发链里不许震动）
        if VIB_SCROLL_REQ != 0 {
            VIB_SCROLL_REQ = 0;
            if VIB_SCROLL_ON != 0 {
                vib_run();
                log_add(b"vibscroll", 1);
            }
        }
        // 震动：设模式 / 震一次都放拍里做。
        if VIB_REQ != 0 {
            let m = VIB_REQ_MODE;
            VIB_REQ = 0;
            VIB_REQ_MODE = 0;
            if m != 0 {
                vib_set_mode(m);
            }
            let r = vib_run();
            DIAG[45] = r as u32;
            VIB_DIRTY = 1;
        }
        if VIB_DIRTY != 0 {
            VIB_DIRTY = 0;
            vib_fill();
            if CUR == PAGE_VIB && ROOT != 0 {
                build_page(PAGE_VIB);
            }
        }
        // 缓存的清理与重建都放在拍里做，事件回调只登记请求。
        if CC_REQ != 0 {
            CC_REQ = 0;
            if CUR == PAGE_CACHE && ROOT != 0 {
                cc_job_start(CC_REQ_ROOT);
            }
        }
        if CC_JOB != 0 {
            let done = cc_job_step();
            DIAG[30] = CC_ROOT as u32;
            DIAG[31] = CC_FILES;
            DIAG[32] = CC_FAIL;
            if done {
                CC_JOB = 0;
                let files = CC_FILES;
                let bytes = CC_BYTES;
                let fail = CC_FAIL;
                cc_scan();
                cc_result_msg(files, bytes, fail);
                CC_ARM = 0;
                CC_KEEP = 1;
                CC_REDRAW = 1;
            }
        }
        if CC_REDRAW != 0 {
            if CUR == PAGE_CACHE && ROOT != 0 {
                CC_REDRAW = 0;
                build_page(PAGE_CACHE);
            } else {
                CC_REDRAW = 0;
                CC_KEEP = 0;
            }
        }
        #[cfg(feature = "procfs")]
        if TICKS % 50 == 0 && CUR == PAGE_MENU {
            refresh_stats();
        }
    }
}

#[cfg(feature = "symtest")]
extern "C" {
    fn clock_gettime(clk_id: i32, tp: *mut u32) -> i32;
}

/// 未定义符号解析实验：装载器若能解析 `clock_gettime`，模块就能直接用内核导出符号。
#[cfg(feature = "symtest")]
#[no_mangle]
pub unsafe extern "C" fn module_main(_argc: i32, _argv: *const *const u8) -> i32 {
    let mut ts = [0u32; 4];
    let r = clock_gettime(0, ts.as_mut_ptr());
    DIAG[0] = 0xC0DE_5EED;
    DIAG[1] = r as u32;
    DIAG[2] = ts[0];
    DIAG[3] = ts[1];
    0
}

#[cfg(not(feature = "symtest"))]
#[no_mangle]
pub unsafe extern "C" fn module_main(_argc: i32, _argv: *const *const u8) -> i32 {
    let create: unsafe extern "C" fn(u32, u32, u32) -> u32 = core::mem::transmute(TIMER_CREATE);
    let _ = create(chaos_tick as *const () as usize as u32, 200, 0);
    0
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
