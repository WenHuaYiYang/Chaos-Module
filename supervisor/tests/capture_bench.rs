//! Host encoder benchmark. It does not model the device, vsync, VFS or flash.
#![allow(dead_code)]
#[path = "../src/capture_format.rs"] mod capture_format;
#[path = "../src/capture_apng.rs"] mod capture_apng;
use capture_format::*;
use std::time::Instant;
struct Count(usize);
impl Sink for Count { fn put(&mut self, b: &[u8]) -> Result<(), i32> {
    // Keep the computed bytes observable, including PNG checksums, without IO.
    std::hint::black_box(b); self.0 += b.len(); Ok(())
} }
fn run(delta: bool) -> (u128, usize) {
    let mut frame = vec![0u8; FRAME_BYTES]; let mut previous = vec![0u8; PIXELS];
    // Allocate the production dictionary on the heap, exactly as the worker does.
    let mut dictionary = unsafe { Box::<Dictionary>::new_zeroed().assume_init() };
    let mut output = Count(0); gif_header(&mut output).unwrap();
    let begin = Instant::now();
    for i in 0..120 {
        frame.fill(0); let x = i * 3 % 300;
        for y in 100..124 { for col in x..x + 24 { frame[(y * WIDTH + col) * 3 + 2] = 255; } }
        let rect = if delta && i != 0 { changed_rect(&frame, &previous).unwrap() } else { Rect::FULL };
        quantize(&frame, &mut previous);
        gif_frame(&mut output, &previous, rect, 7, &mut dictionary).unwrap();
    }
    (begin.elapsed().as_micros(), output.0 + 1)
}
fn main() {
    let full = run(false); let delta = run(true);
    println!("reference_gif_full_us={} bytes={} reference_gif_delta_us={} bytes={}", full.0, full.1, delta.0, delta.1);
    for video in [VideoFormat::Gif, VideoFormat::Apng] {
        let direct = run_pipeline(video, false); let snapshot = run_pipeline(video, true);
        assert_eq!(direct.1, snapshot.1);
        println!("direct_{:?}_delta_us={} snapshot_us={} bytes={} frames=120", video, direct.0, snapshot.0, snapshot.1);
    }
    for noise in [false, true] { let result = run_apng_full(noise);
        println!("apng_full_noise={noise} us={} bytes={} frames=30", result.0, result.1);
    }
}

fn run_pipeline(video: VideoFormat, buffered: bool) -> (u128, usize) {
    let mut frame = vec![0u8; FRAME_BYTES];
    let mut snapshot = vec![0u8; FRAME_BYTES];
    let mut previous = vec![0u8; PIXELS]; let mut current = vec![0u8; PIXELS];
    let mut dictionary = if video == VideoFormat::Gif { Some(unsafe { Box::<Dictionary>::new_zeroed().assume_init() }) } else { None };
    let mut output = Count(0); let mut apng = capture_apng::Encoder::new();
    if video == VideoFormat::Gif { gif_header(&mut output).unwrap(); } else { capture_apng::header(&mut output).unwrap(); }
    let begin = Instant::now();
    for i in 0..120 {
        frame.fill(0); let x = i * 3 % 300;
        for y in 100..124 { for col in x..x + 24 { frame[(y * WIDTH + col) * 3 + 2] = 255; } }
        if buffered { snapshot.copy_from_slice(&frame); quantize(&snapshot, &mut current); }
        else { unsafe { quantize_framebuffer(frame.as_ptr(), &mut current); } }
        let rect = if i == 0 { Rect::FULL } else { indexed_changed_rect(&current, &previous).unwrap() };
        if video == VideoFormat::Gif { gif_frame(&mut output, &current, rect, 7, dictionary.as_mut().unwrap()).unwrap(); }
        else { apng.frame(&mut output, &current, rect, if i % 3 == 0 { 16 } else { 17 }).unwrap(); }
        core::mem::swap(&mut current, &mut previous);
    }
    if video == VideoFormat::Apng { apng.end(&mut output).unwrap(); } else { output.put(&[0x3b]).unwrap(); }
    (begin.elapsed().as_micros(), output.0)
}

fn run_apng_full(noise: bool) -> (u128, usize) {
    let mut data = vec![0u8; PIXELS]; let mut seed = 1u32;
    for (i, byte) in data.iter_mut().enumerate() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *byte = if noise { (seed >> 24) as u8 } else { ((i / 12) % 256) as u8 };
    }
    let mut encoder = capture_apng::Encoder::new(); let mut output = Count(0);
    capture_apng::header(&mut output).unwrap(); let begin = Instant::now();
    for _ in 0..30 { encoder.frame(&mut output, &data, Rect::FULL, 17).unwrap(); }
    (begin.elapsed().as_micros(), output.0)
}
