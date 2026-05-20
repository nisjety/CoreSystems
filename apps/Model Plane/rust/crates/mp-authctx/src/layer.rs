use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, Response};
use chrono::Utc;
use jsonwebtoken::{Algorithm, Validation, decode, decode_header};
use tower::{Layer, Service};

use crate::{AuthCtx, AuthCtxError, Claims, JwksCache};

/// Static configuration for the middleware.
#[derive(Debug, Clone)]
pub struct Config {
    /// The audience value the JWT `aud` claim must equal. Tokens minted
    /// for a different audience are rejected even when the signature is
    /// valid.
    pub audience: String,

    /// Expected issuer (`iss` claim).
    pub expected_issuer: String,

    /// URL of the auth-core JWKS endpoint.
    pub jwks_url: String,

    /// How long a fetched JWKS is treated as fresh before a refresh.
    pub jwks_ttl: Duration,

    /// True → reject anything that doesn't carry a valid token. False
    /// → observe-only (decode + log drift, but pass missing tokens
    /// through to legacy handlers).
    pub enforce: bool,
}

impl Config {
    /// Build a Config from environment variables with sensible defaults.
    ///
    /// Honoured env vars:
    /// - `AUTH_CORE_JWKS_URL`  (default `http://auth-core:3011/api/convex-auth/jwks`)
    /// - `AUTH_CORE_ISSUER`    (default `http://auth-core:3011/api/convex-auth`)
    /// - `AUTHCTX_ENFORCE`     (`1`/`true`/`yes` → enforce mode)
    /// - `AUTHCTX_JWKS_TTL_SECS` (default 900)
    #[must_use]
    pub fn from_env(audience: impl Into<String>) -> Self {
        let jwks_url = env::var("AUTH_CORE_JWKS_URL")
            .unwrap_or_else(|_| "http://auth-core:3011/api/convex-auth/jwks".to_owned());
        let expected_issuer = env::var("AUTH_CORE_ISSUER")
            .unwrap_or_else(|_| "http://auth-core:3011/api/convex-auth".to_owned());
        let enforce = matches!(
            env::var("AUTHCTX_ENFORCE")
                .unwrap_or_default()
                .to_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        );
        let jwks_ttl = env::var("AUTHCTX_JWKS_TTL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .map_or(Duration::from_secs(900), Duration::from_secs);
        Self {
            audience: audience.into(),
            expected_issuer,
            jwks_url,
            jwks_ttl,
            enforce,
        }
    }
}

/// Tower `Layer` that applies the auth context to every request.
#[derive(Clone)]
pub struct AuthCtxLayer {
    inner: Arc<AuthCtxInner>,
}

struct AuthCtxInner {
    config: Config,
    jwks: JwksCache,
}

impl std::fmt::Debug for AuthCtxInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthCtxInner")
            .field("audience", &self.config.audience)
            .field("enforce", &self.config.enforce)
            .finish_non_exhaustive()
    }
}

impl AuthCtxLayer {
    #[must_use]
    pub fn new(config: Config) -> Self {
        let jwks = JwksCache::new(config.jwks_url.clone(), config.jwks_ttl);
        Self {
            inner: Arc::new(AuthCtxInner { config, jwks }),
        }
    }
}

impl<S> Layer<S> for AuthCtxLayer {
    type Service = AuthCtxMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthCtxMiddleware {
            inner,
            ctx: self.inner.clone(),
        }
    }
}

/// The tower `Service` produced by [`AuthCtxLayer`]. Generic over the
/// inner service so it can wrap any axum router or sub-router.
#[derive(Clone)]
pub struct AuthCtxMiddleware<S> {
    inner: S,
    ctx: Arc<AuthCtxInner>,
}

impl<S> Service<Request<Body>> for AuthCtxMiddleware<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let ctx = self.ctx.clone();
        // Service trait gives us `&mut self` but we must outlive the
        // future. The standard tower pattern: clone first, then swap
        // the clone into `self` so the original (which is poll_ready'd)
        // moves into the future.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        Box::pin(async move {
            let token = extract_bearer(req.headers());
            let header_org = header_value(&req, "x-org-id").map(str::to_owned);

            match (ctx.config.enforce, token.as_deref()) {
                (true, None) => {
                    // Enforce + no token → 401.
                    tracing::warn!("authctx enforce: missing bearer; rejecting");
                    return Ok(error_response(&AuthCtxError::MissingHeader));
                }
                (false, None) => {
                    // Observe + no token → pass through unchanged.
                    return inner.call(req).await;
                }
                _ => {}
            }

            let token = token.expect("token present per the match above");
            match verify_token(&ctx, &token).await {
                Ok(claims) => {
                    if let Some(h) = header_org.as_deref() {
                        if h != claims.org_id {
                            if ctx.config.enforce {
                                tracing::warn!(
                                    header = h,
                                    jwt = claims.org_id.as_str(),
                                    "authctx enforce: org_id drift; rejecting"
                                );
                                return Ok(error_response(&AuthCtxError::OrgIdMismatch {
                                    header: h.to_owned(),
                                    jwt: claims.org_id.clone(),
                                }));
                            }
                            tracing::warn!(
                                header = h,
                                jwt = claims.org_id.as_str(),
                                "authctx observe: X-Org-ID drift (trusted header for now)"
                            );
                        }
                    }
                    req.extensions_mut().insert(AuthCtx::verified(claims));
                }
                Err(err) => {
                    if ctx.config.enforce {
                        tracing::warn!(error = %err, "authctx enforce: verification failed");
                        return Ok(error_response(&err));
                    }
                    // Observe mode: log + try unverified decode for telemetry.
                    tracing::warn!(error = %err, "authctx observe: verification failed; trying unverified decode");
                    if let Ok(claims) = decode_unverified(&token) {
                        req.extensions_mut().insert(AuthCtx::observe(claims));
                    }
                }
            }

            inner.call(req).await
        })
    }
}

fn extract_bearer(headers: &http::HeaderMap) -> Option<String> {
    let value = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = value.trim();
    if !trimmed.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("Bearer ")) {
        return None;
    }
    let token = trimmed[7..].trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_owned())
    }
}

fn header_value<'a>(req: &'a Request<Body>, name: &str) -> Option<&'a str> {
    req.headers().get(name)?.to_str().ok().map(str::trim)
}

async fn verify_token(ctx: &AuthCtxInner, token: &str) -> Result<Claims, AuthCtxError> {
    let header = decode_header(token).map_err(|e| AuthCtxError::Decode(e.to_string()))?;
    let kid = header
        .kid
        .ok_or_else(|| AuthCtxError::Decode("JWT header missing kid".to_owned()))?;
    let key = ctx
        .jwks
        .decoding_key(&kid)
        .await?
        .ok_or_else(|| AuthCtxError::Signature(format!("no JWK matches kid={kid}")))?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[ctx.config.audience.as_str()]);
    validation.set_issuer(&[ctx.config.expected_issuer.as_str()]);
    validation.leeway = 30;

    let data = decode::<Claims>(token, &key, &validation)
        .map_err(|e| AuthCtxError::Signature(e.to_string()))?;
    let claims = data.claims;

    if claims.is_expired(Utc::now().timestamp()) {
        return Err(AuthCtxError::Expired);
    }
    if claims.aud != ctx.config.audience {
        return Err(AuthCtxError::AudienceMismatch {
            expected: ctx.config.audience.clone(),
            got: claims.aud,
        });
    }
    if claims.iss != ctx.config.expected_issuer {
        return Err(AuthCtxError::IssuerMismatch {
            expected: ctx.config.expected_issuer.clone(),
            got: claims.iss,
        });
    }
    Ok(claims)
}

fn decode_unverified(token: &str) -> Result<Claims, AuthCtxError> {
    let segments: Vec<&str> = token.split('.').collect();
    if segments.len() != 3 {
        return Err(AuthCtxError::Decode("malformed JWT".to_owned()));
    }
    use base64::Engine;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(segments[1])
        .map_err(|e| AuthCtxError::Decode(e.to_string()))?;
    serde_json::from_slice::<Claims>(&payload)
        .map_err(|e| AuthCtxError::Decode(e.to_string()))
}

fn error_response(err: &AuthCtxError) -> Response<Body> {
    let status = err.status();
    let body = serde_json::json!({ "error": err.to_string() }).to_string();
    Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .expect("empty body is always valid")
        })
}
