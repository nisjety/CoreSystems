//! `GET /v1/artifacts` on the durable filesystem backend (R7, WS5.F).
//!
//! `FilesystemStore` did not override `list`, so the listing was always empty on
//! the fs backend (the trait default), and the rebuilt-edge evidence run could only
//! prove "ZDR adds nothing" by counting files. These tests list through the real
//! route with real auth (as in `scrape_records_durable.rs`): what a scrape wrote is
//! listed, a restarted edge on the same directory lists the same artifacts in the
//! same order, another organization sees none of them, a ZDR scrape adds nothing,
//! and `limit`/`cursor`/`sort` page through them without overlap.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::artifact_store::FilesystemStore;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;

mod support;

const GOLDEN: &str = include_str!("fixtures/auth-core/quarry-token-claims.golden.json");

/// The deployed values of the quarry-edge service (`apps/Ingestion Plane/docker-compose.yml`).
const DEPLOYED_AUDIENCE: &str = "quarry";
const DEPLOYED_ISSUER: &str = "http://localhost:3011/api/convex-auth";
const KID: &str = "artifacts-fs-list-kid";

const PAGE: &str = "<html><head><title>Durable page</title></head><body><main><p>Durable record text that is long enough to read as page content.</p></main></body></html>";

// ── real-token fixture (the same shape as zdr_claim_floor.rs) ───────────────

struct TokenFixture {
    private_pem: String,
}

fn fixture() -> &'static TokenFixture {
    static FIXTURE: OnceLock<TokenFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        use base64::Engine;
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::traits::PublicKeyParts;

        let mut rng = rand::thread_rng();
        let private = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("rsa keygen");
        let public = rsa::RsaPublicKey::from(&private);
        let private_pem = private
            .to_pkcs8_pem(LineEnding::LF)
            .expect("encode private pem")
            .to_string();
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
        let jwks = json!({
            "keys": [{ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KID, "n": n, "e": e }]
        });

        // The JWKS endpoint lives on its own runtime so it outlives the per-test
        // runtimes `#[tokio::test]` creates.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("jwks runtime");
            runtime.block_on(async move {
                let server = MockServer::start().await;
                Mock::given(method("GET"))
                    .and(path("/jwks"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(jwks))
                    .mount(&server)
                    .await;
                tx.send(server.uri()).expect("publish jwks uri");
                std::future::pending::<()>().await;
            });
        });
        let jwks_uri = rx.recv().expect("jwks uri");

        std::env::remove_var("QUARRY_EDGE_AUTH_DEV_BYPASS");
        std::env::remove_var("AUTH_CORE_AUDIENCE_OPTIONAL");
        std::env::set_var("AUTH_CORE_JWKS_URL", format!("{jwks_uri}/jwks"));
        std::env::set_var("AUTH_CORE_AUDIENCE", DEPLOYED_AUDIENCE);
        std::env::set_var("AUTH_CORE_ISSUER", DEPLOYED_ISSUER);

        TokenFixture { private_pem }
    })
}

fn unix_now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_secs(),
    )
    .expect("unix time fits i64")
}

/// A token whose claims are the golden case `case`, freshly timed, after `tweak`.
fn token(case: &str, tweak: impl FnOnce(&mut serde_json::Map<String, Value>)) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let golden: Value = serde_json::from_str(GOLDEN).expect("golden is JSON");
    let entry = &golden["cases"][case];
    assert!(entry.is_object(), "golden has no case `{case}`");
    let mut claims = entry["claims"].as_object().expect("claims object").clone();
    let ttl = entry["ttl_seconds"].as_i64().expect("ttl_seconds");
    let now = unix_now();
    claims.insert("iat".into(), json!(now));
    claims.insert("nbf".into(), json!(now - 5));
    claims.insert("exp".into(), json!(now + ttl));
    tweak(&mut claims);

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    let key = EncodingKey::from_rsa_pem(fixture().private_pem.as_bytes()).expect("signing key");
    encode(&header, &Value::Object(claims), &key).expect("sign token")
}

/// The organization that writes the records in these tests (not ZDR).
fn org_a() -> String {
    token("user_in_standard_org", |_| {})
}

/// A second, unrelated, non-ZDR organization.
fn org_b() -> String {
    token("user_in_standard_org", |claims| {
        claims.insert("org_id".into(), json!("org-second-golden"));
        claims.insert("user_id".into(), json!("user-second-golden"));
        claims.insert("sub".into(), json!("user-second-golden"));
    })
}

/// A user of an organization under ZDR.
fn zdr_org() -> String {
    token("user_in_zdr_org", |_| {})
}

// ── the edge, built on a directory ──────────────────────────────────────────

/// Serves every URL with `PAGE`. No network.
struct FixedPage;

#[async_trait]
impl Driver for FixedPage {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: PAGE.as_bytes().to_vec(),
            duration_ms: 1,
            served_by: DriverKind::Static,
        })
    }
}


struct Edge {
    addr: SocketAddr,
    client: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
}

impl Edge {
    /// An edge whose artifacts are the filesystem store under `root`. Each call
    /// builds a new store and a new router: a second call on the same `root` is a
    /// restarted edge.
    async fn start(root: &Path) -> Self {
        fixture();
        let (tx, mut rx) = mpsc::channel::<quarry_core::event::Event>(1024);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let store = FilesystemStore::new(root.to_path_buf()).expect("filesystem artifact store");
        let state = AppState {
            artifacts: Arc::new(store),
            ..support::base_state(Arc::new(FixedPage), EventSink::new(tx))
        };
        let app = quarry_edge::routes::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            client: reqwest::Client::new(),
            task,
        }
    }

    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }

    async fn scrape(&self, bearer: &str, body: Value) -> Value {
        let response = self
            .client
            .post(format!("http://{}/v1/scrape", self.addr))
            .bearer_auth(bearer)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge");
        assert_eq!(response.status(), 200, "scrape must succeed");
        response.json().await.expect("scrape envelope")
    }

    /// `GET /v1/artifacts{query}` as `bearer`: the page (`data`) of a 200.
    async fn list(&self, bearer: &str, query: &str) -> Value {
        let response = self
            .client
            .get(format!("http://{}/v1/artifacts{query}", self.addr))
            .bearer_auth(bearer)
            .send()
            .await
            .expect("request reaches the edge");
        assert_eq!(response.status(), 200, "listing must succeed");
        let body: Value = response.json().await.expect("list envelope");
        body["data"].clone()
    }
}

fn scratch_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("quarry-artifacts-fs-list-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|item| item["artifact_id"].as_str().expect("artifact_id").to_owned())
        .collect()
}

fn scrape_body(path: &str) -> Value {
    json!({ "url": format!("https://page.test/{path}"), "formats": ["markdown"] })
}

#[tokio::test]
async fn the_artifacts_of_a_scrape_are_listed_on_the_fs_backend() {
    let root = scratch_root("listed");
    let edge = Edge::start(&root).await;
    assert!(
        ids(&edge.list(&org_a(), "").await).is_empty(),
        "nothing before the first scrape"
    );
    let scraped = edge.scrape(&org_a(), scrape_body("one")).await;
    let scrape_id = scraped["data"]["scrape_id"].as_str().expect("scrape_id").to_owned();

    let page = edge.list(&org_a(), "").await;
    let listed = ids(&page);
    assert!(listed.contains(&scrape_id), "the scrape record is listed: {page}");
    let record = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["artifact_id"] == scrape_id.as_str())
        .unwrap();
    assert_eq!(record["kind"], "scrape_record");
    assert_eq!(record["org_id"], "org-standard-golden");
    assert!(record["bytes"].as_u64().unwrap() > 0);
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_restarted_edge_lists_what_the_first_one_wrote() {
    let root = scratch_root("restart");
    let edge = Edge::start(&root).await;
    edge.scrape(&org_a(), scrape_body("one")).await;
    edge.scrape(&org_a(), scrape_body("two")).await;
    let before = ids(&edge.list(&org_a(), "").await);
    assert!(before.len() >= 2, "{before:?}");
    edge.stop().await;

    let restarted = Edge::start(&root).await;
    let after = ids(&restarted.list(&org_a(), "").await);
    assert_eq!(after, before, "the same artifacts in the same order after a restart");
    restarted.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn another_organization_lists_none_of_them() {
    let root = scratch_root("isolation");
    let edge = Edge::start(&root).await;
    edge.scrape(&org_a(), scrape_body("one")).await;
    assert!(!ids(&edge.list(&org_a(), "").await).is_empty());

    let foreign = edge.list(&org_b(), "").await;
    assert!(ids(&foreign).is_empty(), "org B sees nothing of org A: {foreign}");

    // B's own artifacts are listed to B and not to A.
    let b_scrape = edge.scrape(&org_b(), scrape_body("b")).await;
    let b_record = b_scrape["data"]["scrape_id"].as_str().expect("scrape_id").to_owned();
    assert!(ids(&edge.list(&org_b(), "").await).contains(&b_record));
    assert!(!ids(&edge.list(&org_a(), "").await).contains(&b_record));
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_zdr_scrape_adds_nothing_to_the_listing() {
    let root = scratch_root("zdr");
    let edge = Edge::start(&root).await;
    edge.scrape(&org_a(), scrape_body("kept")).await;
    let before = ids(&edge.list(&org_a(), "").await);
    assert!(!before.is_empty());

    let zdr = edge
        .scrape(&org_a(), json!({ "url": "https://page.test/zdr", "zdr": true }))
        .await;
    assert!(zdr["data"].get("scrape_id").is_none(), "{zdr}");
    assert_eq!(ids(&edge.list(&org_a(), "").await), before, "a ZDR scrape stores, and so lists, nothing");

    // The organization itself under ZDR (claims, not the body flag) writes nothing either.
    let org_zdr = token("user_in_zdr_org", |claims| {
        claims.insert("org_id".into(), json!("org-standard-golden"));
    });
    edge.scrape(&org_zdr, scrape_body("zdr-claim")).await;
    assert_eq!(ids(&edge.list(&org_a(), "").await), before, "a ZDR claim stores nothing");
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn limit_and_cursor_page_through_the_listing_without_overlap() {
    let root = scratch_root("paging");
    let edge = Edge::start(&root).await;
    for path in ["a", "b", "c"] {
        edge.scrape(&org_a(), scrape_body(path)).await;
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }
    let all = ids(&edge.list(&org_a(), "").await);
    assert!(all.len() >= 3, "{all:?}");

    let mut walked = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..all.len() + 1 {
        let query = match &cursor {
            Some(cursor) => format!("?limit=2&cursor={cursor}"),
            None => "?limit=2".to_owned(),
        };
        let page = edge.list(&org_a(), &query).await;
        assert!(ids(&page).len() <= 2, "{page}");
        walked.extend(ids(&page));
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(walked, all, "the walk visits every artifact once, in the unpaged order");

    let oldest = ids(&edge.list(&org_a(), "?sort=oldest").await);
    let mut reversed = all.clone();
    reversed.reverse();
    assert_eq!(oldest, reversed, "sort=oldest is the reverse of the default");
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}
