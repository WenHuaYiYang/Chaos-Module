//! Decode on a worker, publish on the UI owner, retire after DELETE and a GPU fence.
//! Eight thumbnails plus one preview, no filesystem/decode work in draw events.
use crate::{capture_decode as codec, fw_api as fw};
use core::{ptr, sync::atomic::{AtomicU8, Ordering}};
const COUNT:usize=9;
#[derive(Clone,Copy)]
struct Slot {object:u32,row:u32,generation:u32,pid:u32,path:[u8;96],allocation:u32,pixels:u32,status:u8,scale:u32,caption:[u8;64]}
impl Slot {const EMPTY:Self=Self{object:0,row:0,generation:0,pid:0,path:[0;96],allocation:0,pixels:0,status:0,scale:256,caption:[0;64]};}
#[repr(align(64))]
#[derive(Clone,Copy)]
struct Descriptor([u32;16]);
static mut SLOTS:[Slot;COUNT]=[Slot::EMPTY;COUNT];
static mut DESCRIPTORS:[Descriptor;COUNT]=[Descriptor([0;16]);COUNT];
static mut SERIAL:u32=0;
static mut STATUS_TEXT:[[u8;160];COUNT]=[[0;160];COUNT];
#[derive(Clone,Copy)]
struct Request {path:[u8;96],slot:usize,generation:u32,width:usize,height:usize}
static mut REQUEST:Request=Request{path:[0;96],slot:0,generation:0,width:0,height:0};
struct ResultImage {allocation:u32,pixels:u32,width:u32,height:u32,error:i32}
static mut RESULT:ResultImage=ResultImage{allocation:0,pixels:0,width:0,height:0,error:0};
// 0 idle; 1 worker owns REQUEST; 2 completed RESULT published to UI owner.
static JOB:AtomicU8=AtomicU8::new(0);
struct Buffer(u32);
impl Buffer {
    unsafe fn new(bytes:usize)->Result<Self,i32> {
        let p=fw::heap_malloc(bytes.max(1)as u32);if p==0{Err(-12)}else{Ok(Self(p))}
    }
    unsafe fn slice(&mut self,n:usize)->&mut[u8]{core::slice::from_raw_parts_mut(self.0 as *mut u8,n)}
}
impl Drop for Buffer {fn drop(&mut self){unsafe{if self.0!=0{fw::fw_free(self.0);}}}}
unsafe fn load(request:Request)->Result<ResultImage,i32> {
    let fd=fw::capture_result(fw::open(request.path.as_ptr(),fw::oflag::RDONLY,0));
    if fd<0{return Err(fd);}
    let data=(||{
        let n=fw::lseek(fd,0,fw::seek::END);
        if n<=0||n>codec::MAX_FILE as i64{return Err(-27);}
        if fw::lseek(fd,0,fw::seek::SET)!=0{return Err(-5);}
        let len=n as usize;let mut buffer=Buffer::new(len)?;let mut used=0;let mut retries=0;
        let bytes=buffer.slice(len);
        while used<len {
            let n=fw::nsh_read(fd,&mut bytes[used..]);
            if n == -4&&retries<8{retries+=1;continue;}
            if n<=0||n as usize>len-used{return Err(if n<0{n}else{-5});}
            used+=n as usize;
        }
        Ok((buffer,len))
    })();
    let close=fw::capture_result(fw::close(fd));
    let(mut file,len)=data?;if close<0{return Err(close);}
    let data=file.slice(len);let info=codec::inspect(data)?;
    let(w,h)=codec::fit(info,request.width,request.height);
    let mut filtered=Buffer::new(info.raw)?;
    let mut image=Buffer::new(w*h*3+63)?;
    let pixels=(image.0+63)&!63;
    let out=core::slice::from_raw_parts_mut(pixels as *mut u8,w*h*3);
    codec::decode(data,filtered.slice(info.raw),out,w,h)?;
    let result=ResultImage{allocation:image.0,pixels,width:w as u32,height:h as u32,error:0};
    image.0=0;Ok(result)
}
unsafe extern "C" fn worker(_:i32,_:*mut *mut u8)->i32 {
    let request=ptr::read(ptr::addr_of!(REQUEST));
    RESULT=match load(request){Ok(image)=>image,Err(e)=>ResultImage{allocation:0,pixels:0,width:0,height:0,error:e}};
    JOB.store(2,Ordering::Release);0
}
unsafe extern "C" fn deleted(event:u32) {
    if fw::event_get_code(event)!=36{return;}
    let tag=fw::event_user_data(event);let i=(tag&15)as usize;
    if i<COUNT && SLOTS[i].generation==tag>>4 { SLOTS[i].object=0;SLOTS[i].row=0; }
}
/// Bind an empty image on the UI owner; decoding is queued by poll after render.
pub(crate) unsafe fn attach(i:usize,object:u32,row:u32,pid:u32,path:[u8;96],scale:u32,caption:[u8;64])->bool {
    if i>=COUNT||object==0||SLOTS[i].object!=0{return false;}
    SERIAL=(SERIAL+1)&0x0fff_ffff;if SERIAL==0{SERIAL=1;}
    if fw::obj_add_event(object,deleted as *const()as u32,36,(SERIAL<<4)|i as u32)==0{return false;}
    let s=&mut SLOTS[i];s.object=object;s.row=row;s.generation=SERIAL;s.pid=pid;s.path=path;s.status=0;s.scale=scale;s.caption=caption;
    if path.iter().position(|&v|v==0).is_none_or(|n|path[..n].ends_with(b".part")){s.status=2;status(i,-22);}
    true
}
unsafe fn status(i:usize,error:i32) {
    let s=&SLOTS[i];if s.row==0{return;}
    let text=if error==0{if i==8{"滑动查看下方删除操作\0".as_ptr()}else{"点击图片预览\0".as_ptr()}}
        else if error == -12 {"内存不足，无法预览\0".as_ptr()}
        else {"无法预览：图片未完成、损坏或格式不支持\0".as_ptr()};
    let length=s.caption.iter().position(|&b|b==0).unwrap_or(64);
    let buffer=&mut STATUS_TEXT[i];buffer.fill(0);buffer[..length].copy_from_slice(&s.caption[..length]);
    let mut n=length;
    if n!=0 {buffer[n..n+3].copy_from_slice(b" | ");n+=3;}
    let mut at=0;
    while *text.add(at)!=0&&n+1<buffer.len(){buffer[n]=*text.add(at);n+=1;at+=1;}
    fw::row_update(s.row,ptr::null(),ptr::null(),buffer.as_ptr(),0,0);
}
/// No retained UI pointer is used after its image DELETE callback. Buffers are
/// immutable while displayed; stale worker results never enter the renderer.
pub(crate) unsafe fn pending()->bool {
    if JOB.load(Ordering::Acquire)!=0{return true;}
    let foreground=crate::st_rd!(crate::state::FG_PAGE);
    SLOTS.iter().any(|s|s.object!=0&&s.status==0&&s.pid==foreground)
}
pub(crate) unsafe fn poll() {
    let completed=JOB.load(Ordering::Acquire)==2;
    let retire=(0..COUNT).any(|i|SLOTS[i].allocation!=0 && (SLOTS[i].object==0||SLOTS[i].status==0));
    if (completed||retire) && !fw::s541_frame_idle(){return;}
    for i in 0..COUNT {
        let s=&mut SLOTS[i];
        if s.allocation!=0 && (s.object==0||s.status==0) {
            fw::fw_img_free_by_path(DESCRIPTORS[i].0.as_ptr()as u32);
            fw::fw_free(s.allocation);s.allocation=0;s.pixels=0;DESCRIPTORS[i].0=[0;16];
        }
    }
    if completed {
        let request=ptr::read(ptr::addr_of!(REQUEST));
        let result=ptr::read(ptr::addr_of!(RESULT));
        let i=request.slot;let s=&mut SLOTS[i];
        if s.object==0 || s.generation!=request.generation {
            if result.allocation!=0{fw::fw_free(result.allocation);}
        } else if result.error!=0 {s.status=2;status(i,result.error);}
        else {
            let bytes=result.width*result.height*3;
            DESCRIPTORS[i].0=[0;16];
            DESCRIPTORS[i].0[..7].copy_from_slice(&[0x0f19,result.width|(result.height<<16),result.width*3,bytes,result.pixels,0,0]);
            fw::s541_clean_pixels(result.pixels as *const u8,bytes);
            fw::s541_clean_pixels(DESCRIPTORS[i].0.as_ptr().cast(),64);
            s.allocation=result.allocation;s.pixels=result.pixels;s.status=1;
            fw::background_image_set_source(s.object,DESCRIPTORS[i].0.as_ptr()as u32);
            fw::background_image_set_scale(s.object,s.scale,s.scale);
            fw::background_image_set_pivot(s.object,0,0);
            let (cw,ch)=if i==8{(280,400)}else{(168,240)};
            fw::obj_align(s.object,fw::align::TOP_LEFT,
                (cw-(result.width*s.scale/256)as i32)/2,(ch-(result.height*s.scale/256)as i32)/2);
            status(i,0);
        }
        JOB.store(0,Ordering::Release);
    }
    if JOB.load(Ordering::Acquire)!=0||!fw::screen_is_on(){return;}
    let foreground=crate::st_rd!(crate::state::FG_PAGE);
    // Prioritize the full preview over remaining gallery thumbnails.
    for i in (0..COUNT).map(|n|if n==0{8}else{n-1}) {
        let s=&SLOTS[i];
        if s.object==0||s.status!=0||s.allocation!=0||s.pid!=foreground{continue;}
        let (width,height)=if i==8{(280,400)}else{(84,120)};
        REQUEST=Request{path:s.path,slot:i,generation:s.generation,width,height};
        JOB.store(1,Ordering::Release);
        let rc=fw::nsh_spawn(b"chaos.preview\0".as_ptr(),worker,ptr::null_mut());
        if rc<=0 {JOB.store(0,Ordering::Release);SLOTS[i].status=2;status(i,rc);}
        break;
    }
}

#[cfg(test)]
include!("../tests/capture_preview_cases.rs");
