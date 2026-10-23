//! A scrape record outlives the edge that wrote it (WS5.F, durability).
//!
//! With the filesystem artifact store, a record written by one edge is read back by
//! id from a NEW edge built on the same directory. The same file also pins the two
//! refusals the record carries: another organization gets not-found (never
//! forbidden), and a ZDR scrape writes no file and returns no id.
//!
//! Real auth, as in `zdr_claim_floor.rs`: every request runs the real `require_auth`
//! against a JWKS endpoint, with the golden claim sets Auth Core mints. The second
//! organization is the standard golden claims with another `org_id`. No dev bypass.

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
const KID: &str = "scrape-records-durable-kid";

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
            captures: None,
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
    /// restarted edge reading the files the first one wrote.
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

    /// Stops serving and drops the router, its state and the store.
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

    async fn read(&self, bearer: &str, id: &str) -> reqwest::Response {
        self.client
            .get(format!("http://{}/v1/scrape/{id}", self.addr))
            .bearer_auth(bearer)
            .send()
            .await
            .expect("request reaches the edge")
    }
}

fn scratch_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "quarry-scrape-records-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    root
}

/// Every regular file under `dir`, at any depth.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// Files whose name contains `needle` (a record is `scrape_record.json`).
fn named_files(dir: &Path, needle: &str) -> usize {
    files_under(dir)
        .iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(needle))
        })
        .count()
}

fn scrape_body() -> Value {
    json!({ "url": "https://page.test/durable", "formats": ["markdown"] })
}

#[tokio::test]
async fn a_scrape_record_is_read_back_by_id_from_a_restarted_edge() {
    let root = scratch_root("restart");
    let edge = Edge::start(&root).await;
    let original = edge.scrape(&org_a(), scrape_body()).await;
    let id = original["data"]["scrape_id"].as_str().expect("scrape_id").to_owned();
    let fingerprint = original["data"]["fingerprint"].as_str().expect("fingerprint").to_owned();
    assert_eq!(named_files(&root, "scrape_record"), 1, "one record file on disk");
    edge.stop().await;

    // A new edge, a new store, the same directory.
    let restarted = Edge::start(&root).await;
    let response = restarted.read(&org_a(), &id).await;
    assert_eq!(response.status(), 200, "the record survives the restart");
    assert_eq!(response.headers()["cache-control"], "private, max-age=30");
    let body: Value = response.json().await.expect("record envelope");
    assert_eq!(body["data"]["fingerprint"], fingerprint.as_str());
    assert_eq!(body["data"]["scrape_id"], id.as_str());
    assert_eq!(body["data"]["effective_options"]["formats"], json!(["markdown"]));
    assert!(body["data"].get("markdown").is_none(), "inline text is never stored");
    restarted.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_zdr_scrape_returns_no_id_and_writes_no_file() {
    let root = scratch_root("zdr");
    let edge = Edge::start(&root).await;
    // A non-ZDR scrape first, so the directory holds files and the ZDR one has to
    // leave them exactly as they are.
    let stored = edge.scrape(&org_a(), scrape_body()).await;
    assert!(stored["data"]["scrape_id"].is_string());
    let files_before = files_under(&root).len();
    let records_before = named_files(&root, "scrape_record");

    let zdr = edge
        .scrape(
            &org_a(),
            json!({ "url": "https://page.test/zdr", "zdr": true }),
        )
        .await;
    assert!(zdr["data"].get("scrape_id").is_none(), "{zdr}");
    assert_eq!(files_under(&root).len(), files_before, "a ZDR scrape writes no file");
    assert_eq!(named_files(&root, "scrape_record"), records_before);
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn another_organization_gets_not_found_and_a_zdr_reader_is_refused() {
    let root = scratch_root("isolation");
    let edge = Edge::start(&root).await;
    let original = edge.scrape(&org_a(), scrape_body()).await;
    let id = original["data"]["scrape_id"].as_str().expect("scrape_id").to_owned();

    let foreign = edge.read(&org_b(), &id).await;
    assert_eq!(foreign.status(), 404, "a foreign id is not found, never forbidden");
    let body: Value = foreign.json().await.expect("error envelope");
    assert_eq!(body["error"]["code"], "NOT_FOUND");

    let zdr = edge.read(&zdr_org(), &id).await;
    assert_eq!(zdr.status(), 403, "a ZDR reader has no read path, even to an earlier record");

    let owner = edge.read(&org_a(), &id).await;
    assert_eq!(owner.status(), 200, "the owner still reads it");
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}

/// The owning organization turns ZDR on after the record was written: the same `org_id`
/// now arrives with `zdr: true`. The refusal is per request, so the owner's own reads
/// are refused too, while another member of the same organization who is not under ZDR
/// still reads the record, because the read is organization-scoped.
#[tokio::test]
async fn a_record_written_before_the_organization_turned_zdr_is_refused_to_its_readers() {
    let root = scratch_root("zdr-same-org");
    let edge = Edge::start(&root).await;
    let original = edge.scrape(&org_a(), scrape_body()).await;
    let id = original["data"]["scrape_id"]
        .as_str()
        .expect("scrape_id")
        .to_owned();

    // The same organization as the record (org-standard-golden), now under ZDR.
    let turned_zdr = token("user_in_zdr_org", |claims| {
        claims.insert("org_id".into(), json!("org-standard-golden"));
    });
    let refused = edge.read(&turned_zdr, &id).await;
    assert_eq!(
        refused.status(),
        403,
        "a ZDR reader of the owning organization is refused"
    );
    let body: Value = refused.json().await.expect("error envelope");
    assert_eq!(body["error"]["code"], "FORBIDDEN");

    // Control: another member of the same organization, not under ZDR, still reads it.
    let member = token("user_in_standard_org", |claims| {
        claims.insert("user_id".into(), json!("user-member-golden"));
        claims.insert("sub".into(), json!("user-member-golden"));
    });
    let read = edge.read(&member, &id).await;
    assert_eq!(
        read.status(),
        200,
        "the read is organization-scoped, not author-scoped"
    );
    edge.stop().await;
    let _ = std::fs::remove_dir_all(&root);
}
