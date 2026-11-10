//! The deferred tool index of a Do loop (build plan W5-070).
//!
//! A run whose catalogue is large offers the model a short list: the built-in tools, every tool
//! that came from a client or an MCP listing only once the model has searched for it, and the
//! `tool_search` tool that does the searching. With `EXECUTION_DEFERRED_TOOL_INDEX_ENABLED` off,
//! or a catalogue of no more than `EXECUTION_DEFERRED_TOOL_INDEX_THRESHOLD` definitions, the offer
//! is the catalogue as it always was.
//!
//! Nothing here widens authority. The deferred names are a subset of the run's own catalogue. The
//! purpose lock, argument validation and every capability, permission and hook gate still run
//! against that full catalogue. A revealed tool is offered from the round after the search that
//! revealed it, and a call to a deferred tool that no search revealed is refused with a
//! model-readable error. `tool_search` is answered in the dispatch pre-pass: it reads the
//! catalogue, writes nothing, and is never asked for approval.
//!
//! The revealed set belongs to one loop, a run's rounds or one delegated child's, and lives in
//! memory only. An approval continuation and a park resume run under the tools they retained, so
//! the index does not apply to them.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mp_contracts::model_plane::v1 as pb;

/// The switch. Off (the default) offers every run its whole catalogue. Only the literal `true`
/// turns it on.
pub(crate) const DEFERRED_INDEX_ENV: &str = "EXECUTION_DEFERRED_TOOL_INDEX_ENABLED";

/// The catalogue size above which the index applies.
pub(crate) const DEFERRED_INDEX_THRESHOLD_ENV: &str = "EXECUTION_DEFERRED_TOOL_INDEX_THRESHOLD";

/// The default threshold: a catalogue of more than this many definitions is indexed.
pub(crate) const DEFERRED_INDEX_THRESHOLD: usize = 40;

/// The synthetic discovery tool the index offers.
pub(crate) const TOOL_SEARCH_NAME: &str = "tool_search";

/// The matches `tool_search` returns when the model names no limit.
const DEFAULT_LIMIT: usize = 5;

/// The most matches one `tool_search` call returns.
const MAX_LIMIT: usize = 8;

/// The prefix every MCP tool name carries. Such a tool is always deferred.
const MCP_TOOL_PREFIX: &str = "mcp__";

/// The most characters of a tool's description one match shows.
const DESCRIPTION_CHARS: usize = 240;

/// Whether an explicit value turns the switch on. Only the literal `true` counts, after trimming,
/// the same strict reading as the other Do-loop switches.
#[must_use]
pub(crate) fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Whether this process indexes the deferred tools of a Do loop.
#[must_use]
pub(crate) fn enabled() -> bool {
    parse_enabled(std::env::var(DEFERRED_INDEX_ENV).ok().as_deref())
}

/// The threshold an explicit value names. A missing or unparseable value is the default.
#[must_use]
pub(crate) fn parse_threshold(raw: Option<&str>) -> usize {
    raw.map(str::trim)
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(DEFERRED_INDEX_THRESHOLD)
}

/// The threshold this process applies.
#[must_use]
pub(crate) fn threshold() -> usize {
    parse_threshold(std::env::var(DEFERRED_INDEX_THRESHOLD_ENV).ok().as_deref())
}

/// The offered definition of `tool_search`.
#[must_use]
pub(crate) fn tool_search_def() -> pb::ToolDefinition {
    pb::ToolDefinition {
        name: TOOL_SEARCH_NAME.to_owned(),
        description: "Search for a tool that is not listed in full here. Give keywords for the capability you need. The best matches are offered to you from your next round, and you can call them by name then. Search before you call a tool you cannot see.".to_owned(),
        parameters_json: r#"{"type":"object","properties":{"query":{"type":"string","description":"Keywords for the capability you need, for example calendar events or read file"},"limit":{"type":"integer","minimum":1,"maximum":8,"description":"Most matches to return, from 1 to 8; default 5"}},"required":["query"]}"#.to_owned(),
    }
}

/// The tools of one loop's catalogue that are held back until `tool_search` reveals them. It is
/// inactive, and offers the catalogue whole, unless the switch is on, the catalogue is over the
/// threshold, and it holds a tool that came from outside the built-in catalogue.
#[derive(Debug, Default, Clone)]
pub(crate) struct DeferredIndex {
    names: BTreeSet<String>,
}

impl DeferredIndex {
    /// The index for a fresh run's full catalogue. `untrusted` names every definition that was
    /// appended from a client or an MCP listing, and an `mcp__` tool is deferred whatever its source.
    #[must_use]
    pub(crate) fn plan(
        on: bool,
        threshold: usize,
        catalogue: &[pb::ToolDefinition],
        untrusted: &BTreeSet<String>,
    ) -> Self {
        // A catalogue that already names `tool_search` keeps its own tool: the synthetic one is not
        // offered beside it.
        if !on
            || catalogue.len() <= threshold
            || catalogue.iter().any(|tool| tool.name == TOOL_SEARCH_NAME)
        {
            return Self::default();
        }
        Self {
            names: catalogue
                .iter()
                .filter(|tool| {
                    untrusted.contains(&tool.name) || tool.name.starts_with(MCP_TOOL_PREFIX)
                })
                .map(|tool| tool.name.clone())
                .collect(),
        }
    }

    /// The index limited to the tools a loop holds. A delegated child keeps the deferral of each
    /// tool it is offered, and a tool it is not offered is not deferred for it.
    #[must_use]
    pub(crate) fn restricted_to(&self, tools: &[pb::ToolDefinition]) -> Self {
        let present: BTreeSet<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        Self {
            names: self
                .names
                .iter()
                .filter(|name| present.contains(name.as_str()))
                .cloned()
                .collect(),
        }
    }

    /// Whether the loop holds a deferred tool, so that its offer is the index's and not the
    /// catalogue's.
    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        !self.names.is_empty()
    }

    /// The tools one round offers: the catalogue less the deferred tools no search has revealed,
    /// with `tool_search` at the end. Inactive, it is the catalogue itself.
    #[must_use]
    pub(crate) fn offer(
        &self,
        catalogue: &[pb::ToolDefinition],
        revealed: &RevealedTools,
    ) -> Vec<pb::ToolDefinition> {
        if !self.is_active() {
            return catalogue.to_vec();
        }
        let mut offered: Vec<pb::ToolDefinition> = catalogue
            .iter()
            .filter(|tool| !self.names.contains(&tool.name) || revealed.contains(&tool.name))
            .cloned()
            .collect();
        offered.push(tool_search_def());
        offered
    }

    /// The refusal of a call to a deferred tool that no search has revealed, or `None` when the
    /// call may go on. The purpose lock has already admitted the name from the full catalogue.
    #[must_use]
    pub(crate) fn not_loaded_refusal(
        &self,
        name: &str,
        revealed: &RevealedTools,
    ) -> Option<String> {
        (self.names.contains(name) && !revealed.contains(name)).then(|| {
            format!(
                "tool '{name}' is not loaded in this run yet, so it cannot be called. Call \
                 {} with keywords for it first, then call it in a later round.",
                TOOL_SEARCH_NAME
            )
        })
    }

    /// Answers one `tool_search` call from the catalogue, against the tools this index holds.
    pub(crate) fn answer(
        &self,
        catalogue: &[pb::ToolDefinition],
        input: &str,
    ) -> Result<Answer, String> {
        let (query, limit) = parse_args(input)?;
        let matches = search(catalogue, &self.names, &query, limit);
        Ok(Answer {
            text: render(&matches),
            revealed: matches.iter().map(|tool| tool.name.clone()).collect(),
        })
    }
}

/// What one `tool_search` call answers the model with, and which tools it reveals.
#[derive(Debug)]
pub(crate) struct Answer {
    /// The result the model reads.
    pub(crate) text: String,
    /// The matched tools, offered from the next round on.
    pub(crate) revealed: Vec<String>,
}

/// The deferred tools one loop has revealed. Only names from its own index are ever added, so a
/// revealed tool is always one the run's catalogue already held.
#[derive(Debug, Default, Clone)]
pub(crate) struct RevealedTools {
    names: BTreeSet<String>,
}

impl RevealedTools {
    /// Whether `name` has been revealed.
    #[must_use]
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    /// Reveals `names`, which the next round offers.
    pub(crate) fn reveal(&mut self, names: Vec<String>) {
        self.names.extend(names);
    }
}

/// The query and the limit of one `tool_search` call. A call without a usable query is refused,
/// and a limit outside 1 to 8 is brought into that range.
fn parse_args(input: &str) -> Result<(String, usize), String> {
    let value: serde_json::Value = serde_json::from_str(input)
        .map_err(|error| format!("invalid tool_search input: {error}"))?;
    let query = value
        .get("query")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "tool_search requires a non-empty 'query'".to_owned())?;
    let limit = value
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .map_or(DEFAULT_LIMIT, |limit| {
            usize::try_from(limit)
                .unwrap_or(MAX_LIMIT)
                .clamp(1, MAX_LIMIT)
        });
    Ok((query.to_owned(), limit))
}

/// The deferred tools that match `query`, best first and then by name, at most `limit` of them.
/// A tool matches when a word of the query starts a word of its name or description, compared
/// case-insensitively. The score is the number of query words that match.
fn search<'a>(
    catalogue: &'a [pb::ToolDefinition],
    deferred: &BTreeSet<String>,
    query: &str,
    limit: usize,
) -> Vec<&'a pb::ToolDefinition> {
    let mut terms = tokens(query);
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, &pb::ToolDefinition)> = catalogue
        .iter()
        .filter(|tool| deferred.contains(&tool.name))
        .filter_map(|tool| {
            let words = tokens(&format!("{} {}", tool.name, tool.description));
            let score = terms
                .iter()
                .filter(|term| words.iter().any(|word| word.starts_with(term.as_str())))
                .count();
            (score > 0).then_some((score, tool))
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, tool)| tool)
        .collect()
}

/// The lower-case words of `text`, split on anything that is not a letter or a digit.
fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The text `tool_search` answers with: one line per match, its name and its description.
fn render(matches: &[&pb::ToolDefinition]) -> String {
    if matches.is_empty() {
        return "No tool matched. Try other keywords for the capability you need.".to_owned();
    }
    let mut text = format!(
        "{} tool(s) matched. Each is offered to you from your next round, and you can call it then:\n",
        matches.len()
    );
    for tool in matches {
        let description = tool
            .description
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let _ = writeln!(
            text,
            "- {}: {}",
            tool.name,
            clip(&description, DESCRIPTION_CHARS)
        );
    }
    text
}

/// `text` shortened to `max_chars` characters, with a marker when it was cut.
fn clip(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut clipped: String = text.chars().take(max_chars).collect();
    clipped.push_str("...");
    clipped
}
