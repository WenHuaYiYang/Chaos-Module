use super::*;
#[test]
fn native_icons_are_not_replaced_and_glass_preserves_alpha() {
    unsafe {
        crate::state(|s|{s.objects.insert(20,10);s.objects.insert(30,10);});
        assert!(!enabled()); toggle();
        let icons = [0x10000,0x10100,0x10200];
        for visual in [0,1,2] {
            if visual != 0 { select(visual); }
            for _ in 0..4 { tick(); }
            assert_eq!(MODE,visual as usize);
            assert!(GLASS_VALID);
            crate::state(|s| {
                assert_eq!(s.preference,[0,2,1][visual as usize]);
                // 应用图标不复制、不隐藏，不给新对象绑定应用启动回调。
                for &icon in &icons {assert!(!s.flags.contains_key(&icon));}
                assert!(!s.events.iter().any(|&(_,_,id)|id>=0x10000));
                assert!(s.labels.is_empty());
                assert!(s.positions.contains(&(VIEW,24,344)));
                assert_eq!(s.honeycomb_writes,0);assert_eq!(s.desktop_writes,0);
            });
        }
        let p = &GLASS_PIXELS[GLASS_INDEX].0;
        assert_eq!(p[3],0); assert!(p.chunks_exact(4).any(|px|px[3]>0&&px[3]<255));
        assert!(p.chunks_exact(4).all(|px|px[..3].iter().all(|&c|c<=px[3])));
        crate::state(|s|assert_eq!(s.selections,vec![2,1]));
        assert!(pending()); // 捕获完成仍需快速跟随官方滚动位置。
        let samples = crate::state(|s|s.samples);
        for y in [350,340,330] {
            crate::state(|s|s.anchor_y=Some(y)); tick();
            assert!(!GLASS_VALID);
            crate::state(|s|assert_eq!(s.samples,samples));
        }
        for _ in 0..3 { tick(); }
        assert!(GLASS_VALID);
        crate::state(|s| {assert_eq!(s.samples,samples+1);assert!(s.cleans>=7);});
        toggle(); tick(); assert_eq!(VIEW,0);
        LOADED=false; assert!(!enabled());
    }
}
#[test]
fn separable_blur_reduces_edges_without_changing_flat_colors() {
    let mut flat=[72;5184]; let mut work=[0;5184];
    crate::s541_pixels::blur(&mut flat,&mut work);assert!(flat.iter().all(|&x|x==72));
    let mut edge=[0;5184];for y in 0..24 {for x in 36..72 {edge[(y*72+x)*3..(y*72+x)*3+3].fill(255);}}
    crate::s541_pixels::blur(&mut edge,&mut work);
    assert!(edge[(12*72+35)*3]>0);assert!(edge[(12*72+36)*3]<255);
}
