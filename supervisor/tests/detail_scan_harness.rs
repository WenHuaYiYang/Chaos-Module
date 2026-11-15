#![allow(dead_code,static_mut_refs)]
use std::collections::HashMap;
static mut D_WORK:[u8;300]=[0;300];
static mut D_PKG:[u8;44]=[0;44];
unsafe fn detail_valid()->bool { true }
mod state { pub const DT_REG:u8=1; }
mod mem { pub unsafe fn rd8(p:*const u8)->u8 { p.read() } }
#[derive(Default)]
struct Vfs {
    dirs:HashMap<String,Vec<(u8,String)>>,
    files:HashMap<String,u32>,
    opened:HashMap<u32,(Vec<(u8,String)>,usize)>,
    next:u32, entry:Vec<u8>, reads:usize, now:u32,
}
static mut VFS:Option<Vfs>=None;
mod fw_api {
    use super::*;
    pub mod oflag { pub const RDONLY:u32=0; }
    pub mod seek { pub const END:u32=2; }
    unsafe fn path(p:*const u8)->String { std::ffi::CStr::from_ptr(p.cast()).to_str().unwrap().to_owned() }
    pub unsafe fn capture_now_ms()->u32 { VFS.as_ref().unwrap().now }
    pub unsafe fn opendir(p:*const u8)->u32 {
        let path=path(p);let v=VFS.as_mut().unwrap();
        let Some(entries)=v.dirs.get(&path).cloned() else{return 0};
        v.next+=1;v.opened.insert(v.next,(entries,0));v.next
    }
    pub unsafe fn closedir(id:u32) { assert!(VFS.as_mut().unwrap().opened.remove(&id).is_some()); }
    // Host pointers are usize; only this VFS seam differs from the 32-bit ABI.
    pub unsafe fn readdir(id:u32)->usize {
        let v=VFS.as_mut().unwrap();v.reads+=1;
        let (entries,at)=v.opened.get_mut(&id).unwrap();
        if *at==entries.len(){return 0;}
        let (kind,name)=&entries[*at];*at+=1;
        v.entry.clear();v.entry.push(*kind);v.entry.extend_from_slice(name.as_bytes());v.entry.push(0);
        v.entry.as_ptr() as usize
    }
    pub unsafe fn open(p:*const u8,_:u32,_:u32)->i32 { if VFS.as_ref().unwrap().files.contains_key(&path(p)){99}else{-1} }
    pub unsafe fn close(_:i32) {}
    pub unsafe fn lseek(_:i32,_:i32,_:u32)->i32 {
        let path=path(core::ptr::addr_of!(SIZE_PATH).cast());
        *VFS.as_ref().unwrap().files.get(&path).unwrap() as i32
    }
}
include!(env!("CHAOS_SCAN_SOURCE"));
unsafe fn reset() {
    if VFS.is_some(){detail_scan_cancel();}
    VFS=Some(Vfs::default());D_SIZE=0;SIZE_ROOT=0;SIZE_DEPTH=0;SIZE_COMPLETE=false;SIZE_PENDING=true;
    D_PKG=[0;44];D_PKG[..5].copy_from_slice(b"pkg.x");
}
fn main() { unsafe {
    reset();
    let v=VFS.as_mut().unwrap();
    let mut entries=vec![(0,".".into()),(0,"..".into()),(0,"sub".into())];
    for i in 0..100 {entries.push((1,format!("f{i}")));v.files.insert(format!("/data/app/pkg.x/f{i}"),10);}
    v.dirs.insert("/data/app/pkg.x".into(),entries);
    v.dirs.insert("/data/app/pkg.x/sub".into(),vec![(1,"nested".into())]);
    v.files.insert("/data/app/pkg.x/sub/nested".into(),123);
    let mut passes=0;
    while detail_scan_pending() {
        let before=VFS.as_ref().unwrap().reads;
        detail_scan_step();passes+=1;
        assert!(VFS.as_ref().unwrap().reads-before<=24);
        assert!(passes<100);
    }
    assert!(passes>1);assert_eq!(D_SIZE,1123);assert!(SIZE_COMPLETE);
    assert!(VFS.as_ref().unwrap().opened.is_empty());
    reset();VFS.as_mut().unwrap().dirs.insert("/data/app/pkg.x".into(),vec![(1,"file".into());100]);
    VFS.as_mut().unwrap().files.insert("/data/app/pkg.x/file".into(),1);
    detail_scan_step();assert!(!VFS.as_ref().unwrap().opened.is_empty());
    detail_scan_cancel();assert!(!detail_scan_pending());assert!(VFS.as_ref().unwrap().opened.is_empty());
    detail_scan_resume();assert!(detail_scan_pending());assert_eq!(D_SIZE,0);
    while detail_scan_pending(){detail_scan_step();}
    assert_eq!(D_SIZE,100);
    reset();while detail_scan_pending(){detail_scan_step();}assert_eq!(D_SIZE,0);
    println!("PASS production detail scan: nested/missing dirs, 24-op budget, cancel/resume, exact sum and closed handles");
} }
