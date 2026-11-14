//! 通用英文数字 / 中文拼音键盘。
//!
//! 页面层只负责标题和生命周期；输入缓冲、大小写、按键事件与完成动作都由本组件管理。
//! 后续页面可通过 open + 新的 Completion 变体复用同一套键盘。
//!
//! 数字和三行字母共用横向滚动视口（一屏 336px，整块 640px），字母键保持 56px，
//! 拖动看后面的键；输入框与操作区固定在视口上下，不随键盘滑走。
//!
//! 中文模式（仅四个「关于本机」自定义字段可用，NSH / 网页密码仍保持纯 ASCII）：
//! * `中` 键切换，进入后字母键输入拼音，输入框显示 `已上屏文本|拼音`；
//! * 键盘首行是候选条：6 个候选字 + 一个「更多」键，候选按字频排序；
//! * 点「更多」把键盘换成候选面板：每行 6 格，纵向滚动查看全部候选（最多 128 个），
//!   点候选上屏并收回；「收」键回到字母键盘；
//! * 候选字来自离线生成的 `pinyin_data.rs`，按前缀查表，完整音节只取本音节字；
//! * 连续输入多个音节时，候选始终对应最长的完整音节前缀（`nihao` 先给 `ni` 的候选），
//!   上屏一个字后剩余拼音继续给候选；空格逐个上屏，OK 一次上屏全部；
//! * 空格上屏首选字；`EN` 键切到「英文/数字」页，首行恢复 10 个数字键；
//! * 退格按 UTF-8 字符整体回退，容量仍按字节计，一个汉字占 3 字节。

// 用 `#[path]` 而不是 crate 级 mod：主机回归 harness 直接 include 本文件，
// 也能拿到同一份候选表，不需要在 harness 里再补一遍 mock。
#[path = "pinyin_data.rs"]
mod pinyin_data;

use crate::*;

pub(crate) const PAGE_PID: u32 = 23;
const CAPACITY: usize = 31;
const NSH_CAPACITY: usize = 255;

const KEY_SHIFT: u32 = 0x100;
const KEY_BACKSPACE: u32 = 0x101;
const KEY_CLEAR: u32 = 0x102;
const KEY_DONE: u32 = 0x103;
const KEY_SYMBOLS: u32 = 0x104;
const KEY_CHINESE: u32 = 0x105;
const KEY_MORE: u32 = 0x106;
const KEY_COLLAPSE: u32 = 0x107;
const KEY_CANDIDATE: u32 = 0x200;
const KEY_CANDIDATE_END: u32 = KEY_CANDIDATE + (MAX_CAND as u32 - 1);

const MAX_CAND: usize = 128; // 候选缓冲上限，也是候选面板的格数上限
const CAND_BAR: usize = 6; // 候选条一屏可见字数（后面跟一个「更多」键）
const PINYIN_CAPACITY: usize = 8;

// 横向滚动键盘：一屏 336px，整块 640px。字母键保持 56px，拖动看后面的键。
const VIEWPORT_WIDTH: i32 = 336;
const KEY_WIDTH: i32 = 56;
const KEY_HEIGHT: i32 = 50;
const KEY_GAP: i32 = 8;
const ROW_STEP: i32 = KEY_HEIGHT + KEY_GAP;
const BOARD_WIDTH: i32 = 10 * (KEY_WIDTH + KEY_GAP);
const BOARD_HEIGHT_PINYIN: i32 = 3 * KEY_HEIGHT + 2 * KEY_GAP;
const BOARD_HEIGHT_FULL: i32 = 4 * KEY_HEIGHT + 3 * KEY_GAP;

// 有候选条时视口下移、只装三行字母；没有候选条时装四行。
const VIEWPORT_TOP_PLAIN: i32 = 56;
const VIEWPORT_TOP_BAR: i32 = 110;
const VIEWPORT_HEIGHT_PLAIN: i32 = BOARD_HEIGHT_FULL + 8; // 232，为滚动条留白
const VIEWPORT_HEIGHT_BAR: i32 = BOARD_HEIGHT_PINYIN + 8; // 174
const ACTION_TOP_PLAIN: i32 = VIEWPORT_TOP_PLAIN + VIEWPORT_HEIGHT_PLAIN + 8;
const ACTION_TOP_BAR: i32 = VIEWPORT_TOP_BAR + VIEWPORT_HEIGHT_BAR + 8;
const ACTION_HEIGHT: u32 = 44;
const BOTTOM_TOP_PLAIN: i32 = ACTION_TOP_PLAIN + ACTION_HEIGHT as i32 + KEY_GAP;
const BOTTOM_TOP_BAR: i32 = ACTION_TOP_BAR + ACTION_HEIGHT as i32 + KEY_GAP;
const INPUT_TOP: i32 = 4;
const INPUT_HEIGHT: u32 = 48;

// 候选条固定在键盘视口上方，不随键盘横滑。
const CAND_BAR_TOP: i32 = 56;
const CAND_BAR_HEIGHT: i32 = 50;
const CAND_BAR_W: i32 = 38;
const CAND_BAR_STEP: i32 = CAND_BAR_W + 4;
const CAND_BAR_X: i32 = 2;
const CAND_MORE_W: i32 = 76;

// 候选面板：每行 6 格，格 52x50、行距 4，整块宽 336，纵向滚动看更多。
const GRID_COLS: usize = 6;
const GRID_W: i32 = 52;
const GRID_STEP_X: i32 = GRID_W + 2;
const GRID_X: i32 = 7;
const GRID_ROW_STEP: i32 = KEY_HEIGHT + 4;

const NUMBER_KEYS: [u32; 10] = [
    b'1' as u32,
    b'2' as u32,
    b'3' as u32,
    b'4' as u32,
    b'5' as u32,
    b'6' as u32,
    b'7' as u32,
    b'8' as u32,
    b'9' as u32,
    b'0' as u32,
];
const QWERTY_KEYS: [u32; 10] = [
    b'q' as u32,
    b'w' as u32,
    b'e' as u32,
    b'r' as u32,
    b't' as u32,
    b'y' as u32,
    b'u' as u32,
    b'i' as u32,
    b'o' as u32,
    b'p' as u32,
];
const HOME_KEYS: [u32; 9] = [
    b'a' as u32,
    b's' as u32,
    b'd' as u32,
    b'f' as u32,
    b'g' as u32,
    b'h' as u32,
    b'j' as u32,
    b'k' as u32,
    b'l' as u32,
];
const BOTTOM_KEYS: [u32; 7] = [
    b'z' as u32,
    b'x' as u32,
    b'c' as u32,
    b'v' as u32,
    b'b' as u32,
    b'n' as u32,
    b'm' as u32,
];

#[derive(Clone, Copy)]
pub(crate) enum Completion {
    SystemVersion,
    SystemText,
    DeviceName,
    DeviceModel,
    NshCommand,
    WebPassword,
}

static mut TEXT: [u8; NSH_CAPACITY + 1] = [0; NSH_CAPACITY + 1];
static mut LENGTH: usize = 0;
static mut UPPERCASE: bool = true;
static mut COMPLETION: u32 = 0;
static mut INPUT_LABEL: u32 = 0;
static mut LETTER_LABELS: [u32; 26] = [0; 26];
static mut SHIFT_LABEL: u32 = 0;
static mut SYMBOL_MODE: bool = false;
static mut SUBMITTED: bool = false;
static mut NSH_DRAFT: [u8; NSH_CAPACITY + 1] = [0; NSH_CAPACITY + 1];

// 中文拼音状态。CAND_CHARS 每格是一个完整 UTF-8 单字 + 结尾 0，可直接喂给
// label_set_text（固件 label 会复制字符串，栈/静态缓冲都安全）。
static mut CHINESE: bool = false;
static mut BROWSING: bool = false; // 候选面板是否打开（纵向滚动看全部候选）
static mut PINYIN: [u8; PINYIN_CAPACITY] = [0; PINYIN_CAPACITY];
static mut PINYIN_LEN: usize = 0;
static mut CAND_CHARS: [[u8; 4]; MAX_CAND] = [[0; 4]; MAX_CAND];
static mut CAND_COUNT: usize = 0;
static mut CAND_PREFIX: usize = 0; // 当前候选对应的拼音前缀长度（上屏后据此推进缓冲）
static mut CAND_LABELS: [u32; MAX_CAND] = [0; MAX_CAND];
static mut DISPLAY: [u8; NSH_CAPACITY + 16] = [0; NSH_CAPACITY + 16];

// All printable ASCII punctuation is available in NSH mode. Other uses keep
// the exact original QWERTY layout and 31-byte validation limit.
const SYMBOL_ROWS: [&[u8]; 4] = [b"!@#$%^&*()", b"_-+=[]{}\\|", b";:'\",.<>/?", b"\x60~"];

fn nsh_mode() -> bool { unsafe { COMPLETION == Completion::NshCommand as u32 } }
fn password_mode() -> bool { unsafe { COMPLETION == Completion::WebPassword as u32 } }
fn symbols_enabled() -> bool { nsh_mode() || password_mode() }
fn chinese_capable() -> bool { !symbols_enabled() }
fn capacity() -> usize { if nsh_mode() { NSH_CAPACITY } else { CAPACITY } }
/// 中文模式里 UPPERCASE 复用为「英文/数字页」开关；false 才是拼音输入页。
fn pinyin_active() -> bool { unsafe { CHINESE && !UPPERCASE } }

/// 把 UTF-8 字串逐字拷进候选格，最多 MAX_CAND 个，跳过重复字。
fn push_chars(bytes: &[u8], out: &mut [[u8; 4]; MAX_CAND], mut n: usize) -> usize {
    let mut i = 0;
    while i < bytes.len() && n < MAX_CAND {
        let lead = bytes[i];
        let width = if lead < 0x80 {
            1
        } else if lead & 0xE0 == 0xC0 {
            2
        } else if lead & 0xF0 == 0xE0 {
            3
        } else {
            4
        };
        if i + width > bytes.len() {
            break;
        }
        let unit = [
            lead,
            bytes.get(i + 1).copied().unwrap_or(0),
            bytes.get(i + 2).copied().unwrap_or(0),
            0,
        ];
        if !out[..n].iter().any(|slot| *slot == unit) {
            out[n] = unit;
            n += 1;
        }
        i += width;
    }
    n
}

/// 第 i 个前缀在紧凑 BLOB 里的字节段。表里没有指针数组，.ko 里就不必带
/// 上千条重定位；查表只是两次 u16 取值 + 一次切片。
fn prefix_bytes(i: usize) -> &'static [u8] {
    let blob = pinyin_data::PREFIXES.as_bytes();
    let a = pinyin_data::PREFIX_OFF[i] as usize;
    let b = pinyin_data::PREFIX_OFF[i + 1] as usize;
    &blob[a..b]
}

fn candidate_bytes(i: usize) -> &'static [u8] {
    let blob = pinyin_data::CANDIDATES.as_bytes();
    let a = pinyin_data::CANDIDATE_OFF[i] as usize;
    let b = pinyin_data::CANDIDATE_OFF[i + 1] as usize;
    &blob[a..b]
}

/// 计算当前拼音的候选字，返回 (候选数, 候选对应的拼音前缀长度)。
///
/// 空拼音给全局高频字。非空时优先取「最长的完整音节前缀」——这样连续打
/// `nihao` 会先给 `ni` 的候选，上屏后剩下的 `hao` 继续给候选；打 `nih`
/// 这种带半截尾巴的也会停在 `ni`，不会候选全空。整串只是半截音节（`zhon`）
/// 时按该前缀给候选；完全查不到（`shqx`）时返回 0，由上屏逻辑回退成字母。
fn fill_candidates(pinyin: &[u8], out: &mut [[u8; 4]; MAX_CAND]) -> (usize, usize) {
    for slot in out.iter_mut() {
        *slot = [0; 4];
    }
    if pinyin.is_empty() {
        return (push_chars(pinyin_data::COMMON.as_bytes(), out, 0), 0);
    }
    let mut exact = None;
    let mut complete = None;
    for i in 0..pinyin_data::COUNT {
        let prefix = prefix_bytes(i);
        if prefix.len() > pinyin.len() || &pinyin[..prefix.len()] != prefix {
            continue;
        }
        if prefix.len() == pinyin.len() {
            exact = Some(i);
        }
        let longer = match complete {
            Some(j) => prefix.len() > prefix_bytes(j).len(),
            None => true,
        };
        if pinyin_data::COMPLETE[i] != 0 && longer {
            complete = Some(i);
        }
    }
    match complete.or(exact) {
        Some(i) => (
            push_chars(candidate_bytes(i), out, 0),
            prefix_bytes(i).len(),
        ),
        None => (0, 0),
    }
}

/// 只改候选标签的文字，不重建对象。候选条模式活 6 个标签，展开网格活 CAND_COUNT 个。
unsafe fn refresh_candidates() {
    let (count, prefix) = fill_candidates(&PINYIN[..PINYIN_LEN], &mut CAND_CHARS);
    CAND_COUNT = count;
    CAND_PREFIX = prefix;
    for i in 0..MAX_CAND {
        let label = CAND_LABELS[i];
        if safe_ptr(label) {
            fw_api::label_set_text(label, CAND_CHARS[i].as_ptr());
        }
    }
}

unsafe fn clear_pinyin() {
    PINYIN_LEN = 0;
    refresh_candidates();
}

/// 上屏一个候选字。容量按字节算，放不下整字就丢弃，绝不写出半个 UTF-8 字符。
/// 上屏后把候选对应的音节从拼音缓冲前面移走，剩下的音节继续给候选。
unsafe fn commit_candidate(index: usize) {
    if index >= CAND_COUNT {
        return;
    }
    let unit = CAND_CHARS[index];
    let mut n = 0;
    while n < 4 && unit[n] != 0 {
        n += 1;
    }
    if n == 0 || LENGTH + n > capacity() {
        return;
    }
    for k in 0..n {
        TEXT[LENGTH + k] = unit[k];
    }
    LENGTH += n;
    TEXT[LENGTH] = 0;
    if CAND_PREFIX > 0 && CAND_PREFIX <= PINYIN_LEN {
        let rest = PINYIN_LEN - CAND_PREFIX;
        for k in 0..rest {
            PINYIN[k] = PINYIN[k + CAND_PREFIX];
        }
        PINYIN_LEN = rest;
    }
    refresh_candidates();
}

/// 没有候选时把剩余拼音字母原样上屏，不吞掉用户的输入。
unsafe fn commit_raw_pinyin() {
    let n = PINYIN_LEN;
    if n == 0 {
        return;
    }
    if LENGTH + n <= capacity() {
        for k in 0..n {
            TEXT[LENGTH + k] = PINYIN[k];
        }
        LENGTH += n;
        TEXT[LENGTH] = 0;
    }
    PINYIN_LEN = 0;
    refresh_candidates();
}

/// 空格 / 点候选：上屏当前音节；没有候选时把剩余字母原样上屏。
unsafe fn commit_pending_pinyin() {
    if PINYIN_LEN == 0 {
        return;
    }
    if CAND_COUNT > 0 {
        commit_candidate(0);
    } else {
        commit_raw_pinyin();
    }
}

/// OK：把还挂着的拼音全部上屏——有候选的逐个音节取首选，没有候选的尾巴原样上屏。
unsafe fn commit_all_pending() {
    let mut guard = 0;
    while PINYIN_LEN > 0 && guard < PINYIN_CAPACITY + 2 {
        guard += 1;
        let before = PINYIN_LEN;
        commit_pending_pinyin();
        if PINYIN_LEN >= before {
            break; // 容量已满，上不去；下面兜底清空，避免死循环
        }
    }
    if PINYIN_LEN > 0 {
        clear_pinyin();
    }
}

/// 退格按「字符」回退：汉字的 3 个字节必须一起删，ASCII 行为与原来一致。
unsafe fn backspace_char() {
    if LENGTH == 0 {
        return;
    }
    let end = LENGTH;
    LENGTH -= 1;
    while LENGTH > 0 && TEXT[LENGTH] & 0xC0 == 0x80 {
        LENGTH -= 1;
    }
    for k in LENGTH..end {
        TEXT[k] = 0;
    }
}

/// 长文本取尾部时不落在一个 UTF-8 字符的中间。
fn tail_start(bytes: &[u8], max: usize) -> usize {
    if bytes.len() <= max {
        return 0;
    }
    let mut i = bytes.len() - max;
    while i < bytes.len() && bytes[i] & 0xC0 == 0x80 {
        i += 1;
    }
    i
}

pub(crate) unsafe fn open(initial: *const u8, completion: Completion) {
    COMPLETION = completion as u32;
    // The title's back gesture cancels editing without losing a draft. A
    // separate buffer survives opening other settings on the shared keyboard.
    let initial = if nsh_mode() && initial.is_null() { NSH_DRAFT.as_ptr() } else { initial };
    LENGTH = 0;
    TEXT[0] = 0;
    if !initial.is_null() {
        while LENGTH < capacity() {
            let c = read_volatile(initial.add(LENGTH));
            if c == 0 {
                break;
            }
            TEXT[LENGTH] = c;
            LENGTH += 1;
        }
        TEXT[LENGTH] = 0;
    }
    UPPERCASE = !symbols_enabled();
    SYMBOL_MODE = false;
    SUBMITTED = false;
    INPUT_LABEL = 0;
    LETTER_LABELS = [0; 26];
    SHIFT_LABEL = 0;
    CAND_LABELS = [0; MAX_CAND];
    CHINESE = false;
    BROWSING = false;
    PINYIN_LEN = 0;
    clear_pinyin();
    save_nsh_draft();
    nav_goto(PAGE_PID, 0);
}

unsafe fn save_nsh_draft() {
    if nsh_mode() {
        NSH_DRAFT[..LENGTH + 1].copy_from_slice(&TEXT[..LENGTH + 1]);
    }
}

pub(crate) unsafe fn title() -> &'static [u8] {
    match COMPLETION {
        x if x == Completion::NshCommand as u32 => "NSH命令输入\0".as_bytes(),
        x if x == Completion::WebPassword as u32 => "网页服务器密码\0".as_bytes(),
        x if x == Completion::SystemText as u32 => "自定义手环系统文本\0".as_bytes(),
        x if x == Completion::DeviceName as u32 => "自定义设备名称\0".as_bytes(),
        x if x == Completion::DeviceModel as u32 => "自定义手环型号\0".as_bytes(),
        _ => "自定义系统版本\0".as_bytes(),
    }
}

unsafe fn refresh_input() {
    if FG_PAGE as usize == PAGE_PID as usize
        && page_is_live(PAGE_PID as usize)
        && safe_ptr(INPUT_LABEL)
    {
        if CHINESE {
            // 中文页把「已上屏文本(尾部) | 正在输入的拼音」拼进独立缓冲，
            // 不改 TEXT —— TEXT 始终是最终要保存的完整 UTF-8 字节串。
            let text = core::slice::from_raw_parts(
                core::ptr::addr_of!(TEXT) as *const u8,
                LENGTH,
            );
            let start = tail_start(text, 24);
            let mut n = 0;
            for &c in &text[start..] {
                DISPLAY[n] = c;
                n += 1;
            }
            DISPLAY[n] = b'|';
            n += 1;
            for k in 0..PINYIN_LEN {
                DISPLAY[n] = PINYIN[k];
                n += 1;
            }
            DISPLAY[n] = 0;
            fw_api::label_set_text(INPUT_LABEL, DISPLAY.as_ptr());
        } else {
            // Keep the insertion point visible for a long NSH command. Input is
            // ASCII-only; custom settings retain their original display behavior.
            let offset = if symbols_enabled() { LENGTH.saturating_sub(20) } else { 0 };
            fw_api::label_set_text(
                INPUT_LABEL,
                (core::ptr::addr_of!(TEXT) as *const u8).add(offset),
            );
        }
    }
}

unsafe fn set_key_text(label: u32, code: u32) {
    if (KEY_CANDIDATE..=KEY_CANDIDATE_END).contains(&code) {
        fw_api::label_set_text(label, CAND_CHARS[(code - KEY_CANDIDATE) as usize].as_ptr());
    } else if code == b' ' as u32 {
        fw_api::label_set_text(label, b"SPACE\0".as_ptr());
    } else if code <= 0xFF {
        let mut c = code as u8;
        if UPPERCASE && c.is_ascii_lowercase() {
            c = c.to_ascii_uppercase();
        }
        fw_api::label_set_text(label, [c, 0].as_ptr());
    } else {
        let text: &[u8] = match code {
            KEY_SHIFT => {
                if CHINESE {
                    if UPPERCASE { b"EN\0" } else { "拼\0".as_bytes() }
                } else if UPPERCASE {
                    b"ABC\0"
                } else {
                    b"abc\0"
                }
            }
            KEY_BACKSPACE => b"<\0",
            KEY_CLEAR => b"CLR\0",
            KEY_DONE => b"OK\0",
            KEY_SYMBOLS => if SYMBOL_MODE { b"abc\0" } else { b"SYM\0" },
            KEY_CHINESE => if CHINESE { b"En\0" } else { "中\0".as_bytes() },
            KEY_MORE => "更多\0".as_bytes(),
            KEY_COLLAPSE => "收\0".as_bytes(),
            _ => b"?\0",
        };
        fw_api::label_set_text(label, text.as_ptr());
    }
}

unsafe fn refresh_case() {
    // 只改活标签，不删对象、不重建页面，保留原生横向滚动位置。
    for i in 0..26 {
        let label = LETTER_LABELS[i];
        if safe_ptr(label) {
            set_key_text(label, b'a' as u32 + i as u32);
        }
    }
    if safe_ptr(SHIFT_LABEL) {
        set_key_text(SHIFT_LABEL, KEY_SHIFT);
    }
}

unsafe fn create_key(
    parent: u32,
    code: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: u32,
) -> bool {
    let key = fw_api::obj_create(parent, 0);
    if key == 0 {
        return false;
    }
    // 默认主题的内边距会压缩标签可用宽度，窄键上曾把字符裁成竖线。
    fw_api::obj_clear_box_spacing(key);
    fw_api::obj_set_size_wh(key, width, height);
    fw_api::obj_align(key, fw_api::align::TOP_LEFT, x, y);
    fw_api::obj_style_color(key, color, 0);
    fw_api::obj_clear_scrollable(key);
    fw_api::obj_add_flag(key, fw_api::LV_OBJ_FLAG_CLICKABLE);
    fw_api::obj_add_event(
        key,
        key_event as *const () as u32,
        fw_api::ev::CLICKED,
        code,
    );

    let label = fw_api::label_create(key);
    if label == 0 {
        return false;
    }
    fw_api::style_apply(label, FW_STYLE_HR_NAME, 255, 0);
    fw_api::obj_clear_scrollable(label);
    fw_api::obj_clear_flag(label, fw_api::LV_OBJ_FLAG_CLICKABLE);
    set_key_text(label, code);
    if (b'a' as u32..=b'z' as u32).contains(&code) {
        LETTER_LABELS[(code - b'a' as u32) as usize] = label;
    } else if (KEY_CANDIDATE..=KEY_CANDIDATE_END).contains(&code) {
        CAND_LABELS[(code - KEY_CANDIDATE) as usize] = label;
    } else if code == KEY_SHIFT {
        SHIFT_LABEL = label;
    }
    fw_api::obj_align(label, fw_api::align::CENTER, 0, 0);
    true
}

/// 候选条：6 个候选字键 + 一个「更多」键，固定在键盘视口上方。
/// 直接挂在页面上，不放进横向滚动视口，所以键盘横滑时这一行不动。
unsafe fn create_candidate_row(parent: u32) -> bool {
    let mut ok = true;
    for i in 0..CAND_BAR {
        if !create_key(
            parent,
            KEY_CANDIDATE + i as u32,
            CAND_BAR_X + i as i32 * CAND_BAR_STEP,
            CAND_BAR_TOP,
            CAND_BAR_W as u32,
            CAND_BAR_HEIGHT as u32,
            0x40_40_40,
        ) {
            ok = false;
        }
    }
    if !create_key(
        parent,
        KEY_MORE,
        CAND_BAR_X + CAND_BAR as i32 * CAND_BAR_STEP,
        CAND_BAR_TOP,
        CAND_MORE_W as u32,
        CAND_BAR_HEIGHT as u32,
        0x40_40_40,
    ) {
        ok = false;
    }
    ok
}

/// 候选面板：把当前全部候选铺成每行 6 格的网格，放在纵向滚动的视口里。
/// 格数随候选数变化，所以面板态下改拼音要重建页面（交给常驻定时器）。
unsafe fn create_candidate_panel(viewport: u32) -> bool {
    let count = CAND_COUNT.min(MAX_CAND);
    let rows = (count + GRID_COLS - 1) / GRID_COLS;
    let content = fw_api::obj_create(viewport, 0);
    if content == 0 {
        return false;
    }
    fw_api::obj_clear_box_spacing(content);
    fw_api::obj_clear_scrollable(content);
    fw_api::obj_set_size(content, VIEWPORT_WIDTH, (rows as i32) * GRID_ROW_STEP);
    fw_api::obj_align(content, fw_api::align::TOP_LEFT, 0, 0);
    fw_api::obj_style_color(content, 0, 0);
    let mut ok = true;
    for i in 0..count {
        let col = (i % GRID_COLS) as i32;
        let row = (i / GRID_COLS) as i32;
        if !create_key(
            content,
            KEY_CANDIDATE + i as u32,
            GRID_X + col * GRID_STEP_X,
            row * GRID_ROW_STEP,
            GRID_W as u32,
            KEY_HEIGHT as u32,
            0x40_40_40,
        ) {
            ok = false;
        }
    }
    ok
}

/// 一行字母在 640px 键盘里居中；QWERTY 三行按字数自然错位。
fn row_start_x(n: usize) -> i32 {
    let total = n as i32 * KEY_WIDTH + (n as i32 - 1) * KEY_GAP;
    (BOARD_WIDTH - total) / 2
}

unsafe fn create_char_row(parent: u32, keys: &[u32], y: i32) -> bool {
    let mut x = row_start_x(keys.len());
    let mut ok = true;
    for &code in keys {
        if !create_key(
            parent,
            code,
            x,
            y,
            KEY_WIDTH as u32,
            KEY_HEIGHT as u32,
            0x26_26_26,
        ) {
            ok = false;
        }
        x += KEY_WIDTH + KEY_GAP;
    }
    ok
}

pub(crate) unsafe fn render(parent: u32) -> bool {
    // 页面仍由固件管理；只有键盘视口可以滚动。操作区最底边 392，
    // 在 424px content 内保留底部 32px 手势区，输入框、SPACE/OK 不随键盘滑走。
    INPUT_LABEL = 0;
    LETTER_LABELS = [0; 26];
    SHIFT_LABEL = 0;
    CAND_LABELS = [0; MAX_CAND];
    if BROWSING && CAND_COUNT == 0 {
        BROWSING = false;
    }
    fw_api::obj_clear_scrollable(parent);
    fw_api::obj_clear_box_spacing(parent);

    INPUT_LABEL = fw_api::label_create(parent);
    if INPUT_LABEL == 0 {
        return false;
    }
    fw_api::style_apply(INPUT_LABEL, FW_STYLE_HR_NAME, 255, 0);
    fw_api::label_set_text(INPUT_LABEL, core::ptr::addr_of!(TEXT) as *const u8);
    fw_api::obj_set_size_wh(INPUT_LABEL, 320, INPUT_HEIGHT);
    fw_api::obj_align(INPUT_LABEL, fw_api::align::TOP_LEFT, 8, INPUT_TOP);

    let mut ok = true;
    let bar = pinyin_active();
    if bar {
        // 候选条固定在视口上方，不参与键盘横滑。
        if !create_candidate_row(parent) {
            ok = false;
        }
    }
    let viewport_top = if bar { VIEWPORT_TOP_BAR } else { VIEWPORT_TOP_PLAIN };
    let viewport_height = if bar { VIEWPORT_HEIGHT_BAR } else { VIEWPORT_HEIGHT_PLAIN };
    let action_top = if bar { ACTION_TOP_BAR } else { ACTION_TOP_PLAIN };
    let bottom_top = if bar { BOTTOM_TOP_BAR } else { BOTTOM_TOP_PLAIN };

    let viewport = fw_api::obj_create(parent, 0);
    if viewport == 0 {
        return false;
    }
    fw_api::obj_clear_box_spacing(viewport);
    fw_api::obj_set_size(viewport, VIEWPORT_WIDTH, viewport_height);
    fw_api::obj_align(viewport, fw_api::align::TOP_LEFT, 0, viewport_top);
    fw_api::obj_style_color(viewport, 0, 0);
    fw_api::obj_add_flag(
        viewport,
        fw_api::LV_OBJ_FLAG_SCROLLABLE | fw_api::LV_OBJ_FLAG_CLICKABLE,
    );
    // 到边界就停；不要把拖动继续传给页面触发右滑返回。标题返回键保留。
    fw_api::obj_clear_flag(
        viewport,
        fw_api::LV_OBJ_FLAG_SCROLL_CHAIN
            | fw_api::LV_OBJ_FLAG_GESTURE_BUBBLE
            | fw_api::LV_OBJ_FLAG_SCROLL_ELASTIC,
    );

    if bar && BROWSING {
        // 候选面板：纵向滚动，看完整候选列表。
        fw_api::obj_set_scroll_dir(viewport, fw_api::LV_DIR_VER);
        if !create_candidate_panel(viewport) {
            ok = false;
        }
    } else {
        fw_api::obj_set_scroll_dir(viewport, fw_api::LV_DIR_HOR);
        let board = fw_api::obj_create(viewport, 0);
        if board == 0 {
            return false;
        }
        fw_api::obj_clear_box_spacing(board);
        fw_api::obj_clear_scrollable(board);
        let board_height = if pinyin_active() { BOARD_HEIGHT_PINYIN } else { BOARD_HEIGHT_FULL };
        fw_api::obj_set_size(board, BOARD_WIDTH, board_height);
        fw_api::obj_align(board, fw_api::align::TOP_LEFT, 0, 0);
        fw_api::obj_style_color(board, 0, 0);

        if symbols_enabled() && SYMBOL_MODE {
            for (i, row) in SYMBOL_ROWS.iter().enumerate() {
                let mut keys = [0u32; 10];
                for (j, &c) in row.iter().enumerate() { keys[j] = c as u32; }
                if !create_char_row(board, &keys[..row.len()], i as i32 * ROW_STEP) { ok = false; }
            }
        } else if pinyin_active() {
            // 拼音页：候选条已固定在视口上方，视口里只有三行字母。
            for (i, row) in [QWERTY_KEYS.as_slice(), HOME_KEYS.as_slice(), BOTTOM_KEYS.as_slice()].iter().enumerate() {
                if !create_char_row(board, row, i as i32 * ROW_STEP) { ok = false; }
            }
        } else {
            // 英文 / 数字页（含中文模式按了 EN 之后）：完整四行，数字键回来。
            for (i, row) in [NUMBER_KEYS.as_slice(), QWERTY_KEYS.as_slice(), HOME_KEYS.as_slice(), BOTTOM_KEYS.as_slice()].iter().enumerate() {
                if !create_char_row(board, row, i as i32 * ROW_STEP) { ok = false; }
            }
        }
    }

    // 操作区第一行：候选面板把 SHIFT 换成「收起」，其余不变。
    let first_code = if bar && BROWSING { KEY_COLLAPSE } else { KEY_SHIFT };
    if !create_key(parent, first_code, 8, action_top, 80, ACTION_HEIGHT, 0x40_40_40) {
        ok = false;
    }
    if !create_key(parent, b' ' as u32, 96, action_top, 144, ACTION_HEIGHT, 0x26_26_26) {
        ok = false;
    }
    if !create_key(parent, KEY_BACKSPACE, 248, action_top, 80, ACTION_HEIGHT, 0x40_40_40) {
        ok = false;
    }

    if !create_key(parent, KEY_CLEAR, 8, bottom_top, 80, ACTION_HEIGHT, 0x40_40_40) {
        ok = false;
    }
    if !create_key(
        parent,
        if symbols_enabled() { KEY_SYMBOLS } else { b'.' as u32 },
        96,
        bottom_top,
        56,
        ACTION_HEIGHT,
        0x26_26_26,
    ) {
        ok = false;
    }
    // 只有四个「关于本机」字段支持中文；NSH / 密码页保持三键布局，OK 仍是 168px。
    // 中文页把 OK 缩到 104px，腾出 56px 给「中 / En」。
    if chinese_capable() {
        if !create_key(parent, KEY_CHINESE, 160, bottom_top, 56, ACTION_HEIGHT, 0x40_40_40) {
            ok = false;
        }
    }
    let (done_x, done_w) = if chinese_capable() { (224, 104) } else { (160, 168) };
    if !create_key(parent, KEY_DONE, done_x, bottom_top, done_w, ACTION_HEIGHT, 0x53_81_FE) {
        ok = false;
    }
    refresh_candidates();
    refresh_input();
    ok
}

unsafe extern "C" fn key_event(event: u32) {
    if event == 0
        || FG_PAGE != PAGE_PID
        || !page_is_live(PAGE_PID as usize)
        || fw_api::event_get_code(event) != fw_api::ev::CLICKED
    {
        return;
    }
    let code = fw_api::event_user_data(event);
    match code {
        KEY_SYMBOLS if symbols_enabled() => {
            SYMBOL_MODE = !SYMBOL_MODE;
            render_req(PAGE_PID);
            return;
        }
        KEY_CHINESE if chinese_capable() => {
            CHINESE = !CHINESE;
            PINYIN_LEN = 0;
            BROWSING = false;
            UPPERCASE = if CHINESE { false } else { !symbols_enabled() };
            SYMBOL_MODE = false;
            // 首行要在「候选条」和「数字行」之间换，必须重建页面。
            render_req(PAGE_PID);
            return;
        }
        KEY_MORE if pinyin_active() => {
            // 候选比候选条能显示的多才值得打开面板。
            if CAND_COUNT > CAND_BAR {
                BROWSING = true;
                render_req(PAGE_PID);
            }
            return;
        }
        KEY_COLLAPSE if pinyin_active() => {
            BROWSING = false;
            render_req(PAGE_PID);
            return;
        }
        KEY_CANDIDATE..=KEY_CANDIDATE_END if pinyin_active() => {
            commit_candidate((code - KEY_CANDIDATE) as usize);
            if BROWSING {
                BROWSING = false;
                render_req(PAGE_PID);
            }
        }
        KEY_SHIFT => {
            UPPERCASE = !UPPERCASE;
            if CHINESE {
                // 切到英文/数字页时丢弃还没上屏的拼音，避免两种输入态混在一起。
                PINYIN_LEN = 0;
                BROWSING = false;
                render_req(PAGE_PID);
            } else {
                refresh_case();
            }
            return;
        }
        KEY_BACKSPACE => {
            if CHINESE && PINYIN_LEN > 0 {
                PINYIN_LEN -= 1;
                refresh_candidates();
                // 面板格数随候选数变，重建列表；候选清空时 render 会退回键盘。
                if BROWSING { render_req(PAGE_PID); }
            } else {
                backspace_char();
            }
        }
        KEY_CLEAR => {
            LENGTH = 0;
            TEXT[0] = 0;
            if CHINESE {
                clear_pinyin();
                if BROWSING {
                    BROWSING = false;
                    render_req(PAGE_PID);
                }
            }
        }
        KEY_DONE => {
            if SUBMITTED { return; }
            // OK 之前还有挂着的拼音：逐个音节上屏首选，没候选的尾巴原样上屏。
            if CHINESE && PINYIN_LEN > 0 {
                commit_all_pending();
            }
            if LENGTH == 0 && COMPLETION != Completion::NshCommand as u32 {
                return;
            }
            let saved = match COMPLETION {
                x if x == Completion::SystemVersion as u32 => {
                    crate::system_version::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::SystemText as u32 => {
                    crate::system_text::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::DeviceName as u32 => {
                    crate::device_name::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::DeviceModel as u32 => {
                    crate::device_model::save_custom(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::NshCommand as u32 => {
                    crate::terminal_ui::submit_command(core::ptr::addr_of!(TEXT) as *const u8)
                }
                x if x == Completion::WebPassword as u32 => {
                    crate::btpan::save_web_password(core::ptr::addr_of!(TEXT) as *const u8)
                }
                _ => false,
            };
            if saved {
                SUBMITTED = true;
                if nsh_mode() { NSH_DRAFT[0] = 0; }
                nav_back();
            } else if nsh_mode() && safe_ptr(INPUT_LABEL) {
                // Keep the buffer unchanged; the next key restores the tail.
                fw_api::label_set_text(INPUT_LABEL, "发送失败，请重试\0".as_ptr());
            } else if password_mode() && safe_ptr(INPUT_LABEL) {
                fw_api::label_set_text(INPUT_LABEL, "密码需8～31位字符\0".as_ptr());
            }
            return;
        }
        0x20..=0xFF => {
            if pinyin_active() && (b'a' as u32..=b'z' as u32).contains(&code) {
                if PINYIN_LEN < PINYIN_CAPACITY {
                    PINYIN[PINYIN_LEN] = code as u8;
                    PINYIN_LEN += 1;
                    refresh_candidates();
                }
            } else if CHINESE && code == b' ' as u32 && PINYIN_LEN > 0 {
                commit_pending_pinyin();
                if BROWSING {
                    BROWSING = false;
                    render_req(PAGE_PID);
                }
            } else if LENGTH < capacity() {
                // 拼音还挂着时按了标点：先把音节上屏，标点落在它后面，
                // 否则字符顺序会变成「.你」。上屏可能刚好占满容量，要再判一次。
                if CHINESE && PINYIN_LEN > 0 {
                    commit_pending_pinyin();
                }
                if LENGTH < capacity() {
                    let mut c = code as u8;
                    if UPPERCASE && c.is_ascii_lowercase() {
                        c -= b'a' - b'A';
                    }
                    TEXT[LENGTH] = c;
                    LENGTH += 1;
                    TEXT[LENGTH] = 0;
                }
            }
        }
        _ => return,
    }
    save_nsh_draft();
    refresh_input();
}

#[cfg(test)]
#[path = "../tests/keyboard_cases.rs"]
mod tests;
