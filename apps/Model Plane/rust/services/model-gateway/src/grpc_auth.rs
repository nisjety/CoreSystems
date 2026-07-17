//! Fail-closed Auth Core verification for model-gateway's gRPC boundary.
//!
//! Verification material is fetched and validated before the listener binds.
//! The synchronous tonic interceptor verifies RS256 credentials and installs a
//! canonical tenant/principal identity. RPC handlers still authorize the
//! tenant fields in their decoded protobuf body before any side effect.

use std::{
    collections::HashMap,
    fmt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::Engine as _;
use jsonwebtoken::{decode, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tonic::{Request, Status};

const MAX_JWKS_BYTES: usize = 1_048_576;
const DEFAULT_LEEWAY_SECS: u64 = 30;

#[derive(Clone)]
pub struct JwtVerifier {
    issuer: Arc<str>,
    audience: Arc<str>,
    leeway_secs: u64,
    keys: Arc<HashMap<String, DecodingKey>>,
}

impl fmt::Debug for JwtVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JwtVerifier")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
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
    issuer_zdr: bool,
    session_bearer: Option<Arc<str>>,
    inference_bearer: Option<Arc<str>>,
    execution_bearer: Option<Arc<str>>,
}

impl fmt::Debug for VerifiedIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedIdentity")
            .field("org_id", &self.org_id)
            .field("principal_id", &self.principal_id)
            .field("kind", &self.kind)
            .field("scopes", &self.scopes)
            .field("issuer_zdr", &self.issuer_zdr)
            .field(
                "session_bearer",
                &self.session_bearer.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "inference_bearer",
                &self.inference_bearer.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "execution_bearer",
                &self.execution_bearer.as_ref().map(|_| "[REDACTED]"),
            )
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

    #[allow(clippy::result_large_err)]
    /// Authorize an organization-bound operation.
    ///
    /// # Errors
    /// Returns an invalid-argument or permission-denied status on mismatch.
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
    /// Authorize a user-bound operation.
    ///
    /// # Errors
    /// Returns an invalid-argument or permission-denied status on mismatch.
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

    #[must_use]
    pub const fn effective_zdr(&self, request_zdr: bool) -> bool {
        self.issuer_zdr || request_zdr
    }

    /// Build a session-core request only when an independently verified,
    /// identity-bound `aud=session-core` bearer accompanied the ingress call.
    ///
    /// # Errors
    /// Returns `unauthenticated` when no verified Session Core credential is
    /// available or the credential cannot be encoded as metadata.
    #[allow(clippy::result_large_err)]
    pub fn session_request<T>(&self, value: T) -> Result<Request<T>, Status> {
        authenticated_downstream_request(value, self.session_bearer.as_deref(), "session-core")
    }

    #[allow(clippy::result_large_err)]
    pub(crate) fn session_bearer(&self) -> Result<&str, Status> {
        self.session_bearer
            .as_deref()
            .ok_or_else(|| Status::unauthenticated("verified session-core credential required"))
    }

    /// Build an inference-core request only when an independently verified,
    /// identity-bound `aud=inference-core` bearer accompanied the ingress call.
    ///
    /// # Errors
    /// Returns `unauthenticated` when no verified Inference Core credential is
    /// available or the credential cannot be encoded as metadata.
    #[allow(clippy::result_large_err)]
    pub fn inference_request<T>(&self, value: T) -> Result<Request<T>, Status> {
        authenticated_downstream_request(value, self.inference_bearer.as_deref(), "inference-core")
    }

    /// Build an execution-core request with independently verified ingress and
    /// Session Core delegation credentials bound to this same identity.
    ///
    /// # Errors
    /// Returns `unauthenticated` when either exact-audience credential is
    /// missing, or `internal` when verified credentials cannot be encoded as
    /// gRPC metadata.
    #[allow(clippy::result_large_err)]
    pub fn execution_request_with_session<T>(&self, value: T) -> Result<Request<T>, Status> {
        let mut request = authenticated_downstream_request(
            value,
            self.execution_bearer.as_deref(),
            "execution-core",
        )?;
        let session_bearer = self
            .session_bearer
            .as_deref()
            .ok_or_else(|| Status::unauthenticated("verified session-core credential required"))?;
        request.metadata_mut().insert(
            "x-session-authorization",
            format!("Bearer {session_bearer}")
                .parse()
                .map_err(|_| Status::internal("verified credential is not forwardable"))?,
        );
        Ok(request)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn user_for_test(org_id: &str, user_id: &str, issuer_zdr: bool) -> Self {
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from(user_id),
            user_id: Some(Arc::from(user_id)),
            scopes: Arc::from([]),
            kind: PrincipalKind::User,
            issuer_zdr,
            session_bearer: None,
            inference_bearer: None,
            execution_bearer: None,
        }
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn service_for_test(
        org_id: &str,
        service_id: &str,
        scopes: &[&str],
        issuer_zdr: bool,
    ) -> Self {
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from(service_id),
            user_id: None,
            scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
            kind: PrincipalKind::Service,
            issuer_zdr,
            session_bearer: None,
            inference_bearer: None,
            execution_bearer: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn user_with_downstream_for_test(org_id: &str, user_id: &str) -> Self {
        let mut identity = Self::user_for_test(org_id, user_id, false);
        identity.session_bearer = Some(Arc::from("test-session-bearer"));
        identity.inference_bearer = Some(Arc::from("test-inference-bearer"));
        identity.execution_bearer = Some(Arc::from("test-execution-bearer"));
        identity
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RpcAccess {
    Invoke,
    Read,
    Write,
    Tool,
    Approval,
}

impl RpcAccess {
    const fn service_scope(self) -> &'static str {
        match self {
            Self::Invoke => "models:invoke",
            Self::Read => "gateway:read",
            Self::Write => "gateway:write",
            Self::Tool => "tools:execute",
            Self::Approval => "approvals:decide",
        }
    }
}

/// Retrieve and authorize the identity installed by the mandatory interceptor.
///
/// # Errors
/// Returns an authentication, authorization, or ZDR policy status when the
/// caller cannot perform the requested operation.
#[allow(clippy::result_large_err)]
pub fn authorize_request<T>(
    request: &Request<T>,
    org_id: &str,
    access: RpcAccess,
) -> Result<VerifiedIdentity, Status> {
    let identity = request
        .extensions()
        .get::<VerifiedIdentity>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("verified caller identity required"))?;
    identity.authorize_org(org_id)?;
    if identity.effective_zdr(false) && matches!(access, RpcAccess::Write | RpcAccess::Approval) {
        return Err(Status::failed_precondition(
            "durable mutation is disabled by zero-retention policy",
        ));
    }
    if identity.is_service() && !identity.has_scope(access.service_scope()) {
        return Err(Status::permission_denied("service scope required"));
    }
    Ok(identity)
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
    reason: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    zdr: bool,
}

pub(crate) struct AuthConfig {
    pub jwks_url: String,
    pub issuer: String,
    pub audience: String,
    pub leeway_secs: u64,
}

impl AuthConfig {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("AUTH_CORE_AUDIENCE")?,
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
    ///
    /// # Errors
    /// Returns an error for missing configuration or unavailable, oversized,
    /// malformed, or cryptographically unusable JWKS material.
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
            leeway_secs: config.leeway_secs,
            keys: Arc::new(keys),
        })
    }

    /// Verify and enrich an inbound gRPC request.
    ///
    /// # Errors
    /// Returns an authentication or authorization status for an invalid
    /// ingress or delegated credential.
    #[allow(clippy::result_large_err)]
    pub fn intercept(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = extract_bearer(&request)?;
        let mut identity = self.decode_identity(token, &self.audience)?;
        identity.session_bearer = self.verify_delegated_bearer(
            &request,
            "x-session-authorization",
            "session-core",
            &identity,
        )?;
        identity.inference_bearer = self.verify_delegated_bearer(
            &request,
            "x-inference-authorization",
            "inference-core",
            &identity,
        )?;
        identity.execution_bearer = self.verify_delegated_bearer(
            &request,
            "x-execution-authorization",
            "execution-core",
            &identity,
        )?;
        request.extensions_mut().insert(identity);
        Ok(request)
    }

    #[allow(clippy::result_large_err)]
    fn decode_identity(&self, token: &str, audience: &str) -> Result<VerifiedIdentity, Status> {
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
        validate_identity(claims, self.leeway_secs)
    }

    #[allow(clippy::result_large_err)]
    fn verify_delegated_bearer(
        &self,
        request: &Request<()>,
        header: &'static str,
        audience: &'static str,
        ingress: &VerifiedIdentity,
    ) -> Result<Option<Arc<str>>, Status> {
        let Some(raw) = request
            .metadata()
            .get(header)
            .and_then(|value| value.to_str().ok())
        else {
            return Ok(None);
        };
        let token = raw
            .strip_prefix("Bearer ")
            .filter(|value| !value.is_empty() && !value.chars().any(char::is_whitespace))
            .ok_or_else(|| Status::unauthenticated("invalid delegated credential"))?;
        let delegated = self.decode_identity(token, audience)?;
        if delegated.org_id != ingress.org_id
            || delegated.principal_id != ingress.principal_id
            || delegated.user_id != ingress.user_id
            || delegated.kind != ingress.kind
        {
            return Err(Status::permission_denied(
                "delegated credential identity mismatch",
            ));
        }
        Ok(Some(Arc::from(token)))
    }
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
fn authenticated_downstream_request<T>(
    value: T,
    bearer: Option<&str>,
    service: &'static str,
) -> Result<Request<T>, Status> {
    let bearer = bearer.ok_or_else(|| {
        Status::unauthenticated(format!("verified {service} credential required"))
    })?;
    let mut request = Request::new(value);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {bearer}")
            .parse()
            .map_err(|_| Status::internal("verified credential is not forwardable"))?,
    );
    Ok(request)
}

#[allow(clippy::result_large_err)]
fn validate_identity(claims: Claims, leeway_secs: u64) -> Result<VerifiedIdentity, Status> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Status::unavailable("authentication clock unavailable"))?
        .as_secs();
    if claims.iat > now.saturating_add(leeway_secs)
        || claims.exp <= claims.iat
        || claims.nbf > claims.exp
    {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    if claims.org_id.trim().is_empty()
        || claims.org_id.trim() != claims.org_id
        || claims.org_id.len() > 256
        || claims.scopes.len() > 64
        || claims
            .scopes
            .iter()
            .any(|scope| scope.is_empty() || scope.len() > 128 || scope.trim() != scope)
    {
        return Err(Status::unauthenticated("invalid caller credential"));
    }
    let scopes: Arc<[String]> = claims.scopes.into();
    match claims.principal_type.as_str() {
        "" | "user"
            if !claims.user_id.trim().is_empty()
                && claims.user_id == claims.user_id.trim()
                && claims.sub == claims.user_id
                && claims.service_id.is_empty()
                && claims.reason.is_empty() =>
        {
            Ok(VerifiedIdentity {
                org_id: Arc::from(claims.org_id),
                principal_id: Arc::from(claims.user_id.clone()),
                user_id: Some(Arc::from(claims.user_id)),
                scopes,
                kind: PrincipalKind::User,
                issuer_zdr: claims.zdr,
                session_bearer: None,
                inference_bearer: None,
                execution_bearer: None,
            })
        }
        "service"
            if claims.user_id.is_empty()
                && !claims.service_id.trim().is_empty()
                && claims.service_id == claims.service_id.trim()
                && claims.service_id.starts_with("service:")
                && claims.sub == claims.service_id
                && claims.reason == claims.reason.trim()
                && (3..=500).contains(&claims.reason.len()) =>
        {
            Ok(VerifiedIdentity {
                org_id: Arc::from(claims.org_id),
                principal_id: Arc::from(claims.service_id),
                user_id: None,
                scopes,
                kind: PrincipalKind::Service,
                issuer_zdr: claims.zdr,
                session_bearer: None,
                inference_bearer: None,
                execution_bearer: None,
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
            || serialized
                .get("use")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|usage| usage != "sig")
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
            "org_id": "org-1", "user_id": "user-1", "principal_type": "user"
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

    async fn test_verifier() -> JwtVerifier {
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
            audience: "model-gateway".to_owned(),
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
    }

    #[tokio::test]
    async fn rejects_missing_malformed_wrong_audience_and_ambiguous_principals() {
        let verifier = test_verifier().await;
        assert_eq!(
            verifier.intercept(Request::new(())).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        let mut ambiguous = user_claims("model-gateway");
        ambiguous["service_id"] = json!("service:forged");
        for token in [
            "not-a-jwt".to_owned(),
            sign(&user_claims("other-audience"), "key-1"),
            sign(&ambiguous, "key-1"),
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

        let token = sign(&user_claims("model-gateway"), "key-1");
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
        assert_eq!(identity.user_id(), Some("user-1"));
    }

    #[tokio::test]
    async fn delegated_credentials_require_target_audience_and_matching_identity() {
        let verifier = test_verifier().await;
        let ingress = sign(&user_claims("model-gateway"), "key-1");
        let session = sign(&user_claims("session-core"), "key-1");
        let inference = sign(&user_claims("inference-core"), "key-1");
        let execution = sign(&user_claims("execution-core"), "key-1");
        let mut request = Request::new(());
        for (name, token) in [
            ("authorization", ingress),
            ("x-session-authorization", session),
            ("x-inference-authorization", inference),
            ("x-execution-authorization", execution),
        ] {
            request
                .metadata_mut()
                .insert(name, format!("Bearer {token}").parse().expect("metadata"));
        }
        let request = verifier.intercept(request).expect("bound credentials");
        let identity = request
            .extensions()
            .get::<VerifiedIdentity>()
            .expect("identity");
        assert!(identity.session_request(()).is_ok());
        assert!(identity.inference_request(()).is_ok());
        let execution_request = identity
            .execution_request_with_session(())
            .expect("bound execution and session credentials");
        assert!(execution_request.metadata().get("authorization").is_some());
        assert!(execution_request
            .metadata()
            .get("x-session-authorization")
            .is_some());

        let wrong_audience = sign(&user_claims("model-gateway"), "key-1");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", sign(&user_claims("model-gateway"), "key-1"))
                .parse()
                .expect("metadata"),
        );
        request.metadata_mut().insert(
            "x-session-authorization",
            format!("Bearer {wrong_audience}")
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier.intercept(request).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        let mut forged = user_claims("session-core");
        forged["sub"] = json!("user-2");
        forged["user_id"] = json!("user-2");
        let mut request = Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", sign(&user_claims("model-gateway"), "key-1"))
                .parse()
                .expect("metadata"),
        );
        request.metadata_mut().insert(
            "x-session-authorization",
            format!("Bearer {}", sign(&forged, "key-1"))
                .parse()
                .expect("metadata"),
        );
        assert_eq!(
            verifier.intercept(request).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[tokio::test]
    async fn execution_delegation_rejects_missing_wrong_audience_and_identity_mismatch() {
        let verifier = test_verifier().await;
        let ingress = sign(&user_claims("model-gateway"), "key-1");
        let session = sign(&user_claims("session-core"), "key-1");

        let mut missing = Request::new(());
        for (name, token) in [
            ("authorization", ingress.clone()),
            ("x-session-authorization", session.clone()),
        ] {
            missing
                .metadata_mut()
                .insert(name, format!("Bearer {token}").parse().expect("metadata"));
        }
        let missing = verifier.intercept(missing).expect("ingress is valid");
        let identity = missing
            .extensions()
            .get::<VerifiedIdentity>()
            .expect("identity");
        assert_eq!(
            identity
                .execution_request_with_session(())
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut wrong_audience = Request::new(());
        for (name, token) in [
            ("authorization", ingress.clone()),
            ("x-session-authorization", session.clone()),
            (
                "x-execution-authorization",
                sign(&user_claims("model-gateway"), "key-1"),
            ),
        ] {
            wrong_audience
                .metadata_mut()
                .insert(name, format!("Bearer {token}").parse().expect("metadata"));
        }
        assert_eq!(
            verifier.intercept(wrong_audience).unwrap_err().code(),
            tonic::Code::Unauthenticated
        );

        let mut forged_claims = user_claims("execution-core");
        forged_claims["sub"] = json!("user-2");
        forged_claims["user_id"] = json!("user-2");
        let mut mismatch = Request::new(());
        for (name, token) in [
            ("authorization", ingress),
            ("x-session-authorization", session),
            ("x-execution-authorization", sign(&forged_claims, "key-1")),
        ] {
            mismatch
                .metadata_mut()
                .insert(name, format!("Bearer {token}").parse().expect("metadata"));
        }
        assert_eq!(
            verifier.intercept(mismatch).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn identity_and_operation_authorization_fail_closed() {
        let user = VerifiedIdentity::user_for_test("org-a", "user-a", false);
        assert_eq!(
            user.authorize_org("").unwrap_err().code(),
            tonic::Code::InvalidArgument
        );
        assert_eq!(
            user.authorize_org("org-b").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
        assert_eq!(
            user.authorize_user("user-b").unwrap_err().code(),
            tonic::Code::PermissionDenied
        );

        let missing = Request::new(());
        assert_eq!(
            authorize_request(&missing, "org-a", RpcAccess::Read)
                .unwrap_err()
                .code(),
            tonic::Code::Unauthenticated
        );

        let service = VerifiedIdentity::service_for_test(
            "org-a",
            "service:execution-core",
            &["gateway:read"],
            false,
        );
        let mut request = Request::new(());
        request.extensions_mut().insert(service);
        authorize_request(&request, "org-a", RpcAccess::Read).expect("scoped read");
        assert_eq!(
            authorize_request(&request, "org-a", RpcAccess::Tool)
                .unwrap_err()
                .code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn issuer_zdr_is_monotonic_and_blocks_durable_mutation() {
        let identity = VerifiedIdentity::user_for_test("org-a", "user-a", true);
        assert!(identity.effective_zdr(false));
        let mut request = Request::new(());
        request.extensions_mut().insert(identity);
        assert_eq!(
            authorize_request(&request, "org-a", RpcAccess::Write)
                .unwrap_err()
                .code(),
            tonic::Code::FailedPrecondition
        );
    }
}
