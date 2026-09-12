//! Diagnostic tool, not part of the crate's public surface: measures real
//! per-frame encode time at a given resolution. This is what found the
//! debug-vs-release gap documented in `docs/architecture.md` -- rerun it
//! after any encoder tuning change to confirm an improvement is real.
#![allow(clippy::cast_possible_truncation)]
use std::env;
use std::time::Instant;

use va_encode::{Av1Encoder, RawFrame, VideoEncoder};

fn main() {
    let mut args = env::args().skip(1);
    let width: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1280);
    let height: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(720);
    let frames: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    println!("bench: {width}x{height}, {frames} frames, debug_assertions={}", cfg!(debug_assertions));

    let mut encoder = Av1Encoder::new(width, height, 1500).unwrap();
    let stride = width * 4;
    let mut bgra = vec![0u8; (stride * height) as usize];
    for (i, chunk) in bgra.chunks_exact_mut(4).enumerate() {
        chunk[0] = (i % 256) as u8;
        chunk[1] = ((i / 7) % 256) as u8;
        chunk[2] = ((i / 13) % 256) as u8;
        chunk[3] = 255;
    }

    for i in 0..frames {
        // Mutate a stripe each frame so it's not a static input.
        let row = (i as usize * 7) % height as usize;
        for x in 0..width as usize {
            let px = row * stride as usize + x * 4;
            bgra[px] = bgra[px].wrapping_add(37);
        }
        let frame = RawFrame { width, height, stride, bgra: bgra.clone() };
        let start = Instant::now();
        let chunks = encoder.encode(&frame, i == 0).unwrap();
        let elapsed = start.elapsed();
        println!("frame {i:3}: {:8.3}s, {} packet(s)", elapsed.as_secs_f64(), chunks.len());
    }
}
