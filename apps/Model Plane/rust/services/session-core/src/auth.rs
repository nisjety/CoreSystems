//! Fail-closed Auth Core verification for session-core's gRPC boundary.
//!
//! Verification material is fetched before the listener starts. The tonic
//! interceptor then performs RS256 verification synchronously and attaches a
//! canonical identity to request extensions. No caller-supplied org/user/actor
//! field becomes authority by itself.

use std::{collections::HashMap, fmt, sync::Arc, time::Duration};

use base64::Engine as _;
use jsonwebtoken::{decode, jwk::JwkSet, Algorithm, DecodingKey, Validation};
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
}

impl fmt::Debug for JwtVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JwtVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("data_plane_audience", &self.data_plane_audience)
            .field("leeway_secs", &self.leeway_secs)
            .field("key_count", &self.keys.len())
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PrincipalKind {
    User,
    Service,
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

    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|candidate| candidate == scope)
    }

    /// Whether the trusted issuer marked this credential Zero Data Retention.
    #[must_use]
    pub fn zdr(&self) -> bool {
        self.zdr
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
        match self.user_id() {
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
        return Err(Status::failed_precondition(
            "ZDR credentials cannot access durable session writes",
        ));
    }
    if caller.is_service() {
        caller.require_service_scope(service_scope)
    } else {
        Ok(())
    }
}

/// Scope a service principal must additionally hold to create a run it owns
/// itself, on top of the ordinary `session:write`.
///
/// It is separate from `session:write` on purpose. Every workload that writes
/// sessions already holds `session:write`; owning a run is a strictly larger
/// power (the row is then readable org-wide and mutable only by that workload),
/// so it must be grantable independently in the service-principal registry.
pub const SYSTEM_RUN_OWNER_SCOPE: &str = "session:runs:system-owner";

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
        return Err(Status::failed_precondition(
            "ZDR credentials cannot own durable runs",
        ));
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
        Self::from_config(AuthConfig::from_env()?).await
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
        })
    }

    #[allow(clippy::result_large_err)]
    pub fn intercept(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = extract_bearer(&request)?;
        let identity = self.verify_token(token, self.audience.as_ref())?;
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
        validate_identity(claims)
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
        if delegated.org_id() != caller.org_id()
            || delegated.user_id() != caller.user_id()
            || delegated.zdr() != caller.zdr()
        {
            return Err(Status::permission_denied(
                "delegated Data Plane identity or retention posture does not match caller",
            ));
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
fn validate_identity(claims: Claims) -> Result<VerifiedIdentity, Status> {
    if claims.org_id.trim().is_empty() || claims.org_id.trim() != claims.org_id {
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
            })
        }
        _ => Err(Status::unauthenticated("invalid caller credential")),
    }
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

#[cfg(test)]
mod tests {
    use tonic::Code;
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, LineEnding},
        traits::PublicKeyParts,
    };
    use serde_json::{json, Value};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn rsa_keypair() -> &'static (String, String, String) {
        static KEYS: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
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
                    .delegated_data_plane_bearer(&delegating_request(&sign(&claims, "key-1")), &caller)
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
        for actor in ["kx7Qa2Zb9Lm4", "MAF5Ey3xL8LwigSZ3ngKfGhUy54MlI2Y", "orchestratorcore"] {
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
        let outside_service = VerifiedIdentity::service_for_test_as("org-2", ORCH, &BOTH_SCOPES, false);
        let error = authorize_owner_row(&outside_service, "org-1", ORCH, OwnerIntent::Mutate)
            .expect_err("even the owning workload is org-scoped");
        assert_eq!(error.code(), Code::PermissionDenied);
    }

    /// R8: a look-alike owner gets none of the system treatment — it falls
    /// through to the ordinary rules, where a human must match it exactly.
    #[test]
    fn a_look_alike_owner_gets_no_system_treatment() {
        let human = VerifiedIdentity::user_for_test("org-1", "user-1");
        let error = authorize_owner_row(&human, "org-1", "service:orchestrator-core-2", OwnerIntent::Read)
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
}
