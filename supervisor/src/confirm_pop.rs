// 删除确认的**真系统消息框**(lvx_page_msgbox) —— "更换字体"页与"桌面图标"页共用一份。
// 两页要的只是"确认删哪一个 + 勾/叉", 差别只在勾了之后干什么; 那部分留给各页,
// 这里只放两页完全相同的状态机(每个槽位各存一份, 见 CP_ST / CP_OBJ / CP_TGT)。
//
// 下面四条规矩都是实测换来的, 只留一处实现, 不要在别的页抄第二份:
//   1. parent 必须是**页根对象**(不是 content): content 会滚动, 挂在它下面的框会随
//      滚动露出别的行, 也盖不住标题栏。
//   2. **建框在点击回调当场做**(固件"连接断开"弹框同款的事件上下文用法); 绕到
//      定时器安静拍去建, 失败会是完全无声的。
//   3. **关框必须真删对象**: 页重建只销毁 content, 挂在页根上的框不会被重建带走,
//      留着就是一片盖住标题栏的全屏黑。
//   4. 判"框还在不在"用**页根子对象表**(`fw_api::obj_is_child_of`), 不能只读那个单例字:
//      固件登记单例是 `bl 0x0C5891A8(holder)` 取**槽指针**再写, 取槽函数在已清零的
//      0x1C 段, 静态确定不了返回的是 holder 还是 holder+off —— 等式可能永远不成立。
//      单例字仍然读, 但只当补充判据(两道门任一命中就删得到, 判据只会更宽不会更窄)。
//
// 另外两件事**留给各页自己做**, 因为两页的既有形态不同、也不该互相牵动:
//   * 关掉框之后各页要排一次整页重建(字体页置 FL_ROWS_REQ, 图标页置 IP_POP_DIRT):
//     obj_delete 只失效框自己那一片, 标题栏那一层不在里面, 不重建就冻成一片黑;
//   * "什么时候才允许碰对象树"的那道门: 字体页是 `window_is_quiet()`(分拍建脸 + 补写),
//      图标页是"亮屏 + 没在跑批(38 张逐拍换)"。
// 状态归零只有两条路径: 建页开头的 `reset`(顺手兜掉漏删的框)与新页实例的 `forget`。

use crate::mem::rd32;
use crate::*;

/// 槽位: 0 = "更换字体"页(页5), 1 = "桌面图标"页(页6)。
pub(crate) const SLOT_FONT: usize = 0;
pub(crate) const SLOT_ICON: usize = 1;
const SLOT_N: usize = 2;

/// 0=没有框 2=框显示中 3=已请求关闭但门没过(等调用方下一拍 `close_deferred`)
static mut CP_ST: [u32; SLOT_N] = [0; SLOT_N];
/// 框句柄(挂在各该页的页根上)
static mut CP_OBJ: [u32; SLOT_N] = [0; SLOT_N];
/// 正在确认删除的目标(字体页 = 清单槽位号, 图标页 = 包号)
static mut CP_TGT: [u32; SLOT_N] = [0; SLOT_N];
/// 文案缓冲与按钮表(0x28 = 2 条目 x 0x14): 条目0 = 叉, 条目1 = 勾, [+0]=回调(bit0=1)
static mut CP_TXT: [[u8; 48]; SLOT_N] = [[0; 48]; SLOT_N];
static mut CP_BTN: [[u32; 10]; SLOT_N] = [[0; 10]; SLOT_N];

/// 槽位对应的页号。页号本身只有 `font_list::FONT_PID` / `icon_apply::ICON_PID` 一处真源,
/// 这里不另写常量。
fn pid_of(slot: usize) -> u32 {
    if slot == SLOT_ICON { icon_apply::ICON_PID } else { font_list::FONT_PID }
}

/// 该页的页根对象。用框架缓存的描述符指针 —— `page_desc_by_index` 的参数是**页栈位置**
/// (0=栈底)而不是页 ID, 拿页号去查会越界, 表现就是弹框创建失败(返回 1)。
unsafe fn root_of(slot: usize) -> u32 {
    let desc = PAGES[pid_of(slot) as usize].desc;
    if !safe_ptr(desc) { return 0; }
    rd32((desc + fw_api::DESC_ROOT) as *const u32)
}

/// 框还在不在: 页根子对象表为主, 单例字为辅。
unsafe fn alive(slot: usize, o: u32) -> bool {
    if o == 0 { return false; }
    fw_api::obj_is_child_of(root_of(slot), o) || rd32(fw_api::MSGBOX_SINGLETON as *const u32) == o
}

/// 弹"删除「名字」?"的确认框: 黑底白字 + 底部灰叉/蓝勾, 与"确认重启？"同款视觉。
/// 只在点击回调里调。**在页根上建出来之后它天然是全屏的**(类固有尺寸 0x150x0x1E0,
/// 页根不滚动), 不需要任何 align 或缩放。
///
/// 返回分级码(调用方直接当界面读数显示, 失败必须看得见):
///   0 = 建出来了; 1 = 页描述符取不到; 2 = 页根取不到; 3 = `msgbox_create` 返回 0。
pub(crate) unsafe fn build(slot: usize, target: u32, name: *const u8, name_len: usize,
                           cb_no: u32, cb_ok: u32) -> u32 {
    let desc = PAGES[pid_of(slot) as usize].desc;
    if !safe_ptr(desc) { return 1; }
    let root = rd32((desc + fw_api::DESC_ROOT) as *const u32);
    if !safe_ptr(root) { return 2; }
    let tp = core::ptr::addr_of_mut!(CP_TXT[slot]) as *mut u8;
    let mut w = W::new(tp, 48);
    w.s("删除「".as_bytes());
    if name_len != 0 && !name.is_null() {
        w.s(core::slice::from_raw_parts(name, name_len));
    }
    w.s("」?".as_bytes());
    w.end();
    let btn = core::ptr::addr_of_mut!(CP_BTN[slot]) as *mut u32;
    core::ptr::write_volatile(btn, cb_no | 1);         // 条目0 = 叉 = 取消
    core::ptr::write_volatile(btn.add(5), cb_ok | 1);  // 条目1 = 勾 = 确认删除
    let mb = fw_api::msgbox_create(root, 0);
    if !safe_ptr(mb) { return 3; }
    fw_api::msgbox_fill(mb, 5, 0, tp, 0, btn, 2);
    st_wr!(CP_OBJ[slot], mb);
    st_wr!(CP_TGT[slot], target);
    st_wr!(CP_ST[slot], 2);
    0
}

/// 框是否还挂在界面上(包括"已请求关闭但还没删掉")。各页用它当"这段时间别重建页面"的门。
pub(crate) unsafe fn shown(slot: usize) -> bool { st_rd!(CP_ST[slot]) != 0 }

/// 两页里还有没有待收尾的框(供节拍自适应用: 框挂着就别降频)。
pub(crate) unsafe fn pending() -> bool {
    st_rd!(CP_ST[SLOT_FONT]) != 0 || st_rd!(CP_ST[SLOT_ICON]) != 0
}

/// 正在确认删除的目标(字体页 = 槽位号, 图标页 = 包号)。
pub(crate) unsafe fn target(slot: usize) -> u32 { st_rd!(CP_TGT[slot]) }

/// 叉/勾按钮的公共处理。关框走固件自己那套(事件上下文里直接 `obj_delete`,
/// 类析构自动清它的单例); 当场门没过就转兜底(返回 2), 由调用方在自己的安静拍调
/// `close_deferred` 补做销毁。按的是叉还是勾由调用方自己知道(两个按钮各自一个回调)。
///
/// 返回 0 = 这次点击与框无关(不是 CLICKED 事件 / 当前没有框);
///      1 = 框已当场删掉; 2 = 已转兜底销毁。
/// 两种非 0 结局调用方都要照常处理: 勾都要执行删除, 并且都要排一次整页重建
/// (obj_delete 只失效框自己那一片, 标题栏那一层要靠这次重建刷新)。
pub(crate) unsafe fn click(slot: usize, event: u32) -> u32 {
    let gc: unsafe extern "C" fn(u32) -> u32 = fw_api::event_get_code;
    if gc(event) != fw_api::ev::CLICKED { return 0; }
    if st_rd!(CP_ST[slot]) != 2 { return 0; }
    let o = st_rd!(CP_OBJ[slot]);
    if !alive(slot, o) {
        st_wr!(CP_ST[slot], 3);
        return 2;
    }
    fw_api::obj_delete(o);
    st_wr!(CP_ST[slot], 0);
    st_wr!(CP_OBJ[slot], 0);
    1
}

/// 兜底销毁: 调用方在自己的安静门里每拍试一次, 直到删掉或确认对象已经不在了。
/// 返回 false = 当前没有待兜底的请求。
pub(crate) unsafe fn close_deferred(slot: usize) -> bool {
    if st_rd!(CP_ST[slot]) != 3 { return false; }
    let o = st_rd!(CP_OBJ[slot]);
    if alive(slot, o) {
        fw_api::obj_delete(o);
    }
    st_wr!(CP_ST[slot], 0);
    st_wr!(CP_OBJ[slot], 0);
    true
}

/// 建页开头(render 开头)调用: 上一次漏删的框在这里兜掉 —— 它挂在页根上, 不在本次重建
/// 销毁的 content 里, 留着就是一整片盖住标题栏的黑。然后状态归零(句柄作废只在这一侧处理)。
pub(crate) unsafe fn reset(slot: usize) {
    let o = st_rd!(CP_OBJ[slot]);
    if fw_api::obj_is_child_of(root_of(slot), o) {
        fw_api::obj_delete(o);
    }
    st_wr!(CP_ST[slot], 0);
    st_wr!(CP_OBJ[slot], 0);
}

/// 新页实例边界(ui.rs 的 on_create)调用: **只丢状态不碰对象**。上一次页根连同挂在
/// 它上面的框已被固件销毁; 新页还可能复用同一块内存, 句柄"看着还对"就 delete 就是
/// 打在新对象上(所以句柄作废一律放 on_create, 不在 on_destroy 里碰状态)。
pub(crate) unsafe fn forget(slot: usize) {
    st_wr!(CP_ST[slot], 0);
    st_wr!(CP_OBJ[slot], 0);
    st_wr!(CP_TGT[slot], 0);
}
