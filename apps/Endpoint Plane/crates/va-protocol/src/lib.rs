//! RustDesk-compatible host-side protocol: message catalog and the
//! per-connection handshake/login/session state machine.
//!
//! Error variants throughout are self-explanatory from their own
//! `#[error(...)]` messages; per-function `# Errors` sections would be
//! pure repetition across ~20 small encode/decode functions, so that
//! pedantic lint is silenced crate-wide (matching `va-wire`'s precedent)
//! rather than hand-written per function.
#![allow(clippy::missing_errors_doc)]

pub mod handshake;
pub mod messages;

pub use handshake::{Credentials, Event, HostIdentity, HostInfo, HostSession, ProtocolError};
pub use messages::*;
