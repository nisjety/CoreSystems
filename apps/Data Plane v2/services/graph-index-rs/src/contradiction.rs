//! Claim contradiction detection (plan P1-4).
//!
//! # Why this exists
//!
//! `graph_claims` has carried `contradicted_by_claim_ids` and `claim_status`
//! since the initial schema, and a **complete read path** was built on top of
//! them: `store::get_contradictions`, `GET /v1/graph/contradictions`, the gRPC
//! `GetContradictions` RPC, `retrieval-engine/search/contradictions.rs`, and
//! Model Plane `session-core::fetch_graph_segments` (which surfaces flagged
//! claims to the LLM as context).
//!
//! Nothing ever wrote them. `persist_extraction` hardcoded
//! `claim_status = 'active'` and omitted the array entirely, so every consumer
//! above queried `jsonb_array_length(...) > 0` against a column that was always
//! empty and got nothing, forever. This module is the missing writer.
//!
//! # Precision over recall, deliberately
//!
//! The detector here is **structural and high-precision**: it only reports a
//! contradiction when two claims are textually near-identical apart from a
//! negation, or apart from their numbers. It will miss paraphrased
//! contradictions ("the plant is certified" vs "certification lapsed in
//! March") — that needs semantic adjudication.
//!
//! That trade is intentional for a citations product. A false contradiction
//! flag is worse than a missed one: it is surfaced to the model as evidence
//! that the corpus disagrees with itself, and it invites a human to
//! adjudicate something that was never in conflict. Low recall is visibly
//! incomplete; low precision is quietly corrosive.
//!
//! # What is NOT done here
//!
//! No auto-supersede. The reference implementation this borrows from
//! (`alash3al/stash`, Apache-2.0, `internal/brain/contradiction.go`) demotes an
//! older fact automatically when an LLM classifies the pair as a *replacement*
//! with confidence ≥ 0.9. Without a confidence signal there is no safe
//! threshold, so both claims stay live and are merely flagged. Adding an
//! [`ClaimAdjudicator`] backed by Model Plane inference is the next increment
//! and slots in behind the trait without touching callers.

/// Outcome of comparing two claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The claims assert incompatible things about the same subject.
    Contradiction,
    /// No detectable conflict. Includes "cannot tell" — this detector is
    /// deliberately silent rather than speculative.
    Compatible,
}

/// Pluggable comparison so a semantic adjudicator (Model Plane inference) can
/// replace or follow the structural pass without changing the caller.
pub trait ClaimAdjudicator {
    fn adjudicate(&self, a: &str, b: &str) -> Verdict;
    /// Recorded on the finding so a reviewer can tell how it was reached.
    fn method(&self) -> &'static str;
}

/// A detected conflict between two claims. Symmetric: neither side is "the
/// newer one", because extraction order is not evidence of recency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedPair {
    pub claim_a: String,
    pub claim_b: String,
    pub method: &'static str,
}

/// Negation markers in both corpus languages (measured 2026-08-05: ~60% English
/// / ~38% Norwegian, mixed within single chunks, so both always apply).
const NEGATIONS: [&str; 12] = [
    // Norwegian
    "ikke", "ingen", "aldri", "uten", "verken", // English
    "not", "no", "never", "without", "neither", "nor", "none",
];

/// Contractions expanded before tokenising so `isn't` and `is not` compare equal.
const CONTRACTIONS: [(&str, &str); 10] = [
    ("isn't", "is not"),
    ("aren't", "are not"),
    ("wasn't", "was not"),
    ("weren't", "were not"),
    ("doesn't", "does not"),
    ("don't", "do not"),
    ("didn't", "did not"),
    ("cannot", "can not"),
    ("can't", "can not"),
    ("won't", "will not"),
];

fn expand_contractions(text: &str) -> String {
    let mut out = text.to_lowercase();
    for (from, to) in CONTRACTIONS {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

/// Content tokens: lowercase alphanumeric words, punctuation dropped.
fn tokenize(text: &str) -> Vec<String> {
    expand_contractions(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn strip_negations(tokens: &[String]) -> (Vec<String>, usize) {
    let mut kept = Vec::with_capacity(tokens.len());
    let mut removed = 0;
    for t in tokens {
        if NEGATIONS.contains(&t.as_str()) {
            removed += 1;
        } else {
            kept.push(t.clone());
        }
    }
    (kept, removed)
}

/// Replaces every numeric run with a placeholder, returning the skeleton and the
/// numbers in order. `"below 4 c"` → (`"below # c"`, `["4"]`).
fn numeric_skeleton(tokens: &[String]) -> (Vec<String>, Vec<String>) {
    let mut skeleton = Vec::with_capacity(tokens.len());
    let mut numbers = Vec::new();
    for t in tokens {
        if t.chars().all(|c| c.is_ascii_digit()) {
            skeleton.push("#".to_string());
            numbers.push(t.clone());
        } else {
            skeleton.push(t.clone());
        }
    }
    (skeleton, numbers)
}

/// Minimum shared tokens before a skeleton match is trusted. Two three-token
/// claims matching on a skeleton is coincidence; eight is a statement.
const MIN_SKELETON_TOKENS: usize = 5;

/// The structural detector: negation asymmetry, then numeric divergence.
pub struct StructuralAdjudicator;

impl ClaimAdjudicator for StructuralAdjudicator {
    fn adjudicate(&self, a: &str, b: &str) -> Verdict {
        let (ta, tb) = (tokenize(a), tokenize(b));
        if ta.is_empty() || tb.is_empty() {
            return Verdict::Compatible;
        }
        // Identical text is not a contradiction — and after P1-2 identical
        // claim text is the *same* claim, so this pair should not exist at all.
        if ta == tb {
            return Verdict::Compatible;
        }

        // Rule 1 — negation asymmetry. Same assertion, opposite polarity:
        // "anlegget er godkjent" vs "anlegget er ikke godkjent".
        let (bare_a, neg_a) = strip_negations(&ta);
        let (bare_b, neg_b) = strip_negations(&tb);
        if bare_a == bare_b
            && bare_a.len() >= MIN_SKELETON_TOKENS.min(bare_a.len().max(1))
            && (neg_a % 2) != (neg_b % 2)
        {
            // Parity, not presence: two negations cancel ("not uncertified").
            return Verdict::Contradiction;
        }

        // Rule 2 — numeric divergence. Same sentence, different figures:
        // "must be stored below 4 degrees" vs "... below 6 degrees".
        let (skel_a, nums_a) = numeric_skeleton(&bare_a);
        let (skel_b, nums_b) = numeric_skeleton(&bare_b);
        if skel_a == skel_b
            && skel_a.len() >= MIN_SKELETON_TOKENS
            && !nums_a.is_empty()
            && nums_a != nums_b
            && (neg_a % 2) == (neg_b % 2)
        {
            return Verdict::Contradiction;
        }

        Verdict::Compatible
    }

    fn method(&self) -> &'static str {
        "structural"
    }
}

/// Compares one new claim against candidate claims that share an entity with
/// it, returning the conflicts found.
///
/// Candidate selection is the caller's job (see
/// `GraphStore::detect_claim_contradictions`) and is what keeps this off an
/// O(n²) all-pairs scan: only claims sharing at least one entity are ever
/// compared, mirroring the reference implementation's `(entity, property)` key.
pub fn detect_against_candidates<A: ClaimAdjudicator>(
    adjudicator: &A,
    claim_id: &str,
    claim_text: &str,
    candidates: &[(String, String)],
) -> Vec<DetectedPair> {
    let mut found = Vec::new();
    for (cand_id, cand_text) in candidates {
        if cand_id == claim_id {
            continue;
        }
        if adjudicator.adjudicate(claim_text, cand_text) == Verdict::Contradiction {
            found.push(DetectedPair {
                claim_a: claim_id.to_string(),
                claim_b: cand_id.clone(),
                method: adjudicator.method(),
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &str, b: &str) -> Verdict {
        StructuralAdjudicator.adjudicate(a, b)
    }

    #[test]
    fn detects_english_negation_asymmetry() {
        assert_eq!(
            v(
                "The processing plant is certified for export",
                "The processing plant is not certified for export"
            ),
            Verdict::Contradiction
        );
    }

    #[test]
    fn detects_norwegian_negation_asymmetry() {
        assert_eq!(
            v(
                "Anlegget er godkjent for eksport til EU",
                "Anlegget er ikke godkjent for eksport til EU"
            ),
            Verdict::Contradiction
        );
    }

    #[test]
    fn expands_contractions_so_isnt_equals_is_not() {
        assert_eq!(
            v(
                "The sample is not compliant with the standard",
                "The sample isn't compliant with the standard"
            ),
            Verdict::Compatible,
            "same polarity after expansion must not be flagged"
        );
        assert_eq!(
            v(
                "The sample is compliant with the standard",
                "The sample isn't compliant with the standard"
            ),
            Verdict::Contradiction
        );
    }

    #[test]
    fn double_negation_cancels_by_parity() {
        // Presence-based logic would wrongly flag this; parity does not.
        assert_eq!(
            v(
                "The batch is not without contamination risk here",
                "The batch is with contamination risk here"
            ),
            Verdict::Compatible
        );
    }

    #[test]
    fn detects_numeric_divergence() {
        assert_eq!(
            v(
                "Product must be stored below 4 degrees at all times",
                "Product must be stored below 6 degrees at all times"
            ),
            Verdict::Contradiction
        );
    }

    #[test]
    fn identical_claims_are_not_contradictions() {
        let s = "Aquatiq supplies hygiene systems to the food industry";
        assert_eq!(v(s, s), Verdict::Compatible);
        assert_eq!(
            v(
                s,
                "  AQUATIQ supplies hygiene systems to the food industry "
            ),
            Verdict::Compatible
        );
    }

    #[test]
    fn unrelated_claims_are_not_contradictions() {
        assert_eq!(
            v(
                "Aquatiq supplies hygiene systems to the food industry",
                "The conference takes place in Bergen in September"
            ),
            Verdict::Compatible
        );
    }

    #[test]
    fn short_claims_do_not_trip_the_skeleton_rule() {
        // "price 4" vs "price 6" is too little shared structure to trust.
        assert_eq!(v("price 4", "price 6"), Verdict::Compatible);
    }

    #[test]
    fn paraphrased_contradiction_is_missed_by_design() {
        // Documents the recall limit honestly: this needs semantic adjudication.
        assert_eq!(
            v(
                "The plant holds a valid export certificate",
                "The plant lost its export certification in March"
            ),
            Verdict::Compatible
        );
    }

    #[test]
    fn detect_against_candidates_skips_self_and_reports_pairs() {
        let cands = vec![
            (
                "c1".to_string(),
                "The plant is certified for export today".to_string(),
            ),
            (
                "c2".to_string(),
                "The plant is not certified for export today".to_string(),
            ),
            (
                "c3".to_string(),
                "Bergen hosted the annual seafood conference".to_string(),
            ),
        ];
        let found = detect_against_candidates(
            &StructuralAdjudicator,
            "c1",
            "The plant is certified for export today",
            &cands,
        );
        assert_eq!(found.len(), 1, "only c2 conflicts; self must be skipped");
        assert_eq!(found[0].claim_b, "c2");
        assert_eq!(found[0].method, "structural");
    }
}
