//! 马达音乐: 把旋律编译成振动脉冲时间轴, 按拍触发 `vibrate(type)`。
//!
//! 格式 `.buzz` (小端):
//!   +0  magic "BZ01"
//!   +4  count  u16   事件数
//!   +6  bpm    u16   仅展示
//!   +8  events[count] 每 3 字节: [delta_10ms u8][type u8][保留 u8]
//!       delta 相对上一事件(首事件相对 0), 单位 10ms, 最大 2550ms。
//!
//! 振动马达只有预设效果(type), 没有频率/幅度控制, 所以"放歌"实际是
//! 用不同 type 与节奏表达鼓点/强弱。播放靠维护节拍驱动: 播放期间 busy()
//! 为真, 整机走 50ms 快拍, 事件精度 10ms 粒度、实际按 50ms 对齐。

use crate::fw_api;

const DIR: &[u8] = b"/data/chaos/music/\0";
/// 文件槽位数: /data/chaos/music/buzz1.buzz .. buzz6.buzz
pub(crate) const SLOTS: usize = 6;
/// 内置示例两条。
pub(crate) const BUILTIN: usize = 2;
pub(crate) const ITEMS: usize = BUILTIN + SLOTS;

const MAGIC: [u8; 4] = *b"BZ01";
const MAX_EVENTS: usize = 2048;
const BUF_LEN: usize = 8 + MAX_EVENTS * 3;

static mut BUF: [u8; BUF_LEN] = [0; BUF_LEN];
static mut EVENTS: usize = 0;
static mut BPM: u16 = 120;

static mut PLAYING: u8 = 0; // 1 = 正在播
static mut SOURCE: u8 = 0; // 0 = 内置, 1..=SLOTS = 文件槽
static mut CURSOR: usize = 0;
static mut DUE: u32 = 0;
static mut LABEL: [u8; 24] = [0; 24];

/// 内置示例 1: 四四拍鼓点(120BPM, 每 8 分音符一下, 重拍用 tipo 0xD, 弱拍 0x1)。
const DEMO_BEAT: &[u8] = &[
    0, 0x0D, 0, // 0ms
    25, 0x01, 0, // 250ms
    25, 0x0D, 0, // 500ms
    25, 0x01, 0, // 750ms
    25, 0x0D, 0, // 1000
    25, 0x01, 0, // 1250
    25, 0x0D, 0, // 1500
    25, 0x01, 0, // 1750
    25, 0x0D, 0, // 2000
    25, 0x01, 0, // 2250
    25, 0x0D, 0, // 2500
    25, 0x01, 0, // 2750
    25, 0x0D, 0, // 3000
    25, 0x01, 0, // 3250
    25, 0x0D, 0, // 3500
    25, 0x01, 0, // 3750
];

/// 内置示例 2: 简单上行音阶(每 250ms, type 递增), 用来听不同 type 的差异。
const DEMO_SCALE: &[u8] = &[
    0, 0x01, 0, 25, 0x02, 0, 25, 0x03, 0, 25, 0x04, 0, 25, 0x05, 0, 25, 0x06, 0, 25, 0x07, 0, 25,
    0x08, 0, 25, 0x09, 0, 25, 0x0A, 0, 25, 0x0B, 0, 25, 0x0C, 0, 25, 0x0D, 0, 25, 0x0E, 0, 25, 0x0F,
    0,
];

fn slot_path(slot: usize, out: &mut [u8; 40]) -> *const u8 {
    // /data/chaos/music/buzzN.buzz
    let mut n = 0usize;
    for &b in DIR {
        if b == 0 {
            break;
        }
        out[n] = b;
        n += 1;
    }
    for &b in b"buzz" {
        out[n] = b;
        n += 1;
    }
    out[n] = b'0' + slot as u8;
    n += 1;
    for &b in b".buzz" {
        out[n] = b;
        n += 1;
    }
    out[n] = 0;
    out.as_ptr()
}

/// 读文件到 BUF, 校验头部, 返回事件数。槽位不存在返回 None。
unsafe fn load_slot(slot: usize) -> Option<usize> {
    let mut path = [0u8; 40];
    let p = slot_path(slot, &mut path);
    let fd = fw_api::open(p, fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return None;
    }
    let buf = core::ptr::addr_of_mut!(BUF) as *mut u8;
    let mut n = 0usize;
    while n < BUF_LEN {
        let got = fw_api::read(fd, buf.add(n), (BUF_LEN - n) as u32);
        if got <= 0 {
            break;
        }
        n += got as usize;
    }
    fw_api::close(fd);
    if n < 8 {
        return None;
    }
    for i in 0..4 {
        if core::ptr::read_volatile(buf.add(i)) != MAGIC[i] {
            return None;
        }
    }
    let count = core::ptr::read_volatile(buf.add(4)) as usize
        | (core::ptr::read_volatile(buf.add(5)) as usize) << 8;
    BPM = core::ptr::read_volatile(buf.add(6)) as u16
        | (core::ptr::read_volatile(buf.add(7)) as u16) << 8;
    let count = count.min(MAX_EVENTS);
    if n < 8 + count * 3 {
        return None;
    }
    Some(count)
}

unsafe fn load_builtin(which: usize) -> usize {
    let src = if which == 0 { DEMO_BEAT } else { DEMO_SCALE };
    let buf = core::ptr::addr_of_mut!(BUF) as *mut u8;
    let n = src.len().min(BUF_LEN);
    for i in 0..n {
        core::ptr::write_volatile(buf.add(i), src[i]);
    }
    BPM = 120;
    n / 3
}

/// 槽位是否有可播放内容(供页面显示)。
pub(crate) unsafe fn slot_present(slot: usize) -> bool {
    let mut path = [0u8; 40];
    let p = slot_path(slot, &mut path);
    let fd = fw_api::open(p, fw_api::oflag::RDONLY, 0);
    if fd < 0 {
        return false;
    }
    fw_api::close(fd);
    true
}

/// 播放第 which 槽(0..BUILTIN 为内置, 其后为文件槽)。
pub(crate) unsafe fn play(which: usize) -> bool {
    if which >= ITEMS {
        return false;
    }
    stop();
    let count = if which < BUILTIN {
        load_builtin(which)
    } else {
        match load_slot(which - BUILTIN + 1) {
            Some(n) => n,
            None => return false,
        }
    };
    if count == 0 {
        return false;
    }
    EVENTS = count;
    SOURCE = which as u8 + 1;
    CURSOR = 0;
    DUE = fw_api::capture_now_ms();
    PLAYING = 1;
    let label = if which < BUILTIN {
        if which == 0 {
            "鼓点示例\0".as_bytes()
        } else {
            "音阶示例\0".as_bytes()
        }
    } else {
        "文件\0".as_bytes()
    };
    let n = label.len().min(LABEL.len() - 1);
    for i in 0..n {
        LABEL[i] = label[i];
    }
    LABEL[n] = 0;
    true
}

pub(crate) unsafe fn stop() {
    PLAYING = 0;
    CURSOR = 0;
    EVENTS = 0;
}

pub(crate) unsafe fn busy() -> bool {
    PLAYING != 0
}

/// 状态副标签(播放中/已停止)。
pub(crate) unsafe fn status() -> *const u8 {
    if PLAYING != 0 {
        core::ptr::addr_of!(LABEL) as *const u8
    } else {
        b"\xe5\xb7\xb2\xe5\x81\x9c\xe6\xad\xa2\0".as_ptr() // "已停止"
    }
}

/// 事件表里第 index 个事件的三元组(offset, delta, type)。
unsafe fn event(index: usize) -> (u8, u8) {
    let buf = core::ptr::addr_of!(BUF) as *const u8;
    let off = 8 + index * 3;
    (
        core::ptr::read_volatile(buf.add(off)),
        core::ptr::read_volatile(buf.add(off + 1)),
    )
}

/// 每拍调用: 到点的事件依次触发。返回是否有事件触发(供上层判断是否要重绘)。
pub(crate) unsafe fn tick(now: u32) -> bool {
    if PLAYING == 0 {
        return false;
    }
    let mut fired = false;
    // 一拍内可能积压多个事件(50ms 拍 + 10ms 粒度), 全部补发。
    while CURSOR < EVENTS {
        let (delta, ty) = event(CURSOR);
        if (now.wrapping_sub(DUE) as i32) < 0 {
            break;
        }
        fw_api::vibrate(ty as u32);
        CURSOR += 1;
        DUE = DUE.wrapping_add((delta as u32) * 10);
        fired = true;
        // 事件用完了: 下一拍结束播放。
        if CURSOR >= EVENTS {
            PLAYING = 0;
            break;
        }
    }
    fired
}
