//! What the pipeline's tests stand on.
//!
//! The database is real: Postgres 16 with `infra/postgres/init.sql` and every
//! migration applied by the real migrator (`MIGRATED_TEST_DATABASE_URL`, which
//! `tests/scripts/run-postgres-contract-tests.sh` provides and a test that cannot
//! find it FAILS on). The token client is the real one, reading what Auth Core's real
//! mint produced (see `context::testing`), and the model is the engine's own port
//! answering per tenant (see `provider::contextualize::testing`). What is
//! replaced is only what cannot be asked what it was sent: the embedding model, the
//! vector store and the message bus. Each of those records the call together with
//! what the database held AT THAT MOMENT, which is how the order of the writes
//! (write-ahead, then embed, then upsert, then done, then announce) is observed
//! instead of assumed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use inference_client_rs::{InferenceTokenClient, RetentionPosture, TokenClientConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::ports::PortFuture;
use super::{BatchItem, Embedder, EventSink, Pipeline, VectorSink};
use crate::budget::testing::{gate as budget_gate, global as budget_global, ScriptedSpend};
use crate::budget::{BudgetGate, BudgetMode, GlobalBudget};
use crate::config::Config;
use crate::context::testing::{ManualClock, TokenGolden, TokenStub, SERVICE_KEY, TOKEN_PATH};
use crate::policy::{PgPolicySource, PolicyReader, PolicySource, ReadError, SourceFuture};
use crate::provider::contextualize::testing::{FakeModel, ModelPort};
use crate::provider::contextualize::{Contextualizer, Settings};
use crate::qdrant_writer::{self, EmbeddingPoint};

pub(crate) const GATE: &str = "MIGRATED_TEST_DATABASE_URL";
pub(crate) const EMBEDDING_MODEL: &str = "embed-v-4-0";
pub(crate) const COLLECTION: &str = "knowledge_test";
pub(crate) const TITLE: &str = "Årsrapport 2025";
const WIDTH: usize = 256;

/// The vector the fake embedder returns for a text: a pure function of the text, so a
/// test can say what the vector of a unit must be (the one of the text it is built
/// from) instead of only that "a vector was stored".
pub(crate) fn vector_for(text: &str) -> Vec<f32> {
    let digest = Sha256::digest(text.as_bytes());
    (0..WIDTH)
        .map(|index| f32::from(digest[index % digest.len()]) / 255.0)
        .collect()
}

// ── the database ────────────────────────────────────────────────────────────

/// A pool on the migrated schema, as the superuser every Data Plane service connects
/// as (so row-level security is inert, exactly as in production). Refuses a database
/// whose name does not end in `_test`: these tests insert and update freely.
pub(crate) async fn connect() -> PgPool {
    let url = std::env::var(GATE).expect(
        "MIGRATED_TEST_DATABASE_URL must point at the migrated Data Plane schema \
         (tests/scripts/run-postgres-contract-tests.sh provides it)",
    );
    let name = url
        .rsplit('/')
        .next()
        .and_then(|tail| tail.split('?').next())
        .unwrap_or_default();
    assert!(
        name.ends_with("_test"),
        "refusing to run against database `{name}`: the name must end in _test"
    );
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .expect("connect to the migrated test database")
}

/// One unit as the database holds it.
#[derive(Clone, Debug)]
pub(crate) struct UnitRow {
    pub knowledge_id: String,
    pub org_id: String,
    pub embedding_status: String,
    pub embedding_model: Option<String>,
    pub embedded_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub chunk_context: Option<String>,
    pub context_status: Option<String>,
    pub context_model: Option<String>,
    pub context_provider: Option<String>,
    pub context_residency: Option<String>,
    pub context_prompt_version: Option<String>,
    pub context_input_hash: Option<String>,
    pub embedded_context_hash: Option<String>,
    pub context_generated_at: Option<DateTime<Utc>>,
    pub context_attempts: i32,
    pub context_next_attempt_at: Option<DateTime<Utc>>,
    pub context_error_class: Option<String>,
}

const ROW_COLUMNS: &str = "knowledge_id, org_id, embedding_status, embedding_model, embedded_at, \
    updated_at, chunk_context, context_status, context_model, context_provider, \
    context_residency, context_prompt_version, context_input_hash, embedded_context_hash, \
    context_generated_at, context_attempts, context_next_attempt_at, context_error_class";

fn row_of(row: &PgRow) -> UnitRow {
    UnitRow {
        knowledge_id: row.get("knowledge_id"),
        org_id: row.get("org_id"),
        embedding_status: row.get("embedding_status"),
        embedding_model: row.get("embedding_model"),
        embedded_at: row.get("embedded_at"),
        updated_at: row.get("updated_at"),
        chunk_context: row.get("chunk_context"),
        context_status: row.get("context_status"),
        context_model: row.get("context_model"),
        context_provider: row.get("context_provider"),
        context_residency: row.get("context_residency"),
        context_prompt_version: row.get("context_prompt_version"),
        context_input_hash: row.get("context_input_hash"),
        embedded_context_hash: row.get("embedded_context_hash"),
        context_generated_at: row.get("context_generated_at"),
        context_attempts: row.get("context_attempts"),
        context_next_attempt_at: row.get("context_next_attempt_at"),
        context_error_class: row.get("context_error_class"),
    }
}

pub(crate) async fn row(pool: &PgPool, knowledge_id: &str) -> UnitRow {
    let sql = format!("SELECT {ROW_COLUMNS} FROM knowledge_units WHERE knowledge_id = $1");
    let found = sqlx::query(&sql)
        .bind(knowledge_id)
        .fetch_optional(pool)
        .await
        .expect("read a unit");
    row_of(&found.unwrap_or_else(|| panic!("unit {knowledge_id} does not exist")))
}

/// What the database held about some units at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    pub knowledge_id: String,
    pub context: Option<String>,
    pub status: Option<String>,
    pub input_hash: Option<String>,
    pub embedding_status: String,
}

async fn snapshot(pool: &PgPool, ids: &[String]) -> Vec<Seen> {
    if ids.is_empty() {
        return Vec::new();
    }
    sqlx::query(
        "SELECT knowledge_id, chunk_context, context_status, context_input_hash, embedding_status \
           FROM knowledge_units WHERE knowledge_id = ANY($1) ORDER BY knowledge_id",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .expect("snapshot units")
    .iter()
    .map(|row| Seen {
        knowledge_id: row.get("knowledge_id"),
        context: row.get("chunk_context"),
        status: row.get("context_status"),
        input_hash: row.get("context_input_hash"),
        embedding_status: row.get("embedding_status"),
    })
    .collect()
}

/// The units whose state the fakes record at every call.
#[derive(Clone, Default)]
pub(crate) struct Watch(Arc<Mutex<Vec<String>>>);

impl Watch {
    pub(crate) fn set(&self, ids: Vec<String>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = ids;
    }

    fn ids(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

// ── fixtures ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub(crate) struct Unit {
    pub knowledge_id: String,
    pub index: i32,
    pub text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Doc {
    pub org: String,
    pub id: String,
    pub title: String,
    pub content: String,
    pub restricted: bool,
    pub units: Vec<Unit>,
}

impl Doc {
    pub(crate) fn ids(&self) -> Vec<String> {
        self.units
            .iter()
            .map(|unit| unit.knowledge_id.clone())
            .collect()
    }
}

// ── Auth Core, per organization ─────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub(crate) enum Posture {
    /// Not zero-retention: a bearer with `zdr: false`.
    Plain,
    /// Zero-retention: a bearer with `zdr: true`.
    Zdr,
    /// Auth Core could not resolve the posture: 503 `retention_posture_unavailable`.
    Unresolved,
    /// Auth Core does not know the organization: 403.
    Refused,
}

/// Auth Core's token endpoint for many organizations. The answers are the bytes the
/// real mint produced for the two organizations the golden captured (a plain one
/// and a zero-retention one), with exactly one thing changed so that they can be
/// served for another organization: the token's `org_id` claim. The real client
/// then applies every check it has (issuer, audience, subject, scopes, reason, posture,
/// lifetime, and that the claim names the organization it asked for).
pub(crate) struct Tenants {
    stub: TokenStub,
    golden: TokenGolden,
    postures: Arc<Mutex<HashMap<String, Posture>>>,
}

fn retargeted(body: &Value, org_id: &str) -> Vec<u8> {
    let token = body["token"].as_str().expect("a token");
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "a compact JWT");
    let mut claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).expect("payload"))
            .expect("claims");
    claims["org_id"] = json!(org_id);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("encode claims"));
    let mut out = body.clone();
    out["token"] = json!(format!("{}.{payload}.{}", parts[0], parts[2]));
    serde_json::to_vec(&out).expect("encode body")
}

impl Tenants {
    pub(crate) async fn start() -> Self {
        let golden = TokenGolden::load();
        let plain = golden.case("embeddingEngineTenantOrgPlain");
        let zdr = golden.case("embeddingEngineTenantOrgZdr");
        let unresolved = golden.case("tenantPostureUnavailable");
        let refused = golden.case("organizationNotInDirectory");
        let postures: Arc<Mutex<HashMap<String, Posture>>> = Arc::default();
        let table = postures.clone();
        let stub = TokenStub::scripted(move |body, _headers| {
            let org = body["orgId"].as_str().unwrap_or_default();
            let posture = table
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(org)
                .copied()
                .unwrap_or(Posture::Plain);
            match posture {
                Posture::Plain => (plain.status, retargeted(&plain.body, org)),
                Posture::Zdr => (zdr.status, retargeted(&zdr.body, org)),
                Posture::Unresolved => (
                    unresolved.status,
                    serde_json::to_vec(&unresolved.body).expect("encode"),
                ),
                Posture::Refused => (
                    refused.status,
                    serde_json::to_vec(&refused.body).expect("encode"),
                ),
            }
        })
        .await;
        Self {
            stub,
            golden,
            postures,
        }
    }

    pub(crate) fn set(&self, org_id: &str, posture: Posture) {
        self.postures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org_id.to_owned(), posture);
    }

    pub(crate) fn client(&self) -> InferenceTokenClient {
        self.stub.client(&self.golden, RetentionPosture::Tenant)
    }

    /// How many requests reached Auth Core, and what each asked for.
    pub(crate) fn requests(&self) -> Vec<Value> {
        self.stub.requests()
    }
}

/// An Auth Core that accepts the connection and never answers. What a bearer mint does when
/// Auth Core is hung rather than down: the shared token client gives up after its own
/// five seconds, which is past a small inline deadline, and the contextualizer does not
/// time-box a mint.
pub(crate) struct HangingAuth {
    server: tokio::task::JoinHandle<()>,
}

impl Drop for HangingAuth {
    fn drop(&mut self) {
        self.server.abort();
    }
}

pub(crate) async fn hanging_auth() -> (HangingAuth, InferenceTokenClient) {
    let app = axum::Router::new().route(
        TOKEN_PATH,
        axum::routing::post(|| async {
            std::future::pending::<()>().await;
            "never"
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let address = listener.local_addr().expect("local address");
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let golden = TokenGolden::load();
    let client = InferenceTokenClient::with_clock(
        TokenClientConfig {
            token_url: format!("http://{address}{TOKEN_PATH}"),
            expected_issuer: golden.issuer(),
            service_id: "embedding-engine".to_owned(),
            service_api_key: SERVICE_KEY.to_owned(),
            posture: RetentionPosture::Tenant,
            allow_unconfigured: false,
        },
        ManualClock::at(golden.frozen_now() + 1),
    )
    .expect("a valid client configuration");
    (HangingAuth { server }, client)
}

// ── the embedding model, the vector store, the bus ──────────────────────────

#[derive(Clone)]
pub(crate) struct EmbedCall {
    pub org_id: String,
    pub zdr: bool,
    pub texts: Vec<String>,
    pub seen: Vec<Seen>,
}

pub(crate) struct FakeEmbedder {
    pool: PgPool,
    watch: Watch,
    calls: Mutex<Vec<EmbedCall>>,
    fail: AtomicUsize,
    /// What the injected failures are, when they are a provider's refusal and not an untyped error.
    refusal: Mutex<Option<tonic::Status>>,
}

impl FakeEmbedder {
    pub(crate) fn fail_next(&self, times: usize) {
        self.fail.store(times, Ordering::SeqCst);
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The next `times` embeddings fail the way a call that inference-core refused fails: with
    /// the typed error the client decodes from this status, which is what the reconciler reads
    /// the health of the provider from.
    pub(crate) fn refuse_next(&self, times: usize, status: tonic::Status) {
        self.fail.store(times, Ordering::SeqCst);
        *self.refusal.lock().unwrap_or_else(PoisonError::into_inner) = Some(status);
    }

    pub(crate) fn calls(&self) -> Vec<EmbedCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Every text that was embedded, in order, across calls.
    pub(crate) fn texts(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .flat_map(|call| call.texts)
            .collect()
    }
}

impl Embedder for FakeEmbedder {
    fn model_name(&self) -> &str {
        EMBEDDING_MODEL
    }

    fn provider_name(&self) -> &str {
        "cohere"
    }

    fn embed_batch<'a>(
        &'a self,
        org_id: &'a str,
        texts: &'a [String],
        zdr: bool,
    ) -> PortFuture<'a, Vec<Vec<f32>>> {
        Box::pin(async move {
            let seen = snapshot(&self.pool, &self.watch.ids()).await;
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(EmbedCall {
                    org_id: org_id.to_owned(),
                    zdr,
                    texts: texts.to_vec(),
                    seen,
                });
            if self
                .fail
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                let refusal = self
                    .refusal
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone();
                if let Some(status) = refusal {
                    return Err(anyhow::Error::new(
                        inference_client_rs::InferenceError::from_status(&status)
                            .during("model-plane embedding"),
                    )
                    .context("embed the units"));
                }
                anyhow::bail!("injected embedding failure");
            }
            Ok(texts.iter().map(|text| vector_for(text)).collect())
        })
    }
}

/// A point as Qdrant would receive it.
#[derive(Clone, Debug)]
pub(crate) struct Recorded {
    pub knowledge_id: String,
    pub document_id: String,
    pub org_id: String,
    pub chunk_index: i32,
    pub text: String,
    pub vector: Vec<f32>,
    /// The `source` the payload carries, read from the conversion Qdrant's client is handed
    /// (so a stamp that the writer drops is not here).
    pub source: Option<String>,
    pub metadata: BTreeMap<String, String>,
    /// The keys of the payload the real conversion produces for it.
    pub payload_keys: BTreeSet<String>,
}

#[derive(Clone)]
pub(crate) struct Upsert {
    pub collection: String,
    pub points: Vec<Recorded>,
    pub seen: Vec<Seen>,
}

pub(crate) struct RecordingVectors {
    pool: PgPool,
    watch: Watch,
    upserts: Mutex<Vec<Upsert>>,
    /// What the store holds now: the last point written for each id.
    stored: Mutex<HashMap<String, Recorded>>,
    fail: AtomicUsize,
}

impl RecordingVectors {
    pub(crate) fn fail_next(&self, times: usize) {
        self.fail.store(times, Ordering::SeqCst);
    }

    pub(crate) fn upsert_count(&self) -> usize {
        self.upserts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Every point of every upsert, in order.
    pub(crate) fn points(&self) -> Vec<Recorded> {
        self.upserts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .flat_map(|upsert| upsert.points.clone())
            .collect()
    }

    pub(crate) fn collections(&self) -> Vec<String> {
        self.upserts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|upsert| upsert.collection.clone())
            .collect()
    }

    pub(crate) fn seen_at_upserts(&self) -> Vec<Vec<Seen>> {
        self.upserts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|upsert| upsert.seen.clone())
            .collect()
    }

    pub(crate) fn stored(&self, knowledge_id: &str) -> Option<Recorded> {
        self.stored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(knowledge_id)
            .cloned()
    }
}

impl VectorSink for RecordingVectors {
    fn upsert<'a>(
        &'a self,
        collection: &'a str,
        points: Vec<EmbeddingPoint>,
    ) -> PortFuture<'a, ()> {
        Box::pin(async move {
            let seen = snapshot(&self.pool, &self.watch.ids()).await;
            let mut recorded: Vec<Recorded> = points
                .iter()
                .map(|point| Recorded {
                    knowledge_id: point.knowledge_id.clone(),
                    document_id: point.document_id.clone(),
                    org_id: point.org_id.clone(),
                    chunk_index: point.chunk_index,
                    text: point.text.clone(),
                    vector: point.vector.clone(),
                    source: None,
                    metadata: point
                        .metadata
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                    payload_keys: BTreeSet::new(),
                })
                .collect();
            // The conversion Qdrant's client is handed, applied for real: what the
            // payload carries is read from it, not from the input.
            for (point, converted) in recorded
                .iter_mut()
                .zip(qdrant_writer::point_structs(points))
            {
                point.payload_keys = converted.payload.keys().cloned().collect();
                point.source = match converted
                    .payload
                    .get(crate::source_stamp::PAYLOAD_KEY)
                    .and_then(|value| value.kind.as_ref())
                {
                    Some(qdrant_client::qdrant::value::Kind::StringValue(source)) => {
                        Some(source.clone())
                    }
                    _ => None,
                };
            }
            if self
                .fail
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                anyhow::bail!("injected vector store failure");
            }
            let mut stored = self.stored.lock().unwrap_or_else(PoisonError::into_inner);
            for point in &recorded {
                stored.insert(point.knowledge_id.clone(), point.clone());
            }
            drop(stored);
            self.upserts
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Upsert {
                    collection: collection.to_owned(),
                    points: recorded,
                    seen,
                });
            Ok(())
        })
    }
}

#[derive(Clone)]
pub(crate) struct Published {
    pub subject: String,
    pub payload: Value,
    pub seen: Vec<Seen>,
}

pub(crate) struct RecordingEvents {
    pool: PgPool,
    watch: Watch,
    published: Mutex<Vec<Published>>,
}

impl RecordingEvents {
    pub(crate) fn published(&self) -> Vec<Published> {
        self.published
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn on(&self, subject: &str) -> Vec<Published> {
        self.published()
            .into_iter()
            .filter(|event| event.subject == subject)
            .collect()
    }
}

impl EventSink for RecordingEvents {
    fn publish<'a>(&'a self, subject: &'a str, payload: Vec<u8>) -> PortFuture<'a, ()> {
        Box::pin(async move {
            let seen = snapshot(&self.pool, &self.watch.ids()).await;
            self.published
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Published {
                    subject: subject.to_owned(),
                    // Unsigned in these tests (no signer is configured): the body is the JSON.
                    payload: serde_json::from_slice(&payload).expect("an unsigned JSON event"),
                    seen,
                });
            Ok(())
        })
    }
}

// ── the rig ─────────────────────────────────────────────────────────────────

fn config() -> Config {
    let base: Config = envy::from_iter::<_, Config>(vec![
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
    .expect("minimal config");
    Config {
        contextual_retrieval_enabled: true,
        contextual_retrieval_model: "gpt-4o-mini".to_string(),
        model_plane_inference_service_api_key: "test-key-0123456789".to_string(),
        ..base
    }
}

pub(crate) fn settings() -> Settings {
    let mut settings = Settings::from_config(&config()).expect("valid settings");
    // A failed call is not retried inline in these tests: a retry sleeps, and what a
    // test asserts is what the failure becomes.
    settings.retry_attempts = 0;
    settings
}

/// The real `org_processing_policy` table, with a switch that makes reading it fail the way a
/// database that is struggling, or that has no such table, does.
pub(crate) struct SwitchableSource {
    inner: PgPolicySource,
    failure: Mutex<Option<fn() -> ReadError>>,
    reads: AtomicUsize,
}

impl SwitchableSource {
    pub(crate) fn fail_with(&self, failure: Option<fn() -> ReadError>) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = failure;
    }

    /// How many times the table was asked.
    pub(crate) fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl PolicySource for SwitchableSource {
    fn read<'a>(
        &'a self,
        org_ids: &'a [String],
    ) -> SourceFuture<'a, Vec<crate::policy::PolicyRow>> {
        Box::pin(async move {
            self.reads.fetch_add(1, Ordering::SeqCst);
            let failure = *self.failure.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(failure) = failure {
                return Err(failure());
            }
            self.inner.read(org_ids).await
        })
    }
}

pub(crate) struct Rig {
    pub pool: PgPool,
    /// The operator policy as the pipeline reads it: the real table, with a cache lifetime of
    /// zero so that a test sees its own change at once (the 30-second cache is tested in
    /// `policy`, and a longer one is built in the tests that need it).
    pub policy: PolicyReader,
    pub policy_source: Arc<SwitchableSource>,
    /// The background budget gate (CR-T9b) over a ledger the test scripts: by default the one
    /// every deployment starts with (alert only, no figure), which must never read the ledger.
    /// A test that is about the budget replaces it (`rig.budget = ...`) and sets what the ledger
    /// says with `rig.spend`.
    pub budget: Arc<BudgetGate>,
    pub spend: Arc<ScriptedSpend>,
    pub tenants: Tenants,
    pub model: Arc<FakeModel>,
    pub embedder: Arc<FakeEmbedder>,
    pub vectors: Arc<RecordingVectors>,
    pub events: Arc<RecordingEvents>,
    pub engine: Contextualizer,
    pub watch: Watch,
    tag: String,
    next: AtomicUsize,
}

impl Rig {
    pub(crate) async fn new() -> Self {
        Self::with(|_| {}).await
    }

    pub(crate) async fn with(tweak: impl FnOnce(&mut Settings)) -> Self {
        let pool = connect().await;
        let tenants = Tenants::start().await;
        let model = FakeModel::new();
        let mut settings = settings();
        tweak(&mut settings);
        let engine = Contextualizer::new(
            Arc::new(ModelPort(model.clone())),
            tenants.client(),
            settings,
        );
        let watch = Watch::default();
        let policy_source = Arc::new(SwitchableSource {
            inner: PgPolicySource::new(pool.clone()),
            failure: Mutex::new(None),
            reads: AtomicUsize::new(0),
        });
        let spend = ScriptedSpend::new();
        Self {
            policy: PolicyReader::with_ttl(policy_source.clone(), std::time::Duration::ZERO),
            policy_source,
            budget: Arc::new(BudgetGate::new(
                GlobalBudget::default(),
                true,
                spend.clone(),
            )),
            spend,
            embedder: Arc::new(FakeEmbedder {
                pool: pool.clone(),
                watch: watch.clone(),
                calls: Mutex::new(Vec::new()),
                fail: AtomicUsize::new(0),
                refusal: Mutex::new(None),
            }),
            vectors: Arc::new(RecordingVectors {
                pool: pool.clone(),
                watch: watch.clone(),
                upserts: Mutex::new(Vec::new()),
                stored: Mutex::new(HashMap::new()),
                fail: AtomicUsize::new(0),
            }),
            events: Arc::new(RecordingEvents {
                pool: pool.clone(),
                watch: watch.clone(),
                published: Mutex::new(Vec::new()),
            }),
            pool,
            tenants,
            model,
            engine,
            watch,
            tag: Uuid::new_v4().simple().to_string()[..8].to_owned(),
            next: AtomicUsize::new(1),
        }
    }

    /// An organization id that no other test uses.
    pub(crate) fn org(&self, name: &str) -> String {
        format!("org{name}-{}", self.tag)
    }

    fn id(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{}-{:04}",
            self.tag,
            self.next.fetch_add(1, Ordering::SeqCst)
        )
    }

    /// A document of the given classification with one pending unit per chunk. The
    /// document's content is the chunks, so each chunk is found in it.
    pub(crate) async fn document(&self, org: &str, classification: &str, chunks: &[&str]) -> Doc {
        self.document_from("test", org, classification, chunks)
            .await
    }

    /// The same, from the given source (`documents.source`): what a vector's `source`
    /// stamp must say.
    pub(crate) async fn document_from(
        &self,
        source: &str,
        org: &str,
        classification: &str,
        chunks: &[&str],
    ) -> Doc {
        let id = self.id("doc");
        sqlx::query(
            "INSERT INTO documents (document_id, org_id, source, type, title, content, status, zdr_classification) \
             VALUES ($1, $2, $6, 'text', $3, $4, 'pending', $5)",
        )
        .bind(&id)
        .bind(org)
        .bind(TITLE)
        .bind(chunks.join("\n\n"))
        .bind(classification)
        .bind(source)
        .execute(&self.pool)
        .await
        .expect("insert a document");
        let mut units = Vec::new();
        for (index, text) in chunks.iter().enumerate() {
            let knowledge_id = Uuid::new_v4().to_string();
            let index = i32::try_from(index).expect("small");
            sqlx::query(
                "INSERT INTO knowledge_units (knowledge_id, document_id, org_id, chunk_index, text) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&knowledge_id)
            .bind(&id)
            .bind(org)
            .bind(index)
            .bind(*text)
            .execute(&self.pool)
            .await
            .expect("insert a unit");
            units.push(Unit {
                knowledge_id,
                index,
                text: (*text).to_owned(),
            });
        }
        Doc {
            org: org.to_owned(),
            id,
            title: TITLE.to_owned(),
            content: chunks.join("\n\n"),
            restricted: classification == "restricted",
            units,
        }
    }

    pub(crate) async fn plain_document(&self, org: &str, chunks: &[&str]) -> Doc {
        self.document(org, "internal", chunks).await
    }

    /// The batch items the stream builds for a document's units.
    pub(crate) fn items(&self, doc: &Doc) -> Vec<BatchItem> {
        doc.units
            .iter()
            .map(|unit| BatchItem {
                knowledge_id: unit.knowledge_id.clone(),
                document_id: doc.id.clone(),
                org_id: doc.org.clone(),
                chunk_index: unit.index,
                text: unit.text.clone(),
                zdr: doc.restricted,
                user_id: Some("user-1".to_owned()),
                document_date: None,
                chunk_context: None,
            })
            .collect()
    }

    pub(crate) fn pipeline(&self) -> Pipeline<'_> {
        Pipeline {
            provider: self.embedder.as_ref(),
            vectors: self.vectors.as_ref(),
            pool: &self.pool,
            collection: COLLECTION,
            events: self.events.as_ref(),
            event_signer: None,
            contextualizer: Some(&self.engine),
            policy: &self.policy,
            budget: self.budget.as_ref(),
            usage: None,
            pass: super::PassKind::Inline,
        }
    }

    /// Set what an organization's own row says about its budget, as the admin API will write it
    /// once the gate reads it (the table's CHECK constraints apply). `mode` is `inherit`,
    /// `alert_only`, `throttle` or `stop`; the figure is dollars a month.
    pub(crate) async fn set_budget(&self, org: &str, mode: &str, figure: Option<f64>) {
        sqlx::query(
            "INSERT INTO org_processing_policy \
                    (org_id, budget_mode, monthly_budget_usd, reason, updated_by) \
             VALUES ($1, $2, $3::float8::numeric, 'engine test', 'engine-test') \
             ON CONFLICT (org_id) DO UPDATE SET \
                    budget_mode = EXCLUDED.budget_mode, \
                    monthly_budget_usd = EXCLUDED.monthly_budget_usd, \
                    reason = EXCLUDED.reason",
        )
        .bind(org)
        .bind(mode)
        .bind(figure)
        .execute(&self.pool)
        .await
        .expect("set the organization's budget");
    }

    /// A gate with a deployment-wide budget (and the test price table: a dollar is a million
    /// input tokens) over this rig's scripted ledger, which is recording.
    pub(crate) fn deployment_budget(&mut self, mode: BudgetMode, monthly_usd: Option<f64>) {
        self.budget = Arc::new(budget_gate(budget_global(mode, monthly_usd), &self.spend));
    }

    /// Set an organization's operator policy, as the admin API would have written it (an off
    /// carries the reason the table requires).
    pub(crate) async fn set_policy(&self, org: &str, switch: &str, pin: Option<&str>) {
        sqlx::query(
            "INSERT INTO org_processing_policy \
                    (org_id, contextual_retrieval, context_prompt_version, reason, updated_by) \
             VALUES ($1, $2, $3, 'engine test', 'engine-test') \
             ON CONFLICT (org_id) DO UPDATE SET \
                    contextual_retrieval = EXCLUDED.contextual_retrieval, \
                    context_prompt_version = EXCLUDED.context_prompt_version, \
                    reason = EXCLUDED.reason",
        )
        .bind(org)
        .bind(switch)
        .bind(pin)
        .execute(&self.pool)
        .await
        .expect("set the organization's policy");
    }

    /// Remove an organization's policy row: back to the system default.
    pub(crate) async fn clear_policy(&self, org: &str) {
        sqlx::query("DELETE FROM org_processing_policy WHERE org_id = $1")
            .bind(org)
            .execute(&self.pool)
            .await
            .expect("remove the organization's policy");
    }

    /// The same pipeline with contextual retrieval switched off: nothing is
    /// generated, and what is already stored must still be what the vector is built
    /// from.
    pub(crate) fn pipeline_without_generation(&self) -> Pipeline<'_> {
        Pipeline {
            contextualizer: None,
            ..self.pipeline()
        }
    }

    pub(crate) async fn run(&self, items: Vec<BatchItem>) -> anyhow::Result<()> {
        super::process_batch(items, &self.pipeline()).await
    }

    pub(crate) async fn row(&self, knowledge_id: &str) -> UnitRow {
        row(&self.pool, knowledge_id).await
    }

    pub(crate) async fn rows(&self, doc: &Doc) -> Vec<UnitRow> {
        let mut rows = Vec::new();
        for unit in &doc.units {
            rows.push(self.row(&unit.knowledge_id).await);
        }
        rows
    }
}
