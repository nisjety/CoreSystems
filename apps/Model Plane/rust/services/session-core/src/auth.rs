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
        Self {
            org_id: Arc::from(org_id),
            principal_id: Arc::from("service:session-core"),
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
    pub leeway_secs: u64,
}

impl AuthConfig {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            jwks_url: required_env("AUTH_CORE_JWKS_URL")?,
            issuer: required_env("AUTH_CORE_ISSUER")?,
            audience: required_env("SESSION_CORE_AUTH_AUDIENCE")?,
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
            leeway_secs: config.leeway_secs,
            keys: Arc::new(keys),
        })
    }

    #[allow(clippy::result_large_err)]
    pub fn intercept(&self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = extract_bearer(&request)?;
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
        validation.set_audience(&[self.audience.as_ref()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = self.leeway_secs;
        let claims = decode::<Claims>(token, key, &validation)
            .map_err(|_| Status::unauthenticated("invalid caller credential"))?
            .claims;
        if claims.aud.as_str() != Some(self.audience.as_ref()) {
            return Err(Status::unauthenticated("invalid caller credential"));
        }
        request.extensions_mut().insert(validate_identity(claims)?);
        Ok(request)
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
            leeway_secs: 0,
        })
        .await
        .expect("verifier")
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
}
