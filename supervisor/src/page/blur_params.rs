//! 系统美化 -> 模糊V2参数；所有数值使用纯数字键盘输入。
use crate::page::*;
use crate::page::kit::{MenuSpec,RowKind,RowModel,RowSub};
pub(crate) struct BlurParamsPage;
pub(crate) static PAGE: BlurParamsPage=BlurParamsPage;
static mut EDITING: usize=0;
static mut INITIAL: [u8;16]=[0;16];
static mut VALUES: [[u8;64];6]=[[0;64];6];
pub(crate) unsafe fn title() -> &'static [u8] {
    match EDITING {0=>"模糊强度\0".as_bytes(),1=>"背景亮度（%）\0".as_bytes(),
        2=>"按钮亮度（%）\0".as_bytes(),3=>"玻璃不透明度（%）\0".as_bytes(),
        4=>"状态底色不透明度（%）\0".as_bytes(),_=>"背景刷新间隔（毫秒）\0".as_bytes()}
}
unsafe fn open(index: usize) {
    EDITING=index;let mut w=W::new(core::ptr::addr_of_mut!(INITIAL).cast(),16);
    w.n(crate::blur_params::get(index));w.end();
    crate::keyboard::open(core::ptr::addr_of!(INITIAL).cast(),crate::keyboard::Completion::BlurParam);
}
unsafe fn value(index: usize) -> *const u8 {
    let p=core::ptr::addr_of_mut!(VALUES[index]).cast();let mut w=W::new(p,64);
    w.n(crate::blur_params::get(index));
    if index==5 {w.s(" 毫秒（100～2000）".as_bytes());}
    else {w.s(b"  (").n(crate::blur_params::MIN[index]).s(b"~").n(crate::blur_params::MAX[index]).c(b')');}
    w.end();p
}
pub(crate) unsafe fn save(text: *const u8) -> bool {
    let mut v=0u32;let mut n=0;
    loop {let c=core::ptr::read_volatile(text.add(n));if c==0 {break;}
        if !c.is_ascii_digit() || n>=4 {return false;}v=v*10+(c-b'0') as u32;n+=1;}
    if n==0 || !crate::blur_params::set(EDITING,v) {return false;}
    render_req(Pid::BlurParams as u32);true
}
pub(crate) unsafe fn error_text() -> *const u8 {
    let p=core::ptr::addr_of_mut!(INITIAL).cast();let mut w=W::new(p,16);
    if crate::blur_params::save_failed() {w.s("保存失败".as_bytes());}
    else {w.n(crate::blur_params::MIN[EDITING]).c(b'~').n(crate::blur_params::MAX[EDITING]);}w.end();p
}
macro_rules! item {($open:ident,$value:ident,$i:expr)=>{
    unsafe fn $open(){open($i)} unsafe fn $value()->*const u8{value($i)}
};}
item!(radius,radius_value,0);item!(background,background_value,1);item!(button,button_value,2);
item!(glass,glass_value,3);item!(tint,tint_value,4);item!(interval,interval_value,5);
unsafe fn reset_status() -> *const u8 {
    if crate::blur_params::save_failed() {"保存失败，请重试\0".as_ptr()} else {b"\0".as_ptr()}
}
unsafe fn reset(){let _=crate::blur_params::reset();render_req(Pid::BlurParams as u32);}
static ROWS: &[RowModel]=&[
    RowModel{label:"模糊强度\0".as_bytes(),sub:RowSub::Dyn(radius_value),kind:RowKind::Fn(radius),trailing:0,refresh:None},
    RowModel{label:"背景亮度\0".as_bytes(),sub:RowSub::Dyn(background_value),kind:RowKind::Fn(background),trailing:0,refresh:None},
    RowModel{label:"按钮亮度\0".as_bytes(),sub:RowSub::Dyn(button_value),kind:RowKind::Fn(button),trailing:0,refresh:None},
    RowModel{label:"玻璃不透明度\0".as_bytes(),sub:RowSub::Dyn(glass_value),kind:RowKind::Fn(glass),trailing:0,refresh:None},
    RowModel{label:"状态底色不透明度\0".as_bytes(),sub:RowSub::Dyn(tint_value),kind:RowKind::Fn(tint),trailing:0,refresh:None},
    RowModel{label:"背景刷新间隔\0".as_bytes(),sub:RowSub::Dyn(interval_value),kind:RowKind::Fn(interval),trailing:0,refresh:None},
    RowModel{label:"恢复默认参数\0".as_bytes(),sub:RowSub::Dyn(reset_status),kind:RowKind::Fn(reset),trailing:0,refresh:None},
    RowModel::note("亮度越高越亮；不透明度越低越透明。刷新间隔越短，动态背景更新越快、耗电越高。参数自动保存，仅对模糊V2生效。\0".as_bytes()),
];
impl MenuSpec for BlurParamsPage {fn rows(&self)->&'static [RowModel]{ROWS}}
impl Page for BlurParamsPage {
    fn fill_title(&self,buf:*mut u8){unsafe{put_str(buf,"模糊V2参数\0".as_bytes());}}
    fn render(&self,ctx:&PageCtx)->PageResult{unsafe{frame::build(self,ctx,ctx.title)}}
    fn on_click(&self,index:usize){unsafe{kit::menu_click(self,index)}}
}
