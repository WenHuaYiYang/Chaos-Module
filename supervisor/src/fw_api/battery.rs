//! p67/.043 system battery percentage, shared with the stock UI.
//! The audited power_manager getter reads the live cached level at
//! 0x2013EF10 + 0xB0. No device scan, file IO or cross-core gauge request.
use core::mem::transmute;

pub(crate) unsafe fn battery_percent() -> Option<u8> {
    let get: unsafe extern "C" fn() -> u32 =
        transmute(super::fwaddr::POWER_MANAGER_BATTERY_LEVEL as usize);
    let value = get();
    if value <= 100 {
        Some(value as u8)
    } else {
        None
    }
}
