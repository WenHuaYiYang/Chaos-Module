use super::*;

fn view(m: &Model, offset: usize) -> String {
    let mut out = [0; PAGE_BYTES];
    m.page(offset, &mut out);
    let n = out.iter().position(|&b| b == 0).unwrap();
    String::from_utf8(out[..n].to_vec()).unwrap()
}

#[test]
fn linefeed_and_crlf_produce_real_lines() {
    let mut m = Model::new();
    m.feed(b"one\r\ntwo\nthree");
    assert_eq!(view(&m, 0), "one\ntwo\nthree");
}

#[test]
fn carriage_return_overwrites_and_erase_line_removes_remainder() {
    let mut m = Model::new();
    m.feed(b"progress 90%\rdone\x1b[K");
    assert_eq!(view(&m, 0), "done");
    m.feed(b"\rOK");
    assert_eq!(view(&m, 0), "OKne");
}

#[test]
fn backspace_preserves_terminal_cursor_semantics_and_unicode_boundary() {
    let mut m = Model::new();
    m.feed("中X\x08Y".as_bytes());
    assert_eq!(view(&m, 0), "中Y");
    m.feed(b"\x08\x08Z\x1b[K");
    assert_eq!(view(&m, 0), "Z");
}

#[test]
fn split_utf8_never_exposes_a_partial_character() {
    let mut m = Model::new();
    let text = "中🙂é".as_bytes();
    for &b in text { m.feed(&[b]); let _ = view(&m, 0); }
    assert_eq!(view(&m, 0), "中🙂é");
    m.feed(&[0xe4, 0xb8]);
    assert_eq!(view(&m, 0), "中🙂é");
    m.feed(&[0xad]);
    assert_eq!(view(&m, 0), "中🙂é中");
}

#[test]
fn malformed_utf8_is_replaced_without_hiding_following_ascii() {
    let mut m = Model::new();
    m.feed(&[0xe4, b'A', 0xff, 0xc0, 0x80, 0xed, 0xa0, 0x80]);
    assert_eq!(view(&m, 0), "?A????");
}

#[test]
fn ansi_csi_osc_dcs_and_charset_sequences_are_filtered_across_chunks() {
    let mut m = Model::new();
    for bytes in [b"\x1b[3".as_slice(), b"1mred\x1b[0m\x1b]title", b" hidden\x1b", b"\\!\x1bPpayload\x1b\\\x1b(Bok"] { m.feed(bytes); }
    assert_eq!(view(&m, 0), "red!ok");
}

#[test]
fn erase_display_resets_history_and_pending_parser() {
    let mut m = Model::new();
    m.feed(b"old\nold2\x1b[2Jnew");
    assert_eq!(view(&m, 0), "new");
    assert_eq!(m.max_offset(), 0);
}

#[test]
fn long_lines_wrap_and_history_rolls_forever() {
    let mut m = Model::new();
    m.feed(&vec![b'x'; 30000]);
    m.feed(b"\nLATEST");
    assert!(view(&m, 0).ends_with("LATEST"));
    assert_eq!(m.count, HISTORY_LINES);
    assert_eq!(m.max_offset(), HISTORY_LINES - PAGE_LINES);
    for line in view(&m, 0).lines() { assert!(line.len() <= COLUMNS); }
}

#[test]
fn paging_old_and_new_output_is_bounded() {
    let mut m = Model::new();
    for n in 0..120 { m.feed(format!("row{n}\n").as_bytes()); }
    assert!(view(&m, 0).contains("row119"));
    assert!(!view(&m, 6).contains("row119"));
    assert_eq!(view(&m, usize::MAX), view(&m, m.max_offset()));
    assert!(!view(&m, m.max_offset()).contains("row0\n"));
}

#[test]
fn tabs_nul_and_control_bytes_are_safe() {
    let mut m = Model::new();
    m.feed(b"a\tb\0\x07\x01");
    assert_eq!(view(&m, 0), "a   b");
    let rev = m.revision;
    m.feed(b"\0\x07");
    assert_eq!(rev, m.revision);
}

#[test]
fn wrapping_unicode_keeps_every_page_valid_utf8() {
    let mut m = Model::new();
    for _ in 0..500 { for &b in "字符🙂".as_bytes() { m.feed(&[b]); let _ = view(&m, 0); } }
    for n in 0..=m.max_offset() { let _ = view(&m, n); }
}
