//! Production log page with native layout seams, including long paths and paging.
#![allow(dead_code, static_mut_refs)]
use core::ptr::{read_volatile, write_volatile};
use std::sync::Mutex;
#[path = "../src/connect_log_buffer.rs"] mod connect_log_buffer;
struct Ui { log: connect_log_buffer::Records<32768>, text: String, height: u32, board: i32, clears: usize }
static UI: Mutex<Option<Ui>> = Mutex::new(None);
fn with<T>(f: impl FnOnce(&mut Ui)->T)->T { f(UI.lock().unwrap().as_mut().unwrap()) }
pub const FW_STYLE_MISANS_REG_24:u32=24;
mod connect_log {
    pub unsafe fn page(p:*mut u8,cap:usize,skip:usize,limit:usize)->(usize,usize){
        super::with(|u|u.log.page(core::slice::from_raw_parts_mut(p,cap),skip,limit))
    }
    pub unsafe fn clear(){super::with(|u|{u.log.clear();u.clears+=1;});}
}
pub mod fw_api {
    pub mod align {pub const TOP_LEFT:u32=0;}
    pub unsafe fn label_create(_:u32)->u32{100}
    pub unsafe fn style_apply(_:u32,_:u32,_:u32,_:u32){}
    pub unsafe fn label_set_text(_:u32,p:*const u8){super::with(|u|u.text=std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().into());}
    pub unsafe fn obj_set_size_wh(_:u32,_:u32,h:u32){super::with(|u|u.height=h);}
    pub unsafe fn obj_align(obj:u32,_:u32,x:i32,y:i32){if obj==1{assert_eq!((x,y),(4,112));}}
    pub unsafe fn obj_clear_scrollable(_:u32){}
}
mod page {
    pub use crate::fw_api;
    pub use core::ptr::{read_volatile,write_volatile};
    pub const TILE_W:u32=156;
    pub enum Pid {ConnectLog=17}
    pub struct PageCtx {pub content:u32,pub title:u32}
    #[derive(Debug)] pub enum PageError {RowFailed(usize)}
    pub type PageResult=Result<(),PageError>;
    pub trait Page {
        fn fill_title(&self,p:*mut u8);
        fn render(&self,c:&PageCtx)->PageResult;
        fn on_click(&self,i:usize);
    }
    pub unsafe fn put_str(p:*mut u8,s:&[u8]){core::ptr::copy_nonoverlapping(s.as_ptr(),p,s.len());}
    pub unsafe fn cstr_len(p:*const u8)->usize{std::ffi::CStr::from_ptr(p.cast()).to_bytes().len()}
    pub fn render_req(pid:u32){assert_eq!(pid,17);}
    pub unsafe fn build_tile_below(_: &PageCtx,_:u32,i:usize,_:&[u8],_:u32,_:u32)->u32{10+i as u32}
    pub unsafe fn build_tile_right(_: &PageCtx,_:u32,i:usize,_:&[u8],_:u32,_:u32)->u32{10+i as u32}
    pub mod kit {
        pub use crate::confirm;
        pub mod scroll {
            pub unsafe fn viewport(_:u32,anchor:u32,_:i32,h:i32,horizontal:bool)->u32{
                assert_eq!(anchor,0);assert_eq!(h,226);assert!(!horizontal);1
            }
            pub unsafe fn board(_:u32,_:i32,h:i32)->u32{crate::with(|u|u.board=h);2}
        }
    }
}
#[path = "../src/page/kit/confirm.rs"] pub mod confirm;
#[path = "../src/page/connect_log.rs"] mod log_page;
use page::Page;
fn setup()->page::PageCtx {
    *UI.lock().unwrap()=Some(Ui{log:connect_log_buffer::Records::new(),text:String::new(),height:0,board:0,clears:0});
    log_page::PAGE.on_click(0);
    page::PageCtx{content:1,title:2}
}
#[test]
fn long_records_remain_scrollable_to_the_final_line(){
    let ctx=setup();
    with(|u|for i in 0..12 {u.log.push(format!("req={} 打开文件 /data/{} 末尾标记={} 成功",i,"W".repeat(700),i).as_bytes());});
    log_page::PAGE.render(&ctx).unwrap();
    with(|u|{
        assert!(u.text.contains("末尾标记=0"));
        assert!(u.height>226&&u.height<32767);
        assert!(u.height>=u.text.lines().count() as u32*36);
        assert!(u.board>=u.height as i32);
    });
}
#[test]
fn can_read_older_records_and_clear_requires_two_clicks(){
    let ctx=setup();
    with(|u|for i in 0..16 {u.log.push(format!("entry={i:02} /data/说明.txt 成功").as_bytes());});
    log_page::PAGE.render(&ctx).unwrap();
    with(|u|{assert!(u.text.contains("entry=15"));assert!(!u.text.contains("entry=03"));});
    log_page::PAGE.on_click(2);log_page::PAGE.render(&ctx).unwrap();
    with(|u|{assert!(u.text.contains("entry=00"));assert!(!u.text.contains("entry=15"));});
    log_page::PAGE.on_click(3);log_page::PAGE.render(&ctx).unwrap();
    with(|u|assert!(u.text.contains("entry=15")));
    log_page::PAGE.on_click(1);log_page::PAGE.render(&ctx).unwrap();
    with(|u|assert_eq!(u.clears,0));
    log_page::PAGE.on_click(1);log_page::PAGE.render(&ctx).unwrap();
    with(|u|assert_eq!(u.clears,1));
    log_page::PAGE.on_click(0);log_page::PAGE.render(&ctx).unwrap();
    with(|u|assert!(u.text.contains("暂无操作日志")));
}
