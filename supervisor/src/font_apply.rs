// 系统字体应用层 —— 把界面文字换成我们自己的字体(登记名 ChaosSans-*, 文件 ChaosWenKai.ttf)。
//
// 两层写回, 缺一不可:
//   1) 本模块的**样式级写回**: 把固件开机建好的那批 lv_style_t 里的 text_font
//      属性(0x5A)换成我们的 face。它只影响**之后**建行时被抄走的那一份 ——
//      行标签的字体是 row_init(0x0C4C8560) 建行时从样式抄下来的 local style 快照
//      (0x0C4C86BC), 所以重新进页会自动跟上, 但常驻页与"此刻正在显示的那一页"
//      不会变。
//   2) font_tree 的**逐对象补写**: 对活对象直写 local style, 补第 1 层够不到的部分。
//      代价是每写一次固件要做全子树失效 + 逐对象发样式变更事件, 所以那边全程门控,
//      而且只在**我们的应用不在前台**时跑(见下面 FA_DEL_PEND 那条等窗口)。
//      它只有"排一次补写"这一种用法: 在"正在写的那一刻"撞上引擎动树(反复后台重扫、
//      在事件派发里原地改对象)实测都会崩, 所以那些形态都不保留。
//
// 红线: 只往固件的样式里放**我们独占名字**(ChaosSans-*)的脸, 绝不把我们的
// 字体对象交给固件缓存节点所有 —— 固件销毁面时会释放节点+0x00 的载荷并级联它的
// 同族链, 共享载荷 = 双重所有权 = 释放后使用/双释放。同样也不能盖掉
// MiSans-SportVF / BaiJamjuree 那 24 条: 它们的 face 归固件的表盘/运动字体槽管。
//
// 为什么必须用"系统没见过的名字"(沿用 MiSans-* 名字的几版全部无效的真根因):
// face 按 (名字,字号) 缓存(0x0C85F9BC), 沿用 MiSans-* 名字重建必然命中开机缓存拿回
// 旧 face, 写回样式等于原样回写。Chaos* 前缀系统没见过 => 缓存必未命中 => 真的打开
// /data/chaos/font 上我们的文件。登记本身幂等(同 (名字,路径) 只加引用计数)。
//
// 时机: 界面上点"重新应用字体"排一次, 开机后也自动应用一次(见 tick 与 boot_auto_tick)。
// 代价与收益: 开机头几秒、自动应用失败之后仍是系统字体, 要手点一次; 换来的是"字体在界面还
// 没建出来之前就位", 覆盖面才是满的。这一层写 RAM 里的样式数组, 不碰对象树、不发事件。
//
// 应用是**分拍做**(apply_start 排队 + apply_step 每拍推进)。写样式本身是零分配
// 的指针写, 贵的是给它配的那批 face —— 每张新脸都要让 FreeType 现读闪盘上 3.4MB 的 TTF。
// 单拍里建几十张脸 = 按住 UI 线程几百毫秒到几秒 = 看门狗黑屏重启(实测: 连"重新应用
// 字体"那一下都会偶然崩)。现在每拍最多开 2 张新脸, 并且台账(FA_CCH_*)
// 满了就不再开: 那个容量同时是整次应用建脸总数的硬上限。

use crate::mem::{rd8, rd16, rd32};
use crate::*;

// ===== 本模块状态(RAM 无痕, 重启即回默认值) =====

/// 1 = 请求重跑一次样式级写回(美化页按钮置位, UI tick 消费)
static mut FA_REQ: u32 = 0;
/// 1 = 本次开机已成功应用过一整轮(开机自动应用的重试门用它, 不从其它计数反推)
static mut FA_OK: u32 = 0;
/// 应用是**分拍做**。一次点完要建几十张 FreeType 脸(每张都要从闪盘上那份
/// 3.4MB TTF 现读表), 连点"重新应用字体"那一下都会偶然崩; 单拍里干几秒的活
/// = 按住 UI 线程 = 看门狗黑屏重启。
/// 预算按"贵的那件事"算: 一拍的额度 = 最多开 FA_FACE_CAP 张**新脸**(台账命中不占额度,
/// 所以纯写指针的一批可以一次过 FA_JOB_CAP 条), 用完就下一拍。
/// stage: 0=空闲 1=在写样式队列 2=在走缓存清单登记认脸 3=收尾
static mut FA_STAGE: u32 = 0;
static mut FA_CUR: u32 = 0;      // 队列游标
static mut FA_JOB_N: u32 = 0;    // 队列长度
const FA_FACE_CAP: u32 = 2;      // 每拍最多新建几张脸
const FA_JOB_CAP: u32 = 16;      // 每拍最多处理几条(全是台账命中时的进度保障)
const FA_JOB_MAX: usize = 176;    // 108 表内 + 33 表外 + 9 本地 + 23 派生 = 173(2 个只读模板已剔), 容量留富余
static mut FA_JOB_ST: [u32; FA_JOB_MAX] = [0; FA_JOB_MAX];
static mut FA_JOB_SZ: [u32; FA_JOB_MAX] = [0; FA_JOB_MAX];
/// 派生样式的来源查不到可信字号 => 跳过不猜(见 DERIVED_STYLE_PAIRS)
static mut FA_C_DER0: u32 = 0;
/// 按红线保留原字体的条目数(SportVF 21 + BaiJamjuree 3 = 24)
static mut FA_C_KEEP: u32 = 0;
/// 认脸表(走固件字体缓存清单): 存进表条数 / 跳过条数
static mut FA_C_ID: u32 = 0;
static mut FA_C_ID0: u32 = 0;
/// 跳过的两类主因分开计: 字号超出 8..120 / 第三种字体族
static mut FA_C_IDBIG: u32 = 0;
static mut FA_C_IDFAM: u32 = 0;
/// 诊断计数: 登记记录数 / face 建出数 / 建失败数 / 写回样式数
static mut FA_C_REG: u32 = 0;
static mut FA_C_FACE: u32 = 0;
static mut FA_C_FACE0: u32 = 0;
static mut FA_C_WR: u32 = 0;

/// (名字指针, 字号) -> face 的复用台账(见 fa_face_of)。
/// 除了开机表那批字号, 还要装下"缓存清单里每个 MiSans 系字号"的回退族/运行期字号(-All 那批)。
/// 重要: 这张表的容量同时是**整次应用建脸总数的硬上限** —— 满了 fa_face_of 直接返回 0
/// (该字号不改); 不设上限时每命中不到一次就多泄漏一块 0x28 并多开一次 FreeType。
const FA_CCH_CAP: u32 = 96;
static mut FA_CCH_NM: [u32; FA_CCH_CAP as usize] = [0; FA_CCH_CAP as usize];
static mut FA_CCH_SZ: [u32; FA_CCH_CAP as usize] = [0; FA_CCH_CAP as usize];
static mut FA_CCH_FACE: [u32; FA_CCH_CAP as usize] = [0; FA_CCH_CAP as usize];
static mut FA_CCH_N: u32 = 0;

// ===== 我们的字体文件与注册名 =====

// 我们的字体文件在安装器里叫 ChaosWenKai.ttf(部署到 /data/chaos/font)。
// **登记的字体名不用它** —— 真正登记的是下面 C_REG 那个"ChaosSans-*"名字:
// face 按 (名字,尺寸) 缓存, 只有系统没见过的名字才会真的打开文件建新 face
// (沿用 MiSans-* 必然命中开机缓存, 写回等于原样回写 —— 这是最初几版无效的真根因)。
//
// 池位与"代"(投递包落盘用):
//   0 = 安装器投递的那一份(开机就在), 1..8 = **st1..st8.ttf** —— 池位号就是文件名里那个数字。
//
// 一共 8 个池位, 并且**不轮转**(字体按清单自由切换):
//   轮转(每落一次盘就换下一个池位)存在的唯一理由是"绝不覆写当前在用的那份" —— 固件建出的
//   face 会按需回读文件, 覆写等于让还活着的老脸读到别人的字节。改成"一个字体永远占同一个
//   槽位"之后这条约束自动消失: 重投同一字体 = 覆写它自己那份文件, 而它此刻作为 live 时
//   0x30 门会拒(见 ipc.rs), 非 live 时没有任何 face 在读它。所以投递包不再需要轮转公式,
//   槽位分配改由 /data/chaos/font/index.txt 这份清单决定(见 font_list.rs)。
//   于是"切换字体"退化成 commit(n) + request() —— 这两句正是 0x30 分支一直在做的(已验证)。
//
// 换池位时必须同时换登记名(FA_GEN -> "ChaosSans-R<代>"): face 缓存按 (名字,尺寸) 命中,
// 沿用旧名字会拿回按旧文件建的脸。
//
// 重要: 池位号必须等于文件名里那个号。这张表原来写的是 st0..st3, 即"池位 1 -> st0.ttf",
// 而 ipc.rs 的 font_slot_file_ok() 与投递包 Lua 都按 "st<池位>.ttf" 拼 —— 两套命名
// 错开一位, 于是校验门看到 st1.ttf 存在就放行, commit(1) 之后却去读不存在的 st0.ttf,
// 字体永远应用不上(现象: st1.ttf 已经躺在 /data/chaos/font 里, Chaos 却识别不了)。
// 现在统一成"池位号 = 文件名号", 三处(ko 表 / ipc.rs 校验门 / 投递包 Lua)必须永远一致。
pub static FONT_PATHS: [&[u8]; 9] = [
    b"/data/chaos/font/ChaosWenKai.ttf\0",    // 池位 0: 安装器投递的那份
    b"/data/chaos/font/st1.ttf\0",            // 池位 1
    b"/data/chaos/font/st2.ttf\0",            // 池位 2
    b"/data/chaos/font/st3.ttf\0",            // 池位 3
    b"/data/chaos/font/st4.ttf\0",            // 池位 4
    b"/data/chaos/font/st5.ttf\0",            // 池位 5
    b"/data/chaos/font/st6.ttf\0",            // 池位 6
    b"/data/chaos/font/st7.ttf\0",            // 池位 7
    b"/data/chaos/font/st8.ttf\0",            // 池位 8
];
/// 可投递的池位数(池位 1..FONT_SLOT_MAX; 池位 0 是安装器自带那份, 不可投递)。
pub(crate) const FONT_SLOT_MAX: u32 = 8;
/// 当前在用的池位(0..8)。
static mut FA_LIVE: u32 = 0;
/// 已经落过几代。0 = 还是安装器那一份(登记名不带后缀)。
static mut FA_GEN: u32 = 0;

/// 当前在用哪一份(界面勾选态与"使用中"副标签、删除门、设备读数都取它)
pub(crate) unsafe fn live_get() -> u32 { st_rd!(FA_LIVE) }

/// 当前在用的字体文件路径
pub(crate) unsafe fn file_path() -> *const u8 {
    path_of(st_rd!(FA_LIVE))
}

/// 池位 -> 路径
pub(crate) unsafe fn path_of(slot: u32) -> *const u8 {
    let i = if slot as usize >= FONT_PATHS.len() { 0 } else { slot as usize };
    FONT_PATHS[i].as_ptr()
}

/// 落盘完成 = 发布: 切到新池位 + 换登记名 + 把 face 复用台账清零。
/// 台账按 (名字**指针**, 字号) 记账, 而名字缓冲地址不变、内容变了 —— 不清账就会
/// 按旧文件建的脸去回写样式, 等于没换。
pub(crate) unsafe fn commit(slot: u32) {
    st_wr!(FA_LIVE, slot);
    let g = st_rd!(FA_GEN) + 1;
    st_wr!(FA_GEN, g);
    let d = core::ptr::addr_of_mut!(C_REG) as *mut u8;
    let mut w = W::new(d, C_REG_LEN);
    w.s(b"ChaosSans-R").n(g);
    w.end();
    st_wr!(FA_CCH_N, 0);
}

// 被 UI 写回、但不在 132 条表里的样式对象: 固件开机字体表盖不到它们, 界面却在用它们。
//
// 审核后剔除: 旧版 38 个里剔除了 3 个无样式证据的地址 ——
// 0x2010C31C / 0x2010C4E8 / 0x2010C4D8 在 style_init(0x0C589398) /
// style_set_font(0x0C587B38) / obj_add_style(0x0C5889F0) 三处都查不到(后两个只出现在
// obj_add_style 的非样式实参位)。对非样式对象调 style_set_font 等于让固件把普通数据
// 当属性数组写, 表现就是"点一下就崩"。算真样式的判据: 上面三处都出现过(单凭某一条
// 证据不算), 与 132 条表用的是同一套筛法。
//
// 已知代价: 这 34 个样式固件从不写字体 => 查不到原字号,
// 只能取 28px 兜底, 结果是文件管理等列表行尺寸不对(拉大或看着偏小)。尺寸是另案
// (台账行 p 名义px>原行盒高 就是为它准备的)。
pub static EXTRA_STYLES: [u32; 41] = [
    // 0x2010C180 / 0x2010C16C 已剔除:
    // 全固件对这两处的唯一引用是"theme init 里当 lv_style_copy 的**源模板**"
    // (0x0C4B9186/0x0C4B970E: style_init(dst) 后 bl 0x0C589000(dst, C180/C16C)),
    // 没有任何被 init/被写/被 add_style 的证据 —— 它们是只读模板, 不是可写样式。
    // 对它们调 style_set_font = 固件把普通数据当属性数组解析 => 写野指针 => 崩。
    // 写回队列每次跑到这一条(队列 0-based 110)都实测崩溃。
    0x2010C3A0, 0x2010C1F0,
    0x2010C388, 0x2010C4A0, 0x2010D33C, 0x2010C364, 0x2010C370,
    0x2010C37C, 0x2010C1C0, 0x2010C208,
    0x2010C214, 0x2010C220, 0x2010C22C, 0x2010C238, 0x2010C244,
    0x2010C1CC, 0x2010C1D8, 0x2010C1E4, 0x2010C1FC, 0x2010CC90,
    0x2010CC84, 0x2010CC78, 0x2010CC6C, 0x2010D348, 0x2010D330,
    0x2010D364, 0x2010D358, 0x2010DA74, 0x2010DA68, 0x2010DA5C,
    0x2010EB64, 0x2010ED4C, 0x2010F0F4,
    // --- 以下 8 个跳过写入 ---
    // 只有"被挂到对象上"这一条证据、没有写字体证据的 6 个(保留供树遍历方案使用):
    0x2010CCA0, 0x2010CCB8, 0x2010CCC4, 0x2010CCDC, 0x2010C2A0,
    0x2010C2F8,
    // 上面已确认的只读模板 2 个:
    0x2010C180, 0x2010C16C,
];
/// 参与字体写回的前 N 个(后 8 个跳过, 见上)。
pub const EXTRA_ACTIVE: usize = 33;
/// 表外样式建 face 用的字号: 固件里没有它们的字号可查, 只能取中号兜底; 唯一有
/// 实证字号的是 0x2010C4A0 = 90px(配方 ldr 0x2010D12C -> style_get_text_font ->
/// style_set_font 0x2010C4A0)。
pub const EXTRA_FALLBACK_SIZE: u32 = 28;
pub const EXTRA_SIZE_C4A0: u32 = 90;
// 页面本地样式(0x2011xxxx 区): 行文字的字体最终落在这些对象上。
// 静态核对到的列表行构造是
//   ldr r0,=<全局样式 0x2010CE74> ; bl 0x0C4BF87C(取该样式的 text_font 属性)
//   ldr r0,=<本地样式 0x20111C10> ; bl 0x0C587B38(写进本地样式)
// 即行文字最终用本地样式; 本地样式若开机就建好并缓存, 就不会再抄一次。
// 这 9 个是枚举"运行期被写字体的本地样式"得到的(style_set_font 的目标位落在 0x2011xxxx 区的那些)。
pub static LOCAL_STYLES: [u32; 9] = [
    0x20111C04, 0x20111C10, 0x2011338C, 0x20113398, 0x201133A4,
    0x20118D44, 0x20118D50, 0x20118E38, 0x20118E44,
];
/// 上面 9 个本地样式**各自的字号**(与 LOCAL_STYLES 一一对应)。
///
/// 来源: 固件里每个本地样式的字体都是"从某个全局样式抄来的", 字号即来源全局样式的
/// 字号。逐条静态核对每个本地样式的写入来源(结果与 132 表零冲突):
///   0x20111C04 <- 0x2010CE74(32)   0x20111C10 <- 0x2010CE74(32)
///   0x2011338C <- 0x2010CE5C(28)   0x20113398 <- 0x2010CE74(32)
///   0x201133A4 <- 0x2010CE74(32)   0x20118D44 <- 0x2010CEF8(28)
///   0x20118D50 <- 0x2010CEF8(28)   0x20118E38 <- 0x2010CEF8(28)
///   0x20118E44 <- 0x2010CEF8(28)
/// 重要: 不能对全部本地样式写同一个固定字号的 face —— 32 与 28 混在一起,
/// 统一写 28 会把 32 的那批整条缩小。
pub static LOCAL_SIZES: [u32; 9] = [
    32, 32, 28, 32, 32,
    28, 28, 28, 28,
];
// 登记的名字用 Chaos 前缀(系统没见过) -> face 缓存必未命中 -> 真的读我们的文件
// (沿用 MiSans-* 必然命中开机缓存, 写回等于原样回写 —— 最初几版无效的真根因)。
// 只登记这一条: 我们那份文件只有一个字重, 132 表里所有被改的样式都取同一张脸,
// 粗体请求也一样落到它上面(实测就是这个效果)。以前按字重登记 8 条是误判,
// 根因是把 BOOT_FONT_TABLE 的 flag=0 读成了"粗体条目" —— 那批其实是 SportVF/泰语。
//
// 投递落盘换文件时这条名字会跟着换(commit -> "ChaosSans-R<代>"), 所以它是 static mut。
// 长度 32 是给代号后缀留的余量(代号十进制追加, 位数远用不满)。
pub(crate) const C_REG_LEN: usize = 32;
pub(crate) static mut C_REG: [u8; C_REG_LEN] = c_reg_init();

const fn c_reg_init() -> [u8; C_REG_LEN] {
    let mut a = [0u8; C_REG_LEN];
    let s = b"ChaosSans-Regular";
    let mut i = 0usize;
    while i < s.len() { a[i] = s[i]; i += 1; }
    a
}
/// 开机字体表(lvx_theme_default_init 区字节级枚举, **132 条**真全量):
/// 每条 = (style 对象地址, 字号, 来源字重 id 是否为 MiSans*/MiSansF*)
/// 生成方式: 在 lvx_theme_default_init 区逐条枚举字体写入调用, 取到全部 132 条。
/// 注意: 早先只收到 82 条, 漏掉的 34 条 id0=MiSans-Demibold(CPU 行/页面标题/
/// 文件管理器走它们), 只改 82 条会出现"部分文字变了部分没变"。
/// **flag=0 的 24 条 = id7 MiSans-SportVF(21) + id9 BaiJamjuree-SemiBold(3)**
/// —— 运动大号数字与泰语字形, 一律保留原字体不动(它们不是"粗体条目", 别按字重去换)。
pub static BOOT_FONT_TABLE: &[(u32, u32, u32); 132] = &[
    (0x2010D324, 0x5A, 1),
    (0x2010D318, 0x54, 1),
    (0x2010D30C, 0x50, 1),
    (0x2010D300, 0x46, 1),
    (0x2010D2F4, 0x44, 1),
    (0x2010D2E8, 0x40, 1),
    (0x2010D2DC, 0x38, 1),
    (0x2010D2D0, 0x30, 1),
    (0x2010D2C4, 0x1C, 1),
    (0x2010D2B8, 0x56, 1),
    (0x2010D2AC, 0x54, 1),
    (0x2010D2A0, 0x50, 1),
    (0x2010D294, 0x48, 1),
    (0x2010D288, 0x44, 1),
    (0x2010D27C, 0x40, 1),
    (0x2010D270, 0x3C, 1),
    (0x2010D264, 0x3A, 1),
    (0x2010D258, 0x30, 1),
    (0x2010D24C, 0x2C, 1),
    (0x2010D240, 0x28, 1),
    (0x2010D234, 0x24, 1),
    (0x2010D228, 0x20, 1),
    (0x2010D21C, 0x1E, 1),
    (0x2010D210, 0x1C, 1),
    (0x2010D204, 0x18, 1),
    (0x2010D1F8, 0x16, 1),
    (0x2010D1EC, 0x82, 1),
    (0x2010D1E0, 0x48, 1),
    (0x2010D1D4, 0x40, 1),
    (0x2010D1C8, 0x34, 1),
    (0x2010D1BC, 0x30, 1),
    (0x2010D1B0, 0x28, 1),
    (0x2010D1A4, 0x24, 1),
    (0x2010D198, 0x22, 1),
    (0x2010D18C, 0x20, 1),
    (0x2010D180, 0x1E, 1),
    (0x2010D174, 0x1A, 1),
    (0x2010D168, 0x10, 1),
    (0x2010D15C, 0x90, 1),
    (0x2010D150, 0x78, 1),
    (0x2010D144, 0x64, 1),
    (0x2010D138, 0x5E, 1),
    (0x2010D12C, 0x5A, 1),
    (0x2010D120, 0x56, 1),
    (0x2010D114, 0x54, 1),
    (0x2010D108, 0x50, 1),
    (0x2010D0FC, 0x48, 1),
    (0x2010D0F0, 0x46, 1),
    (0x2010D0E4, 0x44, 1),
    (0x2010D0D8, 0x40, 1),
    (0x2010D0CC, 0x3C, 1),
    (0x2010D0C0, 0x3A, 1),
    (0x2010D0B4, 0x38, 1),
    (0x2010D0A8, 0x36, 1),
    (0x2010D09C, 0x34, 1),
    (0x2010D090, 0x30, 1),
    (0x2010D084, 0x2C, 1),
    (0x2010D078, 0x28, 1),
    (0x2010D06C, 0x24, 1),
    (0x2010D060, 0x22, 1),
    (0x2010D054, 0x20, 1),
    (0x2010D048, 0x1E, 1),
    (0x2010D03C, 0x1C, 1),
    (0x2010D030, 0x1A, 1),
    (0x2010D024, 0x18, 1),
    (0x2010D018, 0x16, 1),
    (0x2010D00C, 0x14, 1),
    (0x2010D000, 0x10, 1),
    (0x2010CFF4, 0x0C, 1),
    (0x2010CFE8, 0x12, 1),
    (0x2010CFDC, 0x0E, 1),
    (0x2010CFD0, 0x80, 1),
    (0x2010CFC4, 0x78, 1),
    (0x2010CFB8, 0x58, 1),
    (0x2010CFAC, 0x54, 1),
    (0x2010CFA0, 0x50, 1),
    (0x2010CF94, 0x46, 1),
    (0x2010CF88, 0x40, 1),
    (0x2010CF7C, 0x3C, 1),
    (0x2010CF70, 0x32, 1),
    (0x2010CF64, 0x38, 1),
    (0x2010CF58, 0x30, 1),
    (0x2010CF4C, 0x2E, 1),
    (0x2010CF40, 0x2C, 1),
    (0x2010CF34, 0x28, 1),
    (0x2010CF28, 0x24, 1),
    (0x2010CF1C, 0x22, 1),
    (0x2010CF10, 0x20, 1),
    (0x2010CF04, 0x1E, 1),
    (0x2010CEF8, 0x1C, 1),
    (0x2010CEEC, 0x1A, 1),
    (0x2010CEE0, 0x18, 1),
    (0x2010CED4, 0x16, 1),
    (0x2010CEC8, 0x14, 1),
    (0x2010CEBC, 0x11, 1),
    (0x2010CEB0, 0x48, 1),
    (0x2010CEA4, 0x46, 1),
    (0x2010CE98, 0x38, 1),
    (0x2010CE8C, 0x28, 1),
    (0x2010CE80, 0x22, 1),
    (0x2010CE74, 0x20, 1),
    (0x2010CE68, 0x1E, 1),
    (0x2010CE5C, 0x1C, 1),
    (0x2010CE50, 0x1A, 1),
    (0x2010CE44, 0x18, 1),
    (0x2010CE38, 0x14, 1),
    (0x2010CE2C, 0x24, 1),
    (0x2010CE20, 0x11, 1),
    (0x2010CE14, 0x14, 0),
    (0x2010CE08, 0x16, 0),
    (0x2010CDFC, 0x18, 0),
    (0x2010CDF0, 0x1C, 0),
    (0x2010CDE4, 0x20, 0),
    (0x2010CDD8, 0x24, 0),
    (0x2010CDCC, 0x28, 0),
    (0x2010CDC0, 0x2C, 0),
    (0x2010CDB4, 0x30, 0),
    (0x2010CDA8, 0x32, 0),
    (0x2010CD9C, 0x38, 0),
    (0x2010CD90, 0x3A, 0),
    (0x2010CD84, 0x3C, 0),
    (0x2010CD78, 0x40, 0),
    (0x2010CD6C, 0x48, 0),
    (0x2010CD60, 0x4E, 0),
    (0x2010CD54, 0x50, 0),
    (0x2010CD48, 0x58, 0),
    (0x2010CD3C, 0x60, 0),
    (0x2010CD30, 0x78, 0),
    (0x2010CD24, 0x80, 0),
    (0x2010CD18, 0x20, 0),
    (0x2010CD0C, 0x28, 0),
    (0x2010CD00, 0x50, 0),
];

// ===== 派生样式: 行控件挂在 part 上的那批"开机抄件" =====
//
// 固件在构造列表行控件时, 把主题样式的字体**抄进一批内嵌样式**再挂到行上：
//   0x0C4B9D96 r0=0x2010CE2C ; bl 0x0C4BF87C(取 text_font)
//   0x0C4B9DA2 r0=r5+0x1f0   ; bl 0x0C587B38(写进派生样式)     ; r5 = 0x2010C1C0
//   ... 同型共 23 处(0x0C4B8FA6 .. 0x0C4BA52C; 只扫一段会漏前头 6 处, 要全镜像重扫)
//   0x0C4B9E66/68 `bl 0x0C5889F0(obj, 0x2010C3B0, part=0)`      = obj_add_style
//   0x0C4B9E72/74 -> part 0x90000; 0x0C4B9E7C/80 -> 0xa0000; 0x0C4B9E88/8A -> 0xb0000
// 三件证据齐全(style_init + 固件自己的 style_set_font + obj_add_style), 所以它们确实是
// lv_style_t, 写 text_font 与 132 表同类, 不落在"对非样式对象调写接口"那条禁区内。
//
// 关键是时机: 外层有"只建一次"门(`[0x2010C1C0+0x1ec] == 0`), 这批样式在**开机时**
// 就抄走了当时的字体 —— 那时我们的样式级写回还没跑, 抄到的是旧 MiSans face, 之后
// 永不刷新。而 `row_update` 烘字体读的是行的 part 0x90000/0xa0000
// (0x0C4C89D0/0x0C4C89DE 读属性 0x5A -> 0x0C4C8A34 烘进标签) => 这正是"已经建好的
// 行控件仍从 MiSans 闪成我们的字体"的来源。
//
// 字号与字重**运行时按来源样式反查**(不手抄): 来源查不到就跳过并计数, 绝不猜尺寸。
// 0x2010C4A0 不在此列 —— 它已在 EXTRA_STYLES 里(同 90px, 同一张 face)。
//
// 这批地址来自全镜像重扫 bl 0x0C587B38, 补上旧枚举漏掉的写回点。旧枚举
// 只认"miwear_font_create 直调 + ldr style 字面量"完整模式,
// 而 face 来自 0x0C4BF87C 现抄的写回点全部漏掉 —— 0x0C4BF87C 是"从 style 读 prop 0x5A"
// 的取值封装(全固件 1265 处调用), 固件到处在用"从主表抄脸建自己的样式"这一招:
//   - 49 个名单外调用点中 38 个是"从主表现抄 face 写进自己的静态样式";
//     其余 11 个在 0x0C92 字体引擎的运行期属性解析里(样式来自寄存器, 无静态可写)。
//   - 本表(0x2010C1C0 派生表)补 5 条漏网: 0x2010C254 / 0x2010C32C / 0x2010C338 /
//     0x2010C348 / 0x2010C354(调用点 0x0C4B8FA6..0x0C4B9982, 与原 16 条同一次 init、
//     同一道"只建一次"门)。
//   - 另一张同构表在 sport_widgets_styles_apply(0x0C5D4EDC, 基址 0x20117968, 守卫
//     [0x20117968] = 一次性 init): 0x20117990 <- CE5C(28) 与 0x20117A5C <- CF10(32)
//     两条该换; 0x201179A8 <- CD78(64) 与 0x201179B4 <- CD9C(56) 的来源是 SportVF,
//     红线保留不写。0x20111C04/10 与 0x2011338C/98/A4 五条与 LOCAL_STYLES 重合
//     (地址与字号逐条对上, 互为验证), 不重复写。
static DERIVED_STYLE_PAIRS: [(u32, u32); 23] = [
    (0x2010_C3B0, 0x2010_CE2C), (0x2010_C3BC, 0x2010_CF28),
    (0x2010_C3C8, 0x2010_CE5C), (0x2010_C3D4, 0x2010_CE5C),
    (0x2010_C3E4, 0x2010_CF10), (0x2010_C3F4, 0x2010_CF1C),
    (0x2010_C404, 0x2010_D12C), (0x2010_C414, 0x2010_CF34),
    (0x2010_C424, 0x2010_D180), (0x2010_C434, 0x2010_D180),
    (0x2010_C444, 0x2010_CE74), (0x2010_C460, 0x2010_CE2C),
    (0x2010_C46C, 0x2010_CE2C), (0x2010_C478, 0x2010_CE5C),
    (0x2010_C484, 0x2010_CE68), (0x2010_C490, 0x2010_CE74),
    // --- 补: 0x2010C1C0 派生表原先漏掉的 5 条(字号 = 来源主表条目实证) ---
    (0x2010_C254, 0x2010_CE80), (0x2010_C32C, 0x2010_CF64),
    (0x2010_C338, 0x2010_CFA0), (0x2010_C348, 0x2010_CF28),
    (0x2010_C354, 0x2010_CF10),
    // --- 补: sport_widgets 缓存表(0x20117968 基址) 2 条, SportVF 两条按红线不写 ---
    (0x2011_7990, 0x2010_CE5C), (0x2011_7A5C, 0x2010_CF10),
];

/// 在开机字体表里查某个样式对象的 (字号, 字重标志)。查不到 = 不猜。
fn boot_entry(style: u32) -> Option<(u32, u32)> {
    let mut i = 0usize;
    while i < BOOT_FONT_TABLE.len() {
        let (s, sz, flag) = BOOT_FONT_TABLE[i];
        if s == style { return Some((sz, flag)); }
        i += 1;
    }
    None
}

// ===== 缓存节点投毒: 已被否决的做法, 记在这里防止再走一遍 =====
//
// 原做法: 把 108 个开机缓存节点的"载荷指针"(节点+0x00)改指到我们字体节点的载荷,
// 让固件之后每次命中缓存拷走的都是我们的字体 => 新建行第一帧就是它, 不闪。
// 实测后果: 应用后多处变卡、快应用更严重、运动选项目点 GO 直接崩溃。
//
// 根因(字节级): 载荷的**唯一所有者是缓存节点**。
//   vg_font_destroy(0x0C860478) 先顺着 块+0x1C 把 fallback 链整条交给
//   0x0C85F8FC 逐个减引用, 再把块自己减引用; 0x0C85F8FC 里
//   `[节点+0x2c] -= 1`, `ble` 成立(归零)且名字不是 "Emoji"(0x0C1F0C48 比 "Emoji",5)
//   => `bl 0x0C85F2A8` 尾调 0x0C589348(**释放节点+0x00 的载荷**), 然后节点+0x00 清 0、
//   节点摘链并 lv_free。
//   而命中缓存时拷的是 0x24 字节**浅拷贝**(0x0C85F716 ldm/stm), 块+0x1C 的 fallback
//   链头就是从载荷里抄来的 —— 一个载荷被 N 个节点共用时, 这 N 份块各自认为自己独占
//   那条链: 谁销毁谁就去减同一批 fallback 节点的引用计数, 归零即释放。
//   投毒把 6 个 MiSans-* 节点(同字号不同名字)和我们的 Chaos 节点全并到同一载荷上,
//   所以固件正常的建/毁(快应用与表盘动态文字都有 vg_font_destroy 调用点:
//   0x0C921086/0x0C9210CE/0x0C9212B2/0x0C92158A/0x0C9217C0 与 0x0CA8CCF0/0x0CA8D0CA/
//   0x0CA8D1E8/0x0CA8FF74)会成倍超发引用, 结果是**释放仍在 192 个样式里被引用的
//   字体对象** —— 这就是卡顿(度量/字形读成垃圾后反复重绘)与崩溃(释放后使用/双释放)同源。
//
// 结论: 只要载荷被第二个所有者拿到, 引用计数就不再可靠, 加计"钉住引用"也救不了
// 那条共享的 fallback 链。样式级写回(我们只往固件样式里放**我们独占名字**的脸)与
// 逐对象补写都没有这个问题 —— 固件不会按 MiSans 名字去销毁我们的脸。
// 代价(明确接受): 新开的页里行主标题会先 MiSans 后我们的字体, 由逐对象补写在 100ms 内追平。

// ---------------------------------------------------------------------------
// 前置条件
// ---------------------------------------------------------------------------

/// 字体管理器就绪: `lv_global`(= 管理器单例)第一个字 -> 路径管理器 `+0x1C`。
/// 两个都要像真指针才能往下登记/建 face —— 开机早期这两个字还是 0, 直接进固件
/// 的 add/create 就是让它解引用空指针。
pub(crate) unsafe fn manager_ready() -> bool {
    let fm = rd32(LV_GLOBAL as *const u32);
    if !safe_ptr(fm) { return false; }
    let pm = rd32((fm + 0x1C) as *const u32);
    plausible_ptr(pm)
}

// ---------------------------------------------------------------------------
// 换字体**不在 LVGL 事件派发链里做**
// ---------------------------------------------------------------------------
//
// 两条路的行为差别: 走投递包换字体不崩, 走界面换必崩。逐行比对之后
// 有两处实质差异, 这里先消除**第一处**:
//
//   投递包:  font_pack.lua 写 st1.ttf → io.open("/dev/chaos","wb") → chaos_write()
//            → 0x30 分支 → commit(1) + request()
//   界面:    page5 行点击 → 事件派发链 → click_entry → commit(1) + request()
//
//   **差异(这一处消除的): 投递包的 commit 发生在 VFS 写驱动的调用上下文里, 而界面那次
//   发生在 LVGL 的事件派发链里** —— 后者等于在固件正在派发一个事件的过程中, 去改
//   字体管理器的身份(登记名 + 池位 + face 台账), 而驱动层那条路还额外有 WRITE_BUSY
//   重入保护。所以现在改成: 点击**只记选择**(一个字节都不碰字体管理器、不碰任何对象),
//   由 tick(50ms 定时器上下文, 在事件派发之外)那一拍才 commit + request。
//
// 好处是它同时把另一件事也做对了: 点完之后还会在这个页面上停留一会儿, 而身份只在
// tick 里改 —— 那一刻没有"正在派发的事件"被打断。
/// 待切换的槽位(0 = 没有)。界面只写它, 真正落地在 pending_tick。
static mut FA_PENDING: u32 = 0;

/// 界面点某一条: **只记选择**。不碰字体管理器、不碰任何 LVGL 对象。
pub(crate) unsafe fn set_pending(slot: u32) -> bool {
    if slot < 1 || slot > FONT_SLOT_MAX { return false; }
    if slot == st_rd!(FA_LIVE) { return false; }
    st_wr!(FA_PENDING, slot);
    // 界面刷新不在这里排: 调用方(点击路径)用 click_feedback 原地刷新 ——
    // 这里排 FL_ROWS_REQ 会造成"每次点选都整页重建", 连点会砸进重建窗口(实测表现为退回上一级)。
    true
}

pub(crate) unsafe fn pending_get() -> u32 { st_rd!(FA_PENDING) }

/// 撤销未落地的切换请求(删除"刚点过切换"的那份字体时调)。
/// 文件都删了, 不能在退场时再应用它; 只在指向同一槽号时清。
pub(crate) unsafe fn cancel_pending(slot: u32) {
    if st_rd!(FA_PENDING) == slot { st_wr!(FA_PENDING, 0); }
}

/// 一拍一次: 把"点击时记下的选择"真正落地。
///
/// 要点: commit + request 从点击回调(事件派发链)搬到 tick
/// (50ms 定时器上下文) —— 与投递包那条路的"不在事件派发里改字体管理器身份"对齐。
/// 除上下文之外不做任何额外判断: 点完仍停在这一页也照样生效,
/// 只是生效的那一瞬间不在固件的事件派发过程中。
pub(crate) unsafe fn pending_tick() {
    let slot = st_rd!(FA_PENDING);
    if slot == 0 { return; }
    if busy() || st_rd!(FA_REQ) != 0 { return; }    // 上一次还在跑: 等它收尾(不做半途切换)
    if !manager_ready() { return; }                 // 管理器没就绪: 下一拍再试
    if !fw_api::screen_is_on() { return; }          // 息屏不做(与 icon/font 其它跑批同纪律)
    // 注意: 消掉全部"原地改行"的动作之后, 点选切换**仍然崩**。
    // 唯一剩下的差异就只有"我们的页活着"这一条 —— 所以应用在前台时**不落地**, 等 nav_back
    // 退出应用之后, 下一拍才 commit + 应用。落地后的整条链(commit → 分拍写回 →
    // apply_finish → 自动补写)全部发生在没有我们的页活着的时候, 与投递包那条从不崩的
    // 路完全同形。
    if st_rd!(APP_FG) != 0 { return; }
    st_wr!(FA_PENDING, 0);
    commit(slot);                                   // ← 在"应用不在前台"的拍里改身份
    request();
}

// ---------------------------------------------------------------------------
// 样式级写回
// ---------------------------------------------------------------------------

/// 点一次"重新应用字体" => 排好队列, 之后每拍按额度消费(见 FA_STAGE/FA_FACE_CAP)。
/// 返回 false = 管理器还没就绪(下一拍再试)。
pub(crate) unsafe fn apply_start() -> bool {
    if !manager_ready() { return false; }
    st_wr!(FA_C_REG, 0);
    st_wr!(FA_C_KEEP, 0);
    st_wr!(FA_C_DER0, 0);
    st_wr!(FA_C_FACE, 0);
    st_wr!(FA_C_FACE0, 0);
    st_wr!(FA_C_WR, 0);
    st_wr!(FA_JOB_N, 0);
    st_wr!(FA_CUR, 0);
    // 1) 登记我们的名字 -> 字体文件(只这一条, 见 C_REG 注释)。
    //    路径取当前在用池位 —— 投递落盘会切池位并换名字, 两个必须一起变。
    //    恢复模式: 登记到**系统字体文件** —— 除文件路径外与常规应用逐字
    //    同构(独占名字 -> 缓存必未命中 -> 全新建脸, 独占载荷)。不走 (MiSans-*,size)
    //    缓存命中: 那会拿到开机 face 的浅拷贝块, fallback 链头指向共享载荷, 样式全
    //    都变成共享载荷的第二所有者 —— 就是上面"投毒"那节的引用计数坑,
    //    实测后果是"恢复之后崩溃重启"。
    let fp = if st_rd!(FA_REVERT) != 0 { FA_SYSFONT.as_ptr() } else { file_path() };
    fw_api::fw_font_reg(core::ptr::addr_of!(C_REG) as *const u8, fp);
    st_wr!(FA_C_REG, 1);
    // 2) 四组样式对象排成一条队列, 之后按拍消费(额度见 FA_FACE_CAP/FA_JOB_CAP)。
    //    132 表: flag=0 的 24 条来源是 MiSans-SportVF / BaiJamjuree, 按设计保留原字体
    //    (这批不是"粗体条目", 换成我们的字体正好盖掉运动页 96/120/128 的大号数字)。
    let push = |style: u32, sz: u32| {
        let n = st_rd!(FA_JOB_N) as usize;
        if n < FA_JOB_MAX {
            st_wr!(FA_JOB_ST[n], style);
            st_wr!(FA_JOB_SZ[n], sz);
            st_wr!(FA_JOB_N, n as u32 + 1);
        }
    };
    for &(style, sz, flag) in BOOT_FONT_TABLE.iter() {
        if flag == 0 { st_wr!(FA_C_KEEP, st_rd!(FA_C_KEEP) + 1); continue; }
        push(style, sz);
    }
    let mut k = 0usize;
    while k < EXTRA_ACTIVE {
        let style = EXTRA_STYLES[k];
        let sz = if style == 0x2010_C4A0 { EXTRA_SIZE_C4A0 } else { EXTRA_FALLBACK_SIZE };
        push(style, sz);
        k += 1;
    }
    let mut j2 = 0usize;
    while j2 < LOCAL_STYLES.len() {
        push(LOCAL_STYLES[j2], LOCAL_SIZES[j2]);
        j2 += 1;
    }
    // 派生样式: 字号按来源样式在开机表里的记录取; 来源查不到 => 跳过不猜;
    // 来源是 SportVF/泰语(flag=0)的一并跳过, 与 132 表同一条红线。
    let mut d = 0usize;
    while d < DERIVED_STYLE_PAIRS.len() {
        let (dest, src) = DERIVED_STYLE_PAIRS[d];
        d += 1;
        match boot_entry(src) {
            Some((_, 0)) => st_wr!(FA_C_KEEP, st_rd!(FA_C_KEEP) + 1),
            Some((sz, _)) => push(dest, sz),
            None => st_wr!(FA_C_DER0, st_rd!(FA_C_DER0) + 1),
        }
    }
    st_wr!(FA_STAGE, 1);
    true
}

/// 每拍推进应用队列。返回 true = 全部做完(调用方置 FA_OK 并重建前台页)。
pub(crate) unsafe fn apply_step() -> bool {
    if st_rd!(FA_STAGE) == 0 { return false; }
    if st_rd!(FA_STAGE) == 1 {
        // 额度只按"新脸张数"计: 台账命中=只写一个指针(便宜, 可连着过 FA_JOB_CAP 条),
        // 新建一张脸=FreeType 现读那份 3.4MB TTF 的表(贵), 一拍到 FA_FACE_CAP 张就收。
        let mut n = 0u32;
        let mut nf = 0u32;
        while st_rd!(FA_CUR) < st_rd!(FA_JOB_N) && n < FA_JOB_CAP && nf < FA_FACE_CAP {
            let i = st_rd!(FA_CUR) as usize;
            let style = st_rd!(FA_JOB_ST[i]);
            let sz = st_rd!(FA_JOB_SZ[i]);
            st_wr!(FA_CUR, i as u32 + 1);
            n += 1;
            let before = st_rd!(FA_C_FACE);
            let f = fa_face_of(core::ptr::addr_of!(C_REG) as *const u8, sz);
            if st_rd!(FA_C_FACE) != before { nf += 1; }   // 这一条真建了脸
            if f == 0 {
                st_wr!(FA_C_FACE0, st_rd!(FA_C_FACE0) + 1);
                continue;
            }
            fw_api::fw_style_set_font(style, f);
            st_wr!(FA_C_WR, st_rd!(FA_C_WR) + 1);
        }
        if st_rd!(FA_CUR) < st_rd!(FA_JOB_N) { return false; }
        st_wr!(FA_STAGE, 2);
    }
    // 2) 认脸表: 走一遍固件字体缓存清单(见 learn_from_cache 的注释)。
    //    恢复模式同样要跑: 它把旧代的脸与 MiSans 系脸都映射到恢复出来的
    // 系统脸, 补写才有"该换成谁"的依据。
    if st_rd!(FA_STAGE) == 2 {
        if !learn_from_cache() { return false; }
        st_wr!(FA_STAGE, 3);
    }
    st_wr!(FA_STAGE, 0);
    true
}

/// 1 = 本次点按排过应用队列, 还没做收尾重建。
/// 它把"开始应用"与"真做完"两件事分开: apply_step 的返回值只说明"这一拍推进了",
/// 队列还有剩时它也返回 true —— 拿它当重建的门就会在队列没走完时重建(那一刻必崩)。
static mut FA_QUEUED: u32 = 0;

// ---------------------------------------------------------------------------
// 开机自动应用(等多少拍、试几次的门见 tick 的说明)
// ---------------------------------------------------------------------------

/// 首次自动应用前等多少拍(50ms 一拍 => 100 拍 = 5 秒)。
/// 依据: 安装器解包与 lvx_theme_default_init 都在这个窗口里, 早了管理器还没起来。
const FA_FIRST_WAIT: u32 = 100;
/// 一次失败后隔多少拍再试
const FA_RETRY: u32 = 40;
/// 自动最多试几次(试满就不再自己试, 手动按钮仍可 —— 失败只重试, 永不禁用功能)
const FA_TRY_MAX: u32 = 3;
static mut FA_WAIT: u32 = FA_FIRST_WAIT;
static mut FA_TRY: u32 = 0;
/// 自动应用是否已排过队列(排上之后交给分拍队列跑, 不再重复排)
static mut FA_BOOT_Q: u32 = 0;

/// 开机自动应用: 一拍一次, 只做"要不要开始"的判断; 真正的建脸/写样式仍走分拍队列。
unsafe fn boot_auto_tick() {
    // 手动请求优先(调用方已在上一步消费 FA_REQ), 这里只管自动那一条。
    if st_rd!(FA_REQ) != 0 || st_rd!(FA_STAGE) != 0 || st_rd!(FA_BOOT_Q) != 0 { return; }
    if st_rd!(FA_OK) != 0 || st_rd!(FA_TRY) >= FA_TRY_MAX { return; }
    let w = st_rd!(FA_WAIT);
    if w > 0 { st_wr!(FA_WAIT, w - 1); return; }
    st_wr!(FA_TRY, st_rd!(FA_TRY) + 1);
    // 两道就绪门: 文件在位 + 管理器可解引用。取不到就当"还没起来", 隔一会儿再试。
    if !file_ready() || !manager_ready() {
        st_wr!(FA_WAIT, FA_RETRY);
        return;
    }
    // 排上队列: 置 FA_REQ, 由下面的分拍路径消费(与手动点按钮完全同一条路)
    st_wr!(FA_REQ, 1);
    st_wr!(FA_BOOT_Q, 1);
}


/// 收尾重建那一刻的拍号(0 = 还没收尾)
static mut FA_DONE_AT: u32 = 0;

/// 补写任务的"等窗口"旗标(1 = 排着, 等应用退场 + 静置够拍数再 arm)。
/// 为什么必须等: 逐对象补写要写活对象, 点完还停在页 5 的时候, 引擎正在重画我们
/// 这一页 —— 实测的分界是"补写还在跑时操作 = 崩, 等它自停后再操作 = 不崩"。
/// 退场 + 静置之后做 = 与投递包那条路同形(没有我们的页活着, 而且不在导航窗口里)。
/// 它排的是**补写**(font_tree::arm), 不是逐条补差量。
static mut FA_DEL_PEND: u32 = 0;
/// 排上之后还要静置多少拍(50ms 一拍, 20 拍 = 1 秒)才开跑。
/// 依据: "页栈变了(正在开/关页)就就地收工", 因为那正是引擎在动树的窗口。
/// 应用刚退出 / 开机应用刚做完的那一刻就是这个窗口, 所以必须等它过去。
const FA_DEL_SETTLE: u32 = 20;
static mut FA_DEL_WAIT: u32 = 0;

/// 收尾。
///
/// **我们的页在前台时, 这一步一个对象都不碰**: 界面更新只排一次引擎重建
/// (`mark_page_dirty`/`render_req`), 逐对象补写只登记等窗口的旗标(下面 FA_DEL_PEND)。
/// 为什么不能在点选那一拍原地改行 —— `row_update` 不是"只改文字", 它仍会 set_text +
/// 给 trailing 做 set_state, 而 set_state 尾部走 `lv_obj_refresh_style`(全子树失效 +
/// 逐对象发样式变更事件); 在分拍建脸正在动树的那几拍里做这件事就是崩溃轴。
/// 逐字证据: 反汇编 row_update(0x0C4C8904) —— 标签已存在时它仍会 set_text(0x0C589490)
/// + 给 trailing 做 set_state(0x0C589288), 后者尾部走 lv_obj_refresh_style(0x0C1070AC)。
pub(crate) unsafe fn apply_finish() {
    // 恢复模式收尾: 只清 FA_REVERT, 其余照常 —— 认脸表已把旧代脸映射到
    // 恢复出来的系统脸, 补写必须排(它负责把补写过的对象从旧字体换成系统字体)。
    if st_rd!(FA_REVERT) != 0 {
        st_wr!(FA_REVERT, 0);
    }
    // 逐对象补写一律走"等窗口"这条路(不在这里直接 arm):
    //   收工条件是"正在开/关页 = 正在导航, 继续写就是同一崩法"。
    //   应用刚退出 / 开机自动应用刚做完的那一刻, 恰好就是这种导航窗口。
    //   这里排的是自动补写(font_tree::arm), 不是补差量 —— 见 tick 里的说明。
    st_wr!(FA_DEL_PEND, 1);
    st_wr!(FA_DEL_WAIT, FA_DEL_SETTLE);
    if st_rd!(APP_FG) == 0 {
        // 应用不在前台 = 没有我们的页活着, 与投递包那条路同形: 界面这边什么都不用做。
        return;
    }
    let fg = st_rd!(FG_PAGE) as usize;
    if fg > MAX_PID || PAGES[fg].built == 0 {
        return;
    }
    // 前台是我们自己的页: 这一拍起不碰任何对象。界面等引擎重建, 补差量等退场 + 静置。
    if fg == font_list::FONT_PID as usize {
        font_list::mark_page_dirty();     // 内含 render_req(5) + 把待重画请求结掉
    } else {
        render_req(fg);
    }
}

/// "可以不碰对象树"的窗口: 分拍队列没跑、没有待消费的请求、收尾也没排重建。
/// 界面侧(font_list)拿它决定什么时候重建页 5 —— 窗口没开就只登记请求, 不动对象。
pub(crate) unsafe fn quiet() -> bool {
    st_rd!(FA_STAGE) == 0 && st_rd!(FA_REQ) == 0 && st_rd!(FA_QUEUED) == 0
}

/// 正在应用(队列或认脸清单在跑)。投递包落地新字体前用它当互斥: 半途换池位会让
/// 正在建的那批脸读到另一个文件的字节。
pub(crate) unsafe fn busy() -> bool { st_rd!(FA_STAGE) != 0 }

/// 有没有我们这一路的活在推进(供节拍自适应用): 分拍队列、待消费的请求、收尾留下的
/// 重建、开机自动应用的等待/重试计数、"退场后补写"的等窗口。任何一条非 0 就别降频。
pub(crate) unsafe fn pending() -> bool {
    st_rd!(FA_STAGE) != 0 || st_rd!(FA_REQ) != 0 || st_rd!(FA_QUEUED) != 0
        || st_rd!(FA_DEL_PEND) != 0 || st_rd!(FA_WAIT) != 0
}

/// 取(或首次建立)该 (名字,字号) 的 face, 带复用台账。0 = 建不出来。
/// face 复用台账: `fw_mfont_create_named` **每次调用都新分配一块 0x28B 拷贝块**
/// (命中缓存也一样), 而 132 条表里有大量同 (名字,字号) 的条目,
/// 重复点"重新应用"更会成倍泄漏。这里按 (名字指针, 字号) 记一次结果, 之后全复用。
/// 重要: 失败结果只在**本次运行内**复用(同一遍里同组合不重试 = 防慢性 OOM),
/// 下一次点应用遇到失败槽会**重建一次并就地更新** —— 开机早期管理器没就绪
/// 造成的失败不该毒化整个本次开机(否则一次瞬时失败就让"重新应用"永远判不干净)。
unsafe fn fa_face_of(name: *const u8, size: u32) -> u32 {
    let np = name as u32;
    let mut i = 0usize;
    let n = st_rd!(FA_CCH_N) as usize;
    while i < n {
        if st_rd!(FA_CCH_NM[i]) == np && st_rd!(FA_CCH_SZ[i]) == size {
            let f = st_rd!(FA_CCH_FACE[i]);
            if f != 0 { return f; }
            break;      // 失败槽: 落到下面重建
        }
        i += 1;
    }
    let hit = i < n;
    // 硬上限: 台账满了就不再建脸(返回 0 => 这个字号不改)。每张脸都是一次 FreeType
    // 开文件 + 一块 0x28 泄漏 + 常驻度量, 不设上限就是"应用时建多少张全凭缓存里
    // 出现过多少字号", 分配和耗时都压不住。
    if !hit && (i as u32) >= FA_CCH_CAP { return 0; }
    let face = fw_api::fw_mfont_create_named(name, size, 0);
    // 重要: 建失败时固件**不返回 0**, 而是返回 flash 里的默认字体常量
    // LV_FONT_DEFAULT(0x2CCE1734, 见 vg_font_create_core 0x0C860438 的 `mov r0,r4`)。
    // 它落在 0x2C 只读域, plausible_ptr 拦不住 —— 写进样式就等于把这块位图默认字体
    // 烘到该尺寸的所有文字上(尺寸不对、字形不对)。宁可记一次失败、整表判不干净,
    // 也不写一个确定错的东西。(逐对象补写那边 ft_face_for 同一条判据。)
    let ok = if face == 0 || face == FW_FONT_DEFAULT || !plausible_ptr(face) {
        0
    } else {
        face
    };
    if hit {
        st_wr!(FA_CCH_FACE[i], ok);          // 失败槽就地更新, 不动槽位数
    } else if (i as u32) < FA_CCH_CAP {       // 满了不再建: 这张表就是脸数的硬上限
        st_wr!(FA_CCH_NM[i], np);
        st_wr!(FA_CCH_SZ[i], size);
        st_wr!(FA_CCH_FACE[i], ok);
        st_wr!(FA_CCH_N, i as u32 + 1);
    }
    if ok != 0 { st_wr!(FA_C_FACE, st_rd!(FA_C_FACE) + 1); }   // 只计新建, 复用不计
    ok
}

// ===== 认脸表: 从固件字体缓存清单登记 =====
//
// 另一种做法是按开机那 132 个 (名字,字号) 各建一次脸, 再拿对象上的 face 精确匹配。实测:
// 不崩了(派生解引用确实是崩溃源), 但**覆盖归零** —— 常驻页上那些"只有补写才变得了"的文字,
// 它们的 face 是中文回退链的 -All 族与运行期现建的 (名字,字号), 不在 132 表里, 精确匹配把
// 它们全判成认不出。放宽成"名字含 MiSans 就算"能覆盖到, 但那是不加区分的瞎认。
//
// 可读名字这件事不能回到对象身上做(那两跳派生解引用就是上面那条崩溃源)。折中点是:
// **只在点应用时对固件的字体缓存清单走一遍**。清单是固件自己管的单向链表, 表头就是
// `pm` 本体 —— `vg_font_create_core`(0x0C8603B0) 在 0x0C8603EC 用 `ldr r0,[r3,#0x1c]`
// 取到 pm 后直接把它当第一参交给 0x0C85F6CC 遍历(`bl 0xc589050` 取头/尾 + `bl 0xc588810`
// 移动), 链表元素必然是缓存节点(+0 载荷 / +4 名字 / +8 字号) —— 固件自己的匹配函数
// 0x0C860314 读的就是同两个字段。所以在清单上读名字是合法的、有界的; 而补写在每个对象
// 身上仍然只做 9 字比对, 一跳派生解引用都不走。
//
// 分类规则: 名字含 MiSans 且不含 Sport/BaiJ => 换成我们同字号的脸;
// SportVF/泰语 => 登记成 repl=0(认出来就是为了不动它); 其它族 => 不登记(等于不动)。
const FC_WALK_MAX: u32 = 512;        // 清单长度上限(防表被写坏时死循环)
const FC_SIZE_MIN: u32 = 8;          // 认脸的字号区间, 超出的一律不认
const FC_SIZE_MAX: u32 = 120;

/// 把节点+4 的名字读进 buf(上限 39)。含控制字符或空 => 返回 0(判不可信, 不动)。
unsafe fn fc_name(np: u32, buf: &mut [u8; 40]) -> usize {
    let mut n = 0usize;
    while n < 39 {
        let c = rd8((np + n as u32) as *const u8);
        if c == 0 { break; }
        if c < 0x20 || c >= 0x7f { buf[0] = 0; return 0; }
        buf[n] = c;
        n += 1;
    }
    buf[n] = 0;
    n
}

fn fc_has(s: &[u8], p: &[u8]) -> bool {
    if s.len() < p.len() { return false; }
    let mut i = 0;
    while i + p.len() <= s.len() {
        if s[i] == p[0] && &s[i..i + p.len()] == p { return true; }
        i += 1;
    }
    false
}

/// 字体文件在位吗? 只 open/close 一次, 不读内容、不解析 —— 给开机自动应用当第一道门用。
/// 内核 open 不是 POSIX: 只读标志是 1(不是 0)。
pub(crate) unsafe fn file_ready() -> bool {
    let fd = fw_api::open(file_path(), fw_api::oflag::RDONLY, 0);
    if fd < 0 { return false; }
    fw_api::close(fd);
    true
}

/// 走一遍固件字体缓存清单, 把每个节点的 9 字签名与替换目标登记进 font_tree。
/// 只读不改: 不写节点任何字段, 也不把我们的脸交给节点所有 => 与上面"投毒"那条是两回事。
/// 返回 false = 管理器/清单不可信(pm 读不到), 下次 apply 再试。
unsafe fn learn_from_cache() -> bool {
    let fm = rd32(LV_GLOBAL as *const u32);
    if !safe_ptr(fm) { return false; }
    let pm = rd32((fm + 0x1C) as *const u32);
    if !plausible_ptr(pm) { return false; }
    st_wr!(FA_C_ID, 0);
    st_wr!(FA_C_ID0, 0);
    st_wr!(FA_C_IDBIG, 0);
    st_wr!(FA_C_IDFAM, 0);
    st_wr!(SKIP_TAG, 0u8);        // 每次重新走清单都重记一个样本
    let mut node = fw_api::fw_list_head(pm);
    let mut k = 0u32;
    while node != 0 && k < FC_WALK_MAX {
        k += 1;
        if !safe_ptr(node) { break; }
        let payload = rd32(node as *const u32);
        let np = rd32((node + 4) as *const u32);
        let sz = rd16((node + 8) as *const u16) as u32;
        let mut nm = [0u8; 40];
        let nl = if safe_ptr(np) { fc_name(np, &mut nm) } else { 0 };
        if nl == 0 || !plausible_ptr(payload) {
            note_skip(b'B', b"", sz);                       // 名字读不出/没载荷: 不敢认
            st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
        } else {
            let name = &nm[..nl];
            if fc_has(name, b"Chaos") {
                // 我们自己的脸(ChaosSans-*, **含旧代**)也登记: 旧代的脸 -> 当前代的脸
                // (learn_owned 权威覆盖)。否则换字体/恢复之后, 补写过的对象
                // 永远停在旧代(实测: 切换不跟、恢复也不跟)。当前代自己 repl==自己,
                // learn_owned 覆盖为同值, 无害。仍不抢"第一个没登记上"的样本位。
                let repl = fa_face_of(core::ptr::addr_of!(C_REG) as *const u8, sz);
                if repl == 0 {
                    note_skip(b'X', name, sz);
                    st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
                } else if !font_tree::learn_owned(payload, repl) {
                    note_skip(b'T', name, sz);
                    st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
                }
            } else if fc_has(name, b"Sport") || fc_has(name, b"BaiJ") {
                store_ident(payload, 0, name, sz);          // 认出来就是为了不动它
            } else if !fc_has(name, b"MiSans") {
                note_skip(b'F', name, sz);                  // 第三种族: 不在承诺范围内
                st_wr!(FA_C_IDFAM, st_rd!(FA_C_IDFAM) + 1);
                st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
            } else if sz < FC_SIZE_MIN || sz > FC_SIZE_MAX {
                note_skip(b'S', name, sz);                  // 字号超出 FC_SIZE_MIN..MAX
                st_wr!(FA_C_IDBIG, st_rd!(FA_C_IDBIG) + 1);
                st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
            } else {
                let repl = fa_face_of(core::ptr::addr_of!(C_REG) as *const u8, sz);
                if repl == 0 {
                    note_skip(b'X', name, sz);              // 我们的脸建不出来
                    st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
                } else {
                    store_ident(payload, repl, name, sz);
                }
            }
        }
        node = fw_api::fw_list_next(pm, node);
    }
    true
}

/// 登记一条身份; 表满时不静默丢 —— 记 `T` 并计入跳过, 让"还差哪个字体"看得见。
unsafe fn store_ident(payload: u32, repl: u32, name: &[u8], sz: u32) {
    if font_tree::learn(payload, repl) {
        st_wr!(FA_C_ID, st_rd!(FA_C_ID) + 1);
    } else {
        note_skip(b'T', name, sz);
        st_wr!(FA_C_ID0, st_rd!(FA_C_ID0) + 1);
    }
}

/// 诊断用: 记下**第一个**没登记上的字体(名字 + 字号 + 原因字母)。
/// tag: F=第三种族 S=字号超出 8..120 B=名字或载荷读不出 X=我们的脸建不出来 T=认脸表满。
pub(crate) static mut SKIP_TAG: u8 = 0;
pub(crate) static mut SKIP_SZ: u32 = 0;
static mut SKIP_NAME: [u8; 24] = [0; 24];

unsafe fn note_skip(tag: u8, name: &[u8], sz: u32) {
    if st_rd!(SKIP_TAG) != 0 { return; }                   // 只记第一个
    st_wr!(SKIP_TAG, tag);
    st_wr!(SKIP_SZ, sz);
    let d = core::ptr::addr_of_mut!(SKIP_NAME) as *mut u8;
    let mut i = 0usize;
    while i < name.len() && i < 23 {
        write_volatile(d.add(i), name[i]);
        i += 1;
    }
    write_volatile(d.add(i), 0);
}

/// 美化页按钮的请求入口: 下一次 UI tick 真正执行。
/// 为什么不在点击回调里直接跑: 点击回调是固件的事件派发上下文, 里面做几秒量级的
/// 建 face + 写回会拖住派发链(同类操作在派发链里跑过就死锁/触发看门狗)。
/// 恢复系统字体进行中。1 = 应用队列在跑且逐条用 MiSans-Regular 解析;
/// apply_finish 收尾时清 0 —— font_list 的删除流水线拿它当"恢复完成"信号。
static mut FA_REVERT: u32 = 0;
pub(crate) unsafe fn revert_busy() -> u32 { st_rd!(FA_REVERT) }

/// 固件系统字体文件(0x2CB8825C, 别名域字符串; 固件开机就用它建 MiSans face)。
/// 恢复模式把 C_REG 登记到它 —— 建出来的脸与常规应用同构(独占名字/独占载荷)。
pub(crate) static FA_SYSFONT: &[u8] = b"/resource/font/MiSans-Regular.ttf\0";

/// 恢复系统字体(删"在用中的字体"前调): 登记名换代(commit 同款, 缓存必未
/// 命中)后排一次应用队列, apply_start 会把新名字登记到 FA_SYSFONT —— 建出来的脸
/// 从系统字体文件全新生成, 独占载荷(绝不拿共享载荷当第二个所有者, 见上面"投毒")。
/// 写回完成后样式全部指回系统字体, 此时删字体文件才不违反"face 会按需回读文件"。
pub(crate) unsafe fn request_revert() {
    if st_rd!(FA_REVERT) != 0 { return; }
    st_wr!(FA_REVERT, 1);
    let g = st_rd!(FA_GEN) + 1;
    st_wr!(FA_GEN, g);
    let d = core::ptr::addr_of_mut!(C_REG) as *mut u8;
    let mut w = W::new(d, C_REG_LEN);
    w.s(b"ChaosSans-R");
    w.n(g);
    w.end();
    // 复用台账必须清(commit 同款): 台账按**名字指针**比(fa_face_of 里 FA_CCH_NM[i] == np),
    // 而 C_REG 是固定地址的静态缓冲 —— 换代只改内容不改
    // 地址, 不清台账的话上一代的旧脸按指针直接命中, "恢复"会把旧字体原样写回。
    st_wr!(FA_CCH_N, 0);
    request();
}

/// 在用池位清零(在用中的那份被删除后调)。0 = 无在用字体(开机默认态);
/// 之后"重新应用字体"无文件可读会自然空转, 直到应用投递的新字体。
pub(crate) unsafe fn clear_live() { st_wr!(FA_LIVE, 0); }

pub(crate) unsafe fn request() {
    st_wr!(FA_REQ, 1);
}


/// 50ms UI tick 里调用: 消费手动请求 + **开机自动应用**。
///
/// 开机自动应用为什么必须有:
///   只靠界面点一下的话覆盖面永远不满 —— 很多常驻对象在点之前就已经建好了。
///   为什么必须开机就应用(实测出的因果, 不是推测):
///   文本的字体是**控件建的时候**从样式抄进对象自己的 local style 快照的, 所以
///   "换字体那一刻已经建好的界面"不会变 —— 投递新字体后小部件屏上的文字不变,
///   **重启一次就全部恢复**。
///   也就是说: 字体必须在"界面还没建出来"之前就位, 覆盖面才是满的。
///   开机自动应用正好满足这一点 —— 它在开机 5 秒后跑, 早于进任何界面。
///
/// 自动应用的门, 一条不减:
///   * 本次开机已成功(FA_OK)或已试满 FA_TRY_MAX 次 => 不再自动试(手动仍可);
///   * 首次延后 FA_FIRST_WAIT 拍(5 秒): 安装器解包、lvx_theme_default_init 都在这个窗口里;
///   * 动手前两道就绪门: file_ready() + manager_ready() —— 开机早期这两个是 0,
///     直接进固件的 add/create 等于送空指针进去;
///   * 失败只重试, **永不禁用功能**。
/// 它走的是**分拍**队列(apply_start/apply_step), 不是一拍跑完 ——
/// 单拍建几十张脸会按住 UI 线程几百毫秒到几秒(实测: 看门狗黑屏重启)。
pub(crate) unsafe fn tick() {
    // 0) 先处理"待切换" —— 真正换字体身份的那一下。只在"界面真的不在了"时做:
    //    这是把投递包那条路的条件(没有我们的页活着 + 不在事件派发链里)搬过来。
    pending_tick();
    // 0.5) 开机自动应用(见上面说明)。手动请求优先, 所以在它之前消费。
    boot_auto_tick();
    // 管理器没就绪时 apply_start 返回 false, 请求**不清零** => 下一拍重试到就绪为止
    // (早于就绪时它一条固件接口都不碰, 重试不会重复登记名字)。
    if st_rd!(FA_REQ) != 0 && st_rd!(FA_STAGE) == 0 {
        if apply_start() {
            st_wr!(FA_REQ, 0);
            st_wr!(FA_QUEUED, 1);
            st_wr!(FA_DONE_AT, 0);
        }
    }
    if st_rd!(FA_STAGE) != 0 {
        let done = apply_step();
        if done { st_wr!(FA_OK, 1); }
    }
    // 真做完(队列走空 + 没有新请求)的那一拍: 重建前台页, 并把本次标记清掉。
    // 只做这一次 —— 中途一拍都不重建。
    if st_rd!(FA_QUEUED) != 0 && !busy() && st_rd!(FA_REQ) == 0 {
        apply_finish();
        st_wr!(FA_QUEUED, 0);          // 收尾做完才算 settled(quiet() 用它当最后一道门)
        st_wr!(FA_DONE_AT, st_rd!(FA_DONE_AT) + 1);
        // 这里**不直接 arm** 补写: 刚收尾的这一拍正是引擎在拆建我们自己的页, 那一刻写
        // 活对象就是前面那条崩溃轴(引擎动树的窗口里写活对象)。改由下面的 FA_DEL_PEND 等
        // 应用退场 + 静置之后再排。
        // 覆盖代价明确: 切字体后**别的页**要退出重进才跟上(那些页的行本来就是固件建行时
        // 从我们改过的样式现烘的)。当场追不上的那一块交给下面那条等窗口的自动补写。
    }
    // "等窗口"旗标(**自动补写**): 应用退出前台(我们的页全部不在)之后**再静置 1 秒**,
    // 才排那一次逐对象补写。两段依据: 退场 = 与投递包那条不崩的路同形(没有我们的页
    // 活着); 静置 = "正在开/关页就别写"(刚退出的那一刻引擎正在拆我们的页)。
    // 应用字体的**每一条路**(手动"重新应用字体" /
    // 投递包 0x30 / 开机自动应用)收尾都走 apply_finish => 都会自动排一次补写, 把"已经建好
    // 的常驻对象"(表盘/小部件/桌面按钮的旧脸)当场换掉 —— 开机后不用再手动点行 9。
    // 补写覆盖的目标面是"还挂在对象上的旧脸", 但门比较全
    // (两趟指纹 + 渲染门 + 页栈锁定), 所以只留这一条路。
    // 额外一道门: 有"待切换"不排(切换落地后还会再走一遍 apply_finish, 别用旧脸补一遍)。
    if st_rd!(FA_DEL_PEND) != 0 && st_rd!(APP_FG) == 0 && st_rd!(FA_PENDING) == 0 && quiet() {
        let w = st_rd!(FA_DEL_WAIT);
        if w > 0 {
            st_wr!(FA_DEL_WAIT, w - 1);
        } else {
            st_wr!(FA_DEL_PEND, 0);
            crate::font_tree::arm();
        }
    }
}

// FA_C_* 这批计数不在界面显示: 它们不是纯打印量(每拍建脸额度、跳过判定都读它们),
// 拆出去要动很多地方而收益很小, 就留在本模块当诊断计数。

