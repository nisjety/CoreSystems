//! Semantic-tier propagation for **bulk** memory erasure.
//!
//! Memory is dual-written: `memory_grpc::index_memory` writes the durable row
//! (`agent_memory`, via `dreaming::index_agent_memory`) and then indexes a
//! semantic copy on letta-bridge tagged with the *same* `memory_id`. Deleting
//! one memory erases both, because `memory_grpc::delete_memory` explicitly
//! calls `LettaMemoryAdapter::delete_detailed` after its own row is gone.
//!
//! The two BULK paths did not. `grpc::delete_thread_rows` and
//! `gdpr::purge_organization_data` are pure SQL: they removed the durable rows
//! and left the semantic copies — which hold the same customer content —
//! resident in the vector store, with no remaining id anywhere in Postgres to
//! find them by. A user deleting a thread, or an operator honouring an erasure
//! request, got a smaller deletion than the one they asked for and than the
//! summary reported.
//!
//! # Confirmation standard — deliberately the strict one
//!
//! `SessionCore::delete_space_threads` already erases semantic twins for the
//! formal Space-deletion flow, and it sets the bar this module matches rather
//! than re-deciding: a record counts as erased only when the tier replies
//! `deleted = true` **and** reports no degradation. Everything else is
//! `unconfirmed`.
//!
//! That is stricter than it first looks, and the reason is on that call site:
//! a `deleted = false` reply cannot distinguish an idempotent prior delete from
//! an unknown or mismatched semantic record. Both are plausible, only one is
//! erasure, and nothing on the wire separates them — so a tempting third
//! "absent, nothing to do" bucket would be an assumption dressed as an answer.
//! An unconfigured tier is likewise `unconfirmed`, not absent: this process
//! having no letta endpoint says nothing about what an earlier deployment
//! indexed.
//!
//! `unconfirmed > 0` is therefore the single signal that an erasure proof must
//! not claim completeness — see [`SemanticErasure::is_complete`]. Two different
//! confirmation standards for the same operation inside one service is the
//! drift this alignment exists to avoid.
//!
//! # Known limitation (deliberate, not an oversight)
//!
//! Propagation is inline and best-effort: a degraded tier is *reported*, not
//! retried. A durable retry queue is the fuller answer for an erasure
//! obligation and is tracked as follow-up; what is fixed here is that the
//! incompleteness is now counted, logged, and surfaced in the DSAR summary
//! instead of being silently invisible.

use crate::letta_adapter::LettaMemoryAdapter;
use crate::memory_grpc::LETTA_NOT_CONFIGURED;

/// Reported for a `deleted = false` reply carrying no degradation. Same string
/// `delete_space_threads` records in its receipt ledger, so the two surfaces
/// read identically in an audit.
pub(crate) const SEMANTIC_DELETE_NOT_CONFIRMED: &str = "semantic_delete_not_confirmed";

/// Reported when a bulk erasure carried more memories than one pass will
/// attempt. See [`MAX_SEMANTIC_DELETES`].
pub(crate) const SEMANTIC_ERASURE_TRUNCATED: &str = "DEGRADED_SEMANTIC_ERASURE_TRUNCATED";

/// Per-erasure ceiling on semantic deletes attempted in one pass.
///
/// `delete_detailed` costs two upstream RPCs per memory (the delete plus a
/// health probe), so an org-wide erasure over a very large corpus would
/// otherwise hold the consumer for an unbounded time. Anything past the
/// ceiling is counted `unconfirmed` under [`SEMANTIC_ERASURE_TRUNCATED`] rather
/// than dropped quietly — a cap that silently reported success would recreate
/// the exact bug this module fixes.
pub(crate) const MAX_SEMANTIC_DELETES: usize = 2_000;

/// A durable memory row that has been deleted, and whose semantic twin must go
/// with it. `owner` is `agent_memory.owner`, which is the `user_id` the
/// semantic copy was tagged with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ErasedMemory {
    pub(crate) memory_id: String,
    pub(crate) owner: String,
}

/// Outcome of propagating one bulk erasure to the semantic tier.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SemanticErasure {
    pub confirmed: u64,
    pub unconfirmed: u64,
    /// Populated whenever `unconfirmed > 0`; the first reason observed.
    pub degradation_reason: Option<&'static str>,
}

impl SemanticErasure {
    /// Whether every durable row's semantic twin was accounted for. False is
    /// the signal that an erasure proof must not claim completeness.
    pub fn is_complete(&self) -> bool {
        self.unconfirmed == 0
    }

    /// Total memories this pass reasoned about.
    pub fn considered(&self) -> u64 {
        self.confirmed + self.unconfirmed
    }

    fn record_unconfirmed(&mut self, count: u64, reason: &'static str) {
        self.unconfirmed += count;
        if self.degradation_reason.is_none() {
            self.degradation_reason = Some(reason);
        }
    }
}

/// Erase the semantic twins of rows already removed from Postgres.
///
/// Never returns an error: the durable rows are gone and the caller's
/// transaction has committed, so there is nothing left to abort. The outcome is
/// data for the caller's log line and erasure summary.
pub(crate) async fn erase_semantic_copies(
    letta: Option<&LettaMemoryAdapter>,
    org_id: &str,
    erased: &[ErasedMemory],
) -> SemanticErasure {
    let mut outcome = SemanticErasure::default();
    if erased.is_empty() {
        return outcome;
    }
    let Some(letta) = letta else {
        outcome.record_unconfirmed(erased.len() as u64, LETTA_NOT_CONFIGURED);
        return outcome;
    };

    let (attempted, deferred) = if erased.len() > MAX_SEMANTIC_DELETES {
        erased.split_at(MAX_SEMANTIC_DELETES)
    } else {
        (erased, &[] as &[ErasedMemory])
    };

    for entry in attempted {
        let result = letta
            .delete_detailed(org_id, &entry.owner, &entry.memory_id)
            .await;
        // Same predicate as `delete_space_threads`: a degradation alongside a
        // `true` still leaves the delete unproven, because the degradation may
        // be exactly why the reply cannot be trusted.
        match (result.deleted, result.degradation_reason) {
            (true, None) => outcome.confirmed += 1,
            (_, Some(reason)) => outcome.record_unconfirmed(1, reason),
            (false, None) => outcome.record_unconfirmed(1, SEMANTIC_DELETE_NOT_CONFIRMED),
        }
    }
    if !deferred.is_empty() {
        outcome.record_unconfirmed(deferred.len() as u64, SEMANTIC_ERASURE_TRUNCATED);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(n: usize) -> Vec<ErasedMemory> {
        (0..n)
            .map(|i| ErasedMemory {
                memory_id: format!("mem-{i}"),
                owner: "user-1".into(),
            })
            .collect()
    }

    #[tokio::test]
    async fn nothing_erased_is_complete_and_silent() {
        let outcome = erase_semantic_copies(None, "org-1", &[]).await;
        assert_eq!(outcome, SemanticErasure::default());
        assert!(outcome.is_complete());
        assert_eq!(outcome.considered(), 0);
    }

    /// The honesty invariant. An unconfigured tier must not let an erasure
    /// summary claim completeness: this process having no letta endpoint says
    /// nothing about what an earlier deployment indexed.
    #[tokio::test]
    async fn unconfigured_tier_is_unconfirmed_not_absent() {
        let erased = entries(3);
        let outcome = erase_semantic_copies(None, "org-1", &erased).await;

        assert_eq!(outcome.unconfirmed, 3, "must not be silently written off");
        assert_eq!(outcome.confirmed, 0);
        assert_eq!(outcome.degradation_reason, Some(LETTA_NOT_CONFIGURED));
        assert!(
            !outcome.is_complete(),
            "an unconfigured tier cannot yield a complete erasure proof"
        );
        assert_eq!(outcome.considered(), 3);
    }

    /// The cap must be visible in the proof, never a silent truncation.
    #[tokio::test]
    async fn over_the_ceiling_is_reported_not_dropped() {
        let erased = entries(MAX_SEMANTIC_DELETES + 5);
        // With no adapter every entry is unconfirmed anyway, so the ceiling is
        // asserted on the accounting identity rather than on the split: the
        // total considered must still equal the input, whatever path each
        // entry took.
        let outcome = erase_semantic_copies(None, "org-1", &erased).await;
        assert_eq!(
            outcome.considered(),
            erased.len() as u64,
            "every input row must be accounted for in exactly one bucket"
        );
    }

    #[test]
    fn only_unconfirmed_breaks_completeness() {
        let clean = SemanticErasure {
            confirmed: 4,
            unconfirmed: 0,
            degradation_reason: None,
        };
        assert!(clean.is_complete());

        let mut degraded = clean;
        degraded.record_unconfirmed(1, SEMANTIC_ERASURE_TRUNCATED);
        assert!(!degraded.is_complete());
        assert_eq!(
            degraded.degradation_reason,
            Some(SEMANTIC_ERASURE_TRUNCATED)
        );
    }

    /// The first reason wins so a later generic truncation cannot mask the
    /// specific transport failure an operator needs to see.
    #[test]
    fn first_degradation_reason_is_retained() {
        let mut outcome = SemanticErasure::default();
        outcome.record_unconfirmed(1, "DEGRADED_RPC_UNAVAILABLE");
        outcome.record_unconfirmed(9, SEMANTIC_ERASURE_TRUNCATED);
        assert_eq!(outcome.degradation_reason, Some("DEGRADED_RPC_UNAVAILABLE"));
        assert_eq!(outcome.unconfirmed, 10);
    }
}

/// Source-text contract on the callers, following the precedent
/// `learning_events`'s tests set for "both terminal paths must emit".
///
/// The bug this module fixes was not a wrong line of code — it was a *missing*
/// one at a call site nobody was looking at. A unit test of
/// [`erase_semantic_copies`] cannot catch a fourth bulk-delete path that never
/// calls it, and neither can the type system: the returned `Vec` is inert if
/// dropped. So the invariant is asserted against the callers' own source.
///
/// If these fail because a file moved or a helper was renamed, re-point them.
/// Do not delete them: the assertion is the only thing standing between a new
/// bulk-delete path and silently orphaning customer content in a vector store.
#[cfg(test)]
mod caller_contract_tests {
    use std::path::{Path, PathBuf};

    fn src(file: &str) -> String {
        let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
    }

    /// Every row deleted has to be *identifiable* afterwards, or propagation
    /// has nothing to work with. A plain `DELETE` returns only a count.
    #[test]
    fn both_bulk_deletes_capture_the_memory_ids_they_remove() {
        for (file, table) in [
            (
                "grpc.rs",
                "DELETE FROM agent_memory WHERE session_id = $1 RETURNING id, owner",
            ),
            (
                "gdpr.rs",
                "DELETE FROM agent_memory WHERE org_id = $1 RETURNING id, owner",
            ),
        ] {
            assert!(
                src(file).contains(table),
                "{file} must capture the erased memory ids with RETURNING — without \
                 them the semantic twins are unreachable the instant the row is gone"
            );
        }
    }

    /// A degraded vector store must never roll back a delete Postgres already
    /// honoured, so propagation belongs strictly after the commit.
    #[test]
    fn propagation_happens_after_the_commit_in_every_path() {
        let grpc = src("grpc.rs");
        let mut checked = 0;
        // Each guarded gRPC path: find the propagation call, then require a
        // commit textually before it inside the same method.
        for (index, _) in grpc.match_indices("self.propagate_semantic_erasure(") {
            let preceding = &grpc[..index];
            let commit = preceding
                .rfind("tx.commit()")
                .expect("a propagation call with no preceding commit");
            let method_start = preceding
                .rfind("    async fn ")
                .expect("propagation call outside any method");
            assert!(
                commit > method_start,
                "propagate_semantic_erasure runs before its own method's commit; a \
                 degraded semantic tier would then be able to fail a delete that \
                 Postgres had already completed"
            );
            checked += 1;
        }
        assert_eq!(
            checked, 2,
            "expected exactly the two best-effort thread-delete paths to propagate \
             (delete_thread, delete_threads). A new one appeared or one stopped \
             propagating — if a new path reconciles durably instead, like \
             delete_space_threads does, update this count and say so here."
        );
    }

    /// `delete_space_threads` is the deliberate exception, and it must stay
    /// deliberate: it reconciles durably through its own receipt ledger, so it
    /// discards the helper's return value on purpose. An implicit drop would be
    /// indistinguishable from the original bug.
    #[test]
    fn the_durable_path_opts_out_explicitly() {
        let grpc = src("grpc.rs");
        assert!(
            grpc.contains("let _ = delete_thread_rows(&mut tx, thread_id).await?;"),
            "delete_space_threads must discard the erased-memory list EXPLICITLY; \
             an implicit drop reads exactly like the omission this module fixed"
        );
        assert!(
            grpc.contains("space_deletion_semantic_memory_receipts"),
            "the opt-out is only justified while that path still reconciles \
             semantic erasure durably through its receipt ledger"
        );
    }

    /// The GDPR summary must not be able to report a clean erasure while the
    /// semantic tier is unaccounted for.
    #[test]
    fn the_dsar_summary_reports_the_semantic_tier() {
        let gdpr = src("gdpr.rs");
        assert!(
            gdpr.contains("pub semantic_memory: crate::memory_erasure::SemanticErasure"),
            "PurgeSummary must carry the semantic-tier outcome or a DSAR response \
             built from it overstates what was erased"
        );
        assert!(
            gdpr.contains("fn erasure_is_complete"),
            "callers need one honest question to ask of a purge summary"
        );
        let total = gdpr
            .find("pub fn total(&self) -> u64 {")
            .expect("PurgeSummary::total not found");
        let total_body_end = gdpr[total..]
            .find("\n    }")
            .expect("total() has no closing brace")
            + total;
        assert!(
            !gdpr[total..total_body_end].contains("semantic_memory"),
            "total() counts Postgres rows deleted; folding a vector-store count \
             into it would make one number mean two different things"
        );
    }
}
