use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, Response};
use chrono::Utc;
use jsonwebtoken::{decode, decode_header, Algorithm, Validation};
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
    /// - `AUTHCTX_ENFORCE`     (`1`, `true`, `yes` or `on`, in any case, enforce; unset observes.
    ///   See `enforce_from` for the exact reading and how documents-api-go differs.)
    /// - `AUTHCTX_JWKS_TTL_SECS` (default 900)
    #[must_use]
    pub fn from_env(audience: impl Into<String>) -> Self {
        Self::from_lookup(audience, |key| env::var(key).ok())
    }

    /// The reader behind [`Config::from_env`], with the variable lookup injected so the defaults can
    /// be tested without touching the process environment.
    fn from_lookup(audience: impl Into<String>, lookup: impl Fn(&str) -> Option<String>) -> Self {
        let jwks_url = lookup("AUTH_CORE_JWKS_URL")
            .unwrap_or_else(|| "http://auth-core:3011/api/convex-auth/jwks".to_owned());
        let expected_issuer = lookup("AUTH_CORE_ISSUER")
            .unwrap_or_else(|| "http://auth-core:3011/api/convex-auth".to_owned());
        let enforce = enforce_from(lookup("AUTHCTX_ENFORCE").as_deref());
        let jwks_ttl = lookup("AUTHCTX_JWKS_TTL_SECS")
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

/// The reading of `AUTHCTX_ENFORCE`, pure so the accepted spellings are pinned without the process
/// environment.
///
/// Enforce on `1`, `true`, `yes` or `on`, in any case and with no trimming, so ` 1 `, `true ` and
/// `enforce` observe. Unset, empty and every other value observe.
///
/// This is a documented exception to the exact-literal rule for switches, and it stays lenient on
/// purpose: an exact read would enforce fewer spellings and weaken the check. documents-api-go
/// reads the same name the other way round. It observes only on `0`, `false`, `no` and `off`
/// (trimmed, any case) and enforces otherwise, so it enforces when the variable is unset and on a
/// value this reader observes, such as `unexpected`. The Data Plane compose sets
/// `AUTHCTX_ENFORCE: "1"`; no Model Plane compose sets it, so Model Plane services observe unless
/// their environment says otherwise.
///
/// Whether an unset value should enforce, as documents-api-go does, is an open owner decision
/// (fail closed, or keep observing). Until the owner decides, unset observes here, and
/// `unset_authctx_enforce_observes_in_the_config_reader` pins that. Flipping the default is a
/// behaviour change and needs that decision first.
fn enforce_from(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::to_lowercase).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
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
                    // Observe mode: log, then decode without a check for telemetry. The claims are
                    // attached as an unverified context, so `verified_claims()` returns nothing for them.
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
    if !trimmed
        .get(..7)
        .is_some_and(|p| p.eq_ignore_ascii_case("Bearer "))
    {
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
    serde_json::from_slice::<Claims>(&payload).map_err(|e| AuthCtxError::Decode(e.to_string()))
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

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::future::{ready, Ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::{header, Request, Response, StatusCode};
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use tower::{Layer, Service};

    use super::{enforce_from, AuthCtxLayer, Config};
    use crate::{AuthCtx, Verification};

    #[test]
    fn authctx_enforce_enforces_on_the_four_words_in_any_case() {
        for raw in ["1", "true", "TRUE", "True", "yes", "YES", "on", "On"] {
            assert!(enforce_from(Some(raw)), "{raw:?} must enforce");
        }
    }

    /// Everything else observes here: unset, empty, the off words, padded forms and any other word.
    /// documents-api-go enforces for unset, padded and unrecognised values, and that difference is
    /// pinned here so it cannot close by accident.
    #[test]
    fn authctx_enforce_observes_for_everything_else() {
        for raw in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("no"),
            Some("off"),
            Some(" 1 "),
            Some(" true"),
            Some("true "),
            Some("enforce"),
            Some("unexpected"),
        ] {
            assert!(!enforce_from(raw), "{raw:?} must observe");
        }
    }

    /// The Model Plane default, read through the same body `Config::from_env` runs. With
    /// `AUTHCTX_ENFORCE` unset the layer observes, where documents-api-go enforces. That is the
    /// open owner decision on `enforce_from`, so this pin changes only together with that decision.
    #[test]
    fn unset_authctx_enforce_observes_in_the_config_reader() {
        let cfg = Config::from_lookup("model-plane", |_| None);
        assert!(!cfg.enforce);
    }

    #[test]
    fn authctx_enforce_one_enables_enforce_in_the_config_reader() {
        let cfg = Config::from_lookup("model-plane", |key| {
            (key == "AUTHCTX_ENFORCE").then(|| "1".to_owned())
        });
        assert!(cfg.enforce);
    }

    /// A handler stands in for the inner service. It records the `AuthCtx` each request carried, so
    /// a test sees what a handler would read.
    #[derive(Clone)]
    struct Handler {
        seen: Arc<Mutex<Vec<Option<AuthCtx>>>>,
    }

    impl Handler {
        fn new() -> Self {
            Self {
                seen: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn seen(&self) -> Vec<Option<AuthCtx>> {
            self.seen.lock().expect("handler lock").clone()
        }
    }

    impl Service<Request<Body>> for Handler {
        type Response = Response<Body>;
        type Error = Infallible;
        type Future = Ready<Result<Response<Body>, Infallible>>;

        fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, req: Request<Body>) -> Self::Future {
            let ctx = req.extensions().get::<AuthCtx>().cloned();
            self.seen.lock().expect("handler lock").push(ctx);
            ready(Ok(Response::new(Body::empty())))
        }
    }

    fn config(enforce: bool) -> Config {
        Config {
            audience: "model-plane".to_owned(),
            expected_issuer: "https://auth.fixture.invalid/api/convex-auth".to_owned(),
            // Never fetched: the fixture token has no `kid`, so verification refuses it before the
            // JWKS lookup, and the TTL never applies.
            jwks_url: "http://127.0.0.1:9/jwks".to_owned(),
            jwks_ttl: Duration::from_secs(1),
            enforce,
        }
    }

    /// A JWT with no `kid` in its header and a well-formed payload. Verification refuses it before
    /// any key lookup, while observe mode can still decode the payload. The signature is a fixture
    /// word and checks nothing.
    fn token_without_kid() -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(
            r#"{"user_id":"user-fixture","org_id":"org-fixture","sub":"user-fixture","iss":"https://auth.fixture.invalid/api/convex-auth","aud":"model-plane","iat":1,"exp":4102444800}"#,
        );
        format!("{header}.{payload}.fixture-signature")
    }

    fn request_with_bearer(token: &str) -> Request<Body> {
        Request::builder()
            .uri("/probe")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("fixture request is well formed")
    }

    async fn drive(layer: &AuthCtxLayer, handler: Handler, req: Request<Body>) -> Response<Body> {
        let mut service = layer.layer(handler);
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("the middleware is ready");
        service
            .call(req)
            .await
            .expect("the probe handler is infallible")
    }

    /// Observe mode attaches the claims of a token that failed verification, marked unverified. A
    /// handler that asks for verified claims gets none, so the forged org and user are unreachable
    /// through the verified path.
    #[tokio::test]
    async fn observe_attaches_a_failed_token_as_unverified_and_withholds_verified_claims() {
        let handler = Handler::new();
        let layer = AuthCtxLayer::new(config(false));
        let response = drive(
            &layer,
            handler.clone(),
            request_with_bearer(&token_without_kid()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let seen = handler.seen();
        assert_eq!(seen.len(), 1, "the handler ran once");
        let ctx = seen[0]
            .as_ref()
            .expect("observe mode attaches the unverified context");
        assert_eq!(ctx.verification(), Verification::Unverified);
        assert!(
            ctx.verified_claims().is_none(),
            "a handler must not read an unverified context as verified"
        );
        let claims = ctx
            .unverified_claims()
            .expect("the observe context carries the unverified claims");
        assert_eq!(claims.org_id, "org-fixture");
    }

    /// Enforce mode is unchanged: the same token is refused with 401 before the handler runs, so no
    /// unverified context reaches a handler in enforce mode.
    #[tokio::test]
    async fn enforce_refuses_the_same_token_before_the_handler_runs() {
        let handler = Handler::new();
        let layer = AuthCtxLayer::new(config(true));
        let response = drive(
            &layer,
            handler.clone(),
            request_with_bearer(&token_without_kid()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(
            handler.seen().is_empty(),
            "a refused request never reaches the handler"
        );
    }

    /// Without a bearer token, observe mode attaches no context at all, so nothing a handler reads
    /// can be mistaken for a verified one.
    #[tokio::test]
    async fn observe_without_a_token_attaches_no_context() {
        let handler = Handler::new();
        let layer = AuthCtxLayer::new(config(false));
        let req = Request::builder()
            .uri("/probe")
            .body(Body::empty())
            .expect("fixture request is well formed");
        let response = drive(&layer, handler.clone(), req).await;
        assert_eq!(response.status(), StatusCode::OK);
        let seen = handler.seen();
        assert_eq!(seen.len(), 1, "the handler ran once");
        assert!(seen[0].is_none(), "no token, no context");
    }
}
