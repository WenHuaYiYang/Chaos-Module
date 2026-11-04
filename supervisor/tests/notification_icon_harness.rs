//! Host tests that include the complete production notification-icon owner.
//! Firmware addresses are virtual u32 values; invalid object reads fail the test.
#![allow(dead_code, static_mut_refs)]

use std::collections::BTreeMap;
use std::sync::Mutex;

#[macro_export]
macro_rules! st_rd { ($value:expr) => { $value }; }
#[macro_export]
macro_rules! st_wr { ($slot:expr, $value:expr) => { $slot = $value }; }

const HOME: u32 = 0x2000_1000;
const PAGE: u32 = 0x2000_2000;
const OTHER: u32 = 0x2000_3000;
const ICON: u32 = 0x2001_0000;
const ICON2: u32 = 0x2001_1000;
const PATH: u32 = 0x2C00_1000;
const OTHER_PATH: u32 = 0x2C00_2000;
const REPLACEMENT: u32 = 0x200E_0000;
const EVENT: u32 = 0x200F_0000;
const IMAGE_CLASS: u32 = 0x2CCE_61EC;
const CONTAINER_CLASS: u32 = 0x2CCE_6000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Setter { Apply, Ignore, Delete, Style, DeleteAncestors }
#[derive(Clone, Copy, Debug)]
struct Registration { object: u32, callback: u32, code: u32, tag: u32 }
struct State {
    bytes: BTreeMap<u32, u8>,
    children: BTreeMap<u32, Vec<u32>>,
    roots: Option<[u32; 4]>,
    page_root: u32,
    reads: Vec<(u32, usize)>,
    tree_reads: Vec<u32>,
    writes: Vec<(u32, u32)>,
    registrations: Vec<Registration>,
    register_attempts: u32,
    registration_ok: bool,
    setter: Setter,
    empty_source: u32,
    empty_calls: u32,
}
impl State {
    fn new() -> Self {
        Self { bytes: BTreeMap::new(), children: BTreeMap::new(),
            roots: Some([0, 0, 0, HOME]), page_root: 0, reads: Vec::new(),
            tree_reads: Vec::new(), writes: Vec::new(), registrations: Vec::new(),
            register_attempts: 0, registration_ok: true, setter: Setter::Apply,
            empty_source: REPLACEMENT, empty_calls: 0 }
    }
    fn put(&mut self, address: u32, data: &[u8]) {
        for (i, byte) in data.iter().enumerate() { self.bytes.insert(address + i as u32, *byte); }
    }
    fn word(&mut self, address: u32, value: u32) { self.put(address, &value.to_le_bytes()); }
    fn object(&mut self, address: u32, class: u32, source: u32) {
        self.put(address, &[0; 0x38]);
        self.word(address, class); self.word(address + 0x34, source);
        self.children.insert(address, Vec::new());
    }
    fn peek_word(&self, address: u32) -> u32 {
        u32::from_le_bytes(std::array::from_fn(|i| self.bytes[&(address + i as u32)]))
    }
    fn remove_object(&mut self, object: u32) {
        for children in self.children.values_mut() { children.retain(|child| *child != object); }
        self.children.remove(&object);
        self.bytes.retain(|address, _| *address < object || *address >= object + 0x38);
    }
}
static STATE: Mutex<Option<State>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(STATE.lock().unwrap_or_else(|poison| poison.into_inner()).as_mut().unwrap())
}
fn reset() {
    *STATE.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(State::new());
    with(|s| {
        s.put(PATH, background_assets::EMPTY_PATH);
        s.put(original_path(), background_assets::EMPTY_PATH);
        s.put(REPLACEMENT, &[25]); // descriptor, deliberately not a C path
        s.object(HOME, CONTAINER_CLASS, 0);
    });
    unsafe { notification_icon::reset_for_test(); background_assets::reset_for_test(); }
}
fn original_path() -> u32 { background_assets::EMPTY_PATH.as_ptr() as u32 }
fn add_icon(root: u32, object: u32, source: u32) {
    with(|s| {
        s.object(object, IMAGE_CLASS, source);
        s.children.get_mut(&root).unwrap().push(object);
    });
}
fn setup_icon() { reset(); add_icon(HOME, ICON, PATH); }
fn tag(object: u32) -> u32 {
    with(|s| s.registrations.iter().rev().find(|entry| entry.object == object).unwrap().tag)
}
fn source(object: u32) -> u32 { with(|s| s.peek_word(object + 0x34)) }
fn dispatch(target: u32, current: u32, code: u16, tag: u32) {
    with(|s| {
        s.put(EVENT, &[0; 16]); s.word(EVENT, target); s.word(EVENT + 4, current);
        s.put(EVENT + 8, &code.to_le_bytes()); s.word(EVENT + 12, tag);
    });
    unsafe { notification_icon::dispatch_for_test(EVENT); }
}

mod mem {
    pub unsafe fn plausible_ptr(value: u32) -> bool {
        (0x0C00_0000..=0x0DFF_FFFF).contains(&value)
            || (0x2C00_0000..=0x2DFF_FFFF).contains(&value)
            || (0x1C00_0000..=0x1DFF_FFFF).contains(&value)
            || (0x2000_0000..=0x2400_FFFF).contains(&value)
            || (0x3000_0000..=0x3FFF_FFFF).contains(&value)
            // Restore uses the production static C-string, truncated by the
            // target u32 ABI. Its host-only mapped alias is also readable.
            || value.wrapping_sub(crate::original_path()) < crate::background_assets::EMPTY_PATH.len() as u32
    }
    pub unsafe fn safe_ptr(value: u32) -> bool {
        value & 3 == 0 && ((0x2000_0000..=0x2400_FFFF).contains(&value)
            || (0x3000_0000..=0x3FFF_FFFF).contains(&value))
    }
    fn read<const N: usize>(address: u32) -> [u8; N] {
        crate::with(|s| {
            s.reads.push((address, N));
            std::array::from_fn(|i| *s.bytes.get(&(address + i as u32))
                .unwrap_or_else(|| panic!("unmapped firmware read at {:#x}", address + i as u32)))
        })
    }
    pub unsafe fn rd8(address: *const u8) -> u8 { read::<1>(address as u32)[0] }
    pub unsafe fn rd16(address: *const u16) -> u16 { u16::from_le_bytes(read(address as u32)) }
    pub unsafe fn rd32(address: *const u32) -> u32 { u32::from_le_bytes(read(address as u32)) }
}
mod background_assets {
    static mut EMPTY_IMAGE: u32 = 0;
    unsafe fn load(path: &[u8], width: u8, height: u8, remove_black: bool) -> u32 {
        assert_eq!(path, EMPTY_PATH); assert_eq!((width, height), (96, 96)); assert!(remove_black);
        crate::with(|s| { s.empty_calls += 1; s.empty_source })
    }
    include!(env!("CHAOS_NOTIFICATION_ASSET_SOURCE"));
    pub unsafe fn reset_for_test() { EMPTY_IMAGE = 0; }
}
mod fw_api {
    pub unsafe fn background_home_targets() -> Option<[u32; 4]> { crate::with(|s| s.roots) }
    pub unsafe fn background_notification_page_root() -> u32 { crate::with(|s| s.page_root) }
    pub unsafe fn obj_child_count(object: u32) -> i32 {
        crate::with(|s| {
            s.tree_reads.push(object);
            s.children.get(&object).unwrap_or_else(|| panic!("child count on deleted object {object:#x}")).len() as i32
        })
    }
    pub unsafe fn obj_get_child(object: u32, index: u32) -> u32 {
        crate::with(|s| {
            s.tree_reads.push(object);
            s.children.get(&object).unwrap_or_else(|| panic!("child read on deleted object {object:#x}"))
                .get(index as usize).copied().unwrap_or(0)
        })
    }
    pub unsafe fn obj_add_event(object: u32, callback: u32, code: u32, tag: u32) -> u32 {
        crate::with(|s| {
            assert!(s.children.contains_key(&object));
            assert_eq!(callback & 1, 1, "target callback must be Thumb");
            assert_eq!(code, 0x24, "register only DELETE, never STYLE_CHANGED");
            s.register_attempts += 1;
            if !s.registration_ok { return 0; }
            s.registrations.push(crate::Registration { object, callback, code, tag });
            1
        })
    }
    pub unsafe fn background_image_set_source(object: u32, source: u32) {
        let events = crate::with(|s| {
            assert!(s.children.contains_key(&object));
            s.writes.push((object, source));
            if s.setter == crate::Setter::Ignore { return Vec::new(); }
            s.word(object + 0x34, source);
            if s.setter == crate::Setter::DeleteAncestors {
                let objects: Vec<u32> = s.children.keys().copied().collect();
                let events = s.registrations.iter().filter(|entry| objects.contains(&entry.object))
                    .map(|entry| (entry.object, 0x24, entry.tag)).collect();
                for dead in objects { s.remove_object(dead); }
                s.roots = None; s.page_root = 0;
                return events;
            }
            if matches!(s.setter, crate::Setter::Delete | crate::Setter::Style) {
                let tag = s.registrations.iter().rev().find(|entry| entry.object == object).unwrap().tag;
                if s.setter == crate::Setter::Delete { s.remove_object(object); vec![(object, 0x24, tag)] }
                else { vec![(object, 0x2D, tag)] }
            } else { Vec::new() }
        });
        for (target, code, tag) in events { crate::dispatch(target, target, code, tag); }
    }
}
#[allow(unused_imports)]
mod notification_icon {
    // The runner changes only inner doc comments, not production instructions.
    include!(env!("CHAOS_NOTIFICATION_ICON_SOURCE"));
    pub unsafe fn reset_for_test() {
        ENTRIES = [EMPTY; LIMIT]; NEXT_TAG = 1; ACTIVE = false; SOURCE = 0; RETRY_TICKS = 0;
    }
    pub unsafe fn dispatch_for_test(event: u32) { changed(event); }
    pub unsafe fn owned_for_test(object: u32) -> bool {
        (0..LIMIT).any(|slot| ENTRIES[slot].object == object && ENTRIES[slot].owned)
    }
    pub unsafe fn registered_for_test(object: u32) -> bool {
        (0..LIMIT).any(|slot| ENTRIES[slot].object == object)
    }
}
mod palette {
    unsafe fn rd8(address: *const u8) -> u8 { core::ptr::read_volatile(address) }
    include!(env!("CHAOS_NOTIFICATION_PALETTE_SOURCE"));
    pub unsafe fn apply(bytes: &mut [u8]) {
        assert!(bytes.len() >= 1024); clear_black_palette(bytes.as_mut_ptr());
    }
}

#[test]
fn disabled_default_has_no_reads_allocations_or_writes() {
    reset();
    unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
    with(|s| { assert!(s.reads.is_empty()); assert!(s.writes.is_empty()); assert_eq!(s.empty_calls, 0); });
}

#[test]
fn exact_path_including_nul_is_required() {
    assert_eq!(background_assets::EMPTY_PATH, b"/resource/app/notifications/no_message.bin\0");
    for path in [
        b"/resource/app/notifications/no_message.bin\0".as_slice(),
        b"/resource/app/notifications/no_message.bin.bak\0".as_slice(),
        b"/resource/app/notifications/swipe_none.bin\0".as_slice(),
        b"/resource/app/notifications/No_Message.bin\0".as_slice(),
        b"/resource/app/notifications/no_message.bi\0".as_slice(),
        b"/resource/app/other/no_message.bin\0".as_slice(),
    ] {
        setup_icon(); with(|s| { s.bytes.retain(|address, _| !(*address >= PATH && *address < PATH + 128)); s.put(PATH, path); });
        unsafe { assert!(notification_icon::tick(true)); }
        let expected = path == background_assets::EMPTY_PATH;
        with(|s| { assert_eq!(s.writes.len(), expected as usize); assert_eq!(s.registrations.len(), expected as usize); });
        assert_eq!(source(ICON), if expected { REPLACEMENT } else { PATH });
    }
}

#[test]
fn non_image_class_never_reads_or_changes_its_source() {
    setup_icon(); with(|s| s.word(ICON, CONTAINER_CLASS));
    unsafe { assert!(notification_icon::tick(true)); }
    with(|s| { assert!(s.writes.is_empty()); assert!(!s.reads.contains(&(ICON + 0x34, 4))); });
}

#[test]
fn descriptor_source_is_rejected_after_its_first_byte() {
    reset(); add_icon(HOME, ICON, REPLACEMENT);
    unsafe { assert!(notification_icon::tick(true)); }
    with(|s| { assert!(s.writes.is_empty()); assert!(s.reads.contains(&(REPLACEMENT, 1))); });
}

#[test]
fn normal_enable_and_disable_replace_then_restore_original_path() {
    setup_icon();
    unsafe { assert!(notification_icon::tick(true)); assert!(notification_icon::pending()); }
    assert_eq!(source(ICON), REPLACEMENT);
    unsafe { assert!(notification_icon::owned_for_test(ICON)); assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
    assert_eq!(source(ICON), original_path());
    with(|s| assert_eq!(s.writes, [(ICON, REPLACEMENT), (ICON, original_path())]));
}

#[test]
fn repeated_enable_does_not_write_or_register_the_same_live_icon_again() {
    setup_icon();
    unsafe { for _ in 0..20 { assert!(notification_icon::tick(true)); } }
    with(|s| { assert_eq!(s.writes, [(ICON, REPLACEMENT)]); assert_eq!(s.registrations.len(), 1); assert_eq!(s.empty_calls, 1); });
}

#[test]
fn repeated_toggle_reuses_one_delete_listener_without_exhausting_slots() {
    setup_icon();
    unsafe { for _ in 0..300 { assert!(notification_icon::tick(true)); assert!(notification_icon::tick(false)); } }
    with(|s| { assert_eq!(s.writes.len(), 600); assert_eq!(s.registrations.len(), 1); assert_eq!(s.register_attempts, 1); });
    unsafe { assert!(notification_icon::registered_for_test(ICON)); assert!(!notification_icon::owned_for_test(ICON)); }
}

#[test]
fn style_event_never_discards_restore_ownership() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    dispatch(ICON, ICON, 0x2D, tag(ICON));
    unsafe { assert!(notification_icon::owned_for_test(ICON)); assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn delete_event_releases_entry_without_reading_the_dead_object() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    let old_tag = tag(ICON);
    with(|s| { s.remove_object(ICON); s.reads.clear(); });
    dispatch(ICON, ICON, 0x8024, old_tag);
    unsafe { assert!(!notification_icon::registered_for_test(ICON)); assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
    with(|s| assert!(s.reads.iter().all(|(address, _)| *address < ICON || *address >= ICON + 0x38)));
}

#[test]
fn address_reuse_gets_a_new_tag_and_stale_delete_cannot_clear_it() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    let old_tag = tag(ICON);
    with(|s| s.remove_object(ICON)); dispatch(ICON, ICON, 0x24, old_tag);
    add_icon(HOME, ICON, PATH);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_ne!(tag(ICON), old_tag);
    dispatch(ICON, ICON, 0x24, old_tag);
    unsafe { assert!(notification_icon::owned_for_test(ICON)); assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn bubbled_or_wrong_target_delete_cannot_clear_ownership() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    dispatch(ICON, HOME, 0x24, tag(ICON));
    dispatch(ICON2, ICON2, 0x24, tag(ICON));
    unsafe { notification_icon::dispatch_for_test(0); assert!(notification_icon::owned_for_test(ICON)); }
}

#[test]
fn failed_registration_does_not_write_and_retries_successfully() {
    setup_icon(); with(|s| s.registration_ok = false);
    unsafe { assert!(!notification_icon::tick(true)); assert!(notification_icon::pending()); assert!(!notification_icon::registered_for_test(ICON)); }
    with(|s| { assert!(s.writes.is_empty()); s.registration_ok = true; });
    unsafe { assert!(notification_icon::tick(true)); }
    with(|s| { assert_eq!(s.register_attempts, 2); assert_eq!(s.registrations.len(), 1); assert_eq!(s.writes, [(ICON, REPLACEMENT)]); });
}

#[test]
fn failed_enable_setter_reuses_listener_and_can_retry_or_exit() {
    for retry in [false, true] {
        setup_icon(); with(|s| s.setter = Setter::Ignore);
        unsafe { assert!(!notification_icon::tick(true)); assert!(!notification_icon::owned_for_test(ICON)); }
        assert_eq!(source(ICON), PATH);
        if retry {
            with(|s| s.setter = Setter::Apply);
            unsafe { assert!(notification_icon::tick(true)); assert!(notification_icon::owned_for_test(ICON)); }
        }
        unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
        with(|s| assert_eq!(s.registrations.len(), 1));
    }
}

#[test]
fn failed_restore_setter_keeps_ownership_until_the_next_successful_retry() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    with(|s| s.setter = Setter::Ignore);
    unsafe { assert!(!notification_icon::tick(false)); assert!(notification_icon::pending()); assert!(notification_icon::owned_for_test(ICON)); }
    assert_eq!(source(ICON), REPLACEMENT);
    with(|s| s.setter = Setter::Apply);
    unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn delete_during_either_setter_does_not_read_the_object_after_callback() {
    for restoring in [false, true] {
        setup_icon();
        if restoring { unsafe { assert!(notification_icon::tick(true)); } }
        with(|s| s.setter = Setter::Delete);
        unsafe { assert!(notification_icon::tick(!restoring)); assert!(!notification_icon::registered_for_test(ICON)); }
        with(|s| assert!(!s.children.contains_key(&ICON)));
    }
}

#[test]
fn child_setter_deleting_ancestors_aborts_all_old_root_and_sibling_access() {
    for restoring in [false, true] {
        reset();
        with(|s| {
            s.object(OTHER, CONTAINER_CLASS, 0);
            s.children.get_mut(&HOME).unwrap().push(OTHER);
            s.object(PAGE, CONTAINER_CLASS, 0); s.page_root = PAGE;
        });
        add_icon(OTHER, ICON, PATH);
        add_icon(PAGE, ICON2, PATH);
        if restoring {
            unsafe { assert!(notification_icon::tick(true)); assert!(notification_icon::tick(true)); }
            assert_eq!(source(ICON), REPLACEMENT); assert_eq!(source(ICON2), REPLACEMENT);
        }
        with(|s| { s.setter = Setter::DeleteAncestors; s.writes.clear(); });
        unsafe {
            assert!(notification_icon::tick(!restoring));
            assert!(!notification_icon::registered_for_test(ICON));
            assert!(!notification_icon::registered_for_test(ICON2));
            assert!(notification_icon::tick(false));
        }
        with(|s| { assert!(s.children.is_empty()); assert_eq!(s.writes.len(), 1); });
    }
}

#[test]
fn style_during_setter_is_ignored_and_still_restores() {
    setup_icon(); with(|s| s.setter = Setter::Style);
    unsafe { assert!(notification_icon::tick(true)); assert!(notification_icon::owned_for_test(ICON)); assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn missing_roots_defer_enable_and_restore_without_losing_work() {
    setup_icon(); with(|s| s.roots = None);
    unsafe { assert!(!notification_icon::tick(true)); assert!(notification_icon::pending()); }
    with(|s| { assert!(s.writes.is_empty()); s.roots = Some([0, 0, 0, HOME]); });
    unsafe { assert!(notification_icon::tick(true)); }
    with(|s| { s.roots = None; s.reads.clear(); });
    unsafe { assert!(!notification_icon::tick(false)); assert!(notification_icon::pending()); }
    with(|s| { assert!(s.reads.is_empty()); s.roots = Some([0, 0, 0, HOME]); });
    unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
}

#[test]
fn detached_unmapped_icon_is_never_blindly_read_from_the_ledger() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    let old_tag = tag(ICON);
    with(|s| { s.remove_object(ICON); s.reads.clear(); });
    unsafe { assert!(!notification_icon::tick(false)); assert!(notification_icon::pending()); }
    with(|s| assert!(s.reads.iter().all(|(address, _)| *address < ICON || *address >= ICON + 0x38)));
    dispatch(ICON, ICON, 0x24, old_tag);
    unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
}

#[test]
fn detached_live_icon_can_be_restored_after_reattachment() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    with(|s| s.children.get_mut(&HOME).unwrap().clear());
    unsafe { assert!(!notification_icon::tick(false)); }
    with(|s| s.children.get_mut(&HOME).unwrap().push(ICON));
    unsafe { assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn third_party_source_is_preserved_when_releasing_ownership() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    with(|s| { s.word(ICON + 0x34, OTHER_PATH); s.put(OTHER_PATH, b"third-party\0"); s.writes.clear(); });
    unsafe { assert!(notification_icon::tick(false)); assert!(!notification_icon::owned_for_test(ICON)); }
    assert_eq!(source(ICON), OTHER_PATH);
    with(|s| assert!(s.writes.is_empty()));
}

#[test]
fn initial_empty_image_failure_can_exit_without_roots_or_ui_writes() {
    setup_icon(); with(|s| { s.empty_source = 0; s.roots = None; });
    unsafe { assert!(!notification_icon::tick(true)); assert!(notification_icon::pending()); assert!(notification_icon::tick(false)); assert!(!notification_icon::pending()); }
    with(|s| { assert_eq!(s.empty_calls, 1); assert!(s.writes.is_empty()); assert!(s.reads.is_empty()); });
}

#[test]
fn cached_source_restores_even_if_empty_image_later_becomes_unavailable() {
    setup_icon(); unsafe { assert!(notification_icon::tick(true)); }
    with(|s| s.empty_source = 0);
    unsafe { assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
    with(|s| assert_eq!(s.empty_calls, 1));
}

#[test]
fn empty_image_failure_retries_after_twenty_ticks_without_repeated_io() {
    setup_icon(); with(|s| s.empty_source = 0);
    unsafe { assert!(!notification_icon::tick(true)); }
    with(|s| { assert_eq!(s.empty_calls, 1); s.empty_source = REPLACEMENT; });
    for _ in 0..20 {
        unsafe { assert!(!notification_icon::tick(true)); assert!(notification_icon::pending()); }
        with(|s| { assert_eq!(s.empty_calls, 1); assert!(s.writes.is_empty()); });
    }
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT);
    with(|s| assert_eq!(s.empty_calls, 2));
}

#[test]
fn disabling_after_failed_load_resets_backoff_for_a_new_enable() {
    setup_icon(); with(|s| s.empty_source = 0);
    unsafe { assert!(!notification_icon::tick(true)); assert!(notification_icon::tick(false)); }
    with(|s| s.empty_source = REPLACEMENT);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT);
    with(|s| assert_eq!(s.empty_calls, 2));
}

#[test]
fn production_empty_image_caches_only_successful_resource_loads() {
    reset(); with(|s| s.empty_source = 0);
    unsafe { assert_eq!(background_assets::empty_image(), 0); }
    with(|s| s.empty_source = REPLACEMENT);
    unsafe { assert_eq!(background_assets::empty_image(), REPLACEMENT); }
    with(|s| s.empty_source = 0);
    unsafe { assert_eq!(background_assets::empty_image(), REPLACEMENT); }
    with(|s| assert_eq!(s.empty_calls, 2));
}

#[test]
fn late_created_no_message_icon_is_found_on_the_next_active_tick() {
    reset(); unsafe { assert!(notification_icon::tick(true)); }
    add_icon(HOME, ICON, PATH);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT);
}

#[test]
fn non_notification_home_roots_are_never_scanned() {
    reset(); with(|s| { s.object(OTHER, CONTAINER_CLASS, 0); s.roots = Some([OTHER, OTHER, OTHER, HOME]); });
    add_icon(OTHER, ICON, PATH);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), PATH);
    with(|s| { assert!(s.writes.is_empty()); assert!(!s.tree_reads.contains(&OTHER)); });
}

#[test]
fn independent_notification_page_root_works_without_home_root() {
    reset(); with(|s| { s.roots = None; s.object(PAGE, CONTAINER_CLASS, 0); s.page_root = PAGE; });
    add_icon(PAGE, ICON, PATH);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT);
    unsafe { assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path());
}

#[test]
fn both_notification_roots_are_processed_and_restored() {
    setup_icon(); with(|s| { s.object(PAGE, CONTAINER_CLASS, 0); s.page_root = PAGE; });
    add_icon(PAGE, ICON2, PATH);
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT); assert_eq!(source(ICON2), PATH);
    with(|s| assert_eq!(s.writes.len(), 1));
    unsafe { assert!(notification_icon::tick(true)); }
    assert_eq!(source(ICON), REPLACEMENT); assert_eq!(source(ICON2), REPLACEMENT);
    unsafe { assert!(!notification_icon::tick(false)); assert!(notification_icon::pending()); }
    assert_eq!(source(ICON), original_path()); assert_eq!(source(ICON2), REPLACEMENT);
    unsafe { assert!(notification_icon::tick(false)); }
    assert_eq!(source(ICON), original_path()); assert_eq!(source(ICON2), original_path());
    with(|s| assert_eq!(s.registrations.len(), 2));
}

#[test]
fn duplicate_notification_roots_do_not_double_write_or_register() {
    setup_icon(); with(|s| s.page_root = HOME);
    unsafe { assert!(notification_icon::tick(true)); assert!(notification_icon::tick(false)); }
    with(|s| { assert_eq!(s.registrations.len(), 1); assert_eq!(s.writes.len(), 2); });
}

#[test]
fn palette_changes_only_pure_black_alpha_and_preserves_indices_and_guards() {
    let mut data = vec![0xA5; 16 + 1024 + 96 * 96 + 16];
    for index in 0..256 {
        let color = 16 + index * 4;
        let rgb = match index % 5 { 0 => [0, 0, 0], 1 => [1, 0, 0], 2 => [0, 1, 0], 3 => [0, 0, 1], _ => [255, 127, 31] };
        data[color..color + 3].copy_from_slice(&rgb); data[color + 3] = index as u8;
    }
    for index in 0..96 * 96 { data[16 + 1024 + index] = index as u8; }
    let before = data.clone();
    unsafe { palette::apply(&mut data[16..]); }
    for index in 0..data.len() {
        let relative = index.wrapping_sub(16);
        let black_alpha = relative < 1024 && relative % 4 == 3 && before[index - 3..index].iter().all(|byte| *byte == 0);
        assert_eq!(data[index], if black_alpha { 0 } else { before[index] }, "byte {index}");
    }
}

#[test]
fn verified_firmware_resource_keeps_header_colored_pattern_and_all_indices() {
    let Some(path) = option_env!("CHAOS_NOTIFICATION_RESOURCE") else {
        eprintln!("SKIP: no unpacked p67/3.101.043 no_message.bin supplied"); return;
    };
    let before = std::fs::read(path).unwrap();
    assert_eq!(before.len(), 12 + 1024 + 96 * 96);
    assert_eq!(&before[..12], &[25, 10, 0, 0, 96, 0, 96, 0, 96, 0, 0, 0]);
    let mut after = before.clone();
    unsafe { palette::apply(&mut after[12..]); }
    assert_eq!(&after[..12], &before[..12]);
    assert_eq!(&after[12 + 1024..], &before[12 + 1024..]);
    let mut changed = 0;
    let mut colored_pixels = 0;
    for index in 0..256 {
        let color = 12 + index * 4;
        assert_eq!(&after[color..color + 3], &before[color..color + 3]);
        if before[color..color + 3].iter().all(|byte| *byte == 0) {
            assert_eq!(after[color + 3], 0);
            changed += (before[color + 3] != 0) as usize;
        } else { assert_eq!(after[color + 3], before[color + 3]); }
    }
    for index in &before[12 + 1024..] {
        let color = 12 + *index as usize * 4;
        if before[color..color + 3].iter().any(|byte| *byte != 0) {
            assert_eq!(&after[color..color + 4], &before[color..color + 4]); colored_pixels += 1;
        }
    }
    assert!(changed > 0, "fixture must contain opaque/semitransparent pure black");
    assert!(colored_pixels > 0, "fixture must contain a visible non-black icon pattern");
}
