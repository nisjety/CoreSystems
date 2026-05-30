//! Entity-extraction hooks for the chunker (D4+D5 spec §6).
//!
//! `markdown` is the only language wired in v2.3; code (.rs/.go/.ts/.py/.java)
//! will follow once tree-sitter integration lands. The contract here is the
//! [`EntityProposal`] shape — adding a new source type means a new submodule
//! that emits the same struct, then the orchestrator's graph-build job picks
//! it up via the existing NATS event.

pub mod markdown;
#[allow(unused_imports)] // re-export kept for the future chunker wiring
pub use markdown::{extract_markdown_entities, EntityProposal};
