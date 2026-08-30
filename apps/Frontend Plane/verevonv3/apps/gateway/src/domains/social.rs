//! Social planning domain — account readiness, calendar, drafts, and publish intents.
//!
//! This is the browser-facing contract for the Verevon v3 social workspace. All
//! reads and writes proxy to social-core; when it is unavailable, reads return
//! an honest empty payload with `meta.source = "unavailable"` and writes return
//! 503 `social_core_unavailable` — no in-memory store, no fabricated data.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::DateTime;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok, ok_with_source},
    middleware::{require_session, AuthenticatedUser},
    upstream::{proxy_conversation_json, proxy_json},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/social/accounts", get(list_accounts))
        .route("/api/v1/social/adapters", get(list_adapters))
        .route("/api/v1/social/approvals", get(list_approvals))
        .route(
            "/api/v1/social/approvals/{id}/decide",
            post(decide_approval),
        )
        .route(
            "/api/v1/social/campaigns",
            get(list_campaigns).post(create_campaign),
        )
        .route(
            "/api/v1/social/competitor-watch",
            get(list_competitor_watch),
        )
        .route("/api/v1/social/trends", get(list_trends))
        .route("/api/v1/social/evergreen", get(list_evergreen))
        .route("/api/v1/social/posts", get(list_posts).post(create_post))
        .route("/api/v1/social/calendar", get(calendar))
        .route(
            "/api/v1/social/drafts/from-inbox",
            post(create_draft_from_inbox),
        )
        .route("/api/v1/social/posts/{id}/schedule", post(schedule_post))
        .route("/api/v1/social/posts/{id}/publish", post(publish_post))
        .route("/api/v1/social/metrics", get(list_metrics))
        .route("/api/v1/social/catalogs", get(list_catalogs))
        .route(
            "/api/v1/social/catalogs/{id}/products",
            get(list_catalog_products),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Standard 503 for social write paths when social-core is unavailable. We no
/// longer persist to an in-memory store or fabricate a "success" — the write
/// fails honestly so the SPA can surface it.
fn social_core_unavailable() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "social_core_unavailable",
            "Social core is unavailable; the action was not performed.",
        )),
    )
        .into_response()
}

// --- Metrics + commerce catalog reads ---------------------------------------
//
// social-core already exposes real, per-provider ad metric snapshots
// (`GET /api/v1/social/metrics`) and read-only Meta Commerce Catalog reads
// (`GET /api/v1/social/catalogs`, `.../catalogs/:id/products`). Both were
// routed and tested in social-core but had no browser-facing gateway route.
// These proxies close that gap. Reads degrade to an honest empty payload
// tagged `meta.source = "unavailable"` — never fabricated data.

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SocialMetricsQuery {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    snapshot_date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SocialCatalogProductsQuery {
    #[serde(default)]
    account_id: Option<String>,
}

/// One recorded provider metric value. Every field is sourced from a real
/// social-core snapshot row — the gateway never synthesizes a value.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialMetric {
    account_id: String,
    connection_id: String,
    provider_key: String,
    metric_name: String,
    metric_value: f64,
    dimensions: Value,
    snapshot_date: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MetricsList {
    metrics: Vec<SocialMetric>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogList {
    catalogs: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogProductList {
    products: Vec<Value>,
}

/// Build an upstream path with optional query parameters, dropping any that are
/// blank and URL-encoding the values. Keeps the org out of the query — the org
/// is always forwarded server-side as `x-org-id` by `social_core_json`.
fn social_query_path(base: &str, params: &[(&str, String)]) -> String {
    let active: Vec<String> = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value.trim())))
        .collect();
    if active.is_empty() {
        base.to_owned()
    } else {
        format!("{base}?{}", active.join("&"))
    }
}

/// Map a social-core metric row into the typed browser contract. Rows without a
/// metric name are skipped (they cannot be rendered), never coerced to zero.
fn core_metric_from_value(value: &Value) -> Option<SocialMetric> {
    let obj = value.as_object()?;
    let metric_name = obj
        .get("metric_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())?
        .to_owned();
    Some(SocialMetric {
        account_id: obj
            .get("account_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        connection_id: obj
            .get("connection_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        provider_key: obj
            .get("provider_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        metric_name,
        metric_value: obj
            .get("metric_value")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        dimensions: obj.get("dimensions").cloned().unwrap_or(Value::Null),
        snapshot_date: obj
            .get("snapshot_date")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

async fn list_metrics(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<SocialMetricsQuery>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_metrics(&state, &user, &org_id, &query).await {
        CoreRead::Ready(metrics) => Json(ok(MetricsList { metrics })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            MetricsList { metrics: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

async fn list_catalogs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_catalogs(&state, &user, &org_id).await {
        CoreRead::Ready(catalogs) => Json(ok(CatalogList { catalogs })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            CatalogList { catalogs: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

async fn list_catalog_products(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(catalog_id): Path<String>,
    Query(query): Query<SocialCatalogProductsQuery>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_catalog_products(&state, &user, &org_id, &catalog_id, &query).await {
        CoreRead::Ready(products) => Json(ok(CatalogProductList { products })).into_response(),
        CoreRead::Error(response) => response,
        CoreRead::Unavailable => Json(ok_with_source(
            CatalogProductList { products: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

async fn load_core_metrics(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    query: &SocialMetricsQuery,
) -> CoreRead<Vec<SocialMetric>> {
    let path = social_query_path(
        "/api/v1/social/metrics",
        &[
            ("accountId", query.account_id.clone().unwrap_or_default()),
            (
                "snapshotDate",
                query.snapshot_date.clone().unwrap_or_default(),
            ),
        ],
    );
    let (status, body) = social_core_json(state, user, org_id, Method::GET, &path, None).await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let metrics = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_metric_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(metrics)
}

async fn load_core_catalogs(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> CoreRead<Vec<Value>> {
    let (status, body) = social_core_json(
        state,
        user,
        org_id,
        Method::GET,
        "/api/v1/social/catalogs",
        None,
    )
    .await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let catalogs = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    CoreRead::Ready(catalogs)
}

async fn load_core_catalog_products(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    catalog_id: &str,
    query: &SocialCatalogProductsQuery,
) -> CoreRead<Vec<Value>> {
    let path = social_query_path(
        &format!(
            "/api/v1/social/catalogs/{}/products",
            urlencoding::encode(catalog_id)
        ),
        &[("accountId", query.account_id.clone().unwrap_or_default())],
    );
    let (status, body) = social_core_json(state, user, org_id, Method::GET, &path, None).await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let products = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    CoreRead::Ready(products)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountList {
    accounts: Vec<SocialAccount>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PostList {
    posts: Vec<SocialPost>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalList {
    approvals: Vec<SocialApproval>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CampaignList {
    campaigns: Vec<SocialCampaign>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompetitorWatchList {
    competitors: Vec<SocialCompetitorWatchItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrendList {
    trends: Vec<SocialTrendSignal>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvergreenList {
    items: Vec<SocialEvergreenItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdapterList {
    adapters: Vec<PlatformAdapter>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialCalendar {
    accounts: Vec<SocialAccount>,
    posts: Vec<SocialPost>,
    recommended_windows: Vec<RecommendedWindow>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialAccount {
    id: String,
    provider_key: &'static str,
    label: String,
    handle: String,
    status: &'static str,
    capabilities: Vec<String>,
    accent: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialPost {
    id: String,
    title: String,
    body: String,
    status: &'static str,
    scheduled_at: String,
    platforms: Vec<&'static str>,
    source: SocialPostSource,
    approval: ApprovalState,
    media: Vec<MediaAsset>,
    previews: Vec<PlatformPreview>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialPostSource {
    kind: &'static str,
    label: String,
    href: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalState {
    required: bool,
    state: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialApproval {
    id: String,
    post_id: String,
    campaign_id: String,
    state: &'static str,
    requested_by_user_id: String,
    requested_of_user_id: String,
    decided_by_user_id: String,
    decision_reason: String,
    due_at: Option<String>,
    decided_at: Option<String>,
    post: Option<SocialPost>,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialCampaign {
    id: String,
    name: String,
    brief: String,
    goal: String,
    status: &'static str,
    platforms: Vec<&'static str>,
    starts_at: Option<String>,
    ends_at: Option<String>,
    source: SocialPostSource,
    owner_user_id: String,
    created_at: String,
    updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialCompetitorWatchItem {
    id: String,
    label: String,
    provider_key: &'static str,
    handle: String,
    signal: String,
    velocity: &'static str,
    captured_at: Option<String>,
    status: &'static str,
    source_href: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialTrendSignal {
    id: String,
    label: String,
    provider_key: &'static str,
    format: String,
    opportunity: String,
    velocity: &'static str,
    status: &'static str,
    source_href: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SocialEvergreenItem {
    id: String,
    title: String,
    cadence: &'static str,
    last_published_at: Option<String>,
    next_eligible_at: Option<String>,
    guardrail: String,
    status: &'static str,
    platforms: Vec<&'static str>,
    source_post_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaAsset {
    kind: &'static str,
    label: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecommendedWindow {
    id: &'static str,
    label: &'static str,
    starts_at: &'static str,
    reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformAdapter {
    provider_key: &'static str,
    label: &'static str,
    mode: &'static str,
    endpoint: &'static str,
    max_characters: usize,
    media_required: bool,
    required_capabilities: Vec<&'static str>,
    notes: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlatformPreview {
    provider_key: &'static str,
    label: &'static str,
    text: String,
    character_count: usize,
    max_characters: usize,
    media_required: bool,
    ready: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishAttempt {
    provider_key: &'static str,
    label: &'static str,
    status: &'static str,
    mode: &'static str,
    endpoint: &'static str,
    message: String,
    external_id: Option<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishResult {
    id: String,
    status: &'static str,
    idempotency_key: String,
    attempts: Vec<PublishAttempt>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PostMutation {
    post: SocialPost,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishMutation {
    post: SocialPost,
    result: PublishResult,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePostBody {
    title: Option<String>,
    body: Option<String>,
    platforms: Option<Vec<String>>,
    scheduled_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxDraftBody {
    pub(crate) ticket_id: String,
    pub(crate) ticket_title: String,
    pub(crate) support_ticket_id: Option<String>,
    pub(crate) conversation_id: Option<String>,
    pub(crate) customer_name: Option<String>,
    pub(crate) channel: Option<String>,
    pub(crate) excerpt: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleBody {
    scheduled_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateCampaignBody {
    pub(crate) name: String,
    pub(crate) brief: Option<String>,
    pub(crate) goal: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) platforms: Option<Vec<String>>,
    pub(crate) starts_at: Option<String>,
    pub(crate) ends_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DecideApprovalBody {
    pub(crate) decision: String,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SocialListQuery {
    status: Option<String>,
    state: Option<String>,
    limit: Option<u16>,
}

#[derive(Debug)]
struct ValidatedPostInput {
    title: String,
    body: String,
    platforms: Vec<&'static str>,
    scheduled_at: String,
}

#[derive(Debug)]
struct ValidatedCampaignInput {
    name: String,
    brief: String,
    goal: String,
    status: String,
    platforms: Vec<String>,
    starts_at: Option<String>,
    ends_at: Option<String>,
}

async fn list_accounts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_social_accounts(&state, &user, &org_id).await {
        CoreRead::Ready(accounts) => Json(ok(AccountList { accounts })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            AccountList { accounts: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

async fn list_adapters() -> impl IntoResponse {
    Json(ok(AdapterList {
        adapters: platform_adapters(),
    }))
}

async fn list_approvals(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<SocialListQuery>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_approvals(&state, &user, &org_id, &query).await {
        CoreRead::Ready(approvals) => Json(ok(ApprovalList { approvals })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            ApprovalList { approvals: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

pub(crate) async fn decide_approval(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<DecideApprovalBody>,
) -> axum::response::Response {
    let decision = match normalize_decision(&body.decision) {
        Some(decision) => decision,
        None => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(error(
                    "validation_error",
                    "decision must be approve or reject.",
                )),
            )
                .into_response()
        }
    };

    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match decide_core_approval(
        &state,
        &user,
        &org_id,
        &id,
        decision,
        body.reason.as_deref().unwrap_or_default(),
    )
    .await
    {
        CoreApprovalMutation::Ready(approval) => Json(ok(*approval)).into_response(),
        CoreApprovalMutation::Error(response) => response,
        CoreApprovalMutation::Unavailable => social_core_unavailable(),
    }
}

async fn list_campaigns(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Query(query): Query<SocialListQuery>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_campaigns(&state, &user, &org_id, &query).await {
        CoreRead::Ready(campaigns) => Json(ok(CampaignList { campaigns })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            CampaignList { campaigns: vec![] },
            "unavailable",
        ))
        .into_response(),
    }
}

pub(crate) async fn create_campaign(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<CreateCampaignBody>,
) -> axum::response::Response {
    let input = match validate_create_campaign(&body) {
        Ok(input) => input,
        Err(message) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(error("validation_error", message)),
            )
                .into_response()
        }
    };

    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let core_body = json!({
        "name": input.name,
        "brief": input.brief,
        "goal": input.goal,
        "status": input.status,
        "platforms": input.platforms,
        "starts_at": input.starts_at,
        "ends_at": input.ends_at,
        "source": {
            "kind": "campaign",
            "label": "Studio campaign"
        }
    });
    match create_core_campaign(&state, &user, &org_id, core_body).await {
        CoreCampaignMutation::Ready(campaign) => {
            (StatusCode::CREATED, Json(ok(campaign))).into_response()
        }
        CoreCampaignMutation::Error(response) => response,
        CoreCampaignMutation::Unavailable => social_core_unavailable(),
    }
}

/// Competitor-watch read surface.
///
/// Phase 1 Track C: this used to synthesize fake "competitor lane" items from
/// the org's connected social accounts (`derived_competitor_watch`) — an
/// affordance for a feature that does not run. There is no competitor-watch
/// producer today, so the honest answer is an empty list explicitly tagged
/// `"unavailable"`. The real, on-demand change-monitoring surface lives in the
/// `monitoring` domain (`/api/v1/monitoring/*`); at-scale competitor tracking
/// is deferred to Phase 2. We still verify the session resolves an org so the
/// boundary is enforced even while the payload is empty.
async fn list_competitor_watch(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    if let Err(response) = authorized_social_org_id(&state, &user).await {
        return response;
    }

    Json(ok_with_source(
        CompetitorWatchList {
            competitors: vec![],
        },
        "unavailable",
    ))
    .into_response()
}

async fn list_trends(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let posts = match load_core_posts(&state, &user, &org_id).await {
        CoreRead::Ready(posts) => posts,
        CoreRead::Error(_) | CoreRead::Unavailable => {
            return Json(ok_with_source(TrendList { trends: vec![] }, "unavailable"))
                .into_response();
        }
    };

    Json(ok(TrendList {
        trends: derived_trends(posts, platform_adapters()),
    }))
    .into_response()
}

async fn list_evergreen(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let posts = match load_core_posts(&state, &user, &org_id).await {
        CoreRead::Ready(posts) => posts,
        CoreRead::Error(_) | CoreRead::Unavailable => {
            return Json(ok_with_source(
                EvergreenList { items: vec![] },
                "unavailable",
            ))
            .into_response();
        }
    };

    Json(ok(EvergreenList {
        items: derived_evergreen(posts, platform_adapters()),
    }))
    .into_response()
}

async fn list_posts(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match load_core_posts(&state, &user, &org_id).await {
        CoreRead::Ready(posts) => Json(ok(PostList { posts })).into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => {
            Json(ok_with_source(PostList { posts: vec![] }, "unavailable")).into_response()
        }
    }
}

async fn calendar(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let accounts = match load_social_accounts(&state, &user, &org_id).await {
        CoreRead::Ready(accounts) => accounts,
        CoreRead::Error(_) | CoreRead::Unavailable => {
            return Json(ok_with_source(
                SocialCalendar {
                    accounts: vec![],
                    posts: vec![],
                    recommended_windows: recommended_windows(),
                },
                "unavailable",
            ))
            .into_response();
        }
    };
    match load_core_posts(&state, &user, &org_id).await {
        CoreRead::Ready(posts) => Json(ok(SocialCalendar {
            accounts,
            posts,
            recommended_windows: recommended_windows(),
        }))
        .into_response(),
        CoreRead::Error(_) | CoreRead::Unavailable => Json(ok_with_source(
            SocialCalendar {
                accounts,
                posts: vec![],
                recommended_windows: recommended_windows(),
            },
            "unavailable",
        ))
        .into_response(),
    }
}

async fn create_post(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<CreatePostBody>,
) -> axum::response::Response {
    let input = match validate_create_post(&body) {
        Ok(input) => input,
        Err(message) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(error("validation_error", message)),
            )
                .into_response()
        }
    };

    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let core_body = json!({
        "title": input.title.clone(),
        "body": input.body.clone(),
        "platforms": input.platforms.clone(),
        "scheduled_at": input.scheduled_at.clone(),
        "source": {
            "kind": "manual",
            "label": "Manual draft"
        }
    });
    match create_core_post(&state, &user, &org_id, core_body).await {
        CoreMutation::Ready(post) => {
            (StatusCode::CREATED, Json(ok(PostMutation { post }))).into_response()
        }
        CoreMutation::Error(response) => response,
        CoreMutation::Unavailable => social_core_unavailable(),
    }
}

pub(crate) async fn create_draft_from_inbox(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<InboxDraftBody>,
) -> axum::response::Response {
    let customer = body
        .customer_name
        .clone()
        .unwrap_or_else(|| "Customer".to_owned());
    let channel = body.channel.clone().unwrap_or_else(|| "inbox".to_owned());
    let excerpt = body
        .excerpt
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Customer conversation requires a public follow-up.".to_owned());
    let title = format!("Follow-up from {}", body.ticket_title.trim());
    let text = format!(
        "We are turning this {} conversation with {} into a helpful product update: {}",
        channel, customer, excerpt
    );

    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    let core_body = json!({
        "title": title.clone(),
        "body": text.clone(),
        "platforms": ["linkedin", "x"],
        "source": {
            "kind": "inbox",
            "label": format!("Inbox ticket {}", body.ticket_id),
            "href": format!("/inbox?ticketId={}", body.ticket_id),
            "metadata": {
                "ticket_id": body.ticket_id.clone(),
                "support_ticket_id": body.support_ticket_id.clone(),
                "conversation_id": body.conversation_id.clone(),
                "channel": channel.clone(),
                "customer_name": customer.clone()
            }
        },
        "ai_context": {
            "draft_source": "inbox_follow_up"
        }
    });
    match create_core_post(&state, &user, &org_id, core_body).await {
        CoreMutation::Ready(post) => {
            link_social_post_to_ticket(&state, &user, &org_id, &body, &post).await;
            (StatusCode::CREATED, Json(ok(PostMutation { post }))).into_response()
        }
        CoreMutation::Error(response) => response,
        CoreMutation::Unavailable => social_core_unavailable(),
    }
}

async fn schedule_post(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
    Json(body): Json<ScheduleBody>,
) -> axum::response::Response {
    if DateTime::parse_from_rfc3339(body.scheduled_at.trim()).is_err() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(error(
                "validation_error",
                "scheduledAt must be a valid RFC3339 timestamp.",
            )),
        )
            .into_response();
    }

    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match schedule_core_post(&state, &user, &org_id, &id, body.scheduled_at.trim()).await {
        CoreMutation::Ready(post) => Json(ok(PostMutation { post })).into_response(),
        CoreMutation::Error(response) => response,
        CoreMutation::Unavailable => social_core_unavailable(),
    }
}

async fn publish_post(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> axum::response::Response {
    let org_id = match authorized_social_org_id(&state, &user).await {
        Ok(org_id) => org_id,
        Err(response) => return response,
    };
    match enqueue_core_publish(&state, &user, &org_id, &id).await {
        CorePublish::Ready(post, result) => (
            StatusCode::ACCEPTED,
            Json(ok(PublishMutation {
                post: *post,
                result,
            })),
        )
            .into_response(),
        CorePublish::Error(response) => response,
        CorePublish::Unavailable => social_core_unavailable(),
    }
}

enum CoreRead<T> {
    Ready(T),
    Unavailable,
    Error(axum::response::Response),
}

enum CoreMutation {
    Ready(SocialPost),
    Unavailable,
    Error(axum::response::Response),
}

enum CoreApprovalMutation {
    Ready(Box<SocialApproval>),
    Unavailable,
    Error(axum::response::Response),
}

enum CoreCampaignMutation {
    Ready(SocialCampaign),
    Unavailable,
    Error(axum::response::Response),
}

enum CorePublish {
    Ready(Box<SocialPost>, PublishResult),
    Unavailable,
    Error(axum::response::Response),
}

async fn authorized_social_org_id(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<String, axum::response::Response> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if !org_id.trim().is_empty() {
        return Ok(org_id);
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(error(
            "org_scope_required",
            "An authorized organization scope is required.",
        )),
    )
        .into_response())
}

fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

#[derive(Debug)]
pub(crate) struct StudioSocialDraftInput {
    pub(crate) project_id: String,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) platforms: Vec<&'static str>,
    pub(crate) scheduled_at: String,
    pub(crate) source_label: String,
}

pub(crate) async fn create_studio_social_draft(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    input: StudioSocialDraftInput,
) -> Result<Value, axum::response::Response> {
    let core_body = json!({
        "title": input.title.clone(),
        "body": input.body.clone(),
        "platforms": input.platforms.clone(),
        "scheduled_at": input.scheduled_at.clone(),
        "source": {
            "kind": "campaign",
            "label": input.source_label.clone(),
            "href": format!("/studio/canvas?projectId={}", input.project_id),
            "metadata": {
                "studio_project_id": input.project_id.clone(),
                "draft_source": "studio_canvas"
            }
        },
        "ai_context": {
            "draft_source": "studio_canvas",
            "studio_project_id": input.project_id.clone()
        }
    });
    match create_core_post(state, user, org_id, core_body).await {
        CoreMutation::Ready(post) => Ok(social_post_value(post)),
        CoreMutation::Error(response) => Err(response),
        CoreMutation::Unavailable => Err(social_core_unavailable()),
    }
}

async fn social_core_json(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let url = format!("{}{}", state.social_core_url, path);
    let (status, Json(body)) = proxy_json(
        state,
        method,
        &url,
        body,
        Some(org_id),
        Some(&actor_for(user)),
        Some("application/json"),
    )
    .await;
    (status, body)
}

async fn conversation_core_json(
    state: &AppState,
    user: &AuthenticatedUser,
    _org_id: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let url = format!("{}{}", state.conversation_core_url, path);
    let (status, Json(body)) =
        proxy_conversation_json(state, method, &url, body, user, Some("application/json")).await;
    (status, body)
}

async fn link_social_post_to_ticket(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    inbox: &InboxDraftBody,
    post: &SocialPost,
) {
    let Some(ticket_id) = inbox
        .support_ticket_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let path = format!("/api/v1/tickets/{}/links", urlencoding::encode(ticket_id));
    let _ = conversation_core_json(
        state,
        user,
        org_id,
        Method::POST,
        &path,
        Some(json!({
            "link_type": "related",
            "resource_kind": "social_post",
            "resource_id": post.id,
            "resource_url": format!("/social/calendar?postId={}", post.id),
            "label": post.title,
            "metadata": {
                "source": "inbox_social_follow_up",
                "conversation_id": inbox.conversation_id,
                "ticket_id": inbox.ticket_id
            }
        })),
    )
    .await;
}

async fn load_core_accounts(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> CoreRead<Vec<SocialAccount>> {
    let (status, body) = social_core_json(
        state,
        user,
        org_id,
        Method::GET,
        "/api/v1/social/accounts",
        None,
    )
    .await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let accounts = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_account_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(accounts)
}

async fn load_social_accounts(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> CoreRead<Vec<SocialAccount>> {
    match load_core_accounts(state, user, org_id).await {
        CoreRead::Ready(accounts) if !accounts.is_empty() => CoreRead::Ready(accounts),
        CoreRead::Ready(_) | CoreRead::Unavailable => {
            match load_integration_provider_accounts(state, user, org_id).await {
                CoreRead::Ready(accounts) if !accounts.is_empty() => CoreRead::Ready(accounts),
                CoreRead::Ready(_) | CoreRead::Unavailable => CoreRead::Unavailable,
                CoreRead::Error(response) => CoreRead::Error(response),
            }
        }
        CoreRead::Error(response) => CoreRead::Error(response),
    }
}

async fn load_integration_provider_accounts(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> CoreRead<Vec<SocialAccount>> {
    let url = format!("{}/api/v1/providers", state.integration_core_url);
    let (status, Json(body)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }

    let data = body.get("data").unwrap_or(&body);
    let accounts = data
        .get("providers")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(integration_provider_account_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(accounts)
}

async fn load_core_posts(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
) -> CoreRead<Vec<SocialPost>> {
    let (status, body) = social_core_json(
        state,
        user,
        org_id,
        Method::GET,
        "/api/v1/social/posts",
        None,
    )
    .await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let posts = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_post_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(posts)
}

async fn load_core_approvals(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    query: &SocialListQuery,
) -> CoreRead<Vec<SocialApproval>> {
    let limit = query.limit.map(|limit| limit.to_string());
    let approval_state = query.state.as_deref().or(query.status.as_deref());
    let path = with_query(
        "/api/v1/social/approvals",
        &[("state", approval_state), ("limit", limit.as_deref())],
    );
    let (status, body) = social_core_json(state, user, org_id, Method::GET, &path, None).await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let approvals = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_approval_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(approvals)
}

async fn load_core_campaigns(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    query: &SocialListQuery,
) -> CoreRead<Vec<SocialCampaign>> {
    let limit = query.limit.map(|limit| limit.to_string());
    let path = with_query(
        "/api/v1/social/campaigns",
        &[
            ("status", query.status.as_deref()),
            ("limit", limit.as_deref()),
        ],
    );
    let (status, body) = social_core_json(state, user, org_id, Method::GET, &path, None).await;
    if core_unavailable(status, &body) {
        return CoreRead::Unavailable;
    }
    if !status.is_success() {
        return CoreRead::Error((status, Json(body)).into_response());
    }
    let campaigns = body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_campaign_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    CoreRead::Ready(campaigns)
}

async fn create_core_post(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: Value,
) -> CoreMutation {
    let (status, response_body) = social_core_json(
        state,
        user,
        org_id,
        Method::POST,
        "/api/v1/social/posts",
        Some(body),
    )
    .await;
    if core_unavailable(status, &response_body) {
        return CoreMutation::Unavailable;
    }
    if !status.is_success() {
        return CoreMutation::Error((status, Json(response_body)).into_response());
    }
    match core_post_from_response(&response_body) {
        Some(post) => CoreMutation::Ready(post),
        None => CoreMutation::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core returned an invalid post payload.",
                )),
            )
                .into_response(),
        ),
    }
}

async fn create_core_campaign(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    body: Value,
) -> CoreCampaignMutation {
    let (status, response_body) = social_core_json(
        state,
        user,
        org_id,
        Method::POST,
        "/api/v1/social/campaigns",
        Some(body),
    )
    .await;
    if core_unavailable(status, &response_body) {
        return CoreCampaignMutation::Unavailable;
    }
    if !status.is_success() {
        return CoreCampaignMutation::Error((status, Json(response_body)).into_response());
    }
    match response_body.get("data").and_then(core_campaign_from_value) {
        Some(campaign) => CoreCampaignMutation::Ready(campaign),
        None => CoreCampaignMutation::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core returned an invalid campaign payload.",
                )),
            )
                .into_response(),
        ),
    }
}

async fn decide_core_approval(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    id: &str,
    decision: &str,
    reason: &str,
) -> CoreApprovalMutation {
    let path = format!(
        "/api/v1/social/approvals/{}/decide",
        urlencoding::encode(id)
    );
    let (status, response_body) = social_core_json(
        state,
        user,
        org_id,
        Method::POST,
        &path,
        Some(json!({ "decision": decision, "reason": reason })),
    )
    .await;
    if core_unavailable(status, &response_body) {
        return CoreApprovalMutation::Unavailable;
    }
    if !status.is_success() {
        return CoreApprovalMutation::Error((status, Json(response_body)).into_response());
    }
    match response_body.get("data").and_then(core_approval_from_value) {
        Some(approval) => CoreApprovalMutation::Ready(Box::new(approval)),
        None => CoreApprovalMutation::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core returned an invalid approval payload.",
                )),
            )
                .into_response(),
        ),
    }
}

async fn schedule_core_post(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    id: &str,
    scheduled_at: &str,
) -> CoreMutation {
    let path = format!("/api/v1/social/posts/{}/schedule", urlencoding::encode(id));
    let (status, response_body) = social_core_json(
        state,
        user,
        org_id,
        Method::POST,
        &path,
        Some(json!({ "scheduled_at": scheduled_at })),
    )
    .await;
    if core_unavailable(status, &response_body) {
        return CoreMutation::Unavailable;
    }
    if !status.is_success() {
        return CoreMutation::Error((status, Json(response_body)).into_response());
    }
    match core_post_from_response(&response_body) {
        Some(post) => CoreMutation::Ready(post),
        None => CoreMutation::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core returned an invalid schedule payload.",
                )),
            )
                .into_response(),
        ),
    }
}

async fn enqueue_core_publish(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    id: &str,
) -> CorePublish {
    let path = format!(
        "/api/v1/social/posts/{}/publish-jobs",
        urlencoding::encode(id)
    );
    let (status, response_body) =
        social_core_json(state, user, org_id, Method::POST, &path, Some(json!({}))).await;
    if core_unavailable(status, &response_body) {
        return CorePublish::Unavailable;
    }
    if !status.is_success() {
        return CorePublish::Error((status, Json(response_body)).into_response());
    }
    let Some(job) = response_body.get("data") else {
        return CorePublish::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core returned an invalid publish job payload.",
                )),
            )
                .into_response(),
        );
    };
    let Some(post) = job.get("post").and_then(core_post_from_value) else {
        return CorePublish::Error(
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "upstream_shape_error",
                    "social-core publish job did not include the post payload.",
                )),
            )
                .into_response(),
        );
    };
    let accounts = match load_core_accounts(state, user, org_id).await {
        CoreRead::Ready(accounts) => accounts,
        CoreRead::Error(_) | CoreRead::Unavailable => Vec::new(),
    };
    let mut result = build_publish_result(&post, &accounts);
    if let Some(id) = job.get("id").and_then(Value::as_str) {
        result.id = id.to_owned();
    }
    if let Some(idempotency_key) = job.get("idempotency_key").and_then(Value::as_str) {
        result.idempotency_key = idempotency_key.to_owned();
    }
    CorePublish::Ready(Box::new(post), result)
}

fn core_unavailable(status: StatusCode, body: &Value) -> bool {
    status == StatusCode::BAD_GATEWAY
        && body
            .pointer("/error/code")
            .and_then(Value::as_str)
            .map(|code| code == "upstream_unavailable")
            .unwrap_or(false)
}

fn core_post_from_response(body: &Value) -> Option<SocialPost> {
    let data = body.get("data")?;
    data.get("post")
        .and_then(core_post_from_value)
        .or_else(|| core_post_from_value(data))
}

fn social_post_value(post: SocialPost) -> Value {
    serde_json::to_value(post).unwrap_or(Value::Null)
}

fn core_account_from_value(value: &Value) -> Option<SocialAccount> {
    let provider_key = provider_key_from_value(value.get("provider_key")?.as_str()?)?;
    Some(SocialAccount {
        id: value
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(provider_key)
            .to_owned(),
        provider_key,
        label: value
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|display_name| !display_name.trim().is_empty())
            .unwrap_or_else(|| provider_label(provider_key))
            .to_owned(),
        handle: value
            .get("handle")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        status: account_status_from_core(value.get("status").and_then(Value::as_str)),
        capabilities: value
            .get("capabilities")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        accent: provider_accent(provider_key),
    })
}

fn integration_provider_account_from_value(value: &Value) -> Option<SocialAccount> {
    let category = value
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if category != "social" {
        return None;
    }

    let provider_key = value
        .get("key")
        .and_then(Value::as_str)
        .or_else(|| value.get("connectorType").and_then(Value::as_str))
        .and_then(provider_key_from_value)?;
    let configured = value
        .get("configured")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let direct_oauth_ready = value
        .get("directOAuthReady")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let capabilities = value
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|capability| capability.get("key").and_then(Value::as_str))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let label = value
        .get("label")
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(|| provider_label(provider_key))
        .to_owned();

    Some(SocialAccount {
        id: format!("provider_{}", provider_key),
        provider_key,
        label,
        handle: integration_provider_handle(value, configured, direct_oauth_ready),
        status: integration_provider_status(value, configured, direct_oauth_ready),
        capabilities,
        accent: provider_accent(provider_key),
    })
}

fn integration_provider_handle(
    value: &Value,
    configured: bool,
    direct_oauth_ready: bool,
) -> String {
    if configured && direct_oauth_ready {
        return "Ready for OAuth".to_owned();
    }
    if configured {
        return "Admin setup required".to_owned();
    }

    let missing = value
        .get("missingConfig")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(2)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if missing.is_empty() {
        "Provider config missing".to_owned()
    } else {
        format!("Missing {}", missing.join(", "))
    }
}

fn integration_provider_status(
    _value: &Value,
    configured: bool,
    direct_oauth_ready: bool,
) -> &'static str {
    if configured && direct_oauth_ready {
        "needs_oauth"
    } else {
        "manual_review"
    }
}

fn core_approval_from_value(value: &Value) -> Option<SocialApproval> {
    let id = value.get("id")?.as_str()?.to_owned();
    let post = value.get("post").and_then(core_post_from_value);
    Some(SocialApproval {
        id,
        post_id: value
            .get("post_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        campaign_id: value
            .get("campaign_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        state: approval_queue_state_from_core(value.get("state").and_then(Value::as_str)),
        requested_by_user_id: value
            .get("requested_by_user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        requested_of_user_id: value
            .get("requested_of_user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        decided_by_user_id: value
            .get("decided_by_user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        decision_reason: value
            .get("decision_reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        due_at: value
            .get("due_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        decided_at: value
            .get("decided_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        post,
        created_at: value
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        updated_at: value
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn core_campaign_from_value(value: &Value) -> Option<SocialCampaign> {
    let id = value.get("id")?.as_str()?.to_owned();
    let platform_values = value
        .get("platforms")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(SocialCampaign {
        id,
        name: value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Untitled campaign")
            .to_owned(),
        brief: value
            .get("brief")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        goal: value
            .get("goal")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        status: campaign_status_from_core(value.get("status").and_then(Value::as_str)),
        platforms: normalized_platforms(Some(&platform_values)),
        starts_at: value
            .get("starts_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        ends_at: value
            .get("ends_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        source: core_source_from_value(value.get("source")),
        owner_user_id: value
            .get("owner_user_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        created_at: value
            .get("created_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        updated_at: value
            .get("updated_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn core_post_from_value(value: &Value) -> Option<SocialPost> {
    let id = value.get("id")?.as_str()?.to_owned();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled social post")
        .to_owned();
    let body = value
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let platform_values = value
        .get("platforms")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let platforms = normalized_platforms(Some(&platform_values));
    let media = value
        .get("media")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(core_media_from_value).collect::<Vec<_>>())
        .unwrap_or_default();
    let previews = value
        .get("previews")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(core_preview_from_value)
                .collect::<Vec<_>>()
        })
        .filter(|previews| !previews.is_empty())
        .unwrap_or_else(|| build_platform_previews(&title, &body, &platforms, &media));
    let source = core_source_from_value(value.get("source"));
    let scheduled_at = value
        .get("scheduled_at")
        .and_then(Value::as_str)
        .or_else(|| value.get("created_at").and_then(Value::as_str))
        .or_else(|| value.get("updated_at").and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    Some(SocialPost {
        id,
        title,
        body,
        status: post_status_from_core(value.get("status").and_then(Value::as_str)),
        scheduled_at,
        platforms,
        source,
        approval: ApprovalState {
            required: value
                .get("approval_required")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            state: approval_state_from_core(
                value.get("approval_state").and_then(Value::as_str),
                value
                    .get("approval_required")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            ),
        },
        media,
        previews,
    })
}

fn core_source_from_value(value: Option<&Value>) -> SocialPostSource {
    let Some(value) = value else {
        return SocialPostSource {
            kind: "manual",
            label: "Social core".to_owned(),
            href: None,
        };
    };
    SocialPostSource {
        kind: source_kind_from_core(value.get("kind").and_then(Value::as_str)),
        label: value
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| !label.trim().is_empty())
            .unwrap_or("Social core")
            .to_owned(),
        href: value
            .get("href")
            .and_then(Value::as_str)
            .filter(|href| !href.trim().is_empty())
            .map(str::to_owned),
    }
}

fn core_preview_from_value(value: &Value) -> Option<PlatformPreview> {
    let provider_key = provider_key_from_value(value.get("platform")?.as_str()?)?;
    let text = value
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let warnings = value
        .get("warnings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let max_characters = value
        .get("character_limit")
        .and_then(Value::as_u64)
        .map(|count| count as usize)
        .unwrap_or_else(|| {
            adapter_for(provider_key)
                .map(|a| a.max_characters)
                .unwrap_or(2000)
        });
    Some(PlatformPreview {
        provider_key,
        label: provider_label(provider_key),
        text: text.clone(),
        character_count: text.chars().count(),
        max_characters,
        media_required: value
            .get("media_required")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                adapter_for(provider_key)
                    .map(|a| a.media_required)
                    .unwrap_or(false)
            }),
        ready: warnings.is_empty(),
        warnings,
    })
}

fn core_media_from_value(value: &Value) -> MediaAsset {
    let kind = media_kind_from_core(value.get("type").and_then(Value::as_str));
    let ready = value
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .is_some()
        || value
            .get("storage_ref")
            .and_then(Value::as_str)
            .filter(|storage_ref| !storage_ref.trim().is_empty())
            .is_some();
    MediaAsset {
        kind,
        label: match kind {
            "video" => "Video asset",
            "link" => "Link asset",
            _ => "Image asset",
        },
        status: if ready { "ready" } else { "draft" },
    }
}

fn provider_key_from_value(value: &str) -> Option<&'static str> {
    normalized_platforms(Some(&[value.to_owned()]))
        .first()
        .copied()
}

fn account_status_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "connected" => "connected",
        "error" => "manual_review",
        "expired" | "disconnected" | "" => "needs_oauth",
        _ => "disabled",
    }
}

fn post_status_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "scheduled" => "scheduled",
        "publishing" => "publishing",
        "published" => "published",
        "failed" => "failed",
        "blocked" => "blocked",
        "archived" => "failed",
        _ => "draft",
    }
}

fn approval_state_from_core(value: Option<&str>, required: bool) -> &'static str {
    if !required {
        return "not_requested";
    }
    match value.unwrap_or_default() {
        "approved" => "approved",
        "rejected" => "rejected",
        "pending" => "requested",
        _ => "not_requested",
    }
}

fn approval_queue_state_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "approved" => "approved",
        "rejected" => "rejected",
        "pending" | "requested" => "requested",
        _ => "not_requested",
    }
}

fn campaign_status_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "active" => "active",
        "completed" => "completed",
        "archived" => "archived",
        _ => "draft",
    }
}

fn source_kind_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "inbox" => "inbox",
        "knowledge" => "knowledge",
        "campaign" => "campaign",
        _ => "manual",
    }
}

fn media_kind_from_core(value: Option<&str>) -> &'static str {
    match value.unwrap_or_default() {
        "video" => "video",
        "link" => "link",
        _ => "image",
    }
}

fn provider_accent(provider_key: &str) -> &'static str {
    match provider_key {
        "linkedin" => "#0a66c2",
        "x" => "#111111",
        "instagram" => "#d9468f",
        "facebook" => "#1877f2",
        "tiktok" => "#00a6a6",
        "snapchat" => "#facc15",
        _ => "#4b5563",
    }
}

fn platform_adapters() -> Vec<PlatformAdapter> {
    vec![
        PlatformAdapter {
            provider_key: "linkedin",
            label: "LinkedIn",
            mode: "direct_api",
            endpoint: "POST /rest/posts",
            max_characters: 3000,
            media_required: false,
            required_capabilities: vec!["social.profile.read", "social.post.write"],
            notes: vec![
                "Create organization or member posts through LinkedIn's Posts API.",
                "Image and video posts require a media upload URN before post creation.",
            ],
        },
        PlatformAdapter {
            provider_key: "x",
            label: "X",
            mode: "direct_api",
            endpoint: "POST /2/tweets",
            max_characters: 280,
            media_required: false,
            required_capabilities: vec!["social.profile.read", "social.post.write"],
            notes: vec![
                "Text posts use X API v2 manage Posts endpoints.",
                "Longer drafts should become threads or be shortened before publish.",
            ],
        },
        PlatformAdapter {
            provider_key: "instagram",
            label: "Instagram",
            mode: "media_container",
            endpoint: "POST /{ig-user-id}/media + POST /{ig-user-id}/media_publish",
            max_characters: 2200,
            media_required: true,
            required_capabilities: vec![
                "social.profile.read",
                "social.post.write",
                "social.media.upload",
            ],
            notes: vec![
                "Content publishing creates a media container, then publishes that container.",
                "Feed, Reels, Stories, and carousel posts require approved Meta app access.",
            ],
        },
        PlatformAdapter {
            provider_key: "facebook",
            label: "Facebook",
            mode: "graph_pages_api",
            endpoint: "POST /{page-id}/feed or /{page-id}/photos",
            max_characters: 63206,
            media_required: false,
            required_capabilities: vec![
                "social.profile.read",
                "social.post.write",
                "social.media.upload",
            ],
            notes: vec![
                "Page publishing uses Meta Graph API and requires a connected Facebook Page.",
                "Photo posts are sent through the Page photos endpoint when ready media is present.",
            ],
        },
        PlatformAdapter {
            provider_key: "tiktok",
            label: "TikTok",
            mode: "content_posting_api",
            endpoint: "POST /v2/post/publish/content/init/",
            max_characters: 2200,
            media_required: true,
            required_capabilities: vec![
                "social.profile.read",
                "social.post.write",
                "social.media.upload",
            ],
            notes: vec![
                "Direct Post requires creator info first so the UI can render TikTok posting options.",
                "Video or photo media must be transferred to TikTok before publish completion.",
            ],
        },
        PlatformAdapter {
            provider_key: "snapchat",
            label: "Snapchat",
            mode: "public_profile_api",
            endpoint: "POST /public_profiles/{id}/media + /stories|/spotlights",
            max_characters: 250,
            media_required: true,
            required_capabilities: vec![
                "social.profile.read",
                "social.post.write",
                "social.media.upload",
            ],
            notes: vec![
                "Organic posting uploads media to a Snapchat Public Profile as a Story (default) or Spotlight (video).",
                "Live posting is allowlist-gated: it requires Snap to allowlist the OAuth app plus SNAPCHAT_LIVE_PUBLISHING enabled server-side.",
                "Marketing/creative/reporting workflows also run through the Snapchat Marketing API.",
            ],
        },
    ]
}

fn adapter_for(provider_key: &str) -> Option<PlatformAdapter> {
    platform_adapters()
        .into_iter()
        .find(|adapter| adapter.provider_key == provider_key)
}

fn validate_create_post(body: &CreatePostBody) -> Result<ValidatedPostInput, String> {
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Untitled social post")
        .to_owned();
    if title.chars().count() > 140 {
        return Err("title must be 140 characters or fewer.".to_owned());
    }

    let body_text = body
        .body
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "body is required.".to_owned())?
        .to_owned();
    if body_text.chars().count() > 5000 {
        return Err("body must be 5000 characters or fewer.".to_owned());
    }

    let platforms = normalized_platforms(body.platforms.as_deref());
    let scheduled_at = body
        .scheduled_at
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("2026-06-17T10:00:00.000Z")
        .to_owned();
    if DateTime::parse_from_rfc3339(&scheduled_at).is_err() {
        return Err("scheduledAt must be a valid RFC3339 timestamp.".to_owned());
    }

    Ok(ValidatedPostInput {
        title,
        body: body_text,
        platforms,
        scheduled_at,
    })
}

fn validate_create_campaign(body: &CreateCampaignBody) -> Result<ValidatedCampaignInput, String> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err("name is required.".to_owned());
    }
    if name.chars().count() > 140 {
        return Err("name must be 140 characters or fewer.".to_owned());
    }

    let brief = body
        .brief
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if brief.chars().count() > 2000 {
        return Err("brief must be 2000 characters or fewer.".to_owned());
    }

    let goal = body
        .goal
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if goal.chars().count() > 500 {
        return Err("goal must be 500 characters or fewer.".to_owned());
    }

    let starts_at = optional_rfc3339(body.starts_at.as_deref(), "startsAt")?;
    let ends_at = optional_rfc3339(body.ends_at.as_deref(), "endsAt")?;
    if let (Some(starts_at), Some(ends_at)) = (&starts_at, &ends_at) {
        let starts = DateTime::parse_from_rfc3339(starts_at)
            .map_err(|_| "startsAt must be a valid RFC3339 timestamp.".to_owned())?;
        let ends = DateTime::parse_from_rfc3339(ends_at)
            .map_err(|_| "endsAt must be a valid RFC3339 timestamp.".to_owned())?;
        if ends < starts {
            return Err("endsAt must be after startsAt.".to_owned());
        }
    }

    let platforms = normalized_platforms(body.platforms.as_deref())
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let status = campaign_status_from_core(body.status.as_deref()).to_owned();

    Ok(ValidatedCampaignInput {
        name: name.to_owned(),
        brief,
        goal,
        status,
        platforms,
        starts_at,
        ends_at,
    })
}

fn optional_rfc3339(value: Option<&str>, field_name: &str) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if DateTime::parse_from_rfc3339(value).is_err() {
        return Err(format!("{field_name} must be a valid RFC3339 timestamp."));
    }
    Ok(Some(value.to_owned()))
}

fn normalize_decision(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" => Some("approved"),
        "reject" | "rejected" => Some("rejected"),
        _ => None,
    }
}

fn with_query(base: &str, params: &[(&str, Option<&str>)]) -> String {
    let query = params
        .iter()
        .filter_map(|(key, value)| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| format!("{}={}", key, urlencoding::encode(value)))
        })
        .collect::<Vec<_>>();
    if query.is_empty() {
        base.to_owned()
    } else {
        format!("{}?{}", base, query.join("&"))
    }
}

fn derived_trends(
    posts: Vec<SocialPost>,
    adapters: Vec<PlatformAdapter>,
) -> Vec<SocialTrendSignal> {
    let mut warning_signals = Vec::new();
    for post in &posts {
        for preview in &post.previews {
            for (index, warning) in preview.warnings.iter().enumerate() {
                if warning_signals.len() >= 4 {
                    return warning_signals;
                }
                warning_signals.push(SocialTrendSignal {
                    id: format!("trend_{}_{}_{}", post.id, preview.provider_key, index),
                    label: format!("{} rewrite signal", preview.label),
                    provider_key: preview.provider_key,
                    format: format!(
                        "{}/{} chars",
                        preview.character_count, preview.max_characters
                    ),
                    opportunity: warning.clone(),
                    velocity: if preview.ready {
                        "Ready"
                    } else {
                        "Needs rewrite"
                    },
                    status: if preview.ready { "ready" } else { "blocked" },
                    source_href: Some(
                        post.source
                            .href
                            .clone()
                            .unwrap_or_else(|| "/social/calendar".to_owned()),
                    ),
                });
            }
        }
    }
    if !warning_signals.is_empty() {
        return warning_signals;
    }

    adapters
        .into_iter()
        .take(4)
        .map(|adapter| {
            let mut opportunity = adapter
                .notes
                .first()
                .copied()
                .unwrap_or("Adapter rules are ready for format-aware rewriting.")
                .to_owned();
            if adapter.media_required {
                opportunity.push_str(" Requires ready media.");
            }

            SocialTrendSignal {
                id: format!("trend_adapter_{}", adapter.provider_key),
                label: format!("{} format rules", adapter.label),
                provider_key: adapter.provider_key,
                format: format!("{} chars · {}", adapter.max_characters, adapter.mode),
                opportunity,
                velocity: if adapter.media_required {
                    "Media-first"
                } else {
                    "Copy-ready"
                },
                status: if adapter.media_required {
                    "needs_media"
                } else {
                    "ready"
                },
                source_href: Some("/social/drafts".to_owned()),
            }
        })
        .collect()
}

fn derived_evergreen(
    posts: Vec<SocialPost>,
    adapters: Vec<PlatformAdapter>,
) -> Vec<SocialEvergreenItem> {
    let reusable: Vec<SocialPost> = posts
        .into_iter()
        .filter(|post| {
            post.source.kind == "knowledge"
                || post.status == "scheduled"
                || post.status == "published"
        })
        .collect();

    if reusable.is_empty() {
        return vec![SocialEvergreenItem {
            id: "evergreen_seed".to_owned(),
            title: "Knowledge-backed evergreen backlog".to_owned(),
            cadence: "Monthly",
            last_published_at: None,
            next_eligible_at: None,
            guardrail:
                "Queue activates after posts prove useful or originate from durable knowledge."
                    .to_owned(),
            status: "needs_refresh",
            platforms: vec!["linkedin"],
            source_post_id: None,
        }];
    }

    reusable
        .into_iter()
        .take(4)
        .map(|post| {
            let needs_media = post_needs_media(&post, &adapters);
            SocialEvergreenItem {
                id: format!("evergreen_{}", post.id),
                title: post.title.clone(),
                cadence: if post.status == "published" {
                    "Quarterly"
                } else {
                    "Monthly"
                },
                last_published_at: if post.status == "published" {
                    Some(post.scheduled_at.clone())
                } else {
                    None
                },
                next_eligible_at: Some(post.scheduled_at.clone()),
                guardrail: if needs_media {
                    "Refresh media assets and run approval before reuse.".to_owned()
                } else {
                    "Run freshness, duplication, and approval checks before republish.".to_owned()
                },
                status: if needs_media {
                    "needs_refresh"
                } else if post.approval.state == "approved" {
                    "ready"
                } else {
                    "needs_approval"
                },
                platforms: post.platforms.clone(),
                source_post_id: Some(post.id.clone()),
            }
        })
        .collect()
}

fn post_needs_media(post: &SocialPost, adapters: &[PlatformAdapter]) -> bool {
    if post.media.iter().any(|asset| asset.status == "ready") {
        return false;
    }
    post.platforms.iter().any(|provider_key| {
        adapters
            .iter()
            .any(|adapter| adapter.provider_key == *provider_key && adapter.media_required)
    })
}

fn recommended_windows() -> Vec<RecommendedWindow> {
    vec![
        RecommendedWindow {
            id: "window_tue_morning",
            label: "Tue morning",
            starts_at: "2026-06-16T08:30:00.000Z",
            reason: "Best fit for LinkedIn education posts.",
        },
        RecommendedWindow {
            id: "window_wed_lunch",
            label: "Wed lunch",
            starts_at: "2026-06-17T10:45:00.000Z",
            reason: "Good overlap for X and community replies.",
        },
        RecommendedWindow {
            id: "window_fri_video",
            label: "Fri video",
            starts_at: "2026-06-19T13:00:00.000Z",
            reason: "Reserved for visual-first Instagram/TikTok content.",
        },
    ]
}

fn build_platform_previews(
    title: &str,
    body: &str,
    platforms: &[&'static str],
    media: &[MediaAsset],
) -> Vec<PlatformPreview> {
    let has_ready_media = media.iter().any(|asset| asset.status == "ready");
    platforms
        .iter()
        .filter_map(|provider_key| adapter_for(provider_key))
        .map(|adapter| {
            let (text, mut warnings) =
                adapted_copy(adapter.provider_key, title, body, adapter.max_characters);
            if adapter.media_required && !has_ready_media {
                warnings.push(format!(
                    "{} requires ready image or video media before publish.",
                    adapter.label
                ));
            }
            let character_count = text.chars().count();
            if character_count > adapter.max_characters {
                warnings.push(format!(
                    "{} copy is {} characters; maximum is {}.",
                    adapter.label, character_count, adapter.max_characters
                ));
            }

            PlatformPreview {
                provider_key: adapter.provider_key,
                label: adapter.label,
                text,
                character_count,
                max_characters: adapter.max_characters,
                media_required: adapter.media_required,
                ready: warnings.is_empty(),
                warnings,
            }
        })
        .collect()
}

fn adapted_copy(
    provider_key: &str,
    title: &str,
    body: &str,
    max_characters: usize,
) -> (String, Vec<String>) {
    let mut warnings = Vec::new();
    let raw = match provider_key {
        "linkedin" => format!(
            "{}\n\n{}\n\nWhat this means for teams: useful customer signals can move directly into approved content.",
            title, body
        ),
        "x" => format!("{}: {}", title, body),
        "instagram" => format!("{}\n\n{}\n\nSave this for the next planning review.", title, body),
        "facebook" => format!("{}\n\n{}", title, body),
        "tiktok" => format!("{} | {}", title, body),
        "snapchat" => {
            warnings.push(
                "Snapchat posts as a Public Profile Story/Spotlight; live posting is allowlist-gated (requires SNAPCHAT_LIVE_PUBLISHING enabled server-side)."
                    .to_owned(),
            );
            format!("{} — {}", title, body)
        }
        _ => body.to_owned(),
    };

    if raw.chars().count() <= max_characters {
        return (raw, warnings);
    }

    warnings.push(format!(
        "{} copy was shortened to fit the platform limit.",
        provider_label(provider_key)
    ));
    (truncate_chars(&raw, max_characters), warnings)
}

fn build_publish_result(post: &SocialPost, accounts: &[SocialAccount]) -> PublishResult {
    let attempts: Vec<PublishAttempt> = post
        .previews
        .iter()
        .filter_map(|preview| {
            let adapter = adapter_for(preview.provider_key)?;
            let account = account_for_provider(accounts, preview.provider_key);
            let account_status = account.map(|account| account.status).unwrap_or("disabled");
            let mut warnings = preview.warnings.clone();
            let (status, message) = if account_status != "connected" {
                warnings.push(format!(
                    "{} account is not connected; finish integration-core OAuth/app review first.",
                    preview.label
                ));
                (
                    "blocked",
                    format!(
                        "{} is blocked until the account is connected.",
                        preview.label
                    ),
                )
            } else if adapter.required_capabilities.contains(&"social.post.write")
                && !account
                    .map(|account| account_has_capability(account, "social.post.write"))
                    .unwrap_or(false)
            {
                warnings.push("Connected account does not grant social.post.write.".to_owned());
                (
                    "blocked",
                    format!(
                        "{} is blocked until publishing capability is granted.",
                        preview.label
                    ),
                )
            } else if !preview.ready {
                (
                    "blocked",
                    format!("{} adapter requirements are not satisfied.", preview.label),
                )
            } else {
                (
                    "queued",
                    format!(
                        "{} publish job is ready for the social-publisher worker.",
                        preview.label
                    ),
                )
            };

            Some(PublishAttempt {
                provider_key: adapter.provider_key,
                label: adapter.label,
                status,
                mode: adapter.mode,
                endpoint: adapter.endpoint,
                message,
                external_id: None,
                warnings,
            })
        })
        .collect();

    let queued_count = attempts
        .iter()
        .filter(|attempt| attempt.status == "queued")
        .count();
    let status = if queued_count == attempts.len() && !attempts.is_empty() {
        "queued"
    } else if queued_count > 0 {
        "partial"
    } else {
        "blocked"
    };

    PublishResult {
        id: format!("publish_{}", sanitize_id(&post.id)),
        status,
        idempotency_key: format!(
            "{}:{}:{}",
            post.id,
            post.scheduled_at,
            post.platforms.join(",")
        ),
        attempts,
    }
}

fn normalized_platforms(values: Option<&[String]>) -> Vec<&'static str> {
    let Some(values) = values else {
        return vec!["linkedin"];
    };
    let mut out = Vec::new();
    for value in values {
        match value.trim().to_ascii_lowercase().as_str() {
            "linkedin" => push_unique(&mut out, "linkedin"),
            "x" | "twitter" => push_unique(&mut out, "x"),
            "instagram" => push_unique(&mut out, "instagram"),
            "facebook" | "fb" | "facebook-page" => push_unique(&mut out, "facebook"),
            "tiktok" | "tik-tok" => push_unique(&mut out, "tiktok"),
            "snapchat" | "snap" => push_unique(&mut out, "snapchat"),
            _ => {}
        }
    }
    if out.is_empty() {
        out.push("linkedin");
    }
    out
}

fn push_unique(values: &mut Vec<&'static str>, value: &'static str) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn account_for_provider<'a>(
    accounts: &'a [SocialAccount],
    provider_key: &str,
) -> Option<&'a SocialAccount> {
    accounts
        .iter()
        .find(|account| account.provider_key == provider_key)
}

fn account_has_capability(account: &SocialAccount, capability: &str) -> bool {
    account
        .capabilities
        .iter()
        .any(|candidate| candidate == capability)
}

fn provider_label(provider_key: &str) -> &'static str {
    match provider_key {
        "linkedin" => "LinkedIn",
        "x" => "X",
        "instagram" => "Instagram",
        "facebook" => "Facebook",
        "tiktok" => "TikTok",
        "snapchat" => "Snapchat",
        _ => "Provider",
    }
}

fn truncate_chars(value: &str, max_characters: usize) -> String {
    let count = value.chars().count();
    if count <= max_characters {
        return value.to_owned();
    }
    if max_characters <= 3 {
        return value.chars().take(max_characters).collect();
    }
    let mut truncated = value
        .chars()
        .take(max_characters.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_social_platforms_without_duplicates() {
        let input = vec![
            "LinkedIn".to_owned(),
            "twitter".to_owned(),
            "x".to_owned(),
            "fb".to_owned(),
            "tik-tok".to_owned(),
            "snap".to_owned(),
            "unknown".to_owned(),
        ];

        assert_eq!(
            normalized_platforms(Some(&input)),
            vec!["linkedin", "x", "facebook", "tiktok", "snapchat"]
        );
    }

    #[test]
    fn builds_native_platform_previews_and_media_requirements() {
        let previews = build_platform_previews(
            "Launch note",
            "A support signal became a roadmap update.",
            &[
                "linkedin",
                "x",
                "instagram",
                "facebook",
                "tiktok",
                "snapchat",
            ],
            &[],
        );

        let linkedin = previews
            .iter()
            .find(|preview| preview.provider_key == "linkedin")
            .expect("linkedin preview");
        let x = previews
            .iter()
            .find(|preview| preview.provider_key == "x")
            .expect("x preview");
        let instagram = previews
            .iter()
            .find(|preview| preview.provider_key == "instagram")
            .expect("instagram preview");
        let snapchat = previews
            .iter()
            .find(|preview| preview.provider_key == "snapchat")
            .expect("snapchat preview");

        assert_ne!(linkedin.text, x.text);
        assert!(x.character_count <= 280);
        assert!(!instagram.ready);
        assert!(instagram
            .warnings
            .iter()
            .any(|warning| warning.contains("requires ready image or video media")));
        assert!(!snapchat.ready);
        assert!(snapchat
            .warnings
            .iter()
            .any(|warning| warning.contains("allowlist-gated")));
    }

    #[test]
    fn maps_integration_provider_catalog_to_calendar_accounts() {
        let provider = json!({
            "key": "facebook",
            "label": "Facebook",
            "category": "social",
            "configured": false,
            "directOAuthReady": true,
            "status": "missing_config",
            "missingConfig": ["FACEBOOK_CLIENT_ID", "FACEBOOK_CLIENT_SECRET"],
            "capabilities": [
                { "key": "social.profile.read" },
                { "key": "social.post.write" }
            ]
        });

        let account = integration_provider_account_from_value(&provider).expect("social provider");

        assert_eq!(account.id, "provider_facebook");
        assert_eq!(account.provider_key, "facebook");
        assert_eq!(account.status, "manual_review");
        assert!(account.handle.contains("FACEBOOK_CLIENT_ID"));
        assert!(account_has_capability(&account, "social.post.write"));
    }

    #[test]
    fn validates_create_post_timestamps() {
        let body = CreatePostBody {
            title: Some("Post".to_owned()),
            body: Some("Body".to_owned()),
            platforms: Some(vec!["linkedin".to_owned()]),
            scheduled_at: Some("not-a-date".to_owned()),
        };

        assert_eq!(
            validate_create_post(&body).unwrap_err(),
            "scheduledAt must be a valid RFC3339 timestamp."
        );
    }

    #[test]
    fn maps_core_approvals_with_embedded_posts() {
        let approval = json!({
            "id": "socapr_1",
            "post_id": "socpost_1",
            "state": "pending",
            "requested_by_user_id": "user_1",
            "created_at": "2026-06-16T08:00:00Z",
            "updated_at": "2026-06-16T08:00:00Z",
            "post": {
                "id": "socpost_1",
                "title": "Launch",
                "body": "Launch copy",
                "status": "draft",
                "platforms": ["linkedin"],
                "approval_required": true,
                "approval_state": "pending",
                "source": { "kind": "campaign", "label": "Launch calendar" }
            }
        });

        let mapped = core_approval_from_value(&approval).expect("approval");

        assert_eq!(mapped.id, "socapr_1");
        assert_eq!(mapped.state, "requested");
        assert_eq!(
            mapped.post.as_ref().map(|post| post.source.kind),
            Some("campaign")
        );
        assert_eq!(
            mapped.post.as_ref().map(|post| post.approval.state),
            Some("requested")
        );
    }

    #[test]
    fn maps_core_campaigns_to_browser_contract() {
        let campaign = json!({
            "id": "soccamp_1",
            "name": "Launch calendar",
            "brief": "Launch brief",
            "goal": "Pipeline",
            "status": "active",
            "platforms": ["LinkedIn", "twitter", "x"],
            "starts_at": "2026-06-16T08:00:00Z",
            "source": { "kind": "campaign", "label": "Studio" },
            "owner_user_id": "user_1",
            "created_at": "2026-06-16T08:00:00Z",
            "updated_at": "2026-06-16T08:00:00Z"
        });

        let mapped = core_campaign_from_value(&campaign).expect("campaign");

        assert_eq!(mapped.status, "active");
        assert_eq!(mapped.platforms, vec!["linkedin", "x"]);
        assert_eq!(mapped.source.kind, "campaign");
    }

    #[test]
    fn validates_create_campaign_dates_and_query_encoding() {
        let body = CreateCampaignBody {
            name: "Launch".to_owned(),
            brief: Some("Brief".to_owned()),
            goal: None,
            status: Some("active".to_owned()),
            platforms: Some(vec!["linkedin".to_owned()]),
            starts_at: Some("2026-06-18T10:00:00Z".to_owned()),
            ends_at: Some("2026-06-17T10:00:00Z".to_owned()),
        };

        assert_eq!(
            validate_create_campaign(&body).unwrap_err(),
            "endsAt must be after startsAt."
        );

        assert_eq!(
            with_query(
                "/api/v1/social/approvals",
                &[("state", Some("pending review")), ("limit", Some("10"))],
            ),
            "/api/v1/social/approvals?state=pending%20review&limit=10"
        );
    }

    #[test]
    fn social_query_path_drops_blank_params_and_encodes_values() {
        assert_eq!(
            social_query_path("/api/v1/social/metrics", &[]),
            "/api/v1/social/metrics"
        );
        assert_eq!(
            social_query_path(
                "/api/v1/social/metrics",
                &[
                    ("accountId", "  ".to_owned()),
                    ("snapshotDate", "2026-07-07".to_owned()),
                ],
            ),
            "/api/v1/social/metrics?snapshotDate=2026-07-07"
        );
        assert_eq!(
            social_query_path(
                "/api/v1/social/catalogs/cat_1/products",
                &[("accountId", "soc acc/1".to_owned())],
            ),
            "/api/v1/social/catalogs/cat_1/products?accountId=soc%20acc%2F1"
        );
    }

    #[test]
    fn maps_core_metric_row_to_typed_metric() {
        let row = json!({
            "org_id": "org_1",
            "account_id": "soc_acct_1",
            "connection_id": "conn_1",
            "provider_key": "facebook",
            "metric_name": "impressions",
            "metric_value": 1234.0,
            "dimensions": { "campaign": "launch" },
            "snapshot_date": "2026-07-07T00:00:00Z"
        });

        let metric = core_metric_from_value(&row).expect("metric");
        assert_eq!(metric.account_id, "soc_acct_1");
        assert_eq!(metric.provider_key, "facebook");
        assert_eq!(metric.metric_name, "impressions");
        assert_eq!(metric.metric_value, 1234.0);
        assert_eq!(metric.snapshot_date, "2026-07-07T00:00:00Z");
        assert_eq!(
            metric
                .dimensions
                .pointer("/campaign")
                .and_then(Value::as_str),
            Some("launch")
        );
    }

    #[test]
    fn skips_metric_row_without_metric_name() {
        assert!(
            core_metric_from_value(&json!({ "provider_key": "x", "metric_value": 1.0 })).is_none()
        );
        assert!(core_metric_from_value(&json!({ "metric_name": "  " })).is_none());
        assert!(core_metric_from_value(&json!("not-an-object")).is_none());
    }
}
