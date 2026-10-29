use std::{collections::HashMap, fmt, sync::Arc, time::Duration};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tonic::{service::Interceptor, Request, Status};

const SERVICE_READ_SCOPE: &str = "graph:read";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    User,
    Service,
}

#[derive(Debug, Clone)]
pub struct Principal {
    // Retained for downstream audit/visibility policy without ever trusting a
    // caller-supplied user header. Graph reads are currently tenant-scoped.
    #[allow(dead_code)]
    pub subject_id: String,
    pub org_id: String,
    #[allow(dead_code)]
    pub kind: PrincipalKind,
}

impl Principal {
    pub fn authorizes_org(&self, requested_org_id: &str) -> bool {
        !requested_org_id.is_empty() && self.org_id == requested_org_id
    }
}

#[derive(Debug)]
pub enum AuthError {
    InvalidConfiguration(&'static str),
    InvalidCredential,
    InvalidIdentity,
    InsufficientScope,
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidConfiguration(message) => message,
            Self::InvalidCredential => "invalid credential",
            Self::InvalidIdentity => "invalid identity claims",
            Self::InsufficientScope => "insufficient scope",
        };
        f.write_str(message)
    }
}

impl std::error::Error for AuthError {}

#[derive(Clone)]
enum VerificationKeys {
    Static(DecodingKey),
    Jwks(Arc<HashMap<String, DecodingKey>>),
}

#[derive(Clone)]
pub struct JwtVerifier {
    issuer: String,
    audience: String,
    keys: VerificationKeys,
}

#[derive(Debug, Deserialize)]
struct Claims {
    iss: String,
    aud: String,
    sub: String,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    service_id: Option<String>,
    org_id: String,
    #[serde(default)]
    scopes: Vec<String>,
    exp: usize,
    nbf: usize,
}

#[derive(Deserialize)]
struct JwksDocument {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    alg: String,
    #[serde(rename = "use")]
    key_use: String,
    n: String,
    e: String,
}

impl JwtVerifier {
    pub async fn from_env() -> anyhow::Result<Self> {
        let issuer = required_env("JWT_REQUIRED_ISSUER")?;
        let audience = required_env("JWT_REQUIRED_AUDIENCE")?;

        let jwks_url = optional_env("JWT_JWKS_URL");
        let pem = optional_env("JWT_PUBLIC_KEY_PEM");
        let key_file = optional_env("JWT_PUBLIC_KEY_FILE");
        if [jwks_url.is_some(), pem.is_some(), key_file.is_some()]
            .into_iter()
            .filter(|configured| *configured)
            .count()
            != 1
        {
            anyhow::bail!(
                "configure exactly one of JWT_JWKS_URL, JWT_PUBLIC_KEY_PEM, or JWT_PUBLIC_KEY_FILE"
            );
        }

        if let Some(url) = jwks_url.as_deref() {
            return Self::from_jwks(&issuer, &audience, url).await;
        }
        if let Some(pem) = pem.as_deref() {
            return Self::from_pem(issuer, audience, pem.as_bytes()).map_err(Into::into);
        }

        let path = key_file
            .as_deref()
            .expect("exactly one JWT key source validated");
        let contents = std::fs::read(path)
            .map_err(|error| anyhow::anyhow!("read JWT public key file: {error}"))?;
        Self::from_pem(issuer, audience, &contents).map_err(Into::into)
    }

    pub fn from_pem(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        pem: &[u8],
    ) -> Result<Self, AuthError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if issuer.trim().is_empty()
            || issuer.trim() != issuer
            || audience.trim().is_empty()
            || audience.trim() != audience
            || pem.is_empty()
        {
            return Err(AuthError::InvalidConfiguration(
                "JWT issuer, audience, and public key are required",
            ));
        }
        let key = DecodingKey::from_rsa_pem(pem).map_err(|_| {
            AuthError::InvalidConfiguration("JWT public key is not a valid RSA PEM")
        })?;
        Ok(Self {
            issuer,
            audience,
            keys: VerificationKeys::Static(key),
        })
    }

    async fn from_jwks(issuer: &str, audience: &str, url: &str) -> anyhow::Result<Self> {
        let document: JwksDocument = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?
            .get(url)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("fetch JWT JWKS: {error}"))?
            .error_for_status()
            .map_err(|error| anyhow::anyhow!("fetch JWT JWKS: {error}"))?
            .json()
            .await
            .map_err(|error| anyhow::anyhow!("decode JWT JWKS: {error}"))?;

        let mut keys = HashMap::new();
        for key in document
            .keys
            .into_iter()
            .filter(|key| key.alg == "RS256" && key.key_use == "sig" && !key.kid.is_empty())
        {
            let decoding_key = DecodingKey::from_rsa_components(&key.n, &key.e)?;
            if keys.insert(key.kid, decoding_key).is_some() {
                anyhow::bail!("JWT JWKS contains a duplicate key id");
            }
        }
        if keys.is_empty() {
            anyhow::bail!("JWT JWKS contains no usable RS256 signing keys");
        }

        Ok(Self {
            issuer: issuer.to_owned(),
            audience: audience.to_owned(),
            keys: VerificationKeys::Jwks(Arc::new(keys)),
        })
    }

    pub fn verify(&self, token: &str) -> Result<Principal, AuthError> {
        if token.trim().is_empty() {
            return Err(AuthError::InvalidCredential);
        }
        let header = decode_header(token).map_err(|_| AuthError::InvalidCredential)?;
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::InvalidCredential);
        }
        let key = match &self.keys {
            VerificationKeys::Static(key) => key,
            VerificationKeys::Jwks(keys) => {
                let kid = header.kid.as_deref().ok_or(AuthError::InvalidCredential)?;
                keys.get(kid).ok_or(AuthError::InvalidCredential)?
            }
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "nbf", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = 30;

        let claims = decode::<Claims>(token, key, &validation)
            .map_err(|_| AuthError::InvalidCredential)?
            .claims;
        self.principal_from_claims(claims)
    }

    fn principal_from_claims(&self, claims: Claims) -> Result<Principal, AuthError> {
        if claims.iss != self.issuer
            || claims.aud != self.audience
            || claims.sub.trim().is_empty()
            || claims.org_id.trim().is_empty()
            || claims.org_id.trim() != claims.org_id
            || claims.exp == 0
            || claims.nbf == 0
        {
            return Err(AuthError::InvalidIdentity);
        }

        let (subject_id, kind) = match (claims.user_id, claims.service_id) {
            (Some(user_id), None)
                if !user_id.trim().is_empty()
                    && user_id.trim() == user_id
                    && claims.sub == user_id =>
            {
                (user_id, PrincipalKind::User)
            }
            (None, Some(service_id))
                if !service_id.trim().is_empty()
                    && service_id.trim() == service_id
                    && claims.sub == service_id =>
            {
                if !claims
                    .scopes
                    .iter()
                    .any(|scope| scope == SERVICE_READ_SCOPE)
                {
                    return Err(AuthError::InsufficientScope);
                }
                (service_id, PrincipalKind::Service)
            }
            _ => return Err(AuthError::InvalidIdentity),
        };

        Ok(Principal {
            subject_id,
            org_id: claims.org_id,
            kind,
        })
    }
}

fn required_env(name: &'static str) -> anyhow::Result<String> {
    optional_env(name).ok_or_else(|| anyhow::anyhow!("{name} is required"))
}

fn optional_env(name: &'static str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn bearer_from_http(headers: &axum::http::HeaderMap) -> Result<&str, AuthError> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(AuthError::InvalidCredential)?;
    bearer_value(value)
}

fn bearer_from_grpc(request: &Request<()>) -> Result<&str, AuthError> {
    let value = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or(AuthError::InvalidCredential)?;
    bearer_value(value)
}

fn bearer_value(value: &str) -> Result<&str, AuthError> {
    let (scheme, token) = value.split_once(' ').ok_or(AuthError::InvalidCredential)?;
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() || token.contains(' ') {
        return Err(AuthError::InvalidCredential);
    }
    Ok(token)
}

#[derive(Clone)]
pub struct JwtInterceptor {
    verifier: Arc<JwtVerifier>,
}

impl JwtInterceptor {
    pub fn new(verifier: Arc<JwtVerifier>) -> Self {
        Self { verifier }
    }
}

impl Interceptor for JwtInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        let token = bearer_from_grpc(&request)
            .map_err(|_| Status::unauthenticated("invalid or missing credential"))?;
        let principal = self.verifier.verify(token).map_err(|error| match error {
            AuthError::InsufficientScope => Status::permission_denied("insufficient scope"),
            _ => Status::unauthenticated("invalid or missing credential"),
        })?;
        request.extensions_mut().insert(principal);
        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, OnceLock};

    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
        RsaPrivateKey, RsaPublicKey,
    };
    use serde::Serialize;

    use super::{JwtInterceptor, JwtVerifier, Principal, PrincipalKind};

    const ISSUER: &str = "https://control.test/api/convex-auth";
    const AUDIENCE: &str = "data-plane";

    struct TestKeys {
        verifier: JwtVerifier,
        encoding: EncodingKey,
    }

    fn keys() -> &'static TestKeys {
        static KEYS: OnceLock<TestKeys> = OnceLock::new();
        KEYS.get_or_init(|| {
            let private = RsaPrivateKey::new(&mut thread_rng(), 2048).expect("generate test key");
            let private_pem = private
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode private key");
            let public_pem = RsaPublicKey::from(&private)
                .to_public_key_pem(LineEnding::LF)
                .expect("encode public key");
            TestKeys {
                verifier: JwtVerifier::from_pem(ISSUER, AUDIENCE, public_pem.as_bytes())
                    .expect("build verifier"),
                encoding: EncodingKey::from_rsa_pem(private_pem.as_bytes())
                    .expect("build encoding key"),
            }
        })
    }

    #[derive(Clone, Serialize)]
    struct TestClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        sub: &'a str,
        user_id: Option<&'a str>,
        service_id: Option<&'a str>,
        org_id: &'a str,
        scopes: Vec<&'a str>,
        exp: usize,
        nbf: usize,
    }

    fn token(claims: &TestClaims<'_>) -> String {
        encode(&Header::new(Algorithm::RS256), claims, &keys().encoding).expect("sign token")
    }

    fn user_claims(org_id: &str) -> TestClaims<'_> {
        let now = chrono::Utc::now().timestamp() as usize;
        TestClaims {
            iss: ISSUER,
            aud: AUDIENCE,
            sub: "user-1",
            user_id: Some("user-1"),
            service_id: None,
            org_id,
            scopes: vec![],
            exp: now + 300,
            nbf: now.saturating_sub(5),
        }
    }

    #[test]
    fn verifies_valid_user_claims_and_pins_tenant() {
        let principal = keys()
            .verifier
            .verify(&token(&user_claims("org-a")))
            .expect("valid token");
        assert_eq!(principal.org_id, "org-a");
        assert_eq!(principal.subject_id, "user-1");
        assert_eq!(principal.kind, PrincipalKind::User);
    }

    #[test]
    fn rejects_forged_token() {
        let mut forged = token(&user_claims("org-a"));
        forged.push('x');
        assert!(keys().verifier.verify(&forged).is_err());

        // alg=none, empty JSON claims, no signature. This is structurally a
        // JWT but can never pass the RS256-only verifier.
        let unsigned = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.e30.";
        assert!(keys().verifier.verify(unsigned).is_err());
    }

    #[test]
    fn rejects_wrong_issuer_and_audience() {
        let now = chrono::Utc::now().timestamp() as usize;
        let wrong_issuer = TestClaims {
            iss: "https://attacker.invalid",
            ..user_claims("org-a")
        };
        assert!(keys().verifier.verify(&token(&wrong_issuer)).is_err());

        let wrong_audience = TestClaims {
            aud: "model-gateway",
            exp: now + 300,
            ..user_claims("org-a")
        };
        assert!(keys().verifier.verify(&token(&wrong_audience)).is_err());
    }

    #[test]
    fn rejects_expired_or_ambiguous_identity() {
        let mut expired = user_claims("org-a");
        expired.exp = 1;
        assert!(keys().verifier.verify(&token(&expired)).is_err());

        let ambiguous = TestClaims {
            service_id: Some("service-1"),
            ..user_claims("org-a")
        };
        assert!(keys().verifier.verify(&token(&ambiguous)).is_err());

        let subject_mismatch = TestClaims {
            sub: "user-2",
            ..user_claims("org-a")
        };
        assert!(keys().verifier.verify(&token(&subject_mismatch)).is_err());

        let empty_org = TestClaims {
            org_id: "",
            ..user_claims("org-a")
        };
        assert!(keys().verifier.verify(&token(&empty_org)).is_err());
    }

    #[test]
    fn service_principal_requires_graph_read_scope() {
        let now = chrono::Utc::now().timestamp() as usize;
        let unscoped = TestClaims {
            iss: ISSUER,
            aud: AUDIENCE,
            sub: "service-1",
            user_id: None,
            service_id: Some("service-1"),
            org_id: "org-a",
            scopes: vec![],
            exp: now + 300,
            nbf: now.saturating_sub(5),
        };
        assert!(keys().verifier.verify(&token(&unscoped)).is_err());

        let scoped = TestClaims {
            scopes: vec!["graph:read"],
            ..unscoped
        };
        let principal = keys()
            .verifier
            .verify(&token(&scoped))
            .expect("scoped service token");
        assert_eq!(principal.kind, PrincipalKind::Service);
    }

    #[test]
    fn grpc_interceptor_requires_verified_bearer_and_injects_principal() {
        let verifier = Arc::new(keys().verifier.clone());
        let mut interceptor = JwtInterceptor::new(verifier);

        let mut missing = tonic::Request::new(());
        missing
            .metadata_mut()
            .insert("x-org-id", "org-a".parse().expect("metadata"));
        assert_eq!(
            tonic::service::Interceptor::call(&mut interceptor, missing)
                .expect_err("header-only request denied")
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut forged = tonic::Request::new(());
        forged.metadata_mut().insert(
            "authorization",
            "Bearer not-a-jwt".parse().expect("metadata"),
        );
        assert_eq!(
            tonic::service::Interceptor::call(&mut interceptor, forged)
                .expect_err("forged token denied")
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut valid = tonic::Request::new(());
        valid.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", token(&user_claims("org-a")))
                .parse()
                .expect("metadata"),
        );
        let verified = tonic::service::Interceptor::call(&mut interceptor, valid)
            .expect("valid bearer accepted");
        let principal = verified
            .extensions()
            .get::<Principal>()
            .expect("principal injected");
        assert_eq!(principal.org_id, "org-a");
        assert_eq!(principal.kind, PrincipalKind::User);
    }
}
