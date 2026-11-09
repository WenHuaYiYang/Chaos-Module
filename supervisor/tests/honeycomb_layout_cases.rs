#![allow(dead_code)]
#[path = "../src/honeycomb_layout.rs"]
mod layout;
use layout::*;

#[test]
fn hex_rings_are_unique_and_neighbours_are_equally_spaced() {
    let mut points = std::collections::HashSet::new();
    for i in 0..MAX_ICONS {
        let p = position(i);
        assert!(points.insert((p.x, p.y)));
    }
    for i in 1..7 {
        let p = position(i);
        assert!((p.x * p.x + p.y * p.y - 96 * 96).abs() < 100);
    }
    assert_eq!(position(0), Point::default());
    for i in 0..19 {
        let p = position(i);
        assert!(p.x.abs() <= 192 && p.y.abs() <= 166);
    }
}

#[test]
fn edge_lens_is_symmetric_and_the_centre_is_largest() {
    let c = frame(0, Point::default());
    assert_eq!((c.x, c.y, c.diameter), (168, 240, 84));
    for x in 0..250 {
        let a = frame(0, Point { x, y: 0 });
        let b = frame(0, Point { x: -x, y: 0 });
        assert_eq!(a.diameter, b.diameter);
        assert!((28..=84).contains(&a.diameter));
        if x > 0 {
            assert!(a.diameter <= frame(0, Point { x: x - 1, y: 0 }).diameter);
        }
    }
}

#[test]
fn both_axes_move_and_all_apps_can_be_brought_to_the_centre() {
    for i in 0..MAX_ICONS {
        let p = position(i);
        let mut c = Camera::new();
        c.press();
        for _ in 0..20 {
            let pan = c.pan();
            c.drag(
                Point {
                    x: (-p.x - pan.x).clamp(-128, 128),
                    y: (-p.y - pan.y).clamp(-128, 128),
                },
                MAX_ICONS,
            );
        }
        let f = frame(i, c.pan());
        assert_eq!((f.x, f.y), (WIDTH / 2, HEIGHT / 2), "app {i}");
    }
}

#[test]
fn drag_latch_survives_release_and_a_return_to_the_start() {
    let mut c = Camera::new();
    c.press();
    c.drag(Point { x: 20, y: 20 }, 40);
    c.drag(Point { x: -20, y: -20 }, 40);
    assert_eq!(c.pan(), Point::default());
    c.release();
    assert!(!c.can_click());
}

#[test]
fn jitter_does_not_move_the_cloud_or_cancel_a_tap() {
    let mut c = Camera::new();
    c.press();
    assert!(!c.drag(Point { x: 2, y: -2 }, 40));
    c.release();
    assert!(c.can_click());
    assert_eq!(c.pan(), Point::default());
}

#[test]
fn movement_in_each_direction_cancels_launching() {
    for (x, y) in [(20, 0), (-20, 0), (0, 20), (0, -20)] {
        let mut c = Camera::new();
        c.press();
        c.drag(Point { x, y }, 40);
        c.release();
        assert!(!c.can_click());
        assert_ne!(c.pan(), Point::default());
    }
}

#[test]
fn inertia_finishes_and_a_catching_tap_only_stops_it() {
    let mut c = Camera::new();
    c.press();
    c.drag(Point { x: 20, y: 10 }, 40);
    c.release();
    assert!(c.tick(40));
    c.press();
    c.release();
    assert!(!c.can_click());
    assert!(!c.tick(40));
    c.press();
    c.release();
    assert!(c.can_click());
    c.press();
    c.drag(Point { x: -20, y: -20 }, 40);
    c.release();
    for _ in 0..100 {
        c.tick(40);
    }
    assert!(!c.tick(40));
}

#[test]
fn bounds_and_empty_lists_do_not_allow_getting_lost() {
    for count in [0, 1, 2, 7, 40, MAX_ICONS] {
        let mut c = Camera::new();
        c.press();
        for _ in 0..1000 {
            c.drag(
                Point {
                    x: 10000,
                    y: -10000,
                },
                count,
            );
        }
        let p = c.pan();
        assert!(p.x.abs() <= 768 && p.y.abs() <= 664);
        if count <= 1 {
            assert_eq!(p, Point::default());
        }
        c.cancel();
        assert!(!c.can_click());
        assert!(!c.tick(count));
    }
}

#[test]
fn right_edge_drag_scales_spacing_and_icons_together_without_panning() {
    let mut c = Camera::new();
    c.press_at(Point {
        x: WIDTH - 4,
        y: 200,
    });
    assert!(c.drag(Point { x: -40, y: 32 }, 40));
    assert_eq!(c.pan(), Point::default());
    assert_eq!(c.zoom(), 192);
    let f = c.frame(5);
    assert_eq!(f.x - WIDTH / 2, 72); // default ring radius is 96
    assert_eq!(c.frame(0).diameter, 63);
    assert_eq!(c.route(), Route::Cloud);
    c.release();
    assert!(!c.can_click());
    assert!(!c.tick(40));
}

#[test]
fn edge_capture_is_fixed_at_press_and_zoom_has_limits() {
    let mut c = Camera::new();
    c.press_at(Point {
        x: WIDTH - 4,
        y: 200,
    });
    for _ in 0..20 {
        c.drag(Point { x: 0, y: 100 }, 40);
    }
    assert_eq!(c.zoom(), ZOOM_MIN);
    assert_eq!(c.route(), Route::Cloud);
    for _ in 0..20 {
        c.drag(Point { x: 0, y: -100 }, 40);
    }
    assert_eq!(c.zoom(), ZOOM_MAX);
    c.release();
    c.press_at(Point {
        x: WIDTH - ZOOM_EDGE - 1,
        y: 200,
    });
    c.drag(Point { x: 0, y: -30 }, 40);
    assert_eq!(c.zoom(), ZOOM_MAX);
    assert_eq!(c.pan().y, -30);
}

#[test]
fn zoom_keeps_the_central_app_anchored() {
    let mut c = Camera::new();
    c.press();
    c.drag(Point { x: -96, y: 0 }, 40);
    c.release();
    c.press_at(Point {
        x: WIDTH - 4,
        y: 200,
    });
    c.drag(Point { x: 0, y: 32 }, 40);
    assert_eq!(c.frame(5).x, WIDTH / 2);
}

#[test]
fn outward_left_and_top_drags_handoff_but_inertia_and_other_edges_do_not() {
    for (d, route) in [
        (Point { x: 100, y: 0 }, Route::Horizontal),
        (Point { x: 0, y: 100 }, Route::Vertical),
    ] {
        let mut c = Camera::new();
        c.press();
        for _ in 0..20 {
            c.drag(d, 40);
        }
        assert_eq!(c.route(), route);
        c.release();
        assert!(!c.can_click());
        assert!(!c.tick(40));
        c.press();
        assert_eq!(c.route(), Route::Cloud);
    }
    let mut c = Camera::new();
    c.press();
    c.drag(Point { x: 100, y: 0 }, 7);
    c.release();
    for _ in 0..100 {
        c.tick(7);
    }
    assert_eq!(c.route(), Route::Cloud);
    c.press();
    for _ in 0..20 {
        c.drag(Point { x: -100, y: 0 }, 7);
    }
    assert_eq!(c.route(), Route::Cloud);
    c.press();
    for _ in 0..20 {
        c.drag(Point { x: 0, y: -100 }, 7);
    }
    assert_eq!(c.route(), Route::Cloud);
}
