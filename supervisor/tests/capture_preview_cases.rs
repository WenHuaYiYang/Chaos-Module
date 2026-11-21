unsafe fn reset(){
    crate::world(|w|*w=crate::World{foreground:40,awake:true,fence:true,..Default::default()});
    SLOTS=[Slot::EMPTY;COUNT];DESCRIPTORS=[Descriptor([0;16]);COUNT];SERIAL=0;JOB.store(0,Ordering::Release);
}
fn path(name:&[u8])->[u8;96]{let mut path=[0;96];path[..name.len()].copy_from_slice(name);path}
unsafe fn complete(slot:usize,generation:u32,allocation:u32){
    REQUEST.slot=slot;REQUEST.generation=generation;
    RESULT=ResultImage{allocation,pixels:allocation+64,width:84,height:120,error:0};
    JOB.store(2,Ordering::Release);
}
#[test]fn preview_job_is_serial_prioritizes_visible_page_and_discards_stale_result(){unsafe{
    reset();attach(0,100,101,40,path(b"/thumb.png\0"),512,[0;64]);poll();
    assert_eq!(JOB.load(Ordering::Acquire),1);crate::world(|w|assert_eq!(w.spawns,1));poll();crate::world(|w|assert_eq!(w.spawns,1));
    let generation=SLOTS[0].generation;deleted(generation<<4);
    attach(0,102,103,40,path(b"/new.png\0"),512,[0;64]);complete(0,generation,1000);poll();
    crate::world(|w|{assert_eq!(w.free,[1000]);assert!(w.sources.is_empty());});
    assert_eq!(JOB.load(Ordering::Acquire),1);
    complete(0,SLOTS[0].generation,2000);poll();assert_eq!(SLOTS[0].status,1);
    crate::world(|w|assert_eq!(w.sources.len(),1));
    attach(8,200,201,62,path(b"/preview.bmp\0"),256,[0;64]);poll();assert_eq!(JOB.load(Ordering::Acquire),0);
    crate::world(|w|w.foreground=62);poll();assert_eq!(REQUEST.slot,8);
}}
#[test]fn preview_pixels_retire_only_after_delete_and_fence(){unsafe{
    reset();attach(0,100,101,40,path(b"/shot.bmp\0"),512,[0;64]);let generation=SLOTS[0].generation;
    complete(0,generation,3000);poll();assert_eq!(SLOTS[0].allocation,3000);
    let spawns=crate::world(|w|w.spawns);for _ in 0..20{poll();}
    crate::world(|w|{assert_eq!(w.spawns,spawns);assert!(w.free.is_empty());w.fence=false;});
    deleted(generation<<4);poll();crate::world(|w|assert!(w.free.is_empty()));
    crate::world(|w|w.fence=true);poll();crate::world(|w|assert_eq!(w.free,[3000]));
    assert_eq!(SLOTS[0].allocation,0);
}}
#[test]fn preview_error_and_partial_files_never_requeue_or_publish_pixels(){unsafe{
    reset();attach(0,100,101,40,path(b"/shot.png.part\0"),512,[0;64]);poll();
    crate::world(|w|{assert_eq!(w.spawns,0);assert_eq!(w.statuses.len(),1);});
    attach(1,200,201,40,path(b"/bad.png\0"),512,[0;64]);poll();
    RESULT=ResultImage{allocation:0,pixels:0,width:0,height:0,error:-22};JOB.store(2,Ordering::Release);poll();
    for _ in 0..20{poll();}assert_eq!(SLOTS[1].status,2);
    crate::world(|w|{assert_eq!(w.spawns,1);assert!(w.sources.is_empty());});
}}
