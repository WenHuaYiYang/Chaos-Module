//! Bounded line-oriented NSH transcript, not a full VT100 emulator.
//! No allocation; UTF-8 is validated before it reaches an LVGL label.

pub(crate) const HISTORY_LINES: usize = 96;
pub(crate) const PAGE_LINES: usize = 6;
pub(crate) const LINE_BYTES: usize = 72;
const COLUMNS: usize = 18;
pub(crate) const PAGE_BYTES: usize = PAGE_LINES * (LINE_BYTES + 1) + 1;

#[derive(Clone, Copy)]
struct Line { bytes: [u8; LINE_BYTES], len: usize }
const EMPTY: Line = Line { bytes: [0; LINE_BYTES], len: 0 };

pub(crate) struct Model {
    lines: [Line; HISTORY_LINES],
    head: usize,
    count: usize,
    cursor: usize,
    column: usize,
    escape: u8,
    parameter: u16,
    parameter_done: bool,
    pending: [u8; 4],
    pending_len: usize,
    pending_need: usize,
    pub(crate) revision: u32,
    pub(crate) newlines: u32,
}

#[cfg(test)]
#[path = "../tests/terminal_model_cases.rs"]
mod tests;

impl Model {
    pub(crate) const fn new() -> Self {
        Self { lines: [EMPTY; HISTORY_LINES], head: 0, count: 1, cursor: 0,
            column: 0, escape: 0, parameter: 0, parameter_done: false,
            pending: [0; 4], pending_len: 0, pending_need: 0, revision: 0, newlines: 0 }
    }

    pub(crate) fn clear(&mut self) {
        // Reset lengths in place: no large temporary on the small UI stack.
        for line in &mut self.lines { line.len = 0; }
        self.head = 0;
        self.count = 1;
        self.cursor = 0;
        self.column = 0;
        self.escape = 0;
        self.pending_len = 0;
        self.pending_need = 0;
        self.revision = self.revision.wrapping_add(1);
    }

    fn newline(&mut self) {
        self.head = (self.head + 1) % HISTORY_LINES;
        self.lines[self.head].len = 0;
        self.count = (self.count + 1).min(HISTORY_LINES);
        self.cursor = 0;
        self.column = 0;
        self.newlines = self.newlines.wrapping_add(1);
        self.revision = self.revision.wrapping_add(1);
    }

    fn write_char(&mut self, bytes: &[u8]) {
        let width = if bytes[0] < 0x80 { 1 } else { 2 };
        if self.column + width > COLUMNS { self.newline(); }
        let line = &mut self.lines[self.head];
        let mut end = self.cursor;
        if end < line.len {
            end += 1;
            while end < line.len && line.bytes[end] & 0xc0 == 0x80 { end += 1; }
        }
        let new_len = line.len - (end - self.cursor) + bytes.len();
        if new_len > LINE_BYTES { return; }
        line.bytes.copy_within(end..line.len, self.cursor + bytes.len());
        line.bytes[self.cursor..self.cursor + bytes.len()].copy_from_slice(bytes);
        line.len = new_len;
        self.cursor += bytes.len();
        self.column += width;
        self.revision = self.revision.wrapping_add(1);
    }

    fn backspace(&mut self) {
        if self.cursor == 0 { return; }
        let line = &self.lines[self.head];
        self.cursor -= 1;
        while self.cursor > 0 && line.bytes[self.cursor] & 0xc0 == 0x80 { self.cursor -= 1; }
        self.column = core::str::from_utf8(&line.bytes[..self.cursor]).unwrap_or("")
            .chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    }

    fn csi(&mut self, final_byte: u8) {
        match final_byte {
            b'K' => {
                let line = &mut self.lines[self.head];
                if self.parameter == 2 { line.len = 0; self.cursor = 0; self.column = 0; }
                else if self.parameter == 0 { line.len = self.cursor; }
                self.revision = self.revision.wrapping_add(1);
            }
            b'J' if self.parameter == 2 || self.parameter == 3 => self.clear(),
            b'D' => { for _ in 0..self.parameter.max(1).min(80) { self.backspace(); } }
            // Colors and unsupported full-screen cursor commands are filtered.
            _ => {}
        }
    }

    fn byte(&mut self, byte: u8) {
        match self.escape {
            1 => {
                self.escape = match byte { b'[' => 2, b']' => 3, b'P' | b'_' | b'^' => 5, 0x20..=0x2f => 7, _ => 0 };
                self.parameter = 0;
                self.parameter_done = false;
                return;
            }
            2 => {
                if byte == 0x1b { self.escape = 1; }
                else if (0x40..=0x7e).contains(&byte) { self.escape = 0; self.csi(byte); }
                else if byte == b';' { self.parameter_done = true; }
                else if byte.is_ascii_digit() && !self.parameter_done {
                    self.parameter = self.parameter.saturating_mul(10).saturating_add((byte - b'0') as u16);
                }
                return;
            }
            3 | 5 => {
                if byte == 7 { self.escape = 0; }
                else if byte == 0x1b { self.escape += 1; }
                return;
            }
            4 | 6 => {
                if byte == b'\\' { self.escape = 0; }
                else if byte != 0x1b { self.escape -= 1; }
                return;
            }
            7 => { if (0x30..=0x7e).contains(&byte) { self.escape = 0; } return; }
            _ => {}
        }
        if self.pending_len != 0 {
            if byte & 0xc0 == 0x80 {
                self.pending[self.pending_len] = byte;
                self.pending_len += 1;
                if self.pending_len == self.pending_need {
                    let bytes = self.pending;
                    let n = self.pending_len;
                    self.pending_len = 0;
                    if core::str::from_utf8(&bytes[..n]).is_ok() { self.write_char(&bytes[..n]); }
                    else { self.write_char(b"?"); }
                }
                return;
            }
            self.pending_len = 0;
            self.write_char(b"?");
        }
        match byte {
            0x1b => self.escape = 1,
            b'\r' => { self.cursor = 0; self.column = 0; }
            b'\n' => self.newline(),
            8 | 0x7f => self.backspace(),
            b'\t' => { let n = 4 - self.column % 4; for _ in 0..n { self.write_char(b" "); } }
            0x20..=0x7e => self.write_char(&[byte]),
            0xc2..=0xf4 => {
                self.pending[0] = byte;
                self.pending_len = 1;
                self.pending_need = if byte < 0xe0 { 2 } else if byte < 0xf0 { 3 } else { 4 };
            }
            0x80..=0xff => self.write_char(b"?"),
            _ => {}
        }
    }

    pub(crate) fn feed(&mut self, input: &[u8]) { for &byte in input { self.byte(byte); } }
    pub(crate) fn max_offset(&self) -> usize { self.count.saturating_sub(PAGE_LINES) }

    pub(crate) fn page(&self, older_lines: usize, out: &mut [u8; PAGE_BYTES]) {
        let end = self.count.saturating_sub(older_lines.min(self.max_offset()));
        let start = end.saturating_sub(PAGE_LINES);
        let oldest = (self.head + HISTORY_LINES + 1 - self.count) % HISTORY_LINES;
        let mut n = 0;
        for i in start..end {
            let line = &self.lines[(oldest + i) % HISTORY_LINES];
            out[n..n + line.len].copy_from_slice(&line.bytes[..line.len]);
            n += line.len;
            if i + 1 != end { out[n] = b'\n'; n += 1; }
        }
        out[n] = 0;
    }
}
