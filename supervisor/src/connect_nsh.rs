//! CCN1 owner-only NSH API. Independent from the local and web PTYs.
//! This instance never abandons an unreaped child to start another shell.
const TASK_NAME:&[u8]=b"chaos.ccn.nsh\0";
const WORKER_NAME:&[u8]=b"chaos.ccn.nsh.worker\0";
const OVERFLOW_PREFIX:&[u8]=b"\n[ChaosConnect NSH: dropped ";
const ABANDON_ON_TIMEOUT:bool=false;
include!("remote_nsh_engine.rs");

#[no_mangle]
pub extern "C" fn chaos_connect_terminal_start() -> i32 { start().err().unwrap_or(0) }
/// One caller (PAN worker); successful commit returns 0, never a partial count.
#[no_mangle]
pub unsafe extern "C" fn chaos_connect_terminal_send(data: *const u8, length: u32) -> i32 {
    if length == 0 { return send(&[]).err().unwrap_or(0); }
    if data.is_null() { return api_error(EINVAL).err().unwrap(); }
    if length as usize > TX_CAP { return api_error(EMSGSIZE).err().unwrap(); }
    send(core::slice::from_raw_parts(data, length as usize)).err().unwrap_or(0)
}
/// Caller supplies a writable buffer. Bounded, nonblocking, no firmware calls.
#[no_mangle]
pub unsafe extern "C" fn chaos_connect_terminal_receive(out: *mut u8, capacity: u32) -> u32 {
    if capacity == 0 { return 0; }
    if out.is_null() { let _ = api_error(EINVAL); return 0; }
    receive(core::slice::from_raw_parts_mut(out, (capacity as usize).min(RX_CAP))) as u32
}
#[no_mangle]
pub extern "C" fn chaos_connect_terminal_stop() { stop(); }
#[no_mangle]
pub extern "C" fn chaos_connect_terminal_state() -> i32 { state() as i32 }
#[no_mangle]
pub extern "C" fn chaos_connect_terminal_error() -> i32 { error() }

#[cfg(test)]
use self::{chaos_connect_terminal_start as chaos_web_terminal_start,chaos_connect_terminal_send as chaos_web_terminal_send,
    chaos_connect_terminal_receive as chaos_web_terminal_receive,chaos_connect_terminal_stop as chaos_web_terminal_stop,
    chaos_connect_terminal_state as chaos_web_terminal_state,chaos_connect_terminal_error as chaos_web_terminal_error};
#[cfg(test)]
#[path="../tests/web_nsh_cases.rs"] mod tests;
