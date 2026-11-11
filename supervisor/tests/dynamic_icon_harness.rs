//! Production two-choice icon controller against a live-instance/VFS fixture.
#![allow(dead_code, static_mut_refs)]
#[macro_use]
#[path = "../src/mem.rs"] mod production_mem;
#[path = "../src/dynamic_icon.rs"] mod dynamic_icon;
#[path = "../src/dynamic_icon_frames.rs"] mod dynamic_icon_frames;
use std::{collections::BTreeMap, sync::Mutex};
use dynamic_icon::Icon;
const FILE: &[u8] = b"/data/chaos/app-icon.cfg";
const TEMP: &[u8] = b"/data/chaos/app-icon.cfg.tmp";
const ORIGINAL: u32 = 0x1234;
const PLAYBACK: [u32; 2] = [0xAA00, 0xAA80];
struct Handle { path: Vec<u8>, offset: usize }
struct Ui {
    files: BTreeMap<Vec<u8>, Vec<u8>>, handles: BTreeMap<i32, Handle>, next_fd: i32,
    chunk: usize, fail_write: bool, fail_rename: bool, fail_image: bool, fail_restore: bool,
    opens: usize, commits: usize, reads: usize, screen: bool, active: bool,
    icon: Option<Icon>, frame: Option<usize>, now: u32,
    writes: Vec<(u32, u8, usize)>, timer: bool, period: u32, retired: usize,
}
static UI: Mutex<Option<Ui>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Ui) -> T) -> T { f(UI.lock().unwrap().as_mut().unwrap()) }
fn original(object: u32) -> Icon { Icon { object, source: ORIGINAL, original: true, visible: true } }
fn setup(config: Option<&[u8]>) {
    unsafe { dynamic_icon::reset_for_test(); }
    let mut files = BTreeMap::new();
    if let Some(bytes) = config { files.insert(FILE.to_vec(), bytes.to_vec()); }
    *UI.lock().unwrap() = Some(Ui {
        files, handles: BTreeMap::new(), next_fd: 3, chunk: usize::MAX,
        fail_write: false, fail_rename: false, fail_image: false, fail_restore: false,
        opens: 0, commits: 0, reads: 0, screen: true, active: true,
        icon: Some(original(123)), frame: None, now: 0,
        writes: Vec::new(), timer: false, period: 1000, retired: 0,
    });
}
fn tick(now: u32) { with(|u| u.now = now); unsafe { dynamic_icon::tick(); } }
fn animation_tick(now: u32) { with(|u| u.now = now); unsafe { dynamic_icon::animation_tick(0); } }
fn choose(style: u8) { assert!(unsafe { dynamic_icon::select(style) }); }
mod state { pub static mut APP_ID: u32 = 200; }
mod log { pub fn line(_: &str) {} }
mod icon_pack_animation { pub unsafe fn animation_tick() {} }
mod fw_api {
    use super::*;
    pub mod oflag { pub const RDONLY: u32 = 1; pub const WRONLY: u32 = 2; pub const CREAT: u32 = 4; }
    unsafe fn path(p: *const u8) -> Vec<u8> { std::ffi::CStr::from_ptr(p.cast()).to_bytes().to_vec() }
    pub unsafe fn open(p: *const u8, flags: u32, _: u32) -> i32 {
        let p = path(p);
        with(|u| {
            u.opens += 1;
            if flags == oflag::RDONLY && !u.files.contains_key(&p) { return -1; }
            if flags & 0x20 != 0 { assert_eq!(p, TEMP); u.files.insert(p.clone(), Vec::new()); }
            let fd = u.next_fd; u.next_fd += 1; u.handles.insert(fd, Handle { path: p, offset: 0 }); fd
        })
    }
    pub unsafe fn read(fd: i32, out: *mut u8, len: u32) -> i32 {
        with(|u| {
            let h = u.handles.get_mut(&fd).unwrap(); let bytes = u.files.get(&h.path).unwrap();
            let n = (bytes.len() - h.offset).min(len as usize).min(u.chunk);
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(h.offset), out, n); h.offset += n; n as i32
        })
    }
    pub unsafe fn write(fd: i32, data: *const u8, len: u32) -> i32 {
        with(|u| {
            if u.fail_write { return 0; }
            let h = u.handles.get_mut(&fd).unwrap(); assert_eq!(h.path, TEMP);
            let n = (len as usize).min(u.chunk);
            u.files.get_mut(&h.path).unwrap().extend_from_slice(std::slice::from_raw_parts(data, n)); h.offset += n; n as i32
        })
    }
    pub unsafe fn close(fd: i32) -> i32 { with(|u| { assert!(u.handles.remove(&fd).is_some()); }); 0 }
    pub unsafe fn capture_rename(old: *const u8, new: *const u8) -> i32 {
        let old = path(old); let new = path(new);
        with(|u| {
            assert_eq!(old, TEMP); assert_eq!(new, FILE); if u.fail_rename { return -5; }
            let bytes = u.files.remove(&old).unwrap(); u.files.insert(new, bytes); u.commits += 1; 0
        })
    }
    pub unsafe fn fs_remove(p: *const u8) -> i32 { let p = path(p); with(|u| { assert_eq!(p, TEMP); u.files.remove(&p); }); 0 }
    pub fn capture_now_ms() -> u32 { with(|u| u.now) }
    pub unsafe fn screen_is_on() -> bool { with(|u| u.screen) }
    pub mod dynamic_icon {
        use super::*;
        use crate::dynamic_icon_frames::{Animation, PIXELS};
        pub unsafe fn launcher_active() -> bool { with(|u| u.active) }
        pub unsafe fn current(app_id: u32) -> Option<Icon> { assert_eq!(app_id, 200); with(|u| { u.reads += 1; u.icon }) }
        pub unsafe fn owns(source: u32) -> bool { PLAYBACK.contains(&source) }
        pub unsafe fn animation_active(active: bool, period: u32) -> bool {
            with(|u| { u.timer = active; u.period = if active { period } else { 1000 }; }); true
        }
        pub unsafe fn show_frame(icon: Icon, asset: &Animation<'_>, frame: usize) -> Option<u32> {
            if with(|u| u.fail_image) { return None; }
            assert_eq!(asset.count, 193);
            let mut pixels = vec![0; PIXELS]; assert!(asset.decode_frame(frame, &mut pixels));
            let source = if icon.source == PLAYBACK[0] { PLAYBACK[1] } else { PLAYBACK[0] };
            with(|u| {
                assert!(u.screen); let current = u.icon.as_mut().expect("stale handle");
                assert_eq!((icon.object, icon.source), (current.object, current.source));
                assert_ne!(source, icon.source);
                current.source = source; current.original = false;
                u.frame = Some(frame); u.writes.push((icon.object, 1, frame));
            });
            Some(source)
        }
        pub unsafe fn restore_original(icon: Icon) {
            with(|u| {
                assert!(u.screen); let current = u.icon.as_mut().expect("stale handle");
                assert_eq!((icon.object, icon.source), (current.object, current.source));
                if u.fail_restore { return; }
                current.source = ORIGINAL; current.original = true;
                u.frame = None; u.writes.push((icon.object, 0, 0));
            });
        }
        pub unsafe fn retire_frames() {
            with(|u| { assert!(u.icon.unwrap().original, "must confirm restoration before evicting frames"); u.retired += 1; });
        }
    }
}

#[test]
fn fresh_install_automatically_selects_plays_and_saves_dynamic() {
    setup(None); assert_eq!(unsafe { dynamic_icon::selected() }, 1); tick(0);
    with(|u| { assert_eq!(u.frame, Some(0)); assert!(u.timer); assert_eq!(u.files[FILE], b"AIC2\x01"); });
}
#[test]
fn picker_contains_only_original_and_dynamic() {
    assert_eq!(dynamic_icon::NAMES, ["原版\0".as_bytes(), "动态\0".as_bytes()]);
}
#[test]
fn every_old_choice_and_legacy_switch_upgrade_to_dynamic_once() {
    for old in 0..5 {
        setup(Some(&[b'A', b'I', b'C', b'1', old])); tick(0);
        with(|u| { assert_eq!(u.files[FILE], b"AIC2\x01"); assert_eq!(u.frame, Some(0)); });
    }
    for flag in [0, 1] {
        setup(None); with(|u| { u.files.insert(b"/data/chaos/dynamic-icon.cfg".to_vec(), vec![b'D',b'I',b'C',b'1',flag]); });
        tick(0); with(|u| assert_eq!(u.files[FILE], b"AIC2\x01"));
    }
}
#[test]
fn both_choices_persist_after_upgrade_and_reload() {
    for style in 0..2 {
        setup(None); tick(0); choose(style); tick(50);
        let saved = with(|u| u.files[FILE].clone());
        assert_eq!(saved, [b'A',b'I',b'C',b'2',style]); setup(Some(&saved));
        assert_eq!(unsafe { dynamic_icon::selected() }, style); tick(0);
        with(|u| { assert_eq!(u.timer, style == 1); assert_eq!(u.commits, 0); });
    }
}
#[test]
fn invalid_records_default_to_dynamic_and_removed_choices_are_rejected() {
    for bytes in [b"AIC2".as_slice(), b"AIC2\x00x", b"BAD!\x00", b"AIC2\x02"] {
        setup(Some(bytes)); assert_eq!(unsafe { dynamic_icon::selected() }, 1);
        for index in 2..5 { assert!(!unsafe { dynamic_icon::select(index) }); }
    }
}
#[test]
fn saved_original_remains_static_without_timer_or_writes() {
    setup(Some(b"AIC2\x00")); tick(0); tick(1000);
    with(|u| { assert!(u.writes.is_empty()); assert_eq!(u.commits,0); assert!(!u.timer); });
}
#[test]
fn dynamic_keeps_the_uploaded_gif_timing_and_twenty_ms_timer() {
    setup(Some(b"AIC2\x01")); let asset=dynamic_icon_frames::Animation::parse(dynamic_icon_frames::PACKAGED).unwrap();
    for elapsed in (0..=5200).step_by(20) {
        if elapsed==0 { tick(elapsed); } else { animation_tick(elapsed); }
        with(|u| { assert_eq!(u.frame,Some(asset.frame_at(elapsed))); assert!(u.timer); assert_eq!(u.period,20); });
    }
    assert_eq!(with(|u| u.writes.len()),194);
}
#[test]
fn switching_to_original_confirms_native_file_assignment_before_eviction() {
    setup(Some(b"AIC2\x01")); tick(0); choose(0); animation_tick(50);
    with(|u| { assert!(u.icon.unwrap().original); assert!(!u.timer); assert_eq!(u.retired,0); });
    tick(100); with(|u| assert_eq!(u.retired,1)); tick(150); with(|u| assert_eq!(u.retired,1));
}
#[test]
fn file_assignment_oom_retries_even_after_leaving_the_launcher() {
    setup(Some(b"AIC2\x01")); tick(0); choose(0);
    with(|u| { u.fail_restore=true; u.active=false; }); tick(50);
    with(|u| { assert!(!u.icon.unwrap().original); assert_eq!(u.retired,0); u.fail_restore=false; });
    tick(100); with(|u| assert!(u.icon.unwrap().original)); tick(150); with(|u| assert_eq!(u.retired,1));
}
#[test]
fn dynamic_pauses_to_original_and_restarts_when_visible() {
    setup(Some(b"AIC2\x01")); tick(0); animation_tick(500);
    with(|u| u.active=false); animation_tick(520); tick(570);
    with(|u| { assert!(u.icon.unwrap().original); assert!(!u.timer); u.active=true; });
    tick(1000); assert_eq!(with(|u| u.frame),Some(0));
}
#[test]
fn sleep_stops_fast_timer_without_scanning_or_drawing() {
    setup(Some(b"AIC2\x01")); tick(0); animation_tick(500);
    let before=with(|u| { u.screen=false; (u.reads,u.writes.len()) });
    animation_tick(520); tick(1600);
    with(|u| { assert_eq!((u.reads,u.writes.len()),before); assert!(!u.timer); u.screen=true; });
    tick(3000); assert_eq!(with(|u| u.frame),Some(0));
}
#[test]
fn rebuilt_or_missing_launcher_never_writes_old_instances() {
    setup(Some(b"AIC2\x01")); tick(0); with(|u| u.icon=None); tick(50);
    with(|u| u.icon=Some(original(456))); tick(100);
    assert_eq!(with(|u| u.writes.last().copied()),Some((456,1,0)));
}
#[test]
fn external_sources_are_preserved_until_explicit_picker_action() {
    setup(Some(b"AIC2\x01")); tick(0);
    let writes=with(|u| { let icon=u.icon.as_mut().unwrap(); icon.source=777; icon.original=false; u.writes.len() });
    tick(50); with(|u| assert_eq!(u.writes.len(),writes)); choose(1); tick(100);
    assert_eq!(with(|u| u.frame),Some(0));
}
#[test]
fn short_io_and_rapid_choices_save_only_the_final_selection() {
    setup(Some(b"AIC2\x01")); with(|u| u.chunk=2); choose(0); choose(1); choose(0);
    with(|u| assert_eq!(u.commits,0)); tick(0);
    with(|u| { assert_eq!(u.commits,1); assert_eq!(u.files[FILE],b"AIC2\x00"); assert!(u.handles.is_empty()); });
}
#[test]
fn failed_config_save_preserves_the_old_record_and_retries() {
    for rename in [false,true] {
        setup(Some(b"AIC2\x01")); choose(0);
        with(|u| if rename { u.fail_rename=true; } else { u.fail_write=true; }); tick(0);
        let opens=with(|u| { assert_eq!(u.files[FILE],b"AIC2\x01"); assert!(!u.files.contains_key(TEMP)); u.opens });
        tick(999); with(|u| { assert_eq!(u.opens,opens); u.fail_rename=false; u.fail_write=false; });
        tick(1000); with(|u| assert_eq!(u.files[FILE],b"AIC2\x00"));
    }
}
#[test]
fn failed_frame_application_is_retried() {
    setup(None); with(|u| u.fail_image=true); tick(0); assert_eq!(with(|u| u.frame),None);
    with(|u| u.fail_image=false); tick(50); assert_eq!(with(|u| u.frame),Some(0));
}
#[test]
fn clock_wrap_keeps_gif_time() {
    setup(Some(b"AIC2\x01")); let start=u32::MAX-100; tick(start); animation_tick(start.wrapping_add(500));
    let asset=dynamic_icon_frames::Animation::parse(dynamic_icon_frames::PACKAGED).unwrap();
    assert_eq!(with(|u| u.frame),Some(asset.frame_at(500)));
}
