//! Screen capture: a `FrameSource` trait producing raw BGRA8 frames, plus
//! [`TestPatternSource`] (synthetic, no OS dependency) for proving the
//! video pipeline end to end before wiring in real DXGI capture.
//!
//! Per the build plan's own stated order — "swap in real DXGI frames once
//! handshake+login+video-wire-format already work against a synthetic
//! frame source" — a real screen source is a deliberate next step, not
//! something this crate needs before the wire path can be proven live.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("no display/monitor is available to capture")]
    NoDisplay,
}

/// One captured frame, BGRA8 (4 bytes/pixel) — the format DXGI/Desktop
/// Duplication naturally produces, and what `va-encode::RawFrame` expects
/// unmodified. `stride` is bytes per row and may exceed `width * 4` when
/// the source pads rows for alignment.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
}

pub trait FrameSource {
    /// # Errors
    ///
    /// [`CaptureError::NoDisplay`] if there is nothing to capture from.
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError>;
}

/// A synthetic, deterministic frame source: a moving diagonal gradient
/// over a checkerboard, changing every frame. No OS dependency at all —
/// exists specifically to exercise capture -> encode -> wire -> decode
/// without needing real screen content, matching the build plan's own
/// staged order.
pub struct TestPatternSource {
    width: u32,
    height: u32,
    frame_index: u64,
}

impl TestPatternSource {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, frame_index: 0 }
    }
}

impl FrameSource for TestPatternSource {
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let width = self.width;
        let height = self.height;
        let stride = width * 4;
        let mut bgra = vec![0u8; (stride * height) as usize];

        // A moving diagonal band (position derived from frame_index) over
        // a fixed 32px checkerboard -- visibly animated, and cheap enough
        // to compute per-pixel without needing any image/drawing crate.
        let offset = (self.frame_index * 4) % u64::from(width + height);
        for y in 0..height {
            for x in 0..width {
                let checker = ((x / 32) + (y / 32)) % 2 == 0;
                let band = u64::from(x + y).abs_diff(offset) < 16;
                let (r, g, b) = if band {
                    (255u8, 255u8, 255u8)
                } else if checker {
                    (60u8, 90u8, 160u8)
                } else {
                    (20u8, 30u8, 50u8)
                };
                let px = (y * stride + x * 4) as usize;
                bgra[px] = b;
                bgra[px + 1] = g;
                bgra[px + 2] = r;
                bgra[px + 3] = 255;
            }
        }

        self.frame_index += 1;
        Ok(CapturedFrame { width, height, stride, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_frames_of_the_requested_size() {
        let mut source = TestPatternSource::new(64, 48);
        let frame = source.capture_frame().unwrap();
        assert_eq!(frame.width, 64);
        assert_eq!(frame.height, 48);
        assert_eq!(frame.stride, 64 * 4);
        assert_eq!(frame.bgra.len(), (64 * 4 * 48) as usize);
    }

    #[test]
    fn consecutive_frames_actually_change() {
        // A static test source would be a bug that's easy not to notice --
        // encoder delta frames from an unchanging input can look
        // deceptively "correct" while proving nothing about a live stream.
        let mut source = TestPatternSource::new(64, 64);
        let first = source.capture_frame().unwrap();
        let second = source.capture_frame().unwrap();
        assert_ne!(first.bgra, second.bgra);
    }

    #[test]
    fn every_pixel_is_fully_opaque() {
        let mut source = TestPatternSource::new(16, 16);
        let frame = source.capture_frame().unwrap();
        for alpha in frame.bgra.chunks_exact(4).map(|px| px[3]) {
            assert_eq!(alpha, 255);
        }
    }
}
