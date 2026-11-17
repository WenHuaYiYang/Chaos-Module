#![allow(dead_code)]
#![allow(static_mut_refs)]
use std::{cell::RefCell, collections::{HashMap, VecDeque}, ffi::CStr};

#[macro_export]
macro_rules! st_rd { ($v:expr) => { core::ptr::read_volatile(core::ptr::addr_of!($v)) }; }
mod state { pub static mut SIM_LITE: u32 = 0; }
mod screen_rotation {
    pub unsafe fn enabled() -> bool { crate::MOCK.with(|m| m.borrow().flipped) }
    pub unsafe fn ensure_applied() {}
}

#[derive(Default)]
struct Mock {
    files: HashMap<String, Vec<u8>>,
    descriptors: HashMap<i32, (String, usize)>,
    next_fd: i32,
    now: u32,
    awake: bool,
    flipped: bool,
    physical: bool,
    page: [u32; 4],
    sensor_failed: bool,
    rename_failed: bool,
    write_failed: bool,
    write_limit: usize,
    packets: VecDeque<Vec<u8>>,
    creates: u32,
    deletes: u32,
    resets: u32,
    sensor_opens: u32,
    sensor_closes: u32,
    sensor_reads: u32,
    overlay_creates: u32,
    overlay_deletes: u32,
    overlay_size: (u32,u32),
    visible: bool,
    period: u32,
    cursor: (i32, i32, u32, bool),
}
thread_local! { static MOCK: RefCell<Mock> = RefCell::new(Mock::default()); }
unsafe fn path(p: *const u8) -> String { CStr::from_ptr(p.cast()).to_str().unwrap().to_owned() }

mod fw_api {
    use super::*;
    pub mod oflag { pub const RDONLY: u32 = 1; pub const WRONLY: u32 = 2; pub const CREAT: u32 = 4; }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    pub struct GyroInputData {
        pub x:i32, pub y:i32, pub key:u32, pub button:u32,
        pub encoder:i16, pub state:u8, pub continue_reading:u8,
    }
    #[derive(Clone, Copy)]
    pub struct GyroOverlay { pub root:u32, pub ring:u32, pub dot:u32, pub radius:i32 }
    pub unsafe fn open(p:*const u8, flags:u32, _:u32)->i32 {
        let p=path(p);
        MOCK.with(|m| { let mut m=m.borrow_mut();
            if flags & oflag::WRONLY != 0 {
                if flags & 0x20 != 0 { m.files.insert(p.clone(), Vec::new()); }
                else { m.files.entry(p.clone()).or_default(); }
            } else if !m.files.contains_key(&p) { return -1; }
            m.next_fd+=1; let fd=m.next_fd+10; m.descriptors.insert(fd,(p,0)); fd
        })
    }
    pub unsafe fn read(fd:i32, out:*mut u8, len:u32)->i32 {
        MOCK.with(|m| { let mut m=m.borrow_mut();
            if fd==99 {
                m.sensor_reads+=1;
                if let Some(packet)=m.packets.pop_front() {
                    let n=packet.len().min(len as usize);
                    std::ptr::copy_nonoverlapping(packet.as_ptr(),out,n); return n as i32;
                }
                return -11;
            }
            let Some((p,pos))=m.descriptors.get(&fd).cloned() else {return -1;};
            let file=&m.files[&p]; let n=(len as usize).min(file.len()-pos);
            std::ptr::copy_nonoverlapping(file.as_ptr().add(pos),out,n);
            m.descriptors.get_mut(&fd).unwrap().1+=n; n as i32
        })
    }
    pub unsafe fn write(fd:i32, data:*const u8, len:u32)->i32 {
        MOCK.with(|m| { let mut m=m.borrow_mut();
            if m.write_failed {return -1;}
            let Some((p,pos))=m.descriptors.get(&fd).cloned() else {return -1;};
            let n=if m.write_limit==0 {len as usize} else {(len as usize).min(m.write_limit)};
            let bytes=std::slice::from_raw_parts(data,n); let file=m.files.get_mut(&p).unwrap();
            file.resize((pos+n).max(file.len()),0); file[pos..pos+n].copy_from_slice(bytes);
            m.descriptors.get_mut(&fd).unwrap().1+=n; n as i32
        })
    }
    pub unsafe fn close(fd:i32)->i32 {MOCK.with(|m| {let mut m=m.borrow_mut(); if fd==99 {m.sensor_closes+=1;} else {m.descriptors.remove(&fd);} 0})}
    pub unsafe fn fs_remove(p:*const u8)->i32 {MOCK.with(|m| {m.borrow_mut().files.remove(&path(p)); 0})}
    pub unsafe fn capture_rename(old:*const u8,new:*const u8)->i32 {
        MOCK.with(|m| {let mut m=m.borrow_mut();if m.rename_failed{return -1;}
            let b=m.files.remove(&path(old)).unwrap();m.files.insert(path(new),b);0})
    }
    pub fn capture_now_ms()->u32 {MOCK.with(|m|m.borrow().now)}
    pub unsafe fn screen_is_on()->bool {MOCK.with(|m|m.borrow().awake)}
    pub unsafe fn capture_result(n:i32)->i32 {n}
    pub unsafe fn gyro_input_create(_:u32)->u32 {MOCK.with(|m|{m.borrow_mut().creates+=1;});200}
    pub unsafe fn gyro_input_reset(input:u32) {if input!=0 {MOCK.with(|m|{m.borrow_mut().resets+=1;});}}
    pub unsafe fn gyro_input_delete(input:u32) {if input!=0 {MOCK.with(|m|{m.borrow_mut().deletes+=1;});}}
    pub unsafe fn gyro_input_period(_:u32,period:u32) {MOCK.with(|m|m.borrow_mut().period=period);}
    pub unsafe fn gyro_physical_touch(_:u32)->bool {MOCK.with(|m|m.borrow().physical)}
    pub unsafe fn gyro_page_identity()->[u32; 4] {MOCK.with(|m|m.borrow().page)}
    pub unsafe fn gyro_sensor_open()->i32 {MOCK.with(|m| {let mut m=m.borrow_mut();m.sensor_opens+=1;if m.sensor_failed{-1}else{99}})}
    pub unsafe fn gyro_cursor_create(_:u32,size:u32,dot_size:u32)->Option<GyroOverlay> {
        MOCK.with(|m|{let mut m=m.borrow_mut();m.overlay_creates+=1;m.overlay_size=(size,dot_size);});
        Some(GyroOverlay{root:100,ring:101,dot:102,radius:(size/2) as i32})
    }
    pub unsafe fn gyro_cursor_show(_:GyroOverlay,x:i32,y:i32,progress:u32,calibrated:bool) {
        MOCK.with(|m|{let mut m=m.borrow_mut();m.visible=true;m.cursor=(x,y,progress,calibrated);});
    }
    pub unsafe fn gyro_cursor_hide(_:GyroOverlay) {MOCK.with(|m|m.borrow_mut().visible=false);}
    pub unsafe fn obj_delete(_:u32) {MOCK.with(|m|{let mut m=m.borrow_mut();m.overlay_deletes+=1;m.visible=false;});}
}

#[path = "../src/gyro_pointer.rs"]
mod gyro_pointer;
