use tonic::{service::Interceptor, Request, Status};

#[derive(Clone)]
pub struct ApiKeyInterceptor {
    expected_key: Option<String>,
}

impl ApiKeyInterceptor {
    pub fn new(key: Option<String>) -> Self {
        Self {
            expected_key: key.filter(|k| !k.is_empty()),
        }
    }
}

impl Interceptor for ApiKeyInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        // Path 1: INTERNAL_API_KEY (existing v2.x behavior).
        let api_key_ok = match self.expected_key.as_ref() {
            Some(expected) => {
                let provided = req
                    .metadata()
                    .get("x-api-key")
                    .or_else(|| req.metadata().get("x-internal-api-key"))
                    .or_else(|| req.metadata().get("x-internal-key"))
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                provided == expected
            }
            None => true, // No key configured → API-key path not enforced
        };
        if api_key_ok {
            record_trace_context(&req);
            return Ok(req);
        }

        // Path 2: Wave-3.1 §16.1.2 — Bearer JWT alternative.
        // Same RS256 + JWT_PUBLIC_KEY_PEM as the HTTP middleware; we just
        // accept the credential here. Population of AuthContext into the
        // request extensions for gRPC is wave-3.2 (Tonic's `extensions()` is
        // not a request-scoped Axum-style map — we'd need a Tower layer).
        if let Some(token) = bearer_token(&req) {
            if verify_jwt_sync(&token).is_ok() {
                record_trace_context(&req);
                return Ok(req);
            }
        }
        Err(Status::unauthenticated("invalid or missing credential"))
    }
}

fn bearer_token(req: &Request<()>) -> Option<String> {
    let value = req
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())?;
    value.strip_prefix("Bearer ").map(|s| s.to_string())
}

/// Synchronous JWT verification — interceptor runs on Tonic's gRPC thread
/// and cannot await. PEM is loaded once via env; no JWKS fetch (deferred).
fn verify_jwt_sync(token: &str) -> Result<(), ()> {
    let pem = std::env::var("JWT_PUBLIC_KEY_PEM")
        .ok()
        .filter(|p| !p.is_empty())
        .ok_or(())?;
    let key = jsonwebtoken::DecodingKey::from_rsa_pem(pem.as_bytes()).map_err(|_| ())?;
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.validate_exp = true;
    if let Ok(iss) = std::env::var("JWT_REQUIRED_ISSUER") {
        validation.set_issuer(&[iss]);
    }
    if let Ok(aud) = std::env::var("JWT_REQUIRED_AUDIENCE") {
        validation.set_audience(&[aud]);
    }
    jsonwebtoken::decode::<serde_json::Value>(token, &key, &validation)
        .map(|_| ())
        .map_err(|_| ())
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
