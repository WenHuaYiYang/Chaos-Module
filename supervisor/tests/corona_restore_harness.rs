//! Host regression tests for production legacy-surface restoration functions.
#![allow(dead_code, static_mut_refs)]

use std::collections::BTreeMap;
use std::sync::Mutex;
macro_rules! st_rd { ($value:expr) => { $value }; }
macro_rules! st_wr { ($slot:expr, $value:expr) => { $slot = $value }; }
const LIMIT: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq)]
struct Surface { object: u32, root: u32, prop: u32, original: u32, desired: u32, tag: u32 }
const EMPTY: Surface = Surface { object: 0, root: 0, prop: 0, original: 0, desired: 0, tag: 0 };
static mut SURFACES: [Surface; LIMIT] = [EMPTY; LIMIT];
static mut EVENTS: [(u32, u32, u32); LIMIT] = [(0, 0, 0); LIMIT];
static mut EVENT_MASK: u128 = 0;
static mut ACTIVE: u32 = 0;
static mut WORK: u128 = 0;
static mut SCAN: u32 = 0;
static mut PAGE_SCAN: u32 = 0;
static mut PAGE_ROOTS: [u32; 2] = [0; 2];
static mut BACKGROUND: u32 = 0;
struct State { values: BTreeMap<(u32, u32), u32>, parents: BTreeMap<u32, u32>, writes: Vec<(u32, u32, u32)> }
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut State) -> T) -> T { f(STATE.lock().unwrap().as_mut().unwrap()) }
fn reset() {
    *STATE.lock().unwrap() = Some(State { values: BTreeMap::new(), parents: BTreeMap::new(), writes: Vec::new() });
    unsafe {
        SURFACES = [EMPTY; LIMIT]; EVENTS = [(0, 0, 0); LIMIT];
        EVENT_MASK = 0; ACTIVE = 0; WORK = 0; SCAN = 0; PAGE_SCAN = 0;
        PAGE_ROOTS = [0; 2]; BACKGROUND = 0x1234;
    }
}
unsafe fn belongs(object: u32, root: u32) -> bool { with(|s| s.parents.get(&object) == Some(&root)) }
unsafe fn value(object: u32, prop: u32) -> u32 { with(|s| s.values[&(object, prop)]) }
unsafe fn write(object: u32, prop: u32, value: u32) {
    assert_eq!(ACTIVE, 0);
    with(|s| { s.values.insert((object, prop), value); s.writes.push((object, prop, value)); });
}
unsafe fn entry(slot: usize, object: u32, root: u32, original: u32, current: u32) {
    SURFACES[slot] = Surface { object, root, prop: 29, original, desired: current, tag: 128 + slot as u32 };
    with(|s| { s.parents.insert(object, root); s.values.insert((object, 29), current); });
}

// Extracted current production bodies: restore, consume, owns_root, pending, clear_background.
include!(env!("CHAOS_CORONA_RESTORE_SOURCE"));

#[test]
fn active_missing_roots_preserves_restore_work_and_descriptor() {
    reset(); unsafe {
        ACTIVE = 1; WORK = 0b101; SCAN = 1; PAGE_SCAN = 1;
        entry(0, 100, 10, 255, 0); let original = SURFACES;
        assert!(!restore(None));
        assert_eq!(ACTIVE, 1); assert_eq!(WORK, 0b101); assert_eq!(SCAN, 1);
        assert_eq!(PAGE_SCAN, 1); assert_eq!(BACKGROUND, 0x1234); assert_eq!(SURFACES, original);
        assert!(pending());
    }
    with(|s| assert!(s.writes.is_empty()));
}

#[test]
fn inactive_missing_roots_is_already_safe() {
    reset(); unsafe { assert!(restore(None)); assert_eq!(ACTIVE, 0); assert!(!pending()); }
    with(|s| assert!(s.writes.is_empty()));
}

#[test]
fn retry_with_live_roots_restores_style_then_releases_pending_work() {
    reset(); unsafe {
        ACTIVE = 1; WORK = 1; SCAN = 1; entry(0, 100, 10, 255, 0);
        assert!(!restore(None)); assert!(restore(Some([10, 20, 30, 40])));
        assert_eq!(ACTIVE, 0); assert_eq!(WORK, 0); assert_eq!(SCAN, 0); assert!(!pending());
        assert_eq!(BACKGROUND, 0x1234); clear_background(); assert_eq!(BACKGROUND, 0);
    }
    with(|s| assert_eq!(s.writes, [(100, 29, 255)]));
}

#[test]
fn missing_roots_still_consumes_delete_without_discarding_other_work() {
    reset(); unsafe {
        ACTIVE = 1; WORK = 0b11; entry(0, 100, 10, 255, 0); entry(1, 101, 10, 255, 0);
        EVENTS[0] = (SURFACES[0].tag, 100, 2); EVENT_MASK = 1;
        assert!(!restore(None));
        assert_eq!(ACTIVE, 1); assert_eq!(WORK, 0b11); assert_eq!(SCAN, 1);
        assert_eq!(EVENT_MASK, 0); assert_eq!(SURFACES[0], EMPTY); assert_eq!(SURFACES[1].object, 101);
        assert!(restore(Some([10, 20, 30, 40])));
    }
    with(|s| assert_eq!(s.writes, [(101, 29, 255)]));
}

#[test]
fn style_event_during_missing_roots_remains_retryable() {
    reset(); unsafe {
        ACTIVE = 1; entry(0, 100, 10, 255, 0);
        EVENTS[0] = (SURFACES[0].tag, 100, 1); EVENT_MASK = 1;
        assert!(!restore(None)); assert_eq!(WORK, 1); assert!(pending());
    }
    with(|s| assert!(s.writes.is_empty()));
}

#[test]
fn restoration_skips_foreign_or_detached_objects_and_unchanged_values() {
    reset(); unsafe {
        ACTIVE = 1; WORK = 0b11111; PAGE_ROOTS = [50, 0];
        entry(0, 100, 10, 255, 0); entry(1, 101, 99, 255, 0);
        entry(2, 102, 20, 255, 0); entry(3, 103, 30, 255, 255);
        entry(4, 104, 50, 150, 0);
        with(|s| { s.parents.insert(102, 99); });
        assert!(restore(Some([10, 20, 30, 40]))); assert_eq!(ACTIVE, 0); assert_eq!(WORK, 0);
    }
    with(|s| assert_eq!(s.writes, [(100, 29, 255), (104, 29, 150)]));
}
