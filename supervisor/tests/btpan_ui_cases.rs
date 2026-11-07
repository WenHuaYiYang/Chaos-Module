use super::*;

use crate::page::Page;
use std::sync::{Mutex, OnceLock};

static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
fn serial() -> std::sync::MutexGuard<'static, ()> { TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap() }
fn reset_ui() {
    crate::reset_mock();
    unsafe {
        WORKER_STARTED.store(false, std::sync::atomic::Ordering::Release);
        WORKER_ERROR.store(0, std::sync::atomic::Ordering::Release);
        PAN_CACHE = RowCache::EMPTY;
        WEB_CACHE = RowCache::EMPTY;
        INFO_CACHE = RowCache::EMPTY;
        INFO_DIAG_LABEL = 0;
        INFO_DIAG_CONTENT = 0;
        INFO_DIAG_ANCHOR = 0;
        INFO_DIAG_TEXT = [0; 1024];
        UPDATING = false;
        PEER_PAGE = 0;
        SHOWN = [None; 4];
        WEB_HINT = b"\\0";
        PAN_TARGET = None;
        PAN_TARGET_ERROR_SERIAL = 0;
    }
}

#[test]
fn diagnostics_abi_and_page_limits_are_fixed() {
    let _guard = serial(); reset_ui();
    assert_eq!(core::mem::size_of::<BtDiagnostics>(), 56);
    assert!(PAN_ROWS <= ROW_CAP);
    assert!(WEB_ROWS <= ROW_CAP);
    assert!(INFO_ROWS <= ROW_CAP);
}

#[test]
fn password_segments_cover_all_bytes_without_overlap() {
    let _guard = serial(); reset_ui();
    for length in 8..=31 {
        let mut covered = 0;
        let segments = (length + WEB_PASSWORD_CHUNK - 1) / WEB_PASSWORD_CHUNK;
        for part in 0..WEB_PASSWORD_ROWS {
            let start = part * WEB_PASSWORD_CHUNK;
            let end = (start + WEB_PASSWORD_CHUNK).min(length);
            if start < end { assert_eq!(end - start, (start..end).count()); covered += end - start; }
        }
        assert_eq!(covered, length);
        assert!(segments <= WEB_PASSWORD_ROWS);
    }
}

#[test]
fn diagnostic_label_is_attached_after_ten_info_rows() {
    let _guard = serial(); reset_ui();
    unsafe {
        PAGES[PAGE_NETINFO_PID as usize] = PageState { built: 1, content: 40, rows: [0; 12] };
        let mut rows = [0u32; 12];
        let ctx = PageCtx { content: 40, title: 41, rows: rows.as_mut_ptr() };
        PAGE_NETINFO_P.render(&ctx).unwrap();
        assert_eq!(rows[INFO_ROWS - 1], 10);
        assert_eq!(INFO_DIAG_CONTENT, 40);
        assert_eq!(INFO_DIAG_ANCHOR, rows[INFO_ROWS - 1]);
        assert_ne!(INFO_DIAG_LABEL, 0);
    }
}

#[test]
fn diagnostic_label_rejects_stale_content_generation() {
    let _guard = serial(); reset_ui();
    unsafe {
        PAGES[PAGE_NETINFO_PID as usize] = PageState { built: 1, content: 50, rows: [0; 12] };
        let mut rows = [0u32; 12];
        let ctx = PageCtx { content: 50, title: 51, rows: rows.as_mut_ptr() };
        PAGE_NETINFO_P.render(&ctx).unwrap();
        let old = INFO_DIAG_LABEL;
        PAGES[PAGE_NETINFO_PID as usize].content = 99;
        refresh_diagnostics();
        assert_eq!(INFO_DIAG_LABEL, old);
        assert_eq!(INFO_DIAG_CONTENT, 50);
    }
}
