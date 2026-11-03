//! The provider write operations the agent may call, as the Rust side of the write golden
//! (EP-10-S2).
//!
//! integration-corev2 owns the executor and the approval allowlist (`actionRequiresApproval`). Its
//! Go test checks `internal/actions/testdata/provider-write-operations.json` against each provider
//! executor's switch arms, against the allowlist, and against the provider catalogue's direction
//! for the capability each write needs. The fixture in `tests/fixtures/provider-write-operations.json`
//! is a byte-identical copy, pinned by sha256 in `tests/provider_write_operations.rs`.
//!
//! The golden's `operations` are the writes execution-core catalogues as writes. This table is what
//! those rows must say, and execution-core's catalogue must mark exactly these operations as writes.
//! The golden's `excludedWrites` are gated by integration-corev2 but not catalogued here, so
//! execution-core treats them as uncatalogued and pauses them for approval as well.

/// What the person may change on an approval before they approve it (EP-10 D2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditableKind {
    /// A mail send: its recipients, subject and plain-text body may be changed.
    MailSend,
    /// Approved as drafted; the approval card offers no edit.
    NotEditable,
}

impl EditableKind {
    /// The value the golden records for this kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MailSend => "mail_send",
            Self::NotEditable => "none",
        }
    }
}

/// One write operation of a connected provider, as the executor serves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderWriteOperation {
    pub provider: &'static str,
    pub operation: &'static str,
    /// The other spellings the executor and the approval allowlist accept for the same operation.
    pub aliases: &'static [&'static str],
    pub editable_kind: EditableKind,
}

pub const WRITE_OPERATIONS: &[ProviderWriteOperation] = &[
    ProviderWriteOperation {
        provider: "github",
        operation: "issues.create",
        aliases: &["github.issues.create"],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "google",
        operation: "gmail.send",
        aliases: &["google.gmail.send"],
        editable_kind: EditableKind::MailSend,
    },
    ProviderWriteOperation {
        provider: "linkedin",
        operation: "posts.create",
        aliases: &["linkedin.posts.create"],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "meta",
        operation: "instagram.media.create",
        aliases: &[],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "meta",
        operation: "instagram.media.publish",
        aliases: &[],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "meta",
        operation: "messenger.messages.send",
        aliases: &[],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "meta",
        operation: "pages.post",
        aliases: &["facebook.page.post"],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "meta",
        operation: "whatsapp.messages.send",
        aliases: &[],
        editable_kind: EditableKind::NotEditable,
    },
    ProviderWriteOperation {
        provider: "microsoft",
        operation: "mail.send",
        aliases: &["microsoft.mail.send"],
        editable_kind: EditableKind::MailSend,
    },
    ProviderWriteOperation {
        provider: "slack",
        operation: "message.send",
        aliases: &["slack.message.send"],
        editable_kind: EditableKind::NotEditable,
    },
];

/// How the person may edit an approval for this operation, or `None` when it is not a catalogued
/// write. The operation matches its bare name or any alias, ignoring case and surrounding spaces,
/// as execution-core's catalogue does.
pub fn editable_kind(provider: &str, operation: &str) -> Option<EditableKind> {
    let operation = operation.trim();
    WRITE_OPERATIONS
        .iter()
        .find(|write| {
            write.provider == provider
                && (write.operation.eq_ignore_ascii_case(operation)
                    || write
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(operation)))
        })
        .map(|write| write.editable_kind)
}
