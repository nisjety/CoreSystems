//! Room-visible transcript metadata (build plan WS07-S9a, decision SD-20).
//!
//! `GET /v1/threads/{id}/messages` flattens every persisted metadata key onto
//! each turn: grounding snippets, citations, compaction and head summaries,
//! result receipts. A shared-Space reader (a request that carries a Control
//! `space_read_decision_ref`) is not the thread's owner and must not receive
//! that evidence. The plane that owns the data decides what leaves it, so the
//! reduction lives here, not in the gateway BFF or the SPA.
//!
//! The rule is an allowlist, never a denylist: a key reaches a room reader only
//! if it is named in [`ROOM_VISIBLE_METADATA_KEYS`]. A new metadata key is
//! therefore invisible to the room until its owner adds it here. The owner's
//! own Chat read (no decision ref) is unchanged.
//!
//! Kill switch: `SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH=1` restores the old
//! pass-through for every reader. It is an operational rollback, never an
//! authorization input: the Space read decision is still verified by
//! session-core before any message is returned.

use serde_json::{Map, Value};

/// Environment variable that restores the pre-allowlist pass-through.
pub const LEGACY_PASSTHROUGH_ENV: &str = "SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH";

/// Metadata keys a shared-Space reader may see. WS09 adds `"work"`.
pub const ROOM_VISIBLE_METADATA_KEYS: &[&str] = &["artifacts"];

/// Fields of one artifact a room reader may see. Anything else on the stored
/// artifact (provenance, hashes, internal flags) is dropped.
const ROOM_VISIBLE_ARTIFACT_FIELDS: &[&str] = &["id", "kind", "title", "content", "version"];

/// Whether the kill switch is on. Only an explicit `1` or `true` (any case)
/// restores pass-through; unset, empty or any other value keeps the allowlist.
#[must_use]
pub fn legacy_passthrough(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Whether this request is a shared-Space room read: it carries a non-empty
/// Control decision reference. Session-core, not this check, decides whether
/// the decision authorizes anything.
#[must_use]
pub fn is_room_read(space_read_decision_ref: &str) -> bool {
    !space_read_decision_ref.trim().is_empty()
}

fn reduce_artifact(artifact: &Value) -> Option<Value> {
    let source = artifact.as_object()?;
    let reduced: Map<String, Value> = ROOM_VISIBLE_ARTIFACT_FIELDS
        .iter()
        .filter_map(|field| {
            source
                .get(*field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect();
    (!reduced.is_empty()).then_some(Value::Object(reduced))
}

/// Reduces `metadata` to the room-visible keys, with each artifact cut down to
/// its room-visible fields. Returns `None` when nothing survives, so the turn
/// carries no metadata at all (the same shape as a turn that never had any).
#[must_use]
pub fn reduce_room_metadata(metadata: Map<String, Value>) -> Option<Map<String, Value>> {
    let mut reduced = Map::new();
    for key in ROOM_VISIBLE_METADATA_KEYS {
        let Some(value) = metadata.get(*key) else {
            continue;
        };
        let value = if *key == "artifacts" {
            match value {
                Value::Array(items) => {
                    Value::Array(items.iter().filter_map(reduce_artifact).collect())
                }
                // A malformed artifacts value is dropped, never passed through.
                _ => continue,
            }
        } else {
            value.clone()
        };
        reduced.insert((*key).to_owned(), value);
    }
    (!reduced.is_empty()).then_some(reduced)
}

/// The metadata a transcript read returns for this request: the full map for an
/// owner Chat read or with the kill switch on, the allowlisted reduction for a
/// shared-Space read.
#[must_use]
pub fn visible_metadata(
    metadata: Option<Map<String, Value>>,
    space_read_decision_ref: &str,
    legacy_passthrough: bool,
) -> Option<Map<String, Value>> {
    let metadata = metadata?;
    if legacy_passthrough || !is_room_read(space_read_decision_ref) {
        return Some(metadata);
    }
    reduce_room_metadata(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().expect("an object").clone()
    }

    fn stored() -> Map<String, Value> {
        map(json!({
            "grounding": [{"snippet": "private evidence"}],
            "citations": [{"url": "https://example.test"}],
            "compaction": {"summary": "head summary"},
            "head_summary": "older turns",
            "result_receipt": {"id": "r1"},
            "receipts": [1, 2],
            "artifacts": [{
                "id": "a1",
                "kind": "document",
                "title": "Plan",
                "content": "body",
                "version": 3,
                "provenance": {"tool": "x"},
                "sha256": "abc",
                "internal": true
            }],
        }))
    }

    #[test]
    fn grounding_citations_compaction_and_receipt_keys_never_pass_for_a_room_read() {
        let reduced = visible_metadata(Some(stored()), "decision-1", false).expect("artifacts survive");
        let keys: Vec<&str> = reduced.keys().map(String::as_str).collect();
        assert_eq!(keys, ["artifacts"], "only the allowlisted key reaches a room reader");
        for forbidden in ["grounding", "citations", "compaction", "head_summary", "result_receipt", "receipts"] {
            assert!(!reduced.contains_key(forbidden), "{forbidden} leaked to a room read");
        }
    }

    #[test]
    fn artifacts_are_reduced_to_id_kind_title_content_and_version() {
        let reduced = reduce_room_metadata(stored()).expect("artifacts survive");
        assert_eq!(
            reduced["artifacts"],
            json!([{"id": "a1", "kind": "document", "title": "Plan", "content": "body", "version": 3}])
        );
    }

    #[test]
    fn a_partial_artifact_keeps_only_the_visible_fields_it_has() {
        let reduced = reduce_room_metadata(map(json!({
            "artifacts": [{"id": "a1", "provenance": "x"}, {"internal": true}, "not an object", 7, null]
        })))
        .expect("the first artifact survives");
        // An artifact with no visible field and every non-object element is dropped.
        assert_eq!(reduced["artifacts"], json!([{"id": "a1"}]));
    }

    #[test]
    fn a_malformed_artifacts_value_is_dropped_not_passed_through() {
        for malformed in [json!("a string"), json!(7), json!({"id": "a1"}), json!(null), json!(true)] {
            let only = map(json!({"artifacts": malformed, "grounding": "x"}));
            assert_eq!(reduce_room_metadata(only), None, "{malformed}");
        }
    }

    #[test]
    fn an_empty_result_is_none_so_the_turn_carries_no_metadata() {
        assert_eq!(reduce_room_metadata(map(json!({"grounding": "x", "citations": []}))), None);
        assert_eq!(reduce_room_metadata(Map::new()), None);
        // A visible key that is present but empty stays: it is the owner's empty list.
        assert_eq!(
            reduce_room_metadata(map(json!({"artifacts": []}))),
            Some(map(json!({"artifacts": []})))
        );
        assert_eq!(visible_metadata(None, "decision-1", false), None);
        assert_eq!(visible_metadata(None, "", false), None);
    }

    #[test]
    fn an_owner_chat_read_with_no_decision_ref_returns_the_metadata_unchanged() {
        for empty_ref in ["", "   ", "\t\n"] {
            assert_eq!(
                visible_metadata(Some(stored()), empty_ref, false),
                Some(stored()),
                "{empty_ref:?} is not a room read"
            );
        }
        assert!(!is_room_read(""));
        assert!(!is_room_read("  "));
        assert!(is_room_read("decision-1"));
    }

    #[test]
    fn legacy_passthrough_accepts_one_and_true_only() {
        for on in [Some("1"), Some("true"), Some("TRUE"), Some("True"), Some(" 1 "), Some(" true ")] {
            assert!(legacy_passthrough(on), "{on:?}");
        }
        for off in [None, Some(""), Some("0"), Some("false"), Some("yes"), Some("on"), Some("2"), Some("enabled")] {
            assert!(!legacy_passthrough(off), "{off:?}");
        }
    }

    #[test]
    fn the_kill_switch_restores_the_full_map_for_a_room_reader() {
        assert_eq!(visible_metadata(Some(stored()), "decision-1", true), Some(stored()));
        assert_eq!(LEGACY_PASSTHROUGH_ENV, "SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH");
    }

    #[test]
    fn the_allowlist_is_artifacts_only_until_a_later_slice_adds_work() {
        assert_eq!(ROOM_VISIBLE_METADATA_KEYS, ["artifacts"]);
        assert_eq!(ROOM_VISIBLE_ARTIFACT_FIELDS, ["id", "kind", "title", "content", "version"]);
    }
}
