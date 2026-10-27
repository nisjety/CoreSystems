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
