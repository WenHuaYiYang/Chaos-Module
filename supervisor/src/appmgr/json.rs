//! 面向固定缓冲区的轻量 JSON 扫描器，不分配堆内存。

use super::*;

// ===== JSON 扫描(base/len 参数化, REG_BUF 与 OBJ_BUF 共用) =====

#[inline(always)]
pub(crate) unsafe fn s_peek(base: *const u8, len: usize, p: usize) -> u8 {
    if p < len { read_volatile(base.add(p)) } else { 0 }
}

#[inline(always)]
pub(crate) unsafe fn s_ws(base: *const u8, len: usize, p: &mut usize) {
    while *p < len {
        let c = read_volatile(base.add(*p));
        if c != b' ' && c != b'\t' && c != b'\n' && c != b'\r' { break; }
        *p += 1;
    }
}

/// 跳过一个字符串(*p 在开引号)。返回 false = 坏。
pub(crate) unsafe fn s_string_skip(base: *const u8, len: usize, p: &mut usize) -> bool {
    if s_peek(base, len, *p) != b'"' { return false; }
    *p += 1;
    while *p < len {
        let c = read_volatile(base.add(*p)); *p += 1;
        if c == b'\\' { if *p < len { *p += 1; continue; } return false; }
        if c == b'"' { return true; }
    }
    false
}

/// 跳过任意 JSON 值(深度计数, 字符串感知)。返回 false = 坏。
pub(crate) unsafe fn s_value_skip(base: *const u8, len: usize, p: &mut usize) -> bool {
    s_ws(base, len, p);
    let c = s_peek(base, len, *p);
    if c == b'"' { return s_string_skip(base, len, p); }
    if c == b'{' || c == b'[' {
        let mut depth = 0u32;
        loop {
            if *p >= len { return false; }
            let ch = read_volatile(base.add(*p)); *p += 1;
            if ch == b'"' {
                *p -= 1;
                if !s_string_skip(base, len, p) { return false; }
                continue;
            }
            if ch == b'{' || ch == b'[' { depth += 1; }
            else if ch == b'}' || ch == b']' {
                depth -= 1;
                if depth == 0 { return true; }
            }
        }
    }
    // true / false / null / 数字: 吃到分隔符为止
    while *p < len {
        let ch = read_volatile(base.add(*p));
        if ch == b',' || ch == b'}' || ch == b']' || ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' { break; }
        *p += 1;
    }
    true
}

/// 读字符串到 dst(带最小 unescape)。0=ok 1=溢出(串本身已跳过) 2=坏。
pub(crate) unsafe fn s_string_read(base: *const u8, len: usize, dst: *mut u8, cap: usize, p: &mut usize) -> u8 {
    if s_peek(base, len, *p) != b'"' { return 2; }
    *p += 1;
    let mut o = 0usize;
    let mut over = false;
    loop {
        if *p >= len { return 2; }
        let c = read_volatile(base.add(*p)); *p += 1;
        if c == b'"' { break; }
        let mut ch = c;
        if c == b'\\' {
            if *p >= len { return 2; }
            let e = read_volatile(base.add(*p)); *p += 1;
            ch = match e {
                b'"' | b'\\' | b'/' => e,
                b'n' => 0x0A, b't' => 0x09, b'r' => 0x0D, b'b' => 0x08, b'f' => 0x0C,
                b'u' => { if *p + 4 > len { return 2; } *p += 4; 0x20 }
                _ => return 2,
            };
        }
        if o + 1 < cap { write_volatile(dst.add(o), ch); o += 1; } else { over = true; }
    }
    write_volatile(dst.add(o), 0);
    if over { 1 } else { 0 }
}

pub(crate) unsafe fn key_is(key: *const u8, lit: &[u8]) -> bool {
    let n = cstr_len(key);
    if n + 1 != lit.len() { return false; }
    let mut i = 0usize;
    while i < n { if read_volatile(key.add(i)) != lit[i] { return false; } i += 1; }
    true
}
