//! Space room code in Session Core (build plan P-SC1).
//!
//! One file per feature lives under this module, so `grpc.rs` and `auth.rs`
//! (hotspots 12 and 13) only ever gain call-site hunks and registration lines.
//! New Space logic goes in `src/space/<feature>.rs`; nothing here owns a table:
//! the message, thread and run tables stay Session Core's, and every durable
//! Space write still carries a verified Control decision.
//!
//! - [`authz`]: who a message is appended as ([`authz::AppendAs`]) and, in
//!   later slices, the member-append gate.
//! - [`read_service`]: the empty `SpaceRoomReadService` registration (D3).

pub(crate) mod authz;
pub(crate) mod read_service;
