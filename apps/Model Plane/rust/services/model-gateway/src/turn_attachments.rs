//! The files a chat turn attached, as the sandbox will receive them (WS08-S5).
//!
//! The browser sends an attached file as base64 beside the message. When the
//! sandbox staging switch is on, [`ValidatedAttachments`] turns the file kinds of
//! that list into `RunAttachment`s: decoded once, measured, hashed and judged by
//! the same contract execution-core applies on arrival
//! (`mp_contracts::attachments`). The value lives for one request and is passed
//! down to `code_interpreter` by reference, so the model's tool arguments can
//! neither add a file nor alter one: the model-authored `files_in` still exists
//! and lands in the writable root, a different thing.
//!
//! Nothing here stores, logs or serializes the bytes. Images are not handled
//! here (they take the vision path), and a zero-retention turn never reaches this
//! module with files: `zdr_features` refuses attachments first, and this module
//! refuses again rather than rely on that order.

use base64::Engine as _;
use mp_contracts::attachments::{self, AttachmentRefusal as Contract};
use mp_contracts::model_plane::v1::{RunAttachment, RunAttachmentKind};

use crate::attachment_policy::{AttachmentCode, AttachmentRefusal, SANDBOX_FILES_VAR};
use crate::vision::InvokeAttachment;

/// The sentence added to the `code_interpreter` description on a turn that has
/// attached files. Fixed text: it names the directory and nothing the client sent.
pub const TOOL_DESCRIPTION_LINE: &str = " Files the user attached to this message are readable, read-only, under the attachments/ directory of the working directory; copy one out before changing it.";

/// Whether the sandbox staging of attached files is switched on. Opt-in: only the
/// literal `true` enables it, so a typo or an unset variable keeps today's
/// behaviour.
#[must_use]
pub fn sandbox_staging_enabled() -> bool {
    enabled_with(&|key| std::env::var(key).ok())
}

fn enabled_with(get: &dyn Fn(&str) -> Option<String>) -> bool {
    get(SANDBOX_FILES_VAR).is_some_and(|value| value.trim() == "true")
}

/// The validated file attachments of one turn. Empty for every turn without
/// files, with the switch off, and after the turn ends.
#[derive(Clone, Default)]
pub struct ValidatedAttachments {
    files: Vec<RunAttachment>,
}

impl std::fmt::Debug for ValidatedAttachments {
    // Counts only: this type holds customer bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedAttachments")
            .field("files", &self.files.len())
            .finish()
    }
}

impl ValidatedAttachments {
    /// A turn with nothing to stage.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Builds the staged list from the request's attachments. Only `kind:"file"`
    /// entries are taken; images are left to the vision path. With `enabled`
    /// false this is [`Self::none`], so the switch off changes nothing.
    ///
    /// # Errors
    /// The typed refusal for the first file that breaks the contract. Nothing is
    /// dropped: a file that cannot be staged refuses the turn, as the attachment
    /// policy already does for the same causes.
    pub fn from_invoke(
        attachments: &[InvokeAttachment],
        enabled: bool,
        zdr: bool,
    ) -> Result<Self, AttachmentRefusal> {
        if !enabled {
            return Ok(Self::none());
        }
        let mut files = Vec::new();
        for (index, attachment) in attachments.iter().enumerate() {
            if !is_file(attachment) {
                continue;
            }
            files.push(run_attachment(index, attachment)?);
        }
        attachments::validate(&files, zdr).map_err(refuse)?;
        Ok(Self { files })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The list carried on `ExecuteStepRequest.attachments`.
    #[must_use]
    pub fn run_attachments(&self) -> &[RunAttachment] {
        &self.files
    }

    /// The paragraph that tells the model what `attachments/` holds, from
    /// validated names and measured sizes only. Empty when nothing is staged.
    #[must_use]
    pub fn sandbox_note(&self) -> String {
        attachments::sandbox_note(&self.files)
    }

    /// Count and total size, for a log line that carries no content.
    #[must_use]
    pub fn summary(&self) -> (usize, usize) {
        (
            self.files.len(),
            self.files.iter().map(|file| file.content.len()).sum(),
        )
    }
}

fn is_file(attachment: &InvokeAttachment) -> bool {
    attachment
        .kind
        .as_deref()
        .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("file"))
}

fn run_attachment(
    index: usize,
    attachment: &InvokeAttachment,
) -> Result<RunAttachment, AttachmentRefusal> {
    let encoding = |message: &str| {
        AttachmentRefusal {
            code: AttachmentCode::EncodingInvalid,
            message: message.to_owned(),
        }
    };
    let name = attachment
        .name
        .as_deref()
        .ok_or_else(|| encoding("A file attachment needs a name."))?;
    // The same tolerance the policy and the vision path have: a `data:` prefix.
    let payload = attachment
        .data_base64
        .as_deref()
        .filter(|data| !data.is_empty())
        .map(|data| data.rsplit(',').next().unwrap_or(data))
        .ok_or_else(|| encoding("The attachment carries no data."))?;
    // Refuse an oversized payload before spending memory decoding it.
    if payload.len() > attachments::MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + 4 {
        return Err(refuse(Contract::TooLarge));
    }
    let content = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|_| encoding("The attachment data is not valid base64."))?;
    Ok(RunAttachment {
        id: format!("att-{index}"),
        name: name.to_owned(),
        mime: attachment
            .mime_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
        size_bytes: content.len() as u64,
        sha256: attachments::sha256_hex(&content),
        content,
        kind: RunAttachmentKind::File as i32,
    })
}

/// Maps the shared contract's refusal onto the closed codes the gateway already
/// publishes for attachments. The sentence names the cause, never a name or bytes
/// the client chose.
fn refuse(refusal: Contract) -> AttachmentRefusal {
    let (code, message) = match refusal {
        Contract::Zdr => (
            AttachmentCode::MediaUnsupported,
            "Files are not read on a zero-retention turn.",
        ),
        Contract::TooMany => (
            AttachmentCode::CountExceeded,
            "Too many files are attached to one message.",
        ),
        Contract::TooLarge => (AttachmentCode::TooLarge, "An attached file is too large."),
        Contract::TotalTooLarge => (
            AttachmentCode::TooLarge,
            "The attached files add up to more than one message may carry.",
        ),
        Contract::KindUnsupported => (
            AttachmentCode::MediaUnsupported,
            "Only files can be given to the code sandbox.",
        ),
        Contract::Empty
        | Contract::NameInvalid
        | Contract::NameDuplicate
        | Contract::SizeMismatch
        | Contract::HashMismatch => (
            AttachmentCode::EncodingInvalid,
            "An attached file could not be read as sent.",
        ),
    };
    AttachmentRefusal {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, bytes: &[u8]) -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("file".to_owned()),
            name: Some(name.to_owned()),
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            mime_type: Some("text/csv; charset=utf-8".to_owned()),
            ..InvokeAttachment::default()
        }
    }

    fn image() -> InvokeAttachment {
        InvokeAttachment {
            kind: Some("image".to_owned()),
            data_base64: Some("aGk=".to_owned()),
            mime_type: Some("image/png".to_owned()),
            ..InvokeAttachment::default()
        }
    }

    #[test]
    fn the_switch_is_off_unless_the_value_is_literally_true() {
        for value in [None, Some(""), Some("1"), Some("TRUE"), Some("yes"), Some("false")] {
            let value = value.map(str::to_owned);
            assert!(!enabled_with(&|_| value.clone()), "{value:?}");
        }
        assert!(enabled_with(&|_| Some("true".to_owned())));
    }

    #[test]
    fn with_the_switch_off_nothing_is_staged() {
        let staged =
            ValidatedAttachments::from_invoke(&[file("a.csv", b"1,2")], false, false).expect("ok");
        assert!(staged.is_empty());
        assert!(staged.sandbox_note().is_empty());
    }

    #[test]
    fn files_are_measured_hashed_and_images_are_left_alone() {
        let staged = ValidatedAttachments::from_invoke(
            &[image(), file("sales.csv", b"month,revenue\n")],
            true,
            false,
        )
        .expect("a file and an image");
        let [staged_file] = staged.run_attachments() else {
            panic!("only the file is staged");
        };
        assert_eq!(staged_file.name, "sales.csv");
        assert_eq!(staged_file.id, "att-1");
        assert_eq!(staged_file.mime, "text/csv");
        assert_eq!(staged_file.size_bytes, 14);
        assert_eq!(staged_file.sha256, attachments::sha256_hex(b"month,revenue\n"));
        assert_eq!(staged_file.content, b"month,revenue\n");
        assert_eq!(staged.summary(), (1, 14));
        assert!(staged.sandbox_note().contains("attachments/sales.csv"));
    }

    #[test]
    fn an_image_only_turn_stages_nothing() {
        let staged = ValidatedAttachments::from_invoke(&[image()], true, false).expect("ok");
        assert!(staged.is_empty());
    }

    #[test]
    fn unsafe_names_are_refused_never_repaired() {
        for name in ["../a.csv", "dir/a.csv", " a.csv", ".hidden"] {
            let refusal = ValidatedAttachments::from_invoke(&[file(name, b"x")], true, false)
                .expect_err(name);
            assert_eq!(refusal.code, AttachmentCode::EncodingInvalid, "{name}");
            assert!(!refusal.message.contains(name.trim()), "the refusal never echoes the name");
        }
    }

    #[test]
    fn bad_base64_and_missing_names_are_refused() {
        let mut broken = file("a.csv", b"x");
        broken.data_base64 = Some("!!!not-base64".to_owned());
        assert_eq!(
            ValidatedAttachments::from_invoke(&[broken], true, false)
                .expect_err("bad base64")
                .code,
            AttachmentCode::EncodingInvalid
        );
        let mut nameless = file("a.csv", b"x");
        nameless.name = None;
        assert_eq!(
            ValidatedAttachments::from_invoke(&[nameless], true, false)
                .expect_err("no name")
                .code,
            AttachmentCode::EncodingInvalid
        );
    }

    #[test]
    fn caps_and_zdr_refuse_with_the_published_codes() {
        let many: Vec<InvokeAttachment> = (0..=attachments::MAX_ATTACHMENTS)
            .map(|i| file(&format!("f{i}.csv"), b"x"))
            .collect();
        assert_eq!(
            ValidatedAttachments::from_invoke(&many, true, false)
                .expect_err("too many")
                .code,
            AttachmentCode::CountExceeded
        );
        let big = vec![7u8; attachments::MAX_ATTACHMENT_BYTES + 1];
        assert_eq!(
            ValidatedAttachments::from_invoke(&[file("big.bin", &big)], true, false)
                .expect_err("too large")
                .code,
            AttachmentCode::TooLarge
        );
        assert!(ValidatedAttachments::from_invoke(&[file("a.csv", b"x")], true, true).is_err());
        // A zero-retention turn with no files is not touched.
        assert!(ValidatedAttachments::from_invoke(&[image()], true, true)
            .expect("no files")
            .is_empty());
    }

    #[test]
    fn debug_never_prints_bytes() {
        let staged =
            ValidatedAttachments::from_invoke(&[file("a.csv", b"TOPSECRET")], true, false)
                .expect("ok");
        assert!(!format!("{staged:?}").contains("TOPSECRET"));
    }
}
