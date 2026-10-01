// 逐对象补写文字字体 —— 解决"固件自带页面的行文字追不到"这最后一块。
//
// 样式级写回(182 个样式对象)改不到固件页的行, 因为那些行文字用的字体不在我们
// 能枚举到的样式里。实测唯一有效的供给方式是**给活对象自己写 local
// style**(LVGL 里 local 优先于一切 add style 与继承)。本模块在 UI 线程的
// lv_timer 上下文里遍历最上层页的对象树, 按每个对象**当前字号**换成我们的字体。
//
// 三步机制全部字节级实证:
//   读当前 face   : 0x0C589448(obj, 0, 0x5A), 固件 row_init 同款读法, 已实测验证;
//   从 face 取字号: face 是字体管理器的**拷贝块** —— 同 (名字,字号) 每次调用都
//                  新分配一块, 前 0x24B 是被引用的字体对象副本, **+0x24 指回缓存
//                  节点**(0x0C85F724 `str.w r4,[r8,#0x24]`; 删除路径 0x0C85F94C
//                  `ldr r5,[r4,#0x24]` 同读法反证)。节点 +4 = 字体名指针
//                  (指回节点内联区 +0xc), +8 = 字号 u16 —— 该字与 miwear_font_create
//                  传下的 req+4 同源, 终点是 create_font_warpper(path,render,size,style)
//                  第 3 参(0x0C85F816 `ldrh r2,[r5,#4]` + 0x0C85F818 `bl 0x0C587D38`)。
//                  重要: 因为 face 是拷贝, **不能拿指针身份比对字号**, 只能读节点。
//   写回          : 0x0C107790(obj, 0x5A, face, 0)。其尾部 0x0C107892 调
//                  lv_obj_refresh_style(obj, selector, prop), 而 0x5A 在属性标志表
//                  0x2CCE5BC4 里取值 0x05(带子树传播位) => **不需要额外补重绘**。
//
// 纪律:
//   1. 不扫地址区间(MPU 空洞), 只碰固件对象图里走得到的活对象;
//   2. 不缓存对象句柄 —— 每一趟的根都从固件页管理器现取(见 ft_page_root)。
//      "定时器比页面活得久"这条风险靠"每趟重取 + 指针门"消化, 不靠记住上次的对象;
//   3. **补写一律不建脸**。换成谁由认脸表(FT_ID_*)给出, 表是 font_apply 在
//      点"重新应用文楷"时一次性登记完的。理由: face 每建一次净泄漏 0x28
//      且要走 FreeType 开文件, 在遍历里按需建脸 = 在任意界面(可能正是内存峰值的运动
//      实时页)上让固件分配;
//   4. 只写叶子对象 —— 文字控件都是叶子, 往容器上写不产生可见文字只会放大重绘面;
//   5. **认不出身份的对象一律不动**(计 FT_C_NO)。签名不在表里 = 我们说不出这张脸是
//      谁 => 不猜、不试、不改; 补写只负责"报得出名字"的那批文字;
//   6. 一趟的写入量有预算(每次写都会让固件把整棵子树失效 + 发样式变更事件),
//      并且只碰"页身份门"认下来的页(page_kind == 2 且没排上销毁);
//   7. 两条红线(实测: 开补写进运动页点 GO = 黑屏重启, 关补写不崩):
//      a) "只写已有 selector==0 local 槽的对象"这条现在**只记账**(见 ft_has_local0):
//         带着这条仍然会在点 GO 时崩, 而把它当硬门会让一半覆盖消失,
//         所以分配不是崩溃轴; 计数只报"这次写需要新分配槽"的条数。
//      b) **遍历里一个字节都不写** —— 遍历只收集, 写入统一在 ft_walk 返回之后由
//         ft_flush 执行。运动实时页是"页根不变、子树自己重建"的分页视图
//         (sport_realtime_view_add_pages/lazy_loading_*/realtime_view_update_cb),
//         一次写入触发的回调可能把子对象删掉, 继续用栈里的父对象走下一步就是踩自由内存。
//   8. 红线: **树在变的时刻不写**。实测"补写还在跑时点 GO = 崩, 等它自停后再点 GO =
//      不崩" => 崩溃轴不是"改好的字体留在线上", 而是"正在写的那一刻"撞上引擎重建子树。
//      判据做成结构指纹门(见 backfill_tick 里的 fp 比对): 同一页连走两趟, 第二趟与
//      第一趟一字不变才放行这一批。只看页根稳不稳定的门挡不住这种树。
//   9. 红线: **补写是"一次性任务", 不是后台常驻**。触发点只有两个 —— 点"重新应用文楷"
//      (样式写回跑完后自动排一遍)与更换字体页的"补写一遍"按钮。
//      一次任务 = 从页栈栈底到栈顶逐页扫一遍, 扫完整条栈立刻自停。
//      理由: 既然崩在"正在写的那一刻", 就要让写只发生在主动要它发生的那一瞬间;
//      后台每 10 秒重扫 = 在开始锻炼的第 20 分钟突然去动那棵树, 那种崩溃没法预防。
//      代价(明确接受): 任务跑完之后再新开的页不再补写 —— 那些页的行本来就是固件从
//      我们改过的样式里现烘的(样式级写回与逐对象补写的分工), 真有新页不落文楷,
//      再手动跑一遍补写。
//
// ---------------------------------------------------------------------------
// 补写这条路径存在的理由: 实测补写不崩, 而样式级写回(23 条派生样式补齐后)仍盖不住
// 小部件屏与桌面"布局切换"按钮 —— 它们的 face 不经过我们写过的任何样式,
// 只有逐对象直写 local style 才够得到(补写的输出端)。本模块只做这一件事:
//   补写(FT_ON): 一次性任务, 由更换字体页的"补写一遍"按钮触发, 门控全在
//                (两趟指纹一致才写 / 渲染让路 / 页栈深度锁定 / 息屏与导航即收工 /
//                 遍历只收集、遍历外统一写 / 预算摊平失效面)。
//   与界面侧的窗口门互斥: `font_list::window_is_quiet()` 用 `backfill_running()` 让路。
// ---------------------------------------------------------------------------
//
// ---------------------------------------------------------------------------
// 选脸口径: 按同名义 px, 行高不参与决策
// ---------------------------------------------------------------------------
// 两种口径都实测过: 按同名义 px 选脸覆盖最好(只是行文字偏小); 改成"按行高选脸"之后
// 覆盖反而退步(文件管理/表盘管理/亮度/诊断回到 MiSans)。所以这里**按名义 px 选脸**,
// 行高只作为台账数据读出来看, 不参与决策 —— 尺寸问题另案处理, 先把覆盖和崩溃收口。
// 具体换成哪张脸现在由认脸表(FT_ID_*)给出, 口径不变; 在这个口径之上只有三类东西,
// 都不改变"换成哪个字号":
//   A. 渲染门方向(见 ft_render_busy): `*(lv_global+0x14)` 是"正在刷新的 display",
//      空闲时它就是 0; 把 0 当"取不到指针=忙", 等于只在真正渲染的那几毫秒放行,
//      方向整个反了。
//   B. 页身份门: 见 ft_page_root —— 只处理页描述符自己标成原生页(page_kind == 2)的页,
//      且这页没有排上销毁(async_destroy_state == 0)。切页与退出应用时固件正在销毁
//      重建对象树, "退出 Chaos 就崩"正落在这个窗口里; 树本身是否在变由结构指纹门判。
//   C. 写入预算 + 访问上限 + 认脸表满不静默: 限制单次 tick 的失效重绘面,
//      并且消除重复失败带来的额外分配。
//
// ---------------------------------------------------------------------------
// 页身份门为什么用描述符自己的 page_kind, 不用 app_id 白名单
// ---------------------------------------------------------------------------
// app_id 白名单(16/22/200)是两处覆盖损失的直接来源:
//   - 心率应用的"近30天静息/心率设置/心率说明"仍是 MiSans —— 它的 app_id 不在表里,
//     整棵树被身份门跳过(计到 FT_C_FGN);
//   - "打开设置行文字一格一格变文楷" —— 每 tick 只写 40 个对象, 一整页写不完要等
//     下一拍, 于是分几拍看见。
// 两处一起改: 身份门改成 page_kind == 2(所有原生注册页, 字节级证据见 ft_page_root
// 注释), 预算 40 -> 160、访问上限 600 -> 1500。
// 快应用的隔离不放松: 它靠的是 page_kind(固件页面类型定义写明快应用页是 3),
// 不再靠"猜哪些 app_id 是安全的"。当前页的 kind 与 lifecycle 都记在 FT_KIND / FT_LCS
// 里 —— 如果快应用的页其实不是 3, 打开它时这两个值就会暴露真实种类, 这条判据可证伪。

use crate::mem::{rd8, rd16, rd32};
use crate::*;

/// DFS 深度上限(页根 -> content -> 列表 -> 行 -> 标签 实测 4~6 层, 留足余量)
const FT_DEPTH: usize = 16;
/// 单子节点数上限: 超过就认定这个对象不可信, 不再深入
const FT_CHILD_MAX: u32 = 128;
/// 单趟访问对象上限。一次访问只是几条 ldr, 真正受限的是每趟的写入数(FT_TICK_W),
/// 所以这个"截断保险"放宽上限几乎不增加耗时, 却能排掉一种覆盖丢失: 带图表/长列表的
/// 页(心率、报告)子树很大, 600 个访问若先于写完就被截断, 后半棵子树永远轮不到。
const FT_OBJ_MAX: u32 = 1500;
/// 渲染门连续让路满这么多 tick(=5 秒连续"正在渲染")就判这个门读的东西不可信,
/// 从此绕过它(FT_GATE=0)。手表一帧只有几毫秒, 连续 5 秒忙只可能是读错字段。
/// 重要: 门**永远不能**把任务关掉 —— 把"让路计数"当熔断器去清零开关, 一趟遍历的
/// 写入预算就没了意义。每次直写 local style 固件都会做: 全子树失效 + 逐对象发
/// 样式变更事件(0x0C1070AC 体内 `bl 0xc1094fc(o,0x2d,0)`) => 限额把最坏情况摊平。
/// 预算从 40 放到 160: 40 连一整页的行都写不完, 实测看见"行文字一格一格变文楷"。
const FT_TICK_W: u32 = 160;

// ===== 本模块状态(全私有; 外部只经开关、统计与认脸登记这几个入口) =====
// 1=已启用。由"应用文楷"路径在样式级写回成功后置位; 重启即回(RAM 无痕)。
// 累计判据(自 arm() 起): 处理过的叶子数 / 写后读回确认已换的数。
// 两个相等 = 写进去的全部生效; 不等 = 写入口没吃进这个属性(计 FT_C_NOWK)。
static mut FT_C_LEAF: u32 = 0;
// 遍历阶段只**收集**写入请求, 写入统一放到遍历之外(见 FT_BO / ft_flush)。
// 原因 = 每次直写 local style 都可能让固件走分配分支, 而运动实时页
// 这类"页根不变、子树自己重建"的界面会在写入触发的回调里删掉子对象 —— 遍历栈里
// 还攥着已释放的父对象继续走就是黑屏重启。批容量与每 tick 写入预算同一个数。
// ===== 认脸表: 零派生解引用的 face 身份识别 =====
//
// 为什么要换掉原来那套认脸方式。原来读到一个 face 之后要再跳两 hop 才知道它是谁:
//   f -> rd32(f+0x24) = 缓存节点 -> rd32(节点+4) = 名字串 -> 连读 <=33 字节 + 读节点+8 字号。
// 后两跳拿的是**从 face 里读出来的指针**, 而我们的门 safe_ptr 只判"4 字节对齐 + 落在
// 0x2000_0000..0x2400_FFFF / 0x3000_0000..0x3FFF_FFFF", **不判是否映射**; 这段区间里有
// MPU 空洞(禁止按 RAM 区间扫描就是这条)。`块+0x24 = 缓存节点` 只对缓存
// 命中路径发出来的块成立(0x0C85F724), 所以只要某个界面对象解析到的 face 不是缓存块
// (表盘 widget 引擎那类自持字体对象), 第二跳起就是拿假指针读内存 => 精确总线错 =>
// 实测"进运动页点 GO 黑屏重启"。写侧的两条红线治不住它, 因为问题出在读侧的派生解引用。
//
// 等价信息在 face 结构**自己体内**就有: 缓存命中时固件做的是
// `ldr r6,[节点+0]` + ldm/stm 把载荷的 0x24 字节**逐字拷进块**(0x0C85F716), 所以同一
// 载荷拷出的每一个块前 9 个字完全相同。于是应用时按开机表逐条 create 一次**原始**
// (名字,字号) 当样本, 把样本块那 9 个字 + 该换成谁记进下表; 补写时只读候选 face 自己的
// 9 个字来比对。全程不再解引用任何从 face 里读出来的指针, 补写里也不再建脸(脸在应用
// 时就建完了) => 危险动作只剩"读活对象体内的定长字段"。
// 认不出的(表里没有的签名)一律不动: 我们只改能报得清身份的文字。
// 表要装得下固件**当前缓存里的全部**字体: 实测清单里有 293 个节点(登记 222 条),
// 所以上限 192 会被自己撑满并把多出来的条目悄悄丢掉(那就是"还有哪些没变"的一个来源)。
// 现在取 320, 并且 `learn` 回报是否真存进、满了由调用方单独计数, 不再静默丢。
const FT_ID_CAP: usize = 320;
static mut FT_ID_SIG: [[u32; 9]; FT_ID_CAP] = [[0; 9]; FT_ID_CAP];
static mut FT_ID_REPL: [u32; FT_ID_CAP] = [0; FT_ID_CAP];   // 0 = 保留原字体 / 冲突作废
// 第二把识别钥匙 = face 体内两个度量字(+0x0C 行高/基线, +0x10 下划线/位域)。
// 为什么需要: 9 字全等只在"载荷自拷贝之后再没被写过"时成立。对象手里那张脸是固件**早先**
// 从载荷拷出来的副本, 载荷里任何事后被改的字段(缓存句柄/释放链指针一类)都会让新旧副本差
// 一个字 => 全等比对一条都认不中 —— 实测就出现过这种情形: 表里存进 222 条, 一次都没认中。
// 度量字是建脸时按 (名字,字号) 算出来的, 之后没人写; 认不清(同度量不同目标)就作废不猜。
static mut FT_ID_MK0: [u32; FT_ID_CAP] = [0; FT_ID_CAP];
static mut FT_ID_MK1: [u32; FT_ID_CAP] = [0; FT_ID_CAP];
static mut FT_ID_N: u32 = 0;
// 调度: 补写不是"后台一直重扫最上层页", 而是**一次性任务** ——
// 从页栈栈底到栈顶逐页扫, 每页两趟(第一趟只量结构, 第二趟量到一模一样才写),
// 整条栈扫完就收工(自停)。触发点只有两个: 点"重新应用文楷"(样式写回跑完后自动排)
// 与更换字体页的"补写一遍"按钮(手动再跑一遍)。
// 游标 / 当前页是第几趟 / 武装时看到的页栈深度 / 当前页根(两趟比对用)
static mut FT_PI: u32 = 0;
static mut FT_PP: u32 = 0;
static mut FT_PAGES: u32 = 0;
static mut FT_ROOT: u32 = 0;
// 页身份判据(身份门判错了凭这三个值定位): 当前这页的 app_id /
// 描述符 +0x2a page_kind(原生注册路径逐字确认默认落成 2) / +0x28 lifecycle_state(注册默认 4)
static mut FT_AID: u32 = 0xFFFF;
static mut FT_KIND: u32 = 0xFF;
static mut FT_LCS: u32 = 0xFF;
// 统计(判据要可分辨: 每种失败一个独立计数, 不合并)
static mut FT_C_SCAN: u32 = 0;    // 完成的遍历轮数
static mut FT_C_OURS: u32 = 0;    // 已是我们的字体(幂等跳过)
static mut FT_C_KEEP: u32 = 0;    // 有意保留原字体(SportVF/泰语)
static mut FT_C_DFLT: u32 = 0;    // 读不到字体 / 解析到固件默认字体常量
static mut FT_C_NO: u32 = 0;      // 认不出(签名不在表里) -> 一律不动
static mut FT_C_BAD: u32 = 0;     // face 指针本身不可信 -> 不动
static mut FT_C_CF: u32 = 0;      // 登记时签名撞车(不同来源同 9 个字) -> 该条作废
static mut FT_C_MH: u32 = 0;      // 度量档认中次数(严格档没认中但度量认中)
// 任务的中止条件: 一次性任务只在"按下那一下之后的同一串画面"里有意义。
// 页栈深度一变 = 在开页/关页(正在导航) => 立刻收工, 不再往下面的页里写。
static mut FT_TASKN: u32 = 0;      // 本次任务锁定的页栈深度(0 = 第一拍还没量到)
static mut FT_C_NAV: u32 = 0;      // 因导航而中止的次数
// 结构指纹门的依据(实测): "补写还在跑时点 GO = 崩; 等它自停后再点 GO = 不崩"。
// 崩溃轴不是"改过的字体留在线上", 而是**正在写的那一刻**。运动实时页正是
// "页根不变、子树自己建/删"的那种树, 页根稳定门挡不住它 => 加结构指纹门:
static mut FT_NO_LH: u32 = 0;     // 第一个认不出的 face 的行高(0=没样本)
static mut FT_C_NOLS: u32 = 0;    // 没有 selector==0 的 local 槽 => 只记账(见 ft_has_local0)
static mut FT_C_SKIP: u32 = 0;    // 超深/超宽/对象数触顶而未处理
static mut FT_C_NOROOT: u32 = 0;  // 取不到最上层页根
static mut FT_C_FGN: u32 = 0;     // 页身份门没过(page_kind != 2): 快应用/未知种类整页跳过
static mut FT_C_DST: u32 = 0;     // 描述符 +0x24 非 0 = 这页正在排队销毁, 本轮让路

// ===== 补写任务状态(门与预算) =====
/// 连续让路满这么多 tick(=5 秒连续"正在渲染")就判渲染门读的东西不可信, 从此绕过它
/// (FT_GATE=0)。门只让路, 永不关功能 —— 把让路计数当熔断器去清掉整趟遍历的写入
/// 预算是错的。
const FT_BUSY_GIVEUP: u32 = 100;
/// 1 = 补写任务在跑(一次性任务, 页5(更换字体)行9 触发)。
static mut FT_ON: u32 = 0;
/// 遍历攒下的写入批次(遍历结束后由 ft_flush 统一执行 ——
/// 写入触发的回调可能删子对象, 遍历栈里不能再攥着树指针写)。
/// 批容量与每 tick 写入预算同一个数(FT_TICK_W)。
static mut FT_BO: [u32; FT_TICK_W as usize] = [0; FT_TICK_W as usize];
static mut FT_BF: [u32; FT_TICK_W as usize] = [0; FT_TICK_W as usize];
static mut FT_BN: u32 = 0;
/// 本轮(这一拍)还剩多少写入额度: 一次直写 = 固件全子树失效 + 逐对象发事件,
/// 预算把最坏情况摊平(40 -> 160: 40 连一整页的行都写不完)。
static mut FT_BUDGET: u32 = 0;
/// 本次任务已处理完的页数
static mut FT_ROUNDS: u32 = 0;
/// 结构指纹门: 遍历把"对象指针+子点数+深度"滚进 FNV。
/// 同一页两趟指纹一致 = 期间引擎没建/删任何子对象, 才允许写这一批。
static mut FT_FP: u32 = 0;
static mut FT_FP_PREV: u32 = 0;
/// 因结构在变而丢弃批次的页数(只让路, 永不关功能)
static mut FT_C_CHURN: u32 = 0;
/// 写了但读回不等于写入值(写入口没吃进这个属性)
static mut FT_C_NOWK: u32 = 0;
/// 写后读回确认生效的条数(= ft_ident 命中并真写成的)
static mut FT_C_OK: u32 = 0;
/// 因"渲染中"让路的次数(累计, 只观测)
static mut FT_C_BUSY: u32 = 0;
/// 1 = 渲染门生效; 连续让路 FT_BUSY_GIVEUP 拍后判门不可信并绕过(置 0)
static mut FT_GATE: u32 = 1;
static mut FT_BUSYC: u32 = 0;     // 连续让路次数(不连续即清零)

/// 读 face 体内的**定长 9 个字**当签名(认脸用; 只读, 不解派生指针)。
unsafe fn ft_sig_of(f: u32, out: &mut [u32; 9]) {
    let mut w = 0usize;
    while w < 9 {
        out[w] = rd32((f + w as u32 * 4) as *const u32);
        w += 1;
    }
}

/// 登记一条身份。`face` = 固件字体缓存清单里某个节点的**载荷**(由 font_apply
/// 走清单时喂进来); `repl` = 该换成谁的指针, 0 = **有意保留原字体**(SportVF/泰语)。
/// 同签名重复登记(再点一次应用) = 幂等; 签名撞上不同 repl = 该条整条作废 ——
/// 宁可少改一片文字, 也绝不认错字体(认错 = 把运动数字或泰语换成文楷)。
/// 我们自己的脸专用: 同 learn, 但签名/度量冲突时**覆盖**成最新映射而不是
/// 清零弃权。为什么: 认脸表的普通冲突策略(同脸不同目标 => 两边都不动)是给系统脸的
/// 保守取舍; 而 ChaosSans-* 的最新一代应用/恢复就是唯一真相 —— 否则换字体/恢复之后,
/// 补写过的对象会永远停在旧代的脸(实测: 切换不跟、恢复也不跟)。
pub(crate) unsafe fn learn_owned(face: u32, repl: u32) -> bool {
    if !plausible_ptr(face) { return false; }
    let mut sig = [0u32; 9];
    ft_sig_of(face, &mut sig);
    let mk0 = rd32((face + 0x0C) as *const u32);
    let mk1 = rd32((face + 0x10) as *const u32);
    let mut i = 0usize;
    let n = st_rd!(FT_ID_N) as usize;
    while i < n {
        if st_rd!(FT_ID_SIG[i]) == sig
            || (st_rd!(FT_ID_MK0[i]) == mk0 && st_rd!(FT_ID_MK1[i]) == mk1) {
            if st_rd!(FT_ID_REPL[i]) != repl {
                st_wr!(FT_ID_SIG[i], sig);
                st_wr!(FT_ID_MK0[i], mk0);
                st_wr!(FT_ID_MK1[i], mk1);
                st_wr!(FT_ID_REPL[i], repl);
                st_wr!(FT_C_CF, st_rd!(FT_C_CF) + 1);
            }
            return true;
        }
        i += 1;
    }
    if i >= FT_ID_CAP { return false; }
    st_wr!(FT_ID_SIG[i], sig);
    st_wr!(FT_ID_MK0[i], mk0);
    st_wr!(FT_ID_MK1[i], mk1);
    st_wr!(FT_ID_REPL[i], repl);
    st_wr!(FT_ID_N, i as u32 + 1);
    true
}

pub(crate) unsafe fn learn(face: u32, repl: u32) -> bool {
    // 只要求"可读"(mem.rs: plausible_ptr = 像固件里的任何东西, 读内容用它)。
    // 这里若用 safe_ptr(只放行可写 RAM), 载荷/脸在只读域或别名域时会被判不可信 =>
    // 整张表空 => 补写一个也认不出(覆盖归零的候选原因之一)。
    if !plausible_ptr(face) { return false; }
    let mut sig = [0u32; 9];
    ft_sig_of(face, &mut sig);
    let mk0 = rd32((face + 0x0C) as *const u32);
    let mk1 = rd32((face + 0x10) as *const u32);
    let mut i = 0usize;
    let n = st_rd!(FT_ID_N) as usize;
    while i < n {
        if st_rd!(FT_ID_SIG[i]) == sig {
            if st_rd!(FT_ID_REPL[i]) != repl {
                st_wr!(FT_ID_REPL[i], 0);
                st_wr!(FT_C_CF, st_rd!(FT_C_CF) + 1);
            }
            return true;
        }
        // 同度量、不同目标 => 度量档认不准, 这一条两边都不许改(宁可少覆盖)
        if st_rd!(FT_ID_MK0[i]) == mk0 && st_rd!(FT_ID_MK1[i]) == mk1
            && st_rd!(FT_ID_REPL[i]) != repl {
            st_wr!(FT_ID_REPL[i], 0);
            st_wr!(FT_C_CF, st_rd!(FT_C_CF) + 1);
        }
        i += 1;
    }
    if i >= FT_ID_CAP { return false; }        // 表满: 少认一条, 返回假由调用方计数, 不静默
    st_wr!(FT_ID_SIG[i], sig);
    st_wr!(FT_ID_MK0[i], mk0);
    st_wr!(FT_ID_MK1[i], mk1);
    st_wr!(FT_ID_REPL[i], repl);
    st_wr!(FT_ID_N, i as u32 + 1);
    true
}

/// 认脸: 命中返回真 + 该换成谁(repl==0 = 保留原字体); 认不出返回假。
unsafe fn ft_ident(f: u32) -> (bool, u32) {
    let mut sig = [0u32; 9];
    ft_sig_of(f, &mut sig);
    let mk0 = rd32((f + 0x0C) as *const u32);
    let mk1 = rd32((f + 0x10) as *const u32);
    let n = st_rd!(FT_ID_N) as usize;
    let mut i = 0usize;
    while i < n {                                   // 严格档: 9 字全等
        if st_rd!(FT_ID_SIG[i]) == sig { return (true, st_rd!(FT_ID_REPL[i])); }
        i += 1;
    }
    i = 0;
    while i < n {                                   // 度量档: 只比 +0x0C/+0x10
        if st_rd!(FT_ID_MK0[i]) == mk0 && st_rd!(FT_ID_MK1[i]) == mk1 {
            st_wr!(FT_C_MH, st_rd!(FT_C_MH) + 1);   // 这一档命中数 = 载荷被事后改过的证据
            return (true, st_rd!(FT_ID_REPL[i]));
        }
        i += 1;
    }
    (false, 0)
}

// ---------------------------------------------------------------------------
// 单个叶子对象
// ---------------------------------------------------------------------------

/// 处理一个叶子对象: 读它当前解析到的 face -> **认脸** -> 换成表里登记的那张脸排队。
/// 这里只剩三类内存动作: 读固件 API 返回的值、读 face 自己体内的定长 9 个字、
/// 读活对象体内的 local style 表头 —— 不再顺派生指针往下走, 也不再建脸。
/// 认不出身份的一律不动(计 FT_C_NO), 每种不写各占一个计数。
///
unsafe fn ft_fix_leaf(o: u32) {
    st_wr!(FT_C_LEAF, st_rd!(FT_C_LEAF) + 1);
    let f = fw_api::fw_style_get_prop(o, 0, fw_api::LV_STYLE_TEXT_FONT);
    if f == 0 || f == FW_FONT_DEFAULT {
        // 读不到, 或整条解析链上没有任何 text_font 而落到固件默认字体常量:
        // 没有可换的东西 => 跳过并计数。
        st_wr!(FT_C_DFLT, st_rd!(FT_C_DFLT) + 1);
        return;
    }
    // 同上: 认脸只读 face 体内 9 个字, 门槛用"可读"而不是"可写 RAM"
    if !plausible_ptr(f) { st_wr!(FT_C_BAD, st_rd!(FT_C_BAD) + 1); return; }
    // 注意: 不按"是不是我们写的"跳过 —— 旧代的脸也是我们写的, 但换字体/恢复之后
    // 它必须跟着换代。是否跳过交给认脸结果: repl == 当前脸 才是真幂等。
    let (hit, repl) = ft_ident(f);
    if !hit {
        st_wr!(FT_C_NO, st_rd!(FT_C_NO) + 1);               // 认不出 -> 不猜, 不动
        if st_rd!(FT_NO_LH) == 0 {
            // 样本行高(读 face 体内 +0x0C): 行高正常 => 这确实是一张正常的脸, 问题在比对;
            // 行高离谱(0 或几百) => 这个指针根本不是脸, 认不出才是对的。
            st_wr!(FT_NO_LH, rd16((f + FACE_LINE_HEIGHT_OFF) as *const u16) as u32);
        }
        return;
    }
    if repl == 0 {
        st_wr!(FT_C_KEEP, st_rd!(FT_C_KEEP) + 1);           // 表盘运动数字/泰语: 保留原样
        return;
    }
    if repl == f {
        st_wr!(FT_C_OURS, st_rd!(FT_C_OURS) + 1);          // 已是当前代的脸, 真幂等
        return;
    }
    // 到这里的 = "认脸表认识、且该换成我们的脸" —— 即补写的目标对象。
    // 下面这条只**记账**不再拦: 曾把它当红线拦过一遍, 结果是带着它照样在点 GO 时崩、
    // 而硬拦又让覆盖掉了一半(开不开补写都一样) —— 分配不是崩溃轴, 派生
    // 解引用才是(后者已经归零)。留计数的意义: FT_C_NOLS 涨 = 这批对象要靠新分配槽才写得动。
    if !ft_has_local0(o) { st_wr!(FT_C_NOLS, st_rd!(FT_C_NOLS) + 1); }
    if st_rd!(FT_BUDGET) == 0 || st_rd!(FT_BN) >= FT_TICK_W {
        st_wr!(FT_C_SKIP, st_rd!(FT_C_SKIP) + 1);
        return;
    }
    let k = st_rd!(FT_BN) as usize;
    st_wr!(FT_BO[k], o);
    st_wr!(FT_BF[k], repl);
    st_wr!(FT_BN, k as u32 + 1);
    st_wr!(FT_BUDGET, st_rd!(FT_BUDGET) - 1);
}

/// 这个对象是否**已经有 selector==0 的 local style 槽**? 没有槽时写入会让固件走
/// 分配分支 —— 现在只用它记账, 不拦写入(见 ft_fix_leaf)。
///
/// 依据(固件写入口 0x0C107790 自己用的字段, 逐条字节级):
///   0x0C107798 `ldrh r3,[r0,#0x32]` + 0x0C1077A6 `ubfx r1,r3,#4,#6` => 条目数 u16 的
///     bit4..9(6 位, 最多 63 条);
///   0x0C1077B2 `ldr r2,[r0,#0xc]` => 条目数组基址, 循环里 `add r2,r2,#8` => 步长 8;
///   0x0C1077BA/BE `r3=[r2+4]; ubfx r3,#0,#0x18; cmp r8,r3` => 条目+4 低 24 位 = selector;
///   命中 => 0x0C1078BA 原地写; 没命中 => 0x0C1078D8 `movs r2,#0x2b8`(=sizeof 一个样式
///   对象 696B)走**分配**分支。
/// 所以"没有 selector 0 槽"的对象, 我们一写就让固件在申请内存; 而这类对象的文字字体
/// 是从挂载样式继承来的 —— 样式级写回(font_apply)已经改到它了, 跳过不丢覆盖。
/// 反过来, 我们真正要补的那批(行主/副标题、页面标题)都是固件建行时 `row_init` 直烘过
/// local text_font 的, 槽本来就在 => 走原地写, 补写全程不再申请内存。
unsafe fn ft_has_local0(o: u32) -> bool {
    let n = ((rd16((o + 0x32) as *const u16) >> 4) & 0x3F) as u32;
    if n == 0 { return false; }
    let mut a = rd32((o + 0x0C) as *const u32);
    if !safe_ptr(a) { return false; }
    let mut i = 0u32;
    while i < n {
        if (rd32((a + 4) as *const u32) & 0x00FF_FFFF) == 0 { return true; }
        a += 8;
        i += 1;
    }
    false
}

/// 把这一轮攒下的写入**在遍历结束之后**统一执行: 遍历栈(obj/idx/cnt 三个数组)此时
/// 已经不再持有树里的任何指针, 即使某个写入让固件重建了子树, 也不会有"带着已释放的
/// 父对象继续往下走"的窗口。写完仍按读回比对判生效。
unsafe fn ft_flush() {
    let n = st_rd!(FT_BN) as usize;
    let mut i = 0usize;
    while i < n {
        let o = st_rd!(FT_BO[i]);
        let nf = st_rd!(FT_BF[i]);
        fw_api::obj_set_local_style_prop(o, fw_api::LV_STYLE_TEXT_FONT, nf, 0);
        if fw_api::fw_style_get_prop(o, 0, fw_api::LV_STYLE_TEXT_FONT) == nf {
            st_wr!(FT_C_OK, st_rd!(FT_C_OK) + 1);
        } else {
            st_wr!(FT_C_NOWK, st_rd!(FT_C_NOWK) + 1);
        }
        i += 1;
    }
    st_wr!(FT_BN, 0);
}

// ---------------------------------------------------------------------------
// 遍历
// ---------------------------------------------------------------------------

/// 一轮显式栈 DFS(不用递归: 内核栈深度不可靠)。只处理叶子。
/// **全程只读**(写入只在 ft_flush, 而它只在遍历返回后被调用)。
/// 顺带累加**结构指纹**(指纹门): 每个见到的对象贡献"指针 + 子点数 + 深度",
/// 引擎在建/删子树时这个数必然变 => 两趟之间一字不变才允许写。
unsafe fn ft_walk(root: u32) {
    let mut obj = [0u32; FT_DEPTH];
    let mut idx = [0u32; FT_DEPTH];
    let mut cnt = [0u32; FT_DEPTH];
    let mut d = 0usize;
    let mut seen = 0u32;
    st_wr!(FT_FP, 0x811C9DC5);      // FNV 起始值(非 0, 与"没有基准"区分开)
    obj[0] = root;
    idx[0] = 0;
    let rc = fw_api::obj_child_count(root);
    cnt[0] = if rc > 0 { rc as u32 } else { 0 };
    if cnt[0] == 0 { return; }            // 根没有子对象: 没有可换的东西
    loop {
        if idx[d] >= cnt[d] {
            if d == 0 { break; }
            d -= 1;
            continue;
        }
        // 额度门两套: 补写模式看"写入预算"(每次真写都让固件失效一整棵子树, 摊平它);
        // 探测模式只读, 用访问数封顶即可。
        if st_rd!(FT_ON) != 0 {
            if st_rd!(FT_BUDGET) == 0 {
                st_wr!(FT_C_SKIP, st_rd!(FT_C_SKIP) + 1);   // 本轮额度用尽, 下一拍接着扫
                break;
            }
        } else if seen >= FT_TICK_W {
            st_wr!(FT_C_SKIP, st_rd!(FT_C_SKIP) + 1);
            break;
        }
        let i = idx[d];
        idx[d] = i + 1;
        let c = fw_api::obj_get_child(obj[d], i);
        if !safe_ptr(c) { st_wr!(FT_C_BAD, st_rd!(FT_C_BAD) + 1); continue; }
        let cc = fw_api::obj_child_count(c);
        st_wr!(FT_FP, st_rd!(FT_FP).wrapping_mul(0x01000193)
                        ^ c ^ ((cc as u32) << 8) ^ ((d as u32) << 20));
        if cc == 0 {
            ft_fix_leaf(c);
        } else if cc as u32 > FT_CHILD_MAX || d + 1 >= FT_DEPTH {
            st_wr!(FT_C_SKIP, st_rd!(FT_C_SKIP) + 1);
            continue;
        } else {
            d += 1;
            obj[d] = c;
            idx[d] = 0;
            cnt[d] = cc as u32;
        }
        seen += 1;
        if seen >= FT_OBJ_MAX {
            st_wr!(FT_C_SKIP, st_rd!(FT_C_SKIP) + 1);
            break;
        }
    }
    st_wr!(FT_C_SCAN, st_rd!(FT_C_SCAN) + 1);
}

// ---------------------------------------------------------------------------
// 对外接口
// ---------------------------------------------------------------------------

/// 页身份门 + 取根: 页栈里第 idx 页的描述符 -> 只有 `page_kind == 2`
/// (原生注册页)且没排上销毁的页才给出它的对象树根, 否则返回 0 并计对应的跳过数。
///
/// 用 app_id 白名单(16/22/200)当身份门会丢覆盖 —— 心率应用的
/// "近30天静息/心率设置/心率说明"这类页 app_id 不在表里, 整棵树被跳过。白名单
/// 本来只是为了挡快应用, 它想挡的其实是"哪棵树不归原生 page 框架管"。
///
/// 权威判据是描述符自己的 `page_kind`(+0x2a): 注册函数 0x0CA6AD10 在
/// `0x0CA6AD64` 读这个字节, 为 0 就跳到 `0x0CA6ADF6: movs r3,#2 / strb.w r3,[r0,#0x2a]`,
/// 也就是**原生注册页一律是 2**; 快应用页是 3(静态核对固件的页面类型定义)。所以这里
/// 只认 2, 其余(3/0/脏值)一律不碰它的对象树 —— 比白名单宽(所有原生页都进来),
/// 又比白名单更讲得清理由。
/// 另一道 `async_destroy_state`(+0x24, 注册时清 0, 销毁包装按它选"同步还是排队销毁"):
/// 非 0 说明这页已经排上销毁, 对象树随时会被递归删掉, 这一页跳过。
///
/// idx 由一次性任务的游标给出(0 = 栈底), 不只盯最上层页: 同一条栈上
/// 所有活着的原生页都会被打到一遍。
unsafe fn ft_page_root(idx: u32) -> u32 {
    let desc = fw_api::page_desc_by_index(idx);
    if !safe_ptr(desc) {
        st_wr!(FT_C_FGN, st_rd!(FT_C_FGN) + 1);
        return 0;
    }
    let aid = rd16((desc + fw_api::DESC_APP_ID) as *const u16) as u32;
    st_wr!(FT_AID, aid);
    let kind = rd8((desc + fw_api::DESC_KIND) as *const u8) as u32;
    st_wr!(FT_KIND, kind);
    st_wr!(FT_LCS, rd8((desc + fw_api::DESC_LIFECYCLE) as *const u8) as u32);
    if kind != fw_api::PAGE_KIND_NATIVE {
        st_wr!(FT_C_FGN, st_rd!(FT_C_FGN) + 1);
        return 0;
    }
    if rd32((desc + fw_api::DESC_ASYNC_DESTROY) as *const u32) != 0 {
        st_wr!(FT_C_DST, st_rd!(FT_C_DST) + 1);
        return 0;
    }
    let root = rd32((desc + fw_api::DESC_ROOT) as *const u32);
    if !safe_ptr(root) {
        st_wr!(FT_C_NOROOT, st_rd!(FT_C_NOROOT) + 1);
        return 0;
    }
    root
}

/// UI 线程 50ms tick 里调用: 补写任务在跑(FT_ON)才推进一拍, 否则什么都不做。
///
/// 重要: 这里没有常驻扫描, 也没有只读探测。唯一的写入链是"两趟遍历 + 结构指纹门 +
/// ft_flush() 写回", 只在 backfill_tick 那个一次性任务里跑; 任务没被触发时本函数
/// 一条指令都不多走。认脸表(FT_ID_*, 由 font_apply 走固件缓存清单喂)是它的身份来源。
pub(crate) unsafe fn tick() {
    if st_rd!(FT_ON) != 0 { backfill_tick(); }
}

// ---------------------------------------------------------------------------
// 逐对象补写任务: 一次性任务 + 结构/渲染/导航全部门控
// ---------------------------------------------------------------------------

/// 渲染门: `*(lv_global+0x14)` = LVGL 的"正在刷新的 display"
/// (取/存成对: getter 0x0C1052B0 / setter 0x0C1052C4 = `_lv_refr_get/set_disp_refreshing`)。
/// 语义: **没有 display 在刷新时它就是 0**。所以
///   d == 0        => 空闲 => 安全(把 0 当"取不到指针=忙"就把方向整个判反了)
///   d 非 0 不可信 => 判忙(宁可不写)
///   d 非 0 可信   => 看 `disp+0x3a` 的 bit1(0x0c10587c 置位 / 0x0c105aa2、0x0c105eac 清除)
/// 门只让路, 永不关功能。
unsafe fn ft_render_busy() -> bool {
    let g = LV_GLOBAL;
    let d = rd32((g + LVG_DISP) as *const u32);
    if d == 0 { return false; }
    if !safe_ptr(d) { return true; }
    (rd8((d + DISP_FLAG_OFF) as *const u8) & (DISP_RENDERING_BIT as u8)) != 0
}

/// 打开逐对象补写并清零**统计**。认脸表(FT_ID_*)与 FT_C_CF 不在这里清:
/// 表是 font_apply 在应用时一次性登记的, 关掉开关再打开不该把它抹掉
/// (抹了就等于补写从此认不出任何字体)。
pub(crate) unsafe fn arm() {
    st_wr!(FT_ON, 1);
    st_wr!(FT_ROOT, 0);
    st_wr!(FT_PI, 0);
    st_wr!(FT_PP, 0);
    st_wr!(FT_PAGES, 0);
    st_wr!(FT_TASKN, 0);
    st_wr!(FT_C_NAV, 0);
    st_wr!(FT_C_LEAF, 0);
    st_wr!(FT_C_OK, 0);
    st_wr!(FT_C_SCAN, 0);
    st_wr!(FT_C_OURS, 0);
    st_wr!(FT_C_KEEP, 0);
    st_wr!(FT_C_DFLT, 0);
    st_wr!(FT_C_NO, 0);
    st_wr!(FT_C_BAD, 0);
    st_wr!(FT_C_SKIP, 0);
    st_wr!(FT_C_NOWK, 0);
    st_wr!(FT_C_NOLS, 0);
    st_wr!(FT_C_MH, 0);
    st_wr!(FT_ROUNDS, 0);
    st_wr!(FT_FP, 0);
    st_wr!(FT_FP_PREV, 0);
    st_wr!(FT_C_CHURN, 0);
    st_wr!(FT_NO_LH, 0);
    st_wr!(FT_BN, 0);
    st_wr!(FT_C_NOROOT, 0);
    st_wr!(FT_C_BUSY, 0);
    st_wr!(FT_C_FGN, 0);
    st_wr!(FT_BUSYC, 0);
    st_wr!(FT_GATE, 1);        // 每次重新武装都再给渲染门一次自证机会
    st_wr!(FT_AID, 0xFFFF);
    st_wr!(FT_KIND, 0xFF);
    st_wr!(FT_LCS, 0xFF);
    st_wr!(FT_C_DST, 0);
    st_wr!(FT_BUDGET, FT_TICK_W);
}

/// 补写是否在跑(界面侧 `font_list::window_is_quiet()` 用它让路)
pub(crate) unsafe fn backfill_running() -> bool { st_rd!(FT_ON) != 0 }

/// 补写任务读数(页5 行9 副标签; 判据可分辨: 每种结局一个独立计数):
/// (写后生效, 写了没生效, 因导航中止的次数, 因树在变丢弃批次的页数,
///  已处理完的页数, 任务锁定的页栈深度)
pub(crate) unsafe fn backfill_stats() -> (u32, u32, u32, u32, u32, u32) {
    (st_rd!(FT_C_OK), st_rd!(FT_C_NOWK), st_rd!(FT_C_NAV), st_rd!(FT_C_CHURN),
     st_rd!(FT_ROUNDS), st_rd!(FT_PAGES))
}

/// UI 线程 50ms tick 里调用 —— **一次性任务**: 从页栈栈底往栈顶逐页扫, 每页两趟,
/// 整条栈扫完就自己关掉(FT_ON=0)。要点:
///   第一趟只量结构(遍历完把指纹存成基准, 攒下的那批丢掉);
///   第二趟量到与第一趟**一字不变**才真的写 —— 两趟隔 50ms, 期间引擎建/删过子树
///   就一定不等(实测结论: 崩就崩在"写的那一刻树正在变")。
/// 写完这一页游标 +1; 门没过(不是原生页/排队销毁/取不到根)也 +1, 不回头等。
unsafe fn backfill_tick() {
    // 息屏 = 固件正在销毁/重建对象树, 而且此时重绘也没有意义 => 任务就地收工
    if !fw_api::screen_is_on() { st_wr!(FT_ON, 0); st_wr!(FT_BN, 0); return; }
    // 渲染中不改样式(只让路, 连续让路满 100 tick 判门不可信并绕过; 游标不动)
    if st_rd!(FT_GATE) != 0 && ft_render_busy() {
        st_wr!(FT_C_BUSY, st_rd!(FT_C_BUSY) + 1);
        let c = st_rd!(FT_BUSYC) + 1;
        st_wr!(FT_BUSYC, c);
        if c > FT_BUSY_GIVEUP { st_wr!(FT_GATE, 0); }
        else { return; }
    } else {
        st_wr!(FT_BUSYC, 0);
    }
    let depth = fw_api::page_stack_depth();
    if depth == 0 { st_wr!(FT_C_NOROOT, st_rd!(FT_C_NOROOT) + 1); return; }
    st_wr!(FT_PAGES, depth);
    // 页栈深度变了 = 正在开页/关页(导航)。这个任务只在"触发那一下时的那串画面"里有
    // 意义, 导航中继续往下写就是上面那种"写的那一刻树正在变"的崩 => 就地收工。
    if st_rd!(FT_TASKN) == 0 {
        st_wr!(FT_TASKN, depth);
    } else if depth != st_rd!(FT_TASKN) {
        st_wr!(FT_ON, 0);
        st_wr!(FT_BN, 0);
        st_wr!(FT_C_NAV, st_rd!(FT_C_NAV) + 1);
        return;
    }
    if st_rd!(FT_PI) >= depth {              // 整条栈扫完 => 收工
        st_wr!(FT_ON, 0);
        st_wr!(FT_BN, 0);
        return;
    }
    let pi = st_rd!(FT_PI);
    let root = ft_page_root(pi);
    if root == 0 { st_wr!(FT_PI, pi + 1); st_wr!(FT_PP, 0); return; }
    if st_rd!(FT_PP) == 0 {                  // 第一趟: 只量结构
        st_wr!(FT_ROOT, root);
        st_wr!(FT_BUDGET, FT_TICK_W);
        st_wr!(FT_BN, 0);
        ft_walk(root);
        st_wr!(FT_FP_PREV, st_rd!(FT_FP));
        st_wr!(FT_BN, 0);                    // 攒下的批次这一趟不用, 第二趟重新攒
        st_wr!(FT_PP, 1);
        return;
    }
    // 第二趟: 同一页、页根没换、指纹一致才写
    st_wr!(FT_BUDGET, FT_TICK_W);
    st_wr!(FT_BN, 0);
    ft_walk(root);
    if root != st_rd!(FT_ROOT) || st_rd!(FT_FP) != st_rd!(FT_FP_PREV) {
        if st_rd!(FT_BN) != 0 { st_wr!(FT_C_CHURN, st_rd!(FT_C_CHURN) + 1); }
        st_wr!(FT_BN, 0);                    // 这页在变 => 这一批丢掉, 不硬写
    } else {
        ft_flush();
    }
    st_wr!(FT_PP, 0);
    st_wr!(FT_PI, pi + 1);
    st_wr!(FT_ROUNDS, st_rd!(FT_ROUNDS) + 1);
}

// ---------------------------------------------------------------------------
// 补写任务的手动入口(页5(更换字体)行9"补写一遍")
// ---------------------------------------------------------------------------
//
// 这条入口存在的理由: 实测补写不崩(另一半读数同样是"等它自停后再操作 = 不崩");
// 且 23 条派生样式补齐之后小部件屏与"布局切换"仍不覆盖
// —— 它们的 face 不经过任何我们写过的样式, 只有逐对象直写够得到。门控全部保留:
// 两趟指纹一致才写 / 渲染让路 / 页栈深度锁定 / 息屏与导航即收工 / 遍历外统一写 /
// 预算摊平。跑批期间界面只登记请求, 由 `font_list::window_is_quiet()` 那条门让路。

/// 美化页画按钮行用: 补写任务是否在跑。
pub(crate) unsafe fn enabled() -> bool {
    st_rd!(FT_ON) != 0
}

/// 页5 行9: 手动跑一遍一次性补写; 任务在跑时再点 = 中断。
/// 只动静态量, 不碰对象树 => 在点击回调里调用是安全的。
pub(crate) unsafe fn toggle() {
    if st_rd!(FT_ON) != 0 {
        st_wr!(FT_ON, 0);
        st_wr!(FT_BN, 0);
    } else {
        arm();
    }
}

