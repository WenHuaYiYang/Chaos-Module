//! 设置 -> ChaosConnect.
use crate::page::*;
use crate::page::kit::{MenuSpec, RowModel, RowSub, ToggleDef};
use crate::connect_permission as permission;

pub(crate) struct ConnectPage;
pub(crate) static PAGE: ConnectPage = ConnectPage;
unsafe fn toggle() { crate::bridge_cfg::toggle(); }
static ENABLED: ToggleDef = ToggleDef {
    get: crate::bridge_cfg::enabled, toggle, status: None,
};
static ROWS: &[RowModel] = &[
    RowModel::toggle("启用\0".as_bytes(), &ENABLED),
    RowModel::display_sub("连接状态\0".as_bytes(),RowSub::Dyn(connection_text)),
    RowModel::display_sub("开发者模式\0".as_bytes(),RowSub::Dyn(developer_text)),
    RowModel::display_sub("授权状态\0".as_bytes(),RowSub::Dyn(authorization_text)),
    RowModel::action("发起蓝牙配对\0".as_bytes(),crate::connect_radio::start,0,None),
    RowModel::nav("权限设置\0".as_bytes(), Pid::ConnectPermissions as u32),
    RowModel::nav("ChaosConnect日志\0".as_bytes(), Pid::ConnectLog as u32),
    RowModel::note("支持Android ChaosConnect及配套插件。手机首次连接需手环确认，手机权限可在权限设置中控制。\n操作日志仅保存在内存，支持刷新、翻页和清空，重启后清空。\0".as_bytes()),
];
unsafe fn connection_text()->*const u8{
    if !crate::bridge_cfg::enabled(){"已停用\0".as_ptr()}
    else if permission::prompt_is_connection(){"Android等待连接确认\0".as_ptr()}
    else if permission::state()&1!=0 && permission::state()&(1<<5)!=0 && (permission::state()>>6)&7>=3 {"Android未获连接授权\0".as_ptr()}
    else if permission::state()&1!=0{if permission::state()&(1<<5)!=0{"Android会话\0".as_ptr()}else{"插件已连接\0".as_ptr()}}
    else{"等待插件连接\0".as_ptr()}
}
unsafe fn developer_text()->*const u8{
    if permission::state()&3==3{"已开启\0".as_ptr()}else{"已关闭\0".as_ptr()}
}
unsafe fn authorization_text()->*const u8{
    if permission::prompt_is_connection(){return "等待允许Android连接\0".as_ptr();}
    if permission::state()&(1<<5)!=0 && (permission::state()>>2)&7==0 {
        return match(permission::state()>>6)&7 {2=>"Android本次连接已允许\0".as_ptr(),3=>"Android连接已拒绝\0".as_ptr(),4=>"Android连接确认超时\0".as_ptr(),_=>"无待处理请求\0".as_ptr()};
    }
    match(permission::state()>>2)&7{
        1=>"等待手环确认\0".as_ptr(),2=>"本次已允许\0".as_ptr(),
        3=>"本次已拒绝\0".as_ptr(),4=>"授权已超时\0".as_ptr(),_=>"无待处理请求\0".as_ptr(),
    }
}
static mut LAST:u32=u32::MAX;
impl MenuSpec for ConnectPage {
    fn rows(&self) -> &'static [RowModel] { ROWS }
}
impl Page for ConnectPage {
    fn on_enter(&self){unsafe{permission::load();}}
    fn fill_title(&self, buf: *mut u8) { unsafe { put_str(buf, b"ChaosConnect\0"); } }
    fn render(&self, ctx: &PageCtx) -> PageResult { unsafe { frame::build(self, ctx, ctx.title) } }
    fn is_switch_row(&self, idx: usize) -> bool { kit::menu_is_switch(self, idx) }
    fn on_click(&self, idx: usize) { unsafe { kit::menu_click(self, idx) } }
    fn on_switch(&self, idx: usize) { unsafe { kit::menu_switch(self, idx) } }
    fn needs_tick(&self)->bool{true}
    fn tick(&self){unsafe{
        let state=permission::state()|((crate::bridge_cfg::enabled() as u32)<<31);
        if LAST==state{return;}LAST=state;
        for (slot,text) in [(1,connection_text()),(2,developer_text()),(3,authorization_text())]{
            let row=read_volatile(page_rows_ptr(Pid::Connect as usize).add(slot));
            if row!=0{fw_api::row_update(row,fw_api::NONE_PTR,fw_api::NONE_PTR,text,0,0);}
        }
    }}
}

pub(crate) struct PermissionsPage;
pub(crate) static PERMISSIONS:PermissionsPage=PermissionsPage;
unsafe fn policy_text()->*const u8{match permission::policy(){0=>"开启\0".as_ptr(),1=>"关闭\0".as_ptr(),_=>"询问\0".as_ptr()}}
static mut POLICY_ANDROID:bool=false;
unsafe fn open_plugin_policy(){POLICY_ANDROID=false;nav_goto(Pid::ConnectDevPolicy as u32,0);}
unsafe fn open_android_policy(){POLICY_ANDROID=true;nav_goto(Pid::ConnectDevPolicy as u32,0);}
static PERMISSION_ROWS:&[RowModel]=&[
    RowModel::nav("Android端权限\0".as_bytes(),Pid::AndroidPermissions as u32),
    RowModel{label:"允许开发者模式\0".as_bytes(),sub:RowSub::Dyn(policy_text),kind:kit::RowKind::Fn(open_plugin_policy),trailing:0,refresh:None},
    RowModel::note("权限由手环决定，插件无法绕过。关闭后将撤销已启用的开发者模式；询问时需在手环确认。\0".as_bytes()),
];
impl MenuSpec for PermissionsPage{fn rows(&self)->&'static [RowModel]{PERMISSION_ROWS}}
impl Page for PermissionsPage{
    fn on_enter(&self){unsafe{permission::load();}}
    fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"权限设置\0".as_bytes());}}
    fn render(&self,c:&PageCtx)->PageResult{unsafe{frame::build(self,c,c.title)}}
    fn on_click(&self,i:usize){unsafe{kit::menu_click(self,i);}}
}
pub(crate) struct PolicyPage;
pub(crate) static POLICY:PolicyPage=PolicyPage;
impl kit::checklist::CheckSpec for PolicyPage{
    fn count(&self)->usize{3}
    unsafe fn label(&self,i:usize)->*const u8{["开启\0","关闭\0","询问\0"][i.min(2)].as_ptr()}
    fn trailing(&self)->u32{fw_api::trailing::RADIO}
    unsafe fn selected(&self,i:usize)->bool{(if POLICY_ANDROID {permission::android_policy()}else{permission::policy()})==i as u32}
    unsafe fn choose(&self,i:usize){if POLICY_ANDROID {permission::set_android(i as u32,permission::android_mask());}else{permission::set_policy(i as u32);}}
}
impl Page for PolicyPage{
    fn on_enter(&self){unsafe{permission::load();}}
    fn fill_title(&self,p:*mut u8){unsafe{put_str(p,if POLICY_ANDROID {"允许Android连接\0".as_bytes()}else{"允许开发者模式\0".as_bytes()});}}
    fn render(&self,c:&PageCtx)->PageResult{unsafe{
        let last=kit::checklist::render(self,c)?;
        kit::section::note_label(c.content,last,if POLICY_ANDROID {"开启：允许Android连接。\n关闭：立即撤销Android访问。\n询问：每次连接需手环确认，60秒未确认拒绝。\0".as_bytes()}else{"开启：允许插件请求。\n关闭：拒绝请求并撤销开发者模式。\n询问：每次启用需手环确认，授权超时自动拒绝。\0".as_bytes()});Ok(())
    }}
    fn on_click(&self,i:usize){unsafe{kit::checklist::click(self,Pid::ConnectDevPolicy as u32,i);}}
}

pub(crate) struct AndroidPermissionsPage;
pub(crate) static ANDROID_PERMISSIONS:AndroidPermissionsPage=AndroidPermissionsPage;
unsafe fn connection_policy_text()->*const u8{["开启\0","关闭\0","询问\0"][permission::android_policy().min(2)as usize].as_ptr()}
macro_rules! mobile_permission {($def:ident,$get:ident,$toggle:ident,$bit:expr)=>{
    unsafe fn $get()->bool{permission::android_mask()&$bit!=0}
    unsafe fn $toggle(){permission::set_android(permission::android_policy(),permission::android_mask()^$bit);}
    static $def:ToggleDef=ToggleDef{get:$get,toggle:$toggle,status:None};
};}
mobile_permission!(READ_DEF,read_allowed,toggle_read,permission::READ);
mobile_permission!(WRITE_DEF,write_allowed,toggle_write,permission::WRITE);
mobile_permission!(MANAGE_DEF,manage_allowed,toggle_manage,permission::MANAGE);
mobile_permission!(DEV_DEF,dev_allowed,toggle_dev,permission::DEVELOPER);
mobile_permission!(CONTROL_DEF,control_allowed,toggle_control,permission::CONTROL);
static ANDROID_ROWS:&[RowModel]=&[
    RowModel{label:"允许Android连接\0".as_bytes(),sub:RowSub::Dyn(connection_policy_text),kind:kit::RowKind::Fn(open_android_policy),trailing:0,refresh:None},
    RowModel::toggle("读取文件\0".as_bytes(),&READ_DEF),
    RowModel::toggle("写入文件\0".as_bytes(),&WRITE_DEF),
    RowModel::toggle("删除和重命名\0".as_bytes(),&MANAGE_DEF),
    RowModel::toggle("开发者模式\0".as_bytes(),&DEV_DEF),
    RowModel::toggle("设备控制（预留）\0".as_bytes(),&CONTROL_DEF),
    RowModel::note("默认每次连接需确认，仅允许读取文件。关闭权限立即中止对应操作。开发者模式还须通过原开发者策略确认。设备控制为后续功能预留，目前无控制指令。手机不能修改这些权限。\0".as_bytes()),
];
impl MenuSpec for AndroidPermissionsPage{fn rows(&self)->&'static [RowModel]{ANDROID_ROWS}}
impl Page for AndroidPermissionsPage{
    fn on_enter(&self){unsafe{permission::load();}}
    fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"Android端权限\0".as_bytes());}}
    fn render(&self,c:&PageCtx)->PageResult{unsafe{frame::build(self,c,c.title)}}
    fn is_switch_row(&self,i:usize)->bool{kit::menu_is_switch(self,i)}
    fn on_click(&self,i:usize){unsafe{kit::menu_click(self,i)}}
    fn on_switch(&self,i:usize){unsafe{kit::menu_switch(self,i)}}
}
