//! 马达音乐页: 内置示例 + 6 个文件槽 (/data/chaos/music/buzzN.buzz)。
//! 点击即播放/停止; 播放期间维护节拍走 50ms 快拍(buzz_music::busy)。

use crate::buzz_music;
use crate::page::kit::{MenuSpec, RowKind, RowModel, RowSub};
use crate::page::*;

pub(crate) struct BuzzPage;
pub(crate) static PAGE: BuzzPage = BuzzPage;

static mut SLOT_TEXT: [[u8; 24]; buzz_music::SLOTS] = [[0; 24]; buzz_music::SLOTS];

unsafe fn play(builtin: usize) {
    buzz_music::play(builtin);
    refresh();
}
unsafe fn play_slot(slot: usize) {
    buzz_music::play(buzz_music::BUILTIN + slot);
    refresh();
}
unsafe fn stop() {
    buzz_music::stop();
    refresh();
}

/// 状态行原地刷新(有副标题的行必须重传文本, 否则固件会洗掉副标题)。
unsafe fn refresh() {
    let pid = st_rd!(FG_PAGE) as usize;
    if pid > MAX_PID {
        return;
    }
    let r = read_volatile(page_rows_ptr(pid).add(8));
    if r != 0 {
        fw_api::row_update(
            r,
            fw_api::NONE_PTR,
            "状态\0".as_bytes().as_ptr(),
            buzz_music::status(),
            0,
            0,
        );
    }
}

unsafe fn slot_text(i: usize) -> *const u8 {
    let buf = core::ptr::addr_of_mut!(SLOT_TEXT[i]) as *mut u8;
    let mut w = W::new(buf, 24);
    if buzz_music::slot_present(i + 1) {
        w.s("点击播放".as_bytes());
    } else {
        w.s("未上传".as_bytes());
    }
    w.end();
    buf
}
unsafe fn slot0() -> *const u8 { slot_text(0) }
unsafe fn slot1() -> *const u8 { slot_text(1) }
unsafe fn slot2() -> *const u8 { slot_text(2) }
unsafe fn slot3() -> *const u8 { slot_text(3) }
unsafe fn slot4() -> *const u8 { slot_text(4) }
unsafe fn slot5() -> *const u8 { slot_text(5) }

unsafe fn play0() { play(0) }
unsafe fn play1() { play(1) }
unsafe fn play_s0() { play_slot(0) }
unsafe fn play_s1() { play_slot(1) }
unsafe fn play_s2() { play_slot(2) }
unsafe fn play_s3() { play_slot(3) }
unsafe fn play_s4() { play_slot(4) }
unsafe fn play_s5() { play_slot(5) }

static ROWS: &[RowModel] = &[
    RowModel::action("示例·鼓点 120BPM\0".as_bytes(), play0, 0, None),
    RowModel::action("示例·音阶（听 type 差异）\0".as_bytes(), play1, 0, None),
    RowModel { label: "文件 1\0".as_bytes(), sub: RowSub::Dyn(slot0), kind: RowKind::Fn(play_s0), trailing: 0, refresh: None },
    RowModel { label: "文件 2\0".as_bytes(), sub: RowSub::Dyn(slot1), kind: RowKind::Fn(play_s1), trailing: 0, refresh: None },
    RowModel { label: "文件 3\0".as_bytes(), sub: RowSub::Dyn(slot2), kind: RowKind::Fn(play_s2), trailing: 0, refresh: None },
    RowModel { label: "文件 4\0".as_bytes(), sub: RowSub::Dyn(slot3), kind: RowKind::Fn(play_s3), trailing: 0, refresh: None },
    RowModel { label: "文件 5\0".as_bytes(), sub: RowSub::Dyn(slot4), kind: RowKind::Fn(play_s4), trailing: 0, refresh: None },
    RowModel { label: "文件 6\0".as_bytes(), sub: RowSub::Dyn(slot5), kind: RowKind::Fn(play_s5), trailing: 0, refresh: None },
    RowModel { label: "状态\0".as_bytes(), sub: RowSub::Dyn(buzz_music::status), kind: RowKind::Display, trailing: 0, refresh: None },
    RowModel::action("停止\0".as_bytes(), stop, 0, None),
    RowModel::note("马达只有预设效果, 没有音高: 用节奏和不同 type 表达鼓点与强弱。手机同步器: 浏览器打开「网页服务器」页显示的地址并在末尾加 /buzz（例如 http://192.168.42.1/buzz），选歌即可分析并上传到文件 1-6。示例·音阶 会依次触发 type 1-15, 用来听哪些效果适合做鼓点/旋律。\0".as_bytes()),
];

impl MenuSpec for BuzzPage {
    fn rows(&self) -> &'static [RowModel] {
        ROWS
    }
}

impl Page for BuzzPage {
    fn fill_title(&self, buf: *mut u8) {
        unsafe {
            put_str(buf, "马达音乐\0".as_bytes());
        }
    }
    fn render(&self, ctx: &PageCtx) -> PageResult {
        unsafe { frame::build(self, ctx, ctx.title) }
    }
    fn is_switch_row(&self, _idx: usize) -> bool {
        false
    }
    fn on_click(&self, idx: usize) {
        unsafe { kit::menu_click(self, idx) }
    }
    fn needs_tick(&self) -> bool {
        unsafe { buzz_music::busy() }
    }
    fn tick(&self) {
        unsafe {
            if !buzz_music::busy() {
                refresh();
            }
        }
    }
}
