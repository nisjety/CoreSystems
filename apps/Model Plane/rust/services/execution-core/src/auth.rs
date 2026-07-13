//! Auth Core JWT verification for the Execution Core gRPC boundary.
//!
//! Execution Core accepts only a short-lived, user-bound Execution Core bearer
//! as local authority. Credentials for Data Plane, Session Core, and Inference
//! Core are delegated separately, independently verified, identity-bound, and
//! used only for their intended downstream service.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};
use tonic::{Request, Status};
use tracing::{error, warn};

const DEFAULT_JWKS_TTL_SECS: u64 = 300;
const DEFAULT_JWT_LEEWAY_SECS: u64 = 30;
const MAX_JWT_LEEWAY_SECS: u64 = 300;
const MAX_JWKS_BYTES: usize = 1_048_576;

#[derive(Clone)]
pub struct JwtVerifier {
    issuer: Arc<str>,
    audience: Arc<str>,
    jwks_url: Arc<str>,
    jwks_ttl: Duration,
    leeway_secs: u64,
    client: reqwest::Client,
    cache: Arc<RwLock<CachedJwks>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl fmt::Debug for JwtVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JwtVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("jwks_url", &self.jwks_url)
            .field("jwks_ttl", &self.jwks_ttl)
            .field("leeway_secs", &self.leeway_secs)
            .finish_non_exhaustive()
    }
}

struct CachedJwks {
    fetched_at: Instant,
    jwks: JwkSet,
    revision: u64,
    missing_kids: HashSet<String>,
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
    #[serde(default)]
    #[allow(dead_code)]
    nbf: Option<u64>,
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    principal_type: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
}

/// Verified caller identity. The Execution Core ingress credential is never
/// retained or reused as authority at another service boundary.
#[derive(Clone)]
pub struct AuthenticatedUser {
    pub org_id: String,
    pub user_id: String,
}

/// Opaque, independently verified `aud=data-plane` credential. It is bound to
/// the same canonical user and tenant as the Execution Core ingress token and
/// is the only user credential permitted on Data Plane calls.
#[derive(Clone)]
pub struct DelegatedDataPlaneBearer(Arc<str>);

impl DelegatedDataPlaneBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque delegated session-core credential. Execution Core never treats it
/// as local authority; it only forwards it to session-core, which independently
/// verifies `aud=session-core` and pins resource ownership. Debug is redacted.
#[derive(Clone)]
pub struct DelegatedSessionBearer(Arc<str>);

impl DelegatedSessionBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self(Arc::from("session-test-bearer"))
    }
}

/// Opaque, independently verified `aud=inference-core` credential. It is bound
/// to the same canonical user and tenant as the Execution Core ingress token.
#[derive(Clone)]
pub struct DelegatedInferenceBearer(Arc<str>);

impl DelegatedInferenceBearer {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for DelegatedInferenceBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedInferenceBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedDataPlaneBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedDataPlaneBearer([REDACTED])")
    }
}

impl fmt::Debug for DelegatedSessionBearer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DelegatedSessionBearer([REDACTED])")
    }
}

/// Extract the independently delegated Session Core bearer.
///
/// # Errors
/// Returns `unauthenticated` when the metadata is missing or malformed.
#[allow(clippy::result_large_err)]
pub fn delegated_session_bearer<T>(request: &Request<T>) -> Result<DelegatedSessionBearer, Status> {
    request
        .metadata()
        .get("x-session-authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .map(|value| DelegatedSessionBearer(Arc::from(value)))
        .ok_or_else(|| Status::unauthenticated("delegated session credential required"))
}

impl fmt::Debug for AuthenticatedUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedUser")
            .field("org_id", &self.org_id)
            .field("user_id", &self.user_id)
            .finish()
    }
}

impl AuthenticatedUser {
    #[cfg(test)]
    pub(crate) fn for_test(org_id: &str, user_id: &str) -> Self {
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.to_owned(),
        }
    }

    /// Bind tenant and, when supplied by the RPC, user identity to the signed
    /// claims. Empty request identities are rejected rather than treated as a
    /// wildcard.
    ///
    /// # Errors
    /// Returns `permission_denied` when the request attempts to select another
    /// tenant or user and `invalid_argument` when identity is absent.
    #[allow(clippy::result_large_err)]
    pub fn authorize(&self, org_id: &str, user_id: Option<&str>) -> Result<(), Status> {
        if org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if org_id != self.org_id {
            warn!("execution request tenant does not match verified credential");
            return Err(Status::permission_denied("tenant access denied"));
        }
        if let Some(user_id) = user_id {
            if user_id.trim().is_empty() {
                return Err(Status::invalid_argument("user_id is required"));
            }
            if user_id != self.user_id {
                warn!("execution request user does not match verified credential");
                return Err(Status::permission_denied("user access denied"));
            }
        }
        Ok(())
    }
}

pub(crate) struct AuthConfig {
    pub(crate) jwks_url: String,
    pub(crate) issuer: String,
    pub(crate) audience: String,
    pub(crate) jwks_ttl: Duration,
    pub(crate) leeway_secs: u64,
}

impl AuthConfig {
    fn from_env() -> anyhow::Result<Self> {
        let leeway_secs = optional_u64("AUTH_CORE_JWT_LEEWAY_SECS", DEFAULT_JWT_LEEWAY_SECS)
            .min(MAX_JWT_LEEWAY_SECS);
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("EXECUTION_CORE_AUTH_AUDIENCE")?,
            jwks_ttl: Duration::from_secs(optional_u64(
                "AUTH_CORE_JWKS_TTL_SECS",
                DEFAULT_JWKS_TTL_SECS,
            )),
            leeway_secs,
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

fn optional_u64(name: &'static str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

impl JwtVerifier {
    /// Load authentication configuration and fetch the initial JWKS before the
    /// gRPC listener becomes ready. Missing or unavailable verification
    /// material therefore fails startup closed.
    ///
    /// # Errors
    /// Returns an error for missing configuration, an invalid HTTP client, or
    /// an unavailable/malformed JWKS response.
    pub async fn from_env() -> anyhow::Result<Self> {
        Self::from_config(AuthConfig::from_env()?).await
    }

    pub(crate) async fn from_config(config: AuthConfig) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()?;
        let jwks = fetch_jwks(&client, &config.jwks_url).await?;
        Ok(Self {
            issuer: Arc::from(config.issuer),
            audience: Arc::from(config.audience),
            jwks_url: Arc::from(config.jwks_url),
            jwks_ttl: config.jwks_ttl,
            leeway_secs: config.leeway_secs,
            client,
            cache: Arc::new(RwLock::new(CachedJwks {
                fetched_at: Instant::now(),
                jwks,
                revision: 0,
                missing_kids: HashSet::new(),
            })),
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Verify the gRPC bearer cryptographically and return its canonical user
    /// identity. Only RS256 Auth Core user tokens are accepted.
    ///
    /// # Errors
    /// Returns `unauthenticated` for malformed, forged, expired, wrong-issuer,
    /// wrong-audience, or ambiguous credentials. JWKS refresh outages return
    /// `unavailable` and remain fail-closed.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate<T>(&self, request: &Request<T>) -> Result<AuthenticatedUser, Status> {
        let token = extract_bearer(request)?;
        self.verify_user_token(token, &self.audience).await
    }

    /// Verify a separately delegated inference credential and bind it to the
    /// already authenticated Execution Core caller. Missing, malformed, or
    /// wrong-audience credentials fail closed before a run is dispatched.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for an invalid,
    /// unavailable, wrong-audience, or identity-mismatched credential.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_inference<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedInferenceBearer, Status> {
        let token = extract_metadata_bearer(request, "x-inference-authorization")?;
        let delegated = self.verify_user_token(token, "inference-core").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated inference identity does not match caller",
            ));
        }
        Ok(DelegatedInferenceBearer(Arc::from(token)))
    }

    /// Verify a separately delegated Data Plane credential and bind it to the
    /// already authenticated Execution Core caller. The ingress token is never
    /// reused for retrieval, including when its syntax is otherwise valid.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for an invalid,
    /// unavailable, wrong-audience, or identity-mismatched credential.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate_delegated_data_plane<T>(
        &self,
        request: &Request<T>,
        caller: &AuthenticatedUser,
    ) -> Result<DelegatedDataPlaneBearer, Status> {
        let token = extract_metadata_bearer(request, "x-data-plane-authorization")?;
        let delegated = self.verify_user_token(token, "data-plane").await?;
        if delegated.org_id != caller.org_id || delegated.user_id != caller.user_id {
            return Err(Status::permission_denied(
                "delegated Data Plane identity does not match caller",
            ));
        }
        Ok(DelegatedDataPlaneBearer(Arc::from(token)))
    }

    async fn verify_user_token(
        &self,
        token: &str,
        audience: &str,
    ) -> Result<AuthenticatedUser, Status> {
        let header =
            decode_header(token).map_err(|_| Status::unauthenticated("invalid user credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid user credential"));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.trim().is_empty())
            .ok_or_else(|| Status::unauthenticated("invalid user credential"))?;
        let jwk = self.jwk_for(&kid).await?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|error| {
            error!(%error, "Auth Core JWKS contains an unusable signing key");
            Status::unavailable("authentication verification unavailable")
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[audience]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| Status::unauthenticated("invalid user credential"))?
            .claims;
        validate_user_claims(claims)
    }

    async fn jwk_for(&self, kid: &str) -> Result<jsonwebtoken::jwk::Jwk, Status> {
        let observed_revision = {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid user credential"));
                }
            }
            cache.revision
        };

        // Coalesce refreshes. Re-check after acquiring the lock so another
        // request's successful refresh is reused.
        let _refresh = self.refresh_lock.lock().await;
        {
            let cache = self.cache.read().await;
            if cache.revision != observed_revision {
                return cache
                    .jwks
                    .find(kid)
                    .cloned()
                    .ok_or_else(|| Status::unauthenticated("invalid user credential"));
            }
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid user credential"));
                }
            }
        }
        let jwks = fetch_jwks(&self.client, &self.jwks_url)
            .await
            .map_err(|error| {
                error!(%error, "failed to refresh Auth Core JWKS");
                Status::unavailable("authentication verification unavailable")
            })?;
        let jwk = jwks.find(kid).cloned();
        let mut cache = self.cache.write().await;
        let revision = cache.revision.saturating_add(1);
        let missing_kids = if jwk.is_none() {
            HashSet::from([kid.to_owned()])
        } else {
            HashSet::new()
        };
        *cache = CachedJwks {
            fetched_at: Instant::now(),
            jwks,
            revision,
            missing_kids,
        };
        jwk.ok_or_else(|| Status::unauthenticated("invalid user credential"))
    }
}

#[allow(clippy::result_large_err)]
fn extract_bearer<T>(request: &Request<T>) -> Result<&str, Status> {
    extract_metadata_bearer(request, "authorization")
}

#[allow(clippy::result_large_err)]
fn extract_metadata_bearer<'a, T>(
    request: &'a Request<T>,
    header: &'static str,
) -> Result<&'a str, Status> {
    request
        .metadata()
        .get(header)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified delegated credential required"))
}

#[allow(clippy::result_large_err)]
fn validate_user_claims(claims: Claims) -> Result<AuthenticatedUser, Status> {
    let is_user = claims.principal_type.as_deref().unwrap_or("user") == "user";
    if !is_user
        || claims.sub.trim().is_empty()
        || claims.user_id.trim().is_empty()
        || claims.org_id.trim().is_empty()
        || claims.sub != claims.user_id
        || claims.user_id.trim() != claims.user_id
        || claims.org_id.trim() != claims.org_id
        || claims.service_id.is_some()
    {
        return Err(Status::unauthenticated("invalid user credential"));
    }
    Ok(AuthenticatedUser {
        org_id: claims.org_id,
        user_id: claims.user_id,
    })
}

async fn fetch_jwks(client: &reqwest::Client, url: &str) -> anyhow::Result<JwkSet> {
    let response = client.get(url).send().await?.error_for_status()?;
    let body = response.bytes().await?;
    if body.len() > MAX_JWKS_BYTES {
        anyhow::bail!("Auth Core JWKS response exceeds size limit");
    }
    validate_jwks_document(&body)?;
    let jwks: JwkSet = serde_json::from_slice(&body)?;
    if jwks.keys.is_empty() {
        anyhow::bail!("Auth Core JWKS contains no signing keys");
    }
    Ok(jwks)
}

#[derive(Deserialize)]
struct RawJwkSet {
    keys: Vec<RawJwk>,
}

#[derive(Deserialize)]
struct RawJwk {
    kty: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    alg: Option<String>,
    kid: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

fn validate_jwks_document(body: &[u8]) -> anyhow::Result<()> {
    use std::collections::HashSet;

    let raw: RawJwkSet = serde_json::from_slice(body)?;
    let mut usable = 0_usize;
    let mut kids = HashSet::new();
    for key in raw.keys {
        if key.kty.as_deref() != Some("RSA") || key.alg.as_deref() != Some("RS256") {
            continue;
        }
        if key.key_use.as_deref().is_some_and(|value| value != "sig") {
            anyhow::bail!("Auth Core JWKS RSA key is not a signing key");
        }
        let kid = key
            .kid
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing kid"))?;
        if !kids.insert(kid) {
            anyhow::bail!("Auth Core JWKS contains a duplicate key id");
        }
        let modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
            key.n
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing modulus"))?,
        )?;
        if rsa_modulus_bits(&modulus) < 2048 {
            anyhow::bail!("Auth Core JWKS RSA key is smaller than 2048 bits");
        }
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(
            key.e
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Auth Core JWKS RSA key is missing exponent"))?,
        )?;
        if exponent.iter().all(|byte| *byte == 0) {
            anyhow::bail!("Auth Core JWKS RSA exponent is invalid");
        }
        usable = usable.saturating_add(1);
    }
    if usable == 0 {
        anyhow::bail!("Auth Core JWKS contains no usable RS256 signing keys");
    }
    Ok(())
}

fn rsa_modulus_bits(modulus: &[u8]) -> usize {
    let Some((index, first)) = modulus.iter().enumerate().find(|(_, byte)| **byte != 0) else {
        return 0;
    };
    (modulus.len() - index - 1) * 8 + (8 - first.leading_zeros() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    use rsa::traits::PublicKeyParts;
    use serde_json::{json, Value};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn rsa_keypair() -> &'static (String, String, String) {
        static KEYS: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
        KEYS.get_or_init(|| {
            let mut rng = rand::thread_rng();
            let private = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
            let public = rsa::RsaPublicKey::from(&private);
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private PEM")
                .to_string();
            let n =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.n().to_bytes_be());
            let e =
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public.e().to_bytes_be());
            (private_pem, n, e)
        })
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_secs()
    }

    fn claims() -> Value {
        json!({
            "sub": "user-1",
            "iss": "auth-core",
            "aud": "execution-core",
            "exp": now() + 3600,
            "nbf": now() - 5,
            "org_id": "org-1",
            "user_id": "user-1",
            "principal_type": "user"
        })
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

    async fn verifier(kid: &str) -> JwtVerifier {
        let server = MockServer::start().await;
        let (_, n, e) = rsa_keypair();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": kid, "n": n, "e": e
                }]
            })))
            .mount(&server)
            .await;
        JwtVerifier::from_config(AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: "execution-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    fn authenticated_request(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    #[test]
    fn delegated_session_credential_is_required_and_redacted() {
        assert_eq!(
            delegated_session_bearer(&Request::new(()))
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-session-authorization",
            "Bearer dedicated-session-token".parse().expect("metadata"),
        );
        let bearer = delegated_session_bearer(&request).expect("delegated bearer");
        assert_eq!(bearer.as_str(), "dedicated-session-token");
        assert_eq!(format!("{bearer:?}"), "DelegatedSessionBearer([REDACTED])");
    }

    #[tokio::test]
    async fn delegated_inference_credential_requires_exact_audience_and_identity() {
        let verifier = verifier("key-inference").await;
        let execution_token = sign(&claims(), "key-inference");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let mut inference_claims = claims();
        inference_claims["aud"] = json!("inference-core");
        let inference_token = sign(&inference_claims, "key-inference");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {inference_token}")
                .parse()
                .expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_inference(&request, &caller)
            .await
            .expect("delegated inference bearer");
        assert_eq!(delegated.as_str(), inference_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedInferenceBearer([REDACTED])"
        );

        let wrong_audience = sign(&claims(), "key-inference");
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {wrong_audience}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_inference(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = inference_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-inference");
        request.metadata_mut().insert(
            "x-inference-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_inference(&request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn data_plane_credential_is_separate_identity_bound_and_never_ingress_authority() {
        let verifier = verifier("key-data-plane").await;
        let execution_token = sign(&claims(), "key-data-plane");
        let caller = verifier
            .authenticate(&authenticated_request(&execution_token))
            .await
            .expect("execution caller");

        let missing = Request::new(());
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&missing, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut data_claims = claims();
        data_claims["aud"] = json!("data-plane");
        let data_token = sign(&data_claims, "key-data-plane");
        let mut delegated_request = Request::new(());
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {data_token}").parse().expect("metadata"),
        );
        let delegated = verifier
            .authenticate_delegated_data_plane(&delegated_request, &caller)
            .await
            .expect("delegated Data Plane bearer");
        assert_eq!(delegated.as_str(), data_token);
        assert_eq!(
            format!("{delegated:?}"),
            "DelegatedDataPlaneBearer([REDACTED])"
        );

        // An aud=data-plane token must never become Execution Core authority.
        assert_eq!(
            verifier
                .authenticate(&authenticated_request(&data_token))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        // An Execution Core token in the delegated header is the wrong audience.
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {execution_token}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&delegated_request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_identity = data_claims;
        wrong_identity["sub"] = json!("user-2");
        wrong_identity["user_id"] = json!("user-2");
        let wrong_identity = sign(&wrong_identity, "key-data-plane");
        delegated_request.metadata_mut().insert(
            "x-data-plane-authorization",
            format!("Bearer {wrong_identity}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier
                .authenticate_delegated_data_plane(&delegated_request, &caller)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn verifies_rs256_signature_and_canonical_user_identity() {
        let verifier = verifier("key-1").await;
        let token = sign(&claims(), "key-1");
        let user = verifier
            .authenticate(&authenticated_request(&token))
            .await
            .expect("verified user");

        assert_eq!(user.org_id, "org-1");
        assert_eq!(user.user_id, "user-1");
        assert!(!format!("{user:?}").contains(&token));
    }

    #[tokio::test]
    async fn rejects_forged_wrong_standard_claims_and_service_identity() {
        let verifier = verifier("key-2").await;
        assert_eq!(
            verifier
                .authenticate(&Request::new(()))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        let unsigned = authenticated_request("attacker-controlled");
        assert_eq!(
            verifier.authenticate(&unsigned).await.unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        for (field, value) in [
            ("iss", json!("attacker")),
            ("aud", json!("model-gateway")),
            ("exp", json!(now() - 10)),
            ("nbf", json!(now() + 300)),
            ("org_id", json!("")),
            ("user_id", json!("user-2")),
            ("principal_type", json!("service")),
        ] {
            let mut invalid = claims();
            invalid[field] = value;
            let token = sign(&invalid, "key-2");
            assert_eq!(
                verifier
                    .authenticate(&authenticated_request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "field {field} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn binds_request_tenant_and_user_to_signed_claims() {
        let verifier = verifier("key-3").await;
        let token = sign(&claims(), "key-3");
        let user = verifier
            .authenticate(&authenticated_request(&token))
            .await
            .expect("verified user");

        assert!(user.authorize("org-1", Some("user-1")).is_ok());
        assert_eq!(
            user.authorize("org-2", Some("user-1")).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            user.authorize("org-1", Some("user-2")).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            user.authorize("", Some("user-1")).unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn rejects_undersized_and_malformed_rsa_jwks_keys() {
        let weak_modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff; 128]);
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1, 0, 1]);
        let weak = serde_json::to_vec(&json!({
            "keys": [{
                "kty": "RSA", "use": "sig", "alg": "RS256", "kid": "weak",
                "n": weak_modulus, "e": exponent
            }]
        }))
        .expect("JWKS JSON");
        assert!(validate_jwks_document(&weak)
            .expect_err("1024-bit key must fail")
            .to_string()
            .contains("smaller than 2048 bits"));

        let malformed =
            br#"{"keys":[{"kty":"RSA","alg":"RS256","kid":"bad","n":"!!!","e":"AQAB"}]}"#;
        assert!(validate_jwks_document(malformed).is_err());
    }

    #[tokio::test]
    async fn concurrent_unknown_kid_requests_share_one_refresh() {
        let server = MockServer::start().await;
        let (_, n, e) = rsa_keypair();
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "keys": [{
                    "kty": "RSA", "use": "sig", "alg": "RS256",
                    "kid": "known", "n": n, "e": e
                }]
            })))
            .expect(2)
            .mount(&server)
            .await;
        let verifier = JwtVerifier::from_config(AuthConfig {
            jwks_url: format!("{}/jwks", server.uri()),
            issuer: "auth-core".to_owned(),
            audience: "data-plane".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier");
        let token = sign(&claims(), "unknown");

        let results = futures::future::join_all((0..12).map(|_| {
            let verifier = verifier.clone();
            let request = authenticated_request(&token);
            async move { verifier.authenticate(&request).await }
        }))
        .await;

        assert!(results.into_iter().all(|result| {
            result.expect_err("unknown key must fail").code() == tonic::Code::Unauthenticated
        }));
    }
}
