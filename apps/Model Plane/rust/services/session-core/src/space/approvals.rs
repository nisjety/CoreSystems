//! Typed approval card context and derived continuation state (build plan WS10-S1).
//!
//! Two things live here, both content-free:
//!
//! - The approval *card*: `tool_key`, `action_label` and `resume_class`, validated from the
//!   creator's `context_json` and stored under `approvals.metadata.card`. The resume class is the
//!   server's own truth, never the caller's claim: an approval whose continuation descriptor names
//!   a resumable kind (`mp_contracts::space_approvals::RESUMABLE_DESCRIPTOR_TOOLS`) is
//!   `action_continuation`; any other approval can only be `live_poll` (a browser action polled
//!   inside the running loop) or `none` (the grant lets the agent retry the exact action once).
//! - The *continuation* of a granted approval, derived on every read from the delivery outbox, the
//!   start receipt and the outcome (migrations 0014, 0018, 0019, 0020). Nothing is stored for it
//!   and no client can state it, so the card can only claim what the rows prove.
//!
//! No migration: the card is a JSONB key on the existing `approvals.metadata`.

use std::collections::HashMap;

use mp_contracts::model_plane::v1 as pb;
use serde_json::{Map, Value as JsonValue};
use sqlx::FromRow;
use tonic::Status;
use tracing::warn;

use crate::store::Pool;

/// Largest accepted `CreateApprovalRequest.context_json`.
pub(crate) const MAX_CONTEXT_BYTES: usize = 4096;
/// Longest accepted value in the context object, in characters.
pub(crate) const MAX_CONTEXT_VALUE_CHARS: usize = 128;
/// The only keys a creator may state. Everything else is refused, so raw arguments and
/// credential-shaped keys can never enter a card.
const ALLOWED_CONTEXT_KEYS: &[&str] =
    &["tool_key", "action_label", "resume_class", "action_schema_hash"];

/// A stated `action_schema_hash` is kept only in its `sha256:<64 lowercase hex>` form: a
/// commitment to the tool's rule registry digest, never argument text (DEPTH-WS10-S4-1).
fn schema_commitment(value: &str) -> Option<&str> {
    let hex = value.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then_some(value)
}

/// Resume class stored when the approval carries a continuation descriptor.
pub(crate) const RESUME_ACTION_CONTINUATION: &str = "action_continuation";
/// Resume class of a browser live approval polled inside the running loop.
pub(crate) const RESUME_LIVE_POLL: &str = "live_poll";
/// Resume class of an approval that only lets the agent retry the exact action once.
pub(crate) const RESUME_NONE: &str = "none";

/// Validate the creator's `context_json` and build the `metadata.card` value.
///
/// `descriptor_tool` is `Some(tool_name)` when the approval carries a continuation descriptor
/// (the empty string when the descriptor names no tool). Only a descriptor for a resumable kind
/// makes the approval an action continuation (DEPTH-WS10-S9-1): execution-core writes a
/// descriptor for every non-ZDR gated call, so presence alone would stamp shell and MCP approvals
/// as continuations, overstate the card and leave their once-token unclaimable.
///
/// Returns `Ok(None)` when the creator stated no context and there is no descriptor (the
/// approval is stored exactly as before and its continuation is derived without a resume class).
/// A zero-retention caller keeps its `tool_key` and resume class but never an `action_label`.
pub(crate) fn card_from_request(
    context_json: &str,
    descriptor_tool: Option<&str>,
    zdr: bool,
) -> Result<Option<JsonValue>, Status> {
    let has_descriptor = descriptor_tool.is_some();
    let stated = parse_context(context_json)?;
    if stated.is_none() && !has_descriptor {
        return Ok(None);
    }
    let stated = stated.unwrap_or_default();
    let mut card = Map::new();
    if let Some(tool_key) = stated.get("tool_key") {
        card.insert("tool_key".to_owned(), JsonValue::String(tool_key.clone()));
    }
    if !zdr {
        if let Some(label) = stated.get("action_label") {
            card.insert("action_label".to_owned(), JsonValue::String(label.clone()));
        }
    }
    // Content-free, so a zero-retention caller keeps it too. A malformed value is dropped, not
    // refused: it only feeds an optional standing rule.
    if let Some(hash) = stated
        .get("action_schema_hash")
        .and_then(|hash| schema_commitment(hash))
    {
        card.insert(
            "action_schema_hash".to_owned(),
            JsonValue::String(hash.to_owned()),
        );
    }
    // A live poll is honoured only on an approval with no descriptor (the browser loop sends
    // none): a descriptor-bearing approval is either a resumable continuation or the once-token.
    let resume = match descriptor_tool {
        Some(tool) if mp_contracts::space_approvals::is_resumable_descriptor_tool(tool) => {
            RESUME_ACTION_CONTINUATION
        }
        Some(_) => RESUME_NONE,
        None if stated.get("resume_class").map(String::as_str) == Some(RESUME_LIVE_POLL) => {
            RESUME_LIVE_POLL
        }
        None => RESUME_NONE,
    };
    card.insert(
        "resume_class".to_owned(),
        JsonValue::String(resume.to_owned()),
    );
    Ok(Some(JsonValue::Object(card)))
}

/// Parse the allowlisted context object; empty input is "no context".
fn parse_context(context_json: &str) -> Result<Option<HashMap<String, String>>, Status> {
    if context_json.is_empty() {
        return Ok(None);
    }
    if context_json.len() > MAX_CONTEXT_BYTES {
        return Err(Status::invalid_argument("approval context is too large"));
    }
    let value: JsonValue = serde_json::from_str(context_json)
        .map_err(|_| Status::invalid_argument("approval context must be valid JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument("approval context must be an object"))?;
    let mut stated = HashMap::new();
    for (key, value) in object {
        if !ALLOWED_CONTEXT_KEYS.contains(&key.as_str()) {
            return Err(Status::invalid_argument("approval context key is not allowed"));
        }
        let text = value
            .as_str()
            .ok_or_else(|| Status::invalid_argument("approval context values must be strings"))?;
        if text.chars().count() > MAX_CONTEXT_VALUE_CHARS || text.chars().any(char::is_control) {
            return Err(Status::invalid_argument("approval context value is invalid"));
        }
        stated.insert(key.clone(), text.to_owned());
    }
    Ok(Some(stated))
}

/// A text field of the stored card; empty when the approval has none.
pub(crate) fn card_text(metadata: &JsonValue, key: &str) -> String {
    metadata["card"][key].as_str().unwrap_or_default().to_owned()
}

/// What the rows say about one approval's delivery, read in one join.
#[derive(FromRow, Debug, Clone)]
pub(crate) struct ContinuationFacts {
    pub approval_id: String,
    pub status: String,
    pub continuation_mode: String,
    pub resume_class: Option<String>,
    pub delivery_state: Option<String>,
    pub last_failure_code: Option<String>,
    pub has_receipt: bool,
    pub outcome: Option<String>,
    pub outcome_failure_code: Option<String>,
    pub provider_receipt_id: Option<String>,
    pub verification_status: Option<String>,
}

const CONTINUATION_FACTS_SQL: &str = "SELECT a.id AS approval_id, a.status AS status, \
        a.continuation_mode AS continuation_mode, \
        a.metadata #>> '{card,resume_class}' AS resume_class, \
        d.state AS delivery_state, d.last_failure_code AS last_failure_code, \
        (r.receipt_id IS NOT NULL) AS has_receipt, \
        o.outcome AS outcome, o.failure_code AS outcome_failure_code, \
        o.provider_receipt_id AS provider_receipt_id, \
        o.verification_status AS verification_status \
    FROM approvals AS a \
    LEFT JOIN approval_delivery_outbox AS d ON d.approval_id = a.id \
    LEFT JOIN approval_continuation_receipts AS r ON r.delivery_id = d.delivery_id \
    LEFT JOIN approval_continuation_outcomes AS o ON o.receipt_id = r.receipt_id \
    WHERE a.org_id = $1 AND a.id = ANY($2)";

fn verification_to_proto(status: &str) -> i32 {
    match status {
        "verified_success" => pb::VerificationStatus::VerifiedSuccess as i32,
        "verified_failure" => pb::VerificationStatus::VerifiedFailure as i32,
        "partially_verified" => pb::VerificationStatus::PartiallyVerified as i32,
        _ => pb::VerificationStatus::Unknown as i32,
    }
}

/// Derive the continuation of one approval from its rows. `None` while the approval is not
/// granted: there is nothing to continue yet, and nothing is claimed.
///
/// Order matters: a recorded outcome beats a start receipt, which beats the approval's class,
/// which beats the outbox state. A row the creator never classified (created before this
/// slice) is read from the outbox alone, and a terminal `invalid_continuation` there means the
/// worker found nothing it could run, i.e. the approval is not resumable.
pub(crate) fn derive_continuation(facts: &ContinuationFacts) -> Option<pb::ApprovalContinuation> {
    use pb::ContinuationState as State;
    if facts.status != "granted" {
        return None;
    }
    let make = |state: State, failure_code: &str| pb::ApprovalContinuation {
        state: state as i32,
        failure_code: failure_code.to_owned(),
        verification: facts
            .verification_status
            .as_deref()
            .map_or(pb::VerificationStatus::Unspecified as i32, verification_to_proto),
        provider_receipt_id: facts.provider_receipt_id.clone().unwrap_or_default(),
    };
    if !facts.continuation_mode.is_empty() {
        return Some(make(State::AwaitingContinue, ""));
    }
    if let Some(outcome) = facts.outcome.as_deref() {
        return Some(if outcome == "completed" {
            make(State::Completed, "")
        } else {
            let code = facts
                .outcome_failure_code
                .as_deref()
                .unwrap_or(if outcome == "cancelled" {
                    "cancelled"
                } else {
                    "unknown_outcome"
                });
            make(State::Failed, code)
        });
    }
    if facts.has_receipt {
        return Some(make(State::Running, ""));
    }
    match facts.resume_class.as_deref() {
        Some(RESUME_LIVE_POLL) => return Some(make(State::Live, "")),
        Some(RESUME_NONE) => return Some(make(State::NotResumable, "")),
        _ => {}
    }
    match facts.delivery_state.as_deref() {
        Some("terminal") => {
            let code = facts.last_failure_code.as_deref().unwrap_or("unknown_outcome");
            Some(if code == "invalid_continuation" && facts.resume_class.is_none() {
                make(State::NotResumable, "")
            } else {
                make(State::Failed, code)
            })
        }
        Some("pending" | "processing") => Some(make(State::Queued, "")),
        _ => None,
    }
}

/// Read the facts of several approvals of one org.
pub(crate) async fn load_continuation_facts(
    pool: &Pool,
    org_id: &str,
    approval_ids: &[String],
) -> anyhow::Result<HashMap<String, ContinuationFacts>> {
    let rows = sqlx::query_as::<_, ContinuationFacts>(CONTINUATION_FACTS_SQL)
        .bind(org_id)
        .bind(approval_ids)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.approval_id.clone(), row))
        .collect())
}

/// Fill `continuation` on every granted approval of `approvals` (all of one org). A failed read
/// leaves the field unset and is logged: the card then states nothing about the continuation
/// instead of guessing, and the decision it follows is never failed for it.
pub(crate) async fn attach_continuations(
    pool: &Pool,
    org_id: &str,
    approvals: &mut [pb::Approval],
) {
    let granted = pb::ApprovalState::Granted as i32;
    let ids: Vec<String> = approvals
        .iter()
        .filter(|approval| approval.state == granted)
        .map(|approval| approval.id.clone())
        .collect();
    if ids.is_empty() {
        return;
    }
    match load_continuation_facts(pool, org_id, &ids).await {
        Ok(facts) => {
            for approval in approvals.iter_mut() {
                if let Some(facts) = facts.get(&approval.id) {
                    approval.continuation = derive_continuation(facts);
                }
            }
        }
        Err(error) => {
            warn!(error = %error, "approval continuation state unavailable");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_contracts::model_plane::v1::{ContinuationState as State, VerificationStatus as Verification};
    use serde_json::json;

    const TOOL: &str = "mcp__srv__create_note";

    /// A resumable kind's descriptor tool.
    const RESUMABLE: &str = "execute_provider_action";

    /// `has_descriptor` stands for a descriptor of a resumable kind.
    fn card(context: &str, has_descriptor: bool, zdr: bool) -> Option<JsonValue> {
        card_from_request(context, has_descriptor.then_some(RESUMABLE), zdr)
            .expect("the context is accepted")
    }

    fn refusal(context: &str) -> Status {
        card_from_request(context, None, false).expect_err("the context is refused")
    }

    fn resume_class(card: &JsonValue) -> String {
        card_text(&json!({ "card": card }), "resume_class")
    }

    #[test]
    fn no_context_and_no_descriptor_states_nothing() {
        assert_eq!(card("", false, false), None);
        assert_eq!(card("", false, true), None);
    }

    #[test]
    fn a_stated_card_keeps_only_its_allowlisted_text() {
        let stated = json!({ "tool_key": TOOL, "action_label": "Create the note" }).to_string();
        assert_eq!(
            card(&stated, false, false),
            Some(json!({ "tool_key": TOOL, "action_label": "Create the note", "resume_class": "none" }))
        );
        // An empty object states nothing but is still classified: the retry is the continuation.
        assert_eq!(card("{}", false, false), Some(json!({ "resume_class": "none" })));
        // A descriptor with no stated context still yields a card, carrying only the class.
        assert_eq!(card("", true, false), Some(json!({ "resume_class": "action_continuation" })));
    }

    #[test]
    fn the_server_decides_the_class_and_honours_only_a_descriptorless_live_poll() {
        let stated = |class: &str| json!({ "resume_class": class }).to_string();
        // (context, carries a continuation descriptor, class the card ends up with)
        let cases: Vec<(String, bool, &str)> = vec![
            (String::new(), true, RESUME_ACTION_CONTINUATION),
            (stated("none"), true, RESUME_ACTION_CONTINUATION),
            (stated("live_poll"), true, RESUME_ACTION_CONTINUATION),
            (stated("none"), false, RESUME_NONE),
            (stated("live_poll"), false, RESUME_LIVE_POLL),
            // A creator cannot claim a continuation it has no descriptor for.
            (stated("action_continuation"), false, RESUME_NONE),
            (stated("something_else"), false, RESUME_NONE),
            (json!({ "tool_key": TOOL }).to_string(), false, RESUME_NONE),
        ];
        for (context, has_descriptor, expected) in cases {
            let built = card(&context, has_descriptor, false).expect("a card");
            assert_eq!(
                resume_class(&built),
                expected,
                "context {context:?}, descriptor {has_descriptor}"
            );
        }
    }

    #[test]
    fn only_a_resumable_kinds_descriptor_is_an_action_continuation() {
        // DEPTH-WS10-S9-1: execution-core writes a descriptor for every non-ZDR gated call, so a
        // shell or MCP approval carries one too. Its card must read `none` (the once-token), not
        // `action_continuation`, whatever the creator stated.
        let stated = |class: &str| json!({ "tool_key": TOOL, "resume_class": class }).to_string();
        for tool in ["shell", "mcp__srv__create_note", "", "Execute_Provider_Action"] {
            for (context, expected) in [
                (String::new(), RESUME_NONE),
                (stated("none"), RESUME_NONE),
                (stated("action_continuation"), RESUME_NONE),
                // The stated live poll is not honoured on a descriptor-bearing approval.
                (stated("live_poll"), RESUME_NONE),
            ] {
                let built = card_from_request(&context, Some(tool), false)
                    .expect("accepted")
                    .expect("a descriptor always yields a card");
                assert_eq!(resume_class(&built), expected, "tool {tool:?}, context {context:?}");
            }
        }
        // The three resumable kinds still read action_continuation, whatever was stated.
        for tool in mp_contracts::space_approvals::RESUMABLE_DESCRIPTOR_TOOLS {
            for context in [String::new(), stated("none"), stated("live_poll")] {
                let built = card_from_request(&context, Some(tool), true)
                    .expect("accepted")
                    .expect("a card");
                assert_eq!(resume_class(&built), RESUME_ACTION_CONTINUATION, "tool {tool}");
            }
        }
    }

    #[test]
    fn the_schema_commitment_is_kept_only_in_its_sha256_form() {
        // DEPTH-WS10-S4-1: the card carries the tool's schema commitment so an "always" decision
        // can name it; it is content-free, so zero retention keeps it too.
        let good = format!("sha256:{}", "a".repeat(64));
        for zdr in [false, true] {
            let context = json!({ "tool_key": TOOL, "action_schema_hash": good }).to_string();
            let built = card(&context, false, zdr).expect("a card");
            assert_eq!(built["action_schema_hash"], json!(good), "zdr {zdr}");
        }
        for bad in [
            "a".repeat(64),
            format!("sha256:{}", "A".repeat(64)),
            format!("sha256:{}", "a".repeat(63)),
            "sha256:not-hex".to_owned(),
        ] {
            let context = json!({ "tool_key": TOOL, "action_schema_hash": bad }).to_string();
            let built = card(&context, false, false).expect("a card");
            assert!(built.get("action_schema_hash").is_none(), "{bad}");
        }
    }

    #[test]
    fn zero_retention_drops_the_label_and_keeps_the_tool_key_and_class() {
        let stated =
            json!({ "tool_key": TOOL, "action_label": "Create the note", "resume_class": "live_poll" })
                .to_string();
        assert_eq!(
            card(&stated, false, false),
            Some(json!({ "tool_key": TOOL, "action_label": "Create the note", "resume_class": "live_poll" }))
        );
        assert_eq!(
            card(&stated, false, true),
            Some(json!({ "tool_key": TOOL, "resume_class": "live_poll" }))
        );
        assert_eq!(
            card(&stated, true, true),
            Some(json!({ "tool_key": TOOL, "resume_class": "action_continuation" }))
        );
        // A label alone leaves nothing but the class.
        let label_only = json!({ "action_label": "Create the note" }).to_string();
        assert_eq!(card(&label_only, false, true), Some(json!({ "resume_class": "none" })));
    }

    #[test]
    fn a_key_outside_the_allowlist_is_refused_so_no_argument_or_credential_enters_a_card() {
        for key in ["arguments", "input", "api_key", "token", "Tool_Key", "tool_key ", "context", ""] {
            let status = refusal(&format!(r#"{{"{key}":"x"}}"#));
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "key {key:?}");
            assert!(status.message().contains("not allowed"), "key {key:?}: {}", status.message());
        }
        // One forbidden key refuses the whole object, whatever else it states.
        let mixed = refusal(&json!({ "tool_key": TOOL, "password": "x" }).to_string());
        assert_eq!(mixed.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn a_value_must_be_a_string_of_at_most_128_plain_characters() {
        for body in [
            r#"{"tool_key":5}"#,
            r#"{"tool_key":null}"#,
            r#"{"tool_key":true}"#,
            r#"{"tool_key":["a"]}"#,
            r#"{"action_label":{"a":"b"}}"#,
            r#"{"resume_class":1.5}"#,
        ] {
            let status = refusal(body);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{body}");
            assert!(status.message().contains("must be strings"), "{body}: {}", status.message());
        }

        // The cap counts characters, not bytes, and is inclusive.
        let label = |text: &str| json!({ "action_label": text }).to_string();
        for unit in ["a", "\u{e9}", "\u{1f600}"] {
            let at_the_cap = unit.repeat(MAX_CONTEXT_VALUE_CHARS);
            assert!(card(&label(&at_the_cap), false, false).is_some(), "{unit:?} at the cap");
            let over = unit.repeat(MAX_CONTEXT_VALUE_CHARS + 1);
            let status = refusal(&label(&over));
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{unit:?} over the cap");
            assert!(status.message().contains("invalid"), "{}", status.message());
        }

        // A control character of any kind is refused, newline and tab included.
        for body in [
            r#"{"action_label":"line one\nline two"}"#,
            r#"{"action_label":"tab\there"}"#,
            r#"{"action_label":"bell\u0007"}"#,
            r#"{"action_label":"nul\u0000"}"#,
            r#"{"action_label":"del\u007f"}"#,
            r#"{"action_label":"c1\u0085"}"#,
        ] {
            let status = refusal(body);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{body}");
            assert!(status.message().contains("invalid"), "{body}: {}", status.message());
        }
        let plain = label("Create the note, then archive it (draft #2)");
        assert!(card(&plain, false, false).is_some());
    }

    #[test]
    fn the_context_is_capped_at_four_kib_and_must_be_a_json_object() {
        let base = json!({ "tool_key": TOOL }).to_string();
        let padded = |len: usize| format!("{base}{}", " ".repeat(len - base.len()));
        assert_eq!(padded(MAX_CONTEXT_BYTES).len(), MAX_CONTEXT_BYTES);
        assert!(
            card(&padded(MAX_CONTEXT_BYTES), false, false).is_some(),
            "exactly 4 KiB is accepted"
        );
        let too_big = refusal(&padded(MAX_CONTEXT_BYTES + 1));
        assert_eq!(too_big.code(), tonic::Code::InvalidArgument);
        assert!(too_big.message().contains("too large"), "{}", too_big.message());
        // The size is checked before anything else is looked at.
        let not_even_json = refusal(&"x".repeat(MAX_CONTEXT_BYTES + 1));
        assert!(not_even_json.message().contains("too large"), "{}", not_even_json.message());

        for body in ["{", "not json", "tool_key=x", r#"{"tool_key":}"#] {
            let status = refusal(body);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{body}");
            assert!(status.message().contains("valid JSON"), "{body}: {}", status.message());
        }
        for body in ["[]", r#""x""#, "5", "null", r#"["tool_key"]"#, "true"] {
            let status = refusal(body);
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{body}");
            assert!(status.message().contains("must be an object"), "{body}: {}", status.message());
        }
    }

    #[test]
    fn card_text_reads_the_stored_card_or_nothing() {
        let metadata = json!({ "card": { "tool_key": TOOL, "resume_class": 5 } });
        assert_eq!(card_text(&metadata, "tool_key"), TOOL);
        assert_eq!(card_text(&metadata, "action_label"), "");
        assert_eq!(card_text(&metadata, "resume_class"), "", "a non-string reads as absent");
        assert_eq!(card_text(&json!({}), "tool_key"), "");
    }

    fn facts() -> ContinuationFacts {
        ContinuationFacts {
            approval_id: "appr_1".to_owned(),
            status: "granted".to_owned(),
            continuation_mode: String::new(),
            resume_class: None,
            delivery_state: None,
            last_failure_code: None,
            has_receipt: false,
            outcome: None,
            outcome_failure_code: None,
            provider_receipt_id: None,
            verification_status: None,
        }
    }

    /// What a reader of the approval sees: the state and the failure code.
    fn derived(facts: &ContinuationFacts) -> Option<(i32, String)> {
        derive_continuation(facts).map(|continuation| (continuation.state, continuation.failure_code))
    }

    fn state(state: State, failure_code: &str) -> Option<(i32, String)> {
        Some((state as i32, failure_code.to_owned()))
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn derive_continuation_table() {
        let with = |edit: fn(&mut ContinuationFacts)| {
            let mut facts = facts();
            edit(&mut facts);
            facts
        };
        let cases: Vec<(&str, ContinuationFacts, Option<(i32, String)>)> = vec![
            // Nothing is claimed until the approval is granted, whatever the rows say.
            ("requested", with(|f| f.status = "requested".into()), None),
            (
                "denied with an outcome row",
                with(|f| {
                    f.status = "denied".into();
                    f.outcome = Some("completed".into());
                }),
                None,
            ),
            (
                "timed out in loop mode",
                with(|f| {
                    f.status = "timed_out".into();
                    f.continuation_mode = "loop".into();
                }),
                None,
            ),
            // A loop-owned approval is the loop's, and beats every other row.
            (
                "loop mode",
                with(|f| f.continuation_mode = "loop".into()),
                state(State::AwaitingContinue, ""),
            ),
            (
                "segment mode beats a recorded outcome",
                with(|f| {
                    f.continuation_mode = "segment".into();
                    f.outcome = Some("completed".into());
                }),
                state(State::AwaitingContinue, ""),
            ),
            // A recorded outcome beats a start receipt, which beats the class and the outbox.
            ("completed", with(|f| f.outcome = Some("completed".into())), state(State::Completed, "")),
            (
                "failed with its code",
                with(|f| {
                    f.outcome = Some("failed".into());
                    f.outcome_failure_code = Some("provider_rejected".into());
                }),
                state(State::Failed, "provider_rejected"),
            ),
            (
                "failed without a code",
                with(|f| f.outcome = Some("failed".into())),
                state(State::Failed, "unknown_outcome"),
            ),
            (
                "cancelled without a code",
                with(|f| f.outcome = Some("cancelled".into())),
                state(State::Failed, "cancelled"),
            ),
            (
                "cancelled with a code",
                with(|f| {
                    f.outcome = Some("cancelled".into());
                    f.outcome_failure_code = Some("operator_cancelled".into());
                }),
                state(State::Failed, "operator_cancelled"),
            ),
            (
                "an outcome word nobody knows",
                with(|f| f.outcome = Some("mystery".into())),
                state(State::Failed, "unknown_outcome"),
            ),
            (
                "outcome beats receipt",
                with(|f| {
                    f.has_receipt = true;
                    f.outcome = Some("completed".into());
                }),
                state(State::Completed, ""),
            ),
            ("receipt only", with(|f| f.has_receipt = true), state(State::Running, "")),
            (
                "receipt beats the none class",
                with(|f| {
                    f.has_receipt = true;
                    f.resume_class = Some("none".into());
                }),
                state(State::Running, ""),
            ),
            // The approval's own class beats the outbox state.
            (
                "none class, nothing started",
                with(|f| f.resume_class = Some("none".into())),
                state(State::NotResumable, ""),
            ),
            (
                "none class beats a terminal outbox row",
                with(|f| {
                    f.resume_class = Some("none".into());
                    f.delivery_state = Some("terminal".into());
                    f.last_failure_code = Some("lease_expired".into());
                }),
                state(State::NotResumable, ""),
            ),
            (
                "live poll",
                with(|f| f.resume_class = Some("live_poll".into())),
                state(State::Live, ""),
            ),
            (
                "live poll beats a pending outbox row",
                with(|f| {
                    f.resume_class = Some("live_poll".into());
                    f.delivery_state = Some("pending".into());
                }),
                state(State::Live, ""),
            ),
            // A row the creator never classified is read from the outbox alone.
            (
                "legacy approval the worker found nothing to run",
                with(|f| {
                    f.delivery_state = Some("terminal".into());
                    f.last_failure_code = Some("invalid_continuation".into());
                }),
                state(State::NotResumable, ""),
            ),
            (
                "classified approval with the same terminal code",
                with(|f| {
                    f.resume_class = Some("action_continuation".into());
                    f.delivery_state = Some("terminal".into());
                    f.last_failure_code = Some("invalid_continuation".into());
                }),
                state(State::Failed, "invalid_continuation"),
            ),
            (
                "terminal with another code",
                with(|f| {
                    f.delivery_state = Some("terminal".into());
                    f.last_failure_code = Some("lease_expired".into());
                }),
                state(State::Failed, "lease_expired"),
            ),
            (
                "terminal without a code",
                with(|f| f.delivery_state = Some("terminal".into())),
                state(State::Failed, "unknown_outcome"),
            ),
            (
                "pending",
                with(|f| f.delivery_state = Some("pending".into())),
                state(State::Queued, ""),
            ),
            (
                "processing",
                with(|f| f.delivery_state = Some("processing".into())),
                state(State::Queued, ""),
            ),
            (
                "classified action continuation, pending",
                with(|f| {
                    f.resume_class = Some("action_continuation".into());
                    f.delivery_state = Some("pending".into());
                }),
                state(State::Queued, ""),
            ),
            // No row proves anything: nothing is claimed.
            ("no outbox row and no class", facts(), None),
        ];
        for (name, facts, expected) in cases {
            assert_eq!(derived(&facts), expected, "case: {name}");
        }
    }

    #[test]
    fn the_verification_and_the_provider_receipt_ride_along() {
        let mut facts = facts();
        facts.outcome = Some("completed".to_owned());
        facts.provider_receipt_id = Some("prov-1".to_owned());
        for (stored, expected) in [
            (None, Verification::Unspecified),
            (Some("verified_success"), Verification::VerifiedSuccess),
            (Some("verified_failure"), Verification::VerifiedFailure),
            (Some("partially_verified"), Verification::PartiallyVerified),
            (Some("unknown"), Verification::Unknown),
            (Some("garbage"), Verification::Unknown),
        ] {
            facts.verification_status = stored.map(str::to_owned);
            let continuation = derive_continuation(&facts).expect("granted");
            assert_eq!(continuation.verification, expected as i32, "stored {stored:?}");
            assert_eq!(continuation.provider_receipt_id, "prov-1");
        }
        facts.provider_receipt_id = None;
        let continuation = derive_continuation(&facts).expect("granted");
        assert_eq!(continuation.provider_receipt_id, "");
    }

    async fn seed_approval(pool: &Pool, org: &str, run: &str, id: &str, status: &str, mode: &str, class: Option<&str>) {
        let metadata = match class {
            Some(class) => json!({ "card": { "resume_class": class } }),
            None => json!({}),
        };
        sqlx::query(
            "INSERT INTO approvals (id, run_id, kind, status, org_id, user_id, continuation_mode, metadata) \
             VALUES ($1, $2, 'tool_call', $3, $4, 'user-1', $5, $6)",
        )
        .bind(id)
        .bind(run)
        .bind(status)
        .bind(org)
        .bind(mode)
        .bind(&metadata)
        .execute(pool)
        .await
        .expect("seed approval");
    }

    async fn seed_outbox(pool: &Pool, org: &str, run: &str, approval: &str, state: &str, failure: Option<&str>) {
        sqlx::query(
            "INSERT INTO approval_delivery_outbox \
             (delivery_id, approval_id, run_id, org_id, user_id, state, last_failure_code) \
             VALUES ($1, $2, $3, $4, 'user-1', $5, $6)",
        )
        .bind(format!("dlv-{approval}"))
        .bind(approval)
        .bind(run)
        .bind(org)
        .bind(state)
        .bind(failure)
        .execute(pool)
        .await
        .expect("seed outbox row");
    }

    async fn seed_receipt(pool: &Pool, org: &str, run: &str, approval: &str) {
        sqlx::query(
            "INSERT INTO approval_continuation_receipts \
             (receipt_id, delivery_id, approval_id, run_id, org_id, user_id, descriptor_version, \
              action_fingerprint, execution_service_id) \
             VALUES ($1, $2, $3, $4, $5, 'user-1', 1, $6, 'execution-core')",
        )
        .bind(format!("rcp-{approval}"))
        .bind(format!("dlv-{approval}"))
        .bind(approval)
        .bind(run)
        .bind(org)
        .bind("a".repeat(64))
        .execute(pool)
        .await
        .expect("seed receipt");
    }

    async fn seed_outcome(
        pool: &Pool,
        approval: &str,
        outcome: &str,
        provider_receipt: Option<&str>,
        failure: Option<&str>,
        verification: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO approval_continuation_outcomes \
             (receipt_id, outcome, provider_receipt_id, failure_code, verification_status, verification_method) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(format!("rcp-{approval}"))
        .bind(outcome)
        .bind(provider_receipt)
        .bind(failure)
        .bind(verification)
        .bind(verification.map(|_| "readback"))
        .execute(pool)
        .await
        .expect("seed outcome");
    }

    /// `get`, `list` and `decide` all fill the continuation with `attach_continuations`, so this is
    /// the read they share: each granted approval is derived from its own outbox, receipt and
    /// outcome rows, and an approval that is not granted states nothing.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable test Postgres (see space/test_support.rs)"]
    #[allow(clippy::too_many_lines)]
    async fn continuation_is_derived_from_the_outbox_receipt_and_outcome_rows() {
        let Some(pool) = crate::space::test_support::connect().await else {
            return;
        };
        let sfx = mp_ids::new_ulid();
        let (org, thread, run) = (
            format!("org-cont-{sfx}"),
            format!("thr-cont-{sfx}"),
            format!("run-cont-{sfx}"),
        );
        let id = |name: &str| format!("appr-{name}-{sfx}");
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, 'user-1')")
            .bind(&thread)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query("INSERT INTO runs (id, thread_id, goal, org_id, user_id) VALUES ($1, $2, 'goal', $3, 'user-1')")
            .bind(&run)
            .bind(&thread)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed run");

        // queued: granted, the delivery worker has not started it.
        seed_approval(&pool, &org, &run, &id("queued"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("queued"), "pending", None).await;
        // running: a start receipt and no outcome.
        seed_approval(&pool, &org, &run, &id("running"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("running"), "processing", None).await;
        seed_receipt(&pool, &org, &run, &id("running")).await;
        // completed, with the independent read-back.
        seed_approval(&pool, &org, &run, &id("completed"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("completed"), "settled", None).await;
        seed_receipt(&pool, &org, &run, &id("completed")).await;
        seed_outcome(&pool, &id("completed"), "completed", Some("prov-1"), None, Some("verified_success")).await;
        // failed with a closed code, and cancelled without one.
        seed_approval(&pool, &org, &run, &id("failed"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("failed"), "settled", None).await;
        seed_receipt(&pool, &org, &run, &id("failed")).await;
        seed_outcome(&pool, &id("failed"), "failed", None, Some("provider_rejected"), None).await;
        seed_approval(&pool, &org, &run, &id("cancelled"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("cancelled"), "settled", None).await;
        seed_receipt(&pool, &org, &run, &id("cancelled")).await;
        seed_outcome(&pool, &id("cancelled"), "cancelled", None, None, None).await;
        // legacy: never classified, and the worker found nothing it could run.
        seed_approval(&pool, &org, &run, &id("legacy"), "granted", "", None).await;
        seed_outbox(&pool, &org, &run, &id("legacy"), "terminal", Some("invalid_continuation")).await;
        // classified by the creator: the retry is the continuation, or the loop polls it, or the
        // durable loop owns it.
        seed_approval(&pool, &org, &run, &id("none"), "granted", "", Some("none")).await;
        seed_approval(&pool, &org, &run, &id("live"), "granted", "", Some("live_poll")).await;
        seed_approval(&pool, &org, &run, &id("loop"), "granted", "loop", None).await;
        // not decided yet.
        seed_approval(&pool, &org, &run, &id("pending"), "requested", "", None).await;

        let names = [
            "queued", "running", "completed", "failed", "cancelled", "legacy", "none", "live", "loop", "pending",
        ];
        let mut approvals: Vec<pb::Approval> = names
            .iter()
            .map(|name| pb::Approval {
                id: id(name),
                state: (if *name == "pending" {
                    pb::ApprovalState::Requested
                } else {
                    pb::ApprovalState::Granted
                }) as i32,
                ..Default::default()
            })
            .collect();
        attach_continuations(&pool, &org, &mut approvals).await;

        let view = |name: &str| {
            approvals
                .iter()
                .find(|approval| approval.id == id(name))
                .and_then(|approval| approval.continuation.clone())
                .map(|c| (c.state, c.failure_code, c.verification, c.provider_receipt_id))
        };
        let expect = |state: State, code: &str, verification: Verification, receipt: &str| {
            Some((state as i32, code.to_owned(), verification as i32, receipt.to_owned()))
        };
        let none = Verification::Unspecified;
        assert_eq!(view("queued"), expect(State::Queued, "", none, ""));
        assert_eq!(view("running"), expect(State::Running, "", none, ""));
        assert_eq!(
            view("completed"),
            expect(State::Completed, "", Verification::VerifiedSuccess, "prov-1")
        );
        assert_eq!(view("failed"), expect(State::Failed, "provider_rejected", none, ""));
        assert_eq!(view("cancelled"), expect(State::Failed, "cancelled", none, ""));
        assert_eq!(view("legacy"), expect(State::NotResumable, "", none, ""));
        assert_eq!(view("none"), expect(State::NotResumable, "", none, ""));
        assert_eq!(view("live"), expect(State::Live, "", none, ""));
        assert_eq!(view("loop"), expect(State::AwaitingContinue, "", none, ""));
        assert_eq!(view("pending"), None, "an approval that is not granted states no continuation");

        // The facts are read per org, and carry the joined rows.
        let ids: Vec<String> = names.iter().map(|name| id(name)).collect();
        let elsewhere = load_continuation_facts(&pool, "another-org", &ids).await.expect("facts");
        assert!(elsewhere.is_empty(), "another org reads none of these approvals");
        let own = load_continuation_facts(&pool, &org, &ids).await.expect("facts");
        assert_eq!(own.len(), names.len());
        let completed = &own[&id("completed")];
        assert!(completed.has_receipt);
        assert_eq!(completed.outcome.as_deref(), Some("completed"));
        assert_eq!(completed.verification_status.as_deref(), Some("verified_success"));
        assert_eq!(own[&id("legacy")].last_failure_code.as_deref(), Some("invalid_continuation"));
        assert_eq!(own[&id("none")].resume_class.as_deref(), Some("none"));
        assert_eq!(own[&id("loop")].continuation_mode, "loop");

        for sql in [
            "DELETE FROM approvals WHERE org_id = $1",
            "DELETE FROM runs WHERE org_id = $1",
            "DELETE FROM threads WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(&org).execute(&pool).await.ok();
        }
    }
}
