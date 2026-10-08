//! A test double that enforces session-core's REAL credential rules for the
//! managed-run lifecycle and the orchestration writes a delegation makes.
//!
//! # Why this exists
//!
//! The delegation tests in `agent.rs` run against `MockManagedRunLifecycle`, a
//! permissive double: it accepted whatever bearer the test's own expectation
//! named, and the expectation it named was "the fixed-scope service credential".
//! That is the opposite of what session-core demands (`StartManagedRun` refuses a
//! caller with no user id, `RecordRunOutput` needs `session:write`, which
//! Execution Core's service principal does not hold), so every delegation
//! registered a child run in every test and in no live run. The 2026-10-01 live
//! proof of wide research (run `01M3WR6SQMX8ZSZ8VYXN5XXWKQ`) logged
//! `PermissionDenied` for all three children, and nothing in the suite had a
//! chance to notice: the double and the loop agreed with each other.
//!
//! This double does not agree with the loop, it agrees with session-core. It
//! reads the bearer the way session-core's interceptor does (RS256, `kid`,
//! issuer, audience, then the user/service claim arms of `validate_identity`) and
//! applies the same predicates in the same order, each one copied from the
//! session-core source named beside it. session-core is a binary crate, so its
//! helpers cannot be linked from here; if one of the rules below changes there,
//! this file is where the divergence has to be repaired, which is why every rule
//! cites its origin.
//!
//! It also keeps the small amount of state those rules read: who owns a thread
//! and a run, which start keys exist, which edges are attached. A rule that
//! refuses records why in [`Snapshot::refusals`], so a test can say WHICH rule
//! refused instead of only that something did.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use jsonwebtoken::{
    decode, decode_header, encode, jwk::JwkSet, Algorithm, DecodingKey, EncodingKey, Header,
    Validation,
};
use mp_contracts::model_plane::v1 as pb;
use rsa::{
    pkcs8::{EncodePrivateKey, LineEnding},
    traits::PublicKeyParts,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tonic::{Request, Status};

use crate::session_terminal_auth::{ManagedRunTokenProvider, SessionTerminalTokenError};

const ISSUER: &str = "auth-core";
const KID: &str = "rules-double-key";
/// `SESSION_CORE_AUTH_AUDIENCE` in the stack.
const AUDIENCE: &str = "session-core";
/// `terminalization::EXECUTION_CORE_PRINCIPAL`.
pub(crate) const EXECUTION_CORE_PRINCIPAL: &str = "service:execution-core";

// ---------------------------------------------------------------------------
// Credentials shaped as Auth Core issues them
// ---------------------------------------------------------------------------

fn rsa_keypair() -> &'static (String, String, String) {
    static KEYS: OnceLock<(String, String, String)> = OnceLock::new();
    KEYS.get_or_init(|| {
        let private = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).expect("RSA key");
        let public = rsa::RsaPublicKey::from(&private);
        let b64 = &base64::engine::general_purpose::URL_SAFE_NO_PAD;
        (
            private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private PEM")
                .to_string(),
            b64.encode(public.n().to_bytes_be()),
            b64.encode(public.e().to_bytes_be()),
        )
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs()
}

fn sign(claims: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_owned());
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
    )
    .expect("JWT")
}

/// The user's delegated `aud=session-core` bearer: what the gateway verifies and
/// forwards, and what a run carries as `session_bearer`. Bound to one user; its
/// `zdr` claim is that user's retention posture.
pub(crate) fn user_bearer(org_id: &str, user_id: &str, zdr: bool) -> String {
    let now = now_secs();
    sign(&json!({
        "sub": user_id,
        "iss": ISSUER,
        "aud": AUDIENCE,
        "exp": now + 3600,
        "iat": now,
        "nbf": now - 5,
        "org_id": org_id,
        "user_id": user_id,
        "principal_type": "user",
        "scopes": [],
        "zdr": zdr,
    }))
}

/// A service credential as Auth Core mints it for a plane principal: no user,
/// the principal's own id, only the scopes requested for that mint, and the
/// retention posture the registry fixes for the audience
/// (`retentionByAudience["session-core"]`, `persistent` for execution-core).
pub(crate) fn service_token(org_id: &str, service_id: &str, scopes: &[&str]) -> String {
    let now = now_secs();
    sign(&json!({
        "sub": service_id,
        "iss": ISSUER,
        "aud": AUDIENCE,
        "exp": now + 3600,
        "iat": now,
        "nbf": now - 5,
        "org_id": org_id,
        "service_id": service_id,
        "principal_type": "service",
        "scopes": scopes,
        "zdr": false,
    }))
}

/// Execution Core's `ManagedRunTokenProvider` with real, signed credentials:
/// each token carries exactly the one scope its mint asked for, as
/// `SessionTerminalTokenProvider::mint` requests it of Auth Core. (The scope set
/// the principal registry GRANTS execution-core on this audience is
/// `approval:deliver`, `session:heartbeat` and `session:terminalize`; there is no
/// `session:write` to ask for.)
pub(crate) struct SignedServiceTokens;

#[tonic::async_trait]
impl ManagedRunTokenProvider for SignedServiceTokens {
    async fn terminalize_token(&self, org_id: &str) -> Result<String, SessionTerminalTokenError> {
        Ok(service_token(
            org_id,
            EXECUTION_CORE_PRINCIPAL,
            &["session:terminalize"],
        ))
    }

    async fn heartbeat_token(&self, org_id: &str) -> Result<String, SessionTerminalTokenError> {
        Ok(service_token(
            org_id,
            EXECUTION_CORE_PRINCIPAL,
            &["session:heartbeat"],
        ))
    }

    async fn scheduled_step_token(
        &self,
        org_id: &str,
    ) -> Result<String, SessionTerminalTokenError> {
        Ok(service_token(
            org_id,
            EXECUTION_CORE_PRINCIPAL,
            &["session:scheduled-step"],
        ))
    }
}

// ---------------------------------------------------------------------------
// session-core `auth.rs`: verification and identity
// ---------------------------------------------------------------------------

/// `auth::Claims`.
#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[allow(dead_code)]
    iss: String,
    #[allow(dead_code)]
    aud: Value,
    #[allow(dead_code)]
    exp: u64,
    #[allow(dead_code)]
    iat: u64,
    #[allow(dead_code)]
    nbf: u64,
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    service_id: String,
    #[serde(default)]
    principal_type: String,
    #[serde(default)]
    scopes: Vec<String>,
    zdr: bool,
}

/// `auth::VerifiedIdentity`.
#[derive(Clone, Debug)]
pub(crate) struct Caller {
    org_id: String,
    principal_id: String,
    user_id: Option<String>,
    scopes: Vec<String>,
    service: bool,
    zdr: bool,
}

impl Caller {
    /// `VerifiedIdentity::authorize_org`.
    fn authorize_org(&self, org_id: &str) -> Result<(), Status> {
        if org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if org_id != self.org_id {
            return Err(Status::permission_denied("tenant access denied"));
        }
        Ok(())
    }

    /// `VerifiedIdentity::authorize_user`.
    fn authorize_user(&self, user_id: &str) -> Result<(), Status> {
        if user_id.trim().is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }
        match self.user_id.as_deref() {
            Some(verified) if verified == user_id => Ok(()),
            Some(_) => Err(Status::permission_denied("user access denied")),
            None => Err(Status::permission_denied(
                "service identity cannot impersonate a user",
            )),
        }
    }

    /// `VerifiedIdentity::require_service_scope`.
    fn require_service_scope(&self, scope: &str) -> Result<(), Status> {
        if !self.service || !self.scopes.iter().any(|candidate| candidate == scope) {
            return Err(Status::permission_denied("service scope required"));
        }
        Ok(())
    }
}

/// `auth::validate_identity`: the only two shapes of caller session-core knows.
fn validate_identity(claims: Claims) -> Result<Caller, Status> {
    if claims.org_id.trim().is_empty() || claims.org_id.trim() != claims.org_id {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    match claims.principal_type.as_str() {
        "" | "user"
            if !claims.user_id.trim().is_empty()
                && claims.user_id == claims.user_id.trim()
                && claims.sub == claims.user_id
                && claims.service_id.is_empty() =>
        {
            Ok(Caller {
                org_id: claims.org_id,
                principal_id: claims.user_id.clone(),
                user_id: Some(claims.user_id),
                scopes: claims.scopes,
                service: false,
                zdr: claims.zdr,
            })
        }
        "service"
            if claims.user_id.is_empty()
                && !claims.service_id.trim().is_empty()
                && claims.service_id == claims.service_id.trim()
                && claims.sub == claims.service_id =>
        {
            Ok(Caller {
                org_id: claims.org_id,
                principal_id: claims.service_id,
                user_id: None,
                scopes: claims.scopes,
                service: true,
                zdr: claims.zdr,
            })
        }
        _ => Err(Status::unauthenticated("invalid caller credential")),
    }
}

/// `JwtVerifier::intercept` + `verify_token` + `auth::identity`: what session-core
/// makes of the `authorization` metadata on a request, before any RPC body runs.
pub(crate) fn caller_of<T>(request: &Request<T>) -> Result<Caller, Status> {
    let token = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified caller credential required"))?;
    let invalid = || Status::unauthenticated("invalid caller credential");
    let header = decode_header(token).map_err(|_| invalid())?;
    if header.alg != Algorithm::RS256 {
        return Err(invalid());
    }
    let (_, n, e) = rsa_keypair();
    let jwks: JwkSet = serde_json::from_value(json!({"keys": [{
        "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KID, "n": n, "e": e
    }]}))
    .expect("test JWKS");
    let jwk = header
        .kid
        .as_deref()
        .and_then(|kid| jwks.find(kid))
        .ok_or_else(invalid)?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| invalid())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[AUDIENCE]);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = 30;
    let claims = decode::<Claims>(token, &key, &validation)
        .map_err(|_| invalid())?
        .claims;
    if claims.aud.as_str() != Some(AUDIENCE) {
        return Err(invalid());
    }
    validate_identity(claims)
}

/// `auth::authorize_operation`.
fn authorize_operation(caller: &Caller, service_scope: &str) -> Result<(), Status> {
    if caller.zdr && !service_scope.ends_with(":read") {
        return Err(Status::failed_precondition(
            "ZDR credentials cannot access durable session writes",
        ));
    }
    if caller.service {
        caller.require_service_scope(service_scope)
    } else {
        Ok(())
    }
}

/// `auth::authorize_owner_row` for a row that is not a system run's: org equality
/// first, then a human must be the row's user and a service skips that check
/// (relied on by `RecordTerminalOutcome`).
fn authorize_owner_row(caller: &Caller, org_id: &str, user_id: &str) -> Result<(), Status> {
    caller.authorize_org(org_id)?;
    if !caller.service {
        caller.authorize_user(user_id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// session-core `terminalization.rs`
// ---------------------------------------------------------------------------

const MAX_START_KEY_BYTES: usize = 128;
const MAX_GOAL_BYTES: usize = 64 * 1024;
const MAX_RUN_ID_BYTES: usize = 128;
const MAX_AGENT_ID_BYTES: usize = 256;
const MAX_MODE_BYTES: usize = 64;
/// `grpc::MAX_RUN_OUTPUT_CHARS`.
const MAX_RUN_OUTPUT_CHARS: usize = 16_384;
/// `terminalization::ALLOWED_FAILURE_CODES`, which is also the migration's CHECK
/// (`0015`, widened by `0033`) minus `outcome_unknown`, which only the recovery
/// loop may set.
const ALLOWED_FAILURE_CODES: &[&str] = &[
    "browser_failed",
    "dispatch_rejected",
    "dispatch_unreachable",
    "execution_failed",
    "inference_failed",
    "provider_timeout",
    "provider_unavailable",
];

/// `terminalization::validate_token`.
fn validate_token(
    value: &str,
    label: &str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), Status> {
    if (value.is_empty() && !allow_empty)
        || value.len() > maximum
        || value != value.trim()
        || value.chars().any(char::is_control)
    {
        return Err(Status::invalid_argument(format!("invalid {label}")));
    }
    Ok(())
}

/// `terminalization::validate_terminal_outcome`: a receipt's whole public grammar.
/// Shared with the permissive lifecycle mock so that even a test that does not
/// verify credentials cannot settle a run under a code session-core would refuse.
pub(crate) fn validate_terminal_outcome(outcome: i32, failure_code: &str) -> Result<(), Status> {
    let outcome =
        pb::TerminalOutcome::try_from(outcome).unwrap_or(pb::TerminalOutcome::Unspecified);
    match outcome {
        pb::TerminalOutcome::Completed if failure_code.is_empty() => Ok(()),
        pb::TerminalOutcome::Completed => Err(Status::invalid_argument(
            "completed terminal outcome cannot include a failure code",
        )),
        pb::TerminalOutcome::Failed
            if ALLOWED_FAILURE_CODES.contains(&failure_code)
                && failure_code.len() <= 64
                && failure_code == failure_code.trim() =>
        {
            Ok(())
        }
        pb::TerminalOutcome::Failed => Err(Status::invalid_argument(
            "terminal failure code is not allowlisted",
        )),
        pb::TerminalOutcome::Unspecified => {
            Err(Status::invalid_argument("terminal outcome is required"))
        }
    }
}

/// `terminalization::validate_start_key`.
fn validate_start_key(value: &str) -> Result<(), Status> {
    if value.is_empty()
        || value.len() > MAX_START_KEY_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(Status::invalid_argument("invalid start_key"));
    }
    Ok(())
}

/// `terminalization::source_from_wire`.
fn source_from_wire(value: i32) -> Result<pb::ManagedRunSource, Status> {
    let source = pb::ManagedRunSource::try_from(value).unwrap_or(pb::ManagedRunSource::Unspecified);
    if source == pb::ManagedRunSource::Unspecified {
        return Err(Status::invalid_argument(
            "managed terminal source is required",
        ));
    }
    Ok(source)
}

/// `terminalization::authorize_managed_source`: a service credential holding the
/// right scope AND being the workload that owns the source.
fn authorize_managed_source(
    caller: &Caller,
    source: pb::ManagedRunSource,
    required_scope: &str,
) -> Result<(), Status> {
    caller.require_service_scope(required_scope)?;
    let expected_principal = match source {
        pb::ManagedRunSource::GatewayDirect
        | pb::ManagedRunSource::GatewayAgentDispatchRejected
        | pb::ManagedRunSource::GatewayBrowser => "service:model-gateway",
        pb::ManagedRunSource::ExecutionAgent => EXECUTION_CORE_PRINCIPAL,
        pb::ManagedRunSource::ExecutionBrowser => {
            return Err(Status::permission_denied(
                "execution browser terminalizer is not enabled",
            ));
        }
        pb::ManagedRunSource::Unspecified => {
            return Err(Status::invalid_argument(
                "managed terminal source is required",
            ));
        }
    };
    if caller.principal_id != expected_principal {
        return Err(Status::permission_denied(
            "managed terminal source requires its owning workload identity",
        ));
    }
    Ok(())
}

/// What a run row carries for these rules.
#[derive(Clone, Debug)]
pub(crate) struct RunRow {
    pub org_id: String,
    pub user_id: String,
    pub thread_id: String,
    pub parent_run_id: String,
    pub agent_id: String,
    pub goal: String,
    pub start_key: String,
    pub final_output: Option<String>,
    /// `(outcome, failure_code)` of the first terminal receipt. Immutable once set.
    pub terminal: Option<(i32, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edge {
    pub parent_run_id: String,
    pub child_run_id: String,
    pub role: i32,
    pub goal_summary: String,
    pub batch_id: String,
}

/// A run-scoped event the double accepted: `(run_id, kind, subject, status)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecordedEvent {
    pub run_id: String,
    pub kind: &'static str,
    pub subject: String,
    pub status: String,
}

/// One rule that said no: which RPC, which gRPC code, and session-core's own
/// words for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub rpc: &'static str,
    pub code: tonic::Code,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Snapshot {
    pub runs: BTreeMap<String, RunRow>,
    pub edges: Vec<Edge>,
    pub events: Vec<RecordedEvent>,
    pub heartbeats: Vec<String>,
    pub refusals: Vec<Refusal>,
}

impl Snapshot {
    /// The runs that were started for a delegation rather than seeded as the
    /// parent: everything with a `parent_run_id`.
    pub(crate) fn child_runs(&self) -> Vec<(&String, &RunRow)> {
        self.runs
            .iter()
            .filter(|(_, row)| !row.parent_run_id.is_empty())
            .collect()
    }
}

#[derive(Default)]
struct State {
    threads: HashMap<String, (String, String)>,
    runs: BTreeMap<String, RunRow>,
    start_keys: HashMap<(String, String, String), String>,
    edges: Vec<Edge>,
    events: Vec<RecordedEvent>,
    heartbeats: Vec<String>,
    refusals: Vec<Refusal>,
    next_run: u32,
}

/// session-core's managed-run lifecycle and orchestration write rules, over a
/// tiny in-memory ownership model.
#[derive(Default)]
pub(crate) struct SessionCoreRules {
    state: Mutex<State>,
}

impl SessionCoreRules {
    /// A thread owned by `user_id` in `org_id`, as the gateway would have created
    /// it before starting the parent run.
    pub(crate) fn seed_thread(&self, thread_id: &str, org_id: &str, user_id: &str) {
        self.state.lock().unwrap().threads.insert(
            thread_id.to_owned(),
            (org_id.to_owned(), user_id.to_owned()),
        );
    }

    /// The parent run, as the gateway started it under the user's own bearer.
    pub(crate) fn seed_run(&self, run_id: &str, thread_id: &str, org_id: &str, user_id: &str) {
        self.state.lock().unwrap().runs.insert(
            run_id.to_owned(),
            RunRow {
                org_id: org_id.to_owned(),
                user_id: user_id.to_owned(),
                thread_id: thread_id.to_owned(),
                parent_run_id: String::new(),
                agent_id: String::new(),
                goal: String::new(),
                start_key: String::new(),
                final_output: None,
                terminal: None,
            },
        );
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        let state = self.state.lock().unwrap();
        Snapshot {
            runs: state.runs.clone(),
            edges: state.edges.clone(),
            events: state.events.clone(),
            heartbeats: state.heartbeats.clone(),
            refusals: state.refusals.clone(),
        }
    }

    /// Run a rule and keep its refusal, if any, where a test can read it.
    fn recorded<T>(
        &self,
        rpc: &'static str,
        outcome: impl FnOnce(&mut State) -> Result<T, Status>,
    ) -> Result<T, Status> {
        let mut state = self.state.lock().unwrap();
        let result = outcome(&mut state);
        if let Err(status) = &result {
            state.refusals.push(Refusal {
                rpc,
                code: status.code(),
                message: status.message().to_owned(),
            });
        }
        result
    }

    /// `grpc::authorize_thread_owner` with `OwnerIntent::Mutate`.
    fn authorize_thread_owner(
        state: &State,
        caller: &Caller,
        thread_id: &str,
    ) -> Result<(), Status> {
        let (org_id, user_id) = state
            .threads
            .get(thread_id)
            .ok_or_else(|| Status::not_found("thread not found"))?;
        authorize_owner_row(caller, org_id, user_id)
    }

    /// `grpc::authorize_run_owner` with `OwnerIntent::Mutate`.
    fn authorize_run_owner(state: &State, caller: &Caller, run_id: &str) -> Result<(), Status> {
        let row = state
            .runs
            .get(run_id)
            .ok_or_else(|| Status::not_found("run not found"))?;
        authorize_owner_row(caller, &row.org_id, &row.user_id)
    }

    /// `ManagedRunLifecycle::start_managed_run` and
    /// `terminalization::start_managed_run_inner`.
    pub(crate) fn start_managed_run(
        &self,
        caller: &Caller,
        mut req: pb::StartManagedRunRequest,
    ) -> Result<pb::StartManagedRunResponse, Status> {
        self.recorded("start_managed_run", |state| {
            caller.authorize_org(&req.org_id)?;
            let user_id = caller.user_id.clone().ok_or_else(|| {
                Status::permission_denied("user-bound managed-run credential required")
            })?;
            req.user_id.clone_from(&user_id);
            // ZDR users take the metadata-only branch; everyone else needs the
            // write operation and the thread.
            if !caller.zdr {
                authorize_operation(caller, "session:write")?;
                Self::authorize_thread_owner(state, caller, &req.thread_id)?;
            }

            validate_start_key(&req.start_key)?;
            if req.goal.len() > MAX_GOAL_BYTES || req.goal.chars().any(char::is_control) {
                return Err(Status::invalid_argument("invalid goal"));
            }
            let source = source_from_wire(req.terminal_source)?;
            if !matches!(
                source,
                pb::ManagedRunSource::GatewayDirect
                    | pb::ManagedRunSource::ExecutionAgent
                    | pb::ManagedRunSource::GatewayBrowser
            ) {
                return Err(Status::invalid_argument(
                    "unsupported managed terminal source",
                ));
            }
            if caller.zdr {
                if !req.thread_id.is_empty() {
                    return Err(Status::failed_precondition(
                        "ZDR managed runs require a fresh metadata-only thread",
                    ));
                }
            } else {
                validate_token(&req.thread_id, "thread_id", MAX_RUN_ID_BYTES, false)?;
                validate_token(&req.parent_run_id, "parent_run_id", MAX_RUN_ID_BYTES, true)?;
                validate_token(&req.agent_id, "agent_id", MAX_AGENT_ID_BYTES, false)?;
                validate_token(&req.mode, "mode", MAX_MODE_BYTES, false)?;
            }

            // `UNIQUE (org_id, user_id, start_key)` and `existing_start_matches`.
            let key = (req.org_id.clone(), user_id.clone(), req.start_key.clone());
            if let Some(existing_id) = state.start_keys.get(&key).cloned() {
                let existing = &state.runs[&existing_id];
                if existing.thread_id != req.thread_id
                    || existing.parent_run_id != req.parent_run_id
                    || existing.agent_id != req.agent_id
                    || existing.goal != req.goal
                {
                    return Err(Status::already_exists(
                        "managed start key is bound to different immutable input",
                    ));
                }
                return Ok(pb::StartManagedRunResponse {
                    run_id: existing_id,
                    created_at: None,
                    terminal_step_id: "execution-core-agent-final".to_owned(),
                    already_started: true,
                    thread_id: existing.thread_id.clone(),
                });
            }

            // `insert_managed_run_started`: the run is created only from a thread
            // that this exact org and user own.
            let thread_id = if caller.zdr {
                format!("zdr-thread-{}", state.next_run + 1)
            } else {
                match state.threads.get(&req.thread_id) {
                    Some((org_id, owner)) if org_id == &req.org_id && owner == &user_id => {
                        req.thread_id.clone()
                    }
                    _ => {
                        return Err(Status::not_found(
                            "thread not found for managed run creation",
                        ))
                    }
                }
            };
            state.next_run += 1;
            let run_id = format!("child-run-{}", state.next_run);
            state.runs.insert(
                run_id.clone(),
                RunRow {
                    org_id: req.org_id.clone(),
                    user_id,
                    thread_id: thread_id.clone(),
                    parent_run_id: req.parent_run_id.clone(),
                    agent_id: req.agent_id.clone(),
                    goal: req.goal.clone(),
                    start_key: req.start_key.clone(),
                    final_output: None,
                    terminal: None,
                },
            );
            state.start_keys.insert(key, run_id.clone());
            Ok(pb::StartManagedRunResponse {
                run_id,
                created_at: None,
                terminal_step_id: "execution-core-agent-final".to_owned(),
                already_started: false,
                thread_id,
            })
        })
    }

    /// `ManagedRunLifecycle::record_terminal_outcome`.
    pub(crate) fn record_terminal_outcome(
        &self,
        caller: &Caller,
        req: pb::RecordTerminalOutcomeRequest,
    ) -> Result<pb::RecordTerminalOutcomeResponse, Status> {
        self.recorded("record_terminal_outcome", |state| {
            let source = source_from_wire(req.source)?;
            authorize_managed_source(caller, source, "session:terminalize")?;
            Self::authorize_run_owner(state, caller, &req.run_id)?;
            validate_terminal_outcome(req.outcome, &req.failure_code)?;
            let row = state.runs.get_mut(&req.run_id).expect("owner checked");
            // Immutable: a second receipt returns the first.
            let already_applied = row.terminal.is_some();
            if !already_applied {
                row.terminal = Some((req.outcome, req.failure_code.clone()));
            }
            Ok(pb::RecordTerminalOutcomeResponse {
                run_id: req.run_id.clone(),
                source: req.source,
                terminal_step_id: "execution-core-agent-final".to_owned(),
                step_index: 1,
                receipt_id: format!("receipt-{}", req.run_id),
                applied_at: None,
                already_applied,
                reconciliation_required: false,
            })
        })
    }

    /// `ManagedRunLifecycle::record_run_output`.
    pub(crate) fn record_run_output(
        &self,
        caller: &Caller,
        req: pb::RecordRunOutputRequest,
    ) -> Result<pb::RecordRunOutputResponse, Status> {
        self.recorded("record_run_output", |state| {
            if caller.zdr {
                return Err(Status::permission_denied(
                    "a zero-retention run has no durable output",
                ));
            }
            authorize_operation(caller, "session:write")?;
            Self::authorize_run_owner(state, caller, &req.run_id)?;
            let output = req.output.trim();
            if output.is_empty() {
                return Err(Status::invalid_argument("output is required"));
            }
            let stored: String = output.chars().take(MAX_RUN_OUTPUT_CHARS).collect();
            let stored_chars = u32::try_from(stored.chars().count()).unwrap_or(u32::MAX);
            state
                .runs
                .get_mut(&req.run_id)
                .expect("owner checked")
                .final_output = Some(stored);
            Ok(pb::RecordRunOutputResponse {
                run_id: req.run_id.clone(),
                stored_chars,
            })
        })
    }

    /// `ManagedRunLifecycle::heartbeat_managed_run`.
    pub(crate) fn heartbeat_managed_run(
        &self,
        caller: &Caller,
        req: pb::HeartbeatManagedRunRequest,
    ) -> Result<pb::HeartbeatManagedRunResponse, Status> {
        self.recorded("heartbeat_managed_run", |state| {
            let source = source_from_wire(req.source)?;
            authorize_managed_source(caller, source, "session:heartbeat")?;
            Self::authorize_run_owner(state, caller, &req.run_id)?;
            validate_token(&req.run_id, "run_id", MAX_RUN_ID_BYTES, false)?;
            state.heartbeats.push(req.run_id.clone());
            let already_terminal = state.runs[&req.run_id].terminal.is_some();
            Ok(pb::HeartbeatManagedRunResponse {
                renewed_until: None,
                already_terminal,
            })
        })
    }

    /// `OrchestrationCoreService::attach_subagent`.
    pub(crate) fn attach_subagent(
        &self,
        caller: &Caller,
        req: pb::AttachSubagentRequest,
    ) -> Result<pb::AttachSubagentResponse, Status> {
        self.recorded("attach_subagent", |state| {
            authorize_operation(caller, "orchestration:write")?;
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            if req.parent_run_id.is_empty() || req.child_run_id.is_empty() {
                return Err(Status::invalid_argument(
                    "parent_run_id and child_run_id are required",
                ));
            }
            Self::authorize_thread_owner(state, caller, &req.thread_id)?;
            Self::authorize_run_owner(state, caller, &req.parent_run_id)?;
            Self::authorize_run_owner(state, caller, &req.child_run_id)?;
            // `subagent_edges` has a composite primary key and a UNIQUE child.
            if state.edges.iter().any(|edge| {
                edge.child_run_id == req.child_run_id
                    || (edge.parent_run_id == req.parent_run_id
                        && edge.child_run_id == req.child_run_id)
            }) {
                return Err(Status::internal(
                    "duplicate key value violates unique constraint \"subagent_edges_pkey\"",
                ));
            }
            state.edges.push(Edge {
                parent_run_id: req.parent_run_id.clone(),
                child_run_id: req.child_run_id.clone(),
                role: req.role,
                goal_summary: req.goal_summary.clone(),
                batch_id: req.batch_id.clone(),
            });
            Ok(pb::AttachSubagentResponse::default())
        })
    }

    /// `OrchestrationCoreService::record_orchestration_event`, for the events a
    /// delegation emits. `orchestration_grpc::event_run_id` names the run an event
    /// is routed to: a stop is run-scoped only through its `parent_run_id`.
    pub(crate) fn record_orchestration_event(
        &self,
        caller: &Caller,
        req: pb::RecordOrchestrationEventRequest,
    ) -> Result<pb::RecordOrchestrationEventResponse, Status> {
        self.recorded("record_orchestration_event", |state| {
            authorize_operation(caller, "orchestration:write")?;
            let event = req
                .event
                .ok_or_else(|| Status::invalid_argument("event is required"))?
                .event
                .ok_or_else(|| Status::invalid_argument("event payload is required"))?;
            let (run_id, kind, subject, status) = match event {
                pb::orchestration_event::Event::PlanTransitioned(p) => {
                    (p.run_id, "plan_transitioned", p.plan_id, String::new())
                }
                pb::orchestration_event::Event::SubagentStopped(p) => (
                    p.parent_run_id,
                    "subagent_stopped",
                    p.child_run_id,
                    p.status,
                ),
                _ => (
                    String::new(),
                    "unsupported_by_this_double",
                    String::new(),
                    String::new(),
                ),
            };
            if run_id.is_empty() {
                return Err(Status::invalid_argument(
                    "event must carry a non-empty run_id",
                ));
            }
            Self::authorize_run_owner(state, caller, &run_id)?;
            state.events.push(RecordedEvent {
                run_id,
                kind,
                subject,
                status,
            });
            Ok(pb::RecordOrchestrationEventResponse {
                event_id: format!("evt-{}", state.events.len()),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    fn start_request() -> pb::StartManagedRunRequest {
        pb::StartManagedRunRequest {
            thread_id: "thread-1".to_owned(),
            parent_run_id: "run-1".to_owned(),
            agent_id: "task".to_owned(),
            goal: "find the thing".to_owned(),
            mode: "execute".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            start_key: "run-1_tool_1".to_owned(),
            terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
        }
    }

    fn seeded() -> SessionCoreRules {
        let rules = SessionCoreRules::default();
        rules.seed_thread("thread-1", "org-1", "user-1");
        rules.seed_run("run-1", "thread-1", "org-1", "user-1");
        rules
    }

    /// The defect itself, stated against the rules: Execution Core's service
    /// credential has no user, and `StartManagedRun` is user-bound.
    #[test]
    fn a_service_credential_cannot_start_a_managed_run() {
        let rules = seeded();
        let caller = caller_of(&request_with(&service_token(
            "org-1",
            EXECUTION_CORE_PRINCIPAL,
            &["session:terminalize"],
        )))
        .expect("a well-formed service credential verifies");
        let refusal = rules
            .start_managed_run(&caller, start_request())
            .expect_err("a service credential has no user to own the run");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);
        assert_eq!(
            refusal.message(),
            "user-bound managed-run credential required"
        );
    }

    #[test]
    fn the_users_own_bearer_starts_a_run_that_the_user_owns() {
        let rules = seeded();
        let caller = caller_of(&request_with(&user_bearer("org-1", "user-1", false))).unwrap();
        let started = rules
            .start_managed_run(&caller, start_request())
            .expect("the owning user may start a run in their own thread");
        assert!(!started.already_started);
        let snapshot = rules.snapshot();
        let row = &snapshot.runs[&started.run_id];
        assert_eq!(row.user_id, "user-1", "the owner comes from the credential");
        assert_eq!(row.parent_run_id, "run-1");

        // The same start key replays the same run.
        let replay = rules.start_managed_run(&caller, start_request()).unwrap();
        assert!(replay.already_started);
        assert_eq!(replay.run_id, started.run_id);
    }

    #[test]
    fn another_users_bearer_cannot_start_a_run_in_the_thread() {
        let rules = seeded();
        let caller = caller_of(&request_with(&user_bearer("org-1", "user-2", false))).unwrap();
        let refusal = rules
            .start_managed_run(&caller, start_request())
            .expect_err("user-2 does not own thread-1");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);
    }

    #[test]
    fn a_zero_retention_user_cannot_start_a_run_in_an_existing_thread() {
        let rules = seeded();
        let caller = caller_of(&request_with(&user_bearer("org-1", "user-1", true))).unwrap();
        let refusal = rules
            .start_managed_run(&caller, start_request())
            .expect_err("ZDR takes a fresh metadata-only thread");
        assert_eq!(refusal.code(), tonic::Code::FailedPrecondition);
    }

    /// Execution Core's service principal holds no `session:write`, so it cannot
    /// store a run's answer under its own credential either.
    #[test]
    fn a_service_credential_cannot_record_a_run_answer() {
        let rules = seeded();
        let owner = caller_of(&request_with(&user_bearer("org-1", "user-1", false))).unwrap();
        let child = rules
            .start_managed_run(&owner, start_request())
            .unwrap()
            .run_id;
        let service = caller_of(&request_with(&service_token(
            "org-1",
            EXECUTION_CORE_PRINCIPAL,
            &["session:terminalize"],
        )))
        .unwrap();
        let refusal = rules
            .record_run_output(
                &service,
                pb::RecordRunOutputRequest {
                    run_id: child.clone(),
                    output: "an answer".to_owned(),
                },
            )
            .expect_err("the service holds no session:write");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);
        assert_eq!(refusal.message(), "service scope required");

        rules
            .record_run_output(
                &owner,
                pb::RecordRunOutputRequest {
                    run_id: child,
                    output: "an answer".to_owned(),
                },
            )
            .expect("the owning user may");
    }

    /// The code a failed child is settled under has to be one of the receipt
    /// vocabulary's words, or session-core never records the receipt.
    #[test]
    fn a_failed_receipt_must_use_an_allowlisted_failure_code() {
        let rules = seeded();
        let owner = caller_of(&request_with(&user_bearer("org-1", "user-1", false))).unwrap();
        let child = rules
            .start_managed_run(&owner, start_request())
            .unwrap()
            .run_id;
        let service = caller_of(&request_with(&service_token(
            "org-1",
            EXECUTION_CORE_PRINCIPAL,
            &["session:terminalize"],
        )))
        .unwrap();
        let receipt = |code: &str| pb::RecordTerminalOutcomeRequest {
            run_id: child.clone(),
            source: pb::ManagedRunSource::ExecutionAgent as i32,
            outcome: pb::TerminalOutcome::Failed as i32,
            failure_code: code.to_owned(),
        };
        let refusal = rules
            .record_terminal_outcome(&service, receipt("subagent_failed"))
            .expect_err("not in the vocabulary");
        assert_eq!(refusal.code(), tonic::Code::InvalidArgument);
        rules
            .record_terminal_outcome(&service, receipt("execution_failed"))
            .expect("the generic execution failure is");
    }

    #[test]
    fn a_receipt_needs_the_owning_workloads_service_identity() {
        let rules = seeded();
        let user = caller_of(&request_with(&user_bearer("org-1", "user-1", false))).unwrap();
        let refusal = rules
            .record_terminal_outcome(
                &user,
                pb::RecordTerminalOutcomeRequest {
                    run_id: "run-1".to_owned(),
                    source: pb::ManagedRunSource::ExecutionAgent as i32,
                    outcome: pb::TerminalOutcome::Completed as i32,
                    failure_code: String::new(),
                },
            )
            .expect_err("a user is not a terminalizer");
        assert_eq!(refusal.code(), tonic::Code::PermissionDenied);

        let gateway = caller_of(&request_with(&service_token(
            "org-1",
            "service:model-gateway",
            &["session:terminalize"],
        )))
        .unwrap();
        let refusal = rules
            .record_terminal_outcome(
                &gateway,
                pb::RecordTerminalOutcomeRequest {
                    run_id: "run-1".to_owned(),
                    source: pb::ManagedRunSource::ExecutionAgent as i32,
                    outcome: pb::TerminalOutcome::Completed as i32,
                    failure_code: String::new(),
                },
            )
            .expect_err("the source belongs to execution-core");
        assert_eq!(
            refusal.message(),
            "managed terminal source requires its owning workload identity"
        );
    }

    #[test]
    fn a_garbled_or_wrong_audience_credential_is_unauthenticated() {
        assert_eq!(
            caller_of(&request_with("not-a-jwt")).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );
        // The test doubles' stand-in bearers are not credentials at all.
        assert_eq!(
            caller_of(&request_with("session-token"))
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let now = now_secs();
        let wrong_audience = sign(&json!({
            "sub": "user-1", "iss": ISSUER, "aud": "execution-core",
            "exp": now + 60, "iat": now, "nbf": now - 5, "org_id": "org-1",
            "user_id": "user-1", "principal_type": "user", "zdr": false,
        }));
        assert_eq!(
            caller_of(&request_with(&wrong_audience))
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
    }
}
