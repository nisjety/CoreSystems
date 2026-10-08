//! Graph extraction as a state machine over durable per-chunk state (design task CR-T12).
//!
//! The event consumer (`stream`) and the reconciler (`reconcile`) both reach a document's chunks
//! through here, so the rules below hold on both paths and one chunk is never extracted twice
//! at once:
//!
//! 1. **The document's own standing.** A document that is not visible to the whole organization
//!    is `skipped_visibility`; one classified so it must not reach the model is
//!    `skipped_zdr` / `zdr_document`. Zero provider calls, a marker on every chunk.
//! 2. **The organization's gate** ([`admit`]): the processing policy (`graph_extraction = off`
//!    is `skipped_policy`), the retention posture (the bearer's `zdr` claim: a zero-retention
//!    organization is `skipped_zdr` / `zdr_org` and gets no call at all; a posture Auth Core
//!    could not resolve is `pending`, never a send and never a skip for good) and the background
//!    budget (`stop` is `skipped_budget`, `throttle` works one chunk at a time, the default
//!    alerts and proceeds).
//! 3. **The chunk's own claim** (`claim_chunk`): one statement takes the chunk if it is due, counts
//!    the attempt and sets a lease. A chunk that is final (`ready`, `failed_permanent`), empty at
//!    the current prompt version, waiting out its backoff or held by another worker is simply
//!    not claimed: no call, no cost.
//! 4. **The call, then the record.** What came of the attempt ([`Outcome`]) is settled in one
//!    statement ([`Outcome::settle`]): `ready` or `empty` with the model and prompt version
//!    that produced them, a failure with its class and its place on the ladder, or the poison
//!    cap. A chunk that reaches `failed_permanent` is dead-lettered once, whatever the event
//!    verifier's mode.
//!
//! A pass stops early on an outcome that says something about the deployment and not the chunk
//! (a refused credential, a throttle, a provider that is down, a zero-retention route that does
//! not exist), and after [`Settings::breaker`] failures in a row: a provider that is down is
//! asked once, not once per chunk of the backlog.
//!
//! Everything that touches the outside world is behind [`Ports`], so the rules are tested
//! end to end with the real extractor, the real bearer client and the real state SQL where a
//! database is available, and with an in-memory store where it is not.

use std::time::Duration;

use anyhow::Context;
use inference_client_rs::{InferenceError, Reason};

use crate::budget::{self, GlobalBudget, OrgPolicy, Verdict};
use crate::config::Config;
use crate::extractor::{Extracted, GraphExtractor, Posture};
use crate::model::ExtractionResult;
use crate::neo4j::Neo4jClient;
use crate::state::{
    egress_allowed, failure_fields, outcome_for_error, outcome_for_failure, Claim, ErrorClass,
    Outcome, Settlement, Stamp, Standing, Status, ZdrReason, PROMPT_VERSION,
};
use crate::store::GraphStore;

/// Which path reached the chunk. Only for the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Stream,
    Reconcile,
}

impl Origin {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stream => "stream",
            Self::Reconcile => "reconcile",
        }
    }
}

/// How the pipeline is tuned. Every value has a default and a boot-time check (`Config`).
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    /// The poison cap: attempts after which a chunk is `failed_permanent`.
    pub max_attempts: i32,
    /// How long a claim holds a chunk: longer than the call can take, so a worker that dies
    /// mid-call leaves a lease that expires and not one that holds the chunk for good.
    pub lease: Duration,
    /// Failures in a row after which a pass stops asking.
    pub breaker: usize,
    /// How many chunks one marking statement may touch.
    pub mark_batch: i64,
    pub budget: GlobalBudget,
}

/// The failures in a row after which a pass stops.
pub(crate) const BREAKER: usize = 3;

/// The chunks one marking statement writes at most: a bound on a statement, not on the work (the
/// next pass continues where this one stopped, because the rows already marked are not selected
/// again).
pub(crate) const MARK_BATCH: i64 = 2_000;

/// What a chunk's extraction wrote, as the pipeline needs it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Persisted {
    /// False only when the chunk's document was not visible to the organization: nothing was
    /// written.
    pub visible: bool,
    pub entities: usize,
    pub relationships: usize,
    pub claims: usize,
    /// For the contradiction sweep that follows a document.
    pub claim_ids: Vec<String>,
}

/// A chunk that was given up on: published to the dead-letter stream once, content-free.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeadLetter {
    pub org_id: String,
    pub document_id: String,
    pub knowledge_id: String,
    pub error_class: ErrorClass,
    pub attempts: i32,
}

impl DeadLetter {
    /// The payload. A closed class and ids: free text is where document content leaks.
    pub(crate) fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "original_subject": crate::stream::SUBJECT,
            "document_id": self.document_id,
            "org_id": self.org_id,
            "knowledge_id": self.knowledge_id,
            "error": self.error_class.as_str(),
            "error_class": self.error_class.as_str(),
            "attempts": self.attempts,
            "prompt_version": PROMPT_VERSION,
            "reason": "poison_cap",
        })
    }
}

/// Everything the pipeline needs from the world.
pub(crate) trait Ports {
    fn settings(&self) -> &Settings;

    // ── state ────────────────────────────────────────────────────────────────
    async fn claim_chunk(&self, org_id: &str, knowledge_id: &str) -> anyhow::Result<Option<Claim>>;
    async fn settle_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<bool>;
    async fn mark_document(
        &self,
        org_id: &str,
        document_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<u64>;
    async fn mark_org(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64>;
    async fn mark_restricted(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64>;
    async fn document_standing(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Option<Standing>>;
    /// The chunks of a document that is live and visible to the whole organization.
    async fn load_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>>;
    async fn due_documents(&self, org_id: &str, limit: i64) -> anyhow::Result<Vec<String>>;

    // ── the gate ─────────────────────────────────────────────────────────────
    async fn org_policy(&self, org_id: &str) -> anyhow::Result<OrgPolicy>;
    /// The month's background LLM spend in dollars; `None` when it cannot be measured.
    async fn month_spend_usd(&self, org_id: &str) -> anyhow::Result<Option<f64>>;
    async fn posture(&self, org_id: &str, reason: Reason) -> Result<Posture, InferenceError>;

    // ── the work ─────────────────────────────────────────────────────────────
    async fn extract(&self, org_id: &str, text: &str) -> anyhow::Result<Extracted>;
    async fn persist(
        &self,
        org_id: &str,
        knowledge_id: &str,
        extraction: &ExtractionResult,
    ) -> anyhow::Result<Persisted>;
    async fn dead_letter(&self, letter: &DeadLetter);

    // ── community summaries ──────────────────────────────────────────────────
    async fn communities_needing_summary(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, Vec<String>)>>;
    async fn summarize(&self, org_id: &str, labels: &[String]) -> anyhow::Result<String>;
    async fn store_summary(
        &self,
        org_id: &str,
        community_id: &str,
        summary: &str,
    ) -> anyhow::Result<bool>;
}

/// How many chunks (or summaries) a pass may still work: unlimited, or capped by `throttle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Allowance(Option<usize>);

impl Allowance {
    pub(crate) const fn unlimited() -> Self {
        Self(None)
    }

    pub(crate) const fn limited(count: usize) -> Self {
        Self(Some(count))
    }

    fn exhausted(&self) -> bool {
        self.0 == Some(0)
    }

    fn take(&mut self) {
        if let Some(remaining) = self.0.as_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }
}

/// What the organization's gate decided.
#[derive(Debug, PartialEq)]
pub(crate) enum Admission {
    Proceed(Allowance),
    /// Nothing is sent. The outcome is what the organization's chunks are marked with.
    Blocked(Outcome),
}

/// The organization's gate: policy, then retention posture, then budget. In that order because
/// each answer makes the next one moot: an organization that turned graph extraction off needs
/// no bearer, and a zero-retention one needs no budget.
///
/// A database failure reading the policy or the posture is an error, never "proceed": a gate
/// that cannot be read must not be read as open.
pub(crate) async fn admit<P: Ports>(
    ports: &P,
    org_id: &str,
    reason: Reason,
) -> anyhow::Result<Admission> {
    let policy = ports
        .org_policy(org_id)
        .await
        .context("read the organization's processing policy")?;
    if policy.graph_extraction_is_off() {
        return Ok(Admission::Blocked(Outcome::SkippedPolicy));
    }

    match ports.posture(org_id, reason).await {
        Ok(Posture { zdr: true }) => {
            return Ok(Admission::Blocked(Outcome::SkippedZdr(ZdrReason::Org)));
        }
        Ok(Posture { zdr: false }) => {}
        Err(error) => {
            let outcome = outcome_for_failure(&error);
            let error = anyhow::Error::new(error);
            let fields = failure_fields(&error);
            tracing::warn!(
                org_id,
                reason = %reason,
                failure = fields.kind,
                stage = fields.stage,
                grpc_code = fields.grpc_code.as_deref(),
                provider_error = fields.provider_error.as_deref(),
                "graph extraction waits: no bearer for this organization"
            );
            return Ok(Admission::Blocked(outcome));
        }
    }

    let global = &ports.settings().budget;
    let spend = if budget::needs_spend(&policy, global) {
        match ports.month_spend_usd(org_id).await {
            Ok(spend) => spend,
            Err(error) => {
                tracing::warn!(org_id, %error, "the background spend could not be read");
                None
            }
        }
    } else {
        None
    };
    match budget::decide(&policy, global, spend) {
        Verdict::Proceed => Ok(Admission::Proceed(Allowance::unlimited())),
        Verdict::Alert {
            spend_usd,
            budget_usd,
        } => {
            tracing::warn!(
                org_id,
                spend_usd,
                budget_usd,
                mode = "alert_only",
                "graph extraction: the background budget is exceeded; alert only, nothing is stopped"
            );
            Ok(Admission::Proceed(Allowance::unlimited()))
        }
        Verdict::Unmeasured { budget_usd } => {
            tracing::warn!(
                org_id,
                budget_usd,
                "graph extraction: a background budget applies and its spend cannot be measured \
                 (BACKGROUND_PRICE_INPUT_USD_PER_MTOK and BACKGROUND_PRICE_OUTPUT_USD_PER_MTOK, or the ledger); proceeding"
            );
            Ok(Admission::Proceed(Allowance::unlimited()))
        }
        Verdict::Throttle {
            spend_usd,
            budget_usd,
        } => {
            tracing::warn!(
                org_id,
                spend_usd,
                budget_usd,
                mode = "throttle",
                "graph extraction: the background budget is exceeded; working one chunk at a time"
            );
            Ok(Admission::Proceed(Allowance::limited(1)))
        }
        Verdict::Stop {
            spend_usd,
            budget_usd,
        } => {
            tracing::warn!(
                org_id,
                spend_usd,
                budget_usd,
                mode = "stop",
                "graph extraction withheld: the background budget is used up and the budget mode is stop"
            );
            Ok(Admission::Blocked(Outcome::SkippedBudget))
        }
    }
}

/// What came of one document's pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DocumentReport {
    /// The chunks the document has that are visible to the organization.
    pub chunks: usize,
    /// Chunks that stored something.
    pub ready: usize,
    /// Chunks that were extracted and held nothing.
    pub empty: usize,
    /// Chunks that were not due: final, waiting, or held by another worker. No call was made.
    pub not_due: usize,
    /// Chunks that were skipped, deferred or failed this pass.
    pub other: usize,
    /// Chunks that reached `failed_permanent` this pass.
    pub given_up: usize,
    /// What the chunks that stored something stored.
    pub entities: usize,
    pub relationships: usize,
    pub claims: usize,
    pub claim_ids: Vec<String>,
    /// The pass stopped before the last chunk: the breaker, or an outcome about the deployment.
    pub halted: bool,
    /// Chunks that were marked without a call (the document's own standing, or the gate).
    pub marked: u64,
}

/// The document's own standing: whether its chunks may be looked at, and if not, why.
enum DocumentDecision {
    /// There is no such live document.
    Absent,
    Skip(Outcome),
    Eligible,
}

async fn decide_document<P: Ports>(
    ports: &P,
    org_id: &str,
    document_id: &str,
) -> anyhow::Result<DocumentDecision> {
    let Some(standing) = ports.document_standing(org_id, document_id).await? else {
        return Ok(DocumentDecision::Absent);
    };
    // Visibility first: it is the stronger reason, and the rule that has not changed. Graph
    // entities are organization-shared, so extracting a private document would leak its content
    // organization-wide through the graph arm.
    if !standing.visible {
        return Ok(DocumentDecision::Skip(Outcome::SkippedVisibility));
    }
    if !egress_allowed(Some(&standing.classification)) {
        return Ok(DocumentDecision::Skip(Outcome::SkippedZdr(
            ZdrReason::Document,
        )));
    }
    Ok(DocumentDecision::Eligible)
}

/// One document announced by an event: its standing, then the organization's gate, then its
/// chunks.
pub(crate) async fn handle_document<P: Ports>(
    ports: &P,
    origin: Origin,
    org_id: &str,
    document_id: &str,
) -> anyhow::Result<DocumentReport> {
    let max_attempts = ports.settings().max_attempts;
    match decide_document(ports, org_id, document_id).await? {
        DocumentDecision::Absent => {
            tracing::info!(
                org_id,
                document_id,
                origin = origin.as_str(),
                "graph extraction skipped: there is no live document (deleted, or never indexed)"
            );
            return Ok(DocumentReport::default());
        }
        DocumentDecision::Skip(outcome) => {
            let marked = ports
                .mark_document(org_id, document_id, &outcome.settle(0, max_attempts))
                .await?;
            log_document_skipped(origin, org_id, document_id, &outcome, marked);
            return Ok(DocumentReport {
                marked,
                ..DocumentReport::default()
            });
        }
        DocumentDecision::Eligible => {}
    }
    match admit(ports, org_id, Reason::GraphExtraction).await? {
        Admission::Blocked(outcome) => {
            let marked = ports
                .mark_document(org_id, document_id, &outcome.settle(0, max_attempts))
                .await?;
            log_document_skipped(origin, org_id, document_id, &outcome, marked);
            Ok(DocumentReport {
                marked,
                ..DocumentReport::default()
            })
        }
        Admission::Proceed(mut allowance) => {
            process_chunks(ports, origin, org_id, document_id, &mut allowance).await
        }
    }
}

fn log_document_skipped(
    origin: Origin,
    org_id: &str,
    document_id: &str,
    outcome: &Outcome,
    marked: u64,
) {
    let settlement = outcome.settle(0, DEFAULT_FOR_LOG);
    tracing::info!(
        org_id,
        document_id,
        origin = origin.as_str(),
        status = settlement.status.as_str(),
        error_class = settlement.error_class.map(ErrorClass::as_str),
        marked,
        "graph extraction skipped for this document: no provider call was made"
    );
}

/// A cap that never matters to a log line (skips and waits do not count).
const DEFAULT_FOR_LOG: i32 = crate::state::DEFAULT_MAX_ATTEMPTS;

/// The chunks of one document, one claim and one call at a time.
pub(crate) async fn process_chunks<P: Ports>(
    ports: &P,
    origin: Origin,
    org_id: &str,
    document_id: &str,
    allowance: &mut Allowance,
) -> anyhow::Result<DocumentReport> {
    let settings = ports.settings().clone();
    let chunks = ports.load_chunks(org_id, document_id).await?;
    let mut report = DocumentReport {
        chunks: chunks.len(),
        ..DocumentReport::default()
    };
    let mut consecutive_failures = 0usize;
    for (knowledge_id, text) in &chunks {
        if allowance.exhausted() {
            report.halted = true;
            break;
        }
        let Some(claim) = ports.claim_chunk(org_id, knowledge_id).await? else {
            report.not_due += 1;
            continue;
        };
        allowance.take();

        let (outcome, error, persisted) = attempt(ports, org_id, knowledge_id, text).await;
        let settlement = outcome.settle(claim.attempts, settings.max_attempts);
        if !ports
            .settle_chunk(org_id, knowledge_id, &settlement)
            .await?
        {
            tracing::warn!(
                org_id,
                knowledge_id,
                "the chunk's state is gone (the document was re-chunked while it was extracted); \
                 nothing to record"
            );
        }
        log_settled(
            origin,
            org_id,
            document_id,
            knowledge_id,
            &claim,
            &outcome,
            &settlement,
            error.as_ref(),
        );

        match settlement.status {
            Status::Ready => report.ready += 1,
            Status::Empty => report.empty += 1,
            _ => report.other += 1,
        }
        if let Some(persisted) = persisted {
            report.entities += persisted.entities;
            report.relationships += persisted.relationships;
            report.claims += persisted.claims;
            report.claim_ids.extend(persisted.claim_ids);
        }
        if settlement.status == Status::FailedPermanent {
            report.given_up += 1;
            if let Some(error_class) = settlement.error_class {
                ports
                    .dead_letter(&DeadLetter {
                        org_id: org_id.to_owned(),
                        document_id: document_id.to_owned(),
                        knowledge_id: knowledge_id.clone(),
                        error_class,
                        attempts: claim.attempts,
                    })
                    .await;
            }
        }

        if outcome.is_failure() {
            consecutive_failures += 1;
        } else {
            consecutive_failures = 0;
        }
        if outcome.is_systemic() || consecutive_failures >= settings.breaker {
            report.halted = true;
            break;
        }
    }
    Ok(report)
}

/// The call and what it stored: an outcome, the error behind it for the log, and what was
/// persisted.
async fn attempt<P: Ports>(
    ports: &P,
    org_id: &str,
    knowledge_id: &str,
    text: &str,
) -> (Outcome, Option<anyhow::Error>, Option<Persisted>) {
    let extracted = match ports.extract(org_id, text).await {
        Ok(extracted) => extracted,
        Err(error) => return (outcome_for_error(&error), Some(error), None),
    };
    match ports.persist(org_id, knowledge_id, &extracted.result).await {
        Ok(persisted) if !persisted.visible => {
            // The document stopped being visible to the organization between the claim and the
            // write. Nothing was written, and the call is paid for (the extractor recorded it).
            (Outcome::SkippedVisibility, None, Some(persisted))
        }
        Ok(persisted) => {
            let stamp = Stamp {
                prompt_version: PROMPT_VERSION.to_owned(),
                model: extracted.model,
                provider: extracted.provider,
                entities: count(persisted.entities),
                relationships: count(persisted.relationships),
                claims: count(persisted.claims),
            };
            (Outcome::Extracted(stamp), None, Some(persisted))
        }
        // The provider answered and was paid; the database did not take it. The chunk is tried
        // again and counts toward the cap, so a chunk the database keeps refusing is bounded.
        Err(error) => (
            Outcome::Transient {
                class: ErrorClass::Internal,
                retry_after: None,
            },
            Some(error.context("store the extraction")),
            None,
        ),
    }
}

fn count(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// One line per settled chunk, with the typed fields as fields: the gRPC code, the decoded kind
/// and the provider's own kind are what an operator groups by, and the old line carried only the
/// outer context of the error. No prompts, texts or provider bodies are ever logged.
#[allow(clippy::too_many_arguments)]
fn log_settled(
    origin: Origin,
    org_id: &str,
    document_id: &str,
    knowledge_id: &str,
    claim: &Claim,
    outcome: &Outcome,
    settlement: &Settlement,
    error: Option<&anyhow::Error>,
) {
    let fields = error.map(failure_fields).unwrap_or_default();
    let retry_in_secs = settlement.retry_in.map(|wait| wait.as_secs());
    match outcome {
        Outcome::Extracted(stamp) => tracing::info!(
            org_id,
            document_id,
            knowledge_id,
            origin = origin.as_str(),
            status = settlement.status.as_str(),
            model = %stamp.model,
            prompt_version = %stamp.prompt_version,
            entities = stamp.entities,
            relationships = stamp.relationships,
            claims = stamp.claims,
            attempts = claim.attempts,
            "graph extraction recorded"
        ),
        Outcome::SkippedZdr(_)
        | Outcome::SkippedPolicy
        | Outcome::SkippedVisibility
        | Outcome::SkippedBudget
        | Outcome::PostureUnavailable { .. } => tracing::info!(
            org_id,
            document_id,
            knowledge_id,
            origin = origin.as_str(),
            status = settlement.status.as_str(),
            error_class = settlement.error_class.map(ErrorClass::as_str),
            failure = fields.kind,
            stage = fields.stage,
            grpc_code = fields.grpc_code.as_deref(),
            provider_error = fields.provider_error.as_deref(),
            retry_in_secs,
            "graph extraction deferred or skipped: the chunk was not sent"
        ),
        Outcome::ResidencyUnavailable | Outcome::Transient { .. } | Outcome::Permanent { .. } => {
            if settlement.status == Status::FailedPermanent {
                tracing::error!(
                    org_id,
                    document_id,
                    knowledge_id,
                    origin = origin.as_str(),
                    status = settlement.status.as_str(),
                    error_class = settlement.error_class.map(ErrorClass::as_str),
                    failure = fields.kind,
                    stage = fields.stage,
                    grpc_code = fields.grpc_code.as_deref(),
                    provider_error = fields.provider_error.as_deref(),
                    attempts = claim.attempts,
                    "graph extraction GIVEN UP on a chunk: it will not be retried until an operator requeues it"
                );
            } else {
                tracing::warn!(
                    org_id,
                    document_id,
                    knowledge_id,
                    origin = origin.as_str(),
                    status = settlement.status.as_str(),
                    error_class = settlement.error_class.map(ErrorClass::as_str),
                    failure = fields.kind,
                    stage = fields.stage,
                    grpc_code = fields.grpc_code.as_deref(),
                    provider_error = fields.provider_error.as_deref(),
                    attempts = claim.attempts,
                    retry_in_secs,
                    "graph extraction failed"
                );
            }
        }
    }
}

/// What came of one organization's reconcile pass.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OrgReport {
    /// Documents in which at least one chunk stored something.
    pub healed: usize,
    pub documents: usize,
    /// Chunks that reached `failed_permanent` in this pass.
    pub given_up: usize,
    /// Set when the organization's gate withheld the pass; nothing was sent.
    pub blocked: Option<Outcome>,
    /// Chunks marked without a call.
    pub marked: u64,
}

/// One organization's turn in the reconciler: the gate, the markers for what is withheld, then up
/// to `batch` documents that have work.
pub(crate) async fn run_org_pass<P: Ports>(
    ports: &P,
    org_id: &str,
    batch: i64,
) -> anyhow::Result<OrgReport> {
    let settings = ports.settings();
    let max_attempts = settings.max_attempts;
    let restricted = Outcome::SkippedZdr(ZdrReason::Document).settle(0, max_attempts);
    let mut report = OrgReport::default();

    // The chunks of restricted documents are never sent; a coverage figure should say why they
    // have no graph. Bounded, and a marker is not worth failing the pass for.
    match ports.mark_restricted(org_id, &restricted).await {
        Ok(marked) => report.marked += marked,
        Err(error) => tracing::warn!(org_id, %error, "could not mark restricted documents"),
    }

    let mut allowance = match admit(ports, org_id, Reason::GraphExtraction).await? {
        Admission::Blocked(outcome) => {
            let marked = ports
                .mark_org(org_id, &outcome.settle(0, max_attempts))
                .await?;
            let settlement = outcome.settle(0, max_attempts);
            tracing::info!(
                org_id,
                status = settlement.status.as_str(),
                error_class = settlement.error_class.map(ErrorClass::as_str),
                marked,
                "graph reconcile: the organization is withheld; no provider call was made"
            );
            report.marked += marked;
            report.blocked = Some(outcome);
            return Ok(report);
        }
        Admission::Proceed(allowance) => allowance,
    };

    for document_id in ports.due_documents(org_id, batch).await? {
        if allowance.exhausted() {
            break;
        }
        let document = process_chunks(
            ports,
            Origin::Reconcile,
            org_id,
            &document_id,
            &mut allowance,
        )
        .await?;
        report.documents += 1;
        report.given_up += document.given_up;
        if document.ready > 0 {
            report.healed += 1;
        }
        tracing::info!(
            org_id,
            document_id = %document_id,
            chunks = document.chunks,
            ready = document.ready,
            empty = document.empty,
            not_due = document.not_due,
            other = document.other,
            given_up = document.given_up,
            halted = document.halted,
            "graph reconcile: document processed"
        );
        if document.halted {
            break;
        }
    }
    Ok(report)
}

/// The summaries of one organization's communities that lack one, under the same gate as the
/// chunks (policy, posture with its own reason, budget) and the same typed failures.
pub(crate) async fn summarize_communities<P: Ports>(
    ports: &P,
    org_id: &str,
    limit: i64,
) -> anyhow::Result<usize> {
    let mut allowance = match admit(ports, org_id, Reason::GraphCommunitySummary).await? {
        Admission::Blocked(outcome) => {
            let settlement = outcome.settle(0, DEFAULT_FOR_LOG);
            tracing::info!(
                org_id,
                status = settlement.status.as_str(),
                error_class = settlement.error_class.map(ErrorClass::as_str),
                "community summaries skipped: the organization is withheld; no provider call was made"
            );
            return Ok(0);
        }
        Admission::Proceed(allowance) => allowance,
    };
    let pending = ports.communities_needing_summary(org_id, limit).await?;
    let mut written = 0usize;
    let mut consecutive_failures = 0usize;
    for (community_id, labels) in pending {
        if allowance.exhausted() {
            break;
        }
        allowance.take();
        match ports.summarize(org_id, &labels).await {
            Ok(summary) => {
                consecutive_failures = 0;
                match ports.store_summary(org_id, &community_id, &summary).await {
                    // `false` means another run summarised it first: not an error.
                    Ok(true) => written += 1,
                    Ok(false) => tracing::debug!(
                        org_id,
                        community_id,
                        "community summary already present; skipped"
                    ),
                    Err(error) => {
                        tracing::warn!(%error, org_id, community_id, "store community summary failed")
                    }
                }
            }
            Err(error) => {
                let outcome = outcome_for_error(&error);
                let fields = failure_fields(&error);
                tracing::warn!(
                    org_id,
                    community_id,
                    failure = fields.kind,
                    stage = fields.stage,
                    grpc_code = fields.grpc_code.as_deref(),
                    provider_error = fields.provider_error.as_deref(),
                    "community summarisation failed"
                );
                consecutive_failures += 1;
                if outcome.is_systemic() || consecutive_failures >= ports.settings().breaker {
                    break;
                }
            }
        }
    }
    if written > 0 {
        tracing::info!(org_id, written, "community summaries generated");
    }
    Ok(written)
}

// ── the production ports ─────────────────────────────────────────────────────

impl Settings {
    /// From the deployment's configuration, which has been validated at boot
    /// (`Config::validate`).
    pub(crate) fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        let timeout = Duration::from_millis(cfg.model_plane_extraction_timeout_ms.max(1));
        Ok(Self {
            max_attempts: i32::try_from(cfg.graph_extraction_max_attempts)
                .unwrap_or(crate::state::MAX_ATTEMPTS_CEILING),
            // Twice the call's own deadline and a minute: a worker that dies mid-call frees the
            // chunk within minutes, and a call that is merely slow is never claimed over.
            lease: timeout * 2 + Duration::from_secs(60),
            breaker: BREAKER,
            mark_batch: MARK_BATCH,
            budget: cfg.background_budget()?,
        })
    }
}

/// The ports backed by the real store, extractor, Neo4j mirror and NATS.
pub(crate) struct Live {
    pub store: std::sync::Arc<GraphStore>,
    pub extractor: std::sync::Arc<GraphExtractor>,
    pub neo4j: Option<std::sync::Arc<Neo4jClient>>,
    /// `None` when the service has no NATS connection (consumers disabled): a dead letter is then
    /// logged and nothing else.
    pub nats: Option<async_nats::Client>,
    pub settings: Settings,
}

impl Ports for Live {
    fn settings(&self) -> &Settings {
        &self.settings
    }

    async fn claim_chunk(&self, org_id: &str, knowledge_id: &str) -> anyhow::Result<Option<Claim>> {
        self.store
            .claim_chunk(org_id, knowledge_id, self.settings.lease)
            .await
    }

    async fn settle_chunk(
        &self,
        org_id: &str,
        knowledge_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<bool> {
        self.store
            .settle_chunk(org_id, knowledge_id, settlement)
            .await
    }

    async fn mark_document(
        &self,
        org_id: &str,
        document_id: &str,
        settlement: &Settlement,
    ) -> anyhow::Result<u64> {
        self.store
            .mark_document(org_id, document_id, settlement, self.settings.mark_batch)
            .await
    }

    async fn mark_org(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        self.store
            .mark_org(org_id, settlement, self.settings.mark_batch)
            .await
    }

    async fn mark_restricted(&self, org_id: &str, settlement: &Settlement) -> anyhow::Result<u64> {
        self.store
            .mark_restricted_documents(org_id, settlement, self.settings.mark_batch)
            .await
    }

    async fn document_standing(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Option<Standing>> {
        self.store.document_standing(org_id, document_id).await
    }

    async fn load_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        self.store
            .load_org_visible_chunks(org_id, document_id)
            .await
    }

    async fn due_documents(&self, org_id: &str, limit: i64) -> anyhow::Result<Vec<String>> {
        self.store.list_due_documents(org_id, limit).await
    }

    async fn org_policy(&self, org_id: &str) -> anyhow::Result<OrgPolicy> {
        self.store.org_policy(org_id).await
    }

    async fn month_spend_usd(&self, org_id: &str) -> anyhow::Result<Option<f64>> {
        // Dollars need a price table, which the deployment supplies and nothing defaults.
        let Some(prices) = self.settings.budget.price else {
            return Ok(None);
        };
        let (input, output) = self.store.month_llm_tokens(org_id).await?;
        Ok(Some(prices.spend_usd(input, output)))
    }

    async fn posture(&self, org_id: &str, reason: Reason) -> Result<Posture, InferenceError> {
        self.extractor.posture(org_id, reason).await
    }

    async fn extract(&self, org_id: &str, text: &str) -> anyhow::Result<Extracted> {
        self.extractor.extract(text, org_id).await
    }

    async fn persist(
        &self,
        org_id: &str,
        knowledge_id: &str,
        extraction: &ExtractionResult,
    ) -> anyhow::Result<Persisted> {
        let persisted = self
            .store
            .persist_extraction(org_id, knowledge_id, extraction)
            .await?;
        if !persisted.visible {
            return Ok(Persisted::default());
        }
        // A failure here is a failure of the chunk: entities without their mappings are invisible
        // to every reader (the graph arm joins through them), so the chunk is tried again; the
        // writes are idempotent.
        self.store
            .persist_text_unit_mappings(
                org_id,
                knowledge_id,
                &persisted.entity_ids,
                &persisted.rel_ids,
                &persisted.claim_ids,
            )
            .await?;
        // Best-effort Neo4j mirror. Postgres is canonical and the read-model is rebuildable, so
        // a mirror failure must not fail the chunk. This runs only on organization-visible,
        // non-restricted content: both gates sit above it.
        if let Some(neo4j) = &self.neo4j {
            if let Err(error) = neo4j
                .merge_extraction(
                    org_id,
                    &persisted.mirror_entities,
                    &persisted.mirror_relationships,
                )
                .await
            {
                tracing::warn!(%error, knowledge_id, "neo4j mirror failed (non-fatal; postgres is canonical)");
            }
        }
        Ok(Persisted {
            visible: true,
            entities: persisted.entity_ids.len(),
            relationships: persisted.rel_ids.len(),
            claims: persisted.claim_ids.len(),
            claim_ids: persisted.claim_ids,
        })
    }

    async fn dead_letter(&self, letter: &DeadLetter) {
        // Published whatever the event verifier's mode: the dead-letter subject is not one the
        // verifier governs (graph-index's key signs the cost subject and nothing else), and the
        // old gate on "no verifier configured" meant that in production, where events are
        // signed, a chunk that failed for good was never dead-lettered at all.
        let payload = letter.payload();
        let Some(nats) = &self.nats else {
            tracing::error!(
                org_id = %letter.org_id,
                document_id = %letter.document_id,
                knowledge_id = %letter.knowledge_id,
                error_class = letter.error_class.as_str(),
                attempts = letter.attempts,
                "dead letter NOT published: this service has no NATS connection"
            );
            return;
        };
        let bytes = serde_json::to_vec(&payload).unwrap_or_default();
        if let Err(error) = nats.publish(crate::stream::DLQ_SUBJECT, bytes.into()).await {
            tracing::error!(
                %error,
                org_id = %letter.org_id,
                knowledge_id = %letter.knowledge_id,
                "dead letter NOT published"
            );
        }
    }

    async fn communities_needing_summary(
        &self,
        org_id: &str,
        limit: i64,
    ) -> anyhow::Result<Vec<(String, Vec<String>)>> {
        self.store
            .list_communities_needing_summary(org_id, limit)
            .await
    }

    async fn summarize(&self, org_id: &str, labels: &[String]) -> anyhow::Result<String> {
        self.extractor.summarize_community(labels, org_id).await
    }

    async fn store_summary(
        &self,
        org_id: &str,
        community_id: &str,
        summary: &str,
    ) -> anyhow::Result<bool> {
        self.store
            .set_community_summary(org_id, community_id, summary)
            .await
    }
}

#[cfg(test)]
pub(crate) mod harness;
#[cfg(test)]
mod tests;
