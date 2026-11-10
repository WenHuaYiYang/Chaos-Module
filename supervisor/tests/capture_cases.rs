use super::*;
use crate::{world, World};

fn reset() {
    world(|w| { assert!(w.allocations.is_empty()); assert!(w.handles.is_empty()); *w = World::default(); });
    STATE.store(0, Ordering::Relaxed); SHOT.store(0, Ordering::Relaxed);
    START_REQUEST.store(false, Ordering::Relaxed);
    ERROR.store(0, Ordering::Relaxed); SHOT_ERROR.store(0, Ordering::Relaxed);
    WORKER.store(false, Ordering::Relaxed); FRAMES.store(0, Ordering::Relaxed);
    REVISION.store(0, Ordering::Relaxed); SEQUENCE.store(0, Ordering::Relaxed);
    FPS.store(0, Ordering::Relaxed); CONSTRAINED.store(false, Ordering::Relaxed);
    LIMITED.store(false, Ordering::Relaxed);
    REQUEST_SETTINGS.store(0, Ordering::Relaxed); SHOT_STORAGE.store(0, Ordering::Relaxed);
    crate::capture_settings::reset_for_test(); unsafe { crate::capture_page::reset_for_test(); }
}
fn check_clean() { world(|w| { assert!(w.handles.is_empty()); assert!(w.allocations.is_empty()); assert_eq!(w.readers, 0); }); }
fn finish(worker: &mut Worker) { toggle_recording(); unsafe { worker.step(); } assert_eq!(state(), State::Idle); }
fn file_count(extension: &str) -> usize { world(|w| w.files.keys().filter(|s| s.ends_with(extension)).count()) }
fn save_fixture(name: &str) { world(|w| {
    if let Ok(dir) = std::env::var("CAPTURE_TEST_OUTPUT") {
        let data = w.files.iter().find(|(name, _)| name.ends_with(".gif") || name.ends_with(".apng") || name.ends_with(".bmp")).unwrap().1;
        std::fs::write(std::path::Path::new(&dir).join(name), data).unwrap();
    }
}); }

#[test]
fn apng_sixty_vsync_frames_have_exact_one_second_duration() {
    reset(); world(|w| w.frame_mode = 2);
    crate::capture_settings::set(Settings { format: VideoFormat::Apng, recording: Storage::Media, screenshot: Storage::OfflineLog }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for i in 1..60 { world(|w| w.now = 1000 + (i * 1000 + 30) / 60); unsafe { worker.step(); } }
    world(|w| w.now = 2000); finish(&mut worker);
    assert_eq!(frames(), 60); assert_eq!(file_count(".apng"), 1); assert_eq!(error(), 0); check_clean();
    world(|w| {
        assert!(w.files.keys().any(|p| p.starts_with("/data/chaos/media/") && p.ends_with(".apng")));
        assert_eq!(w.reader_opens, 1); assert_eq!(w.seeks, 2); // one final acTL patch only
    }); save_fixture("sixty.apng");
}

#[test]
fn metadata_switches_keep_complete_apng_snapshots() {
    reset(); world(|w| { w.frame_mode = 2; w.torn = true; });
    capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for i in 1..60 {
        world(|w| { w.now = 1000 + (i * 1000 + 30) / 60; w.torn = true; }); unsafe { worker.step(); }
    }
    assert_eq!(frames(), 60); assert_eq!(state(), State::Recording); assert_eq!(error(), 0);
    world(|w| w.now = 2000); finish(&mut worker); check_clean(); save_fixture("flipping.apng");
}

#[test]
fn timer_sampling_skips_stale_deadlines_and_survives_clock_wrap() {
    reset(); world(|w| { w.frame_mode = 2; w.now = u32::MAX - 100; w.vsync = false; });
    capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for _ in 0..3 {
        world(|w| w.now = w.now.wrapping_add(5000)); unsafe { worker.step(); }
        let before = world(|w| w.captures);
        for _ in 0..100 { unsafe { worker.step(); } }
        assert_eq!(world(|w| w.captures), before, "same-time polling must not duplicate captures");
    }
    assert_eq!(frames(), 4);
    world(|w| w.now = w.now.wrapping_add(17)); unsafe { worker.step(); }
    assert_eq!(frames(), 5); finish(&mut worker); assert_eq!(error(), 0); check_clean();
}

#[test]
fn gif_settings_sample_fifteen_frames_from_sixty_display_events() {
    reset(); world(|w| w.frame_mode = 2);
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for i in 1..60 { world(|w| w.now = 1000 + (i * 1000 + 30) / 60); unsafe { worker.step(); } }
    world(|w| w.now = 2000); finish(&mut worker);
    assert_eq!(frames(), 15); check_clean(); save_fixture("fifteen.gif");
}

#[test]
fn apng_noise_and_long_static_holds_decode_with_streaming_chunks() {
    for moving in [true, false] {
        reset(); if !moving { world(|w| w.frame_mode = 1); }
        crate::capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
        let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
        if moving { for _ in 0..2 { world(|w| w.now += 17); unsafe { worker.step(); } } }
        else { world(|w| w.now += 299_999); }
        finish(&mut worker); check_clean();
        save_fixture(if moving { "noise.apng" } else { "static.apng" });
    }
}

#[test]
fn settings_survive_reload_and_failed_saves_preserve_the_old_config() {
    reset(); assert_eq!(capture_settings::get(), Settings::DEFAULT);
    let chosen = Settings { format: VideoFormat::Apng, recording: Storage::Media, screenshot: Storage::OfflineLog };
    capture_settings::set(chosen).unwrap(); capture_settings::reset_for_test(); assert_eq!(capture_settings::get(), chosen);
    let original = world(|w| w.files["/data/chaos/capture.cfg"].clone());
    world(|w| w.budget = 0); assert_eq!(capture_settings::set(Settings::DEFAULT), Err(-28));
    assert_eq!(capture_settings::get(), chosen);
    world(|w| { assert_eq!(w.files["/data/chaos/capture.cfg"], original); assert!(!w.files.contains_key("/data/chaos/capture.cfg.tmp")); w.budget = usize::MAX; w.rename_error = -13; });
    assert_eq!(capture_settings::set(Settings::DEFAULT), Err(-13));
    capture_settings::reset_for_test(); assert_eq!(capture_settings::get(), chosen); check_clean();
}

#[test]
fn settings_reject_invalid_bytes_and_overlong_configs() {
    for bad in [b"CAP2\x09\0\0Z".as_slice(), b"CAP2\0\0\0X", b"CAP2\0\0\0Ztrailing", b"bad"] {
        reset(); world(|w| { w.files.insert("/data/chaos/capture.cfg".into(), bad.to_vec()); });
        assert_eq!(capture_settings::get(), Settings::DEFAULT); check_clean();
    }
}

#[test]
fn recording_and_armed_screenshot_latch_their_own_settings() {
    reset(); world(|w| w.frame_mode = 2);
    capture_settings::set(Settings { screenshot: Storage::Media, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); } toggle_shot();
    capture_settings::set(Settings { format: VideoFormat::Apng, recording: Storage::Media, screenshot: Storage::OfflineLog }).unwrap();
    world(|w| w.on = false); unsafe { worker.step(); } finish(&mut worker); check_clean();
    world(|w| {
        assert!(w.files.keys().any(|p| p.starts_with("/data/offlinelog/") && p.ends_with(".gif")));
        assert!(w.files.keys().any(|p| p.starts_with("/data/chaos/media/") && p.ends_with(".bmp")));
    });
    world(|w| { w.on = true; w.now += 100; }); toggle_recording(); unsafe { worker.step(); } finish(&mut worker);
    assert_eq!(file_count(".apng"), 1); check_clean();
}

#[test]
fn management_uses_both_roots_and_never_confuses_equal_filenames() {
    use crate::{capture_page::*, page::Page, ui};
    reset(); ui(|u| *u = Default::default());
    let name = "chaos-rec-00000001-00000000.apng";
    world(|w| { for root in ["/data/offlinelog", "/data/chaos/media"] { w.files.insert(format!("{root}/{name}"), vec![1]); } });
    let ctx = crate::page::PageCtx { content: 1, title: 2, rows: std::ptr::null_mut() };
    PAGE_RECORDINGS.render(&ctx).unwrap();
    ui(|u| assert_eq!(u.rows.iter().filter(|r| r.0 < 8).count(), 2));
    PAGE_RECORDINGS.on_click(0); PAGE_RECORDINGS.on_click(1); PAGE_RECORDINGS.tick(); assert_eq!(file_count(".apng"), 2);
    PAGE_RECORDINGS.on_click(1); PAGE_RECORDINGS.tick(); assert_eq!(file_count(".apng"), 1);
    world(|w| { assert!(w.files.contains_key(&format!("/data/offlinelog/{name}"))); assert!(!w.files.contains_key(&format!("/data/chaos/media/{name}"))); });
    check_clean();
}

#[test]
fn settings_pages_expose_both_formats_and_independent_storage_choices() {
    use crate::{capture_page::*, page::Page, ui};
    reset(); ui(|u| *u = Default::default());
    let ctx = crate::page::PageCtx { content: 1, title: 2, rows: std::ptr::null_mut() };
    PAGE_RECORDSET.render(&ctx).unwrap();
    ui(|u| {
        assert_eq!(u.rows[0].1, "GIF · 15 fps"); assert_eq!(u.rows[1].1, "APNG · 60 fps");
        assert!(u.selected[&0] && u.selected[&3]);
        assert_eq!(u.events, [(101, 1000, 7), (102, 1001, 7), (103, 1002, 7), (104, 1003, 7)]);
    });
    PAGE_RECORDSET.on_click(1); PAGE_RECORDSET.on_click(2); PAGE_SHOTSET.on_click(0);
    assert_eq!(capture_settings::get(), Settings { format: VideoFormat::Apng, recording: Storage::Media, screenshot: Storage::Media });
    PAGE_SHOTSET.on_click(1); capture_settings::reset_for_test();
    assert_eq!(capture_settings::get(), Settings { format: VideoFormat::Apng, recording: Storage::Media, screenshot: Storage::OfflineLog });
    ui(|u| *u = Default::default()); PAGE_SHOTSET.render(&ctx).unwrap();
    ui(|u| assert_eq!(u.events, [(101, 1000, 7), (102, 1001, 7)]));
    PAGE_CAPTURE.on_click(4); PAGE_CAPTURE.on_click(5); ui(|u| assert_eq!(u.navigations, [41, 42])); check_clean();
}

#[test]
fn direct_quantization_and_indexed_comparison_match_the_reference() {
    let mut bgr = vec![0; format::FRAME_BYTES]; let mut seed = 7u32;
    for b in &mut bgr { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); *b = (seed >> 24) as u8; }
    let mut expected = vec![0; format::PIXELS]; let mut direct = vec![0; format::PIXELS];
    format::quantize(&bgr, &mut expected); unsafe { format::quantize_framebuffer(bgr.as_ptr(), &mut direct); }
    assert_eq!(direct, expected); assert!(format::indexed_changed_rect(&direct, &expected).is_none());
    direct[335] ^= 1; direct[format::PIXELS - 336] ^= 1;
    assert_eq!(format::indexed_changed_rect(&direct, &expected), Some(format::Rect::FULL));
    assert_eq!(crate::capture_apng::duration(300_000), (300, 1));
}

#[test]
fn persistent_reader_and_sequential_buffered_writes() {
    reset(); let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    for _ in 0..8 { world(|m| m.now += FRAME_MS); unsafe { w.step(); } }
    finish(&mut w); check_clean();
    world(|m| {
        assert_eq!(m.reader_opens, 1); assert_eq!(m.seeks, 0);
        let size = m.files.values().next().unwrap().len();
        assert_eq!(m.write_calls as usize, size.div_ceil(OUTPUT_BUFFER_BYTES));
    });
}

#[test]
fn missing_vsync_uses_timed_capture_and_stop_releases_the_reader() {
    for video in [VideoFormat::Gif, VideoFormat::Apng] {
        reset(); world(|w| { w.vsync = false; w.frame_mode = 2; });
        capture_settings::set(Settings { format: video, ..Settings::DEFAULT }).unwrap();
        let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
        let period = if video == VideoFormat::Gif { 67 } else { 17 };
        for _ in 1..20 { world(|w| w.now += period); unsafe { worker.step(); } }
        assert_eq!(state(), State::Recording); assert_eq!(frames(), 20); assert_eq!(error(), 0);
        world(|w| w.now += period); finish(&mut worker); check_clean();
        save_fixture(if video == VideoFormat::Gif { "unsynced.gif" } else { "unsynced.apng" });
    }
}

#[test]
fn complete_frames_are_kept_despite_a_reported_window_flip() {
    reset(); let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    world(|m| { m.now += FRAME_MS; m.torn = true; }); unsafe { w.step(); }
    assert_eq!(frames(), 2);
    world(|m| m.now += FRAME_MS); unsafe { w.step(); }
    assert_eq!(frames(), 3); finish(&mut w); check_clean(); save_fixture("torn.gif");
}

#[test]
fn unchanged_frames_are_merged_and_keep_the_full_duration() {
    reset(); world(|m| m.frame_mode = 1);
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    for _ in 0..75 { world(|m| m.now += FRAME_MS); unsafe { w.step(); } }
    assert_eq!(frames(), 1); finish(&mut w); check_clean();
    world(|m| assert!(m.files.values().next().unwrap().len() < 2048));
    save_fixture("static.gif");
}

#[test]
fn moving_object_encodes_delta_rectangles_and_preserves_the_canvas() {
    reset(); world(|m| m.frame_mode = 2);
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    for _ in 0..2 { world(|m| m.now += FRAME_MS); unsafe { w.step(); } }
    finish(&mut w); assert_eq!(frames(), 3); check_clean(); save_fixture("delta.gif");
}

#[test]
fn fractional_centiseconds_do_not_accumulate_playback_drift() {
    reset(); world(|m| m.frame_mode = 2);
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    for _ in 0..99 { world(|m| m.now += FRAME_MS); unsafe { w.step(); } }
    world(|m| m.now += FRAME_MS);
    finish(&mut w); assert_eq!(frames(), 100); check_clean(); save_fixture("timing.gif");
}

#[test]
fn start_stop_and_background_frames() {
    reset(); let mut worker = Worker::new(); toggle_recording();
    assert_eq!(state(), State::Starting); unsafe { worker.step(); }
    assert_eq!(state(), State::Recording); assert_eq!(frames(), 1);
    assert_eq!(file_count(".gif"), 0); assert_eq!(file_count(".part"), 1);
    world(|w| w.now += 250); unsafe { worker.step(); } assert_eq!(frames(), 2);
    // No page/foreground reference exists in the worker. OFF pauses acquisition.
    world(|w| { w.on = false; w.now += 1000; }); unsafe { worker.step(); } assert_eq!(frames(), 2);
    world(|w| { w.on = true; w.now += 1000; }); unsafe { worker.step(); } assert_eq!(frames(), 3);
    finish(&mut worker); assert_eq!(file_count(".gif"), 1); assert_eq!(file_count(".part"), 0); check_clean();
    world(|w| {
        let bytes = w.files.values().next().unwrap(); assert!(bytes.starts_with(b"GIF89a")); assert_eq!(bytes.last(), Some(&0x3b));
        if let Ok(dir) = std::env::var("CAPTURE_TEST_OUTPUT") { std::fs::write(std::path::Path::new(&dir).join("worker.gif"), bytes).unwrap(); }
    });
}

#[test]
fn stop_before_start_and_during_start() {
    reset(); let mut w = Worker::new(); toggle_recording(); toggle_recording(); unsafe { w.step(); }
    assert_eq!(state(), State::Idle); assert_eq!(file_count(".gif"), 0); check_clean();
    world(|m| m.stop_during_capture = true); toggle_recording(); unsafe { w.step(); }
    assert_eq!(state(), State::Idle); assert_eq!(file_count(".gif"), 1); check_clean();
}

#[test]
fn real_elapsed_startup_time_does_not_wrap_the_duration_limit() {
    reset(); world(|m| m.clock_step = 10);
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    assert_eq!(state(), State::Recording); assert!(!limited());
    finish(&mut w); check_clean();
}

#[test]
fn worker_waits_for_start_request_publication() {
    reset(); let mut w = Worker::new();
    // UI has reserved Starting, but has not finished resetting the counters.
    STATE.store(1, Ordering::Release); unsafe { w.step(); }
    assert_eq!(frames(), 0); world(|m| assert_eq!(m.captures, 0));
    START_REQUEST.store(true, Ordering::Release); unsafe { w.step(); }
    assert_eq!(state(), State::Recording); finish(&mut w); check_clean();
}

#[test]
fn screenshot_waits_for_off_and_only_runs_once() {
    reset(); let mut w = Worker::new(); toggle_shot(); unsafe { w.step(); }
    assert_eq!(shot_state(), 1); world(|m| assert_eq!(m.captures, 0));
    world(|m| m.on = false); unsafe { w.step(); }
    assert_eq!(shot_state(), 3); assert_eq!(file_count(".bmp"), 1); check_clean();
    unsafe { w.step(); w.step(); } world(|m| assert_eq!(m.captures, 1));
    world(|m| {
        let bytes = m.files.values().next().unwrap(); assert_eq!(bytes.len(), 54 + format::FRAME_BYTES);
        if let Ok(dir) = std::env::var("CAPTURE_TEST_OUTPUT") { std::fs::write(std::path::Path::new(&dir).join("worker.bmp"), bytes).unwrap(); }
    });
}

#[test]
fn cancel_timeout_and_clock_wrap() {
    reset(); let mut w = Worker::new(); toggle_shot(); unsafe { w.step(); } toggle_shot();
    world(|m| m.on = false); unsafe { w.step(); } assert_eq!(file_count(".bmp"), 0);
    world(|m| { m.on = true; m.now = u32::MAX - 100; }); toggle_shot(); unsafe { w.step(); }
    world(|m| m.now = m.now.wrapping_add(60_000)); unsafe { w.step(); }
    assert_eq!(shot_state(), 0); assert_eq!(shot_error(), -110); check_clean();
}

#[test]
fn cancel_and_rearm_before_the_worker_runs_gets_a_fresh_timeout() {
    reset(); let mut w = Worker::new(); toggle_shot(); unsafe { w.step(); }
    world(|m| m.now += 59_000); toggle_shot(); toggle_shot();
    world(|m| m.now += 2000); unsafe { w.step(); } assert_eq!(shot_state(), 1);
    toggle_shot(); unsafe { w.step(); } check_clean();
}

#[test]
fn screenshot_and_recording_share_one_worker() {
    reset(); let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    let recording_heap: usize = world(|m| m.allocations.values().map(|layout| layout.size()).sum());
    toggle_shot(); world(|m| m.on = false); unsafe { w.step(); }
    assert_eq!(state(), State::Recording); assert_eq!(file_count(".bmp"), 1);
    assert_eq!(world(|m| m.allocations.values().map(|layout| layout.size()).sum::<usize>()), recording_heap,
        "saving a screenshot must release its large transient buffer while recording continues");
    finish(&mut w); assert_eq!(file_count(".gif"), 1); check_clean();
}

#[test]
fn short_writes_and_interrupts_preserve_valid_files() {
    reset(); world(|w| { w.write_size = 17; w.interrupts = 3; });
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); } finish(&mut w);
    assert_eq!(error(), 0); assert_eq!(file_count(".gif"), 1); check_clean();
}

#[test]
fn failures_never_publish_partial_capture() {
    for mode in 0..7 {
        reset(); world(|w| match mode {
            0 => w.fail_alloc = true, 1 => w.frame_error = -5,
            2 => w.zero_write = true, 3 => w.budget = 5000,
            4 => w.rename_error = -13, 5 => w.close_error = -5,
            _ => w.free_bytes = Some(1024),
        });
        let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
        if state() != State::Idle { finish(&mut w); }
        assert!(error() < 0, "failure mode {mode}");
        assert_eq!(file_count(".gif"), 0); assert_eq!(file_count(".part"), 0); check_clean();
    }
}

#[test]
fn limits_finish_existing_recordings_and_release_buffers() {
    for low_space in [true, false] {
        reset(); let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
        world(|m| { m.now += if low_space { 1000 } else { LIMIT_MS }; if low_space { m.free_bytes = Some(0); } });
        unsafe { w.step(); }
        assert_eq!(state(), State::Idle); assert!(limited()); assert_eq!(file_count(".gif"), 1); check_clean();
    }
}

#[test]
fn size_limit_finishes_before_another_frame_is_written() {
    reset(); let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); }
    for _ in 0..200 {
        if state() == State::Idle { break; }
        world(|m| m.now += FRAME_MS); unsafe { w.step(); }
    }
    assert_eq!(state(), State::Idle); assert!(limited()); assert!(frames() > 1);
    assert_eq!(file_count(".gif"), 1); check_clean();
    world(|m| {
        let bytes = m.files.values().next().unwrap(); assert!(bytes.len() <= LIMIT_BYTES as usize);
        if let Ok(dir) = std::env::var("CAPTURE_TEST_OUTPUT") { std::fs::write(std::path::Path::new(&dir).join("size-limit.gif"), bytes).unwrap(); }
    });
}

#[test]
fn spawn_failure_and_collision_do_not_clobber_existing_files() {
    reset(); world(|w| w.spawn_result = -12); toggle_recording(); assert_eq!(state(), State::Idle); assert_eq!(error(), -12); check_clean();
    reset(); world(|w| { w.files.insert("/data/offlinelog/chaos-rec-00000001-00000000.gif".into(), b"existing".to_vec()); });
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); } finish(&mut w);
    assert_eq!(file_count(".gif"), 2);
    world(|w| assert_eq!(w.files["/data/offlinelog/chaos-rec-00000001-00000000.gif"], b"existing")); check_clean();
}

#[test]
fn filename_boundaries_and_free_space_units() {
    let mut name = [0; format::NAME_BYTES]; format::name(&mut name, Kind::Screenshot, u32::MAX, u32::MAX);
    let n = name.iter().position(|&b| b == 0).unwrap(); assert_eq!(format::kind(&name[..n]), Some(Kind::Screenshot));
    let mut path = [0; 96]; assert!(format::path(b"../apps.json", false, &mut path).is_err());
    for bad in [b"chaos-rec-00000001-00000000.gif/../apps.json".as_slice(), b"chaos-rec-xxxxxxxx-00000000.gif", b"other.gif"] {
        assert!(!format::owned(bad));
    }
    assert!(format::owned(b"chaos-rec-00000001-00000000.gif.part"));
    assert_eq!(format::data_free(b"\n /tmp 20M 5M 15M /tmp\n /data 100M 50M 50M /data\n"), Some(50 << 20));
    assert_eq!(format::data_free(b"/data 1T 1T 123K /data"), Some(123 << 10));
    assert_eq!(format::data_free(b"littlefs 100M 50M 50M /data"), Some(50 << 20));
    assert_eq!(format::data_free(b"/data total used bogus /data"), None);
}

#[test]
fn control_page_has_actions_settings_footers_and_native_destinations() {
    use crate::{capture_page::*, page::Page, ui};
    reset(); ui(|u| *u = Default::default());
    let ctx = crate::page::PageCtx { content: 1, title: 2, rows: std::ptr::null_mut() };
    PAGE_CAPTURE.render(&ctx).unwrap();
    ui(|u| {
        assert_eq!(u.rows.iter().filter(|r| r.3).map(|r| r.1.as_str()).collect::<Vec<_>>(),
            ["开始录屏", "管理录屏", "遮盖截屏", "管理截屏", "录屏设置", "截图设置"]);
        assert_eq!(u.rows.last().unwrap().1, "截图储存于");
        assert_eq!(u.rows.last().unwrap().2, "/data/offlinelog");
    });
    PAGE_CAPTURE.on_click(0); ui(|u| u.rows.clear()); PAGE_CAPTURE.render(&ctx).unwrap();
    ui(|u| assert_eq!(u.rows[0].1, "结束录屏"));
    PAGE_CAPTURE.on_click(1); PAGE_CAPTURE.on_click(3);
    ui(|u| assert_eq!(u.navigations, [39, 40]));
    let mut w = Worker::new(); toggle_recording(); unsafe { w.step(); } check_clean();
}

#[test]
fn file_lists_paginate_confirm_delete_and_preserve_unrelated_logs() {
    use crate::{capture_page::*, page::Page, ui};
    reset(); ui(|u| *u = Default::default());
    world(|w| {
        for i in 0..10 { w.files.insert(format!("/data/offlinelog/chaos-rec-00000001-{i:08x}.gif"), vec![1; 123]); }
        w.files.insert("/data/offlinelog/system.log".into(), vec![2]);
        w.files.insert("/data/offlinelog/chaos-shot-00000001-00000000.bmp".into(), vec![3]);
    });
    let ctx = crate::page::PageCtx { content: 1, title: 2, rows: std::ptr::null_mut() };
    PAGE_RECORDINGS.render(&ctx).unwrap();
    ui(|u| { assert_eq!(u.rows.iter().filter(|r| r.0 < 8).count(), 8); assert!(u.rows.iter().any(|r| r.1 == "下一页")); });
    PAGE_RECORDINGS.on_click(9); ui(|u| u.rows.clear()); PAGE_RECORDINGS.render(&ctx).unwrap();
    ui(|u| assert_eq!(u.rows.iter().filter(|r| r.0 < 8).count(), 2));
    PAGE_RECORDINGS.on_click(0); PAGE_RECORDINGS.tick(); assert_eq!(file_count(".gif"), 10);
    PAGE_RECORDINGS.on_click(0); PAGE_RECORDINGS.tick(); assert_eq!(file_count(".gif"), 9);
    world(|w| assert!(w.files.contains_key("/data/offlinelog/system.log")));
    ui(|u| u.rows.clear()); PAGE_SCREENSHOTS.render(&ctx).unwrap();
    ui(|u| { assert_eq!(u.rows.iter().filter(|r| r.0 < 8).count(), 1); assert!(u.rows[0].1.ends_with(".bmp")); });
    check_clean();
}

#[test]
fn apng_slow_storage_continues_with_real_timing_instead_of_overload_stop() {
    reset(); capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    world(|w| { w.write_cost_ms = 5; w.lost_vsync = true; });
    for _ in 0..5 { world(|w| w.now += 17); unsafe { worker.step(); } }
    assert_eq!(state(), State::Recording); assert_eq!(error(), 0); assert_eq!(frames(), 6);
    assert!(fps() > 0 && fps() < 60); assert!(constrained());
    let duration = world(|w| w.now - 1000);
    finish(&mut worker); assert_eq!(error(), 0); check_clean(); save_fixture("slow.apng");
    if let Ok(dir) = std::env::var("CAPTURE_TEST_OUTPUT") {
        std::fs::write(std::path::Path::new(&dir).join("slow-duration.txt"), duration.to_string()).unwrap();
    }
}

#[test]
fn gaps_and_window_switches_keep_complete_apng_frames() {
    reset(); world(|w| w.frame_mode = 2);
    capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    world(|w| { w.now += 17; w.lost_vsync = true; }); unsafe { worker.step(); }
    assert_eq!(frames(), 2); assert_eq!(state(), State::Recording); assert!(constrained());
    world(|w| { w.now += 17; w.torn = true; }); unsafe { worker.step(); }
    assert_eq!(frames(), 3); assert_eq!(error(), 0);
    world(|w| { w.now += 17; w.lost_vsync = false; }); unsafe { worker.step(); }
    assert_eq!(frames(), 4); world(|w| w.now += 17);
    finish(&mut worker); assert_eq!(error(), 0); check_clean(); save_fixture("gaps.apng");
}

#[test]
fn recording_page_shows_measured_sampling_rate_and_original_settings_only() {
    use crate::{capture_page::*, page::Page, ui};
    reset(); world(|w| w.frame_mode = 2);
    capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for _ in 0..4 { world(|w| w.now += 80); unsafe { worker.step(); } }
    assert_eq!(fps(), 13);
    let ctx = crate::page::PageCtx { content: 1, title: 2, rows: std::ptr::null_mut() };
    ui(|u| *u = Default::default()); PAGE_CAPTURE.render(&ctx).unwrap();
    ui(|u| assert!(u.rows[0].2.contains("采集 13 fps")));
    ui(|u| *u = Default::default()); PAGE_RECORDSET.render(&ctx).unwrap();
    ui(|u| assert_eq!(u.rows.iter().filter(|row| row.3).map(|row| row.1.as_str()).collect::<Vec<_>>(),
        ["GIF · 15 fps", "APNG · 60 fps", "/data/chaos/media", "/data/offlinelog"]));
    finish(&mut worker); check_clean();
}

#[test]
fn timer_fallback_does_not_busy_read_without_vsync() {
    reset(); world(|w| { w.vsync = false; w.frame_mode = 2; });
    capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
    let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
    for ms in 1..1000 { world(|w| w.now = 1000 + ms); unsafe { worker.step(); } }
    world(|w| assert_eq!(w.captures, 59, "timer fallback must remain bounded near the requested rate"));
    assert_eq!(frames(), 59); finish(&mut worker); assert_eq!(error(), 0); check_clean();
}

#[test]
fn neutral_grays_do_not_gain_a_yellow_green_or_blue_tint() {
    let mut bgr = Vec::new(); for gray in 0..=255u8 { bgr.extend([gray; 3]); }
    let mut indices = vec![0u8; 256]; format::quantize(&bgr, &mut indices);
    for (gray, &index) in indices.iter().enumerate() {
        let rgb = &format::PALETTE[index as usize * 3..index as usize * 3 + 3];
        assert_eq!(rgb[0], rgb[1], "gray {gray}"); assert_eq!(rgb[1], rgb[2], "gray {gray}");
        assert!((rgb[0] as i32 - gray as i32).abs() <= 31);
    }
    assert_eq!(&format::PALETTE[36 * 3..36 * 3 + 3], &[36, 36, 36]);
}

#[test]
fn gray_cards_and_color_channels_decode_in_both_recording_formats() {
    for video in [VideoFormat::Gif, VideoFormat::Apng] {
        reset(); world(|w| w.frame_mode = 3);
        capture_settings::set(Settings { format: video, ..Settings::DEFAULT }).unwrap();
        let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
        world(|w| w.now += 200); finish(&mut worker); assert_eq!(error(), 0); check_clean();
        save_fixture(if video == VideoFormat::Gif { "grays.gif" } else { "grays.apng" });
    }
}

#[test]
fn snapshot_repair_is_single_attempt_and_skips_slow_copies() {
    for slow in [false, true] {
        reset(); world(|w| { w.vsync = false; w.frame_mode = 2; w.switch_every_copy = true; w.read_cost_ms = if slow { 8 } else { 0 }; });
        capture_settings::set(Settings { format: VideoFormat::Apng, ..Settings::DEFAULT }).unwrap();
        let mut worker = Worker::new(); toggle_recording(); unsafe { worker.step(); }
        world(|w| assert_eq!(w.captures, if slow { 1 } else { 2 }, "never loop on a moving display"));
        assert_eq!(frames(), 1); assert_eq!(state(), State::Recording);
        finish(&mut worker); assert_eq!(error(), 0); check_clean();
    }
}
