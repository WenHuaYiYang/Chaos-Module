#[path = "../src/gyro_pointer/logic.rs"]
mod logic;
#[path = "../src/gyro_pointer/config.rs"]
mod config;
use logic::*;

struct Rig { c: Controller, now: u32, timestamp: u64, frame: Frame }
impl Rig {
    fn new() -> Self {
        let mut c = Controller::new();
        let frame = c.update(0, None, false, false);
        Self { c, now: 0, timestamp: 1_000_000, frame }
    }
    fn step(&mut self, x: f32, y: f32, z: f32, physical: bool, flipped: bool) -> Frame {
        self.now = self.now.wrapping_add(20); self.timestamp += 20_000;
        self.frame = self.c.update(self.now, Some(Motion {
            timestamp_us: self.timestamp, x, y, z,
        }), physical, flipped);
        self.frame
    }
    fn quiet(&mut self) -> Frame { self.step(0.0, 0.0, 0.0, false, false) }
    fn ready(&mut self) { for _ in 0..30 { self.quiet(); } assert!(self.frame.calibrated); }
    fn down(&mut self) -> Frame {
        for _ in 0..150 { if self.quiet().touch.pressed { return self.frame; } }
        panic!("no gesture after three seconds: {:?}", self.frame);
    }
    fn edge(&mut self, x: f32, y: f32) {
        for _ in 0..45 { self.step(x, y, 0.0, false, false); }
        for _ in 0..12 { self.quiet(); }
    }
    fn reach_edge(&mut self, x: f32, y: f32) -> Frame {
        for _ in 0..120 {
            let f = self.step(x, y, 0.0, false, false);
            let contact = f.cursor.x == MARGIN as i32 || f.cursor.x == WIDTH-1-MARGIN as i32
                || f.cursor.y == MARGIN as i32 || f.cursor.y == HEIGHT-1-MARGIN as i32;
            assert_eq!(f.touch.pressed, contact, "first contact must act immediately: {:?}",f);
            if contact { assert_eq!(f.progress,0);return f; }
        }
        panic!("edge not reached");
    }
}

#[test]
fn calibration_precedes_dwell_and_progress_is_time_based() {
    let mut r = Rig::new();
    for _ in 0..20 { assert!(!r.quiet().calibrated); assert_eq!(r.frame.progress, 0); }
    r.ready();
    let start = r.frame.cursor;
    for _ in 0..20 { assert!(!r.quiet().touch.pressed); }
    assert!(r.frame.progress > 0 && r.frame.progress < 100);
    assert_eq!(r.down().touch.point, start);
}

#[test]
fn moving_wrist_recovers_after_two_seconds_without_learning_the_turn_as_bias() {
    let mut r = Rig::new();
    for _ in 0..100 {
        let f = r.step(0.7, 0.0, 0.0, false, false);
        assert!(!f.calibrated); assert!(!f.touch.pressed);
    }
    assert!(r.step(0.7, 0.0, 0.0, false, false).calibrated);
    for _ in 0..5 { r.step(0.7, 0.0, 0.0, false, false); }
    assert!(r.frame.cursor.y > HEIGHT/2 + 10);
}

#[test]
fn quiet_bias_is_removed_without_cursor_drift() {
    let mut r = Rig::new();
    for _ in 0..400 { r.step(0.02, -0.015, 0.005, false, false); }
    assert_eq!(r.frame.cursor, Point { x: WIDTH / 2, y: HEIGHT / 2 });
}

#[test]
fn gyro_axes_move_cursor_and_stop_at_screen_bounds() {
    let mut r = Rig::new(); r.ready(); r.edge(1.0, 1.0);
    assert_eq!(r.frame.cursor, Point { x: WIDTH-1-MARGIN as i32, y: HEIGHT-1-MARGIN as i32 });
    for _ in 0..80 { r.step(-1.0, -1.0, 0.0, false, false); }
    for _ in 0..12 { r.quiet(); }
    assert_eq!(r.frame.cursor, Point { x: MARGIN as i32, y: MARGIN as i32 });
}

#[test]
fn one_dwell_emits_one_press_and_release_without_repeating() {
    let mut r = Rig::new(); r.ready(); r.down();
    let mut pressed = 1;
    while r.quiet().touch.pressed { pressed += 1; }
    assert_eq!(pressed, 3); // 60ms press, not a single synthetic CLICKED event
    for _ in 0..600 { assert!(!r.quiet().touch.pressed); assert_eq!(r.frame.progress, 0); }
}

#[test]
fn moving_away_rearms_a_center_click() {
    let mut r = Rig::new(); r.ready(); let first = r.down().cursor;
    for _ in 0..5 { r.quiet(); }
    for _ in 0..10 { r.step(0.0, 0.4, 0.0, false, false); }
    let second = r.down();
    assert!(second.cursor.x > first.x + 14);
}

#[test]
fn large_displacement_cancels_the_current_countdown() {
    let mut r = Rig::new(); r.ready();
    for _ in 0..20 { r.quiet(); }
    assert!(r.frame.progress > 0);
    let mut reset=false;
    for _ in 0..3 {let f=r.step(0.8,0.0,0.0,false,false);reset|=f.progress==0;assert!(!f.touch.pressed);}
    assert!(reset);
}

#[test]
fn cumulative_drift_is_measured_from_the_dwell_anchor() {
    let mut r = Rig::new(); r.ready();
    for _ in 0..300 {
        let f = r.step(0.08, 0.0, 0.0, false, false);
        // Every individual frame moves less than a pixel, total drift must reset dwell.
        if f.cursor.y < HEIGHT-25 { assert!(!f.touch.pressed); assert!(f.progress < 40); }
    }
}

#[test]
fn small_jitter_within_the_radius_keeps_the_countdown() {
    let mut r = Rig::new(); r.ready();
    let mut fired = false;
    for i in 0..80 {
        let f = r.step(if i%2 == 0 { 0.03 } else { -0.03 }, 0.0, 0.0, false, false);
        fired |= f.touch.pressed;
    }
    assert!(fired);
}

fn swipe(x: f32, y: f32, dx: i32, dy: i32) {
    let mut r = Rig::new(); r.ready();
    let first = r.reach_edge(x,y).touch.point; let mut previous = first; let mut count = 1;
    loop {
        let f = r.quiet();
        assert_eq!(f.progress,0);
        assert!((f.touch.point.x - previous.x) * dx >= 0);
        assert!((f.touch.point.y - previous.y) * dy >= 0);
        previous = f.touch.point;
        if !f.touch.pressed { break; }
        count += 1;
        assert!(count < 30);
    }
    assert_eq!(count, 15);
    if dx != 0 { assert_eq!((previous.x-first.x).signum(), dx); assert!((previous.x-first.x).abs() >= WIDTH/2); }
    if dy != 0 { assert_eq!((previous.y-first.y).signum(), dy); assert!((previous.y-first.y).abs() >= HEIGHT/2); }
}
#[test] fn top_edge_drags_up() { swipe(-0.9, 0.0, 0, -1); }
#[test] fn bottom_edge_drags_down() { swipe(0.9, 0.0, 0, 1); }
#[test] fn left_edge_drags_left() { swipe(0.0, -0.9, -1, 0); }
#[test] fn right_edge_drags_right() { swipe(0.0, 0.9, 1, 0); }

#[test]
fn moving_into_an_edge_triggers_without_stopping_or_waiting_for_click_delay() {
    let mut r = Rig::new(); r.ready();
    r.c.configure(100,5000,TOLERANCE_DEFAULT_PX, PAGE_PAUSE_DEFAULT_S,r.now);
    let start=r.now;r.reach_edge(0.9,0.0);
    assert!(r.now.wrapping_sub(start)<700);
    assert_eq!(r.frame.cursor.y, HEIGHT-1-MARGIN as i32);
    assert!(r.frame.touch.pressed);
}

#[test]
fn holding_an_edge_repeats_swipes_after_cooldown() {
    let mut r = Rig::new(); r.ready();r.c.configure(100,5000,TOLERANCE_DEFAULT_PX, PAGE_PAUSE_DEFAULT_S,r.now);
    r.reach_edge(0.9,0.0);
    while r.quiet().touch.pressed {}
    let release = r.now;
    for _ in 0..34 { assert!(!r.quiet().touch.pressed);assert_eq!(r.frame.progress,0); }
    assert!(r.quiet().touch.pressed);assert_eq!(r.now.wrapping_sub(release),700);
}

#[test]
fn physical_touch_cancels_pressed_input_and_restarts_dwell() {
    let mut r = Rig::new(); r.ready(); r.down();
    let f = r.step(0.0, 0.0, 0.0, true, false);
    assert!(f.reset_input); assert!(!f.touch.pressed); assert_eq!(f.progress, 0);
    for _ in 0..100 { assert!(!r.step(0.0, 0.0, 0.0, true, false).touch.pressed); }
    for _ in 0..30 { assert!(!r.quiet().touch.pressed); }
}

#[test]
fn disable_or_sleep_cancels_press_and_requires_calibration_on_resume() {
    let mut r = Rig::new(); r.ready(); r.down();
    let f = r.c.suspend(); assert!(f.reset_input); assert!(!f.touch.pressed);
    assert!(!f.calibrated);
    for _ in 0..20 { let f = r.quiet(); assert!(!f.calibrated); assert!(!f.touch.pressed); }
}

#[test]
fn replayed_samples_cannot_keep_the_sensor_alive() {
    let mut r = Rig::new(); r.ready(); let old = r.timestamp;
    let started = r.now;
    for _ in 0..100 {
        r.now += 20;
        let f = r.c.update(r.now, Some(Motion { timestamp_us: old, x: 0.0, y: 0.0, z: 0.0 }), false, false);
        assert!(!f.touch.pressed);
        if r.now.wrapping_sub(started) > SAMPLE_TIMEOUT_MS { assert_eq!(f.progress, 0); }
    }
}

#[test]
fn missing_packets_cancel_a_pressed_gesture() {
    let mut r = Rig::new(); r.ready(); r.down();
    r.now += SAMPLE_TIMEOUT_MS+1;
    let f = r.c.update(r.now, None, false, false);
    assert!(f.reset_input); assert!(!f.touch.pressed); assert!(!f.calibrated);
}

#[test]
fn scheduler_stall_cancels_instead_of_releasing_as_a_click() {
    let mut r = Rig::new(); r.ready(); r.down();
    r.now += 1000; r.timestamp += 1_000_000;
    let f = r.c.update(r.now, Some(Motion { timestamp_us: r.timestamp, x: 0.0, y: 0.0, z: 0.0 }), false, false);
    assert!(f.reset_input); assert!(!f.touch.pressed); assert!(!f.calibrated);
}

#[test]
fn invalid_sensor_values_cancel_input_immediately() {
    for value in [f32::NAN, f32::INFINITY, 100.0] {
        let mut r = Rig::new(); r.ready(); r.down();
        let f = r.step(value, 0.0, 0.0, false, false);
        assert!(f.reset_input); assert!(!f.touch.pressed); assert!(!f.calibrated);
    }
}

#[test]
fn time_wrap_preserves_calibration_countdown_and_release() {
    let mut r = Rig::new(); r.now = u32::MAX-300; r.ready();
    r.down(); while r.quiet().touch.pressed {}
    assert!(r.now < 5000);
}

#[test]
fn flipped_display_reverses_motion_and_inverts_raw_coordinates() {
    assert_eq!(raw_point(Point { x: 16, y: 30 }, true), Point { x: 319, y: 449 });
    assert_eq!(raw_point(raw_point(Point { x: 77, y: 22 }, true), true), Point { x: 77, y: 22 });
    let mut r = Rig::new(); r.ready();
    r.step(0.0, 0.0, 0.0, false, true);
    for _ in 0..10 { r.step(0.0, 0.5, 0.0, false, true); }
    assert!(r.frame.cursor.x < WIDTH/2);
}

#[test]
fn rotation_during_a_press_cancels_the_old_target() {
    let mut r = Rig::new(); r.ready(); r.down();
    let f = r.step(0.0, 0.0, 0.0, false, true);
    assert!(f.reset_input); assert!(!f.touch.pressed);
}

#[test]
fn positive_horizontal_rate_moves_right_without_changing_vertical_axis() {
    let mut r = Rig::new(); r.ready();
    for _ in 0..10 { r.step(0.0, 0.3, 0.0, false, false); }
    assert!(r.frame.cursor.x > WIDTH/2); assert_eq!(r.frame.cursor.y, HEIGHT/2);
    for _ in 0..30 { r.step(0.0, -0.3, 0.0, false, false); }
    assert!(r.frame.cursor.x < WIDTH/2); assert_eq!(r.frame.cursor.y, HEIGHT/2);
}

#[test]
fn sensitivity_scales_motion_at_the_same_angular_velocity() {
    let mut normal = Rig::new(); normal.ready();
    let mut fast = Rig::new(); fast.ready();
    fast.c.configure(200, DWELL_MS, TOLERANCE_DEFAULT_PX, PAGE_PAUSE_DEFAULT_S, fast.now);
    for _ in 0..12 {
        normal.step(0.0, 0.2, 0.0, false, false);
        fast.step(0.0, 0.2, 0.0, false, false);
    }
    let a = normal.frame.cursor.x-WIDTH/2; let b = fast.frame.cursor.x-WIDTH/2;
    assert!(a > 0); assert!((b-2*a).abs() <= 1);
}

#[test]
fn custom_confirmation_times_control_when_the_press_begins() {
    for delay in [CONFIRM_MIN_MS, 1200, CONFIRM_MAX_MS] {
        let mut r = Rig::new(); r.ready(); r.c.configure(100, delay, TOLERANCE_DEFAULT_PX, PAGE_PAUSE_DEFAULT_S, r.now);
        let configured = r.now;
        for _ in 0..400 {
            let f = r.quiet();
            if f.touch.pressed {
                assert_eq!(r.now.wrapping_sub(configured), 700 + delay); break;
            }
        }
        assert!(r.frame.touch.pressed);
    }
}

#[test]
fn changing_preferences_cancels_a_press_and_applies_new_dwell() {
    let mut r = Rig::new(); r.ready(); r.down();
    let f = r.c.configure(150, 1500, TOLERANCE_DEFAULT_PX, PAGE_PAUSE_DEFAULT_S, r.now).unwrap();
    assert!(f.reset_input); assert!(!f.touch.pressed); assert_eq!(f.progress,0);
    for _ in 0..50 { assert!(!r.quiet().touch.pressed); }
}

#[test]
fn enlarged_cursor_stays_visible_and_can_still_reach_all_edge_zones() {
    assert!(CURSOR_SIZE >= 48 && DOT_SIZE >= 20);
    for (x,y) in [(1.0,0.0),(-1.0,0.0),(0.0,1.0),(0.0,-1.0)] {
        let mut r = Rig::new(); r.ready(); r.edge(x,y);
        let p = r.frame.cursor;
        assert!(p.x-MARGIN as i32 >= 0 && p.x+(MARGIN as i32) < WIDTH);
        assert!(p.y-MARGIN as i32 >= 0 && p.y+(MARGIN as i32) < HEIGHT);
        r.down();
    }
}

#[test]
fn legacy_switch_files_gain_default_preferences() {
    use config::Config;
    assert_eq!(Config::decode(b"GTP1\x01"), Some(Config{enabled:true,..Config::defaults()}));
    assert_eq!(Config::decode(b"GTP1\0"), Some(Config::defaults()));
    assert_eq!(Config::decode(b"GTP1\x02"), None);
}

#[test]
fn preferences_round_trip_at_supported_limits() {
    use config::Config;
    for sensitivity in [25,100,300] { for confirm_ms in [300,900,5000] { for tolerance_px in [1,6,30] { for page_pause_s in [0,2,10] {
        let config=Config{enabled:true,sensitivity,confirm_ms,tolerance_px,page_pause_s};
        assert_eq!(Config::decode(&config.encode()),Some(config));
    }}}}
}

#[test]
fn corrupt_truncated_and_out_of_range_preferences_are_rejected() {
    use config::Config;
    let encoded=Config::defaults().encode();
    for i in 0..encoded.len() {let mut corrupt=encoded;corrupt[i]^=1;assert_eq!(Config::decode(&corrupt),None);}
    for len in 0..encoded.len() {assert_eq!(Config::decode(&encoded[..len]),None);}
    let mut trailing=encoded.to_vec();trailing.push(0);assert_eq!(Config::decode(&trailing),None);
    for config in [Config{sensitivity:24,..Config::defaults()},Config{sensitivity:301,..Config::defaults()},
                   Config{confirm_ms:299,..Config::defaults()},Config{confirm_ms:5001,..Config::defaults()},
                   Config{tolerance_px:0,..Config::defaults()},Config{tolerance_px:31,..Config::defaults()},
                   Config{page_pause_s:11,..Config::defaults()}] {
        assert_eq!(Config::decode(&config.encode()),None);
    }
}

#[test]
fn numeric_preference_input_rejects_empty_symbols_and_overflow() {
    assert_eq!(config::parse_number(b"100"),Some(100));
    assert_eq!(config::parse_number(b"5000"),Some(5000));
    for bad in [b"".as_slice(),b"-1",b"1.5",b"12a",b"65536",b"100\0"] {
        assert_eq!(config::parse_number(bad),None);
    }
}

#[test]
fn click_displacement_tolerance_controls_jitter_without_a_hidden_rate_limit() {
    let mut strict=Rig::new();strict.ready();strict.c.configure(100,900,2, PAGE_PAUSE_DEFAULT_S,strict.now);
    let mut loose=Rig::new();loose.ready();loose.c.configure(100,900,20, PAGE_PAUSE_DEFAULT_S,loose.now);
    let mut strict_clicks=0;let mut loose_clicks=0;let mut previous=[false;2];
    for i in 0..240 {
        let rate=if (i/15)%2==0 {0.12} else {-0.12};
        let a=strict.step(rate,0.0,0.0,false,false);
        let b=loose.step(rate,0.0,0.0,false,false);
        strict_clicks+=usize::from(a.touch.pressed&&!previous[0]);
        loose_clicks+=usize::from(b.touch.pressed&&!previous[1]);
        previous=[a.touch.pressed,b.touch.pressed];
    }
    assert_eq!(strict_clicks,0);assert_eq!(loose_clicks,1);
}

#[test]
fn changing_click_tolerance_cancels_an_old_press() {
    let mut r=Rig::new();r.ready();r.down();
    let f=r.c.configure(100,900,12, PAGE_PAUSE_DEFAULT_S,r.now).unwrap();
    assert!(f.reset_input);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
}

#[test]
fn screen_suspend_keeps_custom_click_tolerance() {
    let mut r=Rig::new();r.ready();r.c.configure(175,1600,20, PAGE_PAUSE_DEFAULT_S,r.now);
    r.c.suspend();
    assert!(r.c.configure(175,1600,20, PAGE_PAUSE_DEFAULT_S,r.now).is_none());
}

#[test]
fn gtp2_migrates_saved_speed_and_time_with_default_click_tolerance() {
    let mut old=[b'G',b'T',b'P',b'2',1,175,0,0x40,0x06,0];
    old[9]=old[..9].iter().fold(0xa5,|a,b|a^b);
    assert_eq!(config::Config::decode(&old),Some(config::Config {
        enabled:true,sensitivity:175,confirm_ms:1600,tolerance_px:6,page_pause_s:2,
    }));
    old[9]^=1;assert_eq!(config::Config::decode(&old),None);
}

#[test]
fn all_four_edges_ignore_confirmation_time_on_first_contact() {
    for (x,y) in [(0.9,0.0),(-0.9,0.0),(0.0,0.9),(0.0,-0.9)] {
        let mut r=Rig::new();r.ready();r.c.configure(100,5000,1, PAGE_PAUSE_DEFAULT_S,r.now);
        let start=r.now;let f=r.reach_edge(x,y);
        assert!(r.now.wrapping_sub(start)<900);assert!(f.touch.pressed);assert_eq!(f.progress,0);
    }
}

#[test]
fn leaving_and_reentering_an_edge_starts_a_new_swipe_without_dwell() {
    let mut r=Rig::new();r.ready();r.reach_edge(0.9,0.0);
    while r.quiet().touch.pressed {}
    let release=r.now;
    for _ in 0..5 {assert!(!r.step(-0.9,0.0,0.0,false,false).touch.pressed);}
    let f=r.reach_edge(0.9,0.0);
    assert!(f.touch.pressed);assert!(r.now.wrapping_sub(release)<700);
}

#[test]
fn physical_touch_cancels_an_immediate_edge_swipe() {
    let mut r=Rig::new();r.ready();r.reach_edge(0.9,0.0);
    let f=r.step(0.9,0.0,0.0,true,false);
    assert!(f.reset_input);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
    for _ in 0..40 {assert!(!r.step(0.9,0.0,0.0,true,false).touch.pressed);}
}

#[test]
fn page_reset_centers_and_resumes_after_exactly_two_seconds_while_moving() {
    let mut r=Rig::new();r.ready();r.reach_edge(0.9,0.0);
    let reset=r.c.reset_page(r.now);
    let center=Point{x:WIDTH/2,y:HEIGHT/2};
    assert!(reset.reset_input);assert!(!reset.touch.pressed);assert!(!reset.calibrated);
    assert_eq!(reset.cursor,center);assert_eq!(reset.touch.point,center);assert_eq!(reset.progress,0);
    for _ in 0..99 {
        let f=r.step(0.8,0.6,0.0,false,false);
        assert_eq!(f.cursor,center);assert_eq!(f.progress,0);
        assert!(!f.touch.pressed);assert!(!f.calibrated);
    }
    let resumed=r.step(0.8,0.6,0.0,false,false);
    assert!(resumed.calibrated);assert!(!resumed.touch.pressed);assert_eq!(resumed.progress,0);
    assert_eq!(resumed.cursor,center);
    let moved=r.step(0.8,0.6,0.0,false,false);
    assert!(moved.cursor.x>center.x);assert!(moved.cursor.y>center.y);
}

#[test]
fn page_reset_discards_an_old_click_latch_and_starts_a_full_new_countdown() {
    let mut r=Rig::new();r.ready();r.down();
    assert!(r.c.reset_page(r.now).reset_input);
    for _ in 0..100 {assert!(!r.quiet().touch.pressed);}
    assert!(r.frame.calibrated);assert_eq!(r.frame.progress,0);
    for _ in 0..44 {assert!(!r.quiet().touch.pressed);}
    assert!(r.quiet().touch.pressed);
    assert_eq!(r.frame.cursor,Point{x:WIDTH/2,y:HEIGHT/2});
}

#[test]
fn another_page_change_restarts_the_pause_even_across_clock_wrap() {
    let mut r=Rig::new();r.ready();r.now=u32::MAX-1000;
    r.c.reset_page(r.now);
    for _ in 0..60 {assert!(!r.quiet().calibrated);}
    r.c.reset_page(r.now);
    for _ in 0..99 {assert!(!r.step(0.6,0.0,0.0,false,false).calibrated);}
    assert!(r.step(0.6,0.0,0.0,false,false).calibrated);
}

#[test]
fn page_pause_does_not_resume_on_replayed_or_missing_sensor_data() {
    let mut r=Rig::new();r.ready();r.down();r.c.reset_page(r.now);
    let timestamp=r.timestamp;
    for i in 0..160 {
        r.now+=20;
        let motion=if i%2==0 {Some(Motion{timestamp_us:timestamp,x:0.0,y:0.0,z:0.0})} else {None};
        let f=r.c.update(r.now,motion,false,false);
        assert!(!f.calibrated);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
    }
    assert!(r.step(0.7,0.0,0.0,false,false).calibrated);
    assert!(!r.frame.touch.pressed);assert_eq!(r.frame.progress,0);
}

#[test]
fn a_fresh_recovery_window_is_bounded_after_a_scheduler_stall() {
    let mut r=Rig::new();r.ready();r.down();
    r.now+=800;r.timestamp+=800_000;
    let f=r.step(0.6,0.0,0.0,false,false);
    assert!(f.reset_input);assert!(!f.calibrated);
    for _ in 0..99 {assert!(!r.step(0.6,0.0,0.0,false,false).calibrated);}
    assert!(r.step(0.6,0.0,0.0,false,false).calibrated);
}

#[test]
fn page_reset_keeps_learned_bias_and_ignores_the_turn_during_the_pause() {
    let mut r=Rig::new();
    for _ in 0..30 {r.step(0.1,0.0,0.0,false,false);}
    assert!(r.frame.calibrated);r.c.reset_page(r.now);
    for _ in 0..100 {r.step(0.7,0.0,0.0,false,false);}
    for _ in 0..20 {r.step(0.1,0.0,0.0,false,false);}
    assert_eq!(r.frame.cursor,Point{x:WIDTH/2,y:HEIGHT/2});
}

#[test]
fn configured_page_pause_uses_seconds_and_resumes_while_moving() {
    for seconds in [1,3,10] {
        let mut r=Rig::new();r.ready();r.c.configure(100,900,6,seconds,r.now);
        r.c.reset_page(r.now);
        let started=r.now;
        for _ in 1..seconds*50 {
            let f=r.step(0.7,0.4,0.0,false,false);
            assert!(!f.calibrated);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
            assert_eq!(f.cursor,Point{x:WIDTH/2,y:HEIGHT/2});
        }
        let f=r.step(0.7,0.4,0.0,false,false);
        assert!(f.calibrated);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
        assert_eq!(r.now.wrapping_sub(started),seconds*1000);
    }
}

#[test]
fn zero_page_pause_still_cancels_old_input_and_restarts_the_full_click_countdown() {
    let mut r=Rig::new();r.ready();r.c.configure(100,900,6,0,r.now);r.down();
    let f=r.c.reset_page(r.now);
    assert!(f.reset_input);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
    let f=r.quiet();assert!(f.calibrated);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
    for _ in 0..44 {assert!(!r.quiet().touch.pressed);}
    assert!(r.quiet().touch.pressed);
}

#[test]
fn pause_changes_cancel_an_armed_click_and_survive_suspend() {
    let mut r=Rig::new();r.ready();r.down();
    let f=r.c.configure(100,900,6,7,r.now).unwrap();
    assert!(f.reset_input);assert!(!f.touch.pressed);assert_eq!(f.progress,0);
    r.c.suspend();
    assert!(r.c.configure(100,900,6,7,r.now).is_none());
    r.c.reset_page(r.now);
    for _ in 0..349 {assert!(!r.step(0.7,0.0,0.0,false,false).calibrated);}
    assert!(r.step(0.7,0.0,0.0,false,false).calibrated);
}

#[test]
fn gtp3_migration_preserves_click_tolerance_and_defaults_pause_to_two_seconds() {
    let mut old=[b'G',b'T',b'P',b'3',1,175,0,0x40,0x06,20,0,0];
    old[11]=old[..11].iter().fold(0xa5,|a,b|a^b);
    let expected=config::Config{enabled:true,sensitivity:175,confirm_ms:1600,tolerance_px:20,page_pause_s:2};
    assert_eq!(config::Config::decode(&old),Some(expected));
    assert_eq!(config::Config::decode(&expected.encode()),Some(expected));
    old[11]^=1;assert_eq!(config::Config::decode(&old),None);
}
