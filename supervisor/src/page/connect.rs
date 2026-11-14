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
    RowModel::nav("权限设置\0".as_bytes(), Pid::ConnectPermissions as u32),
    RowModel::nav("ChaosConnect日志\0".as_bytes(), Pid::ConnectLog as u32),
    RowModel::note("通过配套插件浏览、读取和管理文件。\n操作日志仅保存在内存，支持刷新、翻页和清空，重启后清空。\0".as_bytes()),
];
unsafe fn connection_text()->*const u8{
    if !crate::bridge_cfg::enabled(){"已停用\0".as_ptr()}
    else if permission::state()&1!=0{"已连接\0".as_ptr()}
    else{"等待插件连接\0".as_ptr()}
}
unsafe fn developer_text()->*const u8{
    if permission::state()&3==3{"已开启\0".as_ptr()}else{"已关闭\0".as_ptr()}
}
unsafe fn authorization_text()->*const u8{
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
            if row!=0{fw_api::row_update(row,fw_api::NONE_PTR,text,fw_api::NONE_PTR,0,0);}
        }
    }}
}

pub(crate) struct PermissionsPage;
pub(crate) static PERMISSIONS:PermissionsPage=PermissionsPage;
unsafe fn policy_text()->*const u8{match permission::policy(){0=>"开启\0".as_ptr(),1=>"关闭\0".as_ptr(),_=>"询问\0".as_ptr()}}
static PERMISSION_ROWS:&[RowModel]=&[
    RowModel{label:"允许开发者模式\0".as_bytes(),sub:RowSub::Dyn(policy_text),kind:kit::RowKind::Nav(Pid::ConnectDevPolicy as u32),trailing:0,refresh:None},
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
    unsafe fn selected(&self,i:usize)->bool{permission::policy()==i as u32}
    unsafe fn choose(&self,i:usize){permission::set_policy(i as u32);}
}
impl Page for PolicyPage{
    fn on_enter(&self){unsafe{permission::load();}}
    fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"允许开发者模式\0".as_bytes());}}
    fn render(&self,c:&PageCtx)->PageResult{unsafe{
        let last=kit::checklist::render(self,c)?;
        kit::section::note_label(c.content,last,"开启：允许插件请求。\n关闭：拒绝请求并撤销开发者模式。\n询问：每次启用需手环确认，授权超时自动拒绝。\0".as_bytes());Ok(())
    }}
    fn on_click(&self,i:usize){unsafe{kit::checklist::click(self,Pid::ConnectDevPolicy as u32,i);}}
}
