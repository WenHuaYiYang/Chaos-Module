//! 锚点执行探针（只做一件事）。
//!
//! 已验：clock_settime = 0x0C4C1FB4|1、clock_gettime = 0x0C4C205C|1（timespec 12 字节，
//! 64 位 time_t、nsec 在 +8）。滚轮：zoom_picker_create = 0x0C240411、
//! zoom_picker_set_mode = 0x0C240435。
//!
//! 结果通道：魔数 0x0C10C001 开头，宿主按魔数在模块 DATA 里定位。

#![no_std]

use core::panic::PanicInfo;

const CLOCK_SETTIME: usize = 0x0C4C_1FB5; // 入口 0x0C4C1FB4 | Thumb 位
const CLOCK_GETTIME: usize = 0x0C4C_205D; // 入口 0x0C4C205C | Thumb 位
const PICKER_CREATE: usize = 0x0C24_0411;
/// 滚轮 set_range 的候选入口（这一簇里的函数序言 + 读 +0x53C）。
const PICKER_SET_RANGE_CAND: usize = 0x0C24_061D;
const WANT_SEC: u32 = 1_700_000_000;

#[repr(C)]
struct Ts {
    sec: u64,
    nsec: i32,
}

#[no_mangle]
pub static mut CLK: [u32; 64] = [0x0C10_C001, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[used]
#[link_section = ".init_array"]
static CTOR: extern "C" fn() = ctor;

#[no_mangle]
pub extern "C" fn ctor() {}

unsafe fn out(i: usize, v: u32) {
    core::ptr::write_volatile((core::ptr::addr_of_mut!(CLK) as *mut u32).add(i), v);
}

unsafe fn rdu(a: usize) -> u32 {
    core::ptr::read_volatile(a as *const u32)
}

#[no_mangle]
pub unsafe extern "C" fn module_main(_argc: i32, _argv: *const *const u8) -> i32 {
    // 阳性对照：调已知可用的日志入口，确认"模块调固件函数"这条路通。
    #[cfg(feature = "logcall")]
    {
        let f: unsafe extern "C" fn(u32, *const u8) = core::mem::transmute(0x0C50_4A55usize);
        f(4, b"[pclk] logcall ok\0".as_ptr());
        out(1, 0x600D_600D);
        return 0;
    }

    // 对照：只证明这套模块形状能装能执行。
    #[cfg(feature = "noop")]
    {
        out(1, 0xA5A5_A5A5);
        return 0;
    }

    #[cfg(feature = "getonly")]
    {
        let get: unsafe extern "C" fn(u32, *mut Ts) -> i32 = core::mem::transmute(CLOCK_GETTIME);
        let mut t = Ts { sec: 0, nsec: 0 };
        let rg = get(0, &mut t);
        out(1, t.sec as u32);
        out(2, t.nsec as u32);
        out(3, rg as u32);
        return 0;
    }

    #[cfg(feature = "setonly")]
    {
        let set: unsafe extern "C" fn(u32, *const Ts) -> i32 = core::mem::transmute(CLOCK_SETTIME);
        let want = Ts { sec: WANT_SEC as u64, nsec: 0 };
        let rs = set(0, &want);
        out(4, rs as u32);
        return 0;
    }

    // 验滚轮 set_range 候选：建轮 -> 前后各 dump 一遍关键字段 -> 看哪几格变了。
    #[cfg(feature = "range")]
    {
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(0);
        out(1, p);
        if p != 0 {
            let fields = [0x530usize, 0x534, 0x538, 0x53C, 0x540, 0x544, 0x548, 0x94, 0xF8];
            for (i, f) in fields.iter().enumerate() {
                out(2 + i, rdu(p as usize + *f));
            }
            out(11, rdu(p as usize + 0x54C) & 0xFF);
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            for (i, f) in fields.iter().enumerate() {
                out(12 + i, rdu(p as usize + *f));
            }
            out(21, rdu(p as usize + 0x54C) & 0xFF);
        }
        return 0;
    }

    // 上屏验证：把滚轮挂到当前屏幕、设范围 0..23、开模式 1，宿主截图看效果。
    #[cfg(feature = "show")]
    {
        let disp = rdu(0x3C45_0030 + 0x14) as usize;
        let scr = rdu(disp + 0x2B0);
        out(1, scr);
        if scr != 0 {
            let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
            let p = create(scr);
            out(2, p);
            if p != 0 {
                let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                    core::mem::transmute(PICKER_SET_RANGE_CAND);
                setr(p, 0, 23, 1);
                let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
                setmode(p, 1);
                out(3, rdu(p as usize + 0x540));
                out(4, rdu(p as usize + 0x54C) & 0xFF);
            }
        }
        return 0;
    }

    // 设初值 + 取选中文字：create -> range(0..23) -> 候选set_value(p,5) -> 候选text(p)。
    #[cfg(feature = "val")]
    {
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(0);
        out(1, p);
        if p != 0 {
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            let setv: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0741usize);
            setv(p, 5);
            out(2, rdu(p as usize + 0x94));
            out(3, rdu(p as usize + 0xF8));
            let text: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(0x0C24_0871usize);
            let s = text(p);
            out(4, s);
            if s != 0 {
                out(5, rdu(s as usize) & 0xFFFF);
            }
        }
        return 0;
    }

    // 只验"取选中文字"：create -> range -> 候选text(p) -> 存指针与前 8 字节。
    #[cfg(feature = "textonly")]
    {
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(0);
        out(1, p);
        if p != 0 {
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            let text: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(0x0C24_0871usize);
            let s = text(p);
            out(2, s);
            if s != 0 {
                out(3, rdu(s as usize));
                out(4, rdu(s as usize + 4));
            }
            out(5, rdu(p as usize + 0x94));
            out(6, rdu(p as usize + 0xF8));
        }
        return 0;
    }

    // 直接写取值字段上屏：create(屏幕) -> range(0..23) -> mode1 -> 写 +0x9C=12 -> 截图。
    #[cfg(feature = "setfield")]
    {
        let disp = rdu(0x3C45_0030 + 0x14) as usize;
        let scr = rdu(disp + 0x2B0);
        out(1, scr);
        if scr != 0 {
            let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
            let p = create(scr);
            out(2, p);
            if p != 0 {
                let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                    core::mem::transmute(PICKER_SET_RANGE_CAND);
                setr(p, 0, 23, 1);
                let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
                setmode(p, 1);
                out(3, rdu(p as usize + 0x9C));
                out(4, rdu(p as usize + 0x100));
                core::ptr::write_volatile((p as usize + 0x9C) as *mut u32, 12);
                core::ptr::write_volatile((p as usize + 0x100) as *mut u32, 0);
            }
        }
        return 0;
    }

    // 上屏验 set_value 候选：create(屏幕) -> range(0..23) -> 候选(p,12) -> 截图。
    #[cfg(feature = "setval2")]
    {
        let disp = rdu(0x3C45_0030 + 0x14) as usize;
        let scr = rdu(disp + 0x2B0);
        out(1, scr);
        if scr != 0 {
            let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
            let p = create(scr);
            out(2, p);
            if p != 0 {
                let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                    core::mem::transmute(PICKER_SET_RANGE_CAND);
                setr(p, 0, 23, 1);
                let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
                setmode(p, 1);
                let setv: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0871usize);
                setv(p, 12);
                out(3, rdu(p as usize + 0x94));
                out(4, rdu(p as usize + 0xF8));
            }
        }
        return 0;
    }

    // 上屏验核心滚动函数：create(屏幕) -> range(0..23) -> mode 0 -> 核心(p,12,24) -> 截图。
    #[cfg(feature = "setval3")]
    {
        let disp = rdu(0x3C45_0030 + 0x14) as usize;
        let scr = rdu(disp + 0x2B0);
        out(1, scr);
        if scr != 0 {
            let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
            let p = create(scr);
            out(2, p);
            if p != 0 {
                let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                    core::mem::transmute(PICKER_SET_RANGE_CAND);
                setr(p, 0, 23, 1);
                let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
                setmode(p, 0);
                let core: unsafe extern "C" fn(u32, i32, i32) = core::mem::transmute(0x0C23_77A9usize);
                core(p, 12, 24);
                out(3, rdu(p as usize + 0x94));
                out(4, rdu(p as usize + 0xF8));
            }
        }
        return 0;
    }

    // 只读字段：create -> range(0..23) -> mode0 -> core(p,12,24) -> 读 +0xB8..+0xD0 与几个关键格。
    #[cfg(feature = "fields")]
    {
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(0);
        out(1, p);
        if p != 0 {
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
            setmode(p, 0);
            let core: unsafe extern "C" fn(u32, i32, i32) = core::mem::transmute(0x0C23_77A9usize);
            core(p, 12, 24);
            // 0xB8..0xD4，每 4 字节一格
            for k in 0..8usize {
                out(2 + k, rdu(p as usize + 0xB8 + k * 4));
            }
            out(10, rdu(p as usize + 0x94));
            out(11, rdu(p as usize + 0xF8));
            out(12, rdu(p as usize + 0x54C) & 0xFF);
            out(13, rdu(p as usize + 0x540));
        }
        return 0;
    }

    // 宽扫字段：设值 12 后把 +0x60..+0xDC 全读出来，找 12/10/2 落在哪。
    #[cfg(feature = "fields2")]
    {
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(0);
        out(1, p);
        if p != 0 {
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
            setmode(p, 0);
            let core: unsafe extern "C" fn(u32, i32, i32) = core::mem::transmute(0x0C23_77A9usize);
            core(p, 12, 24);
            for k in 0..30usize {
                out(2 + k, rdu(p as usize + 0x60 + k * 4));
            }
        }
        return 0;
    }

    // 差分定位选中索引：设 5 dump 一遍，设 17 再 dump，宿主找变了的格。
    #[cfg(feature = "diff")]
    {
        let disp = rdu(0x3C45_0030 + 0x14) as usize;
        let scr = rdu(disp + 0x2B0);
        let create: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(PICKER_CREATE);
        let p = create(scr);
        out(1, p);
        if p != 0 {
            let setr: unsafe extern "C" fn(u32, i32, i32, i32) =
                core::mem::transmute(PICKER_SET_RANGE_CAND);
            setr(p, 0, 23, 1);
            let setmode: unsafe extern "C" fn(u32, i32) = core::mem::transmute(0x0C24_0435usize);
            setmode(p, 0);
            let core: unsafe extern "C" fn(u32, i32, i32) = core::mem::transmute(0x0C23_77A9usize);
            core(p, 5, 24);
            for k in 0..15usize {
                out(2 + k, rdu(p as usize + 0xE0 + k * 4));
            }
            for k in 0..15usize {
                out(17 + k, rdu(p as usize + 0x530 + k * 4));
            }
            core(p, 17, 24);
            for k in 0..15usize {
                out(32 + k, rdu(p as usize + 0xE0 + k * 4));
            }
            for k in 0..15usize {
                out(47 + k, rdu(p as usize + 0x530 + k * 4));
            }
        }
        return 0;
    }

    // 默认：读 -> 设 -> 读回。
    #[cfg(not(any(
        feature = "noop",
        feature = "getonly",
        feature = "setonly",
        feature = "logcall",
        feature = "range"
    )))]
    {
        let get: unsafe extern "C" fn(u32, *mut Ts) -> i32 = core::mem::transmute(CLOCK_GETTIME);
        let set: unsafe extern "C" fn(u32, *const Ts) -> i32 = core::mem::transmute(CLOCK_SETTIME);
        let mut t = Ts { sec: 0, nsec: 0 };
        let rg = get(0, &mut t);
        out(1, t.sec as u32);
        out(2, t.nsec as u32);
        out(3, rg as u32);
        let want = Ts { sec: WANT_SEC as u64, nsec: 0 };
        let rs = set(0, &want);
        out(4, rs as u32);
        let mut t2 = Ts { sec: 0, nsec: 0 };
        let rg2 = get(0, &mut t2);
        out(5, t2.sec as u32);
        out(6, t2.nsec as u32);
        out(7, rg2 as u32);
    }
    0
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}
