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

use serde::Deserialize;

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

    Ok(FanOutRequest {
        items,
        max_rounds_per_item: raw.max_rounds_per_item,
        result_schema_hint: raw
            .result_schema_hint
            .map(|hint| hint.trim().chars().take(MAX_HINT_CHARS).collect::<String>())
            .filter(|hint| !hint.is_empty()),
    })
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
/// child, not part of what was asked.
#[must_use]
pub fn compose_item_prompt(item: &FanOutItem, result_schema_hint: Option<&str>) -> String {
    let mut prompt = format!("{}\n\n{ITEM_PROMPT_SUFFIX}", item.goal);
    if let Some(hint) = result_schema_hint {
        prompt.push_str("\nRequested answer format: ");
        prompt.push_str(hint);
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
/// the item's sources are listed; the rest are counted, not dropped unseen.
fn render_item(out: &mut String, result: &FanOutItemResult, answer_cap: usize, sources_cap: usize) {
    use std::fmt::Write as _;
    let (goal, goal_omitted) = clip(&one_line(&result.goal), MAX_HEADER_GOAL_CHARS);
    let ellipsis = if goal_omitted > 0 { "…" } else { "" };
    let _ = writeln!(out);
    let _ = writeln!(out, "## [{}] {goal}{ellipsis}", one_line(&result.id));
    match result.status {
        ItemStatus::Ok => {
            let _ = writeln!(out, "Status: ok");
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

    let answered = results.iter().filter(|r| r.answer.is_some()).count();
    // What every section costs before any answer is added, at a given number of
    // listed sources.
    let fixed_at = |sources_cap: usize| -> usize {
        results
            .iter()
            .map(|result| {
                let mut section = String::new();
                render_item(&mut section, result, 0, sources_cap);
                section.chars().count()
            })
            .sum::<usize>()
            + head.chars().count()
    };
    let answers_floor = answered * (MIN_ANSWER_CHARS + TRUNCATION_NOTE_RESERVE);
    let mut sources_cap = MAX_SOURCES_PER_ITEM;
    while sources_cap > 0 && fixed_at(sources_cap) + answers_floor > budget {
        sources_cap /= 2;
    }
    let fixed = fixed_at(sources_cap);
    let answer_cap = if answered == 0 {
        0
    } else {
        (budget.saturating_sub(fixed) / answered)
            .saturating_sub(TRUNCATION_NOTE_RESERVE)
            .max(MIN_ANSWER_CHARS)
    };

    let mut out = head;
    for result in results {
        render_item(&mut out, result, answer_cap, sources_cap);
    }
    out
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
        );
        assert!(prompt.starts_with("price of X\n\n"), "{prompt}");
        assert!(prompt.contains("Sources:"), "the citation ask must be there: {prompt}");
        assert!(
            prompt.contains("Requested answer format: one sentence with the price"),
            "{prompt}"
        );
        assert_eq!(request.items[0].goal, "price of X");

        // No hint → no dangling "Requested answer format:" line.
        let bare = compose_item_prompt(&request.items[0], None);
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
}
