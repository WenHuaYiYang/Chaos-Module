//! Third-level icon picker: home -> settings -> application icon.
use crate::page::*;
use crate::dynamic_icon;

pub(crate) const APP_ICON_PID: u32 = 43;
pub(crate) struct AppIconPage;
pub(crate) static PAGE_APP_ICON: AppIconPage = AppIconPage;

impl Page for AppIconPage {
    fn fill_title(&self, buffer: *mut u8) {
        unsafe { put_str(buffer, "应用图标\0".as_bytes()); }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        let selected = dynamic_icon::selected();
        let mut anchor = ctx.title;
        for (index, text) in dynamic_icon::NAMES.iter().enumerate() {
            let spec = RowSpec { text, sub: fw_api::NONE_PTR,
                trailing: fw_api::trailing::RADIO, clickable: true };
            let row = build_row(ctx, anchor, index, &spec);
            if row == 0 { return Err(PageError::RowFailed(index)); }
            let radio = fw_api::row_trailing(row);
            if radio != 0 { fw_api::obj_add_event(radio, row_ev_fn(index), 7, 0); }
            fw_api::row_update(row, fw_api::NONE_PTR, fw_api::NONE_PTR,
                fw_api::NONE_PTR, 0, (index == selected as usize) as u8);
            anchor = row;
        }
        Ok(())
    } }
    fn on_click(&self, index: usize) { unsafe {
        if index >= dynamic_icon::NAMES.len() { return; }
        if dynamic_icon::select(index as u8) {
            let rows = page_rows_ptr(APP_ICON_PID as usize);
            for slot in 0..dynamic_icon::NAMES.len() {
                let row = read_volatile(rows.add(slot));
                if row != 0 { fw_api::row_update(row, fw_api::NONE_PTR, fw_api::NONE_PTR,
                    fw_api::NONE_PTR, 0, (slot == index) as u8); }
            }
        }
    } }
}
