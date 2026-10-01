//! chaos_mod 最小示例模块: 只验"加载链 + exec 链"通不通
//! 被 supervisor INSTALL(槽) 经 0x0C1EE091 加载后:
//! 1. .init_array 自动跑 chaos_mod_ctor: 写标记 (验证加载链)
//! 2. Lua exec base+1 → module_main: 写标记 + 返回 (验证 exec 链)
//! 标记: 0x2001E000 = 0x4D4F4456 ("VMOD"), 0x2001E004 = 槽/状态
#![no_std]
#![allow(static_mut_refs)]

use core::ptr::write_volatile;

const MARK: usize = 0x2001_E000;

#[inline(always)]
unsafe fn wr32(a: usize, v: u32) {
    write_volatile(a as *mut u32, v)
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.module_main"]
pub extern "C" fn module_main() -> i32 {
    unsafe {
        wr32(MARK, 0x4D4F4456); // "VMOD"
        wr32(MARK + 4, 0x0000_0001);
    }
    0
}

// .init_array: 加载即执行 (modlib insmod 语义, 已验证)
#[used]
#[link_section = ".init_array"]
static MOD_CTOR: unsafe extern "C" fn() -> i32 = chaos_mod_ctor;

#[no_mangle]
#[inline(never)]
#[link_section = ".text.module_main"]
unsafe extern "C" fn chaos_mod_ctor() -> i32 {
    wr32(MARK, 0x4D4F4456); // "VMOD"
    wr32(MARK + 4, 0x0000_0002);
    0
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.module_main"]
pub extern "C" fn __keepers() -> i32 {
    let mut r = 0;
    r += module_main();
    unsafe {
        r += chaos_mod_ctor();
    }
    r
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}