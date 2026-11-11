#[path = "../src/dynamic_icon_frames.rs"] mod frames;
use frames::{Animation, PACKAGED, PIXELS};

#[test]
fn uploaded_gif_geometry_timing_and_transparency() {
    for (data, count, cycle, period) in [(PACKAGED, 193, 5200, 20)] {
        let animation = Animation::parse(data).unwrap();
        assert_eq!((animation.count, animation.cycle_ms, animation.plays, animation.period_ms), (count, cycle, 0, period));
        let mut pixels = [0; PIXELS];
        let mut hashes = std::collections::BTreeSet::new();
        let table = u32::from_le_bytes(data[20..24].try_into().unwrap()) as usize;
        let mut elapsed = 0;
        for index in 0..animation.count {
            assert!(animation.decode_frame(index, &mut pixels));
            for corner in [0, 95, PIXELS - 96, PIXELS - 1] {
                assert_eq!(animation.palette()[pixels[corner] as usize * 4 + 3], 0);
            }
            hashes.insert(pixels.iter().fold(2166136261u32, |h, &v| (h ^ v as u32).wrapping_mul(16777619)));
            let delay = u32::from_le_bytes(data[table + index * 12..table + index * 12 + 4].try_into().unwrap());
            assert_eq!(animation.frame_at(elapsed + delay - 1), index); elapsed += delay;
        }
        assert!(hashes.len() > 20, "must contain the supplied animation, not a repeated poster");
        assert_eq!(animation.frame_at(cycle), 0);
    }
}
#[test]
fn variable_durations_and_finite_repeats_are_honored() {
    let mut data = PACKAGED.to_vec();
    let table = u32::from_le_bytes(data[20..24].try_into().unwrap()) as usize;
    let combined = u32::from_le_bytes(data[table..table + 4].try_into().unwrap())
        + u32::from_le_bytes(data[table + 12..table + 16].try_into().unwrap());
    data[table..table + 4].copy_from_slice(&25u32.to_le_bytes());
    data[table + 12..table + 16].copy_from_slice(&(combined - 25).to_le_bytes());
    data[16..20].copy_from_slice(&2u32.to_le_bytes());
    let animation = Animation::parse(&data).unwrap();
    assert_eq!(animation.frame_at(24), 0); assert_eq!(animation.frame_at(25), 1);
    assert_eq!(animation.frame_at(combined - 1), 1); assert_eq!(animation.frame_at(combined), 2);
    assert_eq!(animation.frame_at(5200), 0); assert_eq!(animation.frame_at(10400), 192);
}
#[test]
fn truncated_invalid_tables_and_bad_rle_never_overwrite_guards() {
    for end in [0, 8, 12, 59, 1083, PACKAGED.len() - 1] {
        assert!(Animation::parse(&PACKAGED[..end]).is_none());
    }
    for at in [0, 4, 8, 10, 12, 20, 24] {
        let mut data = PACKAGED.to_vec(); data[at] ^= 255;
        assert!(Animation::parse(&data).is_none(), "offset {at}");
    }
    let mut data = PACKAGED.to_vec();
    let table = u32::from_le_bytes(data[20..24].try_into().unwrap()) as usize;
    let start = u32::from_le_bytes(data[table + 4..table + 8].try_into().unwrap()) as usize;
    let size = u32::from_le_bytes(data[table + 8..table + 12].try_into().unwrap()) as usize;
    data[start..start + size].fill(0x80);
    let animation = Animation::parse(&data).unwrap();
    let mut guarded = [0xA5; PIXELS + 2];
    assert!(!animation.decode_frame(0, &mut guarded[1..PIXELS + 1]));
    assert_eq!((guarded[0], guarded[PIXELS + 1]), (0xA5, 0xA5));
    assert!(!animation.decode_frame(193, &mut guarded[1..PIXELS + 1]));
}
