//! Production policy/mailbox/prompt with explicit host VFS and LVGL seams.
#![allow(dead_code,static_mut_refs)]
use std::sync::Mutex;
#[derive(Default)]
struct Fixture{config:Option<Vec<u8>>,temp:Vec<u8>,android:Option<Vec<u8>>,android_temp:Vec<u8>,fault:u8,on:bool,shown:bool,other:bool,wakes:u32,vibrations:u32,creates:u32,logs:Vec<String>}
static FIX:Mutex<Option<Fixture>>=Mutex::new(None);
fn with<T>(f:impl FnOnce(&mut Fixture)->T)->T{f(FIX.lock().unwrap().as_mut().unwrap())}
const TOP:u32=0x20051000;const MODAL:u32=0x20052000;const OTHER:u32=0x20053000;
mod connect_log{pub unsafe fn line(s:&str){super::with(|f|f.logs.push(s.into()));}}
mod mem{
    pub fn safe_ptr(p:u32)->bool{p>=0x20000000&&p<0x20200000&&p&3==0}
    pub unsafe fn rd32(_: *const u32)->u32{super::with(|f|if f.other{super::OTHER}else{0})}
}
mod fw_api{
    pub mod oflag{pub const RDONLY:u32=1;pub const WRONLY:u32=2;pub const CREAT:u32=4;}
    pub mod ev{pub const CLICKED:u32=7;}
    pub const MSGBOX_SINGLETON:u32=0x200ced8c;
    pub fn capture_now_ms()->u32{100}
    pub unsafe fn open(path: *const u8,flags:u32,_:u32)->i32{
        let android=std::ffi::CStr::from_ptr(path.cast()).to_bytes().windows(7).any(|s|s==b"android");
        super::with(|f|if android {if flags&2!=0{f.android_temp.clear();4}else if f.android.is_some(){3}else{-1}}
            else if flags&2!=0{f.temp.clear();2}else if f.config.is_some(){1}else{-1})
    }
    pub unsafe fn read(fd:i32,p:*mut u8,n:u32)->i32{super::with(|f|{
        let b=if fd==3{f.android.as_ref().unwrap()}else{f.config.as_ref().unwrap()};let len=b.len().min(n as usize);core::ptr::copy_nonoverlapping(b.as_ptr(),p,len);len as i32
    })}
    pub unsafe fn write(fd:i32,p:*const u8,n:u32)->i32{super::with(|f|{
        let len=if f.fault==1{2}else{n as usize};if fd==4{f.android_temp.extend_from_slice(core::slice::from_raw_parts(p,len));}else{f.temp.extend_from_slice(core::slice::from_raw_parts(p,len));}len as i32
    })}
    pub unsafe fn close(_:i32)->i32{super::with(|f|if f.fault==3{-1}else{0})}
    pub unsafe fn capture_rename(path: *const u8,_: *const u8)->i32{let android=std::ffi::CStr::from_ptr(path.cast()).to_bytes().windows(7).any(|s|s==b"android");super::with(|f|{
        if f.fault==2{return -1;}if android{f.android=Some(f.android_temp.clone());f.android_temp.clear();}else{f.config=Some(f.temp.clone());f.temp.clear();}0
    })}
    pub unsafe fn fs_remove(_: *const u8)->i32{super::with(|f|f.temp.clear());0}
    pub unsafe fn screen_wake_for_connect(){super::with(|f|{f.on=true;f.wakes+=1;});}
    pub unsafe fn vibrate(kind:u32){assert_eq!(kind,13);super::with(|f|f.vibrations+=1);}
    pub unsafe fn screen_is_on()->bool{super::with(|f|f.on)}
    pub unsafe fn display_default()->u32{0x20050000}
    pub unsafe fn display_top_layer(_:u32)->u32{super::TOP}
    pub unsafe fn obj_is_child_of(parent:u32,obj:u32)->bool{super::with(|f|parent==super::TOP&&((obj==super::MODAL&&f.shown)||(obj==super::OTHER&&f.other)))}
    pub unsafe fn obj_delete(obj:u32){assert_eq!(obj,super::MODAL);super::with(|f|f.shown=false);}
    pub unsafe fn event_get_code(event:u32)->u32{event}
    pub unsafe fn msgbox_create(parent:u32,mode:u32)->u32{assert_eq!((parent,mode),(super::TOP,0));super::with(|f|{f.shown=true;f.creates+=1;});super::MODAL}
    pub unsafe fn msgbox_fill(_:u32,kind:u32,_:u32,text:*const u8,_:u32,buttons:*const u32,n:u32){
        assert_eq!((kind,n),(5,2));assert!(std::ffi::CStr::from_ptr(text.cast()).to_str().unwrap().contains("允许"));
        assert_eq!(*buttons&1,1);assert_eq!(*buttons.add(5)&1,1);assert_ne!(*buttons,*buttons.add(5));
    }
    pub unsafe fn obj_add_event(_:u32,_:u32,code:u32,_:u32)->u32{assert_eq!(code,36);1}
}
#[path="../src/connect_permission.rs"] mod connect_permission;
#[path="../src/connect_prompt.rs"] mod connect_prompt;
fn main(){
    let case=std::env::args().nth(1).unwrap();
    *FIX.lock().unwrap()=Some(Fixture{on:true,..Fixture::default()});
    match case.as_str(){
        "default"=>{unsafe{connect_permission::load();}assert_eq!(connect_permission::policy(),2);}
        "invalid"=>{with(|f|f.config=Some(b"CCP1\x03".to_vec()));unsafe{connect_permission::load();}assert_eq!(connect_permission::policy(),2);}
        "persist"=>{
            with(|f|f.config=Some(b"CCP1\x00".to_vec()));unsafe{connect_permission::load();}assert_eq!(connect_permission::policy(),0);
            unsafe{connect_permission::set_policy(1);}assert_eq!(connect_permission::policy(),1);with(|f|assert_eq!(f.config.as_deref(),Some(b"CCP1\x01".as_slice())));
            for fault in [1,2]{with(|f|f.fault=fault);unsafe{connect_permission::set_policy(0);}assert_eq!(connect_permission::policy(),1);with(|f|assert_eq!(f.config.as_deref(),Some(b"CCP1\x01".as_slice())));}
        }
        "android"=>unsafe{
            connect_permission::load();assert_eq!((connect_permission::android_policy(),connect_permission::android_mask()),(2,1));
            connect_permission::set_android(0,31);assert_eq!((connect_permission::android_policy(),connect_permission::android_mask()),(0,31));
            with(|f|assert_eq!(f.android.as_deref(),Some(b"CCA1\x00\x1f\xba".as_slice())));
            for fault in [1,2,3]{with(|f|f.fault=fault);connect_permission::set_android(1,0);assert_eq!((connect_permission::android_policy(),connect_permission::android_mask()),(0,31));}
            with(|f|f.fault=0);connect_permission::set_android(4,31);connect_permission::set_android(0,32);assert_eq!(connect_permission::android_mask(),31);
            connect_permission::chaos_connect_state(1|(1<<5)|(1<<6),44);assert!(connect_permission::prompt_is_connection());assert_eq!(connect_permission::pending(),44);
            with(|f|f.on=false);connect_prompt::tick();with(|f|assert_eq!((f.wakes,f.vibrations),(1,1)));
            connect_prompt::choose(7,true);assert_eq!(connect_permission::chaos_connect_take_decision(44),1);
        },
        "android-load"=>unsafe{
            with(|f|f.android=Some(b"CCA1\x01\x1f\xbb".to_vec()));connect_permission::load();
            assert_eq!((connect_permission::android_policy(),connect_permission::android_mask()),(1,31));
        },
        "android-invalid"=>unsafe{
            with(|f|f.android=Some(b"CCA1\x00\x1f\x00".to_vec()));connect_permission::load();assert_eq!((connect_permission::android_policy(),connect_permission::android_mask()),(2,1));
        },
        "mailbox"=>{
            connect_permission::chaos_connect_state(1|4,11);
            connect_permission::decide(10,true);assert_eq!(connect_permission::chaos_connect_take_decision(11),0);
            connect_permission::decide(11,true);assert_eq!(connect_permission::chaos_connect_take_decision(11),1);assert_eq!(connect_permission::chaos_connect_take_decision(11),0);
            connect_permission::chaos_connect_state(1|4,12);connect_permission::decide(11,true);assert_eq!(connect_permission::chaos_connect_take_decision(12),0);
            connect_permission::decide(12,false);assert_eq!(connect_permission::chaos_connect_take_decision(12),2);
            connect_permission::chaos_connect_state(0,0);connect_permission::decide(12,true);assert_eq!(connect_permission::chaos_connect_take_decision(12),0);
        }
        "prompt"=>unsafe{
            connect_permission::chaos_connect_state(1|4,19);
            with(|f|{f.on=false;f.other=true;});connect_prompt::tick();with(|f|{assert!(!f.shown);assert_eq!((f.wakes,f.vibrations),(1,1));});
            with(|f|{f.on=true;f.other=true;});connect_prompt::tick();with(|f|{assert_eq!(f.creates,0);assert_eq!((f.wakes,f.vibrations),(1,1));});
            with(|f|f.other=false);connect_prompt::tick();connect_prompt::tick();with(|f|assert_eq!(f.creates,1));
            connect_prompt::choose(6,true);assert_eq!(connect_permission::chaos_connect_take_decision(19),0);
            connect_prompt::choose(7,true);assert_eq!(connect_permission::chaos_connect_take_decision(19),1);with(|f|assert!(!f.shown));
            connect_permission::chaos_connect_state(1|4,20);connect_prompt::tick();connect_prompt::choose(7,false);assert_eq!(connect_permission::chaos_connect_take_decision(20),2);
            connect_permission::chaos_connect_state(1|4,21);connect_prompt::tick();connect_permission::chaos_connect_state(1|4,22);
            connect_prompt::choose(7,true);assert_eq!(connect_permission::chaos_connect_take_decision(22),0);connect_prompt::tick();
            connect_permission::chaos_connect_state(0,0);connect_prompt::tick();with(|f|assert!(!f.shown));
        },
        _=>panic!("unknown case"),
    }
    println!("PASS permission {case}");
}
