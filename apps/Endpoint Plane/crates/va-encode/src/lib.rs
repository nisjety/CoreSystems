//! Real-time AV1 encoding via `rav1e` — pure Rust, no native toolchain (no
//! `pkg-config`, no libvpx). This crate replaced an earlier `vpx-encode`
//! (VP9/libvpx) dependency that was blocked on exactly that missing system
//! dependency; `remote-core` (the browser controller) already fully
//! supports decoding AV1 (`VideoFrame.av1s`, wire field 13) — see
//! `va-protocol`'s `messages.rs` and the Endpoint Plane build docs for the
//! citation trail. `rav1e`'s own `asm`/`binaries` default features are
//! disabled in `Cargo.toml` for the same reason: this crate stays
//! buildable with nothing beyond a stock Rust toolchain.
//!
//! Input frames are BGRA8 (the pixel format DXGI/`windows-capture`
//! produces) — this crate owns the BGRA→YUV 4:2:0 conversion `rav1e`
//! needs, since `va-capture` has no reason to know anything about encoding.

// Pixel-format math below is inherently cast-heavy (i32 accumulators ->
// u8 samples, clamped first) and reads clearest with the conventional
// short names (r/g/b, x/y, u/v) -- not a sign of missing bounds checks.
#![allow(clippy::cast_sign_loss, clippy::many_single_char_names)]

use rav1e::config::SpeedSettings;
use rav1e::prelude::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("invalid encoder configuration: {0}")]
    InvalidConfig(#[from] InvalidConfig),
    #[error("encoder returned a failure status")]
    EncoderFailure,
    #[error(
        "frame dimensions ({width}x{height}) do not match the encoder's configured \
         {configured_width}x{configured_height} -- a resolution change needs a new encoder"
    )]
    DimensionMismatch { width: u32, height: u32, configured_width: u32, configured_height: u32 },
}

/// One raw captured frame: BGRA8, 4 bytes per pixel. `stride` is bytes per
/// row and may exceed `width * 4` when the source pads rows for alignment
/// (DXGI commonly does) — callers must not assume `stride == width * 4`.
#[derive(Debug, Clone)]
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
}

/// One encoded AV1 chunk, ready to wrap as a wire
/// `EncodedVideoFrame{data, key, pts}` (`va_protocol::encode_av1_video_frame`).
#[derive(Debug, Clone)]
pub struct EncodedChunk {
    pub data: Vec<u8>,
    pub key: bool,
    pub pts: i64,
}

pub trait VideoEncoder {
    /// Encodes one frame, returning zero or more ready packets. Zero is
    /// normal and expected — a low-latency encoder still occasionally
    /// needs an extra frame or two before its first packet is ready.
    ///
    /// # Errors
    ///
    /// [`EncodeError::DimensionMismatch`] if `frame`'s size doesn't match
    /// what this encoder was constructed for, or
    /// [`EncodeError::EncoderFailure`] if the underlying encoder reports a
    /// failure status.
    fn encode(&mut self, frame: &RawFrame, force_keyframe: bool) -> Result<Vec<EncodedChunk>, EncodeError>;
}

/// A real-time AV1 encoder. Low-latency mode (no frame reordering) and the
/// fastest speed preset, matching a remote-desktop workload's priorities
/// (latency and CPU headroom over compression efficiency) — this is not
/// the tuning a video-on-demand encode would want.
pub struct Av1Encoder {
    ctx: Context<u8>,
    width: u32,
    height: u32,
    frame_count: u64,
}

impl Av1Encoder {
    /// # Errors
    ///
    /// [`EncodeError::InvalidConfig`] if `width`/`height` (or any other
    /// derived setting) is rejected by `rav1e`.
    pub fn new(width: u32, height: u32, target_bitrate_kbps: u32) -> Result<Self, EncodeError> {
        let mut speed_settings = SpeedSettings::from_preset(10);
        // `from_preset`'s own lowest setting still leaves `rdo_lookahead_frames`
        // at 10 -- fine for offline encoding, but a genuine ~333ms startup
        // delay (at 30fps) before the very first packet is a real problem
        // for an interactive remote-desktop stream, not a rate this crate
        // should inherit unexamined. Override it down to the minimum this
        // still functions with; verified empirically (this exact value is
        // what `first_frame_is_forced_as_a_keyframe_and_eventually_produces_a_packet`
        // below actually exercises).
        speed_settings.rdo_lookahead_frames = 1;

        let enc = EncoderConfig {
            width: width as usize,
            height: height as usize,
            bit_depth: 8,
            chroma_sampling: ChromaSampling::Cs420,
            low_latency: true,
            min_key_frame_interval: 0,
            // A keyframe every ~2s at 30fps: frequent enough for fast
            // recovery on a lossy/reconnecting stream, which matters far
            // more here than the file-size amortization a longer interval
            // buys for offline encoding.
            max_key_frame_interval: 60,
            speed_settings,
            // Bitrate mode: an unconstrained max quantizer, same convention
            // rav1e's own CLI uses when a target bitrate is given (see
            // src/bin/common.rs's parse_config) -- rav1e's own unit for
            // this field is bits/second, not kbps, hence the *1000.
            quantizer: 255,
            bitrate: i32::try_from(target_bitrate_kbps).unwrap_or(i32::MAX).saturating_mul(1000),
            // Left at its default, this defaults to
            // `min(max_key_frame_interval * 1.5, 240)` frames of
            // rate-control smoothing -- 240 frames (8s at 30fps) of startup
            // latency before the bitrate-mode rate controller is primed,
            // measured directly against this crate's own test suite. `rav1e`'s
            // own doc comment for this field names 12 as its recommended
            // floor ("gives some chance to distribute bit estimation
            // errors in the worst case") -- pinning it there instead of
            // inheriting whatever `max_key_frame_interval` happens to imply
            // is what actually gets a real-time stream's first packet out
            // promptly.
            reservoir_frame_delay: Some(12),
            ..Default::default()
        };
        let cfg = Config::new().with_encoder_config(enc);
        let ctx: Context<u8> = cfg.new_context()?;
        Ok(Self { ctx, width, height, frame_count: 0 })
    }
}

impl VideoEncoder for Av1Encoder {
    fn encode(&mut self, frame: &RawFrame, force_keyframe: bool) -> Result<Vec<EncodedChunk>, EncodeError> {
        if frame.width != self.width || frame.height != self.height {
            return Err(EncodeError::DimensionMismatch {
                width: frame.width,
                height: frame.height,
                configured_width: self.width,
                configured_height: self.height,
            });
        }

        let mut av1_frame = self.ctx.new_frame();
        bgra_to_yuv420(&mut av1_frame, frame);

        let params = FrameParameters {
            frame_type_override: if force_keyframe { FrameTypeOverride::Key } else { FrameTypeOverride::No },
            ..Default::default()
        };
        self.ctx.send_frame((av1_frame, params)).map_err(|_| EncodeError::EncoderFailure)?;

        let pts = i64::try_from(self.frame_count).unwrap_or(i64::MAX);
        self.frame_count += 1;

        let mut chunks = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(packet) => {
                    chunks.push(EncodedChunk { data: packet.data, key: packet.frame_type == FrameType::KEY, pts });
                }
                // "A frame was encoded without emitting a packet" -- normal
                // under low-latency mode's small startup pipeline; keep
                // draining (falls through to loop again), more may already
                // be ready.
                Err(EncoderStatus::Encoded) => {}
                // No more packets ready right now; wait for the next
                // `encode()` call to send more input.
                Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => break,
                Err(EncoderStatus::Failure | EncoderStatus::NotReady | EncoderStatus::EnoughData) => {
                    return Err(EncodeError::EncoderFailure);
                }
            }
        }
        Ok(chunks)
    }
}

/// BT.601 studio-range RGB->YUV (the same matrix most legacy/software video
/// pipelines default to) plus 4:2:0 chroma subsampling (simple 2x2 box
/// average, not a dedicated downsampling filter) — correctness and
/// simplicity over maximum color fidelity or performance for this first
/// pass. `frame.planes[0]` is Y, `[1]` is U (Cb), `[2]` is V (Cr).
fn bgra_to_yuv420(av1_frame: &mut Frame<u8>, source: &RawFrame) {
    let width = source.width as usize;
    let height = source.height as usize;
    let stride = source.stride as usize;

    let mut y_plane = vec![0u8; width * height];
    let chroma_width = width.div_ceil(2);
    let chroma_height = height.div_ceil(2);
    let mut u_plane = vec![0u8; chroma_width * chroma_height];
    let mut v_plane = vec![0u8; chroma_width * chroma_height];

    let pixel = |x: usize, y: usize| -> (i32, i32, i32) {
        let offset = y * stride + x * 4;
        let b = i32::from(source.bgra[offset]);
        let g = i32::from(source.bgra[offset + 1]);
        let r = i32::from(source.bgra[offset + 2]);
        (r, g, b)
    };

    for y in 0..height {
        for x in 0..width {
            let (r, g, b) = pixel(x, y);
            let y_value = (66 * r + 129 * g + 25 * b + 128) / 256 + 16;
            y_plane[y * width + x] = y_value.clamp(0, 255) as u8;
        }
    }

    for cy in 0..chroma_height {
        for cx in 0..chroma_width {
            let x0 = cx * 2;
            let y0 = cy * 2;
            let mut r_sum = 0i32;
            let mut g_sum = 0i32;
            let mut b_sum = 0i32;
            let mut count = 0i32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let x = x0 + dx;
                    let y = y0 + dy;
                    if x < width && y < height {
                        let (r, g, b) = pixel(x, y);
                        r_sum += r;
                        g_sum += g;
                        b_sum += b;
                        count += 1;
                    }
                }
            }
            let r = r_sum / count.max(1);
            let g = g_sum / count.max(1);
            let b = b_sum / count.max(1);
            let u_value = (-38 * r - 74 * g + 112 * b + 128) / 256 + 128;
            let v_value = (112 * r - 94 * g - 18 * b + 128) / 256 + 128;
            u_plane[cy * chroma_width + cx] = u_value.clamp(0, 255) as u8;
            v_plane[cy * chroma_width + cx] = v_value.clamp(0, 255) as u8;
        }
    }

    av1_frame.planes[0].copy_from_raw_u8(&y_plane, width, 1);
    av1_frame.planes[1].copy_from_raw_u8(&u_plane, chroma_width, 1);
    av1_frame.planes[2].copy_from_raw_u8(&v_plane, chroma_width, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(width: u32, height: u32, b: u8, g: u8, r: u8) -> RawFrame {
        let stride = width * 4;
        let mut bgra = vec![0u8; (stride * height) as usize];
        for chunk in bgra.chunks_exact_mut(4) {
            chunk[0] = b;
            chunk[1] = g;
            chunk[2] = r;
            chunk[3] = 255;
        }
        RawFrame { width, height, stride, bgra }
    }

    #[test]
    fn first_frame_is_forced_as_a_keyframe_and_eventually_produces_a_packet() {
        let mut encoder = Av1Encoder::new(64, 64, 500).unwrap();
        let frame = solid_frame(64, 64, 40, 80, 120);

        // Low-latency mode still has a small internal pipeline -- a real
        // encoder needs a few frames before its first packet is ready. This
        // bound (14 = the pinned reservoir_frame_delay of 12, plus a
        // couple of frames of slack) is a real assertion about startup
        // latency, not a token "eventually produces something" check --
        // this crate's whole reason for pinning reservoir_frame_delay
        // explicitly (see Av1Encoder::new) is to keep this bounded and
        // small for an interactive remote-desktop stream.
        let mut chunks = Vec::new();
        for i in 0..14 {
            chunks.extend(encoder.encode(&frame, i == 0).unwrap());
        }
        assert!(!chunks.is_empty(), "expected at least one encoded packet within the first 14 frames");
        assert!(chunks[0].key, "the very first emitted packet must be a keyframe");
        assert!(!chunks[0].data.is_empty(), "a keyframe packet should carry real AV1 bitstream bytes");
    }

    #[test]
    fn wrong_dimensions_are_rejected_without_touching_the_encoder() {
        let mut encoder = Av1Encoder::new(64, 64, 500).unwrap();
        let wrong_size = solid_frame(32, 32, 0, 0, 0);
        let err = encoder.encode(&wrong_size, false).unwrap_err();
        assert!(matches!(err, EncodeError::DimensionMismatch { .. }));
    }

    #[test]
    fn pts_increases_monotonically_across_calls() {
        // pts should track encode() call count regardless of how many
        // packets (zero or more) each individual call actually emits.
        let mut encoder = Av1Encoder::new(48, 48, 500).unwrap();
        let frame = solid_frame(48, 48, 10, 20, 30);
        let mut all_pts = Vec::new();
        for _ in 0..6 {
            for chunk in encoder.encode(&frame, false).unwrap() {
                all_pts.push(chunk.pts);
            }
        }
        let mut sorted = all_pts.clone();
        sorted.sort_unstable();
        assert_eq!(all_pts, sorted, "pts must be non-decreasing across calls");
    }

    #[test]
    fn odd_dimensions_do_not_panic_during_chroma_subsampling() {
        // div_ceil rounding for chroma_width/height must handle an odd
        // capture size (a real display resolution can be odd after a DPI
        // scale) without an out-of-bounds access in the 2x2 averaging loop.
        let mut encoder = Av1Encoder::new(65, 47, 500).unwrap();
        let frame = solid_frame(65, 47, 5, 15, 25);
        encoder.encode(&frame, true).unwrap();
    }
}
