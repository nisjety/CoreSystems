//! Golden manifest of every platform prompt text and offered tool definition (EP-14-S8).
//!
//! The manifest records one sha256 per item under a stable id. A prompt item is every const,
//! static and fn, at any depth, that sits before the first `#[cfg(test)]` of the three
//! prompt-building files in model-gateway. A named prompt item is one module-level const or static
//! that `NAMED_PROMPT_ITEMS` lists, in a file that also holds consts which are not prompt text. A
//! tool item is a literal `ToolDefinition` block of the Do offer lists in execution-core or of
//! `builtin_tool_defs` in model-gateway, and a tool builder is a function that builds one definition
//! in code and is offered to a run. Each digest is taken over the item's source text with CRLF
//! folded to LF, so a Windows checkout and a Linux runner agree.
//!
//! The text is read from source, as `tool_catalogue_characterisation.rs` reads the Do offer
//! lists, because the builders live in crates that depend on this one. A builder that is renamed
//! or moved fails here with "re-point this test", and a tool block the reader cannot parse fails
//! here rather than being skipped.
//!
//! Not covered: the MCP and client tools (dynamic per run), the server-resolved `tickets.create`
//! (governed by the action-registry golden), and the offer gating in model-gateway `sse.rs`, which
//! decides what is offered rather than what a tool says. A constant that a pinned builder refers to
//! by name (a tool name, a schema or a bound) is not hashed into the builder's entry, so a change to
//! that constant alone moves no entry unless it is also pinned as a named prompt item.
//!
//! A reviewed change is accepted by regenerating the fixture and its pin, then committing them
//! together with an eval case or replay cassette, or a reviewed exception for the new digest
//! (`scripts/gates/scenario-touch.py`):
//! `MP_UPDATE_PROMPT_MANIFEST=1 cargo test -q -p mp-contracts --test prompt_tool_manifest`

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const FIXTURE: &str = "tests/fixtures/prompt-tool-manifest.json";
const PIN: &str = "tests/fixtures/prompt-tool-manifest.json.sha256";
const SCHEMA: &str = "prompt-tool-manifest-v1";
const UPDATE_ENV: &str = "MP_UPDATE_PROMPT_MANIFEST";
/// Source paths below are relative to the Rust workspace, two levels above this crate.
const RUST_ROOT: &str = "../..";

const FORMAT_GUIDANCE: &str = "services/model-gateway/src/format_guidance.rs";
const AUTHOR_GUIDANCE: &str = "services/model-gateway/src/author_guidance.rs";
const PROMPT_CONTEXT: &str = "services/model-gateway/src/prompt_context.rs";
const TOOL_LOOP: &str = "services/model-gateway/src/tool_loop.rs";
const RECALL_CONVERSATIONS: &str = "services/model-gateway/src/recall_conversations.rs";
const THREAD_REFERENCE_TOOLS: &str = "services/model-gateway/src/thread_reference_tools.rs";
const DECK_TOOL: &str = "services/model-gateway/src/deck_tool.rs";
const AGENT: &str = "services/execution-core/src/runtime_loop/agent.rs";
const WEB_TOOLS: &str = "services/execution-core/src/web_tools.rs";
const WORKSPACE_TOOLS: &str = "services/execution-core/src/workspace_tools.rs";
const ASSISTANT_NAVIGATION: &str = "services/model-gateway/src/assistant_navigation.rs";
const AGENT_API_RUN: &str = "services/model-gateway/src/agent_api_run.rs";
const COMPACTION: &str = "services/model-gateway/src/compaction.rs";
const CONNECTOR_READS: &str = "services/model-gateway/src/connector_reads.rs";
const CONNECTOR_SUGGESTION: &str = "services/model-gateway/src/connector_suggestion.rs";
const CONTEXT_REATTACH: &str = "services/model-gateway/src/context_reattach.rs";
const COUNCIL_ROUTES: &str = "services/model-gateway/src/council_routes.rs";
const HTTP_ROUTES: &str = "services/model-gateway/src/http_routes.rs";
const IMAGE_TOOL: &str = "services/model-gateway/src/image_tool.rs";
const REASONING_TRANSLATE_ROUTES: &str = "services/model-gateway/src/reasoning_translate_routes.rs";
const RETRIEVAL: &str = "services/model-gateway/src/retrieval.rs";
const REVIEW_RUN: &str = "services/model-gateway/src/review_run.rs";
const SIDE_CHAT_ROUTES: &str = "services/model-gateway/src/side_chat_routes.rs";
const SOURCE_VALIDATION: &str = "services/model-gateway/src/source_validation.rs";

/// Every item before the first `#[cfg(test)]` of these files is pinned.
const PROMPT_SOURCES: [&str; 3] = [FORMAT_GUIDANCE, AUTHOR_GUIDANCE, PROMPT_CONTEXT];

/// Named prompt items, for a file that also holds consts which are not prompt text. Each entry is a
/// source and the module-level const or static names to pin from it. Only text put into a model
/// request is listed (a system or user message, a tool result or a tool description), each checked at
/// its call site. A file listed here is not added to `PROMPT_SOURCES`.
const NAMED_PROMPT_ITEMS: &[(&str, &[&str])] = &[
    (
        SOURCE_VALIDATION,
        &[
            "CONVERSATION_SCOPE_NOTICE",
            "KNOWLEDGE_ONLY_SCOPE_NOTICE",
            "WEB_ONLY_SCOPE_NOTICE",
            "REVIEW_INSTRUCTION",
            "REQUIREMENTS_INSTRUCTION",
        ],
    ),
    (
        RETRIEVAL,
        &[
            "NO_GROUNDING_SYSTEM_NOTICE",
            "LOW_CONFIDENCE_GROUNDING_NOTICE",
        ],
    ),
    (
        COMPACTION,
        &[
            "DROPPED_HISTORY_NOTICE",
            "CLEARED_TOOL_RESULT_NOTICE",
            "RUN_HISTORY_DROPPED_NOTICE",
            "PINNED_MESSAGE_PREAMBLE",
            "PINNED_TRUNCATION_NOTICE",
        ],
    ),
    (
        CONTEXT_REATTACH,
        &["REATTACH_TRUNCATED_NOTICE", "REATTACH_NO_MATCH_NOTICE"],
    ),
    (
        COUNCIL_ROUTES,
        &["MEMBER_SYSTEM_PROMPT", "SYNTHESIS_SYSTEM_PROMPT"],
    ),
    (SIDE_CHAT_ROUTES, &["SYSTEM_PROMPT"]),
    (REVIEW_RUN, &["SYSTEM_PROMPT"]),
    (REASONING_TRANSLATE_ROUTES, &["SYSTEM_PROMPT"]),
    (ASSISTANT_NAVIGATION, &["NAVIGATION_INSTRUCTIONS"]),
    (AGENT_API_RUN, &["ASK_SYSTEM_PROMPT"]),
    (
        HTTP_ROUTES,
        &[
            "BROWSER_SUGGEST_SYSTEM_PROMPT",
            "RECOMMEND_PLAN_SYSTEM_PROMPT",
            "DICTATE_SYSTEM_PROMPT",
        ],
    ),
];

/// A function whose literal `ToolDefinition` blocks are offered to a run.
struct ToolList {
    source: &'static str,
    signature: &'static str,
}

/// The Do offer lists, the web tool lists and the gateway's built-in list. Their union is what is
/// offered.
const TOOL_LISTS: [ToolList; 7] = [
    ToolList {
        source: AGENT,
        signature: "fn process_tool_defs()",
    },
    ToolList {
        source: AGENT,
        signature: "fn offered_tool_defs()",
    },
    ToolList {
        source: AGENT,
        signature: "fn browser_tool_defs()",
    },
    // The chat loop's definitions live in `builtin_tool_defs_with` (WS16.S8 made `builtin_tool_defs` a
    // one-line wrapper), so the extraction tool it adds behind its switch is pinned with the rest.
    ToolList {
        source: TOOL_LOOP,
        signature: "pub fn builtin_tool_defs_with(",
    },
    // The web crawl and batch tools, offered by `runtime_loop::agent` while the crawl switch is on.
    ToolList {
        source: WEB_TOOLS,
        signature: "pub fn crawl_tool_defs(",
    },
    // The page reader pair, offered by `runtime_loop::agent` while the reader switch is on.
    ToolList {
        source: WEB_TOOLS,
        signature: "fn reader_definitions(",
    },
    // The chat page reader pair, offered by `tool_loop::tools_for_web_reader` beside `fetch_url`
    // while the reader switch is on. Pinned as the execution-core pair is: an edit to either
    // description moves the manifest.
    ToolList {
        source: TOOL_LOOP,
        signature: "fn web_reader_tool_definitions(",
    },
];

/// Functions that build one tool definition in code and are offered to a run. A list that calls
/// one by a bare call must name it here, or the test fails. A function that rewrites offered
/// definitions behind a switch is pinned here too, as its whole text.
const TOOL_BUILDERS: [(&str, &str); 14] = [
    (AGENT, "fn browser_act_tool_def()"),
    (RECALL_CONVERSATIONS, "pub fn tool_definition()"),
    // EP-3 S7: the read-only thread reference tools.
    (THREAD_REFERENCE_TOOLS, "pub fn tool_definitions("),
    // EP-5 S2: the typed deck tool.
    (DECK_TOOL, "fn tool_definition()"),
    // EP-2 S3: batched web_search rewrites the web_search and fetch_url text and schemas.
    (TOOL_LOOP, "fn advertise_batched_search("),
    // The workspace tools: `tool_defs` picks the reads and, under `all`, the writes, which hold the
    // literal definitions.
    (WORKSPACE_TOOLS, "pub fn tool_defs("),
    (WORKSPACE_TOOLS, "fn read_defs()"),
    (WORKSPACE_TOOLS, "fn write_defs()"),
    // The image tool, offered by `image_tool::tools_for_turn` while the switch is on.
    (IMAGE_TOOL, "pub fn tool_definition()"),
    // The connector suggestion, added by `connector_suggestion::tools_for_turn`.
    (CONNECTOR_SUGGESTION, "pub fn tool_definition()"),
    // The connector read and the custom REST read, offered by `tool_loop`.
    (CONNECTOR_READS, "pub fn tool_definition()"),
    (CONNECTOR_READS, "pub fn custom_rest_tool_definition()"),
    // The source review and the segment repair each offer one tool to their model call.
    (SOURCE_VALIDATION, "fn review_tool("),
    (SOURCE_VALIDATION, "fn repair_tool("),
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    id: String,
    source: String,
    sha256: String,
}

impl Entry {
    fn new(kind: &str, source: &str, name: &str, text: &str) -> Self {
        Self {
            id: format!("{kind}/{}/{name}", file_name(source)),
            source: source.to_owned(),
            sha256: digest(text),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Fn,
    Value,
}

fn manifest_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn read_source(source: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(RUST_ROOT)
        .join(source);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do not delete it.",
            path.display()
        )
    });
    normalise(&text)
}

fn read_fixture(relative: &str) -> String {
    let path = manifest_path(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()));
    normalise(&text)
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn file_name(source: &str) -> &str {
    source.rsplit('/').next().unwrap_or(source)
}

fn identifier(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    (end > 0).then(|| &text[..end])
}

/// The keyword and name of a const, static or fn item that starts on this line.
fn item_start(line: &str) -> Option<(Item, &str)> {
    let mut rest = line.trim_start();
    if let Some(after) = rest.strip_prefix("pub") {
        rest = match after.strip_prefix('(') {
            Some(scoped) => scoped.split_once(')')?.1,
            None => after,
        };
        rest = rest.strip_prefix(' ')?;
    }
    for keyword in ["const fn ", "async fn ", "unsafe fn ", "fn "] {
        if let Some(after) = rest.strip_prefix(keyword) {
            return identifier(after).map(|name| (Item::Fn, name));
        }
    }
    for keyword in ["const ", "static "] {
        if let Some(after) = rest.strip_prefix(keyword) {
            return identifier(after).map(|name| (Item::Value, name));
        }
    }
    None
}

fn leading_spaces(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// The last line of the item that starts on `lines[start]`. A value ends at its first `;`; a
/// function ends at the first closing brace at its own indentation after its opening brace.
fn item_end(lines: &[&str], start: usize, kind: Item, name: &str) -> usize {
    let first = lines[start].trim_end();
    let found = match kind {
        Item::Value => lines[start..]
            .iter()
            .position(|line| line.trim_end().ends_with(';'))
            .map(|offset| start + offset),
        Item::Fn if first.ends_with('}') && first.contains('{') => return start,
        Item::Fn => lines[start..]
            .iter()
            .position(|line| line.trim_end().ends_with('{'))
            .and_then(|offset| {
                let closing = format!("{}}}", leading_spaces(lines[start]));
                let body = start + offset + 1;
                lines[body..]
                    .iter()
                    .position(|line| *line == closing)
                    .map(|close| body + close)
            }),
    };
    found.unwrap_or_else(|| panic!("`{name}` has no end in this file; re-point this test"))
}

/// The prompt items of one file: every const, static and fn at any depth before its first
/// `#[cfg(test)]`. Items nest (a fn can hold a `static`), so the scan does not skip past one.
fn prompt_entries(source: &str, text: &str) -> Vec<Entry> {
    let text = normalise(text);
    let all: Vec<&str> = text.lines().collect();
    let production = all
        .iter()
        .position(|line| line.trim() == "#[cfg(test)]")
        .unwrap_or(all.len());
    let lines = &all[..production];
    let mut entries = Vec::new();
    for start in 0..lines.len() {
        if let Some((kind, name)) = item_start(lines[start]) {
            let end = item_end(lines, start, kind, name);
            entries.push(Entry::new(
                "prompt",
                source,
                name,
                &lines[start..=end].join("\n"),
            ));
        }
    }
    entries
}

/// The named prompt items of one file: each listed const or static that sits at module level, which
/// is a line with no indentation. An item in a test module is indented, so it never matches, and a
/// test-only fn does not end the scan. A listed name that is not found panics: a rename must fail
/// here rather than drop the item.
fn named_prompt_entries(source: &str, text: &str, names: &[&str]) -> Vec<Entry> {
    let text = normalise(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut entries = Vec::new();
    for name in names {
        let starts: Vec<usize> = (0..lines.len())
            .filter(|&start| {
                leading_spaces(lines[start]).is_empty()
                    && item_start(lines[start]).is_some_and(|(_, found)| found == *name)
            })
            .collect();
        assert!(
            !starts.is_empty(),
            "`{name}` is not a module-level item of {source}; re-point this test"
        );
        for start in starts {
            let kind = item_start(lines[start]).map_or(Item::Value, |(kind, _)| kind);
            let end = item_end(&lines, start, kind, name);
            entries.push(Entry::new(
                "prompt",
                source,
                name,
                &lines[start..=end].join("\n"),
            ));
        }
    }
    entries
}

fn signature_line(lines: &[&str], signature: &str, source: &str) -> usize {
    lines
        .iter()
        .position(|line| line.trim_start().starts_with(signature))
        .unwrap_or_else(|| panic!("`{signature}` not found in {source}; re-point this test"))
}

/// The function name in a signature such as `pub fn tool_definition()`.
fn signature_name(signature: &str) -> &str {
    signature
        .split_once("fn ")
        .and_then(|(_, rest)| identifier(rest))
        .unwrap_or_else(|| panic!("`{signature}` is not a function signature"))
}

/// The callee of a bare `path::name(),` item, which a list uses to include a builder.
fn bare_call(trimmed: &str) -> Option<&str> {
    let callee = trimmed.strip_suffix("(),")?;
    if callee.is_empty()
        || !callee
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == ':')
    {
        return None;
    }
    callee.rsplit("::").next()
}

/// The tool name of a `ToolDefinition` block: a string literal, or a constant kept as its path
/// so the id stays stable without resolving the constant from another module.
fn block_name(source: &str, line: usize, block: &[&str]) -> String {
    let value = block
        .iter()
        .find_map(|candidate| candidate.trim_start().strip_prefix("name:"))
        .unwrap_or_else(|| {
            panic!("the ToolDefinition at {source}:{line} has no name field; re-point this test")
        })
        .trim()
        .trim_end_matches(',')
        .trim_end_matches(".to_owned()");
    match value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(literal) => literal.to_owned(),
        None => value.to_owned(),
    }
}

/// The literal `ToolDefinition` blocks of one offer function, in source order. A bare call to a
/// builder must be listed in `TOOL_BUILDERS`; an unlisted one fails rather than being skipped.
fn tool_list_entries(source: &str, text: &str, signature: &str) -> Vec<Entry> {
    let text = normalise(text);
    let lines: Vec<&str> = text.lines().collect();
    let start = signature_line(&lines, signature, source);
    let close = lines[start + 1..]
        .iter()
        .position(|line| *line == "}")
        .map(|offset| start + 1 + offset)
        .unwrap_or_else(|| {
            panic!("`{signature}` has no closing brace at column 0 in {source}; re-point this test")
        });
    let builders: BTreeSet<&str> = TOOL_BUILDERS
        .iter()
        .map(|(_, builder)| signature_name(builder))
        .collect();
    let mut entries = Vec::new();
    let mut index = start + 1;
    while index < close {
        let line = lines[index];
        let trimmed = line.trim();
        if let Some(callee) = bare_call(trimmed) {
            assert!(
                builders.contains(callee),
                "unlisted builder call `{callee}()` in {source} `{signature}`; add it to TOOL_BUILDERS"
            );
        } else if trimmed.contains("ToolDefinition {") && !trimmed.starts_with("//") {
            assert!(
                !trimmed.ends_with('}'),
                "the ToolDefinition at {source}:{} is on one line; this manifest reads blocks only",
                index + 1
            );
            let indent = leading_spaces(line);
            let end = lines[index + 1..close]
                .iter()
                .position(|candidate| {
                    candidate
                        .strip_prefix(indent)
                        .is_some_and(|rest| rest.starts_with('}'))
                })
                .map(|offset| index + 1 + offset)
                .unwrap_or_else(|| {
                    panic!(
                        "the ToolDefinition at {source}:{} has no closing brace; re-point this test",
                        index + 1
                    )
                });
            let block = &lines[index..=end];
            let name = block_name(source, index + 1, block);
            entries.push(Entry::new("tool", source, &name, &block.join("\n")));
            // A closing line that also opens the next block (`}, pb::ToolDefinition {`) is read again
            // as that block's start, so the next definition is not skipped.
            index = if lines[end].contains("ToolDefinition {") {
                end
            } else {
                end + 1
            };
            continue;
        }
        index += 1;
    }
    assert!(
        !entries.is_empty(),
        "`{signature}` in {source} holds no ToolDefinition blocks; re-point this test"
    );
    entries
}

/// A function that builds one tool definition in code, pinned as its whole text.
fn tool_builder_entry(source: &str, text: &str, signature: &str) -> Entry {
    let text = normalise(text);
    let lines: Vec<&str> = text.lines().collect();
    let start = signature_line(&lines, signature, source);
    let name = signature_name(signature);
    let end = item_end(&lines, start, Item::Fn, name);
    Entry::new("tool-builder", source, name, &lines[start..=end].join("\n"))
}

/// Gives a repeated id a `#n` suffix in source order, so a duplicate name stays pinned.
fn dedupe_ids(entries: Vec<Entry>) -> Vec<Entry> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    entries
        .into_iter()
        .map(|mut entry| {
            let count = seen.entry(entry.id.clone()).or_default();
            *count += 1;
            if *count > 1 {
                entry.id = format!("{}#{}", entry.id, *count);
            }
            entry
        })
        .collect()
}

/// Every pinned item of the current sources, sorted by id.
fn current_entries() -> Vec<Entry> {
    let mut entries = Vec::new();
    for source in PROMPT_SOURCES {
        entries.extend(prompt_entries(source, &read_source(source)));
    }
    for &(source, names) in NAMED_PROMPT_ITEMS {
        entries.extend(named_prompt_entries(source, &read_source(source), names));
    }
    for list in &TOOL_LISTS {
        entries.extend(tool_list_entries(
            list.source,
            &read_source(list.source),
            list.signature,
        ));
    }
    for (source, signature) in TOOL_BUILDERS {
        entries.push(tool_builder_entry(source, &read_source(source), signature));
    }
    let mut entries = dedupe_ids(entries);
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

fn render(entries: &[Entry]) -> String {
    let items: Vec<Value> = entries
        .iter()
        .map(|entry| json!({"id": entry.id, "source": entry.source, "sha256": entry.sha256}))
        .collect();
    let document = json!({"schema": SCHEMA, "entries": items});
    format!(
        "{}\n",
        serde_json::to_string_pretty(&document).expect("the manifest serialises")
    )
}

fn index_entries(text: &str) -> BTreeMap<String, (String, String)> {
    let document: Value = serde_json::from_str(text).expect("the manifest fixture is JSON");
    let entries = document["entries"]
        .as_array()
        .expect("the manifest fixture has an entries array");
    entries
        .iter()
        .map(|entry| {
            let field = |key: &str| {
                entry[key]
                    .as_str()
                    .expect("each manifest entry has id, source and sha256")
                    .to_owned()
            };
            (field("id"), (field("source"), field("sha256")))
        })
        .collect()
}

/// Names each item that differs between the pinned manifest and the current sources.
fn change_report(pinned: &str, rendered: &str) -> String {
    let before = index_entries(pinned);
    let after = index_entries(rendered);
    let mut lines = Vec::new();
    for (id, (source, sha)) in &after {
        match before.get(id) {
            None => lines.push(format!("added    {id} ({source})")),
            Some((_, old)) if old != sha => lines.push(format!("changed  {id} ({source})")),
            Some(_) => {}
        }
    }
    for (id, (source, _)) in &before {
        if !after.contains_key(id) {
            lines.push(format!("removed  {id} ({source})"));
        }
    }
    format!(
        "the prompt and tool manifest changed:\n  {}\nReview each change. If it is intended, regenerate with\n  {UPDATE_ENV}=1 cargo test -q -p mp-contracts --test prompt_tool_manifest\nand commit the fixture and its pin with an eval case or replay cassette, or a reviewed exception.",
        lines.join("\n  ")
    )
}

#[test]
fn prompt_tool_manifest_matches_golden() {
    let entries = current_entries();
    assert!(
        !entries.is_empty(),
        "parsed no prompt or tool items - this test broke, not the invariant"
    );
    let rendered = render(&entries);

    if std::env::var(UPDATE_ENV).as_deref() == Ok("1") {
        std::fs::write(manifest_path(FIXTURE), &rendered).expect("write the manifest");
        std::fs::write(
            manifest_path(PIN),
            format!("{}  prompt-tool-manifest.json\n", digest(&rendered)),
        )
        .expect("write the pin");
        return;
    }

    let pinned = read_fixture(FIXTURE);
    if rendered != pinned {
        panic!("{}", change_report(&pinned, &rendered));
    }
    let pin = read_fixture(PIN);
    let recorded = pin.split_whitespace().next().unwrap_or_default();
    assert_eq!(
        digest(&pinned),
        recorded,
        "the pin does not match the fixture; regenerate with {UPDATE_ENV}=1 (see the module docs)"
    );
}

#[test]
fn every_listed_source_contributes_items() {
    let entries = current_entries();
    let sources: BTreeSet<&str> = entries.iter().map(|entry| entry.source.as_str()).collect();
    for source in PROMPT_SOURCES
        .into_iter()
        .chain(NAMED_PROMPT_ITEMS.iter().map(|(source, _)| *source))
        .chain(TOOL_LISTS.iter().map(|list| list.source))
        .chain(TOOL_BUILDERS.iter().map(|(source, _)| *source))
    {
        assert!(
            sources.contains(source),
            "{source} contributed no pinned item"
        );
    }
}

#[test]
fn a_one_character_prompt_edit_changes_its_digest() {
    let source = "services/model-gateway/src/guidance.rs";
    let before = r##"pub const GREETING: &str = "Hello there.";"##;
    let after = r##"pub const GREETING: &str = "Hello there!";"##;
    let old = prompt_entries(source, before);
    let new = prompt_entries(source, after);
    assert_eq!(old.len(), 1);
    assert_eq!(new.len(), 1);
    assert_eq!(old[0].id, new[0].id);
    assert_ne!(old[0].sha256, new[0].sha256);
}

#[test]
fn a_tool_description_edit_changes_its_digest() {
    let source = "services/execution-core/src/runtime_loop/agent.rs";
    let sample = |description: &str| {
        format!(
            r##"fn offered_tool_defs() -> Vec<pb::ToolDefinition> {{
    vec![
        pb::ToolDefinition {{
            name: "web_search".to_owned(),
            description: "{description}".to_owned(),
            parameters_json: r#"{{"type":"object"}}"#.to_owned(),
        }},
    ]
}}
"##
        )
    };
    let old = tool_list_entries(source, &sample("Search the web."), "fn offered_tool_defs()");
    let new = tool_list_entries(source, &sample("Search the web!"), "fn offered_tool_defs()");
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].id, "tool/agent.rs/web_search");
    assert_eq!(old[0].id, new[0].id);
    assert_ne!(old[0].sha256, new[0].sha256);
}

#[test]
fn line_endings_do_not_change_a_digest() {
    let source = "services/model-gateway/src/guidance.rs";
    let lf = "pub const A: &str = \"a\";\npub fn b() -> u8 {\n    1\n}\n";
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(prompt_entries(source, lf), prompt_entries(source, &crlf));
}

#[test]
fn items_after_the_test_module_are_not_pinned() {
    let source = "services/model-gateway/src/guidance.rs";
    let text =
        "pub const A: &str = \"a\";\n#[cfg(test)]\nmod tests {\n    const B: &str = \"b\";\n}\n";
    let ids: Vec<String> = prompt_entries(source, text)
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(ids, vec!["prompt/guidance.rs/A".to_owned()]);
}

#[test]
fn repeated_tool_names_are_pinned_under_distinct_ids() {
    let source = "services/model-gateway/src/tool_loop.rs";
    let text = r##"pub fn builtin_tool_defs() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "result_query".to_owned(),
            description: "first".to_owned(),
            parameters_json: r#"{}"#.to_owned(),
        },
        ToolDefinition {
            name: "result_query".to_owned(),
            description: "second".to_owned(),
            parameters_json: r#"{}"#.to_owned(),
        },
    ]
}
"##;
    let ids: Vec<String> = dedupe_ids(tool_list_entries(
        source,
        text,
        "pub fn builtin_tool_defs()",
    ))
    .into_iter()
    .map(|entry| entry.id)
    .collect();
    assert_eq!(
        ids,
        vec![
            "tool/tool_loop.rs/result_query".to_owned(),
            "tool/tool_loop.rs/result_query#2".to_owned(),
        ]
    );
}

#[test]
#[should_panic(expected = "unlisted builder call `mystery_tool_def()`")]
fn an_unlisted_builder_call_fails_loudly() {
    let text = "fn offered_tool_defs() -> Vec<pb::ToolDefinition> {\n    vec![\n        mystery_tool_def(),\n    ]\n}\n";
    let _ = tool_list_entries(AGENT, text, "fn offered_tool_defs()");
}

#[test]
#[should_panic(expected = "has no name field")]
fn a_tool_block_without_a_name_fails_loudly() {
    let text = "fn offered_tool_defs() -> Vec<pb::ToolDefinition> {\n    vec![\n        pb::ToolDefinition {\n            description: \"x\".to_owned(),\n        },\n    ]\n}\n";
    let _ = tool_list_entries(AGENT, text, "fn offered_tool_defs()");
}

#[test]
fn named_prompt_items_exist_once() {
    for &(source, names) in NAMED_PROMPT_ITEMS {
        let text = read_source(source);
        for &name in names {
            let found = named_prompt_entries(source, &text, &[name]);
            assert_eq!(
                found.len(),
                1,
                "`{name}` must name exactly one module-level item in {source}"
            );
        }
    }
}

#[test]
#[should_panic(expected = "is not a module-level item")]
fn a_renamed_named_item_fails_loudly() {
    let text = "pub const OTHER: &str = \"other\";\n";
    let _ = named_prompt_entries("services/model-gateway/src/guidance.rs", text, &["RENAMED"]);
}

#[test]
fn a_named_item_after_a_test_only_fn_is_pinned_and_one_in_a_test_module_is_not() {
    let source = "services/model-gateway/src/guidance.rs";
    let text = "#[cfg(test)]\nfn helper() {}\npub const NOTICE: &str = \"n\";\n#[cfg(test)]\nmod tests {\n    const NOTICE: &str = \"test\";\n}\n";
    let entries = named_prompt_entries(source, text, &["NOTICE"]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "prompt/guidance.rs/NOTICE");
    assert_eq!(entries[0].sha256, digest("pub const NOTICE: &str = \"n\";"));
}

#[test]
fn a_closing_line_that_opens_the_next_block_starts_it() {
    let source = "services/execution-core/src/web_tools.rs";
    let text = "pub fn crawl_tool_defs(zdr: bool) -> Vec<pb::ToolDefinition> {\n    let mut defs = vec![pb::ToolDefinition {\n        name: MAP_TOOL.to_owned(),\n    }, pb::ToolDefinition {\n        name: PLAN_TOOL.to_owned(),\n    }];\n    defs\n}\n";
    let ids: Vec<String> = tool_list_entries(source, text, "pub fn crawl_tool_defs(")
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "tool/web_tools.rs/MAP_TOOL".to_owned(),
            "tool/web_tools.rs/PLAN_TOOL".to_owned(),
        ]
    );
}

/// The chat reader pair in model-gateway is listed and pinned block by block, as the execution-core
/// pair is: an edit to either description moves that definition's entry and no other.
#[test]
fn the_chat_reader_pair_is_pinned_block_by_block() {
    let signature = "fn web_reader_tool_definitions(";
    assert!(
        TOOL_LISTS
            .iter()
            .any(|list| list.source == TOOL_LOOP && list.signature == signature),
        "the chat reader pair is not in TOOL_LISTS, so its descriptions are not pinned"
    );
    let text = read_source(TOOL_LOOP);
    let pinned = tool_list_entries(TOOL_LOOP, &text, signature);
    let ids: Vec<&str> = pinned.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "tool/tool_loop.rs/read_page",
            "tool/tool_loop.rs/find_in_page"
        ]
    );
    for (index, name) in ["read_page", "find_in_page"].into_iter().enumerate() {
        let anchor = format!("name: \"{name}\".to_owned(),\n            description: \"");
        assert_eq!(
            text.matches(&anchor).count(),
            1,
            "`{name}` is not one ToolDefinition in {TOOL_LOOP}; re-point this test"
        );
        let edited = text.replacen(&anchor, &format!("{anchor} "), 1);
        let moved = tool_list_entries(TOOL_LOOP, &edited, signature);
        for (position, (before, after)) in pinned.iter().zip(&moved).enumerate() {
            assert_eq!(before.id, after.id);
            assert_eq!(
                before.sha256 != after.sha256,
                position == index,
                "{} must move only when its own description changes",
                before.id
            );
        }
    }
}
