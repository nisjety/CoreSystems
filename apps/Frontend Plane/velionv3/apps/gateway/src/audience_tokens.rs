use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Deserialize;
use tokio::sync::Mutex;

use crate::config::AppState;

pub(crate) type AudienceTokenCache = Arc<Mutex<HashMap<String, CachedToken>>>;

pub(crate) struct CachedToken {
    pub(crate) token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenResponse {
    token: String,
    expires_in: Option<u64>,
}

pub(crate) fn new_audience_token_cache() -> AudienceTokenCache {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Mint a short-lived plane token via auth-core, caching it until near-expiry.
/// Tokens never leave gateway memory — they are injected as Bearer on upstream requests only.
pub(crate) async fn get_audience_token(
    state: &AppState,
    user_id: &str,
    cookie_header: &str,
    audience: &str,
) -> Option<String> {
    let cache_key = format!("{}:{}", user_id, audience);

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
    let expires_in = token_resp.expires_in.unwrap_or(3600);
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
