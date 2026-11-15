//! Compile the production persistent parameter module with short-I/O seams.
#![allow(dead_code, static_mut_refs)]
use std::cell::RefCell;
#[path="../src/blur_params.rs"] mod blur_params;
#[derive(Default)] struct Io {
    file: Vec<u8>, pos: usize, short: bool, open_fail: bool,
    fail_after: Option<usize>, close_fail: bool, opens: usize, closes: usize,
    busy: bool, calls: usize, applied: Vec<u32>,
}
thread_local! {static IO:RefCell<Io>=RefCell::new(Io::default());}
fn io<T>(f:impl FnOnce(&mut Io)->T)->T {IO.with(|s|f(&mut s.borrow_mut()))}
mod fw_api {
    pub unsafe fn screen_is_on()->bool {true}
    pub mod oflag {pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=256;}
    pub unsafe fn open(p:*const u8,flags:u32,mode:u32)->i32 {
        assert_eq!(std::ffi::CStr::from_ptr(p.cast()).to_bytes(),b"/data/chaos/blur_v2_params.cfg");
        io(|s| {s.opens+=1;if s.open_fail{return -1;}s.pos=0;
            if flags==oflag::RDONLY {assert_eq!(mode,0);3}
            else {assert_eq!(flags,258);assert_eq!(mode,0o640);4}})
    }
    pub unsafe fn read(fd:i32,p:*mut u8,len:u32)->i32 {
        assert_eq!(fd,3);io(|s| {let n=(len as usize).min(s.file.len()-s.pos).min(if s.short {2}else{usize::MAX});
            std::ptr::copy_nonoverlapping(s.file.as_ptr().add(s.pos),p,n);s.pos+=n;n as i32})
    }
    pub unsafe fn write(fd:i32,p:*const u8,len:u32)->i32 {
        assert_eq!(fd,4);io(|s| {if s.fail_after.is_some_and(|n|s.pos>=n){return -1;}
            let n=(len as usize).min(if s.short {2}else{usize::MAX});
            if s.file.len()<s.pos+n {s.file.resize(s.pos+n,0);}
            s.file[s.pos..s.pos+n].copy_from_slice(std::slice::from_raw_parts(p,n));s.pos+=n;n as i32})
    }
    pub unsafe fn close(_:i32)->i32 {io(|s| {s.closes+=1;if s.close_fail {-1}else{0}})}
    use super::io;
}
#[no_mangle] unsafe extern "C" fn chaos_blur_v2_set_params(p:*const u32,count:u32)->i32 {
    assert_eq!(count,6);io(|s| {s.calls+=1;if s.busy{return -2145;}
        s.applied=std::slice::from_raw_parts(p,6).to_vec();0})
}
fn fixture(values:[u16;6])->Vec<u8> {
    let mut b=b"BVP1".to_vec();for v in values {b.extend(v.to_le_bytes());}
    b.push(b.iter().fold(0xa5,|a,b|a^b));b
}
fn values()->[u32;6] {unsafe{std::array::from_fn(|i|blur_params::get(i))}}
fn main(){unsafe{
    let args:Vec<String>=std::env::args().collect();let case=&args[1];
    match case.as_str() {
        "default" => {assert_eq!(values(),blur_params::DEFAULTS);blur_params::apply();blur_params::apply();io(|s|assert_eq!(s.calls,1));},
        "load" => {io(|s|{s.file=(0..args[2].len()).step_by(2).map(|i|u8::from_str_radix(&args[2][i..i+2],16).unwrap()).collect();s.short=true;});
            assert_eq!(values(),[40,70,44,75,38,500]);},
        "valid" => {io(|s|{s.file=fixture([1,10,0,100,0,2000]);s.short=true;});assert_eq!(values(),[1,10,0,100,0,2000]);},
        "magic"|"checksum"|"range"|"truncated" => {
            let mut b=fixture([12,70,44,75,38,500]);
            match case.as_str(){"magic"=>b[0]=0,"checksum"=>b[16]^=1,"range"=>b=fixture([41,70,44,75,38,500]),_=>{b.pop();}}
            io(|s|s.file=b);assert_eq!(values(),blur_params::DEFAULTS);
        },
        "busy" => {
            io(|s|s.busy=true);assert!(blur_params::set(0,30));blur_params::apply();io(|s|assert!(s.applied.is_empty()));
            io(|s|s.busy=false);blur_params::apply();io(|s|assert_eq!(s.applied[0],30));
            blur_params::apply();io(|s|assert_eq!(s.calls,2));
        },
        "persist"|"writefail"|"closefail"|"openfail" => {
            assert_eq!(values(),blur_params::DEFAULTS);io(|s|s.short=true);
            if case=="writefail"{io(|s|s.fail_after=Some(6));}
            if case=="closefail"{io(|s|s.close_fail=true);}
            if case=="openfail"{io(|s|s.open_fail=true);}
            assert_eq!(blur_params::set(0,40),case=="persist");
            assert_eq!(blur_params::save_failed(),case!="persist");
            io(|s|{s.fail_after=None;s.open_fail=false;s.close_fail=false;});
            assert!(blur_params::set(0,40));assert!(!blur_params::save_failed());
            let file=io(|s|s.file.clone());assert_eq!(file,fixture([40,70,44,75,38,500]));
            let hex:String=file.iter().map(|b|format!("{b:02x}")).collect();
            assert!(std::process::Command::new(&args[0]).args(["load",&hex]).status().unwrap().success());
            assert!(blur_params::reset());assert_eq!(values(),blur_params::DEFAULTS);
            assert_eq!(io(|s|s.file.clone()),fixture([12,70,44,75,38,500]));
        },
        "bounds" => {
            for i in 0..6 {let before=values();assert!(!blur_params::set(i,blur_params::MAX[i]+1));assert_eq!(values(),before);
                if blur_params::MIN[i]>0{assert!(!blur_params::set(i,blur_params::MIN[i]-1));}
                assert!(blur_params::set(i,blur_params::MIN[i]));assert!(blur_params::set(i,blur_params::MAX[i]));}
            assert!(!blur_params::set(6,0));assert_eq!(blur_params::get(6),0);
        },
        _=>panic!("unknown scenario"),
    }
    io(|s|{if case!="openfail"{assert_eq!(s.opens,s.closes);}});
    println!("V2 parameters {case}: passed");
}}
