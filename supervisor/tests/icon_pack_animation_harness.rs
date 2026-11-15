//! Production streamed icon-pack player with a low-address host VFS/image fixture.
#![allow(dead_code,static_mut_refs)]
#[path="../src/mem.rs"] mod mem;
#[path="../src/dynamic_icon_frames.rs"] mod dynamic_icon_frames;
#[path="../src/icon_pack_animation.rs"] mod animation;
use std::{collections::BTreeMap,sync::Mutex};
#[derive(Clone,Copy)]
struct Icon { object:u32,source:u32,original:bool,visible:bool }
struct File { name:Vec<u8>,offset:usize }
struct Ui {
    files:BTreeMap<Vec<u8>,Vec<u8>>,fds:BTreeMap<i32,File>,next_fd:i32,
    icons:BTreeMap<u32,Icon>,registry:BTreeMap<u32,u32>,pack:u32,epoch:u32,ready:bool,
    screen:bool,home:bool,now:u32,heap:usize,allocations:usize,oom:bool,file_oom:bool,delete:bool,
    opens:usize,reads:usize,scans:usize,writes:usize,drops:Vec<u32>,timer:bool,period:u32,
    chunk:usize,fail_reads:bool,
}
static UI:Mutex<Option<Ui>>=Mutex::new(None);
fn with<T>(f:impl FnOnce(&mut Ui)->T)->T { f(UI.lock().unwrap().as_mut().unwrap()) }
fn text(pointer:*const u8)->Vec<u8> { unsafe { std::ffi::CStr::from_ptr(pointer.cast()).to_bytes().to_vec() } }
fn alloc(u:&mut Ui,n:usize)->u32 {
    let at=u.heap;u.heap=(u.heap+n+63)&!63;assert!(u.heap<0x20400000);at as u32
}
mod icon_apply {
    pub const ICON_STEMS:[&[u8];38]=[
        b"activities",b"aivs",b"alarm",b"alipay",b"breath",b"calendar",b"camera",b"card",
        b"chronograph",b"compass",b"dealt",b"findphone",b"flashlight",b"heartrate",b"innovation_research",b"interconnect",
        b"mijia",b"music",b"mute",b"oxygen",b"perpetual_calendar",b"pressure",b"recorder",b"settings",
        b"share",b"sleep",b"sports",b"sports_course",b"sports_record",b"sports_status",b"timer",b"todo",
        b"tomato_clock",b"vitality",b"weather",b"womenhealth",b"worldclock",b"wxpay"];
    pub unsafe fn animation_state()->(u32,u32,bool) { super::with(|u|(u.pack,u.epoch,u.ready)) }
    pub unsafe fn animation_target(slot:usize)->u32 { 100+slot as u32 }
    pub unsafe fn registered_icon_source(id:u32)->u32 { super::with(|u|u.registry.get(&id).copied().unwrap_or(0)) }
}
mod fw_api {
    use super::*;
    pub mod oflag { pub const RDONLY:u32=1; }
    pub mod seek { pub const SET:u32=0;pub const END:u32=2; }
    pub unsafe fn heap_malloc(n:u32)->u32 {
        with(|u| { if u.oom { return 0; }u.allocations+=1;alloc(u,n as usize) })
    }
    pub unsafe fn open(path:*const u8,flags:u32,_:u32)->i32 {
        assert_eq!(flags,1);let name=text(path);
        with(|u| { u.opens+=1;if !u.files.contains_key(&name) { return -1; }
            let fd=u.next_fd;u.next_fd+=1;u.fds.insert(fd,File{name,offset:0});fd })
    }
    pub unsafe fn close(fd:i32)->i32 { with(|u|assert!(u.fds.remove(&fd).is_some()));0 }
    pub unsafe fn lseek(fd:i32,off:i64,kind:u32)->i64 {
        with(|u| { let f=u.fds.get_mut(&fd).unwrap();let end=u.files[&f.name].len();
            let at=if kind==2 { end as i64+off } else { off };if at<0 || at as usize>end { return -1; }f.offset=at as usize;at })
    }
    pub unsafe fn read(fd:i32,out:*mut u8,n:u32)->i32 {
        with(|u| { u.reads+=1;if u.fail_reads { return -5; }let f=u.fds.get_mut(&fd).unwrap();
            let bytes=&u.files[&f.name];let count=(bytes.len()-f.offset).min(n as usize).min(u.chunk);
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(f.offset),out,count);f.offset+=count;count as i32 })
    }
    pub fn capture_now_ms()->u32 { with(|u|u.now) }
    pub unsafe fn screen_is_on()->bool { with(|u|u.screen) }
    pub unsafe fn fw_img_free_by_path(source:u32) {
        with(|u| { assert!(u.icons.values().all(|icon|icon.source!=source),"evicted current image");u.drops.push(source); });
    }
    pub unsafe fn background_image_set_source(object:u32,source:u32) {
        with(|u| {
            let id=*u.icons.iter().find(|(_,icon)|icon.object==object).expect("stale image object").0;
            let mut bound=source;
            if (source as *const u8).read()==b'/' {
                if u.file_oom { return; }
                let name=text(source as *const u8);bound=alloc(u,name.len()+1);
                std::ptr::copy_nonoverlapping(name.as_ptr(),bound as *mut u8,name.len());(bound as *mut u8).add(name.len()).write(0);
            }
            u.icons.get_mut(&id).unwrap().source=bound;u.writes+=1;
            if u.delete { u.icons.remove(&id); }
        });
    }
    pub mod dynamic_icon {
        use super::*;
        pub unsafe fn launcher_active()->bool { with(|u|u.home) }
        pub unsafe fn current(id:u32)->Option<Icon> { with(|u|{u.scans+=1;u.icons.get(&id).copied()}) }
        pub unsafe fn pack_animation_active(active:bool,period:u32)->bool { with(|u|{u.timer=active;u.period=period;});true }
    }
}
unsafe extern "C" { fn mmap(at:*mut core::ffi::c_void,size:usize,prot:i32,flags:i32,fd:i32,offset:isize)->*mut core::ffi::c_void; }
fn setup(mask:Option<u64>,count:usize) {
    unsafe {
        assert_eq!(mmap(0x20000000usize as *mut _,0x400000,3,0x32,-1,0) as usize,0x20000000);
        animation::reset_for_test();
    }
    let mut u=Ui { files:BTreeMap::new(),fds:BTreeMap::new(),next_fd:3,icons:BTreeMap::new(),registry:BTreeMap::new(),
        pack:1,epoch:1,ready:true,screen:true,home:true,now:0,heap:0x20020000,allocations:0,oom:false,file_oom:false,delete:false,
        opens:0,reads:0,scans:0,writes:0,drops:Vec::new(),timer:false,period:0,chunk:usize::MAX,fail_reads:false };
    if let Some(mask)=mask {
        let mut manifest=b"CIA1\x60\0\x60\0\x26\0\0\0".to_vec();manifest.extend(mask.to_le_bytes());
        u.files.insert(b"/data/chaos/icons/01/animations.bin".to_vec(),manifest);
    }
    for slot in 0..count {
        let stem=std::str::from_utf8(icon_apply::ICON_STEMS[slot]).unwrap();
        let path=format!("/data/chaos/icons/01/{stem}.bin");let pointer=0x20001000+slot as u32*128;
        unsafe { std::ptr::copy_nonoverlapping(path.as_ptr(),pointer as *mut u8,path.len());(pointer as *mut u8).add(path.len()).write(0); }
        u.registry.insert(100+slot as u32,pointer);
        u.icons.insert(100+slot as u32,Icon{object:1000+slot as u32,source:pointer,original:false,visible:true});
        u.files.insert(format!("/data/chaos/icons/01/{stem}_anim.bin").into_bytes(),dynamic_icon_frames::PACKAGED.to_vec());
    }
    *UI.lock().unwrap()=Some(u);
}
fn tick(now:u32) { with(|u|u.now=now);unsafe { animation::tick(); } }
fn fast(now:u32) { with(|u|u.now=now);unsafe { animation::animation_tick(); } }
fn source(id:u32)->u32 { with(|u|u.icons[&id].source) }
fn is_file(pointer:u32)->bool { unsafe { (pointer as *const u8).read()==b'/' } }
fn pixels(pointer:u32)->Vec<u8> {
    unsafe {
        assert_eq!(std::slice::from_raw_parts(pointer as *const u8,12),&[25,10,0,0,96,0,96,0,96,0,0,0]);
        let size=((pointer+12) as *const u32).read();let data=((pointer+16) as *const u32).read();
        assert_eq!(size,10240);assert_eq!(data&63,0);std::slice::from_raw_parts(data as *const u8,size as usize).to_vec()
    }
}
#[test]fn legacy_static_pack_needs_no_manifest_or_animation_allocations() {
    setup(None,2);tick(0);fast(1000);with(|u|{assert_eq!(u.allocations,0);assert_eq!(u.writes,0);assert!(!u.timer);assert!(u.fds.is_empty());});
}
#[test]fn streamed_pixels_match_the_original_gif_and_static_icons_are_untouched() {
    setup(Some(1),2);tick(0);
    let mut expected=vec![0;dynamic_icon_frames::PIXELS];let gif=dynamic_icon_frames::Animation::parse(dynamic_icon_frames::PACKAGED).unwrap();
    gif.decode_frame(0,&mut expected);let mut all=gif.palette().to_vec();all.extend(&expected);
    assert_eq!(pixels(source(100)),all);assert!(is_file(source(101)));
    let reads=with(|u|u.reads);for time in (20..260).step_by(20) { fast(time); }with(|u|assert_eq!(u.reads,reads));
    fast(260);gif.decode_frame(1,&mut expected);let mut all=gif.palette().to_vec();all.extend(expected);assert_eq!(pixels(source(100)),all);
}
#[test]fn all_visible_icons_play_without_retaining_a_file_per_icon() {
    setup(Some((1u64<<38)-1),38);for time in (0..1900).step_by(50) { tick(time); }
    with(|u| { assert_eq!(u.fds.len(),0);assert_eq!(u.allocations,38);assert_eq!(u.icons.values().filter(|i|!is_file(i.source)).count(),38); });
}
#[test]fn hidden_icons_restore_their_poster_before_cache_retirement() {
    setup(Some(1),1);tick(0);with(|u|u.icons.get_mut(&100).unwrap().visible=false);fast(50);
    assert!(is_file(source(100)));fast(100);with(|u|{assert!(u.fds.is_empty());assert!(!u.drops.is_empty());});
}
#[test]fn sleep_does_not_scan_or_draw_and_wake_restarts_the_gif() {
    setup(Some(1),1);tick(0);fast(500);let before=with(|u|{u.screen=false;(u.scans,u.writes)});
    fast(520);tick(1000);with(|u|{assert_eq!((u.scans,u.writes),before);assert!(u.fds.is_empty());assert!(!u.timer);u.screen=true;});
    tick(1500);let mut p=vec![0;9216];dynamic_icon_frames::Animation::parse(dynamic_icon_frames::PACKAGED).unwrap().decode_frame(0,&mut p);
    assert_eq!(&pixels(source(100))[1024..],p);
}
#[test]fn leaving_desktop_restores_poster_and_closes_streams() {
    setup(Some(1),1);tick(0);with(|u|u.home=false);fast(50);fast(100);
    assert!(is_file(source(100)));with(|u|{assert!(u.fds.is_empty());assert!(!u.timer);});
}
#[test]fn switching_or_removing_pack_does_not_keep_old_animation_streams() {
    setup(Some(1),1);tick(0);with(|u|{u.pack=0;u.epoch+=1;u.ready=false;});tick(50);tick(100);
    assert!(is_file(source(100)));with(|u|{assert!(u.fds.is_empty());assert!(!u.timer);});
}
#[test]fn malformed_animation_and_manifest_leave_static_posters() {
    for bad_manifest in [false,true] {
        setup(Some(1),1);with(|u|{let name=if bad_manifest {b"/data/chaos/icons/01/animations.bin".as_slice()}else{b"/data/chaos/icons/01/activities_anim.bin"};u.files.get_mut(name).unwrap()[0]^=255;});
        tick(0);assert!(is_file(source(100)));with(|u|assert!(u.fds.is_empty()));
    }
}
#[test]fn short_reads_work_and_file_assignment_oom_never_evicts_current_frame() {
    setup(Some(1),1);with(|u|u.chunk=256);tick(0);assert!(!is_file(source(100)));
    with(|u|{u.home=false;u.file_oom=true;});fast(50);assert!(!is_file(source(100)));
    with(|u|u.file_oom=false);fast(100);assert!(is_file(source(100)));fast(150);with(|u|assert!(u.fds.is_empty()));
}
#[test]fn read_errors_restore_poster_and_retry_with_a_valid_stream() {
    setup(Some(1),1);tick(0);with(|u|u.fail_reads=true);fast(260);assert!(is_file(source(100)));fast(280);
    with(|u|u.fail_reads=false);tick(300);assert!(is_file(source(100)));tick(1300);assert!(!is_file(source(100)));
}
#[test]fn allocation_failure_keeps_poster_and_can_recover() {
    setup(Some(1),1);with(|u|u.oom=true);tick(0);assert!(is_file(source(100)));with(|u|u.oom=false);tick(50);assert!(!is_file(source(100)));
}
#[test]fn external_source_changes_are_not_overwritten() {
    setup(Some(1),1);tick(0);let external=0x2000f000u32;
    unsafe{std::ptr::copy_nonoverlapping(b"/external.bin\0".as_ptr(),external as *mut u8,14);}
    with(|u|u.icons.get_mut(&100).unwrap().source=external);fast(50);tick(100);assert_eq!(source(100),external);
}
#[test]fn source_setter_can_delete_the_image_without_stale_instance_reads() {
    setup(Some(1),1);with(|u|u.delete=true);tick(0);fast(50);with(|u|{assert!(u.icons.is_empty());assert!(u.fds.is_empty());});
}
#[test]fn manifest_rejects_unknown_dimensions_counts_trailing_and_high_bits() {
    let valid=b"CIA1\x60\0\x60\0\x26\0\0\0\x01\0\0\0\0\0\0\0";
    assert_eq!(animation::manifest(valid),Some(1));
    for at in [0,4,6,8,10,19] {let mut v=valid.to_vec();v[at]^=255;assert_eq!(animation::manifest(&v),None);}
    assert_eq!(animation::manifest(&valid[..19]),None);
}
