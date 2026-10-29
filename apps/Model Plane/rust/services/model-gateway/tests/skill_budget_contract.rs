//! Pins the two loops' skill-injection budgets together.
//!
//! Both dispatch loops inject operator-authored skill guidance into the prompt,
//! and both must bound it by size as well as count. The loops are separate
//! crates on purpose (`CLAUDE.md`;
//! `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md`) so the budget is
//! duplicated — but if the two numbers drift, chat and deployed agents disagree
//! about how much of the window skills may take, and the same org's skills
//! behave differently depending on which surface ran. That difference would only
//! show up as "the agent answers worse than chat", which nobody traces back to a
//! constant.
//!
//! Reads source text for the same reason `tool_retry_contract.rs` does: neither
//! crate depends on the other, and widening the constants to a shared crate for
//! a test's benefit would couple two build graphs more than the invariant is
//! worth.

use std::path::{Path, PathBuf};

fn services_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("model-gateway's parent is rust/services")
        .to_path_buf()
}

fn read_source(relative: &str) -> String {
    let path = services_dir().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {} ({error}). If the file moved, re-point this test; do \
             not delete it — half the invariant moved with it.",
            path.display()
        )
    })
}

const GATEWAY_SKILLS: &str = "model-gateway/src/skills.rs";
const EXECUTION_SKILLS: &str = "execution-core/src/runtime_loop/skill_budget.rs";

/// A `&str` constant's **logical** value, pulled from source text.
///
/// Deliberately tolerant of formatting. rustfmt puts a long literal on the line
/// *after* the `=` and splits it with `\`-continuations, and the two crates need
/// not wrap at the same column — the names differ in length, so they generally
/// will not. This test asserts the two markers say the same thing, so it has to
/// compare decoded strings; comparing raw source slices fails the moment either
/// side is rewrapped, which is a formatting event and not a drift in the
/// invariant. That exact false failure is why this helper exists in this shape,
/// and it is the second time a source-text parser here has been fooled by
/// `rustfmt` (see `compaction_parity_contract.rs::const_after_from`).
fn str_const(source: &str, name: &str, which: &str) -> String {
    let decl = ["pub const ", "pub(crate) const ", "const "]
        .iter()
        .find_map(|vis| source.find(&format!("{vis}{name}: &str =")))
        .unwrap_or_else(|| panic!("`{name}` not found in {which} — was it renamed?"));

    let after = &source[decl..];
    let open = after
        .find('"')
        .unwrap_or_else(|| panic!("`{name}` in {which} has no string literal"));
    let body = &after[open + 1..];

    // The closing quote is the first UNescaped one.
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
    let raw = &body[..end.unwrap_or_else(|| panic!("`{name}` literal is unterminated in {which}"))];

    // Decode exactly what Rust would: a `\` before a newline swallows the
    // newline and the next line's indentation, and the escapes we actually use.
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
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn usize_const(source: &str, name: &str, which: &str) -> usize {
    for prefix in [
        format!("pub const {name}: usize = "),
        format!("pub(crate) const {name}: usize = "),
    ] {
        if let Some(start) = source.find(&prefix) {
            let rest = &source[start + prefix.len()..];
            let end = rest.find(';').expect("constant has no `;`");
            return rest[..end]
                .trim()
                .replace('_', "")
                .parse()
                .expect("constant is not an integer literal");
        }
    }
    panic!("`{name}` not found in {which} — was it renamed?")
}

#[test]
fn both_loops_allow_skills_the_same_share_of_the_prompt() {
    let gateway = read_source(GATEWAY_SKILLS);
    let execution = read_source(EXECUTION_SKILLS);

    assert_eq!(
        usize_const(&gateway, "SKILL_CONTEXT_BUDGET_CHARS", GATEWAY_SKILLS),
        usize_const(&execution, "SKILL_CONTEXT_BUDGET_CHARS", EXECUTION_SKILLS),
        "the two loops give skills different amounts of the prompt. Change both \
         {GATEWAY_SKILLS} and {EXECUTION_SKILLS}, or neither."
    );
    assert_eq!(
        usize_const(&gateway, "MIN_SKILL_CHARS", GATEWAY_SKILLS),
        usize_const(&execution, "MIN_SKILL_CHARS", EXECUTION_SKILLS),
        "the two loops disagree on when a truncated skill stops being useful, so \
         one would inject a stub the other drops"
    );
}

/// A truncated instruction the model cannot tell is truncated is worse than a
/// dropped one: it acts on half a rule believing it is whole. Both sides must
/// mark it, and with the same words — an operator reading two transcripts should
/// not have to learn two phrasings for the same event.
#[test]
fn both_loops_mark_a_truncated_skill_identically() {
    let gateway = read_source(GATEWAY_SKILLS);
    let execution = read_source(EXECUTION_SKILLS);

    let gateway_marker = str_const(&gateway, "TRUNCATION_MARKER", GATEWAY_SKILLS);
    assert!(
        !gateway_marker.is_empty(),
        "parsed an empty marker — the parser broke, not the invariant"
    );
    assert_eq!(
        gateway_marker,
        str_const(&execution, "TRUNCATION_MARKER", EXECUTION_SKILLS),
        "the two loops announce a truncated skill differently"
    );
}
