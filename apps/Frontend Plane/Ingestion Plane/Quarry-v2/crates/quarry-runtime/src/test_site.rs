//! Local dynamic test site for agent-loop e2e fixtures.
//!
//! Exposes a tiny in-process HTTP server with three pages that exercise the
//! key agent-loop branches:
//!
//! - `GET /` — landing page with a "Sign In" link to `/login`
//! - `GET /login` — login form (cookie-set on submit)
//! - `POST /login` — accepts `username=alice&password=secret`, sets `auth=alice`
//! - `GET /dashboard` — gated; requires `auth` cookie, otherwise redirects to /login
//! - `GET /search?q=...` — returns structured results echoing the query
//! - `GET /otp-login` — E3.1-d one-time-code step: exactly one visible required
//!   non-password input; `POST /otp-login` accepts [`FIXTURE_OTP_CODE`]
//! - `GET /otp-login/ambiguous` — two code inputs, so the E3.1-c preconditions refuse it
//! - `GET /downloads` — W3-E3.4-6: one `download` link ("Get report") to `report.txt`
//! - `GET /downloads/unsolicited` — the same link, clicked by the page itself on load
//!   (an unsolicited download a governed browser must deny)
//! - `GET /downloads/report.txt` — [`FIXTURE_DOWNLOAD_TEXT`] as an attachment
//! - `GET /downloads/report.docx` — an Office (OOXML zip) attachment the download
//!   admission policy refuses
//!
//! Used by integration tests to drive an `AgentLoop` against a deterministic
//! local site (avoiding the live-network flakiness of using example.com).
//!
//! Spawn it from a test:
//!
//! ```ignore
//! let site = TestSite::start().await;
//! let url = site.url();
//! // run agent loop pointed at `url`
//! site.shutdown().await;
//! ```
//!
//! The server binds 127.0.0.1:0 to grab an ephemeral port — concurrent tests
//! never collide.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use axum::{
    extract::{Form, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Router,
};
use serde::Deserialize;

#[derive(Clone, Default)]
struct SiteState {
    /// In-memory request log for assertions in tests.
    log: Arc<tokio::sync::Mutex<Vec<String>>>,
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct OtpForm {
    code: String,
}

/// E3.1-d: the one-time code the `/otp-login` fixture accepts. Pair it with
/// `otp_source::StubOtpSource` in tests; it is a fixture value, not a secret.
pub const FIXTURE_OTP_CODE: &str = "482915";

/// W3-E3.4-6: the bytes of `/downloads/report.txt`. The Chromium proof in
/// quarry-browser (`governed_download_*`) serves the same bytes from a `data:`
/// link, since the driver's egress guard refuses loopback navigation.
pub const FIXTURE_DOWNLOAD_TEXT: &[u8] = b"quarry download fixture\n";

/// W3-E3.4-6: the accessible name of the fixture's download link.
pub const FIXTURE_DOWNLOAD_LINK: &str = "Get report";

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

pub struct TestSite {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
    state: SiteState,
}

impl TestSite {
    /// Start the server on an ephemeral port. The returned handle owns the
    /// server task and must be shut down to release the socket.
    pub async fn start() -> Self {
        let state = SiteState::default();
        let app = Self::router(state.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Self {
            addr,
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
            state,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    pub fn login_url(&self) -> String {
        format!("http://{}/login", self.addr)
    }

    pub fn dashboard_url(&self) -> String {
        format!("http://{}/dashboard", self.addr)
    }

    /// E3.1-d one-time-code step of the login fixture.
    pub fn otp_login_url(&self) -> String {
        format!("http://{}/otp-login", self.addr)
    }

    /// W3-E3.4-6: the page with one governed download link.
    pub fn downloads_url(&self) -> String {
        format!("http://{}/downloads", self.addr)
    }

    /// W3-E3.4-6: the page that starts a download by itself on load.
    pub fn unsolicited_download_url(&self) -> String {
        format!("http://{}/downloads/unsolicited", self.addr)
    }

    /// W3-E3.4-6: one of the fixture files (`report.txt`, `report.docx`).
    pub fn download_file_url(&self, name: &str) -> String {
        format!("http://{}/downloads/{}", self.addr, name)
    }

    pub fn search_url(&self, q: &str) -> String {
        format!("http://{}/search?q={}", self.addr, urlencoding_simple(q))
    }

    pub async fn request_log(&self) -> Vec<String> {
        self.state.log.lock().await.clone()
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }

    fn router(state: SiteState) -> Router {
        Router::new()
            .route("/", get(landing))
            .route("/login", get(login_page).post(login_submit))
            .route("/dashboard", get(dashboard))
            .route("/search", get(search))
            .route("/secrets-lab", get(secrets_lab))
            .route("/secrets-lab/frame", get(secrets_lab_frame))
            .route("/secrets-lab/scrolled", get(secrets_lab_scrolled))
            .route("/otp-login", get(otp_login_page).post(otp_login_submit))
            .route("/otp-login/ambiguous", get(otp_login_ambiguous))
            .route("/downloads", get(downloads_page))
            .route("/downloads/unsolicited", get(downloads_unsolicited))
            .route("/downloads/report.txt", get(download_report_text))
            .route("/downloads/report.docx", get(download_report_docx))
            .with_state(state)
    }
}

async fn record(state: &SiteState, label: &str) {
    state.log.lock().await.push(label.to_string());
}

async fn landing(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /").await;
    Html(
        r#"<!doctype html><html lang="en"><head><title>Test Site</title></head>
<body><h1>Welcome</h1>
<p>Public landing page.</p>
<p><a href="/login" id="signin">Sign In</a></p>
<p><a href="/search?q=quarry" id="search">Search</a></p>
</body></html>"#
            .to_string(),
    )
}

async fn login_page(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /login").await;
    Html(
        r#"<!doctype html><html lang="en"><head><title>Sign in</title></head>
<body><h1>Sign In</h1>
<form method="post" action="/login">
  <input name="username" id="username">
  <input name="password" id="password" type="password">
  <button type="submit" id="submit">Sign In</button>
</form>
</body></html>"#
            .to_string(),
    )
}

async fn login_submit(
    State(state): State<SiteState>,
    Form(form): Form<LoginForm>,
) -> impl IntoResponse {
    record(&state, "POST /login").await;
    if form.username == "alice" && form.password == "secret" {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SET_COOKIE,
            "auth=alice; Path=/; HttpOnly".parse().unwrap(),
        );
        (
            StatusCode::SEE_OTHER,
            headers,
            [(header::LOCATION, "/dashboard")],
            "",
        )
            .into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Html("<h1>Invalid credentials</h1>"),
        )
            .into_response()
    }
}

async fn dashboard(State(state): State<SiteState>, headers: HeaderMap) -> impl IntoResponse {
    record(&state, "GET /dashboard").await;
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if !cookie.contains("auth=") {
        return Redirect::to("/login").into_response();
    }
    Html(
        r#"<!doctype html><html lang="en"><head><title>Dashboard</title></head>
<body><h1>Welcome back, alice</h1>
<p>Authenticated dashboard content.</p>
<p>Session-dependent content lives here.</p>
</body></html>"#
            .to_string(),
    )
    .into_response()
}

async fn search(State(state): State<SiteState>, Query(q): Query<SearchQuery>) -> Html<String> {
    record(&state, &format!("GET /search?q={}", q.q)).await;
    let safe_q = html_escape(&q.q);
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Search results</title></head>
<body><h1>Results for "{safe_q}"</h1>
<ul>
  <li><a href="/result/1">Result 1 about {safe_q}</a></li>
  <li><a href="/result/2">Result 2 about {safe_q}</a></li>
</ul>
</body></html>"#
    ))
}

/// WS12.S1 fixture for the field-facts probe: a password with a value, an empty
/// password, a card number, a phone number, an OTP field and a plain text input.
/// The secret is joined from fragments so no secret-shaped literal is in the source.
async fn secrets_lab(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /secrets-lab").await;
    let canary = concat!("S3cret", "P@ss-0451");
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Secrets lab</title></head>
<body><h1>Secrets lab</h1>
<label for="pw">Password</label><input id="pw" type="password" value="{canary}">
<label for="pw2">New password</label><input id="pw2" type="password">
<label for="cc">Card number</label><input id="cc" autocomplete="cc-number" value="4111111111111111">
<label for="tel">Phone</label><input id="tel" type="tel" value="+4799999999">
<label for="otp">Verification code</label><input id="otp" autocomplete="one-time-code">
<label for="plain">Visible</label><input id="plain" value="visible-text">
<iframe id="lab-frame" src="/secrets-lab/frame" width="320" height="80" style="border:1px solid #888"></iframe>
</body></html>"#
    ))
}

/// WS12.S5: the same-origin iframe of `/secrets-lab`: one password field with a
/// value, so the screenshot scan has to add the frame offset to find it.
async fn secrets_lab_frame(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /secrets-lab/frame").await;
    let canary = concat!("Fr4me", "P@ss-0451");
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Secrets lab frame</title></head>
<body style="margin:8px"><label for="fpw">Frame password</label>
<input id="fpw" type="password" value="{canary}" style="font-size:28px;width:240px">
</body></html>"#
    ))
}

/// WS12.S5: a page whose password field sits far below the first screen, so a
/// viewport screenshot taken after scrolling to it needs the scroll offset to find
/// the field in the image. The field also has a large font so a leak is readable.
async fn secrets_lab_scrolled(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /secrets-lab/scrolled").await;
    let canary = concat!("Scr0ll", "P@ss-0451");
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Secrets lab scrolled</title></head>
<body style="margin:0"><div style="height:1800px;background:#eef">spacer</div>
<label for="spw">Password</label>
<input id="spw" type="password" value="{canary}" style="font-size:32px;width:360px">
<div style="height:600px"></div>
</body></html>"#
    ))
}

/// E3.1-d login fixture, one-time-code step: exactly one visible, required,
/// non-password input (`autocomplete="one-time-code"`), no options and no QR, so
/// the E3.1-c preconditions accept it as a fill target.
async fn otp_login_page(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /otp-login").await;
    Html(
        r#"<!doctype html><html lang="en"><head><title>Verify sign-in</title></head>
<body><h1>Enter the code we emailed you</h1>
<form method="post" action="/otp-login">
  <label for="code">Verification code</label>
  <input name="code" id="code" required inputmode="numeric" autocomplete="one-time-code">
  <button type="submit" id="verify">Verify</button>
</form>
</body></html>"#
            .to_string(),
    )
}

/// E3.1-d: the same step with two code inputs; the preconditions must refuse it.
async fn otp_login_ambiguous(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /otp-login/ambiguous").await;
    Html(
        r#"<!doctype html><html lang="en"><head><title>Verify sign-in</title></head>
<body><h1>Enter the code</h1>
<form method="post" action="/otp-login">
  <input name="code" id="code-a" required autocomplete="one-time-code">
  <input name="code" id="code-b" required autocomplete="one-time-code">
  <button type="submit" id="verify">Verify</button>
</form>
</body></html>"#
            .to_string(),
    )
}

/// The submitted code is compared and never echoed or logged.
async fn otp_login_submit(
    State(state): State<SiteState>,
    Form(form): Form<OtpForm>,
) -> impl IntoResponse {
    record(&state, "POST /otp-login").await;
    if form.code == FIXTURE_OTP_CODE {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SET_COOKIE,
            "auth=alice; Path=/; HttpOnly".parse().unwrap(),
        );
        (
            StatusCode::SEE_OTHER,
            headers,
            [(header::LOCATION, "/dashboard")],
            "",
        )
            .into_response()
    } else {
        (StatusCode::UNAUTHORIZED, Html("<h1>Invalid code</h1>")).into_response()
    }
}

/// W3-E3.4-6: one download link. A governed browser saves it only for a verified
/// `download_ref` on this link, once.
async fn downloads_page(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /downloads").await;
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Downloads</title></head>
<body><h1>Downloads</h1>
<p><a id="report" href="/downloads/report.txt" download="report.txt">{FIXTURE_DOWNLOAD_LINK}</a></p>
</body></html>"#
    ))
}

/// W3-E3.4-6: the same link, clicked by the page on load: a download no action
/// asked for, which a governed browser must deny.
async fn downloads_unsolicited(State(state): State<SiteState>) -> Html<String> {
    record(&state, "GET /downloads/unsolicited").await;
    Html(format!(
        r#"<!doctype html><html lang="en"><head><title>Unsolicited download</title></head>
<body><h1>Unsolicited download</h1>
<p><a id="report" href="/downloads/report.txt" download="report.txt">{FIXTURE_DOWNLOAD_LINK}</a></p>
<script>window.addEventListener('load', () => document.getElementById('report').click());</script>
</body></html>"#
    ))
}

async fn download_report_text(State(state): State<SiteState>) -> impl IntoResponse {
    record(&state, "GET /downloads/report.txt").await;
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"report.txt\"",
            ),
        ],
        FIXTURE_DOWNLOAD_TEXT,
    )
}

/// An OOXML file is a zip: the local-file-header magic is enough for the type
/// check, which refuses Office documents by bytes as well as by name.
async fn download_report_docx(State(state): State<SiteState>) -> impl IntoResponse {
    record(&state, "GET /downloads/report.docx").await;
    (
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"report.docx\"",
            ),
        ],
        &b"PK\x03\x04fixture"[..],
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn urlencoding_simple(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn landing_page_serves_html() {
        let site = TestSite::start().await;
        let resp = reqwest::get(site.url()).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("Welcome"));
        assert!(body.contains("Sign In"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn login_with_valid_creds_sets_cookie_and_redirects() {
        let site = TestSite::start().await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let resp = client
            .post(site.login_url())
            .form(&[("username", "alice"), ("password", "secret")])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 303);
        let set_cookie = resp
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(set_cookie.contains("auth=alice"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn login_with_bad_creds_returns_401() {
        let site = TestSite::start().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(site.login_url())
            .form(&[("username", "bob"), ("password", "wrong")])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
        site.shutdown().await;
    }

    #[tokio::test]
    async fn dashboard_without_cookie_redirects_to_login() {
        let site = TestSite::start().await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let resp = client.get(site.dashboard_url()).send().await.unwrap();
        assert_eq!(resp.status(), 303);
        let location = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, "/login");
        site.shutdown().await;
    }

    #[tokio::test]
    async fn dashboard_with_cookie_returns_authenticated_html() {
        let site = TestSite::start().await;
        let client = reqwest::Client::new();
        let resp = client
            .get(site.dashboard_url())
            .header("cookie", "auth=alice")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("Welcome back"));
        assert!(body.contains("alice"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn search_echoes_query_and_lists_results() {
        let site = TestSite::start().await;
        let resp = reqwest::get(site.search_url("rust async")).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("rust async"));
        assert!(body.contains("Result 1"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn request_log_captures_traversal() {
        let site = TestSite::start().await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let _ = client.get(site.url()).send().await.unwrap();
        let _ = client.get(site.login_url()).send().await.unwrap();
        let _ = client.get(site.dashboard_url()).send().await.unwrap();
        let log = site.request_log().await;
        assert!(log.contains(&"GET /".to_string()));
        assert!(log.contains(&"GET /login".to_string()));
        assert!(log.contains(&"GET /dashboard".to_string()));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn ephemeral_port_is_unique_per_instance() {
        let s1 = TestSite::start().await;
        let s2 = TestSite::start().await;
        assert_ne!(s1.addr, s2.addr);
        s1.shutdown().await;
        s2.shutdown().await;
    }

    #[tokio::test]
    async fn otp_login_page_has_exactly_one_code_input() {
        let site = TestSite::start().await;
        let body = reqwest::get(site.otp_login_url())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body.matches("autocomplete=\"one-time-code\"").count(), 1);
        assert!(!body.contains("type=\"password\""));
        assert!(body.contains("required"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn otp_login_accepts_the_fixture_code_and_refuses_others() {
        let site = TestSite::start().await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let ok = client
            .post(site.otp_login_url())
            .form(&[("code", FIXTURE_OTP_CODE)])
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), 303);
        let bad = client
            .post(site.otp_login_url())
            .form(&[("code", "000000")])
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), 401);
        assert!(!bad.text().await.unwrap().contains("000000"));
        let log = site.request_log().await;
        assert!(log.iter().all(|l| !l.contains(FIXTURE_OTP_CODE)));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn the_ambiguous_otp_page_has_two_code_inputs() {
        let site = TestSite::start().await;
        let body = reqwest::get(format!("{}/ambiguous", site.otp_login_url()))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body.matches("autocomplete=\"one-time-code\"").count(), 2);
        site.shutdown().await;
    }

    #[tokio::test]
    async fn the_downloads_page_has_one_download_link_to_the_report() {
        let site = TestSite::start().await;
        let body = reqwest::get(site.downloads_url())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body.matches(" download=").count(), 1);
        assert!(body.contains("href=\"/downloads/report.txt\""));
        assert!(body.contains(FIXTURE_DOWNLOAD_LINK));
        assert!(
            !body.contains("<script"),
            "the governed page starts nothing by itself"
        );
        site.shutdown().await;
    }

    #[tokio::test]
    async fn the_unsolicited_page_clicks_its_own_download_link() {
        let site = TestSite::start().await;
        let body = reqwest::get(site.unsolicited_download_url())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(body.contains("getElementById('report').click()"));
        assert!(body.contains("href=\"/downloads/report.txt\""));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn the_report_is_served_as_an_attachment_with_the_fixture_bytes() {
        let site = TestSite::start().await;
        let resp = reqwest::get(site.download_file_url("report.txt"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let disposition = resp
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(disposition.starts_with("attachment"));
        assert!(disposition.contains("report.txt"));
        assert_eq!(&resp.bytes().await.unwrap()[..], FIXTURE_DOWNLOAD_TEXT);
        let log = site.request_log().await;
        assert!(log.contains(&"GET /downloads/report.txt".to_string()));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn the_office_report_is_a_zip_attachment() {
        let site = TestSite::start().await;
        let resp = reqwest::get(site.download_file_url("report.docx"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("report.docx"));
        assert!(resp.bytes().await.unwrap().starts_with(b"PK\x03\x04"));
        site.shutdown().await;
    }

    #[tokio::test]
    async fn an_unknown_download_is_not_found() {
        let site = TestSite::start().await;
        let resp = reqwest::get(site.download_file_url("other.pdf"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
        site.shutdown().await;
    }

    #[test]
    fn url_encoding_handles_spaces_and_specials() {
        assert_eq!(urlencoding_simple("hello world"), "hello+world");
        assert_eq!(urlencoding_simple("a&b"), "a%26b");
        assert_eq!(urlencoding_simple("plain"), "plain");
    }

    #[test]
    fn html_escape_protects_against_injection() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"x\""), "&quot;x&quot;");
    }
}
