//! Real screen capture via `windows-capture` (Windows Graphics Capture +
//! DXGI under the hood). Bridges its callback-based API to this crate's
//! synchronous, pull-based [`FrameSource`] trait.
//!
//! API surface verified directly against the cached `windows-capture 1.5.0`
//! source (not docs.rs, not assumed) -- exact file+line citations:
//! - `Monitor::primary()` (`monitor.rs:74`) picks the OS-designated primary
//!   monitor; multi-monitor selection is a known future gap (`Monitor::
//!   enumerate`/`from_index`, `monitor.rs:406,96`), not addressed here.
//! - `Settings::new` (`settings.rs:133`) takes eight positional arguments in
//!   a fixed order; every one except `color_format` is left at `Default`
//!   deliberately -- `GraphicsCaptureApi::new` gates cursor/border/
//!   secondary-window/min-update-interval/dirty-region settings behind
//!   per-property `ApiInformation::IsPropertyPresent` checks
//!   (`graphics_capture_api.rs:161-189,356-424`) that hard-fail
//!   `Settings`/`start_free_threaded` construction on an OS build lacking
//!   that specific property; only `ColorFormat` has no such gate.
//! - `ColorFormat::Bgra8` (`settings.rs:32`, `DXGI_FORMAT_B8G8R8A8_UNORM`)
//!   is exactly `CapturedFrame`'s documented layout -- no conversion step
//!   needed. The crate's own default is `Rgba8` (`settings.rs:38-40`, the
//!   wrong channel order), so this must be requested explicitly.
//! - `GraphicsCaptureApiHandler::start_free_threaded` (`capture.rs:368`) IS
//!   the dedicated OS thread (spawns it internally, `capture.rs:378`, and
//!   initializes COM/WinRT on it, `capture.rs:381-388`) -- wrapping it in
//!   another `std::thread::spawn` would spawn a thread inside a thread for
//!   no reason. It briefly blocks the calling thread doing real GPU/COM
//!   setup (an `mpsc::recv`, `capture.rs:513-529`), so construction happens
//!   via `tokio::task::spawn_blocking` at the one call site that needs it
//!   (`main.rs`), not inside this crate.
//! - `FrameBuffer::{width, height, row_pitch, as_raw_buffer}` (`frame.rs:
//!   441-479`) hand back the raw, row-pitch-padded bytes directly --
//!   deliberately NOT `as_nopadding_buffer` (`frame.rs:489-521`, an extra
//!   rayon-parallel copy to strip padding this crate doesn't need to
//!   strip, since `CapturedFrame.stride` is documented to allow
//!   `stride != width * 4` already).
//! - `CaptureControl::stop` (`capture.rs:143-179`) sets a halt flag, posts
//!   `WM_QUIT` to the capture thread, then **joins it** -- a real blocking
//!   call. Callers must not run this on a thread that cannot afford to
//!   block (a tokio worker mid-poll); see `main.rs`'s cooperative shutdown
//!   for how the one real caller here avoids that.
//! - No consent dialog, but WGC needs a real interactive desktop/window
//!   station (`D3D11CreateDevice`, `d3d11.rs:67-79`) -- will not work from
//!   a headless Session-0 service, consistent with how `verevon-agent`
//!   already only ever runs as an interactive user-session process.
//! - WGC only fires `on_frame_arrived` when the screen actually changes
//!   (`README.md:16`) -- a quiet capture thread over a static desktop is
//!   normal, not a hang. `capture_frame` therefore re-serves the latest
//!   observed frame on every call rather than blocking for a genuinely new
//!   one, decoupling this crate's pull rate from WGC's push rate entirely.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::{CaptureError, CapturedFrame, FrameSource};

/// How long [`DxgiDuplicationSource::capture_frame`] will wait for the
/// FIRST frame before giving up. Every call after that is instant (it
/// re-serves whatever's already in the slot) -- this bound only matters
/// during the capture session's startup window.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(2);

/// Single-slot mailbox: the WGC callback thread (writer) overwrites it with
/// the latest observed frame; `capture_frame` (reader) only ever peeks the
/// latest value, never blocking past the very first call.
struct SharedFrame {
    slot: Mutex<Option<CapturedFrame>>,
    ready: Condvar,
    closed: AtomicBool,
}

struct DuplicationHandler {
    shared: Arc<SharedFrame>,
}

impl GraphicsCaptureApiHandler for DuplicationHandler {
    type Flags = Arc<SharedFrame>;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { shared: ctx.flags })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let mut buffer = frame.buffer().map_err(|_| CaptureError::NoDisplay)?;
        let width = buffer.width();
        let height = buffer.height();
        let stride = buffer.row_pitch();
        let bgra = buffer.as_raw_buffer().to_vec();
        *self.shared.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(CapturedFrame { width, height, stride, bgra });
        self.shared.ready.notify_one();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.shared.closed.store(true, Ordering::Relaxed);
        self.shared.ready.notify_one();
        Ok(())
    }
}

/// Real screen capture of the OS-designated primary monitor.
pub struct DxgiDuplicationSource {
    shared: Arc<SharedFrame>,
    control: Option<CaptureControl<DuplicationHandler, CaptureError>>,
}

impl DxgiDuplicationSource {
    /// Starts a capture session for the primary monitor. Does real GPU/COM
    /// setup and briefly blocks the calling thread (see this module's doc
    /// comment) -- call via `tokio::task::spawn_blocking` from an async
    /// context, never directly in an async fn.
    ///
    /// # Errors
    ///
    /// [`CaptureError::NoDisplay`] if there is no primary monitor, or WGC
    /// session setup fails (unsupported OS build, no interactive desktop
    /// session, DirectX device creation failure).
    pub fn new_for_primary_monitor() -> Result<Self, CaptureError> {
        let monitor = Monitor::primary().map_err(|_| CaptureError::NoDisplay)?;
        let shared = Arc::new(SharedFrame {
            slot: Mutex::new(None),
            ready: Condvar::new(),
            closed: AtomicBool::new(false),
        });

        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            shared.clone(),
        );

        let control =
            DuplicationHandler::start_free_threaded(settings).map_err(|_| CaptureError::NoDisplay)?;

        Ok(Self { shared, control: Some(control) })
    }
}

impl FrameSource for DxgiDuplicationSource {
    fn capture_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let guard = self.shared.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut guard = if guard.is_some() {
            guard
        } else {
            if self.shared.closed.load(Ordering::Relaxed) {
                return Err(CaptureError::NoDisplay);
            }
            let (guard, timeout) = self
                .shared
                .ready
                .wait_timeout_while(guard, FIRST_FRAME_TIMEOUT, |frame| frame.is_none())
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if timeout.timed_out() && guard.is_none() {
                return Err(CaptureError::NotReady);
            }
            guard
        };
        guard.take().map_or(Err(CaptureError::NotReady), |frame| {
            // Put it right back -- this is a peek, not a take: the same
            // frame is re-served on the next call until WGC overwrites it
            // with a genuinely new one.
            let result = frame.clone();
            *guard = Some(frame);
            Ok(result)
        })
    }
}

impl Drop for DxgiDuplicationSource {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            if let Err(err) = control.stop() {
                tracing::warn!(?err, "windows-capture session did not stop cleanly");
            }
        }
    }
}
