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

    pub fn search_url(&self, q: &str) -> String {
        format!(
            "http://{}/search?q={}",
            self.addr,
            urlencoding_simple(q)
        )
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
        let location = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
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
