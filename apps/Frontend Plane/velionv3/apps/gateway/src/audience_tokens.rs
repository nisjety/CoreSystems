use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::AppState;

/// Closed Auth Core audience contract for hardened Model Plane services.
/// `ModelGateway` preserves the compatibility route `/api/model-plane/token`,
/// while the token it returns is still strictly `aud=model-gateway`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
// The closed contract deliberately includes audiences whose routes are not yet
// invoked by this binary. Keeping them here prevents ad-hoc audience strings.
#[allow(dead_code)]
pub(crate) enum ModelServiceAudience {
    ModelGateway,
    SessionCore,
    InferenceCore,
    ExecutionCore,
    CostCore,
    CapabilityCore,
    LettaBridge,
    BrowserBroker,
    SandboxManager,
    BridgeCore,
}

impl ModelServiceAudience {
    pub(crate) const fn claim(self) -> &'static str {
        match self {
            Self::ModelGateway => "model-gateway",
            Self::SessionCore => "session-core",
            Self::InferenceCore => "inference-core",
            Self::ExecutionCore => "execution-core",
            Self::CostCore => "cost-core",
            Self::CapabilityCore => "capability-core",
            Self::LettaBridge => "letta-bridge",
            Self::BrowserBroker => "browser-broker",
            Self::SandboxManager => "sandbox-manager",
            Self::BridgeCore => "bridge-core",
        }
    }

    const fn issuance_slug(self) -> &'static str {
        match self {
            Self::ModelGateway => "model-plane",
            other => other.claim(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RequiredAudienceTokenError {
    pub(crate) audience: ModelServiceAudience,
}

pub(crate) type AudienceTokenCache = Arc<Mutex<HashMap<String, CachedToken>>>;

pub(crate) struct CachedToken {
    pub(crate) token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    // auth-core's plane-token endpoint reports the TTL as `expiresInSeconds`
    // (the model-plane endpoint historically used `expiresIn`); accept both.
    // Without the alias this is None → the cache fell back to a 3600s default
    // and kept serving 5-minute plane tokens long after they expired — which
    // the edge's real JWT `exp` verification (dev-bypass off) correctly rejects.
    #[serde(alias = "expiresInSeconds")]
    expires_in: Option<u64>,
}

pub(crate) fn new_audience_token_cache() -> AudienceTokenCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Mint a short-lived, active-organization-bound plane token via auth-core.
/// Tokens never leave gateway memory — they are injected as Bearer on upstream requests only.
pub(crate) async fn get_audience_token(
    state: &AppState,
    user_id: &str,
    cookie_header: &str,
    audience: &str,
) -> Option<String> {
    // Session-bound tokens carry the active organization. A cache keyed only
    // by user+audience can replay an old-organization token immediately after
    // an org switch, while the Better Auth cookie itself may be unchanged.
    // Mint on every request until the cache key includes an authoritative
    // session generation + active org returned by auth-core.
    let _ = user_id;

    let resp = state
        .client
        .get(format!("{}/api/{}/token", state.auth_core_url, audience))
        .header("cookie", cookie_header)
        .header("x-internal-api-key", &state.internal_api_key)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let token_resp = resp.json::<TokenResponse>().await.ok()?;
    // Safe default: plane tokens are short-lived (auth-core default 300s), so a
    // missing TTL must NOT fall back to an hour of caching past a 5-minute exp.
    Some(token_resp.token)
}

pub(crate) async fn get_model_service_token(
    state: &AppState,
    user_id: &str,
    cookie_header: &str,
    audience: ModelServiceAudience,
) -> Option<String> {
    get_audience_token(state, user_id, cookie_header, audience.issuance_slug()).await
}

pub(crate) async fn require_model_service_token(
    state: &AppState,
    user_id: &str,
    cookie_header: &str,
    audience: ModelServiceAudience,
) -> Result<String, RequiredAudienceTokenError> {
    get_model_service_token(state, user_id, cookie_header, audience)
        .await
        .ok_or(RequiredAudienceTokenError { audience })
}

/// Mint the pre-org onboarding preview token (quarry audience, sentinel org).
/// The onboarding website step runs before an organization exists, so the
/// normal org-scoped `get_audience_token(..., "quarry")` 400s. This hits
/// auth-core's dedicated `/api/quarry/onboarding-token`, which requires only a
/// session. Cached per user under a distinct key so it never collides with the
/// real org-scoped quarry token the user gets after creating an org.
pub(crate) async fn get_onboarding_preview_token(
    state: &AppState,
    user_id: &str,
    cookie_header: &str,
) -> Option<String> {
    let cache_key = format!("{}:quarry-onboarding", user_id);

    {
        let cache = state.audience_token_cache.lock().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.expires_at > Instant::now() {
                return Some(entry.token.clone());
            }
        }
    }

    let resp = state
        .client
        .get(format!(
            "{}/api/quarry/onboarding-token",
            state.auth_core_url
        ))
        .header("cookie", cookie_header)
        .header("x-internal-api-key", &state.internal_api_key)
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let token_resp = resp.json::<TokenResponse>().await.ok()?;
    let expires_in = token_resp.expires_in.unwrap_or(300).max(120);
    let ttl = Duration::from_secs(expires_in.saturating_sub(60));
    let token = token_resp.token.clone();

    {
        let mut cache = state.audience_token_cache.lock().await;
        cache.insert(
            cache_key,
            CachedToken {
                token: token.clone(),
                expires_at: Instant::now() + ttl,
            },
        );
    }

    Some(token)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, time::Duration};

    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use super::{
        get_audience_token, get_model_service_token, get_onboarding_preview_token,
        require_model_service_token, CachedToken, ModelServiceAudience,
    };

    struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

    impl EnvRestore {
        fn set(values: &[(&'static str, &str)]) -> Self {
            let previous = values
                .iter()
                .map(|(name, value)| {
                    let old = std::env::var_os(name);
                    std::env::set_var(name, value);
                    (*name, old)
                })
                .collect();
            Self(previous)
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..).rev() {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn model_service_audiences_are_exact_and_gateway_keeps_compatibility_route() {
        let audiences = [
            ModelServiceAudience::ModelGateway,
            ModelServiceAudience::SessionCore,
            ModelServiceAudience::InferenceCore,
            ModelServiceAudience::ExecutionCore,
            ModelServiceAudience::CostCore,
            ModelServiceAudience::CapabilityCore,
            ModelServiceAudience::LettaBridge,
            ModelServiceAudience::BrowserBroker,
            ModelServiceAudience::SandboxManager,
            ModelServiceAudience::BridgeCore,
        ];
        assert_eq!(audiences[0].claim(), "model-gateway");
        assert_eq!(audiences[0].issuance_slug(), "model-plane");
        for audience in &audiences[1..] {
            assert_eq!(audience.claim(), audience.issuance_slug());
        }
    }

    #[tokio::test]
    async fn audience_tokens_use_the_auth_session_fail_closed_and_cache_only_onboarding() {
        let _env_lock = crate::config::TEST_ENV_LOCK.lock().await;
        let auth_core = MockServer::start().await;
        let _env = EnvRestore::set(&[
            ("APP_ENV", "development"),
            ("AUTH_CORE_URL", &auth_core.uri()),
            ("VELION_PUBLIC_ORIGIN", "http://localhost:5173"),
            ("INTERNAL_API_KEY", "legacy-internal-key-for-gateway-tests"),
            (
                "SESSION_CORE_SERVICE_TOKEN",
                "session-0123456789abcdef0123456789abcdef",
            ),
            (
                "USER_CORE_SERVICE_TOKEN",
                "usercore-0123456789abcdef0123456789abcdef",
            ),
            (
                "BILLING_CORE_SERVICE_TOKEN",
                "billing-0123456789abcdef0123456789abcdef",
            ),
            (
                "ORG_CORE_SERVICE_TOKEN",
                "orgcore-0123456789abcdef0123456789abcdef",
            ),
            (
                "AUDIT_CORE_SERVICE_TOKEN",
                "auditcore-0123456789abcdef0123456789abcdef",
            ),
            (
                "NOTIFICATION_CORE_SERVICE_TOKEN",
                "notify-0123456789abcdef0123456789abcdef",
            ),
            (
                "CONVERSATION_CORE_SERVICE_TOKEN",
                "converse-0123456789abcdef0123456789abcdef",
            ),
        ]);
        let state = crate::config::build_state()
            .await
            .expect("test gateway state");

        Mock::given(method("GET"))
            .and(path("/api/model-plane/token"))
            .and(header("cookie", "better-auth.session=verified"))
            .and(header(
                "x-internal-api-key",
                "legacy-internal-key-for-gateway-tests",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "model-token",
                "expiresInSeconds": 300,
            })))
            .expect(1)
            .mount(&auth_core)
            .await;
        assert_eq!(
            get_model_service_token(
                &state,
                "user-1",
                "better-auth.session=verified",
                ModelServiceAudience::ModelGateway,
            )
            .await
            .as_deref(),
            Some("model-token")
        );

        Mock::given(method("GET"))
            .and(path("/api/session-core/token"))
            .respond_with(ResponseTemplate::new(503))
            .expect(2)
            .mount(&auth_core)
            .await;
        assert!(get_audience_token(
            &state,
            "user-1",
            "better-auth.session=verified",
            "session-core",
        )
        .await
        .is_none());
        let required = require_model_service_token(
            &state,
            "user-1",
            "better-auth.session=verified",
            ModelServiceAudience::SessionCore,
        )
        .await
        .expect_err("non-success issuance must fail closed");
        assert_eq!(required.audience, ModelServiceAudience::SessionCore);

        state.audience_token_cache.lock().await.insert(
            "user-1:quarry-onboarding".to_owned(),
            CachedToken {
                token: "expired-token".to_owned(),
                expires_at: std::time::Instant::now() - Duration::from_secs(1),
            },
        );
        Mock::given(method("GET"))
            .and(path("/api/quarry/onboarding-token"))
            .and(header("cookie", "better-auth.session=verified"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "onboarding-token",
                "expiresInSeconds": 120,
            })))
            .expect(1)
            .mount(&auth_core)
            .await;
        for _ in 0..2 {
            assert_eq!(
                get_onboarding_preview_token(&state, "user-1", "better-auth.session=verified",)
                    .await
                    .as_deref(),
                Some("onboarding-token")
            );
        }
    }
}
