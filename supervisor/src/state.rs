// 全部可变状态与静态常量的唯一声明处。其余模块用 `use crate::state::*;` 访问。
// 这样阅读时状态一目了然, 不必在两千行里翻找。

// ===== 固件地址 (3.101.043, 由静态核对转译; 推断待验, 未真机验证) =====
// 地址来源: 对固件镜像的静态核对。
// 这里只留**业务代码直接引用**的那几个地址; UI/字体/消息框那一批一律走 `fw_api/*`
// 的包装(代码约定: 业务代码禁止 transmute 地址), 完整清单与依据都在 `fw_api/*` 一侧 ——
// 同一个地址不留两个真源。
// 注册链 (symbols: app_registry / launcher)
pub const FW_REGISTER_DRIVER: u32 = 0x0C1A_FF61;   // register_driver(path,fops,mode,priv)

pub const FW_UNREGISTER_DRIVER: u32 = 0x0C1B_002D; // unregister_driver(path)

pub const FW_APP_LOOKUP: u32 = 0x0CA6_9935;        // app_lookup(app_id) -> app*

pub const FW_APP_INSTALL: u32 = 0x0CA6_A30D;       // app_install(desc, pages*, count)

pub const FW_INIT_BUFFER: u32 = 0x0C51_3A45;       // app_launcher_add(app_id) (036 名 init_buffer, 1参)

pub const FW_NOTIFY: u32 = 0x0CA9_A899;            // lvx_notification_insert_message(notif*) 固件完整链(拷贝/登记由它内部自处理; 自己手写那条手动链里的段B memset 实测必崩)

// VFS (symbols: nuttx)
pub const FW_FILE_OPEN: u32 = 0x0C1D_0A29;   // open(path, flags, mode) -> fd

pub const FW_FILE_CLOSE: u32 = 0x0C1B_9D81;  // close(fd) -> 0

pub const FW_FILE_READ: u32 = 0x0C1D_129D;   // read(fd, buf, len) (036 版本核对到的是 0x0C1C1E25)

pub const MAX_ROWS_PER_PAGE: usize = 12;

// 信息页三行实时数值(存储/内存/CPU), 每格 STAT_STRIDE 字节
pub const STAT_STRIDE: usize = 32;
pub static mut STAT_VAL: [[u8; STAT_STRIDE]; 3] = [[0; STAT_STRIDE]; 3];
pub static mut STAT_TICK: u32 = 0;
pub static mut HEAL_TRYS: u32 = 0;            // 自愈重试次数(限次防死循环)

// 蓝牙 HID 遥控(页11): 点击置位 → lv_timer 里真正发送(避开事件派发上下文)

pub const RENDER_NONE: u32 = 0xFFFF_FFFF;           // 无待重建请求 // lv_obj_set_hidden(obj,hidden)

pub const STAT_MAGIC: u32 = 0x5348_4332;  // "2CHS"

pub const STAT_VERSION: u32 = 0x4348_5302; // "CHS2" 新版标识，旧版是 "CHS1"

pub const CMD_MAGIC: u32 = 0x5348_4331;    // "1CHS"

pub const CMD_INSTALL: u32 = 2;

pub const ST_ACTIVE: u32 = 1;

pub const MOD_ID: u32 = 0x4348_4F53;

pub const SLOT_N: usize = 8;

pub const FOPS_VER: usize = 5;

pub const FOPS_WCNT: usize = 8;

pub const FOPS_SLOTS: usize = 12;

pub const FOPS_N: usize = 48;

pub const FOPS_DBG0: usize = 44;

pub const FOPS_DBG1: usize = 45;

pub const FOPS_DBG2: usize = 46;

pub const FOPS_STEP: usize = 47; // 细粒度崩点步号(诊断)：10/11 lookup前后12/13 register前后

pub const STAT_LEN: usize = 192;

pub static mut FOPS: [u32; FOPS_N] = [0; FOPS_N];

#[no_mangle]
pub static DEV_PATH: [u8; 11] = *b"/dev/chaos\0";

#[no_mangle]
pub static PKG_NAME: [u8; 18] = *b"com.chaos.manager\0";

#[no_mangle]
pub static ICON_PATH: [u8; 27] = *b"/data/chaos/chaos_icon.bin\0";

#[no_mangle]
pub static DISPLAY_NAME: [u8; 6] = *b"Chaos\0";

#[no_mangle]
pub static PAGE_MAIN: [u8; 5] = *b"main\0";

// 页名(互异即可, page_goto 按 page_id 解析; page1-4=目录0-3级 page5/6=美化二级
//  page7=查看)。文件管理原来占 6 页(page1-6), 为了给"更换字体""桌面图标"
//  两个二级页腾出**独立 page_id**(独立 page_id 才有固件的页面跳转动画), 目录页收到 4 级:
//  文件管理最深到 /a/b/c(/data/chaos/icons 够用, 再往下进不去了)。
pub static PAGE_DIR0: [u8; 6] = *b"files\0";

pub static PAGE_DIR1: [u8; 7] = *b"files1\0";

pub static PAGE_DIR2: [u8; 7] = *b"files2\0";

pub static PAGE_DIR3: [u8; 7] = *b"files3\0";

/// 页5: 系统美化 -> 更换字体(二级)。页名沿用 "textsub" —— 它只是 page_goto 的解析键,
/// 与页面内容无关, 改它就要三处同步(state/ipc/page), 收益为零。
pub static PAGE_TXTSUB: [u8; 8] = *b"textsub\0";

/// 页6: 系统美化 -> 桌面图标(二级)
pub static PAGE_ICONSUB: [u8; 8] = *b"iconsub\0";

pub static PAGE_VIEWR: [u8; 7] = *b"viewer\0";

pub static PAGE_WFACES: [u8; 7] = *b"wfaces\0";

pub static PAGE_ROT: [u8; 8] = *b"wfshake\0";
pub static PAGE_MGR: [u8; 7] = *b"wfmgmt\0";
pub static PAGE_BRIGHT: [u8; 7] = *b"bright\0";
pub static PAGE_CACHE: [u8; 6] = *b"cache\0";
pub static PAGE_FONTD: [u8; 6] = *b"fontd\0";

pub static mut APP_META: [u32; 16] = [0; 16];

pub const PAGE_COUNT: usize = 14;                   // 注册页数(页0..页13)
pub const MAX_PID: usize = PAGE_COUNT - 1;          // 页号上界, 所有 pid 校验统一引用它
pub static mut DESC: [u32; 406] = [0; 406];         // 14 页 × 29 word(116B/页, 固件描述符尺寸)

pub static mut NOTIF: [u32; 22] = [0; 22];

pub static mut CTOR_DONE: u32 = 0; // ctor 幂等守卫(.init_array + module_main 可能双触发)

pub static mut DRIVER_ON: u32 = 0; // driver 注册成功标志（dtor 幂等守卫）

// 非零初始化可变全局 → 产生 .data PROGBITS 节（与固件自带模块的 ELF 形状一致）
#[used]
#[no_mangle]
pub static mut CHAOS_DATA_MAGIC: u32 = 0x4348_414F; // "OAHC" = "CHAOS" 反序

// read(len >= 4096) 的大块回读缓冲: 读取方拿到的是这里的字节
pub static mut THUNKS_BUF: [u8; 4096] = [0; 4096];

pub static mut THUNKS_LEN: u32 = 0;

pub static BANNER_TEXT: &[u8] = "Chaos 已安装！\0".as_bytes(); // 应用名 = Chaos(中英文统一)

// 页面 UI 生命周期状态（照固件自带页面的模式：on_create 存 r1，渲染在 on_resume）
pub static mut APP_REGISTERED: u32 = 0;  // app 已注册标志（防 notify 触发的 INSTALL 重入）

pub static mut NOTIF_DONE: u32 = 0;      // notify 已执行标志（0x43 幂等, 防重复弹窗/堆操作）

pub static mut WRITE_BUSY: u32 = 0;    // write 重入保护（notify 触发 launcher 回调时静默）

pub static mut LANG: u32 = 0;  // 语言: 0=EN 1=ZH（0x18 设中文, 0x19 设英文）

// 应用名: 中英文统一为 Chaos。
// 保留 NAME_ZH 这个符号名与它的 6 字节形状: ipc.rs 的通知链按语言二选一取它,
// 内容现在与 DISPLAY_NAME 一致 —— 语言差异从此只影响别的文案。
pub static NAME_ZH: &[u8] = "Chaos\0".as_bytes();

pub static BODY_EN: &[u8] = "Chaos installed!\0".as_bytes();

pub static mut APP_ID: u32 = 0;   // 运行时选定的 app_id（白名单空槽自动分配）

pub static mut FREE_COUNT: u32 = 0; // 白名单空闲槽计数

pub static mut FREE_BITS: u32 = 0;  // 空闲位图（bit k = 池[k] 空闲）

// ============================================================
// UI 上下文布局（照固件自带模块的那套结构）
// 自带模块里的偏移: manager_native_init 0x305D、ui_context_init 0x3760
// outer ctx: 0x2724, inner ui_ctx: 0x26C8, 总 0x300B
// ============================================================
// 引擎上下文树: fp+0x04=描述符 fp+0x08=节点数 fp+0x0C=节点(0x1C) fp+0xC0C=布局表 fp+0x10CC=数据表 fp+0x38C=字符串池
// app_id=200 (0xC8) 当前稳定值，真机验证通过
pub const APPID_POOL: [u32; 1] = [0xC8]; // app_id=200。0xC8 与 0xCD 实测都稳定, 不是 app_id 的问题

// ===== UI 布局约定 (label 文本控件 + 目录三件套) =====
// 界面组装方式来自对固件自带 LVGL 后端的静态核对:
// 路由: 前进 page_goto((app_id<<16)|pid) 压栈 / 后退 page_finish(desc) 出栈 = 固件动画转场
//   (本模块后退一律用 fw_api::page_back —— page_finish 无动画且实测不可用)
// 组装顺序: content三件套 → title后创建 → 控件挂content → align_to链
// 文本控件: lvx_label_create + set_label_text + align_to(gap4) 文本界面用文本控件
// 未用的行用 set_hidden 做动态显隐
// 页型: 信息页(label整段+2动作行) / 目录页(10行动态条目) / 查看页(label整块+Next/Back)
// 目录: opendir/readdir/closedir(0x0C1E45xx, 与系统 ls 实现的调用点核对 + 真机验证)
// 红线: root永不自建, content=set_size(336,424)+align(2,0,56)+pad_bottom(32), 控件挂content
pub const FW_STYLE_MISANS_REG_24: u32 = 0x2010_CE44; // MiSans-Regular 24px 样式对象(固件开机代码里的字体样式常量, 中文渲染必需)
// 系统"数据行"用的两个文字样式对象(心率区间面板每行的名称/数值就是它们)
pub const FW_STYLE_HR_NAME: u32 = 0x2010_CE5C;
pub const FW_STYLE_HR_VALUE: u32 = 0x2010_CEF8;

// 表盘链(0xCA7C67C 反汇编核对):
pub const WF_MGR_PTR: u32 = 0x2013_FE6C;             // watchface_manager 全局指针(SRAM字): mgr=*此址

pub const FW_SET_WATCHFACE: u32 = 0x0CA9_5C31;       // set_watchface_by_id(id) 数据层(pending+缓存+持久化JSON)

pub const FW_REFRESH_CUR_WF: u32 = 0x0C5F_32B9;      // refresh_cur_watchface(force,node) 运行时层(destroy旧+create新+激活)

pub const WF_ENGINE_CUR: u32 = 0x2011_9770;          // 引擎当前表盘节点指针字: node = *此址

// 摇一摇(与固件 system_guesture 模块一样订阅 quick_guesture 手势总线, 不自己碰传感器):
// 手势码(固件 system handler 0x0C4BD0FC 分支自证, 日志串即类型名):
pub const GC_SHAKE: u32 = 0x1C2;                     // TYPE_GUESTURE_SHAKE 摇一摇(我们要的); 0x1C3=TYPE_GUESTURE_WRIST 抬腕亮屏 必须丢弃

pub const FW_OPENDIR: u32 = 0x0C1E_4529;           // opendir(path)->DIR*(0=失败+errno) 真机验证

pub const FW_READDIR: u32 = 0x0C1E_4591;           // readdir(DIR*)->dirent*{d_type@0,d_name@1}(0=尽) 真机验证

pub const FW_CLOSEDIR: u32 = 0x0C1E_4565;          // closedir(DIR*) 真机验证

pub const FW_ERRNO_LOCATION: u32 = 0x0C1E_45BD;    // __errno()->int*(固件自带 config 与 nsh 的调用点核对)

// NuttX dirent.h 的 d_type 取值(readdir 返回的 dirent 首字节)。
// 只列过滤真正用到的两个; 其余取值(含 DT_UNKNOWN=0)走"不是目录也不是普通文件"那支。
pub const DT_DIR: u8 = 4;
pub const DT_REG: u8 = 8;

pub static mut FG_PAGE: u32 = 0;                       // 前台 page_id(on_resume 设, dispatch 分流)

// 亮度页(12)状态
pub static mut BRIGHT_VAL: u32 = 128;                  // 当前手动亮度值(0-255, 显示用)
pub static mut BRIGHT_STATUS: [u8; 64] = [0; 64];      // 状态行文本缓冲
pub static mut BRIGHT_REQ: u32 = 0;                    // 请求码: 0=无 1=设值 2=切自动 3=切全功率
pub static mut BRIGHT_REQ_VAL: u32 = 0;                // 设值请求的目标值

pub static mut FILE_LINES: [[u8; 88]; 11] = [[0; 88]; 11];   // 返回行文本在下标 10 // 目录页行文本



pub static mut FILE_VIEW: [u8; 2052] = [0; 2052];     // 查看页当前块原始内容

pub static mut VIEW_DT: u8 = 0;                        // 查看页目标的 dirent 类型(决定能不能读)
pub static mut FILE_VIEW_N: i32 = 0;                    // 实读字节数; <0=错误码(-2=非普通文件未读)

pub static mut VIEW_TEXT: [u8; 2200] = [0; 2200];     // 查看页 label 文本(sanitize/strings+折行)

pub static mut VIEW_BACKTXT: [u8; 88] = [0; 88];      // "< Back > bN" 行文本

pub static mut VIEW_BLK: u32 = 0;                       // 深读块号(skip=blk*2047 顺序读弃)

pub static mut VIEW_LOADED: u32 = 0;                    // 1=当前块已读

pub static mut VIEW_BINARY: u32 = 0;                    // 1=strings模式

pub static mut VIEW_PATH: [u8; 160] = [0; 160];       // 查看页文件全路径

pub static mut PATH_STACK: [[u8; 160]; 8] = [[0; 160]; 8];  // 路径栈(每级全路径, depth=page_id-1)

pub static mut DIR_DEPTH: u32 = 0;

pub static mut DIR_NAMES: [[u8; 64]; 24] = [[0; 64]; 24];

pub static mut DIR_TYPES: [u8; 24] = [0; 24];

pub static mut DIR_COUNT: u32 = 0;

pub static mut DIR_ERR: i32 = 0;                        // opendir 失败 errno(正数)

pub static mut DIR_WIN: u32 = 0;                        // 目录当前窗(8条目/窗)

pub static mut DIR_WIN_SAVE: [u32; 8] = [0; 8];         // 每深度窗位保存(返回时恢复)

// 表盘选择页(page8): RAM 链表枚举缓存
pub static mut WATCH_LINES: [[u8; 88]; 11] = [[0; 88]; 11];

pub static mut WATCH_SUB: [[u8; 88]; 11] = [[0; 88]; 11];    // 副标题(表盘 ID)

pub static mut WF_IDS: [[u8; 16]; 24] = [[0; 16]; 24];   // 表盘 id 串(12位数字, node+8 内联)

pub static mut WF_NAMES: [[u8; 64]; 24] = [[0; 64]; 24];  // 表盘名(node+0x48 内联, 中文)

pub static mut WF_STATE: [u8; 24] = [0; 24];             // node+0x88 状态(3=当前使用)

pub static mut WF_COUNT: u32 = 0;

pub static mut WF_WIN: u32 = 0;

pub static mut WF_ERR: u32 = 0;                          // 1=mgr 指针无效

pub static mut WF_NODES: [u32; 24] = [0; 24];            // 节点指针(对照 mgr 缓存定当前面)

pub static mut WF_PEND: [u8; 24] = [0; 24];              // node+0x90 pending 标志

pub static mut WF_CUR1: u32 = 0;                         // [mgr+0x48] 缓存节点(核心0xCA7C71C路径)

pub static mut WF_CUR2: u32 = 0;                         // [mgr+0x4C] 缓存节点(核心0xCA7C6E2路径)

pub static mut WF_SW_RC: i32 = 0;                        // 最近 set_watchface_by_id 返回值

pub static mut WF_RF_RC: i32 = 0;                        // 最近 refresh_cur_watchface 返回值

// 摇一摇状态
pub static mut SHAKE_SUB: u32 = 0;                       // eventbus 订阅节点(常驻至重启)

pub static mut SHAKE_TIMER: u32 = 0;
pub static mut TIMER_CREATE_N: u32 = 0;   // 诊断: 我们一共创建了几个 lv_timer(应恒为 1)                     // lv_timer 句柄(200ms, UI线程)
/// 节拍自适应的**许可**: 建完定时器读回 `lv_timer_t+0x00` 等于我们传进去的 50 才置 1。
/// 0 = 句柄形状没验上 => 一辈子 50ms(就是改动前的行为), 绝不去写没验证的指针。
pub static mut TICK_ADAPT: u32 = 0;
/// 当前已生效的周期(避免每拍都调 set_period; 也用于"变了才写")
pub static mut TICK_PERIOD_MS: u32 = 50;

pub static mut SHAKE_PENDING: u32 = 0;                   // 回调置旗, timer 消费

pub static mut SHAKE_COOL: u32 = 0;                     // 冷却 tick
// 数据行组件句柄(0=存储行 1=内存行): [行, 数值标签, 进度条]
pub static mut DR_HANDLES: [[u32; 3]; 2] = [[0; 3]; 2];

// 页12 缓存清理: 点击置 CLEAN_REQ, tick 里执行(不在事件回调做长 FS 操作)
pub static mut CACHE_MSG: [u8; 64] = [0; 64];     // 首行文本(扫描结果或清理结果)
pub static mut CACHE_FILES: u32 = 0;              // 扫描/清理的文件数
pub static mut CLEAN_REQ: u32 = 0;                // 1=请求清理
pub static mut CACHE_HAS_RESULT: u32 = 0;         // 1=CACHE_MSG 是刚出的清理结果(渲染消费一次)

/// 字体创建失败时固件回退使用的 flash 静态默认字体(`LV_FONT_DEFAULT`)。
/// 依据: `vg_font_create_core`(0x0C8603B0) 的 param error / check 失败两条分支
/// 都 `mov r0,r4`, r4 = 池 0x0C860468 处的 0x2CCE1734; `miwear_font_create`
/// 越界返回 0, 但成功路径拿到的失败值就是它。`vg_font_destroy`(0x0C860484)
/// 用同值做保护门。静态核对
pub const FW_FONT_DEFAULT: u32 = 0x2CCE_1734;
/// `lv_global` 基址（`lv_init` 0x0C1663EC 对它做 `memset(0x20103174, 0, 0x20c)` +
/// `memzero(+8, 0x318)`，即整块约 0x320 字节）。字体管理器单例就是这个块的第一个字
/// （`font_apply::manager_ready` 读 `+0x1C` 取路径管理器）。静态核对
pub const LV_GLOBAL: u32 = 0x2010_3174;
/// `lv_global + 0x14` = 当前用于刷新的 `lv_display_t *`。取/存是一对纯指令
/// (0x0C1052B0 `ldr r0,[r3,#0x14]; bx lr` / 0x0C1052C4 `str r0,[r3,#0x14]`)，
/// 同一个 0x14 在 `refr_area_part`(0x0C105358) 里被当 display 用(+0x28=flush_cb,
/// +0x25c=inv_p, +0x3C+i*0x10=inv_areas)。静态核对
/// 重要: 上面那对取/存访问器的形状就是 LVGL 的
/// `_lv_refr_get/set_disp_refreshing` —— **"没有 display 在刷新"时这个槽是 0**,
/// 不是"指针取不到"。把 0 当成"忙"方向就整个反了(空闲判忙 => 累计熔断把换字关掉;
/// 只在它非 0 的瞬间放行 => 恰好是渲染窗口)。门的正确写法见 font_tree::ft_render_busy。
/// 注意: `lv_global + 0x18` 是全镜像最热的指针字段(35 次取址), 但"+0x14 与 +0x18
/// 谁是 display"静态证不了(取用它们的代码在 0x1C 模块, 实现体已被厂商清零)。
pub const LVG_DISP: u32 = 0x14;
/// `disp + 0x3a` 的 **bit1** = `rendering_in_progress`。证据：`_lv_inv_area`
/// (0x0C105164) 入口 `mov r4,r0` 存下 disp，紧接着 `ldrb.w r5,[r4,#0x3a]` +
/// `ands r5,r5,#2` + `bne 0x0c10527c`，而 0x0c10527c 就是
/// `!disp->rendering_in_progress` 断言体 —— **本固件断言不返回**(落到下一个函数
/// 入口)，UI 线程挂死等看门狗。所以渲染进行中绝不能改任何对象样式。静态核对
pub const DISP_FLAG_OFF: u32 = 0x3a;
pub const DISP_RENDERING_BIT: u32 = 0x02;
/// face 拷贝块 `+0x0C` = u16 `line_height`(LVGL 排版用的高度)。
/// 证据一：`LV_FONT_DEFAULT`(0x2CCE1734) 的 +0x0C 实测是 16(个位图字体应有的行高,
/// 不是指针)；证据二：`row_init`(0x0C4C8560) 取到行字体后
/// `ldr r1,[r6,#0xc]` -> `bl 0x0C5880E8(标签, r1, 0)` **把行高烘进标签高度**,
/// 第二个标签 `bl 0x0C589188(标签, 1, 0, 行高+2)` 用它算 y 偏移。静态核对
/// 另: face 拷贝块 `+0x24` = 字体管理器缓存节点指针(节点 +4 = 字体名指针, +8 = u16 字号
/// + u16 style, +0x2c = 引用计数)。构造点 0x0C85F724 `str.w r4,[r8,#0x24]`,
/// 删除路径 0x0C85F94C `ldr r5,[r4,#0x24]` 同读法反证。
pub const FACE_LINE_HEIGHT_OFF: u32 = 0x0C;


pub static mut APP_FG: u32 = 0;                  // 1 = 我们的应用在前台(on_resume 置, on_pause/on_destroy 清)

pub static mut SHAKE_EN: u32 = 1;                        // 开关(信息页行3切换)

pub static mut RENDER_REQ: u32 = RENDER_NONE;            // 待重建页号(由 lv_timer 消费)

pub static mut WF_CUR0: u32 = 0;                         // 引擎当前面节点(*0x20119770, 权威标记)

pub static mut WF_TOTAL: u32 = 0;                        // 链表全节点数(过滤前)

pub static mut WF_SEL_BUILTIN: u32 = 0;                  // 轮换页 switch: 内置表盘是否参与轮换(默认关)

pub static mut WF_SHOW_BUILTIN: u32 = 1;                  // 表盘页 switch: 是否显示系统预置(type 0)

// 参与轮换的判定改为按"节点指针"记录被取消参与的节点(默认空 = 全部参与),
// 与列表位置解耦 两个页面列表内容不同也不会串位
pub static mut WF_DESEL: [u32; 24] = [0; 24];            // 被取消参与轮换的表盘节点

pub static mut WF_DESEL_CNT: u32 = 0;

pub static mut WF_MAINTYPE: i32 = -1;                    // 主表盘 type(=引擎当前面的 type; -1=未知不过滤)

#[used]
#[link_section = ".init_array"]
pub static CHAOS_CTOR: unsafe extern "C" fn() = crate::chaos_ctor;

#[used]
#[link_section = ".fini_array"]
pub static CHAOS_DTOR: unsafe extern "C" fn() = crate::chaos_dtor;

// 页面状态集中为一个结构体(替代原来 6 个并行数组)
pub struct Page {
    pub desc: u32,          // 固件给的页面描述符指针(存活判据要从它 +0x30 读 root)
    pub root: u32,
    pub title: u32,
    pub content: u32,
    pub label: u32,
    pub built: u32,
    pub rows: [u32; MAX_ROWS_PER_PAGE],
}

impl Page {
    pub const EMPTY: Page = Page {
        desc: 0, root: 0, title: 0, content: 0, label: 0, built: 0,
        rows: [0; MAX_ROWS_PER_PAGE],
    };
}

pub static mut PAGES: [Page; PAGE_COUNT] = [Page::EMPTY; PAGE_COUNT];
// 每页标题文本缓冲: 标题 = 进入该页时点的行控件名, 目录页/查看页是动态的
pub static mut TITLE_TXT: [[u8; 48]; PAGE_COUNT] = [[0; 48]; PAGE_COUNT];
