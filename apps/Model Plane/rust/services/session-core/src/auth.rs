//! Fail-closed Auth Core verification for session-core's gRPC boundary.
//!
//! Verification material is fetched before the listener starts. The tonic
//! interceptor then performs RS256 verification synchronously and attaches a
//! canonical identity to request extensions. No caller-supplied org/user/actor
//! field becomes authority by itself.

use std::{collections::HashMap, fmt, sync::Arc, time::Duration};

use base64::Engine as _;
use jsonwebtoken::{decode, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use mp_contracts::retention::{
    resolve_turn, ClaimsView, Resolution, RouteView, WaiverView, WAIVER_CONTRACT_VERSION,
};
use serde::Deserialize;
use tonic::{Request, Status};

const MAX_JWKS_BYTES: usize = 1_048_576;
const DEFAULT_LEEWAY_SECS: u64 = 30;
const DEFAULT_DATA_PLANE_AUDIENCE: &str = "data-plane";

/// Metadata key carrying the caller's separately delegated `aud=data-plane`
/// credential. Mirrors the key Model Gateway and Execution Core already use
/// (`model-gateway/src/sse.rs`, `execution-core/src/auth.rs`) so one delegation
/// convention covers every Model Plane hop into Data Plane v2.
pub const DATA_PLANE_AUTH_METADATA_KEY: &str = "x-data-plane-authorization";

#[derive(Clone)]
pub struct JwtVerifier {
    issuer: Arc<str>,
    audience: Arc<str>,
    data_plane_audience: Arc<str>,
    leeway_secs: u64,
    keys: Arc<HashMap<String, DecodingKey>>,
    /// Whether an `api_key` token is a principal here ([`API_KEY_PRINCIPAL_ENV`]).
    accept_api_key: bool,
}

impl fmt::Debug for JwtVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JwtVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("data_plane_audience", &self.data_plane_audience)
            .field("leeway_secs", &self.leeway_secs)
            .field("accept_api_key", &self.accept_api_key)
            .field("key_count", &self.keys.len())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PrincipalKind {
    User,
    Service,
    /// EP-12 S4: an API key exchanged by Auth Core for a short-lived token. It exists only while
    /// [`API_KEY_PRINCIPAL_ENV`] is on, and reaches only [`API_KEY_METHODS`].
    ApiKey,
}

/// `SESSION_CORE_API_KEY_PRINCIPAL`: admit the `api_key` principal type (EP-12 S4). Default off;
/// off, an `api_key` token is `unauthenticated` exactly as before.
pub(crate) const API_KEY_PRINCIPAL_ENV: &str = "SESSION_CORE_API_KEY_PRINCIPAL";

/// Whether the `api_key` switch is on. The repo's strict parser reads it: only the literal `true`.
#[must_use]
pub(crate) fn api_key_principal_enabled(raw: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(raw)
}

/// `SKILL_ORG_ADMIN_CHECK`: a person writing an org-wide skill (`UpsertAgentSkill`, origin
/// `user`, scope `""` or `org`) must hold the org admin role, as capability-core's
/// `registry_apis.go` requires for a human org write (D2-509). Default off; off, a person holding
/// `session:skills:write` writes an org-wide skill as it always could.
pub(crate) const SKILL_ORG_ADMIN_CHECK_ENV: &str = "SKILL_ORG_ADMIN_CHECK";

/// Whether the org-admin check on person-written org skills is on. The repo's strict parser
/// reads it: only the literal `true`.
#[must_use]
pub(crate) fn skill_org_admin_check_enabled(raw: Option<&str>) -> bool {
    mp_contracts::loop_fold::switch_from(raw)
}

/// `sub` of an `api_key` principal is this prefix plus its `key_id`. The shape rules below are
/// model-gateway's and inference-core's `is_well_formed_api_key`, so one token reads the same in
/// every Model Plane service.
const API_KEY_SUBJECT_PREFIX: &str = "apikey:";
const MAX_API_KEY_ID_LEN: usize = 128;
const MAX_ACTING_USER_ID_LEN: usize = 256;

/// Whether a row's `user_id` names an API key, which owns its threads and runs (EP-12 D2). Auth
/// Core actor ids are base62, so a person's id never carries the `:` this prefix does.
#[must_use]
pub(crate) fn is_api_key_owner(user_id: &str) -> bool {
    user_id.starts_with(API_KEY_SUBJECT_PREFIX)
}

/// The one scope Auth Core's API-key exchange grants for `aud=session-core` (EP-12 D3). A key
/// token without it reaches no method.
pub(crate) const API_KEY_SCOPE: &str = "agent:runs";

/// The only RPCs an `api_key` principal may call, as gRPC paths, each with whether it writes
/// durable state. A key creates and reads its own threads and runs and nothing else (EP-12 D2,
/// D5): memory, approvals, Space, attention and every RPC added later are refused because they
/// are not listed. A key's run is terminalized by model-gateway's own service credential, never
/// by the key, so no terminal RPC is listed.
pub(crate) const API_KEY_METHODS: &[(&str, bool)] = &[
    ("/model_plane.v1.SessionCore/CreateThread", true),
    ("/model_plane.v1.SessionCore/AppendMessage", true),
    ("/model_plane.v1.SessionCore/StartRun", true),
    ("/model_plane.v1.SessionCore/ListConversation", false),
    ("/model_plane.v1.RunService/GetRun", false),
    ("/model_plane.v1.RunService/ListRuns", false),
    ("/model_plane.v1.RunService/CancelRun", true),
];

/// Whether `path` is one of [`API_KEY_METHODS`] and, when it is, whether it writes. Exact match
/// only: no trimming, no case folding.
#[must_use]
fn api_key_method_allowed(path: &str) -> Option<bool> {
    API_KEY_METHODS
        .iter()
        .find(|(method, _)| *method == path)
        .map(|(_, writes)| *writes)
}

/// The gRPC path of the call being served, stamped by [`RpcPathStamp`]. tonic hands an
/// interceptor the metadata and extensions of a call but not its URI, so the path rides as an
/// extension that no client can set.
#[derive(Clone, Debug)]
struct RpcPath(Arc<str>);

/// A served gRPC service whose interceptor can see which method is called. Only services that
/// hold an [`API_KEY_METHODS`] entry are wrapped: on any other service the path is absent and
/// [`JwtVerifier::intercept`] refuses a key, so a service stays closed to keys by default.
#[derive(Clone)]
pub(crate) struct RpcPathStamp<S>(S);

impl<S> RpcPathStamp<S> {
    pub(crate) fn new(inner: S) -> Self {
        Self(inner)
    }
}

impl<S, B> tonic::codegen::Service<tonic::codegen::http::Request<B>> for RpcPathStamp<S>
where
    S: tonic::codegen::Service<tonic::codegen::http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.0.poll_ready(context)
    }

    fn call(&mut self, mut request: tonic::codegen::http::Request<B>) -> Self::Future {
        let path = RpcPath(Arc::from(request.uri().path()));
        request.extensions_mut().insert(path);
        self.0.call(request)
    }
}

impl<S: tonic::server::NamedService> tonic::server::NamedService for RpcPathStamp<S> {
    const NAME: &'static str = S::NAME;
}

/// Admit an `api_key` principal on one call: the method must be listed, the token must carry
/// [`API_KEY_SCOPE`], and a ZDR key is refused on every write (EP-12 D6), since a key's threads
/// and runs are durable rows by definition.
#[allow(clippy::result_large_err)]
fn authorize_api_key_call(caller: &VerifiedIdentity, path: Option<&str>) -> Result<(), Status> {
    let Some(writes) = path.and_then(api_key_method_allowed) else {
        return Err(Status::permission_denied("an API key may not call this method"));
    };
    if !caller.has_scope(API_KEY_SCOPE) {
        return Err(Status::permission_denied("API key scope required"));
    }
    if writes && caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("agent_api")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// The `zdr_waiver` claim of a persistent token, after verification.
///
/// An Auth Core decision stamps it onto a token only when the org is ZDR and
/// one user-owned connection that cannot give ZDR was acknowledged by the
/// session user. It binds exactly one provider and one connection, and names
/// the acknowledgement it rests on, which is what a run's immutable receipt
/// records. It is constructed only by [`JwtVerifier`] from a verified token.
#[derive(Clone, Eq, PartialEq)]
pub struct ZdrWaiver {
    provider: String,
    connection_id: String,
    ack_id: String,
}

impl ZdrWaiver {
    /// The canonical provider the waiver binds.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// The one user-owned connection the waiver binds.
    #[must_use]
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    /// The acknowledgement of that connection the waiver rests on.
    #[must_use]
    pub fn ack_id(&self) -> &str {
        &self.ack_id
    }
}

impl fmt::Debug for ZdrWaiver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ZdrWaiver")
            .field("provider", &self.provider)
            .field("connection_id", &self.connection_id)
            .field("ack_id", &self.ack_id)
            .finish()
    }
}

/// Canonical identity derived only from a verified Auth Core credential.
#[derive(Clone, Eq, PartialEq)]
pub struct VerifiedIdentity {
    org_id: Arc<str>,
    principal_id: Arc<str>,
    user_id: Option<Arc<str>>,
    scopes: Arc<[String]>,
    kind: PrincipalKind,
    zdr: bool,
    zdr_waiver: Option<ZdrWaiver>,
    /// The person who owns an `api_key` principal: attribution only, never an owner (EP-12 D2).
    /// `None` for every other principal.
    acting_user_id: Option<Arc<str>>,
}

impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedIdentity")
            .field("org_id", &self.org_id)
            .field("principal_id", &self.principal_id)
            .field("kind", &self.kind)
            .field("scopes", &self.scopes)
            .field("zdr", &self.zdr)
            .field("zdr_waiver", &self.zdr_waiver.is_some())
            .finish_non_exhaustive()
    }
}

impl VerifiedIdentity {
    #[must_use]
    pub fn org_id(&self) -> &str {
        &self.org_id
    }

    #[must_use]
    pub fn principal_id(&self) -> &str {
        &self.principal_id
    }

    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    #[must_use]
    pub fn is_service(&self) -> bool {
        self.kind == PrincipalKind::Service
    }

    /// Whether this credential is an `api_key` principal (EP-12 S4).
    #[must_use]
    pub fn is_api_key(&self) -> bool {
        self.kind == PrincipalKind::ApiKey
    }

    /// The key id of an `api_key` principal; `None` for every other principal.
    #[must_use]
    pub fn api_key_id(&self) -> Option<&str> {
        if self.is_api_key() {
            self.principal_id.strip_prefix(API_KEY_SUBJECT_PREFIX)
        } else {
            None
        }
    }

    /// The person who owns an `api_key` principal, for attribution; `None` for every other
    /// principal. It is never an owner: nothing reads it to authorize a row.
    #[must_use]
    pub fn acting_user_id(&self) -> Option<&str> {
        self.acting_user_id.as_deref()
    }

    /// The id the rows this caller owns carry in `user_id`: the person for a user, the key
    /// principal `apikey:<key_id>` for an `api_key` (EP-12 D2), and `None` for a service. Kept
    /// apart from [`Self::user_id`], which names a person only, so a key is never read as one.
    #[must_use]
    pub fn owner_id(&self) -> Option<&str> {
        match self.kind {
            PrincipalKind::User => self.user_id(),
            PrincipalKind::ApiKey => Some(self.principal_id()),
            PrincipalKind::Service => None,
        }
    }

    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|candidate| candidate == scope)
    }

    /// Whether the verified credential carries the org admin role: a signed scope `admin`,
    /// `org:admin` or any scope ending `:admin`. The rule is capability-core's
    /// `verifiedIsAdmin`; it reads the verified scopes only, never a header.
    #[must_use]
    pub fn is_org_admin(&self) -> bool {
        self.scopes
            .iter()
            .any(|scope| scope == "admin" || scope == "org:admin" || scope.ends_with(":admin"))
    }

    /// Whether the trusted issuer marked this credential Zero Data Retention.
    #[must_use]
    pub fn zdr(&self) -> bool {
        self.zdr
    }

    /// The connection-bound waiver this credential carries, if any. A waiver
    /// only ever rides a persistent credential of a ZDR org.
    #[must_use]
    pub fn zdr_waiver(&self) -> Option<&ZdrWaiver> {
        self.zdr_waiver.as_ref()
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize_org(&self, org_id: &str) -> Result<(), Status> {
        if org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if org_id != self.org_id() {
            return Err(Status::permission_denied("tenant access denied"));
        }
        Ok(())
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize_user(&self, user_id: &str) -> Result<(), Status> {
        if user_id.trim().is_empty() {
            return Err(Status::invalid_argument("user_id is required"));
        }
        match self.owner_id() {
            Some(verified) if verified == user_id => Ok(()),
            Some(_) => Err(Status::permission_denied("user access denied")),
            None => Err(Status::permission_denied(
                "service identity cannot impersonate a user",
            )),
        }
    }

    #[allow(clippy::result_large_err)]
    pub fn require_service_scope(&self, scope: &str) -> Result<(), Status> {
        if !self.is_service() || !self.has_scope(scope) {
            return Err(Status::permission_denied("service scope required"));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn user_for_test(org_id: &str, user_id: &str) -> Self {
        Self::user_for_test_with_zdr(org_id, user_id, false)
    }

    #[cfg(test)]
    pub(crate) fn user_for_test_with_zdr(org_id: &str, user_id: &str, zdr: bool) -> Self {
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from(user_id),
            user_id: Some(Arc::from(user_id)),
            scopes: Arc::from([]),
            kind: PrincipalKind::User,
            zdr,
            zdr_waiver: None,
            acting_user_id: None,
        }
    }

    /// A persistent user identity carrying `waiver`, shaped as the verifier
    /// shapes one: the claim only ever rides a token whose `zdr` is false.
    #[cfg(test)]
    pub(crate) fn user_for_test_with_waiver(
        org_id: &str,
        user_id: &str,
        connection_id: &str,
        ack_id: &str,
    ) -> Self {
        Self {
            zdr_waiver: Some(ZdrWaiver {
                provider: mp_contracts::agentic_subscription::PROVIDER_ID.to_owned(),
                connection_id: connection_id.to_owned(),
                ack_id: ack_id.to_owned(),
            }),
            ..Self::user_for_test_with_zdr(org_id, user_id, false)
        }
    }

    #[cfg(test)]
    pub(crate) fn service_for_test(org_id: &str, scopes: &[&str], zdr: bool) -> Self {
        Self::service_for_test_as(org_id, "service:session-core", scopes, zdr)
    }

    #[cfg(test)]
    pub(crate) fn service_for_test_as(
        org_id: &str,
        principal_id: &str,
        scopes: &[&str],
        zdr: bool,
    ) -> Self {
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from(principal_id),
            user_id: None,
            scopes: scopes
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect::<Vec<_>>()
                .into(),
            kind: PrincipalKind::Service,
            zdr,
            zdr_waiver: None,
            acting_user_id: None,
        }
    }

    /// An `api_key` identity shaped as the verifier shapes one: owned by `apikey:<key_id>`,
    /// attributed to `acting_user_id`, holding [`API_KEY_SCOPE`], never a waiver.
    #[cfg(test)]
    pub(crate) fn api_key_for_test(
        org_id: &str,
        key_id: &str,
        acting_user_id: &str,
        zdr: bool,
    ) -> Self {
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from(format!("{API_KEY_SUBJECT_PREFIX}{key_id}")),
            user_id: None,
            scopes: Arc::from([API_KEY_SCOPE.to_owned()]),
            kind: PrincipalKind::ApiKey,
            zdr,
            zdr_waiver: None,
            acting_user_id: Some(Arc::from(acting_user_id)),
        }
    }
}

/// Retrieve the canonical identity installed by the mandatory interceptor.
/// A missing extension is an internal wiring failure and remains fail closed.
#[allow(clippy::result_large_err)]
pub fn identity<T>(request: &Request<T>) -> Result<VerifiedIdentity, Status> {
    request
        .extensions()
        .get::<VerifiedIdentity>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("verified caller identity required"))
}

#[allow(clippy::result_large_err)]
pub fn authorize_operation(caller: &VerifiedIdentity, service_scope: &str) -> Result<(), Status> {
    if caller.zdr() && !service_scope.ends_with(":read") {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    if caller.is_service() {
        caller.require_service_scope(service_scope)
    } else {
        Ok(())
    }
}

/// The gate of the owner's own housekeeping: deleting or archiving a thread, changing its presentation
/// controls, deleting a memory of their own. Owner decision 17 makes these open to a zero-retention user
/// under every posture, so a human caller is not refused for holding a ZDR credential here. The handler
/// keeps its org and owner checks, and refuses any conversation text it would persist. A service caller
/// is held to [`authorize_operation`] as before.
#[allow(clippy::result_large_err)]
pub fn authorize_owner_housekeeping(
    caller: &VerifiedIdentity,
    service_scope: &str,
) -> Result<(), Status> {
    if caller.is_service() {
        return authorize_operation(caller, service_scope);
    }
    Ok(())
}

/// Scope a service principal must additionally hold to create a run it owns
/// itself, on top of the ordinary `session:write`.
///
/// It is separate from `session:write` on purpose. Every workload that writes
/// sessions already holds `session:write`; owning a run is a strictly larger
/// power (the row is then readable org-wide and mutable only by that workload),
/// so it must be grantable independently in the service-principal registry.
pub const SYSTEM_RUN_OWNER_SCOPE: &str = "session:runs:system-owner";

/// Scope reserved for the Control-owned data-subject deletion coordinator.
/// It intentionally does not grant ordinary session writes or user
/// impersonation; Session Core pairs it with the exact service identity below.
pub const SPACE_DELETION_SCOPE: &str = "session:space-delete";

/// The sole workload permitted to delete Space-bound human transcripts after
/// Control has authorized a data-subject deletion request. Keep this an exact
/// closed set rather than a service prefix: a registry entry alone must never
/// turn an arbitrary workload into an erasure authority.
pub const SPACE_DELETION_SERVICE: &str = "service:control-space-deletion";

/// Verify the deliberately narrow service identity for a Space erasure.
#[allow(clippy::result_large_err)]
pub fn authorize_space_deletion_service(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    if !caller.is_service() || caller.principal_id() != SPACE_DELETION_SERVICE {
        return Err(Status::permission_denied(
            "only the Control space-deletion coordinator may erase Space threads",
        ));
    }
    caller.require_service_scope(SPACE_DELETION_SCOPE)
}

/// Scope and exact identity reserved for Control's action-decision issuer.
/// The resulting read is deliberately narrower than ordinary `session:read`:
/// it derives only a run's non-secret, durable Space bindings and is never an
/// API for a Model workload to impersonate a user or inspect a transcript.
pub const RUN_ACTION_AUTHORITY_SCOPE: &str = "session:run-action-authority";
pub const RUN_ACTION_AUTHORITY_SERVICE: &str = "service:control-action-authorizer";

/// Separate least-privilege scope for Control's scheduled-step authority
/// read. The caller may resolve only the exact prepared scheduled run tuple;
/// it cannot use this scope to read ordinary runs or human-owned context.
pub const SCHEDULED_STEP_AUTHORITY_SCOPE: &str = "session:scheduled-step-authority";

/// Dedicated Session Core scope for Execution Core's service-owned scheduled
/// step claim/receipt lane. It is intentionally distinct from `session:write`
/// and cannot be used to create or mutate human-owned runs.
pub const SCHEDULED_STEP_SCOPE: &str = "session:scheduled-step";
pub const SCHEDULED_STEP_SERVICE: &str = "service:execution-core";

#[allow(clippy::result_large_err)]
pub fn authorize_scheduled_step_service(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("scheduled_step")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    if !caller.is_service() || caller.principal_id() != SCHEDULED_STEP_SERVICE {
        return Err(Status::permission_denied(
            "only Execution Core may claim scheduled steps",
        ));
    }
    caller.require_service_scope(SCHEDULED_STEP_SCOPE)
}

/// Verify the single Control workload allowed to resolve durable run bindings
/// before it issues an owner-targeted action decision.
#[allow(clippy::result_large_err)]
pub fn authorize_run_action_authority_service(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("agent_run")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    if !caller.is_service() || caller.principal_id() != RUN_ACTION_AUTHORITY_SERVICE {
        return Err(Status::permission_denied(
            "only the Control action-authorizer may resolve run action authority",
        ));
    }
    caller.require_service_scope(RUN_ACTION_AUTHORITY_SCOPE)
}

/// Verify the Control workload allowed to resolve one prepared scheduled run
/// before it issues a per-step decision. Keep this identity separate from
/// Execution Core's claim/receipt scope and from generic run inspection.
#[allow(clippy::result_large_err)]
pub fn authorize_scheduled_step_authority_service(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::feature_not_supported("scheduled_step")
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    if !caller.is_service() || caller.principal_id() != RUN_ACTION_AUTHORITY_SERVICE {
        return Err(Status::permission_denied(
            "only the Control scheduled-step authorizer may resolve scheduled-step authority",
        ));
    }
    caller.require_service_scope(SCHEDULED_STEP_AUTHORITY_SCOPE)
}

/// The closed set of principals allowed to own a run with no human behind it.
///
/// A run row is normally owned by a person. A durable workflow fired by cron has
/// no person: `orchestrator-core`'s Temporal activities hold only a service
/// credential, and auth-core mints no user delegation (`issueInternalToken` sets
/// `userId: principal.subject`), so before this existed every system-initiated
/// run failed at `start_run`'s last gate.
///
/// # Why an allowlist and not a prefix test
///
/// `runs.user_id` is bare `TEXT NOT NULL` — no FK, no CHECK, no width. A
/// predicate like `starts_with("service:")` would make every present and future
/// value in that namespace a system-owned row, and the read allowance below
/// would follow it automatically. Exact membership in this constant means
/// appearing in `PLANE_SERVICE_PRINCIPALS_JSON` is NOT sufficient to own runs: a
/// reviewed change to this file is also required.
///
/// # Why these ids and not a `system:` sentinel
///
/// The value is the caller's own signed `service_id` claim, so it is a fact the
/// issuer asserted rather than a literal this code chose. That matters because
/// [`authorize_owner_row`] lets ANY service pass the user check for a row inside
/// its org: with one shared sentinel, every workload holding the scope could
/// create runs indistinguishable from orchestrator-core's and then read and
/// drive them. Deriving the owner from the credential makes that lateral move
/// structurally impossible instead of policy-prevented — `capability-core`'s
/// token can only ever produce `service:capability-core`. It also matches the
/// spelling session-core already uses for non-human actors (see
/// `terminalization.rs`'s `service:model-gateway` / `service:execution-core`).
const SYSTEM_RUN_OWNERS: [&str; 1] = ["service:orchestrator-core"];

/// Whether `user_id` names a principal from [`SYSTEM_RUN_OWNERS`].
///
/// Exact byte equality. No trimming, no case folding, no prefix match: a
/// look-alike such as `"service:orchestrator-core-2"` or a trailing space must
/// NOT be treated as a system owner, and Auth Core actor ids are base62 so a
/// real person's id can never contain the `:` these values do.
#[must_use]
pub fn is_system_run_owner(user_id: &str) -> bool {
    SYSTEM_RUN_OWNERS.contains(&user_id)
}

/// The system-run owner ids, for binding into SQL as `= ANY($n::text[])`.
///
/// Exposed so the org-scoped listing query reads the same constant the
/// authorization code does, rather than re-encoding the set as a LIKE pattern.
#[must_use]
pub fn system_run_owners() -> &'static [&'static str] {
    &SYSTEM_RUN_OWNERS
}

/// What a caller intends to do with a row whose owner is being checked.
///
/// The two intents differ ONLY for system-owned rows. A system run's content is
/// by construction the org-visible set — orchestrator-core refuses to thread a
/// `user_id` for a service principal precisely because that id narrows retrieval
/// to one viewer's visible set — so org-wide readability discloses nothing that
/// org membership did not already grant. Mutating one is different: it is an
/// action on a workload's in-flight run, and only that workload may take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerIntent {
    /// Observe the row (get, list, stream events, read plans/todos/approvals).
    Read,
    /// Change the row or anything hanging off it (cancel, complete, set mode,
    /// checkpoint, reserve a tool action).
    Mutate,
}

/// The single ownership predicate for every owner-gated row in session-core.
///
/// Org equality is absolute and checked first on every path — no branch below
/// relaxes it.
///
/// For a NON-system row the behaviour is unchanged: a human must match the row's
/// user exactly, and a service caller skips that check (pre-existing, relied on
/// by `RecordTerminalOutcome`).
///
/// For a system-owned row:
///   * [`OwnerIntent::Read`] → org equality only.
///   * [`OwnerIntent::Mutate`] → the caller must BE the owning workload. A human
///     is refused, and so is a different service, which is what stops the read
///     allowance from becoming a write allowance.
#[allow(clippy::result_large_err)]
pub fn authorize_owner_row(
    caller: &VerifiedIdentity,
    org_id: &str,
    user_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    caller.authorize_org(org_id)?;
    // A key reaches only rows it owns. The org-wide read of a system-owned row is for the org's
    // people and workloads, not for a credential that can only ever own its own threads (EP-12 S4).
    if caller.is_api_key() {
        return caller.authorize_user(user_id);
    }
    if is_system_run_owner(user_id) {
        return match intent {
            OwnerIntent::Read => Ok(()),
            OwnerIntent::Mutate => {
                if caller.is_service() && caller.principal_id() == user_id {
                    Ok(())
                } else {
                    Err(Status::permission_denied(
                        "only the owning workload may act on a system-owned run",
                    ))
                }
            }
        };
    }
    if !caller.is_service() {
        caller.authorize_user(user_id)?;
    }
    Ok(())
}

/// [`authorize_owner_row`] for a THREAD, which is the one row that can say whose
/// Space it lives in.
///
/// The org-wide read a system-owned row gets is right for a maintenance run: its
/// content is the org's by construction. It is wrong for a system-owned thread
/// that carries a non-empty `space_id`. Every scheduled run's result thread is
/// one (`start_scheduled_run` refuses a thread without a Space), and its content
/// is one person's schedule written into that person's own Space by a workload
/// acting for them. Org equality does not make it the org's, yet that was the
/// only test, so any `session:read` caller in the org could read it by thread id.
///
/// So for a user-bound reader on a [`OwnerIntent::Read`] of such a thread this
/// answers "not found" (what a thread that does not exist answers, so the id is
/// not even confirmed) and the only way in is the Space read-decision branch,
/// which Control issues to the Space's own owner and which never calls this
/// function. Everything else is exactly [`authorize_owner_row`]:
///
///   * the org check still comes first, so a cross-org caller is still refused
///     with `PermissionDenied`;
///   * a service principal keeps its existing read, as on every other row (the
///     owning workload and the planes that act for a run need it, and a service
///     already reads ordinary human threads in its org);
///   * a system thread with no Space (maintenance) stays org-readable;
///   * [`OwnerIntent::Mutate`] is untouched (only the owning workload).
#[allow(clippy::result_large_err)]
pub fn authorize_thread_owner_row(
    caller: &VerifiedIdentity,
    org_id: &str,
    user_id: &str,
    space_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    caller.authorize_org(org_id)?;
    if intent == OwnerIntent::Read
        && is_system_run_owner(user_id)
        && !space_id.trim().is_empty()
        && !caller.is_service()
    {
        return Err(Status::not_found("thread not found"));
    }
    authorize_owner_row(caller, org_id, user_id, intent)
}

/// Authorize a service principal to create a run or thread it owns itself.
///
/// Returns the owner id to persist. Every condition is required; the caller
/// still runs `authorize_operation(caller, "session:write")` separately, which is
/// what keeps a ZDR credential out of a durable write.
#[allow(clippy::result_large_err)]
pub fn authorize_system_run_owner(caller: &VerifiedIdentity) -> Result<String, Status> {
    // Assert the kind explicitly rather than inferring "service" from a missing
    // user_id: the two are equivalent today only because `validate_identity` has
    // exactly two arms, and that is not an invariant worth depending on here.
    if !caller.is_service() {
        return Err(Status::permission_denied(
            "user-bound run credential required",
        ));
    }
    // Checked locally as well as via `authorize_operation`'s scope argument, so
    // the invariant "a system run is durable" does not depend on the argument
    // string passed to a different function.
    if caller.zdr() {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    caller.require_service_scope(SYSTEM_RUN_OWNER_SCOPE)?;
    let owner = caller.principal_id();
    if !is_system_run_owner(owner) {
        return Err(Status::permission_denied(
            "this service principal may not own runs",
        ));
    }
    Ok(owner.to_owned())
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    #[allow(dead_code)]
    iss: String,
    #[allow(dead_code)]
    aud: serde_json::Value,
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
    /// `org_zdr`: `true` when the org is Zero Data Retention. Absent on service
    /// tokens. Only a waiver reads it, and only as `Some(true)`.
    #[serde(default)]
    org_zdr: Option<bool>,
    /// `zdr_waiver`: present only on a persistent token of a ZDR org, bound to
    /// one connection. Parsed and checked by [`parse_zdr_waiver`].
    #[serde(default)]
    zdr_waiver: Option<serde_json::Value>,
    /// `key_id`: the Better Auth key an `api_key` token was exchanged for. Read only while
    /// [`API_KEY_PRINCIPAL_ENV`] is on. Kept as a raw value so a token that carries an odd one
    /// decodes exactly as it did before the switch existed.
    #[serde(default)]
    key_id: Option<serde_json::Value>,
    /// `acting_user_id`: the key's owner on an `api_key` token. Attribution only.
    #[serde(default)]
    acting_user_id: Option<serde_json::Value>,
}

/// The `api_key` shape model-gateway, inference-core and Auth Core agree on: a bounded printable
/// `key_id`, `sub == "apikey:" + key_id`, an owner that is present, unpadded and not itself a key,
/// and no person or service identity beside them.
fn is_well_formed_api_key(claims: &Claims) -> bool {
    let (Some(key_id), Some(owner)) = (
        claims.key_id.as_ref().and_then(serde_json::Value::as_str),
        claims.acting_user_id.as_ref().and_then(serde_json::Value::as_str),
    ) else {
        return false;
    };
    (1..=MAX_API_KEY_ID_LEN).contains(&key_id.len())
        && key_id.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        && claims.sub.strip_prefix(API_KEY_SUBJECT_PREFIX) == Some(key_id)
        && claims.user_id.is_empty()
        && claims.service_id.is_empty()
        && !owner.is_empty()
        && owner.len() <= MAX_ACTING_USER_ID_LEN
        && owner == owner.trim()
        && !owner.starts_with(API_KEY_SUBJECT_PREFIX)
}

/// The wire shape of the `zdr_waiver` claim. Extra fields Auth Core may add
/// (a digest, a decision id) are ignored; these four are required.
#[derive(Debug, Deserialize)]
struct WaiverClaim {
    version: u32,
    provider: String,
    connection: String,
    ack_id: String,
}

pub(crate) struct AuthConfig {
    pub jwks_url: String,
    pub issuer: String,
    pub audience: String,
    /// Audience Data Plane v2 enforces on inbound gRPC (`JWT_REQUIRED_AUDIENCE`,
    /// itself defaulted from `DATA_PLANE_AUTH_AUDIENCE`). Verified locally so a
    /// delegated credential is bound to the caller before it is forwarded.
    pub data_plane_audience: String,
    pub leeway_secs: u64,
}

impl AuthConfig {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("SESSION_CORE_AUTH_AUDIENCE")?,
            data_plane_audience: required_env("DATA_PLANE_AUTH_AUDIENCE")
                .unwrap_or_else(|_| DEFAULT_DATA_PLANE_AUDIENCE.to_owned()),
            leeway_secs: std::env::var("AUTH_CORE_JWT_LEEWAY_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(DEFAULT_LEEWAY_SECS)
                .min(300),
        })
    }
}

fn required_env(name: &'static str) -> anyhow::Result<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name} is required"))
}

impl JwtVerifier {
    /// Fetch and validate Auth Core verification material before serving.
    pub async fn from_env() -> anyhow::Result<Self> {
        let accept_api_key =
            api_key_principal_enabled(std::env::var(API_KEY_PRINCIPAL_ENV).ok().as_deref());
        Ok(Self::from_config(AuthConfig::from_env()?)
            .await?
            .with_api_key_principal(accept_api_key))
    }

    /// Admit or refuse the `api_key` principal type. [`Self::from_config`] refuses it.
    #[must_use]
    pub(crate) fn with_api_key_principal(mut self, accept: bool) -> Self {
        self.accept_api_key = accept;
        self
    }

    pub(crate) async fn from_config(config: AuthConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()?;
        let response = client
            .get(&config.jwks_url)
            .send()
            .await?
            .error_for_status()?;
        let body = response.bytes().await?;
        if body.len() > MAX_JWKS_BYTES {
            anyhow::bail!("Auth Core JWKS response exceeds size limit");
        }
        let jwks: JwkSet = serde_json::from_slice(&body)?;
        let keys = validated_keys(&jwks)?;
        Ok(Self {
            issuer: Arc::from(config.issuer),
            audience: Arc::from(config.audience),
            data_plane_audience: Arc::from(config.data_plane_audience),
            leeway_secs: config.leeway_secs,
            keys: Arc::new(keys),
            accept_api_key: false,
        })
    }

    #[allow(clippy::result_large_err)]
    pub fn intercept(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = extract_bearer(&request)?;
        let identity = self.verify_token(token, self.audience.as_ref())?;
        // A key is admitted here, before any handler runs, so a handler never sees a key on a
        // method it was not reviewed for (EP-12 S4).
        if identity.is_api_key() {
            let path = request.extensions().get::<RpcPath>().map(|path| path.0.as_ref());
            authorize_api_key_call(&identity, path)?;
        }
        request.extensions_mut().insert(identity);
        Ok(request)
    }

    /// Verify one Auth Core RS256 credential for an exact audience. Audience is
    /// an explicit parameter rather than a field read so a token minted for one
    /// plane hop can never satisfy another: the ingress `aud=session-core` token
    /// and a delegated `aud=data-plane` token are checked against their own
    /// audience, and never interchangeably.
    #[allow(clippy::result_large_err)]
    fn verify_token(&self, token: &str, audience: &str) -> Result<VerifiedIdentity, Status> {
        let header = jsonwebtoken::decode_header(token)
            .map_err(|_| Status::unauthenticated("invalid caller credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid caller credential"));
        }
        let key = header
            .kid
            .as_deref()
            .and_then(|kid| self.keys.get(kid))
            .ok_or_else(|| Status::unauthenticated("invalid caller credential"))?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, key, &validation)
            .map_err(|_| Status::unauthenticated("invalid caller credential"))?
            .claims;
        if claims.aud.as_str() != Some(audience) {
            return Err(Status::unauthenticated("invalid caller credential"));
        }
        validate_identity(claims, self.accept_api_key)
    }

    /// Verify the separately delegated Data Plane credential and bind it to the
    /// already authenticated session-core caller.
    ///
    /// `Ok(None)` means the caller delegated nothing. Data Plane grounding is
    /// opportunistic — context assembly still answers from durable local memory —
    /// so a missing delegation degrades instead of failing the whole RPC. A
    /// credential that *is* supplied must verify against `data-plane` and match
    /// the caller's org, user, and retention posture exactly.
    ///
    /// session-core deliberately has no way to mint or self-sign this value. The
    /// ingress token is not a substitute (wrong audience, and Data Plane would
    /// reject it), and neither is a service token: Data Plane retrieval enforces
    /// per-user, private-until-shared authorization from this token's `sub` and
    /// `org_id`, so anything broader would collapse every user's view into one
    /// identity.
    ///
    /// # Errors
    ///
    /// Returns `Unauthenticated` for a malformed or unverifiable credential and
    /// `PermissionDenied` when the delegated identity or ZDR posture differs from
    /// the verified caller.
    #[allow(clippy::result_large_err)]
    pub fn delegated_data_plane_bearer<T>(
        &self,
        request: &Request<T>,
        caller: &VerifiedIdentity,
    ) -> Result<Option<DelegatedDataPlaneBearer>, Status> {
        let Some(token) = optional_metadata_bearer(request, DATA_PLANE_AUTH_METADATA_KEY)? else {
            return Ok(None);
        };
        let delegated = self.verify_token(token, self.data_plane_audience.as_ref())?;
        // A key reads no Data Plane knowledge (EP-12 D5) and is no person's delegation, so it
        // neither delegates nor is delegated.
        if caller.is_api_key() || delegated.is_api_key() {
            return Err(Status::permission_denied("an API key carries no Data Plane delegation"));
        }
        // Every bearer of one request agrees on zero retention AND on the
        // waiver: a waiver for one connection must never ride a grounding call
        // that the caller's own posture does not carry.
        if delegated.org_id() != caller.org_id() || delegated.user_id() != caller.user_id() {
            return Err(Status::permission_denied(
                "delegated Data Plane identity does not match caller",
            ));
        }
        if delegated.zdr() != caller.zdr() || delegated.zdr_waiver() != caller.zdr_waiver() {
            return Err(mp_contracts::zdr_refusal::Refusal::POSTURE_MISMATCH
                .grpc_status(tonic::Code::PermissionDenied));
        }
        Ok(Some(DelegatedDataPlaneBearer(Arc::from(token))))
    }
}

/// Opaque delegated Data Plane credential, verified and identity-bound by
/// [`JwtVerifier::delegated_data_plane_bearer`]. Constructible only there (plus a
/// test shim), so no code path can invent one from decoded claims, a caller
/// header, or a shared internal key.
#[derive(Clone)]
pub struct DelegatedDataPlaneBearer(Arc<str>);

impl DelegatedDataPlaneBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test(token: &str) -> Self {
        Self(Arc::from(token))
    }
}

impl fmt::Debug for DelegatedDataPlaneBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedDataPlaneBearer([REDACTED])")
    }
}

/// Read an optional `Bearer` credential from a non-standard metadata key.
/// A present-but-malformed value is an error rather than `None`: a caller that
/// meant to delegate must never silently fall through to an ungrounded path.
#[allow(clippy::result_large_err)]
fn optional_metadata_bearer<'a, T>(
    request: &'a Request<T>,
    key: &'static str,
) -> Result<Option<&'a str>, Status> {
    let Some(value) = request.metadata().get(key) else {
        return Ok(None);
    };
    value
        .to_str()
        .ok()
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .map(Some)
        .ok_or_else(|| Status::unauthenticated("malformed delegated credential"))
}

impl tonic::service::Interceptor for JwtVerifier {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        self.intercept(request)
    }
}

#[allow(clippy::result_large_err)]
fn extract_bearer(request: &Request<()>) -> Result<&str, Status> {
    request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified caller credential required"))
}

#[allow(clippy::result_large_err)]
fn validate_identity(claims: Claims, accept_api_key: bool) -> Result<VerifiedIdentity, Status> {
    if claims.org_id.trim().is_empty() || claims.org_id.trim() != claims.org_id {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    // While keys are admitted, a key id on any other type makes the token ambiguous. Off, the
    // claim is not read, so every token is read exactly as before.
    if accept_api_key && claims.principal_type != "api_key" && claims.key_id.is_some() {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    // Decided before `scopes` moves out of the claims. An `api_key` token's `zdr` is the org's
    // posture raised by the key's own flag, so a persistent key token of a ZDR org is not one
    // Auth Core mints, and is refused like a person's token without its waiver.
    let api_key_admitted = accept_api_key
        && is_well_formed_api_key(&claims)
        && (claims.zdr || claims.org_zdr != Some(true));
    let zdr_waiver = parse_zdr_waiver(claims.zdr, claims.org_zdr, claims.zdr_waiver.as_ref())?;
    let user_bound = matches!(claims.principal_type.as_str(), "" | "user");
    // A waiver is a decision about one human's connection. A service credential
    // can never carry one, whatever else it claims.
    if zdr_waiver.is_some() && !user_bound {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    // The matrix's rule 3: a persistent user token of a ZDR org is served only
    // under a waiver. Refused here rather than recorded as an ordinary run, so a
    // token minted without its waiver cannot downgrade a ZDR org's turn.
    if user_bound && !claims.zdr && claims.org_zdr == Some(true) && zdr_waiver.is_none() {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    let scopes: Arc<[String]> = claims.scopes.into();
    match claims.principal_type.as_str() {
        "" | "user"
            if !claims.user_id.trim().is_empty()
                && claims.user_id == claims.user_id.trim()
                && claims.sub == claims.user_id
                && claims.service_id.is_empty() =>
        {
            Ok(VerifiedIdentity {
                org_id: Arc::from(claims.org_id),
                principal_id: Arc::from(claims.user_id.clone()),
                user_id: Some(Arc::from(claims.user_id)),
                scopes,
                kind: PrincipalKind::User,
                zdr: claims.zdr,
                zdr_waiver,
                acting_user_id: None,
            })
        }
        "service"
            if claims.user_id.is_empty()
                && !claims.service_id.trim().is_empty()
                && claims.service_id == claims.service_id.trim()
                && claims.sub == claims.service_id =>
        {
            Ok(VerifiedIdentity {
                org_id: Arc::from(claims.org_id),
                principal_id: Arc::from(claims.service_id),
                user_id: None,
                scopes,
                kind: PrincipalKind::Service,
                zdr: claims.zdr,
                zdr_waiver: None,
                acting_user_id: None,
            })
        }
        // EP-12 S4, only while SESSION_CORE_API_KEY_PRINCIPAL is on; off, the type falls to the
        // refusal below. The key principal owns what it creates and the person is attribution
        // only, so `user_id` stays `None`. A waiver never reaches here: the check above refuses
        // one on any token that is not user-bound.
        "api_key" if api_key_admitted => Ok(VerifiedIdentity {
            org_id: Arc::from(claims.org_id),
            principal_id: Arc::from(claims.sub),
            user_id: None,
            scopes,
            kind: PrincipalKind::ApiKey,
            zdr: claims.zdr,
            zdr_waiver: None,
            acting_user_id: claims
                .acting_user_id
                .as_ref()
                .and_then(serde_json::Value::as_str)
                .map(Arc::from),
        }),
        _ => Err(Status::unauthenticated("invalid caller credential")),
    }
}

/// Parse and check the `zdr_waiver` claim of one verified token.
///
/// Absent or `null` is no waiver. Anything else must be a v1 waiver that the
/// shared matrix resolves as a waiver: a persistent token (`zdr` false) of a
/// ZDR org (`org_zdr` true), bound to one provider and one connection, with a
/// non-empty acknowledgement id. An unknown version, a malformed claim, or a
/// waiver on a ZDR token is refused, never read as "no waiver": a token that
/// silently dropped its waiver would be recorded as persistent for a ZDR org.
#[allow(clippy::result_large_err)]
fn parse_zdr_waiver(
    zdr: bool,
    org_zdr: Option<bool>,
    claim: Option<&serde_json::Value>,
) -> Result<Option<ZdrWaiver>, Status> {
    let refused = || Status::unauthenticated("invalid caller credential");
    let Some(claim) = claim.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let waiver: WaiverClaim = serde_json::from_value(claim.clone()).map_err(|_| refused())?;
    if zdr || org_zdr != Some(true) {
        return Err(refused());
    }
    // The shared resolver is the one definition of a valid waiver. Resolving
    // the waiver's own route must yield a waiver for exactly that connection.
    let resolution = resolve_turn(
        &[ClaimsView {
            zdr: false,
            org_zdr: Some(true),
            waiver: Some(WaiverView {
                version: waiver.version,
                provider: &waiver.provider,
                connection: &waiver.connection,
            }),
        }],
        &RouteView {
            provider: &waiver.provider,
            connection: Some(&waiver.connection),
            explicit_zdr: false,
        },
    );
    if !matches!(resolution, Resolution::Waived { .. }) || waiver.version != WAIVER_CONTRACT_VERSION
    {
        return Err(refused());
    }
    for id in [&waiver.connection, &waiver.ack_id] {
        if id.is_empty()
            || id.len() > 128
            || id.trim() != id.as_str()
            || id.chars().any(char::is_control)
        {
            return Err(refused());
        }
    }
    Ok(Some(ZdrWaiver {
        provider: waiver.provider,
        connection_id: waiver.connection,
        ack_id: waiver.ack_id,
    }))
}

fn validated_keys(jwks: &JwkSet) -> anyhow::Result<HashMap<String, DecodingKey>> {
    let mut keys = HashMap::new();
    for jwk in &jwks.keys {
        let serialized = serde_json::to_value(jwk)?;
        if serialized.get("kty").and_then(serde_json::Value::as_str) != Some("RSA")
            || serialized.get("alg").and_then(serde_json::Value::as_str) != Some("RS256")
        {
            continue;
        }
        let kid = serialized
            .get("kid")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing kid"))?;
        let modulus = serialized
            .get("n")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing modulus"))?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(modulus)?;
        if rsa_modulus_bits(&bytes) < 2048 {
            anyhow::bail!("Auth Core JWKS RSA key is smaller than 2048 bits");
        }
        let key = DecodingKey::from_jwk(jwk)?;
        if keys.insert(kid.to_owned(), key).is_some() {
            anyhow::bail!("Auth Core JWKS contains duplicate kid");
        }
    }
    if keys.is_empty() {
        anyhow::bail!("Auth Core JWKS contains no usable RS256 signing keys");
    }
    Ok(keys)
}

fn rsa_modulus_bits(modulus: &[u8]) -> usize {
    let Some((index, first)) = modulus.iter().enumerate().find(|(_, byte)| **byte != 0) else {
        return 0;
    };
    (modulus.len() - index - 1) * 8 + (8 - first.leading_zeros() as usize)
}

/// Credentials shaped exactly as Auth Core issues them, for tests elsewhere in
/// the crate that must go through the production verifier instead of a
/// hand-built [`VerifiedIdentity`].
///
/// A hand-built identity lets a test pick claim values the issuer never
/// produces. That is how a check on a service token's `zdr` passed every test
/// and refused every live atomic save: the tests gave Model Gateway `zdr:
/// false`, and Auth Core never does.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, OnceLock};

    use base64::Engine as _;
    use jsonwebtoken::{encode, jwk::JwkSet, Algorithm, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, LineEnding},
        traits::PublicKeyParts,
    };
    use serde_json::{json, Value};
    use tonic::{Request, Status};

    use super::{validated_keys, JwtVerifier, DEFAULT_DATA_PLANE_AUDIENCE};

    pub(crate) const ISSUER: &str = "auth-core";
    pub(crate) const KID: &str = "key-1";

    /// (private PKCS#8 PEM, modulus, exponent), generated once per test binary.
    pub(crate) fn rsa_keypair() -> &'static (String, String, String) {
        static KEYS: OnceLock<(String, String, String)> = OnceLock::new();
        KEYS.get_or_init(|| {
            let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
            let public = rsa::RsaPublicKey::from(&private);
            (
                private
                    .to_pkcs8_pem(LineEnding::LF)
                    .expect("private PEM")
                    .to_string(),
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
            )
        })
    }

    pub(crate) fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs()
    }

    /// Sign `claims` as Auth Core does: RS256 with a `kid` header.
    pub(crate) fn sign(claims: &Value, kid: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_owned());
        encode(
            &header,
            claims,
            &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
        )
        .expect("JWT")
    }

    /// A verifier for `audience` that trusts [`rsa_keypair`], built from a JWKS
    /// through the key validation `JwtVerifier::from_config` applies to Auth
    /// Core's; only the HTTP fetch is skipped.
    pub(crate) fn verifier(audience: &str) -> JwtVerifier {
        let (_, n, e) = rsa_keypair();
        let jwks: JwkSet = serde_json::from_value(json!({"keys": [{
            "kty": "RSA", "use": "sig", "alg": "RS256", "kid": KID, "n": n, "e": e
        }]}))
        .expect("JWKS");
        JwtVerifier {
            issuer: Arc::from(ISSUER),
            audience: Arc::from(audience),
            data_plane_audience: Arc::from(DEFAULT_DATA_PLANE_AUDIENCE),
            leeway_secs: 0,
            keys: Arc::new(validated_keys(&jwks).expect("usable JWKS")),
            accept_api_key: false,
        }
    }

    /// The payload Auth Core's `issuePlaneToken` signs for a registered service
    /// principal on `POST /api/<audience>/internal-token`, claim for claim:
    /// `sub` and `service_id` are `service:<id>`, there is no `user_id`, the
    /// requested scopes and reason are echoed, and `zdr` is the registry's
    /// `retentionByAudience[<audience>] == "zdr"`.
    pub(crate) fn service_principal_claims(
        audience: &str,
        org_id: &str,
        service_id: &str,
        scopes: &[&str],
        reason: &str,
        zdr: bool,
    ) -> Value {
        let subject = format!("service:{service_id}");
        let now = now();
        json!({
            "iss": ISSUER, "aud": audience, "sub": &subject,
            "iat": now, "nbf": now - 5, "exp": now + 300,
            "org_id": org_id, "principal_type": "service", "service_id": &subject,
            "reason": reason, "scopes": scopes, "zdr": zdr,
        })
    }

    /// The token Model Gateway's `SessionTerminalTokenProvider::terminalize_token`
    /// gets for `org_id`. `zdr` is `true` because
    /// `config/plane-service-principals.json` gives `model-gateway` the posture
    /// `zdr` for `session-core`; Auth Core's audit of the live 2026-09-24 mint
    /// records exactly this.
    pub(crate) fn model_gateway_terminalize_claims(org_id: &str) -> Value {
        service_principal_claims(
            "session-core",
            org_id,
            "model-gateway",
            &["session:terminalize"],
            "model-gateway managed-run terminalization",
            true,
        )
    }

    /// An interactive user's token, as Auth Core's session path issues it.
    pub(crate) fn user_claims(audience: &str, org_id: &str, user_id: &str, zdr: bool) -> Value {
        let now = now();
        json!({
            "iss": ISSUER, "aud": audience, "sub": user_id,
            "iat": now, "nbf": now - 5, "exp": now + 300,
            "org_id": org_id, "user_id": user_id, "principal_type": "user",
            "zdr": zdr,
        })
    }

    /// `message` exactly as tonic hands it to a handler served behind
    /// `with_interceptor(.., verifier)`: the bearer goes through
    /// [`JwtVerifier::intercept`], whose extensions travel with the message.
    ///
    /// # Errors
    ///
    /// The interceptor's refusal, as the caller would receive it.
    #[allow(clippy::result_large_err)]
    pub(crate) fn authenticate<T>(
        verifier: &JwtVerifier,
        token: &str,
        message: T,
    ) -> Result<Request<T>, Status> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("bearer metadata"),
        );
        let (metadata, extensions, ()) = verifier.intercept(request)?.into_parts();
        Ok(Request::from_parts(metadata, extensions, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use serde_json::{json, Value};
    use tonic::Code;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    /// One key per test binary, shared with [`super::test_support`].
    fn rsa_keypair() -> &'static (String, String, String) {
        super::test_support::rsa_keypair()
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs()
    }

    fn user_claims(audience: &str) -> Value {
        json!({
            "sub": "user-1", "iss": "auth-core", "aud": audience,
            "exp": now() + 3600, "iat": now() - 5, "nbf": now() - 5,
            "org_id": "org-1", "user_id": "user-1", "principal_type": "user",
            "zdr": false
        })
    }

    fn ambiguous_claims() -> Value {
        let mut claims = user_claims("session-core");
        claims["service_id"] = json!("service-1");
        claims
    }

    fn sign(claims: &Value, kid: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_owned());
        encode(
            &header,
            claims,
            &EncodingKey::from_rsa_pem(rsa_keypair().0.as_bytes()).expect("encoding key"),
        )
        .expect("JWT")
    }

    async fn test_verifier(audience: &str) -> JwtVerifier {
        let server = MockServer::start().await;
        let (_, n, e) = rsa_keypair();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys": [{
                "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "key-1", "n": n, "e": e
            }]})))
            .mount(&server)
            .await;
        JwtVerifier::from_config(AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: audience.to_owned(),
            data_plane_audience: "data-plane".to_owned(),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    fn delegating_request(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            DATA_PLANE_AUTH_METADATA_KEY,
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    /// Mirrors model-gateway's `data_plane_grpc_authorization_uses_only_verified_bearer`
    /// intent at session-core's boundary: the credential forwarded to Data Plane
    /// v2 may only come from an independently verified, caller-bound delegation.
    /// A self-signed token, the ingress `aud=session-core` token, or another
    /// user's/org's `aud=data-plane` token must never yield a bearer.
    #[tokio::test]
    async fn delegated_data_plane_bearer_requires_verified_caller_bound_credential() {
        let verifier = test_verifier("session-core").await;
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");

        // No delegation at all → degrade to local memory, never a forged bearer.
        assert!(verifier
            .delegated_data_plane_bearer(&Request::new(()), &caller)
            .expect("absent delegation is not an error")
            .is_none());

        // Unverifiable or wrong-audience credentials are rejected outright.
        for token in [
            "not-a-jwt".to_owned(),
            // The ingress credential is never reusable for retrieval.
            sign(&user_claims("session-core"), "key-1"),
            // Signed by an untrusted key for the right audience.
            {
                let foreign = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
                let pem = foreign
                    .to_pkcs8_pem(LineEnding::LF)
                    .expect("private PEM")
                    .to_string();
                let mut header = Header::new(Algorithm::RS256);
                header.kid = Some("key-1".to_owned());
                encode(
                    &header,
                    &user_claims("data-plane"),
                    &EncodingKey::from_rsa_pem(pem.as_bytes()).expect("encoding key"),
                )
                .expect("JWT")
            },
            // Unknown kid cannot silently select a trusted key.
            sign(&user_claims("data-plane"), "attacker-kid"),
        ] {
            assert_eq!(
                verifier
                    .delegated_data_plane_bearer(&delegating_request(&token), &caller)
                    .expect_err("unverifiable delegation must not be forwarded")
                    .code(),
                tonic::Code::Unauthenticated
            );
        }

        // A genuine Data Plane token for a DIFFERENT identity, org, or retention
        // posture is verified but refused: forwarding it would let one caller read
        // another principal's private-until-shared corpus.
        for mutate in [
            (|claims: &mut Value| {
                claims["sub"] = json!("user-2");
                claims["user_id"] = json!("user-2");
            }) as fn(&mut Value),
            |claims: &mut Value| claims["org_id"] = json!("org-2"),
            |claims: &mut Value| claims["zdr"] = json!(true),
        ] {
            let mut claims = user_claims("data-plane");
            mutate(&mut claims);
            assert_eq!(
                verifier
                    .delegated_data_plane_bearer(
                        &delegating_request(&sign(&claims, "key-1")),
                        &caller
                    )
                    .expect_err("cross-identity delegation must be refused")
                    .code(),
                tonic::Code::PermissionDenied
            );
        }

        // A malformed header is an error, not a silent downgrade to no-grounding.
        let mut malformed = Request::new(());
        malformed
            .metadata_mut()
            .insert(DATA_PLANE_AUTH_METADATA_KEY, "not-bearer".parse().unwrap());
        assert_eq!(
            verifier
                .delegated_data_plane_bearer(&malformed, &caller)
                .expect_err("malformed delegation must fail closed")
                .code(),
            tonic::Code::Unauthenticated
        );

        // Only the caller's own verified data-plane credential is forwarded, and
        // it is forwarded verbatim so Data Plane re-verifies the same bytes.
        let token = sign(&user_claims("data-plane"), "key-1");
        let bearer = verifier
            .delegated_data_plane_bearer(&delegating_request(&token), &caller)
            .expect("caller-bound delegation verifies")
            .expect("bearer present");
        assert_eq!(bearer.as_str(), token);
        assert_eq!(
            format!("{bearer:?}"),
            "DelegatedDataPlaneBearer([REDACTED])"
        );
    }

    #[test]
    fn verified_identity_rejects_empty_and_cross_tenant_orgs() {
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert_eq!(
            caller.authorize_org("").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            caller.authorize_org("org-2").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        caller.authorize_org("org-1").expect("same tenant");
    }

    #[test]
    fn caller_owned_actor_and_user_cannot_be_forged() {
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert_eq!(
            caller.authorize_user("user-2").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(caller.principal_id(), "user-1");
    }

    #[test]
    fn issuer_zdr_blocks_every_durable_session_write() {
        let caller = VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        assert_eq!(
            authorize_operation(&caller, "session:write")
                .expect_err("ZDR content must not enter durable session storage")
                .code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(
            authorize_operation(&caller, "approval:decide")
                .expect_err("ZDR approval decisions must not create durable events")
                .code(),
            tonic::Code::FailedPrecondition
        );
        authorize_operation(&caller, "session:read").expect("non-persisting reads remain allowed");
    }

    #[tokio::test]
    async fn rejects_missing_malformed_forged_wrong_audience_and_ambiguous_principals() {
        let verifier = test_verifier("session-core").await;
        assert_eq!(
            verifier.intercept(Request::new(())).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        for token in [
            "not-a-jwt".to_owned(),
            // A legitimate token for another plane is still lateral misuse.
            sign(&user_claims("data-plane"), "key-1"),
            // Multi-audience credentials are not dedicated session tokens.
            {
                let mut claims = user_claims("session-core");
                claims["aud"] = json!(["session-core", "data-plane"]);
                sign(&claims, "key-1")
            },
            sign(&ambiguous_claims(), "key-1"),
            {
                let mut claims = user_claims("session-core");
                claims.as_object_mut().unwrap().remove("zdr");
                sign(&claims, "key-1")
            },
        ] {
            let mut request = Request::new(());
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().expect("metadata"),
            );
            assert_eq!(
                verifier.intercept(request).unwrap_err().code(),
                tonic::Code::Unauthenticated
            );
        }

        let token = sign(&user_claims("session-core"), "key-1");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        let request = verifier.intercept(request).expect("valid token");
        let identity = request
            .extensions()
            .get::<VerifiedIdentity>()
            .expect("identity");
        assert_eq!(identity.org_id(), "org-1");
        assert_eq!(identity.principal_id(), "user-1");
        assert!(!identity.zdr());

        let mut zdr_claims = user_claims("session-core");
        zdr_claims["zdr"] = json!(true);
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", sign(&zdr_claims, "key-1"))
                .parse()
                .expect("metadata"),
        );
        let request = verifier.intercept(request).expect("valid ZDR token");
        assert!(request
            .extensions()
            .get::<VerifiedIdentity>()
            .expect("identity")
            .zdr());
    }

    /// The terminalize token Model Gateway really holds, through the verifier
    /// production builds (JWKS over HTTP): a service identity whose `zdr` is
    /// the registry's posture, `true`. That marker still keeps it out of every
    /// generic durable write, `AppendMessage` included; only the atomic save,
    /// which takes retention from the run, may write with it.
    #[tokio::test]
    async fn model_gateways_terminalize_token_is_a_zdr_marked_service_identity() {
        let verifier = test_verifier("session-core").await;
        let token = sign(
            &test_support::model_gateway_terminalize_claims("org-1"),
            "key-1",
        );
        let request =
            test_support::authenticate(&verifier, &token, ()).expect("Auth Core-issued token");
        let caller = identity(&request).expect("identity");

        assert!(caller.is_service());
        assert_eq!(caller.principal_id(), "service:model-gateway");
        assert_eq!(caller.org_id(), "org-1");
        assert_eq!(caller.user_id(), None);
        assert!(caller.has_scope("session:terminalize"));
        assert!(caller.zdr(), "the registry posture, not the turn's");
        assert_eq!(
            authorize_operation(&caller, "session:write")
                .expect_err("a ZDR-marked credential makes no generic durable write")
                .code(),
            Code::FailedPrecondition
        );
    }

    // ── System-owned runs ───────────────────────────────────────────────────
    //
    // These pin the authorization layer that lets a durable workflow with no
    // human behind it own a run. Each test names the escalation it prevents.

    const ORCH: &str = "service:orchestrator-core";
    const BOTH_SCOPES: [&str; 2] = ["session:write", SYSTEM_RUN_OWNER_SCOPE];

    /// R8: the allowlist is exact. A prefix or fuzzy match would make every
    /// present and future `service:*` value a system owner, and `runs.user_id`
    /// has no CHECK constraining who may write what shape.
    #[test]
    fn owner_allowlist_is_exact_match() {
        assert!(is_system_run_owner(ORCH));
        for look_alike in [
            "service:orchestrator-core ",
            " service:orchestrator-core",
            "Service:Orchestrator-Core",
            "service:orchestrator-core-2",
            "service:orchestrator",
            "system:orchestrator-core",
            "service:orchestrator-core\0",
            "",
        ] {
            assert!(
                !is_system_run_owner(look_alike),
                "{look_alike:?} must not be treated as a system owner",
            );
        }
    }

    /// R8, second half: this is what pins the collision argument. Auth Core actor
    /// ids are base62, so a real person's id can never contain the `:` these
    /// values do.
    #[test]
    fn a_real_auth_core_actor_id_is_never_a_system_owner() {
        for actor in [
            "kx7Qa2Zb9Lm4",
            "MAF5Ey3xL8LwigSZ3ngKfGhUy54MlI2Y",
            "orchestratorcore",
        ] {
            assert!(!is_system_run_owner(actor));
            assert!(!actor.contains(':'), "base62 ids contain no colon");
        }
    }

    #[test]
    fn an_allowlisted_service_with_both_scopes_may_own_a_run() {
        let caller = VerifiedIdentity::service_for_test_as("org-1", ORCH, &BOTH_SCOPES, false);
        assert_eq!(
            authorize_system_run_owner(&caller).expect("allowlisted owner"),
            ORCH,
        );
    }

    /// R3: today's orchestrator-core token holds only `session:write`. Owning a
    /// run is a strictly larger power than writing sessions, so it must not be
    /// implied by the scope every session writer already has.
    #[test]
    fn system_run_requires_the_system_owner_scope() {
        let caller =
            VerifiedIdentity::service_for_test_as("org-1", ORCH, &["session:write"], false);
        let error = authorize_system_run_owner(&caller).expect_err("must require the scope");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R2: cross-service impersonation. Another workload holding the very same
    /// scopes still cannot own runs, because the owner is derived from its own
    /// signed identity rather than chosen.
    #[test]
    fn system_run_owner_must_be_allowlisted() {
        let caller = VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:capability-core",
            &BOTH_SCOPES,
            false,
        );
        let error = authorize_system_run_owner(&caller).expect_err("not allowlisted");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R4: a run row is durable, so a Zero Data Retention credential must never
    /// create one — asserted here as well as transitively via authorize_operation.
    #[test]
    fn zdr_service_credential_cannot_own_a_run() {
        let caller = VerifiedIdentity::service_for_test_as("org-1", ORCH, &BOTH_SCOPES, true);
        let error = authorize_system_run_owner(&caller).expect_err("ZDR must be refused");
        assert_eq!(error.code(), Code::FailedPrecondition);
        // And the pre-existing gate refuses it too, on the write scope.
        assert_eq!(
            authorize_operation(&caller, "session:write")
                .expect_err("ZDR write")
                .code(),
            Code::FailedPrecondition,
        );
    }

    #[test]
    fn space_deletion_requires_exact_coordinator_identity_and_scope() {
        let authorized = VerifiedIdentity::service_for_test_as(
            "org-1",
            SPACE_DELETION_SERVICE,
            &[SPACE_DELETION_SCOPE],
            false,
        );
        authorize_space_deletion_service(&authorized).expect("exact coordinator may erase");

        for (principal, scopes) in [
            (SPACE_DELETION_SERVICE, Vec::<&str>::new()),
            (
                "service:control-space-deletion-copy",
                vec![SPACE_DELETION_SCOPE],
            ),
            ("service:session-core", vec![SPACE_DELETION_SCOPE]),
        ] {
            let caller = VerifiedIdentity::service_for_test_as("org-1", principal, &scopes, false);
            assert_eq!(
                authorize_space_deletion_service(&caller)
                    .expect_err("a broad or look-alike service must not erase Space threads")
                    .code(),
                Code::PermissionDenied,
            );
        }
    }

    #[test]
    fn zdr_space_deletion_service_is_refused() {
        let caller = VerifiedIdentity::service_for_test_as(
            "org-1",
            SPACE_DELETION_SERVICE,
            &[SPACE_DELETION_SCOPE],
            true,
        );
        assert_eq!(
            authorize_space_deletion_service(&caller)
                .expect_err("ZDR credential must not perform durable erasure")
                .code(),
            Code::FailedPrecondition,
        );
    }

    #[test]
    fn run_action_authority_requires_exact_control_identity_scope_and_non_zdr() {
        let authorized = VerifiedIdentity::service_for_test_as(
            "org-1",
            RUN_ACTION_AUTHORITY_SERVICE,
            &[RUN_ACTION_AUTHORITY_SCOPE],
            false,
        );
        authorize_run_action_authority_service(&authorized)
            .expect("exact Control action-authorizer may resolve run bindings");

        for (principal, scopes, zdr, expected) in [
            (
                RUN_ACTION_AUTHORITY_SERVICE,
                Vec::<&str>::new(),
                false,
                Code::PermissionDenied,
            ),
            (
                "service:control-action-authorizer-copy",
                vec![RUN_ACTION_AUTHORITY_SCOPE],
                false,
                Code::PermissionDenied,
            ),
            (
                "service:execution-core",
                vec![RUN_ACTION_AUTHORITY_SCOPE],
                false,
                Code::PermissionDenied,
            ),
            (
                RUN_ACTION_AUTHORITY_SERVICE,
                vec![RUN_ACTION_AUTHORITY_SCOPE],
                true,
                Code::FailedPrecondition,
            ),
        ] {
            let caller = VerifiedIdentity::service_for_test_as("org-1", principal, &scopes, zdr);
            assert_eq!(
                authorize_run_action_authority_service(&caller)
                    .expect_err("only the exact non-ZDR Control principal may resolve authority")
                    .code(),
                expected,
            );
        }
    }

    /// A human never reaches the system branch: the kind is asserted explicitly
    /// rather than inferred from a missing user_id.
    #[test]
    fn a_human_is_not_a_system_run_owner() {
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error = authorize_system_run_owner(&caller).expect_err("humans use the normal path");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// The read allowance: a system run's content is by construction the
    /// org-visible set, so anyone in the org may observe it.
    #[test]
    fn a_system_owned_row_is_readable_by_anyone_in_the_org() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        authorize_owner_row(&human, "org-1", ORCH, OwnerIntent::Read).expect("org-readable");
    }

    /// R7: the read allowance must not become a write allowance.
    #[test]
    fn humans_cannot_mutate_a_system_owned_row() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error = authorize_owner_row(&human, "org-1", ORCH, OwnerIntent::Mutate)
            .expect_err("humans must not drive a system run");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R7, second half: only the workload that owns the run may act on it. This
    /// is what makes the derived owner id matter — a shared sentinel would let
    /// every service holding the scope mutate every system run.
    #[test]
    fn only_the_owning_workload_can_mutate_a_system_owned_row() {
        let owner = VerifiedIdentity::service_for_test_as("org-1", ORCH, &BOTH_SCOPES, false);
        authorize_owner_row(&owner, "org-1", ORCH, OwnerIntent::Mutate).expect("owner may mutate");

        let other = VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:capability-core",
            &BOTH_SCOPES,
            false,
        );
        let error = authorize_owner_row(&other, "org-1", ORCH, OwnerIntent::Mutate)
            .expect_err("a different workload must be refused");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R6: the org boundary is absolute and is checked before any system branch.
    #[test]
    fn system_owned_rows_are_not_readable_across_orgs() {
        let outsider = VerifiedIdentity::user_for_test("org-2", "user-2");
        for intent in [OwnerIntent::Read, OwnerIntent::Mutate] {
            let error = authorize_owner_row(&outsider, "org-1", ORCH, intent)
                .expect_err("cross-org must be refused");
            assert_eq!(error.code(), Code::PermissionDenied);
        }
        let outside_service =
            VerifiedIdentity::service_for_test_as("org-2", ORCH, &BOTH_SCOPES, false);
        let error = authorize_owner_row(&outside_service, "org-1", ORCH, OwnerIntent::Mutate)
            .expect_err("even the owning workload is org-scoped");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R8: a look-alike owner gets none of the system treatment — it falls
    /// through to the ordinary rules, where a human must match it exactly.
    #[test]
    fn a_look_alike_owner_gets_no_system_treatment() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error = authorize_owner_row(
            &human,
            "org-1",
            "service:orchestrator-core-2",
            OwnerIntent::Read,
        )
        .expect_err("not a system row, so the user check applies");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// Non-system rows keep their existing behaviour exactly, including the
    /// pre-existing service skip that RecordTerminalOutcome depends on.
    #[test]
    fn ordinary_rows_are_unchanged_by_the_system_branch() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        for intent in [OwnerIntent::Read, OwnerIntent::Mutate] {
            authorize_owner_row(&human, "org-1", "user-1", intent).expect("own row");
            assert_eq!(
                authorize_owner_row(&human, "org-1", "user-2", intent)
                    .expect_err("another person's row")
                    .code(),
                Code::PermissionDenied,
            );
        }
        let service = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
        authorize_owner_row(&service, "org-1", "user-1", OwnerIntent::Mutate)
            .expect("pre-existing service skip on ordinary rows");
    }

    /// The SQL binding source and the authorization predicate must be the same
    /// set, or the org-scoped listing would show rows the guard does not admit.
    #[test]
    fn the_sql_owner_list_matches_the_predicate() {
        for owner in system_run_owners() {
            assert!(is_system_run_owner(owner));
        }
        assert_eq!(system_run_owners().len(), SYSTEM_RUN_OWNERS.len());
    }

    /// D12: a system-owned thread inside a person's Space is that person's
    /// content, not the org's. An org peer reading it by id is refused with
    /// "not found", the answer a thread that does not exist gives.
    #[test]
    fn an_org_peer_cannot_read_a_space_scoped_system_thread() {
        let peer = VerifiedIdentity::user_for_test("org-1", "user-2");
        let error =
            authorize_thread_owner_row(&peer, "org-1", ORCH, "space-personal", OwnerIntent::Read)
                .expect_err("a colleague must not read a scheduled thread by id");
        assert_eq!(error.code(), Code::NotFound);
    }

    /// The schedule's owner is a person, not the workload that owns the row, so
    /// ownership does not admit them either: they come in through the Space read
    /// decision, which is a different branch that never calls this function.
    #[test]
    fn the_schedule_owner_gets_no_read_from_this_predicate_either() {
        let owner = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error =
            authorize_thread_owner_row(&owner, "org-1", ORCH, "space-personal", OwnerIntent::Read)
                .expect_err("the decision branch is the only way in");
        assert_eq!(error.code(), Code::NotFound);
    }

    /// Maintenance system threads have no Space and keep the org-wide read the
    /// system-run allowance exists for. A blank Space is no Space.
    #[test]
    fn a_maintenance_system_thread_with_no_space_stays_org_readable() {
        let peer = VerifiedIdentity::user_for_test("org-1", "user-2");
        for space in ["", "   "] {
            authorize_thread_owner_row(&peer, "org-1", ORCH, space, OwnerIntent::Read)
                .expect("an unscoped system thread is org-readable");
        }
    }

    /// Services keep their existing read, as on every other row: the owning
    /// workload and the planes that act for a run depend on it.
    #[test]
    fn a_service_keeps_its_read_of_a_space_scoped_system_thread() {
        let owner = VerifiedIdentity::service_for_test_as("org-1", ORCH, &BOTH_SCOPES, false);
        authorize_thread_owner_row(&owner, "org-1", ORCH, "space-personal", OwnerIntent::Read)
            .expect("the owning workload reads its own thread");
        let other = VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:capability-core",
            &BOTH_SCOPES,
            false,
        );
        authorize_thread_owner_row(&other, "org-1", ORCH, "space-personal", OwnerIntent::Read)
            .expect("a service principal keeps the read it has on ordinary rows");
    }

    /// Mutation is exactly what it was: only the owning workload, and a human is
    /// still told "permission denied" rather than "not found".
    #[test]
    fn mutating_a_space_scoped_system_thread_is_unchanged() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error = authorize_thread_owner_row(
            &human,
            "org-1",
            ORCH,
            "space-personal",
            OwnerIntent::Mutate,
        )
        .expect_err("humans never drive a system thread");
        assert_eq!(error.code(), Code::PermissionDenied);
        let owner = VerifiedIdentity::service_for_test_as("org-1", ORCH, &BOTH_SCOPES, false);
        authorize_thread_owner_row(&owner, "org-1", ORCH, "space-personal", OwnerIntent::Mutate)
            .expect("the owning workload may still act on its thread");
    }

    /// R6 again: the org boundary is checked before the Space rule, so a
    /// cross-org caller still gets the refusal it always got.
    #[test]
    fn the_org_check_still_comes_before_the_space_rule() {
        let outsider = VerifiedIdentity::user_for_test("org-2", "user-2");
        for intent in [OwnerIntent::Read, OwnerIntent::Mutate] {
            let error =
                authorize_thread_owner_row(&outsider, "org-1", ORCH, "space-personal", intent)
                    .expect_err("cross-org must be refused");
            assert_eq!(error.code(), Code::PermissionDenied);
        }
    }

    /// The Space rule is about system-owned rows only: an ordinary thread in a
    /// Space, and a look-alike owner, behave exactly as under `authorize_owner_row`.
    #[test]
    fn ordinary_threads_ignore_the_space_rule() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        for intent in [OwnerIntent::Read, OwnerIntent::Mutate] {
            authorize_thread_owner_row(&human, "org-1", "user-1", "space-room", intent)
                .expect("a person's own thread in a Space");
            assert_eq!(
                authorize_thread_owner_row(&human, "org-1", "user-2", "space-room", intent)
                    .expect_err("another person's thread")
                    .code(),
                Code::PermissionDenied,
            );
        }
        let error = authorize_thread_owner_row(
            &human,
            "org-1",
            "service:orchestrator-core-2",
            "space-room",
            OwnerIntent::Read,
        )
        .expect_err("a look-alike owner gets no system treatment, so the user check applies");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// The claims Auth Core stamps on a persistent token of a ZDR org whose
    /// session user acknowledged one connection that cannot give ZDR.
    fn waived_user_claims(audience: &str, connection: &str, ack_id: &str) -> Value {
        let mut claims = user_claims(audience);
        claims["org_zdr"] = json!(true);
        claims["zdr_waiver"] = json!({
            "version": 1,
            "provider": "openai-codex-subscription",
            "connection": connection,
            "ack_id": ack_id,
        });
        claims
    }

    /// The waiver the verifier reads is the one Auth Core stamped, and it rides
    /// only the persistent token it was stamped on. A token without the claim
    /// carries no waiver at all.
    #[tokio::test]
    async fn zdr_waiver_is_read_from_a_persistent_token_of_a_zdr_org() {
        let verifier = test_verifier("session-core").await;
        let token = sign(
            &waived_user_claims("session-core", "conn-1", "ack-1"),
            "key-1",
        );
        let request = test_support::authenticate(&verifier, &token, ()).expect("valid waiver");
        let caller = identity(&request).expect("identity");
        assert!(!caller.zdr(), "a waiver rides a persistent token");
        let waiver = caller.zdr_waiver().expect("the waiver is carried");
        assert_eq!(waiver.provider(), "openai-codex-subscription");
        assert_eq!(waiver.connection_id(), "conn-1");
        assert_eq!(waiver.ack_id(), "ack-1");

        let plain = sign(&user_claims("session-core"), "key-1");
        let request = test_support::authenticate(&verifier, &plain, ()).expect("plain token");
        assert!(identity(&request).expect("identity").zdr_waiver().is_none());
    }

    /// An unknown version, a wrong provider, an empty or padded id, a missing
    /// acknowledgement, a waiver on a ZDR token, a waiver without a ZDR org, and
    /// a waiver on a service credential are all refused. None is silently
    /// dropped: a dropped waiver would be recorded as a persistent run in a ZDR
    /// org.
    #[tokio::test]
    async fn malformed_or_misplaced_zdr_waivers_are_refused_not_dropped() {
        let verifier = test_verifier("session-core").await;
        let good = || waived_user_claims("session-core", "conn-1", "ack-1");
        let mut cases: Vec<(&str, Value)> = Vec::new();
        cases.push(("unknown waiver version", {
            let mut claims = good();
            claims["zdr_waiver"]["version"] = json!(2);
            claims
        }));
        cases.push(("provider the waiver does not bind", {
            let mut claims = good();
            claims["zdr_waiver"]["provider"] = json!("azure-openai");
            claims
        }));
        cases.push(("empty connection", {
            let mut claims = good();
            claims["zdr_waiver"]["connection"] = json!("");
            claims
        }));
        cases.push(("padded connection", {
            let mut claims = good();
            claims["zdr_waiver"]["connection"] = json!(" conn-1 ");
            claims
        }));
        cases.push(("blank acknowledgement", {
            let mut claims = good();
            claims["zdr_waiver"]["ack_id"] = json!("  ");
            claims
        }));
        cases.push(("acknowledgement missing", {
            let mut claims = good();
            claims["zdr_waiver"]
                .as_object_mut()
                .expect("object")
                .remove("ack_id");
            claims
        }));
        cases.push(("waiver that is not an object", {
            let mut claims = good();
            claims["zdr_waiver"] = json!("conn-1");
            claims
        }));
        cases.push(("waiver on a ZDR token", {
            let mut claims = good();
            claims["zdr"] = json!(true);
            claims
        }));
        cases.push(("waiver in an org that is not ZDR", {
            let mut claims = good();
            claims["org_zdr"] = json!(false);
            claims
        }));
        cases.push(("waiver with no org_zdr claim", {
            let mut claims = good();
            claims.as_object_mut().expect("object").remove("org_zdr");
            claims
        }));
        cases.push(("persistent token of a ZDR org with no waiver", {
            let mut claims = good();
            claims.as_object_mut().expect("object").remove("zdr_waiver");
            claims
        }));
        cases.push(("waiver on a service credential", {
            let mut claims = good();
            claims.as_object_mut().expect("object").remove("user_id");
            claims["principal_type"] = json!("service");
            claims["service_id"] = json!("service:verevon-gateway");
            claims["sub"] = json!("service:verevon-gateway");
            claims
        }));
        for (case, claims) in cases {
            let token = sign(&claims, "key-1");
            let mut request = Request::new(());
            request.metadata_mut().insert(
                "authorization",
                format!("Bearer {token}").parse().expect("metadata"),
            );
            assert_eq!(
                verifier.intercept(request).unwrap_err().code(),
                Code::Unauthenticated,
                "{case} must be refused"
            );
        }
    }

    /// Every bearer of one request must carry the same waiver. A delegated
    /// Data Plane bearer that is persistent without the waiver, or that is
    /// waived for another connection, is refused as a posture mismatch.
    #[tokio::test]
    async fn delegated_data_plane_bearer_must_carry_the_same_waiver() {
        let verifier = test_verifier("session-core").await;
        let waived_caller =
            VerifiedIdentity::user_for_test_with_waiver("org-1", "user-1", "conn-1", "ack-1");

        let same = sign(
            &waived_user_claims("data-plane", "conn-1", "ack-1"),
            "key-1",
        );
        verifier
            .delegated_data_plane_bearer(&delegating_request(&same), &waived_caller)
            .expect("the same waiver verifies")
            .expect("the bearer is forwarded");

        let bare = sign(&user_claims("data-plane"), "key-1");
        assert_eq!(
            verifier
                .delegated_data_plane_bearer(&delegating_request(&bare), &waived_caller)
                .expect_err("a waived caller never forwards a bare persistent bearer")
                .code(),
            Code::PermissionDenied
        );

        let other_connection = sign(
            &waived_user_claims("data-plane", "conn-2", "ack-2"),
            "key-1",
        );
        assert_eq!(
            verifier
                .delegated_data_plane_bearer(&delegating_request(&other_connection), &waived_caller)
                .expect_err("another connection's waiver is refused")
                .code(),
            Code::PermissionDenied
        );

        let plain_caller = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert_eq!(
            verifier
                .delegated_data_plane_bearer(&delegating_request(&same), &plain_caller)
                .expect_err("a caller with no waiver never forwards a waived bearer")
                .code(),
            Code::PermissionDenied
        );
    }

    /// WS06-S1: an org or user mismatch of the delegated Data Plane bearer is a plain identity
    /// refusal (no ZDR kind trailer), while a ZDR or waiver mismatch is the typed
    /// `zdr_posture_mismatch` refusal, both `PermissionDenied`.
    #[tokio::test]
    async fn delegated_data_plane_identity_mismatch_is_not_a_posture_refusal() {
        use mp_contracts::zdr_refusal::{code, Refusal};
        let verifier = test_verifier("session-core").await;
        let caller = VerifiedIdentity::user_for_test("org-1", "user-1");

        for mutate in [
            (|claims: &mut Value| {
                claims["sub"] = json!("user-2");
                claims["user_id"] = json!("user-2");
            }) as fn(&mut Value),
            |claims: &mut Value| claims["org_id"] = json!("org-2"),
        ] {
            let mut claims = user_claims("data-plane");
            mutate(&mut claims);
            let status = verifier
                .delegated_data_plane_bearer(&delegating_request(&sign(&claims, "key-1")), &caller)
                .expect_err("identity mismatch is refused");
            assert_eq!(status.code(), Code::PermissionDenied);
            assert!(
                Refusal::from_status(&status).is_none(),
                "an identity mismatch carries no ZDR kind trailer"
            );
        }

        // A ZDR mismatch: the persistent caller is offered a ZDR bearer.
        let mut zdr_claims = user_claims("data-plane");
        zdr_claims["zdr"] = json!(true);
        let status = verifier
            .delegated_data_plane_bearer(&delegating_request(&sign(&zdr_claims, "key-1")), &caller)
            .expect_err("a ZDR mismatch is refused");
        assert_eq!(status.code(), Code::PermissionDenied);
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(code::POSTURE_MISMATCH)
        );

        // A waiver mismatch: a waived caller is offered a bare persistent bearer.
        let waived_caller =
            VerifiedIdentity::user_for_test_with_waiver("org-1", "user-1", "conn-1", "ack-1");
        let bare = sign(&user_claims("data-plane"), "key-1");
        let status = verifier
            .delegated_data_plane_bearer(&delegating_request(&bare), &waived_caller)
            .expect_err("a waiver mismatch is refused");
        assert_eq!(status.code(), Code::PermissionDenied);
        assert_eq!(
            Refusal::from_status(&status).map(|refusal| refusal.code()),
            Some(code::POSTURE_MISMATCH)
        );
    }

    /// WS06-S1: the six pure ZDR refusal sites of `auth.rs` each carry the catalogue kind
    /// (the seventh, the delegated bearer posture mismatch, is pinned above).
    fn assert_typed(status: &Status, expected_code: Code, kind: &str, feature: Option<&str>) {
        use mp_contracts::zdr_refusal::Refusal;
        assert_eq!(status.code(), expected_code);
        let refusal = Refusal::from_status(status).expect("the status carries a catalogue kind");
        assert_eq!(refusal.code(), kind);
        assert_eq!(refusal.feature(), feature);
    }

    #[test]
    fn zdr_refusal_authorize_operation_write_is_durable_mutation_forbidden() {
        let zdr = VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        let status = authorize_operation(&zdr, "session:write").expect_err("ZDR write is refused");
        assert_typed(&status, Code::FailedPrecondition, "zdr_durable_mutation_forbidden", None);
        // A read stays open to a ZDR credential.
        assert!(authorize_operation(&zdr, "session:read").is_ok());
    }

    #[test]
    fn zdr_refusal_space_deletion_service_is_durable_mutation_forbidden() {
        let zdr = VerifiedIdentity::service_for_test_as(
            "org-1",
            SPACE_DELETION_SERVICE,
            &[],
            true,
        );
        let status = authorize_space_deletion_service(&zdr).expect_err("ZDR is refused");
        assert_typed(&status, Code::FailedPrecondition, "zdr_durable_mutation_forbidden", None);
    }

    #[test]
    fn zdr_refusal_scheduled_step_service_names_the_scheduled_step_feature() {
        let zdr = VerifiedIdentity::service_for_test_as("org-1", SCHEDULED_STEP_SERVICE, &[], true);
        let status = authorize_scheduled_step_service(&zdr).expect_err("ZDR is refused");
        assert_typed(
            &status,
            Code::FailedPrecondition,
            "zdr_feature_not_supported",
            Some("scheduled_step"),
        );
    }

    #[test]
    fn zdr_refusal_run_action_authority_names_the_agent_run_feature() {
        let zdr = VerifiedIdentity::service_for_test_as(
            "org-1",
            RUN_ACTION_AUTHORITY_SERVICE,
            &[],
            true,
        );
        let status = authorize_run_action_authority_service(&zdr).expect_err("ZDR is refused");
        assert_typed(
            &status,
            Code::FailedPrecondition,
            "zdr_feature_not_supported",
            Some("agent_run"),
        );
    }

    #[test]
    fn zdr_refusal_scheduled_step_authority_names_the_scheduled_step_feature() {
        let zdr = VerifiedIdentity::service_for_test_as(
            "org-1",
            RUN_ACTION_AUTHORITY_SERVICE,
            &[],
            true,
        );
        let status = authorize_scheduled_step_authority_service(&zdr).expect_err("ZDR is refused");
        assert_typed(
            &status,
            Code::FailedPrecondition,
            "zdr_feature_not_supported",
            Some("scheduled_step"),
        );
    }

    #[test]
    fn zdr_refusal_system_run_owner_is_durable_mutation_forbidden() {
        let zdr = VerifiedIdentity::service_for_test_as(
            "org-1",
            "service:execution-core",
            &[SYSTEM_RUN_OWNER_SCOPE],
            true,
        );
        let status = authorize_system_run_owner(&zdr).expect_err("ZDR cannot own a durable run");
        assert_typed(&status, Code::FailedPrecondition, "zdr_durable_mutation_forbidden", None);
    }

    /// Owner decision 17: a zero-retention user must always be able to delete or archive the threads
    /// and memories they own. The ordinary write gate still refuses that user, so the housekeeping
    /// gate is the only door open to them, and a service caller is held to the write gate.
    #[test]
    fn owner_housekeeping_is_open_to_a_zdr_user_and_still_scoped_for_services() {
        let zdr_user = VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true);
        assert!(authorize_owner_housekeeping(&zdr_user, "session:write").is_ok());
        assert_eq!(
            authorize_operation(&zdr_user, "session:write")
                .expect_err("the ordinary write gate still refuses a ZDR credential")
                .code(),
            Code::FailedPrecondition
        );

        let persistent_user = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert!(authorize_owner_housekeeping(&persistent_user, "session:write").is_ok());

        let unscoped = VerifiedIdentity::service_for_test("org-1", &[], false);
        assert_eq!(
            authorize_owner_housekeeping(&unscoped, "session:write")
                .expect_err("a service without the scope is refused")
                .code(),
            Code::PermissionDenied
        );
        let scoped = VerifiedIdentity::service_for_test("org-1", &["session:write"], false);
        assert!(authorize_owner_housekeeping(&scoped, "session:write").is_ok());
    }

    /// An `api_key` token as Auth Core's API-key exchange mints it for `aud=session-core`.
    fn api_key_claims() -> Value {
        let now = now();
        json!({
            "sub": "apikey:key-1", "iss": "auth-core", "aud": "session-core",
            "iat": now, "nbf": now - 5, "exp": now + 120,
            "org_id": "org-1", "principal_type": "api_key",
            "key_id": "key-1", "acting_user_id": "user-1", "jti": "jti-1",
            "scopes": [API_KEY_SCOPE], "zdr": false
        })
    }

    fn api_key_verifier() -> JwtVerifier {
        super::test_support::verifier("session-core").with_api_key_principal(true)
    }

    /// A call to `path` carrying `claims`, as [`RpcPathStamp`] hands it to the interceptor.
    fn call_on(path: &str, claims: &Value) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", super::test_support::sign(claims, super::test_support::KID))
                .parse()
                .expect("bearer metadata"),
        );
        request.extensions_mut().insert(RpcPath(Arc::from(path)));
        request
    }

    const START_RUN: &str = "/model_plane.v1.SessionCore/StartRun";

    #[test]
    fn api_key_shape_is_exact() {
        let verifier = api_key_verifier();
        let request = verifier
            .intercept(call_on(START_RUN, &api_key_claims()))
            .expect("a well-formed api key token");
        let key = identity(&request).expect("identity installed");
        assert!(key.is_api_key());
        assert!(!key.is_service());
        assert_eq!(key.principal_id(), "apikey:key-1");
        assert_eq!(key.api_key_id(), Some("key-1"));
        assert_eq!(key.acting_user_id(), Some("user-1"));
        assert_eq!(key.owner_id(), Some("apikey:key-1"));
        // The person is attribution only: nothing that reads `user_id` treats a key as them.
        assert_eq!(key.user_id(), None);

        let long_key = "k".repeat(129);
        for (field, value) in [
            ("sub", json!("apikey:key-2")),
            ("sub", json!("key-1")),
            ("key_id", json!("")),
            ("key_id", json!("key 1")),
            ("key_id", json!(long_key)),
            ("key_id", Value::Null),
            ("acting_user_id", json!("")),
            ("acting_user_id", json!(" user-1")),
            ("acting_user_id", json!("apikey:key-9")),
            ("acting_user_id", json!("u".repeat(257))),
            ("acting_user_id", Value::Null),
            ("user_id", json!("user-1")),
            ("service_id", json!("service:x")),
            ("principal_type", json!("apikey")),
            ("org_zdr", json!(true)),
        ] {
            let mut claims = api_key_claims();
            if value.is_null() {
                claims.as_object_mut().expect("claims object").remove(field);
            } else {
                claims[field] = value.clone();
            }
            assert_eq!(
                verifier
                    .intercept(call_on(START_RUN, &claims))
                    .expect_err("a malformed key token is refused")
                    .code(),
                Code::Unauthenticated,
                "{field} = {value}"
            );
        }

        // While keys are admitted, a key id on a person's token makes it ambiguous.
        let mut user_with_key = user_claims("session-core");
        user_with_key["key_id"] = json!("key-1");
        assert_eq!(
            verifier
                .intercept(call_on(START_RUN, &user_with_key))
                .expect_err("an ambiguous token is refused")
                .code(),
            Code::Unauthenticated
        );
    }

    #[test]
    fn api_key_refused_when_switch_off() {
        let verifier = super::test_support::verifier("session-core");
        assert_eq!(
            verifier
                .intercept(call_on(START_RUN, &api_key_claims()))
                .expect_err("off, a key token is not a principal")
                .code(),
            Code::Unauthenticated
        );
        // Off, a person's token is read exactly as before, a stray key id included.
        let mut user_with_key = user_claims("session-core");
        user_with_key["key_id"] = json!("key-1");
        let request = verifier
            .intercept(call_on(START_RUN, &user_with_key))
            .expect("unchanged while the switch is off");
        let user = identity(&request).expect("identity installed");
        assert!(!user.is_api_key());
        assert_eq!(user.user_id(), Some("user-1"));
        assert_eq!(user.owner_id(), Some("user-1"));
    }

    #[test]
    fn api_key_principal_parser_takes_only_literal_true() {
        for raw in ["true", " true "] {
            assert!(api_key_principal_enabled(Some(raw)), "{raw:?}");
        }
        for raw in [None, Some(""), Some("1"), Some("on"), Some("yes"), Some("false")] {
            assert!(!api_key_principal_enabled(raw), "{raw:?}");
        }
    }

    /// Every RPC in every proto file, as a gRPC path. Read from the sources, so an RPC added
    /// later is in the list without anyone touching this test.
    fn every_rpc_path() -> Vec<String> {
        fn walk(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("proto directory is readable") {
                let path = entry.expect("proto directory entry").path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.extension().is_some_and(|extension| extension == "proto") {
                    files.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../proto");
        let mut files = Vec::new();
        walk(&root, &mut files);
        let mut paths = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).expect("proto source is readable");
            let mut package = "";
            let mut service = "";
            for line in source.lines().map(str::trim) {
                if let Some(rest) = line.strip_prefix("package ") {
                    package = rest.trim_end_matches(';').trim();
                } else if let Some(rest) = line.strip_prefix("service ") {
                    service = rest.split_whitespace().next().unwrap_or_default();
                } else if let Some(rest) = line.strip_prefix("rpc ") {
                    let method = rest.split('(').next().unwrap_or_default().trim();
                    paths.push(format!("/{package}.{service}/{method}"));
                }
            }
        }
        paths
    }

    #[test]
    fn api_key_method_allow_list_is_closed() {
        let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", false);
        let paths = every_rpc_path();
        assert!(paths.len() > API_KEY_METHODS.len(), "the proto sources were read");
        // Every listed method is a real RPC, so a rename cannot silently empty the list.
        for (method, _) in API_KEY_METHODS {
            assert!(
                paths.iter().any(|path| path.as_str() == *method),
                "{method} is not an RPC"
            );
        }
        let mut admitted = Vec::new();
        for path in &paths {
            match authorize_api_key_call(&key, Some(path)) {
                Ok(()) => admitted.push(path.as_str()),
                Err(status) => assert_eq!(status.code(), Code::PermissionDenied, "{path}"),
            }
        }
        admitted.sort_unstable();
        admitted.dedup();
        let mut expected: Vec<&str> = API_KEY_METHODS.iter().map(|(method, _)| *method).collect();
        expected.sort_unstable();
        assert_eq!(admitted, expected);
        for path in [
            "/model_plane.v1.SessionCore/StartScheduledRun",
            "/model_plane.v1.ManagedRunLifecycle/RecordTerminalOutcome",
            "/model_plane.v1.SessionCore/CreateThread/",
            "/model_plane.v1.sessioncore/CreateThread",
            "",
        ] {
            assert_eq!(api_key_method_allowed(path), None, "{path:?}");
        }

        // A service that is not stamped hands the interceptor no path, and a key is refused.
        let verifier = api_key_verifier();
        let mut unstamped = call_on(START_RUN, &api_key_claims());
        unstamped.extensions_mut().remove::<RpcPath>();
        assert_eq!(
            verifier
                .intercept(unstamped)
                .expect_err("no path, no admission")
                .code(),
            Code::PermissionDenied
        );
        for unlisted in [
            "/model_plane.v1.SessionCore/ListThreads",
            "/model_plane.v1.SessionCore/GetContextAssembly",
            "/model_plane.v1.RunService/ResolveApprovalAuthority",
        ] {
            assert_eq!(
                verifier
                    .intercept(call_on(unlisted, &api_key_claims()))
                    .expect_err("an unlisted method is refused")
                    .code(),
                Code::PermissionDenied,
                "{unlisted}"
            );
        }

        // A key token without the exchange's scope reaches nothing.
        let mut unscoped = api_key_claims();
        unscoped["scopes"] = json!(["session:write"]);
        assert_eq!(
            verifier
                .intercept(call_on(START_RUN, &unscoped))
                .expect_err("the agent:runs scope is required")
                .code(),
            Code::PermissionDenied
        );
    }

    #[test]
    fn api_key_never_carries_a_waiver() {
        let verifier = api_key_verifier();
        let mut waived = api_key_claims();
        waived["org_zdr"] = json!(true);
        waived["zdr_waiver"] = json!({
            "version": WAIVER_CONTRACT_VERSION,
            "provider": mp_contracts::agentic_subscription::PROVIDER_ID,
            "connection": "conn-a",
            "ack_id": "ack-1",
        });
        assert_eq!(
            verifier
                .intercept(call_on(START_RUN, &waived))
                .expect_err("a key has no connection to waive")
                .code(),
            Code::Unauthenticated
        );
        let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", false);
        assert!(key.zdr_waiver().is_none());
    }

    #[test]
    fn zdr_api_key_write_is_refused() {
        let verifier = api_key_verifier();
        let mut zdr = api_key_claims();
        zdr["zdr"] = json!(true);
        zdr["org_zdr"] = json!(true);
        for (method, writes) in API_KEY_METHODS {
            let result = verifier.intercept(call_on(method, &zdr));
            if *writes {
                assert_typed(
                    &result.expect_err("a ZDR key writes nothing"),
                    Code::FailedPrecondition,
                    mp_contracts::zdr_refusal::code::FEATURE_NOT_SUPPORTED,
                    Some("agent_api"),
                );
            } else {
                assert!(result.is_ok(), "{method}: a read stores nothing");
            }
        }
        // The handlers' own write gate refuses a ZDR key as well.
        let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", true);
        assert_eq!(
            authorize_operation(&key, "session:write")
                .expect_err("a ZDR credential never writes")
                .code(),
            Code::FailedPrecondition
        );
    }

    #[test]
    fn api_key_owns_only_its_own_rows() {
        let key = VerifiedIdentity::api_key_for_test("org-1", "key-1", "user-1", false);
        for intent in [OwnerIntent::Read, OwnerIntent::Mutate] {
            assert!(authorize_owner_row(&key, "org-1", "apikey:key-1", intent).is_ok());
            // Not the person who owns the key, not another key, not a system run, not another org.
            for owner in ["user-1", "apikey:key-2", "service:orchestrator-core"] {
                assert_eq!(
                    authorize_owner_row(&key, "org-1", owner, intent)
                        .expect_err("a key reaches only its own rows")
                        .code(),
                    Code::PermissionDenied,
                    "{owner}"
                );
            }
            assert!(authorize_owner_row(&key, "org-2", "apikey:key-1", intent).is_err());
        }
        // A key cannot own a system run, and the person cannot reach the key's rows.
        assert!(authorize_system_run_owner(&key).is_err());
        assert!(is_api_key_owner(key.owner_id().expect("a key owns rows")));
        assert!(!is_api_key_owner("user-1"));
        assert!(!is_api_key_owner("service:orchestrator-core"));
        let person = VerifiedIdentity::user_for_test("org-1", "user-1");
        assert_eq!(
            authorize_owner_row(&person, "org-1", "apikey:key-1", OwnerIntent::Read)
                .expect_err("a key thread is not its owner's chat")
                .code(),
            Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn rpc_path_stamp_hands_the_path_to_the_interceptor() {
        use tonic::codegen::{http, Service as _};

        #[derive(Clone)]
        struct Capture;
        impl tonic::codegen::Service<http::Request<()>> for Capture {
            type Response = Option<String>;
            type Error = std::convert::Infallible;
            type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

            fn poll_ready(
                &mut self,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), Self::Error>> {
                std::task::Poll::Ready(Ok(()))
            }

            fn call(&mut self, request: http::Request<()>) -> Self::Future {
                std::future::ready(Ok(request
                    .extensions()
                    .get::<RpcPath>()
                    .map(|path| path.0.to_string())))
            }
        }

        let request = http::Request::builder()
            .uri(format!("http://session-core{START_RUN}"))
            .body(())
            .expect("request");
        let seen = RpcPathStamp::new(Capture)
            .call(request)
            .await
            .expect("infallible");
        assert_eq!(seen.as_deref(), Some(START_RUN));
    }
}
