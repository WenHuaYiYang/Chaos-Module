//! Screenshots/recordings: four actions, two settings pages and storage footers. Two independent
//! third-level file lists use immutable filenames for two-tap deletion.
use crate::page::*;
use crate::page::kit::checklist::{click as ck_click, render as ck_render, CheckSpec};
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
pub(super) static PAGE_PREVIEW: PreviewPage = PreviewPage;
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
            0 if capture::state() != State::Stopping => { capture::toggle_recording(); render_req(CAPTURE_PID); }
            1 => { PAGE_RECORDINGS.enter(); nav_goto(RECORDINGS_PID as u32, 0); }
            2 => { capture::toggle_shot(); render_req(CAPTURE_PID); }
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
            render_req(CAPTURE_PID);
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

// 录屏/截图设置页的选项 spec(单选 RADIO): 录屏 = 格式两项 + 存储两项,
// 截图 = 存储两项。点选后走整页重建(底部的条件提示行会随结果增减)。
struct CapSetCheck { recording: bool }
impl CheckSpec for CapSetCheck {
    fn count(&self) -> usize { if self.recording { 4 } else { 2 } }
    unsafe fn label(&self, i: usize) -> *const u8 {
        let texts: [&[u8]; 4] = ["GIF · 15 fps\0".as_bytes(), "APNG · 60 fps\0".as_bytes(), b"/data/chaos/media\0", b"/data/offlinelog\0"];
        texts[if self.recording { i } else { i + 2 }].as_ptr()
    }
    unsafe fn sub(&self, i: usize) -> *const u8 {
        let sub: &[u8] = if self.recording && i < 2 { "录屏格式\0".as_bytes() } else { "储存路径\0".as_bytes() };
        sub.as_ptr()
    }
    fn trailing(&self) -> u32 { fw_api::trailing::RADIO }
    unsafe fn selected(&self, i: usize) -> bool {
        let setting = capture_settings::get();
        if self.recording {
            if i < 2 { i == setting.format as usize } else {
                (i == 2 && setting.recording == Storage::Media) || (i == 3 && setting.recording == Storage::OfflineLog)
            }
        } else {
            (i == 0 && setting.screenshot == Storage::Media) || (i == 1 && setting.screenshot == Storage::OfflineLog)
        }
    }
    unsafe fn choose(&self, i: usize) {
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
        let slot = if self.recording { 0 } else { 1 };
        let pid = if self.recording { RECORDSET_PID } else { SHOTSET_PID };
        SETTINGS_RESULT[slot] = capture_settings::set(setting).err().unwrap_or(0);
        capture::settings_changed();
        render_req(pid as u32);
    }
}

impl Page for SettingsPage {
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, if self.recording { "录屏设置\0".as_bytes() } else { "截图设置\0".as_bytes() }); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe {
        let spec = CapSetCheck { recording: self.recording };
        let mut anchor = ck_render(&spec, ctx)?;
        let mut footer = spec.count();
        if !self.recording {
            anchor = build_row(ctx, anchor, 2, &RowSpec::switch("转换为PNG\0".as_bytes()));
            if anchor == 0 { return Err(PageError::RowFailed(2)); }
            fw_api::row_update(anchor, core::ptr::null(), core::ptr::null(), core::ptr::null(), 0,
                capture_settings::get().screenshot_png as u8);
            footer += 1;
        }
        // 条件提示行(失败/录屏中修改的说明), 跟在选项列表后
        if SETTINGS_RESULT[self.slot()] < 0 {
            build_row(ctx, anchor, footer, &RowSpec::display("设置保存失败，请重试\0".as_bytes(), core::ptr::null()));
        } else if capture::state() != State::Idle && self.recording {
            build_row(ctx, anchor, footer, &RowSpec::display("修改将在下次录屏生效\0".as_bytes(), core::ptr::null()));
        }
        Ok(())
    } }
    fn is_switch_row(&self, i: usize) -> bool { !self.recording && i == 2 }
    fn on_switch(&self, i: usize) { if self.is_switch_row(i) { self.on_click(i); } }
    fn on_click(&self, i: usize) { unsafe {
        if self.is_switch_row(i) {
            let mut setting = capture_settings::get();
            setting.screenshot_png = !setting.screenshot_png;
            SETTINGS_RESULT[self.slot()] = capture_settings::set(setting).err().unwrap_or(0);
            capture::settings_changed(); render_req(self.pid() as u32); return;
        }
        let spec = CapSetCheck { recording: self.recording };
        ck_click(&spec, self.pid() as u32, i);
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
    PREVIEW_ENTRY=Entry::EMPTY; PREVIEW_TARGET=Entry::EMPTY; PREVIEW_DELETE=false; PREVIEW_RESULT=0;
    crate::confirm_pop::forget(crate::confirm_pop::SLOT_SHOT);
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
                .s(if self.kind == Kind::Screenshot { " · 点击预览".as_bytes() }
                    else if list.armed.same_file(item) { " · 再次点击删除".as_bytes() } else { " · 点击删除".as_bytes() }).end();
            let row = build_row(ctx, anchor, i, &RowSpec::tap(&item.name, list.subtitles[i].as_ptr()));
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
                if self.kind == Kind::Screenshot {
                    PREVIEW_ENTRY = item; PREVIEW_RESULT = 0; PREVIEW_DELETE = false;
                    nav_goto(Pid::ShotPreview as u32,0); return;
                }
                if list.armed.same_file(&item) { list.delete = true; }
                else { list.armed = item; }
            }
            _ => return,
        }
        render_req(self.pid() as u32);
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
            if busy { list.delete = false; list.armed = Entry::EMPTY; list.result = -16; render_req(self.pid() as u32); return; }
            list.result = match format::path_at(&list.armed.name, false, list.armed.storage, &mut path) {
                Ok(()) => {
                    let rc = fw_api::capture_result(fw_api::fs_remove(path.as_ptr()));
                    if rc >= 0 { 1 } else { rc }
                }
                Err(e) => e,
            };
            list.delete = false; list.armed = Entry::EMPTY; render_req(self.pid() as u32);
        } else if list.revision != capture::revision() { render_req(self.pid() as u32); }
    } }
}


/// The fixed container participates in scrolling; the GPU image remains its child.
/// Clicking the container reaches the same row callback as the filename.
unsafe fn picture(ctx:&PageCtx,prev:u32,cache:usize,row:u32,path:[u8;96],click:Option<usize>,caption:[u8;64])->Result<u32,PageError> {
    let full=cache==8; let(w,h)=if full{(280,400)}else{(168,240)};
    let box_=fw_api::obj_create(ctx.content,0);
    if box_==0{return Err(PageError::RowFailed(cache));}
    fw_api::obj_clear_box_spacing(box_);
    fw_api::obj_set_size_wh(box_,w,h);
    fw_api::background_set_opa(box_,0);
    fw_api::obj_clear_flag(box_,fw_api::LV_OBJ_FLAG_SCROLLABLE|fw_api::LV_OBJ_FLAG_SCROLL_CHAIN);
    fw_api::obj_align_to(box_,prev,fw_api::align::OUT_BOTTOM_MID,0,8);
    if let Some(slot)=click {
        fw_api::obj_add_flag(box_,fw_api::LV_OBJ_FLAG_CLICKABLE);
        fw_api::obj_add_event(box_,row_cb(),7,slot as u32);
    } else {fw_api::obj_clear_flag(box_,fw_api::LV_OBJ_FLAG_CLICKABLE);}
    let image=fw_api::background_image_create(box_);
    if image==0{return Err(PageError::RowFailed(cache));}
    fw_api::obj_clear_flag(image,fw_api::LV_OBJ_FLAG_CLICKABLE|fw_api::LV_OBJ_FLAG_SCROLLABLE);
    fw_api::obj_align(image,fw_api::align::TOP_LEFT,0,0);
    if !crate::capture_preview::attach(cache,image,row,if full{Pid::ShotPreview as u32}else{SCREENSHOTS_PID as u32},path,if full{256}else{512},caption) {
        fw_api::row_update(row,core::ptr::null(),core::ptr::null(),"预览暂不可用，请刷新\0".as_ptr(),0,0);
    }
    Ok(box_)
}

pub(crate) struct PreviewPage;
static mut PREVIEW_ENTRY:Entry=Entry::EMPTY;
static mut PREVIEW_TARGET:Entry=Entry::EMPTY;
static mut PREVIEW_DELETE:bool=false;
static mut PREVIEW_RESULT:i32=0;
unsafe extern "C" fn preview_no(event:u32) {
    if crate::confirm_pop::click(crate::confirm_pop::SLOT_SHOT,event)!=0 {
        render_req(Pid::ShotPreview as u32);
    }
}
unsafe extern "C" fn preview_yes(event:u32) {
    if crate::confirm_pop::click(crate::confirm_pop::SLOT_SHOT,event)!=0 {PREVIEW_DELETE=true;}
}
impl Page for PreviewPage {
    fn fill_title(&self,buf:*mut u8){unsafe{put_str(buf,"截图预览\0".as_bytes());}}
    fn on_instance_reset(&self){unsafe{crate::confirm_pop::forget(crate::confirm_pop::SLOT_SHOT);}}
    fn on_pause(&self){unsafe{PREVIEW_DELETE=false;}}
    fn render(&self,ctx:&PageCtx)->PageResult{unsafe{
        crate::confirm_pop::reset(crate::confirm_pop::SLOT_SHOT);
        let item=PREVIEW_ENTRY;
        let mut path=[0;96];
        if item.name[0]==0||format::path_at(&item.name,false,item.storage,&mut path).is_err(){
            build_row(ctx,ctx.title,0,&RowSpec::display("未选择截图\0".as_bytes(),core::ptr::null()));return Ok(());
        }
        let info=build_row(ctx,ctx.title,0,&RowSpec::display(&item.name,"加载预览中\0".as_ptr()));
        if info==0{return Err(PageError::RowFailed(0));}
        let image=picture(ctx,info,8,info,path,None,[0;64])?;
        let delete=build_row(ctx,image,1,&RowSpec::clickable("删除截图\0".as_bytes()));
        if delete==0{return Err(PageError::RowFailed(1));}
        if PREVIEW_RESULT<0 {build_row(ctx,delete,2,&RowSpec::display(error_text(PREVIEW_RESULT),core::ptr::null()));}
        Ok(())
    }}
    fn on_click(&self,i:usize){unsafe{
        if i!=1||crate::confirm_pop::shown(crate::confirm_pop::SLOT_SHOT)||PREVIEW_ENTRY.name[0]==0{return;}
        let rc=crate::confirm_pop::build(crate::confirm_pop::SLOT_SHOT,0,core::ptr::null(),0,
            preview_no as *const()as u32,preview_yes as *const()as u32);
        if rc==0{PREVIEW_TARGET=PREVIEW_ENTRY;}
        else {PREVIEW_RESULT=-5;render_req(Pid::ShotPreview as u32);}
    }}
    fn needs_tick(&self)->bool{unsafe{PREVIEW_DELETE||crate::confirm_pop::shown(crate::confirm_pop::SLOT_SHOT)}}
    fn tick(&self){unsafe{
        if crate::confirm_pop::close_deferred(crate::confirm_pop::SLOT_SHOT){render_req(Pid::ShotPreview as u32);}
        if !PREVIEW_DELETE{return;}
        PREVIEW_DELETE=false;
        let item=PREVIEW_TARGET;
        let n=item.name.iter().position(|&b|b==0).unwrap_or(item.name.len());
        if item.name[..n].ends_with(b".part")&&capture::shot_state()==2 {
            PREVIEW_RESULT=-16;render_req(Pid::ShotPreview as u32);return;
        }
        let mut path=[0;96];
        PREVIEW_RESULT=match format::path_at(&item.name,false,item.storage,&mut path) {
            Ok(())=>fw_api::capture_result(fw_api::fs_remove(path.as_ptr())),Err(e)=>e,
        };
        if PREVIEW_RESULT>=0 {
            SCREENSHOTS.result=1;PREVIEW_ENTRY=Entry::EMPTY;
            render_req(SCREENSHOTS_PID as u32);nav_back();
        } else {render_req(Pid::ShotPreview as u32);}
    }}
}

#[cfg(test)]
pub(super) unsafe fn confirm_preview_for_test(yes:bool) {
    if yes {preview_yes(7);} else {preview_no(7);}
}
