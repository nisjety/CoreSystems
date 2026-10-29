//! The sign-in: one consent screen per company, repeated until every company is connected.
//!
//! Visma Connect issues a token for ONE company per approval. So instead of asking the person to run a
//! command once per company, [`login`] keeps the browser tab it opened: after each approval it exchanges
//! the code, stores the result, and answers the redirect with a redirect to the next consent screen, where
//! the person picks the next company. It keeps going until the screen offers a company that was already
//! done in this run (Visma has no more), the person declines or presses Enter, or nobody answers in time;
//! with `configured_only` it also stops once every company in `config/tenants` is connected. What was
//! connected before that point stays connected.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;

use crate::authorize::{build_request, params_from_pasted, read_callback, AuthRequest};
use crate::config::{redirect_is_loopback, SignInConfig};
use crate::error::{Result, SignInError};
use crate::exchange::{Exchanger, Grant};
use crate::jwt::{self, Claims};
use crate::loopback::{Listener, Reply};
use crate::store::{valid_tenant_id, Connection, Session, Store};

/// Scopes that say who the person is rather than what they may call. A company counts as complete when it
/// has the API scopes: an access token does not always list these.
const IDENTITY_SCOPES: [&str; 4] = ["openid", "profile", "email", "offline_access"];
const DEFAULT_ROUND_TIMEOUT: Duration = Duration::from_secs(300);

pub trait Browser: Send + Sync {
    /// Open `address` for the person. False when that could not even be tried; the address is printed
    /// anyway.
    fn open(&self, address: &str) -> bool;
}

#[async_trait]
pub trait Console: Send + Sync {
    fn say(&self, line: &str);
    /// A line typed by the person (the pasted address, in paste mode). `None` when there is no input.
    async fn ask(&self, prompt: &str) -> Option<String>;
    /// Resolves when the person presses Enter to stop; never when there is no terminal to press it in.
    async fn wait_for_enter(&self) {
        std::future::pending::<()>().await
    }
}

/// A company the operator has configured (`config/tenants`): it gives names and a "2 of 8" count, and with
/// `configured_only` it tells the loop when it is done. A person may have more companies than these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownCompany {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct LoginOptions {
    /// Ask for the write scopes as well (`VISMA_USER_SCOPES_WRITE`).
    pub write: bool,
    /// Connect one company and stop.
    pub once: bool,
    /// Stop as soon as every configured company is connected (and do nothing if they all already are),
    /// instead of offering consent screens until Visma has no more companies.
    pub configured_only: bool,
    /// Do not listen for the redirect; ask for the address the browser ended up on instead.
    pub paste: bool,
    /// How long one consent screen may wait for an answer.
    pub round_timeout: Duration,
    /// A safety limit on the number of consent screens in one run.
    pub max_rounds: usize,
    pub known: Vec<KnownCompany>,
}

impl LoginOptions {
    pub fn new(known: Vec<KnownCompany>) -> Self {
        Self {
            write: false,
            once: false,
            configured_only: false,
            paste: false,
            round_timeout: DEFAULT_ROUND_TIMEOUT,
            max_rounds: (known.len() + 3).clamp(4, 40),
            known,
        }
    }
}

/// Why the run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished {
    /// `configured_only`: every configured company is connected with the scopes asked for.
    AllConnected,
    /// The consent screen offered a company that was already done in this run: there are no more.
    NothingNew,
    /// `--once`.
    Once,
    /// `configured_only`: everything was connected already, so nothing was opened.
    AlreadyComplete,
    /// The person declined the next company.
    Declined(String),
    /// The person pressed Enter.
    Stopped,
    /// Nobody answered the next consent screen in time.
    TimedOut,
    /// The safety limit on rounds.
    RoundLimit,
}

impl Finished {
    pub fn summary(&self) -> &'static str {
        match self {
            Self::AllConnected => "every configured company is connected",
            Self::NothingNew => "Visma offered no further company",
            Self::Once => "connected one company, as asked",
            Self::AlreadyComplete => "every configured company was already connected",
            Self::Declined(_) => "the next company was declined",
            Self::Stopped => "stopped at your request",
            Self::TimedOut => "nobody answered the next consent screen in time",
            Self::RoundLimit => "reached the limit on companies per run",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoginReport {
    pub session: Session,
    /// Companies connected or renewed by this run, in order.
    pub connected: Vec<String>,
    pub finished: Finished,
}

/// How the redirect gets back to us.
enum Channel {
    Loopback(Listener),
    Paste,
}

impl Channel {
    async fn open(cfg: &SignInConfig, paste: bool) -> Result<Self> {
        if paste || !redirect_is_loopback(&cfg.redirect_uri) {
            Ok(Self::Paste)
        } else {
            Ok(Self::Loopback(Listener::bind(&cfg.redirect_uri).await?))
        }
    }

    fn redirect_uri(&self, cfg: &SignInConfig) -> String {
        match self {
            Self::Loopback(listener) => listener.redirect_uri().to_string(),
            Self::Paste => cfg.redirect_uri.clone(),
        }
    }

    /// The query of the redirect, and (when a listener caught it) the open request to answer.
    async fn wait(
        &self,
        console: &dyn Console,
        state: &str,
        timeout: Duration,
    ) -> Result<(Vec<(String, String)>, Option<Reply>)> {
        match self {
            Self::Loopback(listener) => tokio::select! {
                incoming = listener.next(state, timeout) => incoming.map(|i| (i.params, Some(i.reply))),
                () = console.wait_for_enter() => Err(SignInError::Cancelled),
            },
            Self::Paste => {
                let answer = console
                    .ask("Paste the address the browser ended up on (it starts with your redirect URL), then press Enter:")
                    .await
                    .ok_or(SignInError::Cancelled)?;
                Ok((params_from_pasted(&answer)?, None))
            }
        }
    }
}

/// Sign in to every company the person approves. See the module documentation for the loop.
pub async fn login(
    cfg: &SignInConfig,
    store: &Store,
    opts: &LoginOptions,
    browser: &dyn Browser,
    console: &dyn Console,
) -> Result<LoginReport> {
    let scopes = cfg.requested_scopes(opts.write);
    let api_scopes: Vec<&String> = scopes
        .iter()
        .filter(|s| !IDENTITY_SCOPES.contains(&s.as_str()))
        .collect();
    let mut session = store.session()?;
    if session
        .as_ref()
        .is_some_and(|s| s.client_id != cfg.client_id)
    {
        return Err(SignInError::Config(
            "this computer is signed in with another application (client id): run `vsv2-mcp logout` first".into(),
        ));
    }
    let before = store.connections()?;
    let done_before = |id: &str| {
        before
            .iter()
            .any(|c| c.tenant_id == id && api_scopes.iter().all(|s| c.has_scope(s)))
    };
    let stop_at_known = opts.configured_only && !opts.known.is_empty();
    if stop_at_known && !opts.once && opts.known.iter().all(|k| done_before(&k.id)) {
        if let Some(session) = session {
            return Ok(LoginReport {
                session,
                connected: Vec::new(),
                finished: Finished::AlreadyComplete,
            });
        }
    }

    let exchanger = Exchanger::new(cfg);
    let channel = Channel::open(cfg, opts.paste).await?;
    let redirect_uri = channel.redirect_uri(cfg);
    let mut connected: Vec<String> = Vec::new();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut pending: Option<Reply> = None;
    let mut finished = Finished::RoundLimit;

    for _round in 0..opts.max_rounds.max(1) {
        let request = build_request(cfg, &redirect_uri, &scopes)?;
        deliver(&channel, console, browser, pending.take(), &request.url).await;
        let (params, reply) = match channel
            .wait(console, &request.state, opts.round_timeout)
            .await
        {
            Ok(answer) => answer,
            Err(SignInError::Timeout(_)) if !connected.is_empty() => {
                finished = Finished::TimedOut;
                break;
            }
            Err(SignInError::Cancelled) if !connected.is_empty() => {
                finished = Finished::Stopped;
                break;
            }
            Err(e) => return Err(e),
        };
        let code = match read_callback(&params, &request, &cfg.issuer) {
            Ok(code) => code,
            Err(SignInError::Denied { error, .. }) if !connected.is_empty() => {
                finished = Finished::Declined(error);
                pending = reply;
                break;
            }
            Err(e) => return Err(stop_with(reply, e).await),
        };
        let company = match accept(
            cfg,
            store,
            &exchanger,
            &request,
            &code,
            &redirect_uri,
            &mut session,
        )
        .await
        {
            Ok(company) => company,
            Err(e) => return Err(stop_with(reply, e).await),
        };
        let repeat = connected.contains(&company.tenant_id);
        if !repeat {
            connected.push(company.tenant_id.clone());
        }
        if let Some(name) = &company.name {
            names.insert(company.tenant_id.clone(), name.clone());
        }
        console.say(&progress_line(
            opts,
            &names,
            &company.tenant_id,
            &connected,
            &done_before,
        ));
        pending = reply;
        if repeat {
            finished = Finished::NothingNew;
            break;
        }
        if opts.once {
            finished = Finished::Once;
            break;
        }
        let all_known = stop_at_known
            && opts
                .known
                .iter()
                .all(|k| connected.contains(&k.id) || done_before(&k.id));
        if all_known {
            finished = Finished::AllConnected;
            break;
        }
    }

    let session = session.ok_or(SignInError::NotSignedIn)?;
    if let Some(reply) = pending.take() {
        reply
            .page(
                &final_title(&finished),
                &final_message(opts, &names, &session, &connected, &finished),
            )
            .await;
    }
    Ok(LoginReport {
        session,
        connected,
        finished,
    })
}

/// Show the consent screen: open it for the first company, send the same tab on for the next ones.
async fn deliver(
    channel: &Channel,
    console: &dyn Console,
    browser: &dyn Browser,
    pending: Option<Reply>,
    url: &str,
) {
    match (channel, pending) {
        (Channel::Loopback(_), Some(reply)) => {
            console.say("Continuing with the next company in the same browser tab.");
            reply.redirect(url).await;
        }
        (Channel::Loopback(listener), None) => {
            console.say("Opening Visma in your browser. If it does not open, go to this address:");
            console.say(&format!("  {url}"));
            // The browser is opened on a one-time local address that redirects to `url`, so `url` (which holds the
            // state and the PKCE challenge) is not on a command line where other users of this computer can see it.
            if !browser.open(&listener.start_url(url)) {
                console.say("(the browser could not be opened automatically)");
            }
            console.say("Choose a company and approve it. Press Enter here to stop after the companies you have.");
        }
        (Channel::Paste, _) => {
            console.say("Open this address in a browser, choose a company and approve it:");
            console.say(&format!("  {url}"));
        }
    }
}

/// Answer the browser with the reason, then hand the error back.
async fn stop_with(reply: Option<Reply>, error: SignInError) -> SignInError {
    if let Some(reply) = reply {
        reply.page("Sign-in stopped", &error.to_string()).await;
    }
    error
}

/// Trade the code for tokens and store them, after checking they belong to the person already signed in.
async fn accept(
    cfg: &SignInConfig,
    store: &Store,
    exchanger: &Exchanger,
    request: &AuthRequest,
    code: &str,
    redirect_uri: &str,
    session: &mut Option<Session>,
) -> Result<Connection> {
    let grant = exchanger
        .authorization_code(code, &request.pkce.verifier, redirect_uri)
        .await?;
    let claims = jwt::claims(&grant.access_token)?;
    let id_claims = grant
        .id_token
        .as_deref()
        .and_then(|token| jwt::claims(token).ok());
    let tenant_id = claims
        .tenant_id
        .clone()
        .filter(|id| valid_tenant_id(id))
        .ok_or_else(|| SignInError::BadToken("it names no company (tenant_id)".into()))?;
    let sub = claims
        .sub
        .clone()
        .or_else(|| id_claims.as_ref().and_then(|c| c.sub.clone()))
        .ok_or_else(|| SignInError::BadToken("it names no person (sub)".into()))?;
    let refresh_token = grant
        .refresh_token
        .clone()
        .ok_or(SignInError::NoRefreshToken)?;
    if let Some(existing) = session.as_ref().filter(|s| s.sub != sub) {
        return Err(SignInError::DifferentUser {
            signed_in_as: existing.display().to_string(),
        });
    }
    let mut connection = Connection::new(
        tenant_id,
        sub.clone(),
        granted_scopes(&grant, &claims),
        refresh_token,
        grant.access_token.clone(),
        Utc::now() + chrono::Duration::seconds(grant.expires_in),
    );
    connection.name = exchanger
        .company_name(&grant.access_token, &connection.tenant_id)
        .await;
    if session.is_none() {
        let label = label_for(id_claims.as_ref(), exchanger, &grant.access_token).await;
        let created = Session::new(sub, label, cfg.client_id.clone());
        store.save_session(&created)?;
        *session = Some(created);
    }
    store.save_connection(&connection)?;
    Ok(connection)
}

fn granted_scopes(grant: &Grant, claims: &Claims) -> Vec<String> {
    let listed: Vec<String> = grant
        .scope
        .as_deref()
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if listed.is_empty() {
        claims.scopes.clone()
    } else {
        listed
    }
}

/// An e-mail address or a name for messages, from the ID token or else the userinfo endpoint. Failing to find
/// one is fine: the sign-in then goes by the person's id.
async fn label_for(
    id_claims: Option<&Claims>,
    exchanger: &Exchanger,
    access_token: &str,
) -> Option<String> {
    if let Some(label) = id_claims.and_then(|c| c.email.clone().or_else(|| c.name.clone())) {
        return Some(label);
    }
    let info = exchanger.userinfo(access_token).await.ok()?;
    info.email.or(info.name)
}

/// How to show a company: the configured name, else the name Visma gave, with its id.
fn name_of(opts: &LoginOptions, names: &HashMap<String, String>, id: &str) -> String {
    match opts
        .known
        .iter()
        .find(|k| k.id == id)
        .map(|k| &k.name)
        .or_else(|| names.get(id))
    {
        Some(name) => format!("{name} ({id})"),
        None => id.to_string(),
    }
}

fn progress_line(
    opts: &LoginOptions,
    names: &HashMap<String, String>,
    id: &str,
    connected: &[String],
    done_before: &dyn Fn(&str) -> bool,
) -> String {
    let name = name_of(opts, names, id);
    if opts.known.is_empty() {
        return format!("Connected {name}.");
    }
    let done = opts
        .known
        .iter()
        .filter(|k| connected.contains(&k.id) || done_before(&k.id))
        .count();
    format!(
        "Connected {name}: {done} of {} configured companies.",
        opts.known.len()
    )
}

fn final_title(finished: &Finished) -> String {
    match finished {
        Finished::AllConnected | Finished::Once | Finished::NothingNew => {
            "Signed in to Visma".to_string()
        }
        _ => "Sign-in stopped".to_string(),
    }
}

fn final_message(
    opts: &LoginOptions,
    names: &HashMap<String, String>,
    session: &Session,
    connected: &[String],
    finished: &Finished,
) -> String {
    let names: Vec<String> = connected
        .iter()
        .map(|id| name_of(opts, names, id))
        .collect();
    format!(
        "{} for {}: {}. You can close this tab and go back to the terminal.",
        if connected.len() == 1 {
            "Connected 1 company".to_string()
        } else {
            format!("Connected {} companies", connected.len())
        },
        session.display(),
        if names.is_empty() {
            finished.summary().to_string()
        } else {
            names.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::json;
    use url::Url;

    /// What the person does on a consent screen.
    #[derive(Clone, Debug)]
    enum Act {
        Approve {
            tenant: &'static str,
            sub: &'static str,
        },
        ApproveWithoutRefreshToken(&'static str),
        Decline,
        AnswerForAnotherRequest,
    }

    fn approve(tenant: &'static str) -> Act {
        Act::Approve {
            tenant,
            sub: "user-1",
        }
    }

    /// The query of the redirect the person's browser would make for `act`.
    fn answer_for(act: &Act, url: &str) -> Option<String> {
        let query: HashMap<String, String> = Url::parse(url)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        let (redirect, state) = (&query["redirect_uri"], &query["state"]);
        let encode =
            |code: &str| url::form_urlencoded::byte_serialize(code.as_bytes()).collect::<String>();
        Some(match act {
            Act::Approve { tenant, sub } => format!("{redirect}?code={}&state={state}&iss=https%3A%2F%2Fconnect.visma.com", encode(&format!("code|{tenant}|{sub}"))),
            Act::ApproveWithoutRefreshToken(tenant) => format!("{redirect}?code={}&state={state}", encode(&format!("norefresh|{tenant}|user-1"))),
            Act::Decline => format!("{redirect}?error=access_denied&error_description=the%20user%20said%20no&state={state}"),
            Act::AnswerForAnotherRequest => format!("{redirect}?code=x&state=not-the-state"),
        })
    }

    #[derive(Default)]
    struct Tab {
        redirects: AtomicUsize,
        urls: Mutex<Vec<String>>,
        last_page: Mutex<Option<(u16, String)>>,
    }

    /// A browser whose person follows a script, keeping to one tab as a real person would.
    struct Pilot {
        acts: Arc<Mutex<VecDeque<Act>>>,
        opened: AtomicUsize,
        /// What the browser was told to open: the command line other users of the computer can see.
        opened_with: Mutex<Vec<String>>,
        tab: Arc<Tab>,
        task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    }

    impl Pilot {
        fn new(acts: Vec<Act>) -> Self {
            Self {
                acts: Arc::new(Mutex::new(acts.into())),
                opened: AtomicUsize::new(0),
                opened_with: Mutex::new(Vec::new()),
                tab: Arc::new(Tab::default()),
                task: Mutex::new(None),
            }
        }

        async fn finish(&self) -> Arc<Tab> {
            let task = self.task.lock().unwrap().take();
            if let Some(task) = task {
                task.await.unwrap();
            }
            self.tab.clone()
        }
    }

    impl Browser for Pilot {
        fn open(&self, address: &str) -> bool {
            self.opened.fetch_add(1, Ordering::SeqCst);
            self.opened_with.lock().unwrap().push(address.to_string());
            let (acts, tab, start) = (self.acts.clone(), self.tab.clone(), address.to_string());
            *self.task.lock().unwrap() = Some(tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap();
                // The browser opens the one-time start address, which sends it on to Visma.
                let first = client.get(&start).send().await.unwrap();
                assert_eq!(first.status(), 302, "the start address redirects");
                let mut url = first.headers()["location"].to_str().unwrap().to_string();
                loop {
                    tab.urls.lock().unwrap().push(url.clone());
                    let Some(act) = acts.lock().unwrap().pop_front() else {
                        return;
                    };
                    let response = client
                        .get(answer_for(&act, &url).unwrap())
                        .send()
                        .await
                        .unwrap();
                    if response.status() == 302 {
                        tab.redirects.fetch_add(1, Ordering::SeqCst);
                        url = response.headers()["location"].to_str().unwrap().to_string();
                        continue;
                    }
                    if response.status() == 400 {
                        // Ignored by the listener: the person is still on the same consent screen.
                        continue;
                    }
                    let status = response.status().as_u16();
                    *tab.last_page.lock().unwrap() = Some((status, response.text().await.unwrap()));
                    return;
                }
            }));
            true
        }
    }

    struct NoBrowser;
    impl Browser for NoBrowser {
        fn open(&self, _: &str) -> bool {
            panic!("the browser must not be opened");
        }
    }

    #[derive(Default)]
    struct Terminal {
        said: Mutex<Vec<String>>,
        pasted: Mutex<VecDeque<Act>>,
        press_enter_in_round: Mutex<Option<usize>>,
        enter_calls: AtomicUsize,
    }

    #[async_trait]
    impl Console for Terminal {
        fn say(&self, line: &str) {
            self.said.lock().unwrap().push(line.to_string());
        }

        async fn ask(&self, _prompt: &str) -> Option<String> {
            let act = self.pasted.lock().unwrap().pop_front()?;
            let url = self.said.lock().unwrap().iter().rev().find_map(|l| {
                l.trim()
                    .strip_prefix("https://")
                    .map(|rest| format!("https://{rest}"))
            })?;
            answer_for(&act, &url)
        }

        async fn wait_for_enter(&self) {
            let call = self.enter_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if *self.press_enter_in_round.lock().unwrap() == Some(call) {
                return;
            }
            std::future::pending::<()>().await
        }
    }

    impl Terminal {
        fn text(&self) -> String {
            self.said.lock().unwrap().join("\n")
        }
    }

    struct Rig {
        _dir: tempfile::TempDir,
        server: mockito::ServerGuard,
        cfg: SignInConfig,
        store: Store,
    }

    /// Visma Connect: a token endpoint that turns `code|tenant|sub` into tokens for that company and person.
    async fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _token = server
            .mock("POST", "/token")
            .with_body_from_request(|request| {
                let body = request.body().unwrap().clone();
                let form: HashMap<String, String> =
                    url::form_urlencoded::parse(&body).into_owned().collect();
                let code = form.get("code").cloned().unwrap_or_default();
                let mut parts = code.split('|');
                let (kind, tenant, sub) = (
                    parts.next().unwrap(),
                    parts.next().unwrap_or("t-?"),
                    parts.next().unwrap_or("user-1"),
                );
                let access = jwt::test_jwt(&json!({
                    "sub": sub, "tenant_id": tenant, "exp": Utc::now().timestamp() + 3600,
                    "scope": ["vismanet_erp_interactive_api:read"]
                }));
                let mut response = json!({
                    "access_token": access, "expires_in": 3600,
                    "scope": "openid profile email offline_access vismanet_erp_interactive_api:read"
                });
                if kind != "norefresh" {
                    response["refresh_token"] = json!(format!("refresh-{tenant}"));
                }
                response.to_string().into_bytes()
            })
            .expect_at_least(0)
            .create_async()
            .await;
        let _organization = server
            .mock("GET", "/v1/organization")
            .match_query(mockito::Matcher::Any)
            .with_body_from_request(|request| {
                let tenant = request
                    .header("ipp-company-id")
                    .first()
                    .map(|v| v.to_str().unwrap().to_string())
                    .unwrap_or_default();
                json!([{"name": format!("Visma name of {tenant}")}])
                    .to_string()
                    .into_bytes()
            })
            .expect_at_least(0)
            .create_async()
            .await;
        let _userinfo = server
            .mock("GET", "/userinfo")
            .with_body(r#"{"sub":"user-1","name":"Ima","email":"ima@example.com"}"#)
            .expect_at_least(0)
            .create_async()
            .await;
        let mut cfg = crate::testutil::config();
        cfg.redirect_uri = "http://127.0.0.1:0/callback".into();
        cfg.token_url = format!("{}/token", server.url());
        cfg.userinfo_url = format!("{}/userinfo", server.url());
        cfg.erp_base_url = server.url();
        cfg.store_dir = dir.path().join("user");
        let store = Store::new(cfg.store_dir.clone());
        Rig {
            _dir: dir,
            server,
            cfg,
            store,
        }
    }

    fn known(ids: &[(&str, &str)]) -> Vec<KnownCompany> {
        ids.iter()
            .map(|(id, name)| KnownCompany {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect()
    }

    fn three() -> Vec<KnownCompany> {
        known(&[
            ("t-1", "Aquatiq AS"),
            ("t-2", "Aqua Pharma Inc"),
            ("t-3", "Monagold Prawn Farm"),
        ])
    }

    fn quick(known: Vec<KnownCompany>) -> LoginOptions {
        LoginOptions {
            round_timeout: Duration::from_secs(5),
            ..LoginOptions::new(known)
        }
    }

    #[tokio::test]
    async fn every_configured_company_is_connected_in_one_browser_tab() {
        let r = rig().await;
        let (pilot, terminal) = (
            Pilot::new(vec![approve("t-1"), approve("t-2"), approve("t-3")]),
            Terminal::default(),
        );
        let opts = LoginOptions {
            configured_only: true,
            ..quick(three())
        };
        let report = login(&r.cfg, &r.store, &opts, &pilot, &terminal)
            .await
            .unwrap();
        let tab = pilot.finish().await;

        assert_eq!(report.connected, ["t-1", "t-2", "t-3"]);
        assert_eq!(report.finished, Finished::AllConnected);
        assert_eq!(
            pilot.opened.load(Ordering::SeqCst),
            1,
            "one browser launch for all companies"
        );
        assert_eq!(
            tab.redirects.load(Ordering::SeqCst),
            2,
            "the same tab was sent on twice"
        );
        let (status, page) = tab.last_page.lock().unwrap().clone().unwrap();
        assert_eq!(status, 200);
        assert!(
            page.contains("Connected 3 companies") && page.contains("Aquatiq AS"),
            "{page}"
        );

        assert_eq!(report.session.display(), "ima@example.com");
        let stored = r.store.connections().unwrap();
        assert_eq!(stored.len(), 3);
        let second = r.store.connection("t-2").unwrap().unwrap();
        assert_eq!(second.refresh_token, "refresh-t-2");
        assert!(
            second.has_scope("vismanet_erp_interactive_api:read") && second.access_is_fresh(3000)
        );
        assert_eq!(
            second.name.as_deref(),
            Some("Visma name of t-2"),
            "the company's own name is kept"
        );
        let text = terminal.text();
        assert!(
            text.contains("1 of 3 configured") && text.contains("3 of 3 configured"),
            "{text}"
        );
        assert!(
            text.contains("Aqua Pharma Inc (t-2)"),
            "the configured name is shown: {text}"
        );
        let opened_with = pilot.opened_with.lock().unwrap().clone();
        assert_eq!(opened_with.len(), 1);
        assert!(
            opened_with[0].contains("/start/") && !opened_with[0].contains("state=") && !opened_with[0].contains("code_challenge"),
            "the browser is opened on a one-time local address, not on the authorization URL: {opened_with:?}"
        );
        let first_url = tab.urls.lock().unwrap()[0].clone();
        assert!(
            first_url.contains("code_challenge_method=S256")
                && first_url.contains("offline_access"),
            "{first_url}"
        );
        assert!(!first_url.contains("s3cret"));
    }

    #[tokio::test]
    async fn a_second_configured_only_run_with_everything_connected_opens_nothing() {
        let r = rig().await;
        let opts = LoginOptions {
            configured_only: true,
            ..quick(three())
        };
        let first = Pilot::new(vec![approve("t-1"), approve("t-2"), approve("t-3")]);
        login(&r.cfg, &r.store, &opts, &first, &Terminal::default())
            .await
            .unwrap();
        first.finish().await;
        let again = login(&r.cfg, &r.store, &opts, &NoBrowser, &Terminal::default())
            .await
            .unwrap();
        assert_eq!(again.finished, Finished::AlreadyComplete);
        assert!(again.connected.is_empty());
    }

    #[tokio::test]
    async fn without_configured_only_the_screens_keep_coming_until_visma_has_no_more() {
        let r = rig().await;
        // Two companies are configured; the person has a third that is not, and then nothing new is offered.
        let known = known(&[("t-1", "Aquatiq AS"), ("t-2", "Aqua Pharma Inc")]);
        let pilot = Pilot::new(vec![
            approve("t-1"),
            approve("t-2"),
            approve("t-3"),
            approve("t-3"),
        ]);
        let terminal = Terminal::default();
        let report = login(&r.cfg, &r.store, &quick(known), &pilot, &terminal)
            .await
            .unwrap();
        let tab = pilot.finish().await;
        assert_eq!(report.connected, ["t-1", "t-2", "t-3"]);
        assert_eq!(report.finished, Finished::NothingNew);
        assert_eq!(tab.redirects.load(Ordering::SeqCst), 3);
        let text = terminal.text();
        assert!(
            text.contains("Visma name of t-3 (t-3)"),
            "an unconfigured company is shown by the name Visma gave: {text}"
        );
        let page = tab.last_page.lock().unwrap().clone().unwrap().1;
        assert!(
            page.contains("Connected 3 companies") && page.contains("Visma name of t-3"),
            "{page}"
        );
    }

    #[tokio::test]
    async fn once_connects_one_company_and_stops() {
        let r = rig().await;
        let pilot = Pilot::new(vec![approve("t-2")]);
        let opts = LoginOptions {
            once: true,
            ..quick(three())
        };
        let report = login(&r.cfg, &r.store, &opts, &pilot, &Terminal::default())
            .await
            .unwrap();
        let tab = pilot.finish().await;
        assert_eq!(
            (report.connected.as_slice(), &report.finished),
            (&["t-2".to_string()][..], &Finished::Once)
        );
        assert_eq!(tab.redirects.load(Ordering::SeqCst), 0);
        assert_eq!(tab.last_page.lock().unwrap().as_ref().unwrap().0, 200);
    }

    #[tokio::test]
    async fn a_company_offered_twice_means_there_are_no_more() {
        let r = rig().await;
        let pilot = Pilot::new(vec![approve("t-1"), approve("t-1")]);
        let report = login(
            &r.cfg,
            &r.store,
            &quick(Vec::new()),
            &pilot,
            &Terminal::default(),
        )
        .await
        .unwrap();
        pilot.finish().await;
        assert_eq!(report.connected, ["t-1"]);
        assert_eq!(report.finished, Finished::NothingNew);
        assert_eq!(r.store.connections().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn declining_the_next_company_keeps_the_ones_already_connected() {
        let r = rig().await;
        let pilot = Pilot::new(vec![approve("t-1"), Act::Decline]);
        let report = login(
            &r.cfg,
            &r.store,
            &quick(three()),
            &pilot,
            &Terminal::default(),
        )
        .await
        .unwrap();
        let tab = pilot.finish().await;
        assert_eq!(report.finished, Finished::Declined("access_denied".into()));
        assert_eq!(report.connected, ["t-1"]);
        assert!(r.store.connection("t-1").unwrap().is_some());
        let page = tab.last_page.lock().unwrap().clone().unwrap().1;
        assert!(page.contains("Sign-in stopped"), "{page}");
    }

    #[tokio::test]
    async fn declining_at_the_start_is_an_error_and_nothing_is_stored() {
        let r = rig().await;
        let pilot = Pilot::new(vec![Act::Decline]);
        let e = login(
            &r.cfg,
            &r.store,
            &quick(three()),
            &pilot,
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        let tab = pilot.finish().await;
        assert!(matches!(e, SignInError::Denied { .. }), "{e}");
        assert!(r.store.session().unwrap().is_none() && r.store.connections().unwrap().is_empty());
        assert!(tab
            .last_page
            .lock()
            .unwrap()
            .clone()
            .unwrap()
            .1
            .contains("the user said no"));
    }

    #[tokio::test]
    async fn a_second_person_cannot_be_added_to_a_sign_in() {
        let r = rig().await;
        let first = Pilot::new(vec![approve("t-1")]);
        login(
            &r.cfg,
            &r.store,
            &LoginOptions {
                once: true,
                ..quick(three())
            },
            &first,
            &Terminal::default(),
        )
        .await
        .unwrap();
        first.finish().await;
        let before = r.store.connection("t-1").unwrap().unwrap();

        let intruder = Pilot::new(vec![Act::Approve {
            tenant: "t-2",
            sub: "user-2",
        }]);
        let e = login(
            &r.cfg,
            &r.store,
            &quick(three()),
            &intruder,
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        intruder.finish().await;
        assert!(
            matches!(e, SignInError::DifferentUser { ref signed_in_as } if signed_in_as == "ima@example.com"),
            "{e}"
        );
        assert!(r.store.connection("t-2").unwrap().is_none());
        assert_eq!(r.store.connection("t-1").unwrap().unwrap(), before);
    }

    #[tokio::test]
    async fn a_sign_in_that_would_last_an_hour_is_refused() {
        let r = rig().await;
        let pilot = Pilot::new(vec![Act::ApproveWithoutRefreshToken("t-1")]);
        let e = login(
            &r.cfg,
            &r.store,
            &quick(three()),
            &pilot,
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        pilot.finish().await;
        assert!(matches!(e, SignInError::NoRefreshToken), "{e}");
        assert!(r.store.session().unwrap().is_none() && r.store.connections().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_stray_answer_is_ignored_and_the_real_one_still_completes_the_sign_in() {
        let r = rig().await;
        let pilot = Pilot::new(vec![Act::AnswerForAnotherRequest, approve("t-1")]);
        let opts = LoginOptions {
            once: true,
            ..quick(three())
        };
        let report = login(&r.cfg, &r.store, &opts, &pilot, &Terminal::default())
            .await
            .unwrap();
        let tab = pilot.finish().await;
        assert_eq!(report.connected, ["t-1"]);
        assert_eq!(tab.last_page.lock().unwrap().as_ref().unwrap().0, 200);
    }

    #[tokio::test]
    async fn an_address_pasted_for_another_request_is_refused() {
        let mut r = rig().await;
        r.cfg.redirect_uri = "https://app.example.com/callback".into();
        let terminal = Terminal {
            pasted: Mutex::new(VecDeque::from([Act::AnswerForAnotherRequest])),
            ..Terminal::default()
        };
        let opts = LoginOptions {
            paste: true,
            ..quick(three())
        };
        let e = login(&r.cfg, &r.store, &opts, &NoBrowser, &terminal)
            .await
            .unwrap_err();
        assert!(matches!(e, SignInError::StateMismatch), "{e}");
        assert!(r.store.connections().unwrap().is_empty());
    }

    #[tokio::test]
    async fn nobody_answering_ends_the_run_and_keeps_what_was_connected() {
        let r = rig().await;
        let opts = LoginOptions {
            round_timeout: Duration::from_millis(300),
            ..LoginOptions::new(three())
        };
        let silent = login(
            &r.cfg,
            &r.store,
            &opts,
            &Pilot::new(vec![]),
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(silent, SignInError::Timeout(_)), "{silent}");

        let pilot = Pilot::new(vec![approve("t-1")]);
        let report = login(&r.cfg, &r.store, &opts, &pilot, &Terminal::default())
            .await
            .unwrap();
        pilot.finish().await;
        assert_eq!(
            (report.connected.as_slice(), &report.finished),
            (&["t-1".to_string()][..], &Finished::TimedOut)
        );
    }

    #[tokio::test]
    async fn pressing_enter_stops_after_the_companies_already_connected() {
        let r = rig().await;
        let pilot = Pilot::new(vec![approve("t-1")]);
        let terminal = Terminal {
            press_enter_in_round: Mutex::new(Some(2)),
            ..Terminal::default()
        };
        let report = login(&r.cfg, &r.store, &quick(three()), &pilot, &terminal)
            .await
            .unwrap();
        pilot.finish().await;
        assert_eq!(
            (report.connected.as_slice(), &report.finished),
            (&["t-1".to_string()][..], &Finished::Stopped)
        );
        let early = Terminal {
            press_enter_in_round: Mutex::new(Some(1)),
            ..Terminal::default()
        };
        let e = login(
            &r.cfg,
            &Store::new(r._dir.path().join("other")),
            &quick(three()),
            &Pilot::new(vec![]),
            &early,
        )
        .await
        .unwrap_err();
        assert!(matches!(e, SignInError::Cancelled), "{e}");
    }

    #[tokio::test]
    async fn write_adds_the_write_scopes_to_the_request() {
        let mut r = rig().await;
        r.cfg.write_scopes = vec!["visma.net.erp.salesorder:write".into()];
        let pilot = Pilot::new(vec![approve("t-1")]);
        let opts = LoginOptions {
            write: true,
            once: true,
            ..quick(three())
        };
        login(&r.cfg, &r.store, &opts, &pilot, &Terminal::default())
            .await
            .unwrap();
        let tab = pilot.finish().await;
        let url = Url::parse(&tab.urls.lock().unwrap()[0]).unwrap();
        let scope = url
            .query_pairs()
            .find(|(k, _)| k == "scope")
            .unwrap()
            .1
            .into_owned();
        assert!(
            scope.contains("visma.net.erp.salesorder:write")
                && scope.contains("vismanet_erp_interactive_api:read"),
            "{scope}"
        );
        let plain = Pilot::new(vec![approve("t-1")]);
        login(
            &r.cfg,
            &Store::new(r._dir.path().join("plain")),
            &LoginOptions {
                once: true,
                ..quick(three())
            },
            &plain,
            &Terminal::default(),
        )
        .await
        .unwrap();
        let tab = plain.finish().await;
        assert!(!tab.urls.lock().unwrap()[0].contains("salesorder"));
    }

    #[tokio::test]
    async fn pasting_the_address_works_without_a_listener() {
        let mut r = rig().await;
        r.cfg.redirect_uri = "https://app.example.com/callback".into();
        let terminal = Terminal {
            pasted: Mutex::new(VecDeque::from([approve("t-1"), approve("t-2")])),
            ..Terminal::default()
        };
        let opts = LoginOptions {
            paste: true,
            configured_only: true,
            ..quick(known(&[("t-1", "A"), ("t-2", "B")]))
        };
        let report = login(&r.cfg, &r.store, &opts, &NoBrowser, &terminal)
            .await
            .unwrap();
        assert_eq!(
            (report.connected.as_slice(), &report.finished),
            (
                &["t-1".to_string(), "t-2".to_string()][..],
                &Finished::AllConnected
            )
        );
        assert!(terminal.text().contains("Open this address in a browser"));
        // Nothing pasted: the person gave up.
        let e = login(
            &r.cfg,
            &Store::new(r._dir.path().join("empty")),
            &quick(three()),
            &NoBrowser,
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(e, SignInError::Cancelled), "{e}");
    }

    #[tokio::test]
    async fn a_sign_in_made_with_another_application_is_not_extended() {
        let r = rig().await;
        r.store
            .save_session(&Session::new("user-1".into(), None, "another-app".into()))
            .unwrap();
        let e = login(
            &r.cfg,
            &r.store,
            &quick(three()),
            &NoBrowser,
            &Terminal::default(),
        )
        .await
        .unwrap_err();
        assert!(e.to_string().contains("another application"), "{e}");
        drop(r.server);
    }
}
