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
//!
//! With the long-page reader on (`EXECUTION_WEB_READER`, EP-2-S10) a fetched page is held as a page
//! rather than cut to its head: `read_page` reads it by outline and section, `find_in_page` searches it,
//! and `result_query` still reads it in windows. Only a page from `web_fetch` is held as one.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mp_contracts::model_plane::v1 as pb;
use mp_contracts::page_reader;
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

/// Most characters one reader result may carry. It is the cap a loop keeps of one tool result, so the
/// ref an envelope names survives the cut and the outline gives way first.
const READER_RESULT_CHARS: usize = 8_000;

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
    /// Whether this is a fetched page the reader may read by section and by search.
    page: bool,
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
        self.hold(org_id, run_id, label, body, false)
    }

    /// Hold a fetched page for the reader (EP-2-S10). The rules are [`Self::park`]'s; the page is also
    /// read by section and by search, and only a page is.
    ///
    /// # Errors
    /// As [`Self::park`].
    pub fn park_page(
        &self,
        org_id: &str,
        run_id: &str,
        label: &str,
        body: &str,
    ) -> Result<String, ParkRefused> {
        self.hold(org_id, run_id, label, body, true)
    }

    fn hold(
        &self,
        org_id: &str,
        run_id: &str,
        label: &str,
        body: &str,
        page: bool,
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
                page,
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

    /// Read one section of a page held for reading, from a character offset into it, or the outline and
    /// the opening of the page when no section is named. The label is repeated with every read.
    ///
    /// # Errors
    /// Returns a model-facing message when the ref is not a page held for this run, has expired, or names
    /// a section the outline does not list.
    pub fn read_page(
        &self,
        org_id: &str,
        run_id: &str,
        page_ref: &str,
        section_id: &str,
        offset: usize,
    ) -> Result<String, String> {
        let (label, body) = self.held_page(org_id, run_id, page_ref)?;
        if section_id.is_empty() {
            return Ok(page_envelope(page_ref, &label, &body));
        }
        let sections = page_reader::outline(&body);
        let Some(section) = sections.iter().find(|candidate| candidate.id == section_id) else {
            return Err(format!(
                "this page has no section '{section_id}'. Use a section id from its outline."
            ));
        };
        let read = page_reader::read_section(&body, section, offset);
        let note = match read.next_offset {
            Some(next) => format!(
                "This section continues: read it again with section '{}' and offset {next} for the rest.",
                section.id
            ),
            None => format!("This is the end of section '{}'.", section.id),
        };
        Ok(json!({
            "ref": page_ref,
            "source": label,
            "section": {"id": section.id, "heading": section.heading, "chars": section.chars},
            "offset": read.offset,
            "text": read.text,
            "returned_chars": read.returned_chars,
            "next_offset": read.next_offset,
            "note": note,
        })
        .to_string())
    }

    /// Search a page held for reading: the passages that best match `query`, each with the section it is in.
    ///
    /// # Errors
    /// As [`Self::read_page`] for the ref.
    pub fn find_in_page(
        &self,
        org_id: &str,
        run_id: &str,
        page_ref: &str,
        query: &str,
    ) -> Result<String, String> {
        let (label, body) = self.held_page(org_id, run_id, page_ref)?;
        let sections = page_reader::outline(&body);
        let passages: Vec<Value> = page_reader::find(
            &body,
            &sections,
            query,
            page_reader::FIND_CHARS,
            page_reader::select_chunks,
        )
        .into_iter()
        .map(|passage| {
            json!({
                "section": passage.section_id,
                "heading": passage.heading,
                "offset": passage.offset,
                "text": passage.text,
            })
        })
        .collect();
        let note = if passages.is_empty() {
            "No passage of this page shares a word with the query. Read a section from the outline with read_page, or search again with other words."
        } else {
            "The passages are the closest matches, most relevant first. Read a passage's section with read_page for the text around it."
        };
        Ok(json!({
            "ref": page_ref,
            "source": label,
            "query": query,
            "passages": passages,
            "note": note,
        })
        .to_string())
    }

    /// The label and the text of a page held for reading. Only a page parked by [`Self::park_page`] reads
    /// this way; every other held result is read by `result_query` alone.
    fn held_page(
        &self,
        org_id: &str,
        run_id: &str,
        page_ref: &str,
    ) -> Result<(String, String), String> {
        let now = Instant::now();
        let state = self.lock();
        let key = Key {
            org_id: org_id.to_owned(),
            run_id: run_id.to_owned(),
            handle_id: page_ref.to_owned(),
        };
        let held = state
            .entries
            .get(&key)
            .filter(|held| now.duration_since(held.stored_at) < self.ttl)
            .ok_or_else(|| {
                format!(
                    "no page reference '{page_ref}' in this run; it may have expired, or belong to another run. Fetch the page again with web_fetch."
                )
            })?;
        if !held.page {
            return Err(format!(
                "'{page_ref}' is not a fetched page. Only a page from web_fetch can be read this way; use {RESULT_QUERY_TOOL} for any other result."
            ));
        }
        Ok((held.label.clone(), held.body.clone()))
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

/// Whether the reader holds this tool's result as a page: the switch is on and the tool is a page fetch.
#[must_use]
pub fn reader_applies(tool_name: &str) -> bool {
    crate::web_tools::reader_enabled() && crate::web_tools::is_page_fetch(tool_name)
}

/// The text a read-class result shows the model inline before anything is held. With the reader on, a
/// fetched page arrives whole, so it is cut to the head the loop has always shown.
#[must_use]
pub fn inline_text<'a>(tool_name: &str, output: &'a str) -> Cow<'a, str> {
    if reader_applies(tool_name) {
        Cow::Owned(crate::web_tools::fetch_head(output))
    } else {
        Cow::Borrowed(output)
    }
}

/// Hold a read-class result for the model and return what the model reads in its place, or `None` when
/// it stays inline. A page the reader holds is parked as a page; any other result is parked as WS25-S5
/// parks it.
#[must_use]
pub fn hold_for_model(
    store: &ResultHandles,
    org_id: &str,
    run_id: &str,
    tool_name: &str,
    label: &str,
    body: &str,
) -> Option<String> {
    hold_as(reader_applies(tool_name), store, org_id, run_id, label, body)
}

fn hold_as(
    page: bool,
    store: &ResultHandles,
    org_id: &str,
    run_id: &str,
    label: &str,
    body: &str,
) -> Option<String> {
    if page {
        let page_ref = store.park_page(org_id, run_id, label, body).ok()?;
        return Some(page_envelope(&page_ref, label, body));
    }
    let handle_id = store.park(org_id, run_id, label, body).ok()?;
    Some(handle_envelope(&handle_id, label, body))
}

/// What the model reads in place of a page the reader holds: its ref, the outline of the whole page and
/// its opening.
#[must_use]
pub fn page_envelope(page_ref: &str, label: &str, body: &str) -> String {
    page_envelope_within(page_ref, label, body, READER_RESULT_CHARS)
}

/// [`page_envelope`] within `budget` characters. The outline gives way one row at a time, from its last
/// section, because the keys serialise alphabetically: a cut at the end would lose `ref` first.
fn page_envelope_within(page_ref: &str, label: &str, body: &str, budget: usize) -> String {
    let sections = page_reader::outline(body);
    let opening = sections
        .first()
        .map_or("", |section| page_reader::read_section(body, section, 0).text);
    let total = body.chars().count();
    let mut shown = sections.len();
    loop {
        let envelope = json!({
            "ref": page_ref,
            "source": label,
            "size_chars": total,
            "outline": outline_rows(&sections[..shown]),
            "content": opening,
            "note": page_note(total, shown, sections.len()),
        })
        .to_string();
        if envelope.chars().count() <= budget || shown == 0 {
            return envelope;
        }
        shown -= 1;
    }
}

/// The note that tells the model how to read a held page and how long its ref lasts.
fn page_note(total_chars: usize, shown: usize, sections: usize) -> String {
    let listing = if shown < sections {
        format!(
            " The outline lists the first {shown} of {sections} sections; find_in_page searches all of them."
        )
    } else {
        String::new()
    };
    format!(
        "The COMPLETE page ({total_chars} characters) is held under this ref, and only its opening is shown \
         above. Read a section from the outline with read_page, or search the page with find_in_page.{listing} \
         The ref stops resolving after about {} minutes.",
        HANDLE_TTL.as_secs() / 60
    )
}

/// The outline a reader shows: one row per section, with the id that reads it.
fn outline_rows(sections: &[page_reader::Section]) -> Vec<Value> {
    sections
        .iter()
        .map(|section| {
            json!({
                "id": section.id,
                "heading": section.heading,
                "chars": section.chars,
            })
        })
        .collect()
}

/// Run one reader call (`read_page` or `find_in_page`) against the run's held pages. `None` when the name
/// is not a reader tool or the reader is off, so the call falls through as it did before. With the reader
/// on and no store (a zero-retention or waived run) the call is refused, as `result_query` refuses one.
#[must_use]
pub fn execute_reader_tool(
    tool_name: &str,
    tool_input: &str,
    org_id: &str,
    run_id: &str,
    handles: Option<&ResultHandles>,
) -> Option<Result<String, String>> {
    let is_reader = matches!(
        tool_name,
        crate::web_tools::READ_PAGE_TOOL | crate::web_tools::FIND_IN_PAGE_TOOL
    );
    if !is_reader || !crate::web_tools::reader_enabled() {
        return None;
    }
    let Some(handles) = handles else {
        return Some(Err(format!(
            "{tool_name} is not available on this run: no page is held for reading on it"
        )));
    };
    Some(match tool_name {
        crate::web_tools::READ_PAGE_TOOL => read_page_call(tool_input, org_id, run_id, handles),
        _ => find_in_page_call(tool_input, org_id, run_id, handles),
    })
}

/// The JSON arguments of one reader call, or the refusal that names the tool.
fn reader_input(tool_input: &str, tool_name: &str) -> Result<Value, String> {
    serde_json::from_str::<Value>(tool_input)
        .map_err(|error| format!("invalid {tool_name} input: {error}"))
}

/// The ref a reader call names, trimmed. A call without one is refused.
fn page_ref_of<'a>(input: &'a Value, tool_name: &str) -> Result<&'a str, String> {
    input
        .get("ref")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|page_ref| !page_ref.is_empty())
        .ok_or_else(|| format!("{tool_name} requires a 'ref' from a web_fetch result"))
}

fn read_page_call(
    tool_input: &str,
    org_id: &str,
    run_id: &str,
    handles: &ResultHandles,
) -> Result<String, String> {
    let input = reader_input(tool_input, crate::web_tools::READ_PAGE_TOOL)?;
    let page_ref = page_ref_of(&input, crate::web_tools::READ_PAGE_TOOL)?;
    let section_id = input
        .get("section")
        .and_then(Value::as_str)
        .map_or("", str::trim);
    let offset = input.get("offset").and_then(Value::as_i64).unwrap_or(0).max(0);
    let offset = usize::try_from(offset).unwrap_or(0);
    handles.read_page(org_id, run_id, page_ref, section_id, offset)
}

fn find_in_page_call(
    tool_input: &str,
    org_id: &str,
    run_id: &str,
    handles: &ResultHandles,
) -> Result<String, String> {
    let input = reader_input(tool_input, crate::web_tools::FIND_IN_PAGE_TOOL)?;
    let page_ref = page_ref_of(&input, crate::web_tools::FIND_IN_PAGE_TOOL)?;
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "find_in_page requires a 'query' of the words to look for".to_owned())?;
    handles.find_in_page(org_id, run_id, page_ref, query)
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

    /// A page with a preamble, a short section and a section that carries the query's words.
    fn page() -> String {
        "Intro words.\n# Costs\nThe levy rose in 2024.\n# Lice\nSea lice treatment with warm water cut the lice count in every pen.\n".to_owned()
    }

    #[test]
    fn a_held_page_reads_back_by_outline_section_and_search() {
        let store = ResultHandles::new(10);
        let text = page();
        let page_ref = store.park_page("org", "run", LABEL, &text).expect("parked");
        let envelope: Value =
            serde_json::from_str(&page_envelope(&page_ref, LABEL, &text)).expect("json");
        assert_eq!(envelope["ref"], json!(page_ref));
        assert_eq!(envelope["outline"].as_array().map(Vec::len), Some(3));
        let section: Value = serde_json::from_str(
            &store
                .read_page("org", "run", &page_ref, "s3", 0)
                .expect("section"),
        )
        .expect("json");
        assert_eq!(section["section"]["heading"], json!("Lice"));
        assert!(section["text"].as_str().expect("text").starts_with("# Lice"));
        assert_eq!(section["source"], json!(LABEL));
        let found: Value = serde_json::from_str(
            &store
                .find_in_page("org", "run", &page_ref, "lice treatment")
                .expect("found"),
        )
        .expect("json");
        assert_eq!(found["passages"][0]["section"], json!("s3"));
        assert_eq!(found["source"], json!(LABEL));
    }

    #[test]
    fn a_reader_refuses_a_result_that_is_not_a_page_and_an_unknown_section() {
        let store = ResultHandles::new(10);
        let text = page();
        let other = store.park("org", "run", LABEL, &text).expect("parked");
        let refused = store
            .read_page("org", "run", &other, "", 0)
            .expect_err("not a page");
        assert!(refused.contains("not a fetched page"), "{refused}");
        let page_ref = store.park_page("org", "run", LABEL, &text).expect("parked");
        let unknown = store
            .read_page("org", "run", &page_ref, "s9", 0)
            .expect_err("no such section");
        assert!(unknown.contains("no section 's9'"), "{unknown}");
    }

    #[test]
    fn a_page_ref_is_unreadable_from_another_run_or_org_and_once_expired() {
        let store = ResultHandles::new(10);
        let text = page();
        let page_ref = store
            .park_page("org", "run_a", LABEL, &text)
            .expect("parked");
        assert!(store.read_page("org", "run_b", &page_ref, "", 0).is_err());
        assert!(store
            .find_in_page("other-org", "run_a", &page_ref, "lice")
            .is_err());
        assert!(store.read_page("org", "run_a", &page_ref, "", 0).is_ok());
        let expired =
            ResultHandles::with_limits(10, MAX_HANDLE_BYTES, MAX_RUN_BYTES, Duration::ZERO);
        let gone = expired.park_page("org", "run", LABEL, &text).expect("parked");
        assert!(expired.read_page("org", "run", &gone, "", 0).is_err());
    }

    #[test]
    fn find_in_page_says_so_when_no_passage_shares_a_word_with_the_query() {
        let store = ResultHandles::new(10);
        let page_ref = store
            .park_page("org", "run", LABEL, &page())
            .expect("parked");
        let none: Value = serde_json::from_str(
            &store
                .find_in_page("org", "run", &page_ref, "zebra crossing")
                .expect("answer"),
        )
        .expect("json");
        assert_eq!(none["passages"], json!([]));
        assert!(none["note"].as_str().expect("note").starts_with("No passage"));
    }

    #[test]
    fn the_page_envelope_keeps_its_ref_when_the_outline_must_shrink() {
        let mut text = String::new();
        for part in 0..30 {
            text.push_str(&format!(
                "## Part {part} of the report\nWords about part {part}.\n"
            ));
        }
        let envelope = page_envelope_within("res_9", LABEL, &text, 1_500);
        assert!(
            envelope.chars().count() <= 1_500,
            "{}",
            envelope.chars().count()
        );
        let value: Value = serde_json::from_str(&envelope).expect("json");
        assert_eq!(value["ref"], json!("res_9"));
        assert!(value["outline"].as_array().expect("outline").len() < 30);
        assert!(value["note"]
            .as_str()
            .expect("note")
            .contains("outline lists the first"));
    }

    #[test]
    fn a_page_is_held_for_reading_only_above_the_threshold_and_only_as_a_page() {
        let large = ResultHandles::new(10_000);
        assert!(large.park_page("org", "run", LABEL, &page()).is_err());
        let small = ResultHandles::new(10);
        let held = hold_as(true, &small, "org", "run", LABEL, &page()).expect("held");
        let reader: Value = serde_json::from_str(&held).expect("json");
        assert!(reader["outline"].is_array());
        let plain_text = hold_as(false, &small, "org", "run", LABEL, &page()).expect("held");
        let plain: Value = serde_json::from_str(&plain_text).expect("json");
        let handle = plain["handle_id"].as_str().expect("handle").to_owned();
        assert!(small.read_page("org", "run", &handle, "", 0).is_err());
    }

    #[test]
    fn a_reader_call_without_its_ref_or_query_is_refused_by_name() {
        let store = ResultHandles::new(10);
        let no_ref = read_page_call("{}", "org", "run", &store).expect_err("no ref");
        assert!(no_ref.contains("read_page requires a 'ref'"), "{no_ref}");
        let no_query = find_in_page_call(r#"{"ref":"res_1"}"#, "org", "run", &store)
            .expect_err("no query");
        assert!(no_query.contains("'query'"), "{no_query}");
        let malformed = read_page_call("not json", "org", "run", &store).expect_err("malformed");
        assert!(malformed.starts_with("invalid read_page input"), "{malformed}");
    }
}
