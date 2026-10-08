//! The Data Plane's client for inference-core.
//!
//! Three Data Plane services call inference-core on behalf of an organization and
//! used to carry three copies of the same code: the Auth Core service-token mint,
//! and a hand-rolled reading of the status a refused call comes back with. They
//! drifted. This crate is the one copy.
//!
//! * [`InferenceTokenClient`] mints, validates and caches the bearer. It knows
//!   the three retention postures (`persistent`, `zdr`, and `tenant`, where the
//!   token carries the organization's own ZDR setting), keeps a bearer per
//!   `(organization, reason)`, shares one mint among concurrent callers, and
//!   turns Auth Core's "I cannot resolve this organization's posture" into a
//!   typed wait.
//! * [`InferenceError`] decodes a failed call from the gRPC code and inference-core's
//!   `x-mp-provider-error` trailer into the closed [`Failure`] set, with the
//!   [`Disposition`] each one means. It never reads the message.
//!
//! What the crate is tested against, and why none of it is typed by hand: the
//! token golden is what Auth Core's real mint, registry rows and tenant resolver
//! produced (`tests/token_contract.rs`, `tests/real_policy.rs`), and the status
//! golden is what inference-core's code emits (`tests/status_contract.rs`). Both
//! are vendored byte for byte, with a sha256 pin, and the repo-level drift check
//! `scripts/tests/zdr-contract-sync.test.sh` fails when a copy differs.
//!
//! Not in this crate: the retrieval engine's own copy of the mint
//! (`retrieval-engine-rs/src/embed/service_auth.rs`) and anything that builds a
//! gRPC request. The crate has no dependency on the Model Plane protocol.

mod clock;
mod status;
mod token;

pub use clock::{Clock, SystemClock};
pub use status::{
    Disposition, Failure, InferenceError, Stage, KIND_RATE_LIMITED, KIND_RESIDENCY_UNAVAILABLE,
    KIND_SUBSCRIPTION_USAGE_LIMIT, KIND_TIER_UNAVAILABLE, KIND_TOO_LONG, KIND_TRAILER,
    KIND_ZDR_UNAVAILABLE, PENDING_RETRY_AFTER, RETRY_AFTER_MS_TRAILER, SKIP_RECHECK_AFTER,
};
pub use token::{
    refresh_margin, Bearer, ConfigError, InferenceTokenClient, MintError, Reason, RetentionPosture,
    TokenClientConfig, TokenStats, INFERENCE_AUDIENCE, INFERENCE_SCOPE, MAX_REFRESH_MARGIN,
    MAX_TOKEN_TTL_SECONDS, TENANT_MAX_REUSE, TENANT_TOKEN_TTL_SECONDS,
};
