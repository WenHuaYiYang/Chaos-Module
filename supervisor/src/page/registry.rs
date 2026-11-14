//! 单一页注册表: 页号(Pid) / 页名 / 页实例 三者对齐由 `pages!` 宏一次生成。
//!
//! 此前页身份散在三处(state.rs 页名 / page.rs PAGE_TABLE / ipc.rs pnames), 全靠下标
//! 人肉对齐; 本文件把它们收成一份清单, 新页只改一行, 编译期断言兜住行序与页号、
//! 页名互异且非空。ipc.rs 的页名指针改为从 PAGE_DEFS 现取, 不再手写第二份。
//!
//! 页名 static 仍住在 state.rs(它是"页名只写一遍"的真源); 这里只引用。
//! PAGE_COUNT 也在 state.rs(发版门按它现算页数); 宏展开的行数与它不符会直接编译失败。

use crate::*;

// 页实例住在兄弟子模块里; 这里 use 进来, 宏行里才能写 `time::PAGE_TIMESET` 这类路径。
use super::{app_icon, appmgr, about, beautify, capture, debug, home, keyboard, log, settings,
            system, time, watchface};

/// 一行的注册项: 页号、页名(page_goto 的解析键)、页面实例。
/// (pid/page 两格是注册表的"自描述": 运行期走 PAGE_TABLE 与 name, 这两格给门禁与工具读。)
#[allow(dead_code)]
pub(crate) struct PageDef {
    pub pid: Pid,
    pub name: &'static [u8],
    pub page: &'static dyn Page,
}

/// 页号枚举: 导航与重建请求一律 `Pid::X as u32`, 禁止裸数字(门禁查)。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Pid {
    Home = 0,
    AppMgr = 1,
    TimeSet = 2,
    DateMd = 3,
    Blur = 4,
    FontList = 5,
    IconSub = 6,
    BlurLegacy = 7,
    WfList = 8,
    WfShake = 9,
    WfMgr = 10,
    Bright = 11,
    Cache = 12,
    Beautify = 13,
    Settings = 14,
    BlurZone = 15,
    Vibrate = 16,
    Log = 17,
    About = 18,
    Fetch = 19,
    Licenses = 20,
    LicenseText = 21,
    SysVer = 22,
    Keyboard = 23,
    AboutMod = 24,
    SysText = 25,
    DevName = 26,
    DevModel = 27,
    AppList = 28,
    Detail = 29,
    Batch = 30,
    Nsh = 31,
    Btpan = 32,
    WebSrv = 33,
    NetInfo = 34,
    Exp = 35,
    BtDiag = 36,
    Debug = 37,
    Capture = 38,
    Recordings = 39,
    Screenshots = 40,
    RecordSet = 41,
    ShotSet = 42,
    AppIcon = 43,
}

/// 注册表: 一行 = (Pid 变体, 页名 static, 页实例)。行序就是页号, 顺序不许变。
macro_rules! pages {
    ( $( $variant:ident = $val:literal , $name:ident , $page:path ; )* ) => {
        pub(crate) static PAGE_DEFS: [PageDef; PAGE_COUNT] = [
            $( PageDef { pid: Pid::$variant, name: &$name, page: &$page }, )*
        ];

        pub(crate) static PAGE_TABLE: [&dyn Page; PAGE_COUNT] = [ $( &$page, )* ];

        // 编译期断言: 行数与 PAGE_COUNT 一致(数组长度失配即编译错)、页号与下标一致、
        // 页名互异且非空 —— 三处对齐从"人肉约定"变成"编译不过"。
        const PID_SEQ: [u32; PAGE_COUNT] = [ $( $val, )* ];
        const NAME_SEQ: [&[u8]; PAGE_COUNT] = [ $( &$name, )* ];
        const _: () = {
            let mut i = 0;
            while i < PAGE_COUNT {
                assert!(PID_SEQ[i] == i as u32, "registry 行序与页号不一致");
                assert!(NAME_SEQ[i].len() > 1 && NAME_SEQ[i][0] != 0, "registry 页名为空");
                let mut j = 0;
                while j < i {
                    // 按 NUL 截断比较(state 里的数组尾部可能是补零)
                    let a = NAME_SEQ[i];
                    let b = NAME_SEQ[j];
                    let mut same = true;
                    let mut k = 0;
                    loop {
                        let ca = if k < a.len() { a[k] } else { 0 };
                        let cb = if k < b.len() { b[k] } else { 0 };
                        if ca != cb { same = false; break; }
                        if ca == 0 { break; }
                        k += 1;
                    }
                    assert!(!same, "registry 页名重复");
                    j += 1;
                }
                i += 1;
            }
        };
    };
}

pages! {
    Home       = 0,  PAGE_MAIN,     home::PAGE_INFO;
    AppMgr     = 1,  PAGE_DIR0,     appmgr::PAGE_APPMGR;
    TimeSet    = 2,  PAGE_TIME,     time::PAGE_TIMESET;
    DateMd     = 3,  PAGE_DATEMD,   time::PAGE_DATE_MD;
    Blur       = 4,  PAGE_BLUR,     beautify::PAGE_BLURP;
    FontList   = 5,  PAGE_TXTSUB,   beautify::PAGE_FONTLIST;
    IconSub    = 6,  PAGE_ICONSUB,  beautify::PAGE_ICONSUB;
    BlurLegacy = 7,  PAGE_BLUR_LEGACY, beautify::PAGE_BLUR_LEGACYP;
    WfList     = 8,  PAGE_WFACES,   watchface::PAGE_WF_LIST_8;
    WfShake    = 9,  PAGE_ROT,      watchface::PAGE_WF_LIST_9;
    WfMgr      = 10, PAGE_MGR,      watchface::PAGE_WF_MGMT;
    Bright     = 11, PAGE_BRIGHT,   settings::PAGE_BRIGHT;
    Cache      = 12, PAGE_CACHE,    settings::PAGE_CACHEP;
    Beautify   = 13, PAGE_FONTD,    beautify::PAGE_BEAUTIFY;
    Settings   = 14, PAGE_ABOUT,    settings::PAGE_SETTINGS;
    BlurZone   = 15, PAGE_ZONE,     beautify::PAGE_ZONEP;
    Vibrate    = 16, PAGE_VIB,      settings::PAGE_VIBP;
    Log        = 17, PAGE_LOG,      log::PAGE_LOG;
    About      = 18, PAGE_ABOUT2,   about::PAGE_ABOUT_PAGE;
    Fetch      = 19, PAGE_FETCH,    settings::PAGE_FETCHP;
    Licenses   = 20, PAGE_LICENSES, about::PAGE_LICENSESP;
    LicenseText= 21, PAGE_LICENSE_MPL, about::PAGE_LICENSEP;
    SysVer     = 22, PAGE_SYSVER,   system::PAGE_SYSVERP;
    Keyboard   = 23, PAGE_KEYBOARD, keyboard::PAGE_KEYBOARDP;
    AboutMod   = 24, PAGE_ABOUT_MOD, system::PAGE_ABOUT_MODP;
    SysText    = 25, PAGE_SYSTEXT,  system::PAGE_SYSTEXTP;
    DevName    = 26, PAGE_DEVNAME,  system::PAGE_DEVNAMEP;
    DevModel   = 27, PAGE_DEVMODEL, system::PAGE_DEVMODELP;
    AppList    = 28, PAGE_APPLIST,  beautify::PAGE_APPLIST;
    Detail     = 29, PAGE_DETAIL,   appmgr::PAGE_DETAILP;
    Batch      = 30, PAGE_BATCH,    appmgr::PAGE_BATCHP;
    Nsh        = 31, PAGE_NSH,      crate::terminal_ui::PAGE;
    Btpan      = 32, PAGE_BTPAN,    crate::btpan::PAGE_BTPAN_P;
    WebSrv     = 33, PAGE_WEBSRV,   crate::btpan::PAGE_WEBSRV_P;
    NetInfo    = 34, PAGE_NETINFO,  crate::btpan::PAGE_NETINFO_P;
    Exp        = 35, PAGE_EXP,      crate::btpan::PAGE_EXP_P;
    BtDiag     = 36, PAGE_BTDIAG,   crate::btpan::PAGE_DIAG_P;
    Debug      = 37, PAGE_DEBUG,    debug::PAGE_DEBUGP;
    Capture    = 38, PAGE_CAPTURE,  capture::PAGE_CAPTURE;
    Recordings = 39, PAGE_RECORDINGS, capture::PAGE_RECORDINGS;
    Screenshots= 40, PAGE_SCREENSHOTS, capture::PAGE_SCREENSHOTS;
    RecordSet  = 41, PAGE_RECORDSET, capture::PAGE_RECORDSET;
    ShotSet    = 42, PAGE_SHOTSET,  capture::PAGE_SHOTSET;
    AppIcon    = 43, PAGE_APP_ICON, app_icon::PAGE_APP_ICON;
}
