//! Local file browser and detail actions, independent of ChaosConnect sessions.
use crate::page::*;
use crate::file_manager as files;
pub(crate) struct FilesPage;
pub(crate) struct DetailPage;
pub(crate) static FILES:FilesPage=FilesPage;
pub(crate) static DETAIL:DetailPage=DetailPage;
static mut LABELS:[[u8;280];8]=[[0;280];8];
static mut META:[u8;768]=[0;768];
static mut SHOW_PREVIEW:bool=false;
static mut DELETE_PENDING:bool=false;

unsafe fn row(ctx:&PageCtx,prev:u32,slot:usize,label:&[u8])->Result<u32,PageError>{
 let r=build_row(ctx,prev,slot,&RowSpec::clickable(label));if r==0{Err(PageError::RowFailed(slot))}else{Ok(r)}
}
unsafe fn text(ctx:&PageCtx,prev:u32,value:*const u8,rows:u32)->u32{
 let label=fw_api::label_create(ctx.content);if label==0{return prev;}
 fw_api::style_apply(label,crate::FW_STYLE_MISANS_REG_24,255,0);
 fw_api::obj_set_size_wh(label,320,rows.max(1)*36);
 fw_api::label_set_text(label,value);
 fw_api::obj_align_to(label,prev,fw_api::align::OUT_BOTTOM_MID,0,8);label
}
impl Page for FilesPage{
 fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"文件管理器\0".as_bytes());}}
 fn on_enter(&self){unsafe{files::load();render_req(Pid::Files as u32);}}
 fn render(&self,ctx:&PageCtx)->PageResult{unsafe{
  let mut prev=build_row(ctx,ctx.title,0,&RowSpec::display("当前目录\0".as_bytes(),files::DIR.as_ptr()));
  if prev==0{return Err(PageError::RowFailed(0));}
  for(i,label)in ["上一级\0","刷新\0","当前目录详情\0","新建文件夹\0","上一页\0","下一页\0"].iter().enumerate(){prev=row(ctx,prev,i+1,label.as_bytes())?;}
  for i in 0..files::COUNT{
   let p=core::ptr::addr_of_mut!(LABELS[i]).cast();let mut w=W::new(p,280);
   w.s(if files::ENTRIES[i].directory{"[目录] ".as_bytes()}else{"[文件] ".as_bytes()});
   let name=&files::ENTRIES[i].name;let name=&name[..files::path::len(name)];
   w.s(if core::str::from_utf8(name).is_ok(){name}else{"非UTF-8名称".as_bytes()});w.end();
   prev=row(ctx,prev,7+i,core::slice::from_raw_parts(p,cstr_len(p)+1))?;
  }
  let mut status=[0u8;128];let mut w=W::new(status.as_mut_ptr(),128);
  w.s("第 ".as_bytes()).n((files::OFFSET/8+1)as u32).s(" 页 · 共 ".as_bytes()).n(files::TOTAL as u32).s(" 项\n".as_bytes());
  w.s(&files::MESSAGE[..files::path::len(&files::MESSAGE)]);w.end();text(ctx,prev,status.as_ptr(),3);Ok(())
 }}
 fn on_click(&self,i:usize){unsafe{
  match i{
   1=>files::up(),2=>files::load(),3=>{files::current();SHOW_PREVIEW=false;nav_goto(Pid::FileDetail as u32,0);return;},
   4=>{files::mkdir();return;},5=>{files::OFFSET=files::OFFSET.saturating_sub(8);files::load();},
   6=>{if files::OFFSET+8<files::TOTAL{files::OFFSET+=8;files::load();}},
   7..=14=>{if files::select(i-7){SHOW_PREVIEW=false;nav_goto(Pid::FileDetail as u32,0);return;}},_=>return,
  }render_req(Pid::Files as u32);
 }}
}
unsafe extern "C" fn no(event:u32){
 if crate::confirm_pop::click(crate::confirm_pop::SLOT_FILE,event)!=0{render_req(Pid::FileDetail as u32);}
}
unsafe extern "C" fn yes(event:u32){
 if crate::confirm_pop::click(crate::confirm_pop::SLOT_FILE,event)!=0{DELETE_PENDING=true;}
}
impl Page for DetailPage{
 fn fill_title(&self,p:*mut u8){unsafe{put_str(p,"文件详情\0".as_bytes());}}
 fn on_instance_reset(&self){unsafe{crate::confirm_pop::forget(crate::confirm_pop::SLOT_FILE);}}
 fn render(&self,ctx:&PageCtx)->PageResult{unsafe{
  crate::confirm_pop::reset(crate::confirm_pop::SLOT_FILE);
  let mut prev=row(ctx,ctx.title,0,if files::INFO.directory!=0{"打开文件夹\0".as_bytes()}else{"预览文件\0".as_bytes()})?;
  prev=row(ctx,prev,1,"重命名\0".as_bytes())?;prev=row(ctx,prev,2,"删除\0".as_bytes())?;
  let mut w=W::new(core::ptr::addr_of_mut!(META).cast(),768);let mut lines=1;let mut col=0;
  let name=core::str::from_utf8(&files::TARGET[..files::path::len(&files::TARGET)]).unwrap_or("非UTF-8路径");
  for c in name.chars(){let width=if c.is_ascii(){1}else{2};if col+width>20{w.c(b'\n');lines+=1;col=0;}let mut b=[0;4];w.s(c.encode_utf8(&mut b).as_bytes());col+=width;}
  w.s("\n类型：".as_bytes()).s(if files::INFO.directory!=0{"目录".as_bytes()}else{"文件".as_bytes()});
  w.s("\n大小：".as_bytes()).sz(files::INFO.size).s(b"\n");
  w.s(&files::MESSAGE[..files::path::len(&files::MESSAGE)]);w.end();
  prev=text(ctx,prev,core::ptr::addr_of!(META).cast(),lines+5);
  if SHOW_PREVIEW{text(ctx,prev,core::ptr::addr_of!(files::PREVIEW).cast(),files::PREVIEW_ROWS+2);}
  Ok(())
 }}
 fn on_click(&self,i:usize){unsafe{match i{
  0=>{if files::INFO.directory!=0{files::DIR=files::TARGET;files::OFFSET=0;files::load();nav_back();return;}files::preview();SHOW_PREVIEW=true;render_req(Pid::FileDetail as u32);},
  1=>files::rename(),
  2=>{let rc=crate::confirm_pop::build(crate::confirm_pop::SLOT_FILE,0,core::ptr::null(),0,no as *const()as u32,yes as *const()as u32);if rc!=0{kit::feedback::show(Pid::FileDetail as u32,"无法创建删除确认框\0".as_bytes(),kit::feedback::TONE_DANGER,40);}},_=>{}
 }}}
 fn needs_tick(&self)->bool{unsafe{DELETE_PENDING||crate::confirm_pop::shown(crate::confirm_pop::SLOT_FILE)}}
 fn tick(&self){unsafe{
  if crate::confirm_pop::close_deferred(crate::confirm_pop::SLOT_FILE){render_req(Pid::FileDetail as u32);}
  if DELETE_PENDING{DELETE_PENDING=false;if files::remove(){nav_back();}else{render_req(Pid::FileDetail as u32);}}
 }}
 fn on_pause(&self){unsafe{DELETE_PENDING=false;}}
}
