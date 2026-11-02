//! Content safety primitives shared by the Model Plane's two tool loops.
//!
//! The chat loop (model-gateway) and the Do loop (execution-core) both
//! decide what a tool result is and whether it carries an instruction. This
//! crate is the one copy of that decision: the input normaliser, the
//! deterministic injection scanner, its marker and rule list, and the trust
//! table that says where a tool's output came from.
//!
//! Classifying a new tool means one line in [`trust`]; a new marker means one
//! line in [`rules`]. Neither service gets a second copy. Consumers share
//! types and this scanner only, never dispatch code.
//!
//! A [`ScanReport`] carries flag kinds and rule ids, never the matched text,
//! so a report can be logged or published on a ZDR turn.

pub mod normalize;
pub mod rules;
pub mod scan;
pub mod trust;

pub use normalize::{fold, sanitize_for_model};
pub use scan::{scan, scan_in, scan_injection, FlagKind, ScanMode, ScanReport};
pub use trust::TrustClass;
