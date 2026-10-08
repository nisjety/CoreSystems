//! What the pipeline does about each unit's context, decided from what the database
//! already holds. Pure: no network, no clock, no database.
//!
//! The decision for one unit is one of four things. The organization's operator policy
//! (CR-T11, `crate::policy`) is part of what is known about the unit: an organization whose
//! policy is off never gets a `Generate`, whatever else is true of the unit.
//!
//! * [`Decision::Keep`]: a context is stored and it stays. The vector is built from it.
//!   No provider call.
//! * [`Decision::Generate`]: there is no usable context, or the stored one is stale
//!   because the document it was read in changed. The provider is asked, inside the
//!   inline time box.
//! * [`Decision::Record`]: nothing is generated and the unit's status says why (a
//!   restricted document, a document that is gone).
//! * [`Decision::Leave`]: nothing is generated and nothing is written. The unit is
//!   embedded from whatever it has.
//!
//! ## When a stored context is kept
//!
//! A context is the answer to one set of inputs (see `context::prompt::input_hash`: the
//! prompt version, the model alias, the window of the document, the title and the chunk).
//! Reuse follows from that, and it errs toward not paying:
//!
//! * the hash of the inputs now equals the stored one: kept (this is what makes a
//!   redelivery, a re-embed and a second run free);
//! * no hash is stored (a context from before provenance was recorded, `legacy`, or one
//!   written by the code this replaces): kept. Whether it is still the answer cannot be
//!   known, and regenerating every one of them on a re-embed is the cost this design
//!   removes. Refreshing them is the reconciler's job, on purpose and within a budget;
//! * a hash is stored and differs, and the prompt version or the model alias is not the
//!   one the context was made with: kept. That is a deliberate rollout, and a rollout is
//!   staged and capped (`CONTEXT_REFRESH_ON_PROMPT_CHANGE`), not a side effect of
//!   whichever units happen to be embedded again;
//! * a hash is stored and differs, with the same prompt version and model alias: the
//!   document changed under the chunk. Regenerated, and the old context stays in force
//!   until the new one is written, so a failed regeneration costs nothing.

use std::time::Duration;

use crate::context::{ContextOutcome, ErrorClass, PendingReason, PromptVersion, ZdrReason};

use super::store::Stored;

/// What a context generated now would be made with.
pub(super) struct Current<'a> {
    pub version: PromptVersion,
    pub model: &'a str,
    pub hash: &'a str,
    /// This run is the reconciler's prompt refresh (`PassKind::RefreshPrompt`): a stored context
    /// made with another prompt version is regenerated instead of kept.
    pub refresh: bool,
}

/// What is known about the unit's document and the engine.
pub(super) enum Reading<'a> {
    /// Contextual retrieval is switched off (the incident kill switch): nothing is
    /// generated, and nothing is recorded.
    Off,
    /// The document is gone, deleted, or has no text.
    DocumentMissing,
    /// An operator switched contextual retrieval off for the unit's organization
    /// (`org_processing_policy`): nothing is generated, and the unit is marked
    /// `skipped_policy` so that "off" is never silent. A context the unit already has stays.
    PolicyOff,
    /// The organization's policy cannot be determined right now (the policy table could not
    /// be read and nothing is known about the organization): nothing is generated and
    /// nothing is recorded, so that an organization that is off is never sent content on a
    /// failed read. The unit stays as it is and is picked up on a later pass.
    PolicyUnknown,
    /// The organization's background budget is used up and its budget mode is `stop`
    /// (explicit, operator-set; CR-T9b): nothing is generated, and a unit with no context is
    /// marked `skipped_budget` so that the stop is never silent. A context the unit already
    /// has stays. Read NOW, like the policy: a unit marked while the budget was used up is
    /// generated again once the gate lets the organization through.
    BudgetStopped,
    Current(Current<'a>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Decision {
    /// Embed from the stored context. `classify_legacy` when the unit carries a context
    /// and no status (written by the code this replaces): it is recorded as `legacy`.
    Keep {
        classify_legacy: bool,
    },
    Generate,
    Record(ContextOutcome),
    Leave,
}

/// Statuses that say "do not generate" on purpose: a permanent failure. A re-embed does not
/// override it.
///
/// `skipped_policy` and `skipped_budget` are NOT among them. Each records what the organization's
/// policy or budget said when the unit was last seen, and both are read now
/// (`Reading::PolicyOff`, `Reading::BudgetStopped`): a unit that was skipped while the policy was
/// off, or while the budget was used up, is generated again once the policy is lifted or the
/// budget allows it (a new month, a raised figure, another mode), instead of staying raw for ever
/// because of a decision that no longer stands. (`skipped_budget` used to be one of the statuses
/// that forbid, before there was a gate to read the budget: nothing could ever lift it.)
fn forbids_generation(status: Option<&str>) -> bool {
    matches!(status, Some("failed_permanent"))
}

pub(super) fn decide(restricted: bool, stored: &Stored, reading: &Reading<'_>) -> Decision {
    let keep = Decision::Keep {
        classify_legacy: stored.status.is_none() && stored.input_hash.is_none(),
    };
    if matches!(reading, Reading::Off) {
        return if stored.context.is_some() {
            keep
        } else {
            Decision::Leave
        };
    }
    if stored.context.is_some() {
        // A restricted document's text is never sent, so a stale context is never
        // regenerated for it; it keeps what it has.
        if restricted {
            return keep;
        }
        return match (&stored.input_hash, reading) {
            // Made from these very inputs: kept, whatever the run is for.
            (Some(stored_hash), Reading::Current(current)) if stored_hash == current.hash => keep,
            // The reconciler's prompt refresh: a context made with another prompt version than
            // the one in force is regenerated, whether its provenance is recorded or not (a
            // `legacy` context carries the label of the prompt that wrote it). Only the prompt:
            // a model alias that differs on its own is not what the refresh is for.
            (_, Reading::Current(current))
                if current.refresh
                    && stored.prompt_version.as_deref() != Some(current.version.as_str()) =>
            {
                Decision::Generate
            }
            (None, _) => keep,
            (Some(_), Reading::Current(current))
                if stored.prompt_version.as_deref() != Some(current.version.as_str())
                    || stored.model.as_deref() != Some(current.model) =>
            {
                Decision::Keep {
                    classify_legacy: false,
                }
            }
            (Some(_), Reading::Current(_)) => Decision::Generate,
            (Some(_), _) => Decision::Keep {
                classify_legacy: false,
            },
        };
    }
    if restricted {
        return Decision::Record(ContextOutcome::skipped_zdr(ZdrReason::Document));
    }
    if forbids_generation(stored.status.as_deref()) {
        return Decision::Leave;
    }
    match reading {
        Reading::Current(_) => Decision::Generate,
        Reading::DocumentMissing => {
            Decision::Record(ContextOutcome::permanent(ErrorClass::DocMissing))
        }
        Reading::Off | Reading::PolicyUnknown => Decision::Leave,
        // Already marked: writing the same status on every redelivery would only churn the row.
        Reading::PolicyOff if stored.status.as_deref() == Some("skipped_policy") => Decision::Leave,
        Reading::PolicyOff => Decision::Record(ContextOutcome::SkippedPolicy),
        // The same for the budget: a stop is marked once, and a redelivery or a re-embed of a
        // unit that is already marked writes nothing.
        Reading::BudgetStopped if stored.status.as_deref() == Some("skipped_budget") => {
            Decision::Leave
        }
        Reading::BudgetStopped => Decision::Record(ContextOutcome::SkippedBudget),
    }
}

/// What a unit becomes when the budget gate (`throttle`, CR-T9b) let other chunks of its
/// organization be generated and not this one: it waits, and nothing is lost.
///
/// * A unit that has a context keeps it (the regeneration that would have replaced it waits; the
///   vector is still built from what is stored).
/// * A unit that has none and is already `pending` is left as it is: writing the same status on
///   every batch would only churn the row.
/// * Any other is marked `pending`, due at once and with no error class (nothing went wrong): the
///   reconciler takes it at its own pace, and that pace is the throttle.
pub(super) fn deferred(stored: &Stored) -> Decision {
    if stored.context.is_some() {
        return Decision::Keep {
            classify_legacy: false,
        };
    }
    if stored.status.as_deref() == Some("pending") {
        return Decision::Leave;
    }
    Decision::Record(ContextOutcome::pending(
        PendingReason::BudgetThrottled,
        Duration::ZERO,
    ))
}

/// `items` in rounds: the first of each organization, then the second of each, and so
/// on, each organization's own order kept. With bounded concurrency over documents, a
/// batch that holds fifty documents of one organization and one of another would
/// otherwise start the one only after the fifty, and when the inline time runs out it
/// is the small organization that is left raw. Interleaving makes every organization's
/// first document start in the first round.
pub(super) fn interleave_by_org<T>(
    items: Vec<T>,
    org_of: impl for<'a> Fn(&'a T) -> &'a str,
) -> Vec<T> {
    use std::collections::VecDeque;

    let mut order: Vec<String> = Vec::new();
    let mut queues: Vec<VecDeque<T>> = Vec::new();
    for item in items {
        let org = org_of(&item);
        match order.iter().position(|seen| seen == org) {
            Some(position) => queues[position].push_back(item),
            None => {
                order.push(org.to_owned());
                queues.push(VecDeque::from([item]));
            }
        }
    }
    let mut out = Vec::new();
    while queues.iter().any(|queue| !queue.is_empty()) {
        for queue in &mut queues {
            if let Some(item) = queue.pop_front() {
                out.push(item);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const OLD: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn stored(status: Option<&str>, context: Option<&str>, hash: Option<&str>) -> Stored {
        Stored {
            knowledge_id: "kid".into(),
            document_id: "doc".into(),
            status: status.map(str::to_owned),
            context: context.map(str::to_owned),
            input_hash: hash.map(str::to_owned),
            prompt_version: Some("ctx-v1".into()),
            model: Some("gpt-4o-mini".into()),
            attempts: 0,
        }
    }

    fn current(version: PromptVersion, model: &'static str) -> Reading<'static> {
        Reading::Current(Current {
            version,
            model,
            hash: NOW,
            refresh: false,
        })
    }

    /// The same reading as the reconciler's prompt refresh sees it.
    fn refreshing(version: PromptVersion, model: &'static str) -> Reading<'static> {
        Reading::Current(Current {
            version,
            model,
            hash: NOW,
            refresh: true,
        })
    }

    fn now() -> Reading<'static> {
        current(PromptVersion::V1, "gpt-4o-mini")
    }

    const KEEP: Decision = Decision::Keep {
        classify_legacy: false,
    };

    #[test]
    fn a_context_made_from_these_inputs_is_kept() {
        let unit = stored(Some("ready"), Some("c"), Some(NOW));
        assert_eq!(decide(false, &unit, &now()), KEEP);
    }

    #[test]
    fn a_context_of_unknown_provenance_is_kept_and_a_missing_status_is_classified() {
        let legacy = stored(Some("legacy"), Some("c"), None);
        assert_eq!(decide(false, &legacy, &now()), KEEP);
        let unclassified = stored(None, Some("c"), None);
        assert_eq!(
            decide(false, &unclassified, &now()),
            Decision::Keep {
                classify_legacy: true
            }
        );
    }

    #[test]
    fn a_changed_prompt_or_model_is_the_refreshs_business_not_a_reason_to_pay_on_a_re_embed() {
        let unit = stored(Some("ready"), Some("c"), Some(OLD));
        assert_eq!(
            decide(false, &unit, &current(PromptVersion::V2, "gpt-4o-mini")),
            KEEP
        );
        assert_eq!(
            decide(false, &unit, &current(PromptVersion::V1, "gpt-5-mini")),
            KEEP
        );
    }

    /// The reconciler's prompt refresh is the one run that regenerates a context made with
    /// another prompt version, whether its provenance is recorded or not. A context already made
    /// with the version in force is not refreshed (not even when the model alias differs: the
    /// refresh is of the prompt), a context made from these very inputs is kept, and a
    /// restricted document is never regenerated.
    #[test]
    fn a_prompt_refresh_regenerates_a_context_of_another_prompt_version_and_nothing_else() {
        let recorded = stored(Some("ready"), Some("c"), Some(OLD));
        let legacy = stored(Some("legacy"), Some("c"), None);
        let unclassified = stored(None, Some("c"), None);
        for unit in [&recorded, &legacy, &unclassified] {
            assert_eq!(
                decide(false, unit, &refreshing(PromptVersion::V2, "gpt-4o-mini")),
                Decision::Generate,
                "made with ctx-v1, ctx-v2 is in force: {:?}",
                unit.status
            );
            // The version already in force: nothing to refresh. (A unit with a recorded hash that
            // differs, same prompt and same model, is regenerated by any run: the document
            // changed under the chunk. That is asserted below.)
            let decision = decide(false, unit, &refreshing(PromptVersion::V1, "gpt-5-mini"));
            assert!(
                matches!(decision, Decision::Keep { .. }),
                "made with the version in force, another model alias: {decision:?}"
            );
            // An inline run (a re-embed) never pays for the rollout.
            assert!(matches!(
                decide(false, unit, &current(PromptVersion::V2, "gpt-4o-mini")),
                Decision::Keep { .. }
            ));
            // Restricted: its text is never sent, so it is never regenerated.
            assert!(matches!(
                decide(true, unit, &refreshing(PromptVersion::V2, "gpt-4o-mini")),
                Decision::Keep { .. }
            ));
        }
        // The version in force and the same model, with no provenance to compare: kept. With a
        // recorded hash that differs the document changed under the chunk, which any run
        // regenerates.
        assert!(matches!(
            decide(
                false,
                &legacy,
                &refreshing(PromptVersion::V1, "gpt-4o-mini")
            ),
            Decision::Keep { .. }
        ));
        assert_eq!(
            decide(
                false,
                &recorded,
                &refreshing(PromptVersion::V1, "gpt-4o-mini")
            ),
            Decision::Generate
        );
        // Made from these very inputs: kept, whatever the run is for.
        let same = stored(Some("ready"), Some("c"), Some(NOW));
        assert_eq!(
            decide(false, &same, &refreshing(PromptVersion::V2, "gpt-4o-mini")),
            KEEP
        );
        // A document that cannot be read cannot be refreshed, and nothing is thrown away.
        assert_eq!(
            decide(false, &recorded, &Reading::DocumentMissing),
            KEEP,
            "without the document"
        );
    }

    #[test]
    fn a_document_that_changed_under_the_chunk_is_regenerated() {
        let unit = stored(Some("ready"), Some("c"), Some(OLD));
        assert_eq!(decide(false, &unit, &now()), Decision::Generate);
    }

    #[test]
    fn without_the_document_a_stored_context_is_kept() {
        let unit = stored(Some("ready"), Some("c"), Some(OLD));
        assert_eq!(decide(false, &unit, &Reading::DocumentMissing), KEEP);
    }

    #[test]
    fn a_unit_without_a_context_is_generated_unless_its_status_forbids_it() {
        for status in [
            None,
            Some("pending"),
            Some("failed_transient"),
            Some("skipped_zdr"),
            // The policy is read now: a unit skipped while it was off is generated once it is lifted.
            Some("skipped_policy"),
            // So is the budget (CR-T9b): a unit the gate held is generated once the gate lets
            // the organization through, and the gate says so with a `Current` reading. A status
            // that nothing could lift would be a stop that never ends.
            Some("skipped_budget"),
        ] {
            assert_eq!(
                decide(false, &stored(status, None, None), &now()),
                Decision::Generate,
                "{status:?}"
            );
        }
        assert_eq!(
            decide(false, &stored(Some("failed_permanent"), None, None), &now()),
            Decision::Leave,
            "a permanent failure is not retried by a re-embed"
        );
    }

    #[test]
    fn a_restricted_document_is_recorded_and_never_generated_for() {
        let skipped = Decision::Record(ContextOutcome::skipped_zdr(ZdrReason::Document));
        assert_eq!(decide(true, &stored(None, None, None), &now()), skipped);
        // Even with a stale context: its text is not sent, so it is not regenerated.
        assert_eq!(
            decide(true, &stored(Some("ready"), Some("c"), Some(OLD)), &now()),
            KEEP
        );
    }

    #[test]
    fn a_missing_document_is_a_permanent_failure_with_its_own_class() {
        assert_eq!(
            decide(
                false,
                &stored(Some("pending"), None, None),
                &Reading::DocumentMissing
            ),
            Decision::Record(ContextOutcome::permanent(ErrorClass::DocMissing))
        );
    }

    #[test]
    fn with_generation_off_nothing_is_generated_or_recorded() {
        assert_eq!(
            decide(false, &stored(None, None, None), &Reading::Off),
            Decision::Leave
        );
        assert_eq!(
            decide(true, &stored(None, None, None), &Reading::Off),
            Decision::Leave
        );
        assert_eq!(
            decide(
                false,
                &stored(Some("ready"), Some("c"), Some(OLD)),
                &Reading::Off
            ),
            KEEP
        );
    }

    // ── the organization's operator policy (CR-T11) ─────────────────────────────────

    /// "Off means zero provider calls", as a property of the decision itself: whatever the
    /// unit is (restricted or not, any status, with or without a stored context, with or
    /// without an input hash), an organization whose policy is off, or whose policy cannot be
    /// determined, never gets a `Generate`. The pipeline calls the provider for nothing else.
    #[test]
    fn an_off_or_unknown_policy_never_generates_whatever_the_unit_is() {
        let statuses = [
            None,
            Some("pending"),
            Some("ready"),
            Some("legacy"),
            Some("skipped_zdr"),
            Some("skipped_policy"),
            Some("skipped_budget"),
            Some("failed_transient"),
            Some("failed_permanent"),
        ];
        for reading in [Reading::PolicyOff, Reading::PolicyUnknown, Reading::Off] {
            for restricted in [false, true] {
                for status in statuses {
                    for context in [None, Some("c")] {
                        for hash in [None, Some(OLD)] {
                            let unit = stored(status, context, hash);
                            assert_ne!(
                                decide(restricted, &unit, &reading),
                                Decision::Generate,
                                "restricted={restricted} status={status:?} context={context:?} hash={hash:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    /// An off policy marks the units it withheld (never silent), once.
    #[test]
    fn an_off_policy_marks_the_units_it_withholds_and_does_not_rewrite_the_mark() {
        for status in [
            None,
            Some("pending"),
            Some("failed_transient"),
            Some("skipped_zdr"),
        ] {
            assert_eq!(
                decide(false, &stored(status, None, None), &Reading::PolicyOff),
                Decision::Record(ContextOutcome::SkippedPolicy),
                "{status:?}"
            );
        }
        assert_eq!(
            decide(
                false,
                &stored(Some("skipped_policy"), None, None),
                &Reading::PolicyOff
            ),
            Decision::Leave,
            "already marked: a redelivery does not write it again"
        );
        // A permanent failure keeps its own, more specific, mark.
        assert_eq!(
            decide(
                false,
                &stored(Some("failed_permanent"), None, None),
                &Reading::PolicyOff
            ),
            Decision::Leave
        );
        // The operator's off is read before the budget: a unit the budget had held is marked by
        // the policy now, and goes back to the budget's mark only if the policy is lifted while
        // the budget is still used up.
        assert_eq!(
            decide(
                false,
                &stored(Some("skipped_budget"), None, None),
                &Reading::PolicyOff
            ),
            Decision::Record(ContextOutcome::SkippedPolicy)
        );
    }

    /// Existing contexts are derived data in our own store: an off policy stops generation, it
    /// does not remove what exists, and the vector is still built from the stored context.
    #[test]
    fn an_off_policy_keeps_the_context_a_unit_already_has() {
        let unit = stored(Some("ready"), Some("c"), Some(OLD));
        assert_eq!(decide(false, &unit, &Reading::PolicyOff), KEEP);
        let legacy = stored(None, Some("c"), None);
        assert_eq!(
            decide(false, &legacy, &Reading::PolicyOff),
            Decision::Keep {
                classify_legacy: true
            }
        );
    }

    /// A restricted document keeps its zero-retention mark under an off policy: that one is
    /// the stronger and permanent fact, and it must survive the policy being lifted.
    #[test]
    fn a_restricted_document_keeps_its_zero_retention_mark_under_an_off_policy() {
        assert_eq!(
            decide(true, &stored(None, None, None), &Reading::PolicyOff),
            Decision::Record(ContextOutcome::skipped_zdr(ZdrReason::Document))
        );
    }

    /// An unknown policy records nothing: no status is written, so that the unit is picked up
    /// as it is when the policy can be read.
    #[test]
    fn an_unknown_policy_records_nothing() {
        for status in [
            None,
            Some("pending"),
            Some("skipped_policy"),
            Some("failed_transient"),
        ] {
            assert_eq!(
                decide(false, &stored(status, None, None), &Reading::PolicyUnknown),
                Decision::Leave,
                "{status:?}"
            );
        }
    }

    // ── the organization's background budget (CR-T9b) ───────────────────────────────

    /// "A stop makes zero provider calls", as a property of the decision itself, exactly as for
    /// an off policy: whatever the unit is, an organization whose budget is used up in `stop`
    /// mode never gets a `Generate`.
    #[test]
    fn a_stopped_budget_never_generates_whatever_the_unit_is() {
        let statuses = [
            None,
            Some("pending"),
            Some("ready"),
            Some("legacy"),
            Some("skipped_zdr"),
            Some("skipped_policy"),
            Some("skipped_budget"),
            Some("failed_transient"),
            Some("failed_permanent"),
        ];
        for restricted in [false, true] {
            for status in statuses {
                for context in [None, Some("c")] {
                    for hash in [None, Some(OLD)] {
                        let unit = stored(status, context, hash);
                        assert_ne!(
                            decide(restricted, &unit, &Reading::BudgetStopped),
                            Decision::Generate,
                            "restricted={restricted} status={status:?} context={context:?} hash={hash:?}"
                        );
                    }
                }
            }
        }
    }

    /// A stop marks the units it withholds (never silent), once, and never writes a mark over a
    /// stronger fact.
    #[test]
    fn a_stopped_budget_marks_the_units_it_withholds_and_does_not_rewrite_the_mark() {
        for status in [
            None,
            Some("pending"),
            Some("failed_transient"),
            Some("skipped_zdr"),
            Some("skipped_policy"),
        ] {
            assert_eq!(
                decide(false, &stored(status, None, None), &Reading::BudgetStopped),
                Decision::Record(ContextOutcome::SkippedBudget),
                "{status:?}"
            );
        }
        assert_eq!(
            decide(
                false,
                &stored(Some("skipped_budget"), None, None),
                &Reading::BudgetStopped
            ),
            Decision::Leave,
            "already marked: a redelivery or a re-embed does not write it again"
        );
        assert_eq!(
            decide(
                false,
                &stored(Some("failed_permanent"), None, None),
                &Reading::BudgetStopped
            ),
            Decision::Leave,
            "a permanent failure keeps its own mark"
        );
        // A restricted document keeps its zero-retention mark: the stronger and permanent fact.
        assert_eq!(
            decide(true, &stored(None, None, None), &Reading::BudgetStopped),
            Decision::Record(ContextOutcome::skipped_zdr(ZdrReason::Document))
        );
    }

    /// A stop stops generation; it removes nothing. The context a unit already has stays, and
    /// the vector is still built from it, whether the context is regenerable or not.
    #[test]
    fn a_stopped_budget_keeps_the_context_a_unit_already_has() {
        for unit in [
            stored(Some("ready"), Some("c"), Some(OLD)),
            stored(Some("ready"), Some("c"), Some(NOW)),
            stored(Some("legacy"), Some("c"), None),
        ] {
            assert_eq!(decide(false, &unit, &Reading::BudgetStopped), KEEP);
        }
        assert_eq!(
            decide(
                false,
                &stored(None, Some("c"), None),
                &Reading::BudgetStopped
            ),
            Decision::Keep {
                classify_legacy: true
            }
        );
    }

    /// A throttle defers; it never skips and never loses a context. What a deferred unit becomes
    /// depends only on what it has.
    #[test]
    fn a_throttled_unit_waits_without_losing_anything_and_without_churning_its_row() {
        // No context: marked pending (due at once, no class), unless it already is.
        for status in [None, Some("failed_transient"), Some("skipped_budget")] {
            assert_eq!(
                deferred(&stored(status, None, None)),
                Decision::Record(ContextOutcome::pending(
                    PendingReason::BudgetThrottled,
                    Duration::ZERO
                )),
                "{status:?}"
            );
        }
        assert_eq!(
            deferred(&stored(Some("pending"), None, None)),
            Decision::Leave,
            "already pending: nothing to write"
        );
        // A context: kept, whatever else is true (a regeneration that waits costs nothing).
        for status in [Some("ready"), Some("legacy"), None] {
            assert_eq!(
                deferred(&stored(status, Some("c"), Some(OLD))),
                KEEP,
                "{status:?}"
            );
        }
        // And a deferral is never a generation.
        assert_ne!(deferred(&stored(None, None, None)), Decision::Generate);
    }

    #[test]
    fn interleaving_starts_every_organization_in_the_first_round_and_keeps_each_ones_order() {
        let jobs = vec![("a", 1), ("a", 2), ("a", 3), ("b", 1), ("c", 1), ("c", 2)];
        let out = interleave_by_org(jobs, |(org, _)| org);
        assert_eq!(
            out,
            vec![("a", 1), ("b", 1), ("c", 1), ("a", 2), ("c", 2), ("a", 3)]
        );
        assert!(interleave_by_org(Vec::<(&str, i32)>::new(), |(org, _)| org).is_empty());
    }
}
