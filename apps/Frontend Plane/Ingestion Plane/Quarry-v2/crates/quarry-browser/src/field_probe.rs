//! WS12.S1: the field-facts probe and the masking of accessibility and target
//! values.
//!
//! The accessibility tree says what a control is (a textbox), not what it holds
//! (a password, a card number, a plain search term). This module asks the page
//! for the attributes of every editable control (`type`, `autocomplete`, labels)
//! and masks the value of each control that is, or might be, a secret. The probe
//! reads attributes only: the value is the projection's own accessibility value,
//! masked before it leaves the driver.
//!
//! Fail closed on values only. An editable control whose facts could not be
//! read (probe off, timeout, CDP error, over the node cap) keeps no value: its
//! value becomes `[redacted:secret]`. The name is never masked by field kind;
//! names and every other text pass only the shape rules (`redact_text`, Live).
//!
//! The flag is `QUARRY_BROWSER_FIELD_PROBE` (default on). `off` skips the probe
//! and masks every editable value (the "off" case in SECRET_REDACTION.md).

// Without the Chromium feature nothing calls the probe, so the masking helpers
// are exercised only by the unit tests.
#![cfg_attr(not(feature = "chromiumoxide"), allow(dead_code))]

use std::collections::HashMap;
use std::time::Duration;

use quarry_core::redaction::field::{classify, mask_with, Classification, FieldFacts, FieldKind};
use quarry_core::redaction::{redact_text_with, Mode, ProbeState, Profile, Tally};
use serde_json::Value;

use crate::{BrowserNativeProjection, ProjectionRedaction};

/// Most editable controls one observation probes. Controls past the cap are
/// not probed and are therefore masked (fail closed).
pub const PROBE_NODE_CAP: usize = 64;

/// Wall-clock budget of one probe, end to end.
pub const PROBE_BUDGET: Duration = Duration::from_millis(400);

/// Roles that take typed text, and so may hold a secret.
const EDITABLE_ROLES: &[&str] = &["textbox", "searchbox", "combobox", "spinbutton"];

/// One editable control the probe should read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTarget {
    pub ax_node_id: String,
    pub backend_node_id: i64,
    pub frame_id: Option<String>,
}

/// Whether the probe runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeSetting {
    On,
    Off,
}

/// Parse the flag value. Only `off`, `0`, `false` and `no` turn the probe off;
/// anything else (including unset) leaves it on, the default.
#[must_use]
pub fn setting_from_env_value(raw: Option<&str>) -> ProbeSetting {
    match raw.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if matches!(value.as_str(), "off" | "0" | "false" | "no") => ProbeSetting::Off,
        _ => ProbeSetting::On,
    }
}

/// The flag as the process reads it.
#[must_use]
pub fn probe_setting() -> ProbeSetting {
    setting_from_env_value(std::env::var("QUARRY_BROWSER_FIELD_PROBE").ok().as_deref())
}

/// What the probe found, per backend node id.
#[derive(Debug, Clone, Default)]
pub struct ProbeOutcome {
    pub state: ProbeState,
    pub probe_ms: u32,
    pub facts: HashMap<i64, FieldFacts>,
}

impl ProbeOutcome {
    /// Set the state from how many controls were attempted and how many were read.
    #[cfg(feature = "chromiumoxide")]
    fn finish(&mut self, attempted: usize, elapsed: Duration) {
        self.probe_ms = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
        self.state = if attempted == 0 {
            ProbeState::Skipped
        } else if self.facts.len() >= attempted {
            ProbeState::Ok
        } else if self.facts.is_empty() {
            ProbeState::Failed
        } else {
            ProbeState::Partial
        };
    }
}

fn is_editable_role(role: Option<&str>) -> bool {
    role.is_some_and(|role| EDITABLE_ROLES.iter().any(|e| role.eq_ignore_ascii_case(e)))
}

fn ax_text(value: Option<&Value>) -> Option<&str> {
    value?.get("value")?.as_str()
}

/// The editable, non-ignored, DOM-backed nodes of a raw `Accessibility.getFullAXTree`
/// result, in tree order, at most [`PROBE_NODE_CAP`].
#[must_use]
pub fn collect_probe_targets(ax_tree: &Value) -> Vec<ProbeTarget> {
    ax_tree
        .get("nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|node| {
            !node
                .get("ignored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter(|node| is_editable_role(ax_text(node.get("role"))))
        .filter_map(|node| {
            Some(ProbeTarget {
                ax_node_id: node.get("nodeId")?.as_str()?.to_owned(),
                backend_node_id: node.get("backendDOMNodeId")?.as_i64()?,
                frame_id: node
                    .get("frameId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .take(PROBE_NODE_CAP)
        .collect()
}

/// Facts from a flat `DOM.describeNode` attribute list (`name, value, name, value, ...`).
#[must_use]
pub fn facts_from_attributes(attributes: &[String]) -> FieldFacts {
    let mut facts = FieldFacts::default();
    for pair in attributes.chunks_exact(2) {
        let value = Some(pair[1].clone());
        match pair[0].as_str() {
            "type" => facts.input_type = value,
            "autocomplete" => facts.autocomplete = value,
            "name" => facts.name_attr = value,
            "id" => facts.id_attr = value,
            "aria-label" => facts.aria_label = value,
            "placeholder" => facts.placeholder = value,
            _ => {}
        }
    }
    facts
}

/// Facts from the object the fallback probe function returns. `None` when the
/// page did not return an element (a null) or the shape is wrong: that is a
/// failed probe, not an empty field.
#[must_use]
pub fn facts_from_call_result(value: &Value) -> Option<FieldFacts> {
    if !value.is_object() {
        return None;
    }
    serde_json::from_value::<FieldFacts>(value.clone()).ok()
}

/// The classification of one editable control. With the probe off, or without
/// facts for the control, the verdict is `Secret`: fail closed.
fn verdict(
    setting: ProbeSetting,
    probe: &ProbeOutcome,
    backend_node_id: Option<i64>,
    name: Option<&str>,
    value: Option<&str>,
) -> Classification {
    let facts = match (setting, backend_node_id) {
        (ProbeSetting::On, Some(id)) => probe.facts.get(&id),
        _ => None,
    };
    let Some(facts) = facts else {
        return Classification {
            kind: FieldKind::Secret,
            rule: Some("field.label"),
        };
    };
    let mut facts = facts.clone();
    facts.accessible_name = name.map(str::to_owned);
    facts.value = value.map(str::to_owned);
    classify(&facts)
}

/// A value of an editable control: the field marker when classified sensitive,
/// then the shape rules (Live) over whatever is left.
fn scrub_value(
    mode: Mode,
    class: Option<Classification>,
    value: &str,
    tally: &mut Tally,
) -> String {
    let masked = match class {
        Some(class) => mask_with(mode, class, value, tally).into_owned(),
        None => value.to_owned(),
    };
    redact_text_with(mode, &masked, Profile::Live, tally).into_owned()
}

/// Any other text (names, static text): the shape rules only.
fn scrub_text(mode: Mode, text: &str, tally: &mut Tally) -> String {
    redact_text_with(mode, text, Profile::Live, tally).into_owned()
}

fn scrub_projection_pass(
    projection: &mut BrowserNativeProjection,
    probe: &ProbeOutcome,
    setting: ProbeSetting,
    mode: Mode,
) -> Tally {
    let mut tally = Tally::new();
    let backend_by_ax: HashMap<String, i64> = projection
        .targets
        .iter()
        .map(|target| (target.ax_node_id.clone(), target.backend_node_id))
        .collect();
    if let Some(accessibility) = projection.accessibility.as_mut() {
        for node in &mut accessibility.nodes {
            let class = is_editable_role(node.role.as_deref()).then(|| {
                verdict(
                    setting,
                    probe,
                    backend_by_ax.get(&node.node_id).copied(),
                    node.name.as_deref(),
                    node.value.as_deref(),
                )
            });
            node.value = node
                .value
                .take()
                .map(|value| scrub_value(mode, class, &value, &mut tally));
            node.name = node
                .name
                .take()
                .map(|name| scrub_text(mode, &name, &mut tally));
        }
    }
    tally
}

/// Mask the projection in place and return the stamp for it. Every AX node
/// counts once in the tally; the executable targets carry the same masked text
/// but are not counted a second time.
pub fn mask_projection(
    projection: &mut BrowserNativeProjection,
    probe: &ProbeOutcome,
    setting: ProbeSetting,
    mode: Mode,
) -> ProjectionRedaction {
    let tally = scrub_projection_pass(projection, probe, setting, mode);
    let mut scratch = Tally::new();
    for target in &mut projection.targets {
        let class = is_editable_role(target.role.as_deref()).then(|| {
            verdict(
                setting,
                probe,
                Some(target.backend_node_id),
                target.name.as_deref(),
                target.value.as_deref(),
            )
        });
        target.field_kind = class.map(|class| class.kind);
        target.value = target
            .value
            .take()
            .map(|value| scrub_value(mode, class, &value, &mut scratch));
        target.name = target
            .name
            .take()
            .map(|name| scrub_text(mode, &name, &mut scratch));
    }
    let redaction = ProjectionRedaction {
        probe: match setting {
            ProbeSetting::On => probe.state,
            ProbeSetting::Off => ProbeState::Skipped,
        },
        probe_ms: match setting {
            ProbeSetting::On => probe.probe_ms,
            ProbeSetting::Off => 0,
        },
        values_masked: tally.values_masked(),
        by_rule: tally
            .by_rule()
            .iter()
            .map(|(rule, count)| ((*rule).to_owned(), *count))
            .collect(),
    };
    projection.redaction = Some(redaction.clone());
    redaction
}

/// The function the fallback probe runs on the resolved element. It reads
/// attributes only; it never reads `value`.
#[cfg(feature = "chromiumoxide")]
const PROBE_FUNCTION: &str = r#"function() {
    if (!(this instanceof Element)) return null;
    const attr = (name) => this.getAttribute(name);
    return {
        input_type: attr('type'),
        autocomplete: attr('autocomplete'),
        name_attr: attr('name'),
        id_attr: attr('id'),
        aria_label: attr('aria-label'),
        placeholder: attr('placeholder'),
    };
}"#;

/// Run the probe against a live page: `DOM.describeNode` first, and the
/// `DOM.resolveNode` + `Runtime.callFunctionOn` sequence when describe fails.
/// Controls are probed concurrently. The whole run stops at `budget`; controls
/// read before the stop keep their facts and the rest are unprobed.
#[cfg(feature = "chromiumoxide")]
pub async fn run_probe(
    page: &::chromiumoxide::Page,
    targets: &[ProbeTarget],
    budget: Duration,
) -> ProbeOutcome {
    use futures::stream::FuturesUnordered;
    use futures::StreamExt;

    let started = std::time::Instant::now();
    let mut outcome = ProbeOutcome::default();
    let mut pending: FuturesUnordered<_> = targets
        .iter()
        .map(|target| probe_one(page, target))
        .collect();
    let _ = tokio::time::timeout(budget, async {
        while let Some((backend_node_id, facts)) = pending.next().await {
            if let Some(facts) = facts {
                outcome.facts.insert(backend_node_id, facts);
            }
        }
    })
    .await;
    outcome.finish(targets.len(), started.elapsed());
    outcome
}

#[cfg(feature = "chromiumoxide")]
async fn probe_one(
    page: &::chromiumoxide::Page,
    target: &ProbeTarget,
) -> (i64, Option<FieldFacts>) {
    let facts = match describe_facts(page, target.backend_node_id).await {
        Some(facts) => Some(facts),
        None => call_facts(page, target).await,
    };
    (target.backend_node_id, facts)
}

#[cfg(feature = "chromiumoxide")]
async fn describe_facts(page: &::chromiumoxide::Page, backend_node_id: i64) -> Option<FieldFacts> {
    use ::chromiumoxide::cdp::browser_protocol::dom::{BackendNodeId, DescribeNodeParams};

    let params = DescribeNodeParams::builder()
        .backend_node_id(BackendNodeId::new(backend_node_id))
        .build();
    let reply = page.execute(params).await.ok()?;
    let attributes = reply.result.node.attributes.unwrap_or_default();
    Some(facts_from_attributes(&attributes))
}

#[cfg(feature = "chromiumoxide")]
async fn call_facts(page: &::chromiumoxide::Page, target: &ProbeTarget) -> Option<FieldFacts> {
    use ::chromiumoxide::cdp::browser_protocol::dom::{BackendNodeId, ResolveNodeParams};
    use ::chromiumoxide::cdp::browser_protocol::page::FrameId;
    use ::chromiumoxide::cdp::js_protocol::runtime::{CallFunctionOnParams, ReleaseObjectParams};

    let mut resolve =
        ResolveNodeParams::builder().backend_node_id(BackendNodeId::new(target.backend_node_id));
    if let Some(frame_id) = target.frame_id.as_deref() {
        // Same frame lookup as the native target actions: the element is
        // resolved in its own frame's realm, never the top page's.
        let context = page
            .frame_execution_context(FrameId::new(frame_id))
            .await
            .ok()??;
        resolve = resolve.execution_context_id(context);
    }
    let resolved = page.execute(resolve.build()).await.ok()?;
    let object_id = resolved.result.object.object_id.clone()?;
    let call = CallFunctionOnParams::builder()
        .function_declaration(PROBE_FUNCTION)
        .object_id(object_id.clone())
        .return_by_value(true)
        .build()
        .ok();
    let facts = match call {
        Some(call) => match page.execute(call).await {
            Ok(reply) => reply
                .result
                .result
                .value
                .as_ref()
                .and_then(facts_from_call_result),
            Err(_) => None,
        },
        None => None,
    };
    // The handle never outlives the probe. Bounded: a dialog can hold the
    // renderer, and the outer probe budget bounds the run anyway.
    let _ = tokio::time::timeout(
        Duration::from_millis(500),
        page.execute(ReleaseObjectParams::new(object_id)),
    )
    .await;
    facts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrowserNativeTarget;
    use quarry_core::contracts::{AccessibilityNode, AccessibilityProjection};
    use serde_json::json;

    /// A JWT-shaped string, joined so that no token-shaped literal is in the source.
    const JWT: &str = concat!(
        "eyJhbGciOiJIUzI1NiJ9.",
        "eyJzdWIiOiIxMjM0NTY3ODkwIn0.",
        "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"
    );
    const PASSWORD: &str = concat!("S3cret", "P@ss-0451");

    fn node(id: &str, role: &str, name: Option<&str>, value: Option<&str>) -> AccessibilityNode {
        AccessibilityNode {
            node_id: id.to_owned(),
            role: Some(role.to_owned()),
            name: name.map(str::to_owned),
            value: value.map(str::to_owned),
            ignored: false,
            frame_id: None,
            child_ids: Vec::new(),
        }
    }

    fn target(
        ax_node_id: &str,
        backend: i64,
        role: &str,
        name: Option<&str>,
        value: Option<&str>,
    ) -> BrowserNativeTarget {
        BrowserNativeTarget {
            ax_node_id: ax_node_id.to_owned(),
            backend_node_id: backend,
            frame_id: None,
            role: Some(role.to_owned()),
            name: name.map(str::to_owned),
            value: value.map(str::to_owned),
            field_kind: None,
        }
    }

    fn projection(
        nodes: Vec<AccessibilityNode>,
        targets: Vec<BrowserNativeTarget>,
    ) -> BrowserNativeProjection {
        BrowserNativeProjection {
            accessibility: Some(AccessibilityProjection {
                source: "chromium_cdp_ax".to_owned(),
                nodes,
                truncated: false,
            }),
            frames: Vec::new(),
            targets,
            redaction: None,
        }
    }

    fn probed(backend: i64, attributes: &[&str]) -> ProbeOutcome {
        let attributes: Vec<String> = attributes.iter().map(|s| (*s).to_owned()).collect();
        let mut outcome = ProbeOutcome {
            state: ProbeState::Ok,
            probe_ms: 3,
            facts: HashMap::new(),
        };
        outcome
            .facts
            .insert(backend, facts_from_attributes(&attributes));
        outcome
    }

    fn value_of(projection: &BrowserNativeProjection, ax_id: &str) -> Option<String> {
        projection
            .accessibility
            .as_ref()
            .and_then(|a| a.nodes.iter().find(|n| n.node_id == ax_id))
            .and_then(|n| n.value.clone())
    }

    #[test]
    fn collect_probe_targets_skips_ignored_and_non_editable_nodes() {
        let tree = json!({ "nodes": [
            { "nodeId": "1", "role": {"type": "role", "value": "textbox"}, "backendDOMNodeId": 11, "frameId": "f" },
            { "nodeId": "2", "role": {"value": "textbox"}, "backendDOMNodeId": 12, "ignored": true },
            { "nodeId": "3", "role": {"value": "heading"}, "backendDOMNodeId": 13 },
            { "nodeId": "4", "role": {"value": "button"}, "backendDOMNodeId": 14 },
            { "nodeId": "5", "role": {"value": "searchbox"} },
            { "nodeId": "6", "role": {"value": "combobox"}, "backendDOMNodeId": 16 },
        ]});
        let targets = collect_probe_targets(&tree);
        let ids: Vec<_> = targets.iter().map(|t| t.ax_node_id.as_str()).collect();
        assert_eq!(ids, vec!["1", "6"]);
        assert_eq!(targets[0].frame_id.as_deref(), Some("f"));
    }

    #[test]
    fn collect_probe_targets_caps_at_64() {
        let nodes: Vec<Value> = (0..100)
            .map(|i| json!({ "nodeId": i.to_string(), "role": {"value": "textbox"}, "backendDOMNodeId": i }))
            .collect();
        let targets = collect_probe_targets(&json!({ "nodes": nodes }));
        assert_eq!(targets.len(), PROBE_NODE_CAP);
        assert_eq!(targets.len(), 64);
    }

    #[test]
    fn facts_from_attributes_parses_type_and_autocomplete() {
        let attributes: Vec<String> = [
            "type",
            "password",
            "autocomplete",
            "new-password",
            "id",
            "pw",
            "aria-label",
            "Show",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        let facts = facts_from_attributes(&attributes);
        assert_eq!(facts.input_type.as_deref(), Some("password"));
        assert_eq!(facts.autocomplete.as_deref(), Some("new-password"));
        assert_eq!(facts.id_attr.as_deref(), Some("pw"));
        assert_eq!(facts.aria_label.as_deref(), Some("Show"));
        assert!(facts.value.is_none(), "the probe never reads a value");
    }

    #[test]
    fn facts_from_call_result_rejects_null_and_accepts_an_object() {
        assert!(facts_from_call_result(&Value::Null).is_none());
        let facts = facts_from_call_result(&json!({ "input_type": "tel", "autocomplete": null }))
            .expect("facts");
        assert_eq!(facts.input_type.as_deref(), Some("tel"));
        assert!(facts.autocomplete.is_none());
    }

    #[test]
    fn password_ax_value_is_masked() {
        let mut p = projection(
            vec![node("7", "textbox", Some("Password"), Some(PASSWORD))],
            vec![target("7", 70, "textbox", Some("Password"), Some(PASSWORD))],
        );
        let probe = probed(70, &["type", "password"]);
        let stamp = mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(value_of(&p, "7").as_deref(), Some("[redacted:password]"));
        assert_eq!(stamp.probe, ProbeState::Ok);
        assert_eq!(stamp.values_masked, 1);
        assert_eq!(stamp.by_rule.get("field.password"), Some(&1));
        assert_eq!(p.targets[0].field_kind, Some(FieldKind::Password));
        assert_eq!(p.targets[0].value.as_deref(), Some("[redacted:password]"));
    }

    #[test]
    fn show_password_toggle_value_is_masked_by_autocomplete_token() {
        // The toggle flips type to text; the autocomplete token still names a password.
        let mut p = projection(
            vec![node("8", "textbox", None, Some(PASSWORD))],
            vec![target("8", 80, "textbox", None, Some(PASSWORD))],
        );
        let probe = probed(80, &["type", "text", "autocomplete", "current-password"]);
        mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(value_of(&p, "8").as_deref(), Some("[redacted:password]"));
        assert_eq!(p.targets[0].field_kind, Some(FieldKind::Password));
    }

    #[test]
    fn unprobed_editable_value_is_masked_fail_closed() {
        let mut p = projection(
            vec![node("9", "textbox", Some("Notes"), Some("visible-text"))],
            vec![target(
                "9",
                90,
                "textbox",
                Some("Notes"),
                Some("visible-text"),
            )],
        );
        // No facts for backend 90: the probe missed it.
        let probe = ProbeOutcome {
            state: ProbeState::Partial,
            probe_ms: 400,
            facts: HashMap::new(),
        };
        let stamp = mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(value_of(&p, "9").as_deref(), Some("[redacted:secret]"));
        assert_eq!(stamp.probe, ProbeState::Partial);
        assert_eq!(stamp.by_rule.get("field.label"), Some(&1));
        assert_eq!(p.targets[0].field_kind, Some(FieldKind::Secret));
        // The name is text, not a value: it is kept.
        assert_eq!(p.targets[0].name.as_deref(), Some("Notes"));
    }

    #[test]
    fn probe_off_masks_every_editable_value_even_with_facts() {
        let mut p = projection(
            vec![node("9", "textbox", Some("Search"), Some("visible-text"))],
            vec![target(
                "9",
                90,
                "textbox",
                Some("Search"),
                Some("visible-text"),
            )],
        );
        let probe = probed(90, &["type", "search"]);
        let stamp = mask_projection(&mut p, &probe, ProbeSetting::Off, Mode::Enforce);
        assert_eq!(value_of(&p, "9").as_deref(), Some("[redacted:secret]"));
        assert_eq!(stamp.probe, ProbeState::Skipped);
        assert_eq!(stamp.probe_ms, 0);
    }

    #[test]
    fn static_text_with_a_jwt_is_masked_by_shape() {
        let text = format!("session token {JWT} end");
        let mut p = projection(
            vec![node("20", "StaticText", Some(text.as_str()), None)],
            Vec::new(),
        );
        let probe = ProbeOutcome::default();
        let stamp = mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        let name = p.accessibility.as_ref().unwrap().nodes[0]
            .name
            .clone()
            .unwrap();
        assert!(
            !name.contains("dozjgNryP4J3"),
            "jwt signature leaked: {name}"
        );
        assert!(stamp.values_masked >= 1);
        assert_eq!(stamp.probe, ProbeState::Skipped);
    }

    #[test]
    fn label_password_is_kept_but_value_masked() {
        // A plain text box whose label says password: the label decides.
        let mut p = projection(
            vec![node("30", "textbox", Some("Password"), Some(PASSWORD))],
            vec![target(
                "30",
                300,
                "textbox",
                Some("Password"),
                Some(PASSWORD),
            )],
        );
        let probe = probed(300, &["type", "text"]);
        mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(
            p.accessibility.as_ref().unwrap().nodes[0].name.as_deref(),
            Some("Password")
        );
        assert_eq!(value_of(&p, "30").as_deref(), Some("[redacted:password]"));
    }

    #[test]
    fn username_value_is_not_masked() {
        let mut p = projection(
            vec![node("40", "textbox", Some("Username"), Some("alice"))],
            vec![target(
                "40",
                400,
                "textbox",
                Some("Username"),
                Some("alice"),
            )],
        );
        let probe = probed(
            400,
            &[
                "type",
                "text",
                "autocomplete",
                "username",
                "name",
                "username",
            ],
        );
        mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(value_of(&p, "40").as_deref(), Some("alice"));
        assert_eq!(p.targets[0].field_kind, Some(FieldKind::Unknown));
    }

    #[test]
    fn fingerprint_and_target_text_inherit_the_mask() {
        // Target text (name and value) carries the same mask as the accessibility
        // node. The fingerprint itself is computed in observation.rs (WS12.S2).
        let mut p = projection(
            vec![node(
                "50",
                "textbox",
                Some("Card number"),
                Some("4111111111111111"),
            )],
            vec![target(
                "50",
                500,
                "textbox",
                Some("Card number"),
                Some("4111111111111111"),
            )],
        );
        let probe = probed(500, &["type", "text", "autocomplete", "cc-number"]);
        mask_projection(&mut p, &probe, ProbeSetting::On, Mode::Enforce);
        assert_eq!(p.targets[0].value.as_deref(), Some("[redacted:card]"));
        assert_eq!(p.targets[0].name.as_deref(), Some("Card number"));
        assert_eq!(value_of(&p, "50").as_deref(), Some("[redacted:card]"));
    }

    #[test]
    fn setting_is_on_unless_explicitly_off() {
        assert_eq!(setting_from_env_value(None), ProbeSetting::On);
        assert_eq!(setting_from_env_value(Some("on")), ProbeSetting::On);
        assert_eq!(setting_from_env_value(Some("garbage")), ProbeSetting::On);
        assert_eq!(setting_from_env_value(Some(" OFF ")), ProbeSetting::Off);
        assert_eq!(setting_from_env_value(Some("0")), ProbeSetting::Off);
    }
}
