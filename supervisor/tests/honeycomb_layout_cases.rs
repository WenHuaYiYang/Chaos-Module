#![allow(dead_code)]
#[path = "../src/honeycomb_layout.rs"]
mod layout;
use layout::*;

#[test]
fn linear_layout_uses_its_own_full_list_extent() {
    let mut c=Camera::new();
    c.linear_end(128,4000);
    assert_eq!(c.pan(),Point{x:0,y:-4000});
    c.press_linear(Point{x:330,y:100});
    c.drag(Point{x:0,y:64},128);
    assert_eq!(c.zoom(),256);
    assert_eq!(c.pan(),Point{x:0,y:-3936});
    c.linear_bounds(3,0);
    assert_eq!(c.pan(),Point::default());
}

#[test]
fn edit_return_animates_both_position_and_size_until_the_exact_final_frame() {
    let mut a = ReturnAnimation::new();
    let from = Frame {
        x: 56,
        y: 80,
        diameter: 112,
        visible: true,
    };
    let target = frame(0, Point::default());
    a.set_start(0, from);
    a.begin(100, 1);
    assert_eq!(a.frame(0, target), from);
    assert!(!a.tick(100));
    let mut previous = from;
    let mut distinct = std::collections::HashSet::new();
    for elapsed in (16..RETURN_DURATION_MS).step_by(16) {
        a.tick(100 + elapsed);
        let f = a.frame(0, target);
        assert!(f.visible);
        assert!((previous.x..=target.x).contains(&f.x));
        assert!((previous.y..=target.y).contains(&f.y));
        assert!((target.diameter..=previous.diameter).contains(&f.diameter));
        assert_ne!(f, target, "completed before duration at {elapsed}ms");
        distinct.insert((f.x, f.y, f.diameter));
        previous = f;
    }
    assert!(
        distinct.len() >= 12,
        "return collapsed to a few geometry frames"
    );
    assert!(a.tick(100 + RETURN_DURATION_MS));
    assert!(!a.active());
    assert_eq!(a.frame(0, target), target);
    assert!(!a.tick(1000));
}

#[test]
fn edit_return_uses_elapsed_time_including_wrap_and_delayed_timer_turns() {
    let mut a = ReturnAnimation::new();
    let mut b = ReturnAnimation::new();
    let from = Frame {
        x: 40,
        y: 60,
        diameter: 112,
        visible: true,
    };
    let target = frame(0, Point::default());
    let start = u32::MAX - 30;
    for animation in [&mut a, &mut b] {
        animation.set_start(0, from);
        animation.begin(start, 1);
    }
    for elapsed in (16..=128).step_by(16) {
        a.tick(start.wrapping_add(elapsed));
    }
    b.tick(start.wrapping_add(128));
    assert_eq!(a.frame(0, target), b.frame(0, target));
    b.tick(start.wrapping_add(1000));
    assert_eq!(b.frame(0, target), target);
    assert!(!b.active());
    a.cancel();
    assert!(!a.active());
    assert_eq!(a.frame(0, target), target);
}

#[test]
fn edit_return_handles_icons_entering_and_leaving_the_viewport() {
    let mut a = ReturnAnimation::new();
    a.set_start(
        0,
        Frame {
            x: 1000,
            y: 1000,
            diameter: 112,
            visible: false,
        },
    );
    a.set_start(
        1,
        Frame {
            x: 100,
            y: 100,
            diameter: 112,
            visible: true,
        },
    );
    a.begin(0, 2);
    let entering = frame(0, Point::default());
    let leaving = Frame {
        x: 1000,
        y: 1000,
        diameter: 28,
        visible: false,
    };
    assert!(!a.frame(0, entering).visible);
    a.tick(16);
    let f = a.frame(0, entering);
    assert!(f.visible);
    assert_eq!((f.x, f.y), (entering.x, entering.y));
    assert!((1..entering.diameter).contains(&f.diameter));
    a.tick(RETURN_DURATION_MS / 2);
    assert!(!a.frame(1, leaving).visible);
    a.tick(RETURN_DURATION_MS);
    assert_eq!(a.frame(0, entering), entering);
    assert_eq!(a.frame(1, leaving), leaving);
    assert_eq!(a.frame(MAX_ICONS, entering), entering);
}

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
fn left_edge_changes_pitch_without_scaling_the_central_icon_or_panning() {
    let mut c = Camera::new();
    c.press_at(Point { x: 4, y: 200 });
    assert!(!c.drag(Point { x: 1, y: 3 }, 40));
    assert!(c.drag(Point { x: 1, y: 29 }, 40));
    assert_eq!(c.spacing(), 320);
    assert_eq!(c.zoom(), 256);
    assert_eq!(c.pan(), Point::default());
    assert_eq!(c.frame(5).x - WIDTH / 2, 120);
    assert_eq!(c.frame(0).diameter, 84);
    assert_eq!(c.route(), Route::Cloud);
    c.release();
    assert!(!c.can_click());
    assert!(!c.tick(40));

    // The opposite edge still scales the icons and the adjusted pitch together.
    c.press_at(Point {
        x: WIDTH - 4,
        y: 200,
    });
    c.drag(Point { x: 0, y: 32 }, 40);
    assert_eq!(c.spacing(), 320);
    assert_eq!(c.zoom(), 192);
    assert_eq!(c.frame(5).x - WIDTH / 2, 90);
    assert_eq!(c.frame(0).diameter, 63);
}

#[test]
fn left_edge_adjustment_has_limits_and_stays_captured_without_navigation() {
    let mut c = Camera::new();
    c.press_at(Point { x: 4, y: 200 });
    for _ in 0..20 {
        c.drag(Point { x: 100, y: 128 }, 40);
    }
    assert_eq!(c.spacing(), SPACING_MAX);
    assert!(!c.drag(Point { x: 128, y: 128 }, 40));
    for _ in 0..20 {
        c.drag(Point { x: 128, y: -100 }, 40);
    }
    assert_eq!(c.spacing(), SPACING_MIN);
    assert_eq!(c.pan(), Point::default());
    assert_eq!(c.route(), Route::Cloud);
    c.release();
    assert!(!c.can_click());
    assert!(!c.tick(40));

    // Outside the strip, crossing into it later remains an ordinary pan.
    for point in [
        Point {
            x: ZOOM_EDGE,
            y: 200,
        },
        Point { x: 4, y: -1 },
        Point { x: 4, y: HEIGHT },
        Point { x: -1, y: 200 },
    ] {
        c.press_at(point);
        c.drag(Point { x: -30, y: -40 }, 40);
        assert_eq!(c.spacing(), SPACING_MIN);
    }
}

#[test]
fn left_edge_taps_and_horizontal_swipes_keep_native_behaviour() {
    let mut c = Camera::new();
    c.press_at(Point { x: 4, y: 200 });
    c.drag(Point { x: 1, y: -2 }, 7);
    assert!(c.can_click()); // Also permits an un-dragged native long press.
    c.release();
    assert!(c.can_click());

    c.press_at(Point { x: 4, y: 200 });
    assert!(c.drag(Point { x: 20, y: 2 }, 7));
    assert_eq!(c.pan(), Point { x: 20, y: 2 });
    c.drag(Point { x: 0, y: -40 }, 7);
    assert_eq!(c.pan().y, -38); // The direction cannot change mid-gesture.
    assert_eq!(c.spacing(), 256);
    for _ in 0..5 {
        c.drag(Point { x: 100, y: 0 }, 7);
    }
    assert_eq!(c.route(), Route::Horizontal);
    c.release();
    assert!(!c.can_click());
}

#[test]
fn spacing_and_zoom_keep_the_central_app_anchored() {
    let mut c = Camera::new();
    c.press();
    c.drag(Point { x: -96, y: 0 }, 40);
    c.release();
    for (point, delta) in [
        (Point { x: 4, y: 200 }, Point { x: 0, y: 32 }),
        (
            Point {
                x: WIDTH - 4,
                y: 200,
            },
            Point { x: 0, y: 32 },
        ),
        (Point { x: 4, y: 200 }, Point { x: 0, y: -48 }),
    ] {
        c.press_at(point);
        c.drag(delta, 40);
        c.release();
        assert_eq!((c.frame(5).x, c.frame(5).y), (WIDTH / 2, HEIGHT / 2));
    }
}

#[test]
fn every_app_remains_reachable_at_combined_spacing_and_zoom_limits() {
    for spacing in [SPACING_MIN, 256, SPACING_MAX] {
        for zoom in [ZOOM_MIN, 256, ZOOM_MAX] {
            for i in 0..MAX_ICONS {
                let mut c = Camera::new();
                c.press_at(Point { x: 4, y: 200 });
                c.drag(
                    Point {
                        x: 0,
                        y: (spacing - 256) / 2,
                    },
                    MAX_ICONS,
                );
                c.release();
                c.press_at(Point {
                    x: WIDTH - 4,
                    y: 200,
                });
                c.drag(
                    Point {
                        x: 0,
                        y: (256 - zoom) / 2,
                    },
                    MAX_ICONS,
                );
                c.release();
                assert_eq!(c.spacing(), spacing);
                assert_eq!(c.zoom(), zoom);
                c.press();
                for _ in 0..20 {
                    let f = c.frame(i);
                    c.drag(
                        Point {
                            x: WIDTH / 2 - f.x,
                            y: HEIGHT / 2 - f.y,
                        },
                        MAX_ICONS,
                    );
                }
                let f = c.frame(i);
                assert_eq!(
                    (f.x, f.y),
                    (WIDTH / 2, HEIGHT / 2),
                    "app {i}, spacing {spacing}, zoom {zoom}"
                );
            }
        }
    }
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
