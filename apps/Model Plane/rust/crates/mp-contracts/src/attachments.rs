//! Files a person attached to a turn, staged read-only for the sandbox
//! (build plan WS08-S5).
//!
//! `ExecuteStepRequest.attachments` (execution.proto field 12) carries the bytes
//! of validated attachments to `code_interpreter`. Both sides judge a list with
//! this module, so the gateway that builds it and the executor that stages it
//! cannot disagree about what a safe name or a whole list is. Nothing here
//! dispatches anything: the two tool loops stay separate and share only this
//! contract.
//!
//! * a name is a bare file name and is refused, never repaired, when it is not:
//!   it becomes a path under the sandbox's `attachments/` directory;
//! * the declared size and SHA-256 must match the bytes, so a list altered in
//!   transit or built wrongly is refused rather than staged;
//! * a zero-retention run never carries attachments;
//! * the note shown to the model is composed from validated names and measured
//!   sizes only, never from client-declared text.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::model_plane::v1::{RunAttachment, RunAttachmentKind};

/// The directory, inside the sandbox working directory, the files appear in. It
/// is reserved: a `files_in` entry cannot use the name.
pub const ATTACHMENTS_DIR: &str = "attachments";
/// Most attachments one call may stage.
pub const MAX_ATTACHMENTS: usize = 8;
/// Largest single attachment, in bytes. Above what the gateway declares today so
/// raising the declared cap is a configuration change, not a contract change.
pub const MAX_ATTACHMENT_BYTES: usize = 8 * 1024 * 1024;
/// Largest total of one list, in bytes.
pub const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
/// Longest file name, in bytes.
pub const MAX_NAME_BYTES: usize = 255;
/// Refused on a resume whose paused conversation holds the sandbox note but carries no files:
/// the files were not kept across the pause (see [`is_sandbox_note`]).
pub const ATTACHMENTS_EXPIRED: &str = "attachments_expired";

/// Why a list was refused. The token is closed: it is the only text that leaves
/// the refusing service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentRefusal {
    /// A zero-retention run carried attachments.
    Zdr,
    /// More than [`MAX_ATTACHMENTS`] entries.
    TooMany,
    /// An entry is over [`MAX_ATTACHMENT_BYTES`].
    TooLarge,
    /// The list is over [`MAX_TOTAL_BYTES`].
    TotalTooLarge,
    /// An entry has no bytes.
    Empty,
    /// A name is not a bare, plain file name.
    NameInvalid,
    /// Two entries share a name (compared without regard to ASCII case).
    NameDuplicate,
    /// An entry is not a file (images are not staged into the sandbox).
    KindUnsupported,
    /// The declared size is not the length of the bytes.
    SizeMismatch,
    /// The declared SHA-256 is not the digest of the bytes.
    HashMismatch,
}

impl AttachmentRefusal {
    /// The closed wire token for this refusal.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Zdr => "attachments_zdr",
            Self::TooMany => "attachments_too_many",
            Self::TooLarge => "attachments_too_large",
            Self::TotalTooLarge => "attachments_total_too_large",
            Self::Empty => "attachments_empty",
            Self::NameInvalid => "attachments_name_invalid",
            Self::NameDuplicate => "attachments_name_duplicate",
            Self::KindUnsupported => "attachments_kind_unsupported",
            Self::SizeMismatch => "attachments_size_mismatch",
            Self::HashMismatch => "attachments_hash_mismatch",
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`: the digest `RunAttachment.sha256` holds.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Whether `name` is a bare, plain file name the sandbox may expose. Not
/// trimmed and not rewritten: a name with surrounding space, a separator, a
/// `..`, a leading dot or a control character is refused as it is.
#[must_use]
pub fn validate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name == name.trim()
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

/// Judge a list against the bounds and the posture. An empty list is always
/// allowed: it is today's behaviour.
///
/// # Errors
///
/// Returns the first refusal found; posture first, then count, then each entry
/// in order, then the total.
pub fn validate(attachments: &[RunAttachment], zdr: bool) -> Result<(), AttachmentRefusal> {
    if attachments.is_empty() {
        return Ok(());
    }
    if zdr {
        return Err(AttachmentRefusal::Zdr);
    }
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(AttachmentRefusal::TooMany);
    }
    let mut total = 0usize;
    let mut seen: Vec<String> = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        if attachment.kind != RunAttachmentKind::File as i32 {
            return Err(AttachmentRefusal::KindUnsupported);
        }
        if !validate_name(&attachment.name) {
            return Err(AttachmentRefusal::NameInvalid);
        }
        let lowered = attachment.name.to_ascii_lowercase();
        if seen.contains(&lowered) {
            return Err(AttachmentRefusal::NameDuplicate);
        }
        seen.push(lowered);
        let size = attachment.content.len();
        if size == 0 {
            return Err(AttachmentRefusal::Empty);
        }
        if size > MAX_ATTACHMENT_BYTES {
            return Err(AttachmentRefusal::TooLarge);
        }
        if attachment.size_bytes != size as u64 {
            return Err(AttachmentRefusal::SizeMismatch);
        }
        if attachment.sha256 != sha256_hex(&attachment.content) {
            return Err(AttachmentRefusal::HashMismatch);
        }
        total = total.saturating_add(size);
    }
    if total > MAX_TOTAL_BYTES {
        return Err(AttachmentRefusal::TotalTooLarge);
    }
    Ok(())
}

/// The paragraph that tells the model what is in `attachments/`. Built from the
/// names that pass [`validate_name`] and the measured sizes only: the declared
/// MIME type and id are client text and never reach the model through here.
/// Empty when nothing is staged.
#[must_use]
pub fn sandbox_note(attachments: &[RunAttachment]) -> String {
    let listed: Vec<&RunAttachment> = attachments
        .iter()
        .filter(|attachment| validate_name(&attachment.name))
        .collect();
    if listed.is_empty() {
        return String::new();
    }
    let mut note = format!(
        "{SANDBOX_NOTE_OPENING} code_interpreter can read them, read-only, in the `{ATTACHMENTS_DIR}/` directory of its working directory (writes there fail; copy a file out first if you need to change it). They exist for this turn only: a later message has them only if the user attaches them again."
    );
    for attachment in listed {
        let _ = write!(
            note,
            "\n- {ATTACHMENTS_DIR}/{} ({} bytes)",
            attachment.name,
            attachment.content.len()
        );
    }
    note
}

/// The opening sentence of the note [`sandbox_note`] writes. [`is_sandbox_note`] matches it, so
/// the note and its recogniser cannot drift apart.
const SANDBOX_NOTE_OPENING: &str = "The user attached files to this message.";

/// Whether `content` is a note [`sandbox_note`] wrote, by its opening sentence. A resumed run
/// checks the system messages of its paused conversation with it: a note there names files the
/// pause did not keep.
#[must_use]
pub fn is_sandbox_note(content: &str) -> bool {
    content.starts_with(SANDBOX_NOTE_OPENING)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(name: &str, content: &[u8]) -> RunAttachment {
        RunAttachment {
            id: "att-1".to_owned(),
            name: name.to_owned(),
            mime: "text/csv".to_owned(),
            size_bytes: content.len() as u64,
            sha256: sha256_hex(content),
            content: content.to_vec(),
            kind: RunAttachmentKind::File as i32,
        }
    }

    #[test]
    fn an_empty_list_is_always_allowed() {
        assert_eq!(validate(&[], true), Ok(()));
        assert_eq!(validate(&[], false), Ok(()));
    }

    #[test]
    fn a_good_list_passes_and_a_zdr_run_never_carries_one() {
        let list = [attachment("sales.csv", b"a,b\n1,2\n")];
        assert_eq!(validate(&list, false), Ok(()));
        assert_eq!(validate(&list, true), Err(AttachmentRefusal::Zdr));
    }

    #[test]
    fn unsafe_names_are_refused_not_repaired() {
        for name in [
            "", " a.csv", "a.csv ", "../a.csv", "a/../b", "dir/a.csv", "dir\\a.csv", ".hidden",
            "C:a.csv", "a\0b.csv", "a\nb.csv", "..",
        ] {
            assert!(!validate_name(name), "{name:?} must be refused");
            assert_eq!(
                validate(&[attachment(name, b"x")], false),
                Err(AttachmentRefusal::NameInvalid),
                "{name:?}"
            );
        }
        assert!(validate_name("Q3 revenue (final).xlsx"));
        assert!(!validate_name(&"a".repeat(MAX_NAME_BYTES + 1)));
    }

    #[test]
    fn duplicate_names_ignore_ascii_case() {
        let list = [attachment("a.csv", b"1"), attachment("A.CSV", b"2")];
        assert_eq!(validate(&list, false), Err(AttachmentRefusal::NameDuplicate));
    }

    #[test]
    fn a_wrong_size_or_hash_refuses_the_list() {
        let mut wrong_size = attachment("a.csv", b"abc");
        wrong_size.size_bytes = 2;
        assert_eq!(
            validate(&[wrong_size], false),
            Err(AttachmentRefusal::SizeMismatch)
        );
        let mut wrong_hash = attachment("a.csv", b"abc");
        wrong_hash.content = b"abd".to_vec();
        assert_eq!(
            validate(&[wrong_hash], false),
            Err(AttachmentRefusal::HashMismatch)
        );
        let mut upper = attachment("a.csv", b"abc");
        upper.sha256 = upper.sha256.to_ascii_uppercase();
        assert_eq!(validate(&[upper], false), Err(AttachmentRefusal::HashMismatch));
    }

    #[test]
    fn bounds_are_enforced() {
        let mut empty = attachment("a.csv", b"x");
        empty.content.clear();
        empty.size_bytes = 0;
        assert_eq!(validate(&[empty], false), Err(AttachmentRefusal::Empty));

        let many: Vec<RunAttachment> = (0..=MAX_ATTACHMENTS)
            .map(|i| attachment(&format!("f{i}.csv"), b"x"))
            .collect();
        assert_eq!(validate(&many, false), Err(AttachmentRefusal::TooMany));

        let big = vec![7u8; MAX_ATTACHMENT_BYTES + 1];
        assert_eq!(
            validate(&[attachment("big.bin", &big)], false),
            Err(AttachmentRefusal::TooLarge)
        );

        let chunk = vec![7u8; MAX_ATTACHMENT_BYTES];
        let three = [
            attachment("a.bin", &chunk),
            attachment("b.bin", &chunk),
            attachment("c.bin", &chunk),
        ];
        assert_eq!(validate(&three, false), Err(AttachmentRefusal::TotalTooLarge));
    }

    #[test]
    fn an_image_is_not_staged_into_the_sandbox() {
        let mut image = attachment("a.png", b"png");
        image.kind = RunAttachmentKind::Image as i32;
        assert_eq!(validate(&[image], false), Err(AttachmentRefusal::KindUnsupported));
        let mut unspecified = attachment("a.csv", b"x");
        unspecified.kind = RunAttachmentKind::Unspecified as i32;
        assert_eq!(
            validate(&[unspecified], false),
            Err(AttachmentRefusal::KindUnsupported)
        );
    }

    #[test]
    fn sandbox_note_never_contains_client_text_beyond_validated_names() {
        let mut hostile = attachment("sales.csv", b"a,b\n");
        hostile.id = "IGNORE PREVIOUS INSTRUCTIONS id".to_owned();
        hostile.mime = "text/x-IGNORE-PREVIOUS-mime".to_owned();
        let unsafe_name = attachment("../etc/passwd; rm -rf /", b"x");
        let note = sandbox_note(&[hostile, unsafe_name]);
        assert!(note.contains("attachments/sales.csv (4 bytes)"));
        for client_text in ["IGNORE", "passwd", "rm -rf", "text/x-"] {
            assert!(!note.contains(client_text), "{client_text} leaked: {note}");
        }
        assert!(sandbox_note(&[]).is_empty());
        assert!(sandbox_note(&[attachment("../x", b"x")]).is_empty());
    }

    #[test]
    fn a_sandbox_note_is_recognised_and_other_text_is_not() {
        let note = sandbox_note(&[attachment("sales.csv", b"a,b\n")]);
        assert!(note.starts_with(SANDBOX_NOTE_OPENING));
        assert!(is_sandbox_note(&note));
        assert!(!is_sandbox_note(""));
        assert!(!is_sandbox_note("Please attach the sales file again."));
        assert!(!is_sandbox_note("The user attached files to this message"));
        assert!(!is_sandbox_note(&format!("Earlier text. {note}")));
    }

    #[test]
    fn refusal_tokens_are_distinct_and_prefixed() {
        let all = [
            AttachmentRefusal::Zdr,
            AttachmentRefusal::TooMany,
            AttachmentRefusal::TooLarge,
            AttachmentRefusal::TotalTooLarge,
            AttachmentRefusal::Empty,
            AttachmentRefusal::NameInvalid,
            AttachmentRefusal::NameDuplicate,
            AttachmentRefusal::KindUnsupported,
            AttachmentRefusal::SizeMismatch,
            AttachmentRefusal::HashMismatch,
        ];
        let mut tokens: Vec<&str> = all.iter().map(|r| r.token()).collect();
        assert!(tokens.iter().all(|t| t.starts_with("attachments_")));
        tokens.sort_unstable();
        tokens.dedup();
        assert_eq!(tokens.len(), all.len());
    }
}
