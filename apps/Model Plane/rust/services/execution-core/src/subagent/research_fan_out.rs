//! `subagent.research_fan_out` — one tool call, N parallel research items,
//! structured per-item results merged into a single tool output.
//!
//! Before this, the only fan-out primitive the model had was `subagent.task`
//! — one goal, one call — so a genuine wide-research question ("check these
//! twelve sources") had to become twelve separate tool calls in one round,
//! each with its own free-text answer the parent model then had to stitch
//! together itself, with no per-item status and no per-item source list. This
//! module owns the SHAPE of a fan-out call (parsing its input, the per-item
//! result, and the merge into one tool output); the actual dispatch — running
//! each item as a real nested agent loop — lives in
//! `runtime_loop::agent::LoopSubagentDispatch`, the same split `subagent/mod.rs`
//! already keeps between "what a delegation means" and "how it runs".
//!
//! # What the parent model can rely on
//!
//! * Every item comes back, in the order it was asked, under its own heading —
//!   including items that failed or were refused before they started. A merge
//!   that reported only the items that ran would let the parent believe a
//!   10-item request got 10 real answers when it got 6.
//! * The whole digest fits the context allowance the loop gives this tool
//!   ([`CONTEXT_CHARS`]; [`MERGE_BUDGET_CHARS`] is what [`merge`] aims for). The
//!   loop truncates a tool result at the tail, so an unbounded digest would
//!   silently lose its LAST items and its summary line; [`merge`] budgets each
//!   item's answer instead and says so where it cut one.
//! * The machine-readable `verification:` line is the SECOND line of the
//!   digest, not the last, for the same reason: the one thing a tail-truncation
//!   must never be able to remove.
//! * With `SUBAGENT_TYPED_RESULTS_ENABLED=true` and a `result_schema`, an item
//!   whose answer ends with a fenced JSON block that matches the schema carries
//!   that value on one `Structured:` line. An answer whose block is missing or
//!   does not match is `invalid_output`, names the paths that failed, and is
//!   never counted as confirmed.

use serde::Deserialize;
use serde_json::Value;

/// Tool name the model calls to fan out N research items in one round. Under
/// [`super::TOOL_PREFIX`], so `is_subagent_tool` and
/// `capability_policy::trusted_capability_id` (both prefix-matched) pick it up
/// with no changes of their own.
pub const RESEARCH_TOOL_NAME: &str = "subagent.research_fan_out";

/// Env var: set to `0`/`false`/`off`/`no`/`disabled` to stop offering — and
/// refuse to dispatch — the fan-out tool without a rebuild. Anything else,
/// including unset, leaves it enabled (chat-wave2-specs wide-research §4: a
/// kill switch for when the subscription route rate-limits in practice).
pub const ENABLED_ENV: &str = "WIDE_RESEARCH_FAN_OUT_ENABLED";

/// Env var: how many items of one fan-out call may be IN FLIGHT at once.
pub const CONCURRENCY_ENV: &str = "WIDE_RESEARCH_FAN_OUT_CONCURRENCY";

/// Default concurrency ceiling for one fan-out call.
///
/// A second, independent knob from the run's TOTAL child ceiling
/// (`subagent::resolve_child_ceiling`) — the same two-knobs-not-one argument
/// `subagent/mod.rs` makes for rounds versus breadth, one level deeper
/// (breadth versus concurrency). Letting one call open every item at once
/// against a single ChatGPT-subscription seat (`openai-codex-subscription`,
/// the route Verevon keeps for gpt-5.6-terra) risks tripping that seat's own
/// `subscription_usage_limit` for the WHOLE org, not just this run; observed
/// reset windows run up to 300 minutes. Owner ruling (2026-09-28): start at 4.
pub const DEFAULT_CONCURRENCY: usize = 4;

/// Hard cap on [`concurrency`], whatever the env asks for. Sixteen is already
/// four times the owner's starting point; a typo (`400`) must not become an
/// unthrottled stampede.
pub const MAX_CONCURRENCY: usize = 16;

/// Whether the fan-out tool is enabled, from an already-read env value. Pure so
/// the policy is unit-testable without mutating the process environment.
#[must_use]
pub fn parse_enabled(raw: Option<&str>) -> bool {
    !matches!(
        raw.map(|value| value.trim().to_ascii_lowercase()).as_deref(),
        Some("0" | "false" | "off" | "no" | "disabled")
    )
}

/// The concurrency ceiling, from an already-read env value: unset, unparsable
/// or zero falls back to [`DEFAULT_CONCURRENCY`], and anything above
/// [`MAX_CONCURRENCY`] is clamped to it.
#[must_use]
pub fn parse_concurrency(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|&limit| limit > 0)
        .map_or(DEFAULT_CONCURRENCY, |limit| limit.min(MAX_CONCURRENCY))
}

#[cfg(test)]
thread_local! {
    /// A per-thread stand-in for the env var, so a test can exercise the switch
    /// without mutating the process environment (which every fan-out test running
    /// in parallel would read). `#[tokio::test]` runs its whole task tree on the
    /// test's own thread, so the override reaches everything that test drives.
    static ENABLED_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Forces [`enabled`] on the current thread until dropped. Test-only.
#[cfg(test)]
pub(crate) struct EnabledOverride;

#[cfg(test)]
impl EnabledOverride {
    pub(crate) fn set(value: bool) -> Self {
        ENABLED_OVERRIDE.with(|cell| cell.set(Some(value)));
        Self
    }
}

#[cfg(test)]
impl Drop for EnabledOverride {
    fn drop(&mut self) {
        ENABLED_OVERRIDE.with(|cell| cell.set(None));
    }
}

/// Whether this deployment offers the fan-out tool right now.
#[must_use]
pub fn enabled() -> bool {
    #[cfg(test)]
    if let Some(forced) = ENABLED_OVERRIDE.with(std::cell::Cell::get) {
        return forced;
    }
    parse_enabled(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// This deployment's concurrency ceiling for one fan-out call.
#[must_use]
pub fn concurrency() -> usize {
    parse_concurrency(std::env::var(CONCURRENCY_ENV).ok().as_deref())
}

/// What the model reads when it calls the tool while it is switched off — aimed
/// at the model so it falls back to `subagent.task` (or does the work itself)
/// rather than retrying a refusal that will not change.
#[must_use]
pub fn disabled_refusal() -> String {
    format!(
        "{RESEARCH_TOOL_NAME} is switched off on this deployment. Delegate the questions one at \
         a time with subagent.task, or research them yourself with the tools you already have."
    )
}

/// Rounds held back from the division below so the PARENT still has one to read
/// the merged digest and answer with. The pool a fan-out claims already counts
/// the parent's own final round; dividing all of it would let a set of items
/// that each use their whole share leave the parent with none, and the run
/// would end "exhausted" holding a complete digest it never got to read.
pub const PARENT_SYNTHESIS_RESERVE: u32 = 1;

/// How many rounds item `index` (of `item_count`) may spend out of the `pool` a
/// fan-out call claimed.
///
/// Owner ruling (2026-09-28): a fan-out call knows every item up front, so it
/// DIVIDES the pool evenly instead of the first-claim-wins split an ad hoc
/// `subagent.task` call gets — winner-take-all was tolerable for eight possible
/// children and starves most of a forty-item fan-out. "Evenly" means the shares
/// differ by at most one: the remainder goes to the first items rather than
/// being dropped, so the whole distributable pool is usable.
#[must_use]
pub fn item_round_share(pool: u32, item_count: u32, index: u32) -> u32 {
    if item_count == 0 {
        return 0;
    }
    let distributable = pool.saturating_sub(PARENT_SYNTHESIS_RESERVE);
    distributable / item_count + u32::from(index < distributable % item_count)
}

/// Longest item id echoed back in the digest. The id is model-supplied text that
/// reappears in the merged output, so it is bounded like any other.
const MAX_ID_CHARS: usize = 64;

/// Longest `result_schema_hint` forwarded into each item's prompt.
const MAX_HINT_CHARS: usize = 500;

/// One requested research item, after parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanOutItem {
    /// Caller-supplied stable id, or a generated `item-<index>` when omitted.
    /// Unique within one call: a repeated id is suffixed with the item's index
    /// so two entries in the digest can never share a heading.
    pub id: String,
    /// Self-contained research question for this item — the ONLY thing seeded
    /// into that item's own nested loop, same isolation rule as
    /// `SubagentTask::goal`.
    pub goal: String,
}

/// A parsed `subagent.research_fan_out` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanOutRequest {
    /// 1..=the run's resolved child ceiling — validated at parse time, see
    /// [`parse_fan_out`].
    pub items: Vec<FanOutItem>,
    /// Requested per-item round budget, still subject to the same clamping
    /// `resolve_round_budget` applies to an ordinary `subagent.task` call.
    pub max_rounds_per_item: Option<u32>,
    /// Free-text hint on what shape each item's answer should take. Advisory
    /// only — appended to each item's own prompt by [`compose_item_prompt`],
    /// never parsed.
    pub result_schema_hint: Option<String>,
    /// The JSON Schema each item's answer must match, when the caller gave one and the
    /// typed-results switch is on ([`typed_results_enabled`]). Object-typed, and at most
    /// [`MAX_RESULT_SCHEMA_BYTES`] serialized.
    pub result_schema: Option<Value>,
}

/// Parse a `subagent.research_fan_out` tool input.
///
/// `ceiling` is the SAME per-run child ceiling an ordinary `subagent.task`
/// call is bound by (`subagent::resolve_child_ceiling`'s resolved value) —
/// checked here, at parse time, so a fan-out call cannot be used to bypass the
/// breadth ceiling by asking for more items in one call than N separate
/// `subagent.task` calls could ever obtain. A request for too many items is
/// refused whole, before any child run starts, rather than discovered one
/// partial refusal at a time deep inside dispatch.
///
/// # Errors
///
/// Returns an explicit, model-readable error when the input is not JSON,
/// carries no items, carries more items than `ceiling`, or any item has no
/// non-empty goal.
pub fn parse_fan_out(tool_input: &str, ceiling: u32) -> Result<FanOutRequest, String> {
    #[derive(Deserialize)]
    struct RawItem {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        goal: Option<String>,
    }

    #[derive(Deserialize)]
    struct RawRequest {
        #[serde(default)]
        items: Vec<RawItem>,
        #[serde(default)]
        max_rounds_per_item: Option<u32>,
        #[serde(default)]
        result_schema_hint: Option<String>,
        #[serde(default)]
        result_schema: Option<Value>,
    }

    let raw: RawRequest = serde_json::from_str(tool_input).map_err(|error| {
        format!(
            "invalid {RESEARCH_TOOL_NAME} input: {error}; expected \
             {{\"items\": [{{\"goal\": \"<self-contained research question>\"}}]}}"
        )
    })?;

    if raw.items.is_empty() {
        return Err(format!(
            "{RESEARCH_TOOL_NAME} requires at least one entry in \"items\""
        ));
    }
    if u32::try_from(raw.items.len()).unwrap_or(u32::MAX) > ceiling {
        return Err(format!(
            "{RESEARCH_TOOL_NAME} asked for {} items, which is more than this run's per-run \
             delegation limit of {ceiling}. Split it into a smaller fan-out call, or narrow the \
             research scope.",
            raw.items.len()
        ));
    }

    let mut seen_ids = std::collections::BTreeSet::new();
    let mut items = Vec::with_capacity(raw.items.len());
    for (index, raw_item) in raw.items.into_iter().enumerate() {
        let goal = raw_item.goal.unwrap_or_default();
        let goal = goal.trim();
        if goal.is_empty() {
            return Err(format!(
                "{RESEARCH_TOOL_NAME} item {index} has no non-empty \"goal\""
            ));
        }
        let mut id = raw_item
            .id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map_or_else(
                || format!("item-{index}"),
                |id| id.chars().take(MAX_ID_CHARS).collect::<String>(),
            );
        if !seen_ids.insert(id.clone()) {
            id = format!("{id}-{index}");
            seen_ids.insert(id.clone());
        }
        items.push(FanOutItem {
            id,
            goal: goal.to_owned(),
        });
    }

    // Read only with the switch on: a switched-off deployment ignores `result_schema`
    // entirely, so a malformed one is not refused either.
    let result_schema = if typed_results_enabled() {
        parse_result_schema(raw.result_schema)?
    } else {
        None
    };

    Ok(FanOutRequest {
        items,
        max_rounds_per_item: raw.max_rounds_per_item,
        result_schema_hint: raw
            .result_schema_hint
            .map(|hint| hint.trim().chars().take(MAX_HINT_CHARS).collect::<String>())
            .filter(|hint| !hint.is_empty()),
        result_schema,
    })
}

/// A `result_schema` as the caller sent it: absent or `null` is none. Anything else must be a JSON
/// object whose `type` is `object`, and at most [`MAX_RESULT_SCHEMA_BYTES`] serialized.
fn parse_result_schema(raw: Option<Value>) -> Result<Option<Value>, String> {
    let Some(schema) = raw else {
        return Ok(None);
    };
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err(format!(
            "{RESEARCH_TOOL_NAME} result_schema must be a JSON Schema object with \"type\": \"object\""
        ));
    }
    let size = schema.to_string().len();
    if size > MAX_RESULT_SCHEMA_BYTES {
        return Err(format!(
            "{RESEARCH_TOOL_NAME} result_schema is {size} bytes serialized, more than the \
             {MAX_RESULT_SCHEMA_BYTES}-byte limit"
        ));
    }
    Ok(Some(schema))
}

/// What every item's nested loop is told beyond its own question. The digest's
/// per-item source list is only as good as the URLs the child actually writes
/// down, and a child that has not been asked to cite will often not.
const ITEM_PROMPT_SUFFIX: &str = "You are one of several researchers working in parallel, each \
on a separate question; answer ONLY the question above. Keep the answer concise and \
self-contained. Finish with a \"Sources:\" list of the http(s) URLs you actually read, one per \
line, and cite nothing you did not read. If you cannot find a reliable answer, say so plainly \
instead of guessing.";

/// The prompt an item's nested loop is seeded with: the item's own question,
/// the shared research/citation instructions, and the caller's optional answer
/// format hint. The lineage edge and the durable child run still record the
/// BARE question (`FanOutItem::goal`) — the boilerplate is instructions to the
/// child, not part of what was asked. A `result_schema` adds the ask for a typed result,
/// which the item's answer is then checked against.
#[must_use]
pub fn compose_item_prompt(
    item: &FanOutItem,
    result_schema_hint: Option<&str>,
    result_schema: Option<&Value>,
) -> String {
    let mut prompt = format!("{}\n\n{ITEM_PROMPT_SUFFIX}", item.goal);
    if let Some(hint) = result_schema_hint {
        prompt.push_str("\nRequested answer format: ");
        prompt.push_str(hint);
    }
    if let Some(schema) = result_schema {
        prompt.push_str("\nEnd your answer with one fenced ```json block matching this schema: ");
        prompt.push_str(&schema.to_string());
    }
    prompt
}

/// One item's outcome after its delegated child run finished, failed, or was
/// refused before it ever started (e.g. the per-run child ceiling was hit
/// partway through the fan-out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Ok,
    Failed,
    Refused,
    /// Finished, but its answer did not end with a typed result that matches the caller's
    /// `result_schema` (see [`FanOutItemResult::checked_against`]). The answer is still shown, with
    /// the reason, and the item is not counted as confirmed.
    InvalidOutput,
}

/// One item's result, ready to be folded into [`merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanOutItemResult {
    pub id: String,
    pub goal: String,
    pub status: ItemStatus,
    pub answer: Option<String>,
    /// Source URLs this item's own answer cites — see [`extract_sources`] for
    /// why this is the only signal available at this layer today.
    pub sources: Vec<String>,
    pub error: Option<String>,
    /// The typed result this item's answer carried, when the caller gave a `result_schema` and the
    /// answer matched it. Set only by [`FanOutItemResult::checked_against`].
    pub structured: Option<Value>,
}

impl FanOutItemResult {
    #[must_use]
    pub fn ok(item: &FanOutItem, answer: String) -> Self {
        let sources = extract_sources(&answer);
        Self {
            id: item.id.clone(),
            goal: item.goal.clone(),
            status: ItemStatus::Ok,
            answer: Some(answer),
            sources,
            error: None,
            structured: None,
        }
    }

    #[must_use]
    pub fn failed(item: &FanOutItem, error: String) -> Self {
        Self {
            id: item.id.clone(),
            goal: item.goal.clone(),
            status: ItemStatus::Failed,
            answer: None,
            sources: Vec::new(),
            error: Some(error),
            structured: None,
        }
    }

    #[must_use]
    pub fn refused(item: &FanOutItem, reason: String) -> Self {
        Self {
            id: item.id.clone(),
            goal: item.goal.clone(),
            status: ItemStatus::Refused,
            answer: None,
            sources: Vec::new(),
            error: Some(reason),
            structured: None,
        }
    }

    /// An item not started because the fan-out's child token budget was spent (EP-2 S9). It is a
    /// refusal whose reason starts with [`BUDGET_REACHED`], so the merge can count it.
    #[must_use]
    pub fn budget_reached(item: &FanOutItem, spent: u64, budget: u64) -> Self {
        Self::refused(item, budget_reached_refusal(spent, budget))
    }

    /// Whether this item ended unstarted at the child token budget.
    #[must_use]
    pub fn is_budget_reached(&self) -> bool {
        self.status == ItemStatus::Refused
            && self
                .error
                .as_deref()
                .is_some_and(|reason| reason.starts_with(BUDGET_REACHED))
    }

    /// Checks this item's answer against the caller's `result_schema`, when there is one. Only an
    /// `ok` answer is checked: an item that failed or was refused has nothing to check. An answer
    /// whose last fenced `json` block matches keeps its status and carries the parsed value in its
    /// `structured` field. One without a valid, matching block becomes [`ItemStatus::InvalidOutput`]
    /// with the reason.
    #[must_use]
    pub fn checked_against(self, schema: Option<&Value>) -> Self {
        let Some(schema) = schema else {
            return self;
        };
        if self.status != ItemStatus::Ok {
            return self;
        }
        let Some(answer) = self.answer.as_deref() else {
            return self;
        };
        match structured_output(answer, schema) {
            Ok(value) => Self {
                structured: Some(value),
                ..self
            },
            Err(reason) => Self {
                status: ItemStatus::InvalidOutput,
                error: Some(reason),
                ..self
            },
        }
    }
}

/// Cap on how many source URLs one item contributes to the merged digest, so
/// one verbose subagent cannot crowd out every other item's sources.
const MAX_SOURCES_PER_ITEM: usize = 8;

/// Pull the plain `http(s)` URLs an item's own answer text cites, deduplicated
/// in first-seen order.
///
/// This is the only "source" signal available at this layer: a delegated
/// child returns its final answer as TEXT ONLY (`run_rounds`'s isolated
/// context does not thread the child's own tool-call/grounding trace back to
/// the parent). A deeper per-item grounding channel — surfacing exactly which
/// `knowledge_search`/`web_search` hit backed which sentence, as
/// `SourceRef`-shaped data rather than a URL scraped out of prose — is real
/// future work, not silently pretended-at here. [`compose_item_prompt`] asks
/// each child to end with a "Sources:" list precisely so this has something
/// to find.
#[must_use]
pub fn extract_sources(answer: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut sources = Vec::new();
    for token in answer.split(|c: char| {
        c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"' | '\'' | '[' | ']')
    }) {
        let candidate = token.trim_end_matches(|c: char| ".,;:!?".contains(c));
        if (candidate.starts_with("http://") || candidate.starts_with("https://"))
            && seen.insert(candidate.to_owned())
        {
            sources.push(candidate.to_owned());
            if sources.len() >= MAX_SOURCES_PER_ITEM {
                break;
            }
        }
    }
    sources
}

/// The most issue paths one invalid answer's reason names; the rest are counted.
const MAX_REASON_PATHS: usize = 3;

/// The body of an answer's last fenced `json` block, trimmed, or `None` when the answer has no
/// closed one. The last block is the one the item was asked to end with, so an example earlier in
/// the answer is not taken for it.
fn last_json_block(answer: &str) -> Option<&str> {
    const OPEN: &str = "```json";
    let start = answer.rfind(OPEN)? + OPEN.len();
    let body = &answer[start..];
    Some(body[..body.find("```")?].trim())
}

/// The typed result an answer carries: its last fenced `json` block, parsed and checked against
/// `schema`. The error is the reason the digest shows. It names issue paths and codes only, never a
/// value or a key the model invented.
fn structured_output(answer: &str, schema: &Value) -> Result<Value, String> {
    let Some(block) = last_json_block(answer) else {
        return Err(
            "the answer does not end with a fenced json block for its result_schema".to_owned(),
        );
    };
    let value: Value = serde_json::from_str(block)
        .map_err(|_| "the fenced json block is not valid JSON".to_owned())?;
    let found = mp_contracts::json_schema_subset::validate(&value, schema);
    if found.is_empty() {
        return Ok(value);
    }
    let paths: Vec<String> = found
        .iter()
        .take(MAX_REASON_PATHS)
        .map(|issue| {
            let shown = issue.issue();
            format!("{} ({})", shown.path, shown.code)
        })
        .collect();
    let more = found.len().saturating_sub(MAX_REASON_PATHS);
    let and_more = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    Err(format!(
        "the fenced json block does not match result_schema at {}{and_more}",
        paths.join(", ")
    ))
}

/// How many characters of this tool's result the agent loop keeps in the
/// model's context. Every other tool gets the loop's flat 8000
/// (`MAX_TOOL_CONTEXT_CHARS`); the digest is the one result whose entire
/// purpose is to carry many independent answers back to the parent, and at 8000
/// a forty-item fan-out gets about 200 characters an item once the headings are
/// paid for — a stub, not an answer. Roughly six thousand tokens at the ceiling.
pub const CONTEXT_CHARS: usize = 24_000;

/// Total size [`merge`] aims for: under [`CONTEXT_CHARS`] with room for the
/// loop's own framing line.
pub const MERGE_BUDGET_CHARS: usize = 22_000;

/// Floor on one answer's share, so that a crowded fan-out still gets a
/// recognisable answer per item instead of a stub. When the floor makes the
/// digest exceed [`CONTEXT_CHARS`] the loop's tail-truncation is the backstop —
/// which is why the summary line sits at the top.
const MIN_ANSWER_CHARS: usize = 120;

/// Room the "answer truncated" note may take, held back from each answer's
/// share so the notes cannot push the digest past its budget.
const TRUNCATION_NOTE_RESERVE: usize = 48;

/// Longest item goal echoed in a heading. The parent wrote the goal itself; it
/// needs a reminder of which item this is, not a second copy of the prompt.
const MAX_HEADER_GOAL_CHARS: usize = 100;

/// Longest failure/refusal reason kept per item.
const MAX_REASON_CHARS: usize = 300;

/// Longest single source URL kept.
const MAX_SOURCE_CHARS: usize = 200;

/// The first `max` characters of `text`, on a char boundary. The second value is
/// how many characters were cut off (zero when nothing was).
fn clip(text: &str, max: usize) -> (String, usize) {
    let total = text.chars().count();
    if total <= max {
        return (text.to_owned(), 0);
    }
    (text.chars().take(max).collect(), total - max)
}

/// `text` on one line: every run of whitespace (a newline in a multi-line
/// question, say) becomes a single space. A section heading that spans lines
/// reads as the start of that section's body, and the digest's structure is the
/// point of the digest.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Render one item's section. `answer_cap == 0` renders the section WITHOUT its
/// answer, which is how [`merge_within`] measures what every section costs
/// before it decides how much of each answer fits. `sources_cap` is how many of
/// the item's sources are listed; the rest are counted, not dropped unseen. `structured_kept`
/// says whether a typed line is written, or only the note that it was left out.
fn render_item(
    out: &mut String,
    result: &FanOutItemResult,
    answer_cap: usize,
    sources_cap: usize,
    structured_kept: bool,
) {
    use std::fmt::Write as _;
    let (goal, goal_omitted) = clip(&one_line(&result.goal), MAX_HEADER_GOAL_CHARS);
    let ellipsis = if goal_omitted > 0 { "…" } else { "" };
    let _ = writeln!(out);
    let _ = writeln!(out, "## [{}] {goal}{ellipsis}", one_line(&result.id));
    match result.status {
        ItemStatus::Ok | ItemStatus::InvalidOutput => {
            if result.status == ItemStatus::Ok {
                let _ = writeln!(out, "Status: ok");
            } else {
                let _ = writeln!(out, "Status: invalid_output");
                if let Some(error) = &result.error {
                    let (reason, _) = clip(error, MAX_REASON_CHARS);
                    let _ = writeln!(out, "Reason: {reason}");
                }
            }
            if let Some(structured) = &result.structured {
                if structured_kept {
                    let _ = writeln!(out, "Structured: {structured}");
                } else {
                    let _ = writeln!(out, "Structured: omitted to fit this digest");
                }
            }
            if answer_cap > 0 {
                if let Some(answer) = &result.answer {
                    let (kept, omitted) = clip(answer.trim(), answer_cap);
                    let _ = writeln!(out, "{kept}");
                    if omitted > 0 {
                        let _ = writeln!(out, "[answer truncated: {omitted} chars cut]");
                    }
                }
            }
            if !result.sources.is_empty() {
                if sources_cap == 0 {
                    let _ = writeln!(
                        out,
                        "Sources: {} cited (list omitted to fit this digest)",
                        result.sources.len()
                    );
                } else {
                    let _ = writeln!(out, "Sources:");
                    for (index, source) in result.sources.iter().take(sources_cap).enumerate() {
                        let (source, _) = clip(source, MAX_SOURCE_CHARS);
                        let _ = writeln!(out, "  {}. {source}", index + 1);
                    }
                    let unlisted = result.sources.len().saturating_sub(sources_cap);
                    if unlisted > 0 {
                        let _ = writeln!(out, "  (+{unlisted} more cited)");
                    }
                }
            }
        }
        ItemStatus::Failed | ItemStatus::Refused => {
            let label = if result.status == ItemStatus::Failed {
                "failed"
            } else {
                "refused"
            };
            let _ = writeln!(out, "Status: {label}");
            if let Some(error) = &result.error {
                let (reason, _) = clip(error, MAX_REASON_CHARS);
                let _ = writeln!(out, "Reason: {reason}");
            }
        }
    }
}

/// Merge every item's outcome into the single tool-output string handed back
/// to the parent model, within [`MERGE_BUDGET_CHARS`]. See [`merge_within`].
#[must_use]
pub fn merge(results: &[FanOutItemResult]) -> String {
    merge_within(results, MERGE_BUDGET_CHARS)
}

/// Merge every item's outcome into one digest of roughly `budget` characters.
///
/// Honest about partial failure: an item refused mid-fan-out (the per-run
/// child ceiling hit partway through) is reported with its real refusal
/// reason, never silently dropped. Ends up as: a one-line header, the
/// machine-readable `verification: {"checked": N, "confirmed": M}` line the
/// existing "Verification notice" UI can key off (chat-wave2-specs wide-research
/// fan-out §2.3), then one section per item in the order asked.
///
/// The budget is spent on the ANSWERS: each section's fixed parts (heading,
/// status, reasons) are always kept, and what is left over is shared evenly
/// between the answers, never below [`MIN_ANSWER_CHARS`] each. An answer that is
/// cut says how much was.
///
/// Sources are fixed parts too, but they are the one fixed part that scales with
/// the model's own output (eight URLs an item), so a large fan-out could spend
/// the whole budget on them before a single answer got its floor and leave the
/// loop's tail-truncation to cut the last items. When that would happen the
/// number of sources listed per item is halved (8, 4, 2, 1, then just the
/// count) until every answer still gets its floor.
#[must_use]
pub fn merge_within(results: &[FanOutItemResult], budget: usize) -> String {
    use std::fmt::Write as _;

    let confirmed = results
        .iter()
        .filter(|r| r.status == ItemStatus::Ok)
        .count();
    let mut head = String::new();
    let _ = writeln!(head, "Wide-research fan-out: {} item(s).", results.len());
    let _ = writeln!(
        head,
        "verification: {{\"checked\": {}, \"confirmed\": {confirmed}}}",
        results.len()
    );
    // Only when the child token budget stopped items (EP-2 S9), so a digest without that is
    // what it was before the budget existed.
    let unstarted = results.iter().filter(|r| r.is_budget_reached()).count();
    if unstarted > 0 {
        let _ = writeln!(
            head,
            "budget: {unstarted} of {} item(s) were not started because the fan-out's child \
             token budget was spent.",
            results.len()
        );
    }

    let answered = results.iter().filter(|r| r.answer.is_some()).count();
    // What every section costs before any answer is added, at a given number of
    // listed sources, with or without the typed lines.
    let fixed_at = |sources_cap: usize, structured_kept: bool| -> usize {
        results
            .iter()
            .map(|result| {
                let mut section = String::new();
                render_item(&mut section, result, 0, sources_cap, structured_kept);
                section.chars().count()
            })
            .sum::<usize>()
            + head.chars().count()
    };
    let answers_floor = answered * (MIN_ANSWER_CHARS + TRUNCATION_NOTE_RESERVE);
    // A typed line is the result the caller asked for, so it is kept ahead of the source
    // lists: it is dropped only when the digest cannot hold every answer's floor with it.
    // With no typed results the flag changes nothing.
    let structured_kept = fixed_at(0, true) + answers_floor <= budget;
    let mut sources_cap = MAX_SOURCES_PER_ITEM;
    while sources_cap > 0 && fixed_at(sources_cap, structured_kept) + answers_floor > budget {
        sources_cap /= 2;
    }
    let fixed = fixed_at(sources_cap, structured_kept);
    let answer_cap = if answered == 0 {
        0
    } else {
        (budget.saturating_sub(fixed) / answered)
            .saturating_sub(TRUNCATION_NOTE_RESERVE)
            .max(MIN_ANSWER_CHARS)
    };

    let mut out = head;
    for result in results {
        render_item(&mut out, result, answer_cap, sources_cap, structured_kept);
    }
    out
}

/// Longest item a wide-research run seeds into its goal (Wave 3 EP-2 S8). An item is the person's
/// own question; a longer one is cut, never dropped.
pub const MAX_WIDE_RESEARCH_ITEM_CHARS: usize = 500;

/// Most items one `WideResearchRequest` is read for, kept and dropped together, so a pasted list
/// cannot grow the run's goal without bound. Items past it are neither researched nor named.
pub const MAX_WIDE_RESEARCH_REQUEST_ITEMS: usize = 64;

/// A wide-research run's items once the org-clamped cap is applied (Wave 3 EP-2 S8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WideResearchPlan {
    /// The cap the run is held to: `WideResearchRequest.item_cap`, at most
    /// [`super::MAX_WIDE_RESEARCH_ITEMS`].
    pub cap: u32,
    /// The items the run researches, in the order given. Empty when the person chose a count
    /// and left the model to split the question.
    pub kept: Vec<String>,
    /// The items past the cap, in the order given. The run does not research them, and the
    /// answer names them ([`dropped_items_note`]).
    pub dropped: Vec<String>,
}

/// What a wide-research run is refused with when its org has wide research off (`item_cap` 0).
#[must_use]
pub fn wide_research_off_refusal() -> String {
    "Wide research is turned off for this organisation (its research.max_fan_out_items policy is \
     0), so this run was not started."
        .to_owned()
}

/// One item as the run's goal carries it: trimmed, control characters as spaces, at most
/// [`MAX_WIDE_RESEARCH_ITEM_CHARS`] characters. `None` for an item with nothing left.
fn wide_research_item(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cut: String = cleaned
        .trim()
        .chars()
        .take(MAX_WIDE_RESEARCH_ITEM_CHARS)
        .collect();
    let item = cut.trim_end();
    (!item.is_empty()).then(|| item.to_owned())
}

/// Splits a wide-research request's items at its cap.
///
/// # Errors
///
/// [`wide_research_off_refusal`] when the cap is 0: the org has wide research off, and the run
/// is refused before its first round rather than started with no delegation allowed.
pub fn plan_wide_research(items: &[String], item_cap: u32) -> Result<WideResearchPlan, String> {
    let cap = item_cap.min(super::MAX_WIDE_RESEARCH_ITEMS);
    if cap == 0 {
        return Err(wide_research_off_refusal());
    }
    let mut kept: Vec<String> = items
        .iter()
        .take(MAX_WIDE_RESEARCH_REQUEST_ITEMS)
        .filter_map(|raw| wide_research_item(raw))
        .collect();
    let at = kept.len().min(usize::try_from(cap).unwrap_or(usize::MAX));
    let dropped = kept.split_off(at);
    Ok(WideResearchPlan { cap, kept, dropped })
}

/// The block appended to a wide-research run's goal: the items to fan out, or the cap the model
/// splits the question within, and the items past the cap it must name. The items are the
/// person's own text, as the goal above them is.
#[must_use]
pub fn wide_research_goal_context(plan: &WideResearchPlan) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    if plan.kept.is_empty() {
        let _ = writeln!(
            out,
            "Wide research: split the question above into at most {} self-contained research \
             items and research them with one {RESEARCH_TOOL_NAME} call, then answer from its \
             merged results.",
            plan.cap
        );
    } else {
        let _ = writeln!(
            out,
            "Wide research: research exactly these {} item(s) with one {RESEARCH_TOOL_NAME} call, \
             one item per entry and in this order, then answer from its merged results.",
            plan.kept.len()
        );
        for (index, item) in plan.kept.iter().enumerate() {
            let _ = writeln!(out, "{}. {item}", index + 1);
        }
    }
    if !plan.dropped.is_empty() {
        let _ = writeln!(
            out,
            "These {} item(s) are past this organisation's limit of {} and are not researched. \
             Say so in the answer and name them:",
            plan.dropped.len(),
            plan.cap
        );
        for item in &plan.dropped {
            let _ = writeln!(out, "- {item}");
        }
    }
    out
}

/// The note a finished wide-research answer ends with when items were past the cap: added by the
/// run itself, so the person learns what was left out even if the model did not say it.
#[must_use]
pub fn dropped_items_note(plan: &WideResearchPlan) -> Option<String> {
    if plan.dropped.is_empty() {
        return None;
    }
    let mut note = format!(
        "Not researched ({} item(s) past this organisation's limit of {}):",
        plan.dropped.len(),
        plan.cap
    );
    for item in &plan.dropped {
        note.push_str("\n- ");
        note.push_str(item);
    }
    Some(note)
}

/// Env var: the switch that makes the fan-out's child token budget refuse items (Wave 3 EP-2 S9,
/// decision D4). Opt-in: only the literal `true` enables it. Off, a spent budget is logged and
/// every item still starts, so a fan-out runs exactly as it did before the budget existed.
pub const BUDGET_ENFORCED_ENV: &str = "WIDE_RESEARCH_CHILD_BUDGET_ENFORCED";

/// Env var: the metered tokens all children of one fan-out call may spend before no further item
/// starts (EP-2 S9).
pub const CHILD_TOKEN_BUDGET_ENV: &str = "WIDE_RESEARCH_CHILD_TOKEN_BUDGET";

/// Default child token budget of one fan-out call (decision D4: 400,000 metered tokens).
pub const DEFAULT_CHILD_TOKEN_BUDGET: u64 = 400_000;

/// What an item's refusal reason starts with when the child token budget stopped it, and the
/// status the person's grid shows for it.
pub const BUDGET_REACHED: &str = "budget_reached";

/// Whether the child token budget is enforced, from an already-read env value: only `true`.
#[must_use]
pub fn parse_budget_enforced(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// The child token budget, from an already-read env value: unset, unparsable or zero falls back
/// to [`DEFAULT_CHILD_TOKEN_BUDGET`]. Zero is not "no budget": switching enforcement off is how a
/// deployment runs without one.
#[must_use]
pub fn parse_child_token_budget(raw: Option<&str>) -> u64 {
    raw.and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&budget| budget > 0)
        .unwrap_or(DEFAULT_CHILD_TOKEN_BUDGET)
}

/// Env var: the switch that makes a wide-research item's answer a typed result (F-t3-071). Opt-in:
/// only the literal `true` enables it. Off, `result_schema` is ignored, the fan-out tool is offered
/// the parameters it always had, and a call runs exactly as it did before.
pub const TYPED_RESULTS_ENV: &str = "SUBAGENT_TYPED_RESULTS_ENABLED";

/// The largest `result_schema` a fan-out call may carry, serialized.
pub const MAX_RESULT_SCHEMA_BYTES: usize = 8 * 1024;

/// Whether typed per-item results are on, from an already-read env value: only the literal `true`.
#[must_use]
pub fn parse_typed_results_enabled(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.trim() == "true")
}

/// Whether typed per-item results are on for this deployment right now.
#[must_use]
pub fn typed_results_enabled() -> bool {
    parse_typed_results_enabled(std::env::var(TYPED_RESULTS_ENV).ok().as_deref())
}

/// The fan-out tool's parameter schema with `result_schema` added to its properties. The tool is
/// offered this schema only while [`typed_results_enabled`] is on. Input that is not an object with
/// `properties` comes back unchanged.
#[must_use]
pub fn parameters_with_result_schema(parameters_json: &str) -> String {
    let Ok(mut schema) = serde_json::from_str::<Value>(parameters_json) else {
        return parameters_json.to_owned();
    };
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return parameters_json.to_owned();
    };
    properties.insert(
        "result_schema".to_owned(),
        serde_json::json!({
            "type": "object",
            "description": "Optional JSON Schema, of type object, that each item's answer must match. Each item then ends its answer with one fenced json block matching it; an item whose block does not match is reported as invalid_output, with the paths that failed."
        }),
    );
    schema.to_string()
}

#[cfg(test)]
thread_local! {
    /// A per-thread stand-in for the two budget env vars (`(enforced, budget)`), for the same
    /// reason as [`ENABLED_OVERRIDE`].
    static BUDGET_OVERRIDE: std::cell::Cell<Option<(bool, u64)>> =
        const { std::cell::Cell::new(None) };
}

/// Forces [`ChildBudget::from_env`] on the current thread until dropped. Test-only.
#[cfg(test)]
pub(crate) struct BudgetOverride;

#[cfg(test)]
impl BudgetOverride {
    pub(crate) fn set(enforced: bool, budget: u64) -> Self {
        BUDGET_OVERRIDE.with(|cell| cell.set(Some((enforced, budget))));
        Self
    }
}

#[cfg(test)]
impl Drop for BudgetOverride {
    fn drop(&mut self) {
        BUDGET_OVERRIDE.with(|cell| cell.set(None));
    }
}

/// One fan-out call's child token budget, resolved once when the call is dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildBudget {
    /// Metered tokens all of the call's finished children may spend.
    pub tokens: u64,
    /// Whether a spent budget stops further items, or is only logged.
    pub enforced: bool,
}

/// What the budget says about starting the next item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetCheck {
    /// Budget left: start the item.
    Within,
    /// Spent, but not enforced: start the item and log it.
    SpentNotEnforced,
    /// Spent and enforced: the item ends [`BUDGET_REACHED`] without starting.
    Reached,
}

impl ChildBudget {
    /// This deployment's budget.
    #[must_use]
    pub fn from_env() -> Self {
        #[cfg(test)]
        if let Some((enforced, tokens)) = BUDGET_OVERRIDE.with(std::cell::Cell::get) {
            return Self { tokens, enforced };
        }
        Self {
            tokens: parse_child_token_budget(std::env::var(CHILD_TOKEN_BUDGET_ENV).ok().as_deref()),
            enforced: parse_budget_enforced(std::env::var(BUDGET_ENFORCED_ENV).ok().as_deref()),
        }
    }

    /// Checked before each item starts, against the metered tokens of the call's FINISHED children.
    /// A started item is never stopped by it: the check sits before the start, so a child in
    /// flight when the budget runs out finishes, and the budget can be overshot by what the
    /// children in flight spend.
    #[must_use]
    pub fn check(self, finished_spent: u64) -> BudgetCheck {
        if finished_spent < self.tokens {
            BudgetCheck::Within
        } else if self.enforced {
            BudgetCheck::Reached
        } else {
            BudgetCheck::SpentNotEnforced
        }
    }
}

/// The metered tokens one inference round adds to a fan-out's child budget: its input and output,
/// unless the ChatGPT subscription served it. A subscription round is usage, not spend, so it adds
/// none (the gateway's run ceiling counts the same way); its rounds still count against the run's
/// round budget. A negative count is a provider bug and adds nothing.
#[must_use]
pub fn metered_tokens(provider_used: &str, input_tokens: i32, output_tokens: i32) -> u64 {
    if provider_used == mp_contracts::agentic_subscription::PROVIDER_ID {
        return 0;
    }
    let count = |tokens: i32| u64::try_from(tokens).unwrap_or(0);
    count(input_tokens).saturating_add(count(output_tokens))
}

/// The refusal an item ends with when the child token budget was spent before it started. Aimed at
/// the parent model, which should answer from the items that ran rather than delegate again.
#[must_use]
pub fn budget_reached_refusal(spent: u64, budget: u64) -> String {
    format!(
        "{BUDGET_REACHED}: not started, because this fan-out's children had already spent {spent} \
         of their {budget}-token budget. Answer from the items that ran and name this one as not \
         researched."
    )
}

/// The fan-out batch a retry run's items join (EP-2 S9): the batch named on the request when it
/// is a retry, so the retried item is grouped with the batch it came from. `None` for every other
/// run, whose fan-out call names its own batch.
#[must_use]
pub fn retry_batch_id<'a>(retry_of_child_run_id: &str, batch_id: &'a str) -> Option<&'a str> {
    (!retry_of_child_run_id.is_empty() && !batch_id.is_empty()).then_some(batch_id)
}

/// What a retry is refused with when the child run cannot be read back or is not a fan-out item
/// of this conversation. One text for every cause, so a refusal does not say whether a run with
/// that id exists for someone else.
#[must_use]
pub fn retry_unavailable_refusal() -> String {
    "That wide-research item cannot be retried: it is not an item of a wide-research run in this \
     conversation that you can read."
        .to_owned()
}

/// The item a retry runs again: the retried child run's goal, after checking the run is a fan-out
/// item (its agent is this tool's label and it has a parent run) of the same thread as the retry.
/// The caller reads the run under the person's own credential, so session-core's run-owner check
/// has already passed.
///
/// # Errors
///
/// [`retry_unavailable_refusal`] for a run of another thread, one that is not a fan-out item, or
/// one with no goal left to research.
pub fn retry_item_goal(
    thread_id: &str,
    child_thread_id: &str,
    child_parent_run_id: &str,
    child_agent_id: &str,
    child_goal: &str,
) -> Result<String, String> {
    let is_item = !thread_id.is_empty()
        && child_thread_id == thread_id
        && !child_parent_run_id.is_empty()
        && child_agent_id == super::label(RESEARCH_TOOL_NAME);
    match wide_research_item(child_goal) {
        Some(goal) if is_item => Ok(goal),
        _ => Err(retry_unavailable_refusal()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, goal: &str) -> FanOutItem {
        FanOutItem {
            id: id.to_owned(),
            goal: goal.to_owned(),
        }
    }

    fn items(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn plan_wide_research_keeps_the_cap_and_names_the_rest() {
        let plan = plan_wide_research(&items(&["a", "b", "c", "d", "e"]), 3).expect("cap 3");
        assert_eq!(plan.cap, 3);
        assert_eq!(plan.kept, items(&["a", "b", "c"]));
        assert_eq!(plan.dropped, items(&["d", "e"]));
        let note = dropped_items_note(&plan).expect("two items were dropped");
        assert!(note.contains("limit of 3"), "{note}");
        assert!(note.contains("\n- d") && note.contains("\n- e"), "{note}");
        let context = wide_research_goal_context(&plan);
        assert!(context.contains(RESEARCH_TOOL_NAME), "{context}");
        assert!(context.contains("1. a\n2. b\n3. c\n"), "{context}");
        assert!(context.contains("- d\n- e\n"), "{context}");
    }

    #[test]
    fn plan_wide_research_with_cap_zero_is_refused() {
        let refused = plan_wide_research(&items(&["a"]), 0).expect_err("the org has it off");
        assert_eq!(refused, wide_research_off_refusal());
    }

    #[test]
    fn plan_wide_research_holds_an_unclamped_cap_to_sixteen() {
        let many: Vec<String> = (0..20).map(|n| format!("item {n}")).collect();
        let plan = plan_wide_research(&many, 40).expect("cap held to 16");
        assert_eq!(plan.cap, crate::subagent::MAX_WIDE_RESEARCH_ITEMS);
        assert_eq!(plan.kept.len(), 16);
        assert_eq!(plan.dropped.len(), 4);
    }

    #[test]
    fn plan_wide_research_cleans_cuts_and_bounds_the_items() {
        let long = "x".repeat(MAX_WIDE_RESEARCH_ITEM_CHARS + 50);
        let plan = plan_wide_research(&items(&["  a\u{0}b  ", "   ", &long]), 8).expect("cap 8");
        assert_eq!(plan.kept[0], "a b");
        assert_eq!(plan.kept.len(), 2, "a blank item is skipped");
        assert_eq!(plan.kept[1].chars().count(), MAX_WIDE_RESEARCH_ITEM_CHARS);

        let flood: Vec<String> = (0..100).map(|n| format!("q{n}")).collect();
        let plan = plan_wide_research(&flood, 16).expect("cap 16");
        assert_eq!(
            plan.kept.len() + plan.dropped.len(),
            MAX_WIDE_RESEARCH_REQUEST_ITEMS
        );
    }

    #[test]
    fn plan_wide_research_without_items_leaves_the_split_to_the_model() {
        let plan = plan_wide_research(&[], 5).expect("a count preset");
        assert!(plan.kept.is_empty() && plan.dropped.is_empty());
        assert_eq!(dropped_items_note(&plan), None);
        let context = wide_research_goal_context(&plan);
        assert!(context.contains("at most 5"), "{context}");
    }

    #[test]
    fn parse_fan_out_accepts_one_to_n_items_and_rejects_empty() {
        let request = parse_fan_out(
            r#"{"items":[{"goal":"look up A"},{"id":"b","goal":"look up B"}]}"#,
            8,
        )
        .expect("two items under the ceiling parse");
        assert_eq!(request.items.len(), 2);
        assert_eq!(request.items[0].id, "item-0");
        assert_eq!(request.items[0].goal, "look up A");
        assert_eq!(request.items[1].id, "b");
        assert_eq!(request.items[1].goal, "look up B");

        let no_items =
            parse_fan_out(r#"{"items":[]}"#, 8).expect_err("an empty fan-out is not runnable");
        assert!(no_items.contains("at least one"), "{no_items}");

        let missing_goal = parse_fan_out(r#"{"items":[{"id":"a"}]}"#, 8)
            .expect_err("an item with no goal is not runnable");
        assert!(missing_goal.contains("goal"), "{missing_goal}");

        assert!(parse_fan_out("not-json", 8).is_err());
    }

    #[test]
    fn parse_fan_out_rejects_more_items_than_the_run_ceiling() {
        let too_many = parse_fan_out(
            r#"{"items":[{"goal":"a"},{"goal":"b"},{"goal":"c"}]}"#,
            2,
        )
        .expect_err("3 items must be refused against a ceiling of 2");
        assert!(
            too_many.contains("per-run delegation limit of 2"),
            "the refusal must name the real ceiling: {too_many}"
        );

        // Exactly at the ceiling is fine — the bound is inclusive.
        assert!(parse_fan_out(r#"{"items":[{"goal":"a"},{"goal":"b"}]}"#, 2).is_ok());
    }

    /// Item ids are model-supplied and are echoed into the digest, so they are
    /// bounded and unique: two entries can never share a heading the parent
    /// would then have to disambiguate.
    #[test]
    fn parse_fan_out_bounds_and_deduplicates_item_ids() {
        let long_id = "x".repeat(200);
        let input = format!(
            r#"{{"items":[{{"id":"{long_id}","goal":"a"}},{{"id":"dup","goal":"b"}},{{"id":"dup","goal":"c"}},{{"id":"  ","goal":"d"}}]}}"#
        );
        let request = parse_fan_out(&input, 8).expect("parses");
        assert_eq!(request.items[0].id.chars().count(), MAX_ID_CHARS);
        assert_eq!(request.items[1].id, "dup");
        assert_eq!(request.items[2].id, "dup-2", "a repeated id is disambiguated");
        assert_eq!(request.items[3].id, "item-3", "a blank id is generated");
        let unique: std::collections::BTreeSet<&str> =
            request.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(unique.len(), request.items.len());
    }

    /// `result_schema_hint` is parsed AND reaches the item's prompt; the bare
    /// goal stays the thing that is recorded as what was asked.
    #[test]
    fn the_schema_hint_reaches_each_items_prompt_and_the_goal_stays_bare() {
        let request = parse_fan_out(
            r#"{"items":[{"goal":"price of X"}],"result_schema_hint":"  one sentence with the price  "}"#,
            8,
        )
        .expect("parses");
        assert_eq!(
            request.result_schema_hint.as_deref(),
            Some("one sentence with the price")
        );
        let prompt = compose_item_prompt(
            &request.items[0],
            request.result_schema_hint.as_deref(),
            None,
        );
        assert!(prompt.starts_with("price of X\n\n"), "{prompt}");
        assert!(prompt.contains("Sources:"), "the citation ask must be there: {prompt}");
        assert!(
            prompt.contains("Requested answer format: one sentence with the price"),
            "{prompt}"
        );
        assert_eq!(request.items[0].goal, "price of X");

        // No hint → no dangling "Requested answer format:" line.
        let bare = compose_item_prompt(&request.items[0], None, None);
        assert!(!bare.contains("Requested answer format"), "{bare}");

        // A blank hint is dropped at parse time, not forwarded as an empty ask.
        let blank = parse_fan_out(
            r#"{"items":[{"goal":"g"}],"result_schema_hint":"   "}"#,
            8,
        )
        .expect("parses");
        assert_eq!(blank.result_schema_hint, None);
    }

    #[test]
    fn merge_reports_partial_failure_honestly() {
        let items: Vec<FanOutItem> = (0..10)
            .map(|i| FanOutItem {
                id: format!("item-{i}"),
                goal: format!("question {i}"),
            })
            .collect();
        let mut results: Vec<FanOutItemResult> = Vec::new();
        for item in items.iter().take(6) {
            results.push(FanOutItemResult::ok(item, format!("answer for {}", item.id)));
        }
        for item in items.iter().skip(6) {
            results.push(FanOutItemResult::refused(
                item,
                "delegation refused: this run has already started 40 subagents, which is the \
                 per-run limit of 40."
                    .to_owned(),
            ));
        }

        let merged = merge(&results);
        assert!(merged.contains("verification: {\"checked\": 10, \"confirmed\": 6}"));
        for item in items.iter().take(6) {
            assert!(merged.contains(&format!("answer for {}", item.id)));
        }
        for item in items.iter().skip(6) {
            assert!(
                merged.contains(&item.id),
                "a refused item must still be named, not dropped: {merged}"
            );
        }
        assert!(
            merged.contains("Status: refused"),
            "the real refusal status must survive into the merge: {merged}"
        );
        assert!(
            merged.matches("per-run limit of 40").count() == 4,
            "every refused item keeps its own real reason: {merged}"
        );
    }

    #[test]
    fn merge_preserves_per_item_sources_not_a_flattened_blob() {
        let item_a = item("a", "find carrier docs");
        let item_b = item("b", "find pricing docs");
        let results = vec![
            FanOutItemResult::ok(
                &item_a,
                "Carrier X ships to Svalbard, see https://carrier-x.example/svalbard.".to_owned(),
            ),
            FanOutItemResult::ok(
                &item_b,
                "Pricing is on https://pricing.example/rates and https://pricing.example/faq."
                    .to_owned(),
            ),
        ];

        assert_eq!(results[0].sources, vec!["https://carrier-x.example/svalbard"]);
        assert_eq!(
            results[1].sources,
            vec![
                "https://pricing.example/rates",
                "https://pricing.example/faq"
            ]
        );

        let merged = merge(&results);
        // Item a's URL must not appear under item b's heading, and vice
        // versa — a flattened blob would lose this attribution.
        let a_section = merged.split("## [b]").next().unwrap();
        assert!(a_section.contains("https://carrier-x.example/svalbard"));
        assert!(!a_section.contains("https://pricing.example/rates"));
    }

    /// The failure this guards: the agent loop keeps a bounded slice of a tool
    /// result and cuts the TAIL. Forty items with a few hundred words each would
    /// have lost every item past the first handful — silently, along with the
    /// summary line. The digest budgets the answers instead, keeps every item's
    /// heading, and says where it cut.
    #[test]
    fn merge_fits_the_context_budget_and_keeps_every_item() {
        let long_answer = format!("{} https://src.example/1", "finding ".repeat(400));
        let results: Vec<FanOutItemResult> = (0..40)
            .map(|i| FanOutItemResult::ok(&item(&format!("q{i}"), &format!("question {i}")), long_answer.clone()))
            .collect();

        let merged = merge(&results);
        assert!(
            merged.chars().count() <= MERGE_BUDGET_CHARS,
            "the digest must fit its budget (and so the loop's {CONTEXT_CHARS}-character \
             allowance for this tool), got {}",
            merged.chars().count()
        );
        for i in 0..40 {
            assert!(
                merged.contains(&format!("## [q{i}]")),
                "item q{i} must survive the budget: {merged}"
            );
        }
        assert!(merged.contains("answer truncated"), "a cut must be announced");
        // The summary line is in the first two lines, out of a tail-cut's reach.
        let head: Vec<&str> = merged.lines().take(2).collect();
        assert!(
            head[1].starts_with("verification: {\"checked\": 40, \"confirmed\": 40}"),
            "{head:?}"
        );
        // Each item still keeps its source, attributed to it.
        assert_eq!(merged.matches("https://src.example/1").count(), 40);
    }

    /// Sources are the one fixed part of a section that grows with the model's own
    /// output. Forty items each citing eight long URLs is ~36k characters of
    /// sources alone: listed in full they would eat the whole budget before any
    /// answer got its floor, and the loop's tail-cut would then drop the last
    /// items. The digest lists fewer sources per item instead — and says so.
    #[test]
    fn a_large_fan_out_with_many_sources_still_fits_and_keeps_every_item() {
        let results: Vec<FanOutItemResult> = (0..40)
            .map(|i| {
                let urls: Vec<String> = (0..8)
                    .map(|j| format!("https://s.example/{i}/{j}/{}", "p".repeat(70)))
                    .collect();
                let answer = format!("{} {}", "finding ".repeat(400), urls.join(" "));
                FanOutItemResult::ok(&item(&format!("q{i}"), &format!("question {i}")), answer)
            })
            .collect();
        assert!(
            results.iter().all(|r| r.sources.len() == MAX_SOURCES_PER_ITEM),
            "the fixture must really carry the maximum sources per item"
        );

        let merged = merge(&results);
        assert!(
            merged.chars().count() <= MERGE_BUDGET_CHARS,
            "sources must not push the digest past its budget, got {}",
            merged.chars().count()
        );
        for i in 0..40 {
            assert!(merged.contains(&format!("## [q{i}]")), "item q{i} was lost");
            assert!(
                merged.contains(&format!("https://s.example/{i}/0/")),
                "item q{i} kept none of its sources"
            );
        }
        assert!(merged.contains("more cited"), "the shortened source lists must say so");
        let head: Vec<&str> = merged.lines().take(2).collect();
        assert!(head[1].starts_with("verification: {\"checked\": 40, \"confirmed\": 40}"), "{head:?}");
    }

    /// A small fan-out keeps every source — the reduction is a last resort.
    #[test]
    fn a_small_fan_out_lists_all_its_sources() {
        let urls: Vec<String> = (0..8).map(|j| format!("https://s.example/{j}")).collect();
        let results = vec![FanOutItemResult::ok(&item("a", "g"), urls.join(" "))];
        let merged = merge(&results);
        for url in &urls {
            assert!(merged.contains(url.as_str()), "{merged}");
        }
        assert!(!merged.contains("more cited"), "{merged}");
    }

    /// A model writes a question as prose. The heading is one line however the
    /// question (or the id) was written, or the second line of it reads as the
    /// first line of the answer.
    #[test]
    fn a_multi_line_goal_or_id_stays_on_one_heading_line() {
        let results = vec![FanOutItemResult::ok(
            &item("a\nb", "Compare X and Y.\nCover:\n\t- price"),
            "the answer".to_owned(),
        )];
        let merged = merge(&results);
        assert!(
            merged.lines().any(|line| line == "## [a b] Compare X and Y. Cover: - price"),
            "{merged}"
        );
    }

    /// The budget only bites when it has to: a small fan-out is returned whole.
    #[test]
    fn merge_does_not_truncate_a_fan_out_that_fits() {
        let answer = "short answer ".repeat(20);
        let results = vec![FanOutItemResult::ok(&item("a", "g"), answer.clone())];
        let merged = merge(&results);
        assert!(merged.contains(answer.trim()));
        assert!(!merged.contains("answer truncated"), "{merged}");
    }

    #[test]
    fn extract_sources_dedupes_and_caps() {
        let repeated = "See https://a.example/1 and https://a.example/1 again, plus https://a.example/1.";
        assert_eq!(extract_sources(repeated), vec!["https://a.example/1"]);

        let many = (0..20)
            .map(|i| format!("https://a.example/{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(extract_sources(&many).len(), MAX_SOURCES_PER_ITEM);
    }

    /// The round pool is divided so the shares differ by at most one, the
    /// remainder is not thrown away, and one round always stays with the
    /// parent so it can read the digest.
    #[test]
    fn item_round_share_divides_evenly_and_reserves_the_parents_round() {
        let shares = |pool: u32, count: u32| -> Vec<u32> {
            (0..count).map(|i| item_round_share(pool, count, i)).collect()
        };
        // 7 = 1 reserved + 6 → 2,2,2.
        assert_eq!(shares(7, 3), vec![2, 2, 2]);
        // 8 = 1 reserved + 7 → the remainder goes to the first item, not away.
        assert_eq!(shares(8, 3), vec![3, 2, 2]);
        // Everything handed out plus the reserve is exactly the pool.
        for pool in 0..40u32 {
            for count in 1..12u32 {
                let total: u32 = shares(pool, count).iter().sum();
                assert_eq!(
                    total,
                    pool.saturating_sub(PARENT_SYNTHESIS_RESERVE),
                    "pool={pool} count={count}"
                );
                let max = shares(pool, count).into_iter().max().unwrap();
                let min = shares(pool, count).into_iter().min().unwrap();
                assert!(max - min <= 1, "shares must differ by at most one");
            }
        }
        // Fewer distributable rounds than items: the first ones get one, the
        // rest honestly get none (and are refused), rather than everyone
        // getting a zero share.
        assert_eq!(shares(4, 5), vec![1, 1, 1, 0, 0]);
        // A pool of one is entirely the parent's; a zero-item call divides nothing.
        assert_eq!(shares(1, 3), vec![0, 0, 0]);
        assert_eq!(item_round_share(9, 0, 0), 0);
    }

    #[test]
    fn env_parsing_defaults_to_enabled_with_the_owners_concurrency() {
        assert!(parse_enabled(None));
        assert!(parse_enabled(Some("")));
        assert!(parse_enabled(Some("true")));
        assert!(parse_enabled(Some("1")));
        // An unrecognised value does not silently disable the tool.
        assert!(parse_enabled(Some("banana")));
        for off in ["0", "false", "FALSE", " off ", "no", "Disabled"] {
            assert!(!parse_enabled(Some(off)), "{off:?} must switch it off");
        }

        assert_eq!(parse_concurrency(None), DEFAULT_CONCURRENCY);
        assert_eq!(DEFAULT_CONCURRENCY, 4, "owner ruling: start at 4");
        assert_eq!(parse_concurrency(Some("junk")), DEFAULT_CONCURRENCY);
        assert_eq!(parse_concurrency(Some("0")), DEFAULT_CONCURRENCY);
        assert_eq!(parse_concurrency(Some("-3")), DEFAULT_CONCURRENCY);
        assert_eq!(parse_concurrency(Some(" 6 ")), 6);
        assert_eq!(parse_concurrency(Some("400")), MAX_CONCURRENCY);
    }

    #[test]
    fn the_disabled_refusal_tells_the_model_what_to_do_instead() {
        let refusal = disabled_refusal();
        assert!(refusal.contains(RESEARCH_TOOL_NAME), "{refusal}");
        assert!(refusal.contains("subagent.task"), "{refusal}");
    }

    #[test]
    fn the_budget_switch_is_only_the_literal_true_and_the_budget_defaults_to_400k() {
        for (raw, on) in [
            (None, false),
            (Some("false"), false),
            (Some("1"), false),
            (Some("TRUE"), false),
            (Some("yes"), false),
            (Some("true"), true),
        ] {
            assert_eq!(parse_budget_enforced(raw), on, "{raw:?}");
        }
        assert_eq!(DEFAULT_CHILD_TOKEN_BUDGET, 400_000, "decision D4");
        assert_eq!(parse_child_token_budget(None), DEFAULT_CHILD_TOKEN_BUDGET);
        assert_eq!(parse_child_token_budget(Some("junk")), DEFAULT_CHILD_TOKEN_BUDGET);
        assert_eq!(parse_child_token_budget(Some("0")), DEFAULT_CHILD_TOKEN_BUDGET);
        assert_eq!(parse_child_token_budget(Some(" 50000 ")), 50_000);
    }

    #[test]
    fn a_spent_budget_refuses_only_when_enforced() {
        let enforced = ChildBudget {
            tokens: 100,
            enforced: true,
        };
        assert_eq!(enforced.check(0), BudgetCheck::Within);
        assert_eq!(enforced.check(99), BudgetCheck::Within);
        assert_eq!(enforced.check(100), BudgetCheck::Reached);
        assert_eq!(enforced.check(5_000), BudgetCheck::Reached);
        let logged = ChildBudget {
            tokens: 100,
            enforced: false,
        };
        assert_eq!(logged.check(99), BudgetCheck::Within);
        assert_eq!(logged.check(100), BudgetCheck::SpentNotEnforced);
    }

    #[test]
    fn subscription_rounds_spend_no_metered_tokens() {
        assert_eq!(metered_tokens("anthropic", 1_200, 300), 1_500);
        assert_eq!(
            metered_tokens(mp_contracts::agentic_subscription::PROVIDER_ID, 1_200, 300),
            0,
            "a subscription child never trips the token budget"
        );
        assert_eq!(metered_tokens("anthropic", -5, 10), 10, "a negative count adds nothing");
    }

    #[test]
    fn budget_reached_items_are_refused_named_and_counted_in_the_digest() {
        let items: Vec<FanOutItem> = (0..10)
            .map(|i| item(&format!("q{i}"), &format!("question {i}")))
            .collect();
        let mut results: Vec<FanOutItemResult> = items
            .iter()
            .take(6)
            .map(|item| FanOutItemResult::ok(item, format!("answer {}", item.id)))
            .collect();
        results.extend(
            items
                .iter()
                .skip(6)
                .map(|item| FanOutItemResult::budget_reached(item, 410_000, 400_000)),
        );
        assert!(results[6].is_budget_reached());
        assert!(!results[0].is_budget_reached());
        assert!(
            !FanOutItemResult::refused(&items[0], "per-run limit".to_owned()).is_budget_reached(),
            "another refusal is not a budget refusal"
        );

        let merged = merge(&results);
        assert!(merged.contains("verification: {\"checked\": 10, \"confirmed\": 6}"));
        assert!(
            merged.contains("budget: 4 of 10 item(s) were not started"),
            "{merged}"
        );
        assert_eq!(merged.matches("Reason: budget_reached").count(), 4, "{merged}");
        for item in items.iter().skip(6) {
            assert!(merged.contains(&item.id), "an unstarted item is still named: {merged}");
        }
    }

    #[test]
    fn a_digest_without_a_budget_refusal_has_no_budget_line() {
        let results = vec![
            FanOutItemResult::ok(&item("a", "question a"), "answer a".to_owned()),
            FanOutItemResult::refused(&item("b", "question b"), "per-run limit".to_owned()),
        ];
        assert!(!merge(&results).contains("budget:"));
    }

    #[test]
    fn a_retry_joins_the_batch_it_names_and_only_a_retry_does() {
        assert_eq!(
            retry_batch_id("run-child", "run-1:tool_1_fan-1"),
            Some("run-1:tool_1_fan-1")
        );
        assert_eq!(retry_batch_id("", "run-1:tool_1_fan-1"), None);
        assert_eq!(retry_batch_id("run-child", ""), None);
    }

    #[test]
    fn a_retry_reads_back_only_a_fan_out_item_of_the_same_thread() {
        let label = super::super::label(RESEARCH_TOOL_NAME);
        assert_eq!(
            retry_item_goal("thread-1", "thread-1", "run-parent", label, "  look up A \n"),
            Ok("look up A".to_owned())
        );
        let refused = Err(retry_unavailable_refusal());
        // Another conversation's run, even one the person owns.
        assert_eq!(
            retry_item_goal("thread-1", "thread-2", "run-parent", label, "look up A"),
            refused
        );
        // A top-level run, or a child that is not a fan-out item.
        assert_eq!(
            retry_item_goal("thread-1", "thread-1", "", label, "look up A"),
            refused
        );
        assert_eq!(
            retry_item_goal("thread-1", "thread-1", "run-parent", "task", "look up A"),
            refused
        );
        // A child with no goal left (a zero-retention child stores none).
        assert_eq!(
            retry_item_goal("thread-1", "thread-1", "run-parent", label, "   "),
            refused
        );
        assert_eq!(retry_item_goal("", "", "run-parent", label, "look up A"), refused);
    }
}
