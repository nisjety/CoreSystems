use std::sync::Arc;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use tonic::{service::Interceptor, Request, Status};

use crate::authz::{AuthContext, AuthMethod, Claims, EffectiveAcl, PolicyClient};

#[derive(Clone)]
pub struct JwtVerifier {
    key: DecodingKey,
    issuer: String,
    audience: String,
}

impl JwtVerifier {
    pub fn from_env() -> anyhow::Result<Self> {
        let pem = required_env("JWT_PUBLIC_KEY_PEM")?;
        let issuer = required_env("JWT_REQUIRED_ISSUER")?;
        let audience = required_env("JWT_REQUIRED_AUDIENCE")?;
        Self::from_pem(issuer, audience, pem.as_bytes())
    }

    pub fn from_pem(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        pem: &[u8],
    ) -> anyhow::Result<Self> {
        let issuer = issuer.into();
        let audience = audience.into();
        anyhow::ensure!(
            !issuer.trim().is_empty() && issuer.trim() == issuer,
            "JWT_REQUIRED_ISSUER is invalid"
        );
        anyhow::ensure!(
            !audience.trim().is_empty() && audience.trim() == audience,
            "JWT_REQUIRED_AUDIENCE is invalid"
        );
        let key = DecodingKey::from_rsa_pem(pem)
            .map_err(|_| anyhow::anyhow!("JWT public key is not a valid RSA PEM"))?;
        Ok(Self {
            key,
            issuer,
            audience,
        })
    }

    // tonic::Status is intentionally returned at the interceptor boundary so
    // authentication failures cannot be accidentally translated into 500s.
    #[allow(clippy::result_large_err)]
    fn verify(&self, token: &str) -> Result<AuthContext, Status> {
        let header = decode_header(token)
            .map_err(|_| Status::unauthenticated("invalid or missing credential"))?;
        if header.alg != Algorithm::RS256 {
            return Err(Status::unauthenticated("invalid or missing credential"));
        }

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "nbf", "aud", "iss", "sub"]);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = 30;

        let claims = decode::<Claims>(token, &self.key, &validation)
            .map_err(|_| Status::unauthenticated("invalid or missing credential"))?
            .claims;
        let org_id = claims
            .org_id
            .filter(|org| !org.trim().is_empty() && org.trim() == org)
            .ok_or_else(|| Status::unauthenticated("invalid identity claims"))?;
        if claims.sub.trim().is_empty() || claims.sub.trim() != claims.sub {
            return Err(Status::unauthenticated("invalid identity claims"));
        }

        Ok(AuthContext {
            user_id: Some(claims.sub),
            org_id,
            auth_method: AuthMethod::Jwt,
            scopes: claims.scopes,
            acl: EffectiveAcl::default(),
            request_id: uuid::Uuid::new_v4().to_string(),
            verified_bearer: Some(token.to_owned()),
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
        let token = bearer_token(&request)?;
        let mut ctx = self.verifier.verify(token)?;
        if let Some(request_id) = request
            .metadata()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            ctx.request_id = request_id.to_owned();
        }
        record_trace_context(&request);
        request.extensions_mut().insert(ctx);
        Ok(request)
    }
}

// Keep the native tonic error type at this boundary for fail-closed auth.
#[allow(clippy::result_large_err)]
fn bearer_token(request: &Request<()>) -> Result<&str, Status> {
    let value = request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| Status::unauthenticated("invalid or missing credential"))?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| Status::unauthenticated("invalid or missing credential"))?;
    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() || token.contains(' ') {
        return Err(Status::unauthenticated("invalid or missing credential"));
    }
    Ok(token)
}

pub async fn authorize_request<P, T>(
    policy: &P,
    request: &Request<T>,
    requested_org_id: &str,
    requested_user_id: Option<&str>,
) -> Result<AuthContext, Status>
where
    P: PolicyClient + ?Sized,
{
    let mut ctx = request
        .extensions()
        .get::<AuthContext>()
        .cloned()
        .ok_or_else(|| Status::unauthenticated("missing verified principal"))?;
    if requested_org_id.is_empty() || requested_org_id != ctx.org_id {
        return Err(Status::permission_denied(
            "tenant does not match authenticated principal",
        ));
    }
    let user_id = ctx
        .user_id
        .as_deref()
        .ok_or_else(|| Status::unauthenticated("missing verified user identity"))?;
    if requested_user_id
        .filter(|user| !user.is_empty())
        .is_some_and(|user| user != user_id)
    {
        return Err(Status::permission_denied(
            "user does not match authenticated principal",
        ));
    }

    let decision = policy.resolve(user_id, &ctx.org_id).await;
    if !decision.is_member || !decision.acl.can_read {
        return Err(Status::permission_denied("Control authorization denied"));
    }
    ctx.acl = decision.acl;
    Ok(ctx)
}

fn record_trace_context(req: &Request<()>) {
    let traceparent = req
        .metadata()
        .get("traceparent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let caller_trace_id = req
        .metadata()
        .get("x-trace-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !traceparent.is_empty() || !caller_trace_id.is_empty() {
        tracing::Span::current().record("traceparent", traceparent);
        tracing::Span::current().record("caller_trace_id", caller_trace_id);
        tracing::debug!(
            traceparent,
            caller_trace_id,
            "grpc trace context propagated"
        );
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

    use super::{authorize_request, JwtInterceptor, JwtVerifier};
    use crate::authz::{EffectiveAcl, PolicyClient, PolicyDecision};

    const ISSUER: &str = "https://control.test/api/convex-auth";
    const AUDIENCE: &str = "data-plane";

    struct Keys {
        verifier: JwtVerifier,
        encoding: EncodingKey,
    }

    fn keys() -> &'static Keys {
        static KEYS: OnceLock<Keys> = OnceLock::new();
        KEYS.get_or_init(|| {
            let private = RsaPrivateKey::new(&mut thread_rng(), 2048).expect("generate key");
            let private_pem = private.to_pkcs8_pem(LineEnding::LF).expect("private pem");
            let public_pem = RsaPublicKey::from(&private)
                .to_public_key_pem(LineEnding::LF)
                .expect("public pem");
            Keys {
                verifier: JwtVerifier::from_pem(ISSUER, AUDIENCE, public_pem.as_bytes())
                    .expect("verifier"),
                encoding: EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
            }
        })
    }

    #[derive(Serialize)]
    struct TestClaims<'a> {
        iss: &'a str,
        aud: &'a str,
        sub: &'a str,
        org_id: &'a str,
        scopes: Vec<&'a str>,
        exp: usize,
        nbf: usize,
    }

    fn token(org_id: &str) -> String {
        let now = chrono::Utc::now().timestamp() as usize;
        encode(
            &Header::new(Algorithm::RS256),
            &TestClaims {
                iss: ISSUER,
                aud: AUDIENCE,
                sub: "user-1",
                org_id,
                scopes: vec![],
                exp: now + 300,
                nbf: now.saturating_sub(5),
            },
            &keys().encoding,
        )
        .expect("token")
    }

    #[test]
    fn interceptor_rejects_missing_shared_key_and_forged_credentials() {
        let mut interceptor = JwtInterceptor::new(Arc::new(keys().verifier.clone()));

        let missing = tonic::Request::new(());
        assert_eq!(
            tonic::service::Interceptor::call(&mut interceptor, missing)
                .expect_err("missing denied")
                .code(),
            tonic::Code::Unauthenticated
        );

        let mut shared_key = tonic::Request::new(());
        shared_key.metadata_mut().insert(
            "x-internal-api-key",
            "legacy-key".parse().expect("metadata"),
        );
        assert_eq!(
            tonic::service::Interceptor::call(&mut interceptor, shared_key)
                .expect_err("shared key denied")
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
                .expect_err("forged denied")
                .code(),
            tonic::Code::Unauthenticated
        );
    }

    #[test]
    fn interceptor_injects_verified_claim_principal() {
        let mut interceptor = JwtInterceptor::new(Arc::new(keys().verifier.clone()));
        let mut request = tonic::Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", token("org-a"))
                .parse()
                .expect("metadata"),
        );
        let request = tonic::service::Interceptor::call(&mut interceptor, request)
            .expect("valid token accepted");
        let ctx = request
            .extensions()
            .get::<crate::authz::AuthContext>()
            .expect("auth context injected");
        assert_eq!(ctx.org_id, "org-a");
        assert_eq!(ctx.user_id.as_deref(), Some("user-1"));
    }

    struct Policy(bool);

    #[async_trait::async_trait]
    impl PolicyClient for Policy {
        async fn resolve(&self, _user_id: &str, _org_id: &str) -> PolicyDecision {
            if self.0 {
                PolicyDecision {
                    is_member: true,
                    acl: EffectiveAcl {
                        can_read: true,
                        ..Default::default()
                    },
                    cause: "ok".into(),
                }
            } else {
                PolicyDecision::deny("denied:no_membership")
            }
        }
    }

    #[tokio::test]
    async fn authorizer_pins_org_user_and_fails_closed_on_policy() {
        let mut interceptor = JwtInterceptor::new(Arc::new(keys().verifier.clone()));
        let mut request = tonic::Request::new(());
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {}", token("org-a"))
                .parse()
                .expect("metadata"),
        );
        let request = tonic::service::Interceptor::call(&mut interceptor, request)
            .expect("valid token accepted");

        let allowed = authorize_request(&Policy(true), &request, "org-a", Some("user-1"))
            .await
            .expect("own tenant allowed");
        assert_eq!(allowed.org_id, "org-a");
        assert_eq!(allowed.user_id.as_deref(), Some("user-1"));

        let cross_tenant = authorize_request(&Policy(true), &request, "org-b", None)
            .await
            .err()
            .expect("cross tenant denied");
        assert_eq!(cross_tenant.code(), tonic::Code::PermissionDenied);

        let policy_denial = authorize_request(&Policy(false), &request, "org-a", None)
            .await
            .err()
            .expect("policy deny closed");
        assert_eq!(policy_denial.code(), tonic::Code::PermissionDenied);
    }
}
