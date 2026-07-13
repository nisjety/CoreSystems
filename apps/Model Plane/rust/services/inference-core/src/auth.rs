//! Fail-closed Auth Core JWT verification for the Inference Core gRPC boundary.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Verifies independently signed user and workload credentials before a
/// request can reach a provider or the prompt cache.
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
    exp: u64,
    iat: u64,
    nbf: u64,
    org_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    principal_type: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    zdr: bool,
}

/// Canonical identity derived only from a verified credential. The original
/// bearer is retained for a future scoped downstream delegation path and is
/// deliberately redacted from `Debug`.
#[derive(Clone)]
pub struct AuthenticatedPrincipal {
    pub org_id: String,
    pub user_id: Option<String>,
    pub subject: String,
    pub scopes: Vec<String>,
    issuer_zdr: bool,
    bearer: Arc<str>,
}

impl fmt::Debug for AuthenticatedPrincipal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthenticatedPrincipal")
            .field("org_id", &self.org_id)
            .field("user_id", &self.user_id)
            .field("subject", &self.subject)
            .field("scopes", &self.scopes)
            .field("issuer_zdr", &self.issuer_zdr)
            .field("bearer", &"[REDACTED]")
            .finish()
    }
}

impl AuthenticatedPrincipal {
    #[cfg(test)]
    pub(crate) fn for_test(org_id: &str, user_id: Option<&str>, issuer_zdr: bool) -> Self {
        let subject = user_id.unwrap_or("service:test");
        Self {
            org_id: org_id.to_owned(),
            user_id: user_id.map(str::to_owned),
            subject: subject.to_owned(),
            scopes: vec!["inference:invoke".to_owned()],
            issuer_zdr,
            bearer: Arc::from("test-bearer"),
        }
    }

    /// Bind a request tenant to the signed tenant. Empty tenant ids are not a
    /// wildcard because every provider operation is tenant-sensitive.
    ///
    /// # Errors
    /// Returns `invalid_argument` for an absent tenant and
    /// `permission_denied` for a cross-tenant request.
    #[allow(clippy::result_large_err)]
    pub fn authorize_org(&self, requested_org_id: &str) -> Result<(), Status> {
        if requested_org_id.trim().is_empty() {
            return Err(Status::invalid_argument("org_id is required"));
        }
        if requested_org_id != self.org_id {
            warn!("inference request tenant does not match verified credential");
            return Err(Status::permission_denied("tenant access denied"));
        }
        Ok(())
    }

    /// Require provider-execution authority.
    ///
    /// # Errors
    /// Returns `permission_denied` when the verified credential lacks the
    /// exact `inference:invoke` scope.
    #[allow(clippy::result_large_err)]
    pub fn authorize_invoke(&self) -> Result<(), Status> {
        if self.scopes.iter().any(|scope| scope == "inference:invoke") {
            return Ok(());
        }
        warn!("verified principal lacks inference invocation scope");
        Err(Status::permission_denied("inference scope required"))
    }

    /// Permit catalogue reads to a dedicated read credential or to a caller
    /// already authorised to invoke a provider.
    ///
    /// # Errors
    /// Returns `permission_denied` when neither exact scope is present.
    #[allow(clippy::result_large_err)]
    pub fn authorize_catalog(&self) -> Result<(), Status> {
        if self
            .scopes
            .iter()
            .any(|scope| matches!(scope.as_str(), "inference:read" | "inference:invoke"))
        {
            return Ok(());
        }
        warn!("verified principal lacks inference catalogue scope");
        Err(Status::permission_denied("inference scope required"))
    }

    /// Require the dedicated routing-policy administration authority. Provider
    /// invocation does not imply control-plane mutation rights.
    ///
    /// # Errors
    /// Returns `permission_denied` unless the verified credential contains the
    /// exact `inference:policy:admin` scope.
    #[allow(clippy::result_large_err)]
    pub fn authorize_policy_admin(&self) -> Result<(), Status> {
        if self
            .scopes
            .iter()
            .any(|scope| scope == "inference:policy:admin")
        {
            return Ok(());
        }
        warn!("verified principal lacks inference routing-policy admin scope");
        Err(Status::permission_denied(
            "inference policy administration scope required",
        ))
    }

    /// Issuer-selected ZDR can only make request policy stricter.
    #[must_use]
    pub const fn effective_zdr(&self, request_zdr: bool) -> bool {
        self.issuer_zdr || request_zdr
    }

    #[must_use]
    pub fn budget_user_id(&self) -> String {
        self.user_id.clone().unwrap_or_default()
    }

    #[must_use]
    pub fn bearer(&self) -> &str {
        &self.bearer
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
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("INFERENCE_CORE_AUTH_AUDIENCE")?,
            jwks_ttl: Duration::from_secs(optional_u64(
                "AUTH_CORE_JWKS_TTL_SECS",
                DEFAULT_JWKS_TTL_SECS,
            )),
            leeway_secs: optional_u64("AUTH_CORE_JWT_LEEWAY_SECS", DEFAULT_JWT_LEEWAY_SECS)
                .min(MAX_JWT_LEEWAY_SECS),
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
    /// Load auth configuration and eagerly validate the JWKS document. The
    /// listener is never bound when verification material is missing or bad.
    ///
    /// # Errors
    /// Returns an error for missing configuration, HTTP client construction,
    /// or an unavailable/malformed JWKS document.
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

    /// Cryptographically verify a gRPC bearer and return its canonical user or
    /// service identity. Only RS256 Auth Core tokens are accepted.
    ///
    /// # Errors
    /// Invalid credentials return `unauthenticated`; a JWKS refresh outage
    /// returns `unavailable` and remains fail closed.
    #[allow(clippy::result_large_err)]
    pub async fn authenticate<T>(
        &self,
        request: &Request<T>,
    ) -> Result<AuthenticatedPrincipal, Status> {
        let token = extract_bearer(request)?;
        let header =
            decode_header(token).map_err(|_| Status::unauthenticated("invalid credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid credential"));
        }
        let kid = header
            .kid
            .filter(|kid| !kid.trim().is_empty())
            .ok_or_else(|| Status::unauthenticated("invalid credential"))?;
        let jwk = self.jwk_for(&kid).await?;
        let key = DecodingKey::from_jwk(&jwk).map_err(|error| {
            error!(%error, "Auth Core JWKS contains an unusable signing key");
            Status::unavailable("authentication verification unavailable")
        })?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iat", "nbf", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_audience(&[self.audience.as_ref()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| Status::unauthenticated("invalid credential"))?
            .claims;
        validate_claims(claims, token, self.leeway_secs)
    }

    async fn jwk_for(&self, kid: &str) -> Result<jsonwebtoken::jwk::Jwk, Status> {
        let observed_revision = {
            let cache = self.cache.read().await;
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid credential"));
                }
            }
            cache.revision
        };

        let _refresh = self.refresh_lock.lock().await;
        {
            let cache = self.cache.read().await;
            if cache.revision != observed_revision {
                return cache
                    .jwks
                    .find(kid)
                    .cloned()
                    .ok_or_else(|| Status::unauthenticated("invalid credential"));
            }
            if cache.fetched_at.elapsed() < self.jwks_ttl {
                if let Some(jwk) = cache.jwks.find(kid) {
                    return Ok(jwk.clone());
                }
                if cache.missing_kids.contains(kid) {
                    return Err(Status::unauthenticated("invalid credential"));
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
        jwk.ok_or_else(|| Status::unauthenticated("invalid credential"))
    }
}

#[allow(clippy::result_large_err)]
fn extract_bearer<T>(request: &Request<T>) -> Result<&str, Status> {
    request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
        .ok_or_else(|| Status::unauthenticated("verified credential required"))
}

#[allow(clippy::result_large_err)]
fn validate_claims(
    claims: Claims,
    token: &str,
    leeway_secs: u64,
) -> Result<AuthenticatedPrincipal, Status> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::unavailable("authentication clock unavailable"))?
        .as_secs();
    if claims.org_id.trim().is_empty()
        || claims.org_id.trim() != claims.org_id
        || claims.sub.trim().is_empty()
        || claims.sub.trim() != claims.sub
        || claims.iat > now.saturating_add(leeway_secs)
        || claims.nbf > claims.exp
        || claims.iat > claims.exp
        || claims
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || scope.trim() != scope || scope.len() > 200)
    {
        return Err(Status::unauthenticated("invalid credential"));
    }

    let user_id = match claims.principal_type.as_deref().unwrap_or("user") {
        "user"
            if !claims.user_id.is_empty()
                && claims.user_id.trim() == claims.user_id
                && claims.sub == claims.user_id
                && claims.service_id.is_none()
                && claims.reason.is_none() =>
        {
            Some(claims.user_id)
        }
        "service"
            if claims.user_id.is_empty()
                && claims.service_id.as_deref() == Some(claims.sub.as_str())
                && claims
                    .service_id
                    .as_deref()
                    .and_then(|id| id.strip_prefix("service:"))
                    .is_some_and(|id| !id.is_empty())
                && claims.reason.as_deref().is_some_and(|reason| {
                    reason == reason.trim() && (3..=500).contains(&reason.len())
                }) =>
        {
            None
        }
        _ => return Err(Status::unauthenticated("invalid credential")),
    };

    Ok(AuthenticatedPrincipal {
        org_id: claims.org_id,
        user_id,
        subject: claims.sub,
        scopes: claims.scopes,
        issuer_zdr: claims.zdr,
        bearer: Arc::from(token),
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
    use jsonwebtoken::{encode, EncodingKey, Header};
    use rand::thread_rng;
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
            let private = rsa::RsaPrivateKey::new(&mut thread_rng(), 2048).expect("RSA key");
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
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_secs()
    }

    fn user_claims() -> Value {
        json!({
            "sub": "user-1", "iss": "auth-core", "aud": "inference-core",
            "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
            "org_id": "org-1", "user_id": "user-1", "principal_type": "user",
            "scopes": ["inference:invoke"], "zdr": true
        })
    }

    fn service_claims() -> Value {
        json!({
            "sub": "service:retrieval", "iss": "auth-core", "aud": "inference-core",
            "iat": now(), "nbf": now() - 5, "exp": now() + 3600,
            "org_id": "org-1", "principal_type": "service",
            "service_id": "service:retrieval", "reason": "query embedding",
            "scopes": ["inference:invoke"], "zdr": true
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
            audience: "inference-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    fn request(token: &str) -> Request<()> {
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().expect("metadata"),
        );
        request
    }

    #[tokio::test]
    async fn verifies_users_services_and_redacts_bearers() {
        let verifier = verifier("key-1").await;
        let user_token = sign(&user_claims(), "key-1");
        let user = verifier
            .authenticate(&request(&user_token))
            .await
            .expect("user");
        assert_eq!(user.org_id, "org-1");
        assert_eq!(user.user_id.as_deref(), Some("user-1"));
        assert_eq!(user.budget_user_id(), "user-1");
        assert_eq!(user.bearer(), user_token);
        assert!(user.effective_zdr(false));
        assert!(!format!("{user:?}").contains(&user_token));

        let service_token = sign(&service_claims(), "key-1");
        let service = verifier
            .authenticate(&request(&service_token))
            .await
            .expect("service");
        assert_eq!(service.user_id, None);
        assert_eq!(service.subject, "service:retrieval");
        assert_eq!(service.budget_user_id(), "");
    }

    #[tokio::test]
    async fn rejects_missing_forged_wrong_standard_and_ambiguous_claims() {
        let verifier = verifier("key-2").await;
        assert_eq!(
            verifier
                .authenticate(&Request::new(()))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        assert_eq!(
            verifier
                .authenticate(&request("attacker-controlled"))
                .await
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );
        for (field, value) in [
            ("iss", json!("attacker")),
            ("aud", json!("model-gateway")),
            ("exp", json!(now() - 10)),
            ("iat", json!(now() + 300)),
            ("nbf", json!(now() + 300)),
            ("org_id", json!("")),
            ("user_id", json!("user-2")),
            ("principal_type", json!("service")),
        ] {
            let mut invalid = user_claims();
            invalid[field] = value;
            let token = sign(&invalid, "key-2");
            assert_eq!(
                verifier
                    .authenticate(&request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated,
                "field {field} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn pins_tenant_and_monotonically_enforces_zdr() {
        let verifier = verifier("key-3").await;
        let token = sign(&user_claims(), "key-3");
        let principal = verifier
            .authenticate(&request(&token))
            .await
            .expect("principal");
        assert!(principal.authorize_org("org-1").is_ok());
        assert_eq!(
            principal.authorize_org("org-2").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            principal.authorize_org("").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert!(principal.effective_zdr(false));
        assert!(principal.effective_zdr(true));
    }

    #[tokio::test]
    async fn enforces_invoke_and_catalog_scopes_with_least_privilege() {
        let verifier = verifier("key-scopes").await;

        let invoke = verifier
            .authenticate(&request(&sign(&user_claims(), "key-scopes")))
            .await
            .expect("invoke principal");
        assert!(invoke.authorize_invoke().is_ok());
        assert!(invoke.authorize_catalog().is_ok());

        let mut read_claims = user_claims();
        read_claims["scopes"] = json!(["inference:read"]);
        let read = verifier
            .authenticate(&request(&sign(&read_claims, "key-scopes")))
            .await
            .expect("read principal");
        assert!(read.authorize_catalog().is_ok());
        assert_eq!(
            read.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        let mut wrong_claims = user_claims();
        wrong_claims["scopes"] = json!(["cost:read"]);
        let wrong = verifier
            .authenticate(&request(&sign(&wrong_claims, "key-scopes")))
            .await
            .expect("wrong-scope principal remains authenticated");
        assert_eq!(
            wrong.authorize_catalog().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            wrong.authorize_invoke().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn routing_policy_admin_requires_its_own_exact_scope() {
        let verifier = verifier("key-policy-admin").await;

        let invoke = verifier
            .authenticate(&request(&sign(&user_claims(), "key-policy-admin")))
            .await
            .expect("invoke principal");
        assert_eq!(
            invoke.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        let mut admin_claims = user_claims();
        admin_claims["scopes"] = json!(["inference:policy:admin"]);
        let admin = verifier
            .authenticate(&request(&sign(&admin_claims, "key-policy-admin")))
            .await
            .expect("policy admin principal");
        assert!(admin.authorize_policy_admin().is_ok());

        let mut prefix_claims = user_claims();
        prefix_claims["scopes"] = json!(["inference:policy:admin:extra"]);
        let prefix = verifier
            .authenticate(&request(&sign(&prefix_claims, "key-policy-admin")))
            .await
            .expect("prefix principal");
        assert_eq!(
            prefix.authorize_policy_admin().unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn unknown_key_ids_are_refreshed_once_then_rejected_from_negative_cache() {
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
            audience: "inference-core".to_owned(),
            jwks_ttl: Duration::from_secs(300),
            leeway_secs: 0,
        })
        .await
        .expect("verifier");
        let token = sign(&user_claims(), "unknown");

        for _ in 0..2 {
            assert_eq!(
                verifier
                    .authenticate(&request(&token))
                    .await
                    .unwrap_err()
                    .code(),
                tonic::Code::Unauthenticated
            );
        }
    }

    #[test]
    fn rejects_weak_and_malformed_rsa_jwks_keys() {
        let weak_modulus = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff; 128]);
        let exponent = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([1, 0, 1]);
        let weak = serde_json::to_vec(&json!({
            "keys": [{"kty": "RSA", "use": "sig", "alg": "RS256", "kid": "weak",
                "n": weak_modulus, "e": exponent}]
        }))
        .expect("JWKS");
        assert!(validate_jwks_document(&weak)
            .expect_err("weak key")
            .to_string()
            .contains("smaller than 2048 bits"));
        assert!(validate_jwks_document(
            br#"{"keys":[{"kty":"RSA","alg":"RS256","kid":"bad","n":"!!!","e":"AQAB"}]}"#
        )
        .is_err());
    }
}
