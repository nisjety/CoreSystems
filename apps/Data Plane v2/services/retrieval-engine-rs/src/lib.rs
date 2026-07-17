// clippy 1.94 surfaced a pre-existing cosmetic doc lint in several untouched
// modules' doc comments. Allow it crate-wide so the ownership-gate PR's
// `clippy -D warnings` gate is green without churning unrelated files.
#![allow(clippy::doc_overindented_list_items)]

pub mod agent_config;
pub mod audit;
pub mod authz;
pub mod cache;
pub mod config;
pub mod context_pack;
pub mod context_pins;
pub mod db;
pub mod embed;
pub mod grpc;
pub mod metrics;
pub mod pipeline;
pub mod rate_limit;
pub mod redact;
pub mod search;
pub mod trace;
