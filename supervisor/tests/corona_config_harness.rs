//! Host configuration tests compiling the entire production corona_blur.rs.
//! Each scenario runs in a fresh process, so production lazy-load flags reset
//! naturally; no extracted function copies or test-only production resets.
//!
//! c0ebd5f9 把配置模型从"两个开关文件(corona_blur.cfg + blur_v2.cfg)"换成了
//! "一个文件三态"(:/data/chaos/blur_algo.cfg, 0=关 / 1=原生 / 2=V2 / 3=V3),
//! 开关与算法号合并在同一个字节里; 旧 blur_v2.cfg 只剩一次性迁移。
#![allow(dead_code, static_mut_refs)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::process::Command;

const ALGO: &str = "/data/chaos/blur_algo.cfg";
const LEGACY: &str = "/data/chaos/blur_v2.cfg";
#[no_mangle]
pub extern "C" fn chaos_blur_select_algorithm(_:u32)->i32 {0}
#[no_mangle]
pub extern "C" fn chaos_blur_control_buttons_set(_:u32) {}
#[no_mangle]
pub extern "C" fn chaos_corona_set_scene(_:u32) {}
mod blur_params {pub unsafe fn apply(){}}
#[no_mangle]
pub extern "C" fn chaos_blur_launcher_buttons_set(_:u32) {}
mod launcher_buttons_config {pub unsafe fn enabled()->bool{true}}
mod cc_buttons_config { pub unsafe fn enabled() -> bool { true } }

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
        assert!(path == super::ALGO || path == super::LEGACY, "unexpected config path");
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
        // 0=关, 1/2/3 = 开启 + 算法号, 一个字节同时表达开关和档位。
        assert!(bytes[0] <= 3);
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
        })
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

/// 三态约定: 0=关 / 1=原生 / 2=V2 / 3=V3。关闭时 algo() 无意义, 只看 active()。
unsafe fn assert_algo(value: u8) {
    let on = value != 0;
    assert_eq!(corona_blur::active(), on, "value {value}: enabled flag");
    assert_eq!(corona_blur::enabled(), on, "value {value}: enabled flag");
    if on { assert_eq!(corona_blur::algo(), (value - 1) as u32, "value {value}: algorithm"); }
}
fn reboot(value: Option<&[u8]>) {
    let encode = match value {
        None => "missing".to_owned(),
        Some(bytes) if bytes.is_empty() => "empty".to_owned(),
        Some(bytes) => bytes[0].to_string(),
    };
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["probe", &encode])
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

/// 写盘失败后**本次切换立即生效**(SAVE_FAILED 只用于下次同值重试),
/// 且失败期间同值调用必须继续重试写盘, 成功一次后重新变干净。
unsafe fn save_failure_retries(algo: u32, failure: SaveFailure) {
    let value = (algo + 1) as u8;
    seed(ALGO, &[0]);
    assert_algo(0);
    fail_save(ALGO, failure);
    corona_blur::set_algo(algo);
    assert_algo(value);
    assert_eq!(saved(ALGO), [0], "failed save must not change the file");
    with(|s| assert_eq!(s.remounts, 1, "failed saves must still apply immediately"));
    assert_closed();

    let before = io_counts();
    for _ in 0..100 { assert_algo(value); }
    assert_eq!(io_counts(), before, "reads/polls must not retry failed saves");

    corona_blur::set_algo(algo);
    with(|s| assert_eq!(s.opens.len(), 3, "same-value calls must retry each failed switch"));
    assert_closed();

    allow_save(ALGO);
    corona_blur::set_algo(algo);
    assert_eq!(saved(ALGO), [value]);
    let before = io_counts();
    for _ in 0..100 { corona_blur::set_algo(algo); }
    assert_eq!(io_counts(), before, "a successful save must stay clean");
    assert_closed();
    reboot(Some(&[value]));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let case = args.get(1).map(String::as_str).expect("scenario required");
    unsafe {
        match case {
            "probe" => {
                let expected = match args[2].as_str() {
                    "missing" => 0u8,
                    "empty" => { seed(ALGO, &[]); 0u8 },
                    value => {
                        let value: u8 = value.parse().unwrap();
                        seed(ALGO, &[value]);
                        // 只接受 0..=3, 其余按"坏值"处理成关闭。
                        if value <= 3 { value } else { 0 }
                    }
                };
                assert_algo(expected);
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); });
            }
            "missing" => {
                assert_algo(0);
                // 先试新文件, 再试旧文件迁移: 两个都是 -1。
                with(|s| { assert_eq!(s.opens.len(), 2); assert_eq!(s.reads, 0); });
            }
            "empty" => {
                seed(ALGO, &[]);
                assert_algo(0);
                with(|s| { assert_eq!(s.reads, 1); assert_eq!(s.closes, 1); });
            }
            "invalid" => {
                seed(ALGO, &[4]);
                assert_algo(0);
                assert_eq!(saved(ALGO), [4]);
                with(|s| assert!(s.writes.is_empty()));
            }
            "short-read" | "read-error" | "read-open-error" => {
                seed(ALGO, &[1]);
                with(|s| {
                    s.read_result = match case { "short-read" => Some(0), "read-error" => Some(-1), _ => None };
                    s.read_open_error = case == "read-open-error";
                });
                assert_algo(0);
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); });
            }
            "read-once" => {
                seed(ALGO, &[1]);
                assert_algo(1);
                seed(ALGO, &[0]);
                for _ in 0..100 { assert_algo(1); }
                with(|s| { assert_eq!(s.reads, 1); assert_eq!(s.opens.len(), 1); });
            }
            // 旧 blur_v2.cfg = 1 迁移成"开启 + V3": 观感不变但拿到场景门控省电。
            "legacy-migrate" => {
                seed(LEGACY, &[1]);
                assert_algo(3);
                with(|s| {
                    assert_eq!(s.opens, [(ALGO.to_owned(), fw_api::oflag::RDONLY),
                                         (LEGACY.to_owned(), fw_api::oflag::RDONLY)]);
                    assert_eq!(s.reads, 1, "新文件打不开才读旧文件");
                });
                // 迁移只读不写: 真正的落盘发生在用户下一次切换。
                with(|s| assert!(s.writes.is_empty()));
            }
            "legacy-off" => {
                seed(LEGACY, &[0]);
                assert_algo(0);
                with(|s| assert!(s.writes.is_empty()));
            }
            // 新文件存在时绝不看旧文件, 哪怕旧文件写着"开"。
            "legacy-ignored" => {
                seed(ALGO, &[0]); seed(LEGACY, &[1]);
                assert_algo(0);
                with(|s| assert_eq!(s.opens.len(), 1, "新文件可读就不再回退"));
            }
            "combinations" => {
                for value in [1u8, 2, 3, 0, 3, 1] {
                    if value == 0 { corona_blur::set_enabled(false); } else { corona_blur::set_algo((value - 1) as u32); }
                    assert_algo(value);
                }
                with(|s| { assert_eq!(s.remounts, 6); assert_eq!(s.writes.len(), 6); assert!(s.commits.is_empty()); });
            }
            // 同一档位重复点不写盘: 开关与算法合并在一个字节里, 没有"关着但选 V2"的中间态。
            "unchanged" => {
                seed(ALGO, &[2]);
                for _ in 0..100 { corona_blur::set_algo(1); }
                with(|s| { assert!(s.writes.is_empty()); assert_eq!(s.remounts, 0); assert_eq!(s.reads, 1); });
                corona_blur::set_algo(2);
                for _ in 0..100 { corona_blur::set_algo(2); }
                with(|s| { assert_eq!(s.writes.len(), 1); assert_eq!(s.remounts, 1); assert_eq!(s.reads, 1); });
                assert_eq!(saved(ALGO), [3]);
            }
            // 关闭后再次选算法: 字节从 0 变 2, 必须重写并 remount。
            "reopen" => {
                seed(ALGO, &[3]);
                corona_blur::set_enabled(false);
                assert_algo(0);
                corona_blur::set_algo(1);
                assert_algo(2);
                with(|s| assert_eq!(s.writes, [(ALGO.to_owned(), vec![0]), (ALGO.to_owned(), vec![2])]));
                assert_eq!(saved(ALGO), [2]);
            }
            "restart" => {
                assert_algo(0);
                reboot(Some(&[0]));
                corona_blur::set_algo(2); reboot(Some(&[3]));
                corona_blur::set_algo(0); reboot(Some(&[1]));
                corona_blur::set_enabled(false); reboot(Some(&[0]));
            }
            "save-open-error" => save_failure_retries(1, SaveFailure::Open),
            "save-write-error" => save_failure_retries(1, SaveFailure::Write(-1)),
            "save-short-write" => save_failure_retries(1, SaveFailure::Write(0)),
            "save-invalid-count" => save_failure_retries(2, SaveFailure::Write(2)),
            "save-latest-value" => {
                seed(ALGO, &[0]);
                fail_save(ALGO, SaveFailure::Open);
                corona_blur::set_algo(2);
                assert_algo(3);
                assert_closed();
                allow_save(ALGO);
                corona_blur::set_enabled(false);
                assert_algo(0);
                assert_eq!(saved(ALGO), [0]);
                let before = io_counts();
                for _ in 0..100 { corona_blur::set_enabled(false); }
                assert_eq!(io_counts(), before);
                with(|s| {
                    assert_eq!(s.remounts, 2);
                    assert_eq!(s.writes, [(ALGO.to_owned(), vec![0])]);
                });
                reboot(Some(&[0]));
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
                with(|s| {
                    assert_eq!(s.polls, 1);
                    // 两个配置文件都不存在: 先试新文件, 再试旧文件迁移, 都打不开即"关闭"。
                    assert_eq!(s.opens, [(ALGO.to_owned(), fw_api::oflag::RDONLY),
                                         (LEGACY.to_owned(), fw_api::oflag::RDONLY)]);
                    assert_eq!(s.remounts, 0);
                });
            }
            _ => panic!("unknown scenario: {case}"),
        }
    }
    assert_closed();
    if case != "probe" { println!("{case}: OK"); }
}
