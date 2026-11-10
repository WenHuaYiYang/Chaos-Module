//! Screenshots/recordings: four actions, two settings pages and storage footers. Two independent
//! third-level file lists use immutable filenames for two-tap deletion.
use crate::page::*;
use crate::capture::{self, State};
use crate::capture_format::{self as format, Kind, Storage, VideoFormat};
use crate::capture_settings;

pub(crate) const CAPTURE_PID: u32 = 38;
const RECORDINGS_PID: usize = 39;
const SCREENSHOTS_PID: usize = 40;
const RECORDSET_PID: usize = 41;
const SHOTSET_PID: usize = 42;
const FILES_PER_PAGE: usize = 8;
pub(super) static PAGE_CAPTURE: CapturePage = CapturePage;
pub(super) static PAGE_RECORDINGS: FilesPage = FilesPage { kind: Kind::Recording };
pub(super) static PAGE_SCREENSHOTS: FilesPage = FilesPage { kind: Kind::Screenshot };
pub(super) static PAGE_RECORDSET: SettingsPage = SettingsPage { recording: true };
pub(super) static PAGE_SHOTSET: SettingsPage = SettingsPage { recording: false };
static mut LAST_REVISION: u32 = u32::MAX;
static mut LAST_FRAMES: u32 = u32::MAX;
static mut LAST_FPS: u32 = u32::MAX;
static mut STATUS: [u8; 96] = [0; 96];

fn error_text(code: i32) -> &'static [u8] {
    match code {
        -12 => "内存不足\0".as_bytes(),
        -28 => "储存空间不足\0".as_bytes(),
        -95 => "屏幕格式不支持\0".as_bytes(),
        -110 => "等待息屏超时\0".as_bytes(),
        _ => "操作失败，请重试\0".as_bytes(),
    }
}

pub(crate) struct CapturePage;
impl Page for CapturePage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, "截屏与录屏\0".as_bytes()); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        let mut w = W::new(core::ptr::addr_of_mut!(STATUS).cast(), 96);
        let settings = capture_settings::get();
        let active = capture::state() != State::Idle;
        let sub = if capture::error() == -110 { "未获取到画面\0".as_bytes().as_ptr() }
        else if capture::error() != 0 { error_text(capture::error()).as_ptr() }
        else if capture::state() == State::Stopping { "正在保存\0".as_bytes().as_ptr() }
        else if active {
            status(&mut w);
            core::ptr::addr_of!(STATUS).cast()
        } else if capture::limited() { "达到录屏上限，已保存\0".as_bytes().as_ptr() }
        else if capture::frames() != 0 {
            w.s("已保存 ".as_bytes()).n(capture::frames()).s(" 帧 · 采集 ".as_bytes()).n(capture::fps()).s(b" fps").end();
            core::ptr::addr_of!(STATUS).cast()
        }
        else if settings.format == VideoFormat::Gif { "GIF · 15 fps\0".as_bytes().as_ptr() }
        else { "APNG · 60 fps\0".as_bytes().as_ptr() };
        let shot_sub = if capture::shot_error() != 0 { error_text(capture::shot_error()).as_ptr() }
        else { match capture::shot_state() {
            1 => "等待息屏，点击取消\0".as_bytes().as_ptr(),
            2 => "正在截屏\0".as_bytes().as_ptr(),
            3 => "截屏已保存\0".as_bytes().as_ptr(),
            _ => "息屏后进行截图\0".as_bytes().as_ptr(),
        } };
        build_rows(ctx, ctx.title, &[
            (0, RowSpec::tap(if active { "结束录屏\0".as_bytes() } else { "开始录屏\0".as_bytes() }, sub)),
            (1, RowSpec::clickable("管理录屏\0".as_bytes())),
            (2, RowSpec::tap("遮盖截屏\0".as_bytes(), shot_sub)),
            (3, RowSpec::clickable("管理截屏\0".as_bytes())),
            (4, RowSpec::clickable("录屏设置\0".as_bytes())),
            (5, RowSpec::clickable("截图设置\0".as_bytes())),
            (6, RowSpec::display("录屏储存于\0".as_bytes(), settings.recording.root().as_ptr())),
            (7, RowSpec::display("截图储存于\0".as_bytes(), settings.screenshot.root().as_ptr())),
        ])?;
        LAST_REVISION = capture::revision(); LAST_FRAMES = capture::frames();
        LAST_FPS = capture::fps();
        Ok(())
    } }
    fn on_click(&self, idx: usize) { unsafe {
        match idx {
            0 if capture::state() != State::Stopping => { capture::toggle_recording(); render_req(CAPTURE_PID as usize); }
            1 => { PAGE_RECORDINGS.enter(); nav_goto(RECORDINGS_PID as u32, 0); }
            2 => { capture::toggle_shot(); render_req(CAPTURE_PID as usize); }
            3 => { PAGE_SCREENSHOTS.enter(); nav_goto(SCREENSHOTS_PID as u32, 0); }
            4 => nav_goto(RECORDSET_PID as u32, 0),
            5 => nav_goto(SHOTSET_PID as u32, 0),
            _ => {}
        }
    } }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) { unsafe {
        // Update rows in place: don't rebuild a scrolling list every frame.
        if LAST_REVISION != capture::revision() {
            render_req(CAPTURE_PID as usize);
        } else if (LAST_FRAMES != capture::frames() || LAST_FPS != capture::fps())
            && matches!(capture::state(), State::Starting | State::Recording) {
            LAST_FRAMES = capture::frames();
            LAST_FPS = capture::fps();
            let mut w = W::new(core::ptr::addr_of_mut!(STATUS).cast(), 96);
            status(&mut w);
            let row = read_volatile(page_rows_ptr(CAPTURE_PID as usize));
            if row != 0 { fw_api::row_update(row, core::ptr::null(), core::ptr::null(), core::ptr::addr_of!(STATUS).cast(), 0, 0); }
        }
    } }
}

unsafe fn status(w: &mut W) {
    w.s(capture::active_format().label()).s(b" / ").n(capture::frames()).s(" 帧 · 采集 ".as_bytes()).n(capture::fps()).s(b" fps");
    w.end();
}

static mut SETTINGS_RESULT: [i32; 2] = [0; 2];
pub(crate) struct SettingsPage { recording: bool }
impl SettingsPage {
    fn pid(&self) -> usize { if self.recording { RECORDSET_PID } else { SHOTSET_PID } }
    fn slot(&self) -> usize { if self.recording { 0 } else { 1 } }
}
impl Page for SettingsPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, if self.recording { "录屏设置\0".as_bytes() } else { "截图设置\0".as_bytes() }); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        let setting = capture_settings::get(); let mut anchor = ctx.title;
        let path_slot = if self.recording { 2 } else { 0 };
        let location = if self.recording { setting.recording } else { setting.screenshot };
        let texts: [&[u8]; 4] = ["GIF · 15 fps\0".as_bytes(), "APNG · 60 fps\0".as_bytes(), b"/data/chaos/media\0", b"/data/offlinelog\0"];
        let count = if self.recording { 4 } else { 2 };
        for i in 0..count {
            let text = texts[if self.recording { i } else { i + 2 }];
            let sub = if self.recording && i < 2 { "录屏格式\0".as_bytes() } else { "储存路径\0".as_bytes() };
            let spec = RowSpec { text, sub: sub.as_ptr(), trailing: fw_api::trailing::RADIO, clickable: true };
            let row = build_row(ctx, anchor, i, &spec);
            if row == 0 { return Err(PageError::RowFailed(i)); }
            // Bind the radio itself here, leaving existing pages' row events unchanged.
            let radio = fw_api::row_trailing(row);
            if radio != 0 { fw_api::obj_add_event(radio, row_ev_fn(i), 7, 0); }
            let selected = if self.recording && i < 2 { i == setting.format as usize }
                else { (i == path_slot && location == Storage::Media) || (i == path_slot + 1 && location == Storage::OfflineLog) };
            fw_api::row_update(row, core::ptr::null(), core::ptr::null(), core::ptr::null(), 0, selected as u8);
            anchor = row;
        }
        if SETTINGS_RESULT[self.slot()] < 0 {
            build_row(ctx, anchor, count, &RowSpec::display("设置保存失败，请重试\0".as_bytes(), core::ptr::null()));
        } else if capture::state() != State::Idle && self.recording {
            build_row(ctx, anchor, count, &RowSpec::display("修改将在下次录屏生效\0".as_bytes(), core::ptr::null()));
        }
        Ok(())
    } }
    fn on_click(&self, i: usize) { unsafe {
        let mut setting = capture_settings::get();
        match (self.recording, i) {
            (true, 0) => setting.format = VideoFormat::Gif,
            (true, 1) => setting.format = VideoFormat::Apng,
            (true, 2) => setting.recording = Storage::Media,
            (true, 3) => setting.recording = Storage::OfflineLog,
            (false, 0) => setting.screenshot = Storage::Media,
            (false, 1) => setting.screenshot = Storage::OfflineLog,
            _ => return,
        }
        SETTINGS_RESULT[self.slot()] = capture_settings::set(setting).err().unwrap_or(0);
        capture::settings_changed(); render_req(self.pid());
    } }
}

#[derive(Clone, Copy)]
struct Entry { name: [u8; format::NAME_BYTES], bytes: u32, storage: Storage }
impl Entry {
    const EMPTY: Self = Self { name: [0; format::NAME_BYTES], bytes: 0, storage: Storage::OfflineLog };
    fn same_file(&self, other: &Self) -> bool { self.name == other.name && self.storage == other.storage }
}
struct Listing {
    entries: [Entry; FILES_PER_PAGE], count: usize, offset: usize, more: bool,
    armed: Entry, delete: bool, result: i32, revision: u32,
    subtitles: [[u8; 64]; FILES_PER_PAGE],
}
impl Listing {
    const fn new() -> Self { Self {
        entries: [Entry::EMPTY; FILES_PER_PAGE], count: 0, offset: 0, more: false,
        armed: Entry::EMPTY, delete: false, result: 0, revision: 0,
        subtitles: [[0; 64]; FILES_PER_PAGE],
    } }
}
static mut RECORDINGS: Listing = Listing::new();
static mut SCREENSHOTS: Listing = Listing::new();
#[cfg(test)]
pub(super) unsafe fn reset_for_test() {
    RECORDINGS = Listing::new(); SCREENSHOTS = Listing::new(); SETTINGS_RESULT = [0; 2];
    LAST_REVISION = u32::MAX; LAST_FRAMES = u32::MAX;
    LAST_FPS = u32::MAX;
}

pub(crate) struct FilesPage { kind: Kind }
impl FilesPage {
    fn pid(&self) -> usize { if self.kind == Kind::Recording { RECORDINGS_PID } else { SCREENSHOTS_PID } }
    unsafe fn listing(&self) -> &'static mut Listing {
        if self.kind == Kind::Recording { &mut *core::ptr::addr_of_mut!(RECORDINGS) }
        else { &mut *core::ptr::addr_of_mut!(SCREENSHOTS) }
    }
    unsafe fn enter(&self) {
        let list = self.listing();
        if !list.delete { list.armed = Entry::EMPTY; }
        list.offset = 0; list.result = 0;
    }
    unsafe fn scan(&self) {
        let list = self.listing();
        list.count = 0; list.more = false; list.entries.fill(Entry::EMPTY);
        list.revision = capture::revision();
        let mut matched = 0;
        for storage in Storage::ALL {
        let dir = fw_api::opendir(storage.root().as_ptr());
        if dir == 0 {
            let e = fw_api::capture_result(-1);
            if e != -2 { list.result = e; }
            continue;
        }
        loop {
            let entry = fw_api::readdir(dir);
            if entry == 0 { break; }
            if read_volatile(entry as *const u8) != DT_REG { continue; }
            let name = (entry as usize + 1) as *const u8;
            let mut item = Entry::EMPTY;
            item.storage = storage;
            let mut len = 0;
            while len < format::NAME_BYTES - 1 {
                let b = read_volatile(name.add(len));
                if b == 0 { break; }
                item.name[len] = b; len += 1;
            }
            if format::file_kind(&item.name[..len]) != Some(self.kind) { continue; }
            if item.name[..len].ends_with(b".part") &&
                ((self.kind == Kind::Recording && capture::state() != State::Idle)
                 || (self.kind == Kind::Screenshot && capture::shot_state() == 2)) { continue; }
            if matched < list.offset { matched += 1; continue; }
            if list.count == FILES_PER_PAGE { list.more = true; break; }
            let mut path = [0; 96];
            if format::path_at(&item.name, false, storage, &mut path).is_err() { continue; }
            let fd = fw_api::open(path.as_ptr(), fw_api::oflag::RDONLY, 0);
            if fd >= 0 {
                let size = fw_api::lseek(fd, 0, fw_api::seek::END);
                if size >= 0 { item.bytes = size.min(u32::MAX as i64) as u32; }
                fw_api::close(fd);
            }
            list.entries[list.count] = item; list.count += 1;
        }
        fw_api::closedir(dir);
        if list.more { break; }
        }
        // External removal can invalidate many pages at once. Fall back to
        // page one in a single retry, never recurse once per missing page.
        if list.count == 0 && list.offset != 0 { list.offset = 0; self.scan(); }
    }
}
impl Page for FilesPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, if self.kind == Kind::Recording {
        "管理录屏\0".as_bytes()
    } else { "管理截屏\0".as_bytes() }); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        self.scan();
        let list = self.listing();
        let mut anchor = ctx.title;
        if list.count == 0 { anchor = build_row(ctx, anchor, 0, &RowSpec::display(
            if list.result < 0 { error_text(list.result) } else { "暂无文件\0".as_bytes() }, core::ptr::null())); }
        for i in 0..list.count {
            let item = &list.entries[i];
            let mut w = W::new(list.subtitles[i].as_mut_ptr(), 64);
            w.sz(item.bytes).s(if item.storage == Storage::Media { " · 媒体".as_bytes() } else { " · 日志".as_bytes() })
                .s(if list.armed.same_file(item) { " · 再次点击删除".as_bytes() } else { " · 点击删除".as_bytes() }).end();
            let prefix = if self.kind == Kind::Recording { 10 } else { 11 };
            let row = build_row(ctx, anchor, i, &RowSpec::tap(&item.name[prefix..], list.subtitles[i].as_ptr()));
            if row == 0 { return Err(PageError::RowFailed(i)); } anchor = row;
        }
        if list.offset != 0 { anchor = build_row(ctx, anchor, 8, &RowSpec::clickable("上一页\0".as_bytes())); }
        if list.more { anchor = build_row(ctx, anchor, 9, &RowSpec::clickable("下一页\0".as_bytes())); }
        anchor = build_row(ctx, anchor, 10, &RowSpec::clickable("刷新\0".as_bytes()));
        if list.result != 0 { build_row(ctx, anchor, 11, &RowSpec::display(
            if list.result > 0 { "已删除\0".as_bytes() } else { error_text(list.result) }, core::ptr::null())); }
        Ok(())
    } }
    fn on_click(&self, idx: usize) { unsafe {
        let list = self.listing();
        if list.delete { return; }
        list.result = 0;
        match idx {
            8 if list.offset != 0 => { list.offset = list.offset.saturating_sub(FILES_PER_PAGE); list.armed = Entry::EMPTY; }
            9 if list.more => { list.offset += FILES_PER_PAGE; list.armed = Entry::EMPTY; }
            10 => { list.armed = Entry::EMPTY; }
            i if i < list.count => {
                let item = list.entries[i];
                if list.armed.same_file(&item) { list.delete = true; }
                else { list.armed = item; }
            }
            _ => return,
        }
        render_req(self.pid());
    } }
    fn needs_tick(&self) -> bool { true }
    fn tick(&self) { unsafe {
        let list = self.listing();
        if list.delete {
            let mut path = [0; 96];
            let len = list.armed.name.iter().position(|&b| b == 0).unwrap_or(list.armed.name.len());
            let busy = list.armed.name[..len].ends_with(b".part") &&
                ((self.kind == Kind::Recording && capture::state() != State::Idle)
                 || (self.kind == Kind::Screenshot && capture::shot_state() == 2));
            if busy { list.delete = false; list.armed = Entry::EMPTY; list.result = -16; render_req(self.pid()); return; }
            list.result = match format::path_at(&list.armed.name, false, list.armed.storage, &mut path) {
                Ok(()) => {
                    let rc = fw_api::capture_result(fw_api::fs_remove(path.as_ptr()));
                    if rc >= 0 { 1 } else { rc }
                }
                Err(e) => e,
            };
            list.delete = false; list.armed = Entry::EMPTY; render_req(self.pid());
        } else if list.revision != capture::revision() { render_req(self.pid()); }
    } }
}
