//! p67/3.101.043快应用动画：原生启动入口与前台/创建回调适配。
use crate::{fw_api as fw, mem::{rd16,rd32,safe_ptr}};
use core::{mem::transmute,ptr::write_volatile};
const CONFIG:&[u8]=b"/data/chaos/quickapp-animation.cfg\0";
const SHOW_SLOT:u32=0x200eb6f8;
const SHOW_STOCK:u32=0x0ca6cc99;
const CREATE_STOCK:u32=0x0c547699;
const FOREGROUND_STOCK:u32=0x0c547765;
static mut LOADED:bool=false;
static mut ENABLED:bool=false;
static mut INSTALLED:bool=false;
static mut PENDING_ID:u32=0;
static mut RETRIES:u8=0;
static mut CONTEXT:[u32;14]=[0;14];
pub(crate) unsafe fn enabled()->bool {
    if !LOADED {LOADED=true;let fd=fw::open(CONFIG.as_ptr(),fw::oflag::RDONLY,0);
        if fd>=0 {let mut v=0;ENABLED=fw::read(fd,&mut v,1)==1&&v==1;fw::close(fd);}}
    ENABLED
}
pub(crate) unsafe fn toggle(){
    ENABLED=!enabled();
    let v=[ENABLED as u8];let fd=fw::open(CONFIG.as_ptr(),fw::oflag::WRONLY|fw::oflag::CREAT,0o640);
    if fd>=0 {fw::write(fd,v.as_ptr(),1);fw::close(fd);}
    tick();
}
pub(crate) unsafe fn status()->*const u8 {
    if !enabled(){return "已关闭\0".as_ptr();}
    if INSTALLED{"已开启\0".as_ptr()}else{"固件入口未匹配\0".as_ptr()}
}
unsafe fn word(p:u32,off:u32)->u32{rd32((p+off)as *const u32)}
unsafe fn record(id:u32)->u32{
    let vt=word(fw::APP_REGISTRY,0x18);
    if vt&3!=0||!crate::mem::plausible_ptr(vt){return 0;}
    let find=word(vt,0x4c);
    if find&1==0||!crate::mem::plausible_ptr(find){return 0;}
    let f:unsafe extern "C" fn(u32)->u32=transmute(find as usize);
    let app=f(id);
    if safe_ptr(app)&&rd16((app+0x16)as *const u16)as u32==id {app}else{0}
}
unsafe fn queue(app:u32){
    if enabled()&&safe_ptr(app){PENDING_ID=rd16((app+0x16)as *const u16)as u32;RETRIES=20;}
}
unsafe extern "C" fn create(app:u32,parent:u32,data:u32)->u32{
    let id=rd16((app+0x16)as *const u16)as u32;
    let f:unsafe extern "C" fn(u32,u32,u32)->u32=transmute(CREATE_STOCK as usize);
    let result=f(app,parent,data);queue(record(id));result
}
unsafe extern "C" fn foreground(app:u32){
    let id=rd16((app+0x16)as *const u16)as u32;
    let f:unsafe extern "C" fn(u32)=transmute(FOREGROUND_STOCK as usize);
    f(app);queue(record(id));
}
unsafe extern "C" fn show(id:u32,arg:u32,input:u32,output:u32)->u32{
    if enabled()&&id>255 {
        let app=record(id);
        if app!=0&&(word(app,0x4c)==CREATE_STOCK||word(app,0x4c)==create as *const ()as u32) {
            let init:unsafe extern "C" fn(*mut u32)=transmute(0x0ca45609usize);
            init(core::ptr::addr_of_mut!(CONTEXT).cast());
            CONTEXT[2]=0x33;CONTEXT[9]=168;CONTEXT[10]=240;
            if safe_ptr(input)&&word(input,0)==0x0ca46249 {
                let ctx=word(input,4);
                if safe_ptr(ctx){for i in 0..14 {CONTEXT[i]=word(ctx,i as u32*4);}}
            }
            if word(app,0x4c)==CREATE_STOCK {write_volatile((app+0x4c)as *mut u32,create as *const ()as u32);}
            if word(app,0x50)==FOREGROUND_STOCK {write_volatile((app+0x50)as *mut u32,foreground as *const ()as u32);}
        }
    }
    let f:unsafe extern "C" fn(u32,u32,u32,u32)->u32=transmute(SHOW_STOCK as usize);
    f(id,arg,input,output)
}
pub(crate) unsafe fn pending()->bool{ENABLED&&PENDING_ID!=0}
unsafe fn restore_callbacks(){
    // 仅枚举当前注册表，不持有已删除应用的旧实例指针。
    let head=word(fw::APP_REGISTRY,0);if !safe_ptr(head){return;}
    let mut node=word(head,4);
    for _ in 0..256 {
        if node==head||!safe_ptr(node){break;}
        let id=rd16((node+0x10)as *const u16)as u32;
        if id>255 {
            let app=record(id);
            if app!=0 {
                if word(app,0x4c)==create as *const ()as u32{write_volatile((app+0x4c)as *mut u32,CREATE_STOCK);}
                if word(app,0x50)==foreground as *const ()as u32{write_volatile((app+0x50)as *mut u32,FOREGROUND_STOCK);}
            }
        }
        node=word(node,4);
    }
}
pub(crate) unsafe fn tick(){
    let on=enabled();let ours=show as *const ()as u32;
    if on&&!INSTALLED&&word(SHOW_SLOT,0)==SHOW_STOCK {write_volatile(SHOW_SLOT as *mut u32,ours);INSTALLED=true;}
    if !on {
        if INSTALLED{restore_callbacks();}
        if INSTALLED&&word(SHOW_SLOT,0)==ours{write_volatile(SHOW_SLOT as *mut u32,SHOW_STOCK);}
        INSTALLED=false;PENDING_ID=0;return;
    }
    if PENDING_ID==0{return;}
    if RETRIES==0{PENDING_ID=0;return;}RETRIES-=1;
    if !fw::screen_is_on()||fw::top_app_id()!=PENDING_ID{return;}
    let app=record(PENDING_ID);if app==0{PENDING_ID=0;return;}
    let proxy=word(app,4);if !safe_ptr(proxy){return;}
    let root=word(proxy,0x2c);
    if !safe_ptr(root)||word(root,0x2c)&1!=0{return;}
    PENDING_ID=0;
    let animate:unsafe extern "C" fn(u32,u32,*const u32)=transmute(0x0ca46249usize);
    animate(root,0,core::ptr::addr_of!(CONTEXT).cast());
}
