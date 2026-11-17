//! Follow the stock p67/.043 return-to-watchface path used by the system's
//! return-home timer: reset the home panels, then request an app-level return.
//! Ordinary page_goto/page_back navigation does not perform this operation.
use core::mem::transmute;

pub(crate) unsafe fn floating_ball_home() -> bool {
    let request = core::ptr::read_volatile(super::fwaddr::APP_RETURN_HOME_SLOT as *const u32);
    if request & 1 == 0 || !crate::mem::plausible_ptr(request) {
        return false;
    }
    let prepare: unsafe extern "C" fn(u32) =
        transmute(super::fwaddr::HOME_PREPARE_WATCHFACE as usize);
    let home: unsafe extern "C" fn(u32) = transmute(request as usize);
    prepare(0);
    home(0);
    true
}
