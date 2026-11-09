//! Exercise the complete production controller with an isolated firmware port.
#![allow(dead_code, static_mut_refs)]
#[path = "../src/desktop_scale.rs"]
mod desktop_scale;
#[path = "../src/honeycomb.rs"]
mod honeycomb;
#[path = "../src/honeycomb_layout.rs"]
mod honeycomb_layout;
#[macro_use]
#[path = "../src/mem.rs"]
mod production_mem;
mod mem {
    pub unsafe fn rd32(_: *const u32) -> u32 {
        0
    } // no desktop container in host model
    pub unsafe fn safe_ptr(_: u32) -> bool {
        false
    }
}
use honeycomb_layout::{Frame, Point, MAX_ICONS};
use std::sync::Mutex;

#[derive(Default)]
struct Ui {
    config: Option<Vec<u8>>,
    written: Vec<u8>,
    fail_write: bool,
    write_calls: usize,
    rw_config: Option<u8>,
    rw_written: Vec<u8>,
    fail_rw_write: bool,
    owner: u32,
    supported: bool,
    visible: bool,
    icons: Vec<u32>,
    revision: u32,
    attached: usize,
    detached: usize,
    placed: Vec<u32>,
    launched: usize,
    native_events: usize,
    target: u32,
    code: u32,
    delta: Point,
    frame_open: bool,
    commits: usize,
    geometry_ok: bool,
    layout_phase: bool,
    layout_rects: Vec<u32>,
    layout_commits: usize,
    point: Point,
    chain: u32,
    fast_timer: bool,
    timer_active: bool,
    entry_source: bool,
    pressed_app: bool,
    native_codes: Vec<u32>,
    hook_calls: usize,
    frames: Vec<Frame>,
    edit_detached: usize,
    cleared_snapshots: usize,
    transitions: usize,
    entry_trace: Vec<&'static str>,
}
static UI: Mutex<Option<Ui>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Ui) -> T) -> T {
    f(UI.lock().unwrap().as_mut().unwrap())
}
mod log {
    pub fn line(_: &str) {}
    pub fn kv(_: &str, _: u32) {}
}
mod fw_api {
    use super::*;
    pub mod oflag {
        pub const RDONLY: u32 = 0;
        pub const WRONLY: u32 = 1;
        pub const CREAT: u32 = 0x40;
    }
    pub unsafe fn open(path: *const u8, flags: u32, _: u32) -> i32 {
        with(|u| assert!(!u.layout_phase, "file I/O during layout"));
        let rw = std::ffi::CStr::from_ptr(path.cast())
            .to_bytes()
            .ends_with(b"applist.cfg");
        if flags == 0 {
            with(|u| {
                if rw {
                    if u.rw_config.is_some() {
                        5
                    } else {
                        -1
                    }
                } else if u.config.is_some() {
                    3
                } else {
                    -1
                }
            })
        } else {
            if rw {
                6
            } else {
                4
            }
        }
    }
    pub unsafe fn read(fd: i32, b: *mut u8, n: u32) -> i32 {
        if fd == 5 {
            return with(|u| {
                *b = u.rw_config.unwrap();
                1
            });
        }
        with(|u| {
            let a = u.config.as_ref().unwrap();
            let len = a.len().min(n as usize);
            std::ptr::copy_nonoverlapping(a.as_ptr(), b, len);
            len as i32
        })
    }
    pub unsafe fn close(_: i32) {}
    pub unsafe fn write(fd: i32, b: *const u8, n: u32) -> i32 {
        if fd == 6 {
            return with(|u| {
                if u.fail_rw_write {
                    return -1;
                }
                u.rw_written = std::slice::from_raw_parts(b, n as usize).to_vec();
                n as i32
            });
        }
        with(|u| {
            u.write_calls += 1;
            if u.fail_write {
                return -1;
            }
            u.written = std::slice::from_raw_parts(b, n as usize).to_vec();
            n as i32
        })
    }
    pub unsafe fn event_get_code(_: u32) -> u32 {
        with(|u| u.code)
    }
    pub mod honeycomb {
        use super::*;
        pub unsafe fn launcher() -> u32 {
            with(|u| u.owner)
        }
        pub unsafe fn supported(_: u32) -> bool {
            with(|u| u.supported)
        }
        pub unsafe fn visible(_: u32) -> bool {
            with(|u| u.visible)
        }
        pub unsafe fn revision(_: u32) -> u32 {
            with(|u| u.revision)
        }
        pub unsafe fn icons(_: u32, out: &mut [u32; MAX_ICONS]) -> usize {
            with(|u| {
                if u.icons.len() > MAX_ICONS {
                    return 0;
                }
                out[..u.icons.len()].copy_from_slice(&u.icons);
                u.icons.len()
            })
        }
        pub unsafe fn attach(_: u32, _: &[u32]) -> bool {
            with(|u| {
                assert!(!u.layout_phase);
                u.attached += 1;
            });
            true
        }
        pub unsafe fn hook_icons(a: &[u32]) -> bool {
            with(|u| {
                assert!(!u.layout_phase, "hook/visibility in layout");
                u.hook_calls += 1;
                for x in a {
                    assert!(u.icons.contains(x));
                }
            });
            true
        }
        pub unsafe fn detach(_: u32, a: &[u32]) {
            hook_icons(a);
            with(|u| u.detached += 1);
        }
        pub unsafe fn detach_for_edit(_: u32, a: &[u32]) {
            hook_icons(a);
            with(|u| {
                u.detached += 1;
                u.edit_detached += 1;
                u.entry_trace.push("release");
            });
        }
        pub unsafe fn begin_frame(_: u32, _: &[u32]) {
            with(|u| {
                assert!(!u.layout_phase, "animation deletion in layout");
                assert!(!u.frame_open);
                u.frame_open = true;
            });
        }
        pub unsafe fn present(_: u32) {
            with(|u| {
                assert!(!u.layout_phase, "invalidation in layout");
                assert!(u.frame_open);
                u.frame_open = false;
                u.commits += 1;
            });
        }
        pub unsafe fn commit_rects(_: u32) {
            with(|u| {
                assert!(u.layout_phase);
                u.layout_commits += 1;
            });
        }
        pub unsafe fn layout_rect(icon: u32, _: Frame) -> bool {
            with(|u| {
                assert!(u.layout_phase);
                assert!(u.icons.contains(&icon));
                u.layout_rects.push(icon);
                u.geometry_ok
            })
        }
        pub unsafe fn geometry_matches(_: u32, _: Frame) -> bool {
            with(|u| u.geometry_ok)
        }
        pub unsafe fn place(icon: u32, f: Frame) -> bool {
            with(|u| {
                assert!(!u.layout_phase, "transform/flag setter in layout");
                assert!(u.frame_open);
                assert!(u.icons.contains(&icon), "stale icon");
                u.placed.push(icon);
                u.frames.push(f);
                u.revision += 1;
            });
            true
        }
        pub unsafe fn current_target(_: u32) -> u32 {
            with(|u| u.target)
        }
        pub unsafe fn parent(_: u32) -> u32 {
            with(|u| u.owner)
        }
        pub unsafe fn base_event(_: u32) {}
        pub unsafe fn native_event(_: u32) {
            with(|u| {
                u.native_events += 1;
                u.native_codes.push(u.code);
                if u.code == 5 {
                    assert!(
                        u.edit_detached > 0,
                        "editor started before honeycomb release"
                    );
                    u.entry_trace.push("editor");
                    u.supported = false;
                }
            });
        }
        pub unsafe fn vector(_: u32) -> Point {
            with(|u| u.delta)
        }
        pub unsafe fn point(_: u32) -> Point {
            with(|u| u.point)
        }
        pub unsafe fn entry_source(_: u32) -> bool {
            with(|u| u.entry_source)
        }
        pub unsafe fn clear_snapshot(_: u32) -> bool {
            with(|u| {
                assert!(!u.layout_phase);
                u.cleared_snapshots += 1;
                u.entry_trace.push("clear");
            });
            true
        }
        pub unsafe fn begin_transition(_: u32, _: u32) -> bool {
            with(|u| {
                assert!(!u.layout_phase);
                u.transitions += 1;
                u.entry_trace.push("begin");
            });
            true
        }
        pub unsafe fn pressed_icon(_: u32, _: u32) -> bool {
            with(|u| u.pressed_app)
        }
        pub unsafe fn navigation(_: u32, _: u32, chain: u32) {
            with(|u| {
                assert!(!u.layout_phase);
                u.chain = chain;
            });
        }
        pub unsafe fn frame_timer_start() -> bool {
            with(|u| u.fast_timer)
        }
        pub unsafe fn frame_timer_active(on: bool) {
            with(|u| {
                assert!(!u.layout_phase);
                u.timer_active = on;
            });
        }
        pub unsafe fn launch(_: u32) {
            with(|u| u.launched += 1);
        }
    }
}
unsafe fn event(code: u32, delta: Point) {
    with(|u| {
        u.target = u.owner;
        u.code = code;
        u.delta = delta;
    });
    honeycomb::class_event(0, 1);
}
unsafe fn click() {
    with(|u| u.target = 10);
    honeycomb::click_event(1);
}
unsafe fn layout() {
    with(|u| u.layout_phase = true);
    honeycomb::layout_event(1, 0);
    with(|u| u.layout_phase = false);
}
unsafe fn start() {
    honeycomb::toggle();
    honeycomb::tick();
    assert_eq!(with(|u| u.attached), 1);
}
fn main() {
    unsafe {
        *UI.lock().unwrap() = Some(Ui {
            owner: 1,
            supported: true,
            visible: true,
            geometry_ok: true,
            pressed_app: true,
            point: Point { x: 168, y: 240 },
            icons: (10..50).collect(),
            ..Ui::default()
        });
        let case = std::env::args().nth(1).unwrap();
        match case.as_str() {
            "defaults" => {
                honeycomb::tick();
                assert!(!honeycomb::enabled());
                assert_eq!(with(|u| u.attached), 0);
            }
            "config" => {
                with(|u| u.config = Some(b"HCB1\x01".to_vec()));
                honeycomb::tick();
                assert!(!honeycomb::enabled());
                assert_eq!(with(|u| u.attached), 0);
                assert_eq!(with(|u| u.written.clone()), b"HCB1\x00");
            }
            "invalid" => {
                with(|u| u.config = Some(b"HCB1\x02".to_vec()));
                honeycomb::tick();
                assert!(!honeycomb::enabled());
            }
            "short" => {
                with(|u| u.config = Some(b"HCB1".to_vec()));
                honeycomb::tick();
                assert!(!honeycomb::enabled());
            }
            "save-retry" => {
                with(|u| u.fail_write = true);
                start();
                assert_eq!(with(|u| u.write_calls), 1);
                with(|u| u.fail_write = false);
                honeycomb::tick();
                assert_eq!(with(|u| u.written.clone()), b"HCB1\x00");
                honeycomb::tick();
                assert_eq!(with(|u| u.write_calls), 2);
            }
            "toggle-restore" => {
                start();
                honeycomb::toggle();
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
                assert_eq!(with(|u| u.written.clone()), b"HCB1\x00");
            }
            "tap" => {
                start();
                event(1, Point::default());
                event(8, Point::default());
                click();
                assert_eq!(with(|u| u.launched), 1);
            }
            "drag" => {
                start();
                for (x, y) in [(20, 0), (-20, 0), (0, 20), (0, -20)] {
                    event(1, Point::default());
                    event(2, Point { x, y });
                    event(8, Point::default());
                    click();
                }
                assert_eq!(with(|u| u.launched), 0);
            }
            "rebuild" => {
                start();
                with(|u| {
                    u.icons = (70..110).collect();
                    u.revision += 1;
                    u.placed.clear();
                });
                honeycomb::tick();
                assert_eq!(with(|u| u.placed.clone()), (70..110).collect::<Vec<_>>());
            }
            "remove" => {
                start();
                with(|u| {
                    u.icons = vec![20, 21];
                    u.revision += 1;
                    u.placed.clear();
                });
                event(1, Point::default());
                event(2, Point { x: 20, y: 10 });
                assert!(with(|u| u.placed.is_empty()));
                honeycomb::tick();
                assert_eq!(with(|u| u.placed.clone()), vec![20, 21]);
            }
            "destroy" => {
                start();
                with(|u| u.owner = 0);
                honeycomb::destroyed(1);
                honeycomb::tick();
                with(|u| {
                    u.owner = 2;
                    u.revision += 1;
                });
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 2);
            }
            "unsupported" => {
                with(|u| u.supported = false);
                honeycomb::toggle();
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 0);
                with(|u| u.supported = true);
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 1);
                with(|u| u.supported = false);
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
            }
            "idle" => {
                start();
                with(|u| u.placed.clear());
                for _ in 0..100 {
                    honeycomb::tick();
                }
                assert!(with(|u| u.placed.is_empty()));
            }
            "offscreen" => {
                with(|u| u.visible = false);
                start();
                assert_eq!(with(|u| u.commits), 1);
                assert!(!with(|u| u.timer_active));
            }
            "capacity" => {
                with(|u| u.icons = (10..10 + MAX_ICONS as u32 + 1).collect());
                honeycomb::toggle();
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 0);
            }
            "grow-over-capacity" => {
                start();
                with(|u| {
                    u.icons = (10..10 + MAX_ICONS as u32 + 1).collect();
                    u.revision += 1;
                });
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
            }
            "remove-all" => {
                start();
                with(|u| {
                    u.icons.clear();
                    u.revision += 1;
                });
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
            }
            "exclusive-on" => {
                assert!(desktop_scale::lens_on());
                start();
                assert!(
                    !desktop_scale::lens_on()
                        && !desktop_scale::pitch_on()
                        && !desktop_scale::inset_on()
                );
                desktop_scale::tick();
                assert_eq!(with(|u| u.rw_written.clone()), vec![0]);
            }
            "exclusive-rw" | "exclusive-lens" | "exclusive-inset" => {
                start();
                let bit = match case.as_str() {
                    "exclusive-lens" => {
                        desktop_scale::set_lens(true);
                        1
                    }
                    "exclusive-inset" => {
                        desktop_scale::set_inset(true);
                        4
                    }
                    _ => {
                        desktop_scale::set_pitch(true);
                        2
                    }
                };
                assert!(!honeycomb::enabled());
                assert_eq!(desktop_scale::lens_on(), bit == 1);
                assert_eq!(desktop_scale::pitch_on(), bit == 2);
                assert_eq!(desktop_scale::inset_on(), bit == 4);
                desktop_scale::tick();
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
                assert_eq!(with(|u| u.rw_written.clone()), vec![bit]);
                assert_eq!(with(|u| u.written.clone()), b"HCB1\x00");
            }
            "exclusive-boot" => {
                with(|u| {
                    u.config = Some(b"HCB1\x01".to_vec());
                    u.rw_config = Some(7);
                });
                assert!(desktop_scale::lens_on());
                assert!(desktop_scale::pitch_on() && desktop_scale::inset_on());
                assert!(!honeycomb::enabled());
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 0);
            }
            "layout-callback" => {
                start();
                with(|u| u.placed.clear());
                layout();
                assert!(with(|u| u.placed.is_empty()));
                assert_eq!(with(|u| u.layout_rects.len()), 40);
                assert_eq!(with(|u| u.layout_commits), 1);
                assert_eq!(with(|u| u.commits), 1);
            }
            "rw-save-retry" => {
                with(|u| u.fail_rw_write = true);
                start();
                desktop_scale::tick();
                assert!(with(|u| u.rw_written.is_empty()));
                with(|u| u.fail_rw_write = false);
                desktop_scale::tick();
                assert_eq!(with(|u| u.rw_written.clone()), vec![0]);
            }
            "geometry-failed" => {
                with(|u| u.geometry_ok = false);
                honeycomb::toggle();
                honeycomb::tick();
                assert!(!honeycomb::enabled());
                assert_eq!(with(|u| u.detached), 1);
                honeycomb::tick();
                assert_eq!(with(|u| u.written.clone()), b"HCB1\x00");
            }
            "layout-fault-deferred" => {
                start();
                with(|u| u.geometry_ok = false);
                layout();
                assert!(honeycomb::enabled());
                assert_eq!(with(|u| u.detached), 0);
                honeycomb::tick();
                assert!(!honeycomb::enabled());
                assert_eq!(with(|u| u.detached), 1);
            }
            "layout-repeat" => {
                start();
                with(|u| u.placed.clear());
                for _ in 0..100 {
                    layout();
                }
                assert!(with(|u| u.placed.is_empty()));
                assert_eq!(with(|u| u.commits), 1);
            }
            "gesture-deferred" => {
                start();
                with(|u| u.placed.clear());
                event(1, Point::default());
                event(2, Point { x: 20, y: 10 });
                assert!(with(|u| u.placed.is_empty()));
                honeycomb::tick();
                assert!(!with(|u| u.placed.is_empty()));
            }
            "zoom-edge" => {
                start();
                let before = with(|u| u.frames.clone());
                with(|u| u.point = Point { x: 330, y: 240 });
                event(1, Point::default());
                event(2, Point { x: 2, y: 48 });
                event(8, Point::default());
                click();
                honeycomb::tick();
                assert_eq!(with(|u| u.launched), 0);
                assert_eq!(with(|u| u.chain), 0);
                let after = with(|u| u.frames.clone());
                let last = &after[after.len() - 40..];
                assert!(last[0].diameter < before[0].diameter);
                assert!((last[5].x - 168).abs() < (before[5].x - 168).abs());
            }
            "exit-left" | "exit-top" => {
                start();
                event(1, Point::default());
                let d = if case == "exit-left" {
                    Point { x: 100, y: 0 }
                } else {
                    Point { x: 0, y: 100 }
                };
                for _ in 0..12 {
                    event(2, d);
                }
                assert_eq!(
                    with(|u| u.chain),
                    if case == "exit-left" { 0x100 } else { 0x200 }
                );
                event(8, Point::default());
                click();
                assert_eq!(with(|u| u.launched), 0);
                event(1, Point::default());
                assert_eq!(with(|u| u.chain), 0);
            }
            "inside-no-exit" => {
                start();
                event(1, Point::default());
                event(2, Point { x: 30, y: 10 });
                assert_eq!(with(|u| u.chain), 0);
            }
            "entry-sync" => {
                with(|u| u.visible = false);
                start();
                with(|u| {
                    u.entry_source = true;
                    u.visible = true;
                    u.code = 11;
                    u.placed.clear();
                    u.revision += 1;
                });
                honeycomb::entry_event(1);
                assert_eq!(with(|u| u.placed.len()), 40);
            }
            "frame-timer" => {
                with(|u| u.fast_timer = true);
                start();
                let hooks = with(|u| u.hook_calls);
                event(1, Point::default());
                event(2, Point { x: 20, y: 10 });
                with(|u| u.placed.clear());
                honeycomb::animation_tick(1);
                assert_eq!(with(|u| u.placed.len()), 40);
                assert_eq!(with(|u| u.hook_calls), hooks);
                event(8, Point::default());
                for _ in 0..100 {
                    honeycomb::animation_tick(1);
                }
                assert!(!with(|u| u.timer_active));
            }
            "long-press" => {
                start();
                event(1, Point::default());
                event(5, Point::default());
                assert!(with(|u| u.native_codes.contains(&5)));
                assert_eq!(with(|u| u.entry_trace.clone()), vec!["release", "editor"]);
                assert_eq!(with(|u| u.edit_detached), 1);
                with(|u| u.placed.clear());
                layout();
                assert!(with(|u| u.layout_rects.is_empty()));
                honeycomb::tick();
                assert_eq!(with(|u| u.detached), 1);
                assert!(honeycomb::enabled());
                with(|u| u.supported = true);
                honeycomb::tick();
                assert_eq!(with(|u| u.attached), 2);
            }
            "drag-no-editor" | "zoom-no-editor" | "blank-no-editor" => {
                start();
                if case == "zoom-no-editor" {
                    with(|u| u.point = Point { x: 330, y: 240 });
                }
                if case == "blank-no-editor" {
                    with(|u| u.pressed_app = false);
                }
                event(1, Point::default());
                if case == "drag-no-editor" {
                    event(2, Point { x: 30, y: 10 });
                }
                event(5, Point::default());
                assert!(!with(|u| u.native_codes.contains(&5)));
            }
            "pre-entry" => {
                start();
                with(|u| {
                    u.entry_source = true;
                    u.code = 9;
                });
                assert_eq!(honeycomb::chaos_honeycomb_pre_scroll(1), 1);
                assert_eq!(with(|u| u.entry_trace.clone()), vec!["clear", "begin"]);
                let commits = with(|u| u.commits);
                honeycomb::entry_event(1);
                assert_eq!(with(|u| u.commits), commits);
            }
            "pre-end" | "pre-unrelated" => {
                start();
                with(|u| {
                    u.entry_source = case == "pre-end";
                    u.code = if case == "pre-end" { 11 } else { 9 };
                });
                assert_eq!(honeycomb::chaos_honeycomb_pre_scroll(1), 0);
                assert_eq!(with(|u| u.cleared_snapshots), 0);
            }
            "pre-disabled" => {
                with(|u| {
                    u.entry_source = true;
                    u.code = 9;
                });
                assert_eq!(honeycomb::chaos_honeycomb_pre_scroll(1), 0);
            }
            _ => panic!("unknown scenario"),
        }
        println!("PASS {case}");
    }
}
