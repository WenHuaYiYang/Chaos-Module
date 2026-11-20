//! 系统美化 -> 模糊V2参数；所有数值使用纯数字键盘输入。
use crate::page::*;
use crate::page::kit::{MenuSpec,RowKind,RowModel,RowSub};
pub(crate) struct BlurParamsPage;
pub(crate) static PAGE: BlurParamsPage=BlurParamsPage;
pub(crate) struct BlurGroupPage {button: bool}
pub(crate) static BACKGROUND: BlurGroupPage=BlurGroupPage{button:false};
pub(crate) static BUTTON: BlurGroupPage=BlurGroupPage{button:true};
static mut BUTTON_GROUP: bool=false;
static mut EDITING: usize=0;
static mut INITIAL: [u8;16]=[0;16];
static mut VALUES: [[u8;64];crate::blur_params::COUNT]=[[0;64];crate::blur_params::COUNT];
const LABELS: [&[u8];crate::blur_params::COUNT]=[
    "模糊强度\0".as_bytes(),"背景亮度\0".as_bytes(),"按钮亮度\0".as_bytes(),
    "玻璃不透明度\0".as_bytes(),"状态底色不透明度\0".as_bytes(),"背景采集间隔\0".as_bytes(),
    "模糊强度\0".as_bytes(),"背景不透明度\0".as_bytes(),"饱和度\0".as_bytes(),"对比度\0".as_bytes(),
    "染色红色（R）\0".as_bytes(),"染色绿色（G）\0".as_bytes(),"染色蓝色（B）\0".as_bytes(),"染色比例\0".as_bytes(),
    "饱和度\0".as_bytes(),"对比度\0".as_bytes(),"染色红色（R）\0".as_bytes(),
    "染色绿色（G）\0".as_bytes(),"染色蓝色（B）\0".as_bytes(),"染色比例\0".as_bytes(),"背景采集间隔\0".as_bytes(),
];
pub(crate) unsafe fn title() -> &'static [u8] {LABELS[EDITING]}
unsafe fn open(index: usize) {
    EDITING=index;let mut w=W::new(core::ptr::addr_of_mut!(INITIAL).cast(),16);
    w.n(crate::blur_params::get(index));w.end();
    crate::keyboard::open(core::ptr::addr_of!(INITIAL).cast(),crate::keyboard::Completion::BlurParam);
}
unsafe fn value(index: usize) -> *const u8 {
    let p=core::ptr::addr_of_mut!(VALUES[index]).cast();let mut w=W::new(p,64);
    w.n(crate::blur_params::get(index));
    if index==5 || index==20 {w.s(" 毫秒".as_bytes());}
    else if matches!(index,1..=4 | 7..=9 | 13..=15 | 19) {w.c(b'%');}
    w.s(b"  (").n(crate::blur_params::MIN[index]).s(b"~").n(crate::blur_params::MAX[index]).c(b')');
    w.end();p
}
pub(crate) unsafe fn save(text: *const u8) -> bool {
    let mut v=0u32;let mut n=0;
    loop {let c=core::ptr::read_volatile(text.add(n));if c==0 {break;}
        if !c.is_ascii_digit() || n>=4 {return false;}v=v*10+(c-b'0') as u32;n+=1;}
    if n==0 || !crate::blur_params::set(EDITING,v) {return false;}
    render_group();true
}
pub(crate) unsafe fn error_text() -> *const u8 {
    let p=core::ptr::addr_of_mut!(INITIAL).cast();let mut w=W::new(p,16);
    if crate::blur_params::save_failed() {w.s("保存失败".as_bytes());}
    else {w.n(crate::blur_params::MIN[EDITING]).c(b'~').n(crate::blur_params::MAX[EDITING]);}w.end();p
}
macro_rules! item {($open:ident,$value:ident,$i:expr)=>{
    unsafe fn $open(){open($i)} unsafe fn $value()->*const u8{value($i)}
};}
item!(radius,radius_value,0);item!(background,background_value,1);
item!(button,button_value,2);item!(glass,glass_value,3);item!(tint,tint_value,4);
item!(interval,interval_value,5);item!(button_radius,button_radius_value,6);
item!(bg_opacity,bg_opacity_value,7);item!(bg_sat,bg_sat_value,8);item!(bg_contrast,bg_contrast_value,9);
item!(bg_red,bg_red_value,10);item!(bg_green,bg_green_value,11);item!(bg_blue,bg_blue_value,12);item!(bg_tint,bg_tint_value,13);
item!(btn_sat,btn_sat_value,14);item!(btn_contrast,btn_contrast_value,15);
item!(btn_red,btn_red_value,16);item!(btn_green,btn_green_value,17);item!(btn_blue,btn_blue_value,18);item!(btn_tint,btn_tint_value,19);
item!(bg_interval,bg_interval_value,20);
macro_rules! param_row {($i:expr,$action:ident,$value:ident)=>{
    RowModel{label:LABELS[$i],sub:RowSub::Dyn($value),kind:RowKind::Fn($action),trailing:0,refresh:None}
};}
unsafe fn reset_status() -> *const u8 {
    if crate::blur_params::save_failed() {"保存失败，请重试\0".as_ptr()} else {b"\0".as_ptr()}
}
unsafe fn render_group(){render_req(if BUTTON_GROUP {Pid::BlurButtonParams as u32}else{Pid::BlurBackgroundParams as u32});}
unsafe fn reset(){let _=crate::blur_params::reset_group(BUTTON_GROUP);render_group();}
static ROWS: &[RowModel]=&[
    RowModel::nav("背景调整\0".as_bytes(),Pid::BlurBackgroundParams as u32),
    RowModel::nav("按钮调整\0".as_bytes(),Pid::BlurButtonParams as u32),
    RowModel::note("背景和按钮分别设置模糊、亮度、透明度、饱和度、对比度和RGB染色。参数自动保存，模糊V2与V3共用；按钮调整同时应用于控制中心和启动器布局切换按钮。\0".as_bytes()),
];
static BACKGROUND_ROWS: &[RowModel]=&[
    RowModel::section("模糊与透明度\0".as_bytes()),
    param_row!(0,radius,radius_value),param_row!(1,background,background_value),param_row!(7,bg_opacity,bg_opacity_value),
    RowModel::section("色彩调整\0".as_bytes()),
    param_row!(8,bg_sat,bg_sat_value),param_row!(9,bg_contrast,bg_contrast_value),
    param_row!(13,bg_tint,bg_tint_value),param_row!(10,bg_red,bg_red_value),
    param_row!(11,bg_green,bg_green_value),param_row!(12,bg_blue,bg_blue_value),
    RowModel::section("刷新与重置\0".as_bytes()),
    param_row!(20,bg_interval,bg_interval_value),
    RowModel{label:"恢复背景默认参数\0".as_bytes(),sub:RowSub::Dyn(reset_status),kind:RowKind::Fn(reset),trailing:0,refresh:None},
    RowModel::note("以下参数由模糊V2与V3共用；省电滤波使用原生参数，不应用这些设置。\0".as_bytes()),
    RowModel::note("强度0关闭模糊。亮度、饱和度和对比度100%为原值；饱和度0为灰度，对比度0为均匀灰色。染色比例0不染色，100%为所设RGB纯色。\0".as_bytes()),
    RowModel::note("不透明度0隐藏模糊层、透出下层画面。背景与按钮独立调色；两边共享表盘采集，可见区域中较短的采集间隔会让共享源更快更新。\0".as_bytes()),
];
static BUTTON_ROWS: &[RowModel]=&[
    RowModel::section("模糊与透明度\0".as_bytes()),
    param_row!(6,button_radius,button_radius_value),param_row!(2,button,button_value),
    param_row!(3,glass,glass_value),param_row!(4,tint,tint_value),
    RowModel::section("色彩调整\0".as_bytes()),
    param_row!(14,btn_sat,btn_sat_value),param_row!(15,btn_contrast,btn_contrast_value),
    param_row!(19,btn_tint,btn_tint_value),param_row!(16,btn_red,btn_red_value),
    param_row!(17,btn_green,btn_green_value),param_row!(18,btn_blue,btn_blue_value),
    RowModel::section("刷新与重置\0".as_bytes()),
    param_row!(5,interval,interval_value),
    RowModel{label:"恢复按钮默认参数\0".as_bytes(),sub:RowSub::Dyn(reset_status),kind:RowKind::Fn(reset),trailing:0,refresh:None},
    RowModel::note("强度0关闭模糊。亮度、饱和度和对比度100%为原值，0%分别为黑色、灰度、均匀灰色。RGB各0～255，染色比例控制颜色覆盖。\0".as_bytes()),
    RowModel::note("玻璃与状态底色为两层：完全透明需把两项不透明度都设为0。采集间隔只控制共享表盘源更新，不限制拖动取样；间隔越短耗电越高。\0".as_bytes()),
];
impl MenuSpec for BlurGroupPage {fn rows(&self)->&'static [RowModel]{if self.button {BUTTON_ROWS}else{BACKGROUND_ROWS}}}
impl Page for BlurGroupPage {
    fn fill_title(&self,buf:*mut u8){unsafe{put_str(buf,if self.button {"按钮调整\0".as_bytes()}else{"背景调整\0".as_bytes()});}}
    fn render(&self,ctx:&PageCtx)->PageResult{unsafe{BUTTON_GROUP=self.button;frame::build(self,ctx,ctx.title)}}
    fn on_click(&self,index:usize){unsafe{BUTTON_GROUP=self.button;kit::menu_click(self,index)}}
}
impl MenuSpec for BlurParamsPage {fn rows(&self)->&'static [RowModel]{ROWS}}
impl Page for BlurParamsPage {
    fn fill_title(&self,buf:*mut u8){unsafe{put_str(buf,"模糊V2参数\0".as_bytes());}}
    fn render(&self,ctx:&PageCtx)->PageResult{unsafe{frame::build(self,ctx,ctx.title)}}
    fn on_click(&self,index:usize){unsafe{kit::menu_click(self,index)}}
}
