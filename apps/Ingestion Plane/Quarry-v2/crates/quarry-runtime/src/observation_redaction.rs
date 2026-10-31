//! WS12.S2: observation-level secret masking.
//!
//! `ObservationRunner::execute` builds a [`BrowserObservation`] from page
//! content. Everything it returns to a caller passes the `Live` profile of the
//! rule module (`quarry_core::redaction`), and everything it emits or stores
//! beyond the caller (events, JSON artifacts) passes `Durable`. This module
//! holds the pieces so `observation.rs` only calls them:
//!
//! * [`Redactor`] masks the parts of one observation and counts what it did;
//!   [`Redactor::stamp`] turns the counts into the [`RedactionStamp`] the
//!   observation carries (counts per static rule id, never content, never
//!   lengths).
//! * [`action_for_event`] is the action as the `ActionStarted` event may carry
//!   it: typed text is masked by the field kind of the control it went to.
//! * [`redact_json_artifact`] masks a JSON artifact before it is written.
//!
//! The mode (`enforce`, `shadow`, `off`) is the process-wide one set at boot;
//! every function here also exists with an explicit mode so a test never has to
//! touch the process-wide value.

use std::borrow::Cow;

use quarry_browser::{BrowserNativeProjection, ProjectionRedaction};
use quarry_core::contracts::{
    AgentAction, BrowserDialog, ChallengeSignal, ConsoleLine, DomSummary, ExtractionResult,
};
use quarry_core::redaction::action::{
    redacted_for_record, selector_kind, RecordBranch, RecordPolicy,
};
use quarry_core::redaction::field::{mask_with, Classification};
use quarry_core::redaction::{
    self, redact_json_with, redact_text_with, redact_url_with, FieldKind, Mode, ProbeState,
    Profile, RedactionStamp, ScreenshotState, Tally,
};
use serde_json::Value;

/// Roles that take typed text, and so may hold a secret.
const EDITABLE_ROLES: &[&str] = &["textbox", "searchbox", "combobox", "spinbutton"];

fn is_editable(role: Option<&str>) -> bool {
    role.is_some_and(|role| EDITABLE_ROLES.iter().any(|e| role.eq_ignore_ascii_case(e)))
}

/// The classification that records a field kind under its rule id.
fn classification(kind: FieldKind) -> Classification {
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

/// WS12.S5: what happened to the screenshot of one observation. Counts only: no
/// rectangle, no offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenshotReport {
    pub state: ScreenshotState,
    pub rects: u32,
    pub oopif_frames: u32,
    pub mask_ms: u32,
}

impl ScreenshotReport {
    /// The report of one masked (or withheld) screenshot.
    #[must_use]
    pub fn of(masked: &quarry_browser::MaskedScreenshot) -> Self {
        Self {
            state: masked.state,
            rects: u32::try_from(masked.rects.len()).unwrap_or(u32::MAX),
            oopif_frames: masked.oopif_frames,
            mask_ms: masked.mask_ms,
        }
    }
}

/// Masks the parts of one observation and counts what it masked.
#[derive(Debug)]
pub struct Redactor {
    mode: Mode,
    tally: Tally,
    /// What the driver's own masking pass reported for the native projection.
    projection: Option<ProjectionRedaction>,
    /// What the screenshot mask reported (WS12.S5); `None` when no screenshot was taken.
    screenshot: Option<ScreenshotReport>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    /// A redactor under the process-wide mode.
    #[must_use]
    pub fn new() -> Self {
        Self::with_mode(redaction::mode())
    }

    /// A redactor under an explicit mode.
    #[must_use]
    pub fn with_mode(mode: Mode) -> Self {
        Self {
            mode,
            tally: Tally::new(),
            projection: None,
            screenshot: None,
        }
    }

    /// Record what happened to this observation's screenshot.
    pub fn screenshot(&mut self, report: ScreenshotReport) {
        self.screenshot = Some(report);
    }

    /// Free text returned to a caller: the high-confidence shapes only.
    pub fn live(&mut self, input: &str) -> String {
        redact_text_with(self.mode, input, Profile::Live, &mut self.tally).into_owned()
    }

    fn live_opt(&mut self, value: &mut Option<String>) {
        if let Some(text) = value.take() {
            *value = Some(self.live(&text));
        }
    }

    /// A URL returned to a caller: userinfo secrets only. The query stays, the
    /// caller needs a working address.
    pub fn live_url(&mut self, input: &str) -> String {
        redact_url_with(self.mode, input, Profile::Live, &mut self.tally).into_owned()
    }

    /// The native projection: the driver's accessibility nodes and executable
    /// targets.
    ///
    /// A projection the driver masked (`redaction` is set) is kept as it is and
    /// its counts are folded into the stamp. A projection that no masking pass
    /// produced fails closed here: every editable control's value becomes
    /// `[redacted:secret]` and every other text passes the shape rules, so a
    /// driver without the field-facts probe can never return a typed secret.
    pub fn projection(&mut self, projection: &mut BrowserNativeProjection) {
        if let Some(done) = projection.redaction.clone() {
            self.projection = Some(done);
            return;
        }
        let secret = classification(FieldKind::Secret);
        let mode = self.mode;
        if let Some(accessibility) = projection.accessibility.as_mut() {
            for node in &mut accessibility.nodes {
                let editable = is_editable(node.role.as_deref());
                if let Some(value) = node.value.take() {
                    node.value = Some(self.value(editable.then_some(secret), &value));
                }
                self.live_opt(&mut node.name);
            }
        }
        // The executable targets carry the same text as the nodes; they are
        // masked but not counted a second time.
        let mut scratch = Tally::new();
        for target in &mut projection.targets {
            let editable = is_editable(target.role.as_deref());
            if editable {
                target.field_kind = Some(FieldKind::Secret);
            }
            if let Some(value) = target.value.take() {
                let masked = if editable {
                    mask_with(mode, secret, &value, &mut scratch).into_owned()
                } else {
                    value
                };
                target.value =
                    Some(redact_text_with(mode, &masked, Profile::Live, &mut scratch).into_owned());
            }
            if let Some(name) = target.name.take() {
                target.name =
                    Some(redact_text_with(mode, &name, Profile::Live, &mut scratch).into_owned());
            }
        }
        self.projection = Some(ProjectionRedaction::default());
    }

    /// One value: the field marker first (when the field is sensitive), then the
    /// shape rules over what is left.
    fn value(&mut self, class: Option<Classification>, value: &str) -> String {
        let masked = match class {
            Some(class) => mask_with(self.mode, class, value, &mut self.tally),
            None => Cow::Borrowed(value),
        };
        redact_text_with(self.mode, &masked, Profile::Live, &mut self.tally).into_owned()
    }

    /// The scanner summary of the page: the visible-text snippet, the element
    /// texts and names, the click map names, and the text inside each element's
    /// fingerprint (its id, a hash, is left alone, so a snapshot target still
    /// verifies).
    pub fn dom_summary(&mut self, summary: &mut DomSummary) {
        self.live_opt(&mut summary.text_snippet);
        for element in &mut summary.interactive_elements {
            self.live_opt(&mut element.text);
            self.live_opt(&mut element.aria_label);
            self.live_opt(&mut element.accessible_name);
            self.live_opt(&mut element.placeholder);
            if let Some(fingerprint) = element.fingerprint.as_mut() {
                fingerprint.normalized_text = self.live(&fingerprint.normalized_text);
                for (_, value) in &mut fingerprint.attributes {
                    *value = self.live(value);
                }
            }
        }
        if let Some(click_map) = summary.click_map.as_mut() {
            for row in click_map {
                self.live_opt(&mut row.name);
            }
        }
    }

    /// The page title.
    pub fn title(&mut self, title: &mut Option<String>) {
        self.live_opt(title);
    }

    /// Console lines.
    pub fn console(&mut self, lines: &mut [ConsoleLine]) {
        for line in lines {
            line.text = self.live(&line.text);
        }
    }

    /// Open JavaScript dialogs: the message and the prompt default.
    pub fn dialogs(&mut self, dialogs: &mut [BrowserDialog]) {
        for dialog in dialogs {
            dialog.message = self.live(&dialog.message);
            self.live_opt(&mut dialog.default_prompt);
        }
    }

    /// Extracted field values: high-confidence shapes only, so an extraction
    /// profile for ordinary page data keeps working.
    pub fn extraction(&mut self, result: &mut ExtractionResult) {
        for field in &mut result.fields {
            if let Some(value) = field.value.as_mut() {
                redact_json_with(self.mode, value, Profile::Live, &mut self.tally);
            }
        }
    }

    /// The text a challenge signal quotes as its evidence.
    pub fn challenge(&mut self, signal: &mut ChallengeSignal) {
        for line in &mut signal.evidence {
            *line = self.live(line);
        }
    }

    /// Nothing was masked.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.tally.is_empty()
            && self
                .projection
                .as_ref()
                .is_none_or(|p| p.values_masked == 0)
    }

    /// The stamp for the observation: the rule version, `Live`, the mode, the
    /// counts per rule id (this pass plus the driver's), and the probe state.
    #[must_use]
    pub fn stamp(&self) -> RedactionStamp {
        let mut stamp = self.tally.stamp(Profile::Live, self.mode);
        if let Some(projection) = &self.projection {
            stamp.values_masked = stamp.values_masked.saturating_add(projection.values_masked);
            for (rule, count) in &projection.by_rule {
                let entry = stamp.by_rule.entry(rule.clone()).or_insert(0);
                *entry = entry.saturating_add(*count);
            }
            stamp.probe = projection.probe;
            stamp.probe_ms = projection.probe_ms;
        } else {
            stamp.probe = ProbeState::Skipped;
        }
        if let Some(report) = &self.screenshot {
            stamp.screenshot = report.state;
            stamp.rects = report.rects;
            stamp.oopif_frames = report.oopif_frames;
            stamp.mask_ms = report.mask_ms;
        }
        stamp
    }
}

/// The action as the `ActionStarted` event may carry it (`Durable`).
///
/// Typed text is masked by the kind of the control it went to (`typed`, from
/// `typed_field_kind`): a password, an OTP, a card number, a phone number or an
/// unprobed field becomes `[redacted:<kind>]`; text for any other control keeps
/// its words but passes the `Durable` rules. A raw `type` action names its
/// control by selector only, so the selector's own words (`#password`) decide;
/// when they say nothing either (`#q`, `input[name=x1]`) the control is not known
/// and the text is masked as a secret, as a ref-addressed action is (D2-064). A key
/// press that types a character is masked by the control the run last focused
/// (`typed`; D2-065).
///
/// `policy` is the process-wide receipt text policy (`record_policy()`), the same
/// one the stored receipt is built under (X-19): under `all` an ordinary typed
/// text is replaced in the event too.
/// A navigation loses its secret query and userinfo; a selected value, a dialog
/// prompt and a script pass the `Durable` rules.
#[must_use]
pub fn action_for_event(
    action: &AgentAction,
    typed: Option<FieldKind>,
    policy: RecordPolicy,
    mode: Mode,
    tally: &mut Tally,
) -> Option<Value> {
    let typed = match (action, typed) {
        (AgentAction::Type { selector, .. }, None)
            if selector_kind(selector) == FieldKind::Unknown =>
        {
            Some(FieldKind::Secret)
        }
        _ => typed,
    };
    // WS12.S3: the one function every stored or emitted action goes through (the
    // receipt builder calls it too), under the same policy.
    let out = redacted_for_record(RecordBranch::Typed, action, typed, policy, mode, tally);
    serde_json::to_value(&out).ok()
}

/// A URL for an event (`Durable`): secret query and fragment values and userinfo
/// are masked; the scheme, host, path and key names stay.
#[must_use]
pub fn event_url(url: &str, mode: Mode, tally: &mut Tally) -> String {
    redact_url_with(mode, url, Profile::Durable, tally).into_owned()
}

/// A JSON artifact before it is written: every string leaf (and the value of
/// every secret-named key) passes the `Durable` rules.
///
/// # Errors
/// The value does not serialise.
pub fn redact_json_artifact<T: serde::Serialize + ?Sized>(
    value: &T,
    mode: Mode,
    tally: &mut Tally,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut json = serde_json::to_value(value)?;
    redact_json_with(mode, &mut json, Profile::Durable, tally);
    serde_json::to_vec(&json)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bytes::Bytes;
    use quarry_browser::{BrowserDriver, BrowserEgressPolicy, BrowserNativeTarget};
    use quarry_core::contracts::{
        AccessibilityNode, AccessibilityProjection, AgentActionRequest, AgentConstraints,
        ElementFingerprint, ExtractionFieldResult, InteractiveElement,
    };
    use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
    use quarry_core::event::{Event, EventType};
    use quarry_core::ids::Id;
    use quarry_core::lease::{BrowserLease, ProxyAffinity};
    use quarry_core::redaction::RULE_VERSION;
    use quarry_core::zdr::ZdrMode;
    use tokio::sync::mpsc;

    use super::*;
    use crate::events::EventSink;
    use crate::observation::{ObservationContext, ObservationRunner};

    // Assembled at run time so no secret-shaped literal exists in the repository.
    fn jwt() -> String {
        format!(
            "{}.{}.{}",
            ["eyJ", "hbGciOiJIUzI1NiJ9"].concat(),
            ["eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0"].concat(),
            ["SflKxwRJSM", "eKKF2QT4fwpMeJf36POk6yJV_adQssw5c"].concat()
        )
    }

    // Assembled at run time, like the token fragments, so no literal looks like a credential.
    fn password() -> String {
        ["S3cret", "P@ss-0451"].concat()
    }

    fn node(id: &str, role: &str, name: &str, value: Option<&str>) -> AccessibilityNode {
        AccessibilityNode {
            node_id: id.to_owned(),
            role: Some(role.to_owned()),
            name: Some(name.to_owned()),
            value: value.map(str::to_owned),
            ignored: false,
            frame_id: None,
            child_ids: vec![],
        }
    }

    fn native_target(id: i64, role: &str, name: &str, value: Option<&str>) -> BrowserNativeTarget {
        BrowserNativeTarget {
            ax_node_id: format!("ax{id}"),
            backend_node_id: id,
            frame_id: None,
            role: Some(role.to_owned()),
            name: Some(name.to_owned()),
            value: value.map(str::to_owned),
            field_kind: None,
        }
    }

    fn unmasked_projection() -> BrowserNativeProjection {
        BrowserNativeProjection {
            accessibility: Some(AccessibilityProjection {
                source: "test".to_owned(),
                nodes: vec![
                    node("ax1", "textbox", "Password", Some(&password())),
                    node("ax2", "StaticText", "Token", Some(&jwt())),
                ],
                truncated: false,
            }),
            frames: vec![],
            targets: vec![native_target(1, "textbox", "Password", Some(&password()))],
            redaction: None,
        }
    }

    fn all_text(value: &impl serde::Serialize) -> String {
        serde_json::to_string(value).unwrap()
    }

    #[test]
    fn an_unmasked_projection_fails_closed_on_editable_values() {
        let mut projection = unmasked_projection();
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        redactor.projection(&mut projection);
        let nodes = &projection.accessibility.as_ref().unwrap().nodes;
        assert_eq!(nodes[0].value.as_deref(), Some("[redacted:secret]"));
        assert!(!all_text(&nodes).contains(&password()));
        assert!(!all_text(&nodes).contains(&jwt()));
        assert_eq!(
            projection.targets[0].value.as_deref(),
            Some("[redacted:secret]")
        );
        assert_eq!(projection.targets[0].field_kind, Some(FieldKind::Secret));
        let stamp = redactor.stamp();
        assert!(stamp.values_masked >= 2);
        assert_eq!(stamp.probe, ProbeState::Skipped);
        assert_eq!(stamp.rule_version, RULE_VERSION);
    }

    #[test]
    fn a_projection_the_driver_masked_is_kept_and_its_counts_are_folded_in() {
        let mut projection = unmasked_projection();
        projection.accessibility.as_mut().unwrap().nodes[0].value =
            Some("[redacted:password]".to_owned());
        projection.redaction = Some(ProjectionRedaction {
            probe: ProbeState::Ok,
            probe_ms: 7,
            values_masked: 1,
            by_rule: [("field.password".to_owned(), 1)].into_iter().collect(),
        });
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        redactor.projection(&mut projection);
        let stamp = redactor.stamp();
        assert_eq!(stamp.values_masked, 1);
        assert_eq!(stamp.by_rule.get("field.password"), Some(&1));
        assert_eq!(stamp.probe, ProbeState::Ok);
        assert_eq!(stamp.probe_ms, 7);
    }

    #[test]
    fn shadow_mode_counts_but_does_not_mutate() {
        let mut projection = unmasked_projection();
        let before = all_text(&projection.accessibility);
        let mut redactor = Redactor::with_mode(Mode::Shadow);
        redactor.projection(&mut projection);
        assert_eq!(all_text(&projection.accessibility), before);
        let stamp = redactor.stamp();
        assert_eq!(stamp.mode, Mode::Shadow);
        assert!(stamp.values_masked >= 2, "shadow still counts");
        let token = redactor.live(&format!("token {}", jwt()));
        assert!(token.contains(&jwt()), "shadow returns the input unchanged");
    }

    #[test]
    fn off_mode_does_nothing_and_counts_nothing() {
        let mut projection = unmasked_projection();
        let before = all_text(&projection.accessibility);
        let mut redactor = Redactor::with_mode(Mode::Off);
        redactor.projection(&mut projection);
        assert_eq!(all_text(&projection.accessibility), before);
        assert_eq!(redactor.stamp().values_masked, 0);
    }

    fn element(text: &str) -> InteractiveElement {
        InteractiveElement {
            tag: "button".to_owned(),
            selector: "#go".to_owned(),
            selector_alternatives: vec![],
            text: Some(text.to_owned()),
            role: Some("button".to_owned()),
            aria_label: Some(text.to_owned()),
            accessible_name: Some(text.to_owned()),
            placeholder: Some(text.to_owned()),
            test_id: None,
            fingerprint: Some(ElementFingerprint {
                fingerprint_id: "efp_keepme".to_owned(),
                tag: "button".to_owned(),
                normalized_text: text.to_ascii_lowercase(),
                attributes: vec![("aria-label".to_owned(), text.to_owned())],
                structural_path: "button[0]".to_owned(),
                logical_id: None,
            }),
            click_index: Some(1),
        }
    }

    #[test]
    fn the_dom_summary_masks_shapes_and_keeps_the_fingerprint_id() {
        let secret = jwt();
        let mut summary = DomSummary {
            node_count: 3,
            interactive_elements: vec![element(&secret)],
            text_snippet: Some(format!("welcome {secret} and a normal sentence")),
            click_map: Some(vec![quarry_core::contracts::ElementClickTarget {
                ref_id: "@e1".to_owned(),
                index: 1,
                tag: "button".to_owned(),
                selector: Some("#go".to_owned()),
                name: Some(secret.clone()),
            }]),
        };
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        redactor.dom_summary(&mut summary);
        let text = all_text(&summary);
        assert!(!text
            .to_ascii_lowercase()
            .contains(&secret.to_ascii_lowercase()));
        assert!(text.contains("a normal sentence"));
        assert_eq!(
            summary.interactive_elements[0]
                .fingerprint
                .as_ref()
                .unwrap()
                .fingerprint_id,
            "efp_keepme"
        );
        assert!(redactor.stamp().values_masked >= 4);
    }

    #[test]
    fn a_text_snippet_jwt_is_masked_live_but_a_labelled_secret_is_not() {
        // Live carries the high-confidence shapes only; the labelled-secret
        // contextual rule is Durable, so a page that says `password: x` in prose
        // is still readable to the caller.
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        let masked = redactor.live(&format!("jwt {} end", jwt()));
        assert!(masked.contains("[redacted:token]") || masked.contains("[redacted:"));
        assert!(!masked.contains(&jwt()));
        let prose = redactor.live("the password: hunter2 is shown in the docs");
        assert!(prose.contains("hunter2"));
        let mut tally = Tally::new();
        let durable = redact_text_with(
            Mode::Enforce,
            "the password: hunter2 is shown in the docs",
            Profile::Durable,
            &mut tally,
        );
        assert!(!durable.contains("hunter2"));
    }

    #[test]
    fn extraction_values_pass_the_live_shapes_only() {
        let mut result = ExtractionResult {
            profile_id: "p".to_owned(),
            fields: vec![ExtractionFieldResult {
                name: "token".to_owned(),
                value: Some(serde_json::json!({ "v": jwt(), "ok": "kept" })),
                source: None,
                evidence_selector: None,
            }],
            truncated: false,
        };
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        redactor.extraction(&mut result);
        let text = all_text(&result);
        assert!(!text.contains(&jwt()));
        assert!(text.contains("kept"));
    }

    #[test]
    fn the_stamp_carries_the_rule_version_and_counts_never_content() {
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        let _ = redactor.live(&format!("a {} b", jwt()));
        let stamp = redactor.stamp();
        let json = all_text(&stamp);
        assert_eq!(stamp.rule_version, RULE_VERSION);
        assert_eq!(stamp.profile, Profile::Live);
        assert!(stamp.values_masked >= 1);
        assert!(!json.contains(&jwt()));
        assert!(!json.contains("eyJ"));
    }

    #[test]
    fn action_for_event_masks_typed_text_by_field_kind() {
        let mut tally = Tally::new();
        let action = AgentAction::TypeRef {
            snapshot_id: "snap".to_owned(),
            generation: 1,
            ref_id: "@e1".to_owned(),
            text: password(),
        };
        let value = action_for_event(
            &action,
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut tally,
        )
        .unwrap();
        assert_eq!(value["text"], "[redacted:password]");
        assert!(!value.to_string().contains(&password()));
        assert_eq!(tally.by_rule().get("field.password"), Some(&1));
        // An ordinary field keeps its words.
        let value = action_for_event(
            &action,
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(value["text"], password());
    }

    #[test]
    fn the_event_follows_the_receipt_text_policy() {
        // X-19: an ordinary typed text stays under `sensitive` and is replaced under `all`,
        // in the event exactly as in the stored receipt.
        let action = AgentAction::TypeRef {
            snapshot_id: "snap".to_owned(),
            generation: 1,
            ref_id: "@e1".to_owned(),
            text: "wireless headphones".to_owned(),
        };
        let kept = action_for_event(
            &action,
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(kept["text"], "wireless headphones");
        let replaced = action_for_event(
            &action,
            Some(FieldKind::Unknown),
            RecordPolicy::AllTyped,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(replaced["text"], "[redacted:secret]");
        // The stored receipt of the same action and policy says the same.
        for policy in [RecordPolicy::SensitiveOnly, RecordPolicy::AllTyped] {
            let event = action_for_event(
                &action,
                Some(FieldKind::Unknown),
                policy,
                Mode::Enforce,
                &mut Tally::new(),
            )
            .unwrap();
            let stored = redacted_for_record(
                RecordBranch::Typed,
                &action,
                Some(FieldKind::Unknown),
                policy,
                Mode::Enforce,
                &mut Tally::new(),
            );
            assert_eq!(event, serde_json::to_value(&stored).unwrap(), "{policy:?}");
        }
    }

    #[test]
    fn a_key_press_in_an_event_is_masked_by_the_focused_control() {
        // D2-065: one character per press.
        let press = |key: &str| AgentAction::Press { key: key.to_owned() };
        let in_password = action_for_event(
            &press("x"),
            Some(FieldKind::Password),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(in_password["key"], "[redacted:password]");
        let unknown = action_for_event(
            &press("x"),
            None,
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(unknown["key"], "[redacted:secret]");
        let enter = action_for_event(
            &press("Enter"),
            None,
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(enter["key"], "Enter");
    }

    #[test]
    fn a_raw_type_action_is_classified_by_its_selector() {
        let action = AgentAction::Type {
            selector: "input#password".to_owned(),
            text: password(),
        };
        let value = action_for_event(
            &action,
            None,
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert!(!value.to_string().contains(&password()));
    }

    #[test]
    fn a_raw_type_action_with_no_known_control_is_masked_in_an_event() {
        // D2-064: the selector says nothing, the snapshot is not read for a raw
        // `type`, so the text may be a password: the event masks it.
        for selector in ["#q", "input[name=x1]", "div.field > input"] {
            let action = AgentAction::Type {
                selector: selector.to_owned(),
                text: password(),
            };
            let mut tally = Tally::new();
            let value = action_for_event(
                &action,
                None,
                RecordPolicy::SensitiveOnly,
                Mode::Enforce,
                &mut tally,
            )
            .unwrap();
            assert_eq!(value["text"], "[redacted:secret]", "{selector}");
            assert_eq!(tally.by_rule().get("field.label"), Some(&1), "{selector}");
        }
        // A control the probe decided is ordinary keeps its words.
        let action = AgentAction::Type {
            selector: "#q".to_owned(),
            text: "wireless headphones".to_owned(),
        };
        let value = action_for_event(
            &action,
            Some(FieldKind::Unknown),
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        assert_eq!(value["text"], "wireless headphones");
    }

    #[test]
    fn action_for_event_drops_the_secret_query_from_a_navigation() {
        let action = AgentAction::Navigate {
            url: format!("https://example.test/cb?access_token={}&page=2", jwt()),
        };
        let value = action_for_event(
            &action,
            None,
            RecordPolicy::SensitiveOnly,
            Mode::Enforce,
            &mut Tally::new(),
        )
        .unwrap();
        let text = value.to_string();
        assert!(!text.contains(&jwt()));
        assert!(text.contains("example.test/cb"));
        assert!(text.contains("page=2"));
    }

    #[test]
    fn put_json_artifact_masks_string_leaves() {
        let body = serde_json::json!({ "note": format!("t {}", jwt()), "n": 3 });
        let bytes = redact_json_artifact(&body, Mode::Enforce, &mut Tally::new()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains(&jwt()));
        assert!(text.contains("\"n\":3"));
    }

    // ---- the runner, end to end, through a fixture driver ----

    #[derive(Default)]
    struct FixtureDriver {
        html: &'static [u8],
        title: Option<&'static str>,
        projection: Option<BrowserNativeProjection>,
        console: Vec<String>,
    }

    #[async_trait::async_trait]
    impl BrowserDriver for FixtureDriver {
        async fn configure_egress_policy(
            &self,
            _session: &quarry_browser::BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        fn capabilities(&self) -> quarry_browser::BrowserDriverCapabilities {
            quarry_browser::BrowserDriverCapabilities {
                atomic_target_actions: true,
                ..Default::default()
            }
        }
        async fn acquire(
            &self,
            lease: &BrowserLease,
        ) -> QuarryResult<quarry_browser::BrowserSession> {
            Ok(quarry_browser::BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
                inner: Arc::new(tokio::sync::Mutex::new(
                    quarry_browser::SessionInner::default(),
                )),
            })
        }
        async fn release(&self, _session: quarry_browser::BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(
            &self,
            _session: &quarry_browser::BrowserSession,
            _url: &str,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn click(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
        ) -> QuarryResult<()> {
            Err(QuarryError::unsupported_action("click"))
        }
        async fn click_point(
            &self,
            _session: &quarry_browser::BrowserSession,
            _x: f64,
            _y: f64,
        ) -> QuarryResult<()> {
            Err(QuarryError::unsupported_action("click_point"))
        }
        async fn type_text(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
            _text: &str,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn press(
            &self,
            _session: &quarry_browser::BrowserSession,
            _key: &str,
        ) -> QuarryResult<()> {
            Err(QuarryError::unsupported_action("press"))
        }
        async fn select(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
            _value: &str,
        ) -> QuarryResult<()> {
            Err(QuarryError::unsupported_action("select"))
        }
        async fn content(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(self.html))
        }
        async fn screenshot(
            &self,
            _session: &quarry_browser::BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn back(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<()> {
            Err(QuarryError::unsupported_action("back"))
        }
        async fn evaluate(
            &self,
            _session: &quarry_browser::BrowserSession,
            _script: &str,
        ) -> QuarryResult<serde_json::Value> {
            Ok(serde_json::json!({
                "url": "https://example.test/login",
                "title": self.title,
            }))
        }
        async fn native_page_projection(
            &self,
            _session: &quarry_browser::BrowserSession,
        ) -> QuarryResult<BrowserNativeProjection> {
            self.projection
                .clone()
                .ok_or_else(|| QuarryError::unsupported_action("native_page_projection"))
        }
        async fn devtools_events(
            &self,
            _session: &quarry_browser::BrowserSession,
            _after: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<quarry_browser::BrowserDevtoolsEvent>> {
            Ok(self
                .console
                .iter()
                .enumerate()
                .map(|(i, text)| quarry_browser::BrowserDevtoolsEvent {
                    sequence: i as u64,
                    tab_id: None,
                    category: "console".to_owned(),
                    name: "Runtime.consoleAPICalled".to_owned(),
                    level: Some("log".to_owned()),
                    method: None,
                    url: None,
                    status: None,
                    text: Some(text.clone()),
                    timestamp_ms: 0,
                    payload: serde_json::Value::Null,
                })
                .collect())
        }
    }

    fn lease() -> BrowserLease {
        BrowserLease {
            lease_id: Id::new(),
            profile_id: Id::new(),
            session_affinity_key: "test".into(),
            proxy_affinity: ProxyAffinity {
                pool: String::new(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: String::new(),
            persist_profile: false,
            viewport: None,
            org_id: "org".into(),
        }
    }

    fn request(lease: &BrowserLease, action: AgentAction, zdr: ZdrMode) -> AgentActionRequest {
        AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action,
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 10,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr,
            extraction_profile: None,
        }
    }

    fn runner(driver: FixtureDriver, events: Option<EventSink>) -> ObservationRunner {
        ObservationRunner {
            browser: Arc::new(driver),
            artifacts: None,
            events,
            visual_processor: None,
            org_id: "org".into(),
        }
    }

    const LOGIN_HTML: &[u8] =
        b"<html><head><title>Sign in</title></head><body><p>Hello</p><input name=\"p\"></body></html>";

    async fn observe(
        driver: FixtureDriver,
        zdr: ZdrMode,
    ) -> quarry_core::contracts::BrowserObservation {
        let lease = lease();
        let runner = runner(driver, None);
        let session = runner.browser.acquire(&lease).await.unwrap();
        let mut ctx = ObservationContext::default();
        runner
            .execute(
                &request(
                    &lease,
                    AgentAction::Navigate {
                        url: "https://example.test/login".to_owned(),
                    },
                    zdr,
                ),
                &session,
                &mut ctx,
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn execute_masks_password_field_value_in_observation() {
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            title: Some("Sign in"),
            projection: Some(unmasked_projection()),
            ..Default::default()
        };
        let observation = observe(driver, ZdrMode::Off).await;
        let body = all_text(&observation);
        assert!(
            !body.contains(&password()),
            "the typed password must not be returned"
        );
        let stamp = observation
            .redaction
            .expect("the observation carries a stamp");
        assert_eq!(stamp.rule_version, RULE_VERSION);
        assert!(stamp.values_masked >= 1);
        assert_eq!(
            observation
                .proof_bundle
                .unwrap()
                .redaction_rule_version
                .as_deref(),
            Some(RULE_VERSION)
        );
    }

    #[tokio::test]
    async fn execute_masks_card_and_otp_and_phone_fields() {
        let mut projection = unmasked_projection();
        {
            let nodes = &mut projection.accessibility.as_mut().unwrap().nodes;
            nodes.push(node(
                "ax3",
                "textbox",
                "Card number",
                Some("4111111111111111"),
            ));
            nodes.push(node("ax4", "textbox", "One-time code", Some("482913")));
            nodes.push(node("ax5", "textbox", "Phone", Some("+4799999999")));
        }
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            projection: Some(projection),
            ..Default::default()
        };
        let observation = observe(driver, ZdrMode::Off).await;
        let body = all_text(&observation);
        for secret in ["4111111111111111", "482913", "+4799999999"] {
            assert!(!body.contains(secret), "{secret} must be masked");
        }
    }

    #[tokio::test]
    async fn execute_masks_under_zdr_on() {
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            title: Some("Sign in"),
            projection: Some(unmasked_projection()),
            console: vec![format!("debug {}", jwt())],
            ..Default::default()
        };
        let observation = observe(driver, ZdrMode::On).await;
        let body = all_text(&observation);
        assert!(!body.contains(&password()));
        assert!(!body.contains(&jwt()));
        assert!(observation.redaction.is_some());
    }

    #[tokio::test]
    async fn execute_text_snippet_jwt_is_masked_live_but_labelled_secret_is_not() {
        let html: &'static [u8] = Box::leak(
            format!(
                "<html><body><p>session {}</p><p>the password: hunter2 is shown here</p></body></html>",
                jwt()
            )
            .into_bytes()
            .into_boxed_slice(),
        );
        let driver = FixtureDriver {
            html,
            projection: None,
            ..Default::default()
        };
        let observation = observe(driver, ZdrMode::Off).await;
        let snippet = observation.dom_summary.unwrap().text_snippet.unwrap();
        assert!(!snippet.contains(&jwt()));
        assert!(snippet.contains("hunter2"));
    }

    #[tokio::test]
    async fn execute_stamp_carries_rule_version_and_counts_not_content() {
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            projection: Some(unmasked_projection()),
            ..Default::default()
        };
        let observation = observe(driver, ZdrMode::Off).await;
        let stamp = observation.redaction.unwrap();
        let json = all_text(&stamp);
        assert!(!json.contains(&password()));
        assert!(stamp.by_rule.values().sum::<u32>() >= 1);
    }

    #[tokio::test]
    async fn action_started_event_payload_has_no_typed_text() {
        let (tx, mut rx) = mpsc::channel::<Event>(16);
        let lease = lease();
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            ..Default::default()
        };
        let runner = runner(driver, Some(EventSink::new(tx)));
        let session = runner.browser.acquire(&lease).await.unwrap();
        let mut ctx = ObservationContext::default();
        runner
            .execute(
                &request(
                    &lease,
                    AgentAction::Type {
                        selector: "input#password".to_owned(),
                        text: password(),
                    },
                    ZdrMode::Off,
                ),
                &session,
                &mut ctx,
            )
            .await
            .unwrap();
        let mut saw_started = false;
        while let Ok(event) = rx.try_recv() {
            let payload = event.payload.to_string();
            assert!(!payload.contains(&password()), "{payload}");
            if event.event_type == EventType::ActionStarted {
                saw_started = true;
            }
        }
        assert!(saw_started);
    }

    #[tokio::test]
    async fn observation_ready_event_url_has_no_token() {
        let (tx, mut rx) = mpsc::channel::<Event>(16);
        let lease = lease();
        let driver = FixtureDriver {
            html: LOGIN_HTML,
            ..Default::default()
        };
        let runner = runner(driver, Some(EventSink::new(tx)));
        let session = runner.browser.acquire(&lease).await.unwrap();
        let mut ctx = ObservationContext::default();
        let url = format!("https://example.test/cb?access_token={}", jwt());
        runner
            .execute(
                &request(&lease, AgentAction::Navigate { url }, ZdrMode::Off),
                &session,
                &mut ctx,
            )
            .await
            .unwrap();
        let mut saw_ready = false;
        while let Ok(event) = rx.try_recv() {
            assert!(!event.payload.to_string().contains(&jwt()));
            if event.event_type == EventType::ObservationReady {
                saw_ready = true;
            }
        }
        assert!(saw_ready);
    }

    #[test]
    fn a_masked_title_does_not_change_what_a_challenge_looks_for() {
        // The classifier matches words ("verify you are human"), which no rule
        // masks; a title that is itself a token is the only case a mask changes.
        let mut redactor = Redactor::with_mode(Mode::Enforce);
        let mut title = Some("Verify you are human".to_owned());
        redactor.title(&mut title);
        assert_eq!(title.as_deref(), Some("Verify you are human"));
        assert!(redactor.is_clean());
    }

    #[test]
    fn the_error_envelope_is_masked_by_redacted() {
        let error = QuarryError::new(ErrorCode::DriverFailed, format!("driver said {}", jwt()))
            .with_details(serde_json::json!({ "echo": jwt() }))
            .redacted();
        assert!(!error.message.contains(&jwt()));
        assert!(!all_text(&error.details).contains(&jwt()));
        assert_eq!(error.code, ErrorCode::DriverFailed);
    }
}
