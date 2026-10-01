// 安装协议(设备文件 IPC + 注册链)

use crate::*;

#[inline(always)]
pub(crate) unsafe fn fops_rd(off: usize) -> u32 {
    core::ptr::read_volatile(core::ptr::addr_of_mut!(FOPS[off]))
}

#[inline(always)]
pub(crate) unsafe fn fops_wr(off: usize, val: u32) {
    write_volatile(core::ptr::addr_of_mut!(FOPS[off]), val);
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_open(_: *mut u8) -> i32 {
    core::arch::asm!("nop");
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_close(_: *mut u8) -> i32 {
    core::arch::asm!("", options(nomem, nostack));
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_read(_: *mut u8, buf: *mut u8, len: u32) -> i32 {
    // 词位 ABI(Lua 端 status_words 是 1 基数组: words[1] = 本缓冲的第 0 个 u32)。
    // 下表左列是 Lua 词位, 代码里的 put 索引 = 词位 - 1(Rust 0 基) ——
    // **两者只差 1, 20261001 曾把前四字段写偏一词导致 "status magic mismatch"**:
    //   Lua1=魔数(idx0) / Lua2=槽数(idx1) / Lua3=写计数(idx2) / Lua4=版本(idx3) /
    //   Lua11.. 槽位四元组(idx10..) / Lua43..45 调试字(idx42..44) /
    //   Lua46 fops 地址(idx45) / Lua47 字体槽(idx46) / Lua48 步骤号(idx47)
    #[inline(always)]
    unsafe fn put(words: *mut u32, i: usize, v: u32) {
        write_volatile(words.add(i), v);
    }

    // 大缓冲(>=4096)的读是 thunks 通道, 与状态块无关
    if len >= 4096 && THUNKS_LEN > 0 {
        let n = core::cmp::min(THUNKS_LEN, len) as i32;
        core::ptr::copy_nonoverlapping(THUNKS_BUF.as_ptr(), buf, n as usize);
        return n;
    }
    if (len as usize) < STAT_LEN { return 0; }
    let words = buf as *mut u32;
    put(words, 0, STAT_MAGIC);
    put(words, 1, SLOT_N as u32);
    put(words, 2, fops_rd(FOPS_WCNT));
    put(words, 3, STAT_VERSION);
    // 槽位区从 idx 10 起, 每槽 4 词: 首词是槽值, 其余 3 词恒 0
    let mut slot = 0;
    while slot < SLOT_N {
        let quad = 10 + slot * 4;
        put(words, quad, fops_rd(FOPS_SLOTS + slot * 4));
        put(words, quad + 1, 0);
        put(words, quad + 2, 0);
        put(words, quad + 3, 0);
        slot += 1;
    }
    put(words, 42, fops_rd(FOPS_DBG0));
    put(words, 43, fops_rd(FOPS_DBG1));
    put(words, 44, fops_rd(FOPS_DBG2));
    put(words, 45, core::ptr::addr_of_mut!(FOPS) as u32);
    // idx 45(Lua 词 46): 当前在用的字体池位(0 = 安装器那份, 1..8 = st1..st8)。
    // 字体投递包靠它判"这一份现在在不在用": 在用的那份绝不能被覆写(font_apply 红线)。
    // 槽位分配不靠它算, 由 /data/chaos/font/index.txt 清单决定。
    put(words, 46, font_apply::live_get());
    put(words, 47, fops_rd(FOPS_STEP));
    STAT_LEN as i32
}


#[inline(always)]
pub(crate) unsafe fn file_open_ro(path: *const u8) -> i32 {
    // 内核层 O_RDONLY=1, 不是 POSIX 的 0!
    // 这套取值(O_RDONLY=1/O_WRONLY=2/O_CREAT=4/O_RDWR=3)与固件自带模块的用法一致,
    // 设备节点与 /dev/console 的调用点都核对过。
    // flag=0 → open 拿到 fd 但无访问模式 → read 必 -1 (真机 n=FFFFFFFF 根因)
    // 写路径 file_open flag=6 = O_WRONLY|O_CREAT, 与固件自带模块的写组合一致(所以写一直成功)
    let f: unsafe extern "C" fn(*const u8, u32, u32) -> i32 = core::mem::transmute(FW_FILE_OPEN as usize);
    f(path, 1, 0)
}


#[inline(always)]
pub(crate) unsafe fn file_close(fd: i32) -> i32 {
    let f: unsafe extern "C" fn(i32) -> i32 = core::mem::transmute(FW_FILE_CLOSE as usize);
    f(fd)
}

/// 字体池位文件在不在 —— 只给字体投递命令(0x30)用。
///
/// 为什么必须验这一下: 池位一旦切到一个不存在或没写完的文件, 之后建出的每一张脸
/// 都会失败, 界面文字整片回退, 而真机上"投递成功"与"投递失败"长得一模一样。
/// 路径形状必须与 `font_apply::FONT_PATHS` 里的 st1..st8 逐字同行(这里现拼是为了带池位号)。
/// 写法与 `font_apply::file_ready()` 同形: 只 open/close, 不读内容。
unsafe fn font_slot_file_ok(slot: u32) -> bool {
    let mut buf = [0u8; 28];
    {
        let mut w = W::new(buf.as_mut_ptr(), buf.len());
        w.s(b"/data/chaos/font/st").n(slot).s(b".ttf");
        w.end();
    }
    let fd = fw_api::open(buf.as_ptr(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { false } else { fw_api::close(fd); true }
}

// 安装流程的步号只记在 FOPS_STEP，由 read 状态结构带回（不写文件）。
pub(crate) unsafe fn trace_step(step: u8) {
    // 不写 /data/chaos/step.txt: 一次上报要 ~8 次 file_write(固件 file_write 内部
    // malloc)，在 fops.write 上下文里有堆副作用 → 切表盘时崩(步号 170)。
    // 步号只更新 FOPS_STEP 这块 BSS, 读取方从状态结构的 step 字段拿回去显示。
    fops_wr(FOPS_STEP, step as u32);
}

#[inline(never)]
pub(crate) unsafe fn build_structures() {
    let cb_signal = chaos_on_signal as *const () as u32;
    let cb_pause = chaos_on_pause as *const () as u32;
    let cb_destroy = chaos_on_destroy as *const () as u32;
    let cb_ui_destroy = chaos_on_ui_destroy as *const () as u32;
    // 14 页注册(固件自带模块是 3 页模式, 这里扩到 14 页; 全部填充无空页 —— 空描述符会崩):
// page0=信息 page1..4=目录0..3级 page5=更换字体 page6=桌面图标 page7=文件查看
// page8/9=表盘切换/摇一摇轮换 page10=表盘管理 page11=亮度 page12=缓存 page13=系统美化菜单。
// **每级目录独立 page_id → 进入/返回/标题< 全走 page_goto/page_back 固件动画**;
// 美化页的两个二级页同样给独立 page_id, 就是为了让它们也有这个动画。
let cb_create = chaos_on_create as *const () as u32;
let cb_resume = chaos_on_resume as *const () as u32;
let app_id = st_rd!(APP_ID);

let desc = core::ptr::addr_of_mut!(DESC) as *mut u32;
let pnames: [u32; PAGE_COUNT] = [
    core::ptr::addr_of!(PAGE_MAIN) as u32,
    core::ptr::addr_of!(PAGE_DIR0) as u32,
    core::ptr::addr_of!(PAGE_DIR1) as u32,
    core::ptr::addr_of!(PAGE_DIR2) as u32,
    core::ptr::addr_of!(PAGE_DIR3) as u32,
    core::ptr::addr_of!(PAGE_TXTSUB) as u32,
    core::ptr::addr_of!(PAGE_ICONSUB) as u32,
    core::ptr::addr_of!(PAGE_VIEWR) as u32,
    core::ptr::addr_of!(PAGE_WFACES) as u32,
    core::ptr::addr_of!(PAGE_ROT) as u32,
    core::ptr::addr_of!(PAGE_MGR) as u32,
    core::ptr::addr_of!(PAGE_BRIGHT) as u32,
    core::ptr::addr_of!(PAGE_CACHE) as u32,
    core::ptr::addr_of!(PAGE_FONTD) as u32,
];
let mut pg = 0usize;
while pg < PAGE_COUNT {
    let b = desc.add(pg * 29);                       // 每页 29 word = 116B
    write_volatile(b.add(4), pnames[pg]);            // +0x10 page_name
    write_volatile(b.add(5), app_id << 16 | pg as u32); // +0x14 page_id | +0x16 app_id
    write_volatile(b.add(13), cb_signal);            // +0x34 on_signal
    write_volatile(b.add(19), cb_create);            // +0x4C on_create
    write_volatile(b.add(20), cb_resume);            // +0x50 on_resume
    write_volatile(b.add(22), cb_pause);             // +0x58 on_pause
    write_volatile(b.add(23), cb_destroy);           // +0x5C on_destroy(页面注销时, 对象树还在)
    write_volatile(b.add(24), cb_ui_destroy);        // +0x60 on_ui_destroy(递归删对象树之前)
    pg += 1;
}

    let meta = core::ptr::addr_of_mut!(APP_META) as *mut u32;
    write_volatile(meta.add(2), core::ptr::addr_of!(PKG_NAME) as u32);
    write_volatile(meta.add(3), core::ptr::addr_of!(ICON_PATH) as u32);
    write_volatile(meta.add(4), st_rd!(APP_ID)); // app_id 动态
    // +0x1C: display_name 回调（函数指针，返回名字字符串）。
    // 依据：0x0C4F2908 内部 `blx [app节点+0x1C]` 两次调用它；固件自带模块里这个成员
    // 就是一个 10 字节的 .text 函数(type=2)。
    // 注意: 这一格放字符串指针会崩 —— init_buffer 内 blx 直接跳到字符串数据上(步号 16)。
    write_volatile(meta.add(7), chaos_display_name as *const () as u32);

    let notif = core::ptr::addr_of_mut!(NOTIF) as *mut u32;
    // 依据（notify_installed @0x0CA81FB8 + lookup @0x0CA81D18）：
    // lookup 按 notif+0x00/+0x04 的 8 字节 key 在注册表匹配。能命中注册表的字节流是
    // `01 'S' 'U' 'P' | 'O' 'N' 'A' 'C'` → key = (0x50555301, 0x43414E4F)。
    // key 填 (1, 0) 匹配不到 → notify 走"插入新记录"分支，那条路会崩。
    write_volatile(notif.add(0), 0x5055_5301); // 01 'S' 'U' 'P' (LE)
    write_volatile(notif.add(1), 0x4341_4E4F); // 'O' 'N' 'A' 'C' (LE)
    // 标题/正文按系统语言二选一(中英文现在都是 Chaos, 见 state.rs 的 NAME_ZH/BANNER_TEXT)
    let lang = st_rd!(LANG);
    if lang != 0 {
        write_volatile(notif.add(3), NAME_ZH.as_ptr() as u32);
        write_volatile(notif.add(4), NAME_ZH.as_ptr() as u32);
        write_volatile(notif.add(5), BANNER_TEXT.as_ptr() as u32); // "Chaos 已安装！"
    } else {
        write_volatile(notif.add(3), core::ptr::addr_of!(DISPLAY_NAME) as u32);
        write_volatile(notif.add(4), core::ptr::addr_of!(DISPLAY_NAME) as u32);
        write_volatile(notif.add(5), BODY_EN.as_ptr() as u32); // "Chaos installed!"
    }
    write_volatile(notif.add(7), core::ptr::addr_of!(ICON_PATH) as u32);
    write_volatile(notif.add(8), core::ptr::addr_of!(ICON_PATH) as u32);
    write_volatile(notif.add(20), 1);
}

#[inline(never)]
pub(crate) unsafe fn cmd_install(phase: u32) {
    let lookup: FnLookup = core::mem::transmute(FW_APP_LOOKUP as usize);
    let install: FnInstall = core::mem::transmute(FW_APP_INSTALL as usize);
    let init_buffer: FnInitBuffer = core::mem::transmute(FW_INIT_BUFFER as usize);

    // 重入保护: notify_installed 会触发 launcher 重新发 INSTALL arg0=0。
    // 已注册时跳过 register_app/init_buffer（幂等），直接返回成功，避免重复注册崩。
    if st_rd!(APP_REGISTERED) != 0 {
        trace_step(0x42); // 66: 重入跳过
        return;
    }

    // 空槽自动分配: 遍历白名单找到第一个 lookup 为空槽的 id
    let mut selected: u32 = 0;
    let mut free_count: u32 = 0;
    let mut free_bits: u32 = 0;
    let mut k = 0usize;
    while k < APPID_POOL.len() {
        let cand = APPID_POOL[k];
        if lookup(cand).is_null() {
            free_count += 1;
            free_bits |= 1u32 << k; // bit k = 空闲
            if selected == 0 {
                selected = cand;
            }
        }
        k += 1;
    }
    if selected == 0 {
        trace_step(0x33); // 51: 白名单全占, 放弃注册
        fops_wr(FOPS_DBG0, 0xFFFF_FFFF);
        fops_wr(FOPS_DBG1, 0);
        return;
    }
    st_wr!(APP_ID, selected);
    st_wr!(FREE_COUNT, free_count);
    st_wr!(FREE_BITS, free_bits);
    fops_wr(FOPS_DBG0, selected as u32);      // d0 = 选中的 app_id
    fops_wr(FOPS_DBG1, free_count as u32);    // d1 = 空闲槽数
    fops_wr(FOPS_DBG2, free_bits);            // d2 = 空闲位图(bit k 对应池[k])

    // 所有 phase 先 build（用选定的 APP_ID 填 desc/meta）
    build_structures();

    // phase 0x10 及以上：app_lookup 前置（读 SRAM2 0x200EB040）
    trace_step(10); // app_lookup 前
    let existing = lookup(selected);
    trace_step(11); // app_lookup 后
    fops_wr(FOPS_DBG1, existing as u32);

    if phase < 0x11 { return; }

    // 槽位应该是空的（刚选出来的）仅防御：
    if !existing.is_null() {
        trace_step(0x1F); // 31: 竞态重占, 跳过注册
        return;
    }

    // phase 0x11 及以上：register_app
    let meta = core::ptr::addr_of_mut!(APP_META) as *const u32;
    let desc_addr = core::ptr::addr_of_mut!(DESC) as u32;
    // 全部注册页 stride 0x74=116B(页数增删必须同步这里, 漏一格就是漏注册一页)
    let items: [u32; PAGE_COUNT] = [
        desc_addr, desc_addr + 0x74, desc_addr + 0xE8, desc_addr + 0x15C,
        desc_addr + 0x1D0, desc_addr + 0x244, desc_addr + 0x2B8, desc_addr + 0x32C,
        desc_addr + 0x3A0, desc_addr + 0x414, desc_addr + 0x488, desc_addr + 0x4FC,
        desc_addr + 0x570,
        desc_addr + 0x5E4,
    ];
    trace_step(12); // register_app 前
    let install_result = install(meta, items.as_ptr(), PAGE_COUNT as u32);
    trace_step(13); // register_app 后
    fops_wr(FOPS_DBG1, install_result as u32);
    // 注册成功置标志（防 notify 触发的重入）
    st_wr!(APP_REGISTERED, 1);

    if phase < 0x12 { return; }

    // phase 0x12 及以上：app_lookup 验证 + init_buffer
    trace_step(14);
    let installed = lookup(st_rd!(APP_ID));
    trace_step(15);
    fops_wr(FOPS_DBG2, installed as u32);

    // register 未生效（lookup 验证 NULL）时跳过 init_buffer。
    // init_buffer 第一句 blx [[0x200EB040+0x18]+4]，注册未生效时该回调指针为空 → 必崩。
    if installed.is_null() {
        trace_step(9); // ST=9 = register 未生效，跳过 init_buffer
        if phase < 0x13 { return; }
    } else {
        trace_step(16); // init_buffer 前
        let app_id = st_rd!(APP_ID);
        let _buf = init_buffer(app_id); // init_buffer(自己的app_id)，同拍同id —— 与固件自带模块的调用方式一致
        trace_step(17); // init_buffer 后
        if phase < 0x13 { return; }
    }

        // phase 0x13：固件 notify 主链（与 0x43 同一条路）—— 不需要自己搭
    // COPY/MALLOC/SEGB/MEMSET/MEMCPY 那条手动链，lvx_notification_insert_message 内部自处理
    if phase >= 0x13 {
        notify_firmware_full();
    }
}

// 命令 0x43: 走固件 notify_installed（完整链的成功路径）
// 幂等: NOTIF_DONE=1 后跳过。不自己调 0x0CA81D18(模块上下文创建记录破坏堆
// → 切表盘崩); 固件 notify_installed 内部自己调(固件上下文, 安全)。
pub(crate) unsafe fn notify_firmware_full() {
    if st_rd!(NOTIF_DONE) != 0 {
        fops_wr(FOPS_DBG0, 0); // 已 notify
        return;
    }
    st_wr!(NOTIF_DONE, 1);
    build_structures();  // 填 NOTIF (含 0x50=1 主路径标志, key=SUPONAC)
    let notif = core::ptr::addr_of!(NOTIF) as *const u32;
    // 固件 notify_installed 主链 (内部: 深拷贝/0x0CA81D18 查询/主路径/regnotify)
    let f: unsafe extern "C" fn(*const u32) -> i32 =
        core::mem::transmute(FW_NOTIFY as usize);
    let rc = f(notif);
    fops_wr(FOPS_DBG0, rc as u32);
    fops_wr(FOPS_DBG2, rc as u32);
}

#[no_mangle]
pub(crate) unsafe extern "C" fn chaos_write(_: *mut u8, buf: *const u8, len: u32) -> i32 {
    // 命令帧至少 16 字节: magic | cmd | arg0 | arg1
    if buf.is_null() || (len as usize) < 16 { return 0; }
    // 重入保护(WRITE_BUSY): notify_installed 内部的 lookup 会触发 launcher 重发
    // INSTALL, 此时上一个 write 还没返回(notify 在栈上) —— 只能静默丢弃,
    // 让 notify 主链先走完; 拦不住就会在嵌套里崩。
    if st_rd!(WRITE_BUSY) != 0 {
        return 0;
    }
    st_wr!(WRITE_BUSY, 1);
    let writes_now = fops_rd(FOPS_WCNT) + 1;
    fops_wr(FOPS_WCNT, writes_now);
    let frame = buf as *const u32;
    if core::ptr::read_volatile(frame) != CMD_MAGIC {
        st_wr!(WRITE_BUSY, 0);
        return 0;
    }
    let cmd = core::ptr::read_volatile(frame.add(1));
    let arg0 = core::ptr::read_volatile(frame.add(2));
    // 第 4 字历史上恒 0; 字体投递(0x30)拿它带"目标池位", 其余命令不读它。
    let arg1 = core::ptr::read_volatile(frame.add(3));

    // INSTALL 重入在入口级拦截。
    // APP_REGISTERED=1 后（已经注册过），INSTALL 命令直接返回成功，
    // 不进 cmd_install —— 嵌套/异步的重入都被这层挡住，不崩。
    // 只在 cmd_install 内部挡不住: 实测入口 WRITE_BUSY=1、APP_REGISTERED=1 时仍停在步号 14。
    // return 前必须清 WRITE_BUSY: 跳过函数尾部清锁会让残留的 1 拒掉后续所有 write(安装卡在步骤8)
    if cmd == CMD_INSTALL && st_rd!(APP_REGISTERED) != 0
        && arg0 <= 2 {
        // 只拦 launcher 重入的标准注册命令(INSTALL arg0=0/1/2)
        // 拦截范围必须这么窄: 一并挡掉其它 arg0 会让 notify 等后续命令永远进不来
        st_wr!(WRITE_BUSY, 0); // 清锁再返回
        return 16; // 已注册，INSTALL 重入静默成功（重复的注册命令按幂等处理）
    }
    match cmd {
        CMD_INSTALL => {
            // 安装流程用到的命令: 0/1/2 注册发布, 0x13 注册链, 0x43 固件 notify,
            // 0x18/0x19 语言, 0x22 回读槽统计, 0x30 字体投递与切换(投递包)
            match arg0 {
                0    => { fops_wr(FOPS_STEP, 0); cmd_install(0x12); }   // 注册链(止于 init_buffer)
                1 | 2 => { /* no-op publish: 不调固件，仅置 ACTIVE */ }
                0x13 => { fops_wr(FOPS_STEP, 0); cmd_install(0x13); }   // 完整注册链
                0x43 => { fops_wr(FOPS_STEP, 0); notify_firmware_full(); } // 固件完整 notify 链
                0x18 => { st_wr!(LANG, 1); } // 设中文
                0x19 => { st_wr!(LANG, 0); } // 设英文
                0x22 => {
                    // 回读槽统计: d0=选中id d1=空闲数 d2=空闲位图
                    fops_wr(FOPS_DBG0, st_rd!(APP_ID));
                    fops_wr(FOPS_DBG1, st_rd!(FREE_COUNT));
                    fops_wr(FOPS_DBG2, st_rd!(FREE_BITS));
                }
                // 字体投递(字体包 Lua 专用): arg1 = 目标池位(1..8), 发这条命令时
                // Lua 已经把新字体整份写进了那个池位。这里只做三件事: 切池位、换登记名
                // (ChaosSans-R<代>, 必须换 —— 固件 face 缓存按 (名字,尺寸) 命中, 沿用旧名
                // 会拿回按旧文件建的脸)、请求重应用(下一拍起分拍重建所有脸)。
                //
                // 三道门, 任何一条不过就不动: 池位合法 / 不是当前正在用的那一份(覆写在用
                // 文件会让还活着的老脸读到别人的字节 = font_apply 红线) / 当前没有正在跑的
                // 应用(半途换池位会让正在建的那批脸读到另一个文件)。被拒时池位不变,
                // 投递包靠"回读 +0x46 是否等于自己请求的值"判成败, 所以这里不需要额外回报。
                //
                // 这条路也是**切换字体**的唯一入口: 清单页点某一条 = 同一句命令
                // (ko 侧 font_list::click_entry 直接调 commit + request)。所以门放宽到
                // 池位 1..8, 与 FONT_SLOT_MAX 同一常量, 不写死数字。
                0x30 => {
                    let slot = arg1;
                    if slot >= 1 && slot <= font_apply::FONT_SLOT_MAX
                        && slot != font_apply::live_get()
                        && !font_apply::busy()
                        && font_slot_file_ok(slot)
                    {
                        font_apply::commit(slot);
                        font_apply::request();
                    }
                }

                _   => {}
            }
            // 每条 INSTALL 类命令收尾都把槽 0 刷成"激活"记录(激活态/保留/模块 id/0),
            // Lua 端 read_status 的槽位区读的就是这四词。
            fops_wr(FOPS_SLOTS, ST_ACTIVE);
            fops_wr(FOPS_SLOTS + 1, 0);
            fops_wr(FOPS_SLOTS + 2, MOD_ID);
            fops_wr(FOPS_SLOTS + 3, 0);
        }
        _ => {}
    }
    // 清理: 释放 write 重入锁
    st_wr!(WRITE_BUSY, 0);
    trace_step(0xAA); // write 完成返回
    16
}

