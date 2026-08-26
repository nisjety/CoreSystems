//! Guards that the two loops compact the same way, and recover the same way.
//!
//! # Why this test exists
//!
//! Until 2026-08-24 the governed agent loop had **no compaction at all**, while
//! the inline chat loop had tiers. That asymmetry is invisible from either side:
//! nothing fails, the deployed agent just carries every round's tool payload
//! until a provider rejects the prompt and the run ends on the graceful-failure
//! sentence. Chat degrades; the agent died.
//!
//! Bringing tier 1 to `execution-core::compaction_budget` fixes that and creates
//! a new hazard in its place: **two implementations of one policy.** The two
//! services deploy separately and neither may depend on the other, which is the
//! same situation `tool_retry_contract.rs` and `skill_budget_contract.rs` are in,
//! and the same answer — read the other service's source and assert the numbers
//! and the notice text still match.
//!
//! What must agree:
//!
//! * the payload budget and the spared-tail count, or a run compacts at a
//!   different point depending on which surface drove it;
//! * the prefix each loop uses to RECOGNISE a tool-result block, and the prefix
//!   each loop's formatter actually WRITES — a detector that has drifted from its
//!   formatter silently compacts nothing;
//! * the notice left behind, so one compaction does not read as two different
//!   events;
//! * that both loops offer `reattach_skill`, since they share the skill budget
//!   that truncates the instruction it recovers.

use std::path::{Path, PathBuf};

fn read(relative: &str) -> String {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("execution-core's parent is rust/services")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If it moved, re-point this test; do not \
             delete it — it is the only thing keeping the two loops' compaction \
             from drifting apart.",
            path.display()
        )
    })
}

/// Value of a `usize`/`&str` const or struct field, by name, searching only
/// after `from`.
///
/// The anchor is not optional decoration: a bare name search finds the struct's
/// FIELD DECLARATION (`pub keep_recent: usize,`) before the literal that assigns
/// it, and then walks forward to the next unrelated `=`. That is how the second
/// run of this test read `keep_recent` as 32 000.
fn const_after_from(source: &str, file: &str, from: &str, name: &str) -> String {
    let start = source
        .find(from)
        .unwrap_or_else(|| panic!("`{from}` not found in {file}; re-point this test"));
    let source = &source[start..];
    let at = source
        .find(&format!("{name}:"))
        .unwrap_or_else(|| panic!("`{name}` not found after `{from}` in {file}"));
    // Two forms, because the two loops legitimately spell these differently:
    // a const is `NAME: TYPE = VALUE;` and a struct-literal field is
    // `NAME: VALUE,`. Telling them apart by whether `=` arrives before the
    // terminator is what makes this work on both — a `=`-only reader walks a
    // struct field forward into an unrelated statement, and a terminator-only
    // reader returns a const's TYPE.
    let tail = &source[at + name.len() + 1..];
    let end = tail
        .find([',', ';'])
        .expect("a const or field ends in a comma or semicolon");
    let span = &tail[..end];
    match span.find('=') {
        Some(eq) => span[eq + 1..].trim().to_owned(),
        None => span.trim().to_owned(),
    }
}

/// Extract one function body by brace balance, starting at its signature.
fn function_body(text: &str, signature: &str) -> String {
    let start = text
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is gone; re-point this test"));
    let open = text[start..]
        .find('{')
        .expect("a function signature is followed by a body")
        + start;
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return text[open..=open + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces after `{signature}`");
}

fn digits(value: &str) -> String {
    value.chars().filter(char::is_ascii_digit).collect()
}

#[test]
fn both_loops_compact_at_the_same_threshold() {
    let gateway = read("model-gateway/src/compaction.rs");
    let agent = read("execution-core/src/compaction_budget.rs");

    // The gateway keeps them in a struct literal; this loop keeps them as two
    // consts. Compare the numbers, which is what actually has to match.
    let gateway_budget = digits(&const_after_from(
        &gateway,
        "compaction.rs",
        "DEFAULT_TOOL_PAYLOAD_BUDGET",
        "max_chars",
    ));
    let agent_budget = digits(&const_after_from(
        &agent,
        "compaction_budget.rs",
        "pub(crate) const MAX_TOOL_PAYLOAD_CHARS",
        "MAX_TOOL_PAYLOAD_CHARS",
    ));
    assert!(
        !gateway_budget.is_empty() && !agent_budget.is_empty(),
        "parsed no budget at all — this test broke, not the invariant"
    );
    assert_eq!(
        gateway_budget, agent_budget,
        "the two loops would compact at different points: chat at {gateway_budget} chars, \
         the deployed agent at {agent_budget}"
    );

    let gateway_keep = digits(&const_after_from(
        &gateway,
        "compaction.rs",
        "DEFAULT_TOOL_PAYLOAD_BUDGET",
        "keep_recent",
    ));
    let agent_keep = digits(&const_after_from(
        &agent,
        "compaction_budget.rs",
        "pub(crate) const KEEP_RECENT_PAYLOADS",
        "KEEP_RECENT_PAYLOADS",
    ));
    assert_eq!(
        gateway_keep, agent_keep,
        "the loops would spare a different number of recent payloads — the ones the \
         model is actively reasoning over"
    );
}

/// A detector that has drifted from its formatter compacts NOTHING, silently.
/// Both halves are checked on both sides.
#[test]
fn each_loops_detector_still_matches_what_its_formatter_writes() {
    let gateway = read("model-gateway/src/compaction.rs");
    let agent = read("execution-core/src/compaction_budget.rs");
    let agent_formatter = read("execution-core/src/runtime_loop/agent.rs");

    let gateway_prefix = const_after_from(
        &gateway,
        "compaction.rs",
        "const TOOL_RESULT_PREFIX",
        "TOOL_RESULT_PREFIX",
    );
    let agent_prefix = const_after_from(
        &agent,
        "compaction_budget.rs",
        "const TOOL_RESULT_PREFIX",
        "TOOL_RESULT_PREFIX",
    );
    assert_eq!(
        gateway_prefix, agent_prefix,
        "the two loops recognise a tool-result block by different prefixes"
    );

    // And the prefix is one this loop's formatter actually emits.
    let literal = agent_prefix.trim_matches('"');
    assert!(
        !literal.is_empty(),
        "parsed an empty prefix — this test broke, not the invariant"
    );
    let formatter = agent_formatter
        .split("fn format_tool_context(")
        .nth(1)
        .expect("format_tool_context is gone; re-point this test");
    assert!(
        formatter.contains(literal),
        "`format_tool_context` no longer writes the prefix `{literal}` that compaction \
         looks for, so the agent loop would carry every payload forever while \
         reporting nothing wrong"
    );
}

#[test]
fn both_loops_leave_the_same_notice_behind() {
    let gateway = read("model-gateway/src/compaction.rs");
    let agent = read("execution-core/src/compaction_budget.rs");

    let strip = |text: String| -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && *c != '"' && *c != '\\')
            .collect()
    };
    let gateway_notice = strip(const_after_from(
        &gateway,
        "compaction.rs",
        "pub const CLEARED_TOOL_RESULT_NOTICE",
        "CLEARED_TOOL_RESULT_NOTICE",
    ));
    let agent_notice = strip(const_after_from(
        &agent,
        "compaction_budget.rs",
        "pub(crate) const CLEARED_TOOL_RESULT_NOTICE",
        "CLEARED_TOOL_RESULT_NOTICE",
    ));
    assert!(
        gateway_notice.len() > 40,
        "parsed no notice — this test broke, not the invariant"
    );
    assert_eq!(
        gateway_notice, agent_notice,
        "one compaction would read as two different events depending on which loop ran it"
    );
}

/// The loops share the skill budget that truncates an instruction, so they must
/// share the recovery for it. A deployed agent left reading half a rule that
/// chat could read whole is the asymmetry this whole file is about.
#[test]
fn both_loops_offer_the_skill_recovery_their_shared_budget_requires() {
    let gateway = read("model-gateway/src/tool_loop.rs");
    let agent = read("execution-core/src/runtime_loop/agent.rs");
    for (name, source) in [("model-gateway", &gateway), ("execution-core", &agent)] {
        assert!(
            source.contains(r#"name: "reattach_skill".to_owned(),"#),
            "{name} does not offer `reattach_skill`, but its skill budget truncates \
             instructions and tells the model to call it"
        );
    }

    // And the marker that tells the model to call it must exist on both sides,
    // or one loop advertises a recovery nothing points at.
    let gateway_marker = read("model-gateway/src/skills.rs");
    let agent_marker = read("execution-core/src/runtime_loop/skill_budget.rs");
    for (name, source) in [
        ("model-gateway", &gateway_marker),
        ("execution-core", &agent_marker),
    ] {
        assert!(
            source.contains("reattach_skill"),
            "{name}'s truncation marker does not name the recovery tool, so a cut \
             instruction is a dead end again"
        );
    }
}

/// Pre-dispatch argument validation must run in **both** loops.
///
/// `mp_contracts::tool_arguments` began as `model-gateway/argument_repair.rs` with
/// a doc comment claiming it was wired to a `tool_loop` `mcp_call` arm. That arm
/// does not exist: 393 lines and 12 tests reachable from nothing. Having wired it,
/// the failure mode to guard is the asymmetric one — a validator only chat runs
/// means a deployed agent accepts arguments chat would have refused, and the
/// difference shows up as a tool that works in one surface and not the other.
///
/// Source text, because "is this called before dispatch" is a call-shape property
/// no value can observe.
#[test]
fn both_loops_validate_tool_arguments_before_dispatch() {
    let chat = read("model-gateway/src/tool_loop.rs");
    let agent = read("execution-core/src/runtime_loop/agent.rs");

    for (name, source) in [("model-gateway", &chat), ("execution-core", &agent)] {
        assert!(
            source.contains("mp_contracts::tool_arguments::validate_arguments"),
            "{name} does not validate tool arguments against their declared schema"
        );
        assert!(
            source.contains("mp_contracts::tool_arguments::repair_message"),
            "{name} validates but does not tell the model WHAT to fix, which turns a \
             repairable call into a dead end"
        );
    }

    // Chat validates at the top of `dispatch_tool`, before the match; the agent
    // loop validates in the sequential pre-pass alongside the purpose-lock. Both
    // must precede any dispatch, so a rejected call reaches no executor.
    let dispatch = function_body(&chat, "pub async fn dispatch_tool(");
    let validate_at = dispatch
        .find("builtin_argument_problem")
        .expect("dispatch_tool no longer validates arguments");
    let match_at = dispatch
        .find("match call.name.as_str()")
        .expect("dispatch_tool's arm match is gone; re-point this test");
    assert!(
        validate_at < match_at,
        "validation must run BEFORE the dispatch arms, or a bad call reaches an executor \
         first and the check is decoration"
    );
}

/// Both loops must ground-check supplied arguments, not merely schema-check them,
/// and must do it the same way.
///
/// A schema check passes an invented postal code: the value is well-formed, and
/// that is exactly how one reached a live carrier aggregator. Measured
/// 2026-08-25 — required user-only values were fabricated in 55-70 % of samples,
/// every one schema-valid. If one loop grounds and the other does not, the same
/// call is refused in chat and executed by a deployed agent, which is the worst
/// version of this: the surface with less human attention is the permissive one.
#[test]
fn both_loops_ground_check_arguments_and_refuse_rather_than_re_guess() {
    let chat = read("model-gateway/src/tool_loop.rs");
    let agent = read("execution-core/src/runtime_loop/agent.rs");

    for (name, source) in [("model-gateway", &chat), ("execution-core", &agent)] {
        assert!(
            source.contains("mp_contracts::tool_arguments::ungrounded_arguments"),
            "{name} does not ground-check arguments, so an invented postal code \
             reaches an executor"
        );
        assert!(
            source.contains("mp_contracts::tool_arguments::grounding_message"),
            "{name} detects a fabrication but reuses the schema-repair wording, which \
             invites a second guess instead of a question"
        );
    }

    // The grounding view must exclude the system prompt on both sides. Otherwise
    // a value that appears only in the preamble grounds itself — `from.name:
    // "Verevon"` was a real observed fabrication.
    for (name, source) in [("model-gateway", &chat), ("execution-core", &agent)] {
        assert!(
            source.contains(r#"message.role != "system""#),
            "{name} grounds against the system prompt too, which would ground values \
             the preamble mentions"
        );
    }
}

/// The prompt-side list and the enforcement-side list must name the same tools.
///
/// A tool told "ask, do not guess" but never checked is advice the measurement
/// already showed is ignored 19/20. A tool checked but never told is a refusal
/// the model was given no way to anticipate. Either half alone is worse than
/// neither, because both read as "handled".
#[test]
fn the_prompt_and_the_enforcement_agree_on_which_tools_need_user_supplied_values() {
    let agent = read("execution-core/src/runtime_loop/agent.rs");
    let contracts = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("execution-core -> services -> rust")
            .join("crates/mp-contracts/src/tool_arguments.rs"),
    )
    .expect("cannot read tool_arguments.rs");

    let prompt_list = between(&agent, "const USER_SUPPLIED_ARG_TOOLS: &[&str] = &[", "];");
    let check_list = between(&contracts, "const GROUNDED_ARGUMENT_PATHS", "];\n");

    for tool in ["get_shipping_quotes", "book_shipment"] {
        assert!(
            prompt_list.contains(tool),
            "{tool} is ground-checked but the prompt never warns about it"
        );
        assert!(
            check_list.contains(tool),
            "{tool} is warned about in the prompt but never enforced"
        );
    }
    // And the lists must not have grown apart. Every quoted name in the prompt
    // list has to appear in the check table.
    for name in quoted_names(prompt_list) {
        assert!(
            check_list.contains(&name),
            "`{name}` is in USER_SUPPLIED_ARG_TOOLS but has no entry in \
             GROUNDED_ARGUMENT_PATHS — the prompt asks for a value nothing verifies"
        );
    }

    // The chat loop keeps its own list under the same name, with its own
    // spellings (`shipping_get_quotes` plus the Console's dotted alias). Same
    // rule: every name the prompt warns about must be one the gate verifies.
    let chat = read("model-gateway/src/tool_loop.rs");
    let chat_list = between(&chat, "const USER_SUPPLIED_ARG_TOOLS: &[&str] = &[", "];");
    let chat_names = quoted_names(chat_list);
    assert!(
        !chat_names.is_empty(),
        "the gateway's USER_SUPPLIED_ARG_TOOLS list is gone or renamed; re-point this test"
    );
    for name in chat_names {
        assert!(
            check_list.contains(&name),
            "`{name}` is in the gateway's USER_SUPPLIED_ARG_TOOLS but has no \
             GROUNDED_ARGUMENT_PATHS entry — chat warns about a value nothing verifies"
        );
    }
}

/// Both loops must give the elicitation rule in the SAME WORDS.
///
/// The snippet was measured on the agent loop (+18.3 pp, zero under-calling
/// regression) and then carried to chat verbatim. Two loops describing the same
/// rule differently is how the same org's assistant asks for a postal code in
/// one surface and invents it in the other — and the wording is load-bearing:
/// the "asking for a fact, not asking permission" sentence exists to avoid
/// contradicting the measured anti-permission wording both preambles carry.
/// Compared as DECODED strings, because the two files wrap the literal at
/// different columns and raw source comparison asserts formatting, not words
/// (the rustfmt lesson from `skill_budget_contract.rs`, applied pre-emptively).
#[test]
fn both_loops_state_the_elicitation_rule_in_identical_words() {
    let agent = read("execution-core/src/runtime_loop/agent.rs");
    let chat = read("model-gateway/src/tool_loop.rs");
    let a = decoded_str_const(&agent, "SNIPPET_USER_SUPPLIED_ARGS", "execution-core");
    let c = decoded_str_const(&chat, "SNIPPET_USER_SUPPLIED_ARGS", "model-gateway");
    assert_eq!(
        a, c,
        "the two loops teach different elicitation rules; change both or neither"
    );
    assert!(
        a.contains("never invent a user-only value"),
        "the rule lost its core sentence"
    );
}

/// A `&str` constant's logical value: find the declaration, take the literal,
/// decode `\`-continuations and simple escapes. Formatting-tolerant on purpose.
fn decoded_str_const(source: &str, name: &str, which: &str) -> String {
    let decl = ["pub(crate) const ", "pub const ", "const "]
        .iter()
        .find_map(|vis| source.find(&format!("{vis}{name}: &str =")))
        .unwrap_or_else(|| panic!("`{name}` not found in {which}"));
    let after = &source[decl..];
    let open = after.find('"').expect("no string literal");
    let body = &after[open + 1..];
    let mut end = None;
    let mut escaped = false;
    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                end = Some(index);
                break;
            }
            _ => {}
        }
    }
    let raw = &body[..end.unwrap_or_else(|| panic!("unterminated literal for {name}"))];
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\n') => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            Some('n') => out.push('\n'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Text between two markers, for reading a list out of source.
fn between<'a>(source: &'a str, from: &str, to: &str) -> &'a str {
    let start = source
        .find(from)
        .unwrap_or_else(|| panic!("`{from}` not found; re-point this test"))
        + from.len();
    let rest = &source[start..];
    let end = rest
        .find(to)
        .unwrap_or_else(|| panic!("no `{to}` after `{from}`"));
    &rest[..end]
}

/// Every double-quoted string in a source fragment.
fn quoted_names(fragment: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = fragment;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        match rest.find('"') {
            Some(close) => {
                out.push(rest[..close].to_owned());
                rest = &rest[close + 1..];
            }
            None => break,
        }
    }
    out
}

/// The two loops' trust classifiers must name the same tools.
///
/// `TrustClass::classify` exists in both crates, and both copies carry a comment
/// saying to keep the rule in sync by hand. They had already drifted three ways:
/// the gateway knew `fetch_url` and execution-core did not, execution-core knew
/// `web_fetch` and the gateway's list differed, and **neither** knew `web.search`
/// or `web.read` — the Quarry-backed public-web tools, which are dispatched
/// (`runtime_loop/mod.rs`) and were therefore falling through to `OrgInternal`.
///
/// That fall-through is the whole reason this test exists. `OrgInternal` means
/// `is_external()` is false, which means `framing()` returns `None` — so a page
/// fetched from the open internet reached the model with no "treat this as data,
/// not instructions" framing, and a skipped injection scan was not even flagged
/// for audit. A misclassified tool is not a cosmetic drift; it is a silent hole
/// in injection defense.
#[test]
fn both_trust_classifiers_treat_the_same_tools_as_external_web() {
    let agent = read("execution-core/src/provenance.rs");
    let chat = read("model-gateway/src/moderation.rs");

    let arm_of = |source: &str, which: &str| -> Vec<String> {
        let arm = between(
            source,
            "if tool_name.starts_with(\"mcp__\")",
            "Self::ExternalWeb",
        );
        let names = quoted_names(arm);
        assert!(
            !names.is_empty(),
            "{which}: parsed no tool names before the ExternalWeb arm — the match \
             shape changed, re-point this test rather than deleting it"
        );
        let mut sorted = names;
        sorted.sort();
        sorted
    };

    let agent_names = arm_of(&agent, "execution-core");
    let chat_names = arm_of(&chat, "model-gateway");
    assert_eq!(
        agent_names, chat_names,
        "the two loops disagree about which tools return untrusted external content, \
         so the same page is framed as untrusted on one surface and as org-internal \
         on the other"
    );

    // The dotted Quarry pair is the drift this test was written for. Pin it by
    // name so a future edit cannot quietly drop it from both at once and stay
    // green.
    for required in ["web.search", "web.read"] {
        assert!(
            agent_names.iter().any(|name| name == required),
            "`{required}` fetches public web content and is dispatched, so it must be \
             ExternalWeb — as OrgInternal it gets no untrusted framing at all"
        );
    }
}

/// Every ground-checked tool must actually be *offered* by at least one loop,
/// and every shipping-shaped tool a loop offers must be ground-checked.
///
/// # The failure this exists to catch
///
/// The first version of the grounding table named only `get_shipping_quotes`,
/// which is the **agent** loop's spelling. The chat loop's equivalent is
/// `shipping_get_quotes`, with dimensions nested under `package` instead of at
/// the top level. So chat called `ungrounded_arguments` on every tool call and
/// the check could never fire once — present, parity-tested, and inert.
///
/// The parity test above asserts both loops *call* the checker. That is not the
/// same as either loop being covered by it, and reading the first as the second
/// is how this repo keeps shipping code with no reachable caller. This test
/// closes the gap from the other side: it compares the table against what the
/// catalogues actually advertise.
#[test]
fn every_ground_checked_tool_is_offered_and_every_offered_shipping_tool_is_checked() {
    let contracts = read_contracts("tool_arguments.rs");
    let chat = read("model-gateway/src/tool_loop.rs");
    let agent = read("execution-core/src/runtime_loop/agent.rs");

    // Comments inside the table contain prose in backticks and quotes; stripping
    // them first is required or the name parser reads English as tool names.
    let table_raw = between(&contracts, "const GROUNDED_ARGUMENT_PATHS", "];\n");
    let table: String = table_raw
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Entries are (tool, &[paths]); the tool names are the ones followed by a
    // comma on their own — take every quoted string that has no '.' in it.
    // Entries are `("tool", &[paths])`, so a tool name is the first quoted
    // string after an opening paren. Filtering by shape instead (underscores, no
    // dot) reads `carrier_code` as a tool — argument paths and tool names are not
    // distinguishable that way.
    let checked = tool_names_in_table(&table);
    assert!(
        checked.len() >= 3,
        "parsed {checked:?} from the table — the shape changed, re-point this test"
    );

    for tool in &checked {
        // Reachable = advertised in a catalogue (`name: "…"`), OR accepted by a
        // dispatch match arm (`"…" =>` / `| "…"`). The second route exists
        // because the Console declares `shipping.get_quotes` as a CLIENT tool —
        // no catalogue carries it, yet the model can call it and the executor
        // runs it, which is precisely why it needs a table entry.
        let advertised = chat.contains(&format!("name: \"{tool}\""))
            || agent.contains(&format!("name: \"{tool}\""));
        let dispatched = chat.contains(&format!("\"{tool}\" =>"))
            || chat.contains(&format!("| \"{tool}\""))
            || agent.contains(&format!("\"{tool}\" =>"))
            || agent.contains(&format!("| \"{tool}\""));
        assert!(
            advertised || dispatched,
            "`{tool}` is ground-checked but no loop advertises or dispatches it, so \
             the check can never fire — either the name is wrong (this is exactly how \
             `shipping_get_quotes` was missed) or the entry is dead"
        );
    }

    // And the other direction: a loop must not advertise a shipping tool that
    // nothing ground-checks. Shipping is where the fabrication was measured, so
    // a new one appearing uncovered is the regression to catch.
    for (name, source) in [("model-gateway", &chat), ("execution-core", &agent)] {
        for candidate in tools_requiring_user_held_values(source) {
            assert!(
                checked.contains(&candidate),
                "{name} offers `{candidate}`, whose schema asks for a postal code or a \
                 package dimension, but GROUNDED_ARGUMENT_PATHS has no entry for it — \
                 those values would be invented unchecked"
            );
        }
    }
}

/// Tool names from a `&[(&str, &[&str])]` table: the first quoted string after
/// each opening paren.
fn tool_names_in_table(table: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = table.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != '(' {
            index += 1;
            continue;
        }
        let mut cursor = index + 1;
        while cursor < bytes.len() && bytes[cursor].is_whitespace() {
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == '"' {
            let mut name = String::new();
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor] != '"' {
                name.push(bytes[cursor]);
                cursor += 1;
            }
            if !name.is_empty() {
                out.push(name);
            }
        }
        index = cursor.max(index + 1);
    }
    out
}

/// Tools whose schema asks for a value only the user can hold.
///
/// Keyed on the **schema**, not the name. Names were the wrong criterion twice
/// over: the two loops spell the same capability differently
/// (`get_shipping_quotes` / `shipping_get_quotes`), and matching "shipment"
/// catches `track_shipment`, whose only required field is a tracking number the
/// user states in the request. What actually needs grounding is a postal code or
/// a package dimension — neither is derivable from anything but the conversation.
fn tools_requiring_user_held_values(source: &str) -> Vec<String> {
    const MARKERS: [&str; 2] = ["postal_code", "length_cm"];
    let mut out = Vec::new();
    for fragment in source.split("name: \"").skip(1) {
        let Some(end) = fragment.find('"') else {
            continue;
        };
        let name = &fragment[..end];
        // Only look as far as this definition's own schema, not into the next.
        let scope = match fragment.find("\n        pb::ToolDefinition") {
            Some(at) => &fragment[..at],
            None => &fragment[..fragment.len().min(4000)],
        };
        if MARKERS.iter().any(|marker| scope.contains(marker)) && !out.contains(&name.to_owned()) {
            out.push(name.to_owned());
        }
    }
    out
}

/// Read a file from the shared contracts crate.
fn read_contracts(file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("execution-core -> services -> rust")
        .join("crates/mp-contracts/src")
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {} ({error})", path.display()))
}
