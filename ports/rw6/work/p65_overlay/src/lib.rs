//! Chaos 在 Redmi Watch 6（p65 / best1502p）上的 Overlay 骨架。
//!
//! 思路：不走固件的页面系统（那套在本机模拟环境里导航即崩），而是把一棵全屏容器直接
//! 挂到当前屏幕上，自己管"哪一屏"：切页 = 删旧子树 + 建新子树，退出 = 删根。
//!
//! 所有固件调用都只用已经在本机执行确认过的入口：
//!   建控件 `create_obj(类, 父)` + `init(obj, 0)`、写文本、样式漏斗、加标志、加事件、定时器、删对象。
//!
//! 事件回调里只登记"要切到哪一页"，真正的重建放在定时器拍上（与 043 侧的纪律一致：
//! 回调不改控件树）。

#![no_std]

use core::panic::PanicInfo;

const LOG: usize = 0x0C50_4A55;
const CREATE: usize = 0x0037_75BD;
const INIT: usize = 0x0037_76B1;
const STYLE: usize = 0x0037_8965;
const SET_TEXT: usize = 0x0C78_C879;
const ADD_EVENT: usize = 0x0C25_70BD;
const ADD_FLAG: usize = 0x0038_7B99;
const TIMER_CREATE: usize = 0x0C74_14D9;
const OBJ_DELETE: usize = 0x0037_FE3F;
const MFONT_NAMED: usize = 0x0C21_88ED;

/// LVGL 全局根：`lv_display_get_default()` 读的就是 `*(0x3C450030 + 0x14)`。
const GC_ROOT: usize = 0x3C45_0030;

const OBJ_CLASS: u32 = 0x3C1D_E5C8;
const LABEL_CLASS: u32 = 0x0C31_4608;

const ST_W: u32 = 0x01;
const ST_H: u32 = 0x04;
const ST_X: u32 = 0x07;
const ST_Y: u32 = 0x08;
const ST_BG_COLOR: u32 = 0x1C;
const ST_BG_OPA: u32 = 0x1D;
const ST_TEXT_FONT: u32 = 0x5A;

const FLAG_CLICKABLE: u32 = 2;
const EV_CLICKED: u32 = 7;

const PAGE_ABOUT: u32 = 0;
const PAGE_LIST: u32 = 1;

/// 结果通道（宿主 `lsmod` 拿 DATA 基址后按魔数找）：[0]=魔数 [1]=根对象 [2]=当前页 [3]=拍计数
#[no_mangle]
pub static mut OVERLAY_STATE: [u32; 8] = [0x4F56_4C31, 0, 0, 0, 0, 0, 0, 0];   // "OVL1"

static mut ROOT: u32 = 0;
static mut SCREEN: u32 = 0;
static mut FACE: u32 = 0;
static mut CUR: u32 = PAGE_ABOUT;
static mut WANT: u32 = 0xFFFF_FFFF;
static mut TICK: u32 = 0;
static mut BUILT: u32 = 0;
static mut TICKS: u32 = 0;

/// 一个指针像不像 LVGL 对象：首字是 lv_obj_class 或某个控件类（类表在 PSRAM 上）。
unsafe fn looks_like_obj(p: u32) -> bool {
    if p < 0x3C000000 || p >= 0x3E000000 {
        return false;
    }
    let c = core::ptr::read_volatile(p as *const u32);
    c == OBJ_CLASS || c == LABEL_CLASS || (c >= 0x3C200000 && c < 0x3C400000)
}

/// display 里找当前屏：不同启动这一段的偏移不一定相同，按"像对象 + parent 为空"挑。
unsafe fn find_screen(disp: u32) -> u32 {
    let cands = [0x2B0u32, 0x2AC, 0x2B4, 0x2BC, 0x2C0, 0x2C4, 0x2A8, 0x2C8];
    for off in cands {
        let v = core::ptr::read_volatile((disp + off) as *const u32);
        if looks_like_obj(v) {
            let parent = core::ptr::read_volatile((v as usize + 4) as *const u32);
            if parent == 0 {
                return v;
            }
        }
    }
    0
}

unsafe fn log(s: &'static [u8]) {
    let f: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(LOG);
    f(4, s.as_ptr());
}

unsafe fn create(class: u32, parent: u32) -> u32 {
    let f: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(CREATE);
    let init: unsafe extern "C" fn(u32, u32) = core::mem::transmute(INIT);
    let o = f(class, parent);
    if o != 0 {
        init(o, 0);
    }
    o
}

unsafe fn prop(o: u32, p: u32, v: u32) {
    let f: unsafe extern "C" fn(u32, u32, u32, u32) = core::mem::transmute(STYLE);
    f(o, p, v, 0);
}

unsafe fn text(o: u32, s: *const u8) {
    let f: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(SET_TEXT);
    f(o, s);
}

unsafe fn label(parent: u32, s: *const u8, x: u32, y: u32) -> u32 {
    let o = create(LABEL_CLASS, parent);
    if o != 0 {
        text(o, s);
        prop(o, ST_X, x);
        prop(o, ST_Y, y);
        if FACE != 0 {
            prop(o, ST_TEXT_FONT, FACE);
        }
    }
    o
}

/// 一行可点的条目：容器 + 文本，带 CLICKABLE 与点击回调，user_data 是目标页号。
unsafe fn row(parent: u32, s: *const u8, y: u32, target: u32) -> u32 {
    let r = create(OBJ_CLASS, parent);
    if r == 0 {
        return 0;
    }
    prop(r, ST_W, 400);
    prop(r, ST_H, 64);
    prop(r, ST_X, 16);
    prop(r, ST_Y, y);
    prop(r, ST_BG_COLOR, 0x1A1A1A);
    prop(r, ST_BG_OPA, 0xFF);
    label(r, s, 16, 18);
    let addf: unsafe extern "C" fn(u32, u32) = core::mem::transmute(ADD_FLAG);
    addf(r, FLAG_CLICKABLE);
    let adde: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = core::mem::transmute(ADD_EVENT);
    let _ = adde(r, on_row as usize as u32, EV_CLICKED, target);
    r
}

/// 输入派发链上的回调：只登记目标页，不碰控件树。
extern "C" fn on_row(e: u32) -> u32 {
    unsafe {
        let code = core::ptr::read_volatile((e as usize + 0x08) as *const u32);
        let ud = core::ptr::read_volatile((e as usize + 0x0C) as *const u32);
        if code == EV_CLICKED && (ud == PAGE_ABOUT || ud == PAGE_LIST) {
            WANT = ud;
        }
    }
    0
}

unsafe fn build() {
    if SCREEN == 0 {
        return;
    }
    if ROOT != 0 {
        let del: unsafe extern "C" fn(u32) = core::mem::transmute(OBJ_DELETE);
        del(ROOT);
        ROOT = 0;
    }
    let root = create(OBJ_CLASS, SCREEN);
    if root == 0 {
        return;
    }
    ROOT = root;
    OVERLAY_STATE[4] = root;
    OVERLAY_STATE[5] = CUR;
    prop(root, ST_W, 432);
    prop(root, ST_H, 514);
    prop(root, ST_X, 0);
    prop(root, ST_Y, 0);
    prop(root, ST_BG_COLOR, 0x000000);
    prop(root, ST_BG_OPA, 0xFF);

    if CUR == PAGE_ABOUT {
        label(root, b"Chaos\0".as_ptr(), 32, 28);
        row(root, b"Settings\0".as_ptr(), 120, PAGE_LIST);
        label(root, b"RW6 overlay\0".as_ptr(), 32, 440);
    } else {
        label(root, b"Settings\0".as_ptr(), 32, 28);
        row(root, b"About\0".as_ptr(), 120, PAGE_ABOUT);
    }
}

extern "C" fn tick(_t: u32) {
    unsafe {
        TICKS = TICKS.wrapping_add(1);
        OVERLAY_STATE[1] = TICKS;
    }
}

#[no_mangle]
pub extern "C" fn chaos_ctor() {
    unsafe { log(b"[overlay] ctor\0"); }
}

#[used]
#[link_section = ".init_array"]
static CTOR: extern "C" fn() = chaos_ctor;

#[no_mangle]
pub unsafe extern "C" fn module_main(_argc: i32, _argv: *const *const u8) -> i32 {
    // display 可能还没建好（NSH 提示符比 UI 早），一律等到拍上再取。
    let create_timer: unsafe extern "C" fn(u32, u32, u32) -> u32 = core::mem::transmute(TIMER_CREATE);
    OVERLAY_STATE[7] = create_timer(tick as usize as u32, 100, 0);
    0
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
