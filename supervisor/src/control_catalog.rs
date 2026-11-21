//! Android device-control catalogue. All gets/sets execute on the UI owner.
//! No ChaosConnect, permissions, transport, bridge or raw-memory setters exist.
use crate::*;
#[derive(Clone, Copy)]
struct Def {
    id: u16,
    group: &'static str,
    label: &'static str,
    kind: u8,
    min: u32,
    max: u32,
    options: &'static str,
}
static DEFS:&[Def]=&[
    Def{id:1,group:"显示与交互",label:"亮度（nit）",kind:1,min:1,max:2000,options:""},
    Def{id:2,group:"显示与交互",label:"自动亮度",kind:0,min:0,max:1,options:""},
    Def{id:3,group:"显示与交互",label:"震动",kind:0,min:0,max:1,options:""},
    Def{id:4,group:"显示与交互",label:"屏幕翻转（本次运行）",kind:0,min:0,max:1,options:""},
    Def{id:5,group:"显示与交互",label:"关闭遮盖熄屏",kind:0,min:0,max:1,options:""},
    Def{id:6,group:"显示与交互",label:"自动熄屏秒数",kind:1,min:1,max:2147483,options:""},
    Def{id:7,group:"显示与交互",label:"Chaos应用图标",kind:2,min:0,max:3,options:"原版|动态|暗色土豆|暗色图标"},
    Def{id:8,group:"桌面与美化",label:"在启动器显示Chaos",kind:0,min:0,max:1,options:""},
    Def{id:9,group:"显示与交互",label:"滑动列表震动",kind:0,min:0,max:1,options:""},
    Def{id:10,group:"显示与交互",label:"右滑返回震动",kind:0,min:0,max:1,options:""},
    Def{id:11,group:"显示与交互",label:"测试震动",kind:4,min:0,max:0,options:""},
    Def{id:20,group:"桌面与美化",label:"s541控制按钮",kind:0,min:0,max:1,options:""},
    Def{id:21,group:"桌面与美化",label:"缩放同RW6",kind:0,min:0,max:1,options:""},
    Def{id:22,group:"桌面与美化",label:"排距同RW6",kind:0,min:0,max:1,options:""},
    Def{id:23,group:"桌面与美化",label:"内缩同RW6",kind:0,min:0,max:1,options:""},
    Def{id:24,group:"桌面与美化",label:"蜂窝视图",kind:0,min:0,max:1,options:""},
    Def{id:25,group:"桌面与美化",label:"重启恢复字体和图标",kind:0,min:0,max:1,options:""},
    Def{id:26,group:"桌面与美化",label:"重启恢复模糊",kind:0,min:0,max:1,options:""},
    Def{id:27,group:"系统模糊",label:"模糊模式",kind:2,min:0,max:3,options:"系统背景|省电滤波|模糊V2|模糊V3"},
    Def{id:28,group:"系统模糊",label:"旧版背景模式",kind:2,min:0,max:2,options:"系统背景|静态表盘背景|完全透明背景"},
    Def{id:29,group:"系统模糊",label:"旧版实时模糊",kind:0,min:0,max:1,options:""},
    Def{id:30,group:"模糊区域",label:"启动器",kind:0,min:0,max:1,options:""},
    Def{id:31,group:"模糊区域",label:"控制中心",kind:0,min:0,max:1,options:""},
    Def{id:32,group:"模糊区域",label:"小部件",kind:0,min:0,max:1,options:""},
    Def{id:33,group:"模糊区域",label:"通知中心",kind:0,min:0,max:1,options:""},
    Def{id:34,group:"模糊区域",label:"控制中心按钮",kind:0,min:0,max:1,options:""},
    Def{id:35,group:"模糊区域",label:"启动器按钮",kind:0,min:0,max:1,options:""},
    Def{id:36,group:"系统模糊",label:"生成或更新表盘背景",kind:4,min:0,max:0,options:""},
    Def{id:40,group:"模糊V2/V3参数",label:"背景模糊强度",kind:1,min:0,max:40,options:""},
    Def{id:41,group:"模糊V2/V3参数",label:"背景亮度",kind:1,min:0,max:200,options:""},
    Def{id:42,group:"模糊V2/V3参数",label:"按钮亮度",kind:1,min:0,max:200,options:""},
    Def{id:43,group:"模糊V2/V3参数",label:"按钮玻璃不透明度",kind:1,min:0,max:100,options:""},
    Def{id:44,group:"模糊V2/V3参数",label:"按钮状态底色不透明度",kind:1,min:0,max:100,options:""},
    Def{id:45,group:"模糊V2/V3参数",label:"按钮背景采集间隔",kind:1,min:100,max:5000,options:""},
    Def{id:46,group:"模糊V2/V3参数",label:"按钮模糊强度",kind:1,min:0,max:40,options:""},
    Def{id:47,group:"模糊V2/V3参数",label:"背景不透明度",kind:1,min:0,max:100,options:""},
    Def{id:48,group:"模糊V2/V3参数",label:"背景饱和度",kind:1,min:0,max:200,options:""},
    Def{id:49,group:"模糊V2/V3参数",label:"背景对比度",kind:1,min:0,max:200,options:""},
    Def{id:50,group:"模糊V2/V3参数",label:"背景染色R",kind:1,min:0,max:255,options:""},
    Def{id:51,group:"模糊V2/V3参数",label:"背景染色G",kind:1,min:0,max:255,options:""},
    Def{id:52,group:"模糊V2/V3参数",label:"背景染色B",kind:1,min:0,max:255,options:""},
    Def{id:53,group:"模糊V2/V3参数",label:"背景染色比例",kind:1,min:0,max:100,options:""},
    Def{id:54,group:"模糊V2/V3参数",label:"按钮饱和度",kind:1,min:0,max:200,options:""},
    Def{id:55,group:"模糊V2/V3参数",label:"按钮对比度",kind:1,min:0,max:200,options:""},
    Def{id:56,group:"模糊V2/V3参数",label:"按钮染色R",kind:1,min:0,max:255,options:""},
    Def{id:57,group:"模糊V2/V3参数",label:"按钮染色G",kind:1,min:0,max:255,options:""},
    Def{id:58,group:"模糊V2/V3参数",label:"按钮染色B",kind:1,min:0,max:255,options:""},
    Def{id:59,group:"模糊V2/V3参数",label:"按钮染色比例",kind:1,min:0,max:100,options:""},
    Def{id:60,group:"模糊V2/V3参数",label:"背景采集间隔",kind:1,min:100,max:5000,options:""},
    Def{id:61,group:"模糊V2/V3参数",label:"恢复背景默认参数",kind:4,min:0,max:0,options:""},
    Def{id:62,group:"模糊V2/V3参数",label:"恢复按钮默认参数",kind:4,min:0,max:0,options:""},
    Def{id:70,group:"日志级别",label:"EMERG",kind:0,min:0,max:1,options:""},
    Def{id:71,group:"日志级别",label:"ALERT",kind:0,min:0,max:1,options:""},
    Def{id:72,group:"日志级别",label:"CRIT",kind:0,min:0,max:1,options:""},
    Def{id:73,group:"日志级别",label:"ERR",kind:0,min:0,max:1,options:""},
    Def{id:74,group:"日志级别",label:"WARNING",kind:0,min:0,max:1,options:""},
    Def{id:75,group:"日志级别",label:"NOTICE",kind:0,min:0,max:1,options:""},
    Def{id:76,group:"日志级别",label:"INFO",kind:0,min:0,max:1,options:""},
    Def{id:77,group:"日志级别",label:"DEBUG",kind:0,min:0,max:1,options:""},
    Def{id:78,group:"日志级别",label:"仅开启EMERG",kind:4,min:0,max:0,options:""},
    Def{id:80,group:"截图与录屏",label:"截图转换为PNG",kind:0,min:0,max:1,options:""},
    Def{id:81,group:"截图与录屏",label:"截图存储位置",kind:2,min:0,max:1,options:"离线日志|媒体目录"},
    Def{id:82,group:"截图与录屏",label:"录屏存储位置",kind:2,min:0,max:1,options:"离线日志|媒体目录"},
    Def{id:83,group:"截图与录屏",label:"录屏格式",kind:2,min:0,max:1,options:"GIF|APNG"},
    Def{id:84,group:"截图与录屏",label:"立即截图",kind:4,min:0,max:0,options:""},
    Def{id:85,group:"截图与录屏",label:"录屏",kind:0,min:0,max:1,options:""},
Def{id:86,group:"截图与录屏",label:"录屏和截图状态",kind:5,min:0,max:0,options:""},
    Def{id:90,group:"快应用动画",label:"快应用开启动画",kind:0,min:0,max:1,options:""},
    Def{id:91,group:"快应用动画",label:"快应用关闭动画",kind:0,min:0,max:1,options:""},
    Def{id:100,group:"触控点",label:"启用触控点",kind:0,min:0,max:1,options:""},
    Def{id:101,group:"触控点",label:"灵敏度",kind:1,min:25,max:300,options:""},
    Def{id:102,group:"触控点",label:"点击确认毫秒",kind:1,min:300,max:5000,options:""},
    Def{id:103,group:"触控点",label:"停留容差像素",kind:1,min:1,max:30,options:""},
    Def{id:104,group:"触控点",label:"切页暂停秒数",kind:1,min:0,max:10,options:""},
    Def{id:110,group:"灵动点",label:"启用灵动点",kind:0,min:0,max:1,options:""},
    Def{id:111,group:"灵动点",label:"圆点大小",kind:1,min:32,max:72,options:""},
    Def{id:112,group:"灵动点",label:"不透明度档位",kind:1,min:0,max:4,options:""},
    Def{id:113,group:"灵动点",label:"吸附边缘",kind:2,min:0,max:3,options:"左侧|右侧|顶部|底部"},
    Def{id:114,group:"灵动点",label:"沿边位置",kind:1,min:0,max:1000,options:""},
    Def{id:115,group:"灵动点",label:"快捷功能1",kind:2,min:0,max:8,options:"时间|亮度|截图|常亮|回到表盘|震动|自动亮度|录屏|屏幕翻转"},
    Def{id:116,group:"灵动点",label:"快捷功能2",kind:2,min:0,max:8,options:"时间|亮度|截图|常亮|回到表盘|震动|自动亮度|录屏|屏幕翻转"},
    Def{id:117,group:"灵动点",label:"快捷功能3",kind:2,min:0,max:8,options:"时间|亮度|截图|常亮|回到表盘|震动|自动亮度|录屏|屏幕翻转"},
    Def{id:130,group:"关于本机修改",label:"OS版本",kind:2,min:0,max:3,options:"3.101.043|4.103.022|91.78.13|自定义"},
    Def{id:131,group:"关于本机修改",label:"OS版本自定义",kind:3,min:0,max:31,options:""},
    Def{id:132,group:"关于本机修改",label:"系统文本",kind:2,min:0,max:2,options:"Xiaomi HyperOS|HarmonyOS|自定义"},
    Def{id:133,group:"关于本机修改",label:"系统文本自定义",kind:3,min:0,max:31,options:""},
    Def{id:134,group:"关于本机修改",label:"设备名称",kind:2,min:0,max:5,options:"小米手环10 Pro|小米手环10 Pro 陶瓷版|小米手环11 Pro|小米手环11 Pro 陶瓷版|三星note7|自定义"},
    Def{id:135,group:"关于本机修改",label:"设备名称自定义",kind:3,min:0,max:31,options:""},
    Def{id:136,group:"关于本机修改",label:"手环型号",kind:2,min:0,max:3,options:"M2551B1|M2553B1|M9178B13|自定义"},
    Def{id:137,group:"关于本机修改",label:"手环型号自定义",kind:3,min:0,max:31,options:""},
    Def{id:150,group:"字体与图标",label:"字体槽位",kind:2,min:0,max:8,options:"官方字体|字体1|字体2|字体3|字体4|字体5|字体6|字体7|字体8"},
    Def{id:151,group:"字体与图标",label:"桌面图标包",kind:2,min:0,max:8,options:"系统图标|图标包1|图标包2|图标包3|图标包4|图标包5|图标包6|图标包7|图标包8"},
    Def{id:152,group:"字体与图标",label:"删除字体槽位（确认后执行）",kind:1,min:1,max:8,options:""},
    Def{id:153,group:"字体与图标",label:"删除图标包（确认后执行）",kind:1,min:1,max:8,options:""},
    Def{id:160,group:"表盘管理",label:"摇一摇切换表盘",kind:0,min:0,max:1,options:""},
    Def{id:161,group:"表盘管理",label:"轮换包含内置表盘",kind:0,min:0,max:1,options:""},
    Def{id:162,group:"表盘管理",label:"列表显示内置表盘",kind:0,min:0,max:1,options:""},
    Def{id:163,group:"表盘管理",label:"切换表盘（12位ID）",kind:3,min:0,max:16,options:""},
    Def{id:164,group:"表盘管理",label:"轮换排除表盘（ID，空格分隔）",kind:3,min:0,max:240,options:""},
    Def{id:170,group:"日期与时间",label:"本地日期时间（YYYYMMDDhhmmss）",kind:3,min:0,max:14,options:""},
    Def{id:180,group:"系统维护",label:"清理系统缓存和日志",kind:4,min:0,max:0,options:""},
    Def{id:181,group:"系统维护",label:"刷新资源缓存",kind:4,min:0,max:0,options:""},
    Def{id:182,group:"系统维护",label:"重新应用字体和图标",kind:4,min:0,max:0,options:""},
    Def{id:183,group:"系统维护",label:"模块版本",kind:5,min:0,max:0,options:""},
    Def{id:191,group:"应用管理",label:"隐藏应用（包名）",kind:3,min:0,max:240,options:""},
    Def{id:192,group:"应用管理",label:"显示应用（包名）",kind:3,min:0,max:240,options:""},
    Def{id:193,group:"应用管理",label:"删除应用（包名，确认后执行）",kind:3,min:0,max:240,options:""},
    Def{id:194,group:"应用管理",label:"打开应用（包名）",kind:3,min:0,max:240,options:""},
    Def{id:195,group:"应用管理",label:"切换应用批量排除（包名）",kind:3,min:0,max:240,options:""},
    Def{id:196,group:"应用管理",label:"已安装应用数量",kind:5,min:0,max:0,options:""},
    Def{id:197,group:"应用管理",label:"深度隐藏",kind:0,min:0,max:1,options:""},
    Def{id:198,group:"应用管理",label:"重新应用隐藏",kind:4,min:0,max:0,options:""},
    Def{id:199,group:"应用管理",label:"应用管理状态",kind:5,min:0,max:0,options:""},
    Def{id:200,group:"网络服务",label:"启用btpan联网",kind:0,min:0,max:1,options:""},
    Def{id:201,group:"网络服务",label:"网页服务器",kind:0,min:0,max:1,options:""},
    Def{id:202,group:"网络服务",label:"网页服务器密码",kind:3,min:5,max:31,options:""},
    Def{id:203,group:"网络服务",label:"连接btpan（蓝牙MAC地址）",kind:3,min:17,max:17,options:""},
    Def{id:204,group:"网络服务",label:"断开btpan连接",kind:4,min:0,max:0,options:""},
    Def{id:205,group:"网络服务",label:"网络地址和状态",kind:5,min:0,max:0,options:""},
    Def{id:206,group:"网络服务",label:"扫描btpan设备",kind:4,min:0,max:0,options:""},
    Def{id:207,group:"网络服务",label:"确认btpan配对",kind:4,min:0,max:0,options:""},
    Def{id:208,group:"网络服务",label:"网页服务器账号和地址",kind:5,min:0,max:0,options:""},
];
pub(crate) type Value = [u8; 256];
fn bytes(input: &[u8]) -> Value {
    let mut out = [0; 256];
    let n = input.len().min(255);
    out[..n].copy_from_slice(&input[..n]);
    out
}
unsafe fn cstring(p: *const u8) -> Value {
    if p.is_null() {
        return [0; 256];
    }
    let mut out = [0; 256];
    let mut n = 0;
    while n < 255 && *p.add(n) != 0 {
        out[n] = *p.add(n);
        n += 1;
    }
    out
}
unsafe fn number(n: u32) -> Value {
    let mut out = [0; 256];
    W::new(out.as_mut_ptr(), 256).n(n).end();
    out
}
fn decimal(text: &[u8]) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    let mut n = 0u32;
    for &c in text {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((c - b'0') as u32)?;
    }
    Some(n)
}
unsafe fn value(id: u16) -> Value {
    if id >= 1000 {
        return number(0);
    }
    match id {
        1 => number(fw_api::brightness_get_target()),
        2 => number((fw_api::brightness_get_auto_adjustment() != 0) as u32),
        3 => number((!fw_api::vibration_muted()) as u32),
        4 => number((crate::screen_rotation::enabled()) as u32),
        5 => number((crate::touch_palm::enabled()) as u32),
        6 => number(crate::dimmer_timeout::seconds()),
        7 => number(crate::dynamic_icon::selected() as u32),
        8 => number((crate::launcher_entry::enabled()) as u32),
        20 => number((crate::s541_launcher::enabled()) as u32),
        21 => number(crate::desktop_scale::lens_on() as u32),
        22 => number(crate::desktop_scale::pitch_on() as u32),
        23 => number(crate::desktop_scale::inset_on() as u32),
        24 => number(crate::honeycomb::enabled() as u32),
        25 => number((crate::beautify_restore::enabled()) as u32),
        26 => number((crate::res_hook::held()) as u32),
        27 => number(if crate::corona_blur::enabled() {
            crate::corona_blur::algo() + 1
        } else {
            0
        }),
        28 => number(if crate::background_home::mode() == 3 {
            2
        } else {
            crate::background_home::mode()
        }),
        29 => number(crate::background_home::realtime_on()),
        30 => number(crate::blur_zone::on(0) as u32),
        31 => number(crate::blur_zone::on(1) as u32),
        32 => number(crate::blur_zone::on(2) as u32),
        33 => number(crate::blur_zone::on(3) as u32),
        34 => number(crate::blur_zone::on(4) as u32),
        35 => number(crate::blur_zone::on(5) as u32),
        40 => number(crate::blur_params::get(0)),
        41 => number(crate::blur_params::get(1)),
        42 => number(crate::blur_params::get(2)),
        43 => number(crate::blur_params::get(3)),
        44 => number(crate::blur_params::get(4)),
        45 => number(crate::blur_params::get(5)),
        46 => number(crate::blur_params::get(6)),
        47 => number(crate::blur_params::get(7)),
        48 => number(crate::blur_params::get(8)),
        49 => number(crate::blur_params::get(9)),
        50 => number(crate::blur_params::get(10)),
        51 => number(crate::blur_params::get(11)),
        52 => number(crate::blur_params::get(12)),
        53 => number(crate::blur_params::get(13)),
        54 => number(crate::blur_params::get(14)),
        55 => number(crate::blur_params::get(15)),
        56 => number(crate::blur_params::get(16)),
        57 => number(crate::blur_params::get(17)),
        58 => number(crate::blur_params::get(18)),
        59 => number(crate::blur_params::get(19)),
        60 => number(crate::blur_params::get(20)),
        61 => number(0),
        62 => number(0),
        70 => number(((crate::log_level::mask() >> 0) & 1) as u32),
        71 => number(((crate::log_level::mask() >> 1) & 1) as u32),
        72 => number(((crate::log_level::mask() >> 2) & 1) as u32),
        73 => number(((crate::log_level::mask() >> 3) & 1) as u32),
        74 => number(((crate::log_level::mask() >> 4) & 1) as u32),
        75 => number(((crate::log_level::mask() >> 5) & 1) as u32),
        76 => number(((crate::log_level::mask() >> 6) & 1) as u32),
        77 => number(((crate::log_level::mask() >> 7) & 1) as u32),
        78 => number(0),
        80 => number(crate::capture_settings::get().screenshot_png as u32),
        81 => number(crate::capture_settings::get().screenshot as u32),
        82 => number(crate::capture_settings::get().recording as u32),
        83 => number(crate::capture_settings::get().format as u32),
        84 => number(0),
        85 => number((crate::capture::state() != crate::capture::State::Idle) as u32),
        90 => number((crate::quickapp_animation::enabled()) as u32),
        91 => number((crate::quickapp_animation::close_enabled()) as u32),
        100 => number((crate::gyro_pointer::enabled()) as u32),
        101 => number(crate::gyro_pointer::sensitivity()),
        102 => number(crate::gyro_pointer::confirm_ms()),
        103 => number(crate::gyro_pointer::tolerance_px()),
        104 => number(crate::gyro_pointer::page_pause_s()),
        110 => number(crate::floating_ball::config().enabled as u32),
        111 => number(crate::floating_ball::config().size as u32),
        112 => number(crate::floating_ball::config().opacity as u32),
        113 => number(crate::floating_ball::config().dock.edge as u32),
        114 => number(crate::floating_ball::config().dock.offset as u32),
        115 => number(crate::floating_ball::config().slots[0] as u32),
        116 => number(crate::floating_ball::config().slots[1] as u32),
        117 => number(crate::floating_ball::config().slots[2] as u32),
        130 => number(crate::system_version::selected() as u32),
        131 => cstring(crate::system_version::custom_display()),
        132 => number(crate::system_text::selected() as u32),
        133 => cstring(crate::system_text::custom_display()),
        134 => number(crate::device_name::selected() as u32),
        135 => cstring(crate::device_name::custom_display()),
        136 => number(crate::device_model::selected() as u32),
        137 => cstring(crate::device_model::custom_display()),
        150 => number(crate::font_list::remote_selected()),
        151 => number(crate::icon_apply::cur_pack()),
        152 => number(0),
        153 => number(0),
        160 => number((st_rd!(SHAKE_EN) != 0) as u32),
        161 => number(st_rd!(WF_SEL_BUILTIN)),
        162 => number(st_rd!(WF_SHOW_BUILTIN)),
        163 => bytes(b""),
        164 => rotation_exclusions(),
        170 => time_value(),
        180 => number(0),
        181 => number(0),
        182 => number(0),
        183 => bytes(crate::VERSION_TEXT.as_bytes()),
        191 => bytes(b""),
        192 => bytes(b""),
        193 => bytes(b""),
        194 => bytes(b""),
        195 => bytes(b""),
        196 => number(crate::appmgr::count() as u32),
        200 => number(crate::btpan::remote_pan_enabled() as u32),
        201 => number(crate::btpan::remote_web_enabled() as u32),
        202 => bytes(b""),
        203 => bytes(b""),
        204 => number(0),
        205 => crate::btpan::remote_info(),
        9 => number(background_home::swipe_list_on()),
        10 => number(background_home::swipe_back_on()),
        11 => number(0),
        36 => number(0),
        197 => number(appmgr::deep_on() as u32),
        198 => number(0),
        199 => bytes(appmgr::msg()),
        206 => number(0),
        207 => number(0),
        208 => btpan::remote_credentials(),
        86 => capture_status(),
        _ => [0; 256],
    }
}
unsafe fn write(id: u16, text: &[u8]) -> bool {
    if id >= 1000 {
        return write_dynamic(id, text);
    }
    let Some(d) = DEFS.iter().find(|d| d.id == id) else {
        return false;
    };
    if d.kind == 5
        || (d.kind == 4 && !text.is_empty())
        || text.len() > 240
        || core::str::from_utf8(text).is_err()
        || text.contains(&0)
    {
        return false;
    }
    let n = if d.kind <= 2 {
        let Some(n) = decimal(text) else {
            return false;
        };
        if n < d.min || n > d.max {
            return false;
        }
        n
    } else {
        0
    };
    if d.kind == 3 && (text.len() < d.min as usize || text.len() > d.max as usize) {
        return false;
    }
    let mut cstr = [0; 256];
    cstr[..text.len()].copy_from_slice(text);
    match id {
        1 => {
            fw_api::brightness_apply_raw(n);
            true
        }
        2 => {
            if (fw_api::brightness_get_auto_adjustment() != 0) != (n != 0) {
                let target = fw_api::brightness_get_target();
                fw_api::brightness_set_auto_adjustment(n);
                if n == 0 && target > 0 {
                    fw_api::brightness_apply_raw(target);
                };
            }
            true
        }
        3 => {
            if (!fw_api::vibration_muted()) != (n != 0) {
                fw_api::vibration_set_muted(n == 0);
            }
            true
        }
        4 => {
            if (crate::screen_rotation::enabled()) != (n != 0) {
                crate::screen_rotation::toggle();
            }
            true
        }
        5 => {
            if (crate::touch_palm::enabled()) != (n != 0) {
                crate::touch_palm::toggle();
            }
            true
        }
        6 => crate::dimmer_timeout::save(cstr.as_ptr()),
        7 => crate::dynamic_icon::select(n as u8),
        8 => {
            if (crate::launcher_entry::enabled()) != (n != 0) {
                crate::launcher_entry::set_enabled(n != 0);
            }
            true
        }
        20 => {
            if (crate::s541_launcher::enabled()) != (n != 0) {
                crate::s541_launcher::toggle();
            }
            true
        }
        21 => {
            crate::desktop_scale::set_lens(n != 0);
            true
        }
        22 => {
            crate::desktop_scale::set_pitch(n != 0);
            true
        }
        23 => {
            crate::desktop_scale::set_inset(n != 0);
            true
        }
        24 => {
            crate::honeycomb::set_enabled(n != 0);
            true
        }
        25 => {
            if (crate::beautify_restore::enabled()) != (n != 0) {
                crate::beautify_restore::request_toggle();
            }
            true
        }
        26 => {
            if (crate::res_hook::held()) != (n != 0) {
                crate::res_hook::request_toggle();
            }
            true
        }
        27 => {
            if n == 0 {
                crate::corona_blur::set_enabled(false);
                crate::background_home::request(0);
            } else {
                crate::corona_blur::set_algo(n - 1);
            }
            true
        }
        28 => {
            crate::background_home::request(if n == 2 { 3 } else { n });
            true
        }
        29 => {
            crate::background_home::set_realtime(n);
            true
        }
        30 => {
            if crate::blur_zone::set(0, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        31 => {
            if crate::blur_zone::set(1, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        32 => {
            if crate::blur_zone::set(2, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        33 => {
            if crate::blur_zone::set(3, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        34 => {
            if crate::blur_zone::set(4, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        35 => {
            if crate::blur_zone::set(5, n != 0) {
                crate::background_home::remount();
            }
            true
        }
        40 => crate::blur_params::set(0, n),
        41 => crate::blur_params::set(1, n),
        42 => crate::blur_params::set(2, n),
        43 => crate::blur_params::set(3, n),
        44 => crate::blur_params::set(4, n),
        45 => crate::blur_params::set(5, n),
        46 => crate::blur_params::set(6, n),
        47 => crate::blur_params::set(7, n),
        48 => crate::blur_params::set(8, n),
        49 => crate::blur_params::set(9, n),
        50 => crate::blur_params::set(10, n),
        51 => crate::blur_params::set(11, n),
        52 => crate::blur_params::set(12, n),
        53 => crate::blur_params::set(13, n),
        54 => crate::blur_params::set(14, n),
        55 => crate::blur_params::set(15, n),
        56 => crate::blur_params::set(16, n),
        57 => crate::blur_params::set(17, n),
        58 => crate::blur_params::set(18, n),
        59 => crate::blur_params::set(19, n),
        60 => crate::blur_params::set(20, n),
        61 => crate::blur_params::reset_group(false),
        62 => crate::blur_params::reset_group(true),
        70 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 0)
            } else {
                old & !(1 << 0)
            });
            true
        }
        71 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 1)
            } else {
                old & !(1 << 1)
            });
            true
        }
        72 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 2)
            } else {
                old & !(1 << 2)
            });
            true
        }
        73 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 3)
            } else {
                old & !(1 << 3)
            });
            true
        }
        74 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 4)
            } else {
                old & !(1 << 4)
            });
            true
        }
        75 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 5)
            } else {
                old & !(1 << 5)
            });
            true
        }
        76 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 6)
            } else {
                old & !(1 << 6)
            });
            true
        }
        77 => {
            let old = crate::log_level::mask();
            crate::log_level::set(if n != 0 {
                old | (1 << 7)
            } else {
                old & !(1 << 7)
            });
            true
        }
        78 => {
            crate::log_level::set(1);
            true
        }
        80 => {
            let mut s = crate::capture_settings::get();
            s.screenshot_png = n != 0;
            crate::capture_settings::set(s).is_ok()
        }
        81 => {
            let mut s = crate::capture_settings::get();
            s.screenshot = if n == 0 {
                crate::capture_format::Storage::OfflineLog
            } else {
                crate::capture_format::Storage::Media
            };
            crate::capture_settings::set(s).is_ok()
        }
        82 => {
            let mut s = crate::capture_settings::get();
            s.recording = if n == 0 {
                crate::capture_format::Storage::OfflineLog
            } else {
                crate::capture_format::Storage::Media
            };
            crate::capture_settings::set(s).is_ok()
        }
        83 => {
            let mut s = crate::capture_settings::get();
            s.format = if n == 0 {
                crate::capture_format::VideoFormat::Gif
            } else {
                crate::capture_format::VideoFormat::Apng
            };
            crate::capture_settings::set(s).is_ok()
        }
        84 => crate::capture::request_shot_now().is_ok(),
        85 => {
            if (crate::capture::state() != crate::capture::State::Idle) != (n != 0) {
                crate::capture::toggle_recording();
            }
            true
        }
        90 => {
            if (crate::quickapp_animation::enabled()) != (n != 0) {
                crate::quickapp_animation::toggle();
            }
            true
        }
        91 => {
            if (crate::quickapp_animation::close_enabled()) != (n != 0) {
                crate::quickapp_animation::toggle_close();
            }
            true
        }
        100 => {
            if (crate::gyro_pointer::enabled()) != (n != 0) {
                crate::gyro_pointer::toggle();
            }
            true
        }
        101 => crate::gyro_pointer::set_parameter(0, text),
        102 => crate::gyro_pointer::set_parameter(1, text),
        103 => crate::gyro_pointer::set_parameter(2, text),
        104 => crate::gyro_pointer::set_parameter(3, text),
        110 => {
            let mut c = crate::floating_ball::config();
            c.enabled = n != 0;
            crate::floating_ball::update(c)
        }
        111 => {
            let mut c = crate::floating_ball::config();
            c.size = n as _;
            crate::floating_ball::update(c)
        }
        112 => {
            let mut c = crate::floating_ball::config();
            c.opacity = n as _;
            crate::floating_ball::update(c)
        }
        113 => {
            let mut c = crate::floating_ball::config();
            c.dock.edge = crate::floating_ball::model::Edge::ALL[n as usize];
            crate::floating_ball::update(c)
        }
        114 => {
            let mut c = crate::floating_ball::config();
            c.dock.offset = n as _;
            crate::floating_ball::update(c)
        }
        115 => {
            let mut c = crate::floating_ball::config();
            let action = crate::floating_ball::model::Action::ALL[n as usize];
            if let Some(j) = c.slots.iter().position(|a| *a == action) {
                c.slots.swap(0, j);
            } else {
                c.slots[0] = action;
            }
            crate::floating_ball::update(c)
        }
        116 => {
            let mut c = crate::floating_ball::config();
            let action = crate::floating_ball::model::Action::ALL[n as usize];
            if let Some(j) = c.slots.iter().position(|a| *a == action) {
                c.slots.swap(1, j);
            } else {
                c.slots[1] = action;
            }
            crate::floating_ball::update(c)
        }
        117 => {
            let mut c = crate::floating_ball::config();
            let action = crate::floating_ball::model::Action::ALL[n as usize];
            if let Some(j) = c.slots.iter().position(|a| *a == action) {
                c.slots.swap(2, j);
            } else {
                c.slots[2] = action;
            }
            crate::floating_ball::update(c)
        }
        130 => {
            crate::system_version::choose(n as usize);
            true
        }
        131 => crate::system_version::save_custom(cstr.as_ptr()),
        132 => {
            crate::system_text::choose(n as usize);
            true
        }
        133 => crate::system_text::save_custom(cstr.as_ptr()),
        134 => {
            crate::device_name::choose(n as usize);
            true
        }
        135 => crate::device_name::save_custom(cstr.as_ptr()),
        136 => {
            crate::device_model::choose(n as usize);
            true
        }
        137 => crate::device_model::save_custom(cstr.as_ptr()),
        150 => crate::font_list::remote_select(n),
        151 => crate::icon_apply::remote_select(n),
        152 => crate::font_list::remote_delete(n),
        153 => crate::icon_apply::remote_delete(n),
        160 => {
            if (st_rd!(SHAKE_EN) != 0) != (n != 0) {
                st_wr!(SHAKE_EN, n);
                crate::watchface::persist_shake_enabled(n);
            }
            true
        }
        161 => {
            st_wr!(WF_SEL_BUILTIN, n);
            true
        }
        162 => {
            st_wr!(WF_SHOW_BUILTIN, n);
            true
        }
        163 => crate::watchface::remote_select(text),
        164 => set_rotation_exclusions(text),
        170 => set_time(text),
        180 => {
            crate::cacheclean::clean();
            true
        }
        181 => {
            crate::res_hook::refresh_cache();
            true
        }
        182 => {
            crate::font_apply::request();
            crate::res_hook::refresh_cache();
            true
        }
        183 => false,
        191 => crate::appmgr::remote_action(text, 0),
        192 => crate::appmgr::remote_action(text, 1),
        193 => crate::appmgr::remote_action(text, 2),
        194 => crate::appmgr::remote_action(text, 3),
        195 => crate::appmgr::remote_action(text, 4),
        196 => false,
        200 => {
            crate::btpan::remote_pan(n != 0);
            true
        }
        201 => {
            crate::btpan::remote_web(n != 0);
            true
        }
        202 => crate::btpan::save_web_password(cstr.as_ptr()),
        203 => crate::btpan::remote_connect(text),
        204 => {
            crate::btpan::remote_disconnect();
            true
        }
        205 => false,
        9 => {
            background_home::set_swipe_vib(true, n);
            true
        }
        10 => {
            background_home::set_swipe_vib(false, n);
            true
        }
        11 => {
            background_home::vib_test_req();
            true
        }
        36 => {
            background_home::generate();
            true
        }
        197 => {
            if appmgr::deep_on() != (n != 0) {
                appmgr::toggle_apply_mode();
                appmgr::do_mode_apply();
            }
            appmgr::deep_on() == (n != 0)
        }
        198 => {
            appmgr::request_reapply();
            appmgr::do_reapply();
            true
        }
        199 => false,
        206 => btpan::remote_scan(),
        207 => btpan::remote_pair_confirm(),
        208 => false,
        86 => false,
        _ => false,
    }
}

unsafe fn rotation_exclusions() -> Value {
    let mut out = [0; 256];
    let mut w = W::new(out.as_mut_ptr(), 256);
    for i in 0..st_rd!(WF_DESEL_CNT).min(24) as usize {
        if i != 0 {
            w.c(b' ');
        }
        w.s(&cstring((WF_DESEL[i] + 8) as *const u8)[..12]);
    }
    w.end();
    out
}
unsafe fn set_rotation_exclusions(text: &[u8]) -> bool {
    let win = st_rd!(WF_WIN);
    watchface::load_watchfaces(1);
    st_wr!(WF_WIN, win);
    let mut nodes = [0u32; 24];
    let mut count = 0;
    for id in text.split(|c| *c == b' ').filter(|s| !s.is_empty()) {
        if id.len() != 12 || count == 24 {
            return false;
        }
        let Some(i) = (0..st_rd!(WF_COUNT).min(24) as usize).find(|i| &WF_IDS[*i][..12] == id)
        else {
            return false;
        };
        if !nodes[..count].contains(&WF_NODES[i]) {
            nodes[count] = WF_NODES[i];
            count += 1;
        }
    }
    let mut old = [0; 24];
    let n = st_rd!(WF_DESEL_CNT).min(24) as usize;
    old[..n].copy_from_slice(&WF_DESEL[..n]);
    for node in old[..n].iter() {
        watchface::desel_set(*node, false);
    }
    for node in nodes[..count].iter() {
        watchface::desel_set(*node, true);
    }
    true
}
unsafe fn time_value() -> Value {
    let mut out = [0; 256];
    if let Some(t) = time_set::now_tm() {
        let nums = [
            (t[5] + 1900) as u32,
            (t[4] + 1) as u32,
            t[3] as u32,
            t[2] as u32,
            t[1] as u32,
            t[0] as u32,
        ];
        let mut w = W::new(out.as_mut_ptr(), 256);
        for (i, n) in nums.iter().enumerate() {
            if i > 0 && *n < 10 {
                w.c(b'0');
            }
            w.n(*n);
        }
        w.end();
    }
    out
}
unsafe fn set_time(text: &[u8]) -> bool {
    if text.len() != 14 {
        return false;
    }
    let Some(y) = decimal(&text[..4]) else {
        return false;
    };
    let mut v = [0; 5];
    for i in 0..5 {
        let Some(n) = decimal(&text[4 + i * 2..6 + i * 2]) else {
            return false;
        };
        v[i] = n;
    }
    time_set::set(y, v[0], v[1], v[2], v[3], v[4]) == 0
}

// A bounded mailbox bridges the CCN1 owner and LVGL owner. Neither side spins;
// firmware APIs are only invoked from tick(), after a second authorization check.
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
struct Mail {
    generation: u32,
    token: u32,
    op: u8,
    pending: bool,
    input: [u8; 244],
    len: usize,
    out: [u8; 2304],
    used: usize,
}
struct Shared(UnsafeCell<Mail>);
unsafe impl Sync for Shared {}
static LOCK: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(1);
static SERIAL: AtomicU32 = AtomicU32::new(1);
static LAST_REQUEST: AtomicU32 = AtomicU32::new(0);
static MAIL: Shared = Shared(UnsafeCell::new(Mail {
    generation: 0,
    token: 0,
    op: 0,
    pending: false,
    input: [0; 244],
    len: 0,
    out: [0; 2304],
    used: 0,
}));
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        LOCK.store(false, Ordering::Release);
    }
}
fn lock() -> Option<Guard> {
    LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .ok()
        .map(|_| Guard)
}
fn allowed() -> bool {
    let s = connect_permission::state();
    extern "C" {
        fn chaos_bridge_enabled() -> u32;
    }
    s & 1 != 0
        && s & (1 << 5) != 0
        && (s >> 6) & 7 == 2
        && connect_permission::android_policy() != connect_permission::DENY
        && connect_permission::android_mask() & connect_permission::CONTROL != 0
        && unsafe { chaos_bridge_enabled() != 0 }
}
#[no_mangle]
pub extern "C" fn chaos_control_reset() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    LAST_REQUEST.store(0, Ordering::Release);
}
#[no_mangle]
pub unsafe extern "C" fn chaos_control_submit(
    op: u32,
    p: *const u8,
    n: u32,
    token: *mut u32,
) -> u32 {
    if !allowed() {
        return 3;
    }
    if !matches!(op, 0x70 | 0x71) || n > 244 || p.is_null() || token.is_null() {
        return 1;
    }
    let data = core::slice::from_raw_parts(p, n as usize);
    if (op == 0x70 && n != 2)
        || (op == 0x71 && (n < 4 || u16::from_le_bytes([data[2], data[3]]) as u32 != n - 4))
    {
        return 1;
    }
    let Some(_guard) = lock() else {
        return 8;
    };
    let m = &mut *MAIL.0.get();
    let generation = GENERATION.load(Ordering::Acquire);
    if m.generation == generation && m.pending {
        return 8;
    }
    let t = SERIAL.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    if t == 0 {
        return 8;
    }
    LAST_REQUEST.store(fw_api::capture_now_ms(), Ordering::Release);
    m.generation = generation;
    m.token = t;
    m.op = op as u8;
    m.pending = true;
    m.len = n as usize;
    m.used = 0;
    m.input[..m.len].copy_from_slice(data);
    *token = t;
    0
}
#[no_mangle]
pub unsafe extern "C" fn chaos_control_result(token: u32, out: *mut u8, cap: u32) -> i32 {
    if !allowed() {
        return -3;
    }
    if out.is_null() || cap < 2 {
        return -1;
    }
    let Some(_guard) = lock() else {
        return -8;
    };
    let m = &*MAIL.0.get();
    if token == 0 || m.token != token || m.generation != GENERATION.load(Ordering::Acquire) {
        return -11;
    }
    if m.pending {
        *out = 0;
        *out.add(1) = 0;
        return 2;
    }
    if m.used > cap as usize {
        return -13;
    }
    core::ptr::copy_nonoverlapping(m.out.as_ptr(), out, m.used);
    m.used as i32
}
struct Encoder<'a> {
    buf: &'a mut [u8],
    n: usize,
    failed: bool,
}
impl Encoder<'_> {
    fn put(&mut self, b: &[u8]) -> bool {
        if self.n + b.len() > self.buf.len() {
            self.failed = true;
            return false;
        }
        self.buf[self.n..self.n + b.len()].copy_from_slice(b);
        self.n += b.len();
        true
    }
    fn u16(&mut self, n: u16) -> bool {
        self.put(&n.to_le_bytes())
    }
    fn text(&mut self, s: &[u8]) -> bool {
        self.u16(s.len() as u16) && self.put(s)
    }
}
fn vbytes(v: &Value) -> &[u8] {
    let mut n = v.iter().position(|c| *c == 0).unwrap_or(v.len());
    while n > 0 && core::str::from_utf8(&v[..n]).is_err() {
        n -= 1;
    }
    &v[..n]
}
pub(crate) unsafe fn busy() -> bool {
    let last = LAST_REQUEST.load(Ordering::Acquire);
    last != 0 && allowed() && fw_api::capture_now_ms().wrapping_sub(last) < 1000
}
pub(crate) unsafe fn tick() {
    if !watchface::control_ready() {
        return;
    }
    let Some(_guard) = lock() else {
        return;
    };
    let m = &mut *MAIL.0.get();
    if !m.pending {
        return;
    }
    if m.generation != GENERATION.load(Ordering::Acquire) {
        m.pending = false;
        return;
    }
    m.out[0] = 1;
    m.out[1] = 0;
    m.used = 2;
    if !allowed() {
        m.out[1] = 3;
        m.pending = false;
        return;
    }
    if st_rd!(SIM_LITE) != 0 {
        m.out[1] = 12;
        m.pending = false;
        return;
    }
    if m.op == 0x70 {
        let offset = u16::from_le_bytes([m.input[0], m.input[1]]) as usize;
        if offset == 0 {
            snapshot_items(m.generation);
        }
        let total = DEFS.len() + APP_COUNT + FACE_COUNT;
        if offset > total {
            m.out[1] = 1;
        } else {
            let mut end = offset;
            let mut e = Encoder {
                buf: &mut m.out,
                n: 2,
                failed: false,
            };
            e.put(&[1]);
            e.u16(total as u16);
            e.u16(offset as u16);
            e.put(&[0]);
            for index in offset..(offset + 8).min(total) {
                let before = e.n;
                let d = definition(index);
                let v = value(d.id);
                e.u16(d.id);
                e.put(&[d.kind]);
                e.put(&d.min.to_le_bytes());
                e.put(&d.max.to_le_bytes());
                e.text(d.group.as_bytes());
                e.text(d.label.as_bytes());
                e.text(vbytes(&v));
                e.text(d.options.as_bytes());
                if e.failed {
                    e.failed = false;
                    e.n = before;
                    break;
                }
                end = index + 1;
            }
            e.buf[5..7].copy_from_slice(&(end as u16).to_le_bytes());
            e.buf[7] = (end - offset) as u8;
            if end == offset && offset < total {
                m.used = 2;
                m.out[1] = 13;
            } else {
                m.used = e.n;
            }
        }
    } else {
        let id = u16::from_le_bytes([m.input[0], m.input[1]]);
        let win = st_rd!(WF_WIN);
        let ok = write(id, &m.input[4..m.len]);
        watchface::remote_restore_view(win);
        if ok && st_rd!(APP_FG) != 0 && st_rd!(FG_PAGE) <= MAX_PID as u32 {
            crate::ui::render_req(st_rd!(FG_PAGE));
        }
        if ok {
            let v = value(id);
            let mut e = Encoder {
                buf: &mut m.out,
                n: 2,
                failed: false,
            };
            e.u16(id);
            e.text(vbytes(&v));
            if e.failed {
                m.used = 2;
                m.out[1] = 13;
            } else {
                m.used = e.n;
            }
            if let Some(d) = DEFS.iter().find(|d| d.id == id) {
                let mut text = [0; 128];
                let mut w = W::new(text.as_mut_ptr(), 128);
                w.s("Android修改Chaos：".as_bytes()).s(d.label.as_bytes());
                w.end();
                connect_log::line(
                    core::str::from_utf8(vbytes(&bytes(&text))).unwrap_or("Android修改Chaos设置"),
                );
            }
        } else {
            m.out[1] = 9;
        }
    }
    m.pending = false;
}

static mut FONT_OPTIONS: Value = [0; 256];
static mut ICON_OPTIONS: Value = [0; 256];
static mut APP_COUNT: usize = 0;
static mut FACE_COUNT: usize = 0;
static mut ITEMS_GENERATION: u32 = 0;
static mut APP_PACKAGES: [[u8; 44]; 64] = [[0; 44]; 64];
static mut APP_LABELS: [[u8; 96]; 64] = [[0; 96]; 64];
static mut FACE_IDS: [[u8; 16]; 24] = [[0; 16]; 24];
static mut FACE_LABELS: [[u8; 96]; 24] = [[0; 96]; 24];
unsafe fn snapshot_items(generation: u32) {
    FONT_OPTIONS = font_list::remote_options();
    ICON_OPTIONS = icon_apply::remote_options();
    appmgr::reload();
    APP_COUNT = appmgr::count().min(64);
    for i in 0..APP_COUNT {
        APP_PACKAGES[i] = [0; 44];
        let pkg = cstring(appmgr::pkg_ptr(i));
        APP_PACKAGES[i][..vbytes(&pkg).len()].copy_from_slice(vbytes(&pkg));
        let name = cstring(appmgr::entry_primary(i));
        let mut w = W::new(APP_LABELS[i].as_mut_ptr(), 96);
        w.s(vbytes(&name))
            .c(b'\n')
            .s(vbytes(&pkg))
            .s(if appmgr::visible(i) {
                "（显示）".as_bytes()
            } else {
                "（隐藏）".as_bytes()
            });
        w.end();
    }
    let win = st_rd!(WF_WIN);
    watchface::load_watchfaces(1);
    st_wr!(WF_WIN, win);
    FACE_COUNT = st_rd!(WF_COUNT).min(24) as usize;
    for i in 0..FACE_COUNT {
        FACE_IDS[i] = WF_IDS[i];
        let name = cstring(WF_NAMES[i].as_ptr());
        let mut w = W::new(FACE_LABELS[i].as_mut_ptr(), 96);
        w.s(vbytes(&name))
            .c(b'\n')
            .s(&FACE_IDS[i][..12])
            .s(if watchface::desel_has(WF_NODES[i]) {
                "（已排除轮换）".as_bytes()
            } else {
                "（参与轮换）".as_bytes()
            });
        w.end();
    }
    watchface::remote_restore_view(win);
    ITEMS_GENERATION = generation;
}
unsafe fn label(p: *const u8, n: usize) -> &'static str {
    let text = core::slice::from_raw_parts(p, n);
    let mut end = text.iter().position(|c| *c == 0).unwrap_or(n);
    while end > 0 && core::str::from_utf8(&text[..end]).is_err() {
        end -= 1;
    }
    core::str::from_utf8_unchecked(&text[..end])
}
unsafe fn definition(index: usize) -> Def {
    if index < DEFS.len() {
        let mut d = DEFS[index];
        if d.id == 150 {
            d.options = label(FONT_OPTIONS.as_ptr(), 256);
        }
        if d.id == 151 {
            d.options = label(ICON_OPTIONS.as_ptr(), 256);
        }
        return d;
    }
    let i = index - DEFS.len();
    if i < APP_COUNT {
        Def {
            id: 1000 + i as u16,
            group: "已安装应用",
            label: label(APP_LABELS[i].as_ptr(), 96),
            kind: 2,
            min: 0,
            max: 5,
            options: "保持不变|打开应用|隐藏应用|显示应用|切换批量排除|卸载应用",
        }
    } else {
        let j = i - APP_COUNT;
        Def {
            id: 2000 + j as u16,
            group: "已安装表盘",
            label: label(FACE_LABELS[j].as_ptr(), 96),
            kind: 2,
            min: 0,
            max: 3,
            options: "保持不变|切换表盘|加入摇一摇轮换|移出摇一摇轮换",
        }
    }
}
unsafe fn write_dynamic(id: u16, text: &[u8]) -> bool {
    if ITEMS_GENERATION != GENERATION.load(Ordering::Acquire) {
        return false;
    }
    let Some(n) = decimal(text) else {
        return false;
    };
    if id >= 1000 && ((id - 1000) as usize) < APP_COUNT {
        let i = (id - 1000) as usize;
        if n == 0 {
            return true;
        }
        let op = match n {
            1 => 3,
            2 => 0,
            3 => 1,
            4 => 4,
            5 => 2,
            _ => return false,
        };
        let p = APP_PACKAGES[i];
        let end = p.iter().position(|c| *c == 0).unwrap_or(44);
        appmgr::remote_action(&p[..end], op)
    } else if id >= 2000 && ((id - 2000) as usize) < FACE_COUNT {
        let i = (id - 2000) as usize;
        let id = FACE_IDS[i];
        if n == 0 {
            return true;
        }
        if n == 1 {
            return watchface::remote_select(&id[..12]);
        }
        if n > 3 {
            return false;
        }
        let win = st_rd!(WF_WIN);
        watchface::load_watchfaces(1);
        st_wr!(WF_WIN, win);
        for j in 0..st_rd!(WF_COUNT).min(24) as usize {
            if WF_IDS[j] == id {
                watchface::desel_set(WF_NODES[j], n == 3);
                return true;
            }
        }
        false
    } else {
        false
    }
}
unsafe fn capture_status() -> Value {
    let mut out = [0; 256];
    let mut w = W::new(out.as_mut_ptr(), 256);
    w.s(if capture::state() == capture::State::Idle {
        "未录制".as_bytes()
    } else {
        "录制中".as_bytes()
    })
    .s(" 帧数 ".as_bytes())
    .n(capture::frames())
    .s(" 错误码 ".as_bytes())
    .n(capture::error().unsigned_abs());
    w.end();
    out
}
