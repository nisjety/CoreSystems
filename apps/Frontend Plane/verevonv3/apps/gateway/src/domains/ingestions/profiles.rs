//! Browser profiles — `GET /api/ingestions/profiles`.
//!
//! Port of verevonv2's `app/api/ingestions/profiles/route.ts`: list quarry
//! browser profiles, then probe the first few for restore capability, mapping
//! each into the SPA's `ProfilePayload`.

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::Response,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, envelope::unwrap_data, middleware::AuthenticatedUser};

use super::shared::{cookie_header, forward, okay, quarry_call, quarry_token};

/// Cap restore probes — each is a quarry round-trip and the picker only needs a few.
const MAX_PROFILE_PROBES: usize = 10;

pub(super) async fn list_profiles(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let (status, body) = quarry_call(
        &state,
        Method::GET,
        "/v1/profiles",
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }

    let data = unwrap_data(&body);
    let profile_ids: Vec<String> = data
        .get("profiles")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();

    let state_ref = &state;
    let token_ref = token.as_deref();
    let user_id = user.user_id.as_str();

    let profiles = join_all(profile_ids.iter().take(MAX_PROFILE_PROBES).map(|id| {
        let id = id.clone();
        async move {
            let path = format!("/v1/profiles/{}/restore_probe", urlencoding::encode(&id));
            let (status, body) = quarry_call(
                state_ref,
                Method::POST,
                &path,
                Some(json!({ "url": "https://example.com" })),
                token_ref,
                user_id,
            )
            .await;
            let probe = status.is_success().then(|| unwrap_data(&body));
            to_profile(&id, probe.as_ref())
        }
    }))
    .await;

    okay(json!({ "profiles": profiles }))
}

/// Map a quarry `restore_probe` into the SPA's profile shape, summing the three
/// storage buckets into a single `storage` count (v2 parity).
fn to_profile(id: &str, probe: Option<&Value>) -> Value {
    let int_at = |key: &str| -> i64 {
        probe
            .and_then(|p| p.get(key))
            .and_then(Value::as_i64)
            .unwrap_or(0)
    };
    let str_or_null = |key: &str| -> Value {
        probe
            .and_then(|p| p.get(key))
            .filter(|v| v.is_string())
            .cloned()
            .unwrap_or(Value::Null)
    };

    json!({
        "id": id,
        "restorable": probe.and_then(|p| p.get("restorable")).and_then(Value::as_bool).unwrap_or(false),
        "cookies": int_at("cookies_count"),
        "storage": int_at("local_storage_count") + int_at("session_storage_count") + int_at("indexed_db_count"),
        "locale": str_or_null("locale"),
        "timezone": str_or_null("timezone"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_profile_sums_storage_and_defaults_missing() {
        let probe = json!({
            "restorable": true,
            "cookies_count": 4,
            "local_storage_count": 2,
            "session_storage_count": 1,
            "indexed_db_count": 3,
            "locale": "nb-NO"
        });
        let profile = to_profile("p1", Some(&probe));
        assert_eq!(profile["id"], "p1");
        assert_eq!(profile["restorable"], true);
        assert_eq!(profile["cookies"], 4);
        assert_eq!(profile["storage"], 6);
        assert_eq!(profile["locale"], "nb-NO");
        assert_eq!(profile["timezone"], Value::Null);
    }

    #[test]
    fn to_profile_handles_absent_probe() {
        let profile = to_profile("p2", None);
        assert_eq!(profile["restorable"], false);
        assert_eq!(profile["cookies"], 0);
        assert_eq!(profile["storage"], 0);
        assert_eq!(profile["locale"], Value::Null);
    }
}
