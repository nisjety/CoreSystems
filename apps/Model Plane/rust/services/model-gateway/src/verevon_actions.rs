//! Verevon READ actions reachable from the inline chat tool loop.
//!
//! This is the "AI-first" bridge: data a signed-in Verevon user can see in the
//! UI should also be answerable in chat. Each function here maps one question
//! ("which social accounts are connected?", "what is in our knowledge base?")
//! onto the SAME upstream the BFF gateway's `domains/*` modules call, so chat
//! and UI read one source of truth.
//!
//! Three invariants hold for every function in this module:
//!
//! 1. **Read-only.** Nothing here mutates upstream state. Write actions
//!    (`tickets.create`, `social.publish_post`, `workflows.toggle_policy`, …)
//!    are deliberately absent: the inline loop has no human-approval gate, so
//!    side effects must go through execution-core's governed agentic path
//!    (`mode: "ask"`) where capability policy and approval actually apply.
//! 2. **Tenant-scoped by the caller's verified context.** `org_id` is always
//!    the org from the verified request — never a model-supplied argument.
//!    There is intentionally no `org_id` field in any tool's input schema, so
//!    a model cannot even express "read another tenant".
//! 3. **Bounded output.** Every list is row-capped and text-truncated before it
//!    reaches the model, because the tool loop re-sends the whole accumulated
//!    history on each round — an untruncated 500-row list is paid for again on
//!    every subsequent round.
//!
//! Failures return `Err(String)` naming the actual cause (unreachable, HTTP
//! status, decode failure). Never an empty success that reads like "you have no
//! data" when the truth is "the upstream could not be asked".

use mp_contracts::dataplane::documents_v2 as doc_pb;
use serde_json::{json, Value};

use crate::{auth::VerifiedDataPlaneBearer as VerifiedBearer, state::AppState};

/// Max rows any one list tool inlines into the conversation.
const MAX_ROWS: usize = 25;

/// Max chars for a free-text field (post body, document title) in a summary.
const MAX_FIELD_CHARS: usize = 180;

/// Default/max `limit` accepted from the model for a list call.
const DEFAULT_LIMIT: i64 = 25;
const MAX_LIMIT: i64 = 100;

/// Post statuses social-core recognises (`internal/social/types.go`). Validated
/// locally so a typo becomes an actionable tool error instead of a silently
/// empty upstream result the model would report as "no posts".
const POST_STATUSES: [&str; 6] = [
    "draft",
    "pending_approval",
    "scheduled",
    "publishing",
    "published",
    "failed",
];

/// Campaign statuses social-core recognises (`internal/social/types.go`).
const CAMPAIGN_STATUSES: [&str; 4] = ["draft", "active", "completed", "archived"];

fn trim_field(value: &Value) -> Value {
    match value.as_str() {
        Some(text) => {
            let trimmed = text.trim();
            if trimmed.chars().count() <= MAX_FIELD_CHARS {
                Value::String(trimmed.to_owned())
            } else {
                let mut out: String = trimmed.chars().take(MAX_FIELD_CHARS).collect();
                out.push('…');
                Value::String(out)
            }
        }
        None => value.clone(),
    }
}

/// Project an upstream row onto an allow-listed set of keys, truncating text.
///
/// Deliberately an allow-list rather than a redaction list: upstream rows carry
/// fields the model has no business seeing (`connection_id`, raw `metadata`,
/// provider tokens' expiry bookkeeping), and an allow-list cannot leak a field
/// added upstream later.
fn project(row: &Value, keys: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    for key in keys {
        if let Some(value) = row.get(*key) {
            if !value.is_null() {
                out.insert((*key).to_owned(), trim_field(value));
            }
        }
    }
    Value::Object(out)
}

/// Row-cap a list and report what was withheld, so the model can say "showing
/// 25 of 312" instead of silently treating a truncated page as the whole set.
fn capped_rows(rows: &[Value], keys: &[&str]) -> (Vec<Value>, usize) {
    let shown: Vec<Value> = rows
        .iter()
        .take(MAX_ROWS)
        .map(|r| project(r, keys))
        .collect();
    (shown, rows.len().saturating_sub(MAX_ROWS))
}

fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Validate an optional enum-ish filter against an allow-list.
fn validate_filter(name: &str, value: &str, allowed: &[&str]) -> Result<Option<String>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let lowered = value.to_lowercase();
    if allowed.contains(&lowered.as_str()) {
        Ok(Some(lowered))
    } else {
        Err(format!(
            "unsupported {name} '{value}' — valid values are: {}",
            allowed.join(", ")
        ))
    }
}

/// GET an Application Plane core that gates on the shared internal API key,
/// scoping the read to `org_id` via the `x-org-id` header those cores read
/// (`requireOrgID` in insight-core / social-core).
///
/// `org_id` comes from the verified request context at the call site; this
/// function refuses an empty one rather than sending an unscoped read, because
/// both cores treat a missing org as a 400 and the failure mode we must never
/// have is a read that accidentally spans tenants.
async fn application_core_get(
    state: &AppState,
    base_url: &str,
    path: &str,
    org_id: &str,
    query: &[(&str, String)],
    upstream: &str,
) -> Result<Value, String> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err(format!(
            "{upstream} read requires a verified organization; this chat turn has none"
        ));
    }
    if state.application_core_internal_key.trim().is_empty() {
        return Err(format!(
            "{upstream} is not configured for this deployment (APPLICATION_CORE_INTERNAL_KEY / INTERNAL_API_KEY is unset)"
        ));
    }
    let url = format!("{}{path}", base_url.trim_end_matches('/'));
    let response = state
        .http_client
        .get(&url)
        .query(query)
        .header("x-internal-api-key", &state.application_core_internal_key)
        .header("x-org-id", org_id)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("{upstream} unreachable: {err}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("{upstream} returned HTTP {}", status.as_u16()));
    }
    response
        .json::<Value>()
        .await
        .map_err(|err| format!("{upstream} returned an unparseable response: {err}"))
}

/// Unwrap the `{ "data": … }` envelope both cores use.
fn envelope_data(body: &Value) -> &Value {
    body.get("data").unwrap_or(&Value::Null)
}

/// Rows from the `{ "data": [...] }` envelope.
///
/// The length of this is the number of rows the upstream RETURNED for the
/// requested page — never an org-wide total, because social-core applies the
/// `limit` before responding and reports no unfiltered count. Callers must not
/// label it `total`: a model told "total: 25" will confidently answer "you have
/// 25 posts" when the org has 300.
fn data_rows(body: &Value) -> Vec<Value> {
    envelope_data(body).as_array().cloned().unwrap_or_default()
}

/// Shared tail for a social-core list read: row-cap, then report the counts in
/// terms that cannot be mistaken for an org-wide total.
fn social_list_payload(
    upstream: &str,
    filters: &Value,
    rows: &[Value],
    keys: &[&str],
    limit: i64,
) -> Value {
    let (items, withheld) = capped_rows(rows, keys);
    // Saturating rather than wrapping: a row count that somehow exceeded i64
    // must read as "definitely page-bounded", never wrap to a small number and
    // silently claim the page was complete.
    let returned = i64::try_from(rows.len()).unwrap_or(i64::MAX);
    json!({
        "upstream": upstream,
        "filters": filters,
        "returned_by_upstream": rows.len(),
        "shown": items.len(),
        "withheld_by_output_cap": withheld,
        "count_is_page_bounded": returned >= limit,
        "note": "Counts describe this page only — the upstream applies `limit` and reports no org-wide total. If count_is_page_bounded is true, more rows almost certainly exist: say so rather than presenting this as a complete count.",
        "items": items,
    })
}

// ============================================================================
// insight-core — Application Plane metrics projection
// ============================================================================

/// `insights.overview` → insight-core `GET /api/v1/insights/overview`.
///
/// Org-scoped by the `x-org-id` header set from the verified request org.
///
/// # Errors
///
/// Returns an error when insight-core is unreachable, rejects the read, or
/// returns a payload that is not the documented `{ data: Overview }` envelope.
pub async fn insights_overview(state: &AppState, org_id: &str) -> Result<String, String> {
    let body = application_core_get(
        state,
        &state.insight_core_base_url,
        "/api/v1/insights/overview",
        org_id,
        &[],
        "insight-core",
    )
    .await?;
    let overview = envelope_data(&body);
    if !overview.is_object() {
        return Err("insight-core returned no overview object for this organization".to_owned());
    }

    // Scorecards are the headline numbers the Insights UI renders; surfaces are
    // the per-area event rollups behind them.
    let scorecards = overview
        .get("scorecards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (scorecards, scorecards_withheld) = capped_rows(
        &scorecards,
        &["label", "surface", "metric", "value", "unit", "source"],
    );

    let surfaces = overview
        .get("surfaces")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (surfaces, surfaces_withheld) =
        capped_rows(&surfaces, &["surface", "total_events", "last_event_at"]);

    // Connector *slots* are a capability catalogue (native / planned /
    // requires_token_lease / disabled), NOT a list of live connections — only
    // the status is reported so the model cannot present a planned connector as
    // an active integration.
    let connectors = overview
        .get("connectors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (connectors, connectors_withheld) =
        capped_rows(&connectors, &["type", "display_name", "surface", "status"]);

    Ok(json!({
        "upstream": "insight-core /api/v1/insights/overview",
        "generated_at": overview.get("generated_at").cloned().unwrap_or(Value::Null),
        "window": overview.get("window").cloned().unwrap_or(Value::Null),
        "scorecards": scorecards,
        "scorecards_withheld": scorecards_withheld,
        "surfaces": surfaces,
        "surfaces_withheld": surfaces_withheld,
        "connector_slots": connectors,
        "connector_slots_withheld": connectors_withheld,
        "connector_lag": overview.get("connector_lag").cloned().unwrap_or(Value::Null),
        "note": "connector_slots is the supported-connector catalogue with wiring status, not a list of live connections.",
    })
    .to_string())
}

// ============================================================================
// social-core — Application Plane social accounts / posts / campaigns
// ============================================================================

/// `social.list_accounts` → social-core `GET /api/v1/social/accounts`.
///
/// Org-scoped by the `x-org-id` header set from the verified request org.
/// `connection_id`, `metadata`, and token expiry are dropped by the projection:
/// the model needs to know an account exists and whether its token is healthy,
/// not the credential bookkeeping behind it.
///
/// # Errors
///
/// Returns an error when social-core is unreachable or rejects the read.
pub async fn social_list_accounts(state: &AppState, org_id: &str) -> Result<String, String> {
    let body = application_core_get(
        state,
        &state.social_core_base_url,
        "/api/v1/social/accounts",
        org_id,
        &[],
        "social-core",
    )
    .await?;
    let rows = data_rows(&body);
    // Accounts is the one social list social-core does NOT paginate (no `limit`
    // parameter on the route), so the returned count IS the org-wide total.
    let (accounts, withheld) = capped_rows(
        &rows,
        &[
            "id",
            "provider_key",
            "display_name",
            "handle",
            "status",
            "capabilities",
            "token_state",
        ],
    );
    Ok(json!({
        "upstream": "social-core /api/v1/social/accounts",
        "total_connected": rows.len(),
        "shown": accounts.len(),
        "withheld_by_output_cap": withheld,
        "accounts": accounts,
    })
    .to_string())
}

/// `social.list_posts` → social-core `GET /api/v1/social/posts`.
///
/// Org-scoped by the `x-org-id` header set from the verified request org; the
/// `status`/`platform` filters are the model's, the org is not.
///
/// # Errors
///
/// Returns an error for an unsupported `status` filter, or when social-core is
/// unreachable or rejects the read.
pub async fn social_list_posts(
    state: &AppState,
    org_id: &str,
    status: &str,
    platform: &str,
    limit: Option<i64>,
) -> Result<String, String> {
    let status = validate_filter("status", status, &POST_STATUSES)?;
    let limit = clamp_limit(limit);
    let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
    if let Some(status) = status.as_ref() {
        query.push(("status", status.clone()));
    }
    let platform = platform.trim();
    if !platform.is_empty() {
        query.push(("platform", platform.to_lowercase()));
    }

    let body = application_core_get(
        state,
        &state.social_core_base_url,
        "/api/v1/social/posts",
        org_id,
        &query,
        "social-core",
    )
    .await?;
    Ok(social_list_payload(
        "social-core /api/v1/social/posts",
        &json!({
            "status": status,
            "platform": (!platform.is_empty()).then(|| platform.to_lowercase()),
            "limit": limit,
        }),
        &data_rows(&body),
        &[
            "id",
            "title",
            "body",
            "status",
            "platforms",
            "approval_required",
            "approval_state",
            "scheduled_at",
            "updated_at",
        ],
        limit,
    )
    .to_string())
}

/// `social.list_campaigns` → social-core `GET /api/v1/social/campaigns`.
///
/// Org-scoped by the `x-org-id` header set from the verified request org.
///
/// # Errors
///
/// Returns an error for an unsupported `status` filter, or when social-core is
/// unreachable or rejects the read.
pub async fn social_list_campaigns(
    state: &AppState,
    org_id: &str,
    status: &str,
    limit: Option<i64>,
) -> Result<String, String> {
    let status = validate_filter("status", status, &CAMPAIGN_STATUSES)?;
    let limit = clamp_limit(limit);
    let mut query: Vec<(&str, String)> = vec![("limit", limit.to_string())];
    if let Some(status) = status.as_ref() {
        query.push(("status", status.clone()));
    }

    let body = application_core_get(
        state,
        &state.social_core_base_url,
        "/api/v1/social/campaigns",
        org_id,
        &query,
        "social-core",
    )
    .await?;
    Ok(social_list_payload(
        "social-core /api/v1/social/campaigns",
        &json!({ "status": status, "limit": limit }),
        &data_rows(&body),
        &[
            "id",
            "name",
            "goal",
            "status",
            "platforms",
            "starts_at",
            "ends_at",
        ],
        limit,
    )
    .to_string())
}

// ============================================================================
// Data Plane v2 — knowledge base document inventory
// ============================================================================

/// `knowledge.list_documents` → Data Plane v2 `DocumentService.ListDocuments`.
///
/// The inventory counterpart to `knowledge_search`: search answers "what does a
/// document SAY", this answers "WHICH documents do we have". Reuses the same
/// gRPC client and the same per-caller `VerifiedDataPlaneBearer` the BFF's
/// `GET /v1/documents` route uses, so Data Plane applies the caller's own
/// document visibility — a shared service credential would silently broaden it.
///
/// Org-scoped twice over: `org_id` in the request body comes from the verified
/// request context, and the bearer Data Plane authorizes carries the same org.
///
/// # Errors
///
/// Returns an error when the bearer is missing, Data Plane rejects the call, or
/// the gRPC read fails.
pub async fn knowledge_list_documents(
    state: &AppState,
    org_id: &str,
    bearer: &VerifiedBearer,
    doc_type: &str,
    limit: Option<i64>,
) -> Result<String, String> {
    let org_id = org_id.trim();
    if org_id.is_empty() {
        return Err(
            "knowledge.list_documents requires a verified organization; this chat turn has none"
                .to_owned(),
        );
    }
    let limit = clamp_limit(limit);
    let request = doc_pb::ListDocumentsRequest {
        org_id: org_id.to_owned(),
        limit: i32::try_from(limit).unwrap_or(25),
        offset: 0,
        r#type: doc_type.trim().to_lowercase(),
    };
    let request = crate::retrieval::authorize(tonic::Request::new(request), bearer)
        .map_err(|status| format!("knowledge.list_documents auth failed: {}", status.message()))?;

    let response = state
        .document_client
        .clone()
        .list_documents(request)
        .await
        .map_err(|status| format!("knowledge.list_documents failed: {}", status.message()))?
        .into_inner();

    // `content` is deliberately NOT projected: a document inventory that inlined
    // full bodies would blow the context this cap exists to protect. Callers who
    // want content use knowledge_search.
    let rows: Vec<Value> = response
        .documents
        .iter()
        .map(|d| {
            json!({
                "document_id": d.document_id,
                "title": trim_field(&Value::String(d.title.clone())),
                "source": d.source,
                "type": d.r#type,
                "status": d.status,
            })
        })
        .collect();
    let (documents, withheld) =
        capped_rows(&rows, &["document_id", "title", "source", "type", "status"]);

    Ok(json!({
        "upstream": "data-plane-v2 DocumentService.ListDocuments",
        "total_in_org": response.total,
        "returned": documents.len(),
        "withheld": withheld,
        "documents": documents,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_field_caps_long_text_and_preserves_non_strings() {
        let short = Value::String("  hello  ".to_owned());
        assert_eq!(trim_field(&short), Value::String("hello".to_owned()));

        let long = Value::String("x".repeat(MAX_FIELD_CHARS + 50));
        let trimmed = trim_field(&long);
        let text = trimmed.as_str().expect("string");
        assert_eq!(
            text.chars().count(),
            MAX_FIELD_CHARS + 1,
            "capped plus ellipsis"
        );
        assert!(text.ends_with('…'));

        // Numbers, arrays and objects pass through untouched.
        assert_eq!(trim_field(&json!(7)), json!(7));
        assert_eq!(trim_field(&json!(["a"])), json!(["a"]));
    }

    #[test]
    fn project_is_an_allow_list_that_drops_unlisted_and_null_fields() {
        let row = json!({
            "id": "post_1",
            "title": "Launch",
            "connection_id": "conn_secret",
            "metadata": { "token": "leak" },
            "handle": Value::Null,
        });
        let projected = project(&row, &["id", "title", "handle"]);
        assert_eq!(projected, json!({ "id": "post_1", "title": "Launch" }));
        assert!(
            projected.get("connection_id").is_none(),
            "unlisted field dropped"
        );
        assert!(
            projected.get("metadata").is_none(),
            "unlisted field dropped"
        );
        assert!(projected.get("handle").is_none(), "null field dropped");
    }

    #[test]
    fn capped_rows_limits_output_and_reports_the_remainder() {
        let rows: Vec<Value> = (0..MAX_ROWS + 7)
            .map(|i| json!({ "id": format!("r{i}"), "drop": "me" }))
            .collect();
        let (shown, withheld) = capped_rows(&rows, &["id"]);
        assert_eq!(shown.len(), MAX_ROWS);
        assert_eq!(withheld, 7);
        assert_eq!(shown[0], json!({ "id": "r0" }));

        let (shown, withheld) = capped_rows(&rows[..3], &["id"]);
        assert_eq!(shown.len(), 3);
        assert_eq!(withheld, 0, "a short list withholds nothing");
    }

    #[test]
    fn clamp_limit_defaults_and_bounds_model_supplied_values() {
        assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(-5)), 1);
        assert_eq!(clamp_limit(Some(10)), 10);
        assert_eq!(clamp_limit(Some(9_999)), MAX_LIMIT);
    }

    #[test]
    fn validate_filter_accepts_allow_listed_values_case_insensitively() {
        assert_eq!(validate_filter("status", "", &POST_STATUSES), Ok(None));
        assert_eq!(validate_filter("status", "  ", &POST_STATUSES), Ok(None));
        assert_eq!(
            validate_filter("status", "SCHEDULED", &POST_STATUSES),
            Ok(Some("scheduled".to_owned()))
        );
        assert_eq!(
            validate_filter("status", "draft", &CAMPAIGN_STATUSES),
            Ok(Some("draft".to_owned()))
        );
    }

    #[test]
    fn validate_filter_rejects_unknown_values_and_names_the_valid_set() {
        let err = validate_filter("status", "postponed", &POST_STATUSES)
            .expect_err("unknown status must be rejected");
        assert!(
            err.contains("postponed"),
            "error names the bad input: {err}"
        );
        assert!(err.contains("scheduled"), "error lists valid values: {err}");
        // A campaign status is not a post status — the allow-lists are distinct.
        assert!(validate_filter("status", "active", &POST_STATUSES).is_err());
        assert!(validate_filter("status", "publishing", &CAMPAIGN_STATUSES).is_err());
    }

    #[test]
    fn envelope_data_and_data_rows_tolerate_a_missing_or_wrong_shaped_envelope() {
        let body = json!({ "data": [{ "id": "a" }, { "id": "b" }] });
        assert_eq!(data_rows(&body).len(), 2);
        // No envelope, or a non-array `data`, must yield an empty list rather
        // than panicking — the callers turn that into an honest zero-row read.
        assert!(data_rows(&json!({})).is_empty());
        assert!(data_rows(&json!({ "data": { "not": "an array" } })).is_empty());
        assert_eq!(envelope_data(&json!({})), &Value::Null);
    }

    #[test]
    fn social_list_payload_never_presents_a_page_count_as_an_org_wide_total() {
        // A full page means "there is probably more". Reporting this as a total
        // is how a model ends up telling a user they have 5 posts when they
        // have 300, so the payload must flag it instead.
        let rows: Vec<Value> = (0..5).map(|i| json!({ "id": format!("p{i}") })).collect();
        let full = social_list_payload("u", &json!({}), &rows, &["id"], 5);
        assert_eq!(full["returned_by_upstream"], json!(5));
        assert_eq!(
            full["count_is_page_bounded"],
            json!(true),
            "a full page must be flagged as page-bounded"
        );
        assert!(
            full.get("total").is_none(),
            "never label a page count 'total'"
        );

        // A short page cannot be hiding anything.
        let partial = social_list_payload("u", &json!({}), &rows, &["id"], 50);
        assert_eq!(partial["count_is_page_bounded"], json!(false));

        // The output cap is reported separately from the upstream page size.
        let many: Vec<Value> = (0..MAX_ROWS + 4)
            .map(|i| json!({ "id": format!("p{i}") }))
            .collect();
        let capped = social_list_payload("u", &json!({}), &many, &["id"], 100);
        assert_eq!(capped["returned_by_upstream"], json!(MAX_ROWS + 4));
        assert_eq!(capped["shown"], json!(MAX_ROWS));
        assert_eq!(capped["withheld_by_output_cap"], json!(4));
    }

    #[tokio::test]
    async fn application_core_reads_refuse_an_empty_org_before_making_a_request() {
        let state = AppState::new();
        // Empty org is the one failure we must never turn into a live request:
        // insight-core/social-core would 400, but an unscoped read is the class
        // of bug that leaks across tenants, so it is refused locally.
        let err = insights_overview(&state, "   ")
            .await
            .expect_err("blank org must be refused");
        assert!(err.contains("verified organization"), "{err}");

        let err = social_list_accounts(&state, "")
            .await
            .expect_err("blank org must be refused");
        assert!(err.contains("verified organization"), "{err}");
    }

    #[tokio::test]
    async fn application_core_reads_refuse_when_the_internal_key_is_unconfigured() {
        let state = AppState::new();
        assert!(
            state.application_core_internal_key.is_empty(),
            "default state has no internal key"
        );
        let err = social_list_posts(&state, "org_1", "", "", None)
            .await
            .expect_err("missing internal key must be refused");
        assert!(err.contains("APPLICATION_CORE_INTERNAL_KEY"), "{err}");
    }

    #[tokio::test]
    async fn social_list_filters_are_validated_before_any_upstream_call() {
        let state = AppState::new();
        // A bad filter must fail with the filter error, NOT the
        // internal-key/unreachable error — proving validation runs first.
        let err = social_list_posts(&state, "org_1", "postponed", "", None)
            .await
            .expect_err("bad status must be rejected");
        assert!(err.contains("unsupported status"), "{err}");

        let err = social_list_campaigns(&state, "org_1", "publishing", None)
            .await
            .expect_err("bad status must be rejected");
        assert!(err.contains("unsupported status"), "{err}");
    }
}
