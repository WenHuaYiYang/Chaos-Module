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
        SHOWN = [None; DEV_SLOTS];
        DEV_BUILT = 0;
        DEV_SIG = [[0; 6]; DEV_SLOTS];
        SCAN_ATTEMPT = 0;
        WEB_HINT = b"\\0";
        PAN_TARGET = None;
        PAN_TARGET_ERROR_SERIAL = 0;
    }
}

#[test]
fn diagnostics_abi_and_page_limits_are_fixed() {
    let _guard = serial(); reset_ui();
    assert_eq!(core::mem::size_of::<BtDiagnostics>(), 164);
    assert!(PAN_ROWS <= ROW_CAP);
    assert!(WEB_ROWS <= ROW_CAP);
    assert!(INFO_ROWS <= ROW_CAP);
}

#[test]
fn web_page_rows_no_longer_split_the_password() {
    // v1.1.5 起网页服务器页不再把密码切成多行展示(9 行: 开关/状态/地址/用户名/
    // 密码占位/设置密码/btpan跳转/统计/诊断), 密码只在"设置密码"里输入。
    let _guard = serial(); reset_ui();
    assert_eq!(WEB_ROWS, 9);
    assert_eq!((WEB_SET_PASSWORD, WEB_BTPAN), (5, 6));
    assert!(WEB_BTPAN < WEB_ROWS);
}

#[test]
fn diagnostic_label_is_attached_after_ten_info_rows() {
    let _guard = serial(); reset_ui();
    unsafe {
        PAGES[PAGE_NETINFO_PID as usize] = PageState { built: 1, content: 40, title: 0, rows: [0; 12] };
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
        PAGES[PAGE_NETINFO_PID as usize] = PageState { built: 1, content: 50, title: 0, rows: [0; 12] };
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
