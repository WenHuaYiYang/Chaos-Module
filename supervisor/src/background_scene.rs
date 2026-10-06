//! 背景素材与页面图像的所有权；调用方必须在稳定的 UI 上下文使用。

use crate::background_pixels::{self as pixels, BlurJob};
use crate::fw_api;
use crate::mem::{rd8, rd16, rd32, safe_ptr};

const DESCRIPTOR_BYTES: usize = 28;
const DATA_ALIGNMENT: u32 = 48;
const LOW_OFFSET: usize = DESCRIPTOR_BYTES + DATA_ALIGNMENT as usize - 1 + pixels::FULL_BYTES;
const ALLOCATION_BYTES: usize = LOW_OFFSET + pixels::PIXEL_BYTES + pixels::SCRATCH_BYTES;
const DELETE_EVENT: u32 = 0x24;
const BG_OPA: u32 = 0x1D;

pub struct Backdrop<const N: usize = 3> {
    allocation: u32,
    job: Option<BlurJob>,
    ready: bool,
    expanding: bool,
    expanded_rows: usize,
    parents: [u32; N],
    images: [u32; N],
    opacity: [u32; N],
    generation: u32,
}

impl<const N: usize> Backdrop<N> {
    fn data(&self) -> u32 {
        (self.allocation + DESCRIPTOR_BYTES as u32 + DATA_ALIGNMENT - 1)
            / DATA_ALIGNMENT * DATA_ALIGNMENT
    }
    pub const fn new() -> Self {
        Self { allocation: 0, job: None, ready: false, expanding: false, expanded_rows: 0, parents: [0; N],
               images: [0; N], opacity: [0; N], generation: 0 }
    }

    /// 捕获后立即降采样并归还共享槽，后续批次仅使用自有内存。
    pub unsafe fn capture(&mut self) -> bool {
        if self.allocation != 0 { return false; }
        let buffer = fw_api::background_snapshot_acquire();
        if buffer == 0 { return false; }
        let valid = safe_ptr(buffer) && rd8(buffer as *const u8) == 25
            && rd8((buffer + 1) as *const u8) == 15;
        let mut copied = false;
        if valid {
            let width = rd16((buffer + 4) as *const u16) as usize;
            let height = rd16((buffer + 6) as *const u16) as usize;
            let stride = rd16((buffer + 8) as *const u16) as usize;
            let size = rd32((buffer + 12) as *const u32) as usize;
            let source = rd32((buffer + 16) as *const u32);
            if width == pixels::SOURCE_WIDTH && height == pixels::SOURCE_HEIGHT
                && stride >= width * 3 && size >= stride * height && size <= 1024 * 1024
                && safe_ptr(source) {
                self.allocation = fw_api::heap_malloc(ALLOCATION_BYTES as u32);
                if self.allocation != 0 {
                    let input = core::slice::from_raw_parts(source as *const u8, size);
                    let output = core::slice::from_raw_parts_mut(
                        (self.allocation + LOW_OFFSET as u32) as *mut u8, pixels::PIXEL_BYTES);
                    copied = pixels::downsample(input, width, height, stride, output).is_ok();
                }
            }
        }
        let returned = fw_api::background_snapshot_release(buffer) != 0;
        if !copied || !returned { self.release(); return false; }
        self.job = Some(BlurJob::new());
        true
    }

    /// 固定素材必须是完整的 84 x 120 RGB888 编码，复制后不保留输入。
    pub unsafe fn load_fixed(&mut self, encoded: &[u8]) -> bool {
        if self.allocation != 0 || encoded.len() != 12 + pixels::PIXEL_BYTES
            || encoded[..12] != [25, 15, 0, 0, 84, 0, 120, 0, 252, 0, 0, 0] {
            return false;
        }
        self.allocation = fw_api::heap_malloc(ALLOCATION_BYTES as u32);
        if self.allocation == 0 { return false; }
        core::ptr::copy_nonoverlapping(encoded.as_ptr().add(12),
            (self.allocation + LOW_OFFSET as u32) as *mut u8, pixels::PIXEL_BYTES);
        self.expanding = true;
        self.expanded_rows = 0;
        true
    }

    /// 每拍只做一批模糊或展开，完整素材就绪前不挂到页面。
    pub unsafe fn step(&mut self) -> bool {
        if self.ready { return true; }
        if self.expanding {
            let source = core::slice::from_raw_parts(
                (self.allocation + LOW_OFFSET as u32) as *const u8, pixels::PIXEL_BYTES);
            let output = core::slice::from_raw_parts_mut(
                self.data() as *mut u8, pixels::FULL_BYTES);
            match pixels::expand_rows(source, output, self.expanded_rows, pixels::MAX_BATCH_LINES) {
                Ok(count) => self.expanded_rows += count,
                Err(_) => { self.release(); return false; }
            }
            if self.expanded_rows == pixels::SOURCE_HEIGHT { self.finish(); }
            return self.ready;
        }
        let Some(job) = self.job.as_mut() else { return false; };
        let output = core::slice::from_raw_parts_mut(
            (self.allocation + LOW_OFFSET as u32) as *mut u8, pixels::PIXEL_BYTES);
        let scratch = core::slice::from_raw_parts_mut(
            (self.allocation + (LOW_OFFSET + pixels::PIXEL_BYTES) as u32) as *mut u8,
            pixels::SCRATCH_BYTES);
        if job.step(output, scratch, pixels::MAX_BATCH_LINES).is_err() {
            self.release();
            return false;
        }
        if job.is_done() {
            self.job = None;
            self.expanding = true;
            self.expanded_rows = 0;
        }
        false
    }

    unsafe fn finish(&mut self) {
        let descriptor = self.allocation as *mut u8;
        core::ptr::write_bytes(descriptor, 0, DESCRIPTOR_BYTES);
        core::ptr::copy_nonoverlapping(
            [25u8, 15, 0, 0, 80, 1, 224, 1, 240, 3, 0, 0].as_ptr(), descriptor, 12);
        core::ptr::write_volatile(descriptor.add(12) as *mut u32, pixels::FULL_BYTES as u32);
        core::ptr::write_volatile(descriptor.add(16) as *mut u32,
                                  self.data());
        self.job = None;
        self.expanding = false;
        self.ready = true;
    }

    /// 只接受经生命周期门确认存活的父对象，创建失败则保持系统底板。
    /// 回调只记录 userdata 与目标，返回后再调用 deleted，不在回调里释放素材。
    pub unsafe fn attach(&mut self, parents: [u32; N], callback: u32) -> bool {
        if !self.ready || self.images.iter().any(|&image| image != 0)
            || callback & 1 == 0 || N == 0 || N > 4 || parents.iter().all(|&parent| parent == 0)
            || parents.iter().any(|&parent| parent != 0 && !safe_ptr(parent))
            || self.generation == 0x3FFF_FFFF {
            return false;
        }
        for i in 0..N {
            if parents[i] != 0 && parents[..i].contains(&parents[i]) { return false; }
        }
        self.generation += 1;
        for slot in 0..N {
            let parent = parents[slot];
            if parent == 0 { continue; }
            let image = fw_api::background_image_create(parent);
            if image == 0 { self.detach(); return false; }
            self.parents[slot] = parent;
            self.images[slot] = image;
            self.opacity[slot] = fw_api::fw_style_get_prop(parent, 0, BG_OPA);
            if fw_api::obj_add_event(image, callback, DELETE_EVENT,
                                    self.generation << 2 | slot as u32) == 0 {
                self.detach();
                return false;
            }
            fw_api::background_image_set_floating(image);
            fw_api::background_image_set_source(image, self.allocation);
            fw_api::background_image_set_scale(image, 256, 256);
            fw_api::background_image_set_pivot(image, 0, 0);
            fw_api::obj_align(image, 9, 0, 0);
            fw_api::background_clear_flags(image, 0x12);
            fw_api::background_image_move_back(image);
        }
        for parent in parents {
            if parent != 0 { fw_api::background_set_opa(parent, 0); }
        }
        true
    }

    /// 删除事件返回后撤销句柄；代号阻止旧事件误清新一批图像。
    pub fn deleted(&mut self, tag: u32, target: u32) -> bool {
        let slot = (tag & 3) as usize;
        if slot < N && tag >> 2 == self.generation && self.images[slot] == target {
            self.images[slot] = 0;
            return true;
        }
        false
    }

    /// 仅向调用方重新验证存活的业务根恢复被外部删除图像留下的底色。
    pub unsafe fn restore_deleted(&mut self, live_parents: [u32; N]) {
        for slot in 0..N {
            if self.images[slot] == 0 && self.parents[slot] != 0 {
                if self.parents[slot] == live_parents[slot] {
                    fw_api::background_set_opa(live_parents[slot], self.opacity[slot]);
                }
                self.parents[slot] = 0;
            }
        }
    }

    /// 在 UI 调度中先恢复底板，再删除仍在原子表里的图像。
    pub unsafe fn detach(&mut self) {
        for slot in 0..N {
            let parent = self.parents[slot];
            let image = self.images[slot];
            if image != 0 && parent != 0 && fw_api::obj_is_child_of(parent, image) {
                fw_api::background_set_opa(parent, self.opacity[slot]);
                // 删除会重入事件回调，先撤销本次句柄。
                self.images[slot] = 0;
                self.parents[slot] = 0;
                fw_api::obj_delete(image);
            }
        }
    }

    /// 所有图像的删除与类析构均已返回后，才能退役缓存和释放像素。
    pub unsafe fn release(&mut self) -> bool {
        if self.images.iter().any(|&image| image != 0) { return false; }
        let cached = self.ready;
        self.job = None;
        self.ready = false;
        self.expanding = false;
        self.expanded_rows = 0;
        self.parents = [0; N];
        if self.allocation != 0 {
            if cached { fw_api::fw_img_free_by_path(self.allocation); }
            fw_api::fw_free(self.allocation);
            self.allocation = 0;
        }
        true
    }

    pub fn descriptor(&self) -> u32 { self.allocation }
    pub fn pending(&self) -> bool { self.job.is_some() || self.expanding }
    pub fn matches(&self, parents: [u32; N]) -> bool {
        self.ready && self.parents == parents
            && (0..N).all(|i| parents[i] == 0 || self.images[i] != 0)
    }

    pub fn unmounted(&self) -> bool { self.images.iter().all(|&image| image == 0) }
}
