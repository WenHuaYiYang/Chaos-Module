use super::*;
#[test]
fn native_icons_are_not_replaced_and_glass_preserves_alpha() {
    unsafe {
        crate::state(|s|{s.objects.insert(20,10);});
        assert!(!enabled()); toggle();
        let icons = [0x10000,0x10100,0x10200];
        for visual in [0,1,2] {
            if visual != 0 { select(visual); }
            for _ in 0..4 { render(20, &icons, Point::default(), false); }
            assert_eq!(MODE,visual as usize); assert_eq!(&ICONS[..COUNT], &icons);
            assert!(GLASS_VALID);
            crate::state(|s| {
                assert_eq!(s.file[5],visual as u8);
                // 应用图标不复制、不隐藏，不给新对象绑定应用启动回调。
                for &icon in &icons {assert!(!s.flags.contains_key(&icon));}
                assert!(!s.events.iter().any(|&(_,_,id)|id>=0x10000));
            });
        }
        let p = &GLASS_PIXELS[GLASS_INDEX].0;
        assert_eq!(p[3],0); assert!(p.chunks_exact(4).any(|px|px[3]>0&&px[3]<255));
        assert!(p.chunks_exact(4).all(|px|px[..3].iter().all(|&c|c<=px[3])));
        let many:Vec<_>=(0..128).map(|i|0x10000+i*256).collect();
        for _ in 0..4 { render(20,&many,Point{x:0,y:-range(128)},false); }
        assert_eq!(COUNT,128);assert!(GLASS_VALID);
        crate::state(|s|assert!(s.objects.values().filter(|&&parent|parent==10).count()<24));
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
