//! E4.2 step 1, evaluator half: check a step's [`StepExpectation`] against the
//! page Quarry just observed.
//!
//! The contract types live in `quarry_core::step_expect`. This module holds the
//! pure evaluator and the switch reader. The wiring is in `observation.rs`
//! (`ObservationContext::pending_expect` in, `ActionOutcome` and
//! `ProofBundle.expectation` out) and, behind the same switch, in the edge.
//!
//! Everything here runs in memory over data the observation already holds. The
//! page text is never stored, logged or put in a verdict: a verdict carries
//! zero-based condition indexes and closed reason tokens only. Under ZDR the
//! evaluation is the same and nothing persists, because a ZDR run writes no
//! step receipts.
//!
//! Flag: `QUARRY_EDGE__STEP_EXPECT`, default off. The edge reads it with
//! [`step_expect_enabled`] and only then hands an expectation to the runner.

use quarry_core::contracts::{BrowserSnapshot, SemanticLocator, SnapshotTarget};
use quarry_core::step_expect::{ExpectCondition, ExpectVerdict, StepExpectation};

use crate::observation::semantic_matches;

/// The flag that turns step expectations on. Same name and parsing as the edge.
pub const FLAG: &str = "QUARRY_EDGE__STEP_EXPECT";

/// Most page text, in bytes, an evaluation reads. Longer text is cut at a
/// character boundary; a `text_absent` over a longer page therefore only
/// speaks for the first 256 KiB.
pub const PAGE_TEXT_MAX_BYTES: usize = 256 * 1024;

/// The verdict reasons this module produces. Closed tokens, never page text.
pub const REASON_NO_SNAPSHOT: &str = "no_snapshot";
/// `text_present` or `text_absent` without `within`, and no page text was read.
pub const REASON_NO_PAGE_TEXT: &str = "no_page_text";
/// A `within` locator matched no element or more than one.
pub const REASON_WITHIN_UNRESOLVED: &str = "within_unresolved";

/// Whether `QUARRY_EDGE__STEP_EXPECT` is on in this process (`1`, `true`,
/// `yes`, `on`). Read per call; default off.
pub fn step_expect_enabled() -> bool {
    std::env::var(FLAG)
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// What an evaluation may read. All borrowed, all in memory.
#[derive(Debug, Clone, Copy)]
pub struct ExpectInput<'a> {
    /// The page URL after the step. The fragment is dropped before matching.
    pub url: &'a str,
    /// The snapshot built from the page after the step, when there is one.
    pub snapshot: Option<&'a BrowserSnapshot>,
    /// The visible text of the page, capped at [`PAGE_TEXT_MAX_BYTES`], when
    /// it was read.
    pub page_text: Option<&'a str>,
}

/// Whether evaluating `expect` reads the whole-page text. The runner builds the
/// text only when this is true.
pub fn needs_page_text(expect: &StepExpectation) -> bool {
    expect.all.iter().any(|condition| {
        matches!(
            condition,
            ExpectCondition::TextPresent { within: None, .. }
                | ExpectCondition::TextAbsent { within: None, .. }
        )
    })
}

/// Cuts `text` to at most [`PAGE_TEXT_MAX_BYTES`] at a character boundary.
pub fn cap_page_text(mut text: String) -> String {
    if text.len() > PAGE_TEXT_MAX_BYTES {
        let mut end = PAGE_TEXT_MAX_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// What one condition came to.
enum Check {
    Holds,
    Fails,
    Unevaluable(&'static str),
}

/// Checks every condition. A condition that fails settles the verdict as
/// `NotMet` even when another cannot be checked, since the expectation is an
/// AND and cannot hold. Otherwise the first condition that cannot be checked
/// makes it `Unevaluable`; otherwise it is `Met`.
pub fn evaluate(expect: &StepExpectation, input: &ExpectInput) -> ExpectVerdict {
    let mut unmet: Vec<u32> = Vec::new();
    let mut unevaluable: Option<&'static str> = None;
    for (index, condition) in expect.all.iter().enumerate() {
        match check(condition, input) {
            Check::Holds => {}
            Check::Fails => unmet.push(index as u32),
            Check::Unevaluable(reason) => {
                unevaluable.get_or_insert(reason);
            }
        }
    }
    if !unmet.is_empty() {
        ExpectVerdict::NotMet { unmet }
    } else if let Some(reason) = unevaluable {
        ExpectVerdict::Unevaluable {
            reason: reason.to_owned(),
        }
    } else {
        ExpectVerdict::Met
    }
}

fn check(condition: &ExpectCondition, input: &ExpectInput) -> Check {
    match condition {
        ExpectCondition::UrlMatches { pattern } => {
            let url = input.url.split('#').next().unwrap_or_default();
            holds(glob_matches(pattern, url))
        }
        ExpectCondition::ElementPresent { locator } => match input.snapshot {
            Some(snapshot) => holds(count_matches(snapshot, locator) > 0),
            None => Check::Unevaluable(REASON_NO_SNAPSHOT),
        },
        ExpectCondition::ElementAbsent { locator } => match input.snapshot {
            Some(snapshot) => holds(count_matches(snapshot, locator) == 0),
            None => Check::Unevaluable(REASON_NO_SNAPSHOT),
        },
        ExpectCondition::TextPresent { text, within } => {
            match text_found(text, within.as_ref(), input) {
                Ok(found) => holds(found),
                Err(reason) => Check::Unevaluable(reason),
            }
        }
        ExpectCondition::TextAbsent { text, within } => {
            match text_found(text, within.as_ref(), input) {
                Ok(found) => holds(!found),
                Err(reason) => Check::Unevaluable(reason),
            }
        }
    }
}

fn holds(value: bool) -> Check {
    if value {
        Check::Holds
    } else {
        Check::Fails
    }
}

fn count_matches(snapshot: &BrowserSnapshot, locator: &SemanticLocator) -> usize {
    snapshot
        .targets
        .iter()
        .filter(|target| semantic_matches(target, locator))
        .count()
}

/// Whether `text` is in the page text or in the one element `within` names,
/// compared without regard to case. `Err` is a closed reason token.
fn text_found(
    text: &str,
    within: Option<&SemanticLocator>,
    input: &ExpectInput,
) -> Result<bool, &'static str> {
    let needle = text.to_lowercase();
    match within {
        None => {
            let page_text = input.page_text.ok_or(REASON_NO_PAGE_TEXT)?;
            Ok(page_text.to_lowercase().contains(&needle))
        }
        Some(locator) => {
            let snapshot = input.snapshot.ok_or(REASON_NO_SNAPSHOT)?;
            let mut matches = snapshot
                .targets
                .iter()
                .filter(|target| semantic_matches(target, locator));
            let (Some(target), None) = (matches.next(), matches.next()) else {
                return Err(REASON_WITHIN_UNRESOLVED);
            };
            Ok(target_text(target).to_lowercase().contains(&needle))
        }
    }
}

/// The name and the text of one target, joined by a space.
fn target_text(target: &SnapshotTarget) -> String {
    let name = target.name.as_deref().unwrap_or_default();
    let text = target.text.as_deref().unwrap_or_default();
    format!("{name} {text}")
}

/// Glob match where `*` stands for any run of bytes (including none) and every
/// other byte is literal and case-sensitive. Iterative two-pointer, so a hostile
/// pattern cannot recurse or blow up: it is `O(pattern * text)` at worst.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0usize, 0usize);
    // The last `*` seen in the pattern and the text position it last resumed at.
    let mut star: Option<usize> = None;
    let mut resume = 0usize;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            resume = t;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some(star_at) = star {
            p = star_at + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}
