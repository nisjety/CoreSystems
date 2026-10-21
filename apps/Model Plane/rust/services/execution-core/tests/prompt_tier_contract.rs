//! Guards the order of the chat prompt's blocks by tier.
//!
//! # Why this test exists
//!
//! A provider caches the longest prefix a prompt repeats. One block that
//! changes every turn, placed ahead of the conversation, makes every byte after
//! it a miss, and nothing fails: the turn answers correctly and costs more.
//! That is how the date notice sat ahead of the whole conversation.
//!
//! `mp_contracts::prompt_tier` names five tiers, Stable < Context <
//! Conversation < Volatile < Current, and the gateway's
//! `PromptContext::render_stable_prefix` puts every slot under one with an
//! `out.add(PromptTier::..., slot)` call. This crate must not depend on the
//! gateway (the two loops deploy separately), so the property is checked on
//! source, the way `compaction_parity_contract.rs` checks the two loops'
//! shared numbers:
//!
//! * every slot of `PromptContext` is added exactly once, so a new slot cannot
//!   ship without a tier;
//! * the tiers of the `add` calls never decrease, so a Volatile block never
//!   precedes the Conversation, and a Stable block never follows a Context one;
//! * the conversation is added under Conversation and the user turn being
//!   answered last, under Current.

use std::path::{Path, PathBuf};

/// Every reader normalises CRLF to LF, as `compaction_parity_contract.rs` does.
fn read_from(root: &Path, relative: &str) -> String {
    let path: PathBuf = root.join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| {
            panic!(
                "cannot read {} ({error}). If it moved, re-point this test; do not \
                 delete it, it is what keeps the prompt's blocks in tier order.",
                path.display()
            )
        })
        .replace("\r\n", "\n")
}

fn services() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("execution-core's parent is rust/services")
        .to_path_buf()
}

fn rust_root() -> PathBuf {
    services()
        .parent()
        .expect("services' parent is rust")
        .to_path_buf()
}

fn renderer() -> String {
    read_from(&services(), "model-gateway/src/prompt_context.rs")
}

/// The text from `signature` to the end of its braces.
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

/// The tier names in declaration order, which is their order: the enum derives
/// `Ord`, so the first variant is the earliest tier.
fn tier_order() -> Vec<String> {
    let source = read_from(&rust_root(), "crates/mp-contracts/src/prompt_tier.rs");
    let body = function_body(&source, "pub enum PromptTier");
    let names: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|line| line.ends_with(',') && !line.starts_with("//"))
        .map(|line| line.trim_end_matches(',').to_owned())
        .collect();
    assert!(
        names.len() >= 5,
        "parsed {names:?} from PromptTier: the enum changed shape, re-point this test"
    );
    names
}

/// The `(tier, slot)` of each `out.add(PromptTier::Tier, slot);` call, in source
/// order. A slot added through a longer expression is not recognised, which is
/// the point: every slot goes through one reviewable call shape.
fn tiered_adds(renderer: &str) -> Vec<(String, String)> {
    let body = function_body(renderer, "fn render_stable_prefix(");
    body.lines()
        .map(str::trim)
        .filter_map(|line| {
            let call = line.strip_prefix("out.add(PromptTier::")?;
            let (tier, rest) = call.split_once(',')?;
            let slot = rest.trim().strip_suffix(");")?;
            Some((tier.trim().to_owned(), slot.trim().to_owned()))
        })
        .collect()
}

/// The fields of `pub struct PromptContext`, which are its slots.
fn slots(renderer: &str) -> Vec<String> {
    let body = function_body(renderer, "pub struct PromptContext");
    body.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("pub ") && line.ends_with(','))
        .map(|line| {
            line.trim_start_matches("pub ")
                .split(':')
                .next()
                .expect("a field has a name")
                .trim()
                .to_owned()
        })
        .collect()
}

#[test]
fn every_prompt_slot_is_added_under_exactly_one_tier() {
    let renderer = renderer();
    let slots = slots(&renderer);
    assert!(
        slots.len() >= 15,
        "parsed {slots:?} from PromptContext: the struct changed shape, re-point this test"
    );
    let adds = tiered_adds(&renderer);

    // `current` is the user turn split off `history`: it is added too, but is
    // not a slot of its own.
    for slot in &slots {
        let count = adds.iter().filter(|(_, added)| added == slot).count();
        assert_eq!(
            count, 1,
            "PromptContext slot `{slot}` is added {count} times in render_stable_prefix; a \
             slot must be added once, under one PromptTier, or it is sent unlabelled \
             (or twice)"
        );
    }
    for (tier, added) in &adds {
        assert!(
            slots.contains(added) || added == "current",
            "render_stable_prefix adds `{added}` under {tier}, which is not a PromptContext slot"
        );
    }
}

#[test]
fn the_tiers_of_the_stable_order_never_decrease() {
    let order = tier_order();
    let adds = tiered_adds(&renderer());
    assert!(
        adds.len() >= 16,
        "parsed {adds:?}: the add calls changed shape, re-point this test"
    );
    let rank = |tier: &str| -> usize {
        order
            .iter()
            .position(|name| name == tier)
            .unwrap_or_else(|| panic!("`{tier}` is not a PromptTier variant ({order:?})"))
    };
    for pair in adds.windows(2) {
        let (earlier, later) = (&pair[0], &pair[1]);
        assert!(
            rank(&earlier.0) <= rank(&later.0),
            "`{}` ({}) is added before `{}` ({}): a block of a later tier ahead of an \
             earlier one changes every byte after it, so the prefix cache misses. In \
             particular a Volatile block must never precede the Conversation",
            earlier.1,
            earlier.0,
            later.1,
            later.0
        );
    }
}

#[test]
fn the_conversation_and_the_current_turn_hold_their_places() {
    let adds = tiered_adds(&renderer());
    assert!(
        adds.contains(&("Conversation".to_owned(), "history".to_owned())),
        "the thread must be added under Conversation"
    );
    assert_eq!(
        adds.last(),
        Some(&("Current".to_owned(), "current".to_owned())),
        "the user turn being answered must come last, under Current"
    );
    let volatile_at = adds
        .iter()
        .position(|(tier, _)| tier == "Volatile")
        .expect("the per-turn blocks are Volatile");
    let history_at = adds
        .iter()
        .position(|(_, slot)| slot == "history")
        .expect("history is added");
    assert!(
        history_at < volatile_at,
        "a Volatile block precedes the Conversation"
    );
    assert!(
        adds.contains(&("Volatile".to_owned(), "date_notice".to_owned())),
        "the date notice changes daily and belongs in Volatile"
    );
}

/// The parser must be able to see a violation, or the three tests above could
/// pass on a source they read nothing from.
#[test]
fn the_checks_reject_a_misordered_renderer() {
    let misordered = "\
fn render_stable_prefix(self) -> TieredPrompt {
    let mut out = TieredPrompt::default();
    out.add(PromptTier::Stable, conversation_scope);
    out.add(PromptTier::Volatile, date_notice);
    out.add(PromptTier::Conversation, history);
    out
}
";
    let order = tier_order();
    let adds = tiered_adds(misordered);
    assert_eq!(adds.len(), 3, "{adds:?}");
    let rank = |tier: &str| order.iter().position(|name| name == tier).unwrap();
    assert!(
        adds.windows(2)
            .any(|pair| rank(&pair[0].0) > rank(&pair[1].0)),
        "a Volatile block ahead of the Conversation was not detected"
    );
}
