//! The engine against the rules it really runs under.
//!
//! Auth Core is a loopback stand-in that answers with the bytes its real mint
//! captured, read by the real shared token client with all of its claim checks. The
//! statuses inference-core sends come from the pinned contract it generates, read by
//! the real decoder. What the stand-ins decide is nothing: a port that records what
//! it was sent and answers as scripted, so a test can see exactly what left the
//! process and when.
//!
//! Time is virtual where a test is about time: each such test mints its bearer first
//! (real I/O), then freezes the clock, so a sleep of 1,500 ms is 1,500 ms and no
//! timer in an HTTP client can fire early while a socket is merely idle.

// A scripted port answers with the real `tonic::Status`, which is large; so is every
// closure that returns one.
#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use inference_client_rs::{InferenceTokenClient, KIND_TRAILER};

use super::*;
use crate::context::outcome::{ladder_backoff, ErrorClass, Status};
use crate::context::testing::{
    golden_status, migration_sql, require_contract, TokenGolden, TokenStub,
};
use crate::context::validate::{MAX_CHARS, MIN_CHARS};

static NEXT_DOCUMENT: AtomicU64 = AtomicU64::new(1);

const PLAIN_TENANT: &str = "embeddingEngineTenantOrgPlain";
const ZDR_TENANT: &str = "embeddingEngineTenantOrgZdr";
const PLAIN_PERSISTENT: &str = "embeddingEngineContextualRetrievalPersistent";

const DOC: &str = "Årsrapport 2025 for Fjordfisk AS.\n\nDriftsmarginen økte til 31 prosent. Røyking av laks i Tromsø ble utvidet til to nye anlegg.";
const CHUNKS: [&str; 3] = [
    "Driftsmarginen økte til 31 prosent.",
    "Røyking av laks i Tromsø ble utvidet",
    "til to nye anlegg.",
];
const GOOD: &str = "Fjordfisk AS' årsrapport for 2025, om driftsmarginen på 31 prosent.";

fn minimal_config() -> Config {
    envy::from_iter::<_, Config>(vec![
        (
            "DATABASE_URL".to_string(),
            "postgres://test.invalid/test".to_string(),
        ),
        (
            "NATS_URL".to_string(),
            "nats://test.invalid:4222".to_string(),
        ),
        (
            "QDRANT_URL".to_string(),
            "http://test.invalid:6334".to_string(),
        ),
    ])
    .expect("minimal config")
}

fn enabled_config() -> Config {
    Config {
        contextual_retrieval_enabled: true,
        contextual_retrieval_model: "gpt-4o-mini".to_string(),
        // A real credential is required outside standalone startup; the value is
        // never used because nothing here reaches a real network.
        model_plane_inference_service_api_key: "test-key-0123456789".to_string(),
        ..minimal_config()
    }
}

fn answer(content: &str, stop_reason: &str) -> model_plane::v1::InferResponse {
    model_plane::v1::InferResponse {
        request_id: String::new(),
        content: content.to_string(),
        model_used: "gpt-4o-mini-2024-07-18".to_string(),
        stop_reason: stop_reason.to_string(),
        input_tokens: 612,
        output_tokens: 48,
        provider_used: "azure-openai".to_string(),
        residency: "eu".to_string(),
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: Some(model_plane::v1::ProviderTiming {
            usage_reported: true,
            response_cache_hit: false,
            attempts: 1,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[derive(Clone)]
struct Recorded {
    request: model_plane::v1::InferRequest,
    authorization: String,
    at: Instant,
}

type Responder = Box<
    dyn Fn(usize, &Recorded) -> Result<model_plane::v1::InferResponse, tonic::Status> + Send + Sync,
>;

/// A port that records every request it is sent and answers as scripted.
struct Scripted {
    calls: Mutex<Vec<Recorded>>,
    responder: Responder,
    delay_ms: AtomicU64,
}

impl Scripted {
    fn new(responder: Responder) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            responder,
            delay_ms: AtomicU64::new(0),
        })
    }

    fn call_count(&self) -> usize {
        self.recorded().len()
    }

    fn recorded(&self) -> Vec<Recorded> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set_delay(&self, delay: Duration) {
        self.delay_ms.store(
            u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
}

impl InferencePort for Scripted {
    fn infer(&self, request: tonic::Request<model_plane::v1::InferRequest>) -> InferFuture<'_> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let recorded = Recorded {
            request: request.get_ref().clone(),
            authorization,
            at: Instant::now(),
        };
        let index = {
            let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
            calls.push(recorded.clone());
            calls.len() - 1
        };
        Box::pin(async move {
            let delay = Duration::from_millis(self.delay_ms.load(Ordering::Relaxed));
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
            let mut response = (self.responder)(index, &recorded)?;
            if response.request_id.is_empty() {
                response.request_id = recorded.request.request_id.clone();
            }
            Ok(response)
        })
    }
}

/// An engine wired to the real token client over the captured answers of Auth Core.
struct Harness {
    engine: Contextualizer,
    port: Arc<Scripted>,
    stub: TokenStub,
    token_client: InferenceTokenClient,
    org_id: String,
    token: String,
    /// Unique to this harness, so a test can find its own lines in the shared log.
    /// Fixed width: a test finds its lines by substring, and `doc-1` is a substring of
    /// `doc-10` to `doc-19`, so an unpadded id made a log test count other tests' lines
    /// whenever it drew a single-digit number (about 3% of runs under load).
    document_id: String,
}

impl Harness {
    async fn new(case_name: &str, responder: Responder) -> Self {
        Self::with(case_name, |_| {}, responder).await
    }

    async fn with(
        case_name: &str,
        tweak: impl FnOnce(&mut Settings),
        responder: Responder,
    ) -> Self {
        let golden = TokenGolden::load();
        let case = golden.case(case_name);
        let stub = TokenStub::replaying(vec![case.clone()]).await;
        let token_client = stub.client(&golden, case.posture());
        let port = Scripted::new(responder);
        let mut settings = Settings::from_config(&enabled_config()).expect("valid settings");
        tweak(&mut settings);
        let engine = Contextualizer::new(port.clone(), token_client.clone(), settings);
        let token = case.body["token"].as_str().unwrap_or_default().to_owned();
        Self {
            engine,
            port,
            stub,
            token_client,
            org_id: case.org_id,
            token,
            document_id: format!("doc-{:06}", NEXT_DOCUMENT.fetch_add(1, Ordering::Relaxed)),
        }
    }

    /// Mint once through the real client (real I/O), then freeze virtual time.
    async fn freeze_time(&self) {
        self.token_client
            .bearer(&self.org_id, Reason::ContextualRetrieval)
            .await
            .expect("the golden answers this organization with a token");
        tokio::time::pause();
    }

    fn document<'a>(&'a self, text: &'a str, total_chunks: usize) -> DocumentInput<'a> {
        DocumentInput {
            org_id: &self.org_id,
            document_id: &self.document_id,
            meta: DocumentMeta {
                title: "Årsrapport 2025",
                source: "",
                date: "",
            },
            text,
            restricted: false,
            prompt_version: None,
            total_chunks: Some(total_chunks),
        }
    }

    async fn run(
        &self,
        text: &str,
        chunks: &[&str],
        deadline: Option<Instant>,
    ) -> Vec<ChunkResult> {
        let prepared = self.engine.prepare(self.document(text, chunks.len()));
        let inputs = chunk_inputs(chunks);
        self.engine
            .contextualize(&prepared, &inputs, deadline)
            .await
    }
}

fn chunk_inputs<'a>(chunks: &[&'a str]) -> Vec<ChunkInput<'a>> {
    chunks
        .iter()
        .enumerate()
        .map(|(index, text)| ChunkInput {
            index: i32::try_from(index).expect("small"),
            text,
        })
        .collect()
}

fn always(response: model_plane::v1::InferResponse) -> Responder {
    Box::new(move |_, _| Ok(response.clone()))
}

fn failing(status: impl Fn() -> tonic::Status + Send + Sync + 'static) -> Responder {
    Box::new(move |_, _| Err(status()))
}

/// Waited for what was asked, within the one millisecond a timer rounds to.
fn assert_waited(actual: Duration, expected: Duration) {
    assert!(
        actual >= expected && actual < expected + Duration::from_millis(2),
        "waited {actual:?}, expected {expected:?}"
    );
}

// ── ZDR ──────────────────────────────────────────────────────────────────────

/// The counting double the design asks for: a bearer that says the organization is
/// zero-retention makes NO call to inference-core, whatever the document.
#[tokio::test]
async fn a_zdr_organization_makes_zero_provider_calls() {
    let h = Harness::new(ZDR_TENANT, always(answer(GOOD, "stop"))).await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), 0, "not one request left the process");
    for result in &results {
        assert_eq!(result.outcome, ContextOutcome::skipped_zdr(ZdrReason::Org));
        assert_eq!(result.outcome.status(), Status::SkippedZdr);
        assert_eq!(result.outcome.error_class(), Some(ErrorClass::ZdrOrg));
        assert!(result.calls.is_empty());
    }
    // One mint for the whole document, and it asked for this feature's reason.
    assert_eq!(h.stub.request_count(), 1);
    assert_eq!(h.stub.requests()[0]["reason"], "contextual-retrieval");
}

/// The same organization, not zero-retention: the call is made and says so.
#[tokio::test]
async fn an_organization_that_is_not_zero_retention_is_sent_with_the_flag_off() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), CHUNKS.len());
    assert!(results.iter().all(|r| r.outcome.generated().is_some()));
    assert!(h.port.recorded().iter().all(|call| !call.request.zdr));
}

/// The posture the stack runs under today (the registry still says `persistent`):
/// the token carries `zdr: false` for every organization, and the call is made.
#[tokio::test]
async fn the_persistent_posture_of_the_running_stack_works_end_to_end() {
    let h = Harness::new(PLAIN_PERSISTENT, always(answer(GOOD, "stop"))).await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert!(results.iter().all(|r| r.outcome.generated().is_some()));
    assert!(h.port.recorded().iter().all(|call| !call.request.zdr));
}

/// The setting changes while a document is being worked on. A bearer is taken for each
/// provider call, so the call after the change carries the new `zdr` claim, and that
/// check (not the one made when the document started) is what stops it: not one more
/// request leaves the process, and the rest of the document is not asked.
///
/// Auth Core answers the first mint with the captured plain-tenant token. Its answer
/// after the flip is the same token with its `zdr` claim set and its times moved: the
/// client reads claims and does not verify signatures (inference-core does), and every
/// other rule it applies (organization, reason, lifetime, posture) still applies.
#[tokio::test]
async fn an_organization_that_turns_zero_retention_part_way_stops_the_remaining_calls() {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use chrono::{SecondsFormat, TimeZone, Utc};
    use inference_client_rs::{Clock as _, RetentionPosture};

    use crate::context::testing::ManualClock;

    let golden = TokenGolden::load();
    let case = golden.case(PLAIN_TENANT);
    let clock = ManualClock::at(golden.frozen_now() + 1);
    let mints = Arc::new(AtomicU64::new(0));
    let stub = {
        let (case, clock, mints) = (case.clone(), clock.clone(), mints.clone());
        TokenStub::scripted(move |_, _| {
            if mints.fetch_add(1, Ordering::SeqCst) == 0 {
                return (case.status, serde_json::to_vec(&case.body).expect("encode"));
            }
            let now = clock.now().timestamp();
            let token = case.body["token"].as_str().expect("token");
            let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
            let mut claims: serde_json::Value =
                serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&parts[1]).expect("payload"))
                    .expect("claims");
            claims["zdr"] = true.into();
            claims["iat"] = now.into();
            claims["nbf"] = (now - 5).into();
            claims["exp"] = (now + 60).into();
            parts[1] = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("encode"));
            let expires_at = Utc
                .timestamp_opt(now + 60, 0)
                .single()
                .expect("time")
                .to_rfc3339_opts(SecondsFormat::Millis, true);
            let bundle = serde_json::json!({
                "token": parts.join("."),
                "expiresAt": expires_at,
                "expiresInSeconds": 60,
                "issuer": case.body["issuer"],
                "audience": case.body["audience"],
            });
            (case.status, serde_json::to_vec(&bundle).expect("encode"))
        })
        .await
    };
    let client = stub.client_on(&golden, RetentionPosture::Tenant, clock.clone());
    // The first provider call answers, and a minute and more pass while it does.
    let port = {
        let clock = clock.clone();
        Scripted::new(Box::new(move |index, _| {
            if index == 0 {
                clock.advance(120);
            }
            Ok(answer(GOOD, "stop"))
        }))
    };
    let mut settings = Settings::from_config(&enabled_config()).expect("valid settings");
    settings.concurrency = 1;
    let engine = Contextualizer::new(port.clone(), client, settings);
    let prepared = engine.prepare(DocumentInput {
        org_id: &case.org_id,
        document_id: "doc-flip",
        meta: DocumentMeta::default(),
        text: DOC,
        restricted: false,
        prompt_version: None,
        total_chunks: Some(CHUNKS.len()),
    });

    let results = engine
        .contextualize(&prepared, &chunk_inputs(&CHUNKS), None)
        .await;

    assert_eq!(
        mints.load(Ordering::SeqCst),
        2,
        "the second bearer is a new mint"
    );
    assert_eq!(port.call_count(), 1, "nothing is sent after the flip");
    assert!(results[0].outcome.generated().is_some());
    for result in &results[1..] {
        assert_eq!(result.outcome, ContextOutcome::skipped_zdr(ZdrReason::Org));
        assert!(result.calls.is_empty());
    }
}

/// A restricted document is refused before anything of it is read, windowed or sent:
/// no mint, no call.
#[tokio::test]
async fn a_restricted_document_is_refused_before_anything_is_read() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let prepared = h.engine.prepare(DocumentInput {
        restricted: true,
        ..h.document(DOC, 3)
    });
    let results = h
        .engine
        .contextualize(&prepared, &chunk_inputs(&CHUNKS), None)
        .await;
    assert_eq!(h.port.call_count(), 0);
    assert_eq!(h.stub.request_count(), 0, "not even a bearer was asked for");
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::skipped_zdr(ZdrReason::Document)
        );
        assert_eq!(result.outcome.error_class(), Some(ErrorClass::ZdrDocument));
    }
}

/// A persistent deployment meets a zero-retention organization's token: the client
/// refuses it (the safe direction), and nothing is sent.
#[tokio::test]
async fn a_deployment_that_does_not_expect_zdr_refuses_a_zdr_token_and_sends_nothing() {
    let golden = TokenGolden::load();
    let case = golden.case(ZDR_TENANT);
    let stub = TokenStub::replaying(vec![case.clone()]).await;
    let client = stub.client(&golden, inference_client_rs::RetentionPosture::Persistent);
    let port = Scripted::new(always(answer(GOOD, "stop")));
    let engine = Contextualizer::new(
        port.clone(),
        client,
        Settings::from_config(&enabled_config()).unwrap(),
    );
    let prepared = engine.prepare(DocumentInput {
        org_id: &case.org_id,
        document_id: "doc-1",
        meta: DocumentMeta::default(),
        text: DOC,
        restricted: false,
        prompt_version: None,
        total_chunks: Some(3),
    });
    let results = engine
        .contextualize(&prepared, &chunk_inputs(&CHUNKS), None)
        .await;
    assert_eq!(port.call_count(), 0);
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::transient(ErrorClass::Auth, Some(Duration::from_secs(300)))
        );
    }
}

// ── the bearer ───────────────────────────────────────────────────────────────

/// Auth Core could not resolve the posture: its real 503 body, through the real
/// client. One request for the whole document, no inference call, and a wait.
#[tokio::test]
async fn an_unresolved_posture_is_a_wait_after_one_request_never_a_send_or_a_skip() {
    let h = Harness::with(
        "tenantPostureUnavailable",
        |_| {},
        always(answer(GOOD, "stop")),
    )
    .await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), 0);
    assert_eq!(h.stub.request_count(), 1, "one request for three chunks");
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::pending(PendingReason::PostureUnavailable, Duration::from_secs(300))
        );
        assert_eq!(result.outcome.status(), Status::Pending);
        assert!(!result.outcome.counts_as_attempt());
    }
}

#[tokio::test]
async fn an_organization_directory_that_cannot_answer_is_the_same_wait() {
    let h = Harness::with(
        "tenantDirectoryUnavailable",
        |_| {},
        always(answer(GOOD, "stop")),
    )
    .await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), 0);
    assert!(results
        .iter()
        .all(|r| r.outcome.status() == Status::Pending));
}

#[tokio::test]
async fn an_organization_auth_core_does_not_know_is_a_deployment_problem_not_the_units() {
    let h = Harness::with(
        "organizationNotInDirectory",
        |_| {},
        always(answer(GOOD, "stop")),
    )
    .await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), 0);
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::transient(ErrorClass::Auth, Some(Duration::from_secs(300)))
        );
        assert!(!result.outcome.counts_as_attempt());
    }
}

/// The bearer is taken from the client's cache at each call and never held across the
/// document: one mint, and a lookup per chunk.
#[tokio::test]
async fn a_bearer_is_fetched_for_every_provider_call_and_minted_once() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let before = h.token_client.stats();
    let results = h.run(DOC, &CHUNKS, None).await;
    assert!(results.iter().all(|r| r.outcome.generated().is_some()));
    let stats = h.token_client.stats();
    assert_eq!(stats.mints - before.mints, 1, "one request to Auth Core");
    // Once up front and once per chunk's call.
    let lookups =
        (stats.hits + stats.misses + stats.joined) - (before.hits + before.misses + before.joined);
    assert!(lookups > CHUNKS.len() as u64, "{stats:?}");
    // And every request carried the real captured token as its bearer.
    for call in h.port.recorded() {
        assert_eq!(call.authorization, format!("Bearer {}", h.token));
    }
}

// ── what is sent ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_request_carries_the_floors_the_prompt_and_nothing_else() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    h.run(DOC, &CHUNKS[..1], None).await;
    let calls = h.port.recorded();
    assert_eq!(calls.len(), 1);
    let request = &calls[0].request;
    assert_eq!(request.org_id, h.org_id);
    assert_eq!(request.model, "gpt-4o-mini");
    // Residency and provider floors are set, not left to the router.
    assert_eq!(request.provider_hint, "azure-openai");
    assert_eq!(request.min_residency, "eu");
    assert!(!request.zdr);
    assert_eq!(request.temperature, 0.0);
    assert_eq!(request.max_tokens, 128);
    assert_eq!(request.thinking_budget_tokens, 0);
    assert_eq!(
        request.min_privacy_tier,
        model_plane::v1::PrivacyTier::Unspecified as i32
    );
    assert!(request.tools.is_empty() && request.tool_choice.is_empty());
    assert!(request.subscription_connection_id.is_empty());
    assert!(!request.prefer_priority_service_tier);
    assert!(request.structured_output_schema.is_empty());
    assert!(!request.request_id.is_empty());
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, "user");
    // ctx-v1 over the whole document, exactly as before.
    assert_eq!(
        request.messages[0].content,
        crate::context::prompt::build_prompt(
            PromptVersion::V1,
            &DocumentMeta::default(),
            DOC,
            CHUNKS[0]
        )
    );
}

#[tokio::test]
async fn a_pinned_ctx_v2_prompt_carries_the_title_and_comes_before_the_chunk() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let prepared = h.engine.prepare(DocumentInput {
        prompt_version: Some(PromptVersion::V2),
        ..h.document(DOC, 3)
    });
    let results = h
        .engine
        .contextualize(&prepared, &chunk_inputs(&CHUNKS[..1]), None)
        .await;
    let generated = results[0].outcome.generated().expect("ready");
    assert_eq!(generated.prompt_version, PromptVersion::V2);
    let sent = &h.port.recorded()[0].request.messages[0].content;
    assert!(
        sent.starts_with("<document title=\"Årsrapport 2025\">\n"),
        "{sent}"
    );
}

#[tokio::test]
async fn every_request_has_its_own_id() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    h.run(DOC, &CHUNKS, None).await;
    let ids: BTreeSet<String> = h
        .port
        .recorded()
        .into_iter()
        .map(|call| call.request.request_id)
        .collect();
    assert_eq!(ids.len(), CHUNKS.len());
}

// ── what comes back ──────────────────────────────────────────────────────────

#[tokio::test]
async fn an_answer_that_finished_is_kept_with_what_served_it_and_what_it_cost() {
    let mut response = answer(GOOD, "stop");
    response.cache_read_input_tokens = 512;
    let h = Harness::new(PLAIN_TENANT, always(response)).await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    let generated = results[0].outcome.generated().expect("ready");
    assert_eq!(generated.text, GOOD);
    assert_eq!(generated.model_used, "gpt-4o-mini-2024-07-18");
    assert_eq!(generated.provider_used, "azure-openai");
    assert_eq!(generated.residency, "eu");
    assert_eq!(generated.prompt_version, PromptVersion::V1);
    assert_eq!(
        generated.request_id,
        h.port.recorded()[0].request.request_id
    );
    assert_eq!(generated.usage.input_tokens, 612);
    assert_eq!(generated.usage.output_tokens, 48);
    assert_eq!(generated.usage.cache_read_input_tokens, 512);
    assert!(generated.usage.is_billable());
    // The hash is what a later pass compares to decide this is still the answer.
    let prepared = h.engine.prepare(h.document(DOC, 3));
    assert_eq!(
        generated.input_hash,
        h.engine.input_hash(&prepared, &chunk_inputs(&CHUNKS)[0])
    );
    assert_eq!(results[0].calls.len(), 1);
    assert_eq!(results[0].calls[0].usage, generated.usage);
}

/// The trap: an empty answer was `Ok(None)` and nothing said so.
#[tokio::test]
async fn an_empty_answer_is_an_explicit_invalid_output_and_still_metered() {
    let h = Harness::new(PLAIN_TENANT, always(answer("   \n", "stop"))).await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::InvalidOutput, None)
    );
    assert_eq!(
        results[0].outcome.error_class(),
        Some(ErrorClass::InvalidOutput)
    );
    // The provider billed for it all the same.
    assert_eq!(results[0].calls.len(), 1);
    assert!(results[0].calls[0].usage.is_billable());
    // And nothing retried it inline: a deterministic bad answer is not paid for twice.
    assert_eq!(h.port.call_count(), 1);
}

#[tokio::test]
async fn a_cut_off_answer_is_shortened_a_content_filter_is_final_and_neither_is_retried() {
    let h = Harness::new(
        PLAIN_TENANT,
        always(answer(
            "Fjordfisk AS rapporterte om driftsmarginen i 2025. Neste setning ble avbrutt midt i et ord som kj",
            "length",
        )),
    )
    .await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    let generated = results[0].outcome.generated().expect("ready");
    assert_eq!(
        generated.text,
        "Fjordfisk AS rapporterte om driftsmarginen i 2025."
    );
    assert!(generated.cut_at_sentence);
    assert_eq!(h.port.call_count(), 1);

    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "content_filter"))).await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(
        results[0].outcome,
        ContextOutcome::permanent(ErrorClass::ContentFilter)
    );
    assert_eq!(h.port.call_count(), 1, "never retried");
    assert_eq!(results[0].calls.len(), 1, "but paid for, and recorded");
}

/// A response inference-core answered from its own cache cost nothing and says so.
#[tokio::test]
async fn a_response_cache_hit_is_captured_as_not_billable() {
    let mut response = answer(GOOD, "stop");
    response.provider_timing = Some(model_plane::v1::ProviderTiming {
        usage_reported: true,
        response_cache_hit: true,
        ..Default::default()
    });
    let h = Harness::new(PLAIN_TENANT, always(response)).await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    let usage = results[0].outcome.generated().expect("ready").usage;
    assert!(usage.response_cache_hit);
    assert!(!usage.is_billable());
}

#[tokio::test]
async fn a_server_that_predates_the_timing_field_is_read_by_its_token_counts() {
    let mut with_counts = answer(GOOD, "stop");
    with_counts.provider_timing = None;
    assert!(usage_of(&with_counts).usage_reported);
    let mut without = answer(GOOD, "stop");
    without.provider_timing = None;
    without.input_tokens = 0;
    without.output_tokens = 0;
    assert!(
        !usage_of(&without).usage_reported,
        "no counts and no flag mean unknown"
    );
    // A negative count from a misbehaving server is clamped, not wrapped.
    let mut hostile = answer(GOOD, "stop");
    hostile.input_tokens = -5;
    assert_eq!(usage_of(&hostile).input_tokens, 0);
}

// ── retries ──────────────────────────────────────────────────────────────────

/// The retry test the design asks for: the delay IS the retry-after trailer. The
/// status is the one inference-core's contract says a throttle carries (1,500 ms).
#[tokio::test]
async fn a_throttled_call_is_retried_after_exactly_what_the_provider_asked() {
    let h = Harness::new(
        PLAIN_TENANT,
        Box::new(|index, _| {
            if index == 0 {
                Err(golden_status("provider_rate_limited"))
            } else {
                Ok(answer(GOOD, "stop"))
            }
        }),
    )
    .await;
    h.freeze_time().await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert!(results[0].outcome.generated().is_some());
    let calls = h.port.recorded();
    assert_eq!(calls.len(), 2);
    assert_waited(calls[1].at - calls[0].at, Duration::from_millis(1_500));
}

#[tokio::test]
async fn without_a_provider_wish_the_wait_is_the_ladder_step() {
    let h = Harness::new(
        PLAIN_TENANT,
        Box::new(|index, _| {
            if index < 2 {
                Err(tonic::Status::resource_exhausted("slow down"))
            } else {
                Ok(answer(GOOD, "stop"))
            }
        }),
    )
    .await;
    h.freeze_time().await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert!(results[0].outcome.generated().is_some());
    let calls = h.port.recorded();
    assert_eq!(calls.len(), 3);
    let base = Duration::from_millis(2_000);
    assert_waited(calls[1].at - calls[0].at, ladder_backoff(base, 0, 0));
    assert_waited(calls[2].at - calls[1].at, ladder_backoff(base, 1, 0));
}

#[tokio::test]
async fn retries_are_bounded_and_the_last_failure_is_the_outcome() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.retry_attempts = 2,
        failing(|| golden_status("provider_rate_limited")),
    )
    .await;
    h.freeze_time().await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(h.port.call_count(), 3, "the call and two retries");
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::RateLimited, Some(Duration::from_millis(1_500)))
    );
    // The provider's wait travels with the outcome, for the reconciler's schedule.
    assert_eq!(
        results[0].outcome.retry_after(),
        Some(Duration::from_millis(1_500))
    );
}

/// A wait that would run past the deadline is not slept: the outcome carries it.
#[tokio::test]
async fn a_wait_that_does_not_fit_the_deadline_is_not_slept() {
    let h = Harness::new(
        PLAIN_TENANT,
        failing(|| {
            let mut metadata = tonic::metadata::MetadataMap::new();
            metadata.insert(
                KIND_TRAILER,
                tonic::metadata::MetadataValue::from_static("rate_limited"),
            );
            metadata.insert(
                inference_client_rs::RETRY_AFTER_MS_TRAILER,
                tonic::metadata::MetadataValue::from_static("5000"),
            );
            tonic::Status::with_metadata(tonic::Code::ResourceExhausted, "", metadata)
        }),
    )
    .await;
    h.freeze_time().await;
    let start = Instant::now();
    let results = h
        .run(DOC, &CHUNKS[..1], Some(start + Duration::from_secs(1)))
        .await;
    assert_eq!(h.port.call_count(), 1);
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::RateLimited, Some(Duration::from_secs(5)))
    );
    assert!(
        Instant::now() - start < Duration::from_secs(1),
        "no sleep past the deadline"
    );
}

/// A refusal is the answer: asked once, never retried, and the rest of the document
/// is not asked at all.
#[tokio::test]
async fn a_residency_refusal_is_one_call_for_the_whole_document_and_loud() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.concurrency = 1,
        failing(|| golden_status("provider_residency_violation")),
    )
    .await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(
        h.port.call_count(),
        1,
        "four more refusals would only cost round trips"
    );
    for result in results {
        assert_eq!(result.outcome, ContextOutcome::ResidencyUnavailable);
        assert_eq!(result.outcome.status(), Status::FailedTransient);
        assert_eq!(
            result.outcome.error_class(),
            Some(ErrorClass::ResidencyUnavailable)
        );
    }
}

#[tokio::test]
async fn a_zdr_refusal_from_the_route_is_a_skip_not_a_retry() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.concurrency = 1,
        failing(|| golden_status("provider_zdr_unavailable")),
    )
    .await;
    let results = h.run(DOC, &CHUNKS, None).await;
    assert_eq!(h.port.call_count(), 1);
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::skipped_zdr(ZdrReason::NoAttestedRoute)
        );
        assert_eq!(
            result.outcome.error_class(),
            Some(ErrorClass::ZdrUnavailable)
        );
    }
}

#[tokio::test]
async fn a_server_error_inside_inference_core_is_retried_and_then_reported() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.retry_attempts = 1,
        failing(|| golden_status("embedding_provider_http")),
    )
    .await;
    h.freeze_time().await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(h.port.call_count(), 2);
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::Internal, None)
    );
}

// ── the deadline ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_deadline_that_has_passed_makes_no_call_and_mints_nothing() {
    // No bearer is minted beforehand: if one were asked for, the stand-in would see it.
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let results = h.run(DOC, &CHUNKS, Some(Instant::now())).await;
    assert_eq!(h.port.call_count(), 0);
    assert_eq!(h.stub.request_count(), 0, "not even a bearer was asked for");
    for result in results {
        assert_eq!(
            result.outcome,
            ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO)
        );
        assert_eq!(result.outcome.status(), Status::Pending);
    }
}

/// One document in a batch is cut off at its deadline while another, with its own,
/// completes: the unit left unfinished is `pending` and due at once, and nothing is
/// started after the deadline.
#[tokio::test]
async fn a_chunk_that_misses_the_deadline_is_pending_and_nothing_starts_after_it() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.concurrency = 1,
        always(answer(GOOD, "stop")),
    )
    .await;
    h.port.set_delay(Duration::from_secs(3));
    h.freeze_time().await;
    let start = Instant::now();
    let results = h
        .run(
            DOC,
            &[CHUNKS[0], CHUNKS[1], CHUNKS[2], "fjerde"],
            Some(start + Duration::from_secs(5)),
        )
        .await;
    let statuses: Vec<Status> = results.iter().map(|r| r.outcome.status()).collect();
    assert_eq!(
        statuses,
        [
            Status::Ready,
            Status::Pending,
            Status::Pending,
            Status::Pending
        ]
    );
    assert_eq!(
        h.port.call_count(),
        2,
        "the second was cut off; the rest never started"
    );
    assert!(Instant::now() - start <= Duration::from_secs(5) + Duration::from_millis(2));
    assert_eq!(
        results[1].outcome,
        ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO)
    );
}

/// A call is never allowed to outlive the time that is left, however long its own
/// timeout is.
#[tokio::test]
async fn a_call_is_bounded_by_the_time_left_not_by_its_own_timeout() {
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    h.port.set_delay(Duration::from_secs(60));
    h.freeze_time().await;
    let start = Instant::now();
    let results = h
        .run(DOC, &CHUNKS[..1], Some(start + Duration::from_secs(8)))
        .await;
    assert_eq!(
        results[0].outcome,
        ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO)
    );
    assert!(Instant::now() - start <= Duration::from_secs(8) + Duration::from_millis(2));
    let timeout_header = h.port.recorded()[0].request.org_id.clone();
    assert!(!timeout_header.is_empty());
}

/// Without a deadline a slow provider is still bounded by the call's own timeout, and
/// that is a retryable timeout, not a deadline.
#[tokio::test]
async fn without_a_deadline_the_per_call_timeout_still_applies() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| {
            settings.timeout = Duration::from_secs(2);
            settings.retry_attempts = 0;
        },
        always(answer(GOOD, "stop")),
    )
    .await;
    h.port.set_delay(Duration::from_secs(60));
    h.freeze_time().await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::Timeout, None)
    );
}

// ── order and windows ────────────────────────────────────────────────────────

fn long_document() -> String {
    "Driftsmarginen økte til 31 prosent og røyking av laks i Tromsø ble utvidet. ".repeat(100)
}

/// A window long enough to be cached: the first chunk goes alone and the rest follow
/// together, so the prefix is warm for them.
#[tokio::test]
async fn the_first_chunk_of_a_cacheable_window_runs_alone_then_the_rest_together() {
    let document = long_document();
    assert!(document.len() >= PREFIX_CACHE_MIN_BYTES);
    let chunks = [
        &document[..60],
        &document[100..160],
        &document[200..260],
        &document[300..360],
    ];
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    h.port.set_delay(Duration::from_secs(1));
    h.freeze_time().await;
    h.run(&document, &chunks, None).await;
    let calls = h.port.recorded();
    assert_eq!(calls.len(), 4);
    assert_waited(calls[1].at - calls[0].at, Duration::from_secs(1));
    assert_eq!(calls[1].at, calls[2].at);
    assert_eq!(calls[2].at, calls[3].at);
    // They share the front of the prompt, which is what a prefix cache needs.
    let first = &calls[0].request.messages[0].content;
    let second = &calls[1].request.messages[0].content;
    assert!(first.starts_with("Here is a document:\n\n<document>\n"));
    let shared = first
        .bytes()
        .zip(second.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    assert!(
        shared >= document.len(),
        "the whole document precedes the chunk"
    );
}

/// A short document has no prefix to warm, and waiting for the first chunk would only
/// add its latency.
#[tokio::test]
async fn a_window_too_short_to_cache_starts_every_chunk_together() {
    assert!(DOC.len() < PREFIX_CACHE_MIN_BYTES);
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    h.port.set_delay(Duration::from_secs(1));
    h.freeze_time().await;
    h.run(DOC, &CHUNKS, None).await;
    let calls = h.port.recorded();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0].at, calls[1].at);
    assert_eq!(calls[1].at, calls[2].at);
}

#[tokio::test]
async fn the_concurrency_setting_bounds_how_many_calls_are_in_flight() {
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.concurrency = 2,
        always(answer(GOOD, "stop")),
    )
    .await;
    h.port.set_delay(Duration::from_secs(1));
    h.freeze_time().await;
    let chunks: Vec<&str> = vec![
        CHUNKS[0],
        CHUNKS[1],
        CHUNKS[2],
        "fjerde bit",
        "femte bit",
        "sjette bit",
    ];
    h.run(DOC, &chunks, None).await;
    let mut starts: Vec<Instant> = h.port.recorded().iter().map(|c| c.at).collect();
    starts.sort();
    // Six calls, two at a time: three distinct start times, a second apart.
    let distinct: BTreeSet<Instant> = starts.iter().copied().collect();
    assert_eq!(distinct.len(), 3, "{starts:?}");
}

/// A long document is read in sections, each chunk in the one it sits inside.
#[tokio::test]
async fn a_long_document_is_read_in_sections_and_each_chunk_inside_its_own() {
    let mut text = String::new();
    for index in 0..800 {
        text.push_str(&format!(
            "Avsnitt {index}: Røyking av laks i Tromsø koster 5 € per kilo, tallene for kvartal {index} er endelige.\n"
        ));
    }
    assert!(text.chars().count() > 70_000);
    let early = "Avsnitt 5: Røyking av laks i Tromsø koster 5 € per kilo, tallene for kvartal 5 er endelige.";
    let late = "Avsnitt 790: Røyking av laks i Tromsø koster 5 € per kilo, tallene for kvartal 790 er endelige.";
    assert!(text.contains(early) && text.contains(late));
    let h = Harness::new(PLAIN_TENANT, always(answer(GOOD, "stop"))).await;
    let results = h.run(&text, &[early, late], None).await;
    assert!(results.iter().all(|r| r.outcome.generated().is_some()));
    let calls = h.port.recorded();
    let prompt_of = |chunk: &str| {
        calls
            .iter()
            .map(|call| &call.request.messages[0].content)
            .find(|prompt| prompt.contains(&format!("<chunk>\n{chunk}\n</chunk>")))
            .unwrap_or_else(|| panic!("no request carried {chunk}"))
            .clone()
    };
    let (early_prompt, late_prompt) = (prompt_of(early), prompt_of(late));
    assert!(early_prompt.contains(early) && late_prompt.contains(late));
    assert_ne!(
        early_prompt.split("</document>").next(),
        late_prompt.split("</document>").next(),
        "different sections of the document"
    );
    // Never the whole megabyte: a window, a header and markers.
    assert!(early_prompt.chars().count() < 40_000 && late_prompt.chars().count() < 40_000);
    // The late chunk's window opens with the document's header.
    assert!(late_prompt.contains("Avsnitt 0: Røyking"));
    assert!(late_prompt.contains("[...]"));
}

// ── too long ─────────────────────────────────────────────────────────────────

fn big_document() -> String {
    "Driftsmarginen økte til 31 prosent og røyking av laks i Tromsø ble utvidet. ".repeat(900)
}

/// The provider says the input is too long: one smaller window, once. The hash stays
/// the one of the planned window, so a document that always needs this is not paid
/// for again every pass.
#[tokio::test]
async fn an_input_that_is_too_long_is_retried_once_with_a_smaller_window() {
    let document = big_document();
    assert!(document.chars().count() > 50_000);
    let chunk = document[..60].to_string();
    let h = Harness::new(
        PLAIN_TENANT,
        Box::new(|index, _| {
            if index == 0 {
                Err(golden_status("provider_too_long"))
            } else {
                Ok(answer(GOOD, "stop"))
            }
        }),
    )
    .await;
    let results = h.run(&document, &[chunk.as_str()], None).await;
    let calls = h.port.recorded();
    assert_eq!(
        calls.len(),
        2,
        "no wait, no backoff: a smaller window, once"
    );
    let (first, second) = (
        &calls[0].request.messages[0].content,
        &calls[1].request.messages[0].content,
    );
    assert!(
        second.len() < first.len() * 3 / 4,
        "{} !< {}",
        second.len(),
        first.len()
    );
    let generated = results[0].outcome.generated().expect("ready");
    let prepared = h.engine.prepare(h.document(&document, 1));
    assert_eq!(
        generated.input_hash,
        h.engine.input_hash(
            &prepared,
            &ChunkInput {
                index: 0,
                text: &chunk
            }
        ),
        "the planned inputs, not the fallback's"
    );
}

#[tokio::test]
async fn an_input_that_is_still_too_long_after_the_shrink_is_permanent() {
    let document = big_document();
    let chunk = document[..60].to_string();
    let h = Harness::new(PLAIN_TENANT, failing(|| golden_status("provider_too_long"))).await;
    let results = h.run(&document, &[chunk.as_str()], None).await;
    assert_eq!(h.port.call_count(), 2);
    assert_eq!(
        results[0].outcome,
        ContextOutcome::permanent(ErrorClass::TooLong)
    );
}

#[tokio::test]
async fn a_document_too_small_to_shrink_is_permanent_after_one_call() {
    let h = Harness::new(PLAIN_TENANT, failing(|| golden_status("provider_too_long"))).await;
    let results = h.run(DOC, &CHUNKS[..1], None).await;
    assert_eq!(h.port.call_count(), 1);
    assert_eq!(
        results[0].outcome,
        ContextOutcome::permanent(ErrorClass::TooLong)
    );
}

// ── logs ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl LogBuffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

/// The process-wide log of these tests. One global subscriber, installed once: a
/// thread-local one per test races with the callsite cache when tests run in parallel
/// and loses events. A test finds its own lines by an identifier only it uses.
fn logs() -> &'static LogBuffer {
    static BUFFER: std::sync::OnceLock<LogBuffer> = std::sync::OnceLock::new();
    BUFFER.get_or_init(|| {
        let buffer = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(buffer.clone())
            .finish();
        tracing::subscriber::set_global_default(subscriber)
            .expect("no other test installs a global subscriber");
        buffer
    })
}

const SECRET_DOCUMENT: &str = "ZXQ-SECRET-DOCUMENT-TEXT om Fjordfisk";
const SECRET_CHUNK: &str = "ZXQ-SECRET-CHUNK-TEXT";
const SECRET_CONTEXT: &str = "ZXQ-SECRET-CONTEXT-TEXT fra årsrapporten for 2025.";
const SECRET_BODY: &str = "ZXQ-SECRET-PROVIDER-BODY";

fn assert_content_free(logs: &str) {
    for secret in [
        SECRET_DOCUMENT,
        SECRET_CHUNK,
        SECRET_CONTEXT,
        SECRET_BODY,
        "Here is a document",
    ] {
        assert!(
            !logs.contains(secret),
            "a log line carried {secret}:\n{logs}"
        );
    }
}

/// Nothing a document or a provider said reaches a log: not the prompt, not the
/// context, not the body of an error (which can echo the prompt).
#[tokio::test]
async fn nothing_that_a_document_or_a_provider_said_is_ever_logged() {
    let logs = logs();
    let document = format!("{SECRET_DOCUMENT}. Mer tekst her.");
    let chunks = [SECRET_CHUNK];

    // A failure whose message echoes what was sent.
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| {
            settings.retry_attempts = 1;
            settings.retry_base = Duration::from_millis(1);
        },
        failing(|| {
            tonic::Status::unavailable(format!(
                "500: {{\"error\":\"{SECRET_BODY} {SECRET_DOCUMENT}\"}}"
            ))
        }),
    )
    .await;
    let results = h.run(&document, &chunks, None).await;
    assert_eq!(
        results[0].outcome,
        ContextOutcome::transient(ErrorClass::ProviderUnavailable, None)
    );

    // A success whose context is stored, not logged.
    let h = Harness::new(PLAIN_TENANT, always(answer(SECRET_CONTEXT, "stop"))).await;
    let results = h.run(&document, &chunks, None).await;
    assert_eq!(
        results[0].outcome.generated().expect("ready").text,
        SECRET_CONTEXT
    );

    // A refused bearer and a skip.
    let h = Harness::with(
        "tenantPostureUnavailable",
        |_| {},
        always(answer(GOOD, "stop")),
    )
    .await;
    h.run(&document, &chunks, None).await;
    let h = Harness::new(ZDR_TENANT, always(answer(GOOD, "stop"))).await;
    h.run(&document, &chunks, None).await;

    let text = logs.text();
    assert_content_free(&text);
    // What is logged is the class and the counts, which is what an operator needs.
    assert!(text.contains("provider_unavailable"), "{text}");
    assert!(text.contains("posture_unavailable"), "{text}");
    assert!(text.contains("zdr_org"), "{text}");
}

/// A throttled bulk ingest says it once per document, not once per chunk.
#[tokio::test]
async fn a_failing_document_is_one_line_not_one_per_chunk() {
    // Installed before anything is logged.
    let _ = logs();
    let h = Harness::with(
        PLAIN_TENANT,
        |settings| settings.retry_attempts = 0,
        failing(|| golden_status("provider_http")),
    )
    .await;
    h.run(DOC, &CHUNKS, None).await;
    let mine = h.document_id.clone();
    let warnings = logs()
        .text()
        .lines()
        .filter(|line| {
            line.contains(&mine) && line.contains("WARN") && line.contains("not contextualized")
        })
        .count();
    assert_eq!(warnings, 1, "three failing chunks, one line");
}

// ── the settings ─────────────────────────────────────────────────────────────

#[test]
fn the_defaults_are_the_floors_the_design_requires() {
    let settings = Settings::from_config(&enabled_config()).expect("defaults are valid");
    assert_eq!(settings.provider_hint, "azure-openai");
    assert_eq!(settings.min_residency, "eu");
    assert_eq!(settings.prompt_version, PromptVersion::V1);
    assert_eq!(settings.window.window_chars(), 32_000);
    assert_eq!(settings.max_tokens, 128);
    assert_eq!(settings.inline_budget.document, Duration::from_secs(8));
    assert_eq!(settings.inline_budget.batch, Duration::from_secs(20));
    assert_eq!(settings.document_concurrency, 2);
}

/// Compose passes every variable, empty when unset: an empty value must not remove a
/// floor. The weaker residency is spelled out.
#[test]
fn an_empty_floor_takes_the_default_and_a_weaker_one_must_be_spelled_out() {
    let blank = Config {
        contextual_retrieval_provider_hint: "  ".to_string(),
        contextual_retrieval_min_residency: String::new(),
        contextual_retrieval_prompt_version: String::new(),
        ..enabled_config()
    };
    let settings = Settings::from_config(&blank).expect("blank is the default");
    assert_eq!(settings.provider_hint, "azure-openai");
    assert_eq!(settings.min_residency, "eu");
    assert_eq!(settings.prompt_version, PromptVersion::V1);

    let explicit = Config {
        contextual_retrieval_min_residency: " Global ".to_string(),
        contextual_retrieval_provider_hint: "openai".to_string(),
        contextual_retrieval_prompt_version: "ctx-v2".to_string(),
        ..enabled_config()
    };
    let settings = Settings::from_config(&explicit).expect("explicit");
    assert_eq!(settings.min_residency, "global");
    assert_eq!(settings.provider_hint, "openai");
    assert_eq!(settings.prompt_version, PromptVersion::V2);
}

#[test]
fn a_misconfiguration_stops_the_service_at_startup_and_names_the_variable() {
    let cases: Vec<(Config, &str)> = vec![
        (
            Config {
                contextual_retrieval_model: "  ".to_string(),
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_MODEL",
        ),
        (
            Config {
                contextual_retrieval_min_residency: "europe".to_string(),
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY",
        ),
        (
            Config {
                contextual_retrieval_prompt_version: "ctx-v3".to_string(),
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_PROMPT_VERSION",
        ),
        (
            Config {
                contextual_retrieval_window_chars: 1_000,
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_WINDOW_CHARS",
        ),
        (
            Config {
                contextual_retrieval_window_chars: 60_000,
                contextual_retrieval_max_document_chars: 48_000,
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS",
        ),
        (
            Config {
                contextual_retrieval_max_tokens: 4,
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_MAX_TOKENS",
        ),
        (
            Config {
                contextual_retrieval_max_tokens: 100_000,
                ..enabled_config()
            },
            "CONTEXTUAL_RETRIEVAL_MAX_TOKENS",
        ),
        (
            Config {
                context_inline_budget_ms: 31_000,
                ..enabled_config()
            },
            "CONTEXT_INLINE_BUDGET_MS",
        ),
        (
            Config {
                context_inline_document_budget_ms: 0,
                ..enabled_config()
            },
            "CONTEXT_INLINE_DOCUMENT_BUDGET_MS",
        ),
        (
            Config {
                context_inline_document_concurrency: 0,
                ..enabled_config()
            },
            "CONTEXT_INLINE_DOCUMENT_CONCURRENCY",
        ),
        (
            Config {
                context_inline_document_concurrency: MAX_DOCUMENT_CONCURRENCY + 1,
                ..enabled_config()
            },
            "CONTEXT_INLINE_DOCUMENT_CONCURRENCY",
        ),
    ];
    for (config, variable) in cases {
        let error = Settings::from_config(&config).expect_err(variable);
        assert!(error.to_string().contains(variable), "{variable}: {error}");
    }
}

#[test]
fn every_residency_inference_core_accepts_is_accepted() {
    for token in RESIDENCY_TOKENS {
        let config = Config {
            contextual_retrieval_min_residency: token.to_string(),
            ..enabled_config()
        };
        assert!(Settings::from_config(&config).is_ok(), "{token}");
    }
}

/// The bound on documents at once has a floor of one (zero would stall every ingest) and a
/// ceiling (what one batch may ask of the provider), and both ends are valid.
#[test]
fn the_document_concurrency_accepts_its_whole_range() {
    for bound in [1, 2, 4, MAX_DOCUMENT_CONCURRENCY] {
        let config = Config {
            context_inline_document_concurrency: bound,
            ..enabled_config()
        };
        let settings = Settings::from_config(&config).expect("in range");
        assert_eq!(settings.document_concurrency, bound);
    }
}

#[test]
fn async_only_generation_is_a_valid_setting() {
    let config = Config {
        context_inline_budget_ms: 0,
        ..enabled_config()
    };
    let settings = Settings::from_config(&config).expect("budget 0 is async only");
    assert!(settings.inline_budget.batch.is_zero());
}

/// Contextual retrieval is on unless the incident kill switch is thrown (owner ruling of
/// 2026-10-03, CR-T13): a configuration that says nothing about it builds a contextualizer, with
/// the default model. `tokio::test` because building one opens a lazily-connected channel.
#[tokio::test]
async fn a_configuration_that_says_nothing_builds_a_contextualizer() {
    let cfg = Config {
        // A real credential is required outside standalone startup; never used here.
        model_plane_inference_service_api_key: "test-key-0123456789".to_string(),
        ..minimal_config()
    };
    assert!(
        cfg.contextual_retrieval_enabled,
        "contextual retrieval is on by default; the flag is the incident kill switch"
    );
    let built = Contextualizer::from_config(&cfg)
        .expect("the defaults are a startable configuration")
        .expect("contextual retrieval is on by default");
    assert_eq!(built.model_name(), "gpt-4o-mini");
}

/// While the incident kill switch is thrown, and its reason is on record, there is no
/// contextualizer and no error: the callers embed the raw text.
#[test]
fn the_kill_switch_with_a_reason_yields_no_contextualizer_rather_than_an_error() {
    let cfg = Config {
        contextual_retrieval_enabled: false,
        context_retrieval_kill_switch_reason: "provider incident 2026-10-07".to_string(),
        ..minimal_config()
    };
    let built = Contextualizer::from_config(&cfg).expect("a thrown switch is not an error");
    assert!(built.is_none());
}

/// A kill switch nobody explained is not a configuration, wherever the builder is called from
/// (the service, the reconciler, the operator's tool).
#[test]
fn the_kill_switch_without_a_reason_is_refused_by_the_builder_too() {
    let cfg = Config {
        contextual_retrieval_enabled: false,
        ..minimal_config()
    };
    let err = match Contextualizer::from_config(&cfg) {
        Ok(_) => panic!("an unexplained kill switch must be refused"),
        Err(err) => err,
    };
    assert!(
        err.to_string()
            .contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
        "unexpected error: {err}"
    );
}

/// Standalone startup relaxes the credential on purpose (the client binds and stays unusable),
/// which is not the case these tests are about.
fn standalone_startup() -> bool {
    std::env::var("APP_ENV").is_ok_and(|value| value.trim().eq_ignore_ascii_case("standalone"))
}

#[test]
fn the_boot_check_passes_a_startable_configuration() {
    check_boot(&enabled_config()).expect("the settings are valid and the credential is there");
}

/// On by default means a deployment with no credential cannot generate a context, and it must
/// say so when it starts and not after weeks of doing nothing.
#[test]
fn the_boot_check_refuses_the_feature_on_without_the_service_credential() {
    if standalone_startup() {
        return;
    }
    let error = check_boot(&minimal_config()).expect_err("no credential, feature on");
    assert!(
        error
            .to_string()
            .contains("MODEL_PLANE_INFERENCE_SERVICE_API_KEY"),
        "unexpected error: {error}"
    );
}

#[test]
fn the_boot_check_refuses_a_setting_that_is_out_of_range_and_names_it() {
    let config = Config {
        contextual_retrieval_max_tokens: 5,
        ..enabled_config()
    };
    let error = check_boot(&config).expect_err("a token cap of five");
    assert!(
        error
            .to_string()
            .contains("CONTEXTUAL_RETRIEVAL_MAX_TOKENS"),
        "unexpected error: {error}"
    );
}

#[test]
fn the_boot_check_refuses_an_unexplained_kill_switch() {
    let config = Config {
        contextual_retrieval_enabled: false,
        ..enabled_config()
    };
    let error = check_boot(&config).expect_err("a switch that is off without a reason");
    assert!(
        error
            .to_string()
            .contains("CONTEXT_RETRIEVAL_KILL_SWITCH_REASON"),
        "unexpected error: {error}"
    );
}

/// With the switch thrown nothing is asked, so a missing credential and an out-of-range setting
/// stop nothing: the incident is not made worse by a service that will not start.
#[test]
fn the_boot_check_has_nothing_to_check_while_the_kill_switch_is_thrown() {
    let config = Config {
        contextual_retrieval_enabled: false,
        context_retrieval_kill_switch_reason: "provider incident 2026-10-07".to_string(),
        contextual_retrieval_max_tokens: 5,
        ..minimal_config()
    };
    check_boot(&config).expect("nothing is asked, so nothing is checked");
}

#[test]
fn enabling_without_a_model_fails_closed() {
    let cfg = Config {
        contextual_retrieval_enabled: true,
        contextual_retrieval_model: "   ".to_string(),
        ..minimal_config()
    };
    let err = match Contextualizer::from_config(&cfg) {
        Ok(_) => panic!("an enabled contextualizer with no model must fail"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("CONTEXTUAL_RETRIEVAL_MODEL"),
        "unexpected error: {err}"
    );
}

/// Concurrency is clamped to at least 1: a zero would stall every ingest instead of
/// degrading it. `tokio::test` because building the contextualizer calls
/// `Endpoint::connect_lazy`, which registers with the Tokio reactor.
#[tokio::test]
async fn zero_concurrency_is_clamped_rather_than_stalling_the_stream() {
    let cfg = Config {
        contextual_retrieval_concurrency: 0,
        ..enabled_config()
    };
    let built = Contextualizer::from_config(&cfg)
        .expect("valid config")
        .expect("enabled");
    assert_eq!(built.settings.concurrency, 1);
}

#[tokio::test]
async fn an_empty_chunk_list_makes_no_calls_and_mints_nothing() {
    let cfg = Config {
        // Unreachable on purpose: reaching the network at all would surface as a
        // hang/error instead of the empty vec asserted below.
        model_plane_ai_core_grpc_url: "http://127.0.0.1:1".to_string(),
        ..enabled_config()
    };
    let built = Contextualizer::from_config(&cfg)
        .expect("valid config")
        .expect("enabled");
    let prepared = built.prepare(DocumentInput {
        org_id: "org-1",
        document_id: "doc",
        meta: DocumentMeta::default(),
        text: "doc body",
        restricted: false,
        prompt_version: None,
        total_chunks: None,
    });
    assert!(built.contextualize(&prepared, &[], None).await.is_empty());
}

// ── compose ──────────────────────────────────────────────────────────────────

const DATA_PLANE_COMPOSE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docker-compose.yml");
const MODEL_PLANE_COMPOSE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../Model Plane/deploy/docker-compose.yml"
);

/// The `KEY: value` lines of one service's `environment:` block, by plain text: the
/// compose files are hand-written with one level of nesting, and a YAML parser is a
/// dependency this crate does not otherwise need.
fn service_environment(compose: &str, service: &str) -> Vec<(String, String)> {
    let header = format!("  {service}:");
    let mut lines = compose.lines().map(|line| line.trim_end_matches('\r'));
    for line in lines.by_ref() {
        if line == header {
            break;
        }
    }
    let mut in_environment = false;
    let mut found = Vec::new();
    for line in lines {
        // The next service starts at two spaces of indent.
        if line.len() > 2
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.ends_with(':')
        {
            break;
        }
        if line.trim() == "environment:" {
            in_environment = true;
            continue;
        }
        if in_environment {
            let trimmed = line.trim();
            if let Some((key, value)) = trimmed.split_once(": ") {
                if key
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    found.push((key.to_string(), value.trim().trim_matches('"').to_string()));
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') && !trimmed.starts_with("- ")
            {
                in_environment = false;
            }
        }
    }
    found
}

/// `${NAME:-default}` evaluated with nothing set; `None` for a value with no default.
fn compose_default(value: &str) -> Option<String> {
    let inner = value.strip_prefix("${")?.strip_suffix('}')?;
    let (_, default) = inner.split_once(":-")?;
    Some(default.to_string())
}

fn read_compose(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            assert!(
                !require_contract(),
                "{path} is not reachable from this checkout ({error}); the compose contract cannot be checked"
            );
            eprintln!("skipped: {path} is not in this tree ({error})");
            None
        }
    }
}

/// The effective residency floor, under the real compose environment of the service,
/// with nothing overridden. Compose passes an empty string for an unset variable, and
/// an empty floor must not mean "no floor".
#[test]
fn the_compose_environment_cannot_weaken_the_floors_by_leaving_them_blank() {
    let Some(compose) = read_compose(DATA_PLANE_COMPOSE) else {
        return;
    };
    let environment = service_environment(&compose, "embedding-engine");
    assert!(
        environment
            .iter()
            .any(|(key, _)| key == "CONTEXTUAL_RETRIEVAL_ENABLED"),
        "the embedding-engine block was not found; the extractor needs updating"
    );
    let mut vars: Vec<(String, String)> = vec![
        ("DATABASE_URL".into(), "postgres://test.invalid/test".into()),
        ("NATS_URL".into(), "nats://test.invalid:4222".into()),
        ("QDRANT_URL".into(), "http://test.invalid:6334".into()),
        ("CONTEXTUAL_RETRIEVAL_ENABLED".into(), "true".into()),
        ("CONTEXTUAL_RETRIEVAL_MODEL".into(), "gpt-4o-mini".into()),
        (
            "MODEL_PLANE_INFERENCE_SERVICE_API_KEY".into(),
            "test-key-0123456789".into(),
        ),
    ];
    for (key, value) in &environment {
        if (key.starts_with("CONTEXTUAL_RETRIEVAL_") || key.starts_with("CONTEXT_INLINE_"))
            && key != "CONTEXTUAL_RETRIEVAL_ENABLED"
            && key != "CONTEXTUAL_RETRIEVAL_MODEL"
        {
            if let Some(default) = compose_default(value) {
                vars.push((key.clone(), default));
            }
        }
    }
    let config =
        envy::from_iter::<_, Config>(vars).expect("the compose defaults are a valid configuration");
    let settings = Settings::from_config(&config).expect("and a startable one");
    assert_eq!(
        settings.provider_hint, "azure-openai",
        "the route stays pinned"
    );
    assert_eq!(settings.min_residency, "eu", "the residency floor stays EU");
}

/// The compose file's defaults for this service's contextual retrieval variables, evaluated with
/// nothing set, are the code's defaults, and contextual retrieval is on (CR-T13). The two are
/// written in two places, and a default that differs makes the container behave other than the
/// binary says it does: a compose `false` over a code `true` is how the feature stayed off while
/// the documents said it was on. The whole parsed configuration is compared, so a variable added
/// to one place and not to the other is a failure too.
#[test]
fn the_compose_defaults_are_the_code_defaults_and_contextual_retrieval_is_on() {
    const PREFIXES: [&str; 5] = [
        "CONTEXTUAL_RETRIEVAL_",
        "CONTEXT_INLINE_",
        "CONTEXT_RECONCILER_",
        "CONTEXT_REFRESH_",
        "CONTEXT_RETRIEVAL_",
    ];
    let Some(compose) = read_compose(DATA_PLANE_COMPOSE) else {
        return;
    };
    let environment = service_environment(&compose, "embedding-engine");
    let default_of = |key: &str| {
        environment
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| compose_default(value))
    };
    assert_eq!(
        default_of("CONTEXTUAL_RETRIEVAL_ENABLED").as_deref(),
        Some("true"),
        "the compose default of the switch is on: the flag is the incident kill switch"
    );
    assert_eq!(
        default_of("CONTEXTUAL_RETRIEVAL_MODEL").as_deref(),
        Some("gpt-4o-mini"),
        "the compose default names the model, so the model (and its cost) is never implicit"
    );

    let mut vars: Vec<(String, String)> = vec![
        ("DATABASE_URL".into(), "postgres://test.invalid/test".into()),
        ("NATS_URL".into(), "nats://test.invalid:4222".into()),
        ("QDRANT_URL".into(), "http://test.invalid:6334".into()),
    ];
    for (key, value) in &environment {
        if PREFIXES.iter().any(|prefix| key.starts_with(prefix)) {
            if let Some(default) = compose_default(value) {
                vars.push((key.clone(), default));
            }
        }
    }
    let from_compose =
        envy::from_iter::<_, Config>(vars).expect("the compose defaults are a valid configuration");
    assert_eq!(
        format!("{from_compose:?}"),
        format!("{:?}", minimal_config()),
        "a compose default differs from the code default of the same variable"
    );
}

/// A floor is only as good as inference-core's proof of where its deployment is. The
/// compose file of the Model Plane must carry `AZURE_OPENAI_REGION` through to
/// inference-core, or `eu` is refused on every call. (Today the variable has an empty
/// default and the live value comes from the git-ignored `.env`: a fresh environment
/// refuses every call, which is why a residency refusal is an outcome of its own and
/// loud. The runbook's preflight checks the live value.)
#[test]
fn the_residency_floor_has_a_region_to_be_proved_against() {
    let Some(data_plane) = read_compose(DATA_PLANE_COMPOSE) else {
        return;
    };
    let Some(model_plane) = read_compose(MODEL_PLANE_COMPOSE) else {
        return;
    };

    let engine = service_environment(&data_plane, "embedding-engine");
    let floor = engine
        .iter()
        .find(|(key, _)| key == "CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY")
        .and_then(|(_, value)| compose_default(value))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MIN_RESIDENCY.to_string());
    if floor == "global" {
        return; // no floor to prove
    }

    let inference = service_environment(&model_plane, "inference-core");
    assert!(
        !inference.is_empty(),
        "the inference-core block of the Model Plane compose was not found"
    );
    let region = inference
        .iter()
        .find(|(key, _)| key == "AZURE_OPENAI_REGION")
        .unwrap_or_else(|| {
            panic!(
                "the residency floor is `{floor}` but inference-core is not given AZURE_OPENAI_REGION: its Azure \
                 deployment would read as unproven and every contextualization call would be refused"
            )
        });
    // Wired through from the environment (or fixed to an EU region), never a literal
    // non-EU region.
    let value = region.1.as_str();
    assert!(
        value.starts_with("${AZURE_OPENAI_REGION") || is_eu_region_label(value),
        "AZURE_OPENAI_REGION is `{value}`"
    );
}

/// Azure regions inference-core treats as EU/EEA (provider/mod.rs `EU_AZURE_REGIONS`,
/// the ones this deployment could plausibly use).
fn is_eu_region_label(value: &str) -> bool {
    [
        "swedencentral",
        "westeurope",
        "northeurope",
        "francecentral",
        "germanywestcentral",
        "norwayeast",
        "polandcentral",
        "italynorth",
        "spaincentral",
        "switzerlandnorth",
    ]
    .contains(&value.trim().to_ascii_lowercase().as_str())
}

/// The migration's CHECK vocabulary and this crate's statuses were compared in the
/// outcome tests; this one is about the table the engine writes into: it must exist
/// where the rest of the build expects it.
#[test]
fn the_migration_this_engine_writes_for_is_in_the_tree() {
    if migration_sql().is_none() {
        assert!(
            !require_contract(),
            "20261003130000_context_provenance.sql is not in this checkout"
        );
    }
}

// ── bounds the validation relies on ─────────────────────────────────────────

// At most about 60 words, which is well under 600 characters in any language.
const _: () = assert!(MIN_CHARS < 20 && MAX_CHARS >= 400);
