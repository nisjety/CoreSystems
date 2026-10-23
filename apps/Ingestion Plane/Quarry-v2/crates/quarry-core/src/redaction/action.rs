//! Receipt redaction for agent actions (WS8.B narrow branch of the WS12
//! `redacted_for_record` shape).
//!
//! A human step on a non-ZDR run is recorded as metadata only: what was typed,
//! selected, pressed or uploaded is replaced by a marker, and a navigation keeps
//! its scheme, host and path but loses its query, fragment and userinfo. The
//! action kind and its locator stay, so a timeline can still say what happened.

use url::Url;

use crate::contracts::AgentAction;
use crate::ids::kinds;

/// The marker that replaces free text in a stored human receipt.
pub const REDACTED: &str = "[redacted]";

/// The action as it may be stored for a human step's metadata receipt.
///
/// Upload ids are typed ULIDs and cannot hold the marker, so they are replaced
/// by the nil artifact id (`art_00000000000000000000000000`), which can never be
/// a real artifact. Callers must not treat it as a reference.
pub fn redacted_for_record(action: &AgentAction) -> AgentAction {
    let mut out = action.clone();
    match &mut out {
        AgentAction::Navigate { url } => *url = navigate_without_query(url),
        AgentAction::Type { text, .. }
        | AgentAction::TypeRef { text, .. }
        | AgentAction::FrameTypeRef { text, .. }
        | AgentAction::TypeSemantic { text, .. } => *text = REDACTED.to_owned(),
        AgentAction::Select { value, .. }
        | AgentAction::SelectRef { value, .. }
        | AgentAction::FrameSelectRef { value, .. }
        | AgentAction::SelectSemantic { value, .. } => *value = REDACTED.to_owned(),
        AgentAction::Press { key } => *key = REDACTED.to_owned(),
        AgentAction::RespondDialog { prompt_text, .. } => {
            if prompt_text.is_some() {
                *prompt_text = Some(REDACTED.to_owned());
            }
        }
        AgentAction::UploadRef { artifact_id, .. }
        | AgentAction::FrameUploadRef { artifact_id, .. } => {
            *artifact_id = kinds::ArtifactKind::from_ulid(ulid::Ulid::nil());
        }
        AgentAction::Evaluate { script } => *script = REDACTED.to_owned(),
        _ => {}
    }
    out
}

/// Scheme, host (with port), and path only. Anything that does not parse is
/// replaced whole, never kept, so a malformed URL cannot leak through.
pub fn navigate_without_query(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(mut url) => {
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.to_string()
        }
        Err(_) => REDACTED.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::AgentAction;

    #[test]
    fn human_receipt_redacts_typed_text_and_selected_values() {
        let typed = redacted_for_record(&AgentAction::Type {
            selector: "#pw".to_owned(),
            text: "hunter2".to_owned(),
        });
        assert!(matches!(typed, AgentAction::Type { ref text, .. } if text == REDACTED));

        let selected = redacted_for_record(&AgentAction::SelectRef {
            snapshot_id: "snap_x".to_owned(),
            generation: 1,
            ref_id: "r1".to_owned(),
            value: "secret-option".to_owned(),
        });
        assert!(matches!(selected, AgentAction::SelectRef { ref value, .. } if value == REDACTED));

        let pressed = redacted_for_record(&AgentAction::Press { key: "Enter".to_owned() });
        assert!(matches!(pressed, AgentAction::Press { ref key } if key == REDACTED));
    }

    #[test]
    fn human_receipt_navigate_drops_query_fragment_and_userinfo() {
        let kept = navigate_without_query("https://user:pw@example.com:8443/a/b?token=abc#frag");
        assert_eq!(kept, "https://example.com:8443/a/b");
        assert!(!kept.contains("token"));
        assert!(!kept.contains("pw"));
        assert_eq!(navigate_without_query("not a url"), REDACTED);
    }

    #[test]
    fn human_receipt_upload_id_is_the_nil_placeholder() {
        let redacted = redacted_for_record(&AgentAction::UploadRef {
            snapshot_id: "snap_x".to_owned(),
            generation: 1,
            ref_id: "r1".to_owned(),
            artifact_id: kinds::ArtifactKind::new(),
            approval_grant_id: "grant_x".to_owned(),
        });
        match redacted {
            AgentAction::UploadRef { artifact_id, .. } => {
                assert_eq!(artifact_id.ulid(), ulid::Ulid::nil());
            }
            other => panic!("unexpected action {other:?}"),
        }
    }

    #[test]
    fn navigation_free_actions_are_unchanged() {
        let click = AgentAction::Click { selector: "#go".to_owned() };
        assert!(matches!(
            redacted_for_record(&click),
            AgentAction::Click { ref selector } if selector == "#go"
        ));
    }
}
