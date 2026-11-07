//! Host mocks around the real terminal UI/model. No firmware call is executed.
#![allow(dead_code, static_mut_refs)]
use std::sync::Mutex;
#[path = "../src/terminal_ui.rs"]
mod terminal_ui;
#[path = "../src/fw_api/align.rs"]
pub mod align;
static TEST_LOCK: Mutex<()> = Mutex::new(());
const FW_STYLE_HR_NAME: u32 = 1;
const TILE_W: u32 = 160;
const FACE_LINE_HEIGHT_OFF: u32 = 0x0c;
static mut FG_PAGE: u32 = 31;
static mut APP_ID: u32 = 200;

#[derive(Debug, PartialEq)]
pub enum PageError { RowFailed(usize) }
pub type PageResult = Result<(), PageError>;
pub struct PageCtx { content: u32, title: u32, rows: *mut u32 }
mod page {
    pub trait Page: Sync {
        fn fill_title(&self, buf: *mut u8);
        fn render(&self, ctx: &crate::PageCtx) -> crate::PageResult;
        fn on_click(&self, idx: usize);
        fn needs_tick(&self) -> bool { false }
        fn tick(&self) {}
    }
}
#[derive(Clone, Default)]
pub struct Object { pub text: Vec<u8>, pub size: (u32, u32), pub position: (i32, i32), pub flags: u32, pub scroll_dir: u32 }
pub struct Mock {
    pub objects: Vec<Object>, pub writes: Vec<(u32, Vec<u8>)>, pub live: bool,
    pub state: nsh::State, pub starts: usize, pub start_error: Option<i32>,
    pub error: i32, pub send_error: Option<i32>, pub attempts: Vec<Vec<u8>>,
    pub sent: Vec<Vec<u8>>, pub incoming: std::collections::VecDeque<u8>,
    pub receives: usize, pub navigations: Vec<u32>, pub fail_at: Option<usize>,
    pub screen_on: bool, pub top_page: (u32, u32),
}
static MOCK: Mutex<Option<Mock>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Mock) -> T) -> T { f(MOCK.lock().unwrap().as_mut().unwrap()) }
fn reset() {
    *MOCK.lock().unwrap() = Some(Mock { objects: vec![], writes: vec![], live: true,
        state: nsh::State::Idle, starts: 0, start_error: None, error: 0, send_error: None,
        attempts: vec![], sent: vec![], incoming: Default::default(), receives: 0,
        navigations: vec![], fail_at: None, screen_on: true, top_page: (200, 31) });
    unsafe { FG_PAGE = 31; }
}
fn safe_ptr(p: u32) -> bool { with(|m| p != 0 && p as usize <= m.objects.len()) }
fn page_is_live(_: usize) -> bool { with(|m| m.live) }
unsafe fn put_str(p: *mut u8, s: &[u8]) { for (i, b) in s.iter().enumerate() { *p.add(i) = *b; } }
fn nav_back() { with(|m| m.navigations.push(0)); }
fn build_tile_below(_: &PageCtx, _: u32, _: usize, text: &[u8], w: u32, h: u32) -> u32 {
    let id = fw_api::label_create(0);
    if id != 0 { fw_api::obj_set_size_wh(id, w, h); unsafe { fw_api::label_set_text(id, text.as_ptr()); } }
    id
}
fn build_tile_right(c: &PageCtx, p: u32, s: usize, t: &[u8], w: u32, h: u32) -> u32 { build_tile_below(c, p, s, t, w, h) }
mod keyboard {
    pub enum Completion { NshCommand }
    pub unsafe fn open(_: *const u8, _: Completion) { crate::FG_PAGE = 23; crate::with(|m| m.navigations.push(23)); }
}
mod nsh {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum State { Idle, Starting, Running, Exited, Failed }
    pub fn state() -> State { crate::with(|m| m.state) }
    pub fn error() -> i32 { crate::with(|m| m.error) }
    pub unsafe fn start() -> Result<(), i32> { crate::with(|m| {
        m.starts += 1;
        if let Some(e) = m.start_error { m.error = e; m.state = State::Failed; return Err(e); }
        m.state = State::Starting; Ok(())
    }) }
    pub fn send(b: &[u8]) -> Result<(), i32> { crate::with(|m| {
        m.attempts.push(b.to_vec());
        if let Some(e) = m.send_error { m.error = e; return Err(e); }
        if !matches!(m.state, State::Starting | State::Running) { return Err(-107); }
        m.sent.push(b.to_vec()); Ok(())
    }) }
    pub fn receive(out: &mut [u8]) -> usize { crate::with(|m| {
        m.receives += 1;
        let n = out.len().min(m.incoming.len());
        for b in &mut out[..n] { *b = m.incoming.pop_front().unwrap(); }
        n
    }) }
}
mod fw_api {
    pub use crate::align;
    pub fn screen_is_on() -> bool { crate::with(|m| m.screen_on) }
    pub fn top_page_ids() -> (u32, u32) { crate::with(|m| m.top_page) }
    pub const LV_STYLE_TEXT_FONT: u32 = 0x5a;
    pub const LV_OBJ_FLAG_SCROLLABLE: u32 = 0x10;
    pub const LV_OBJ_FLAG_GESTURE_BUBBLE: u32 = 0x8000;
    pub const LV_OBJ_FLAG_SCROLL_ELASTIC: u32 = 0x20;
    pub const LV_DIR_HOR: u32 = 3;
    pub fn obj_create(parent: u32, _: u32) -> u32 { label_create(parent) }
    pub fn fw_style_get_prop(_: u32, _: u32, _: u32) -> u32 { 0 }
    pub fn obj_add_flag(id: u32, flags: u32) { crate::with(|m| m.objects[id as usize - 1].flags |= flags); }
    pub fn obj_clear_flag(id: u32, flags: u32) { crate::with(|m| m.objects[id as usize - 1].flags &= !flags); }
    pub fn obj_set_scroll_dir(id: u32, dir: u32) { crate::with(|m| m.objects[id as usize - 1].scroll_dir = dir); }
    pub fn obj_set_width(id: u32, w: i32) { crate::with(|m| m.objects[id as usize - 1].size.0 = w as u32); }
    pub fn label_create(_: u32) -> u32 { crate::with(|m| {
        let id = m.objects.len() + 1;
        if m.fail_at == Some(id) { return 0; }
        m.objects.push(Default::default()); id as u32
    }) }
    pub fn style_apply(_: u32, _: u32, _: u32, _: u32) {}
    pub fn obj_set_size_wh(p: u32, w: u32, h: u32) { crate::with(|m| m.objects[p as usize - 1].size = (w, h)); }
    pub fn obj_align(p: u32, _: u32, x: i32, y: i32) { crate::with(|m| m.objects[p as usize - 1].position = (x, y)); }
    pub fn obj_align_to(_: u32, _: u32, _: u32, _: i32, _: i32) {}
    pub fn obj_clear_box_spacing(_: u32) {}
    pub fn obj_clear_scrollable(_: u32) {}
    pub unsafe fn label_set_text(p: u32, text: *const u8) {
        let b = std::ffi::CStr::from_ptr(text.cast()).to_bytes().to_vec();
        std::str::from_utf8(&b).expect("display must always be valid UTF-8");
        crate::with(|m| { m.objects[p as usize - 1].text = b.clone(); m.writes.push((p, b)); });
    }
}
