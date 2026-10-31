//! Run-scoped result handles (WS25-S5).
//!
//! A read-class tool result larger than the model's context cap is held here
//! in full, and the model is given a short handle instead. `result_query` reads
//! the held text back in character windows, so a long MCP or knowledge result
//! reaches the model in slices rather than as a truncated blob.
//!
//! The store lives for one run and is shared by the run and every subagent it
//! delegates to, because they all carry the same `run_id`. A handle resolves
//! only for the `(org_id, run_id)` that parked it, and everything is released
//! when the run finalizes.
//!
//! Every read repeats the label the result was recorded under, which states its
//! source and any screening verdict. The verdict is the one assessed on the
//! complete payload before it was parked, so a flagged page stays flagged on
//! every read back.
//!
//! Under zero-data-retention, and on a waived route, nothing is parked: the
//! caller never gets a store, and the result falls back to inline truncation,
//! as it does today. The switch is `EXECUTION_RESULT_HANDLES`, read as the
//! literal `true` only; off keeps every run byte-identical.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::result_query::{
    apply_text_query, describe_text, HandleQuery, TextRange, MAX_TEXT_READ_CHARS,
};
use serde_json::{json, Value};

/// Environment switch for run-scoped handles in execution-core. Off keeps
/// today's inline truncation.
pub const EXECUTION_RESULT_HANDLES_ENV: &str = "EXECUTION_RESULT_HANDLES";

/// The model-facing name of the read-back tool.
pub const RESULT_QUERY_TOOL: &str = "result_query";

/// Largest single result that may be parked. A larger one stays inline.
pub const MAX_HANDLE_BYTES: usize = 1024 * 1024;

/// Most text a single run may hold at once. A run past it stays inline.
pub const MAX_RUN_BYTES: usize = 4 * 1024 * 1024;

/// How long a held result resolves after it was parked.
pub const HANDLE_TTL: Duration = Duration::from_secs(900);

/// Whether run-scoped handles are switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    parse_enabled(std::env::var(EXECUTION_RESULT_HANDLES_ENV).ok().as_deref())
}

/// `on` and `true` enable (trimmed, any case); `1`, `yes`, an empty value and anything else stay off.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim)
        .is_some_and(|v| v.eq_ignore_ascii_case("on") || v.eq_ignore_ascii_case("true"))
}

/// The store for one run, or `None` when handles do not apply to it: the switch
/// is off, or the run is zero-retention or waived. `None` means every result of
/// the run stays inline, which is today's behaviour.
#[must_use]
pub fn for_run(zdr: bool, waived: bool, park_over_chars: usize) -> Option<Arc<ResultHandles>> {
    handles_apply(enabled(), zdr, waived).then(|| Arc::new(ResultHandles::new(park_over_chars)))
}

/// Whether a run gets a store, given the switch and the run's posture. Zero-retention
/// and waived runs never do, whatever the switch says.
#[must_use]
pub fn handles_apply(enabled: bool, zdr: bool, waived: bool) -> bool {
    enabled && !zdr && !waived
}

/// Whether a tool's output may be parked. Only read-class results are: a
/// write's output is the record of what it did, and the tools below either
/// already have a bounded output of their own or are the read-back itself.
#[must_use]
pub fn is_parkable_tool(tool_name: &str) -> bool {
    tool_name != RESULT_QUERY_TOOL
        && tool_name != crate::subagent::research_fan_out::RESEARCH_TOOL_NAME
        && !crate::subagent::is_subagent_tool(tool_name)
        && !crate::process_tools::is_process_tool(tool_name)
        && !crate::permission::is_risky_call(tool_name, "")
        && !crate::permission::is_restricted_context_write(tool_name)
}

/// The offered definition of `result_query`.
#[must_use]
pub fn tool_definition() -> pb::ToolDefinition {
    pb::ToolDefinition {
        name: RESULT_QUERY_TOOL.to_owned(),
        description: "Read a result that was held under a handle because it was too large to show in full. Returns a window of its text together with the source it was recorded under. Use range to read further along it.".to_owned(),
        parameters_json: r#"{"type":"object","properties":{"handle_id":{"type":"string","description":"The handle_id of the held result"},"range":{"type":"object","description":"Characters to read","properties":{"offset":{"type":"integer","description":"Character offset to start from; default 0"},"length":{"type":"integer","description":"Characters to read, at most 4000; default 4000"}}}},"required":["handle_id"]}"#.to_owned(),
    }
}

/// Why a result was not parked. The caller keeps it inline in every case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkRefused {
    /// The result fits the context cap, so it needs no handle.
    BelowThreshold,
    /// The result is bigger than one handle may hold.
    TooLarge,
    /// The run already holds as much text as it may.
    RunFull,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    org_id: String,
    run_id: String,
    handle_id: String,
}

struct Held {
    stored_at: Instant,
    label: String,
    body: String,
}

impl Held {
    fn bytes(&self) -> usize {
        self.label.len() + self.body.len()
    }
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Held>,
    next: u64,
}

/// The held results of one run.
pub struct ResultHandles {
    park_over_chars: usize,
    max_handle_bytes: usize,
    max_run_bytes: usize,
    ttl: Duration,
    state: Mutex<State>,
}

impl ResultHandles {
    /// A store that parks results longer than `park_over_chars` characters.
    #[must_use]
    pub fn new(park_over_chars: usize) -> Self {
        Self::with_limits(
            park_over_chars,
            MAX_HANDLE_BYTES,
            MAX_RUN_BYTES,
            HANDLE_TTL,
        )
    }

    fn with_limits(
        park_over_chars: usize,
        max_handle_bytes: usize,
        max_run_bytes: usize,
        ttl: Duration,
    ) -> Self {
        Self {
            park_over_chars,
            max_handle_bytes,
            max_run_bytes,
            ttl,
            state: Mutex::new(State::default()),
        }
    }

    /// Hold `body` under a new handle for `(org_id, run_id)`, returning the
    /// handle id. `label` is the source line every read repeats.
    ///
    /// # Errors
    /// Returns why the result stays inline: it fits the cap, it is larger than
    /// one handle may hold, or the run is already at its total.
    pub fn park(
        &self,
        org_id: &str,
        run_id: &str,
        label: &str,
        body: &str,
    ) -> Result<String, ParkRefused> {
        if body.chars().count() <= self.park_over_chars {
            return Err(ParkRefused::BelowThreshold);
        }
        let bytes = label.len() + body.len();
        if bytes > self.max_handle_bytes {
            return Err(ParkRefused::TooLarge);
        }
        let now = Instant::now();
        let mut state = self.lock();
        // Expired handles go first, so they stop counting against the run.
        state
            .entries
            .retain(|_, held| now.duration_since(held.stored_at) < self.ttl);
        let run_used: usize = state
            .entries
            .iter()
            .filter(|(key, _)| key.org_id == org_id && key.run_id == run_id)
            .map(|(_, held)| held.bytes())
            .sum();
        if run_used + bytes > self.max_run_bytes {
            return Err(ParkRefused::RunFull);
        }
        state.next += 1;
        let handle_id = format!("res_{:016x}", state.next);
        state.entries.insert(
            Key {
                org_id: org_id.to_owned(),
                run_id: run_id.to_owned(),
                handle_id: handle_id.clone(),
            },
            Held {
                stored_at: now,
                label: label.to_owned(),
                body: body.to_owned(),
            },
        );
        Ok(handle_id)
    }

    /// Read a window of a held result. The label is returned with every read.
    ///
    /// # Errors
    /// Returns a model-facing message when the handle is not held for this
    /// run, has expired, or the query names something a text result lacks.
    pub fn read(
        &self,
        org_id: &str,
        run_id: &str,
        handle_id: &str,
        query: &HandleQuery,
    ) -> Result<Value, String> {
        let now = Instant::now();
        let state = self.lock();
        let key = Key {
            org_id: org_id.to_owned(),
            run_id: run_id.to_owned(),
            handle_id: handle_id.to_owned(),
        };
        let held = state
            .entries
            .get(&key)
            .filter(|held| now.duration_since(held.stored_at) < self.ttl)
            .ok_or_else(|| {
                format!(
                    "no held result has handle_id '{handle_id}' in this run; it may have expired, \
                     so ask for the data again"
                )
            })?;
        let mut value = apply_text_query(&held.body, query)?;
        value["source"] = json!(held.label);
        Ok(value)
    }

    /// Drop every result the run holds. Called when the run finalizes.
    pub fn release_run(&self, org_id: &str, run_id: &str) {
        self.lock()
            .entries
            .retain(|key, _| !(key.org_id == org_id && key.run_id == run_id));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The text the model sees in place of a parked result: the handle, the source
/// it was recorded under, its size, and its opening characters.
#[must_use]
pub fn handle_envelope(handle_id: &str, label: &str, body: &str) -> String {
    json!({
        "handle_id": handle_id,
        "source": label,
        "size_chars": body.chars().count(),
        "summary": describe_text(body, body.len()).summary,
        "note": format!(
            "The COMPLETE result is held under this handle; only its opening is shown above. \
             Call {RESULT_QUERY_TOOL} with this handle_id and a range of {{offset, length}} in \
             characters to read the rest, at most {MAX_TEXT_READ_CHARS} characters a read. The \
             source above applies to every read."
        ),
    })
    .to_string()
}

/// Run one `result_query` call against the run's store.
///
/// # Errors
/// Returns a model-facing message when the input is malformed, the run holds no
/// results, or the read itself is refused.
pub fn execute_result_query(
    tool_input: &str,
    org_id: &str,
    run_id: &str,
    handles: Option<&ResultHandles>,
) -> Result<String, String> {
    let input: Value = serde_json::from_str(tool_input)
        .map_err(|error| format!("invalid {RESULT_QUERY_TOOL} input: {error}"))?;
    let handle_id = input
        .get("handle_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("{RESULT_QUERY_TOOL} requires a 'handle_id'"))?;
    let range = match input.get("range") {
        None | Some(Value::Null) => None,
        Some(range) if range.is_object() => Some(TextRange {
            offset: range
                .get("offset")
                .and_then(Value::as_u64)
                .map_or(0, |offset| usize::try_from(offset).unwrap_or(usize::MAX)),
            length: range
                .get("length")
                .and_then(Value::as_u64)
                .map_or(0, |length| usize::try_from(length).unwrap_or(usize::MAX)),
        }),
        Some(_) => {
            return Err(format!(
                "{RESULT_QUERY_TOOL} range must be an object with offset and length"
            ))
        }
    };
    let handles = handles.ok_or_else(|| {
        format!("no result was held in this run, so {RESULT_QUERY_TOOL} has nothing to read")
    })?;
    let query = HandleQuery {
        range,
        ..HandleQuery::default()
    };
    handles
        .read(org_id, run_id, handle_id, &query)
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "[source: web_fetch]\nNOTE: external content";

    fn window(offset: usize, length: usize) -> HandleQuery {
        HandleQuery {
            range: Some(TextRange { offset, length }),
            ..HandleQuery::default()
        }
    }

    fn body(chars: usize) -> String {
        (0..chars)
            .map(|i| char::from(b'a' + u8::try_from(i % 26).expect("a letter index")))
            .collect()
    }

    #[test]
    fn an_oversized_result_is_parked_and_read_back_in_three_slices() {
        let store = ResultHandles::new(100);
        let text = body(250);
        let handle = store.park("org", "run", LABEL, &text).expect("parked");
        let mut rebuilt = String::new();
        for offset in [0, 100, 200] {
            let slice = store
                .read("org", "run", &handle, &window(offset, 100))
                .expect("a slice");
            rebuilt.push_str(slice["text"].as_str().expect("text"));
            assert_eq!(slice["total_chars"], json!(250));
        }
        assert_eq!(rebuilt, text);
    }

    #[test]
    fn every_slice_repeats_the_label_the_result_was_recorded_under() {
        let store = ResultHandles::new(10);
        let handle = store
            .park("org", "run", LABEL, &body(40))
            .expect("parked");
        for offset in [0, 20] {
            let slice = store
                .read("org", "run", &handle, &window(offset, 20))
                .expect("a slice");
            assert_eq!(slice["source"], json!(LABEL));
        }
    }

    #[test]
    fn a_result_within_the_cap_is_not_parked() {
        let store = ResultHandles::new(100);
        assert_eq!(
            store.park("org", "run", LABEL, &body(100)),
            Err(ParkRefused::BelowThreshold)
        );
    }

    #[test]
    fn a_result_over_one_handle_stays_inline() {
        let store = ResultHandles::with_limits(10, 50, MAX_RUN_BYTES, HANDLE_TTL);
        assert_eq!(
            store.park("org", "run", "", &body(60)),
            Err(ParkRefused::TooLarge)
        );
    }

    #[test]
    fn a_run_past_its_total_falls_back_inline_for_the_next_result() {
        let store = ResultHandles::with_limits(10, MAX_HANDLE_BYTES, 100, HANDLE_TTL);
        store.park("org", "run", "", &body(60)).expect("first fits");
        assert_eq!(
            store.park("org", "run", "", &body(60)),
            Err(ParkRefused::RunFull)
        );
    }

    #[test]
    fn a_handle_is_unreadable_from_another_run_or_another_org() {
        let store = ResultHandles::new(10);
        let handle = store.park("org", "run_a", LABEL, &body(40)).expect("parked");
        let query = window(0, 10);
        assert!(store.read("org", "run_b", &handle, &query).is_err());
        assert!(store.read("other-org", "run_a", &handle, &query).is_err());
        assert!(store.read("org", "run_a", &handle, &query).is_ok());
    }

    #[test]
    fn releasing_a_run_drops_its_results_and_leaves_others() {
        let store = ResultHandles::new(10);
        let mine = store.park("org", "run_a", "", &body(40)).expect("parked");
        let theirs = store.park("org", "run_b", "", &body(40)).expect("parked");
        store.release_run("org", "run_a");
        assert!(store.read("org", "run_a", &mine, &window(0, 5)).is_err());
        assert!(store.read("org", "run_b", &theirs, &window(0, 5)).is_ok());
    }

    #[test]
    fn an_expired_handle_does_not_resolve() {
        let store = ResultHandles::with_limits(10, MAX_HANDLE_BYTES, MAX_RUN_BYTES, Duration::ZERO);
        let handle = store.park("org", "run", "", &body(40)).expect("parked");
        assert!(store.read("org", "run", &handle, &window(0, 5)).is_err());
    }

    #[test]
    fn only_on_and_true_enable_handles() {
        for raw in ["true", " true ", "on", "ON", "TRUE"] {
            assert!(parse_enabled(Some(raw)), "{raw:?} must enable handles");
        }
        for raw in [None, Some("1"), Some("yes"), Some("off"), Some("")] {
            assert!(!parse_enabled(raw), "{raw:?} must not enable handles");
        }
    }

    #[test]
    fn zdr_and_waived_runs_get_no_store_even_with_the_switch_on() {
        assert!(handles_apply(true, false, false));
        assert!(!handles_apply(true, true, false));
        assert!(!handles_apply(true, false, true));
        assert!(!handles_apply(true, true, true));
        assert!(!handles_apply(false, false, false));
    }

    #[test]
    fn result_query_is_never_parked_and_a_web_read_is() {
        assert!(!is_parkable_tool(RESULT_QUERY_TOOL));
        assert!(!is_parkable_tool("shell"));
        assert!(!is_parkable_tool("execute_provider_action"));
        assert!(is_parkable_tool("web_fetch"));
    }

    #[test]
    fn the_read_back_tool_is_offered_under_its_contract_name() {
        let def = tool_definition();
        assert_eq!(def.name, RESULT_QUERY_TOOL);
        let schema: Value = serde_json::from_str(&def.parameters_json).expect("json schema");
        assert_eq!(schema["required"], json!(["handle_id"]));
    }

    #[test]
    fn a_read_without_a_store_or_a_handle_is_refused_with_a_reason() {
        let missing = execute_result_query(r#"{"handle_id":"res_1"}"#, "org", "run", None);
        assert!(missing.expect_err("no store").contains("nothing to read"));
        let store = ResultHandles::new(10);
        let no_id = execute_result_query("{}", "org", "run", Some(&store));
        assert!(no_id.expect_err("no handle").contains("handle_id"));
        let bad_range = execute_result_query(
            r#"{"handle_id":"res_1","range":"all"}"#,
            "org",
            "run",
            Some(&store),
        );
        assert!(bad_range.expect_err("bad range").contains("range"));
    }

    #[test]
    fn the_envelope_names_the_handle_and_its_source() {
        let envelope: Value =
            serde_json::from_str(&handle_envelope("res_1", LABEL, &body(40))).expect("json");
        assert_eq!(envelope["handle_id"], json!("res_1"));
        assert_eq!(envelope["source"], json!(LABEL));
        assert_eq!(envelope["size_chars"], json!(40));
    }
}
