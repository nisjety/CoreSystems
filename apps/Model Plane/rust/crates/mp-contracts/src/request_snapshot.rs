//! What one model call was sent and offered (EP-14 S6, decision D2 (b)).
//!
//! A request snapshot answers "what was the model told and offered": the system
//! prompt, the tool catalogue, the parameters, the model the router chose and a
//! manifest of the messages. A message body is never part of it: user and
//! assistant turns already persist in `messages`, and tool results are the
//! largest content risk, so the manifest keeps only role, source tag, length
//! and digest per message.
//!
//! The system prompt and the tool catalogue travel beside the snapshot as two
//! capped blobs, each named by the sha256 of its full text, so session-core can
//! keep one copy per run however many calls repeat it. The snapshot itself
//! holds only their digests.
//!
//! inference-core builds the snapshot and session-core stores it; both read the
//! shape from here so the schema name and the caps cannot drift apart.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model_plane::v1::InferPurpose;

/// The schema name every snapshot carries. A later shape is a new name.
pub const SCHEMA: &str = "request-snapshot-v1";

/// The most bytes of system prompt text one run stores.
pub const SYSTEM_CAP_BYTES: usize = 256 * 1024;

/// The most bytes of tool catalogue JSON one run stores. Larger than the
/// system cap because a catalogue carries every tool's parameter schema.
pub const TOOLS_CAP_BYTES: usize = 512 * 1024;

/// The snapshot of one model call, as stored in `run_request_snapshots.snapshot`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestSnapshotV1 {
    /// Always [`SCHEMA`].
    pub schema: String,
    /// The model the call was served by, after routing.
    pub model: String,
    /// The provider the call was served by.
    pub provider: String,
    /// The model the caller asked for, when routing replaced it (a `verevon-*`
    /// mode or a role); empty when the caller's model was used as named.
    pub routed_from: String,
    /// The generation parameters the call was sent with.
    pub params: SnapshotParams,
    /// The sha256 of the full system prompt text; empty when the call had none.
    pub system_digest: String,
    /// The sha256 of the full tool catalogue JSON; empty when no tool was offered.
    pub tools_digest: String,
    /// How many leading messages the caller placed in the stable and context tiers.
    pub prompt_tiers: SnapshotPromptTiers,
    /// One entry per message, in order, without its body.
    pub messages: Vec<MessageManifestEntry>,
    /// One entry per attached image, without its bytes.
    pub image_parts: Vec<ImagePartManifestEntry>,
}

/// The generation parameters of a call. Every field is a setting, never content.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotParams {
    pub temperature: f32,
    pub max_tokens: u32,
    pub tool_choice: String,
    /// The reasoning level the call named, as its lowercase name; empty for none.
    pub reasoning_effort: String,
    pub thinking_budget_tokens: u32,
    /// Whether the call asked for structured output. The schema itself is not
    /// kept: it can quote content the caller derived from the conversation.
    pub structured_output: bool,
}

/// The prompt tier plan the caller sent (`mp_contracts::prompt_tier`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotPromptTiers {
    pub stable: u32,
    pub context: u32,
}

/// One message of a call, without its body.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageManifestEntry {
    pub role: String,
    /// Where the message came from when the caller said so: the message name,
    /// or `compaction_summary` for the summary of compacted history; empty otherwise.
    pub source_tag: String,
    /// The length of the body in characters.
    pub chars: u32,
    /// The sha256 of the body.
    pub sha256: String,
}

impl MessageManifestEntry {
    /// The manifest entry of a message. The body is read for its length and
    /// digest and not kept.
    #[must_use]
    pub fn of(role: &str, name: &str, compaction_summary: bool, body: &str) -> Self {
        let source_tag = if !name.is_empty() {
            name.to_owned()
        } else if compaction_summary {
            "compaction_summary".to_owned()
        } else {
            String::new()
        };
        Self {
            role: role.to_owned(),
            source_tag,
            chars: u32::try_from(body.chars().count()).unwrap_or(u32::MAX),
            sha256: sha256_hex(body.as_bytes()),
        }
    }
}

/// One attached image of a call, without its bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImagePartManifestEntry {
    /// The index of the message the image belongs to.
    pub index: u32,
    pub mime: String,
    pub bytes: u32,
}

/// A blob cut to its cap: the stored text, and whether it was cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CappedText {
    pub text: String,
    pub truncated: bool,
}

/// The lowercase hex sha256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `text` cut to at most `cap` bytes on a character boundary. The digest of a
/// blob is taken over the full text before the cut, so two calls that sent the
/// same prompt share one blob whether or not it was cut.
#[must_use]
pub fn cap_text(text: &str, cap: usize) -> CappedText {
    if text.len() <= cap {
        return CappedText {
            text: text.to_owned(),
            truncated: false,
        };
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    CappedText {
        text: text[..end].to_owned(),
        truncated: true,
    }
}

/// `value` as JSON with every object's keys sorted, so equal content always has
/// the same text and so the same digest.
#[must_use]
pub fn canonical_json(value: &serde_json::Value) -> String {
    canonicalize(value).to_string()
}

fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonicalize(&map[key]));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

impl RequestSnapshotV1 {
    /// The snapshot as canonical JSON, the text session-core stores.
    #[must_use]
    pub fn to_canonical_json(&self) -> String {
        // A struct of strings, numbers and lists always serializes.
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        canonical_json(&value)
    }
}

/// The closed name of a wire `InferPurpose`, the value stored in
/// `run_request_snapshots.call_purpose`. It is the enum name without its
/// `INFER_PURPOSE_` prefix, lowercased, which is also the inference metrics'
/// `purpose` label. An unknown value is `unspecified`, never a free string.
#[must_use]
pub fn purpose_name(purpose: i32) -> String {
    match InferPurpose::try_from(purpose) {
        Ok(known) => known
            .as_str_name()
            .trim_start_matches("INFER_PURPOSE_")
            .to_ascii_lowercase(),
        Err(_) => "unspecified".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> RequestSnapshotV1 {
        RequestSnapshotV1 {
            schema: SCHEMA.to_owned(),
            model: "gpt-5.1".to_owned(),
            provider: "azure_openai".to_owned(),
            routed_from: "verevon-balance".to_owned(),
            params: SnapshotParams {
                temperature: 0.5,
                max_tokens: 1024,
                tool_choice: "auto".to_owned(),
                reasoning_effort: "medium".to_owned(),
                thinking_budget_tokens: 0,
                structured_output: false,
            },
            system_digest: sha256_hex(b"You are Verevon."),
            tools_digest: String::new(),
            prompt_tiers: SnapshotPromptTiers {
                stable: 1,
                context: 0,
            },
            messages: vec![
                MessageManifestEntry::of("system", "", false, "You are Verevon."),
                MessageManifestEntry::of("user", "", false, "hello"),
            ],
            image_parts: vec![],
        }
    }

    /// The stored shape is pinned: a change to a field name, the key order or
    /// the digest helper changes this text, and stored rows would no longer
    /// read as `request-snapshot-v1`.
    #[test]
    fn request_snapshot_v1_golden() {
        let json = fixture().to_canonical_json();
        let expected = concat!(
            r#"{"image_parts":[],"messages":["#,
            r#"{"chars":16,"role":"system","sha256":""#,
            "5019c4b3a083d2d6a60a1819dabf66263809e960eafac51ff14207cea0e9fe99",
            r#"","source_tag":""},"#,
            r#"{"chars":5,"role":"user","sha256":""#,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            r#"","source_tag":""}],"#,
            r#""model":"gpt-5.1","#,
            r#""params":{"max_tokens":1024,"reasoning_effort":"medium","structured_output":false,"temperature":0.5,"thinking_budget_tokens":0,"tool_choice":"auto"},"#,
            r#""prompt_tiers":{"context":0,"stable":1},"#,
            r#""provider":"azure_openai","routed_from":"verevon-balance","#,
            r#""schema":"request-snapshot-v1","system_digest":""#,
            "5019c4b3a083d2d6a60a1819dabf66263809e960eafac51ff14207cea0e9fe99",
            r#"","tools_digest":""}"#,
        );
        assert_eq!(json, expected);
        // The snapshot carries no message body.
        assert!(!json.contains("hello"));
        assert!(!json.contains("You are Verevon"));
        let back: RequestSnapshotV1 = serde_json::from_str(&json).expect("round trip");
        assert_eq!(back, fixture());
    }

    #[test]
    fn request_snapshot_caps_truncate() {
        let short = cap_text("abc", 8);
        assert_eq!(short, CappedText { text: "abc".to_owned(), truncated: false });

        let exact = cap_text("abcd", 4);
        assert!(!exact.truncated);

        // A cap inside a multi-byte character cuts before it, never through it.
        let cut = cap_text("ab\u{e9}cd", 3);
        assert_eq!(cut.text, "ab");
        assert!(cut.truncated);

        let long = "x".repeat(SYSTEM_CAP_BYTES + 10);
        let capped = cap_text(&long, SYSTEM_CAP_BYTES);
        assert_eq!(capped.text.len(), SYSTEM_CAP_BYTES);
        assert!(capped.truncated);
        assert!(TOOLS_CAP_BYTES > SYSTEM_CAP_BYTES);
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let a = serde_json::json!({"b": 1, "a": {"d": [{"z": 1, "y": 2}], "c": 3}});
        assert_eq!(canonical_json(&a), r#"{"a":{"c":3,"d":[{"y":2,"z":1}]},"b":1}"#);
    }

    #[test]
    fn manifest_entry_tags_the_source_and_keeps_no_body() {
        let summary = MessageManifestEntry::of("system", "", true, "earlier: x");
        assert_eq!(summary.source_tag, "compaction_summary");
        assert_eq!(summary.chars, 10);
        let named = MessageManifestEntry::of("tool", "web_search", false, "r\u{e9}sultat");
        assert_eq!(named.source_tag, "web_search");
        // Characters, not bytes.
        assert_eq!(named.chars, 8);
        assert_eq!(named.sha256, sha256_hex("r\u{e9}sultat".as_bytes()));
    }

    /// Every purpose maps to its own closed name, and a value this build does
    /// not know is `unspecified`: the stored column never holds a free string.
    #[test]
    fn call_purpose_closed_list() {
        let expected = [
            (InferPurpose::Unspecified, "unspecified"),
            (InferPurpose::Answer, "answer"),
            (InferPurpose::ToolRound, "tool_round"),
            (InferPurpose::AgentRound, "agent_round"),
            (InferPurpose::Title, "title"),
            (InferPurpose::FollowUps, "follow_ups"),
            (InferPurpose::VerificationJudge, "verification_judge"),
            (InferPurpose::ResearchPlan, "research_plan"),
            (InferPurpose::ResearchSynthesis, "research_synthesis"),
            (InferPurpose::CompactionSummary, "compaction_summary"),
            (InferPurpose::MemoryExtraction, "memory_extraction"),
            (InferPurpose::MemoryGrounding, "memory_grounding"),
            (InferPurpose::BrowserPlanner, "browser_planner"),
            (InferPurpose::SafetyScreen, "safety_screen"),
            (InferPurpose::Validation, "validation"),
            (InferPurpose::DictationCleanup, "dictation_cleanup"),
            (InferPurpose::ScheduledRound, "scheduled_round"),
        ];
        for (purpose, name) in expected {
            assert_eq!(purpose_name(purpose as i32), name);
        }
        assert_eq!(purpose_name(9_999), "unspecified");
        assert_eq!(purpose_name(-1), "unspecified");
    }
}
