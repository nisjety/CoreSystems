//! What the approval card shows of a paused write before the person decides it (EP-10-S1,
//! decision D4).
//!
//! Pure: session-core decrypts the retained descriptor for the run's owner on a pending approval
//! and passes its `tool_name` and `input` here; the result is the only thing the preview RPC
//! returns, so the raw descriptor never leaves session-core. A mail send carries every recipient,
//! the full subject and the full plain-text body. Every other call carries its provider,
//! operation and connection and no argument values, since only a mail send has a card that knows
//! how to show them. The caller never logs the result or puts it in an event.

use serde_json::Value;

use crate::approval_edit::{FIELD_BODY_TEXT, FIELD_SUBJECT, FIELD_TO};
use crate::provider_writes::{
    is_mail_send, mail_recipients, EditableKind, ProviderWriteOperation, WRITE_OPERATIONS,
};

/// The one tool whose input names a provider operation.
const PROVIDER_ACTION_TOOL: &str = "execute_provider_action";

/// What a preview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// A mail send, with its recipients, subject and body.
    MailSend,
    /// Another call: its provider, operation and connection, no values.
    Other,
    /// The approval keeps no descriptor (a browser takeover, a cause with no tool call), so there
    /// is nothing beyond the card itself to show.
    NoDescriptor,
}

impl PreviewKind {
    /// The wire value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MailSend => "mail_send",
            Self::Other => "other",
            Self::NoDescriptor => "none",
        }
    }
}

/// The display fields of one approval preview, in the proto's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPreviewView {
    pub kind: PreviewKind,
    pub provider: String,
    pub operation: String,
    pub connection_id: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body_text: String,
    pub has_html: bool,
    pub is_reply: bool,
    pub raw_only: bool,
    pub editable_fields: Vec<&'static str>,
}

impl ApprovalPreviewView {
    /// The preview of an approval with no retained descriptor (EP-1's `browser_human_assist`
    /// among them, integration round B C-B4): every field empty.
    #[must_use]
    pub fn no_descriptor() -> Self {
        Self::bare(PreviewKind::NoDescriptor, String::new(), String::new(), String::new())
    }

    fn bare(kind: PreviewKind, provider: String, operation: String, connection_id: String) -> Self {
        Self {
            kind,
            provider,
            operation,
            connection_id,
            to: Vec::new(),
            subject: String::new(),
            body_text: String::new(),
            has_html: false,
            is_reply: false,
            raw_only: false,
            editable_fields: Vec::new(),
        }
    }
}

/// Shape the retained `tool_name` and `input` of a descriptor into what the card shows.
///
/// The recipients are split as integration-corev2 splits them (`mail_recipients`), so the card
/// lists exactly who the send reaches. A Gmail `raw` draft keeps its fields inside an encoded
/// message: it is marked `raw_only`, shows none of them and offers no edit.
#[must_use]
pub fn preview_from_input(tool_name: &str, input: &Value) -> ApprovalPreviewView {
    if tool_name != PROVIDER_ACTION_TOOL {
        return ApprovalPreviewView::bare(
            PreviewKind::Other,
            String::new(),
            String::new(),
            String::new(),
        );
    }
    let text = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let operation = text("operation");
    let connection_id = text("connection_id");
    let provider = provider_of(&operation).unwrap_or("").to_owned();
    if !is_mail_send(&operation) {
        return ApprovalPreviewView::bare(PreviewKind::Other, provider, operation, connection_id);
    }

    let mut view =
        ApprovalPreviewView::bare(PreviewKind::MailSend, provider, operation, connection_id);
    let Some(body) = input.get("body").and_then(Value::as_object) else {
        return view;
    };
    let field = |key: &str| body.get(key).and_then(Value::as_str);
    view.is_reply = field("threadId").is_some_and(|value| !value.trim().is_empty())
        || field("inReplyTo").is_some_and(|value| !value.trim().is_empty());
    if body.contains_key("raw") {
        view.raw_only = true;
        return view;
    }
    view.to = mail_recipients(body.get("to"));
    view.subject = field("subject").unwrap_or("").trim().to_owned();
    view.body_text = field("bodyText").unwrap_or("").to_owned();
    view.has_html = field("bodyHtml").is_some_and(|html| !html.trim().is_empty());
    if editable_kind_of(&view.operation) == Some(EditableKind::MailSend) {
        view.editable_fields = vec![FIELD_TO, FIELD_SUBJECT, FIELD_BODY_TEXT];
    }
    view
}

/// The catalogued write row an operation names, by its bare name or an alias, ignoring case and
/// surrounding spaces as execution-core's catalogue does. The descriptor's input names no
/// provider, and each catalogued operation belongs to one.
fn write_row(operation: &str) -> Option<&'static ProviderWriteOperation> {
    let operation = operation.trim();
    WRITE_OPERATIONS.iter().find(|write| {
        write.operation.eq_ignore_ascii_case(operation)
            || write
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(operation))
    })
}

fn provider_of(operation: &str) -> Option<&'static str> {
    write_row(operation).map(|write| write.provider)
}

fn editable_kind_of(operation: &str) -> Option<EditableKind> {
    write_row(operation).map(|write| write.editable_kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mail(body: Value) -> Value {
        json!({
            "connection_id": "conn-1",
            "operation": "gmail.send",
            "params": { "userId": "me" },
            "body": body,
        })
    }

    #[test]
    fn mail_send_preview_keeps_every_recipient_and_the_full_body() {
        let recipients: Vec<String> = (0..12).map(|i| format!("r{i}@example.com")).collect();
        let body_text = format!("Hei,\n\n{}\nHilsen", "x".repeat(4000));
        let view = preview_from_input(
            "execute_provider_action",
            &mail(json!({
                "to": recipients.join(", "),
                "subject": format!("  {}  ", "S".repeat(400)),
                "bodyText": body_text,
            })),
        );
        assert_eq!(view.kind, PreviewKind::MailSend);
        assert_eq!(view.provider, "google");
        assert_eq!(view.operation, "gmail.send");
        assert_eq!(view.connection_id, "conn-1");
        assert_eq!(view.to, recipients, "no recipient is cut, unlike the 10 the reason names");
        assert_eq!(view.subject, "S".repeat(400), "the subject is trimmed, never shortened");
        assert_eq!(view.body_text, body_text, "the body is whole, line breaks kept");
        assert!(!view.has_html && !view.is_reply && !view.raw_only);
        assert_eq!(view.editable_fields, vec!["to", "subject", "body_text"]);
    }

    #[test]
    fn mail_send_preview_names_the_microsoft_provider_and_marks_html() {
        let view = preview_from_input(
            "execute_provider_action",
            &json!({
                "connection_id": "conn-ms",
                "operation": "Microsoft.Mail.Send",
                "body": {
                    "to": ["a@example.com", " b@example.com "],
                    "subject": "Hi",
                    "bodyText": "Text",
                    "bodyHtml": "<p>Text</p>",
                },
            }),
        );
        assert_eq!(view.kind, PreviewKind::MailSend);
        assert_eq!(view.provider, "microsoft");
        assert_eq!(view.to, vec!["a@example.com", "b@example.com"]);
        assert!(view.has_html);
        assert_eq!(view.body_text, "Text", "the formatted body is flagged, never shown");
    }

    #[test]
    fn other_writes_carry_no_values() {
        let view = preview_from_input(
            "execute_provider_action",
            &json!({
                "connection_id": "conn-2",
                "operation": "message.send",
                "params": { "channel": "C1" },
                "body": { "text": "secret plan", "to": "x@example.com", "subject": "S" },
            }),
        );
        assert_eq!(
            view,
            ApprovalPreviewView::bare(
                PreviewKind::Other,
                "slack".to_owned(),
                "message.send".to_owned(),
                "conn-2".to_owned(),
            )
        );

        let not_a_provider_call =
            preview_from_input("delete_records", &json!({ "operation": "mail.send", "id": "r-1" }));
        assert_eq!(not_a_provider_call.kind, PreviewKind::Other);
        assert!(not_a_provider_call.operation.is_empty());
        assert!(not_a_provider_call.connection_id.is_empty());
        assert!(not_a_provider_call.to.is_empty() && not_a_provider_call.body_text.is_empty());
    }

    #[test]
    fn gmail_raw_draft_is_raw_only() {
        let view = preview_from_input(
            "execute_provider_action",
            &mail(json!({
                "raw": "RnJvbTogYUBleGFtcGxlLmNvbQ==",
                "to": "a@example.com",
                "threadId": "t-9",
            })),
        );
        assert_eq!(view.kind, PreviewKind::MailSend);
        assert!(view.raw_only);
        assert!(view.is_reply);
        assert!(view.to.is_empty() && view.subject.is_empty() && view.body_text.is_empty());
        assert!(view.editable_fields.is_empty(), "a raw draft is preview-only");
    }

    #[test]
    fn reply_headers_mark_is_reply() {
        for body in [
            json!({ "to": "a@example.com", "bodyText": "x", "threadId": "t-1" }),
            json!({ "to": "a@example.com", "bodyText": "x", "inReplyTo": "<m-1@example.com>" }),
        ] {
            assert!(preview_from_input("execute_provider_action", &mail(body)).is_reply);
        }
        let blank = mail(json!({ "to": "a@example.com", "bodyText": "x", "threadId": "  " }));
        assert!(!preview_from_input("execute_provider_action", &blank).is_reply);
    }

    #[test]
    fn a_mail_send_without_a_body_shows_nothing_and_offers_no_edit() {
        let view = preview_from_input(
            "execute_provider_action",
            &json!({ "connection_id": "conn-1", "operation": "mail.send" }),
        );
        assert_eq!(view.kind, PreviewKind::MailSend);
        assert!(view.to.is_empty() && view.editable_fields.is_empty());
    }

    #[test]
    fn an_approval_without_a_descriptor_has_the_no_descriptor_form() {
        let view = ApprovalPreviewView::no_descriptor();
        assert_eq!(view.kind.as_str(), "none");
        assert!(view.provider.is_empty() && view.operation.is_empty());
        assert!(view.connection_id.is_empty());
        assert!(view.to.is_empty() && view.editable_fields.is_empty());
    }

    #[test]
    fn the_kinds_have_fixed_wire_values() {
        assert_eq!(PreviewKind::MailSend.as_str(), "mail_send");
        assert_eq!(PreviewKind::Other.as_str(), "other");
        assert_eq!(PreviewKind::NoDescriptor.as_str(), "none");
    }
}
