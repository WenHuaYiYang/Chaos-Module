//! Host VFS/display fixture used by the real native overlay and controller.
use crate::*;
pub type Obj = u32;
pub const LV_OBJ_FLAG_CLICKABLE: u32 = 2;
pub const LV_OBJ_FLAG_SCROLLABLE: u32 = 16;
pub const LV_OBJ_FLAG_SCROLL_CHAIN: u32 = 0x300;
pub const LV_OBJ_FLAG_GESTURE_BUBBLE: u32 = 0x8000;
pub mod ev {
    pub const ALL: u32 = 0;
    pub const CLICKED: u32 = 7;
}
pub mod oflag {
    pub const RDONLY: u32 = 1;
    pub const WRONLY: u32 = 2;
    pub const CREAT: u32 = 4;
}
#[path = "../src/fw_api/align.rs"]
pub mod align;
#[path = "../src/fw_api/floating_ball.rs"]
pub mod floating_ball;
pub mod honeycomb {
    pub unsafe fn point(_: u32) -> crate::floating_ball::model::Point {
        crate::world(|w| w.input.point)
    }
}
pub unsafe fn event_get_code(_: u32) -> u32 {
    world(|w| w.input.code)
}
pub unsafe fn event_user_data(_: u32) -> u32 {
    world(|w| w.input.tag)
}
pub unsafe fn display_default() -> u32 {
    100
}
pub unsafe fn display_top_layer(_: u32) -> u32 {
    1
}
pub unsafe fn obj_create(parent: u32, _: u32) -> u32 {
    allocate(parent)
}
pub unsafe fn label_create(parent: u32) -> u32 {
    allocate(parent)
}
pub unsafe fn background_image_create(parent: u32) -> u32 {
    allocate(parent)
}
pub unsafe fn obj_is_child_of(parent: u32, child: u32) -> bool {
    world(|w| w.objects.get(&child).is_some_and(|o| o.parent == parent))
}
pub unsafe fn obj_delete(object: u32) {
    remove_tree(object);
}
pub unsafe fn obj_clear_box_spacing(_: u32) {}
pub unsafe fn obj_set_size_wh(object: u32, width: u32, height: u32) {
    world(|w| w.objects.get_mut(&object).unwrap().size = (width, height));
}
pub unsafe fn obj_clear_flag(object: u32, flag: u32) {
    world(|w| w.objects.get_mut(&object).unwrap().flags &= !flag);
}
pub unsafe fn obj_add_flag(object: u32, flag: u32) {
    world(|w| w.objects.get_mut(&object).unwrap().flags |= flag);
}
pub unsafe fn background_set_opa(object: u32, opacity: u32) {
    obj_set_local_style_prop(object, 29, opacity, 0);
}
pub unsafe fn obj_style_color(object: u32, color: u32, _: u32) {
    obj_set_local_style_prop(object, 28, color, 0);
}
pub unsafe fn obj_set_local_style_prop(object: u32, property: u32, value: u32, _: u32) {
    world(|w| {
        w.objects
            .get_mut(&object)
            .unwrap()
            .properties
            .insert(property, value);
    });
}
pub unsafe fn obj_align(object: u32, _: u32, x: i32, y: i32) {
    world(|w| w.objects.get_mut(&object).unwrap().position = (x, y));
}
pub unsafe fn obj_add_event(object: u32, _: u32, code: u32, tag: u32) {
    world(|w| w.objects.get_mut(&object).unwrap().events.push((code, tag)));
}
pub unsafe fn label_apply_defaults(_: u32, _: u32) {}
pub unsafe fn fw_mfont_create_named(_: *const u8, _: u32, _: u32) -> u32 {
    0
}
pub unsafe fn obj_set_style_text_font(_: u32, _: u32, _: u32) {}
pub unsafe fn label_set_text(object: u32, text: *const u8) {
    let text = cstr(text);
    world(|w| w.objects.get_mut(&object).unwrap().text = text);
}
pub unsafe fn s541_clean_pixels(_: *const u8, _: u32) {}
pub unsafe fn s541_frame_idle() -> bool {
    world(|w| w.gpu_idle)
}
pub unsafe fn fw_img_free_by_path(_: u32) {}
pub unsafe fn background_image_set_pivot(_: u32, _: i32, _: i32) {}
pub unsafe fn background_image_set_source(object: u32, source: u32) {
    world(|w| {
        w.image_publications += 1;
        w.objects
            .get_mut(&object)
            .unwrap()
            .properties
            .insert(1000, source);
    });
}
pub unsafe fn image_set_scale(_: u32, _: i32, _: i32) {}
pub unsafe fn timer_create(_: u32, period: u32, _: u32) -> u32 {
    world(|w| {
        let id = 1000 + w.timers.len() as u32;
        w.timers.insert(id, period);
        id
    })
}
pub unsafe fn timer_period_of(timer: u32) -> u32 {
    world(|w| *w.timers.get(&timer).unwrap_or(&0))
}
pub unsafe fn timer_set_period(timer: u32, period: u32) {
    world(|w| *w.timers.get_mut(&timer).unwrap() = period);
}
pub fn capture_now_ms() -> u32 {
    world(|w| w.now)
}
pub unsafe fn screen_is_on() -> bool {
    world(|w| w.on)
}
pub unsafe fn screen_timeout_ms() -> u32 {
    world(|w| w.timeout)
}
pub unsafe fn screen_timeout_set(value: u32) {
    world(|w| {
        w.timeout = value;
        w.timeout_refreshes += 1;
    });
}
pub unsafe fn screen_timeout_refresh() {
    world(|w| w.timeout_refreshes += 1);
}
pub unsafe fn brightness_get_target() -> u32 {
    world(|w| w.brightness)
}
pub unsafe fn brightness_get_auto_adjustment() -> u32 {
    world(|w| w.auto)
}
pub unsafe fn brightness_set_auto_adjustment(value: u32) {
    world(|w| w.auto = value);
}
pub unsafe fn brightness_apply_raw(value: u32) {
    world(|w| {
        w.brightness = value;
        w.brightness_writes += 1;
    });
}
pub unsafe fn local_time_now() -> Option<[i32; 11]> {
    world(|w| {
        w.clock_reads += 1;
        let mut tm = [0; 11];
        tm[2] = 9;
        tm[1] = (7 + w.now.wrapping_sub(1000) / 60_000) as i32 % 60;
        Some(tm)
    })
}
pub unsafe fn capture_result(value: i32) -> i32 {
    value
}
pub unsafe fn capture_mkdir_path(_: *const u8) -> i32 {
    0
}
pub unsafe fn open(path: *const u8, flags: u32, _: u32) -> i32 {
    let name = cstr(path);
    world(|w| {
        if flags & oflag::CREAT != 0 {
            w.files.entry(name.clone()).or_default();
            if flags & 0x20 != 0 {
                w.files.get_mut(&name).unwrap().clear();
            }
        } else if !w.files.contains_key(&name) {
            return -2;
        }
        let fd = w.next_fd;
        w.next_fd += 1;
        w.handles.insert(fd, (name, 0));
        fd
    })
}
pub unsafe fn close(fd: i32) -> i32 {
    world(|w| {
        assert!(w.handles.remove(&fd).is_some());
        let error = w.close_error;
        w.close_error = 0;
        error
    })
}
pub unsafe fn nsh_read(fd: i32, out: &mut [u8]) -> i32 {
    world(|w| {
        w.reads += 1;
        let (name, at) = w.handles.get_mut(&fd).unwrap();
        let bytes = &w.files[name];
        let n = out
            .len()
            .min(w.read_limit)
            .min(bytes.len().saturating_sub(*at));
        out[..n].copy_from_slice(&bytes[*at..*at + n]);
        *at += n;
        n as i32
    })
}
pub unsafe fn nsh_write(fd: i32, input: &[u8]) -> i32 {
    world(|w| {
        w.writes += 1;
        if w.write_budget == 0 {
            return -28;
        }
        let n = input.len().min(w.write_limit).min(w.write_budget);
        w.write_budget -= n;
        let (name, at) = w.handles.get_mut(&fd).unwrap();
        let bytes = w.files.get_mut(name).unwrap();
        bytes.resize(bytes.len().max(*at + n), 0);
        bytes[*at..*at + n].copy_from_slice(&input[..n]);
        *at += n;
        n as i32
    })
}
pub unsafe fn fs_remove(path: *const u8) -> i32 {
    world(|w| {
        w.files.remove(&cstr(path));
        0
    })
}
pub unsafe fn capture_rename(old: *const u8, new: *const u8) -> i32 {
    world(|w| {
        if w.rename_error != 0 {
            return w.rename_error;
        }
        let bytes = w.files.remove(&cstr(old)).unwrap();
        w.files.insert(cstr(new), bytes);
        0
    })
}

pub unsafe fn battery_percent() -> Option<u8> {
    world(|w| {
        w.battery_reads += 1;
        w.battery
    })
}
pub unsafe fn vibration_muted() -> bool {
    world(|w| w.muted)
}
pub unsafe fn vibration_set_muted(muted: bool) {
    world(|w| w.muted = muted);
}
pub unsafe fn floating_ball_home() -> bool {
    world(|w| {
        w.home_requests += 1;
        if w.home_available && w.home_step {
            w.home_depth = w.home_depth.saturating_sub(1).max(1);
            if w.home_depth == 1 {
                w.home_app = HOME_APP_ID;
                w.home_watchface = true;
            }
        }
        w.home_available
    })
}

pub const HOME_APP_ID: u32 = 16;
pub unsafe fn page_stack_depth() -> u32 {
    world(|w| w.home_depth)
}
pub unsafe fn top_app_id() -> u32 {
    world(|w| w.home_app)
}
pub unsafe fn watchface_is_on() -> u32 {
    world(|w| w.home_watchface as u32)
}
