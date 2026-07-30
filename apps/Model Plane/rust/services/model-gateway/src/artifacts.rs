//! Chat artifact versioning + event construction (canvas surface).
//!
//! An artifact is a first-class piece of work product the model produces
//! alongside its prose: a document it can rewrite, a code file, an HTML page to
//! preview, or a generated binary (xlsx/docx/pdf) from the code interpreter.
//! The `artifact` SSE event and the client's "Artefakter" tab already existed —
//! but the ONLY producer was image generation, so the whole surface was
//! effectively dead. This module is the shared producer for every other kind.
//!
//! ## Why versions live here
//!
//! Canvas iteration means re-emitting the SAME artifact id with a higher
//! version ("make the intro shorter" rewrites the document in place). The model
//! cannot be trusted to track that counter — it has no reliable memory of what
//! it emitted three turns ago, and a wrong version silently corrupts the
//! client's history. So the gateway owns it: the model supplies an id, and we
//! assign the version.
//!
//! The store is process-local (same posture as `plan_mode`, `approvals`, and
//! `trajectories` in [`crate::state::AppState`]). A deploy resets counters, so a
//! later update restarts at v1 — the CONTENT is still correct, only the history
//! depth is lost. That is the honest trade for not adding a table; if artifact
//! history ever needs to survive a restart, this is the seam to swap for
//! session-core persistence.

use std::sync::Arc;

use dashmap::DashMap;

use crate::sse_events::ChatEvent;

/// Artifact kinds the chat client knows how to render. Kept as an enum here
/// (rather than free-form strings at each call site) so a typo cannot silently
/// produce an artifact the UI will not display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// Source code. `title` carries the filename when known.
    Code,
    /// Markdown prose the client renders as a document.
    Document,
    /// A complete HTML document the client previews in a sandboxed iframe.
    Html,
    /// A generated spreadsheet, delivered as a `data:` URI.
    Spreadsheet,
    /// Any other generated binary (docx, pdf, …), delivered as a `data:` URI.
    File,
    /// A raster image, delivered as a `data:` URI or an https URL.
    Image,
}

impl ArtifactKind {
    /// The wire value placed in `artifact.kind`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Document => "document",
            Self::Html => "html",
            Self::Spreadsheet => "spreadsheet",
            Self::File => "file",
            Self::Image => "image",
        }
    }

    /// Parses a model-supplied kind. Unknown values are rejected rather than
    /// defaulted, so the model gets a corrective error instead of an artifact
    /// the client silently drops.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "code" => Some(Self::Code),
            // "markdown" and "text" are the phrasings a model reaches for
            // unprompted; accept them rather than failing a good-faith call.
            "document" | "markdown" | "md" | "text" => Some(Self::Document),
            "html" => Some(Self::Html),
            "spreadsheet" | "xlsx" => Some(Self::Spreadsheet),
            "file" => Some(Self::File),
            "image" => Some(Self::Image),
            _ => None,
        }
    }

    /// Kinds whose `content` is model-authored text (as opposed to a `data:`
    /// URI produced by the code interpreter). Only these may be created
    /// directly by the `create_artifact` tool.
    #[must_use]
    pub fn is_text_authored(self) -> bool {
        matches!(self, Self::Code | Self::Document | Self::Html)
    }
}

/// Largest text artifact the model may author in one call. Generous enough for
/// a long document or a multi-hundred-line file, bounded so a runaway
/// generation cannot pin the stream or the client.
pub const MAX_TEXT_ARTIFACT_CHARS: usize = 200_000;

/// Per-`(thread, artifact)` version counter.
#[derive(Clone, Default)]
pub struct ArtifactVersionStore {
    versions: Arc<DashMap<(String, String), u32>>,
}

impl ArtifactVersionStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the version to stamp on the next emission of `artifact_id` in
    /// `thread_id`: 1 for a new artifact, previous + 1 for an update.
    pub fn next_version(&self, thread_id: &str, artifact_id: &str) -> u32 {
        let key = (thread_id.to_owned(), artifact_id.to_owned());
        let mut entry = self.versions.entry(key).or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    }

    /// The current version, or `None` when the artifact has never been emitted
    /// in this thread — which is how `update_artifact` detects a bad id instead
    /// of silently creating a fresh artifact under a name the user never saw.
    #[must_use]
    pub fn current_version(&self, thread_id: &str, artifact_id: &str) -> Option<u32> {
        self.versions
            .get(&(thread_id.to_owned(), artifact_id.to_owned()))
            .map(|entry| *entry)
    }
}

/// Builds the `artifact` event. Separate from the store so the version decision
/// and the wire shape are testable independently.
#[must_use]
pub fn artifact_event(
    id: &str,
    kind: ArtifactKind,
    title: &str,
    content: &str,
    version: u32,
) -> ChatEvent {
    ChatEvent::Artifact {
        id: id.to_owned(),
        kind: kind.as_str().to_owned(),
        title: title.to_owned(),
        content: content.to_owned(),
        version,
    }
}

/// Builds the `attachment` event for a generated binary, so the file is
/// downloadable from the message itself and not only from the artifact panel.
#[must_use]
pub fn attachment_event(id: &str, name: &str, mime: &str, data_uri: &str, size: i64) -> ChatEvent {
    ChatEvent::Attachment {
        id: id.to_owned(),
        name: name.to_owned(),
        mime: mime.to_owned(),
        url: data_uri.to_owned(),
        size,
    }
}

/// Chooses the artifact kind for a file the code interpreter produced, from its
/// mime type. Text-ish outputs become previewable artifacts; spreadsheets get
/// their own kind so the client can label them; everything else is a file.
#[must_use]
pub fn kind_for_generated_file(mime: &str, name: &str) -> ArtifactKind {
    let lower_name = name.to_lowercase();
    if mime.starts_with("image/") {
        return ArtifactKind::Image;
    }
    if mime.contains("spreadsheet") || lower_name.ends_with(".xlsx") || lower_name.ends_with(".csv")
    {
        return ArtifactKind::Spreadsheet;
    }
    if mime == "text/html" || lower_name.ends_with(".html") {
        return ArtifactKind::Html;
    }
    if mime == "text/markdown" || lower_name.ends_with(".md") {
        return ArtifactKind::Document;
    }
    ArtifactKind::File
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_start_at_one_and_increment_per_artifact() {
        let store = ArtifactVersionStore::new();
        assert_eq!(store.next_version("t1", "a"), 1);
        assert_eq!(store.next_version("t1", "a"), 2);
        assert_eq!(store.next_version("t1", "a"), 3);
        // A different artifact in the same thread has its own counter.
        assert_eq!(store.next_version("t1", "b"), 1);
        // The same artifact id in a different thread is a different artifact —
        // two users iterating on "report" must not share a version line.
        assert_eq!(store.next_version("t2", "a"), 1);
    }

    #[test]
    fn current_version_is_none_until_first_emission() {
        let store = ArtifactVersionStore::new();
        assert_eq!(store.current_version("t1", "a"), None);
        store.next_version("t1", "a");
        assert_eq!(store.current_version("t1", "a"), Some(1));
    }

    #[test]
    fn kind_parsing_accepts_synonyms_and_rejects_unknown() {
        assert_eq!(ArtifactKind::parse("code"), Some(ArtifactKind::Code));
        assert_eq!(ArtifactKind::parse("  HTML "), Some(ArtifactKind::Html));
        // Synonyms a model reaches for on its own.
        assert_eq!(ArtifactKind::parse("markdown"), Some(ArtifactKind::Document));
        assert_eq!(ArtifactKind::parse("text"), Some(ArtifactKind::Document));
        // Unknown must be rejected, not defaulted: a defaulted kind renders as
        // the wrong thing (or nothing) with no error anyone can see.
        assert_eq!(ArtifactKind::parse("hologram"), None);
    }

    #[test]
    fn only_text_kinds_are_model_authorable() {
        assert!(ArtifactKind::Code.is_text_authored());
        assert!(ArtifactKind::Document.is_text_authored());
        assert!(ArtifactKind::Html.is_text_authored());
        // A model cannot hand-author a binary; those come from the interpreter.
        assert!(!ArtifactKind::Spreadsheet.is_text_authored());
        assert!(!ArtifactKind::File.is_text_authored());
        assert!(!ArtifactKind::Image.is_text_authored());
    }

    #[test]
    fn generated_file_kind_follows_mime_then_extension() {
        assert_eq!(
            kind_for_generated_file("image/png", "chart.png"),
            ArtifactKind::Image
        );
        assert_eq!(
            kind_for_generated_file(
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "report.xlsx"
            ),
            ArtifactKind::Spreadsheet
        );
        // csv has a text mime but is spreadsheet-shaped to a user.
        assert_eq!(
            kind_for_generated_file("text/csv", "rows.csv"),
            ArtifactKind::Spreadsheet
        );
        assert_eq!(
            kind_for_generated_file("text/html", "page.html"),
            ArtifactKind::Html
        );
        // A docx is a file: the client offers a download, not a preview.
        assert_eq!(
            kind_for_generated_file(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "brev.docx"
            ),
            ArtifactKind::File
        );
        assert_eq!(
            kind_for_generated_file("application/pdf", "faktura.pdf"),
            ArtifactKind::File
        );
    }

    #[test]
    fn events_carry_the_wire_shape_the_client_parses() {
        match artifact_event("a1", ArtifactKind::Document, "Rapport", "# Hei", 2) {
            ChatEvent::Artifact {
                id,
                kind,
                title,
                content,
                version,
            } => {
                assert_eq!(id, "a1");
                assert_eq!(kind, "document");
                assert_eq!(title, "Rapport");
                assert_eq!(content, "# Hei");
                assert_eq!(version, 2);
            }
            other => panic!("expected artifact, got {other:?}"),
        }
        match attachment_event("f1", "r.xlsx", "application/x", "data:application/x;base64,AA", 2) {
            ChatEvent::Attachment {
                id,
                name,
                mime,
                url,
                size,
            } => {
                assert_eq!(id, "f1");
                assert_eq!(name, "r.xlsx");
                assert_eq!(mime, "application/x");
                assert!(url.starts_with("data:"));
                assert_eq!(size, 2);
            }
            other => panic!("expected attachment, got {other:?}"),
        }
    }
}
