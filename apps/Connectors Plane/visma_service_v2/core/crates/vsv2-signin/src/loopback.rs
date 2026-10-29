//! A tiny HTTP listener on the loopback address that catches the browser's redirect back from Visma.
//!
//! It answers `GET <redirect path>` for a loopback `Host`, and only a request that carries the `state` of the
//! sign-in now waiting for its answer is handed on; anything else (a favicon request, a stray or forged answer, a
//! page on another site pointing a browser here) gets an error page and is ignored, so it cannot end the sign-in.
//! Every connection is served on its own task with a short head timeout, so an idle connection cannot hold up the
//! real answer. The request that carries the answer is kept open: the caller replies when it knows what to show,
//! a redirect straight on to the next company's consent screen or a page saying it is done.
//!
//! The browser is opened on a one-time `/start/<nonce>` address that redirects to the real authorization URL, so
//! the URL (which holds the `state` and the PKCE challenge) does not appear in a process list.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;
use url::Url;

use crate::error::{Result, SignInError};
use crate::pkce::random_string;

const MAX_HEAD_BYTES: usize = 16 * 1024;
/// A browser sends its request at once; a connection that does not is not a browser.
const HEAD_TIMEOUT: Duration = Duration::from_secs(3);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Connections served at the same time; further ones are closed at once.
const MAX_CONNECTIONS: usize = 16;

/// The one-time address that sends the browser on to the authorization URL.
struct StartLink {
    nonce: String,
    target: String,
}

/// What the connection tasks need to decide what a request is.
struct Shared {
    path: String,
    port: u16,
    /// The `state` of the request that is waiting for its answer; `None` between rounds.
    waiting_for: Mutex<Option<String>>,
    start: Mutex<Option<StartLink>>,
    answers: mpsc::Sender<Incoming>,
    connections: Arc<Semaphore>,
}

pub struct Listener {
    shared: Arc<Shared>,
    answers: tokio::sync::Mutex<mpsc::Receiver<Incoming>>,
    redirect_uri: String,
    origin: String,
    tasks: Vec<JoinHandle<()>>,
}

/// The query of the callback request, and the open connection to answer it on.
pub struct Incoming {
    pub params: Vec<(String, String)>,
    pub reply: Reply,
}

pub struct Reply {
    stream: TcpStream,
}

impl Listener {
    /// Listen where `redirect_uri` points: `localhost`, `127.0.0.1` or `[::1]`, and its port (0 picks a free
    /// one, which then shows in [`redirect_uri`](Self::redirect_uri)).
    pub async fn bind(redirect_uri: &str) -> Result<Self> {
        let url = Url::parse(redirect_uri)
            .map_err(|_| SignInError::Config("the redirect URI is not a URL".into()))?;
        let port = url.port().unwrap_or(80);
        let bind_error = |addr: SocketAddr, e: std::io::Error| {
            SignInError::Config(format!(
                "cannot listen on {addr} for the redirect ({e}): is another sign-in running, or is the port in use? Change the port in VISMA_USER_REDIRECT_URI and in the Developer Portal, or use --paste"
            ))
        };
        let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let mut listeners = Vec::new();
        match url.host_str() {
            Some("127.0.0.1") => {
                listeners.push(TcpListener::bind(v4).await.map_err(|e| bind_error(v4, e))?)
            }
            Some("[::1]") => {
                let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
                listeners.push(TcpListener::bind(v6).await.map_err(|e| bind_error(v6, e))?);
            }
            Some("localhost") => {
                // A browser may resolve `localhost` to either address: listen on both. A machine without IPv6
                // is fine; another process holding the IPv6 side is not, since the browser could reach it.
                let first = TcpListener::bind(v4).await.map_err(|e| bind_error(v4, e))?;
                let chosen = first.local_addr().map_err(|e| bind_error(v4, e))?.port();
                let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, chosen));
                match TcpListener::bind(v6).await {
                    Ok(second) => {
                        listeners.push(first);
                        listeners.push(second);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                        return Err(bind_error(v6, e))
                    }
                    Err(_) => listeners.push(first),
                }
            }
            _ => {
                return Err(SignInError::Config(
                    "the redirect URI is not a loopback address".into(),
                ))
            }
        }
        let bound = listeners[0]
            .local_addr()
            .map_err(|e| bind_error(v4, e))?
            .port();
        let mut actual = url.clone();
        if port == 0 {
            actual
                .set_port(Some(bound))
                .map_err(|()| SignInError::Config("cannot set the redirect port".into()))?;
        }
        let (sender, receiver) = mpsc::channel(8);
        let shared = Arc::new(Shared {
            path: url.path().to_string(),
            port: bound,
            waiting_for: Mutex::new(None),
            start: Mutex::new(None),
            answers: sender,
            connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        });
        let tasks = listeners
            .into_iter()
            .map(|listener| tokio::spawn(accept_loop(listener, shared.clone())))
            .collect();
        Ok(Self {
            shared,
            answers: tokio::sync::Mutex::new(receiver),
            redirect_uri: actual.to_string(),
            origin: actual.origin().ascii_serialization(),
            tasks,
        })
    }

    /// The redirect URI to send to Visma: the configured one, with the real port.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// A one-time local address that redirects to `target`, for opening the browser without putting `target`
    /// on a command line. It works once; a new call replaces it.
    pub fn start_url(&self, target: &str) -> String {
        let nonce = random_string();
        *self.shared.start.lock().expect("start link lock") = Some(StartLink {
            nonce: nonce.clone(),
            target: target.to_string(),
        });
        format!("{}/start/{nonce}", self.origin)
    }

    /// Wait for the answer to the request whose `state` is given. Requests that are not that answer are
    /// answered with an error and skipped.
    pub async fn next(&self, state: &str, timeout: Duration) -> Result<Incoming> {
        let mut answers = self.answers.lock().await;
        // An answer that arrived after the previous wait ended belongs to a request that is over.
        while answers.try_recv().is_ok() {}
        *self.shared.waiting_for.lock().expect("state lock") = Some(state.to_string());
        let outcome = tokio::time::timeout(timeout, answers.recv()).await;
        *self.shared.waiting_for.lock().expect("state lock") = None;
        match outcome {
            Ok(Some(incoming)) => Ok(incoming),
            Ok(None) => Err(SignInError::Http("the redirect listener stopped".into())),
            Err(_) => Err(SignInError::Timeout(timeout.as_secs())),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(_) => {
                // Out of descriptors or a reset while accepting: pause rather than spin.
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Ok(permit) = shared.connections.clone().try_acquire_owned() else {
            continue; // over the cap: the connection is closed by dropping it
        };
        let shared = shared.clone();
        tokio::spawn(async move {
            let _permit = permit;
            serve(stream, &shared).await;
        });
    }
}

/// Read one request and answer it, or hand it on when it is the callback this sign-in is waiting for.
async fn serve(mut stream: TcpStream, shared: &Shared) {
    let Ok(Some(head)) = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut stream)).await else {
        return;
    };
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (
        request_line.next().unwrap_or_default(),
        request_line.next().unwrap_or_default(),
    );
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if method != "GET" {
        return respond_text(stream, 405, "Method Not Allowed").await;
    }
    // The `Host` must name this machine's loopback and our port, or a web page elsewhere could aim a browser at
    // the listener under another name.
    if !is_own_host(&host, shared.port) {
        return respond_text(stream, 400, "Bad Request").await;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if let Some(nonce) = path.strip_prefix("/start/") {
        let link = {
            let mut start = shared.start.lock().expect("start link lock");
            if start.as_ref().is_some_and(|link| link.nonce == nonce) {
                start.take()
            } else {
                None
            }
        };
        return match link {
            Some(link) => Reply { stream }.redirect(&link.target).await,
            None => respond_text(stream, 404, "Not Found").await,
        };
    }
    if path != shared.path {
        return respond_text(stream, 404, "Not Found").await;
    }
    let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let waiting_for = shared.waiting_for.lock().expect("state lock").clone();
    let state = params
        .iter()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.as_str());
    if waiting_for.is_none() || state != waiting_for.as_deref() {
        return Reply { stream }
            .page_with_status(
                400,
                "Not the answer to this sign-in",
                "This request does not belong to the sign-in in progress, so it was ignored.",
            )
            .await;
    }
    // A full channel or a stopped receiver drops the connection, which only ends this one request.
    let _ = shared
        .answers
        .send(Incoming {
            params,
            reply: Reply { stream },
        })
        .await;
}

fn is_own_host(host: &str, port: u16) -> bool {
    ["localhost", "127.0.0.1", "[::1]"]
        .iter()
        .any(|name| host == format!("{name}:{port}"))
        || (port == 80 && ["localhost", "127.0.0.1", "[::1]"].contains(&host))
}

async fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            return Some(String::from_utf8_lossy(&buffer[..end]).into_owned());
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return None;
        }
    }
}

async fn write_all(mut stream: TcpStream, bytes: &[u8]) {
    // The person may already have closed the tab: a failed write is not a failed sign-in.
    let _ = tokio::time::timeout(WRITE_TIMEOUT, async {
        let _ = stream.write_all(bytes).await;
        let _ = stream.shutdown().await;
    })
    .await;
}

async fn respond_text(stream: TcpStream, status: u16, reason: &str) {
    let body = format!("{status} {reason}");
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_all(stream, format!("{head}{body}").as_bytes()).await;
}

impl Reply {
    /// Send the browser on to `location` (the next consent screen).
    pub async fn redirect(self, location: &str) {
        if location.chars().any(char::is_control) {
            return self
                .page("Sign-in stopped", "The next address was not valid.")
                .await;
        }
        let head = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        write_all(self.stream, head.as_bytes()).await;
    }

    /// Show a plain page: the sign-in is finished (or stopped), and why.
    pub async fn page(self, title: &str, message: &str) {
        self.page_with_status(200, title, message).await;
    }

    async fn page_with_status(self, status: u16, title: &str, message: &str) {
        let reason = if status == 200 { "OK" } else { "Bad Request" };
        let body = format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{t}</title>\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
             <style>body{{font:16px system-ui,sans-serif;max-width:34rem;margin:15vh auto;padding:0 1rem;color:#1b1b1b}}\
             @media(prefers-color-scheme:dark){{body{{background:#161616;color:#eee}}}}h1{{font-size:1.3rem}}</style>\
             </head><body><h1>{t}</h1><p>{m}</p></body></html>",
            t = escape_html(title),
            m = escape_html(message)
        );
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n",
            body.len()
        );
        write_all(self.stream, format!("{head}{body}").as_bytes()).await;
    }
}

fn escape_html(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            c => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn browser() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    async fn listener() -> Listener {
        Listener::bind("http://127.0.0.1:0/callback").await.unwrap()
    }

    fn base(listener: &Listener) -> String {
        listener
            .redirect_uri()
            .trim_end_matches("/callback")
            .to_string()
    }

    const SECS: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn the_real_port_shows_in_the_redirect_uri_when_zero_was_asked() {
        let l = listener().await;
        assert!(
            l.redirect_uri().starts_with("http://127.0.0.1:") && !l.redirect_uri().contains(":0/")
        );
        assert!(l.redirect_uri().ends_with("/callback"));
    }

    #[tokio::test]
    async fn the_answer_for_the_waiting_request_is_caught_and_the_browser_can_be_sent_on() {
        let l = listener().await;
        let url = format!(
            "{}/callback?code=a%20b&state=s1&iss=https%3A%2F%2Fconnect.visma.com",
            base(&l)
        );
        let client = tokio::spawn(async move { browser().get(url).send().await.unwrap() });
        let incoming = l.next("s1", SECS).await.unwrap();
        let get = |name: &str| {
            incoming
                .params
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("code").as_deref(), Some("a b"));
        assert_eq!(get("iss").as_deref(), Some("https://connect.visma.com"));
        incoming
            .reply
            .redirect("https://connect.visma.com/connect/authorize?next=1")
            .await;
        let response = client.await.unwrap();
        assert_eq!(response.status(), 302);
        assert_eq!(
            response.headers()["location"],
            "https://connect.visma.com/connect/authorize?next=1"
        );
    }

    #[tokio::test]
    async fn the_final_page_escapes_what_it_shows() {
        let l = listener().await;
        let url = format!("{}/callback?code=x&state=s", base(&l));
        let client = tokio::spawn(async move { browser().get(url).send().await.unwrap() });
        let incoming = l.next("s", SECS).await.unwrap();
        incoming
            .reply
            .page("Done", "Connected <b>Aquatiq</b> & friends")
            .await;
        let response = client.await.unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'none'"));
        let body = response.text().await.unwrap();
        assert!(
            body.contains("&lt;b&gt;Aquatiq&lt;/b&gt; &amp; friends")
                && !body.contains("<b>Aquatiq"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn requests_that_are_not_the_answer_are_refused_and_never_end_the_wait() {
        let l = listener().await;
        let base = base(&l);
        let client = tokio::spawn(async move {
            let b = browser();
            let status = |response: reqwest::Response| response.status().as_u16();
            let favicon = status(b.get(format!("{base}/favicon.ico")).send().await.unwrap());
            let post = status(
                b.post(format!("{base}/callback?code=x&state=s"))
                    .send()
                    .await
                    .unwrap(),
            );
            let rebinding = status(
                b.get(format!("{base}/callback?code=x&state=s"))
                    .header("Host", "evil.example")
                    .send()
                    .await
                    .unwrap(),
            );
            // Forged or stray answers to the callback path: no state, the wrong state, an error without a state.
            let no_state = status(b.get(format!("{base}/callback")).send().await.unwrap());
            let wrong_state = status(
                b.get(format!("{base}/callback?code=x&state=other"))
                    .send()
                    .await
                    .unwrap(),
            );
            let stateless_error = status(
                b.get(format!(
                    "{base}/callback?error=access_denied%0D%0Arun%3A%20curl"
                ))
                .send()
                .await
                .unwrap(),
            );
            let real = status(
                b.get(format!("{base}/callback?code=real&state=s"))
                    .send()
                    .await
                    .unwrap(),
            );
            (
                favicon,
                post,
                rebinding,
                no_state,
                wrong_state,
                stateless_error,
                real,
            )
        });
        let incoming = l.next("s", SECS).await.unwrap();
        assert!(incoming.params.contains(&("code".into(), "real".into())));
        incoming.reply.page("Done", "ok").await;
        assert_eq!(client.await.unwrap(), (404, 405, 400, 400, 400, 400, 200));
    }

    #[tokio::test]
    async fn an_answer_that_arrives_when_no_sign_in_is_waiting_is_refused() {
        let l = listener().await;
        let status = browser()
            .get(format!("{}/callback?code=x&state=s", base(&l)))
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn waiting_gives_up_after_the_timeout() {
        let l = listener().await;
        let e = l
            .next("s", Duration::from_millis(150))
            .await
            .err()
            .expect("times out");
        assert!(matches!(e, SignInError::Timeout(_)), "{e}");
    }

    #[tokio::test]
    async fn idle_connections_do_not_hold_up_the_real_answer() {
        let l = listener().await;
        let address = l
            .redirect_uri()
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        // Five connections that say nothing, as a stuck or hostile client would.
        let mut idle = Vec::new();
        for _ in 0..5 {
            idle.push(TcpStream::connect(&address).await.unwrap());
        }
        let url = format!("{}/callback?code=x&state=s", base(&l));
        let client =
            tokio::spawn(async move { browser().get(url).send().await.unwrap().status().as_u16() });
        let started = Instant::now();
        let incoming = l.next("s", SECS).await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "answered in {:?}",
            started.elapsed()
        );
        incoming.reply.page("Done", "ok").await;
        assert_eq!(client.await.unwrap(), 200);
        drop(idle);
    }

    #[tokio::test]
    async fn the_start_address_redirects_once_to_its_target_and_nowhere_else() {
        let l = listener().await;
        let start =
            l.start_url("https://connect.visma.com/connect/authorize?state=s&code_challenge=c");
        assert!(
            start.starts_with(&format!("{}/start/", base(&l))),
            "{start}"
        );
        let b = browser();
        let first = b.get(&start).send().await.unwrap();
        assert_eq!(first.status(), 302);
        assert_eq!(
            first.headers()["location"],
            "https://connect.visma.com/connect/authorize?state=s&code_challenge=c"
        );
        assert_eq!(
            b.get(&start).send().await.unwrap().status(),
            404,
            "a start address works once"
        );
        assert_eq!(
            b.get(format!("{}/start/wrong", base(&l)))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }

    #[tokio::test]
    async fn only_loopback_redirects_can_be_listened_on_and_a_busy_port_is_explained() {
        assert!(Listener::bind("https://app.example.com/cb").await.is_err());
        let first = Listener::bind("http://127.0.0.1:0/callback").await.unwrap();
        let busy = first.redirect_uri().to_string();
        let e = Listener::bind(&busy)
            .await
            .err()
            .expect("port is taken")
            .to_string();
        assert!(
            e.contains("VISMA_USER_REDIRECT_URI") && e.contains("--paste"),
            "{e}"
        );
    }

    #[tokio::test]
    async fn localhost_listens_on_the_chosen_port() {
        let l = Listener::bind("http://localhost:0/callback").await.unwrap();
        assert!(l.redirect_uri().starts_with("http://localhost:"));
        let url = format!("{}?code=x&state=s", l.redirect_uri());
        let client =
            tokio::spawn(async move { browser().get(url).send().await.unwrap().status().as_u16() });
        let incoming = l.next("s", SECS).await.unwrap();
        incoming.reply.page("Done", "ok").await;
        assert_eq!(client.await.unwrap(), 200);
    }

    #[tokio::test]
    async fn another_process_holding_the_ipv6_side_of_localhost_is_an_error() {
        // Take [::1]:P with a port that is free on IPv4 too; skip on a machine without IPv6.
        let Ok(squatter) = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await else {
            return;
        };
        let port = squatter.local_addr().unwrap().port();
        let e = Listener::bind(&format!("http://localhost:{port}/callback"))
            .await
            .err()
            .expect("refused")
            .to_string();
        assert!(e.contains("cannot listen") && e.contains("::1"), "{e}");
    }
}
