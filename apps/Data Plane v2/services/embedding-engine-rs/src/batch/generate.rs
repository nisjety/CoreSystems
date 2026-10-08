//! Generating the contexts of a batch: several documents at once, inside the inline
//! time box, so that nothing one document or one tenant does can hold up the rest.
//!
//! The batch used to contextualize its documents one after another, without a
//! deadline. One document whose provider was slow, or one tenant whose bearer could not
//! be minted, held every document behind it, and the whole consumer with them, for as
//! long as the provider took. Now:
//!
//! * documents run concurrently, a bounded number at a time
//!   (`CONTEXT_INLINE_DOCUMENT_CONCURRENCY`), each bounding its own chunks
//!   (`CONTEXTUAL_RETRIEVAL_CONCURRENCY`). The most provider calls in flight from one batch
//!   is the product, and it is bounded, so a burst of documents cannot open a hundred
//!   streams and crowd out the embedding traffic that shares the provider;
//! * documents start in rounds across organizations (see `plan::interleave_by_org`), so
//!   when the time runs out it is not the small organization that was never started;
//! * every document has its own deadline, never past the batch's. What is not finished by
//!   then comes back as an explicit outcome (`pending`, due at once) and is embedded raw;
//! * a document cannot outlive its deadline by more than `GRACE`, even when something the
//!   contextualizer does not time-box (a bearer mint that hangs) is what is stuck: the
//!   document's work is dropped and its chunks are given the same outcome.
//!
//! Nothing here decides what a result means; that is `context::outcome`'s, and what is
//! done with it is `contexts`'s.

use std::time::Duration;

use futures::stream::{self, StreamExt};
use tokio::time::Instant;

use crate::context::{
    BatchClock, ChunkInput, ChunkResult, ContextOutcome, PendingReason, PreparedDocument,
};
use crate::provider::contextualize::Contextualizer;

use super::plan::interleave_by_org;

/// How long past its own deadline a document is waited for before its work is dropped.
/// The contextualizer honours the deadline itself and returns at it; this is only the
/// bound on what it cannot see.
const GRACE: Duration = Duration::from_millis(500);

/// One document's chunks to contextualize.
pub(super) struct Job<'a> {
    pub prepared: &'a PreparedDocument<'a>,
    /// `(position in the batch, the chunk)`.
    pub chunks: Vec<(usize, ChunkInput<'a>)>,
}

fn deadline_outcome() -> ContextOutcome {
    // Due again at once: the reconciler takes it, and the unit is embedded raw until then.
    ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO)
}

/// Contextualize every job. One result per chunk, keyed by its position in the batch.
/// Infallible: every chunk of every job ends in an outcome.
pub(super) async fn run(
    contextualizer: &Contextualizer,
    jobs: Vec<Job<'_>>,
    clock: BatchClock,
) -> Vec<(usize, ChunkResult)> {
    let jobs = interleave_by_org(jobs, |job| job.prepared.input().org_id);
    stream::iter(jobs)
        .map(|job| async move { run_one(contextualizer, job, clock).await })
        // Unordered: an ordered buffer would hold a slot behind a slow document and quietly
        // lower the concurrency below what was set.
        .buffer_unordered(contextualizer.document_concurrency())
        .flat_map(stream::iter)
        .collect()
        .await
}

async fn run_one(
    contextualizer: &Contextualizer,
    job: Job<'_>,
    clock: BatchClock,
) -> Vec<(usize, ChunkResult)> {
    let deadline = clock.document_deadline(Instant::now());
    let inputs: Vec<ChunkInput<'_>> = job.chunks.iter().map(|(_, chunk)| *chunk).collect();
    let finished = tokio::time::timeout_at(
        deadline + GRACE,
        contextualizer.contextualize(job.prepared, &inputs, Some(deadline)),
    )
    .await;
    let results = finished.unwrap_or_else(|_| {
        tracing::warn!(
            org_id = job.prepared.input().org_id,
            document_id = job.prepared.input().document_id,
            chunks = job.chunks.len(),
            "contextual retrieval: a document did not finish within its inline deadline; \
             its chunks are left for the reconciler"
        );
        inputs
            .iter()
            .map(|_| ChunkResult::without_calls(deadline_outcome()))
            .collect()
    });
    job.chunks
        .into_iter()
        .map(|(position, _)| position)
        .zip(results)
        .collect()
}

#[cfg(test)]
mod tests {
    //! The orchestration of documents under a clock the tests own.
    //!
    //! Time is virtual: each test mints its bearers first (real I/O against the Auth Core
    //! stand-in), then freezes the clock, so a provider that takes 3,000 ms takes exactly
    //! that and no timer inside an HTTP client can fire early while a socket is merely idle.

    use std::sync::Arc;
    use std::time::Duration;

    use inference_client_rs::Reason;
    use tokio::time::Instant;

    use super::*;
    use crate::batch::testing::{settings, Tenants};
    use crate::context::{DocumentInput, DocumentMeta, InlineBudget};
    use crate::provider::contextualize::testing::{Behaviour, FakeModel, ModelPort};
    use crate::provider::contextualize::Settings;

    struct Setup {
        engine: Contextualizer,
        model: Arc<FakeModel>,
        tenants: Tenants,
        client: inference_client_rs::InferenceTokenClient,
    }

    async fn setup(tweak: impl FnOnce(&mut Settings)) -> Setup {
        let tenants = Tenants::start().await;
        let client = tenants.client();
        let model = FakeModel::new();
        let mut settings = settings();
        tweak(&mut settings);
        let engine =
            Contextualizer::new(Arc::new(ModelPort(model.clone())), client.clone(), settings);
        Setup {
            engine,
            model,
            tenants,
            client,
        }
    }

    impl Setup {
        /// Mint once for each organization through the real client, then freeze virtual time.
        async fn freeze(&self, orgs: &[&str]) {
            for org in orgs {
                self.client
                    .bearer(org, Reason::ContextualRetrieval)
                    .await
                    .expect("the stand-in answers every organization with a bearer");
            }
            tokio::time::pause();
        }
    }

    fn prepared<'a>(
        setup: &Setup,
        org: &'a str,
        id: &'a str,
        text: &'a str,
    ) -> PreparedDocument<'a> {
        setup.engine.prepare(DocumentInput {
            org_id: org,
            document_id: id,
            meta: DocumentMeta {
                title: "Rapport",
                source: "",
                date: "",
            },
            text,
            restricted: false,
            prompt_version: None,
            total_chunks: Some(1),
        })
    }

    /// One chunk per document: a document is its own chunk, which is what makes a call
    /// per document countable.
    const TEXT: &str = "Fjordfisk AS økte driftsmarginen til 31 prosent i 2025.";

    fn jobs<'a>(documents: &'a [PreparedDocument<'a>]) -> Vec<Job<'a>> {
        documents
            .iter()
            .enumerate()
            .map(|(position, prepared)| Job {
                prepared,
                chunks: vec![(
                    position,
                    ChunkInput {
                        index: 0,
                        text: TEXT,
                    },
                )],
            })
            .collect()
    }

    fn by_position(mut results: Vec<(usize, ChunkResult)>) -> Vec<ChunkResult> {
        results.sort_by_key(|(position, _)| *position);
        results.into_iter().map(|(_, result)| result).collect()
    }

    /// Took what was expected, within the milliseconds a timer rounds up to (every sleep and
    /// timeout rounds to the next millisecond, and a chain of them adds the roundings).
    fn assert_took(started: Instant, expected: Duration) {
        let took = started.elapsed();
        assert!(
            took >= expected && took < expected + Duration::from_millis(20),
            "took {took:?}, expected {expected:?}"
        );
    }

    fn is_deadline(result: &ChunkResult) -> bool {
        result.outcome == deadline_outcome()
    }

    async fn run_all(
        setup: &Setup,
        documents: &[PreparedDocument<'_>],
    ) -> (Instant, Vec<ChunkResult>) {
        let started = Instant::now();
        let clock = setup.engine.inline_budget().start(started);
        let results = by_position(run(&setup.engine, jobs(documents), clock).await);
        (started, results)
    }

    /// The bound is on documents at once, and it is what it says: eight documents that
    /// each take a second finish in two seconds with four at a time, in eight with one,
    /// and in one with eight. (One test per bound: freezing the clock is for good, and a
    /// second mint under a frozen clock would time out.)
    async fn eight_documents_with_the_bound_at(bound: usize, expected_seconds: u64) {
        let s = setup(|settings| {
            settings.document_concurrency = bound;
            settings.concurrency = 1;
            settings.inline_budget = InlineBudget::new(8_000, 30_000).unwrap();
        })
        .await;
        s.model
            .set_default(Behaviour::AnswerAfter(Duration::from_secs(1)));
        let orgs: Vec<String> = (0..8).map(|index| format!("orgc{index}")).collect();
        let refs: Vec<&str> = orgs.iter().map(String::as_str).collect();
        s.freeze(&refs).await;
        let ids: Vec<String> = (0..8).map(|index| format!("doc{index}")).collect();
        let documents: Vec<_> = (0..8)
            .map(|index| prepared(&s, &orgs[index], &ids[index], TEXT))
            .collect();

        let (started, results) = run_all(&s, &documents).await;

        assert_took(started, Duration::from_secs(expected_seconds));
        assert_eq!(s.model.peak_in_flight(), bound, "bound {bound}");
        assert!(results
            .iter()
            .all(|result| result.outcome.generated().is_some()));
        assert_eq!(s.model.call_count(), 8);
    }

    #[tokio::test]
    async fn four_documents_at_a_time_take_two_rounds() {
        eight_documents_with_the_bound_at(4, 2).await;
    }

    #[tokio::test]
    async fn one_document_at_a_time_is_sequential() {
        eight_documents_with_the_bound_at(1, 8).await;
    }

    #[tokio::test]
    async fn as_many_documents_as_there_are_run_in_one_round() {
        eight_documents_with_the_bound_at(8, 1).await;
    }

    /// One document whose provider never answers ends at its own deadline (8 s) and
    /// nothing else notices: the fast documents have their contexts at once, and the batch
    /// ends when the slow one's time is up, not when its provider gives up.
    #[tokio::test]
    async fn a_document_that_never_answers_ends_at_its_own_deadline_and_holds_nobody() {
        let s = setup(|settings| {
            settings.document_concurrency = 4;
            settings.inline_budget = InlineBudget::new(8_000, 20_000).unwrap();
        })
        .await;
        s.model.set_for_org("orgslow", Behaviour::Hang);
        s.freeze(&["orgslow", "orgfast"]).await;
        let ids = ["slow", "fast1", "fast2", "fast3"];
        let orgs = ["orgslow", "orgfast", "orgfast", "orgfast"];
        let documents: Vec<_> = (0..4)
            .map(|index| prepared(&s, orgs[index], ids[index], TEXT))
            .collect();

        let (started, results) = run_all(&s, &documents).await;

        assert_took(started, Duration::from_secs(8));
        assert!(is_deadline(&results[0]), "{:?}", results[0].outcome);
        for fast in &results[1..] {
            assert!(fast.outcome.generated().is_some(), "{:?}", fast.outcome);
        }
        assert_eq!(
            s.model.calls_for("orgslow").len(),
            1,
            "the slow tenant was tried once"
        );
    }

    /// The batch has its own deadline, and a document never runs past it: with one document
    /// at a time and 8 s per document inside a 20 s batch, the third gets what is left (4 s)
    /// and the rest are not started at all (no call, and no bearer asked for).
    #[tokio::test]
    async fn the_batch_deadline_bounds_the_documents_that_start_late() {
        let s = setup(|settings| {
            settings.document_concurrency = 1;
            settings.inline_budget = InlineBudget::new(8_000, 20_000).unwrap();
        })
        .await;
        s.model.set_default(Behaviour::Hang);
        s.freeze(&["orgslow"]).await;
        let ids: Vec<String> = (0..5).map(|index| format!("doc{index}")).collect();
        let documents: Vec<_> = ids
            .iter()
            .map(|id| prepared(&s, "orgslow", id, TEXT))
            .collect();

        let (started, results) = run_all(&s, &documents).await;

        assert_took(started, Duration::from_secs(20));
        assert!(results.iter().all(is_deadline));
        assert_eq!(
            s.model.call_count(),
            3,
            "the two that never started asked nothing"
        );
    }

    /// `CONTEXT_INLINE_BUDGET_MS=0` is "raw first": nothing is generated inline, nothing
    /// is minted, and every chunk is left for the reconciler.
    #[tokio::test]
    async fn a_zero_batch_budget_generates_nothing_inline() {
        let s =
            setup(|settings| settings.inline_budget = InlineBudget::new(8_000, 0).unwrap()).await;
        let documents = [prepared(&s, "orgzero", "doc", TEXT)];

        let (_, results) = run_all(&s, &documents).await;

        assert_eq!(results.len(), 1);
        assert!(is_deadline(&results[0]));
        assert_eq!(s.model.call_count(), 0);
        assert!(
            s.tenants.requests().is_empty(),
            "not even a bearer was asked for"
        );
    }

    /// What the contextualizer cannot time-box (a bearer mint against an Auth Core that is hung)
    /// still cannot hold a document past its deadline: the document's work is dropped half a
    /// second after it, and its chunks get the same outcome as any other that ran out of time,
    /// not the five-second mint timeout's "posture unavailable".
    #[tokio::test]
    async fn a_bearer_mint_that_hangs_cannot_hold_a_document_past_its_deadline() {
        let (_auth, client) = crate::batch::testing::hanging_auth().await;
        let model = FakeModel::new();
        let mut settings = settings();
        settings.inline_budget = InlineBudget::new(1_000, 2_000).unwrap();
        let engine = Contextualizer::new(Arc::new(ModelPort(model.clone())), client, settings);
        tokio::time::pause();
        let document = engine.prepare(DocumentInput {
            org_id: "orghung",
            document_id: "doc",
            meta: DocumentMeta {
                title: "Rapport",
                source: "",
                date: "",
            },
            text: TEXT,
            restricted: false,
            prompt_version: None,
            total_chunks: Some(1),
        });
        let documents = [document];

        let started = Instant::now();
        let clock = engine.inline_budget().start(started);
        let results = by_position(run(&engine, jobs(&documents), clock).await);

        assert!(is_deadline(&results[0]), "{:?}", results[0].outcome);
        assert_took(started, Duration::from_millis(1_500));
        assert_eq!(model.call_count(), 0);
    }

    /// With bounded concurrency a batch that holds three documents of one organization
    /// and one of another would start the one last. Interleaving starts it second: when the
    /// time runs out it is the large organization's tail that is left, not the small one.
    #[tokio::test]
    async fn the_small_organization_is_not_left_behind_the_large_ones_documents() {
        let s = setup(|settings| {
            settings.document_concurrency = 1;
            settings.inline_budget = InlineBudget::new(8_000, 8_000).unwrap();
        })
        .await;
        s.model
            .set_for_org("orgbig", Behaviour::AnswerAfter(Duration::from_secs(3)));
        s.model
            .set_for_org("orgsmall", Behaviour::AnswerAfter(Duration::from_secs(1)));
        s.freeze(&["orgbig", "orgsmall"]).await;
        // The order the stream delivered them in: the big organization's first.
        let ids = ["big1", "big2", "big3", "small1"];
        let orgs = ["orgbig", "orgbig", "orgbig", "orgsmall"];
        let documents: Vec<_> = (0..4)
            .map(|index| prepared(&s, orgs[index], ids[index], TEXT))
            .collect();

        let (_, results) = run_all(&s, &documents).await;

        assert!(
            results[3].outcome.generated().is_some(),
            "the small organization's document was not started in time: {:?}",
            results[3].outcome
        );
        assert!(results[0].outcome.generated().is_some());
        assert!(results[1].outcome.generated().is_some());
        assert!(
            is_deadline(&results[2]),
            "the big organization's tail is what is left raw"
        );
    }
}
