//! Host-only harness for the real keyboard module; no firmware addresses are called.
//! Run with tools/test_keyboard.sh (not cargo test: the supervisor is ARM no_std).
#![allow(dead_code, static_mut_refs)]

use std::ptr::read_volatile;

#[path = "../src/fw_api/align.rs"]
pub mod firmware_align;
#[path = "../src/keyboard.rs"]
mod keyboard;

const FW_STYLE_HR_NAME: u32 = 1;
static mut FG_PAGE: u32 = 0;

fn safe_ptr(ptr: u32) -> bool {
    mock::with(|ui| ptr != 0 && (ptr as usize) <= ui.objects.len())
}
fn page_is_live(_: usize) -> bool {
    mock::with(|ui| ui.live)
}
fn nav_goto(pid: u32, _: u32) {
    unsafe {
        FG_PAGE = pid;
    }
}
fn nav_back() {
    mock::with(|ui| ui.back_count += 1);
}

macro_rules! completion_mock {
    ($name:ident, $index:expr) => {
        mod $name {
            pub unsafe fn save_custom(text: *const u8) -> bool {
                let bytes = std::ffi::CStr::from_ptr(text.cast()).to_bytes().to_vec();
                crate::mock::with(|ui| {
                    ui.saves.push(($index, bytes));
                    ui.save_ok
                })
            }
        }
    };
}
completion_mock!(system_version, 0);
completion_mock!(system_text, 1);
completion_mock!(device_name, 2);
completion_mock!(device_model, 3);

mod mock {
    use std::sync::Mutex;

    pub static TEST_LOCK: Mutex<()> = Mutex::new(());
    static UI: Mutex<Option<Ui>> = Mutex::new(None);

    #[derive(Clone, Default, Debug)]
    pub struct Object {
        pub parent: u32,
        pub size: (i32, i32),
        pub pos: (i32, i32),
        pub alignment: u32,
        pub flags: u32,
        pub scroll_dir: u32,
        pub scroll_x: i32,
        pub spacing_cleared: bool,
        pub code: Option<u32>,
        pub text: Vec<u8>,
    }

    #[derive(Default)]
    pub struct Ui {
        pub objects: Vec<Object>,
        pub event_code: u32,
        pub key_code: u32,
        pub live: bool,
        pub save_ok: bool,
        pub saves: Vec<(u32, Vec<u8>)>,
        pub back_count: usize,
        pub fail_at: Option<usize>,
    }

    pub fn with<T>(f: impl FnOnce(&mut Ui) -> T) -> T {
        f(UI.lock().unwrap().as_mut().unwrap())
    }
    pub fn reset() {
        *UI.lock().unwrap() = Some(Ui {
            live: true,
            save_ok: true,
            ..Ui::default()
        });
    }
    pub fn edit(id: u32, f: impl FnOnce(&mut Object)) {
        with(|ui| f(&mut ui.objects[id as usize - 1]));
    }
    pub fn object(id: u32) -> Object {
        with(|ui| ui.objects[id as usize - 1].clone())
    }
    pub fn create(parent: u32) -> u32 {
        with(|ui| {
            if ui.fail_at == Some(ui.objects.len() + 1) {
                return 0;
            }
            ui.objects.push(Object {
                parent,
                flags: 0xbb76,
                scroll_dir: 0xf,
                ..Object::default()
            });
            ui.objects.len() as u32
        })
    }
}

mod fw_api {
    use super::mock;

    pub const LV_DIR_HOR: u32 = 0x3;
    pub const LV_OBJ_FLAG_CLICKABLE: u32 = 0x2;
    pub const LV_OBJ_FLAG_SCROLLABLE: u32 = 0x10;
    pub const LV_OBJ_FLAG_SCROLL_ELASTIC: u32 = 0x20;
    pub const LV_OBJ_FLAG_SCROLL_CHAIN: u32 = 0x300;
    pub const LV_OBJ_FLAG_GESTURE_BUBBLE: u32 = 0x8000;
    pub use crate::firmware_align as align;
    pub mod ev {
        pub const CLICKED: u32 = 7;
    }

    pub fn obj_create(parent: u32, _: u32) -> u32 {
        mock::create(parent)
    }
    pub fn label_create(parent: u32) -> u32 {
        mock::create(parent)
    }
    pub fn obj_set_size(id: u32, w: i32, h: i32) {
        mock::edit(id, |o| o.size = (w, h));
    }
    pub fn obj_set_size_wh(id: u32, w: u32, h: u32) {
        obj_set_size(id, w as i32, h as i32);
    }
    pub fn obj_align(id: u32, alignment: u32, x: i32, y: i32) {
        mock::edit(id, |o| {
            o.alignment = alignment;
            o.pos = (x, y);
        });
    }
    pub fn obj_style_color(_: u32, _: u32, _: u32) {}
    pub fn style_apply(_: u32, _: u32, _: u32, _: u32) {}
    pub fn obj_clear_box_spacing(id: u32) {
        mock::edit(id, |o| o.spacing_cleared = true);
    }
    pub fn obj_clear_flag(id: u32, flags: u32) {
        mock::edit(id, |o| o.flags &= !flags);
    }
    pub fn obj_add_flag(id: u32, flags: u32) {
        mock::edit(id, |o| o.flags |= flags);
    }
    pub fn obj_clear_scrollable(id: u32) {
        obj_clear_flag(id, LV_OBJ_FLAG_SCROLLABLE);
    }
    pub fn obj_set_scroll_dir(id: u32, dir: u32) {
        mock::edit(id, |o| o.scroll_dir = dir);
    }
    pub fn obj_add_event(id: u32, _: u32, event: u32, code: u32) {
        assert_eq!(event, ev::CLICKED);
        mock::edit(id, |o| o.code = Some(code));
    }
    pub unsafe fn label_set_text(id: u32, text: *const u8) {
        let bytes = std::ffi::CStr::from_ptr(text.cast()).to_bytes().to_vec();
        mock::edit(id, |o| o.text = bytes);
    }
    pub fn event_get_code(_: u32) -> u32 {
        mock::with(|ui| ui.event_code)
    }
    pub fn event_user_data(_: u32) -> u32 {
        mock::with(|ui| ui.key_code)
    }
}
