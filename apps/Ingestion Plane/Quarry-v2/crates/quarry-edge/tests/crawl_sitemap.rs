//! `POST /v1/internal/crawl/sitemap` (INNH-6a).
//!
//! The orchestrator asks the edge for the URLs a crawl may seed from the site's
//! sitemaps. The site is the standard fixture site (`FixtureSiteDriver`), so no
//! network is used and every request the edge makes shows in the site's log.
//!
//! The route is served with `QUARRY_EDGE__CRAWL_SITEMAP=on`; this binary sets it.
//! `crawl_options_off.rs` covers the default (off).

use std::{net::SocketAddr, sync::Arc};

use hmac::{Hmac, Mac};
use quarry_edge::{internal_auth::InternalSigner, state::AppState};
use quarry_runtime::{
    EventSink, RobotsEnforceMode, RobotsGate, RobotsGateConfig, UnreachablePolicy,
};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::mpsc;

mod support;

use support::fixture_site::{FixtureReply, FixtureSiteDriver, FIXTURE_A, HOST_A};

const RUNTIME_TOKEN: &str = "crawl-sitemap-runtime-token";
const INTERNAL_SECRET: &str = "crawl-sitemap-internal-secret-0123456789";
const ORG: &str = "org_crawl_sitemap_test";

type HmacSha256 = Hmac<Sha256>;

/// `X-Quarry-Run-Sig`: hex(HMAC-SHA256(secret, org_id + "\n" + url)).
fn run_sig(org: &str, url: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(INTERNAL_SECRET.as_bytes()).unwrap();
    mac.update(format!("{org}\n{url}").as_bytes());
    format!("{:x}", mac.finalize().into_bytes())
}

async fn spawn_edge(
    site: FixtureSiteDriver,
    mode: RobotsEnforceMode,
) -> (SocketAddr, FixtureSiteDriver) {
    std::env::set_var("QUARRY_EDGE__RUNTIME_AUTH_TOKEN", RUNTIME_TOKEN);
    std::env::set_var("QUARRY_EDGE__CRAWL_SITEMAP", "on");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(site.clone());
    let config = RobotsGateConfig {
        mode,
        unreachable: UnreachablePolicy::Disallow,
        ..RobotsGateConfig::default()
    };
    let gate = Arc::new(RobotsGate::new(driver.clone(), config));
    let state = AppState {
        internal_signer: Some(Arc::new(InternalSigner::new(INTERNAL_SECRET).unwrap())),
        robots: Some(gate),
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (addr, site)
}

fn crawl_context() -> Value {
    json!({
        "v": 1, "kind": "crawl", "depth": 0, "max_depth": 3,
        "seed_hosts": [HOST_A], "seed_url": format!("{FIXTURE_A}/"),
        "scope": {}, "scope_mode": "enforce", "robots": "respect",
    })
}

async fn post(
    addr: SocketAddr,
    org: &str,
    sig_org: &str,
    token: &str,
    body: Value,
) -> (u16, Value) {
    let url = body["url"].as_str().unwrap_or_default().to_owned();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/internal/crawl/sitemap"))
        .bearer_auth(token)
        .header("x-quarry-run-sig", run_sig(sig_org, &url))
        .json(&{
            let mut body = body;
            body["org_id"] = json!(org);
            body
        })
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

async fn discover(addr: SocketAddr, crawl: Value, max_urls: Option<u32>) -> (u16, Value) {
    let mut body = json!({ "url": format!("{FIXTURE_A}/"), "crawl": crawl });
    if let Some(max) = max_urls {
        body["max_urls"] = json!(max);
    }
    post(addr, ORG, ORG, RUNTIME_TOKEN, body).await
}

fn xml(body: &str) -> FixtureReply {
    FixtureReply::with_status(200, "application/xml", body.as_bytes().to_vec())
}

fn urlset(locs: &[&str]) -> String {
    let entries: String = locs
        .iter()
        .map(|l| format!("<url><loc>{l}</loc></url>"))
        .collect();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">{entries}</urlset>")
}

fn index(locs: &[&str]) -> String {
    let entries: String = locs
        .iter()
        .map(|l| format!("<sitemap><loc>{l}</loc></sitemap>"))
        .collect();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><sitemapindex>{entries}</sitemapindex>")
}

fn urls_of(body: &Value) -> Vec<String> {
    body["urls"]
        .as_array()
        .expect("urls is an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn robots_with(lines: &str) -> FixtureReply {
    FixtureReply::text(format!("User-agent: *\nDisallow: /private/\n{lines}"))
}

/// The standard site with a sitemap named by robots.txt: an orphan page only the
/// sitemap lists, a robots-disallowed page, an image, an external page, repeats and junk.
fn site_with_listed_sitemap() -> FixtureSiteDriver {
    FixtureSiteDriver::standard_site()
        .serve(
            &format!("{FIXTURE_A}/robots.txt"),
            robots_with("Sitemap: http://fixture-a.crawl.test/maps/main.xml\n"),
        )
        .serve(
            &format!("{FIXTURE_A}/maps/main.xml"),
            xml(&urlset(&[
                "http://fixture-a.crawl.test/docs/a",
                "http://fixture-a.crawl.test/orphan",
                "http://fixture-a.crawl.test/docs/a#top",
                "http://FIXTURE-A.crawl.test:80/docs/a",
                "http://fixture-a.crawl.test/private/secret",
                "http://fixture-a.crawl.test/img/logo.png",
                "http://fixture-b.crawl.test/ext",
                "not a url",
                "http://fixture-a.crawl.test/docs/a?x=1&amp;y=2",
            ])),
        )
}

#[tokio::test]
async fn discovery_returns_scoped_deduped_robots_filtered_urls_and_finds_the_orphan() {
    let (addr, site) = spawn_edge(site_with_listed_sitemap(), RobotsEnforceMode::Enforce).await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        urls_of(&body),
        vec![
            "http://fixture-a.crawl.test/docs/a",
            "http://fixture-a.crawl.test/orphan",
            "http://fixture-a.crawl.test/docs/a?x=1&y=2",
        ],
        "{body}"
    );
    assert_eq!(body["discovered"], 9);
    assert_eq!(body["sitemaps_fetched"], 1);
    assert_eq!(body["nested_fetched"], 0);
    let skipped = &body["skipped"];
    assert_eq!(skipped["duplicate"], 2, "{body}");
    assert_eq!(skipped["robots_disallowed"], 1, "{body}");
    assert_eq!(
        skipped["out_of_scope"], 2,
        "the image and the external page: {body}"
    );
    assert_eq!(skipped["invalid"], 1);
    assert_eq!(skipped["cap"], 0);
    assert_eq!(body["truncated"], false);
    // Nothing the sitemap lists was fetched: the edge reads sitemap files, never pages.
    assert!(!site.was_requested("http://fixture-a.crawl.test/orphan"));
    assert!(!site.was_requested("http://fixture-a.crawl.test/private/secret"));
    assert!(site.was_requested("http://fixture-a.crawl.test/maps/main.xml"));
}

#[tokio::test]
async fn observe_mode_robots_drops_nothing() {
    let (addr, _site) = spawn_edge(site_with_listed_sitemap(), RobotsEnforceMode::Observe).await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert!(
        urls_of(&body).contains(&"http://fixture-a.crawl.test/private/secret".to_owned()),
        "observe never drops on robots.txt's word: {body}"
    );
    assert_eq!(body["skipped"]["robots_disallowed"], 0);
}

#[tokio::test]
async fn the_cap_is_max_urls_and_counts_the_rest() {
    let site = FixtureSiteDriver::standard_site().serve(
        &format!("{FIXTURE_A}/sitemap.xml"),
        xml(&urlset(&[
            "http://fixture-a.crawl.test/p/1",
            "http://fixture-a.crawl.test/p/2",
            "http://fixture-a.crawl.test/p/3",
            "http://fixture-a.crawl.test/p/4",
        ])),
    );
    let (addr, _site) = spawn_edge(site, RobotsEnforceMode::Enforce).await;
    let (status, body) = discover(addr, crawl_context(), Some(2)).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        urls_of(&body),
        vec![
            "http://fixture-a.crawl.test/p/1",
            "http://fixture-a.crawl.test/p/2"
        ]
    );
    assert_eq!(body["skipped"]["cap"], 2);
    assert_eq!(body["discovered"], 4);
}

/// A gzip file of `data` made of one stored block, so the test needs no compressor.
fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut crc: u32 = 0xFFFF_FFFF;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    let crc = !crc;
    let len = u16::try_from(data.len()).expect("one stored block");
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    out.push(1);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&(!len).to_le_bytes());
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    out
}

#[tokio::test]
async fn a_gzip_sitemap_behind_an_index_is_read_and_the_fallback_is_sitemap_xml() {
    let site = FixtureSiteDriver::standard_site()
        .serve(
            &format!("{FIXTURE_A}/sitemap.xml"),
            xml(&index(&[
                "http://fixture-a.crawl.test/s1.xml.gz",
                "http://fixture-a.crawl.test/s2.xml",
            ])),
        )
        .serve(
            &format!("{FIXTURE_A}/s1.xml.gz"),
            FixtureReply::with_status(
                200,
                "application/gzip",
                gzip_stored(urlset(&["http://fixture-a.crawl.test/gz/1"]).as_bytes()),
            ),
        )
        .serve(
            &format!("{FIXTURE_A}/s2.xml"),
            xml(&urlset(&["http://fixture-a.crawl.test/plain/1"])),
        );
    let (addr, _site) = spawn_edge(site, RobotsEnforceMode::Enforce).await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        urls_of(&body),
        vec![
            "http://fixture-a.crawl.test/gz/1",
            "http://fixture-a.crawl.test/plain/1"
        ]
    );
    assert_eq!(body["sitemaps_fetched"], 3);
    assert_eq!(body["nested_fetched"], 2);
}

#[tokio::test]
async fn nesting_is_bounded_and_a_missing_sitemap_is_an_empty_answer() {
    // Level 0 index -> level 1 index -> level 2 index -> level 3 urlset: the last is never read.
    let site = FixtureSiteDriver::standard_site()
        .serve(
            &format!("{FIXTURE_A}/sitemap.xml"),
            xml(&index(&["http://fixture-a.crawl.test/l1.xml"])),
        )
        .serve(
            &format!("{FIXTURE_A}/l1.xml"),
            xml(&index(&["http://fixture-a.crawl.test/l2.xml"])),
        )
        .serve(
            &format!("{FIXTURE_A}/l2.xml"),
            xml(&index(&["http://fixture-a.crawl.test/l3.xml"])),
        )
        .serve(
            &format!("{FIXTURE_A}/l3.xml"),
            xml(&urlset(&["http://fixture-a.crawl.test/too-deep"])),
        );
    let (addr, site) = spawn_edge(site, RobotsEnforceMode::Enforce).await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert!(urls_of(&body).is_empty());
    assert!(
        !site.was_requested("http://fixture-a.crawl.test/l3.xml"),
        "level 3 is past the bound"
    );

    // No sitemap at all: a 404 on /sitemap.xml is an answer, not an error.
    let (addr, _site) = spawn_edge(
        FixtureSiteDriver::standard_site(),
        RobotsEnforceMode::Enforce,
    )
    .await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert!(urls_of(&body).is_empty());
    assert_eq!(body["skipped"]["sitemaps_unavailable"], 1);
}

#[tokio::test]
async fn a_sitemap_on_another_host_or_an_internal_address_is_never_fetched() {
    let site = FixtureSiteDriver::standard_site()
        .serve(
            &format!("{FIXTURE_A}/robots.txt"),
            robots_with(
                "Sitemap: http://evil.crawl.test/sitemap.xml\n\
                 Sitemap: http://169.254.169.254/latest/meta-data/\n\
                 Sitemap: http://fixture-a.crawl.test/ok.xml\n",
            ),
        )
        .serve(
            &format!("{FIXTURE_A}/ok.xml"),
            xml(&index(&[
                "http://evil.crawl.test/child.xml",
                "http://fixture-a.crawl.test/ok-child.xml",
            ])),
        )
        .serve(
            &format!("{FIXTURE_A}/ok-child.xml"),
            xml(&urlset(&["http://fixture-a.crawl.test/fine"])),
        )
        .serve(
            "http://evil.crawl.test/sitemap.xml",
            xml(&urlset(&["http://evil.crawl.test/x"])),
        )
        .serve(
            "http://evil.crawl.test/child.xml",
            xml(&urlset(&["http://evil.crawl.test/y"])),
        );
    let (addr, site) = spawn_edge(site, RobotsEnforceMode::Enforce).await;
    let (status, body) = discover(addr, crawl_context(), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(urls_of(&body), vec!["http://fixture-a.crawl.test/fine"]);
    assert_eq!(
        body["skipped"]["sitemaps_refused"], 3,
        "two robots lines and one child of an index: {body}"
    );
    assert!(
        site.requested_urls_on("evil.crawl.test").is_empty(),
        "the other host was never asked"
    );
    assert!(site.requested_urls_on("169.254.169.254").is_empty());
}

#[tokio::test]
async fn the_route_checks_the_token_the_org_binding_and_the_body() {
    let (addr, _site) = spawn_edge(site_with_listed_sitemap(), RobotsEnforceMode::Enforce).await;
    let base = json!({ "url": format!("{FIXTURE_A}/"), "crawl": crawl_context() });

    let (status, _) = post(addr, ORG, ORG, "not-the-token", base.clone()).await;
    assert_eq!(status, 401, "a wrong runtime token");

    let (status, body) = post(addr, ORG, "org_somebody_else", RUNTIME_TOKEN, base.clone()).await;
    assert_eq!(status, 401, "a signature for another org: {body}");

    let mut no_scope = base.clone();
    no_scope.as_object_mut().unwrap().remove("crawl");
    let (status, body) = post(addr, ORG, ORG, RUNTIME_TOKEN, no_scope).await;
    assert_eq!(status, 400, "{body}");

    let mut bad_scope = base.clone();
    bad_scope["crawl"]["scope"] = json!({ "allow_subdomian": true });
    let (status, body) = post(addr, ORG, ORG, RUNTIME_TOKEN, bad_scope).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["details"]["fields"][0]["path"], "crawl");

    let mut zdr = base.clone();
    zdr["zdr"] = json!(true);
    let (status, body) = post(addr, ORG, ORG, RUNTIME_TOKEN, zdr).await;
    assert_eq!(status, 400, "no ZDR path: {body}");

    let (status, body) = post(
        addr,
        ORG,
        ORG,
        RUNTIME_TOKEN,
        json!({ "url": "ftp://x.test/", "crawl": crawl_context() }),
    )
    .await;
    assert_eq!(status, 400, "{body}");

    let (status, body) = post(addr, "  ", "  ", RUNTIME_TOKEN, base).await;
    assert_eq!(status, 400, "an empty org: {body}");
}
