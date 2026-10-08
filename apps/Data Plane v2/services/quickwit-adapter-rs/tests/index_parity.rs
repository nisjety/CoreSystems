//! The keyword-index parity check (CR-T9a) against the rules it really runs under.
//!
//! * The Quickwit client is tested against a stand-in index that answers the way the live index
//!   (Quickwit 0.8.1) was MEASURED to answer, not the way a document says it should: the `terms`
//!   aggregation reads the lower-cased fast field, returns at most `size` buckets and reports the
//!   documents it left out, a prefix query on the raw-tokenized id is case-sensitive, a range
//!   query or a sort on that field is refused. The stand-in is a transport with those facts; every
//!   rule under test (the partitioning, the completeness check, the de-duplication, the report)
//!   is the adapter's own. The same checks run against a REAL Quickwit in
//!   `a_real_quickwit_answers_the_parity_check_as_the_stand_in_does` (set `QUICKWIT_TEST_URL`).
//! * The route is tested through the real router, with the real verifier and tokens it signs and
//!   checks, over the real migrated schema (`MIGRATED_TEST_DATABASE_URL`, which a test that needs
//!   it FAILS without): the organization is the bearer's, the scope is `data:quality:admin`, and a
//!   measurement that cannot be taken is a 503 and not a report.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::post,
    Json, Router,
};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use quickwit_adapter_rs::{
    api::router_with_store,
    auth::{AdminClaims, AdminVerifier},
    jobs,
    parity::{self, ParityReport},
    quickwit::{IndexedUnits, QuickwitClient},
    rebuild::RebuildContext,
};
use rsa::{
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
    RsaPrivateKey,
};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

const INDEX_CONFIG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../infra/quickwit/dataplane-corpus-index.yaml"
);

// ── the stand-in index ──────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Doc {
    org_id: String,
    knowledge_id: String,
    /// `None`: the field is absent. (The indexer never writes a blank one: see
    /// `a_blank_context_is_indexed_as_no_context` in `rebuild.rs`.)
    context_body: Option<String>,
}

fn doc(org: &str, id: &str, context: Option<&str>) -> Doc {
    Doc {
        org_id: org.to_owned(),
        knowledge_id: id.to_owned(),
        context_body: context.map(str::to_owned),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    /// Answers every search with this status.
    Status(u16),
    /// Answers as a Quickwit that ignored the aggregation would.
    IgnoreAggregation,
}

struct Index {
    docs: Mutex<Vec<Doc>>,
    mode: Mutex<Mode>,
    requests: Mutex<Vec<Value>>,
}

impl Index {
    fn requests(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What a query of the shapes the adapter writes selects. The org id is quoted, and the id prefix
/// is matched against the id as it was indexed (a raw term: case-sensitive), which is how the
/// live index answers.
fn selects(query: &str, doc: &Doc) -> bool {
    let quoted = |after: &str| -> Option<String> {
        let start = query.find(after)? + after.len();
        let rest = &query[start..];
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?),
                '"' => return Some(out),
                other => out.push(other),
            }
        }
        None
    };
    if quoted("org_id:\"").as_deref() != Some(doc.org_id.as_str()) {
        return false;
    }
    if let Some(start) = query.find("knowledge_id:") {
        let rest = &query[start + "knowledge_id:".len()..];
        let prefix = rest.split('*').next().unwrap_or_default();
        if !doc.knowledge_id.starts_with(prefix) {
            return false;
        }
    }
    if query.contains("context_body:*") {
        // The live index answers `field:*` by whether the field is PRESENT (measured: a
        // whitespace-only body matches too), because the index keeps field presence.
        return doc.context_body.is_some();
    }
    true
}

async fn search(
    State(index): State<Arc<Index>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    index
        .requests
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(body.clone());
    let mode = *index.mode.lock().unwrap_or_else(PoisonError::into_inner);
    if let Mode::Status(code) = mode {
        return (
            StatusCode::from_u16(code).expect("status"),
            Json(json!({"message": "refused"})),
        );
    }
    let query = body["query"].as_str().unwrap_or_default();
    let matching: Vec<Doc> = index
        .docs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|doc| selects(query, doc))
        .cloned()
        .collect();
    let mut answer = json!({
        "num_hits": matching.len(),
        "hits": [],
        "elapsed_time_micros": 1,
        "errors": [],
    });
    if let (Some(terms), Mode::Normal) = (body.pointer("/aggs/ids/terms"), mode) {
        assert_eq!(terms["field"], "knowledge_id", "the parity check reads ids");
        let size = usize::try_from(terms["size"].as_u64().expect("a size")).expect("size");
        // The lower-cased fast field.
        let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
        for doc in &matching {
            *counts.entry(doc.knowledge_id.to_lowercase()).or_default() += 1;
        }
        let mut buckets: Vec<(String, u64)> = counts.into_iter().collect();
        buckets.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let kept: Vec<_> = buckets.iter().take(size).collect();
        let left_out: u64 = buckets.iter().skip(size).map(|(_, count)| count).sum();
        answer["aggregations"] = json!({"ids": {
            "buckets": kept.iter().map(|(key, count)| json!({"doc_count": count, "key": key})).collect::<Vec<_>>(),
            "doc_count_error_upper_bound": 0,
            "sum_other_doc_count": left_out,
        }});
    }
    (StatusCode::OK, Json(answer))
}

struct Quickwit {
    url: String,
    index: Arc<Index>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Quickwit {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Quickwit {
    async fn start(docs: Vec<Doc>) -> Self {
        let index = Arc::new(Index {
            docs: Mutex::new(docs),
            mode: Mutex::new(Mode::Normal),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/api/v1/{index}/search", post(search))
            .with_state(index.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let url = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { url, index, server }
    }

    fn client(&self) -> QuickwitClient {
        QuickwitClient::new(&self.url, "dataplane-corpus", INDEX_CONFIG).expect("a client")
    }

    fn set_mode(&self, mode: Mode) {
        *self
            .index
            .mode
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = mode;
    }
}

fn ids(range: std::ops::Range<usize>) -> Vec<String> {
    range
        .map(|n| format!("{n:08x}-0000-4000-8000-000000000000"))
        .collect()
}

fn set(values: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    values.into_iter().collect()
}

// ── the client against the stand-in ─────────────────────────────────────────

/// Duplicates are one unit, ids come back lower-cased, a unit with no context body has none,
/// another organization's units are not counted, and a unit of another entity type is not a unit.
#[tokio::test]
async fn the_index_is_listed_by_terms_and_deduplicated_by_unit() {
    let mut docs = vec![
        doc(
            "org-a",
            "AAAAAAAA-0000-4000-8000-000000000001",
            Some("En kontekst."),
        ),
        // The same unit indexed twice: a delete that waits for a merge leaves both generations.
        doc("org-a", "aaaaaaaa-0000-4000-8000-000000000001", None),
        doc("org-a", "aaaaaaaa-0000-4000-8000-000000000002", None),
        doc("org-a", "aaaaaaaa-0000-4000-8000-000000000003", None),
        doc(
            "org-a",
            "aaaaaaaa-0000-4000-8000-000000000004",
            Some("Kontekst."),
        ),
        doc(
            "org-b",
            "bbbbbbbb-0000-4000-8000-000000000001",
            Some("Deres."),
        ),
    ];
    docs.extend((0..3).map(|n| {
        doc(
            "org-b",
            &format!("cccccccc-0000-4000-8000-00000000000{n}"),
            None,
        )
    }));
    let quickwit = Quickwit::start(docs).await;

    let units = quickwit
        .client()
        .indexed_units("org-a")
        .await
        .expect("listed");

    assert_eq!(
        units.documents, 5,
        "the index's own count, duplicates included"
    );
    assert_eq!(
        units.distinct,
        set([1, 2, 3, 4].map(|n| format!("aaaaaaaa-0000-4000-8000-00000000000{n}")))
    );
    assert_eq!(
        units.with_context,
        set([1, 4].map(|n| format!("aaaaaaaa-0000-4000-8000-00000000000{n}"))),
        "a duplicate with a context body gives the unit one"
    );
    // Every request named the organization, in the query, as the index is asked for it.
    for request in quickwit.index.requests() {
        let query = request["query"].as_str().expect("a query");
        assert!(query.contains("org_id:\"org-a\""), "{query}");
        assert!(query.contains("entity_type:\"knowledge_unit\""), "{query}");
        assert_eq!(request["max_hits"], 0, "ids, never documents");
    }
    assert_eq!(
        quickwit
            .client()
            .indexed_units("org-none")
            .await
            .expect("empty"),
        IndexedUnits::default()
    );
}

/// More units than one request holds: the organization is split by id prefix, every part is read,
/// and the whole is what the index holds, not a prefix of it.
#[tokio::test]
async fn an_organization_bigger_than_one_request_is_listed_by_prefix() {
    let all = ids(0..60);
    let docs: Vec<Doc> = all
        .iter()
        .enumerate()
        .map(|(n, id)| doc("org-a", id, (n % 3 != 0).then_some("Kontekst.")))
        .collect();
    let quickwit = Quickwit::start(docs).await;

    let units = quickwit
        .client()
        .indexed_units_by("org-a", 7)
        .await
        .expect("listed in parts");

    assert_eq!(units.distinct, set(all.clone()));
    assert_eq!(units.documents, 60);
    assert_eq!(
        units.with_context,
        set(all
            .iter()
            .enumerate()
            .filter(|(n, _)| n % 3 != 0)
            .map(|(_, id)| id.clone()))
    );
    let prefixed = quickwit
        .index
        .requests()
        .iter()
        .filter(|request| {
            request["query"]
                .as_str()
                .is_some_and(|q| q.contains("knowledge_id:"))
        })
        .count();
    assert!(prefixed > 0, "the index was never split by prefix");
    // The whole fits in one request when the request is big enough: one read, no splitting.
    let fresh = Quickwit::start(all.iter().map(|id| doc("org-a", id, None)).collect()).await;
    fresh
        .client()
        .indexed_units("org-a")
        .await
        .expect("one request");
    assert!(fresh
        .index
        .requests()
        .iter()
        .all(|request| !request["query"]
            .as_str()
            .unwrap_or_default()
            .contains("knowledge_id:")));
}

/// A part that does not add up to the whole is an error. An id the prefix query cannot reach (an
/// upper-case one: the raw term keeps its case, and the prefix is lower-case hexadecimal) would
/// otherwise make the index look smaller than it is, and a parity report that reads a smaller
/// index as the whole one is a pass that is not.
#[tokio::test]
async fn ids_that_cannot_be_split_by_prefix_are_an_error_not_a_smaller_count() {
    let mut docs: Vec<Doc> = ids(0..30).iter().map(|id| doc("org-a", id, None)).collect();
    docs.extend((0..5).map(|n| {
        doc(
            "org-a",
            &format!("ZZZZZZZZ-0000-4000-8000-00000000000{n}"),
            None,
        )
    }));
    let quickwit = Quickwit::start(docs).await;

    let error = quickwit
        .client()
        .indexed_units_by("org-a", 7)
        .await
        .expect_err("units that cannot be listed");
    assert!(
        error.to_string().contains("lower-case hexadecimal"),
        "{error}"
    );
    // The same units are listed when one request holds them: no split is needed.
    let units = quickwit
        .client()
        .indexed_units("org-a")
        .await
        .expect("one request");
    assert_eq!(units.distinct.len(), 35);
}

#[tokio::test]
async fn an_index_that_errors_or_ignores_the_aggregation_is_an_error() {
    let quickwit = Quickwit::start(vec![doc("org-a", &ids(0..1)[0], None)]).await;
    quickwit.set_mode(Mode::Status(500));
    assert!(quickwit.client().indexed_units("org-a").await.is_err());
    quickwit.set_mode(Mode::Status(400));
    assert!(quickwit.client().indexed_units("org-a").await.is_err());
    // A Quickwit that answered the count and dropped the aggregation: not an empty index.
    quickwit.set_mode(Mode::IgnoreAggregation);
    let error = quickwit
        .client()
        .indexed_units("org-a")
        .await
        .expect_err("no aggregation in the answer");
    assert!(error.to_string().contains("aggregation"), "{error}");
    // An index that is unreachable.
    let closed = QuickwitClient::new("http://127.0.0.1:1", "dataplane-corpus", INDEX_CONFIG)
        .expect("a client");
    assert!(closed.indexed_units("org-a").await.is_err());
}

// ── the route ───────────────────────────────────────────────────────────────

fn private_key() -> RsaPrivateKey {
    static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    KEY.get_or_init(|| RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key"))
        .clone()
}

fn token(org_id: &str, scopes: &[&str], service: bool) -> String {
    let private_pem = private_key()
        .to_pkcs8_pem(LineEnding::LF)
        .expect("private PEM");
    let subject = if service {
        "service:data-quality"
    } else {
        "operator-1"
    };
    let claims = AdminClaims {
        user_id: (!service).then(|| "operator-1".to_owned()),
        service_id: service.then(|| subject.to_owned()),
        principal_type: if service { "service" } else { "user" }.into(),
        org_id: org_id.into(),
        scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
        issuer: "https://control.example/api/convex-auth".into(),
        audience: "data-plane".into(),
        subject: subject.into(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
            + 300,
    };
    encode(
        &Header::new(Algorithm::RS256),
        &claims,
        &EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
    )
    .expect("token")
}

fn app(pool: sqlx::PgPool, quickwit: QuickwitClient) -> axum::Router {
    let public_pem = private_key()
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .expect("public PEM");
    let verifier = AdminVerifier::from_pem(
        public_pem.as_bytes(),
        "data-plane",
        "https://control.example/api/convex-auth",
    )
    .expect("verifier");
    let ctx = Arc::new(RebuildContext {
        pool,
        quickwit,
        batch_size: 10,
    });
    router_with_store(
        ctx,
        Arc::new(verifier),
        Arc::new(jobs::InMemoryAdminJobStore::new(
            std::time::Duration::ZERO,
            1,
        )),
    )
}

fn unreachable_pool() -> sqlx::PgPool {
    PgPoolOptions::new()
        // A database that is not there fails the measurement, quickly.
        .acquire_timeout(std::time::Duration::from_millis(300))
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .expect("a lazy pool")
}

async fn get(
    app: &axum::Router,
    uri: &str,
    bearer: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut request = Request::builder().method("GET").uri(uri);
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .map(|value| value.to_str().expect("ascii").to_owned());
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .expect("body");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cache,
    )
}

/// The route is behind the verifier and the scope. These need no database: the checks come
/// before anything is read.
#[tokio::test]
async fn the_route_needs_a_verified_bearer_with_the_quality_scope() {
    let quickwit = Quickwit::start(Vec::new()).await;
    let app = app(unreachable_pool(), quickwit.client());
    for uri in ["/admin/index-parity", "/admin/index-parity/documents"] {
        let (status, _, _) = get(&app, uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}: no credential");
        let (status, _, _) = get(&app, uri, Some("not-a-token")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}: not a token");
        // The scopes the rebuild routes use are not this route's: a credential that can rebuild
        // the index cannot read what the quality service reads.
        let rebuild_only = token(
            "org-a",
            &["data:search:rebuild", "data:search:rebuild:global"],
            false,
        );
        let (status, body, _) = get(&app, uri, Some(&rebuild_only)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {body}");
        let (status, _, _) = get(&app, uri, Some(&token("org-a", &[], true))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: no scope at all");
    }
    assert!(
        quickwit.index.requests().is_empty(),
        "the index was read before the credential was checked"
    );
}

/// A measurement that cannot be taken is a 503 with a content-free code, never a report: the
/// quality service reads anything but a 200 as `unavailable`, which is not a pass.
#[tokio::test]
async fn a_measurement_that_cannot_be_taken_is_a_503_and_not_a_report() {
    let quickwit = Quickwit::start(Vec::new()).await;
    let app = app(unreachable_pool(), quickwit.client());
    let bearer = token("org-a", &[parity::SCOPE], true);
    for uri in ["/admin/index-parity", "/admin/index-parity/documents"] {
        let (status, body, cache) = get(&app, uri, Some(&bearer)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert_eq!(body["error"]["code"], "index_parity_unavailable");
        assert!(
            !body.to_string().contains("127.0.0.1"),
            "the reason names a host: {body}"
        );
        assert_eq!(cache, None, "an error is not a report");
    }
}

// ── the route over the real schema ──────────────────────────────────────────

const DATABASE: &str = "MIGRATED_TEST_DATABASE_URL";

async fn migrated_pool() -> sqlx::PgPool {
    let url = std::env::var(DATABASE).expect(
        "MIGRATED_TEST_DATABASE_URL must point at the migrated Data Plane schema \
         (tests/scripts/run-postgres-contract-tests.sh builds one)",
    );
    let name = url
        .rsplit('/')
        .next()
        .and_then(|tail| tail.split('?').next())
        .unwrap_or_default();
    assert!(
        name.ends_with("_test"),
        "refusing to run against database `{name}`"
    );
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect to the migrated test database")
}

struct Seeded {
    /// `(knowledge_id, document_id)` of the units that are eligible, in order.
    eligible: Vec<(String, String)>,
}

/// One organization's corpus, every kind of unit the predicate tells apart: eligible with and
/// without a context, a document that is restricted, a deleted one, a unit not embedded yet.
async fn seed(pool: &sqlx::PgPool, org: &str) -> Seeded {
    let mut eligible = Vec::new();
    let documents = [
        ("internal", false, "done", Some("En kontekst."), 3),
        ("public", false, "done", None, 2),
        ("sensitive", false, "done", Some("Mer kontekst."), 1),
        ("restricted", false, "done", Some("Aldri."), 2),
        ("internal", true, "done", Some("Slettet."), 2),
        ("internal", false, "pending", Some("Ikke embeddet."), 2),
    ];
    for (index, (classification, deleted, embedding, context, units)) in
        documents.iter().enumerate()
    {
        let document_id = format!("{org}-doc-{index}");
        sqlx::query(
            "INSERT INTO documents (document_id, org_id, source, type, title, content, status, \
                    zdr_classification, deleted_at) \
             VALUES ($1, $2, 'test', 'text', 'Tittel', 'Innhold', 'indexed', $3, \
                     CASE WHEN $4 THEN now() ELSE NULL END)",
        )
        .bind(&document_id)
        .bind(org)
        .bind(classification)
        .bind(deleted)
        .execute(pool)
        .await
        .expect("insert a document");
        for unit in 0..*units {
            let knowledge_id =
                format!("{:08x}-{index:04x}-4000-8000-{unit:012x}", rand_suffix(org));
            sqlx::query(
                "INSERT INTO knowledge_units (knowledge_id, document_id, org_id, chunk_index, text, \
                        embedding_status, chunk_context) VALUES ($1, $2, $3, $4, 'tekst', $5, $6)",
            )
            .bind(&knowledge_id)
            .bind(&document_id)
            .bind(org)
            .bind(unit)
            .bind(embedding)
            .bind(context)
            .execute(pool)
            .await
            .expect("insert a unit");
            if !["restricted"].contains(classification) && !deleted && *embedding == "done" {
                eligible.push((knowledge_id, document_id.clone()));
            }
        }
    }
    Seeded { eligible }
}

fn rand_suffix(org: &str) -> u32 {
    org.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}

/// The report, over the real schema and a stand-in index: the organization is the bearer's (an
/// organization named in the query string is not read), the expectation is the adapter's
/// predicate (restricted, deleted and not-yet-embedded units are not expected), and a unit
/// missing from the index, a stale one and a context body that differs are each counted.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_route_measures_the_callers_organization_over_the_real_schema() {
    let pool = migrated_pool().await;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let org = format!("org-par-{}", &suffix[..8]);
    let other = format!("org-oth-{}", &suffix[..8]);
    let mine = seed(&pool, &org).await;
    let theirs = seed(&pool, &other).await;
    assert_eq!(
        mine.eligible.len(),
        6,
        "three documents with 3 + 2 + 1 eligible units"
    );

    // The index holds: the first two eligible units (the first with its context, the second
    // without the context Postgres has), one unit Postgres does not expect, and all of the other
    // organization's units.
    let (first, second) = (&mine.eligible[0].0, &mine.eligible[1].0);
    let mut docs = vec![
        doc(&org, first, Some("En kontekst.")),
        doc(&org, second, None),
        doc(&org, "99999999-9999-4999-8999-999999999999", None),
    ];
    docs.extend(
        theirs
            .eligible
            .iter()
            .map(|(id, _)| doc(&other, id, Some("x"))),
    );
    let quickwit = Quickwit::start(docs).await;
    let app = app(pool.clone(), quickwit.client());
    let bearer = token(&org, &[parity::SCOPE], true);

    // An organization in the query string is ignored: the report is the bearer's.
    let (status, body, cache) = get(
        &app,
        &format!("/admin/index-parity?org_id={other}"),
        Some(&bearer),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(cache.as_deref(), Some("no-store"));
    let report: ParityReport = serde_json::from_value(body.clone()).expect("the contract's shape");
    assert_eq!(report.contract, "index-parity-v1");
    assert_eq!(report.org_id, org);
    assert_eq!(report.postgres.eligible_units, 6);
    // Eligible with a context: documents 0 (3 units) and 2 (1 unit); document 1 has none.
    assert_eq!(report.postgres.eligible_with_context, 4);
    assert_eq!(report.quickwit.documents, 3);
    assert_eq!(report.quickwit.distinct_units, 3);
    assert_eq!(report.quickwit.with_context_body, 1);
    assert_eq!(report.difference.missing_units, 4);
    assert_eq!(report.difference.unexpected_units, 1);
    // The second unit is the first document's: its Postgres context is not blank and its index
    // body is. (The first unit's context is indexed, so it does not count.)
    assert_eq!(report.difference.context_body_missing, 1, "{body}");
    assert_eq!(report.difference.context_body_unexpected, 0);
    assert!(chrono::DateTime::parse_from_rfc3339(&report.checked_at).is_ok());
}

/// The listing of documents to re-announce: the documents of the caller's organization with a
/// unit missing from the index or a context that differs, and nothing of another organization.
#[tokio::test]
#[ignore = "requires MIGRATED_TEST_DATABASE_URL: the migrated Data Plane schema"]
async fn the_route_names_the_documents_to_re_announce_and_no_content() {
    let pool = migrated_pool().await;
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let org = format!("org-doc-{}", &suffix[..8]);
    let mine = seed(&pool, &org).await;
    // The index holds only the unit of the third document.
    let held = &mine.eligible[5];
    let quickwit = Quickwit::start(vec![doc(&org, &held.0, Some("Mer kontekst."))]).await;
    let app = app(pool, quickwit.client());

    let (status, body, _) = get(
        &app,
        "/admin/index-parity/documents",
        Some(&token(&org, &[parity::SCOPE], false)),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["contract"], parity::DOCUMENTS_CONTRACT);
    assert_eq!(body["documents_with_drift"], 2);
    assert_eq!(body["truncated"], false);
    let named: Vec<&str> = body["documents"]
        .as_array()
        .expect("documents")
        .iter()
        .map(|document| document["document_id"].as_str().expect("an id"))
        .collect();
    assert_eq!(named, [format!("{org}-doc-0"), format!("{org}-doc-1")]);
    assert_eq!(body["documents"][0]["missing_units"], 3);
    let text = body.to_string();
    for secret in ["tekst", "kontekst", "Tittel", "Innhold"] {
        assert!(
            !text.contains(secret),
            "the listing carries content: {text}"
        );
    }
}

// ── a real Quickwit ─────────────────────────────────────────────────────────

/// The same checks against a REAL Quickwit 0.8.1, which is what the stand-in above was written
/// from. Run by the live proof (and by hand: `docker run -p 127.0.0.1:57280:7280
/// quickwit/quickwit:0.8.1 run`, `QUICKWIT_TEST_URL=http://127.0.0.1:57280`). It creates its own
/// index, from the shipped index configuration with only the id and the storage changed.
#[tokio::test]
#[ignore = "requires QUICKWIT_TEST_URL: a throwaway Quickwit 0.8.1 (see the test's documentation)"]
async fn a_real_quickwit_answers_the_parity_check_as_the_stand_in_does() {
    let base = std::env::var("QUICKWIT_TEST_URL").expect("QUICKWIT_TEST_URL");
    let index_id = format!(
        "parity-test-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    let config = std::fs::read_to_string(INDEX_CONFIG)
        .expect("the shipped index configuration")
        .replace('\r', "")
        .replace(
            "index_id: dataplane-corpus",
            &format!("index_id: {index_id}"),
        )
        .replace(
            "index_uri: s3://quickwit/indexes/dataplane-corpus",
            &format!("index_uri: file:///quickwit/qwdata/indexes/{index_id}"),
        );
    let path = std::env::temp_dir().join(format!("{index_id}.yaml"));
    std::fs::write(&path, config).expect("write the test index configuration");
    let client = QuickwitClient::new(&base, &index_id, &path).expect("a client");
    client.ensure_index().await.expect("create the test index");

    // Phase one: 60 units of one organization (a third with a context body), a second generation
    // of one of them, and another organization's. Read in one request, and in many (seven terms
    // at a time: the prefix partitioning against the real index).
    let all = ids(0..60);
    let mut documents: Vec<quickwit_adapter_rs::model::QuickwitDocument> = Vec::new();
    for (n, id) in all.iter().enumerate() {
        let body = (n % 3 == 1).then(|| "Kontekst om fisk.".to_owned());
        documents.push(unit_document("org-a", id, body));
    }
    documents.push(unit_document(
        "org-a",
        &all[1],
        Some("Gammel generasjon.".to_owned()),
    ));
    documents.extend(
        ids(100..110)
            .iter()
            .map(|id| unit_document("org-b", id, Some("x".into()))),
    );
    client
        .ingest_rebuild_batch(&documents)
        .await
        .expect("ingest");
    wait_for(&client, "org-a", 61).await;

    let want_distinct = set(all.iter().cloned());
    let want_context = set(all
        .iter()
        .enumerate()
        .filter(|(n, _)| n % 3 == 1)
        .map(|(_, id)| id.clone()));
    for terms in [parity_terms_default(), 7] {
        let units = client
            .indexed_units_by("org-a", terms)
            .await
            .unwrap_or_else(|error| panic!("listed with {terms} terms per request: {error:#}"));
        assert_eq!(units.documents, 61, "the index counts the duplicate");
        assert_eq!(units.distinct, want_distinct, "{terms} terms per request");
        assert_eq!(
            units.with_context, want_context,
            "{terms} terms per request"
        );
    }

    // Phase two: a unit whose id has capital letters. The index reads the lower-cased field, so a
    // read in one request returns it lower-cased, and a read that has to split by prefix cannot
    // reach it (the prefix query is on the raw term, which keeps its case) and says so.
    let capitals = "0000AAAA-0000-4000-8000-000000000000";
    client
        .ingest_rebuild_batch(&[unit_document(
            "org-a",
            capitals,
            Some("Stor bokstav.".to_owned()),
        )])
        .await
        .expect("ingest the capitals");
    wait_for(&client, "org-a", 62).await;
    let units = client.indexed_units("org-a").await.expect("one request");
    assert!(
        units.distinct.contains(&capitals.to_lowercase()),
        "lower-cased by the index"
    );
    assert!(units.with_context.contains(&capitals.to_lowercase()));
    let error = client
        .indexed_units_by("org-a", 7)
        .await
        .expect_err("a unit the prefix cannot reach");
    assert!(
        error.to_string().contains("lower-case hexadecimal"),
        "{error:#}"
    );
    let _ = std::fs::remove_file(&path);
}

/// Wait for the index to commit what was ingested (it commits within its timeout).
async fn wait_for(client: &QuickwitClient, org: &str, documents: u64) {
    for _ in 0..60 {
        if client.count_documents(Some(org)).await.unwrap_or(0) == documents {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("the index never held {documents} documents of {org}");
}

fn parity_terms_default() -> u64 {
    quickwit_adapter_rs::quickwit::TERMS_PER_REQUEST
}

fn unit_document(
    org: &str,
    knowledge_id: &str,
    context_body: Option<String>,
) -> quickwit_adapter_rs::model::QuickwitDocument {
    quickwit_adapter_rs::model::QuickwitDocument {
        timestamp: chrono::Utc::now(),
        rebuild_batch_id: None,
        entity_type: "knowledge_unit".into(),
        org_id: org.into(),
        document_id: Some("doc".into()),
        knowledge_id: Some(knowledge_id.into()),
        source_object_id: None,
        chunk_index: Some(0),
        source: Some("test".into()),
        title: Some("Tittel".into()),
        body: "En tekst.".into(),
        context_body,
        site_id: None,
        drive_id: None,
        item_id: None,
        path: None,
        mime_type: None,
        modified_at: None,
        quickxor_hash: None,
        sha1_hash: None,
        content_hash: None,
        acl_tags: Vec::new(),
        metadata: json!({}),
    }
}
