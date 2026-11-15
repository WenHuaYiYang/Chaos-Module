//! Production local controller + production C filesystem adapter seams on real POSIX files.
#![allow(dead_code,static_mut_refs)]
use std::path::PathBuf;
mod keyboard{
 pub enum Completion{FileName}
 pub unsafe fn open(_: *const u8,_:Completion){}
}
mod page{pub enum Pid{Files=50,FileDetail=51}}
pub fn render_req(_:u32){}
pub struct W{p:*mut u8,cap:usize,n:usize}
impl W{
 pub fn new(p:*mut u8,cap:usize)->Self{Self{p,cap,n:0}}
 pub unsafe fn c(&mut self,c:u8)->&mut Self{if self.n+1<self.cap{*self.p.add(self.n)=c;self.n+=1;}self}
 pub unsafe fn s(&mut self,s:&[u8])->&mut Self{for &c in s{self.c(c);}self}
 pub unsafe fn n(&mut self,n:u32)->&mut Self{self.s(n.to_string().as_bytes())}
 pub unsafe fn end(&mut self){*self.p.add(self.n)=0;}
}
#[path="../src/file_manager.rs"]mod manager;
fn main(){unsafe{
 let root=PathBuf::from(std::env::var_os("CHAOS_CONNECT_TEST_ROOT").unwrap());
 std::fs::create_dir_all(root.join("data")).unwrap();
 for i in 0..19{std::fs::write(root.join(format!("data/item{i}.txt")),format!("hello-{i}")).unwrap();}
 manager::load();assert_eq!((manager::TOTAL,manager::COUNT),(19,8));
 let mut seen=std::collections::BTreeSet::new();
 for page in 0..3{manager::OFFSET=page*8;manager::load();for e in &manager::ENTRIES[..manager::COUNT]{seen.insert(e.name);}}
 assert_eq!(seen.len(),19);
 manager::OFFSET=999;manager::load();assert_eq!(manager::OFFSET,16);
 manager::mkdir();assert!(manager::save_name("新目录\0".as_ptr()));assert!(root.join("data/新目录").is_dir());
 manager::mkdir();assert!(!manager::save_name(b"../escape\0".as_ptr()));
 assert!(!manager::save_name("新目录\0".as_ptr()));
 manager::DIR=manager::path::join(b"/data","新目录".as_bytes()).unwrap();manager::load();assert_eq!(manager::COUNT,0);
 manager::current();assert!(manager::remove());assert!(!root.join("data/新目录").exists());
 manager::DIR=[0;256];manager::DIR[..6].copy_from_slice(b"/data\0");manager::OFFSET=0;manager::load();
 let i=manager::ENTRIES[..manager::COUNT].iter().position(|e|!e.directory).unwrap();
 assert!(manager::select(i));manager::preview();
 let s=std::ffi::CStr::from_ptr(manager::PREVIEW.as_ptr().cast()).to_str().unwrap();assert!(s.contains("hello-"));
 manager::rename();assert!(manager::save_name(b"renamed.txt\0".as_ptr()));assert!(root.join("data/renamed.txt").is_file());
 assert!(manager::remove());assert!(!root.join("data/renamed.txt").exists());
 manager::current();assert!(!manager::remove()); // /data is nonempty: never recursive
 manager::up();assert_eq!(&manager::DIR[..2],b"/\0");manager::current();assert!(!manager::remove());
 std::fs::write(root.join("data/binary"),[0xff,0x80,1,2]).unwrap();
 manager::TARGET=[0;256];manager::TARGET[..13].copy_from_slice(b"/data/binary\0");manager::INFO.directory=0;manager::INFO.size=4;manager::preview();
 assert!(std::ffi::CStr::from_ptr(manager::PREVIEW.as_ptr().cast()).to_str().unwrap().contains("FF 80"));
 std::fs::write(root.join("data/chinese"),"字".repeat(1000)).unwrap();
 manager::TARGET=[0;256];manager::TARGET[..14].copy_from_slice(b"/data/chinese\0");manager::INFO.size=3000;manager::preview();
 let preview=std::ffi::CStr::from_ptr(manager::PREVIEW.as_ptr().cast()).to_str().unwrap();assert!(preview.contains("字")&&!preview.contains("十六进制"));
 manager::TARGET=[0;256];manager::TARGET[..10].copy_from_slice(b"/dev/tty0\0");manager::preview();assert_eq!(manager::PREVIEW[0],0);
 println!("PASS local files: paging, UTF-8 mkdir, traversal/conflict rejection, preview, rename, delete, nonrecursive directories, root/device protection");
}}
