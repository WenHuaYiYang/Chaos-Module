#![no_std]
#![no_main]
#![allow(dead_code)]
#[path = "../src/fw_api/battery.rs"]
mod battery;
#[path = "../src/mem.rs"]
mod mem;
#[path = "../src/fw_api/navigation.rs"]
mod navigation;
mod fwaddr {
    pub const POWER_MANAGER_BATTERY_LEVEL: u32 = 0x0CA6_E6A9;
    pub const HOME_PREPARE_WATCHFACE: u32 = 0x0C50_FD7D;
    pub const APP_RETURN_HOME_SLOT: u32 = 0x200E_B6F0;
}
#[no_mangle]
pub unsafe extern "C" fn test_battery() -> i32 {
    battery::battery_percent().map_or(-1, |v| v as i32)
}
#[no_mangle]
pub unsafe extern "C" fn test_home() -> u32 {
    navigation::floating_ball_home() as u32
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
