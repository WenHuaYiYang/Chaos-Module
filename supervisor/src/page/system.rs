//! 页22/24/25/26/27：系统关于页的版本、文本、名称与型号修改。
//!
//! 四个"预设 + 自定义"选择页(22/25/26/27)统一走 kit/checklist: 建复选框行、
//! 补写勾选、点击派发与原地刷新全部由 spec 驱动, 页面只提供数据函数。

use crate::page::*;
use crate::page::Page;
use crate::page::frame;
use crate::page::kit::checklist::{click as ck_click, render as ck_render, CheckSpec};
use crate::page::kit::{MenuSpec, RowModel};

// ===== 页22: 设置 -> 系统版本显示 =====
pub(crate) struct SystemVersionPage;
pub(super) static PAGE_SYSVERP: SystemVersionPage = SystemVersionPage;

struct SysVerCheck;
impl CheckSpec for SysVerCheck {
    fn count(&self) -> usize { 4 }
    unsafe fn label(&self, i: usize) -> *const u8 {
        match i {
            0 => "3.101.043\0".as_ptr(),
            1 => "4.103.022\0".as_ptr(),
            2 => "91.78.13\0".as_ptr(),
            _ => "自定义\0".as_ptr(),
        }
    }
    unsafe fn sub(&self, i: usize) -> *const u8 {
        if i == 3 { crate::system_version::custom_display() } else { core::ptr::null() }
    }
    unsafe fn selected(&self, i: usize) -> bool { crate::system_version::selected() == i }
    unsafe fn choose(&self, i: usize) {
        if i < 3 { crate::system_version::choose(i); } else { crate::system_version::edit_custom(); }
    }
}

impl Page for SystemVersionPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统版本显示\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { ck_render(&SysVerCheck, ctx).map(|_| ()) }
    }

    fn on_click(&self, idx: usize) {
        unsafe { ck_click(&SysVerCheck, Pid::SysVer as u32, idx) }
    }
}

// ===== 页24: 设置 -> 系统关于页面修改 =====
pub(crate) struct SystemAboutPage;
pub(super) static PAGE_ABOUT_MODP: SystemAboutPage = SystemAboutPage;
static ABOUT_MOD_ROWS: &[RowModel] = &[
        RowModel::nav("系统版本显示\0".as_bytes(), Pid::SysVer as u32),
        RowModel::nav("手环系统文本\0".as_bytes(), Pid::SysText as u32),
        RowModel::nav("设备名称\0".as_bytes(), Pid::DevName as u32),
        RowModel::nav("手环型号\0".as_bytes(), Pid::DevModel as u32),
];

impl MenuSpec for SystemAboutPage {
    fn rows(&self) -> &'static [RowModel] { ABOUT_MOD_ROWS }
}

impl Page for SystemAboutPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统关于页面修改\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }

    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
}

// ===== 页25: 系统关于页面修改 -> 手环系统文本 =====
pub(crate) struct SystemTextPage;
pub(super) static PAGE_SYSTEXTP: SystemTextPage = SystemTextPage;

struct SysTextCheck;
impl CheckSpec for SysTextCheck {
    fn count(&self) -> usize { 3 }
    unsafe fn label(&self, i: usize) -> *const u8 {
        match i {
            0 => "Xiaomi HyperOS\0".as_ptr(),
            1 => "HarmonyOS\0".as_ptr(),
            _ => "自定义\0".as_ptr(),
        }
    }
    unsafe fn sub(&self, i: usize) -> *const u8 {
        if i == 2 { crate::system_text::custom_display() } else { core::ptr::null() }
    }
    unsafe fn selected(&self, i: usize) -> bool { crate::system_text::selected() == i }
    unsafe fn choose(&self, i: usize) {
        if i < 2 { crate::system_text::choose(i); } else { crate::system_text::edit_custom(); }
    }
}

impl Page for SystemTextPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环系统文本\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { ck_render(&SysTextCheck, ctx).map(|_| ()) }
    }

    fn on_click(&self, idx: usize) {
        unsafe { ck_click(&SysTextCheck, Pid::SysText as u32, idx) }
    }
}

// ===== 页26: 系统关于页面修改 -> 设备名称 =====
pub(crate) struct DeviceNamePage;
pub(super) static PAGE_DEVNAMEP: DeviceNamePage = DeviceNamePage;

struct DevNameCheck;
impl CheckSpec for DevNameCheck {
    fn count(&self) -> usize { crate::device_name::preset_count() + 1 }
    unsafe fn label(&self, i: usize) -> *const u8 {
        let custom = crate::device_name::preset_count();
        if i < custom { crate::device_name::preset_text(i).as_ptr() } else { "自定义\0".as_ptr() }
    }
    unsafe fn sub(&self, i: usize) -> *const u8 {
        if i == crate::device_name::preset_count() { crate::device_name::custom_display() }
        else { core::ptr::null() }
    }
    unsafe fn selected(&self, i: usize) -> bool { crate::device_name::selected() == i }
    unsafe fn choose(&self, i: usize) {
        let custom = crate::device_name::preset_count();
        if i < custom { crate::device_name::choose(i); } else { crate::device_name::edit_custom(); }
    }
}

impl Page for DeviceNamePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设备名称\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { ck_render(&DevNameCheck, ctx).map(|_| ()) }
    }

    fn on_click(&self, idx: usize) {
        unsafe { ck_click(&DevNameCheck, Pid::DevName as u32, idx) }
    }
}

// ===== 页27: 系统关于页面修改 -> 手环型号 =====
pub(crate) struct DeviceModelPage;
pub(super) static PAGE_DEVMODELP: DeviceModelPage = DeviceModelPage;

struct DevModelCheck;
impl CheckSpec for DevModelCheck {
    fn count(&self) -> usize { 4 }
    unsafe fn label(&self, i: usize) -> *const u8 {
        match i {
            0 => "M2551B1\0".as_ptr(),
            1 => "M2553B1\0".as_ptr(),
            2 => "M9178B13\0".as_ptr(),
            _ => "自定义\0".as_ptr(),
        }
    }
    unsafe fn sub(&self, i: usize) -> *const u8 {
        if i == 3 { crate::device_model::custom_display() } else { core::ptr::null() }
    }
    unsafe fn selected(&self, i: usize) -> bool { crate::device_model::selected() == i }
    unsafe fn choose(&self, i: usize) {
        if i < 3 { crate::device_model::choose(i); } else { crate::device_model::edit_custom(); }
    }
}

impl Page for DeviceModelPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环型号\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { ck_render(&DevModelCheck, ctx).map(|_| ()) }
    }

    fn on_click(&self, idx: usize) {
        unsafe { ck_click(&DevModelCheck, Pid::DevModel as u32, idx) }
    }
}
