//! What a person may change on a paused mail send before they approve it, and the call that results
//! (EP-10-S3, decision D2).
//!
//! Pure: session-core applies it inside the grant (EP-10-S4, the authority), execution-core runs the
//! result (S5), and the SPA mirrors the limits in `src/shared/approvals/approval-edit-limits.ts`.
//! Both sides read `tests/fixtures/approval-edit-cases.json`, so the limits cannot drift apart.
//!
//! Only `to`, `subject` and `bodyText` of a normalised mail send change. `connection_id`,
//! `operation`, `params`, the threading fields and every other key are copied unchanged, so an edit
//! can never retarget a connection or turn a send into another operation. A Gmail `raw` draft keeps
//! its fields inside an encoded message and is preview-only.

use serde::Deserialize;
use serde_json::Value;

use crate::provider_writes::{is_mail_send, mail_recipients};

/// Fewest recipients a mail send may have (integration-corev2 refuses none).
pub const MIN_RECIPIENTS: usize = 1;
/// Most recipients a mail send may have (integration-corev2's limit).
pub const MAX_RECIPIENTS: usize = 50;
/// Longest subject in UTF-8 bytes after trimming (RFC 5322 line limit, integration-corev2's check).
pub const MAX_SUBJECT_BYTES: usize = 998;
/// Longest edited plain-text body in UTF-8 bytes. It keeps an edited descriptor under session-core's
/// 64 KiB descriptor cap with room for the rest of the call.
pub const MAX_BODY_TEXT_BYTES: usize = 48 * 1024;
/// Longest single recipient, RFC 5321's path limit plus room for a display name.
pub const MAX_RECIPIENT_BYTES: usize = 320;

/// The fields a person changed, in the names the audit event and the model's note use.
pub const FIELD_TO: &str = "to";
pub const FIELD_SUBJECT: &str = "subject";
pub const FIELD_BODY_TEXT: &str = "body_text";

/// A person's changes to a drafted mail send. A field left out keeps the draft's value. The JSON
/// keys are the mail body's own (`to`, `subject`, `bodyText`); any other key is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailSendPatch {
    #[serde(default)]
    pub to: Option<Vec<String>>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default, rename = "bodyText")]
    pub body_text: Option<String>,
}

/// The call to run after an edit, and which fields the person changed (names only, in the fixed
/// order `to`, `subject`, `body_text`).
#[derive(Debug, Clone, PartialEq)]
pub struct EditedCall {
    pub input: Value,
    pub edited_fields: Vec<&'static str>,
}

/// Why an edit was refused. The codes are a closed set the gateway and the SPA map one to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// The call is not an editable mail send (another operation, a Gmail `raw` draft, no body).
    NotEditable,
    /// A recipient does not parse as an address, or the list is empty.
    InvalidRecipient,
    /// More than `MAX_RECIPIENTS` recipients.
    TooManyRecipients,
    /// The subject has a line break or is longer than `MAX_SUBJECT_BYTES`.
    InvalidSubject,
    /// The body is empty: a send needs a body, and an edited body replaces any formatted one.
    InvalidBody,
    /// The body is longer than `MAX_BODY_TEXT_BYTES`.
    EditTooLarge,
    /// The patch changes nothing the draft does not already say.
    NoChange,
}

impl EditError {
    /// Every code, in a fixed order, for the goldens.
    pub const ALL: [Self; 7] = [
        Self::NotEditable,
        Self::InvalidRecipient,
        Self::TooManyRecipients,
        Self::InvalidSubject,
        Self::InvalidBody,
        Self::EditTooLarge,
        Self::NoChange,
    ];

    /// The wire code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotEditable => "not_editable",
            Self::InvalidRecipient => "invalid_recipient",
            Self::TooManyRecipients => "too_many_recipients",
            Self::InvalidSubject => "invalid_subject",
            Self::InvalidBody => "invalid_body",
            Self::EditTooLarge => "edit_too_large",
            Self::NoChange => "no_change",
        }
    }
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for EditError {}

/// Apply a person's patch to the retained `execute_provider_action` input of a mail send.
///
/// Checks run in a fixed order so both sides report the same code: editable at all, then
/// recipients, subject and body, then whether anything changed. A body edit drops `bodyHtml`, since
/// the person edited the text and the formatted copy would no longer match it; an edit of only
/// `to` or `subject` keeps it.
pub fn apply_edit(original_input: &Value, patch: &MailSendPatch) -> Result<EditedCall, EditError> {
    let original = original_input.as_object().ok_or(EditError::NotEditable)?;
    let operation = original
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(EditError::NotEditable)?;
    if !is_mail_send(operation) {
        return Err(EditError::NotEditable);
    }
    let body = original
        .get("body")
        .and_then(Value::as_object)
        .ok_or(EditError::NotEditable)?;
    if body.contains_key("raw") {
        return Err(EditError::NotEditable);
    }

    let to = patch.to.as_deref().map(check_recipients).transpose()?;
    let subject = patch.subject.as_deref().map(check_subject).transpose()?;
    let body_text = patch.body_text.as_deref().map(check_body).transpose()?;

    let mut edited_fields = Vec::new();
    let mut edited_body = body.clone();
    if let Some(to) = to {
        if to != mail_recipients(body.get("to")) {
            edited_body.insert(
                "to".to_owned(),
                Value::Array(to.into_iter().map(Value::String).collect()),
            );
            edited_fields.push(FIELD_TO);
        }
    }
    if let Some(subject) = subject {
        let drafted = body.get("subject").and_then(Value::as_str).unwrap_or("");
        if subject != drafted.trim() {
            edited_body.insert("subject".to_owned(), Value::String(subject));
            edited_fields.push(FIELD_SUBJECT);
        }
    }
    if let Some(text) = body_text {
        let drafted = body.get("bodyText").and_then(Value::as_str).unwrap_or("");
        if text != drafted {
            edited_body.insert("bodyText".to_owned(), Value::String(text.to_owned()));
            edited_body.remove("bodyHtml");
            edited_fields.push(FIELD_BODY_TEXT);
        }
    }
    if edited_fields.is_empty() {
        return Err(EditError::NoChange);
    }

    let mut input = original.clone();
    input.insert("body".to_owned(), Value::Object(edited_body));
    Ok(EditedCall {
        input: Value::Object(input),
        edited_fields,
    })
}

/// The recipients, trimmed, in the person's order. The count is checked before the addresses so
/// an oversized list reports its size, not its first bad entry.
fn check_recipients(to: &[String]) -> Result<Vec<String>, EditError> {
    if to.len() > MAX_RECIPIENTS {
        return Err(EditError::TooManyRecipients);
    }
    if to.len() < MIN_RECIPIENTS {
        return Err(EditError::InvalidRecipient);
    }
    to.iter()
        .map(|recipient| {
            let recipient = recipient.trim();
            if is_recipient(recipient) {
                Ok(recipient.to_owned())
            } else {
                Err(EditError::InvalidRecipient)
            }
        })
        .collect()
}

/// The subject, trimmed as integration-corev2 trims it before its own checks.
fn check_subject(subject: &str) -> Result<String, EditError> {
    if subject.contains(['\r', '\n']) {
        return Err(EditError::InvalidSubject);
    }
    let subject = subject.trim();
    if subject.len() > MAX_SUBJECT_BYTES {
        return Err(EditError::InvalidSubject);
    }
    Ok(subject.to_owned())
}

/// The body as written. Its size is checked first so an oversized body reports its size.
fn check_body(text: &str) -> Result<&str, EditError> {
    if text.len() > MAX_BODY_TEXT_BYTES {
        return Err(EditError::EditTooLarge);
    }
    if text.trim().is_empty() {
        return Err(EditError::InvalidBody);
    }
    Ok(text)
}

/// A conservative RFC 5322 mailbox: `local@domain` or `Display Name <local@domain>`, a dot-atom
/// local part and a dot-separated host name. It is stricter than integration-corev2's parser (no
/// quoted local parts, no comments, no address literals), so whatever passes here also passes
/// there, and a comma can never smuggle a second recipient into one entry.
fn is_recipient(recipient: &str) -> bool {
    if recipient.is_empty()
        || recipient.len() > MAX_RECIPIENT_BYTES
        || recipient.chars().any(char::is_control)
    {
        return false;
    }
    let address = match recipient.strip_suffix('>') {
        Some(rest) => {
            let Some((name, address)) = rest.rsplit_once('<') else {
                return false;
            };
            if name.contains(['<', '>', ',', ';', '@']) {
                return false;
            }
            address
        }
        None => recipient,
    };
    is_addr_spec(address)
}

fn is_addr_spec(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    if local.is_empty() || local.len() > 64 || domain.is_empty() || domain.len() > 255 {
        return false;
    }
    let atext = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~".contains(c);
    let local_ok = local
        .split('.')
        .all(|atom| !atom.is_empty() && atom.chars().all(atext));
    let domain_ok = domain.contains('.')
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    local_ok && domain_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn draft() -> Value {
        json!({
            "connection_id": "conn-1",
            "operation": "gmail.send",
            "params": { "userId": "me" },
            "body": {
                "to": "alice@example.com, bob@example.com",
                "subject": "Status",
                "bodyText": "Hei",
                "bodyHtml": "<p>Hei</p>",
                "threadId": "t-1",
                "inReplyTo": "<m-1@example.com>",
            },
        })
    }

    #[test]
    fn approval_edit_changes_only_the_named_fields_and_keeps_the_rest() {
        let patch = MailSendPatch {
            subject: Some("  Status, revised ".to_owned()),
            ..MailSendPatch::default()
        };
        let edited = apply_edit(&draft(), &patch).expect("subject edit");
        assert_eq!(edited.edited_fields, vec![FIELD_SUBJECT]);
        let mut expected = draft();
        expected["body"]["subject"] = json!("Status, revised");
        assert_eq!(edited.input, expected, "bodyHtml and threading are kept");
    }

    #[test]
    fn approval_edit_of_the_body_drops_the_formatted_copy() {
        let patch = MailSendPatch {
            body_text: Some("Hei igjen".to_owned()),
            ..MailSendPatch::default()
        };
        let edited = apply_edit(&draft(), &patch).expect("body edit");
        assert_eq!(edited.edited_fields, vec![FIELD_BODY_TEXT]);
        assert_eq!(edited.input["body"]["bodyText"], "Hei igjen");
        assert!(edited.input["body"].get("bodyHtml").is_none());
        assert_eq!(edited.input["body"]["threadId"], "t-1");
        assert_eq!(edited.input["connection_id"], "conn-1");
        assert_eq!(edited.input["operation"], "gmail.send");
        assert_eq!(edited.input["params"], json!({ "userId": "me" }));
    }

    #[test]
    fn approval_edit_compares_recipients_the_way_the_send_splits_them() {
        let same = MailSendPatch {
            to: Some(vec![" alice@example.com".to_owned(), "bob@example.com".to_owned()]),
            ..MailSendPatch::default()
        };
        assert_eq!(apply_edit(&draft(), &same), Err(EditError::NoChange));
        let added = MailSendPatch {
            to: Some(vec![
                "alice@example.com".to_owned(),
                "bob@example.com".to_owned(),
                "Carol <carol@example.org>".to_owned(),
            ]),
            ..MailSendPatch::default()
        };
        let edited = apply_edit(&draft(), &added).expect("adding a recipient is allowed");
        assert_eq!(
            edited.input["body"]["to"],
            json!(["alice@example.com", "bob@example.com", "Carol <carol@example.org>"])
        );
        assert_eq!(edited.edited_fields, vec![FIELD_TO]);
    }

    #[test]
    fn approval_edit_refuses_what_it_cannot_edit() {
        let patch = MailSendPatch {
            subject: Some("x".to_owned()),
            ..MailSendPatch::default()
        };
        let mut raw = draft();
        raw["body"] = json!({ "raw": "TUlNRQ" });
        let mut other = draft();
        other["operation"] = json!("message.send");
        for input in [raw, other, json!({ "operation": "mail.send" }), json!("text")] {
            assert_eq!(apply_edit(&input, &patch), Err(EditError::NotEditable), "{input}");
        }
    }

    #[test]
    fn approval_edit_checks_recipients() {
        let edit = |to: Vec<&str>| {
            apply_edit(
                &draft(),
                &MailSendPatch {
                    to: Some(to.into_iter().map(str::to_owned).collect()),
                    ..MailSendPatch::default()
                },
            )
        };
        assert_eq!(edit(vec![]), Err(EditError::InvalidRecipient));
        for bad in [
            "",
            "alice",
            "alice@",
            "@example.com",
            "alice@example",
            "a b@example.com",
            "alice@example.com, eve@example.com",
            "alice@example.com\r\nBcc: eve@example.com",
            "\"quoted\"@example.com",
            "Alice <alice@example.com",
            "alice@-example.com",
        ] {
            assert_eq!(edit(vec![bad]), Err(EditError::InvalidRecipient), "{bad:?}");
        }
        let many: Vec<String> = (0..=MAX_RECIPIENTS).map(|n| format!("p{n}@x.example")).collect();
        assert_eq!(
            edit(many.iter().map(String::as_str).collect()),
            Err(EditError::TooManyRecipients)
        );
        let max: Vec<String> = (0..MAX_RECIPIENTS).map(|n| format!("p{n}@x.example")).collect();
        assert!(edit(max.iter().map(String::as_str).collect()).is_ok());
    }

    #[test]
    fn approval_edit_checks_the_subject_and_the_body() {
        let subject = |text: String| {
            apply_edit(
                &draft(),
                &MailSendPatch {
                    subject: Some(text),
                    ..MailSendPatch::default()
                },
            )
        };
        assert_eq!(subject("a\nb".to_owned()), Err(EditError::InvalidSubject));
        assert_eq!(subject("a\rb".to_owned()), Err(EditError::InvalidSubject));
        assert_eq!(
            subject("x".repeat(MAX_SUBJECT_BYTES + 1)),
            Err(EditError::InvalidSubject)
        );
        assert!(subject("x".repeat(MAX_SUBJECT_BYTES)).is_ok());

        let body = |text: String| {
            apply_edit(
                &draft(),
                &MailSendPatch {
                    body_text: Some(text),
                    ..MailSendPatch::default()
                },
            )
        };
        assert_eq!(body("  \n".to_owned()), Err(EditError::InvalidBody));
        assert_eq!(
            body("x".repeat(MAX_BODY_TEXT_BYTES + 1)),
            Err(EditError::EditTooLarge)
        );
        assert!(body("x".repeat(MAX_BODY_TEXT_BYTES)).is_ok());
    }

    #[test]
    fn approval_edit_with_nothing_new_is_no_change() {
        assert_eq!(
            apply_edit(&draft(), &MailSendPatch::default()),
            Err(EditError::NoChange)
        );
        let same = MailSendPatch {
            subject: Some("Status".to_owned()),
            body_text: Some("Hei".to_owned()),
            ..MailSendPatch::default()
        };
        assert_eq!(apply_edit(&draft(), &same), Err(EditError::NoChange));
    }

    #[test]
    fn approval_edit_patch_refuses_unknown_keys() {
        let patch: Result<MailSendPatch, _> =
            serde_json::from_value(json!({ "to": ["a@x.example"], "connection_id": "conn-2" }));
        assert!(patch.is_err(), "a patch cannot name a frozen field");
        let patch: MailSendPatch =
            serde_json::from_value(json!({ "bodyText": "Hei" })).expect("bodyText parses");
        assert_eq!(patch.body_text.as_deref(), Some("Hei"));
    }

    #[test]
    fn approval_edit_codes_are_closed_and_distinct() {
        let codes: std::collections::BTreeSet<&str> =
            EditError::ALL.iter().map(|error| error.code()).collect();
        assert_eq!(codes.len(), EditError::ALL.len());
    }
}
