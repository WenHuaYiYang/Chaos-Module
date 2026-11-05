use super::*;
use crate::page::Page;

unsafe fn setup() {
    crate::reset();
    MODEL = model::Model::new();
    OLDER_LINES = 0;
    LOCAL_ERROR = 0;
    forget_ui();
    PAGE.render(&PageCtx { content: 1, title: 2, rows: core::ptr::null_mut() }).unwrap();
    crate::with(|m| m.writes.clear());
}
unsafe fn output() -> String {
    crate::with(|m| {
        let mut lines: Vec<_> = OUTPUT_LINES.iter().map(|&id| String::from_utf8(m.objects[id as usize - 1].text.clone()).unwrap()).collect();
        while lines.last().is_some_and(|s| s.is_empty()) { lines.pop(); }
        lines.join("\n")
    })
}

#[test]
fn rendering_never_starts_or_sends_a_command() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe { setup(); crate::with(|m| { assert_eq!(m.starts, 0); assert!(m.sent.is_empty()); }); }
}

#[test]
fn command_and_newline_are_one_atomic_enqueue_and_empty_input_is_enter() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); assert!(submit_command(b"echo hi\0".as_ptr()));
        assert!(submit_command(b"\0".as_ptr()));
        crate::with(|m| { assert_eq!(m.sent, [b"echo hi\n".to_vec(), b"\n".to_vec()]); assert_eq!(m.starts, 1); });
    }
}

#[test]
fn failed_send_is_visible_and_retry_does_not_duplicate_partial_command() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| { m.state = crate::nsh::State::Running; m.send_error = Some(-11); });
        assert!(!submit_command(b"ls /data\0".as_ptr()));
        PAGE.tick();
        crate::with(|m| { assert!(m.sent.is_empty()); assert_eq!(m.attempts, [b"ls /data\n".to_vec()]); assert!(String::from_utf8_lossy(&m.objects[STATUS_LABEL as usize - 1].text).contains("-11")); m.send_error = None; });
        assert!(submit_command(b"ls /data\0".as_ptr()));
        crate::with(|m| assert_eq!(m.sent, [b"ls /data\n".to_vec()]));
    }
}

#[test]
fn limits_reject_overlong_nonascii_and_embedded_newline_without_start() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); let mut long = vec![b'a'; 256]; long.push(0);
        assert!(!submit_command(long.as_ptr()));
        assert!(!submit_command(b"a\nb\0".as_ptr()));
        assert!(!submit_command("中\0".as_ptr()));
        crate::with(|m| { assert_eq!(m.starts, 0); assert!(m.sent.is_empty()); });
        long[255] = 0; assert!(submit_command(long.as_ptr()));
        crate::with(|m| assert_eq!(m.sent[0].len(), 256));
    }
}

#[test]
fn restart_is_ignored_while_starting_or_running() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); PAGE.on_click(1); PAGE.on_click(1);
        crate::with(|m| { assert_eq!(m.starts, 1); m.state = crate::nsh::State::Running; });
        PAGE.on_click(1); crate::with(|m| { assert_eq!(m.starts, 1); m.state = crate::nsh::State::Exited; });
        PAGE.on_click(1); crate::with(|m| assert_eq!(m.starts, 2));
    }
}

#[test]
fn start_failure_is_reported_and_does_not_enqueue() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe { setup(); crate::with(|m| m.start_error = Some(-12)); assert!(!submit_command(b"ls\0".as_ptr())); PAGE.tick(); crate::with(|m| { assert!(m.sent.is_empty()); assert!(String::from_utf8_lossy(&m.objects[STATUS_LABEL as usize - 1].text).contains("-12")); }); }
}

#[test]
fn idle_ticks_do_not_reallocate_label_text() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe { setup(); for _ in 0..20 { PAGE.tick(); } crate::with(|m| assert!(m.writes.is_empty())); }
}

#[test]
fn latest_output_pagination_and_clear_are_live() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| { for n in 0..30 { m.incoming.extend(format!("line{n}\n").bytes()); } });
        PAGE.tick(); assert!(output().contains("line29"));
        PAGE.on_click(6); assert!(!output().contains("line29"));
        let old = output(); crate::with(|m| m.incoming.extend(b"fresh\n")); PAGE.tick(); assert_eq!(output(), old);
        PAGE.on_click(9); assert!(output().contains("fresh"));
        PAGE.on_click(2); assert_eq!(output(), "");
    }
}

#[test]
fn foreground_lifecycle_never_writes_stale_labels_or_stops_shell() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| m.state = crate::nsh::State::Running);
        PAGE.on_click(0); crate::with(|m| m.incoming.extend(b"keyboard output")); PAGE.tick();
        crate::with(|m| { assert!(m.writes.is_empty()); assert_eq!(m.state, crate::nsh::State::Running); });
        FG_PAGE = PAGE_PID; PAGE.tick(); assert_eq!(output(), "keyboard output");
        forget_ui(); crate::with(|m| { m.writes.clear(); m.live = false; m.incoming.extend(b"more"); });
        PAGE.tick(); crate::with(|m| assert!(m.writes.is_empty()));
    }
}

#[test]
fn controls_send_exact_bytes_and_return_does_not_write_after_navigation() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| m.state = crate::nsh::State::Running);
        PAGE.on_click(3); PAGE.on_click(4); PAGE.on_click(8);
        crate::with(|m| { assert_eq!(m.sent, [vec![3], b"exit\n".to_vec(), vec![10]]); m.writes.clear(); });
        PAGE.on_click(5); crate::with(|m| { assert!(m.writes.is_empty()); assert_eq!(m.state, crate::nsh::State::Running); });
    }
}

#[test]
fn each_tick_drain_is_bounded() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe { setup(); crate::with(|m| { m.incoming.extend(vec![b'x'; 5000]); m.receives = 0; }); PAGE.tick(); crate::with(|m| { assert_eq!(m.receives, 8); assert_eq!(m.incoming.len(), 5000 - 8 * 256); }); }
}

#[test]
fn six_natural_width_lines_and_horizontal_viewport_avoid_soft_wrapping() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| m.incoming.extend(b"WWWWWWWWWWWWWWWWWW\n2\n3\n4\n5\n6")); PAGE.tick();
        crate::with(|m| {
            let view = &m.objects[OUTPUT_BOX as usize - 1];
            assert_eq!(view.scroll_dir, fw_api::LV_DIR_HOR);
            assert_eq!(view.size.1, 6 * 42 + 8);
            assert_ne!(view.flags & fw_api::LV_OBJ_FLAG_SCROLLABLE, 0);
            assert_eq!(OUTPUT_LINES.len(), 6);
            for (i, &id) in OUTPUT_LINES.iter().enumerate() {
                let line = &m.objects[id as usize - 1];
                assert_eq!(line.size, (0, 0), "natural content width/height must not be constrained");
                assert_eq!(line.position, (0, (i * 42) as i32));
                assert!(!line.text.contains(&b'\n'));
            }
            assert_eq!(m.objects[OUTPUT_LINES[5] as usize - 1].text, b"6");
        });
    }
}

#[test]
fn line_pitch_follows_verified_font_height_with_safe_fallback() {
    assert_eq!(line_pitch(38), 42);
    assert_eq!(line_pitch(64), 68);
    assert_eq!(line_pitch(0), 42);
    assert_eq!(line_pitch(65535), 42);
}

#[test]
fn rebuild_repaints_new_labels_without_restarting_or_losing_history() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| { m.state = crate::nsh::State::Running; m.incoming.extend(b"still here"); }); PAGE.tick();
        forget_ui(); crate::with(|m| { m.objects.clear(); m.writes.clear(); });
        PAGE.render(&PageCtx { content: 1, title: 2, rows: core::ptr::null_mut() }).unwrap();
        assert_eq!(output(), "still here");
        crate::with(|m| { assert_eq!(m.state, crate::nsh::State::Running); assert_eq!(m.starts, 0); assert!(!m.writes.is_empty()); });
    }
}

#[test]
fn partial_widget_creation_failure_never_uses_zero_handles_on_tick() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        for fail_at in 1..=18 {
            crate::reset(); MODEL = model::Model::new(); forget_ui();
            crate::with(|m| m.fail_at = Some(fail_at));
            let _ = PAGE.render(&PageCtx { content: 1, title: 2, rows: core::ptr::null_mut() });
            PAGE.tick();
        }
    }
}

#[test]
fn covered_or_dark_terminal_drains_rx_without_updating_widgets() {
    let _g = crate::TEST_LOCK.lock().unwrap();
    unsafe {
        setup(); crate::with(|m| { m.top_page = (16, 0); m.incoming.extend(b"background"); });
        PAGE.tick(); PAGE.on_click(1);
        crate::with(|m| { assert!(m.writes.is_empty()); assert_eq!(m.starts, 0); assert!(m.incoming.is_empty()); m.top_page = (200, 31); m.screen_on = false; m.incoming.extend(b" output"); });
        PAGE.tick(); crate::with(|m| { assert!(m.writes.is_empty()); m.screen_on = true; });
        PAGE.tick(); assert_eq!(output(), "background output");
    }
}
