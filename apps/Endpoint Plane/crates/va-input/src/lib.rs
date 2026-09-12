//! Turns an already-decoded `va_protocol::MouseEvent`/`KeyEvent` into a
//! real OS input event, via `enigo` on Windows (its public API is fully
//! safe -- see this crate's `Cargo.toml` for why that's compatible with
//! this workspace's `unsafe_code = "forbid"` lint).
//!
//! This crate has NO permission model and NO session awareness -- it is a
//! pure "given a decoded event, do it" executor, matching `va-capture`/
//! `va-encode`'s own single-concern scope. Whether an event should even
//! reach this crate (login state, a real consent/permission flow) is
//! entirely the caller's job; `va-protocol`'s own `handshake` module docs
//! already flag that a real product needs one and this pass doesn't build
//! it.
//!
//! `remote-core` (the browser controller this is built to interoperate
//! with) only ever sends `KeyEvent.mode = Legacy` (named `ControlKey`s and
//! Unicode codepoints, never `Map`-mode positional scancodes) and only
//! ever sets one bit of `MouseEvent`'s button flags at a time -- this
//! crate's mapping tables and `handle_mouse_event`/`handle_key_event`
//! reflect exactly that, not `RustDesk`'s full protocol surface.
//!
//! Windows-only, matching this project's current scope (see the build
//! plan's "genuinely still not built" list) -- `enigo` is only a
//! dependency `cfg(windows)` (its `Cargo.toml` doc comment explains why),
//! so this whole crate compiles to nothing on any other target rather
//! than half-compiling into a crate with no real implementation.
#![cfg(windows)]

use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use thiserror::Error;
use va_protocol::{KeyEvent, KeyIdentification, MouseButtonFlags, MouseEvent, MouseEventKind};

#[derive(Debug, Error)]
pub enum InputError {
    #[error("no sensible Windows equivalent exists for this input: {0}")]
    Unsupported(&'static str),
    #[error("no ControlKey mapping exists for wire value {0} -- not seen on a real wire capture yet")]
    UnsupportedControlKey(u32),
    #[error("the input backend failed: {0}")]
    Backend(String),
}

/// `ControlKey`'s wire value (see `va_protocol::messages`'s doc comment for
/// the full enum) -> enigo's own `Key`. Built directly from enigo's
/// `TryFrom<Key> for VIRTUAL_KEY` Windows implementation, not guessed.
/// Values with no sensible Windows equivalent (`CtrlAltDel`, `Power`,
/// `LockScreen`) return `Unsupported` rather than silently no-op-ing or
/// picking an unrelated key.
fn control_key_to_enigo(code: u32) -> Result<enigo::Key, InputError> {
    use enigo::Key;
    Ok(match code {
        1 | 24 => Key::Alt, // Alt, Option (no distinct Windows key for Option)
        2 => Key::Backspace,
        3 => Key::CapsLock,
        4 => Key::Control,
        5 => Key::Delete,
        6 => Key::DownArrow,
        7 => Key::End,
        8 => Key::Escape,
        9 => Key::F1,
        10 => Key::F10,
        11 => Key::F11,
        12 => Key::F12,
        13 => Key::F2,
        14 => Key::F3,
        15 => Key::F4,
        16 => Key::F5,
        17 => Key::F6,
        18 => Key::F7,
        19 => Key::F8,
        20 => Key::F9,
        21 => Key::Home,
        22 => Key::LeftArrow,
        23 => Key::Meta,
        25 => Key::PageDown,
        26 => Key::PageUp,
        // enigo's `Key` has no separate numpad-enter variant (it's
        // VK_RETURN either way, distinguished only by an extended-key
        // scancode bit enigo doesn't expose) -- 72 (NumpadEnter) is the
        // same key.
        27 | 72 => Key::Return,
        28 => Key::RightArrow,
        29 => Key::Shift,
        30 => Key::Space,
        31 => Key::Tab,
        32 => Key::UpArrow,
        33 => Key::Numpad0,
        34 => Key::Numpad1,
        35 => Key::Numpad2,
        36 => Key::Numpad3,
        37 => Key::Numpad4,
        38 => Key::Numpad5,
        39 => Key::Numpad6,
        40 => Key::Numpad7,
        41 => Key::Numpad8,
        42 => Key::Numpad9,
        43 => Key::Cancel,
        44 => Key::Clear,
        46 => Key::Pause,
        58 => Key::Insert,
        59 => Key::Help,
        60 => Key::Sleep,
        61 => Key::Separator,
        62 => Key::Scroll,
        63 => Key::Numlock,
        64 => Key::RWin,
        // 65 (Apps, the real context-menu key) and 45 (Menu -- a
        // legacy/X11-derived duplicate `remote-core` never actually emits)
        // collapse onto the same enigo key. 45 isn't independently
        // confirmed against a real wire capture, so this is a best-effort
        // guess pending live testing, not a verified mapping like the rest
        // of this table.
        45 | 65 => Key::Apps,
        66 => Key::Multiply,
        67 => Key::Add,
        68 => Key::Subtract,
        69 => Key::Decimal,
        70 => Key::Divide,
        // No portable `Key::Equals` in enigo (only layout-specific
        // `OEMPlus`) -- routing through `Unicode('=')` is layout-aware
        // (enigo resolves it via `VkKeyScanExW`) instead of a named key.
        71 => Key::Unicode('='),
        73 => Key::RShift,
        74 => Key::RControl,
        75 => Key::RMenu,
        76 => Key::VolumeMute,
        77 => Key::VolumeUp,
        78 => Key::VolumeDown,
        // Windows blocks synthetic Ctrl+Alt+Del (the Secure Attention
        // Sequence) from SendInput unconditionally, by design -- only
        // `SendSAS()` from a process opted into `EnableGenericSAS` policy
        // can do it, a mechanism enigo doesn't touch and this crate
        // doesn't implement.
        100 => return Err(InputError::Unsupported("CtrlAltDel (blocked by Windows SAS)")),
        // No key-event equivalent; the correct call is
        // `LockWorkStation()`, a plain function call, not a SendInput key
        // -- out of scope for this trait, which only injects key/mouse
        // events.
        101 => return Err(InputError::Unsupported("LockScreen (needs LockWorkStation, not a key event)")),
        // `Key::Power` in enigo is macOS-only; there's no Windows
        // key-event equivalent either way (real power control needs
        // `InitiateSystemShutdown`/`SetSuspendState`, not `SendInput`).
        79 => return Err(InputError::Unsupported("Power (no Windows key-event equivalent)")),
        other => return Err(InputError::UnsupportedControlKey(other)),
    })
}

/// Injects already-decoded `va_protocol` input events via `enigo`.
pub struct EnigoInjector {
    enigo: Enigo,
    /// The real primary display's own size in pixels, queried once at
    /// construction via `enigo`'s own `main_display()` -- NOT the same as
    /// `reported_size` below. A display resolution change mid-session
    /// (e.g. an RDP client reconnecting at a different size) is a known,
    /// unhandled limitation: this is queried once, not re-queried per
    /// event.
    real_display_size: (i32, i32),
    /// The resolution `DisplayInfo`/the encoded video actually reports to
    /// the controller (see `main.rs`'s `WIDTH`/`HEIGHT`) -- virtually never
    /// the display's real resolution, since the encoder runs at a fixed,
    /// measured-real-time size regardless of what's actually being
    /// captured (`va-encode`'s own resize step). Incoming `MouseEvent`
    /// x/y are expressed in THIS space, matching what the controller's own
    /// canvas is actually showing, and must be rescaled up to
    /// `real_display_size` before reaching `enigo` -- passing them through
    /// unscaled would land every click in the wrong place as soon as the
    /// two resolutions differ (which, in practice, is always).
    reported_size: (u32, u32),
}

impl EnigoInjector {
    /// `reported_width`/`reported_height` must match whatever
    /// `DisplayInfo` this host actually advertised at login (`main.rs`'s
    /// `WIDTH`/`HEIGHT`) -- see this struct's docs for why.
    ///
    /// # Errors
    ///
    /// [`InputError::Backend`] if `enigo` fails to initialize (e.g. no
    /// interactive desktop session is available) or its primary-display
    /// query fails.
    pub fn new(reported_width: u32, reported_height: u32) -> Result<Self, InputError> {
        let enigo = Enigo::new(&Settings::default()).map_err(backend_err)?;
        let real_display_size = enigo.main_display().map_err(backend_err)?;
        Ok(Self { enigo, real_display_size, reported_size: (reported_width, reported_height) })
    }

    /// Rescales a coordinate (position OR delta -- both scale the same way)
    /// from `reported_size` space to `real_display_size` space.
    fn scale(&self, x: i32, y: i32) -> (i32, i32) {
        scale_coordinate(self.real_display_size, self.reported_size, x, y)
    }

    /// # Errors
    ///
    /// [`InputError::Backend`] if the underlying `enigo` call fails.
    pub fn handle_mouse_event(&mut self, event: &MouseEvent) -> Result<(), InputError> {
        match event.kind {
            MouseEventKind::Move => self.move_to(event.x, event.y, Coordinate::Abs),
            MouseEventKind::MoveRelative => self.move_to(event.x, event.y, Coordinate::Rel),
            MouseEventKind::Down | MouseEventKind::Up => {
                // Coordinates travel with every click, not just moves --
                // move first so the click lands where the controller
                // actually pointed, rather than trusting a prior Move to
                // have already placed the cursor.
                self.move_to(event.x, event.y, Coordinate::Abs)?;
                let Some(button) = button_from_flags(event.button_flags) else {
                    tracing::warn!(?event.button_flags, "mouse Down/Up with no recognized button flag, ignoring");
                    return Ok(());
                };
                let direction =
                    if event.kind == MouseEventKind::Down { Direction::Press } else { Direction::Release };
                self.enigo.button(button, direction).map_err(backend_err)
            }
            // `x`/`y` carry horizontal/vertical scroll DELTA here, not a
            // position -- confirmed against `RustDeskProtocol.ts`'s own
            // `sendMouse(MouseEventKind.Wheel, 0, deltaX, deltaY)` call.
            // Units aren't defined by the wire protocol itself (the host
            // is expected to scale/invert per-platform); `enigo::scroll`'s
            // own `WHEEL_DELTA=120` internal multiplier hasn't been
            // calibrated against a real trackpad/mouse yet -- flagged as
            // an open follow-up, not solvable from source alone.
            MouseEventKind::Wheel => {
                if event.x != 0 {
                    self.enigo.scroll(event.x, Axis::Horizontal).map_err(backend_err)?;
                }
                if event.y != 0 {
                    self.enigo.scroll(event.y, Axis::Vertical).map_err(backend_err)?;
                }
                Ok(())
            }
            MouseEventKind::Trackpad | MouseEventKind::Unknown(_) => {
                tracing::debug!(?event.kind, "unsupported MouseEvent kind, ignoring");
                Ok(())
            }
        }
    }

    /// # Errors
    ///
    /// [`InputError::Backend`] if the underlying `enigo` call fails, or
    /// [`InputError::Unsupported`] for an identification this host has no
    /// sensible way to inject (an unmapped `ControlKey`, or an empty
    /// oneof).
    pub fn handle_key_event(&mut self, event: &KeyEvent) -> Result<(), InputError> {
        let direction = if event.down { Direction::Press } else { Direction::Release };
        match &event.identification {
            KeyIdentification::ControlKey(code) => {
                let key = control_key_to_enigo(*code)?;
                self.enigo.key(key, direction).map_err(backend_err)
            }
            KeyIdentification::Chr(code) => {
                // `remote-core` never sends this today (Unicode codepoints
                // cover it); handled for proto3 forward compatibility via
                // the same Unicode path.
                let key = enigo::Key::Unicode(char::from_u32(*code).unwrap_or('\u{FFFD}'));
                self.enigo.key(key, direction).map_err(backend_err)
            }
            KeyIdentification::Unicode(codepoint) => {
                let key = enigo::Key::Unicode(char::from_u32(*codepoint).unwrap_or('\u{FFFD}'));
                self.enigo.key(key, direction).map_err(backend_err)
            }
            // Bulk composed text (IME, paste, `keyboard.type()`) -- sent
            // once as a single down-only event with no matching release
            // (`KeyboardMapper.ts`'s `mapTextToIdentification`), so this
            // ignores `event.down`/`direction` entirely and just types it.
            KeyIdentification::Seq(text) => self.enigo.text(text).map_err(backend_err),
            KeyIdentification::None => Err(InputError::Unsupported("KeyEvent with no identification set")),
        }
    }

    fn move_to(&mut self, x: i32, y: i32, coordinate: Coordinate) -> Result<(), InputError> {
        let (x, y) = self.scale(x, y);
        self.enigo.move_mouse(x, y, coordinate).map_err(backend_err)
    }
}

/// Rescales `(x, y)` from `reported_size` space (whatever `DisplayInfo`/the
/// encoded video actually reports) to `real_size` space (the real primary
/// display's own pixel dimensions). A free function, not a method, so it's
/// testable without constructing a real `EnigoInjector` (which touches real
/// OS input hooks).
fn scale_coordinate(real_size: (i32, i32), reported_size: (u32, u32), x: i32, y: i32) -> (i32, i32) {
    let (real_w, real_h) = real_size;
    let (reported_w, reported_h) = reported_size;
    let scaled_x = if reported_w == 0 { x } else { x * real_w / i32::try_from(reported_w).unwrap_or(i32::MAX) };
    let scaled_y = if reported_h == 0 { y } else { y * real_h / i32::try_from(reported_h).unwrap_or(i32::MAX) };
    (scaled_x, scaled_y)
}

fn button_from_flags(flags: MouseButtonFlags) -> Option<Button> {
    if flags.contains(MouseButtonFlags::LEFT) {
        Some(Button::Left)
    } else if flags.contains(MouseButtonFlags::RIGHT) {
        Some(Button::Right)
    } else if flags.contains(MouseButtonFlags::WHEEL) {
        Some(Button::Middle)
    } else if flags.contains(MouseButtonFlags::BACK) {
        Some(Button::Back)
    } else if flags.contains(MouseButtonFlags::FORWARD) {
        Some(Button::Forward)
    } else {
        None
    }
}

fn backend_err(err: impl std::fmt::Display) -> InputError {
    InputError::Backend(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests exercise only the pure mapping functions below --
    // `EnigoInjector` itself is deliberately never constructed here.
    // `Enigo::new` installs real OS-level input hooks and its methods
    // move the real mouse/inject real keystrokes on whatever machine runs
    // the test suite; that's what live testing (a real connected session)
    // is for, not `cargo test`.

    #[test]
    fn control_key_maps_named_keys_to_the_expected_enigo_key() {
        assert_eq!(control_key_to_enigo(27).unwrap(), enigo::Key::Return);
        assert_eq!(control_key_to_enigo(8).unwrap(), enigo::Key::Escape);
        assert_eq!(control_key_to_enigo(29).unwrap(), enigo::Key::Shift);
    }

    #[test]
    fn control_key_option_collapses_onto_alt() {
        // ControlKey::Alt = 1, ControlKey::Option = 24 -- enigo has no
        // distinct Windows key for Option.
        assert_eq!(control_key_to_enigo(1).unwrap(), control_key_to_enigo(24).unwrap());
    }

    #[test]
    fn control_key_numpad_enter_collapses_onto_return() {
        assert_eq!(control_key_to_enigo(72).unwrap(), control_key_to_enigo(27).unwrap());
    }

    #[test]
    fn control_key_equals_routes_through_layout_aware_unicode() {
        assert_eq!(control_key_to_enigo(71).unwrap(), enigo::Key::Unicode('='));
    }

    #[test]
    fn control_key_ctrl_alt_del_power_and_lock_screen_are_unsupported() {
        assert!(matches!(control_key_to_enigo(100), Err(InputError::Unsupported(_))), "CtrlAltDel");
        assert!(matches!(control_key_to_enigo(79), Err(InputError::Unsupported(_))), "Power");
        assert!(matches!(control_key_to_enigo(101), Err(InputError::Unsupported(_))), "LockScreen");
    }

    #[test]
    fn control_key_unrecognized_value_is_forward_compatible_not_a_panic() {
        assert!(matches!(control_key_to_enigo(9999), Err(InputError::UnsupportedControlKey(9999))));
    }

    #[test]
    fn button_from_flags_maps_each_single_bit() {
        assert_eq!(button_from_flags(MouseButtonFlags(MouseButtonFlags::LEFT)), Some(Button::Left));
        assert_eq!(button_from_flags(MouseButtonFlags(MouseButtonFlags::RIGHT)), Some(Button::Right));
        assert_eq!(button_from_flags(MouseButtonFlags(MouseButtonFlags::WHEEL)), Some(Button::Middle));
        assert_eq!(button_from_flags(MouseButtonFlags(MouseButtonFlags::BACK)), Some(Button::Back));
        assert_eq!(button_from_flags(MouseButtonFlags(MouseButtonFlags::FORWARD)), Some(Button::Forward));
    }

    #[test]
    fn button_from_flags_none_when_no_bit_is_set() {
        assert_eq!(button_from_flags(MouseButtonFlags(0)), None);
    }

    #[test]
    fn scale_coordinate_maps_reported_space_to_real_display_space() {
        // A 480x270-reported click dead center should land dead center of
        // a real 1920x1080 display -- the exact mismatch real DXGI capture
        // (native resolution) plus a fixed-size encoder (480x270) creates.
        assert_eq!(scale_coordinate((1920, 1080), (480, 270), 240, 135), (960, 540));
    }

    #[test]
    fn scale_coordinate_is_identity_when_reported_matches_real() {
        assert_eq!(scale_coordinate((1920, 1080), (1920, 1080), 500, 300), (500, 300));
    }

    #[test]
    fn scale_coordinate_does_not_divide_by_zero_for_a_degenerate_reported_size() {
        assert_eq!(scale_coordinate((1920, 1080), (0, 0), 10, 10), (10, 10));
    }
}
