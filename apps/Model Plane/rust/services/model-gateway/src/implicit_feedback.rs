//! Publishes detected dissatisfaction onto the feedback channel.
//!
//! [`crate::dissatisfaction`] decides WHAT a turn signalled; this decides where
//! that lands. It reuses `mp.v1.feedback.rated` rather than inventing a subject,
//! because the plan's own warning about Phase 1.2 is that a signal nothing reads
//! is worthless — the existing subject already has a live consumer
//! (orchestrator-core's feedback store), so an implicit signal is read the day it
//! is emitted.
//!
//! # Three things this must not get wrong
//!
//! **It must not overwrite a human's rating.** The explicit path keys on
//! `feedback_{run}_{user}` so a re-rating replaces. An implicit signal using that
//! key would silently erase a real thumbs-down, or be erased by one. Implicit
//! keys are namespaced and further split by signal kind, so they replace only
//! themselves.
//!
//! **It must point at the run being complained about.** A correction arrives on
//! turn N and is evidence against turn N-1's answer. Attaching it to the new run
//! would blame the turn that expressed the complaint.
//!
//! **It must be marked.** The payload carries `source: "implicit"` plus the kind
//! and strength, so the consumer weights it far below a stated judgement instead
//! of treating behaviour as a verdict.

use chrono::Utc;
use mp_events::envelope::Envelope;
use mp_ids::new_ulid;
use serde_json::json;

use crate::chat_turn_registry::PreviousTurn;
use crate::dissatisfaction::Signal;

/// Subject shared with explicit ratings. Same vocabulary, same consumer.
pub const FEEDBACK_SUBJECT: &str = "mp.v1.feedback.rated";

/// Marks the payload as behavioural inference rather than a stated judgement.
const SOURCE_IMPLICIT: &str = "implicit";

/// Every implicit signal is a negative one. There is no behavioural signal for
/// satisfaction — treating silence as approval would make every unanswered turn
/// a positive vote — so the canonical grade is always `poor`.
const IMPLICIT_RATING: &str = "poor";

/// Build the envelopes for one turn's signals.
///
/// Returns one envelope per signal rather than a combined one: "unhappy in two
/// ways" is different evidence from "unhappy once", and collapsing here would
/// throw that away before the consumer can weight it.
///
/// Returns empty when there is nothing to attach the evidence to — no signals, no
/// previous run, or a ZDR turn.
#[must_use]
pub fn envelopes_for(
    signals: &[Signal],
    previous: &PreviousTurn,
    org_id: &str,
    user_id: &str,
    zdr: bool,
) -> Vec<Envelope> {
    // A ZDR turn must not deposit run-derived state anywhere durable, and the
    // feedback store is durable. Dropping the signal is the correct cost.
    if zdr || signals.is_empty() {
        return Vec::new();
    }
    if previous.run_id.trim().is_empty() || org_id.trim().is_empty() || user_id.trim().is_empty() {
        return Vec::new();
    }

    signals
        .iter()
        .map(|signal| Envelope {
            event_id: new_ulid(),
            event_type: "FEEDBACK_RATED".to_owned(),
            schema_version: 1,
            ts: Utc::now(),
            producer: "model-gateway".to_owned(),
            correlation_id: previous.run_id.clone(),
            causation_id: String::new(),
            // Namespaced away from `feedback_{run}_{user}` so an implicit signal
            // can never replace a human's rating, and split by kind so a
            // regenerate and a correction on the same turn are both kept while a
            // repeated detection of the SAME kind stays one sample.
            idempotency_key: format!(
                "feedback_implicit_{}_{}_{}",
                previous.run_id,
                user_id,
                signal.kind.as_str()
            ),
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
            resource_ref: format!("run/{}", previous.run_id),
            payload: json!({
                "run_id": previous.run_id,
                "skill_id": previous.skill_ids.first().cloned().unwrap_or_default(),
                "skill_ids": previous.skill_ids,
                "rating": IMPLICIT_RATING,
                "source": SOURCE_IMPLICIT,
                "signal_kind": signal.kind.as_str(),
                "signal_strength": signal.strength,
                // No note: the user's message IS the evidence, and copying it
                // here would put conversation content into the feedback store.
                "note": serde_json::Value::Null,
            }),
            zdr: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::dissatisfaction::{Signal, SignalKind};

    fn previous() -> PreviousTurn {
        PreviousTurn {
            message: "hva er saldoen".to_owned(),
            run_id: "run-1".to_owned(),
            skill_ids: vec!["skill-a".to_owned(), "skill-b".to_owned()],
            elapsed: Duration::from_secs(10),
        }
    }

    fn signal(kind: SignalKind) -> Signal {
        Signal { kind, strength: kind.strength() }
    }

    /// The bug this namespacing exists to prevent: an implicit signal silently
    /// replacing a user's real thumbs-down, or being replaced by one.
    #[test]
    fn an_implicit_key_can_never_collide_with_an_explicit_rating() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "org", "user", false);
        let explicit_key = format!("feedback_{}_{}", "run-1", "user");
        assert_eq!(envelopes.len(), 1);
        assert_ne!(envelopes[0].idempotency_key, explicit_key);
        assert!(envelopes[0].idempotency_key.starts_with("feedback_implicit_"));
    }

    /// Two kinds on one turn are two samples; the same kind twice is one.
    #[test]
    fn keys_split_by_kind_but_not_by_occurrence() {
        let both = envelopes_for(
            &[signal(SignalKind::Regenerate), signal(SignalKind::Correction)],
            &previous(),
            "org",
            "user",
            false,
        );
        assert_eq!(both.len(), 2);
        assert_ne!(both[0].idempotency_key, both[1].idempotency_key);

        let repeated =
            envelopes_for(&[signal(SignalKind::Regenerate)], &previous(), "org", "user", false);
        assert_eq!(repeated[0].idempotency_key, both[0].idempotency_key);
    }

    /// The evidence must land on the run that produced the answer being
    /// complained about, not the turn carrying the complaint.
    #[test]
    fn the_signal_attaches_to_the_previous_run_and_its_skills() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "org", "user", false);
        let payload = &envelopes[0].payload;
        assert_eq!(payload["run_id"], "run-1");
        assert_eq!(envelopes[0].correlation_id, "run-1");
        assert_eq!(payload["skill_ids"][0], "skill-a");
        assert_eq!(payload["skill_ids"][1], "skill-b");
    }

    /// Marked as inference so the consumer discounts it. An unmarked payload
    /// would be counted as a stated judgement.
    #[test]
    fn the_payload_is_marked_implicit_with_its_kind_and_strength() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::NearDuplicate)], &previous(), "org", "user", false);
        let payload = &envelopes[0].payload;
        assert_eq!(payload["source"], "implicit");
        assert_eq!(payload["signal_kind"], "near_duplicate");
        assert_eq!(payload["rating"], "poor");
        assert!(payload["signal_strength"].as_f64().unwrap() > 0.0);
    }

    /// A ZDR turn must deposit nothing durable, and the feedback store is durable.
    #[test]
    fn a_zdr_turn_emits_nothing() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "org", "user", true);
        assert!(envelopes.is_empty(), "ZDR must not reach the durable feedback store");
    }

    #[test]
    fn nothing_is_emitted_without_something_to_attach_to() {
        assert!(envelopes_for(&[], &previous(), "org", "user", false).is_empty());

        let no_run = PreviousTurn { run_id: String::new(), ..previous() };
        assert!(
            envelopes_for(&[signal(SignalKind::Correction)], &no_run, "org", "user", false)
                .is_empty()
        );
        assert!(
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "", "user", false)
                .is_empty()
        );
    }

    /// The user's message is the evidence; copying it here would move
    /// conversation content into the feedback store.
    #[test]
    fn no_conversation_content_is_carried_into_the_payload() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "org", "user", false);
        let serialized = serde_json::to_string(&envelopes[0].payload).unwrap();
        assert!(!serialized.contains("hva er saldoen"), "payload leaked the turn text");
        assert!(envelopes[0].payload["note"].is_null());
    }

    /// The envelope itself must not claim ZDR: it is only ever built for a
    /// non-ZDR turn, and a `true` here would make natsx suppress it.
    #[test]
    fn the_envelope_is_not_marked_zdr() {
        let envelopes =
            envelopes_for(&[signal(SignalKind::Correction)], &previous(), "org", "user", false);
        assert!(!envelopes[0].zdr);
    }
}
