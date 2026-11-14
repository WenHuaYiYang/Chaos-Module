//! p67/3.101.043快应用动画：原生启动入口与前台/创建回调适配。
use crate::{fw_api as fw, mem::{rd8,rd16,rd32,safe_ptr}};
use core::{mem::transmute,ptr::write_volatile};
const CONFIG:&[u8]=b"/data/chaos/quickapp-animation.cfg\0";
const CLOSE_CONFIG:&[u8]=b"/data/chaos/quickapp-close-animation.cfg\0";
// 0x0CA6D518读取接口+4的vtable，再从表+0x18取show_app。
const TABLE_SLOT:u32=0x200eb688;
const STOCK_TABLE:u32=0x2cde7d18;
const TABLE_WORDS:usize=15;
const SHOW_STOCK:u32=0x0ca6cc99;
const CREATE_STOCK:u32=0x0c547699;
const FOREGROUND_STOCK:u32=0x0c547765;
const HIDE_STOCK:u32=0x0c5475f9;
const DESTROY_STOCK:u32=0x0c54751d;
static mut LOADED:bool=false;
static mut ENABLED:bool=false;
static mut CLOSE_LOADED:bool=false;
static mut CLOSE_ENABLED:bool=false;
static mut INSTALLED:bool=false;
static mut ORIGINAL_TABLE:u32=0;
static mut TABLE:[u32;TABLE_WORDS]=[0;TABLE_WORDS];
static mut PENDING_ID:u32=0;
static mut RETRIES:u8=0;
static mut CONTEXT:[u32;14]=[0;14];
static mut DEFERRED_ID:u32=0;
static mut DESC_IN:[u32;2]=[0;2];
static mut DESC_OUT:[u32;2]=[0;2];
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
pub(crate) unsafe fn close_enabled()->bool {
    if !CLOSE_LOADED {CLOSE_LOADED=true;let fd=fw::open(CLOSE_CONFIG.as_ptr(),fw::oflag::RDONLY,0);
        if fd>=0 {let mut v=0;CLOSE_ENABLED=fw::read(fd,&mut v,1)==1&&v==1;fw::close(fd);}}
    CLOSE_ENABLED
}
pub(crate) unsafe fn toggle_close(){
    CLOSE_ENABLED=!close_enabled();
    let v=[CLOSE_ENABLED as u8];let fd=fw::open(CLOSE_CONFIG.as_ptr(),fw::oflag::WRONLY|fw::oflag::CREAT,0o640);
    if fd>=0{fw::write(fd,v.as_ptr(),1);fw::close(fd);}tick();
}
pub(crate) unsafe fn close_status()->*const u8 {
    if !close_enabled(){return "已关闭\0".as_ptr();}
    if INSTALLED&&word(TABLE_SLOT,0)==core::ptr::addr_of!(TABLE)as u32{"已开启\0".as_ptr()}
    else{"固件入口未匹配\0".as_ptr()}
}
pub(crate) unsafe fn status()->*const u8 {
    if !enabled(){return "已关闭\0".as_ptr();}
    if INSTALLED&&word(TABLE_SLOT,0)==core::ptr::addr_of!(TABLE)as u32{
        "已开启\0".as_ptr()
    }else{"固件入口未匹配\0".as_ptr()}
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
unsafe fn closing(app:u32){
    if !close_enabled()||!safe_ptr(app){return;}
    let proxy=word(app,4);if !safe_ptr(proxy){return;}
    if rd8((proxy+0x39)as *const u8)==0{return;}
    let id=rd16((app+0x16)as *const u16)as u32;
    crate::quickapp_exit::capture(id,word(proxy,0x2c),core::ptr::addr_of!(CONTEXT).cast());
}
unsafe extern "C" fn hide(app:u32){closing(app);let f:unsafe extern "C" fn(u32)=transmute(HIDE_STOCK as usize);f(app);}
unsafe extern "C" fn destroy(app:u32){closing(app);let f:unsafe extern "C" fn(u32)=transmute(DESTROY_STOCK as usize);f(app);}
unsafe extern "C" fn show(id:u32,arg:u32,input:u32,output:u32)->u32{
    if enabled()&&crate::quickapp_splash::owns(id){return 0;}
    if (enabled()||close_enabled())&&id>255 {
        let app=record(id);
        if app!=0&&(word(app,0x4c)==CREATE_STOCK||word(app,0x4c)==create as *const ()as u32) {
            crate::quickapp_exit::cancel();
            let init:unsafe extern "C" fn(*mut u32)=transmute(0x0ca45609usize);
            init(core::ptr::addr_of_mut!(CONTEXT).cast());
            CONTEXT[2]=0x33;CONTEXT[9]=168;CONTEXT[10]=240;
            if safe_ptr(input)&&word(input,0)==0x0ca46249 {
                let ctx=word(input,4);
                if safe_ptr(ctx){for i in 0..14 {CONTEXT[i]=word(ctx,i as u32*4);}}
            }
            if word(app,0x4c)==CREATE_STOCK {write_volatile((app+0x4c)as *mut u32,create as *const ()as u32);}
            if word(app,0x50)==FOREGROUND_STOCK {write_volatile((app+0x50)as *mut u32,foreground as *const ()as u32);}
            if close_enabled(){
                if word(app,0x5c)==HIDE_STOCK{write_volatile((app+0x5c)as *mut u32,hide as *const ()as u32);}
                if word(app,0x64)==DESTROY_STOCK{write_volatile((app+0x64)as *mut u32,destroy as *const ()as u32);}
            }
            // 原生桌面传入空启动参数；只延后我们能完整持有的描述符。
            // 路径/深链接等未知参数仍按原始同步调用传递。
            let valid_in=input==0||(safe_ptr(input)&&(word(input,0)==0||word(input,0)==0x0ca46249));
            let valid_out=output==0||(safe_ptr(output)&&(word(output,0)==0||word(output,0)==0x0ca462d9)&&word(output,4)==0);
            if enabled()&&arg==0&&valid_in&&valid_out&&crate::quickapp_splash::start(id,core::ptr::addr_of!(CONTEXT).cast()) {
                DESC_IN=[0x0ca46249,core::ptr::addr_of!(CONTEXT)as u32];
                DESC_OUT=[0x0ca462d9,0];DEFERRED_ID=id;
                return 0;
            }
        }
    }
    let f:unsafe extern "C" fn(u32,u32,u32,u32)->u32=transmute(SHOW_STOCK as usize);
    f(id,arg,input,output)
}
pub(crate) unsafe fn pending()->bool{fw::screen_is_on()&&((ENABLED&&(PENDING_ID!=0||crate::quickapp_splash::active()))||crate::quickapp_exit::active())}
unsafe fn install()->bool {
    let current=word(TABLE_SLOT,0);
    let ours=core::ptr::addr_of!(TABLE) as u32;
    if INSTALLED {return current==ours;}
    if current&3!=0||!crate::mem::plausible_ptr(current){return false;}
    // Flash表或兼容RAM表均需逐项匹配，不能丢弃其他模块的回调。
    for i in 0..TABLE_WORDS {
        let original=word(STOCK_TABLE,i as u32*4);
        if word(current,i as u32*4)!=original{return false;}
        TABLE[i]=original;
    }
    if TABLE[6]!=SHOW_STOCK{return false;}
    TABLE[6]=show as *const () as u32;
    ORIGINAL_TABLE=current;
    core::arch::asm!("dmb",options(nostack,preserves_flags));
    write_volatile(TABLE_SLOT as *mut u32,ours);
    INSTALLED=word(TABLE_SLOT,0)==ours;
    INSTALLED
}
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
                if word(app,0x5c)==hide as *const ()as u32{write_volatile((app+0x5c)as *mut u32,HIDE_STOCK);}
                if word(app,0x64)==destroy as *const ()as u32{write_volatile((app+0x64)as *mut u32,DESTROY_STOCK);}
            }
        }
        node=word(node,4);
    }
}
pub(crate) unsafe fn tick(){
    let open_on=enabled();let close_on=close_enabled();let on=open_on||close_on;
    let ours=core::ptr::addr_of!(TABLE) as u32;
    if on {let _=install();}
    crate::quickapp_exit::tick(close_on);
    if !open_on {
        // 已接受的点击不能因用户关闭开关而丢失。
        if DEFERRED_ID!=0{let id=DEFERRED_ID;DEFERRED_ID=0;
            let f:unsafe extern "C" fn(u32,u32,u32,u32)->u32=transmute(SHOW_STOCK as usize);
            f(id,0,DESC_IN.as_ptr()as u32,DESC_OUT.as_ptr()as u32);}
        crate::quickapp_splash::cancel();
        PENDING_ID=0;
    }
    if !on {
        if INSTALLED{restore_callbacks();}
        if INSTALLED&&word(TABLE_SLOT,0)==ours{write_volatile(TABLE_SLOT as *mut u32,ORIGINAL_TABLE);}
        INSTALLED=false;PENDING_ID=0;return;
    }
    if !open_on{return;}
    if DEFERRED_ID!=0 {
        if crate::quickapp_splash::launch_ready(){
            let id=DEFERRED_ID;DEFERRED_ID=0;
            let f:unsafe extern "C" fn(u32,u32,u32,u32)->u32=transmute(SHOW_STOCK as usize);
            f(id,0,DESC_IN.as_ptr()as u32,DESC_OUT.as_ptr()as u32);
            PENDING_ID=id;RETRIES=200;
        }else if !crate::quickapp_splash::active(){DEFERRED_ID=0;}
    }
    if crate::quickapp_splash::active(){
        let id=crate::quickapp_splash::app();let app=record(id);
        let proxy=if app!=0{word(app,4)}else{0};
        let root=if safe_ptr(proxy){word(proxy,0x2c)}else{0};
        crate::quickapp_splash::tick(id,root);
        if crate::quickapp_splash::active(){return;}
        PENDING_ID=0;
    }
    if PENDING_ID==0{return;}
    if RETRIES==0{PENDING_ID=0;return;}RETRIES-=1;
    if !fw::screen_is_on(){return;}
    let app=record(PENDING_ID);if app==0{PENDING_ID=0;return;}
    let proxy=word(app,4);if !safe_ptr(proxy){return;}
    let root=word(proxy,0x2c);
    if !safe_ptr(root)||word(root,0x2c)&1!=0{return;}
    if !crate::quickapp_splash::front(root,PENDING_ID){return;}
    PENDING_ID=0;
    let animate:unsafe extern "C" fn(u32,u32,*const u32)=transmute(0x0ca46249usize);
    animate(root,0,core::ptr::addr_of!(CONTEXT).cast());
}
