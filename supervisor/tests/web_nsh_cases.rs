use super::*;
use crate::{with, Mock, Read, TEST_LOCK};

fn reset() {
    with(|m| *m = Mock::default());
    STATE.store(State::Idle as u8, Ordering::Relaxed);
    API_ERROR.store(0, Ordering::Relaxed); SESSION_ERROR.store(0, Ordering::Relaxed);
    WORKER_STARTED.store(false, Ordering::Relaxed); REQUEST.store(false, Ordering::Relaxed);
    STOP_REQUEST.store(false, Ordering::Relaxed);
    CHILD_DONE.store(false, Ordering::Relaxed); CHILD_EXIT.store(0, Ordering::Relaxed);
    CHILD_CONSOLE.store(0, Ordering::Relaxed); ORIGINAL_EXIT.store(0, Ordering::Relaxed);
    DROPPED.store(0, Ordering::Relaxed); NOTICE_COUNT.store(0, Ordering::Relaxed); NOTICE_OFFSET.store(0, Ordering::Relaxed);
    unsafe { RX.reset_quiescent(); TX.reset_quiescent(); }
}
fn setup() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner()); reset(); guard
}
fn boot() -> Option<Session> {
    assert_eq!(start(), Ok(()));
    let mut session = None; unsafe { worker_step(&mut session); }
    assert_eq!(state(), State::Running); session
}
fn step(session: &mut Option<Session>) { unsafe { worker_step(session); } }
fn invoke_shell() -> i32 {
    let entry = with(|m| m.spawns.iter().find(|s| !s.2).unwrap().1);
    unsafe { entry(1, core::ptr::null_mut()) }
}
fn drain() -> Vec<u8> {
    let mut text = Vec::new(); let mut chunk = [0u8; 7];
    for _ in 0..4096 { let n = receive(&mut chunk); if n == 0 { return text; } text.extend_from_slice(&chunk[..n]); }
    panic!("unbounded receive");
}

#[test]
fn worker_spawn_failure_is_retryable_and_ui_never_waits() {
    let _lock = setup();
    with(|m| m.worker_result = -12);
    assert_eq!(start(), Err(-12)); assert_eq!(state(), State::Failed); assert_eq!(error(), -12);
    assert!(!REQUEST.load(Ordering::Acquire)); assert!(!WORKER_STARTED.load(Ordering::Acquire));
    assert_eq!(receive(&mut [0; 5]), 0);
    with(|m| { assert!(m.waits.is_empty()); m.worker_result = 100; });
    assert_eq!(start(), Ok(())); assert_eq!(state(), State::Starting);
    with(|m| { assert_eq!(m.spawns.len(), 2); assert!(m.waits.is_empty()); });
}

#[test]
fn every_startup_failure_closes_only_owned_fds_and_destroys_actions_once() {
    let _lock = setup();
    for phase in 0..10 {
        reset();
        with(|m| match phase {
            0 => m.master_result = -19, 1 => m.master_result = 2, 2 => m.prepare_result = -22,
            3 => m.slave_result = -12, 4 => m.init_result = -12,
            5..=8 => m.add_fail = Some(phase - 5), 9 => m.child_result = -11, _ => unreachable!(),
        });
        assert_eq!(start(), Ok(())); let mut session = None; step(&mut session);
        assert!(session.is_none()); assert_eq!(state(), State::Failed); assert!(error() < 0);
        with(|m| {
            let expected = match phase { 0 => vec![], 1 => vec![2], 2 | 3 => vec![5], _ => vec![6, 5] };
            assert_eq!(m.closed, expected, "phase {phase}");
            assert_eq!(m.destroys, usize::from(phase >= 5), "phase {phase}");
            assert!(m.waits.is_empty());
        });
    }
}

#[test]
fn dup2_is_child_only_and_active_start_is_idempotent() {
    let _lock = setup();
    assert_eq!(start(), Ok(())); assert_eq!(send(b"abc"), Ok(())); assert_eq!(start(), Ok(()));
    let mut session = None; step(&mut session); assert_eq!(start(), Ok(()));
    with(|m| {
        assert_eq!(m.actions, [(6, 0), (6, 1), (6, 2), (6, 5)]);
        assert_eq!(m.events[..2], ["actions_destroy", "close:6"]);
        assert_eq!(m.spawns.len(), 2); assert_eq!(m.closed, [6]);
        assert_eq!(m.spawns[0].0, b"chaos.web.nsh.worker"); assert!(m.spawns[0].2);
        assert_eq!(m.spawns[1].0, b"chaos.web.nsh"); assert!(!m.spawns[1].2);
    });
    step(&mut session); with(|m| assert_eq!(m.written, b"abc"));
}

#[test]
fn send_is_full_block_and_rejects_nul_eot_but_accepts_ctrl_c() {
    let _lock = setup(); let mut session = boot();
    assert_eq!(send(b"abc\0suffix"), Err(EINVAL));
    assert_eq!(send(b"abc\x04suffix"), Err(ENOTSUP));
    assert_eq!(send(&[b'x'; TX_CAP - 1]), Ok(()));
    assert_eq!(send(b"yz"), Err(EBUSY)); // not even the first byte can be committed
    assert_eq!(send(&[3]), Ok(())); assert_eq!(send(b"z"), Err(EBUSY));
    for _ in 0..10 { step(&mut session); }
    with(|m| { assert_eq!(m.written.len(), TX_CAP); assert!(m.written[..TX_CAP - 1].iter().all(|&b| b == b'x')); assert_eq!(m.written[TX_CAP - 1], 3); });
}

#[test]
fn tx_partial_eintr_eagain_zero_preserve_suffix_and_fifo_order() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.write_results.extend([2, EINTR, EAGAIN, 0, 1]));
    assert_eq!(send(b"abcdef"), Ok(())); step(&mut session);
    assert_eq!(send(b"XYZ"), Ok(()));
    for _ in 0..8 { step(&mut session); }
    with(|m| {
        assert_eq!(m.written, b"abcdefXYZ");
        assert_eq!(m.writes, [b"abcdef".to_vec(), b"cdef".to_vec(), b"cdef".to_vec(), b"cdef".to_vec(), b"cdef".to_vec(), b"def".to_vec(), b"XYZ".to_vec()]);
    });
    assert_eq!(error(), 0);
}

#[test]
fn rx_overflow_keeps_pumping_and_reports_exact_dropped_count_in_short_receives() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.reads.push_back(Read::Bytes(vec![b'a'; 10000])));
    for _ in 0..6 { step(&mut session); }
    with(|m| { assert!(m.reads.is_empty()); assert_eq!(m.waits.len(), 6); });
    assert_eq!(DROPPED.load(Ordering::Acquire), 1808);
    let output = drain(); assert!(output[..RX_CAP].iter().all(|&b| b == b'a'));
    assert_eq!(&output[RX_CAP..], b"\n[Web NSH: dropped 1808 output bytes]\n");
    assert_eq!(receive(&mut []), 0); assert_eq!(DROPPED.load(Ordering::Acquire), 0);
}

#[test]
fn callback_does_not_publish_final_or_double_free_before_positive_reap() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.callback_in_run = true); assert_eq!(invoke_shell(), 0);
    assert!(CHILD_DONE.load(Ordering::Acquire)); assert_eq!(CHILD_CONSOLE.load(Ordering::Acquire), 0);
    step(&mut session); assert_eq!(state(), State::Running); assert!(session.is_some());
    with(|m| { assert_eq!(m.native_exits.len(), 1); assert!(m.freed.is_empty()); assert_eq!(m.closed, [6]); m.wait_results.push_back(200); });
    step(&mut session); assert_eq!(state(), State::Exited); assert!(session.is_none());
    with(|m| { assert_eq!(m.native_exits.len(), 1); assert!(m.freed.is_empty()); assert_eq!(m.closed, [6, 5]); });
}

#[test]
fn normal_session_return_delegates_to_its_instance_exit_once() {
    let _lock = setup(); let mut session = boot();
    assert_eq!(invoke_shell(), 0); with(|m| { assert_eq!(m.native_exits, [(0x1234, 0x12345000, 0)]); m.wait_results.push_back(200); });
    step(&mut session); assert_eq!(state(), State::Exited);
}

#[test]
fn direct_task_death_without_callback_reaps_and_frees_leftover_only_after_wait() {
    let _lock = setup(); let mut session = boot();
    CHILD_CONSOLE.store(0x12345000, Ordering::Release); ORIGINAL_EXIT.store(0x1234, Ordering::Release);
    step(&mut session); with(|m| assert!(m.freed.is_empty()));
    with(|m| { m.wait_results.push_back(200); m.reads.push_back(Read::Bytes(b"tail\n".to_vec())); });
    step(&mut session); assert_eq!(state(), State::Exited); assert!(!CHILD_DONE.load(Ordering::Acquire));
    assert_eq!(drain(), b"tail\n");
    with(|m| {
        assert_eq!(m.freed, [0x12345000]); assert!(m.native_exits.is_empty()); assert_eq!(m.closed, [6, 5]);
        let waited = m.events.iter().position(|e| e == "wait:200:200").unwrap();
        let freed = m.events.iter().position(|e| e.starts_with("free:")).unwrap(); assert!(waited < freed);
    });
}

#[test]
fn constructor_failure_and_missing_callback_are_reaped_before_failed_state() {
    let _lock = setup();
    for missing_callback in [false, true] {
        reset(); let mut session = boot();
        with(|m| if missing_callback { m.original_exit = 0; } else { m.console = 0; });
        let expected = if missing_callback { EIO } else { ENOMEM };
        assert_eq!(invoke_shell(), expected); step(&mut session); assert_eq!(state(), State::Running);
        with(|m| { assert!(m.freed.is_empty()); m.wait_results.push_back(200); });
        step(&mut session); assert_eq!(state(), State::Failed); assert_eq!(error(), expected);
        with(|m| assert_eq!(m.freed.len(), usize::from(missing_callback)));
    }
}

#[test]
fn fatal_wait_or_io_error_cannot_start_a_second_live_child() {
    let _lock = setup();
    for failure in [0, 1, 2] {
        reset(); let mut session = boot(); assert_eq!(send(b"pending"), Ok(()));
        with(|m| match failure {
            0 => m.wait_results.push_back(-10), 1 => m.write_results.push_back(-32),
            _ => m.reads.push_back(Read::Error(-5)),
        });
        step(&mut session); assert_eq!(state(), State::Running); assert!(error() < 0);
        assert_eq!(start(), Ok(())); assert!(send(b"not accepted").is_err());
        with(|m| { assert_eq!(m.spawns.len(), 2); assert_eq!(m.closed, [6]); m.wait_results.push_back(200); });
        step(&mut session); assert_eq!(state(), State::Failed); assert!(session.is_none());
    }
}

#[test]
fn wait_eintr_is_retryable_and_direct_exit_does_not_need_done_flag() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.wait_results.extend([EINTR, 0, 200]));
    step(&mut session); assert_eq!(error(), 0); step(&mut session); assert_eq!(state(), State::Running);
    step(&mut session); assert_eq!(state(), State::Exited); assert!(!CHILD_DONE.load(Ordering::Acquire));
}

#[test]
fn direct_nonzero_exit_status_is_reported_without_inventing_errno() {
    let _lock = setup(); let mut session = boot();
    with(|m| { m.wait_status = 7 << 8; m.wait_results.push_back(200); });
    step(&mut session); assert_eq!(state(), State::Exited); assert_eq!(error(), 7);
}

#[test]
fn read_eio_racing_normal_exit_is_not_a_spurious_session_failure() {
    let _lock = setup();
    for callback_done in [false, true] {
        reset(); let mut session = boot();
        if callback_done { assert_eq!(invoke_shell(), 0); }
        with(|m| { m.wait_results.extend([0, if callback_done { 0 } else { 200 }]); m.reads.push_back(Read::Error(EIO)); });
        step(&mut session); assert_eq!(error(), 0);
        if callback_done {
            assert_eq!(state(), State::Running); with(|m| m.wait_results.push_back(200)); step(&mut session);
        }
        assert_eq!(state(), State::Exited); assert_eq!(error(), 0);
    }
}

#[test]
fn confirmed_exit_tail_drain_is_bounded_to_64_chunks() {
    let _lock = setup(); let mut session = boot();
    with(|m| { m.wait_results.push_back(200); m.reads.push_back(Read::Bytes(vec![b'z'; 65 * IO_CHUNK])); });
    step(&mut session); assert_eq!(state(), State::Exited);
    with(|m| { assert_eq!(m.read_calls, 64); assert!(!m.reads.is_empty()); assert_eq!(m.closed, [6, 5]); });
    assert_eq!(DROPPED.load(Ordering::Acquire), 8192);
}

#[test]
fn restart_reuses_resident_worker_and_resets_only_after_prior_final_state() {
    let _lock = setup(); let mut session = boot();
    assert_eq!(send(b"old pending"), Ok(()));
    with(|m| { m.wait_results.push_back(200); m.reads.push_back(Read::Bytes(b"old output".to_vec())); });
    step(&mut session); assert_eq!(state(), State::Exited);
    assert_eq!(start(), Ok(())); assert_eq!(state(), State::Starting);
    assert!(drain().is_empty()); assert_eq!(send(b"new"), Ok(()));
    step(&mut session); step(&mut session);
    with(|m| { assert_eq!(m.spawns.iter().filter(|s| s.2).count(), 1); assert_eq!(m.spawns.len(), 3); assert_eq!(m.written, b"new"); });
}

#[test]
fn queue_two_threads_preserve_order_through_many_wraps() {
    let ring: Spsc<64> = Spsc::new();
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 0..50000u32 {
                let bytes = i.to_le_bytes();
                while !unsafe { ring.push_all(&bytes) } { std::thread::yield_now(); }
            }
        });
        let mut data = Vec::new(); let mut bytes = [0u8; 7];
        while data.len() < 200000 {
            let n = unsafe { ring.pop(&mut bytes) };
            if n == 0 { std::thread::yield_now(); } else { data.extend_from_slice(&bytes[..n]); }
        }
        for (i, value) in data.chunks_exact(4).enumerate() { assert_eq!(u32::from_le_bytes(value.try_into().unwrap()), i as u32); }
    });
}

#[test]
fn stop_before_worker_open_cancels_without_creating_child_or_touching_fds() {
    let _lock = setup();
    assert_eq!(chaos_web_terminal_start(), 0);
    assert_eq!(send(b"not executed\n"), Ok(()));
    chaos_web_terminal_stop();
    assert_eq!(chaos_web_terminal_state(), State::Stopping as i32);
    assert_eq!(chaos_web_terminal_start(), EBUSY);
    assert_eq!(send(b"late\n"), Err(EBUSY));
    let mut session = None; step(&mut session);
    assert!(session.is_none()); assert_eq!(state(), State::Exited);
    with(|m| { assert_eq!(m.spawns.len(), 1); assert!(m.actions.is_empty());
        assert!(m.closed.is_empty()); assert!(m.writes.is_empty()); });
    assert_eq!(start(), Ok(())); step(&mut session); step(&mut session);
    with(|m| { assert_eq!(m.spawns.len(), 2); assert!(m.written.is_empty()); });
}

#[test]
fn stop_during_open_or_spawn_never_gets_overwritten_by_running() {
    let _lock = setup();
    for during_prepare in [true, false] {
        reset();
        with(|m| { m.stop_in_prepare = during_prepare; m.stop_in_child_spawn = !during_prepare; });
        assert_eq!(start(), Ok(())); let mut session = None; step(&mut session);
        assert!(session.is_some()); assert_eq!(state(), State::Stopping);
        assert_eq!(start(), Err(EBUSY));
        step(&mut session);
        with(|m| { assert_eq!(m.written, STOP_SEQUENCE); assert_eq!(m.closed, [6]);
            assert!(m.freed.is_empty()); m.wait_results.push_back(200); });
        step(&mut session); assert!(session.is_none()); assert_eq!(state(), State::Exited);
        with(|m| assert_eq!(m.closed, [6, 5]));
    }
}

#[test]
fn stop_discards_pending_tx_suffix_and_never_executes_unfinished_line() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.write_results.push_back(2));
    assert_eq!(send(b"unsafe partial command"), Ok(())); step(&mut session);
    assert_eq!(send(b"queued second command\n"), Ok(()));
    stop(); stop(); assert_eq!(state(), State::Stopping);
    step(&mut session); step(&mut session);
    with(|m| {
        assert_eq!(&m.written[..2], b"un");
        assert_eq!(&m.written[2..], STOP_SEQUENCE);
        assert_eq!(m.writes.len(), 2);
    });
    assert_eq!(STOP_SEQUENCE[0], 3);
    assert!(STOP_SEQUENCE[1..257].iter().all(|&b| b == 8));
    assert_eq!(&STOP_SEQUENCE[257..], b"exit\n");
    assert_eq!(STOP_SEQUENCE.iter().filter(|&&b| b == b'\n').count(), 1);
}

#[test]
fn stop_sequence_short_writes_eagain_and_eintr_are_retryable_until_reap() {
    let _lock = setup(); let mut session = boot();
    with(|m| m.write_results.extend([1, EAGAIN, EINTR, 0, 20]));
    stop();
    for _ in 0..8 { step(&mut session); }
    with(|m| { assert_eq!(m.written, STOP_SEQUENCE); assert_eq!(m.closed, [6]);
        assert!(m.freed.is_empty()); assert_eq!(m.spawns.len(), 2); });
    assert_eq!(state(), State::Stopping); assert_eq!(error(), 0);
    assert_eq!(start(), Err(EBUSY));
    with(|m| m.wait_results.push_back(200)); step(&mut session);
    assert_eq!(state(), State::Exited); assert!(session.is_none());
    assert_eq!(start(), Ok(()));
}

#[test]
fn stop_eio_or_epipe_keeps_ownership_and_does_not_invent_failure() {
    let _lock = setup();
    for result in [EIO, EPIPE] {
        reset(); let mut session = boot();
        CHILD_CONSOLE.store(0x12345000, Ordering::Release);
        with(|m| { m.write_results.push_back(result); m.reads.push_back(Read::Error(EIO)); });
        stop(); step(&mut session); step(&mut session);
        assert_eq!(state(), State::Stopping); assert_eq!(error(), 0);
        with(|m| { assert_eq!(m.closed, [6]); assert!(m.freed.is_empty());
            assert_eq!(m.writes.len(), 1); m.wait_results.push_back(200); });
        step(&mut session); assert_eq!(state(), State::Exited);
        with(|m| assert_eq!(m.freed, [0x12345000]));
    }
}

#[test]
fn web_input_preserves_lf_and_partial_writes_without_cr_conversion() {
    let _lock = setup(); let mut session = boot();
    let command = b"help\n";
    with(|m| m.write_results.extend([4, EAGAIN, 1]));
    unsafe { assert_eq!(chaos_web_terminal_send(command.as_ptr(), command.len() as u32), 0); }
    for _ in 0..3 { step(&mut session); }
    with(|m| { assert_eq!(m.written, command); assert_eq!(m.writes[1], b"\n"); });
}

#[test]
fn exported_ffi_rejects_invalid_pointers_and_oversize_without_dereference() {
    let _lock = setup();
    unsafe {
        assert_eq!(chaos_web_terminal_send(core::ptr::null(), 0), 0);
        assert_eq!(chaos_web_terminal_send(core::ptr::null(), 1), EINVAL);
        assert_eq!(chaos_web_terminal_send(1usize as *const u8, (TX_CAP + 1) as u32), EMSGSIZE);
        assert_eq!(chaos_web_terminal_receive(core::ptr::null_mut(), 0), 0);
        assert_eq!(chaos_web_terminal_receive(core::ptr::null_mut(), 1), 0);
    }
    assert_eq!(chaos_web_terminal_error(), EINVAL);
}

