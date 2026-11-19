//! Global overlay on the existing, verified p67/.043 display top layer.
//! All creation/deletion and text updates run from the maintenance UI timer.
use super::*;
use crate::{
    floating_ball::{
        model::{Action, Config, Point, View, HEIGHT, WIDTH},
        pixels,
    },
    mem::safe_ptr,
};

pub const BALL: u32 = 0;
pub const DISMISS: u32 = 1;
pub const BAND: u32 = 2;
pub const ACTION: u32 = 10;
pub const MINUS_TEN: u32 = 20;
pub const MINUS_ONE: u32 = 21;
pub const PLUS_ONE: u32 = 22;
pub const PLUS_TEN: u32 = 23;
pub const CLOSE: u32 = 24;
pub const ROOT: u32 = 90;
const HIDDEN: u32 = 1;
// LVGL flags already used by the launcher: floating=bit18, gesture bubble=bit15.
// PRESS_LOCK=bit13 keeps a long-held ball as the target when its bounds move.
const FLOATING: u32 = 1 << 18;
const PRESS_LOCK: u32 = 1 << 13;
const BUBBLE: u32 = (1 << 14) | LV_OBJ_FLAG_GESTURE_BUBBLE;

#[derive(Clone, Copy)]
pub struct Input {
    pub code: u32,
    pub tag: u32,
    pub point: Point,
}
pub unsafe fn input(event: u32) -> Input {
    let code = event_get_code(event);
    let tag = event_user_data(event);
    let point = if matches!(tag, BALL | BAND) && matches!(code, 1 | 2 | 5 | 7 | 8) {
        let p = honeycomb::point(event);
        Point { x: p.x, y: p.y }
    } else {
        Point::default()
    };
    Input { code, tag, point }
}

#[repr(align(64))]
struct Pixels([u8; pixels::BYTES]);
#[repr(align(64))]
struct Descriptor([u32; 7]);
static mut PIXELS: [Pixels; 2] = [Pixels([0; pixels::BYTES]), Pixels([0; pixels::BYTES])];
static mut DESCRIPTORS: [Descriptor; 2] = [Descriptor([0; 7]), Descriptor([0; 7])];
static mut FRAME_TIMER: u32 = 0;
static mut FRAME_PERIOD: u32 = 1000;
static mut LABEL_FACE: u32 = 0;
static mut FACE_TRIED: bool = false;

pub unsafe fn ensure_frames(callback: u32) {
    if FRAME_TIMER == 0 {
        FRAME_TIMER = timer_create(callback, FRAME_PERIOD, 0);
    }
}
pub unsafe fn frames_active(active: bool) {
    if FRAME_TIMER == 0 || timer_period_of(FRAME_TIMER) != FRAME_PERIOD {
        return;
    }
    let period = if active { 16 } else { 1000 };
    if period != FRAME_PERIOD {
        timer_set_period(FRAME_TIMER, period);
        FRAME_PERIOD = period;
    }
}
#[cfg(test)]
pub unsafe fn reset_for_test() {
    FRAME_TIMER = 0;
    FRAME_PERIOD = 1000;
}

unsafe fn top_layer() -> u32 {
    let display = display_default();
    if !safe_ptr(display) {
        return 0;
    }
    let top = display_top_layer(display);
    if safe_ptr(top) {
        top
    } else {
        0
    }
}
unsafe fn visible(object: u32, show: bool) {
    if show {
        obj_clear_flag(object, HIDDEN);
    } else {
        obj_add_flag(object, HIDDEN);
    }
}
unsafe fn plain(parent: u32, width: u32, height: u32) -> u32 {
    let object = obj_create(parent, 0);
    if object != 0 {
        obj_clear_box_spacing(object);
        obj_set_size_wh(object, width, height);
        obj_clear_flag(
            object,
            LV_OBJ_FLAG_CLICKABLE | LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_SCROLL_CHAIN | BUBBLE,
        );
        obj_add_flag(object, FLOATING);
        background_set_opa(object, 0);
    }
    object
}
unsafe fn label(parent: u32, text: &[u8]) -> u32 {
    let object = label_create(parent);
    if object != 0 {
        label_apply_defaults(object, 0);
        if !FACE_TRIED {
            FACE_TRIED = true;
            let face = fw_mfont_create_named(b"MiSans-Regular\0".as_ptr(), 20, 0);
            if safe_ptr(face) {
                LABEL_FACE = face;
            }
        }
        if LABEL_FACE != 0 {
            obj_set_style_text_font(object, LABEL_FACE, 0);
        }
        label_set_text(object, text.as_ptr());
        obj_clear_flag(object, LV_OBJ_FLAG_CLICKABLE | BUBBLE);
        obj_align(object, align::CENTER, 0, 0);
    }
    object
}
unsafe fn button(parent: u32, width: u32, height: u32, tag: u32, callback: u32) -> u32 {
    let object = plain(parent, width, height);
    if object != 0 {
        obj_add_flag(object, LV_OBJ_FLAG_CLICKABLE);
        obj_set_local_style_prop(object, 0x1c, 0x7fff, 0);
        obj_style_color(object, 0x484c55, 0);
        background_set_opa(object, 215);
        obj_add_event(object, callback, ev::CLICKED, tag);
    }
    object
}

#[repr(align(64))]
struct BandPixels([u8; pixels::BAND_BYTES]);
static mut BAND_PIXELS: [BandPixels; 2] = [
    BandPixels([0; pixels::BAND_BYTES]),
    BandPixels([0; pixels::BAND_BYTES]),
];
static mut BAND_DESCRIPTORS: [Descriptor; 2] = [Descriptor([0; 7]), Descriptor([0; 7])];

static mut BAND_PAINTER: pixels::BandPainter = pixels::BandPainter::NEW;
#[repr(align(64))]
struct TextPixels([u8; crate::floating_ball::curved::BYTES]);
static mut TEXT_PIXELS: [TextPixels; 6] =
    [const { TextPixels([0; crate::floating_ball::curved::BYTES]) }; 6];
static mut TEXT_DESCRIPTORS: [Descriptor; 6] = [const { Descriptor([0; 7]) }; 6];

pub struct Overlay {
    top: u32,
    root: u32,
    ball: u32,
    image: u32,
    scrim: u32,
    band: u32,
    hit: u32,
    text_layers: [u32; 3],
    toast: u32,
    toast_text: u32,
    shown_size: u8,
    ball_key: Option<(u8, Option<u8>)>,
    ball_slot: usize,
    shown_position: Option<Point>,
    shown: bool,
    current: View,
    previous: View,
    band_slot: usize,
    band_key: Option<crate::floating_ball::model::BandStyle>,
    text_slots: [usize; 3],
    text_keys: [Option<crate::floating_ball::curved::Style>; 3],
}
impl Overlay {
    pub unsafe fn create(callback: u32) -> Option<Self> {
        let top = top_layer();
        if top == 0 {
            return None;
        }
        let root = plain(top, WIDTH as u32, HEIGHT as u32);
        if root == 0 {
            return None;
        }
        obj_align(root, align::TOP_LEFT, 0, 0);
        obj_add_event(root, callback, 36, ROOT);
        let mut ui = Self {
            top,
            root,
            ball: 0,
            image: 0,
            scrim: 0,
            band: 0,
            hit: 0,
            text_layers: [0; 3],
            toast: 0,
            toast_text: 0,
            shown_size: 0,
            ball_key: None,
            ball_slot: 0,
            shown_position: None,
            shown: true,
            current: View::Closed,
            previous: View::Closed,
            band_slot: 0,
            band_key: None,
            text_slots: [0; 3],
            text_keys: [None; 3],
        };
        if !ui.build(callback) {
            ui.destroy();
            return None;
        }
        Some(ui)
    }
    unsafe fn build(&mut self, callback: u32) -> bool {
        self.scrim = plain(self.root, WIDTH as u32, HEIGHT as u32);
        if self.scrim == 0 {
            return false;
        }
        obj_add_flag(self.scrim, LV_OBJ_FLAG_CLICKABLE);
        obj_add_event(self.scrim, callback, ev::CLICKED, DISMISS);
        self.band = background_image_create(self.root);
        if self.band == 0 {
            return false;
        }
        obj_clear_flag(self.band, LV_OBJ_FLAG_CLICKABLE | BUBBLE);
        obj_add_flag(self.band, FLOATING);
        background_image_set_pivot(self.band, 0, 0);
        self.hit = plain(self.root, 180, 180);
        if self.hit == 0 {
            return false;
        }
        obj_add_flag(self.hit, LV_OBJ_FLAG_CLICKABLE | PRESS_LOCK);
        obj_add_event(self.hit, callback, ev::ALL, BAND);
        for object in &mut self.text_layers {
            *object = background_image_create(self.root);
            if *object == 0 {
                return false;
            }
            obj_clear_flag(*object, LV_OBJ_FLAG_CLICKABLE | BUBBLE);
            obj_add_flag(*object, FLOATING);
            background_image_set_pivot(*object, 0, 0);
            visible(*object, false);
        }
        self.toast = plain(self.root, 248, 88);
        if self.toast == 0 {
            return false;
        }
        obj_style_color(self.toast, 0x181a1e, 0);
        background_set_opa(self.toast, 215);
        obj_set_local_style_prop(self.toast, 0x1c, 24, 0);
        obj_align(self.toast, align::TOP_LEFT, 44, 196);
        self.toast_text = label(self.toast, b"\0");
        if self.toast_text == 0 {
            return false;
        }
        self.ball = plain(self.root, 48, 48);
        if self.ball == 0 {
            return false;
        }
        obj_add_flag(self.ball, LV_OBJ_FLAG_CLICKABLE | PRESS_LOCK);
        obj_add_event(self.ball, callback, ev::ALL, BALL);
        self.image = background_image_create(self.ball);
        if self.image == 0 {
            return false;
        }
        obj_clear_flag(self.image, LV_OBJ_FLAG_CLICKABLE | BUBBLE);
        for object in [self.scrim, self.band, self.hit, self.toast] {
            visible(object, false);
        }
        true
    }
    pub unsafe fn live(&self) -> bool {
        let top = top_layer();
        top != 0 && top == self.top && obj_is_child_of(top, self.root)
    }
    pub unsafe fn destroy(self) {
        if self.live() {
            obj_delete(self.root);
        }
    }
    pub unsafe fn show(&mut self, show: bool) {
        if self.shown != show {
            self.shown = show;
            visible(self.root, show);
        }
    }
    pub unsafe fn ball(&mut self, config: Config, position: Point, battery: Option<u8>) -> bool {
        let key = (config.opacity, battery);
        let mut ready = true;
        if self.ball_key != Some(key) {
            if s541_frame_idle() {
                let next = self.ball_slot ^ 1;
                if DESCRIPTORS[next].0[0] != 0 {
                    fw_img_free_by_path(DESCRIPTORS[next].0.as_ptr() as u32);
                }
                pixels::render_battery(&mut PIXELS[next].0, config.opacity, battery);
                s541_clean_pixels(PIXELS[next].0.as_ptr(), pixels::BYTES as u32);
                DESCRIPTORS[next].0 = [
                    0x11019,
                    pixels::SIDE as u32 | (pixels::SIDE as u32) << 16,
                    pixels::SIDE as u32 * 4,
                    pixels::BYTES as u32,
                    PIXELS[next].0.as_ptr() as u32,
                    0,
                    0,
                ];
                s541_clean_pixels(DESCRIPTORS[next].0.as_ptr().cast(), 64);
                background_image_set_source(self.image, DESCRIPTORS[next].0.as_ptr() as u32);
                self.ball_slot = next;
                self.ball_key = Some(key);
            } else {
                ready = false;
            }
        }
        visible(self.image, self.ball_key.is_some());
        if self.shown_size != config.size {
            obj_set_size_wh(self.ball, config.size as u32, config.size as u32);
            let scale = config.size as i32 * 256 / pixels::SIDE as i32;
            image_set_scale(self.image, scale, scale);
            obj_align(self.image, align::CENTER, 0, 0);
            self.shown_size = config.size;
        }
        if self.shown_position != Some(position) {
            obj_align(self.ball, align::TOP_LEFT, position.x, position.y);
            self.shown_position = Some(position);
        }
        ready
    }
    unsafe fn skin(&mut self, key: crate::floating_ball::model::BandStyle) -> bool {
        if self.band_key == Some(key) {
            return true;
        }
        if !s541_frame_idle() {
            return false;
        }
        let next = self.band_slot ^ 1;
        if BAND_DESCRIPTORS[next].0[0] != 0 {
            fw_img_free_by_path(BAND_DESCRIPTORS[next].0.as_ptr() as u32);
        }
        BAND_PAINTER.render(&mut BAND_PIXELS[next].0, key);
        s541_clean_pixels(BAND_PIXELS[next].0.as_ptr(), pixels::BAND_BYTES as u32);
        BAND_DESCRIPTORS[next].0 = [
            0x11019,
            pixels::BAND_SIDE as u32 | ((pixels::BAND_SIDE as u32) << 16),
            pixels::BAND_SIDE as u32 * 4,
            pixels::BAND_BYTES as u32,
            BAND_PIXELS[next].0.as_ptr() as u32,
            0,
            0,
        ];
        s541_clean_pixels(BAND_DESCRIPTORS[next].0.as_ptr().cast(), 64);
        background_image_set_source(self.band, BAND_DESCRIPTORS[next].0.as_ptr() as u32);
        self.band_slot = next;
        self.band_key = Some(key);
        true
    }
    unsafe fn text_skin(&mut self, index: usize, key: crate::floating_ball::curved::Style) -> bool {
        use crate::floating_ball::curved;
        if self.text_keys[index] == Some(key) {
            return true;
        }
        if !s541_frame_idle() {
            return false;
        }
        let next = self.text_slots[index] ^ 1;
        let slot = index * 2 + next;
        if TEXT_DESCRIPTORS[slot].0[0] != 0 {
            fw_img_free_by_path(TEXT_DESCRIPTORS[slot].0.as_ptr() as u32);
        }
        let Some(mask) = BAND_PAINTER.mask(key.size) else {
            return false;
        };
        curved::render(&mut TEXT_PIXELS[slot].0, key, mask);
        s541_clean_pixels(TEXT_PIXELS[slot].0.as_ptr(), curved::BYTES as u32);
        // Native I8 with alpha-bearing BGRA palette, identical to GIF icons.
        TEXT_DESCRIPTORS[slot].0 = [
            0x0A19,
            pixels::BAND_SIDE as u32 | ((pixels::BAND_SIDE as u32) << 16),
            pixels::BAND_SIDE as u32,
            curved::BYTES as u32,
            TEXT_PIXELS[slot].0.as_ptr() as u32,
            0,
            0,
        ];
        s541_clean_pixels(TEXT_DESCRIPTORS[slot].0.as_ptr().cast(), 64);
        background_image_set_source(
            self.text_layers[index],
            TEXT_DESCRIPTORS[slot].0.as_ptr() as u32,
        );
        self.text_slots[index] = next;
        self.text_keys[index] = Some(key);
        true
    }
    pub unsafe fn render(
        &mut self,
        config: Config,
        view: View,
        progress: u32,
        blend: u32,
        clock: &[u8; 6],
        keep: Option<u8>,
        brightness: u32,
    ) -> bool {
        use crate::floating_ball::{
            curved,
            model::{self, BandStyle},
        };
        if self.current != view {
            self.previous = self.current;
            self.current = view;
        }
        let blend = if view == View::Closed {
            1000
        } else {
            blend.min(1000)
        };
        let progress = progress.min(1000);
        let current = self.current;
        let previous = self.previous;
        let opacity = |which: View| -> u32 {
            if current == which {
                if previous == which {
                    255
                } else {
                    blend * 255 / 1000
                }
            } else if previous == which {
                (1000 - blend) * 255 / 1000
            } else {
                0
            }
        };
        let alphas = [
            opacity(View::Fan),
            opacity(View::Duration),
            opacity(View::Brightness),
        ];
        let has_band = (current.band() || previous.band()) && progress > 0;
        // scrim 只服务带弧带的三级菜单(Fan/Duration/Brightness): 它的作用是"点空白
        // 收起菜单"。Message 是 2 秒后自动收起的提示条, 不该拦截触摸 —— 原来按
        // `view != Closed` 显示, 于是每次快捷操作后都有最长 2 秒的"第一下点击被吃掉"。
        visible(self.scrim, view.band());
        visible(self.hit, view.band());
        let mut pending = false;
        if has_band {
            let highlight = if view == View::Duration {
                keep
            } else if view == View::Fan && keep.is_some() {
                config
                    .slots
                    .iter()
                    .position(|&a| a == Action::KeepAwake)
                    .map(|i| i as u8)
            } else {
                None
            };
            pending |= !self.skin(BandStyle {
                corner: config.corner(),
                size: config.size,
                opacity: config.opacity,
                brightness: if view == View::Brightness {
                    Some(model::brightness_progress(brightness))
                } else {
                    None
                },
                highlight,
            });
            let (_, outer) = model::band_radii(config.size);
            let q = 720 + progress * 280 / 1000;
            let radius = outer * q as i32 / 1000;
            let center = config.inward_point(0, 0);
            let x = if config.corner().right() {
                center.x - radius
            } else {
                center.x
            };
            let y = if config.corner().bottom() {
                center.y - radius
            } else {
                center.y
            };
            if self.band_key.is_some() {
                visible(self.band, true);
                image_set_scale(
                    self.band,
                    radius * 256 / pixels::BAND_SIDE as i32,
                    radius * 256 / pixels::BAND_SIDE as i32,
                );
                obj_align(self.band, align::TOP_LEFT, x, y);
                obj_set_local_style_prop(self.band, 95, progress * 255 / 1000, 0);
            } else {
                visible(self.band, false);
            }
            obj_set_size_wh(self.hit, outer as u32, outer as u32);
            obj_align(
                self.hit,
                align::TOP_LEFT,
                if config.corner().right() {
                    center.x - outer
                } else {
                    center.x
                },
                if config.corner().bottom() {
                    center.y - outer
                } else {
                    center.y
                },
            );
            for index in 0..3 {
                if alphas[index] == 0 {
                    visible(self.text_layers[index], false);
                    continue;
                }
                let key = curved::Style {
                    corner: config.corner(),
                    size: config.size,
                    kind: [
                        curved::Kind::Main,
                        curved::Kind::Duration,
                        curved::Kind::Brightness,
                    ][index],
                    slots: if index == 0 {
                        config.slots
                    } else {
                        Config::DEFAULT.slots
                    },
                    clock: if index == 0 { *clock } else { [0; 6] },
                    brightness: if index == 2 { brightness } else { 0 },
                };
                pending |= !self.text_skin(index, key);
                if self.text_keys[index].is_some() {
                    let object = self.text_layers[index];
                    visible(object, true);
                    image_set_scale(
                        object,
                        radius * 256 / pixels::BAND_SIDE as i32,
                        radius * 256 / pixels::BAND_SIDE as i32,
                    );
                    obj_align(object, align::TOP_LEFT, x, y);
                    obj_set_local_style_prop(object, 95, alphas[index] * progress / 1000, 0);
                }
            }
        } else {
            visible(self.band, false);
            for &object in &self.text_layers {
                visible(object, false);
            }
        }
        let toast_alpha = opacity(View::Message);
        visible(self.toast, toast_alpha != 0);
        if toast_alpha != 0 {
            obj_set_local_style_prop(self.toast, 95, toast_alpha, 0);
        }
        if blend == 1000 {
            self.previous = self.current;
        }
        pending
    }
    pub unsafe fn text(&self, _title: &[u8], value: &[u8]) {
        label_set_text(self.toast_text, value.as_ptr());
    }
}
