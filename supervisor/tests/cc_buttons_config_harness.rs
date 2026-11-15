#![allow(dead_code,static_mut_refs)]
use std::cell::RefCell;
use std::collections::BTreeMap;
#[derive(Default)] struct State { files:BTreeMap<String,u8>,fds:BTreeMap<i32,String>,writes:u32,fail:bool }
thread_local!{static STATE:RefCell<State>=RefCell::new(State::default());}
fn state<T>(f:impl FnOnce(&mut State)->T)->T{STATE.with(|s|f(&mut s.borrow_mut()))}
#[macro_export]macro_rules! st_rd{($v:expr)=>{core::ptr::read_volatile(core::ptr::addr_of!($v))};}
#[macro_export]macro_rules! st_wr{($v:expr,$n:expr)=>{core::ptr::write_volatile(core::ptr::addr_of_mut!($v),$n)};}
mod fw_api {
 pub mod oflag{pub const RDONLY:u32=0;pub const WRONLY:u32=1;pub const CREAT:u32=2;}
 pub unsafe fn open(p:*const u8,flags:u32,_:u32)->i32 {
  let p=std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().to_string();
  super::state(|s|{if flags==0&&!s.files.contains_key(&p){return -1;}let fd=s.fds.len()as i32+3;s.fds.insert(fd,p);fd})
 }
 pub unsafe fn read(fd:i32,p:*mut u8,_:u32)->i32{super::state(|s|{*p=s.files[&s.fds[&fd]];1})}
 pub unsafe fn write(fd:i32,p:*const u8,_:u32)->i32{super::state(|s|{s.writes+=1;if s.fail{return -1;}s.files.insert(s.fds[&fd].clone(),*p);1})}
 pub unsafe fn close(fd:i32){super::state(|s|{s.fds.remove(&fd);});}
}
#[path="../src/cc_buttons_config.rs"]mod cc_buttons_config;
#[path="../src/blur_zone.rs"]mod blur_zone;
fn main(){unsafe{
 let mode=std::env::args().nth(1).unwrap();let key="/data/chaos/blur_cc_buttons.cfg";
 if mode=="off"{state(|s|{s.files.insert(key.into(),0);});}
 if mode=="invalid"{state(|s|{s.files.insert(key.into(),99);});}
 state(|s|{s.files.insert("/data/chaos/blur_regions.cfg".into(),5);});
 assert_eq!(blur_zone::mask(),5);
 assert_eq!(blur_zone::on(4),mode!="off");assert_eq!(blur_zone::REGION_N,5);
 assert!(!blur_zone::on(5)&&!blur_zone::set(5,true));
 let next=mode=="off";
 if mode=="retry"{state(|s|s.fail=true);}
 assert!(blur_zone::set(4,next));assert_eq!(blur_zone::on(4),next);
 if mode=="retry"{state(|s|s.fail=false);assert!(!blur_zone::set(4,next));}
 state(|s|{assert_eq!(s.files[key],next as u8);assert_eq!(s.files["/data/chaos/blur_regions.cfg"],5);assert!(s.fds.is_empty());});
 assert_eq!(blur_zone::mask(),5);
 assert!(blur_zone::set(0,false));assert!(!blur_zone::on(0));assert_eq!(blur_zone::on(4),next);
 println!("CC region config: {mode} passed (independent legacy mask, persistent toggle and retry)");
}}
