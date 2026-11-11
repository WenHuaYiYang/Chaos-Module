//! Exercise the production third-level picker with native-row seams.
#![allow(dead_code, static_mut_refs)]
use std::sync::Mutex;
#[derive(Default)]
struct Ui { selected: u8, names: Vec<String>, checked: [bool; 2], radios: Vec<(u32,u32,u32)>, selections: Vec<u8>, fail: Option<usize> }
static UI: Mutex<Option<Ui>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Ui) -> T) -> T { f(UI.lock().unwrap().as_mut().unwrap()) }
mod dynamic_icon {
    pub const NAMES: [&[u8]; 2] = ["原版\0".as_bytes(), "动态\0".as_bytes()];
    pub unsafe fn selected() -> u8 { super::with(|u| u.selected) }
    pub unsafe fn select(style: u8) -> bool { super::with(|u| { u.selected = style; u.selections.push(style); }); true }
}
pub mod fw_api {
    pub const NONE_PTR: *const u8 = core::ptr::null();
    pub mod trailing { pub const RADIO: u32 = 6; }
    pub unsafe fn row_trailing(row: u32) -> u32 { row + 100 }
    pub unsafe fn obj_add_event(row: u32, callback: u32, code: u32, _: u32) -> u32 {
        super::with(|u| u.radios.push((row, callback, code))); 1
    }
    pub unsafe fn row_update(row: u32, a: *const u8, b: *const u8, c: *const u8, _: u32, checked: u8) {
        assert!(a.is_null() && b.is_null() && c.is_null());
        super::with(|u| u.checked[(row - 1000) as usize] = checked != 0);
    }
}
mod page {
    pub use crate::fw_api;
    pub use core::ptr::read_volatile;
    #[derive(Debug, PartialEq)] pub enum PageError { RowFailed(usize) }
    pub type PageResult = Result<(), PageError>;
    pub struct PageCtx { pub content: u32, pub title: u32, pub rows: *mut u32 }
    pub struct RowSpec<'a> { pub text: &'a [u8], pub sub: *const u8, pub trailing: u32, pub clickable: bool }
    pub trait Page {
        fn fill_title(&self, out: *mut u8);
        fn render(&self, ctx: &PageCtx) -> PageResult;
        fn on_click(&self, _: usize) {}
    }
    static mut ROWS: [u32; 12] = [0; 12];
    pub unsafe fn page_rows_ptr(pid: usize) -> *mut u32 { assert_eq!(pid,43); core::ptr::addr_of_mut!(ROWS).cast() }
    pub fn row_ev_fn(index: usize) -> u32 { 2000 + index as u32 }
    pub unsafe fn put_str(out: *mut u8, text: &[u8]) { core::ptr::copy_nonoverlapping(text.as_ptr(),out,text.len()); }
    pub unsafe fn build_row(ctx: &PageCtx, previous: u32, index: usize, spec: &RowSpec<'_>) -> u32 {
        assert_eq!(spec.trailing, 6); assert!(spec.clickable && spec.sub.is_null());
        assert_eq!(previous, if index == 0 { ctx.title } else { 999 + index as u32 });
        if super::with(|u| u.fail == Some(index)) { return 0; }
        let row = 1000 + index as u32;
        super::with(|u| u.names.push(std::str::from_utf8(spec.text).unwrap().trim_end_matches('\0').to_owned()));
        ctx.rows.add(index).write(row); row
    }
}
#[path = "../src/page/app_icon.rs"] mod app_icon;
use page::Page;
fn setup(selected: u8) -> page::PageCtx {
    *UI.lock().unwrap() = Some(Ui { selected, ..Ui::default() });
    let rows = unsafe { page::page_rows_ptr(43) };
    for index in 0..12 { unsafe { rows.add(index).write(0); } }
    page::PageCtx { content: 100, title: 200, rows }
}
#[test]
fn renders_only_original_and_dynamic_radio_rows() {
    let ctx = setup(1); app_icon::PAGE_APP_ICON.render(&ctx).unwrap();
    with(|u| {
        assert_eq!(u.names, ["原版", "动态"]);
        assert_eq!(u.checked, [false,true]);
        assert_eq!(u.radios.len(), 2);
        for (index, &(target, callback, code)) in u.radios.iter().enumerate() {
            assert_eq!((target,callback,code),(1100+index as u32,2000+index as u32,7));
        }
    });
}
#[test]
fn selection_updates_radios_in_place_without_rebuilding() {
    let ctx = setup(0); app_icon::PAGE_APP_ICON.render(&ctx).unwrap(); app_icon::PAGE_APP_ICON.on_click(1);
    with(|u| { assert_eq!(u.selected,1); assert_eq!(u.checked,[false,true]); assert_eq!(u.names.len(),2); });
    app_icon::PAGE_APP_ICON.on_click(0);
    with(|u| { assert_eq!(u.selected,0); assert_eq!(u.checked,[true,false]); });
    app_icon::PAGE_APP_ICON.on_click(2); with(|u| assert_eq!(u.selections,[1,0]));
}
#[test]
fn independent_page_title_and_id_match_the_third_level_route() {
    setup(0); assert_eq!(app_icon::APP_ICON_PID,43);
    let mut title = [0u8;48]; app_icon::PAGE_APP_ICON.fill_title(title.as_mut_ptr());
    assert_eq!(&title[..13],"应用图标\0".as_bytes());
}
#[test]
fn failed_row_stops_construction() {
    let ctx = setup(0); with(|u| u.fail = Some(1));
    assert_eq!(app_icon::PAGE_APP_ICON.render(&ctx),Err(page::PageError::RowFailed(1)));
    with(|u| assert_eq!(u.names.len(),1));
}
