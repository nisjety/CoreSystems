//! quarry-transform — HTML → markdown/links/metadata, fingerprint, diff, chunk.
//!
//! Donor: `internal/transform/` + `internal/scraper/article_extractor.go`.

pub mod attributes;
pub mod branding;
pub mod branding_rendered;
pub mod chunk;
pub mod determinism;
pub mod diff;
pub mod fingerprint;
pub mod images;
pub mod links;
pub mod markdown;
pub mod metadata;
pub mod readability;
pub mod robots;
pub mod sitemap;
pub mod source_trace;

pub use determinism::{verify_deterministic, DeterminismError};
pub use fingerprint::{content_fingerprint, Fingerprint};
pub use metadata::{sidecar_path, write_sidecar, ArtifactMeta};
pub use readability::{extract as extract_readable, html_to_readable_markdown, Readable};
