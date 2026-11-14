//! 控制条局部背景：可分离模糊、双线性放大及预乘透明度。
pub(crate) fn blur(rgb: &mut [u8; 5184], work: &mut [u8; 5184]) {
    for _ in 0..3 {
        for y in 0..24 { for x in 0..72 { for c in 0..3 {
            let mut sum = 0u32;
            for dx in -4..=4 { sum += rgb[(y * 72 + (x as i32 + dx).clamp(0, 71) as usize) * 3 + c] as u32; }
            work[(y * 72 + x) * 3 + c] = (sum / 9) as u8;
        } } }
        for y in 0..24 { for x in 0..72 { for c in 0..3 {
            let mut sum = 0u32;
            for dy in -4..=4 { sum += work[(((y as i32 + dy).clamp(0, 23) as usize) * 72 + x) * 3 + c] as u32; }
            rgb[(y * 72 + x) * 3 + c] = (sum / 9) as u8;
        } } }
    }
}
pub(crate) fn glass(rgb: &[u8; 5184], mask: &[u8; 27648], out: &mut [u8; 110592]) {
    for y in 0..96 { for x in 0..288 {
        let at = (y * 288 + x) * 4;
        let a = ((mask[y * 288 + x] as u32 * 160) / 138).min(160);
        let sx = x / 4; let sy = y / 4; let fx = x % 4; let fy = y % 4;
        for c in 0..3 {
            let p = |xx: usize, yy: usize| rgb[(yy.min(23) * 72 + xx.min(71)) * 3 + c] as u32;
            let top = p(sx, sy) * (4 - fx) as u32 + p(sx + 1, sy) * fx as u32;
            let bottom = p(sx, sy + 1) * (4 - fx) as u32 + p(sx + 1, sy + 1) * fx as u32;
            out[at + c] = (((top * (4 - fy) as u32 + bottom * fy as u32) / 16) * a / 255) as u8;
        }
        out[at + 3] = a as u8;
    } }
}
