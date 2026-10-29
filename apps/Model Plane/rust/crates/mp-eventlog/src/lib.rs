//! mp-eventlog — shared domain types for the Phase-4 `EventLog` service.
//!
//! Owns three concerns every `EventLog` implementation and consumer needs:
//!   - **Filter validation** — `EventFilter` has cross-field invariants that
//!     must be checked the same way in the session-core RPC server, in any
//!     future in-process consumer, and in tests.
//!   - **Cursor codec** — `Cursor` is opaque on the wire but has a single
//!     canonical encoding. Encoding lives here so the DB layer, the RPC
//!     server, and any language port agree on the bits.
//!   - **Idempotency-key derivation** — the append path must derive the
//!     canonical idempotency key from `(producer, event_type, resource_ref,
//!     client_key)` using blake3. Same rule as `mp-events` envelope
//!     idempotency — kept separate here so `EventLog` callers can compute the
//!     key without pulling in the whole events crate.
//!
//! Design rules (shared with `mp-orchestration`, `mp-slo`):
//!   - No IO. No async. No service dependency.
//!   - Every invariant lives in a typed error, never a silent drop.
//!   - Serde JSON round-trips are part of the contract and are tested.

#![deny(missing_docs)]

pub mod cursor;
pub mod error;
pub mod filter;
pub mod idempotency;

pub use cursor::{Cursor, CursorPayload};
pub use error::{EventLogError, EventLogResult};
pub use filter::{EventFilter, EventTypeMask};
pub use idempotency::derive_idempotency_key;
