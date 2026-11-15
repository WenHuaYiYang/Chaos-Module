#![allow(dead_code)]
#[path = "../src/capture_format.rs"]
mod capture_format;
#[path = "../src/capture_apng.rs"]
mod capture_apng;
#[path = "../src/capture_settings.rs"]
mod capture_settings;
#[path = "../src/capture.rs"]
mod capture;
#[path = "../src/page/capture.rs"]
mod capture_page;
use std::{cell::RefCell, collections::HashMap};

struct World {
    files: HashMap<String, Vec<u8>>, handles: HashMap<i32, (String, usize)>,
    allocations: HashMap<usize, std::alloc::Layout>, next_fd: i32,
    now: u32, clock_step: u32, on: bool, captures: usize, spawn_result: i32, frame_error: i32,
    write_size: usize, interrupts: u32, zero_write: bool, budget: usize,
    free_bytes: Option<u64>, fail_alloc: bool, rename_error: i32, close_error: i32,
    stop_during_capture: bool, lost_vsync: bool, write_cost_ms: u32, read_cost_ms: u32, switch_every_copy: bool,
    readers: u32, reader_opens: u32, reader_error: i32, vsync: bool, torn: bool, frame_mode: u8,
    write_calls: u32, seeks: u32,
    directories: HashMap<usize, (Vec<String>, usize)>, directory_entry: [u8; 256],
}
impl Default for World {
    fn default() -> Self { Self {
        files: HashMap::new(), handles: HashMap::new(), allocations: HashMap::new(), next_fd: 3,
        now: 1000, clock_step: 0, on: true, captures: 0, spawn_result: 42, frame_error: 0,
        write_size: usize::MAX, interrupts: 0, zero_write: false, budget: usize::MAX,
        free_bytes: Some(64 * 1024 * 1024), fail_alloc: false, rename_error: 0, close_error: 0,
        stop_during_capture: false, lost_vsync: false, write_cost_ms: 0, read_cost_ms: 0, switch_every_copy: false,
        readers: 0, reader_opens: 0, reader_error: 0, vsync: true, torn: false, frame_mode: 0,
        write_calls: 0, seeks: 0,
        directories: HashMap::new(), directory_entry: [0; 256],
    } }
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<R>(f: impl FnOnce(&mut World) -> R) -> R { WORLD.with(|w| f(&mut w.borrow_mut())) }
fn cstr(p: *const u8) -> String { unsafe { std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().to_owned() } }

mod fw_api {
    use super::*;
    pub mod oflag { pub const RDONLY: u32 = 1; pub const WRONLY: u32 = 2; pub const CREAT: u32 = 4; }
    pub mod trailing { pub const RADIO: u32 = 6; pub const CHECKBOX: u32 = 2; }
    pub unsafe fn row_trailing(row: u32) -> u32 { row + 100 }
    pub unsafe fn obj_add_event(object: u32, callback: u32, event: u32, _: u32) {
        crate::ui(|u| u.events.push((object, callback, event)));
    }
    pub mod seek { pub const SET: u32 = 0; pub const END: u32 = 2; }
    pub unsafe fn capture_result(rc: i32) -> i32 { rc }
    pub unsafe fn capture_mkdir() -> i32 { 0 }
    pub unsafe fn capture_mkdir_path(_: *const u8) -> i32 { 0 }
    pub unsafe fn capture_rename(a: *const u8, b: *const u8) -> i32 { world(|w| {
        if w.rename_error != 0 { return w.rename_error; }
        let Some(bytes) = w.files.remove(&cstr(a)) else { return -2; };
        w.files.insert(cstr(b), bytes); 0
    }) }
    pub fn capture_now_ms() -> u32 { world(|w| { let now = w.now; w.now = w.now.wrapping_add(w.clock_step); now }) }
    pub unsafe fn capture_free_bytes() -> Option<u64> { world(|w| w.free_bytes) }
    pub unsafe fn heap_malloc(n: u32) -> usize { world(|w| {
        if w.fail_alloc { return 0; }
        let layout = std::alloc::Layout::from_size_align(n as usize, 8).unwrap();
        let p = std::alloc::alloc_zeroed(layout) as usize; w.allocations.insert(p, layout); p
    }) }
    pub unsafe fn fw_free(p: usize) { world(|w| { let layout = w.allocations.remove(&p).unwrap(); std::alloc::dealloc(p as *mut u8, layout); }); }
    pub unsafe fn open(p: *const u8, flags: u32, _: u32) -> i32 { world(|w| {
        let name = cstr(p);
        if flags & 4 != 0 {
            if flags & 8 != 0 && w.files.contains_key(&name) { return -17; }
            w.files.entry(name.clone()).or_default();
            if flags & 0x20 != 0 { w.files.get_mut(&name).unwrap().clear(); }
        } else if !w.files.contains_key(&name) { return -2; }
        let fd = w.next_fd; w.next_fd += 1; w.handles.insert(fd, (name, 0)); fd
    }) }
    pub unsafe fn close(fd: i32) -> i32 { world(|w| {
        assert!(w.handles.remove(&fd).is_some(), "double close");
        let error = w.close_error; w.close_error = 0; error
    }) }
    pub unsafe fn fs_remove(p: *const u8) -> i32 { world(|w| if w.files.remove(&cstr(p)).is_some() { 0 } else { -2 }) }
    pub unsafe fn nsh_write(fd: i32, bytes: &[u8]) -> i32 { world(|w| {
        w.write_calls += 1;
        w.now = w.now.wrapping_add(w.write_cost_ms);
        if w.interrupts != 0 { w.interrupts -= 1; return -4; }
        if w.zero_write { return 0; }
        if w.budget == 0 { return -28; }
        let n = bytes.len().min(w.write_size).min(w.budget); w.budget -= n;
        let (name, offset) = w.handles.get_mut(&fd).expect("write to closed fd");
        let file = w.files.get_mut(name).unwrap();
        file.resize(file.len().max(*offset + n), 0);
        file[*offset..*offset + n].copy_from_slice(&bytes[..n]); *offset += n; n as i32
    }) }
    pub unsafe fn nsh_read(fd: i32, out: &mut [u8]) -> i32 { world(|w| {
        let (name, cursor) = w.handles.get_mut(&fd).unwrap(); let file = w.files.get(name).unwrap();
        let n = out.len().min(file.len().saturating_sub(*cursor));
        out[..n].copy_from_slice(&file[*cursor..*cursor + n]); *cursor += n; n as i32
    }) }
    pub unsafe fn lseek(fd: i32, offset: i64, whence: u32) -> i64 { world(|w| {
        w.seeks += 1;
        let (name, cursor) = w.handles.get_mut(&fd).unwrap();
        *cursor = (if whence == seek::END { w.files[name].len() as i64 } else { 0 } + offset) as usize;
        *cursor as i64
    }) }
    pub unsafe fn capture_frame(out: &mut [u8]) -> Result<(), i32> {
        let (error, stop) = world(|w| {
            w.captures += 1; let mut n = w.captures as u32;
            for p in out.chunks_exact_mut(3) {
                n = n.wrapping_mul(1664525).wrapping_add(1013904223);
                p.copy_from_slice(&n.to_le_bytes()[..3]);
            }
            if w.frame_mode != 0 {
                out.fill(0);
                if w.frame_mode == 2 {
                    let x = (w.captures * 30) % 300;
                    for y in 100..112 { for dx in x..x + 20 { out[(y * 336 + dx) * 3 + 2] = 255; } }
                }
                if w.frame_mode == 3 {
                    let colors = [[0,0,255], [0,255,0], [255,0,0], [0,255,255], [255,0,255], [255,255,0]];
                    for row in out.chunks_exact_mut(336 * 3) {
                        for (x, pixel) in row.chunks_exact_mut(3).enumerate() {
                            if x < 256 { pixel.fill(x as u8); }
                            else { pixel.copy_from_slice(&colors[((x - 256) / 12).min(5)]); }
                        }
                    }
                }
            }
            w.now = w.now.wrapping_add(w.read_cost_ms);
            let stop = w.stop_during_capture; w.stop_during_capture = false;
            (w.frame_error, stop)
        });
        if stop { capture::toggle_recording(); }
        if error != 0 { Err(error) } else { Ok(()) }
    }
    pub unsafe fn screen_is_on() -> bool { world(|w| w.on) }
    pub struct CaptureReader { last: Option<u32>, missed: u32, unstable: u32 }
    impl CaptureReader {
        pub unsafe fn open() -> Result<Self, i32> { world(|w| {
            if w.reader_error != 0 { return Err(w.reader_error); }
            w.readers += 1; w.reader_opens += 1; Ok(Self { last: None, missed: 0, unstable: 0 })
        }) }
        pub fn missed(&self) -> u32 { self.missed }
        pub fn unstable(&self) -> u32 { self.unstable }
        pub unsafe fn pause(&mut self) { self.last = Some(world(|w| w.now)); }
        pub unsafe fn frame(&mut self, out: &mut [u8], timer_due: bool) -> Result<Option<u32>, i32> {
            if !world(|w| w.on) { return Ok(None); }
            let now = world(|w| w.now);
            let fresh = world(|w| w.vsync) && self.last != Some(now);
            if !fresh && !timer_due { return Ok(None); } self.last = Some(now);
            if world(|w| w.lost_vsync) { self.missed += 1; }
            let started = capture_now_ms();
            capture_frame(out)?;
            let mut captured = capture_now_ms();
            let changed = world(|w| { let changed = w.torn || w.switch_every_copy; w.torn = false; changed });
            if changed { self.unstable += 1; }
            if capture::retry_fast_snapshot(changed, captured.wrapping_sub(started)) && world(|w| w.on) {
                capture_frame(out)?; captured = capture_now_ms();
                if world(|w| w.switch_every_copy) { self.unstable += 1; }
            }
            self.last = Some(world(|w| w.now));
            Ok(Some(captured))
        }
        pub unsafe fn wait_vsync(&mut self) {}
    }
    impl Drop for CaptureReader { fn drop(&mut self) { world(|w| w.readers -= 1); } }
    pub unsafe fn nsh_spawn(_: *const u8, _: unsafe extern "C" fn(i32, *mut *mut u8) -> i32, _: *mut usize) -> i32 { world(|w| w.spawn_result) }
    pub unsafe fn nsh_usleep(_: u32) -> i32 { 0 }
    pub unsafe fn opendir(path: *const u8) -> usize { world(|w| {
        let prefix = format!("{}/", cstr(path));
        let mut names: Vec<_> = w.files.keys().filter_map(|name| name.strip_prefix(&prefix).map(str::to_owned)).collect();
        names.sort(); w.directories.insert(1000, (names, 0)); 1000
    }) }
    pub unsafe fn readdir(dir: usize) -> usize { world(|w| {
        let (names, next) = w.directories.get_mut(&dir).unwrap();
        if *next == names.len() { return 0; }
        w.directory_entry.fill(0); w.directory_entry[0] = 8;
        let name = &names[*next]; *next += 1;
        w.directory_entry[1..1 + name.len()].copy_from_slice(name.as_bytes());
        w.directory_entry.as_ptr() as usize
    }) }
    pub unsafe fn closedir(dir: usize) -> i32 { world(|w| { w.directories.remove(&dir).unwrap(); }); 0 }
    pub unsafe fn row_update(row: u32, _: *const u8, _: *const u8, _: *const u8, _: i32, selected: u8) -> i32 {
        crate::ui(|u| { u.selected.insert(row as usize - 1, selected != 0); }); 0
    }
}

#[derive(Default)]
struct Ui { rows: Vec<(usize, String, String, bool)>, slots: [u32; 12], navigations: Vec<u32>, renders: Vec<usize>, selected: HashMap<usize, bool>, events: Vec<(u32, u32, u32)> }
thread_local! { static UI: RefCell<Ui> = RefCell::new(Ui::default()); }
fn ui<R>(f: impl FnOnce(&mut Ui) -> R) -> R { UI.with(|u| f(&mut u.borrow_mut())) }

mod page {
    pub mod kit { pub use crate::checklist; }
    pub(crate) use crate::fw_api;
    pub(crate) use std::ptr::read_volatile;
    pub const DT_REG: u8 = 8;
    pub struct PageCtx { pub content: u32, pub title: u32, pub rows: *mut u32 }
    #[derive(Debug)]
    pub enum PageError { RowFailed(usize) }
    pub type PageResult = Result<(), PageError>;
    pub trait Page {
        fn fill_title(&self, buf: *mut u8);
        fn render(&self, ctx: &PageCtx) -> PageResult;
        fn on_click(&self, i: usize);
        fn needs_tick(&self) -> bool { false }
        fn tick(&self) {}
    }
    pub struct RowSpec<'a> { pub text: &'a [u8], pub sub: *const u8, pub clickable: bool, pub trailing: u32 }
    impl<'a> RowSpec<'a> {
        pub fn clickable(text: &'a [u8]) -> Self { Self { text, sub: std::ptr::null(), clickable: true, trailing: 0 } }
        pub fn tap(text: &'a [u8], sub: *const u8) -> Self { Self { text, sub, clickable: true, trailing: 0 } }
        pub fn display(text: &'a [u8], sub: *const u8) -> Self { Self { text, sub, clickable: false, trailing: 0 } }
    }
    pub unsafe fn build_row(_: &PageCtx, _: u32, slot: usize, spec: &RowSpec) -> u32 {
        assert!(slot < 12);
        crate::ui(|u| {
            u.rows.push((slot, crate::cstr(spec.text.as_ptr()), if spec.sub.is_null() { String::new() } else { crate::cstr(spec.sub) }, spec.clickable));
            u.slots[slot] = slot as u32 + 1; slot as u32 + 1
        })
    }
    pub unsafe fn build_rows(ctx: &PageCtx, mut prev: u32, rows: &[(usize, RowSpec)]) -> PageResult {
        for (slot, spec) in rows { prev = build_row(ctx, prev, *slot, spec); } Ok(())
    }
    pub unsafe fn put_str(out: *mut u8, bytes: &[u8]) { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len()); }
    pub fn render_req(pid: u32) { crate::ui(|u| u.renders.push(pid as usize)); }
    pub unsafe fn nav_goto(pid: u32, _: u32) { crate::ui(|u| u.navigations.push(pid)); }
    pub fn page_rows_ptr(_: usize) -> *mut u32 { crate::ui(|u| u.slots.as_mut_ptr()) }
    pub unsafe fn row_ev_fn(slot: usize) -> u32 { 1000 + slot as u32 }
    pub struct W { ptr: *mut u8, cap: usize, len: usize }
    impl W {
        pub fn new(ptr: *mut u8, cap: usize) -> Self { Self { ptr, cap, len: 0 } }
        pub unsafe fn s(&mut self, bytes: &[u8]) -> &mut Self {
            for &byte in bytes { if byte == 0 || self.len + 1 == self.cap { break; } *self.ptr.add(self.len) = byte; self.len += 1; } self
        }
        pub unsafe fn n(&mut self, n: u32) -> &mut Self { self.s(n.to_string().as_bytes()) }
        pub unsafe fn sz(&mut self, n: u32) -> &mut Self { self.n(n).s(b" B") }
        pub unsafe fn end(&mut self) { *self.ptr.add(self.len) = 0; }
    }
}

#[path = "../src/page/kit/checklist.rs"]
pub mod checklist;
use page::*;
const MAX_PID: usize = 42;
fn row_cb() -> u32 { 2000 }
unsafe fn cstr_len(p: *const u8) -> usize { std::ffi::CStr::from_ptr(p.cast()).to_bytes().len() }
