//! Standard Bluetooth pairing entry. UI only queues work for the native BT loop.
use crate::*;
use crate::page::*;
use core::sync::atomic::{AtomicBool,AtomicI32,Ordering};
use crate::page::kit::{MenuSpec,RowKind,RowModel,RowSub};
extern "C" {
    fn chaos_connect_radio_poll()->u32;
    fn chaos_connect_pair_start();
    fn chaos_connect_pair_scan();
    fn chaos_connect_pair_device(index:u32);
    fn chaos_connect_pair_name(index:u32,out:*mut u8,capacity:u32)->u32;
    fn chaos_connect_radio_status()->u32;
}
static WORKER_STARTED:AtomicBool=AtomicBool::new(false);
static WORKER_ERROR:AtomicI32=AtomicI32::new(0);
unsafe extern "C" fn worker(_:i32,_:*mut *mut u8)->i32 {
    loop {let delay=chaos_connect_radio_poll().clamp(100_000,1_000_000);fw_api::nsh_usleep(delay);}
}
pub(crate) fn ensure_worker(){
    if WORKER_STARTED.swap(true,Ordering::AcqRel){return;}
    let pid=unsafe{fw_api::nsh_spawn(b"chaos.connect\0".as_ptr(),worker,core::ptr::null_mut())};
    WORKER_ERROR.store(if pid>0{0}else{pid.min(-1)},Ordering::Release);
}
static mut STATUS:[u8;96]=[0;96];
static mut NAMES:[[u8;64];6]=[[0;64];6];
static mut TICKS:u32=0;
pub(crate) unsafe fn start(){
    if !crate::bridge_cfg::enabled(){crate::bridge_cfg::toggle();}
    if WORKER_ERROR.load(Ordering::Acquire)<0{WORKER_STARTED.store(false,Ordering::Release);}
    ensure_worker();chaos_connect_pair_start();nav_goto(Pid::ConnectPair as u32,0);
}
unsafe fn scan(){chaos_connect_pair_scan();}
unsafe fn status()->*const u8 {
    let value=chaos_connect_radio_status();let p=core::ptr::addr_of_mut!(STATUS).cast();let mut w=W::new(p,96);
    let error=WORKER_ERROR.load(Ordering::Acquire);
    if error<0{w.s("蓝牙任务启动失败：".as_bytes()).n(error.unsigned_abs());}
    else if value&0x80000000!=0 {w.s("蓝牙错误：".as_bytes()).n(value&0xffff);}
    else if value&8!=0 {w.s("手机蓝牙已连接，等待协议授权".as_bytes());}
    else if value&2!=0 {w.s("配对窗口已开启（60秒）".as_bytes());}
    else if value&4!=0 {w.s("已完成配对，可在手机连接ChaosConnect".as_bytes());}
    else if value&1!=0 {w.s("服务就绪，等待已配对手机".as_bytes());}
    else {w.s("正在准备蓝牙服务".as_bytes());}w.end();p
}
unsafe fn name(index:usize)->*const u8{
    let p=core::ptr::addr_of_mut!(NAMES[index]).cast();
    if chaos_connect_pair_name(index as u32,p,64)==0 {put_str(p,"未发现设备\0".as_bytes());}p
}
macro_rules! peer {($select:ident,$name:ident,$i:expr)=>{
    unsafe fn $select(){chaos_connect_pair_device($i);}
    unsafe fn $name()->*const u8{name($i as usize)}
};}
peer!(select0,name0,0);peer!(select1,name1,1);peer!(select2,name2,2);
peer!(select3,name3,3);peer!(select4,name4,4);peer!(select5,name5,5);
static ROWS:&[RowModel]=&[
    RowModel::action("扫描可配对手机\0".as_bytes(),scan,0,None),
    RowModel::display_sub("蓝牙状态\0".as_bytes(),RowSub::Dyn(status)),
    RowModel{label:"设备1\0".as_bytes(),sub:RowSub::Dyn(name0),kind:RowKind::Fn(select0),trailing:0,refresh:None},
    RowModel{label:"设备2\0".as_bytes(),sub:RowSub::Dyn(name1),kind:RowKind::Fn(select1),trailing:0,refresh:None},
    RowModel{label:"设备3\0".as_bytes(),sub:RowSub::Dyn(name2),kind:RowKind::Fn(select2),trailing:0,refresh:None},
    RowModel{label:"设备4\0".as_bytes(),sub:RowSub::Dyn(name3),kind:RowKind::Fn(select3),trailing:0,refresh:None},
    RowModel{label:"设备5\0".as_bytes(),sub:RowSub::Dyn(name4),kind:RowKind::Fn(select4),trailing:0,refresh:None},
    RowModel{label:"设备6\0".as_bytes(),sub:RowSub::Dyn(name5),kind:RowKind::Fn(select5),trailing:0,refresh:None},
    RowModel::note("手机保持在系统蓝牙设置页面。可以在手机搜索并配对手环，也可扫描后点选手机，由手环发起普通蓝牙配对。配对窗口60秒后恢复原可见性。完成后在Android ChaosConnect中连接，仍需在手环确认本次会话；权限由手环控制。\0".as_bytes()),
];
pub(crate) struct PairPage;
pub(crate) static PAGE:PairPage=PairPage;
impl MenuSpec for PairPage{fn rows(&self)->&'static [RowModel]{ROWS}}
impl Page for PairPage{
    fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"蓝牙配对\0".as_bytes());}}
    fn render(&self,c:&PageCtx)->PageResult{unsafe{frame::build(self,c,c.title)}}
    fn on_click(&self,i:usize){unsafe{kit::menu_click(self,i)}}
    fn needs_tick(&self)->bool{true}
    fn tick(&self){unsafe{
        TICKS=TICKS.wrapping_add(1);if TICKS%5!=0{return;}
        for slot in 1..8 {
            let row=read_volatile(page_rows_ptr(Pid::ConnectPair as usize).add(slot));
            if row!=0{let text=if slot==1{status()}else{name(slot-2)};fw_api::row_update(row,fw_api::NONE_PTR,fw_api::NONE_PTR,text,0,0);}
        }
    }}
}
