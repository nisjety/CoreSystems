//! Space room code in Session Core (build plan P-SC1).
//!
//! One file per feature lives under this module, so `grpc.rs` and `auth.rs`
//! (hotspots 12 and 13) only ever gain call-site hunks and registration lines.
//! New Space logic goes in `src/space/<feature>.rs`; nothing here owns a table:
//! the message, thread and run tables stay Session Core's, and every durable
//! Space write still carries a verified Control decision.
//!
//! - [`authz`]: who a message is appended as ([`authz::AppendAs`]) and the member-append gate
//!   (WS01-S3).
//! - [`append`]: the member rules inside the append transaction (WS01-S3).
//! - [`attention_outbox`]: the boot-time flag of the attention outbox (WS11-S5; the rows are written by the
//!   trigger in migration 0080).
//! - [`approvals`]: the approval card context and the derived continuation state (WS10-S1).
//! - [`read_service`]: `SpaceRoomReadService`, the room read aggregates (D3; `ListSpaceArtifacts` WS07-S9).
//! - [`events`]: the Space event log's flag sync, listener, sweeper, gate and erasure hooks
//!   (WS02-S1; the log itself is written by triggers in migration 0072).
//! - [`history`]: `SpaceHistoryService`, permission-checked message search and message address resolution
//!   (WS13-S1; the index is migration 0077).
//! - [`memory_scope`]: the memory containment resolver for shared-room threads (WS17-S1).
//! - [`revisions`]: `MessageRevisionService` (edit or delete one message), the revision fold every read applies
//!   (WS12-S4; the log itself is `message_revisions`, migration 0076).
//! - [`mentions`]: identity-bound mentions of a human post, the extended reply slot and the stored facts a
//!   wake is verified against (WS08-S2; the rows are `message_mentions`, migration 0075).
//! - [`thread_activity`]: the thread activity summary backfill and pending probe (WS15-S5; the summary itself is
//!   kept by the trigger in migration 0078).

pub(crate) mod append;
pub(crate) mod approvals;
pub(crate) mod attention_outbox;
pub(crate) mod authz;
pub(crate) mod events;
pub(crate) mod history;
pub(crate) mod memory_scope;
pub(crate) mod read_service;
pub(crate) mod revisions;
pub(crate) mod thread_activity;
pub(crate) mod mentions;
#[cfg(test)]
pub(crate) mod test_support;
