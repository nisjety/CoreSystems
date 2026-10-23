//! Hermetic fixture site for the crawl tests (WS14.S1).
//!
//! [`FixtureSiteDriver`] is a [`Driver`] that answers from memory. It opens no
//! socket, so it is not subject to the connection-level SSRF guard
//! (`resolve_public_url`), and a fixture host name needs no DNS. It serves what a
//! test configures, records every request it receives with the instant it
//! arrived, and answers 404 for a URL it was not told about. The 404 is recorded
//! as well, so a test can assert that the crawler asked for something the site
//! did not have, not only what it never asked for.
//!
//! Redirects are followed hop by hop, as a real client does. Every hop is
//! recorded and `final_url` on the returned response is the last hop. A loop
//! stops after [`MAX_REDIRECTS`] hops and returns the last redirect response.
//!
//! Routes are keyed by the full URL (scheme, host, port, path, query). The
//! fragment is not sent on the wire, so it is ignored both for the lookup and
//! for the recorded URL.
//!
//! The live-stack counterpart is `deploy/compose/crawl-fixture/` (nginx). The
//! content of [`FixtureSiteDriver::standard_site`] mirrors that directory, so a
//! Rust test and a run on the real stack see the same links.
//!
//! Test-only. Nothing in production code may depend on this module.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use url::Url;

/// Redirect hops followed before the driver gives up and returns the last redirect.
pub const MAX_REDIRECTS: usize = 5;

/// Origin of the first fixture host: the crawl seed in the live recipe.
pub const FIXTURE_A: &str = "http://fixture-a.crawl.test";
/// Second name of the first fixture host, served from the same content.
pub const FIXTURE_A_DOCS: &str = "http://docs.fixture-a.crawl.test";
/// Origin of the external fixture host that the index page links to.
pub const FIXTURE_B: &str = "http://fixture-b.crawl.test";

/// Host names used by the standard site, for [`FixtureSiteDriver::requested_urls_on`].
pub const HOST_A: &str = "fixture-a.crawl.test";
pub const HOST_A_DOCS: &str = "docs.fixture-a.crawl.test";
pub const HOST_B: &str = "fixture-b.crawl.test";

/// The index page of the first fixture host. Keep in sync with
/// `deploy/compose/crawl-fixture/a/index.html`.
pub const STANDARD_INDEX_A: &str = "<!doctype html>\n\
<html><head><title>Fixture A</title></head>\n\
<body>\n\
<h1>Fixture A</h1>\n\
<a href=\"/docs/a\">docs</a>\n\
<a href=\"/blog/b\">blog</a>\n\
<a href=\"/private/secret\">private</a>\n\
<img src=\"/img/logo.png\" alt=\"logo\">\n\
<a href=\"mailto:fixture@fixture-a.crawl.test\">mail</a>\n\
<a href=\"http://fixture-b.crawl.test/ext\">external</a>\n\
<a href=\"http://docs.fixture-a.crawl.test/\">docs host</a>\n\
</body></html>\n";

/// `robots.txt` of the first fixture host. Keep in sync with
/// `deploy/compose/crawl-fixture/a/robots.txt`.
pub const STANDARD_ROBOTS_A: &str = "User-agent: *\nDisallow: /private/\nCrawl-delay: 2\n";

/// A configured answer for one URL.
#[derive(Clone, Debug)]
pub struct FixtureReply {
    pub status: u16,
    pub content_type: String,
    /// Extra response headers. `location` is read for redirects.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl FixtureReply {
    /// 200 `text/html`.
    pub fn html(body: impl Into<String>) -> Self {
        Self::with_status(200, "text/html; charset=utf-8", body.into().into_bytes())
    }

    /// 200 `text/plain`.
    pub fn text(body: impl Into<String>) -> Self {
        Self::with_status(200, "text/plain; charset=utf-8", body.into().into_bytes())
    }

    /// Any status with a text body, typed `text/plain`. Used for overrides such
    /// as a 503 on `robots.txt`.
    pub fn text_with_status(status: u16, body: impl Into<String>) -> Self {
        Self::with_status(status, "text/plain; charset=utf-8", body.into().into_bytes())
    }

    /// Any status and any body bytes with an explicit content type.
    pub fn with_status(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: content_type.to_owned(),
            headers: Vec::new(),
            body,
        }
    }

    /// A redirect with `status` to `location`, absolute or relative to the request URL.
    pub fn redirect(status: u16, location: &str) -> Self {
        let mut reply = Self::with_status(status, "text/plain; charset=utf-8", Vec::new());
        reply
            .headers
            .push(("location".to_owned(), location.to_owned()));
        reply
    }
}

/// One request the driver received, in arrival order.
#[derive(Clone, Debug)]
pub struct FixtureRequest {
    /// The URL asked for, fragment removed.
    pub url: Url,
    /// When the request arrived at the driver.
    pub at: Instant,
    /// The status this hop was answered with.
    pub status: u16,
}

#[derive(Default)]
struct SiteState {
    routes: HashMap<String, FixtureReply>,
    log: Vec<FixtureRequest>,
}

/// In-memory site. Cheap to clone: clones share the routes and the request log.
#[derive(Clone, Default)]
pub struct FixtureSiteDriver {
    state: Arc<Mutex<SiteState>>,
}

impl FixtureSiteDriver {
    /// An empty site: every URL answers 404 until one is served.
    pub fn new() -> Self {
        Self::default()
    }

    /// Answers `url` with `reply`. A later call for the same URL replaces the answer.
    pub fn serve(self, url: &str, reply: FixtureReply) -> Self {
        let key = route_key(&parse(url));
        self.lock().routes.insert(key, reply);
        self
    }

    /// The standard fixture site: the content of `deploy/compose/crawl-fixture/a`
    /// (both host names) and `b`, as the live stack serves it.
    ///
    /// `fixture-a` has an index, a `robots.txt` with `Disallow: /private/` and
    /// `Crawl-delay: 2`, the pages `/docs/a`, `/blog/b`, `/private/secret` and an
    /// image. `fixture-b` has only `/ext`. Nothing else is served, so every other
    /// URL answers 404.
    pub fn standard_site() -> Self {
        Self::new()
            .serve(&format!("{FIXTURE_A}/"), FixtureReply::html(STANDARD_INDEX_A))
            .serve(
                &format!("{FIXTURE_A_DOCS}/"),
                FixtureReply::html(STANDARD_INDEX_A),
            )
            .serve(
                &format!("{FIXTURE_A}/robots.txt"),
                FixtureReply::text(STANDARD_ROBOTS_A),
            )
            .serve(
                &format!("{FIXTURE_A}/docs/a"),
                FixtureReply::html(page("Docs A", "<a href=\"/\">home</a>")),
            )
            .serve(
                &format!("{FIXTURE_A}/blog/b"),
                FixtureReply::html(page("Blog B", "<a href=\"/docs/a\">docs</a>")),
            )
            .serve(
                &format!("{FIXTURE_A}/private/secret"),
                FixtureReply::html(page("Secret", "<p>private page</p>")),
            )
            .serve(
                &format!("{FIXTURE_A}/img/logo.png"),
                FixtureReply::with_status(200, "image/png", PNG_SIGNATURE.to_vec()),
            )
            .serve(
                &format!("{FIXTURE_B}/ext"),
                FixtureReply::html(page("External", "<p>external target page</p>")),
            )
    }

    /// Every request received, in arrival order. Includes 404 answers.
    pub fn requests(&self) -> Vec<FixtureRequest> {
        self.lock().log.clone()
    }

    /// The URLs requested on `host`, in arrival order, fragments removed.
    pub fn requested_urls_on(&self, host: &str) -> Vec<String> {
        self.lock()
            .log
            .iter()
            .filter(|request| request.url.host_str() == Some(host))
            .map(|request| request.url.to_string())
            .collect()
    }

    /// Whether `url` (fragment ignored) was requested at least once.
    pub fn was_requested(&self, url: &str) -> bool {
        let key = route_key(&parse(url));
        self.lock()
            .log
            .iter()
            .any(|request| route_key(&request.url) == key)
    }

    fn lock(&self) -> MutexGuard<'_, SiteState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Answers one hop: records it, and returns the configured reply or a 404.
    fn answer(&self, url: &Url) -> FixtureReply {
        let at = Instant::now();
        let mut state = self.lock();
        let reply = state
            .routes
            .get(&route_key(url))
            .cloned()
            .unwrap_or_else(not_found);
        state.log.push(FixtureRequest {
            url: strip_fragment(url),
            at,
            status: reply.status,
        });
        reply
    }
}

#[async_trait]
impl Driver for FixtureSiteDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        let started = Instant::now();
        let mut current = strip_fragment(url);
        let mut hops = 0usize;
        loop {
            let reply = self.answer(&current);
            let location = reply
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("location"))
                .map(|(_, value)| value.clone());
            let next = match location {
                Some(location) if is_redirect(reply.status) && hops < MAX_REDIRECTS => {
                    current.join(&location).ok()
                }
                _ => None,
            };
            match next {
                Some(next) => {
                    current = next;
                    hops += 1;
                }
                None => {
                    let mut headers = vec![("content-type".to_owned(), reply.content_type)];
                    headers.extend(reply.headers);
                    return Ok(FetchResponse {
                        status: reply.status,
                        final_url: current,
                        headers,
                        body: reply.body,
                        duration_ms: u64::try_from(started.elapsed().as_millis())
                            .unwrap_or(u64::MAX),
                        served_by: DriverKind::Static,
                    });
                }
            }
        }
    }
}

/// An HTML page with `body` inside a minimal document.
pub fn page(title: &str, body: &str) -> String {
    format!("<!doctype html><html><head><title>{title}</title></head><body>{body}</body></html>\n")
}

/// The 8-byte PNG signature. The fixture image is not decoded by anything.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn not_found() -> FixtureReply {
    FixtureReply::text_with_status(404, "not found")
}

fn parse(url: &str) -> Url {
    Url::parse(url).unwrap_or_else(|error| panic!("fixture url {url:?} does not parse: {error}"))
}

fn strip_fragment(url: &Url) -> Url {
    let mut url = url.clone();
    url.set_fragment(None);
    url
}

fn route_key(url: &Url) -> String {
    strip_fragment(url).to_string()
}
