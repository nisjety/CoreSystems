//! Typed context references a chat send carries next to its text (Wave 3 EP-3 S5).
//!
//! The composer attaches atomic references (a Data Plane document, a past thread, the turn's
//! Space, a skill, a selection in an artifact) as a structured list, never as text in the
//! prompt. This module is the shape and the bounds only, shared by the gateway and its tests;
//! what each kind does for a turn, and when it is refused, is the gateway's resolver
//! (`model-gateway` `context_refs`). A reference is a SELECTOR: its id names something the
//! server resolves under the caller's own authority, and its label is display text for the chip,
//! never trusted and never sent to a model.

use std::collections::BTreeSet;

/// Most references one send may carry, of every kind together.
pub const MAX_CONTEXT_REFS: usize = 20;
/// Most thread references one send may carry (EP-3 OD-7): each is read lazily at tool time, so
/// the cap bounds what the model may browse, not what is injected.
pub const MAX_THREAD_REFS: usize = 20;
/// Most artifact selections one send may carry: a selection edit revises one range.
pub const MAX_ARTIFACT_SELECTIONS: usize = 1;
/// Longest label kept, in characters. A longer one is cut, never refused: it is display text.
pub const MAX_LABEL_CHARS: usize = 120;
/// Longest id accepted, in bytes. Every id this names (document, thread, Space, skill, artifact)
/// is an ASCII identifier well under this length.
pub const MAX_ID_LEN: usize = 128;

/// What a reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContextRefKind {
    /// A Data Plane document the viewer can read; it narrows the turn's knowledge search.
    Document,
    /// One of the owner's own earlier chat threads, read lazily by thread tools (EP-3 S7).
    Thread,
    /// The turn's own Space. It never widens a scope: it must equal the thread's Space.
    Space,
    /// A skill from the org's catalogue, merged into the turn's explicit skill picks.
    Skill,
    /// A range in the current revision of an artifact, for a selection edit (EP-3 S6).
    ArtifactSelection,
}

impl ContextRefKind {
    /// Every kind, in wire order.
    pub const ALL: [Self; 5] = [
        Self::Document,
        Self::Thread,
        Self::Space,
        Self::Skill,
        Self::ArtifactSelection,
    ];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Thread => "thread",
            Self::Space => "space",
            Self::Skill => "skill",
            Self::ArtifactSelection => "artifact_selection",
        }
    }

    /// The kind a wire spelling names; an unknown spelling names none, and is refused by the
    /// caller rather than guessed at.
    #[must_use]
    pub fn from_wire(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// The range an `artifact_selection` names. Offsets are character offsets into the revision the
/// client displayed; the quote itself never travels, only its SHA-256, so the server can check the
/// range still holds the text the person selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSelection {
    pub artifact_id: String,
    pub version: u32,
    pub start: u32,
    pub end: u32,
    /// Lowercase hex SHA-256 of the selected text.
    pub quote_sha256: String,
}

/// One reference as the send carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRef {
    pub kind: ContextRefKind,
    pub id: String,
    /// Display text for the chip. Never trusted, never sent to a model.
    pub label: String,
    /// Present exactly when `kind` is [`ContextRefKind::ArtifactSelection`].
    pub selection: Option<ArtifactSelection>,
}

/// Why a list of references was refused. Each names the first rule the list broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextRefError {
    /// More than [`MAX_CONTEXT_REFS`] references.
    TooMany,
    /// More than [`MAX_THREAD_REFS`] thread references.
    TooManyThreads,
    /// More than [`MAX_ARTIFACT_SELECTIONS`] artifact selections.
    TooManySelections,
    /// An id that is empty, too long or not ASCII-safe.
    InvalidId,
    /// The same kind and id twice.
    Duplicate,
    /// A selection on a kind that takes none, or an `artifact_selection` without a well-formed one.
    InvalidSelection,
}

impl ContextRefError {
    /// A stable reason string, for the refusal's `details`.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::TooMany => "too_many_refs",
            Self::TooManyThreads => "too_many_thread_refs",
            Self::TooManySelections => "too_many_selections",
            Self::InvalidId => "invalid_id",
            Self::Duplicate => "duplicate_ref",
            Self::InvalidSelection => "invalid_selection",
        }
    }
}

/// An id that can name a document, thread, Space, skill or artifact: 1 to [`MAX_ID_LEN`] ASCII
/// letters, digits, `_`, `-`, `.` or `:`. Nothing that could break out of a filter or a path.
#[must_use]
pub fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn selection_is_well_formed(reference: &ContextRef) -> bool {
    match (reference.kind, &reference.selection) {
        (ContextRefKind::ArtifactSelection, Some(selection)) => {
            selection.artifact_id == reference.id
                && selection.start < selection.end
                && is_sha256_hex(&selection.quote_sha256)
        }
        (ContextRefKind::ArtifactSelection, None) => false,
        (_, selection) => selection.is_none(),
    }
}

/// The label as kept: trimmed, control characters dropped, cut to [`MAX_LABEL_CHARS`].
#[must_use]
pub fn capped_label(label: &str) -> String {
    label
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_LABEL_CHARS)
        .collect()
}

/// Checks a send's references and returns them with capped labels, in the order sent.
///
/// The counts are checked first, so an oversized list is refused before any id is looked at.
///
/// # Errors
///
/// The first [`ContextRefError`] the list breaks.
pub fn validate_context_refs(refs: &[ContextRef]) -> Result<Vec<ContextRef>, ContextRefError> {
    if refs.len() > MAX_CONTEXT_REFS {
        return Err(ContextRefError::TooMany);
    }
    let count = |kind: ContextRefKind| refs.iter().filter(|r| r.kind == kind).count();
    if count(ContextRefKind::Thread) > MAX_THREAD_REFS {
        return Err(ContextRefError::TooManyThreads);
    }
    if count(ContextRefKind::ArtifactSelection) > MAX_ARTIFACT_SELECTIONS {
        return Err(ContextRefError::TooManySelections);
    }
    let mut seen: BTreeSet<(ContextRefKind, &str)> = BTreeSet::new();
    let mut out = Vec::with_capacity(refs.len());
    for reference in refs {
        if !is_safe_id(&reference.id) {
            return Err(ContextRefError::InvalidId);
        }
        if !selection_is_well_formed(reference) {
            return Err(ContextRefError::InvalidSelection);
        }
        if !seen.insert((reference.kind, reference.id.as_str())) {
            return Err(ContextRefError::Duplicate);
        }
        out.push(ContextRef {
            label: capped_label(&reference.label),
            ..reference.clone()
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(kind: ContextRefKind, id: &str) -> ContextRef {
        ContextRef {
            kind,
            id: id.to_owned(),
            label: format!("label {id}"),
            selection: None,
        }
    }

    fn selection(id: &str) -> ContextRef {
        ContextRef {
            selection: Some(ArtifactSelection {
                artifact_id: id.to_owned(),
                version: 3,
                start: 4,
                end: 9,
                quote_sha256: "a".repeat(64),
            }),
            ..reference(ContextRefKind::ArtifactSelection, id)
        }
    }

    #[test]
    fn rejects_more_than_20_refs() {
        let twenty: Vec<ContextRef> = (0..20)
            .map(|n| reference(ContextRefKind::Document, &format!("doc-{n}")))
            .collect();
        assert_eq!(validate_context_refs(&twenty).map(|refs| refs.len()), Ok(20));
        let mut twenty_one = twenty;
        twenty_one.push(reference(ContextRefKind::Skill, "skill.a"));
        assert_eq!(validate_context_refs(&twenty_one), Err(ContextRefError::TooMany));
    }

    #[test]
    fn thread_refs_are_capped_at_20() {
        let threads: Vec<ContextRef> = (0..20)
            .map(|n| reference(ContextRefKind::Thread, &format!("thread-{n}")))
            .collect();
        assert!(validate_context_refs(&threads).is_ok());
        assert_eq!(MAX_THREAD_REFS, MAX_CONTEXT_REFS, "today the total cap binds first");
    }

    #[test]
    fn rejects_two_selections() {
        assert!(validate_context_refs(&[selection("art-1")]).is_ok());
        assert_eq!(
            validate_context_refs(&[selection("art-1"), selection("art-2")]),
            Err(ContextRefError::TooManySelections)
        );
    }

    #[test]
    fn label_is_capped() {
        let mut long = reference(ContextRefKind::Document, "doc-1");
        long.label = format!("  {}\u{7}", "é".repeat(200));
        let kept = validate_context_refs(&[long]).expect("a long label is cut, not refused");
        assert_eq!(kept[0].label.chars().count(), MAX_LABEL_CHARS);
        assert!(kept[0].label.chars().all(|ch| ch == 'é'));
    }

    #[test]
    fn duplicates_rejected() {
        let doc = reference(ContextRefKind::Document, "doc-1");
        assert_eq!(
            validate_context_refs(&[doc.clone(), doc]),
            Err(ContextRefError::Duplicate)
        );
        // The same id under two kinds names two different things.
        assert!(validate_context_refs(&[
            reference(ContextRefKind::Document, "x-1"),
            reference(ContextRefKind::Skill, "x-1"),
        ])
        .is_ok());
    }

    #[test]
    fn ids_must_be_ascii_safe() {
        let too_long = "a".repeat(MAX_ID_LEN + 1);
        for bad in ["", "a b", "../etc", "id\"", "ø", too_long.as_str()] {
            assert_eq!(
                validate_context_refs(&[reference(ContextRefKind::Document, bad)]),
                Err(ContextRefError::InvalidId),
                "{bad:?}"
            );
        }
        assert!(is_safe_id("skill.web-search:v2_1"));
    }

    #[test]
    fn a_selection_belongs_only_to_an_artifact_selection_and_must_be_well_formed() {
        let mut stray = selection("doc-1");
        stray.kind = ContextRefKind::Document;
        assert_eq!(validate_context_refs(&[stray]), Err(ContextRefError::InvalidSelection));
        let bare = reference(ContextRefKind::ArtifactSelection, "art-1");
        assert_eq!(validate_context_refs(&[bare]), Err(ContextRefError::InvalidSelection));
        let mut empty_range = selection("art-1");
        if let Some(range) = empty_range.selection.as_mut() {
            range.end = range.start;
        }
        assert_eq!(validate_context_refs(&[empty_range]), Err(ContextRefError::InvalidSelection));
        let mut other_artifact = selection("art-1");
        if let Some(range) = other_artifact.selection.as_mut() {
            range.artifact_id = "art-2".to_owned();
        }
        assert_eq!(validate_context_refs(&[other_artifact]), Err(ContextRefError::InvalidSelection));
    }

    #[test]
    fn kinds_round_trip_on_the_wire() {
        for kind in ContextRefKind::ALL {
            assert_eq!(ContextRefKind::from_wire(kind.as_str()), Some(kind));
        }
        assert_eq!(ContextRefKind::from_wire("mention"), None);
    }
}
