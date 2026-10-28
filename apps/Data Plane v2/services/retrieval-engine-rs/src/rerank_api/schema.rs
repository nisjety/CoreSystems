//! Wire schema for `POST /v1/rerank` (QB-T05a): the strict request, its validation into a
//! typed input, the content digest that keys replay and cost, and the response.
//!
//! Errors are fixed codes. A serde message is never echoed, because it can carry a value the
//! caller sent (candidate text included) and error bodies must never carry query or document
//! text.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::limits;
use super::PROFILE_WEB_V1;
use crate::pipeline::types::ZdrMode;

const INVALID_REQUEST: &str = "invalid_request";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    request_id: String,
    query: String,
    profile: String,
    zdr_mode: ZdrMode,
    #[serde(default)]
    sovereign_required: Option<bool>,
    #[serde(default)]
    top_n: Option<u64>,
    #[serde(default)]
    deadline_ms: Option<u64>,
    documents: Vec<WireDocument>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireDocument {
    id: String,
    text: String,
}

/// One candidate as the provider will see it (text already cut to the limit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub text: String,
}

/// A request that passed every limit.
#[derive(Debug, Clone, PartialEq)]
pub struct RerankInput {
    pub request_id: String,
    pub query: String,
    pub zdr_mode: ZdrMode,
    /// Absent means strict (`true`), as the engine treats an absent sovereignty flag.
    pub sovereign_required: bool,
    pub top_n: usize,
    pub deadline: Duration,
    pub documents: Vec<Candidate>,
    /// Ids whose text was cut to [`limits::MAX_TEXT_CHARS`], in input order.
    pub truncated_ids: Vec<String>,
}

/// Validate a request body. The first failing rule names the code, in this order: the zdr mode
/// (present, and one of the three wire values), then the shape (no unknown field, no caller
/// model, every required field), profile, request id, query, documents (count, ids, texts),
/// top_n, deadline.
pub fn parse_request(value: &Value) -> Result<RerankInput, &'static str> {
    let object = value.as_object().ok_or(INVALID_REQUEST)?;
    match object.get("zdr_mode") {
        None => return Err("zdr_mode_required"),
        Some(Value::String(mode)) => {
            ZdrMode::from_str(mode).map_err(|_| "zdr_mode_invalid")?;
        }
        Some(_) => return Err("zdr_mode_invalid"),
    }
    let wire = WireRequest::deserialize(value).map_err(|_| INVALID_REQUEST)?;
    if wire.profile != PROFILE_WEB_V1 {
        return Err("profile_unsupported");
    }

    if !limits::valid_request_id(&wire.request_id) {
        return Err("request_id_invalid");
    }
    if wire.query.trim().is_empty() || wire.query.chars().count() > limits::MAX_QUERY_CHARS {
        return Err("query_invalid");
    }
    if wire.documents.is_empty() || wire.documents.len() > limits::MAX_DOCUMENTS {
        return Err("documents_invalid");
    }
    let mut seen = HashSet::with_capacity(wire.documents.len());
    let mut documents = Vec::with_capacity(wire.documents.len());
    let mut truncated_ids = Vec::new();
    for document in wire.documents {
        if !limits::valid_document_id(&document.id) {
            return Err("document_id_invalid");
        }
        if !seen.insert(document.id.clone()) {
            return Err("document_id_duplicate");
        }
        if document.text.trim().is_empty() {
            return Err("document_text_invalid");
        }
        let (kept, cut) = limits::truncate_chars(&document.text, limits::MAX_TEXT_CHARS);
        if cut {
            truncated_ids.push(document.id.clone());
        }
        documents.push(Candidate {
            text: kept.to_owned(),
            id: document.id,
        });
    }
    let top_n = match wire.top_n {
        None => documents.len(),
        Some(n) if n >= 1 && n <= documents.len() as u64 => n as usize,
        Some(_) => return Err("top_n_invalid"),
    };
    let deadline_ms = wire.deadline_ms.unwrap_or(limits::DEADLINE_DEFAULT_MS);
    if !(limits::DEADLINE_MIN_MS..=limits::DEADLINE_MAX_MS).contains(&deadline_ms) {
        return Err("deadline_invalid");
    }
    Ok(RerankInput {
        request_id: wire.request_id,
        query: wire.query,
        zdr_mode: wire.zdr_mode,
        sovereign_required: wire.sovereign_required.unwrap_or(true),
        top_n,
        deadline: Duration::from_millis(deadline_ms),
        documents,
        truncated_ids,
    })
}

const DIGEST_DOMAIN: &[u8] = b"rerank-v1";

fn put(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

/// blake3 over every input that shapes the response or the provider call, each field
/// length-prefixed so no two different requests share an encoding.
#[must_use]
pub fn content_digest(input: &RerankInput) -> String {
    let mut hasher = blake3::Hasher::new();
    put(&mut hasher, DIGEST_DOMAIN);
    put(&mut hasher, PROFILE_WEB_V1.as_bytes());
    put(&mut hasher, input.query.as_bytes());
    put(&mut hasher, input.zdr_mode.as_str().as_bytes());
    put(&mut hasher, &[u8::from(input.sovereign_required)]);
    put(&mut hasher, &(input.top_n as u64).to_le_bytes());
    put(&mut hasher, &(input.documents.len() as u64).to_le_bytes());
    for document in &input.documents {
        put(&mut hasher, document.id.as_bytes());
        put(&mut hasher, document.text.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// One entry of the response. Scored entries carry `score`; unscored ones omit it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResultEntry {
    pub id: String,
    /// Position in the request's `documents`.
    pub index: usize,
    pub scored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

/// The response body for every outcome. `status` is `ok`, `skipped` or `degraded`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RerankResponse {
    pub request_id: String,
    pub status: &'static str,
    pub reason: Option<&'static str>,
    pub profile: &'static str,
    pub zdr_actions_applied: Vec<&'static str>,
    pub truncated_ids: Vec<String>,
    pub results: Vec<ResultEntry>,
}

/// The ZDR actions a restrictive call takes. Empty when none apply.
pub const ZDR_ACTIONS: &[&str] = &["no_provider_call", "no_cost_event", "no_result_cache"];

/// Input order, unscored. Every non-ok outcome answers this way.
#[must_use]
pub fn identity_results(documents: &[Candidate]) -> Vec<ResultEntry> {
    documents
        .iter()
        .enumerate()
        .map(|(index, document)| ResultEntry {
            id: document.id.clone(),
            index,
            scored: false,
            score: None,
        })
        .collect()
}

/// Ranked, scored entries for an `ok` outcome.
#[must_use]
pub fn scored_results(documents: &[Candidate], ranked: &[(usize, f32)]) -> Vec<ResultEntry> {
    ranked
        .iter()
        .map(|&(index, score)| ResultEntry {
            id: documents[index].id.clone(),
            index,
            scored: true,
            score: Some(score),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn body() -> Value {
        json!({
            "request_id": "rr-schema-0001",
            "query": "q",
            "profile": "web-v1",
            "zdr_mode": "disabled",
            "documents": [{"id": "a", "text": "alpha"}, {"id": "b", "text": "beta"}],
        })
    }

    #[test]
    fn a_minimal_request_takes_the_documented_defaults() {
        let input = parse_request(&body()).expect("valid");
        assert_eq!(input.top_n, 2, "top_n defaults to the document count");
        assert_eq!(input.deadline, Duration::from_millis(2_000));
        assert!(input.sovereign_required, "absent sovereignty is strict");
        assert_eq!(input.zdr_mode, ZdrMode::Disabled);
    }

    #[test]
    fn the_zdr_mode_is_required_and_never_defaulted() {
        let mut value = body();
        value.as_object_mut().expect("object").remove("zdr_mode");
        assert_eq!(parse_request(&value), Err("zdr_mode_required"));
        value["zdr_mode"] = json!(null);
        assert_eq!(parse_request(&value), Err("zdr_mode_invalid"));
        value["zdr_mode"] = json!("off");
        assert_eq!(parse_request(&value), Err("zdr_mode_invalid"));
    }

    #[test]
    fn the_digest_separates_fields_that_would_collide_when_concatenated() {
        let mut first = parse_request(&body()).expect("valid");
        let mut second = first.clone();
        first.query = "ab".to_owned();
        first.documents[0].text = "c".to_owned();
        second.query = "a".to_owned();
        second.documents[0].text = "bc".to_owned();
        assert_ne!(content_digest(&first), content_digest(&second));
        assert_eq!(content_digest(&first).len(), 64);
    }

    #[test]
    fn a_skipped_identity_list_keeps_input_order_and_omits_scores() {
        let input = parse_request(&body()).expect("valid");
        let results = identity_results(&input.documents);
        assert_eq!(results.iter().map(|r| r.index).collect::<Vec<_>>(), [0, 1]);
        assert!(results.iter().all(|r| !r.scored && r.score.is_none()));
    }
}
