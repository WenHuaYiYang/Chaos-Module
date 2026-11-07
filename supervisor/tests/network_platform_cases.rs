use super::*;
use crate::{with,Mock,Read,TEST_LOCK};
fn setup()->std::sync::MutexGuard<'static,()> {
    let g=TEST_LOCK.lock().unwrap_or_else(|p|p.into_inner());
    with(|m|*m=Mock::default());LAST_MS.store(0,Ordering::Relaxed);REBOOT_PID.store(0,Ordering::Relaxed);REBOOT_RESULT.store(0,Ordering::Relaxed);g
}
#[test]
fn timespec_layout_clock_wrap_and_failed_reads_preserve_last_tick() {
    let _g=setup();assert_eq!(core::mem::size_of::<Timespec>(),16);assert_eq!(core::mem::align_of::<Timespec>(),8);
    assert_eq!(network_monotonic_ms(),123456);
    with(|m|m.clock=(4_294_967,999_999_999,0));assert_eq!(network_monotonic_ms(),703);
    for bad in [(0,0,-5),(-1,0,0),(1,-1,0),(1,1_000_000_000,0)] {with(|m|m.clock=bad);assert_eq!(network_monotonic_ms(),703);}
}
#[test]
fn uptime_is_seconds_not_wrapped_millisecond_conversion() {
    let _g=setup();with(|m|m.clock=(5_000_000,1,0));let mut sec=0;
    assert_eq!(unsafe{chaos_network_uptime_seconds(&mut sec)},0);assert_eq!(sec,5_000_000);
    assert_eq!(unsafe{chaos_network_uptime_seconds(core::ptr::null_mut())},EINVAL);
    with(|m|m.clock=(0,0,-5));assert_eq!(unsafe{chaos_network_uptime_seconds(&mut sec)},-5);
}
#[test]
fn readonly_meminfo_preserves_partial_io_and_closes_owned_fd_once() {
    let _g=setup();with(|m|m.reads.extend([Read::Bytes(b"ab".to_vec()),Read::Error(EINTR),Read::Bytes(b"cd".to_vec())]));
    let mut b=[0xa5;8];assert_eq!(unsafe{chaos_network_read_meminfo(b.as_mut_ptr(),8)},4);assert_eq!(&b[..4],b"abcd");assert_eq!(&b[4..],&[0xa5;4]);
    with(|m|{assert_eq!(m.paths,[b"/proc/meminfo".to_vec()]);assert_eq!(m.closes,[7]);});
}
#[test]
fn retry_budget_eagain_and_invalid_read_count_are_bounded() {
    let _g=setup();with(|m|m.reads.extend((0..100).map(|_|Read::Error(EINTR))));
    assert_eq!(unsafe{network_read_file(b"/proc/meminfo\0".as_ptr(),&mut [0;8])},EAGAIN);
    with(|m|{assert_eq!(m.read_calls,32);assert_eq!(m.closes,[7]);m.reads.clear();m.reads.push_back(Read::Error(EAGAIN));});
    assert_eq!(unsafe{network_read_file(b"x\0".as_ptr(),&mut [0;8])},EAGAIN);
    with(|m|m.reads.push_back(Read::InvalidCount(9)));
    assert_eq!(unsafe{network_read_file(b"x\0".as_ptr(),&mut [0;8])},EIO);
}
#[test]
fn failed_open_never_closes_unknown_fd_or_writes_output() {
    let _g=setup();with(|m|{m.open_rc=-1;m.errno=2;});let mut b=[0xa5;8];
    assert_eq!(unsafe{chaos_network_read_meminfo(b.as_mut_ptr(),8)},-2);assert_eq!(b,[0xa5;8]);
    with(|m|{assert!(m.closes.is_empty());assert_eq!(m.read_calls,0);});
}
#[test]
fn reboot_only_executes_fixed_command_on_separate_child_and_reaps_positive_pid() {
    let _g=setup();assert_eq!(chaos_network_reboot(),0);
    let entry=with(|m|{assert!(m.system_commands.is_empty());assert_eq!(m.spawns.len(),1);assert_eq!(m.spawns[0].0,b"chaos.web.reboot");m.spawns[0].1});
    assert_eq!(unsafe{entry(1,core::ptr::null_mut())},0);
    assert_eq!(chaos_network_reboot(),EBUSY);
    with(|m|{assert_eq!(m.system_commands,[b"reboot\0".to_vec()]);m.wait_results.push_back(-10);});
    network_monotonic_ms();assert_eq!(REBOOT_PID.load(Ordering::Acquire),300);
    with(|m|m.wait_results.push_back(300));network_monotonic_ms();assert_eq!(REBOOT_PID.load(Ordering::Acquire),0);
    assert_eq!(chaos_network_reboot(),0);with(|m|assert_eq!(m.spawns.len(),2));
}
#[test]
fn reboot_spawn_failure_is_reported_and_retryable() {
    let _g=setup();with(|m|m.spawn_rc=-12);assert_eq!(chaos_network_reboot(),-12);assert_eq!(REBOOT_PID.load(Ordering::Acquire),0);
    with(|m|{assert!(m.system_commands.is_empty());m.spawn_rc=300;});assert_eq!(chaos_network_reboot(),0);
}
