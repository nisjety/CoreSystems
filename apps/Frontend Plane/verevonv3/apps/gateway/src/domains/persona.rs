//! `/api/v1/me/persona` — the user layer of ADR-0003's authored-instruction
//! hierarchy (chat-wave2 persona spec, `plans/system-audits/
//! chat-wave2-specs-2026-09-28/persona.md`).
//!
//! Storage is Convex (`users.personaInstructions`/`personaEnabled`), the same
//! backend `orgs/instructions.rs` uses for the org layer — mirrors that
//! module end to end, but self-service: a signed-in caller only ever reads
//! or writes their own row (`user.user_id`), so unlike the org route there is
//! no admin/membership gate, just `require_session`.
//!
//! The per-thread override (session-core's `threads.persona_enabled`) is a
//! separate surface, reached through `chat/history.rs`'s thread-presentation
//! endpoints — this route only owns the global on/off switch and the text.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState, contracts::ActionActor, envelope::error, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::spaces::convex_gateway_call;

/// Deliberately smaller than `MAX_ORG_INSTRUCTIONS_LENGTH` (4000): a persona
/// is meant to be a short style/tone preference, not a policy document, and
/// `authored_instructions_message` does no runtime token trimming for this
/// message class — the write-time cap is the only safety net.
const MAX_PERSONA_INSTRUCTIONS_LENGTH: usize = 1500;

type GatewayJsonResponse = (StatusCode, Json<Value>);

pub(crate) fn router(state: AppState) -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/v1/me/persona",
            axum::routing::get(get_persona).patch(update_persona),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            crate::middleware::require_session,
        ))
}

async fn get_persona(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> GatewayJsonResponse {
    let (instructions, enabled) = fetch_persona(&state, &user).await;
    (
        StatusCode::OK,
        Json(json!({"data": {"instructions": instructions, "enabled": enabled}})),
    )
}

async fn fetch_persona(state: &AppState, user: &AuthenticatedUser) -> (Option<String>, bool) {
    let Ok(value) = convex_gateway_call(
        state,
        "query",
        "users:personaForGateway",
        json!({ "externalAuthId": user.user_id }),
    )
    .await
    else {
        return (None, true);
    };
    let instructions = value
        .get("instructions")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let enabled = value.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    (instructions, enabled)
}

#[derive(Deserialize)]
pub(crate) struct UpdatePersonaRequest {
    #[serde(default)]
    pub(crate) instructions: Option<String>,
    #[serde(default)]
    pub(crate) enabled: Option<bool>,
}

async fn update_persona(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<UpdatePersonaRequest>,
) -> GatewayJsonResponse {
    let instructions = body
        .instructions
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // Character count, not byte length — same Norwegian æ/ø/å rationale as
    // `orgs/instructions.rs::update_org_instructions`.
    if instructions.is_some_and(|value| value.chars().count() > MAX_PERSONA_INSTRUCTIONS_LENGTH) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "persona_instructions_too_long",
                "Persona instructions must be 1500 characters or fewer.",
            )),
        );
    }
    let mut args = json!({ "externalAuthId": user.user_id });
    let object = args.as_object_mut().expect("json! object literal");
    if body.instructions.is_some() {
        object.insert("instructions".to_owned(), json!(instructions));
    }
    if let Some(enabled) = body.enabled {
        object.insert("enabled".to_owned(), json!(enabled));
    }
    let result = convex_gateway_call(&state, "mutation", "users:setPersonaForGateway", args).await;
    match result {
        Ok(value) => (StatusCode::OK, Json(json!({"data": value}))),
        Err(()) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "persona_unavailable",
                "Persona instructions could not be saved.",
            )),
        ),
    }
}

/// The reply tones a person can pick (EP-8-S3). This is the closed set user-core validates on save;
/// the gateway checks it again before a value goes on to model-gateway.
const TONE_PRESETS: [&str; 6] = [
    "default",
    "professional",
    "friendly",
    "direct",
    "warm",
    "neutral",
];

/// How long one person's tone is reused before user-core is asked again (EP-8-S3). A tone is a style
/// setting, so a minute between a change and its first reply is acceptable.
const TONE_PRESET_TTL: Duration = Duration::from_secs(60);

/// The last read of each person's tone, keyed by user-core's base URL and the user id, with the instant
/// it was read. A failed read is kept as well, so a user-core outage costs one slow read a minute per
/// person rather than one per turn. The URL keeps two deployments' reads apart.
static TONE_PRESET_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Option<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether `value` is one of the closed tone names, `default` included.
pub(crate) fn is_tone_preset(value: &str) -> bool {
    TONE_PRESETS.contains(&value)
}

/// The tone a turn carries as `persona_tone` (EP-8-S3), or `None` for `default`, for an unknown value,
/// and whenever user-core cannot be read. Reading nothing leaves a turn exactly as it was before tones
/// existed, and a tone grants nothing, so failing open is safe. Read at most once a minute per person.
pub(crate) async fn fetch_tone_preset(state: &AppState, user: &AuthenticatedUser) -> Option<String> {
    let key = format!("{}|{}", state.user_core_url, user.user_id);
    let now = Instant::now();
    if let Some(cached) = cached_tone_preset(&key, now) {
        return cached;
    }
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let url = format!("{}/api/v1/settings/ai", state.user_core_url);
    let (status, Json(settings)) =
        proxy_json(state, Method::GET, &url, None, None, Some(&actor), None).await;
    let tone = if status.is_success() {
        settings
            .get("tonePreset")
            .and_then(Value::as_str)
            .filter(|tone| is_tone_preset(tone) && *tone != "default")
            .map(str::to_owned)
    } else {
        None
    };
    store_tone_preset(key, now, tone.clone());
    tone
}

fn cached_tone_preset(key: &str, now: Instant) -> Option<Option<String>> {
    let cache = TONE_PRESET_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    cache
        .get(key)
        .filter(|(read_at, _)| now.duration_since(*read_at) < TONE_PRESET_TTL)
        .map(|(_, tone)| tone.clone())
}

fn store_tone_preset(key: String, now: Instant, tone: Option<String>) {
    let mut cache = TONE_PRESET_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // A miss already cost a round trip, so this is the moment to drop the reads that have expired.
    cache.retain(|_, (read_at, _)| now.duration_since(*read_at) < TONE_PRESET_TTL);
    cache.insert(key, (now, tone));
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_partial_json as wm_body_partial_json, method as wm_method, path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::fetch_tone_preset;

    /// Same harness shape as `orgs/instructions.rs`'s test module: mocked
    /// auth-core/user-core session + Convex, real router via `build_router`.
    /// No `org_role` parameter here — persona has no admin gate, so unlike
    /// the org-instructions harness this never varies the session-context
    /// role.
    async fn persona_response(
        method: &str,
        write_body: Option<Value>,
        read_value: Value,
        write_value: Value,
    ) -> (u16, Value) {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role": "member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .and(wm_body_partial_json(
                json!({"path": "users:personaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": read_value })))
            .mount(&application)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/mutation"))
            .and(wm_body_partial_json(
                json!({"path": "users:setPersonaForGateway"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "value": write_value })))
            .mount(&application)
            .await;
        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();

        let request = Request::builder()
            .method(method)
            .uri("/api/v1/me/persona")
            .header("cookie", "better-auth.session_token=session-1");
        let request = if let Some(body) = write_body {
            request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap()
        } else {
            request.body(Body::empty()).unwrap()
        };
        let response = crate::build_router(state).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    #[tokio::test]
    async fn a_user_can_read_their_own_persona() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = persona_response(
            "GET",
            None,
            json!({"instructions": "Be terse.", "enabled": true}),
            json!({}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "Be terse.");
        assert_eq!(body["data"]["enabled"], true);
    }

    #[tokio::test]
    async fn a_user_can_write_their_own_persona() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = persona_response(
            "PATCH",
            Some(json!({"instructions": "Be terse.", "enabled": true})),
            json!({"instructions": null, "enabled": true}),
            json!({"instructions": "Be terse.", "enabled": true}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "Be terse.");
        assert_eq!(body["data"]["enabled"], true);
    }

    #[tokio::test]
    async fn overlong_persona_instructions_are_rejected_before_reaching_convex() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = persona_response(
            "PATCH",
            Some(json!({"instructions": "x".repeat(1501)})),
            json!({"instructions": null, "enabled": true}),
            json!({}),
        )
        .await;
        assert_eq!(status, 400, "{body}");
    }

    /// 'æ' is 2 bytes in UTF-8. 1500 of them is exactly at the character
    /// limit but 3000 bytes — a byte-length check would wrongly reject this
    /// valid Norwegian text.
    #[tokio::test]
    async fn four_hundred_norwegian_characters_are_accepted() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, _body) = persona_response(
            "PATCH",
            Some(json!({"instructions": "æ".repeat(1500)})),
            json!({"instructions": null, "enabled": true}),
            json!({"instructions": "æ".repeat(1500), "enabled": true}),
        )
        .await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn enabled_false_is_persisted_and_read_back() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let (status, body) = persona_response(
            "PATCH",
            Some(json!({"enabled": false})),
            json!({"instructions": "Be terse.", "enabled": true}),
            json!({"instructions": "Be terse.", "enabled": false}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["enabled"], false);
    }

    #[tokio::test]
    async fn omitting_instructions_leaves_the_saved_text_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // PATCH with only `{enabled}` — the mutation call must not carry an
        // `instructions` key at all, matching `setPersonaForGateway`'s
        // "omit = leave unchanged" contract on the Convex side.
        let (status, body) = persona_response(
            "PATCH",
            Some(json!({"enabled": false})),
            json!({"instructions": "Existing text.", "enabled": true}),
            json!({"instructions": "Existing text.", "enabled": false}),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["instructions"], "Existing text.");
    }

    fn tone_test_user(user_id: &str) -> crate::middleware::AuthenticatedUser {
        crate::middleware::AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: "tone@example.com".to_owned(),
            user_name: "Tone User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        }
    }

    async fn tone_user_core(answer: ResponseTemplate, expected_reads: Option<u64>) -> MockServer {
        let user_core = MockServer::start().await;
        let mock = Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/settings/ai"))
            .respond_with(answer);
        let mock = match expected_reads {
            Some(times) => mock.expect(times),
            None => mock,
        };
        mock.mount(&user_core).await;
        user_core
    }

    /// Two turns inside a minute cost one read of user-core: the second is served from the cache.
    #[tokio::test]
    async fn tone_preset_is_read_once_a_minute_per_person() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user_core = tone_user_core(
            ResponseTemplate::new(200).set_body_json(json!({"tonePreset": "warm"})),
            Some(1),
        )
        .await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let user = tone_test_user("user-tone-cached");

        assert_eq!(
            fetch_tone_preset(&state, &user).await.as_deref(),
            Some("warm")
        );
        assert_eq!(
            fetch_tone_preset(&state, &user).await.as_deref(),
            Some("warm")
        );
    }

    /// Only a named tone is sent: `default`, an unknown name and a missing field all send nothing.
    #[tokio::test]
    async fn only_a_named_tone_is_sent() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            ("user-tone-default", json!({"tonePreset": "default"}), None),
            ("user-tone-unknown", json!({"tonePreset": "formal"}), None),
            ("user-tone-missing", json!({"aiEnabled": true}), None),
            (
                "user-tone-direct",
                json!({"tonePreset": "direct"}),
                Some("direct"),
            ),
        ];
        for (user_id, answer, expected) in cases {
            let user_core =
                tone_user_core(ResponseTemplate::new(200).set_body_json(answer), None).await;
            let mut state = crate::tests::test_state(false);
            state.user_core_url = user_core.uri();
            assert_eq!(
                fetch_tone_preset(&state, &tone_test_user(user_id))
                    .await
                    .as_deref(),
                expected,
                "{user_id}"
            );
        }
    }

    /// An unreadable user-core adds no tone, and the failed read is kept for the minute like a good one.
    #[tokio::test]
    async fn unreadable_user_core_sends_no_tone() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let user_core = tone_user_core(ResponseTemplate::new(503), Some(1)).await;
        let mut state = crate::tests::test_state(false);
        state.user_core_url = user_core.uri();
        let user = tone_test_user("user-tone-outage");

        assert_eq!(fetch_tone_preset(&state, &user).await, None);
        assert_eq!(fetch_tone_preset(&state, &user).await, None);
    }
}
