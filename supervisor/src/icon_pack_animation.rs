//! Optional, streamed DGF1 launcher animations alongside legacy CIPK posters.
//! UI owner only. Published descriptors/pixels remain allocated for module life.
use crate::{fw_api as fw, icon_apply as packs, mem};
use crate::dynamic_icon_frames::{self as frames, FrameIndex, WIDTH, HEIGHT, PIXELS, PALETTE_BYTES,
    HEADER_BYTES, MAX_METADATA, MAX_ENCODED, MAX_FILE_BYTES};
use fw::dynamic_icon as port;
const STEMS: usize = 38;
pub const MAX_ACTIVE: usize = STEMS;
const EMPTY: usize = STEMS;
const DATA_BYTES: usize = PALETTE_BYTES + PIXELS;
const PATH_CAP: usize = 128;

#[repr(C)]
struct Player {
    buffers: [[u8; DATA_BYTES+63];2],
    descriptors: [[u32;7];2],
    ready: [bool;2],
    metadata: [u8;MAX_METADATA],
    metadata_len: usize,
    file_bytes: usize,
    fd: i32,
    pack: u32,
    epoch: u32,
    app_id: u32,
    start: Option<u32>,
    last_frame: usize,
    last_source: u32,
    failed_at: Option<u32>,
    stopping: bool,
}
static mut PLAYERS: [usize;STEMS] = [0;STEMS];
static mut ACTIVE: [usize;MAX_ACTIVE] = [EMPTY;MAX_ACTIVE];
static mut SCRATCH: [u8;MAX_ENCODED] = [0;MAX_ENCODED];
static mut PACK: u32 = 0;
static mut EPOCH: u32 = 0;
static mut INDEX_CHECKED: bool = false;
static mut MASK: u64 = 0;
static mut CURSOR: usize = 0;
static mut BUSY: bool = false;

pub fn manifest(data: &[u8]) -> Option<u64> {
    if data.len()!=20 || &data[..4]!=b"CIA1" || data[4..12]!=[96,0,96,0,38,0,0,0] { return None; }
    let mask=u64::from_le_bytes(data[12..20].try_into().ok()?);
    if mask >> STEMS != 0 { return None; } Some(mask)
}
/// Caller supplies a 128-byte buffer; only module-owned stems and numeric IDs.
pub(crate) unsafe fn file_path(out: *mut u8, pack: u32, slot: usize, animated: bool) {
    if !(1..=8).contains(&pack) || slot>=STEMS { out.write(0); return; }
    let root=b"/data/chaos/icons/";
    core::ptr::copy_nonoverlapping(root.as_ptr(),out,root.len());
    let mut at=root.len();
    for byte in [b'0'+(pack/10) as u8,b'0'+(pack%10) as u8,b'/'] { out.add(at).write(byte);at+=1; }
    let stem=packs::ICON_STEMS[slot];
    core::ptr::copy_nonoverlapping(stem.as_ptr(),out.add(at),stem.len()); at+=stem.len();
    let tail: &[u8]=if animated { b"_anim.bin\0" } else { b".bin\0" };
    core::ptr::copy_nonoverlapping(tail.as_ptr(),out.add(at),tail.len());
}
unsafe fn matches(source: u32, path: &[u8]) -> bool {
    if !mem::plausible_ptr(source) { return false; }
    for (offset,&expected) in path.iter().enumerate() {
        let Some(at)=source.checked_add(offset as u32) else { return false; };
        if !mem::plausible_ptr(at) || mem::rd8(at as *const u8)!=expected { return false; }
        if expected==0 { return true; }
    }
    false
}
unsafe fn read_exact(fd: i32, out: &mut [u8]) -> bool {
    let mut used=0;
    for _ in 0..64 {
        if used==out.len() { return true; }
        let n=fw::read(fd,out.as_mut_ptr().add(used),(out.len()-used) as u32);
        if n<=0 || n as usize>out.len()-used { return false; }
        used+=n as usize;
    }
    used==out.len()
}
unsafe fn close_stream(player: *mut Player) {
    if (*player).fd>=0 { fw::close((*player).fd); (*player).fd=-1; }
}
unsafe fn close(player: *mut Player) {
    close_stream(player);
    (*player).start=None; (*player).last_frame=usize::MAX;
}
unsafe fn descriptor(player: *mut Player, slot: usize) -> u32 {
    core::ptr::addr_of!((*player).descriptors[slot]) as u32
}
unsafe fn owns(player: *mut Player, source: u32) -> bool {
    (0..2).any(|slot| (*player).ready[slot] && descriptor(player,slot)==source)
}
unsafe fn retire(player: *mut Player) {
    for slot in 0..2 { if (*player).ready[slot] { fw::fw_img_free_by_path(descriptor(player,slot)); } }
}
unsafe fn player(slot: usize) -> *mut Player {
    if PLAYERS[slot]==0 {
        let memory=fw::heap_malloc(core::mem::size_of::<Player>() as u32) as *mut Player;
        if memory.is_null() { return memory; }
        core::ptr::write_bytes(memory.cast::<u8>(),0,core::mem::size_of::<Player>());
        // Option representations must be initialized as values, not assumed.
        (*memory).fd=-1; (*memory).start=None; (*memory).failed_at=None;
        (*memory).last_frame=usize::MAX;
        PLAYERS[slot]=memory as usize;
    }
    PLAYERS[slot] as *mut Player
}
unsafe fn load(player: *mut Player, slot: usize, now: u32) -> bool {
    if (*player).fd>=0 { return true; }
    if (*player).failed_at.is_some_and(|last| now.wrapping_sub(last)<1000) { return false; }
    (*player).failed_at=Some(now);
    let mut path=[0;PATH_CAP]; file_path(path.as_mut_ptr(),PACK,slot,true);
    let fd=fw::open(path.as_ptr(),fw::oflag::RDONLY,0);
    if fd<0 { return false; }
    let size=fw::lseek(fd,0,fw::seek::END);
    let mut ok=size>0 && size as u64<=MAX_FILE_BYTES as u64 && fw::lseek(fd,0,fw::seek::SET)==0;
    let meta=&mut (*player).metadata;
    let mut header=[0;HEADER_BYTES];
    ok=ok && read_exact(fd,&mut header);
    if (*player).metadata_len!=0 {
        ok=ok && size as usize==(*player).file_bytes && header==meta[..HEADER_BYTES];
        if !ok { fw::close(fd);(*player).metadata_len=0;return false; }
        (*player).fd=fd;(*player).failed_at=None;return true;
    }
    if ok { meta[..HEADER_BYTES].copy_from_slice(&header); }
    let length=if ok { frames::metadata_len(&meta[..HEADER_BYTES]).unwrap_or(0) } else { 0 };
    ok=ok && (HEADER_BYTES..=MAX_METADATA).contains(&length);
    ok=ok && read_exact(fd,&mut meta[HEADER_BYTES..length]);
    ok=ok && FrameIndex::parse(&meta[..length],size as usize).is_some();
    if !ok { fw::close(fd); return false; }
    (*player).metadata_len=length; (*player).file_bytes=size as usize; (*player).fd=fd;
    (*player).failed_at=None; (*player).start=None; (*player).last_frame=usize::MAX;
    true
}
unsafe fn stop(entry: usize) {
    let slot=ACTIVE[entry]; if slot==EMPTY { return; }
    let player=PLAYERS[slot] as *mut Player;
    close(player); (*player).stopping=true;
    if let Some(icon)=port::current((*player).app_id) {
        if owns(player,icon.source) {
            let source=packs::registered_icon_source((*player).app_id);
            if mem::plausible_ptr(source) {
                fw::background_image_set_source(icon.object,source);
                // File assignment may OOM or DELETE. Confirm on the next pass.
            }
            return;
        }
    }
    retire(player); (*player).stopping=false; ACTIVE[entry]=EMPTY;
}
unsafe fn stop_all() { for entry in 0..MAX_ACTIVE { stop(entry); } }
unsafe fn drive() {
    if BUSY { return; } BUSY=true;
    let (pack,epoch,ready)=packs::animation_state();
    if !fw::screen_is_on() {
        for entry in 0..MAX_ACTIVE {
            if ACTIVE[entry]!=EMPTY { close(PLAYERS[ACTIVE[entry]] as *mut Player); }
        }
        port::pack_animation_active(false,20); BUSY=false; return;
    }
    if !ready || pack!=PACK || epoch!=EPOCH || !port::launcher_active() {
        stop_all(); port::pack_animation_active(false,20); BUSY=false; return;
    }
    let now=fw::capture_now_ms(); let mut period=50; let mut running=false;
    for entry in 0..MAX_ACTIVE {
        let slot=ACTIVE[entry]; if slot==EMPTY { continue; }
        let player=PLAYERS[slot] as *mut Player;
        let Some(icon)=port::current((*player).app_id) else { stop(entry);continue; };
        let mut path=[0;PATH_CAP]; file_path(path.as_mut_ptr(),PACK,slot,false);
        if !icon.visible || (!owns(player,icon.source) && !matches(icon.source,&path)) {
            stop(entry); continue;
        }
        (*player).stopping=false;
        if (*player).metadata_len==0 && !load(player,slot,now) { stop(entry);continue; }
        let index=FrameIndex::parse(&(&(*player).metadata)[..(*player).metadata_len],(*player).file_bytes).unwrap();
        if (*player).last_source!=icon.source { (*player).start=None; (*player).last_frame=usize::MAX; }
        let start=*(*player).start.get_or_insert(now);
        let frame=index.frame_at(now.wrapping_sub(start));
        period=period.min(index.period_ms); running=true;
        if frame==(*player).last_frame { continue; }
        let (offset,length)=index.block(frame).unwrap();
        if !load(player,slot,now) { stop(entry);continue; }
        let scratch=&mut SCRATCH[..length];
        if fw::lseek((*player).fd,offset as i64,fw::seek::SET)!=offset as i64 || !read_exact((*player).fd,scratch) {
            (*player).failed_at=Some(now); stop(entry); continue;
        }
        close_stream(player);
        let target=if icon.source==descriptor(player,0) { 1 } else { 0 };
        let source=descriptor(player,target);
        if (*player).ready[target] { fw::fw_img_free_by_path(source); }
        let raw=core::ptr::addr_of_mut!((*player).buffers[target]) as usize;
        let data=((raw+63)&!63) as *mut u8;
        let bytes=core::slice::from_raw_parts_mut(data,DATA_BYTES);
        bytes[..PALETTE_BYTES].copy_from_slice(&(&(*player).metadata)[HEADER_BYTES..HEADER_BYTES+PALETTE_BYTES]);
        if !frames::decode(scratch,&mut bytes[PALETTE_BYTES..]) {
            (*player).failed_at=Some(now); stop(entry); continue;
        }
        (*player).descriptors[target]=[0xA19,(HEIGHT as u32)<<16|WIDTH as u32,WIDTH as u32,DATA_BYTES as u32,data as u32,0,0];
        (*player).ready[target]=true; (*player).last_frame=frame; (*player).last_source=source;
        fw::background_image_set_source(icon.object,source);
        if owns(player,icon.source) && icon.source!=source { fw::fw_img_free_by_path(icon.source); }
        // No instance access after setter; persistent player storage is safe.
    }
    port::pack_animation_active(running,period); BUSY=false;
}
/// Maintenance loads at most one new visible candidate per 50ms pass.
pub(crate) unsafe fn tick() {
    let (pack,epoch,ready)=packs::animation_state();
    if (pack,epoch)!=(PACK,EPOCH) || !ready {
        drive();
        if ACTIVE.iter().any(|&slot| slot!=EMPTY) { return; }
        if (pack,epoch)!=(PACK,EPOCH) { PACK=pack;EPOCH=epoch;INDEX_CHECKED=false;MASK=0;CURSOR=0; }
        if !ready { return; }
    }
    if !INDEX_CHECKED {
        INDEX_CHECKED=true; MASK=0;
        let mut path=[0u8;PATH_CAP];
        let root=b"/data/chaos/icons/";
        path[..root.len()].copy_from_slice(root);
        let at=root.len(); path[at]=b'0'+(pack/10) as u8;path[at+1]=b'0'+(pack%10) as u8;
        path[at+2..at+18].copy_from_slice(b"/animations.bin\0");
        let fd=fw::open(path.as_ptr(),fw::oflag::RDONLY,0);
        if fd>=0 {
            let mut bytes=[0;20];
            if fw::lseek(fd,0,fw::seek::END)==20 && fw::lseek(fd,0,fw::seek::SET)==0 && read_exact(fd,&mut bytes) {
                MASK=manifest(&bytes).unwrap_or(0);
            }
            fw::close(fd);
        }
    }
    if MASK!=0 && fw::screen_is_on() && port::launcher_active() {
        if let Some(entry)=ACTIVE.iter().position(|&slot| slot==EMPTY) {
            for _ in 0..STEMS {
                let slot=CURSOR; CURSOR=(CURSOR+1)%STEMS;
                if MASK&(1u64<<slot)==0 || ACTIVE.contains(&slot) { continue; }
                let id=packs::animation_target(slot); if id==0 { continue; }
                let Some(icon)=port::current(id) else { continue; };
                if !icon.visible { continue; }
                let mut path=[0;PATH_CAP]; file_path(path.as_mut_ptr(),pack,slot,false);
                if !matches(icon.source,&path) { continue; }
                let player=player(slot); if player.is_null() { break; }
                if ((*player).pack,(*player).epoch)!=(pack,epoch) {
                    close(player);(*player).metadata_len=0;(*player).failed_at=None;(*player).pack=pack;(*player).epoch=epoch;
                }
                (*player).app_id=id; ACTIVE[entry]=slot; break;
            }
        }
    }
    drive();
}
pub(crate) unsafe fn animation_tick() { drive(); }
#[cfg(test)]
pub(crate) unsafe fn reset_for_test() {
    PLAYERS=[0;STEMS];ACTIVE=[EMPTY;MAX_ACTIVE];PACK=0;EPOCH=0;INDEX_CHECKED=false;MASK=0;CURSOR=0;BUSY=false;
}
