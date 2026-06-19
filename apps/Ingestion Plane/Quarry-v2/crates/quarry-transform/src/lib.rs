//! quarry-transform — HTML → markdown/links/metadata, fingerprint, diff, chunk.
//!
//! Donor: `internal/transform/` + `internal/scraper/article_extractor.go`.

pub mod attributes;
pub mod branding;
pub mod branding_rendered;
pub mod charset;
pub mod chunk;
pub mod determinism;
pub mod diff;
pub mod docx;
pub mod fingerprint;
pub mod images;
pub mod json_ld;
pub mod lang;
pub mod links;
pub mod markdown;
pub mod metadata;
pub mod pdf;
pub mod readability;
pub mod robots;
pub mod sitemap;
pub mod soft_404;
pub mod source_trace;
pub mod synonyms;

pub use determinism::{verify_deterministic, DeterminismError};
pub use fingerprint::{content_fingerprint, Fingerprint};
pub use metadata::{sidecar_path, write_sidecar, ArtifactMeta};
pub use readability::{extract as extract_readable, html_to_readable_markdown, Readable};
