//! ChaosConnect-only INFO records, RAM only; all access is on the UI/owner loop.
use crate::connect_log_buffer::Records;
use crate::text::W;

static mut RECORDS: Records<{32 * 1024}> = Records::new();

fn pad(w: &mut W, value: i32) {
    let n = value.clamp(0, 99) as u8;
    unsafe { w.c(b'0' + n / 10).c(b'0' + n % 10); }
}

pub(crate) unsafe fn line(message: &str) { append(message.as_bytes()); }

unsafe fn append(message: &[u8]) {
    let mut bytes = [0u8; 1024];
    let mut w = W::new(bytes.as_mut_ptr(), bytes.len());
    w.c(b'[');
    if let Some(tm) = crate::time_set::now_tm() {
        pad(&mut w, tm[2]); w.c(b':'); pad(&mut w, tm[1]); w.c(b':'); pad(&mut w, tm[0]);
    } else { w.s(b"??:??:??"); }
    w.s(b"] INFO ");
    // Escape controls and invalid filename bytes; a filename cannot forge log rows.
    let mut rest = message;
    while !rest.is_empty() && w.len() + 5 < bytes.len() {
        let valid = match core::str::from_utf8(rest) { Ok(s) => s.len(), Err(e) => e.valid_up_to() };
        if valid == 0 {
            const HEX: &[u8] = b"0123456789abcdef";
            let b = rest[0]; w.s(b"\\x").c(HEX[(b >> 4) as usize]).c(HEX[(b & 15) as usize]);
            rest = &rest[1..]; continue;
        }
        for ch in core::str::from_utf8_unchecked(&rest[..valid]).chars() {
            if w.len() + 5 >= bytes.len() { break; }
            if ch.is_control() { w.c(b' '); }
            else { let mut tmp = [0; 4]; w.s(ch.encode_utf8(&mut tmp).as_bytes()); }
        }
        rest = &rest[valid..];
    }
    w.end();
    (&mut *core::ptr::addr_of_mut!(RECORDS)).push(&bytes[..w.len()]);
}

/// File engine callbacks run on the owner loop, never on Bluetooth RX/TX threads.
#[no_mangle]
pub unsafe extern "C" fn chaos_connect_log(message: *const u8) {
    if message.is_null() { return; }
    let mut n = 0;
    while n < 900 && *message.add(n) != 0 { n += 1; }
    append(core::slice::from_raw_parts(message, n));
}

pub(crate) unsafe fn clear() { (&mut *core::ptr::addr_of_mut!(RECORDS)).clear(); }
pub(crate) unsafe fn page(out: *mut u8, cap: usize, skip: usize, limit: usize) -> (usize, usize) {
    (&*core::ptr::addr_of!(RECORDS)).page(core::slice::from_raw_parts_mut(out, cap), skip, limit)
}
