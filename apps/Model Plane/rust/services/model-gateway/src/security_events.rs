//! Publishes security-relevant tool-result screening signals onto the
//! existing security-event channel (`mp.v1.security.{org_id}`, mirroring the
//! established `mp.v1.usage.{org_id}` / `mp.v1.feedback.rated` pattern —
//! see [`crate::implicit_feedback`] for the sibling implementation this one
//! is modeled on).
//!
//! **Metadata and content hashes ONLY — never the flagged content itself.**
//! ZDR discipline is absolute here: a BLAKE3 hash cannot be inverted back
//! into the bytes it was taken from, so publishing it is safe even on a ZDR
//! turn. The envelope's own `zdr` flag still travels with the event so a
//! downstream consumer applies its own retention discipline on top.
//!
//! An event fires when [`ToolProvenance::is_audit_worthy`] says so: a
//! detected injection marker, a screening posture that degraded under its
//! size/deadline/concurrency bounds, or externally-sourced content that
//! reached the model without ever being screened. A clean, screened, or
//! org-internal result is not an event — see that method's docs for the
//! exact rule.

use chrono::Utc;
use mp_events::envelope::Envelope;
use mp_ids::new_ulid;
use serde_json::json;

use crate::moderation::ToolProvenance;

/// Event type carried on the security subject for a tool-result screening
/// signal. Subscribers should treat any unrecognized `event_type` on this
/// subject as forward-compatible (log + skip), matching the convention
/// documented on `mp_events::subjects`' other event families.
pub const EVENT_TYPE_TOOL_RESULT_SCREENING: &str = "TOOL_RESULT_SCREENING_FLAGGED";
pub const EVENT_TYPE_SEMANTIC_SHADOW: &str = "TOOL_RESULT_SEMANTIC_SHADOW";
/// One inline injection decision under `tighten` or `enforce` (WS26-S6): the mode, the
/// outcome, the coverage, the category scores and whether the payload was withheld.
pub const EVENT_TYPE_SEMANTIC_ENFORCED: &str = "TOOL_RESULT_SEMANTIC_ENFORCED";

/// Build a security event envelope for one tool outcome's provenance, if and
/// only if it is audit-worthy. Returns `None` for a clean/screened/
/// org-internal result — silence is the expected common case, matching
/// `append_tool_outcomes`' own "only surface what changes model behavior"
/// rule.
///
/// Pure: no I/O, no network. The caller (`run_tool_rounds`) is responsible
/// for the actual `EventPublisher::publish` call, exactly like
/// [`crate::implicit_feedback::envelopes_for`].
#[must_use]
pub fn envelope_for(
    provenance: &ToolProvenance,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    tool_name: &str,
    zdr: bool,
) -> Option<Envelope> {
    if !provenance.is_audit_worthy() {
        return None;
    }
    // Without an org/run to scope it to, the event has no home an
    // investigator could ever query it by — drop rather than publish a
    // partially-identified signal.
    if org_id.trim().is_empty() || run_id.trim().is_empty() {
        return None;
    }
    Some(Envelope {
        event_id: new_ulid(),
        event_type: EVENT_TYPE_TOOL_RESULT_SCREENING.to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: run_id.to_owned(),
        causation_id: String::new(),
        // Namespaced by run + tool + the exact content hash, so a retried
        // identical call in the same run dedupes on replay instead of
        // fanning out duplicate events for the same bytes.
        idempotency_key: format!(
            "security_{run_id}_{tool_name}_{}",
            provenance.screening.content_hash
        ),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("run/{run_id}"),
        payload: json!({
            "tool": tool_name,
            "trust_class": provenance.trust.label(),
            "screening_posture": provenance.screening.posture.label(),
            // Lets an investigator correlate this event with the exact bytes
            // that were screened, without the bytes ever leaving the
            // process — see module docs on why this is ZDR-safe.
            "content_hash": provenance.screening.content_hash,
            // Rule ids name what fired, never the matched text: they are
            // published on the same ZDR-safe channel as the hash.
            "flags": provenance.screening.rule_ids,
        }),
        zdr,
        ..Default::default()
    })
}

pub fn shadow_envelope_for(
    provenance: &ToolProvenance,
    shadow: &crate::semantic_screening::ShadowScreeningOutcome,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    tool_name: &str,
    zdr: bool,
) -> Option<Envelope> {
    let crate::semantic_screening::ShadowScreeningOutcome::Completed {
        verdict,
        agrees_with_deterministic,
    } = shadow
    else {
        return None;
    };
    if org_id.trim().is_empty() || run_id.trim().is_empty() {
        return None;
    }
    Some(Envelope {
        event_id: new_ulid(),
        event_type: EVENT_TYPE_SEMANTIC_SHADOW.to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: run_id.to_owned(),
        causation_id: String::new(),
        // Namespaced exactly like `envelope_for`'s screening event, plus a
        // `shadow_` prefix so the two event kinds for the same
        // (run, tool, hash) triple never collide on idempotency key.
        idempotency_key: format!(
            "security_shadow_{run_id}_{tool_name}_{}",
            provenance.screening.content_hash
        ),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("run/{run_id}"),
        payload: json!({
            "tool": tool_name,
            "trust_class": provenance.trust.label(),
            "deterministic_posture": provenance.screening.posture.label(),
            "semantic_flagged": verdict.flagged,
            "semantic_categories": {
                "hate": verdict.categories.hate,
                "harassment": verdict.categories.harassment,
                "violence": verdict.categories.violence,
                "self_harm": verdict.categories.self_harm,
                "sexual": verdict.categories.sexual,
            },
            "agreement": agrees_with_deterministic,
            "content_hash": provenance.screening.content_hash,
        }),
        zdr,
        ..Default::default()
    })
}

/// Build the envelope for one inline injection decision, if the decision says
/// something: the classifier was asked (a verdict or an unavailable call), or the
/// payload was withheld. A skipped decision stays silent, like a clean result. Nothing
/// is published on a ZDR turn, as for the shadow pass.
#[must_use]
pub fn enforced_envelope_for(
    screen: &crate::semantic_screening::InlineScreen,
    org_id: &str,
    user_id: &str,
    run_id: &str,
    tool_name: &str,
    zdr: bool,
) -> Option<Envelope> {
    use crate::semantic_screening::InjectionOutcome;

    let asked = !matches!(screen.outcome, InjectionOutcome::Skipped(_));
    if zdr || !(asked || screen.withhold) {
        return None;
    }
    if org_id.trim().is_empty() || run_id.trim().is_empty() {
        return None;
    }
    let categories = match &screen.outcome {
        InjectionOutcome::Completed(verdict) => json!({
            "instruction_override": verdict.categories.instruction_override,
            "role_impersonation": verdict.categories.role_impersonation,
            "exfiltration_request": verdict.categories.exfiltration_request,
            "tool_use_request": verdict.categories.tool_use_request,
            "hidden_text": verdict.categories.hidden_text,
        }),
        InjectionOutcome::Skipped(_) | InjectionOutcome::Unavailable(_) => serde_json::Value::Null,
    };
    Some(Envelope {
        event_id: new_ulid(),
        event_type: EVENT_TYPE_SEMANTIC_ENFORCED.to_owned(),
        schema_version: 1,
        ts: Utc::now(),
        producer: "model-gateway".to_owned(),
        correlation_id: run_id.to_owned(),
        causation_id: String::new(),
        // Namespaced like the other two screening events, so the three kinds never
        // collide on the same (run, tool, hash) triple.
        idempotency_key: format!(
            "security_enforced_{run_id}_{tool_name}_{}",
            screen.screening.content_hash
        ),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        resource_ref: format!("run/{run_id}"),
        payload: json!({
            "tool": tool_name,
            "trust_class": screen.trust.label(),
            "mode": screen.mode.as_wire(),
            "deterministic_posture": screen.deterministic.label(),
            "screening_posture": screen.screening.posture.label(),
            "outcome": screen.outcome.label(),
            "coverage": screen.coverage.label(),
            "withheld": screen.withhold,
            "semantic_categories": categories,
            // The hash and the closed labels above are the whole payload: no text.
            "content_hash": screen.screening.content_hash,
        }),
        zdr,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moderation::{ScreeningOutcome, ScreeningPosture, TrustClass};
    use crate::semantic_screening::{SemanticCategories, SemanticVerdict, ShadowScreeningOutcome};

    fn provenance(trust: TrustClass, posture: ScreeningPosture) -> ToolProvenance {
        ToolProvenance {
            trust,
            screening: ScreeningOutcome {
                posture,
                content_hash: crate::moderation::content_hash(b"some payload"),
                rule_ids: Vec::new(),
            },
        }
    }

    #[test]
    fn a_flagged_event_carries_rule_ids_and_no_text() {
        let mut p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        p.screening.rule_ids = vec!["injection.ignore_previous_instructions", "obfuscation.tag_block"];
        let envelope = envelope_for(&p, "org-1", "user-1", "run-1", "web_fetch", false)
            .expect("flagged result must be audited");
        assert_eq!(
            envelope.payload["flags"],
            serde_json::json!(["injection.ignore_previous_instructions", "obfuscation.tag_block"])
        );
    }

    #[test]
    fn a_flagged_result_produces_an_event() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        let envelope = envelope_for(&p, "org-1", "user-1", "run-1", "fetch_url", false)
            .expect("flagged result must be audited");
        assert_eq!(envelope.event_type, EVENT_TYPE_TOOL_RESULT_SCREENING);
        assert_eq!(envelope.org_id, "org-1");
        assert_eq!(envelope.user_id, "user-1");
        assert_eq!(envelope.resource_ref, "run/run-1");
        assert_eq!(envelope.payload["tool"], "fetch_url");
        assert_eq!(envelope.payload["trust_class"], "external-web");
        assert_eq!(envelope.payload["screening_posture"], "flagged");
        assert_eq!(
            envelope.payload["content_hash"],
            crate::moderation::content_hash(b"some payload")
        );
        assert!(!envelope.zdr);
    }

    #[test]
    fn a_degraded_result_produces_an_event() {
        let p = provenance(TrustClass::BrowserScraped, ScreeningPosture::Degraded);
        let envelope = envelope_for(&p, "org-1", "user-1", "run-1", "browser_agent", true)
            .expect("degraded result must be audited");
        assert_eq!(envelope.payload["screening_posture"], "degraded");
        // ZDR travels with the event so a consumer applies its own
        // retention discipline — the payload itself never carries content
        // either way.
        assert!(envelope.zdr);
    }

    #[test]
    fn unscreened_external_content_produces_an_event() {
        // External trust class that somehow never got a real scan (e.g. the
        // org's injection_defense policy is off) is exactly the case an
        // investigator needs visibility into — untrusted content reached the
        // model with no screening at all.
        let p = provenance(TrustClass::ThirdPartyMcp, ScreeningPosture::PolicyDisabled);
        let envelope = envelope_for(&p, "org-1", "user-1", "run-1", "mcp__srv__tool", false)
            .expect("unscreened external content must be audited");
        assert_eq!(envelope.payload["screening_posture"], "policy-disabled");
        assert_eq!(envelope.payload["trust_class"], "third-party-mcp");
    }

    #[test]
    fn a_clean_screened_result_produces_no_event() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Clean);
        assert!(envelope_for(&p, "org-1", "user-1", "run-1", "fetch_url", false).is_none());
    }

    #[test]
    fn unscreened_org_internal_content_produces_no_event() {
        // NotApplicable + org-internal is the expected shape of the vast
        // majority of tool calls (create_artifact, knowledge_search, ...) —
        // this must stay silent or every ordinary turn would fire an event.
        let p = provenance(TrustClass::OrgInternal, ScreeningPosture::NotApplicable);
        assert!(envelope_for(&p, "org-1", "user-1", "run-1", "create_artifact", false).is_none());
    }

    #[test]
    fn an_audit_worthy_result_missing_org_or_run_scope_produces_no_event() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        assert!(envelope_for(&p, "", "user-1", "run-1", "fetch_url", false).is_none());
        assert!(envelope_for(&p, "org-1", "user-1", "", "fetch_url", false).is_none());
    }

    #[test]
    fn the_idempotency_key_is_namespaced_by_run_tool_and_content_hash() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        let a = envelope_for(&p, "org-1", "user-1", "run-1", "fetch_url", false).unwrap();
        // A different run must not collide with the first.
        let b = envelope_for(&p, "org-1", "user-1", "run-2", "fetch_url", false).unwrap();
        assert_ne!(a.idempotency_key, b.idempotency_key);
        // The identical (run, tool, hash) triple reproduces the same key —
        // a retried identical call dedupes on replay.
        let c = envelope_for(&p, "org-1", "user-1", "run-1", "fetch_url", false).unwrap();
        assert_eq!(a.idempotency_key, c.idempotency_key);
    }

    // --- shadow_envelope_for ------------------------------------------------

    fn completed_shadow(flagged: bool, agrees: bool) -> ShadowScreeningOutcome {
        ShadowScreeningOutcome::Completed {
            verdict: SemanticVerdict {
                flagged,
                categories: SemanticCategories {
                    hate: 0.1,
                    harassment: 0.0,
                    violence: 0.0,
                    self_harm: 0.0,
                    sexual: 0.0,
                },
            },
            agrees_with_deterministic: agrees,
        }
    }

    #[test]
    fn a_completed_shadow_evaluation_produces_an_event_with_agreement() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        let shadow = completed_shadow(true, true);
        let envelope =
            shadow_envelope_for(&p, &shadow, "org-1", "user-1", "run-1", "fetch_url", false)
                .expect("a completed shadow pass must be audited");
        assert_eq!(envelope.event_type, EVENT_TYPE_SEMANTIC_SHADOW);
        assert_eq!(envelope.payload["semantic_flagged"], true);
        assert_eq!(envelope.payload["agreement"], true);
        assert_eq!(envelope.payload["deterministic_posture"], "flagged");
        assert_eq!(envelope.payload["semantic_categories"]["hate"], 0.1);
        // Never the raw text, only its hash — same ZDR contract as
        // `envelope_for`.
        assert_eq!(
            envelope.payload["content_hash"],
            crate::moderation::content_hash(b"some payload")
        );
    }

    #[test]
    fn a_disagreeing_shadow_evaluation_is_still_audited() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Clean);
        let shadow = completed_shadow(true, false);
        let envelope =
            shadow_envelope_for(&p, &shadow, "org-1", "user-1", "run-1", "fetch_url", false)
                .expect("disagreement is exactly the signal an operator needs");
        assert_eq!(envelope.payload["agreement"], false);
        assert_eq!(envelope.payload["deterministic_posture"], "clean");
        assert_eq!(envelope.payload["semantic_flagged"], true);
    }

    #[test]
    fn a_skipped_or_unavailable_shadow_pass_produces_no_event() {
        use crate::semantic_screening::{ShadowSkipReason, ShadowUnavailableReason};
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Clean);
        for shadow in [
            ShadowScreeningOutcome::Skipped(ShadowSkipReason::Zdr),
            ShadowScreeningOutcome::Skipped(ShadowSkipReason::NotSampled),
            ShadowScreeningOutcome::Skipped(ShadowSkipReason::NotExternalTrust),
            ShadowScreeningOutcome::Unavailable(ShadowUnavailableReason::Timeout),
            ShadowScreeningOutcome::Unavailable(ShadowUnavailableReason::ConcurrencyExhausted),
        ] {
            assert!(
                shadow_envelope_for(&p, &shadow, "org-1", "user-1", "run-1", "fetch_url", false)
                    .is_none(),
                "{shadow:?} must not produce a shadow event"
            );
        }
    }

    #[test]
    fn shadow_event_missing_org_or_run_scope_produces_no_event() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Clean);
        let shadow = completed_shadow(false, true);
        assert!(
            shadow_envelope_for(&p, &shadow, "", "user-1", "run-1", "fetch_url", false).is_none()
        );
        assert!(
            shadow_envelope_for(&p, &shadow, "org-1", "user-1", "", "fetch_url", false).is_none()
        );
    }

    #[test]
    fn shadow_idempotency_key_never_collides_with_the_deterministic_event() {
        let p = provenance(TrustClass::ExternalWeb, ScreeningPosture::Flagged);
        let deterministic =
            envelope_for(&p, "org-1", "user-1", "run-1", "fetch_url", false).unwrap();
        let shadow_env = shadow_envelope_for(
            &p,
            &completed_shadow(true, true),
            "org-1",
            "user-1",
            "run-1",
            "fetch_url",
            false,
        )
        .unwrap();
        assert_ne!(deterministic.idempotency_key, shadow_env.idempotency_key);
    }

    // --- enforced_envelope_for ----------------------------------------------

    fn inline_screen(
        outcome: crate::semantic_screening::InjectionOutcome,
        withhold: bool,
    ) -> crate::semantic_screening::InlineScreen {
        crate::semantic_screening::InlineScreen {
            trust: TrustClass::ExternalWeb,
            mode: crate::moderation::ScreeningMode::Tighten,
            screening: ScreeningOutcome {
                posture: ScreeningPosture::Flagged,
                content_hash: crate::moderation::content_hash(b"some payload"),
                rule_ids: vec![crate::semantic_screening::SEMANTIC_RULE_ID],
            },
            deterministic: ScreeningPosture::Clean,
            outcome,
            coverage: crate::semantic_screening::Coverage::Full,
            withhold,
        }
    }

    #[test]
    fn an_enforced_event_carries_mode_outcome_coverage_and_no_text() {
        use crate::semantic_screening::{InjectionCategories, InjectionOutcome, InjectionVerdict};
        let verdict = InjectionVerdict {
            flagged: true,
            categories: InjectionCategories {
                instruction_override: 0.9,
                role_impersonation: 0.0,
                exfiltration_request: 0.0,
                tool_use_request: 0.1,
                hidden_text: 0.0,
            },
        };
        let screen = inline_screen(InjectionOutcome::Completed(verdict), false);
        let envelope =
            enforced_envelope_for(&screen, "org-1", "user-1", "run-1", "fetch_url", false)
                .expect("a completed inline call is audited");
        assert_eq!(envelope.event_type, EVENT_TYPE_SEMANTIC_ENFORCED);
        assert_eq!(envelope.payload["mode"], "tighten");
        assert_eq!(envelope.payload["outcome"], "completed");
        assert_eq!(envelope.payload["coverage"], "full");
        assert_eq!(envelope.payload["withheld"], false);
        assert_eq!(envelope.payload["semantic_categories"]["instruction_override"], 0.9);
        assert_eq!(
            envelope.payload["content_hash"],
            crate::moderation::content_hash(b"some payload")
        );
        assert!(!envelope.payload.to_string().contains("some payload"));
    }

    #[test]
    fn a_skipped_inline_decision_is_silent_unless_the_payload_was_withheld() {
        use crate::semantic_screening::{InjectionOutcome, InjectionSkipReason};
        let skipped = inline_screen(
            InjectionOutcome::Skipped(InjectionSkipReason::NotClean),
            false,
        );
        assert!(
            enforced_envelope_for(&skipped, "org-1", "user-1", "run-1", "fetch_url", false)
                .is_none()
        );
        let withheld = inline_screen(
            InjectionOutcome::Skipped(InjectionSkipReason::NotClean),
            true,
        );
        let envelope = enforced_envelope_for(&withheld, "org-1", "user-1", "run-1", "fetch_url", false)
            .expect("a withheld payload is always reported");
        assert_eq!(envelope.payload["withheld"], true);
        assert!(envelope.payload["semantic_categories"].is_null());
    }

    #[test]
    fn no_inline_event_is_published_on_a_zdr_turn() {
        use crate::semantic_screening::{InjectionOutcome, InjectionUnavailableReason};
        let unavailable = inline_screen(
            InjectionOutcome::Unavailable(InjectionUnavailableReason::Timeout),
            true,
        );
        assert!(enforced_envelope_for(
            &unavailable,
            "org-1",
            "user-1",
            "run-1",
            "fetch_url",
            true
        )
        .is_none());
    }
}
