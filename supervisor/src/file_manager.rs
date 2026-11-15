//! Local filesystem controller using the already verified .043 C VFS adapter.
use crate::keyboard;
#[path="file_path.rs"]pub(crate) mod path;
const CAP:usize=path::CAP;
pub(crate) const PAGE_SIZE:usize=8;
#[repr(C)]#[derive(Clone,Copy)]pub(crate) struct Stat{pub size:u32,pub directory:u8,pad:[u8;3],pub mtime:u32}
impl Stat{const EMPTY:Self=Self{size:0,directory:0,pad:[0;3],mtime:0};}
#[repr(C)]struct Dirent{name:[u8;256],directory:u8}
extern "C"{
 fn cc_io_opendir(path:*const u8)->*mut u8;fn cc_io_readdir(dir:*mut u8,out:*mut Dirent)->i32;fn cc_io_closedir(dir:*mut u8)->i32;
 fn cc_io_stat(path:*const u8,out:*mut Stat)->i32;fn cc_io_mkdir(path:*const u8)->i32;fn cc_io_rename(a:*const u8,b:*const u8)->i32;
 fn cc_io_remove(path:*const u8)->i32;fn cc_io_rmdir(path:*const u8)->i32;fn cc_io_open(path:*const u8,flags:u32)->i32;
 fn cc_io_read(fd:i32,out:*mut u8,n:u32)->i32;fn cc_io_close(fd:i32)->i32;
}
#[derive(Clone,Copy)]pub(crate) struct Entry{pub name:[u8;CAP],pub directory:bool}
impl Entry{const EMPTY:Self=Self{name:[0;CAP],directory:false};}
pub(crate) static mut DIR:[u8;CAP]=[0;CAP];
pub(crate) static mut TARGET:[u8;CAP]=[0;CAP];
pub(crate) static mut ENTRIES:[Entry;PAGE_SIZE]=[Entry::EMPTY;PAGE_SIZE];
pub(crate) static mut COUNT:usize=0;
pub(crate) static mut TOTAL:usize=0;
pub(crate) static mut OFFSET:usize=0;
pub(crate) static mut INFO:Stat=Stat::EMPTY;
pub(crate) static mut MESSAGE:[u8;96]=[0;96];
pub(crate) static mut PREVIEW:[u8;8192]=[0;8192];
pub(crate) static mut PREVIEW_ROWS:u32=0;
static mut EDIT:u8=0;
unsafe fn message(text:&str,rc:i32){
 let mut w=crate::W::new(core::ptr::addr_of_mut!(MESSAGE).cast(),96);w.s(text.as_bytes());
 if rc<0{w.s(b" errno=").n(rc.unsigned_abs());}w.end();
}
pub(crate) unsafe fn load(){
 if DIR[0]==0{DIR[..6].copy_from_slice(b"/data\0");}
 COUNT=0;TOTAL=0;ENTRIES=[Entry::EMPTY;PAGE_SIZE];
 let d=cc_io_opendir(DIR.as_ptr());if d.is_null(){message("无法打开目录",0);return;}
 loop{
  let mut e=Dirent{name:[0;CAP],directory:0};let rc=cc_io_readdir(d,&mut e);
  if rc!=0{if rc<0{message("读取目录失败",rc);}break;}
  let n=path::len(&e.name);if n==0||&e.name[..n]==b"."||&e.name[..n]==b".."{continue;}
  if TOTAL>=OFFSET&&COUNT<PAGE_SIZE{ENTRIES[COUNT]=Entry{name:e.name,directory:e.directory!=0};COUNT+=1;}TOTAL+=1;
 }
 cc_io_closedir(d);
 if OFFSET>=TOTAL&&OFFSET!=0{OFFSET=TOTAL.saturating_sub(1)/PAGE_SIZE*PAGE_SIZE;load();}
}
pub(crate) unsafe fn up(){DIR=path::parent(&DIR);OFFSET=0;MESSAGE[0]=0;load();}
pub(crate) unsafe fn select(i:usize)->bool{
 if i>=COUNT{return false;}let Some(p)=path::join(&DIR,&ENTRIES[i].name)else{message("路径无效或过长",0);return false;};
 TARGET=p;INFO=Stat::EMPTY;
 let rc=cc_io_stat(TARGET.as_ptr(),core::ptr::addr_of_mut!(INFO));if rc<0{message("文件信息读取失败",rc);return false;}
 if INFO.directory!=0{DIR=TARGET;OFFSET=0;MESSAGE[0]=0;load();false}else{true}
}
pub(crate) unsafe fn current(){TARGET=DIR;INFO=Stat::EMPTY;cc_io_stat(TARGET.as_ptr(),core::ptr::addr_of_mut!(INFO));}
pub(crate) unsafe fn mkdir(){EDIT=1;keyboard::open(core::ptr::null(),keyboard::Completion::FileName);}
pub(crate) unsafe fn rename(){
 EDIT=2;let at=path::len(&TARGET);let name=TARGET[..at].iter().rposition(|&c|c==b'/').map(|i|i+1).unwrap_or(0);
 keyboard::open(TARGET.as_ptr().add(name),keyboard::Completion::FileName);
}
pub(crate) unsafe fn save_name(value:*const u8)->bool{
 let mut n=0;while n<255&&*value.add(n)!=0{n+=1;}
 let name=core::slice::from_raw_parts(value,n);let base=if EDIT==1{DIR}else{path::parent(&TARGET)};
 let Some(dest)=path::join(&base,name)else{message("名称无效或路径过长",0);return false;};
 if path::blocked(&dest)||EDIT==2&&(path::blocked(&TARGET)||path::len(&TARGET)==1){message("该路径不支持操作",0);return false;}
 let mut existing=Stat::EMPTY;
 if cc_io_stat(dest.as_ptr(),&mut existing)==0{message("目标已存在",0);return false;}
 let rc=if EDIT==1{cc_io_mkdir(dest.as_ptr())}else{cc_io_rename(TARGET.as_ptr(),dest.as_ptr())};
 if rc<0{message("操作失败",rc);return false;}
 if EDIT==2{let old=TARGET;TARGET=dest;if DIR==old{DIR=dest;}}
 message("操作成功",0);load();
 crate::render_req(crate::page::Pid::Files as u32);
 if EDIT==2{crate::render_req(crate::page::Pid::FileDetail as u32);}
 true
}
pub(crate) unsafe fn remove()->bool{
 if path::len(&TARGET)<=1||path::blocked(&TARGET){message("该路径不支持删除",0);return false;}
 let rc=if INFO.directory!=0{cc_io_rmdir(TARGET.as_ptr())}else{cc_io_remove(TARGET.as_ptr())};
 if rc<0{message("删除失败（目录必须为空）",rc);return false;}
 if TARGET==DIR{DIR=path::parent(&DIR);OFFSET=0;}
 message("已删除",0);load();true
}
pub(crate) unsafe fn preview(){
 PREVIEW[0]=0;PREVIEW_ROWS=1;
 if INFO.directory!=0||path::blocked(&TARGET){message("该路径不支持预览",0);return;}
 let fd=cc_io_open(TARGET.as_ptr(),1);if fd<0{message("打开文件失败",fd);return;}
 let mut bytes=[0u8;2048];let mut n=0;
 while n<bytes.len(){let r=cc_io_read(fd,bytes.as_mut_ptr().add(n),(bytes.len()-n)as u32);if r<=0{if r<0{message("读取文件失败",r);}break;}n+=r as usize;}
 cc_io_close(fd);
 let mut w=crate::W::new(core::ptr::addr_of_mut!(PREVIEW).cast(),8192);let mut width=0;
 let text=match core::str::from_utf8(&bytes[..n]){
  Ok(text)=>Some(text),Err(e)if e.error_len().is_none()=>core::str::from_utf8(&bytes[..e.valid_up_to()]).ok(),_=>None,
 };
 if let Some(text)=text{
  for c in text.chars(){
   if c=='\n'{w.c(b'\n');PREVIEW_ROWS+=1;width=0;continue;}
   let cost=if c.is_ascii(){1}else{2};if width+cost>20{w.c(b'\n');PREVIEW_ROWS+=1;width=0;}
   if c.is_control(){w.c(b' ');}else{let mut b=[0;4];w.s(c.encode_utf8(&mut b).as_bytes());}width+=cost;
  }
 }else{
  w.s("二进制（十六进制）\n".as_bytes());PREVIEW_ROWS+=1;const HEX:&[u8]=b"0123456789ABCDEF";
  for(i,&b)in bytes[..n.min(256)].iter().enumerate(){w.c(HEX[(b>>4)as usize]).c(HEX[(b&15)as usize]).c(b' ');if i%6==5{w.c(b'\n');PREVIEW_ROWS+=1;}}
 }
 if INFO.size>n as u32{w.s("\n（仅显示前2KiB）".as_bytes());PREVIEW_ROWS+=2;}w.end();
}
