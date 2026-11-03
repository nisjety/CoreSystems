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
//!
//! The approval reason of a mail send (`provider_effect_summary`) lives here too (EP-10-S3), so
//! session-core can recompute it for an edited approval with the words execution-core used.

use std::fmt::Write as _;

use serde_json::Value;

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

/// Most recipients an approval names before it says how many more there are.
const SUMMARY_RECIPIENTS: usize = 10;
/// Most characters of a mail's subject an approval shows.
const SUMMARY_SUBJECT_CHARS: usize = 200;
/// Most characters of a mail's opening an approval shows.
const SUMMARY_OPENING_CHARS: usize = 120;

/// What a provider write would do, in words that follow "The agent would" (WS23-S8). Defined for a
/// mail send only, and read the way integration-corev2 reads the send: the recipients in `body.to`
/// (a comma-separated string or a list), the subject, and the first 120 characters of the
/// plain-text body. `None` for every other operation and for an input that does not parse.
///
/// The caller keeps these words in the approval's reason and nowhere else. That reason is scrubbed,
/// and no audit row, outbox event or log line repeats it.
#[must_use]
pub fn provider_effect_summary(tool_input: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Input {
        operation: String,
        #[serde(default)]
        body: Value,
    }
    let input: Input = serde_json::from_str(tool_input).ok()?;
    if !is_mail_send(&input.operation) {
        return None;
    }
    let recipients = mail_recipients(input.body.get("to"));
    let mut summary = String::from("send an email to ");
    if recipients.is_empty() {
        summary.push_str("no one");
    } else {
        let shown: Vec<&str> = recipients
            .iter()
            .take(SUMMARY_RECIPIENTS)
            .map(String::as_str)
            .collect();
        summary.push_str(&shown.join(", "));
        if recipients.len() > SUMMARY_RECIPIENTS {
            let _ = write!(
                summary,
                " and {} more",
                recipients.len() - SUMMARY_RECIPIENTS
            );
        }
    }
    match input
        .body
        .get("subject")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|subject| !subject.is_empty())
    {
        Some(subject) => {
            let _ = write!(
                summary,
                " with the subject '{}'",
                one_line(subject, SUMMARY_SUBJECT_CHARS)
            );
        }
        None => summary.push_str(" with no subject"),
    }
    if let Some(text) = input
        .body
        .get("bodyText")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let _ = write!(
            summary,
            "; it opens '{}'",
            one_line(text, SUMMARY_OPENING_CHARS)
        );
    }
    Some(summary)
}

/// Whether an operation sends a mail, under the names integration-corev2 gates for approval.
#[must_use]
pub fn is_mail_send(operation: &str) -> bool {
    matches!(
        operation.trim().to_ascii_lowercase().as_str(),
        "gmail.send" | "google.gmail.send" | "mail.send" | "microsoft.mail.send"
    )
}

/// The recipients a mail send reaches, split as integration-corev2 splits them: a string on commas,
/// or each string item of a list.
#[must_use]
pub fn mail_recipients(to: Option<&Value>) -> Vec<String> {
    let raw: Vec<&str> = match to {
        Some(Value::String(list)) => list.split(',').collect(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    raw.into_iter()
        .map(str::trim)
        .filter(|recipient| !recipient.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The first `max` characters of `text` on one line, marked when cut. A line break becomes a
/// space, so a summary never spans lines in an approval.
fn one_line(text: &str, max: usize) -> String {
    let mut line: String = text
        .chars()
        .take(max)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if text.chars().count() > max {
        line.push('…');
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail_input(operation: &str, body: &Value) -> String {
        serde_json::json!({
            "connection_id": "conn-1",
            "operation": operation,
            "body": body,
        })
        .to_string()
    }

    #[test]
    fn provider_effect_summary_reads_a_mail_send_the_way_execution_core_did() {
        let body = serde_json::json!({
            "to": " a@x.example ,b@y.example, ",
            "subject": "Hi",
            "bodyText": "line one\nline two",
        });
        assert_eq!(
            provider_effect_summary(&mail_input("google.gmail.send", &body)).as_deref(),
            Some(
                "send an email to a@x.example, b@y.example with the subject 'Hi'; \
                 it opens 'line one line two'"
            )
        );
        assert_eq!(
            provider_effect_summary(&mail_input("message.send", &body)),
            None
        );
        assert_eq!(provider_effect_summary("not json"), None);
    }

    #[test]
    fn provider_effect_summary_marks_a_cut_opening() {
        let body = serde_json::json!({ "to": ["a@x.example"], "bodyText": "y".repeat(121) });
        let summary = provider_effect_summary(&mail_input("mail.send", &body)).expect("summary");
        assert!(
            summary.ends_with(&format!("'{}…'", "y".repeat(120))),
            "{summary}"
        );
    }

    #[test]
    fn mail_recipients_split_strings_and_lists_alike() {
        let listed = serde_json::json!(["a@x.example", " ", 7, "b@y.example "]);
        let joined = serde_json::json!("a@x.example, ,b@y.example");
        let expected = vec!["a@x.example".to_owned(), "b@y.example".to_owned()];
        assert_eq!(mail_recipients(Some(&listed)), expected);
        assert_eq!(mail_recipients(Some(&joined)), expected);
        assert!(mail_recipients(None).is_empty());
    }
}
