//! Receipt redaction for agent actions and observations (WS12.S3, with the
//! WS8.B human branch).
//!
//! One module owns what a stored receipt may say about an action (R-23). There
//! are two branches, chosen by the caller from the receipt plan:
//!
//! * [`RecordBranch::HumanMetadata`] is the WS8.B branch. A human step on a
//!   non-ZDR run is recorded as metadata only: what was typed, selected, pressed
//!   or uploaded is replaced by a marker, and a navigation keeps its scheme, host
//!   and path but loses its query, fragment and userinfo. The action kind and its
//!   locator stay, so a timeline can still say what happened. It is the stricter
//!   branch and wins whenever a caller picks it.
//! * [`RecordBranch::Typed`] is the agent branch. Typed text is masked by the
//!   kind of the control it went to (a password, an OTP, a card number, a phone
//!   number, a labelled secret). Under [`RecordPolicy::SensitiveOnly`] (the
//!   default, `QUARRY_EDGE__RECEIPT_TEXT_MODE=sensitive`) the text for any other
//!   control keeps its words, so a search query stays in the audit trail and a
//!   compiled procedure; it still passes the `Durable` text rules, which mask a
//!   token-shaped value wherever it sits. Under [`RecordPolicy::AllTyped`]
//!   (`all`) every typed text is replaced.
//!
//! The control kind comes from the snapshot the action named, read before the
//! action ran (`typed_field_kind` in `quarry-runtime`). When the kind is not
//! known for a ref-addressed typing action the text is masked as a secret: an
//! action that cannot say where it typed fails closed. A raw `type` action
//! names its control by selector only, so the selector's own words decide.
//!
//! [`redact_observation_durable`] is the same pass over an observation that a
//! receipt stores.

use std::borrow::Cow;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;
use url::Url;

use super::field::{classify_field, mask_with, Classification, FieldFacts};
use super::{
    redact_json_with, redact_text_with, redact_url_with, FieldKind, Mode, Profile, Tally,
};
use crate::contracts::{AgentAction, BrowserObservation};
use crate::ids::kinds;

/// The marker that replaces free text in a stored human receipt.
pub const REDACTED: &str = "[redacted]";

/// Which text a stored agent receipt keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RecordPolicy {
    /// Text typed into a sensitive control is masked; text for any other control
    /// keeps its words (after the `Durable` text rules). The default.
    #[default]
    SensitiveOnly,
    /// Every typed text, selected value and dialog prompt is replaced. Safer; a
    /// procedure compiled from such receipts cannot replay without its input.
    AllTyped,
}

impl RecordPolicy {
    /// Parse `sensitive` or `all` (case-insensitive). The edge reads its own
    /// configuration and calls this; the library never reads the environment.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "sensitive" => Some(Self::SensitiveOnly),
            "all" => Some(Self::AllTyped),
            _ => None,
        }
    }

    /// The configuration word for the policy.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SensitiveOnly => "sensitive",
            Self::AllTyped => "all",
        }
    }
}

static RECORD_POLICY: OnceLock<RecordPolicy> = OnceLock::new();

/// Set the process-wide receipt text policy. Callable once, before any receipt
/// is built. Returns the policy already in force when it was set or read
/// before: the first read pins the default, so a process never changes policy
/// while it is serving.
///
/// # Errors
/// The policy that is already in force.
pub fn init_record_policy(policy: RecordPolicy) -> Result<(), RecordPolicy> {
    RECORD_POLICY.set(policy).map_err(|_| record_policy())
}

/// The process-wide receipt text policy (default [`RecordPolicy::SensitiveOnly`]).
/// The first call pins it.
#[must_use]
pub fn record_policy() -> RecordPolicy {
    *RECORD_POLICY.get_or_init(RecordPolicy::default)
}

/// Which shape a stored receipt takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordBranch {
    /// A person's step on a metadata-only plan (`HUMAN_RECEIPT_MODE=metadata`):
    /// the action is reduced to its kind and locator. Wins over any text policy.
    HumanMetadata,
    /// Everything else: typed text is masked by the control kind and the policy.
    Typed,
}

/// The classification that records a field kind under its rule id.
const fn classification(kind: FieldKind) -> Classification {
    Classification {
        kind,
        rule: match kind {
            FieldKind::Password => Some("field.password"),
            FieldKind::Otp => Some("field.otp"),
            FieldKind::Card => Some("field.card"),
            FieldKind::Phone => Some("field.phone"),
            FieldKind::Secret => Some("field.label"),
            FieldKind::Unknown => None,
        },
    }
}

/// The input type named in a selector (`input[type=password]`,
/// `[type="password"]`), lowercase.
fn selector_input_type(selector: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| {
        Regex::new(r#"(?i)\[\s*type\s*[~|^$*]?=\s*["']?([a-z]+)"#)
            .unwrap_or_else(|e| panic!("selector type rule does not compile: {e}"))
    });
    re.captures(selector)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_ascii_lowercase())
}

/// The field kind a raw selector names: `[type=password]` or a label word in the
/// selector (`#password`, `input[name=otp]`). Used when there is no snapshot.
#[must_use]
pub fn selector_kind(selector: &str) -> FieldKind {
    let facts = FieldFacts {
        input_type: selector_input_type(selector),
        name_attr: Some(selector.to_owned()),
        id_attr: Some(selector.to_owned()),
        ..FieldFacts::default()
    };
    classify_field(&facts)
}

/// Typed text as a stored receipt may carry it.
fn typed_text(
    mode: Mode,
    policy: RecordPolicy,
    text: &str,
    kind: FieldKind,
    tally: &mut Tally,
) -> String {
    // A human metadata receipt already says nothing: its marker is final.
    if text == REDACTED {
        return text.to_owned();
    }
    let class = if kind.is_sensitive() {
        classification(kind)
    } else if policy == RecordPolicy::AllTyped {
        classification(FieldKind::Secret)
    } else {
        classification(FieldKind::Unknown)
    };
    let masked = mask_with(mode, class, text, tally);
    redact_text_with(mode, &masked, Profile::Durable, tally).into_owned()
}

/// A selected value or dialog prompt as a stored receipt may carry it.
fn free_text(mode: Mode, policy: RecordPolicy, text: &str, tally: &mut Tally) -> String {
    typed_text(mode, policy, text, FieldKind::Unknown, tally)
}

/// The action as a stored receipt may carry it (R-23: the one function).
///
/// `typed` is the kind of the control a typing action went to, read from the
/// snapshot before the action ran; `None` when it is not known (a raw `type`
/// action is then classified by its selector, a ref-addressed one is masked as a
/// secret). `tally` counts what was masked, by rule id, with no content.
#[must_use]
pub fn redacted_for_record(
    branch: RecordBranch,
    action: &AgentAction,
    typed: Option<FieldKind>,
    policy: RecordPolicy,
    mode: Mode,
    tally: &mut Tally,
) -> AgentAction {
    if branch == RecordBranch::HumanMetadata {
        return redacted_for_human_receipt(action);
    }
    let mut out = action.clone();
    match &mut out {
        AgentAction::Type { selector, text } => {
            let kind = typed.unwrap_or_else(|| selector_kind(selector));
            *text = typed_text(mode, policy, text, kind, tally);
        }
        AgentAction::TypeRef { text, .. }
        | AgentAction::FrameTypeRef { text, .. }
        | AgentAction::TypeSemantic { text, .. } => {
            let kind = typed.unwrap_or(FieldKind::Secret);
            *text = typed_text(mode, policy, text, kind, tally);
        }
        AgentAction::Select { value, .. }
        | AgentAction::SelectRef { value, .. }
        | AgentAction::FrameSelectRef { value, .. }
        | AgentAction::SelectSemantic { value, .. } => {
            *value = free_text(mode, policy, value, tally);
        }
        AgentAction::Navigate { url } => {
            *url = redact_url_with(mode, url, Profile::Durable, tally).into_owned();
        }
        AgentAction::RespondDialog { prompt_text, .. } => {
            if let Some(text) = prompt_text.take() {
                *prompt_text = Some(free_text(mode, policy, &text, tally));
            }
        }
        AgentAction::Evaluate { script } => {
            *script = redact_text_with(mode, script, Profile::Durable, tally).into_owned();
        }
        _ => {}
    }
    out
}

impl AgentAction {
    /// This action as a stored agent receipt may carry it, under the
    /// process-wide redaction mode. See [`redacted_for_record`].
    #[must_use]
    pub fn redacted_for_record(&self, policy: RecordPolicy, typed: Option<FieldKind>) -> Self {
        redacted_for_record(
            RecordBranch::Typed,
            self,
            typed,
            policy,
            super::mode(),
            &mut Tally::new(),
        )
    }
}

/// The action as it may be stored for a human step's metadata receipt.
///
/// Upload ids are typed ULIDs and cannot hold the marker, so they are replaced
/// by the nil artifact id (`art_00000000000000000000000000`), which can never be
/// a real artifact. Callers must not treat it as a reference.
#[must_use]
pub fn redacted_for_human_receipt(action: &AgentAction) -> AgentAction {
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

/// Mask the value of every `url` and `*_url` key under the `Durable` URL rules
/// (secret query and fragment values and userinfo), which the text rules over a
/// JSON leaf do not do.
fn redact_url_keys(value: &mut Value, mode: Mode, tally: &mut Tally, depth: usize) {
    if depth > 64 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                let is_url_key = key == "url" || key.ends_with("_url");
                match v {
                    Value::String(s) if is_url_key => {
                        if let Cow::Owned(new) = redact_url_with(mode, s, Profile::Durable, tally) {
                            *s = new;
                        }
                    }
                    _ => redact_url_keys(v, mode, tally, depth + 1),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_url_keys(item, mode, tally, depth + 1);
            }
        }
        _ => {}
    }
}

/// The observation as a stored receipt may carry it: every string leaf passes the
/// `Durable` rules (the value of a secret-named key and every URL field too), the
/// observation's own redaction stamp is left as it is.
///
/// `None` when the redacted form cannot be rebuilt: the receipt then stores no
/// observation rather than one that was only partly masked.
#[must_use]
pub fn redact_observation_durable(
    observation: &BrowserObservation,
    mode: Mode,
    tally: &mut Tally,
) -> Option<BrowserObservation> {
    if mode == Mode::Off {
        return Some(observation.clone());
    }
    let mut value = serde_json::to_value(observation).ok()?;
    // The stamp is counts keyed by rule id (`field.password`): the key rule would
    // blank those numbers and the stamp would no longer read.
    if let Some(map) = value.as_object_mut() {
        map.remove("redaction");
    }
    redact_url_keys(&mut value, mode, tally, 0);
    redact_json_with(mode, &mut value, Profile::Durable, tally);
    if mode == Mode::Shadow {
        return Some(observation.clone());
    }
    let mut out: BrowserObservation = serde_json::from_value(value).ok()?;
    out.redaction = observation.redaction.clone();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::AgentAction;

    // Assembled at run time so no secret-shaped literal exists in the repository.
    fn password() -> String {
        ["S3cret", "P@ss-0451"].concat()
    }

    fn jwt() -> String {
        format!(
            "{}.{}.{}",
            ["eyJ", "hbGciOiJIUzI1NiJ9"].concat(),
            ["eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0"].concat(),
            ["SflKxwRJSM", "eKKF2QT4fwpMeJf36POk6yJV_adQssw5c"].concat()
        )
    }

    fn type_ref(text: &str) -> AgentAction {
        AgentAction::TypeRef {
            snapshot_id: "snap_x".to_owned(),
            generation: 1,
            ref_id: "@e1".to_owned(),
            text: text.to_owned(),
        }
    }

    fn record(
        action: &AgentAction,
        typed: Option<FieldKind>,
        policy: RecordPolicy,
    ) -> (AgentAction, Tally) {
        let mut tally = Tally::new();
        let out = redacted_for_record(
            RecordBranch::Typed,
            action,
            typed,
            policy,
            Mode::Enforce,
            &mut tally,
        );
        (out, tally)
    }

    fn text_of(action: &AgentAction) -> String {
        match action {
            AgentAction::Type { text, .. }
            | AgentAction::TypeRef { text, .. }
            | AgentAction::FrameTypeRef { text, .. }
            | AgentAction::TypeSemantic { text, .. } => text.clone(),
            other => panic!("not a typing action: {other:?}"),
        }
    }

    #[test]
    fn type_ref_into_password_is_masked() {
        let (out, tally) = record(
            &type_ref(&password()),
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
        );
        assert_eq!(text_of(&out), "[redacted:password]");
        assert_eq!(tally.by_rule().get("field.password"), Some(&1));
        // A one-time code and a card number get their own markers.
        let (out, _) = record(
            &type_ref("123456"),
            Some(FieldKind::Otp),
            RecordPolicy::SensitiveOnly,
        );
        assert_eq!(text_of(&out), "[redacted:otp]");
    }

    #[test]
    fn type_ref_into_search_box_keeps_text_under_sensitive_policy() {
        let (out, tally) = record(
            &type_ref("wireless headphones"),
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
        );
        assert_eq!(text_of(&out), "wireless headphones");
        assert!(tally.is_empty());
    }

    #[test]
    fn an_unknown_kind_for_a_ref_typing_action_fails_closed() {
        let (out, _) = record(&type_ref("anything"), None, RecordPolicy::SensitiveOnly);
        assert_eq!(text_of(&out), "[redacted:secret]");
    }

    #[test]
    fn all_typed_policy_masks_everything() {
        let (out, _) = record(
            &type_ref("wireless headphones"),
            Some(FieldKind::Unknown),
            RecordPolicy::AllTyped,
        );
        assert_eq!(text_of(&out), "[redacted:secret]");
        // A sensitive kind keeps its own marker.
        let (out, _) = record(
            &type_ref(&password()),
            Some(FieldKind::Password),
            RecordPolicy::AllTyped,
        );
        assert_eq!(text_of(&out), "[redacted:password]");
        let (out, _) = record(
            &AgentAction::SelectRef {
                snapshot_id: "snap_x".to_owned(),
                generation: 1,
                ref_id: "@e2".to_owned(),
                value: "Norway".to_owned(),
            },
            None,
            RecordPolicy::AllTyped,
        );
        assert!(matches!(out, AgentAction::SelectRef { ref value, .. } if value == "[redacted:secret]"));
    }

    #[test]
    fn selector_type_with_password_selector_is_masked() {
        for selector in [
            "input[type=password]",
            "input[type=\"password\"]",
            "form [type='password']",
            "#password",
            "input[name=otp]",
        ] {
            let (out, _) = record(
                &AgentAction::Type {
                    selector: selector.to_owned(),
                    text: password(),
                },
                None,
                RecordPolicy::SensitiveOnly,
            );
            assert!(
                !text_of(&out).contains(&password()),
                "selector {selector} left the text"
            );
        }
        // An unremarkable selector keeps the text.
        let (out, _) = record(
            &AgentAction::Type {
                selector: "#q".to_owned(),
                text: "shoes".to_owned(),
            },
            None,
            RecordPolicy::SensitiveOnly,
        );
        assert_eq!(text_of(&out), "shoes");
    }

    #[test]
    fn jwt_typed_into_unclassified_field_is_masked_by_shape() {
        let (out, tally) = record(
            &type_ref(&format!("pasted {}", jwt())),
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
        );
        assert!(!text_of(&out).contains("eyJ"));
        assert!(text_of(&out).starts_with("pasted "));
        assert_eq!(tally.by_rule().get("shape.jwt"), Some(&1));
    }

    #[test]
    fn select_and_dialog_prompt_are_masked() {
        let (out, _) = record(
            &AgentAction::Select {
                selector: "#plan".to_owned(),
                value: jwt(),
            },
            None,
            RecordPolicy::SensitiveOnly,
        );
        assert!(!serde_json::to_string(&out).unwrap().contains("eyJ"));
        let dialog = AgentAction::RespondDialog {
            dialog_id: "dlg_1".to_owned(),
            accept: true,
            approval_grant_id: "grant_x".to_owned(),
            prompt_text: Some(jwt()),
        };
        let (out, _) = record(&dialog, None, RecordPolicy::SensitiveOnly);
        assert!(!serde_json::to_string(&out).unwrap().contains("eyJ"));
        // Under the strict policy any prompt is replaced.
        let plain = AgentAction::RespondDialog {
            dialog_id: "dlg_1".to_owned(),
            accept: true,
            approval_grant_id: "grant_x".to_owned(),
            prompt_text: Some("my name".to_owned()),
        };
        let (out, _) = record(&plain, None, RecordPolicy::AllTyped);
        assert!(matches!(
            out,
            AgentAction::RespondDialog { prompt_text: Some(ref t), .. } if t == "[redacted:secret]"
        ));
    }

    #[test]
    fn navigate_url_query_token_is_masked() {
        let (out, _) = record(
            &AgentAction::Navigate {
                url: format!("https://example.test/cb?access_token={}&page=2", jwt()),
            },
            None,
            RecordPolicy::SensitiveOnly,
        );
        let AgentAction::Navigate { url } = out else {
            panic!("not a navigation");
        };
        assert!(!url.contains("eyJ"));
        assert!(url.contains("example.test/cb"));
        assert!(url.contains("page=2"));
    }

    #[test]
    fn the_human_branch_is_the_stricter_one_and_ignores_the_policy() {
        let mut tally = Tally::new();
        let out = redacted_for_record(
            RecordBranch::HumanMetadata,
            &type_ref("wireless headphones"),
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut tally,
        );
        assert_eq!(text_of(&out), REDACTED);
    }

    #[test]
    fn off_mode_changes_nothing_and_shadow_counts_only() {
        let action = type_ref(&password());
        let mut tally = Tally::new();
        let off = redacted_for_record(
            RecordBranch::Typed,
            &action,
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
            Mode::Off,
            &mut tally,
        );
        assert_eq!(text_of(&off), password());
        let shadow = redacted_for_record(
            RecordBranch::Typed,
            &action,
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
            Mode::Shadow,
            &mut tally,
        );
        assert_eq!(text_of(&shadow), password());
        assert!(!tally.is_empty());
    }

    #[test]
    fn masking_is_idempotent() {
        let (once, _) = record(
            &type_ref(&password()),
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
        );
        let (twice, tally) = record(&once, None, RecordPolicy::SensitiveOnly);
        assert_eq!(text_of(&once), text_of(&twice));
        assert!(tally.is_empty());
    }

    #[test]
    fn record_policy_parses_the_two_words() {
        assert_eq!(RecordPolicy::parse(" Sensitive "), Some(RecordPolicy::SensitiveOnly));
        assert_eq!(RecordPolicy::parse("ALL"), Some(RecordPolicy::AllTyped));
        assert_eq!(RecordPolicy::parse("metadata"), None);
        assert_eq!(RecordPolicy::default().as_str(), "sensitive");
    }

    #[test]
    fn human_receipt_redacts_typed_text_and_selected_values() {
        let typed = redacted_for_human_receipt(&AgentAction::Type {
            selector: "#pw".to_owned(),
            text: "hunter2".to_owned(),
        });
        assert!(matches!(typed, AgentAction::Type { ref text, .. } if text == REDACTED));

        let selected = redacted_for_human_receipt(&AgentAction::SelectRef {
            snapshot_id: "snap_x".to_owned(),
            generation: 1,
            ref_id: "r1".to_owned(),
            value: "secret-option".to_owned(),
        });
        assert!(matches!(selected, AgentAction::SelectRef { ref value, .. } if value == REDACTED));

        let pressed = redacted_for_human_receipt(&AgentAction::Press { key: "Enter".to_owned() });
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
        let redacted = redacted_for_human_receipt(&AgentAction::UploadRef {
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
            redacted_for_human_receipt(&click),
            AgentAction::Click { ref selector } if selector == "#go"
        ));
    }
}
