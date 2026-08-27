//! Data Plane v2's typed retrieval endpoints, reachable from the inline chat
//! tool loop.
//!
//! Data Plane v2 exposes roughly fourteen typed retrieval endpoints and returns
//! `suggested_next_tools` on every retrieval naming which one is worth trying
//! next. Before this module the gateway offered the model exactly one of them
//! (`knowledge_search`, the general hybrid `Retrieve`), so a hint like
//! `/v1/retrieve/contradictions` had nowhere to land — see
//! [`crate::retrieval_metadata`] for the mapping that connects the two.
//!
//! The three tools here cover every endpoint a hint can currently name:
//! graph expansion, wiki search, and cross-source contradictions. All three are
//! HTTP POSTs against the retrieval engine, using the same base URL and the same
//! independently verified Data Plane bearer that graph grounding already uses —
//! not a second transport.
//!
//! Same three invariants as [`crate::verevon_actions`], for the same reasons:
//!
//! 1. **Read-only.** Nothing here mutates Data Plane state.
//! 2. **Tenant-scoped by the caller's verified context.** `org_id` comes from
//!    the verified request and appears in no tool input schema, so a model
//!    cannot express "read another tenant". Data Plane pins it again from the
//!    bearer regardless (`pin_request_org`).
//! 3. **Bounded output.** Row-capped and text-truncated before it reaches the
//!    model, because the tool loop re-sends the accumulated history every round.
//!
//! Failures return `Err(String)` naming the actual cause. Never an empty success
//! that reads as "there is no such material" when the truth is "the upstream
//! could not be asked".

use serde_json::{json, Value};

use crate::{auth::VerifiedDataPlaneBearer as VerifiedBearer, state::AppState};

/// Max rows any one of these tools inlines into the conversation.
const MAX_ROWS: usize = 10;

/// Max chars for a free-text field (wiki snippet, claim text, community
/// summary) in a result row.
const MAX_FIELD_CHARS: usize = 400;

/// Default/max `limit` accepted from the model.
const DEFAULT_LIMIT: i64 = 5;
const MAX_LIMIT: i64 = 10;

/// How long any one typed retrieval may take. Matches the graph-grounding
/// timeout: these run inside a chat turn, and a slow auxiliary read must not
/// hold the stream open.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);

fn clamp_limit(requested: Option<i64>) -> i64 {
    requested.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn text(value: &Value, key: &str) -> String {
    let raw = value.get(key).and_then(Value::as_str).unwrap_or_default();
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_FIELD_CHARS {
        return collapsed;
    }
    let mut out: String = collapsed.chars().take(MAX_FIELD_CHARS).collect();
    out.push('…');
    out
}

fn id(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(MAX_ROWS)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// POST one typed retrieval endpoint and return its decoded JSON body.
///
/// The single place these tools touch the network, so the auth posture is
/// stated once: the caller's own verified `aud=data-plane` bearer, no
/// caller-selected identity headers, and `x-org-id` only as the same verified
/// org already in the body — Data Plane rejects a mismatch rather than
/// silently rewriting it.
async fn post_typed(
    state: &AppState,
    bearer: &VerifiedBearer,
    org_id: &str,
    path: &str,
    body: Value,
) -> Result<Value, String> {
    let response = state
        .http_client
        .post(format!(
            "{}{path}",
            crate::retrieval::retrieval_http_base_url()
        ))
        .timeout(TIMEOUT)
        .bearer_auth(bearer.as_str())
        .header("x-org-id", org_id)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("Data Plane retrieval is unreachable: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Data Plane retrieval returned HTTP {} for {path}",
            status.as_u16()
        ));
    }
    response
        .json::<Value>()
        .await
        .map_err(|error| format!("Data Plane retrieval returned an unreadable body: {error}"))
}

/// `knowledge_graph_search` — `/v1/retrieve/graph`.
///
/// The endpoint a `low_confidence` hint names first: entities and community
/// summaries semantically adjacent to the query, which is what covers material
/// the chunk-level hybrid search ranked poorly.
pub async fn graph_search(
    state: &AppState,
    bearer: &VerifiedBearer,
    org_id: &str,
    query: &str,
    limit: Option<i64>,
    zdr: bool,
) -> Result<String, String> {
    let max_entities = clamp_limit(limit);
    let payload = post_typed(
        state,
        bearer,
        org_id,
        "/v1/retrieve/graph",
        json!({
            "org_id": org_id,
            "query": query,
            "max_entities": max_entities,
            "include_communities": true,
            "zdr_mode": crate::retrieval::data_plane_zdr_mode(zdr),
        }),
    )
    .await?;

    let entities: Vec<Value> = payload
        .get("entities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_ROWS)
                .map(|entity| {
                    json!({
                        "entity_id": id(entity, "entity_id"),
                        "type": id(entity, "type"),
                        "text": text(entity, "text"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let communities: Vec<String> = payload
        .get("communities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_ROWS)
                .map(|community| text(community, "summary"))
                .filter(|summary| !summary.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(envelope(
        "graph",
        query,
        entities.len() + communities.len(),
        json!({ "entities": entities, "community_summaries": communities }),
    ))
}

/// `knowledge_wiki_search` — `/v1/retrieve/wiki`.
///
/// Published wiki pages matching the query. Named by both the low-confidence
/// hint and the nothing-retrieved hint, because a curated page often covers
/// what no single ingested chunk does.
pub async fn wiki_search(
    state: &AppState,
    bearer: &VerifiedBearer,
    org_id: &str,
    query: &str,
    limit: Option<i64>,
) -> Result<String, String> {
    let payload = post_typed(
        state,
        bearer,
        org_id,
        "/v1/retrieve/wiki",
        json!({ "org_id": org_id, "query": query, "limit": clamp_limit(limit) }),
    )
    .await?;

    let pages: Vec<Value> = payload
        .get("pages")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_ROWS)
                .map(|page| {
                    json!({
                        "page_id": id(page, "page_id"),
                        "title": text(page, "title"),
                        "path": id(page, "path"),
                        "snippet": text(page, "snippet"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(envelope(
        "wiki",
        query,
        pages.len(),
        json!({ "pages": pages }),
    ))
}

/// `knowledge_contradictions` — `/v1/retrieve/contradictions`.
///
/// Claims the knowledge graph records as contradicted by other claims. This is
/// the endpoint the "three or more sources" hint names, and the one confirmed
/// live in the response that motivated this work.
///
/// An empty result is a real, useful answer here — "the sources do not
/// disagree" — so the envelope reports it explicitly rather than as a bare
/// empty list the model might read as a failure.
pub async fn contradictions(
    state: &AppState,
    bearer: &VerifiedBearer,
    org_id: &str,
    query: Option<&str>,
    limit: Option<i64>,
) -> Result<String, String> {
    let mut body = json!({ "org_id": org_id, "limit": clamp_limit(limit) });
    // Omitted rather than sent empty: Data Plane treats `query: null` as "every
    // contradiction in the org", which is the right answer to "do we have any
    // conflicting information?" and a wrong one to a text search for "".
    if let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) {
        body["query"] = json!(query);
    }
    let payload = post_typed(state, bearer, org_id, "/v1/retrieve/contradictions", body).await?;

    let rows: Vec<Value> = payload
        .get("contradictions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(MAX_ROWS)
                .map(|row| {
                    json!({
                        "claim_id": id(row, "claim_id"),
                        "claim_text": text(row, "claim_text"),
                        "confidence": row.get("confidence").and_then(Value::as_f64).unwrap_or(0.0),
                        "contradicted_by": string_array(row, "contradicted_by"),
                        "source_refs": string_array(row, "source_refs"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(envelope(
        "contradictions",
        query.unwrap_or_default(),
        rows.len(),
        json!({ "contradictions": rows }),
    ))
}

/// One envelope shape for all three tools, mirroring execution-core's
/// `KnowledgeSearchEnvelope`: an explicit `no_results` rather than a bare empty
/// list, so "asked and found nothing" never reads to the model as "not asked".
fn envelope(kind: &str, query: &str, result_count: usize, results: Value) -> String {
    let mut out = json!({
        "kind": kind,
        "status": "ok",
        "no_results": result_count == 0,
        "query": query,
        "result_count": result_count,
    });
    if let (Some(object), Some(results)) = (out.as_object_mut(), results.as_object()) {
        for (key, value) in results {
            object.insert(key.clone(), value.clone());
        }
    }
    out.to_string()
}

#[cfg(test)]
mod tests {
    use super::{clamp_limit, envelope, string_array, text, DEFAULT_LIMIT, MAX_LIMIT};
    use serde_json::json;

    #[test]
    fn a_model_supplied_limit_is_clamped_into_range() {
        assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(-9)), 1);
        assert_eq!(clamp_limit(Some(3)), 3);
        assert_eq!(clamp_limit(Some(i64::MAX)), MAX_LIMIT);
    }

    /// The tool loop re-sends the whole accumulated history each round, so an
    /// untruncated field is paid for again on every subsequent round.
    #[test]
    fn free_text_fields_are_truncated_and_missing_ones_are_empty() {
        let long = "x".repeat(5_000);
        let row = json!({ "claim_text": long });
        let truncated = text(&row, "claim_text");
        assert!(truncated.chars().count() < 5_000);
        assert!(text(&row, "absent").is_empty());
        assert!(text(&json!({ "claim_text": 42 }), "claim_text").is_empty());
    }

    #[test]
    fn string_arrays_tolerate_absence_and_non_string_elements() {
        let row = json!({ "source_refs": ["doc-1", 7, null, "doc-2"] });
        assert_eq!(string_array(&row, "source_refs"), vec!["doc-1", "doc-2"]);
        assert!(string_array(&row, "absent").is_empty());
        assert!(string_array(&json!({ "source_refs": "doc-1" }), "source_refs").is_empty());
    }

    /// "Asked and found nothing" must be distinguishable from "not asked" —
    /// the same honesty contract the grounding path holds.
    #[test]
    fn an_empty_result_is_reported_as_an_explicit_no_results() {
        let out = envelope(
            "contradictions",
            "pricing",
            0,
            json!({ "contradictions": [] }),
        );
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(parsed["no_results"], true);
        assert_eq!(parsed["result_count"], 0);
        assert_eq!(parsed["kind"], "contradictions");
        assert_eq!(parsed["status"], "ok");
        assert!(parsed["contradictions"].is_array());

        let filled = envelope("wiki", "pricing", 2, json!({ "pages": ["a", "b"] }));
        let parsed: serde_json::Value = serde_json::from_str(&filled).expect("valid JSON");
        assert_eq!(parsed["no_results"], false);
        assert_eq!(parsed["result_count"], 2);
    }
}
