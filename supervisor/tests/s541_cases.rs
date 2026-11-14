use super::*;

#[test]
fn three_views_switch_persist_and_restore_native_controls() {
    unsafe {
        crate::state(|s|{s.objects.insert(20,10);s.objects.insert(30,10);
            for icon in [0x10000,0x10100,0x10200] {s.objects.insert(icon,20);} });
        assert!(!enabled());
        toggle(); tick();
        assert!(alive());
        crate::state(|s|{assert_eq!(*s.flags.get(&20).unwrap_or(&0)&1,0);
            assert_eq!(s.flags[&0x10000]&1,1);assert_eq!(s.flags[&30]&1,1);assert!(s.labels.is_empty());});
        for visual in [1,2,0] {
            select(visual); tick();
            assert_eq!(MODE,visual as usize); assert!(alive());
            crate::state(|s|assert_eq!(s.file[5],visual as u8));
        }
        crate::state(|s|assert_eq!(s.labels.len(),6));
        let old_view = VIEW;
        crate::state(|s|s.event_code=36);
        deleted(old_view);
        crate::state(|s|{s.objects.remove(&old_view);s.event_code=0;});
        assert_eq!(VIEW,0);
        tick(); assert!(alive()); assert_ne!(VIEW,old_view);
        launch(77); crate::state(|s|assert_eq!(s.launches,1));
        toggle();tick();
        assert_eq!(VIEW,0);
        crate::state(|s|{assert_eq!(s.flags[&20]&1,0);assert_eq!(s.flags[&30]&1,0);});
        LOADED=false; assert!(!enabled());
        crate::state(|s|s.fail=true); toggle();tick();
        assert_eq!(VIEW,0);
        crate::state(|s|assert_eq!(s.flags[&20]&1,0));
    }
}
