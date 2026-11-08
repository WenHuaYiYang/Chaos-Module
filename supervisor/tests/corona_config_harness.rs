//! Host configuration tests compiling the entire production corona_blur.rs.
//! Each scenario runs in a fresh process, so production lazy-load flags reset
//! naturally; no extracted function copies or test-only production resets.
#![allow(dead_code, static_mut_refs)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::process::Command;

const CORONA: &str = "/data/chaos/corona_blur.cfg";
const V2: &str = "/data/chaos/blur_v2.cfg";

#[path = "../src/corona_blur.rs"]
mod corona_blur;

#[derive(Default)]
struct State {
    files: BTreeMap<String, Vec<u8>>,
    fds: BTreeMap<i32, (String, u32)>,
    next_fd: i32,
    opens: Vec<(String, u32)>,
    reads: usize,
    writes: Vec<(String, Vec<u8>)>,
    closes: usize,
    read_result: Option<i32>,
    read_open_error: bool,
    write_open_errors: BTreeSet<String>,
    write_results: BTreeMap<String, i32>,
    remounts: usize,
    commits: Vec<(u32, u32)>,
    commit_result: i32,
    polls: usize,
    pending: u32,
    idle: i32,
}
thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|s| f(&mut s.borrow_mut()))
}
fn seed(path: &str, bytes: &[u8]) {
    with(|s| { s.files.insert(path.to_owned(), bytes.to_vec()); });
}
fn saved(path: &str) -> Vec<u8> { with(|s| s.files[path].clone()) }
fn assert_closed() { with(|s| assert!(s.fds.is_empty(), "all successful opens must be closed")); }

mod fw_api {
    pub mod oflag {
        pub const RDONLY: u32 = 1;
        pub const WRONLY: u32 = 2;
        pub const CREAT: u32 = 0x100;
    }
    pub unsafe fn open(path: *const u8, flags: u32, mode: u32) -> i32 {
        let path = super::CStr::from_ptr(path.cast()).to_str().unwrap().to_owned();
        assert!(path == super::CORONA || path == super::V2, "unexpected config path");
        super::with(|s| {
            s.opens.push((path.clone(), flags));
            if flags == oflag::RDONLY {
                assert_eq!(mode, 0);
                if s.read_open_error || !s.files.contains_key(&path) { return -1; }
            } else {
                assert_eq!(flags, oflag::WRONLY | oflag::CREAT);
                assert_eq!(mode, 0o640);
                if s.write_open_errors.contains(&path) { return -1; }
                s.files.entry(path.clone()).or_default();
            }
            s.next_fd += 1;
            let fd = s.next_fd;
            s.fds.insert(fd, (path, flags));
            fd
        })
    }
    pub unsafe fn read(fd: i32, out: *mut u8, len: u32) -> i32 {
        assert_eq!(len, 1);
        super::with(|s| {
            let (path, flags) = &s.fds[&fd];
            assert_eq!(*flags, oflag::RDONLY);
            s.reads += 1;
            if let Some(result) = s.read_result { return result; }
            match s.files[path].first() {
                Some(byte) => { *out = *byte; 1 }
                None => 0,
            }
        })
    }
    pub unsafe fn write(fd: i32, input: *const u8, len: u32) -> i32 {
        assert_eq!(len, 1);
        let bytes = std::slice::from_raw_parts(input, len as usize).to_vec();
        assert!(bytes[0] <= 1);
        super::with(|s| {
            let (path, flags) = s.fds[&fd].clone();
            assert_eq!(flags, oflag::WRONLY | oflag::CREAT);
            let result = s.write_results.get(&path).copied().unwrap_or(1);
            if result == 1 {
                let data = s.files.get_mut(&path).unwrap();
                if data.is_empty() { data.push(bytes[0]); } else { data[0] = bytes[0]; }
            }
            s.writes.push((path, bytes));
            result
        })
    }
    pub unsafe fn close(fd: i32) {
        super::with(|s| {
            assert!(s.fds.remove(&fd).is_some());
            s.closes += 1;
        });
    }
}
mod background_home {
    pub unsafe fn remount() { super::with(|s| s.remounts += 1); }
}

#[no_mangle]
pub extern "C" fn chaos_corona_backgrounds_commit(on: u32, mask: u32) -> i32 {
    with(|s| { s.commits.push((on, mask)); s.commit_result })
}
#[no_mangle]
pub extern "C" fn chaos_corona_backgrounds_poll() { with(|s| s.polls += 1); }
#[no_mangle]
pub extern "C" fn chaos_corona_backgrounds_pending() -> u32 { with(|s| s.pending) }
#[no_mangle]
pub extern "C" fn chaos_corona_backgrounds_idle() -> i32 { with(|s| s.idle) }
#[no_mangle]
pub extern "C" fn chaos_corona_backgrounds_owned() -> u32 { 1 }
#[no_mangle]
pub extern "C" fn chaos_corona_diagnostics(_out: *mut u8, _capacity: u32) -> u32 { 0 }

unsafe fn assert_modes(corona: bool, v2: bool) {
    assert_eq!(corona_blur::enabled(), corona);
    assert_eq!(corona_blur::v2_enabled(), v2);
    assert_eq!(corona_blur::active(), corona || v2);
}
fn reboot(corona: bool, v2: bool) {
    let bytes = with(|s| (s.files.get(CORONA).cloned(), s.files.get(V2).cloned()));
    let encode = |value: Option<Vec<u8>>| -> String {
        match value {
            None => "missing".to_owned(),
            Some(bytes) if bytes.is_empty() => "empty".to_owned(),
            Some(bytes) => bytes[0].to_string(),
        }
    };
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["probe", &encode(bytes.0), &encode(bytes.1),
               &u8::from(corona).to_string(), &u8::from(v2).to_string()])
        .status().unwrap();
    assert!(status.success(), "fresh-process restart probe failed");
}

#[derive(Clone, Copy)]
enum SaveFailure { Open, Write(i32) }
fn fail_save(path: &str, failure: SaveFailure) {
    with(|s| match failure {
        SaveFailure::Open => { s.write_open_errors.insert(path.to_owned()); },
        SaveFailure::Write(result) => { s.write_results.insert(path.to_owned(), result); },
    });
}
fn allow_save(path: &str) {
    with(|s| { s.write_open_errors.remove(path); s.write_results.remove(path); });
}
fn io_counts() -> (usize, usize, usize) {
    with(|s| (s.opens.len(), s.writes.len(), s.closes))
}
unsafe fn save_failure_retries(initial: bool, failure: SaveFailure) {
    let target = !initial;
    seed(CORONA, &[u8::from(initial)]); seed(V2, &[u8::from(initial)]);
    assert_modes(initial, initial);
    fail_save(CORONA, failure); fail_save(V2, failure);
    corona_blur::set(target); corona_blur::set_v2_enabled(target);
    assert_modes(target, target);
    assert_eq!(saved(CORONA), [u8::from(initial)]);
    assert_eq!(saved(V2), [u8::from(initial)]);
    with(|s| assert_eq!(s.remounts, 2, "failed saves must still apply immediately"));
    assert_closed();

    let before = io_counts();
    for _ in 0..100 {
        assert_modes(target, target);
        corona_blur::backgrounds_poll();
    }
    assert_eq!(io_counts(), before, "reads/polls must not retry failed saves");

    corona_blur::set(target); corona_blur::set_v2_enabled(target);
    with(|s| assert_eq!(s.opens.len(), 6, "same-value calls must retry each failed switch"));
    assert_closed();

    allow_save(CORONA);
    corona_blur::set(target);
    assert_eq!(saved(CORONA), [u8::from(target)]);
    assert_eq!(saved(V2), [u8::from(initial)]);
    let before = io_counts();
    for _ in 0..100 { corona_blur::set(target); }
    assert_eq!(io_counts(), before, "a successful peer save must stay clean");
    corona_blur::set_v2_enabled(target);
    with(|s| assert_eq!(s.opens.len(), 8, "peer success must not clear this switch's failure"));
    assert_eq!(saved(V2), [u8::from(initial)]);
    assert_closed();

    allow_save(V2);
    corona_blur::set_v2_enabled(target);
    assert_eq!(saved(V2), [u8::from(target)]);
    let before = io_counts();
    for _ in 0..100 { corona_blur::set(target); corona_blur::set_v2_enabled(target); }
    assert_eq!(io_counts(), before, "successful retries must clear both failure flags");
    with(|s| {
        assert_eq!(s.opens.len(), 9);
        assert_eq!(s.remounts, 2, "persistence-only retries must not remount");
        assert_eq!(s.writes.len(), if matches!(failure, SaveFailure::Open) { 2 } else { 7 });
        assert_eq!(s.closes, if matches!(failure, SaveFailure::Open) { 4 } else { 9 });
        assert!(s.writes.iter().all(|(_, bytes)| bytes == &[u8::from(target)]));
    });
    reboot(target, target);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let case = args.get(1).map(String::as_str).expect("scenario required");
    unsafe {
        match case {
            "probe" => {
                for (path, value) in [(CORONA, &args[2]), (V2, &args[3])] {
                    match value.as_str() {
                        "missing" => {},
                        "empty" => seed(path, &[]),
                        _ => seed(path, &[value.parse().unwrap()]),
                    }
                }
                assert_modes(args[4] == "1", args[5] == "1");
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); });
            }
            "missing" => {
                assert_modes(false, false);
                with(|s| { assert_eq!(s.opens.len(), 2); assert_eq!(s.reads, 0); });
            }
            "empty" => {
                seed(CORONA, &[]); seed(V2, &[]);
                assert_modes(false, false);
                with(|s| { assert_eq!(s.reads, 2); assert_eq!(s.closes, 2); });
            }
            "invalid" => {
                seed(CORONA, &[2]); seed(V2, &[255]);
                assert_modes(false, false);
                assert_eq!(saved(CORONA), [2]); assert_eq!(saved(V2), [255]);
                with(|s| assert!(s.writes.is_empty()));
            }
            "short-read" | "read-error" | "read-open-error" => {
                seed(CORONA, &[1]); seed(V2, &[1]);
                with(|s| {
                    s.read_result = match case { "short-read" => Some(0), "read-error" => Some(-1), _ => None };
                    s.read_open_error = case == "read-open-error";
                });
                assert_modes(false, false);
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); });
            }
            "read-once" => {
                seed(CORONA, &[1]); seed(V2, &[0]);
                assert_modes(true, false);
                seed(CORONA, &[0]); seed(V2, &[1]);
                for _ in 0..100 { assert_modes(true, false); }
                with(|s| { assert_eq!(s.reads, 2); assert_eq!(s.opens.len(), 2); });
            }
            "independent-load" => {
                seed(CORONA, &[1]); seed(V2, &[1]);
                assert!(corona_blur::active());
                with(|s| assert_eq!(s.opens, [(CORONA.to_owned(), fw_api::oflag::RDONLY)]));
                assert!(corona_blur::v2_enabled());
                with(|s| assert_eq!(s.reads, 2));
            }
            "combinations" => {
                for (corona, v2) in [(false, false), (true, false), (true, true), (false, true), (false, false)] {
                    corona_blur::set(corona);
                    corona_blur::set_v2_enabled(v2);
                    assert_modes(corona, v2);
                }
                with(|s| { assert_eq!(s.remounts, 4); assert_eq!(s.writes.len(), 4); assert!(s.commits.is_empty()); });
            }
            "independent-save" => {
                seed(CORONA, &[1]); seed(V2, &[0]);
                corona_blur::set_v2_enabled(true);
                assert_eq!(saved(CORONA), [1]); assert_eq!(saved(V2), [1]);
                corona_blur::set(false);
                assert_eq!(saved(CORONA), [0]); assert_eq!(saved(V2), [1]);
                with(|s| assert_eq!(s.writes, [(V2.to_owned(), vec![1]), (CORONA.to_owned(), vec![0])]));
            }
            "restart" => {
                assert_modes(false, false);
                reboot(false, false);
                corona_blur::set_v2_enabled(true); reboot(false, true);
                corona_blur::set(true); reboot(true, true);
                corona_blur::set_v2_enabled(false); reboot(true, false);
                corona_blur::set(false); reboot(false, false);
            }
            "unchanged" => {
                seed(CORONA, &[0]); seed(V2, &[1]);
                for _ in 0..100 { corona_blur::set(false); corona_blur::set_v2_enabled(true); }
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); assert_eq!(s.reads, 2); });
                corona_blur::set(true); corona_blur::set_v2_enabled(false);
                for _ in 0..100 { corona_blur::set(true); corona_blur::set_v2_enabled(false); }
                with(|s| { assert_eq!(s.writes.len(), 2); assert_eq!(s.remounts, 2); assert_eq!(s.reads, 2); });
            }
            "save-open-error" => save_failure_retries(false, SaveFailure::Open),
            "save-write-error" => save_failure_retries(false, SaveFailure::Write(-1)),
            "save-short-write" => save_failure_retries(false, SaveFailure::Write(0)),
            "save-invalid-count" => save_failure_retries(false, SaveFailure::Write(2)),
            "save-disable-retry" => save_failure_retries(true, SaveFailure::Write(-1)),
            "save-latest-value" => {
                seed(CORONA, &[0]); seed(V2, &[0]);
                fail_save(CORONA, SaveFailure::Open);
                fail_save(V2, SaveFailure::Write(0));
                corona_blur::set(true); corona_blur::set_v2_enabled(true);
                assert_modes(true, true);
                assert_closed();
                allow_save(CORONA); allow_save(V2);
                corona_blur::set(false); corona_blur::set_v2_enabled(false);
                assert_modes(false, false);
                assert_eq!(saved(CORONA), [0]); assert_eq!(saved(V2), [0]);
                let before = io_counts();
                for _ in 0..100 { corona_blur::set(false); corona_blur::set_v2_enabled(false); }
                assert_eq!(io_counts(), before);
                with(|s| {
                    assert_eq!(s.remounts, 4);
                    assert_eq!(s.writes, [(V2.to_owned(), vec![1]), (CORONA.to_owned(), vec![0]), (V2.to_owned(), vec![0])]);
                });
                reboot(false, false);
            }
            "native-abi" => {
                with(|s| s.commit_result = -2145);
                assert_eq!(corona_blur::backgrounds_commit(true, 0xdead_beef), -2145);
                with(|s| s.commit_result = 0);
                assert_eq!(corona_blur::backgrounds_commit(false, 0xffff_fff0), 0);
                with(|s| assert_eq!(s.commits, [(1, 15), (0, 0)]));
                corona_blur::backgrounds_poll();
                assert!(!corona_blur::backgrounds_pending()); assert!(!corona_blur::backgrounds_idle());
                with(|s| { s.pending = 3; s.idle = 1; });
                assert!(corona_blur::backgrounds_pending()); assert!(corona_blur::backgrounds_idle());
                with(|s| { assert_eq!(s.polls, 1); assert!(s.opens.is_empty()); assert_eq!(s.remounts, 0); });
            }
            _ => panic!("unknown scenario: {case}"),
        }
    }
    assert_closed();
    if case != "probe" { println!("{case}: OK"); }
}
