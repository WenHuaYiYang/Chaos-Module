//! 页22/24/25/26/27：系统关于页的版本、文本、名称与型号修改。

use crate::page::*;
use crate::page::Page;

// ===== 页22: 设置 -> 系统版本显示 =====
pub(crate) struct SystemVersionPage;
pub(super) static PAGE_SYSVERP: SystemVersionPage = SystemVersionPage;

unsafe fn system_version_checks(rows: *mut u32) {
    let selected = crate::system_version::selected();
    for i in 0..4 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for SystemVersionPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统版本显示\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("3.101.043\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("4.103.022\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("91.78.13\0".as_bytes(), core::ptr::null())),
                (3, RowSpec::check("自定义\0".as_bytes(), crate::system_version::custom_display())),
            ])?;
            system_version_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 3 {
                crate::system_version::choose(idx);
                if page_is_live(crate::system_version::PAGE_PID as usize) {
                    system_version_checks(page_rows_ptr(crate::system_version::PAGE_PID as usize));
                }
            } else if idx == 3 {
                crate::system_version::edit_custom();
            }
        }
    }
}

// ===== 页24: 设置 -> 系统关于页面修改 =====
pub(crate) struct SystemAboutPage;
pub(super) static PAGE_ABOUT_MODP: SystemAboutPage = SystemAboutPage;

impl Page for SystemAboutPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "系统关于页面修改\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::clickable("系统版本显示\0".as_bytes())),
                (1, RowSpec::clickable("手环系统文本\0".as_bytes())),
                (2, RowSpec::clickable("设备名称\0".as_bytes())),
                (3, RowSpec::clickable("手环型号\0".as_bytes())),
            ]).map(|_| ())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            match idx {
                0 => nav_goto(crate::system_version::PAGE_PID, 0),
                1 => nav_goto(crate::system_text::PAGE_PID, 0),
                2 => nav_goto(crate::device_name::PAGE_PID, 0),
                3 => nav_goto(crate::device_model::PAGE_PID, 0),
                _ => {}
            }
        }
    }
}

// ===== 页25: 系统关于页面修改 -> 手环系统文本 =====
pub(crate) struct SystemTextPage;
pub(super) static PAGE_SYSTEXTP: SystemTextPage = SystemTextPage;

unsafe fn system_text_checks(rows: *mut u32) {
    let selected = crate::system_text::selected();
    for i in 0..3 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for SystemTextPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环系统文本\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("Xiaomi HyperOS\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("HarmonyOS\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("自定义\0".as_bytes(), crate::system_text::custom_display())),
            ])?;
            system_text_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 2 {
                crate::system_text::choose(idx);
                if page_is_live(crate::system_text::PAGE_PID as usize) {
                    system_text_checks(page_rows_ptr(crate::system_text::PAGE_PID as usize));
                }
            } else if idx == 2 {
                crate::system_text::edit_custom();
            }
        }
    }
}

// ===== 页26: 系统关于页面修改 -> 设备名称 =====
pub(crate) struct DeviceNamePage;
pub(super) static PAGE_DEVNAMEP: DeviceNamePage = DeviceNamePage;

unsafe fn device_name_checks(rows: *mut u32) {
    let selected = crate::device_name::selected();
    for i in 0..=crate::device_name::preset_count() {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for DeviceNamePage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "设备名称\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            // 文本和数量都以 device_name::PRESETS 为准，新增预设后这里自动建行。
            let mut previous = ctx.title;
            for i in 0..crate::device_name::preset_count() {
                let row = build_row(ctx, previous, i, &RowSpec::check(
                    crate::device_name::preset_text(i), core::ptr::null()));
                if row == 0 { return Err(PageError::RowFailed(i)); }
                previous = row;
            }
            let custom = crate::device_name::preset_count();
            let row = build_row(ctx, previous, custom, &RowSpec::check(
                "自定义\0".as_bytes(), crate::device_name::custom_display()));
            if row == 0 { return Err(PageError::RowFailed(custom)); }
            device_name_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            let custom = crate::device_name::preset_count();
            if idx < custom {
                crate::device_name::choose(idx);
                if page_is_live(crate::device_name::PAGE_PID as usize) {
                    device_name_checks(page_rows_ptr(crate::device_name::PAGE_PID as usize));
                }
            } else if idx == custom {
                crate::device_name::edit_custom();
            }
        }
    }
}

// ===== 页27: 系统关于页面修改 -> 手环型号 =====
pub(crate) struct DeviceModelPage;
pub(super) static PAGE_DEVMODELP: DeviceModelPage = DeviceModelPage;

unsafe fn device_model_checks(rows: *mut u32) {
    let selected = crate::device_model::selected();
    for i in 0..4 {
        let row = read_volatile(rows.add(i));
        if row != 0 {
            fw_api::row_update(
                row,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                fw_api::NONE_PTR,
                0,
                (i == selected) as u8,
            );
        }
    }
}

impl Page for DeviceModelPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe { put_str(buf, "手环型号\0".as_bytes()); }
    }

    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe {
            build_rows(ctx, ctx.title, &[
                (0, RowSpec::check("M2551B1\0".as_bytes(), core::ptr::null())),
                (1, RowSpec::check("M2553B1\0".as_bytes(), core::ptr::null())),
                (2, RowSpec::check("M9178B13\0".as_bytes(), core::ptr::null())),
                (3, RowSpec::check("自定义\0".as_bytes(), crate::device_model::custom_display())),
            ])?;
            device_model_checks(ctx.rows);
            Ok(())
        }
    }

    fn on_click(&self, idx: usize) {
        unsafe {
            if idx < 3 {
                crate::device_model::choose(idx);
                if page_is_live(crate::device_model::PAGE_PID as usize) {
                    device_model_checks(page_rows_ptr(crate::device_model::PAGE_PID as usize));
                }
            } else if idx == 3 {
                crate::device_model::edit_custom();
            }
        }
    }
}
