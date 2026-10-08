use axum::{
    extract::{Extension, Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, domains::chat::shared as model_plane, envelope::error,
    middleware::AuthenticatedUser, upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

/// The per-model feature flag inference-core adds to `/v1/models` for a
/// deployment whose zero-retention contract an operator has attested
/// (`ProviderCapabilities::feature_flags`, derived from `supports_zdr`). The
/// composer reads the same string (`ZERO_RETENTION_CAPABILITY` in
/// `src/shared/api/chat-client.ts`).
const ZDR_FEATURE: &str = "zdr";

/// Privacy tiers that exist only for a provider with a verified zero-retention
/// contract: inference-core's `PrivacyTier::classify` returns `zdr_contractual`
/// or `sovereign` only when `supports_zdr` holds. A second signal for the same
/// bit, because the Anthropic provider's model list hardcodes its features and
/// never lists `zdr` even when attested, while its tier is derived correctly.
const ZDR_TIERS: [&str; 2] = ["zdr_contractual", "sovereign"];

/// True when a `/v1/models` entry is a CHAT model whose provider carries a
/// recorded zero-retention attestation. A non-chat model (an image or
/// embedding deployment) can carry the flag but cannot serve a ZDR chat turn,
/// so it never opens the gate. Exact string matches on purpose: the wire is
/// lower-case snake_case, and a lookalike (`zdr_ready`, `ZDR`) is not an
/// attestation.
fn model_attests_zdr_for_chat(model: &Value) -> bool {
    if model.get("modality").and_then(Value::as_str) != Some("chat") {
        return false;
    }
    let flagged = model
        .get("features")
        .and_then(Value::as_array)
        .is_some_and(|features| features.iter().any(|f| f.as_str() == Some(ZDR_FEATURE)));
    let tiered = model
        .get("privacy_tier")
        .and_then(Value::as_str)
        .is_some_and(|tier| ZDR_TIERS.contains(&tier));
    flagged || tiered
}

/// Whether the catalogue shows at least one attested chat model. `None` when
/// the body is not a catalogue at all (no `models` array): nothing is known
/// about attestation, which is different from knowing there is none. The
/// SPA's `hasZeroRetentionModel` answers the same question over the same
/// fixture (`tests/fixtures/zdr-model-catalogue-cases.json`), so the toggle,
/// the composer and this gate cannot disagree.
fn catalogue_attests_zdr_chat_model(catalogue: &Value) -> Option<bool> {
    let models = catalogue.get("models")?.as_array()?;
    Some(models.iter().any(model_attests_zdr_for_chat))
}

enum ZdrSwitchGate {
    Open,
    NoAttestedProvider,
    Unavailable,
}

/// Read the model catalogue as the acting admin and decide whether the org ZDR
/// switch may be turned ON.
///
/// The catalogue is deployment-level (providers are configured per deployment,
/// not per org), so any admin's credentials answer it. The credentials are the
/// ones the model selector's own route uses: the `aud=model-gateway` bearer
/// plus the separately minted `aud=inference-core` bearer that model-gateway's
/// catalogue route requires.
///
/// Fails closed: a catalogue that cannot be read is `Unavailable`, never
/// treated as attested and never as "none attested".
async fn zdr_switch_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> ZdrSwitchGate {
    let (model_token, inference_token) = tokio::join!(
        model_plane::model_token(state, user, headers),
        model_plane::required_inference_token(state, user, headers),
    );
    let (Some(model_token), Ok(inference_token)) = (model_token, inference_token) else {
        tracing::warn!("org ZDR gate: the catalogue credentials could not be minted");
        return ZdrSwitchGate::Unavailable;
    };
    let url = format!("{}/v1/models", state.model_gateway_url);
    let (status, Json(catalogue)) = model_plane::proxy_model_json_with_inference(
        state,
        Method::GET,
        &url,
        None,
        Some(&model_token),
        Some(&inference_token),
        user,
    )
    .await;
    if !status.is_success() {
        tracing::warn!(
            status = status.as_u16(),
            "org ZDR gate: the model catalogue could not be read"
        );
        return ZdrSwitchGate::Unavailable;
    }
    match catalogue_attests_zdr_chat_model(&catalogue) {
        Some(true) => ZdrSwitchGate::Open,
        Some(false) => ZdrSwitchGate::NoAttestedProvider,
        None => {
            tracing::warn!("org ZDR gate: the model catalogue answer was not a catalogue");
            ZdrSwitchGate::Unavailable
        }
    }
}

/// The refusal to answer with when org ZDR may NOT be turned on right now, or
/// `None` when it may. Every route that can write `interactiveRetention.zdr =
/// true` into an organization calls this, so there is one rule and one pair of
/// codes: the settings route below, the agent action that dispatches to it, and
/// organization creation (the onboarding step carries the same switch).
///
/// `org_id` is `None` while the organization does not exist yet. That is the
/// usual first-time creator, and for them this gate can only ever answer 503
/// `zdr_gate_unavailable`: auth-core mints no Model Plane or Inference Core
/// credential for a session without an active organization (400 "No active
/// organisation on session"), so the catalogue cannot be read. It fails closed, as
/// intended, but it does not open when a provider is attested; ZDR is then switched
/// on in the settings, where it does. A pre-organization credential from the
/// Control Plane would lift this (docs/ZDR_OPERATOR_SEED_RUNBOOK.md, section 8).
pub(crate) async fn zdr_enable_refusal(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: Option<&str>,
) -> Option<GatewayJsonResponse> {
    match zdr_switch_gate(state, user, headers).await {
        ZdrSwitchGate::Open => None,
        ZdrSwitchGate::NoAttestedProvider => {
            tracing::info!(
                org_id = org_id.unwrap_or("(not created yet)"),
                "org ZDR switch refused: no provider carries a zero-retention attestation"
            );
            Some((
                StatusCode::CONFLICT,
                Json(error(
                    "zdr_no_attested_provider",
                    "Zero Data Retention cannot be turned on: no AI provider in this environment has a recorded zero-retention attestation, so every interactive request would be refused.",
                )),
            ))
        }
        ZdrSwitchGate::Unavailable => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "zdr_gate_unavailable",
                "Zero Data Retention cannot be turned on right now: the provider catalogue could not be checked. Try again shortly. Turning it off is always possible.",
            )),
        )),
    }
}

/// PATCH /api/v1/orgs/:id/settings — org-admin-gated organization settings.
///
/// Currently carries the interactive Zero-Data-Retention posture. The value is
/// persisted as durable organization intent in org-core; live token-issue
/// enforcement remains with auth-core's managed, attested retention policy.
/// This route deliberately never becomes a per-request ZDR override: it only
/// records the org's chosen posture, gated to org owners/admins.
///
/// Turning ZDR ON is also gated on an attested provider: with none, every
/// interactive request of that org would be refused (inference-core fails a ZDR
/// request closed), so the switch would only break the workspace. The gate is
/// in this gateway and nowhere else on purpose: this gateway is the only holder
/// of org-core's `org:settings:write:self`, and only it can read the Model Plane
/// catalogue. It covers every route here that can write `zdr = true`: this one,
/// the `org.update_zdr` agent action that dispatches to it, and organization
/// creation (`onboarding/actions/organization.rs`), whose onboarding step carries
/// the same switch. org-core itself stays ungated, which is the operator-only
/// seed path for a dedicated test org (docs/ZDR_OPERATOR_SEED_RUNBOOK.md).
/// Turning ZDR OFF is always allowed and never reads the catalogue.
pub(crate) async fn update_org_settings(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    let zdr = body.get("zeroDataRetention").and_then(Value::as_bool);
    let support_ai_mode = body.get("supportAiMode").and_then(Value::as_str);
    if (zdr.is_some() && support_ai_mode.is_some()) || (zdr.is_none() && support_ai_mode.is_none())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "validation_error",
                "provide exactly one of zeroDataRetention or supportAiMode.",
            )),
        );
    }
    if let Some(mode) = support_ai_mode {
        if !matches!(mode, "off" | "assist" | "review") {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "validation_error",
                    "supportAiMode must be off, assist, or review.",
                )),
            );
        }
    }
    if zdr == Some(true) {
        if let Some(refusal) = zdr_enable_refusal(&state, &user, &headers, Some(&id)).await {
            return refusal;
        }
    }
    let url = format!(
        "{}/api/v1/organizations/{}/settings",
        state.org_core_url,
        urlencoding::encode(&id)
    );
    proxy_json(
        &state,
        Method::PATCH,
        &url,
        Some(match (zdr, support_ai_mode) {
            (Some(zdr), None) => json!({ "zeroDataRetention": zdr }),
            (None, Some(mode)) => json!({ "supportAiMode": mode }),
            _ => unreachable!(),
        }),
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    //! The org ZDR switch is driven through the REAL router: the real session
    //! middleware (get-session + session-context decide the caller's org role),
    //! the real audience-token mint contract (cookie + internal key to
    //! auth-core, one token per audience), the real upstream credential
    //! headers (`Authorization` for model-gateway plus the separate
    //! `X-Inference-Authorization` its catalogue route requires) and org-core's
    //! delegated-service headers. Upstreams only answer when those are present,
    //! so a gate that forgot a credential would see the catalogue as down.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{
        body_json as wm_body_json, header as wm_header, method as wm_method, path as wm_path,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::AppState;

    const COOKIE: &str = "better-auth.session_token=session-1";
    const MODEL_TOKEN: &str = "model-gateway-token-for-user-1";
    const INFERENCE_TOKEN: &str = "inference-core-token-for-user-1";

    fn fixture_cases() -> Vec<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/zdr-model-catalogue-cases.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()));
        let fixture: Value = serde_json::from_str(&raw).expect("fixture is JSON");
        fixture["cases"]
            .as_array()
            .expect("fixture has a cases array")
            .clone()
    }

    fn catalogue_body(case_name: &str) -> Value {
        fixture_cases()
            .into_iter()
            .find(|case| case["name"] == case_name)
            .unwrap_or_else(|| panic!("fixture case {case_name} is missing"))["body"]
            .clone()
    }

    /// Every service the settings route touches, as real HTTP servers.
    struct Upstreams {
        auth: MockServer,
        user_core: MockServer,
        model_gateway: MockServer,
        org_core: MockServer,
        /// The caller's active organization: `None` is a user still onboarding.
        active_org: Option<&'static str>,
    }

    impl Upstreams {
        /// `inference_mint` is auth-core's answer to the `inference-core`
        /// audience mint; the model-gateway mint succeeds. (Both are refused for a
        /// session without an active organization, see `start_in`.)
        async fn start(org_role: &str, inference_mint: ResponseTemplate) -> Self {
            Self::start_in(org_role, inference_mint, Some("org-1")).await
        }

        async fn start_in(
            org_role: &str,
            inference_mint: ResponseTemplate,
            active_org: Option<&'static str>,
        ) -> Self {
            let auth = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/get-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                    "session": {"activeOrganizationId": active_org}
                })))
                .mount(&auth)
                .await;
            // auth-core's plane-token endpoints answer only a request that
            // carries the session cookie AND the gateway's internal key, and only for
            // a session with an ACTIVE ORGANIZATION. For a user who is still
            // onboarding they answer 400 "No active organisation on session"
            // (auth-core model-plane-token.controller.ts and plane-token.controller.ts),
            // so no credential can be minted for that user yet. A double that minted
            // one anyway would exercise a path that does not exist.
            let (model_mint, inference_mint) = if active_org.is_some() {
                (
                    ResponseTemplate::new(200).set_body_json(json!({
                        "token": MODEL_TOKEN, "expiresInSeconds": 900
                    })),
                    inference_mint,
                )
            } else {
                let refusal = || {
                    ResponseTemplate::new(400).set_body_json(json!({
                        "statusCode": 400,
                        "message": "No active organisation on session; user-core returned no orgId"
                    }))
                };
                (refusal(), refusal())
            };
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/model-plane/token"))
                .and(wm_header("cookie", COOKIE))
                .and(wm_header("x-internal-api-key", "test-key"))
                .respond_with(model_mint)
                .mount(&auth)
                .await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/inference-core/token"))
                .and(wm_header("cookie", COOKIE))
                .and(wm_header("x-internal-api-key", "test-key"))
                .respond_with(inference_mint)
                .mount(&auth)
                .await;
            let user_core = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "userId": "user-1", "orgId": "org-1", "role": org_role,
                    "onboardingStatus": "COMPLETED"
                })))
                .mount(&user_core)
                .await;
            Self {
                auth,
                user_core,
                model_gateway: MockServer::start().await,
                org_core: MockServer::start().await,
                active_org,
            }
        }

        /// A user who is still onboarding: signed in, no active organization. auth-core
        /// mints no Model Plane or Inference Core credential for such a session, so the
        /// catalogue gate cannot read the catalogue for them (see the test below).
        async fn onboarding() -> Self {
            Self::start_in(
                "owner",
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": INFERENCE_TOKEN, "expiresInSeconds": 300
                })),
                None,
            )
            .await
        }

        async fn with_default_mint(org_role: &str) -> Self {
            Self::start(
                org_role,
                ResponseTemplate::new(200).set_body_json(json!({
                    "token": INFERENCE_TOKEN, "expiresInSeconds": 300
                })),
            )
            .await
        }

        /// model-gateway's `GET /v1/models`: it answers only a request that
        /// carries BOTH delegated credentials, as the real route does
        /// (`VerifiedInferenceBearer`), and the caller's org when there is one.
        async fn catalogue_answers(&self, response: ResponseTemplate) {
            self.mount_catalogue(response, self.active_org).await;
        }

        /// The same, for a session-only flow such as organization creation: no
        /// membership decision is attached to the caller there, so the gateway sends no
        /// `x-org-id` (`authorized_org_id` is empty) even when the session has an active
        /// organization, and the double must not demand one.
        async fn catalogue_answers_session_only(&self, response: ResponseTemplate) {
            self.mount_catalogue(response, None).await;
        }

        async fn mount_catalogue(&self, response: ResponseTemplate, org: Option<&str>) {
            let mut mock = Mock::given(wm_method("GET"))
                .and(wm_path("/v1/models"))
                .and(wm_header(
                    "authorization",
                    format!("Bearer {MODEL_TOKEN}").as_str(),
                ))
                .and(wm_header(
                    "x-inference-authorization",
                    format!("Bearer {INFERENCE_TOKEN}").as_str(),
                ));
            if let Some(org) = org {
                mock = mock.and(wm_header("x-org-id", org));
            }
            mock.respond_with(response).mount(&self.model_gateway).await;
        }

        /// Better Auth's organization list, which `create-organization` reads
        /// first. A 503 ends the flow right after the gate, which is all these
        /// tests need to see: whether the request got past it.
        async fn organization_list_answers(&self, response: ResponseTemplate) {
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/auth/organization/list"))
                .respond_with(response)
                .mount(&self.auth)
                .await;
        }

        /// Whether auth-core was asked for the organization LIST or to CREATE one.
        async fn organization_flow_requests(&self) -> (usize, usize) {
            let requests = self.auth.received_requests().await.expect("recording");
            let count = |path: &str| {
                requests
                    .iter()
                    .filter(|request| request.url.path() == path)
                    .count()
            };
            (
                count("/api/auth/organization/list"),
                count("/api/auth/organization/create"),
            )
        }

        /// org-core's settings route, answering only a request that carries the
        /// gateway's delegated-service identity for THIS org and the exact body.
        async fn org_core_accepts(&self, body: Value, stored: Value) {
            Mock::given(wm_method("PATCH"))
                .and(wm_path("/api/v1/organizations/org-1/settings"))
                .and(wm_header("x-service-id", "verevon-gateway"))
                .and(wm_header("x-delegation-version", "v3"))
                .and(wm_header("x-org-id", "org-1"))
                .and(wm_header("x-user-id", "user-1"))
                .and(wm_body_json(body))
                .respond_with(ResponseTemplate::new(200).set_body_json(stored))
                .mount(&self.org_core)
                .await;
        }

        fn state(&self, model_gateway_reachable: bool) -> AppState {
            let mut state = crate::tests::test_state(false);
            state.auth_core_url = self.auth.uri();
            state.user_core_url = self.user_core.uri();
            state.org_core_url = self.org_core.uri();
            if model_gateway_reachable {
                state.model_gateway_url = self.model_gateway.uri();
            }
            state
        }

        async fn org_core_requests(&self) -> usize {
            self.org_core
                .received_requests()
                .await
                .expect("recording")
                .len()
        }

        async fn catalogue_requests(&self) -> usize {
            self.model_gateway
                .received_requests()
                .await
                .expect("recording")
                .len()
        }

        /// Token mints auth-core served (everything except the session lookup).
        async fn token_mints(&self) -> usize {
            self.auth
                .received_requests()
                .await
                .expect("recording")
                .iter()
                .filter(|request| request.url.path().ends_with("/token"))
                .count()
        }
    }

    async fn send(state: AppState, request: Request<Body>) -> (u16, Value) {
        let response = crate::build_router(state).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let parsed: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, parsed)
    }

    async fn patch_settings(state: AppState, body: Value) -> (u16, Value) {
        send(
            state,
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/orgs/org-1/settings")
                .header("cookie", COOKIE)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    /// The agent-action route calls the same handler: it must not be a way
    /// around the gate.
    async fn execute_action(state: AppState, action_id: &str, input: Value) -> (u16, Value) {
        send(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/v1/actions/execute")
                .header("cookie", COOKIE)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "actionId": action_id, "input": input }).to_string(),
                ))
                .unwrap(),
        )
        .await
    }

    fn stored_zdr(on: bool) -> Value {
        json!({"id": "org-1", "metadata": {"interactiveRetention": {"zdr": on}}})
    }

    #[test]
    fn the_predicate_answers_every_shared_fixture_case_as_the_composer_does() {
        // The SPA runs the same file through the real `listModels()` and
        // `hasZeroRetentionModel` (src/shared/api/chat-client.zdr-capability.test.ts).
        let cases = fixture_cases();
        assert!(cases.len() >= 10, "the fixture lost cases: {}", cases.len());
        let mut attested = 0;
        let mut refused = 0;
        for case in &cases {
            let expected = case["attested_chat_model"]
                .as_bool()
                .unwrap_or_else(|| panic!("case {} has no verdict", case["name"]));
            assert_eq!(
                super::catalogue_attests_zdr_chat_model(&case["body"]),
                Some(expected),
                "case {}",
                case["name"]
            );
            if expected {
                attested += 1;
            } else {
                refused += 1;
            }
        }
        assert!(
            attested >= 3 && refused >= 5,
            "attested={attested} refused={refused}"
        );
    }

    #[test]
    fn a_body_that_is_not_a_catalogue_says_nothing_about_attestation() {
        for body in [
            json!({}),
            json!({"models": null}),
            json!({"models": "gpt"}),
            json!({"error": "inference-core list_models failed: unavailable"}),
            // A bare array is the SPA's tolerance for an old shape, not what
            // model-gateway sends; the gate refuses to guess from it.
            json!([{"id": "m", "modality": "chat", "features": ["zdr"]}]),
        ] {
            assert_eq!(
                super::catalogue_attests_zdr_chat_model(&body),
                None,
                "{body}"
            );
        }
    }

    #[tokio::test]
    async fn turning_zdr_on_is_refused_while_no_provider_is_attested_and_org_core_is_never_called()
    {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("fleet_as_deployed_today_nothing_attested")),
        )
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;

        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "zdr_no_attested_provider", "{body}");
        assert_eq!(
            up.catalogue_requests().await,
            1,
            "the gate reads the catalogue once"
        );
        assert_eq!(
            up.org_core_requests().await,
            0,
            "a refused switch writes nothing"
        );
    }

    #[tokio::test]
    async fn turning_zdr_on_goes_through_once_a_chat_provider_is_attested() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("owner").await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("azure_openai_chat_model_attested")),
        )
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["metadata"]["interactiveRetention"]["zdr"], true,
            "{body}"
        );
        assert_eq!(up.org_core_requests().await, 1);
    }

    #[tokio::test]
    async fn an_attestation_carried_only_by_the_privacy_tier_still_opens_the_gate() {
        // The Anthropic provider's model list hardcodes tools/vision/reasoning and
        // never lists `zdr`, but its privacy tier is derived from the same
        // supports_zdr bit. Refusing it would block a provider inference-core
        // really serves ZDR on.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.catalogue_answers(ResponseTemplate::new(200).set_body_json(catalogue_body(
            "anthropic_attested_but_its_feature_list_omits_zdr_so_the_tier_decides",
        )))
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;
        assert_eq!(status, 200, "{body}");
    }

    #[tokio::test]
    async fn a_catalogue_that_cannot_be_read_fails_closed_on_enable_with_a_distinct_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for failing in [
            ResponseTemplate::new(502)
                .set_body_json(json!({"error": "inference-core list_models failed"})),
            ResponseTemplate::new(401).set_body_json(json!({"error": "unauthorized"})),
            // 200 but not a catalogue: nothing is known about attestation.
            ResponseTemplate::new(200).set_body_json(json!({"unexpected": true})),
            ResponseTemplate::new(200).set_body_string("not json"),
        ] {
            let up = Upstreams::with_default_mint("admin").await;
            up.catalogue_answers(failing).await;
            up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
                .await;

            let (status, body) =
                patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;

            assert_eq!(status, 503, "{body}");
            assert_eq!(body["error"]["code"], "zdr_gate_unavailable", "{body}");
            assert_eq!(up.org_core_requests().await, 0);
        }
    }

    #[tokio::test]
    async fn an_unreachable_model_gateway_fails_closed_on_enable() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(false), json!({"zeroDataRetention": true})).await;

        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_gate_unavailable", "{body}");
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn a_failed_inference_credential_mint_fails_closed_without_calling_model_gateway() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::start("admin", ResponseTemplate::new(503)).await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("azure_openai_chat_model_attested")),
        )
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;

        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_gate_unavailable", "{body}");
        assert_eq!(up.catalogue_requests().await, 0);
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn turning_zdr_off_never_asks_the_catalogue_even_when_it_is_down() {
        // Disabling is always allowed: an org must be able to leave ZDR when the
        // attestation lapsed or model-gateway is down.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.org_core_accepts(json!({"zeroDataRetention": false}), stored_zdr(false))
            .await;

        let (status, body) =
            patch_settings(up.state(false), json!({"zeroDataRetention": false})).await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(body["metadata"]["interactiveRetention"]["zdr"], false);
        assert_eq!(up.catalogue_requests().await, 0);
        assert_eq!(
            up.token_mints().await,
            0,
            "no credential is minted just to switch off"
        );
        assert_eq!(up.org_core_requests().await, 1);
    }

    #[tokio::test]
    async fn the_support_ai_mode_setting_is_not_gated_on_attestation() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.org_core_accepts(
            json!({"supportAiMode": "assist"}),
            json!({"id": "org-1", "metadata": {"supportAi": {"mode": "assist"}}}),
        )
        .await;

        let (status, body) =
            patch_settings(up.state(false), json!({"supportAiMode": "assist"})).await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(up.catalogue_requests().await, 0);
        assert_eq!(up.token_mints().await, 0);
    }

    #[tokio::test]
    async fn a_plain_member_is_refused_before_any_catalogue_or_org_core_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("member").await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("azure_openai_chat_model_attested")),
        )
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) =
            patch_settings(up.state(true), json!({"zeroDataRetention": true})).await;

        assert_eq!(status, 403, "{body}");
        assert_eq!(up.catalogue_requests().await, 0);
        assert_eq!(up.token_mints().await, 0);
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn a_non_boolean_value_cannot_slip_past_the_gate() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;
        for value in [json!("true"), json!(1), json!(null)] {
            let (status, body) =
                patch_settings(up.state(true), json!({"zeroDataRetention": value})).await;
            assert_eq!(status, 400, "{body}");
        }
        assert_eq!(up.org_core_requests().await, 0);
        assert_eq!(up.catalogue_requests().await, 0);
    }

    #[tokio::test]
    async fn the_agent_action_route_is_gated_the_same_way() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("fleet_as_deployed_today_nothing_attested")),
        )
        .await;
        up.org_core_accepts(json!({"zeroDataRetention": true}), stored_zdr(true))
            .await;

        let (status, body) = execute_action(
            up.state(true),
            "org.update_zdr",
            json!({"orgId": "org-1", "zeroDataRetention": true}),
        )
        .await;

        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "zdr_no_attested_provider", "{body}");
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn the_agent_action_route_can_still_turn_zdr_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("admin").await;
        up.org_core_accepts(json!({"zeroDataRetention": false}), stored_zdr(false))
            .await;

        let (status, body) = execute_action(
            up.state(false),
            "org.update_zdr",
            json!({"orgId": "org-1", "zeroDataRetention": false}),
        )
        .await;

        assert_eq!(status, 200, "{body}");
        assert_eq!(up.org_core_requests().await, 1);
        assert_eq!(up.catalogue_requests().await, 0);
    }

    // ---- Organization creation (the onboarding step's switch) ----------------
    //
    // org-core stores `metadata.interactiveRetention` exactly as the gateway
    // forwards it, on create and on re-mirror alike. So `create-organization` is
    // a second way to write `zdr = true`, and the gate has to stand in front of it
    // too; otherwise the onboarding toggle would be a side door past the switch.

    async fn create_organization(state: AppState, metadata: Option<Value>) -> (u16, Value) {
        let mut body = json!({ "name": "Acme" });
        if let Some(metadata) = metadata {
            body["metadata"] = metadata;
        }
        send(
            state,
            Request::builder()
                .method("POST")
                .uri("/api/v1/onboarding/actions/create-organization")
                .header("cookie", COOKIE)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    #[tokio::test]
    async fn creating_an_org_with_zdr_on_is_refused_while_nothing_is_attested_and_nothing_is_created(
    ) {
        // A creator who already has an active organization (a repeat onboarding, a
        // second workspace): their session can mint, so the gate reads the catalogue.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("owner").await;
        up.catalogue_answers_session_only(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("fleet_as_deployed_today_nothing_attested")),
        )
        .await;
        up.organization_list_answers(ResponseTemplate::new(200).set_body_json(json!([])))
            .await;

        let (status, body) = create_organization(
            up.state(true),
            Some(json!({"interactiveRetention": {"zdr": true}})),
        )
        .await;

        assert_eq!(status, 409, "{body}");
        assert_eq!(body["error"]["code"], "zdr_no_attested_provider", "{body}");
        assert_eq!(
            up.organization_flow_requests().await,
            (0, 0),
            "a refused creation must not even look for an organization, let alone create one"
        );
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn creating_an_org_with_zdr_on_fails_closed_when_the_catalogue_cannot_be_read() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("owner").await;
        up.organization_list_answers(ResponseTemplate::new(200).set_body_json(json!([])))
            .await;

        let (status, body) = create_organization(
            up.state(false),
            Some(json!({"interactiveRetention": {"zdr": true}})),
        )
        .await;

        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_gate_unavailable", "{body}");
        assert_eq!(up.organization_flow_requests().await, (0, 0));
    }

    #[tokio::test]
    async fn creating_an_org_with_zdr_on_goes_on_to_creation_once_a_chat_provider_is_attested() {
        // For a creator whose session can mint (an active organization already exists).
        // A first-time creator has none: see the test below.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::with_default_mint("owner").await;
        up.catalogue_answers_session_only(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("azure_openai_chat_model_attested")),
        )
        .await;
        // The flow's first real step. Failing it stops the request right after
        // the gate, which is enough to show the gate let it through.
        up.organization_list_answers(ResponseTemplate::new(503))
            .await;

        let (status, body) = create_organization(
            up.state(true),
            Some(json!({"interactiveRetention": {"zdr": true}})),
        )
        .await;

        assert_eq!(status, 502, "{body}");
        assert_eq!(
            body["error"]["code"], "organization_lookup_failed",
            "{body}"
        );
        assert_eq!(up.catalogue_requests().await, 1);
        assert_eq!(up.organization_flow_requests().await, (1, 0));
    }

    #[tokio::test]
    async fn a_first_time_creator_cannot_switch_zdr_on_at_creation_even_when_a_provider_is_attested(
    ) {
        // The usual creator has NO active organization yet, and auth-core refuses to
        // mint a Model Plane or Inference Core credential for such a session (400
        // "No active organisation on session"). The gate cannot read the catalogue
        // without one, so it fails closed with 503 zdr_gate_unavailable WHATEVER is
        // attested, and nothing is created. Switching ZDR on is then done in the
        // settings once the organization exists, where the gate opens by itself.
        // Making creation itself read the catalogue needs a pre-organization
        // credential from the Control Plane (open item in the ZDR runbook, section 8).
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let up = Upstreams::onboarding().await;
        up.catalogue_answers(
            ResponseTemplate::new(200)
                .set_body_json(catalogue_body("azure_openai_chat_model_attested")),
        )
        .await;
        up.organization_list_answers(ResponseTemplate::new(200).set_body_json(json!([])))
            .await;

        let (status, body) = create_organization(
            up.state(true),
            Some(json!({"interactiveRetention": {"zdr": true}})),
        )
        .await;

        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_gate_unavailable", "{body}");
        assert_eq!(
            up.catalogue_requests().await,
            0,
            "no credential could be minted, so the catalogue was never asked"
        );
        assert_eq!(up.organization_flow_requests().await, (0, 0));
        assert_eq!(up.org_core_requests().await, 0);
    }

    #[tokio::test]
    async fn creating_an_org_without_an_on_switch_never_reads_the_catalogue() {
        // Off, absent, and every shape org-core does not read as "on" must not
        // cost a catalogue round trip, or let onboarding fail on a model-gateway
        // outage for an org that asked for nothing.
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for metadata in [
            None,
            Some(json!({})),
            Some(json!({"interactiveRetention": {"zdr": false}})),
            Some(json!({"interactiveRetention": {"zdr": "true"}})),
            Some(json!({"interactiveRetention": "on"})),
            Some(json!({"onboarding_branding": {"site_name": "Acme"}})),
        ] {
            let up = Upstreams::onboarding().await;
            up.organization_list_answers(ResponseTemplate::new(503))
                .await;

            let (status, body) = create_organization(up.state(false), metadata.clone()).await;

            assert_eq!(status, 502, "{metadata:?} -> {body}");
            assert_eq!(
                body["error"]["code"], "organization_lookup_failed",
                "{body}"
            );
            assert_eq!(up.catalogue_requests().await, 0, "{metadata:?}");
            assert_eq!(up.token_mints().await, 0, "{metadata:?}");
            assert_eq!(
                up.organization_flow_requests().await,
                (1, 0),
                "{metadata:?}"
            );
        }
    }
}
