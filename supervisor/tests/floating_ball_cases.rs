use super::*;
use crate::{cstr, world, World};
use model::{Action, Corner, Dock, Edge, HEIGHT, SIZES, WIDTH};

fn reset() {
    unsafe {
        reset_for_test();
        port::reset_for_test();
        world(|w| *w = World::default());
    }
}
fn step(ms: u32) {
    world(|w| w.now = w.now.wrapping_add(ms));
    unsafe {
        tick();
    }
}
fn animate(ms: u32) {
    world(|w| w.now = w.now.wrapping_add(ms));
    unsafe {
        frame(1000);
    }
}
fn send(code: u32, tag: u32, point: Point) {
    world(|w| w.input = port::Input { code, tag, point });
    unsafe {
        event(1);
    }
}
fn click(tag: u32) {
    if (port::ACTION..port::ACTION + 3).contains(&tag) {
        choose((tag - port::ACTION) as usize);
    } else {
        send(7, tag, Point::default());
    }
}
fn choose(index: usize) {
    let c = unsafe { config() };
    let p = c.fan_centers()[index];
    send(7, port::BAND, p);
}
fn start_keep(index: usize) {
    choose(1);
    step(0);
    unsafe {
        assert_eq!(VIEW, View::Duration);
        assert!(!keep_awake());
    }
    animate(192);
    choose(index);
    step(0);
    animate(192);
}
fn has_text(text: &str) -> bool {
    world(|w| w.objects.values().any(|o| o.text == text))
}
fn tagged(tag: u32) -> u32 {
    world(|w| {
        *w.objects
            .iter()
            .find(|(_, o)| o.events.iter().any(|&(_, t)| t == tag))
            .unwrap()
            .0
    })
}
fn enabled() {
    unsafe {
        let mut c = config();
        c.enabled = true;
        assert!(update(c));
        tick();
    }
}
fn open() {
    let p = unsafe { POSITION };
    send(1, port::BALL, p);
    send(8, port::BALL, p);
    click(port::BALL);
    animate(192);
    unsafe {
        assert_eq!(VIEW, View::Fan);
    }
}
fn children_text(object: u32) -> Vec<String> {
    world(|w| {
        w.objects
            .values()
            .filter(|o| o.parent == object)
            .map(|o| o.text.clone())
            .collect()
    })
}

#[test]
fn configuration_is_strict_versioned_and_preserves_unique_assignments() {
    let c = Config::DEFAULT;
    assert!(!c.enabled);
    assert_eq!(c.corner(), Corner::TopRight);
    assert_eq!(
        c.slots,
        [Action::Time, Action::KeepAwake, Action::Brightness]
    );
    assert_eq!(Config::decode(&c.encode()), Some(c));
    for i in 0..CONFIG_LEN {
        let mut b = c.encode();
        b[i] ^= 0x80;
        assert!(Config::decode(&b).is_none());
    }
    let mut longer = c.encode().to_vec();
    longer.push(0);
    assert!(Config::decode(&longer).is_none());
    for (field, bad) in [(4, 2), (5, 0), (5, 73), (6, 5), (7, 4), (8, 9), (9, 0)] {
        let mut bytes = c.encode();
        bytes[field] = bad;
        bytes[15] = bytes[..15].iter().fold(0xa5, |sum, b| sum ^ b);
        assert!(Config::decode(&bytes).is_none());
    }
    let mut c = c;
    assert!(c.assign(0, Action::Brightness));
    assert_eq!(
        c.slots,
        [Action::Brightness, Action::KeepAwake, Action::Time]
    );
    assert!(c.assign(1, Action::Screenshot));
    assert!(c.valid());
    assert!(!c.assign(3, Action::Time));
}

#[test]
fn every_supported_size_and_corner_fits_all_three_fan_targets() {
    for size in model::MIN_SIZE..=model::MAX_SIZE {
        for corner in Corner::ALL {
            let p = corner.anchor(size);
            assert!(
                p.x >= 0 && p.y >= 0 && p.x + size as i32 <= WIDTH && p.y + size as i32 <= HEIGHT
            );
            let centers = model::fan_centers(corner, size, 1000);
            for p in centers {
                assert!(p.x >= 30 && p.x <= WIDTH - 30 && p.y >= 30 && p.y <= HEIGHT - 30);
            }
            for pair in centers.windows(2) {
                let dx = pair[0].x - pair[1].x;
                let dy = pair[0].y - pair[1].y;
                assert!(
                    dx * dx + dy * dy >= 44 * 44,
                    "arc tap sectors are too small"
                );
            }
        }
    }
}

#[test]
fn hold_drag_clamps_to_screen_and_never_opens_a_menu_on_release() {
    for (touch, wanted) in [
        (Point { x: -5000, y: -5000 }, Corner::TopLeft),
        (Point { x: 5000, y: -5000 }, Corner::TopRight),
        (Point { x: -5000, y: 5000 }, Corner::BottomLeft),
        (Point { x: 5000, y: 5000 }, Corner::BottomRight),
    ] {
        let mut g = Gesture::NEW;
        let origin = Corner::TopRight.anchor(48);
        g.press(Point { x: 300, y: 36 }, origin, u32::MAX - 100);
        let p = g.motion(touch, 400, 48).unwrap();
        assert_eq!(p, model::clamp_position(p, 48));
        let dock = g.release(touch, 416, 48).unwrap();
        assert_eq!(dock.anchor(48), wanted.anchor(48));
        assert!(!g.can_click());
    }
    let mut g = Gesture::NEW;
    g.press(Point::default(), Point::default(), 0);
    assert_eq!(g.motion(Point { x: 10, y: 0 }, 20, 48), None);
    g.hold();
    assert_eq!(g.release(Point { x: 200, y: 200 }, 1000, 48), None);
    assert!(!g.can_click());
}

#[test]
fn easing_is_continuous_monotonic_bounded_and_survives_clock_wrap() {
    let t = Tween {
        from: 0,
        to: 1000,
        started: u32::MAX - 80,
        duration: 180,
    };
    let mut previous = 0;
    for ms in (0..=192).step_by(16) {
        let at = t.started.wrapping_add(ms);
        let value = t.value(at);
        assert!(value >= previous && value <= 1000);
        previous = value;
    }
    assert_eq!(previous, 1000);
    let reverse = Tween {
        from: t.value(9),
        to: 0,
        started: 9,
        duration: 180,
    };
    assert_eq!(reverse.value(9), t.value(9));
    assert_eq!(reverse.value(189), 0);
}

#[test]
fn flat_ball_has_clear_solid_materials_real_alpha_and_no_glow_or_shadow() {
    let mut rgba = [0; pixels::BYTES];
    pixels::render(&mut rgba, 1);
    assert_eq!(&rgba[..4], &[0; 4]);
    assert!(rgba.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 200));
    for p in rgba.chunks_exact(4) {
        assert!(p[..3].iter().all(|&v| v <= p[3]));
    }
    pixels::render(&mut rgba, 0);
    assert_eq!(
        rgba[(pixels::SIDE * (pixels::SIDE / 2) + pixels::SIDE / 2) * 4 + 3],
        255
    );
    for opacity in [0u8, 1] {
        pixels::render(&mut rgba, opacity);
        let pixel = |x, y| &rgba[(y * pixels::SIDE + x) * 4..(y * pixels::SIDE + x) * 4 + 4];
        let material = pixel(8, 32);
        for (x, y) in [(10, 20), (20, 10), (32, 8), (44, 12), (54, 24), (14, 47)] {
            assert_eq!(
                pixel(x, y),
                material,
                "solid progress material gained a highlight/gradient"
            );
        }
        let background = pixel(32, 43);
        for (x, y) in [(16, 32), (46, 33), (31, 19)] {
            assert_eq!(
                pixel(x, y),
                background,
                "flat disk gained a shaded edge or highlight"
            );
        }
        for (x, y) in [(0, 32), (63, 32), (32, 0), (32, 63)] {
            let edge = pixel(x, y);
            for c in 0..3 {
                assert!(
                    (edge[c] as i32 * background[3] as i32 - background[c] as i32 * edge[3] as i32)
                        .abs()
                        <= 255,
                    "edge gained a highlight"
                );
            }
        }
        assert!(
            material[0] as i32 - background[0] as i32 >= 140,
            "battery contrast is too low"
        );
        for y in 0..pixels::SIDE {
            for x in 0..pixels::SIDE {
                let dx = x as i32 * 2 + 1 - 64;
                let dy = y as i32 * 2 + 1 - 64;
                if dx * dx + dy * dy >= 65 * 65 {
                    assert_eq!(pixel(x, y), &[0; 4], "shadow/glow extends outside ball");
                }
            }
        }
    }
}

#[test]
fn closed_overlay_passes_background_touches_and_survives_foreground_changes() {
    reset();
    step(0);
    world(|w| assert_eq!(w.objects.len(), 1));
    enabled();
    let root = tagged(port::ROOT);
    let ball = tagged(port::BALL);
    world(|w| {
        assert_eq!(w.objects[&root].parent, 1);
        assert_eq!(w.objects[&root].flags & 2, 0);
        assert_eq!(w.objects[&ball].position, (276, 12));
        assert_eq!(
            w.objects[&tagged_without_borrow(w, port::DISMISS)].flags & 1,
            1
        );
    });
    let unrelated = crate::allocate(1);
    step(250);
    world(|w| {
        assert!(w.objects.contains_key(&unrelated));
        assert!(w.objects.contains_key(&root));
        assert!(w.deleted.is_empty());
    });
}
fn tagged_without_borrow(w: &World, tag: u32) -> u32 {
    *w.objects
        .iter()
        .find(|(_, o)| o.events.iter().any(|&(_, t)| t == tag))
        .unwrap()
        .0
}

#[test]
fn time_is_inline_in_the_fan_and_does_not_open_another_page() {
    reset();
    enabled();
    open();
    let publications = world(|w| w.image_publications);
    choose(0);
    step(0);
    unsafe {
        assert_eq!(VIEW, View::Fan);
        assert!(PENDING.is_none());
    }
    step(60_000);
    world(|w| {
        assert!(
            w.image_publications > publications,
            "inline clock glyph texture did not update"
        )
    });
    unsafe {
        assert_eq!(VIEW, View::Fan);
    }
}

#[test]
fn animation_runs_at_16ms_without_allocations_file_io_or_clock_formatting() {
    reset();
    enabled();
    open();
    click(port::DISMISS);
    let before = world(|w| (w.allocations, w.reads, w.writes, w.clock_reads));
    animate(16);
    unsafe {
        assert!(FAN.value(world(|w| w.now)) > 0);
    }
    world(|w| assert!(w.timers.values().any(|&ms| ms == 16)));
    for _ in 0..12 {
        animate(16);
    }
    world(|w| {
        assert_eq!((w.allocations, w.reads, w.writes, w.clock_reads), before);
        assert!(w.timers.values().all(|&ms| ms == 1000));
        assert_eq!(
            w.objects[&tagged_without_borrow(w, port::DISMISS)].flags & 1,
            1
        );
    });
}

#[test]
fn drag_snaps_to_nearest_edge_point_saves_once_and_suppresses_click() {
    reset();
    enabled();
    send(1, port::BALL, Point { x: 300, y: 36 });
    world(|w| w.now += 500);
    send(5, port::BALL, Point { x: 300, y: 36 });
    send(2, port::BALL, Point { x: 60, y: 420 });
    animate(16);
    let dragged = world(|w| w.objects[&tagged_without_borrow(w, port::BALL)].position);
    assert_ne!(dragged, (276, 12));
    send(8, port::BALL, Point { x: 60, y: 420 });
    click(port::BALL);
    animate(208);
    unsafe {
        assert_eq!(VIEW, View::Closed);
        assert_eq!(config().corner(), Corner::BottomLeft);
    }
    step(0);
    let ball = tagged(port::BALL);
    world(|w| {
        assert_eq!(w.objects[&ball].position, (12, 396));
        assert!(w.handles.is_empty());
    });
    let writes = world(|w| w.writes);
    step(1000);
    world(|w| assert_eq!(w.writes, writes));
    // A second drag starts from the settled edge point, without an old tween pulling it back.
    send(1, port::BALL, Point { x: 36, y: 444 });
    world(|w| w.now += 500);
    send(5, port::BALL, Point { x: 36, y: 444 });
    send(2, port::BALL, Point { x: 100, y: 300 });
    animate(16);
    world(|w| assert_eq!(w.objects[&ball].position, (76, 252)));
}

#[test]
fn brightness_adjustment_is_fine_clamped_and_leaves_automatic_mode() {
    reset();
    enabled();
    open();
    choose(2);
    step(0);
    animate(192);
    let c = unsafe { config() };
    let (_, outer) = model::band_radii(c.size);
    let p = model::inward_point(c.corner(), c.size, 70, 70);
    send(1, port::BAND, p);
    step(0);
    send(5, port::BAND, p);
    let previous = unsafe { SLIDER_VALUE };
    let moved = model::inward_point(c.corner(), c.size, 70, 71);
    send(2, port::BAND, moved);
    step(0);
    world(|w| {
        assert_eq!(w.brightness, previous + 1);
        assert_eq!(w.auto, 0);
    });
    send(8, port::BAND, moved);
    step(32);
    let before = world(|w| w.brightness);
    let start = c.inward_point(outer - 20, 0);
    let end = c.inward_point(0, outer - 20);
    send(1, port::BAND, start);
    step(32);
    world(|w| assert_eq!(w.brightness, before));
    send(2, port::BAND, end);
    send(8, port::BAND, end);
    step(32);
    world(|w| assert_eq!(w.brightness, 2000));
    send(1, port::BAND, end);
    send(2, port::BAND, start);
    send(8, port::BAND, start);
    step(32);
    world(|w| assert_eq!(w.brightness, 1));
    world(|w| {
        assert!(w.objects.values().all(|o| o
            .events
            .iter()
            .all(|&(_, tag)| tag != port::MINUS_ONE && tag != port::PLUS_ONE)))
    });
}

#[test]
fn keep_awake_restores_timeout_on_toggle_disable_or_manual_screen_off() {
    for reason in 0..3 {
        reset();
        enabled();
        open();
        start_keep(0);
        unsafe {
            assert!(keep_awake());
            assert_eq!(VIEW, View::Fan);
        }
        world(|w| assert_eq!(w.timeout, MAX_TIMEOUT));
        match reason {
            0 => {
                choose(1);
                step(0);
                animate(192);
                choose(0);
            }
            1 => unsafe {
                let mut c = config();
                c.enabled = false;
                update(c);
            },
            _ => world(|w| w.on = false),
        }
        step(0);
        world(|w| assert_eq!(w.timeout, 15_000));
        unsafe {
            assert!(!keep_awake());
        }
    }
}

#[test]
fn system_timeout_change_takes_precedence_over_an_existing_keep_awake_lease() {
    reset();
    enabled();
    open();
    start_keep(0);
    world(|w| w.timeout = 25_000);
    step(250);
    unsafe {
        assert!(!keep_awake());
    }
    unsafe {
        let mut c = config();
        c.enabled = false;
        update(c);
    }
    step(0);
    world(|w| assert_eq!(w.timeout, 25_000));
}

#[test]
fn screenshot_hides_whole_overlay_before_capture_and_restores_after_completion() {
    reset();
    enabled();
    unsafe {
        let mut c = config();
        c.assign(2, Action::Screenshot);
        update(c);
    }
    step(0);
    open();
    click(port::ACTION + 2);
    step(0);
    let root = tagged(port::ROOT);
    world(|w| {
        assert_eq!(w.shot_requests, 0);
        assert_eq!(w.objects[&root].flags & 1, 1);
    });
    step(149);
    world(|w| assert_eq!(w.shot_requests, 0));
    step(1);
    world(|w| {
        assert_eq!(w.shot_requests, 1);
        assert_eq!(w.objects[&root].flags & 1, 1);
        w.shot_state = 2;
    });
    step(250);
    world(|w| {
        assert_eq!(w.objects[&root].flags & 1, 1);
        w.shot_state = 3;
    });
    step(250);
    world(|w| {
        assert_eq!(w.objects[&root].flags & 1, 0);
        assert_eq!(w.shot_requests, 1);
    });
    unsafe {
        assert_eq!(VIEW, View::Message);
    }
}

#[test]
fn power_off_cancels_unqueued_screenshot_without_capturing_on_wake() {
    reset();
    enabled();
    unsafe {
        let mut c = config();
        c.assign(2, Action::Screenshot);
        update(c);
    }
    step(0);
    open();
    click(port::ACTION + 2);
    step(0);
    world(|w| w.on = false);
    step(200);
    world(|w| w.on = true);
    step(200);
    world(|w| assert_eq!(w.shot_requests, 0));
}

#[test]
fn native_tree_loss_recreates_only_our_layer_and_disable_removes_it() {
    reset();
    enabled();
    let root = tagged(port::ROOT);
    crate::remove_tree(root);
    step(250);
    assert_ne!(root, tagged(port::ROOT));
    unsafe {
        let mut c = config();
        c.enabled = false;
        update(c);
    }
    step(0);
    world(|w| {
        assert_eq!(w.objects.len(), 1);
        assert!(w.handles.is_empty());
    });
}

#[test]
fn short_io_round_trip_and_failed_saves_keep_the_last_complete_config() {
    reset();
    enabled();
    let original = world(|w| w.files["/data/chaos/floating-ball.cfg"].clone());
    unsafe {
        let mut c = config();
        c.size = SIZES[5];
        assert!(update(c));
    }
    world(|w| w.write_budget = 3);
    step(0);
    unsafe {
        assert!(save_failed());
    }
    world(|w| {
        assert_eq!(w.files["/data/chaos/floating-ball.cfg"], original);
        assert!(!w.files.contains_key("/data/chaos/floating-ball.cfg.tmp"));
        assert!(w.handles.is_empty());
    });
    unsafe {
        reset_for_test();
    }
    unsafe {
        assert_eq!(config().size, 48);
        assert!(config().enabled);
    }
    world(|w| {
        w.write_budget = usize::MAX;
        w.rename_error = -13;
    });
    unsafe {
        let mut c = config();
        c.opacity = 0;
        update(c);
    }
    step(0);
    world(|w| {
        assert_eq!(w.files["/data/chaos/floating-ball.cfg"], original);
        assert!(w.handles.is_empty());
    });
}

#[test]
fn arc_sectors_and_progress_follow_the_same_geometry_in_all_four_corners() {
    for size in SIZES {
        for corner in Corner::ALL {
            let points = model::fan_centers(corner, size, 1000);
            for i in 0..3 {
                assert_eq!(model::band_sector(corner, size, points[i]), Some(i));
            }
            let (inner, outer) = model::band_radii(size);
            assert_eq!(
                model::band_sector(corner, size, model::inward_point(corner, size, 0, 0)),
                None
            );
            assert_eq!(
                model::band_sector(
                    corner,
                    size,
                    model::inward_point(corner, size, outer + 1, 0)
                ),
                None
            );
            let mut previous = 0;
            for v in 0..=100 {
                let p = model::inward_point(corner, size, 100 - v, v);
                let progress = model::band_progress(corner, size, p);
                assert!(progress >= previous);
                previous = progress;
            }
            assert_eq!(previous, 1000);
            assert!(inner > size as i32 / 2);
        }
    }
    assert_eq!(model::slider_brightness(0), 1);
    assert_eq!(model::slider_brightness(1000), 2000);
}
#[test]
fn duration_selection_has_three_presets_and_expiry_restores_timeout_even_at_clock_wrap() {
    for index in 0..3 {
        reset();
        enabled();
        open();
        world(|w| w.now = u32::MAX - 2000);
        start_keep(index);
        let started = unsafe { KEEP_STARTED };
        let duration = model::KEEP_MINUTES[index] * 60_000;
        unsafe {
            assert_eq!(KEEP_DURATION, duration);
        }
        world(|w| w.now = started.wrapping_add(duration - 1));
        step(0);
        unsafe {
            assert!(keep_awake());
        }
        step(1);
        unsafe {
            assert!(!keep_awake());
        }
        world(|w| assert_eq!(w.timeout, 15_000));
    }
}
#[test]
fn choosing_a_different_duration_restarts_the_timer_without_losing_original_timeout() {
    reset();
    enabled();
    open();
    start_keep(0);
    step(120_000);
    choose(1);
    step(0);
    animate(192);
    choose(2);
    step(0);
    animate(192);
    let started = unsafe { KEEP_STARTED };
    unsafe {
        assert_eq!(KEEP_DURATION, 900_000);
        assert_eq!(TIMEOUT_LEASE, Some(15_000));
    }
    world(|w| w.now = started.wrapping_add(900_000));
    step(0);
    world(|w| assert_eq!(w.timeout, 15_000));
}
#[test]
fn brightness_arc_drag_tracks_input_and_hold_sliding_adjusts_one_nit() {
    for corner in Corner::ALL {
        reset();
        enabled();
        unsafe {
            let mut c = config();
            c.dock = Dock::from_corner(corner);
            update(c);
        }
        step(0);
        open();
        choose(2);
        step(0);
        animate(192);
        let c = unsafe { config() };
        let p = model::fan_centers(corner, c.size, 1000)[1];
        send(1, port::BAND, p);
        let (_, outer) = model::band_radii(c.size);
        let r = outer - 25;
        send(2, port::BAND, model::inward_point(corner, c.size, 0, r));
        animate(16);
        unsafe {
            assert!((1590..=1610).contains(&SLIDER_VALUE));
        }
        step(16);
        world(|w| {
            assert!((1590..=1610).contains(&w.brightness));
            assert_eq!(w.auto, 0);
        });
        send(8, port::BAND, model::inward_point(corner, c.size, r, 0));
        step(32);
        world(|w| assert_eq!(w.brightness, 1));
        let p = model::inward_point(corner, c.size, 70, 70);
        send(1, port::BAND, p);
        step(0);
        send(5, port::BAND, p);
        let original = unsafe { SLIDER_VALUE };
        send(2, port::BAND, model::inward_point(corner, c.size, 70, 71));
        step(32);
        world(|w| assert_eq!(w.brightness, original + 1));
    }
}
#[test]
fn secondary_menus_return_to_the_primary_arc_when_the_ball_is_tapped() {
    for index in [1, 2] {
        reset();
        enabled();
        open();
        choose(index);
        step(0);
        animate(192);
        unsafe {
            assert_eq!(
                VIEW,
                if index == 1 {
                    View::Duration
                } else {
                    View::Brightness
                }
            );
        }
        open();
        unsafe {
            assert_eq!(VIEW, View::Fan);
        }
    }
}
#[test]
fn band_raster_is_a_continuous_mirrored_annulus_with_real_white_slider_fill() {
    use model::BandStyle;
    let mut left = [0; pixels::BAND_BYTES];
    let mut right = [0; pixels::BAND_BYTES];
    let base = BandStyle {
        corner: Corner::TopLeft,
        size: 48,
        opacity: 1,
        brightness: None,
        highlight: None,
    };
    pixels::render_band(&mut left, base);
    pixels::render_band(
        &mut right,
        BandStyle {
            corner: Corner::TopRight,
            ..base
        },
    );
    let n = pixels::BAND_SIDE;
    for y in 0..n {
        for x in 0..n {
            assert_eq!(
                &left[(y * n + x) * 4..(y * n + x) * 4 + 4],
                &right[(y * n + n - 1 - x) * 4..(y * n + n - 1 - x) * 4 + 4]
            );
        }
    }
    assert_eq!(left[3], 0);
    assert!(left.chunks_exact(4).filter(|p| p[3] > 0).count() > n * n / 3);
    pixels::render_band(
        &mut right,
        BandStyle {
            brightness: Some(1000),
            ..base
        },
    );
    assert!(
        right
            .chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200)
            .count()
            > n * n / 3
    );
    for p in right.chunks_exact(4) {
        assert!(p[..3].iter().all(|&v| v <= p[3]));
    }
}
#[test]
fn gpu_busy_defers_band_publication_and_resumes_without_leaking_or_file_io() {
    reset();
    enabled();
    world(|w| w.gpu_idle = false);
    open();
    let before = world(|w| (w.allocations, w.reads, w.writes));
    animate(200);
    world(|w| assert!(w.timers.values().any(|&p| p == 16)));
    world(|w| w.gpu_idle = true);
    animate(16);
    world(|w| {
        assert!(w.timers.values().all(|&p| p == 1000));
        assert_eq!((w.allocations, w.reads, w.writes), before);
    });
}

#[test]
fn rounded_black_band_has_no_outline_and_separators_stop_before_both_edges() {
    use model::BandStyle;
    let mut painter = pixels::BandPainter::NEW;
    let mut out = [0; pixels::BAND_BYTES];
    let style = BandStyle {
        corner: Corner::TopLeft,
        size: 48,
        opacity: 1,
        brightness: None,
        highlight: None,
    };
    painter.render(&mut out, style);
    let (inner, outer) = model::band_radii(48);
    let pixel = |r: i32, co: i32, si: i32| {
        let x = (r * co * 128 / (outer * 1000)).clamp(0, 127) as usize;
        let y = (r * si * 128 / (outer * 1000)).clamp(0, 127) as usize;
        let a = (y * 128 + x) * 4;
        [out[a], out[a + 1], out[a + 2], out[a + 3]]
    };
    let center = pixel((inner + outer) / 2, 966, 259);
    assert_eq!(&center[..3], &[0; 3]);
    assert!(center[3] > 100 && center[3] < 220);
    for r in [inner + 3, outer - 3] {
        let p = pixel(r, 866, 500);
        assert!(p[0] <= 3, "separator or outline reaches the arc boundary");
    }
    assert!(pixel((inner + outer) / 2, 866, 500)[0] > 50);
    let mask = painter.mask(48).unwrap();
    assert_eq!(mask[127], 0, "outer end corner is square");
    assert!(model::band_sector(
        Corner::TopLeft,
        48,
        model::inward_point(Corner::TopLeft, 48, outer - 1, 1)
    )
    .is_none());
}

#[test]
fn mask_codec_rejects_truncated_and_overlong_runs() {
    let mut out = [0; 3];
    assert!(!pixels::decode(&[0x82], &mut out));
    assert!(!pixels::decode(&[0x83, 255], &mut out));
    assert!(!pixels::decode(&[2, 1, 2], &mut out));
    assert!(pixels::decode(&[0x82, 255], &mut out));
    assert_eq!(out, [255; 3]);
}

#[test]
fn letters_follow_the_tangent_instead_of_forming_horizontal_labels() {
    use curved::{Kind, Style, PALETTE_BYTES};
    let mut painter = pixels::BandPainter::NEW;
    let mask = painter.mask(48).unwrap();
    let mut text = [0; curved::BYTES];
    curved::render(
        &mut text,
        Style {
            corner: Corner::TopRight,
            size: 48,
            kind: Kind::Main,
            slots: Config::DEFAULT.slots,
            clock: *b"12:34\0",
            brightness: 0,
        },
        mask,
    );
    let (mut xmin, mut xmax, mut ymin, mut ymax) = (128, 0, 128, 0);
    for y in 0..128 {
        for x in 0..128 {
            let u = 127 - x;
            let v = y;
            if text[PALETTE_BYTES + y * 128 + x] & 127 > 8 && v * 1000 < u * 577 {
                xmin = xmin.min(x);
                xmax = xmax.max(x);
                ymin = ymin.min(y);
                ymax = ymax.max(y);
            }
        }
    }
    assert!(
        ymax - ymin > xmax - xmin + 5,
        "clock remains a horizontal row"
    );
}

#[test]
fn all_curved_text_stays_inside_the_rounded_band_at_all_sizes_and_corners() {
    use curved::{Kind, Style, PALETTE_BYTES};
    let mut painter = pixels::BandPainter::NEW;
    for size in SIZES {
        for corner in Corner::ALL {
            for kind in [Kind::Main, Kind::Duration, Kind::Brightness] {
                let mask = painter.mask(size).unwrap();
                let mut out = [0; curved::BYTES];
                curved::render(
                    &mut out,
                    Style {
                        corner,
                        size,
                        kind,
                        slots: Config::DEFAULT.slots,
                        clock: *b"23:59\0",
                        brightness: 1200,
                    },
                    mask,
                );
                let mut count = 0;
                for y in 0..128 {
                    for x in 0..128 {
                        if out[PALETTE_BYTES + y * 128 + x] & 127 != 0 {
                            let cx = if corner.right() { 127 - x } else { x };
                            let cy = if corner.bottom() { 127 - y } else { y };
                            assert!(mask[cy * 128 + cx] > 0);
                            count += 1;
                        }
                    }
                }
                assert!(count > 150, "curved text is empty or clipped away");
            }
        }
    }
}

#[test]
fn brightness_text_uses_dark_ink_over_white_fill_and_light_ink_over_black() {
    use curved::{Kind, Style, PALETTE_BYTES};
    let mut p = pixels::BandPainter::NEW;
    let mask = p.mask(48).unwrap();
    for (nit, dark) in [(1, false), (2000, true)] {
        let mut out = [0; curved::BYTES];
        curved::render(
            &mut out,
            Style {
                corner: Corner::TopRight,
                size: 48,
                kind: Kind::Brightness,
                slots: Config::DEFAULT.slots,
                clock: [0; 6],
                brightness: nit,
            },
            mask,
        );
        assert!(out[PALETTE_BYTES..].iter().any(|&i| i & 127 != 0));
        assert!(out[PALETTE_BYTES..]
            .iter()
            .filter(|&&i| i & 127 != 0)
            .all(|&i| (i >= 128) == dark));
    }
}

#[test]
fn curved_text_is_readable_after_corner_changes_instead_of_mirroring_letters() {
    use curved::{Kind, Style};
    let mut p = pixels::BandPainter::NEW;
    let mask = p.mask(48).unwrap();
    let base = Style {
        corner: Corner::TopRight,
        size: 48,
        kind: Kind::Main,
        slots: Config::DEFAULT.slots,
        clock: *b"12:34\0",
        brightness: 0,
    };
    let (mut a, mut b) = ([0; curved::BYTES], [0; curved::BYTES]);
    curved::render(&mut a, base, mask);
    curved::render(
        &mut b,
        Style {
            corner: Corner::TopLeft,
            ..base
        },
        mask,
    );
    let mut different = 0;
    for y in 0..128 {
        for x in 0..128 {
            different += (a[1024 + y * 128 + x] != b[1024 + y * 128 + 127 - x]) as usize;
        }
    }
    assert!(different > 100, "corner changes mirror the letter shapes");
}

#[test]
fn edge_projection_preserves_the_nearest_point_instead_of_moving_to_a_corner() {
    for size in model::MIN_SIZE..=model::MAX_SIZE {
        for (position, edge) in [
            (Point { x: 40, y: 185 }, Edge::Left),
            (
                Point {
                    x: WIDTH - size as i32 - 31,
                    y: 185,
                },
                Edge::Right,
            ),
            (Point { x: 115, y: 31 }, Edge::Top),
            (
                Point {
                    x: 115,
                    y: HEIGHT - size as i32 - 31,
                },
                Edge::Bottom,
            ),
        ] {
            let dock = Dock::nearest(position, size);
            assert_eq!(dock.edge, edge);
            let target = dock.anchor(size);
            if matches!(edge, Edge::Left | Edge::Right) {
                assert!((target.y - position.y).abs() <= 1);
            } else {
                assert!((target.x - position.x).abs() <= 1);
            }
        }
    }
}
#[test]
fn every_edge_position_keeps_the_full_arc_visible_and_hit_targets_aligned() {
    for size in model::MIN_SIZE..=model::MAX_SIZE {
        for edge in Edge::ALL {
            for offset in (0..=1000).step_by(25) {
                let c = Config {
                    size,
                    dock: Dock { edge, offset },
                    ..Config::DEFAULT
                };
                let (_, outer) = model::band_radii(size);
                for (u, v) in [(0, 0), (outer, 0), (0, outer), (outer, outer)] {
                    let p = c.inward_point(u, v);
                    assert!(
                        (0..WIDTH).contains(&p.x) && (0..HEIGHT).contains(&p.y),
                        "arc clips {c:?} {p:?}"
                    );
                }
                for (i, point) in c.fan_centers().iter().enumerate() {
                    assert_eq!(c.band_sector(*point), Some(i));
                }
                assert_eq!(Config::decode(&c.encode()), Some(c));
            }
        }
    }
}
#[test]
fn dot1_settings_migrate_without_losing_slots_or_the_previous_corner() {
    for corner in Corner::ALL {
        reset();
        let mut old = [b'D', b'O', b'T', b'1', 1, 56, 1, corner as u8, 0, 3, 2, 0];
        old[11] = old[..11].iter().fold(0xa5, |sum, b| sum ^ b);
        world(|w| {
            w.files.insert(cstr(FILE.as_ptr()), old.to_vec());
        });
        step(0);
        unsafe {
            let c = config();
            assert_eq!(c.anchor(), corner.anchor(56));
            assert_eq!(
                c.slots,
                [Action::Time, Action::KeepAwake, Action::Screenshot]
            );
            let mut next = c;
            next.dock = Dock {
                edge: Edge::Right,
                offset: 437,
            };
            assert!(update(next));
        }
        step(0);
        world(|w| {
            assert_eq!(&w.files[&cstr(FILE.as_ptr())][..4], b"DOT2");
        });
        unsafe {
            LOADED = false;
            load();
            assert_eq!(CONFIG.dock.offset, 437);
        }
    }
}
#[test]
fn battery_progress_uses_the_broad_flat_inner_stroke_and_keeps_the_edge_fixed() {
    let mut zero = [0; pixels::BYTES];
    let mut quarter = [0; pixels::BYTES];
    let mut half = [0; pixels::BYTES];
    let mut full = [0; pixels::BYTES];
    let mut reference = [0; pixels::BYTES];
    let pixel = |x, y| (y * pixels::SIDE + x) * 4;
    for opacity in [1u8, 0] {
        pixels::render(&mut reference, opacity);
        pixels::render_battery(&mut zero, opacity, None);
        pixels::render_battery(&mut quarter, opacity, Some(25));
        pixels::render_battery(&mut half, opacity, Some(50));
        pixels::render_battery(&mut full, opacity, Some(100));
        assert_eq!(
            full, reference,
            "full battery must match the flat full-stroke artwork"
        );
        let lower_left = pixel(14, 47);
        assert!(quarter[lower_left] > zero[lower_left]);
        let upper_left = pixel(14, 16);
        assert_eq!(quarter[upper_left], zero[upper_left]);
        assert!(half[upper_left] > quarter[upper_left]);
        let top = pixel(44, 12);
        assert_eq!(half[top], zero[top]);
        assert!(full[top] > half[top]);
        // Width is visibly broad at native resolution, inside the bevel.
        let changed = (0..32)
            .filter(|&x| zero[pixel(x, 32)] != full[pixel(x, 32)])
            .count();
        assert!(
            changed >= 8,
            "battery material shrank back to a thin ring: {changed}"
        );
        for data in [&zero, &quarter, &half, &full] {
            assert_eq!(&data[..4], &[0; 4]);
            for y in 0..pixels::SIDE {
                for x in 0..pixels::SIDE {
                    let at = pixel(x, y);
                    let p = &data[at..at + 4];
                    assert!(p[..3].iter().all(|&v| v <= p[3]));
                    assert_eq!(
                        p[3],
                        reference[at + 3],
                        "battery updates must preserve the selected opacity"
                    );
                    let dx = x as i32 * 2 + 1 - 64;
                    let dy = y as i32 * 2 + 1 - 64;
                    if dx * dx + dy * dy >= 62 * 62 || dx * dx + dy * dy <= 30 * 30 {
                        assert_eq!(
                            p,
                            &reference[at..at + 4],
                            "rim or central battery icon changed"
                        );
                    }
                }
            }
        }
    }
    reset();
    enabled();
    let publications = world(|w| w.image_publications);
    world(|w| w.battery = Some(24));
    step(5000);
    world(|w| assert_eq!(w.image_publications, publications + 1));
    let publications = world(|w| w.image_publications);
    step(5000);
    world(|w| assert_eq!(w.image_publications, publications));
    let reads = world(|w| w.battery_reads);
    for _ in 0..20 {
        animate(16);
    }
    world(|w| assert_eq!(w.battery_reads, reads));
}
#[test]
fn screenshot_results_automatically_disappear_after_two_seconds_even_at_clock_wrap() {
    for success in [true, false] {
        reset();
        enabled();
        unsafe {
            let mut c = config();
            c.assign(0, Action::Screenshot);
            update(c);
        }
        step(0);
        open();
        choose(0);
        step(0);
        step(160);
        world(|w| {
            w.now = u32::MAX - 900;
            w.shot_state = if success { 3 } else { 0 };
        });
        step(0);
        unsafe {
            assert_eq!(VIEW, View::Message);
        }
        step(1999);
        unsafe {
            assert_eq!(VIEW, View::Message);
        }
        step(1);
        unsafe {
            assert_eq!(VIEW, View::Closed);
        }
        let root = tagged(port::ROOT);
        world(|w| assert_eq!(w.objects[&root].flags & 1, 0));
    }
}
#[test]
fn practical_shortcuts_dispatch_from_any_slot_and_report_the_result() {
    for (action, expected) in [
        (Action::Watchface, ""),
        (Action::Vibration, "震动已关闭"),
        (Action::AutoBrightness, "自动亮度已关闭"),
        (Action::Recording, "正在启动录屏"),
        (Action::Rotate, "已翻转屏幕"),
    ] {
        reset();
        enabled();
        unsafe {
            let mut c = config();
            c.assign(0, action);
            update(c);
        }
        step(0);
        open();
        if action == Action::Watchface {
            world(|w| {
                w.home_app = 200;
                w.home_depth = 2;
                w.home_watchface = false;
                w.home_step = true;
            });
        }
        choose(0);
        world(|w| {
            assert_eq!(w.home_requests, 0);
            assert!(!w.muted);
        });
        step(0);
        if action == Action::Watchface {
            world(|w| assert_eq!(w.home_requests, 1));
            unsafe {
                assert_eq!(VIEW, View::Closed);
            }
        } else {
            assert!(has_text(expected), "missing shortcut feedback: {expected}");
            step(2000);
            unsafe {
                assert_eq!(VIEW, View::Closed);
            }
        }
        if action == Action::Vibration {
            open();
            choose(0);
            step(0);
            assert!(has_text("震动已开启"));
            world(|w| assert!(!w.muted));
        }
    }
}
#[test]
fn brightness_does_not_jump_on_touch_and_hardware_updates_are_coalesced() {
    reset();
    enabled();
    open();
    choose(2);
    step(0);
    animate(192);
    let c = unsafe { config() };
    let p = c.fan_centers()[1];
    send(1, port::BAND, p);
    step(32);
    world(|w| {
        assert_eq!(w.brightness, 600);
        assert_eq!(w.auto, 1);
        assert_eq!(w.brightness_writes, 0);
    });
    for y in 70..90 {
        send(2, port::BAND, c.inward_point(70, y));
    }
    animate(16);
    let first = world(|w| w.brightness_writes);
    assert_eq!(first, 1);
    send(2, port::BAND, c.inward_point(70, 100));
    animate(16);
    world(|w| assert_eq!(w.brightness_writes, first));
    animate(16);
    world(|w| {
        assert_eq!(w.brightness_writes, first + 1);
        assert!(w.brightness > 600);
    });
    send(5, port::BAND, c.inward_point(70, 100));
    unsafe {
        assert!(
            !SLIDER_FINE,
            "moving a held slider must not unexpectedly enter fine mode"
        );
    }
    send(2, port::BAND, c.inward_point(70, 110));
    send(8, port::BAND, c.inward_point(70, 110));
    animate(16);
    world(|w| assert!(w.timers.values().any(|&period| period == 16)));
    animate(16);
    unsafe {
        assert!(PENDING_BRIGHTNESS.is_none());
    }
}
#[test]
fn all_additional_shortcuts_have_visible_curved_glyphs() {
    let mut painter = pixels::BandPainter::NEW;
    for action in Action::ALL {
        for corner in Corner::ALL {
            let mut text = [0; curved::BYTES];
            curved::render(
                &mut text,
                curved::Style {
                    corner,
                    size: 32,
                    kind: curved::Kind::Main,
                    slots: [action; 3],
                    clock: *b"12:34\0",
                    brightness: 600,
                },
                painter.mask(32).unwrap(),
            );
            assert!(
                text[curved::PALETTE_BYTES..].iter().any(|&p| p & 127 > 32),
                "missing {action:?}"
            );
        }
    }
}

#[test]
fn unavailable_native_home_service_reports_a_short_nonblocking_message() {
    reset();
    enabled();
    unsafe {
        let mut c = config();
        c.assign(0, Action::Watchface);
        update(c);
    }
    step(0);
    open();
    world(|w| {
        w.home_available = false;
        w.home_app = 200;
        w.home_depth = 2;
        w.home_watchface = false;
    });
    choose(0);
    step(0);
    assert!(has_text("返回表盘暂不可用"));
    step(2000);
    unsafe {
        assert_eq!(VIEW, View::Closed);
    }
}

fn request_nested_home(stepwise: bool) {
    reset();
    enabled();
    unsafe {
        let mut c = config();
        c.assign(0, Action::Watchface);
        update(c);
    }
    step(0);
    open();
    world(|w| {
        w.home_app = 200;
        w.home_depth = 4;
        w.home_watchface = false;
        w.home_step = stepwise;
    });
    choose(0);
    step(0);
}
#[test]
fn return_to_watchface_continues_after_a_single_page_return_until_the_watchface_is_front() {
    request_nested_home(true);
    world(|w| {
        assert_eq!(w.home_requests, 1);
        assert_eq!(w.home_depth, 3);
        assert!(!w.home_watchface);
    });
    unsafe {
        assert!(HOME_STARTED.is_some());
        assert!(busy());
    }
    world(|w| w.home_depth = 0);
    step(200);
    unsafe {
        assert!(HOME_STARTED.is_some());
    }
    world(|w| w.home_depth = 3);
    step(400);
    step(600);
    step(1);
    world(|w| {
        assert_eq!(w.home_requests, 3);
        assert_eq!(w.home_depth, 1);
        assert!(w.home_watchface);
    });
    unsafe {
        assert!(HOME_STARTED.is_none());
    }
    step(1200);
    world(|w| assert_eq!(w.home_requests, 3));
    open();
    choose(0);
    step(0);
    world(|w| assert_eq!(w.home_requests, 3));
}
#[test]
fn pending_home_return_times_out_and_cancels_when_user_opens_a_new_page_or_turns_screen_off() {
    for reason in 0..3 {
        request_nested_home(false);
        match reason {
            0 => {
                step(8000);
                assert!(has_text("返回表盘未完成，请重试"));
            }
            1 => {
                world(|w| w.home_depth = 5);
                step(600);
            }
            _ => {
                world(|w| w.on = false);
                step(600);
            }
        }
        unsafe {
            assert!(HOME_STARTED.is_none());
        }
        step(1200);
        world(|w| assert_eq!(w.home_requests, 1));
    }
}

#[test]
fn battery_circle_is_complete_and_the_hollow_dot_tracks_the_shrinking_end() {
    let mut full = [0; pixels::BYTES];
    pixels::render_battery(&mut full, 0, Some(100));
    let pixel = |x, y| (y * pixels::SIDE + x) * 4;
    let material = full[pixel(8, 32)..pixel(8, 32) + 4].to_vec();
    for (x, y) in [(47, 50), (40, 54)] {
        assert_eq!(
            &full[pixel(x, y)..pixel(x, y) + 4],
            material.as_slice(),
            "circle still has the old open gap"
        );
    }
    let positions = [(8, 32), (32, 8), (56, 32), (32, 56)];
    for (i, percent) in [25, 50, 75, 100].iter().enumerate() {
        let mut out = [0; pixels::BYTES];
        pixels::render_battery(&mut out, 0, Some(*percent));
        let background = &out[pixel(32, 43)..pixel(32, 43) + 4];
        for (j, (x, y)) in positions.iter().enumerate() {
            let marker = &out[pixel(*x, *y)..pixel(*x, *y) + 4];
            if i == j {
                assert_eq!(
                    marker, background,
                    "dot failed to move to the progress endpoint"
                );
            } else {
                assert_ne!(marker, background, "a second, fixed dot was left behind");
            }
        }
    }
}

#[test]
fn full_battery_track_remains_joined_at_both_edges_for_every_progress_state() {
    for percent in [0, 25, 50, 75, 100] {
        let mut out = [0; pixels::BYTES];
        pixels::render_battery(&mut out, 0, Some(percent));
        let background = out[(43 * pixels::SIDE + 32) * 4];
        // The small marker hole must never sever either side of the ring.
        for radius in [19.0_f64, 29.0] {
            for degrees in 0..360 {
                let angle = (degrees as f64).to_radians();
                let x = (32.0 + radius * angle.sin()).floor() as usize;
                let y = (32.0 - radius * angle.cos()).floor() as usize;
                let at = (y * pixels::SIDE + x) * 4;
                assert!(
                    out[at] as i32 - background as i32 > 12,
                    "ring has an unjoined edge: {percent}% {degrees} degrees"
                );
            }
        }
    }
}
