//! Post-stream claim-anchor pass (chat-wave2 citations spec §2.4).
//!
//! Once the answer is complete, locate where each recorded citation's
//! bracketed marker (`[n]` or `[n, m]`) actually landed in the text and
//! attach a `claim_id` (a stable id for the paragraph-ish segment the marker
//! sits in) plus the marker's own byte offsets (UTF-8, matching
//! `regex::Match::start`/`end` — NOT a Unicode codepoint or UTF-16 count, see
//! the field docs on `ChatEvent::Citation::start`/`end`). This never judges
//! whether a
//! citation is correct — it only dates an already-recorded citation to a
//! claim boundary, the same marker the client's own `parseInline` (mirrored
//! here) already renders as a chip.
//!
//! Segment-splitting mirrors `source_validation.rs`'s idea (paragraph/fence-
//! aware blocks), reused as an idea only — that module scores generated
//! document text against conversation attachments, a different job.
//!
//! First-occurrence only: if the model cites the same source at two
//! disjoint claims, only the first occurrence's `claim_id`/offsets are
//! recorded — mirrors the client's own `upsertCitation` first-write-wins
//! dedup (`chat-normalizers.ts`), so a second sighting merges onto the same
//! row rather than creating a second one.
//!
//! Resolution is BY NUMBER (`RecordedCitation.number`), never by position in
//! `recorded` — the same reason `retrieval.rs`'s off-by-gap fix exists: a
//! producer's own `[n]` is the only thing the model actually wrote, and
//! trusting array position again would reintroduce the exact bug the number
//! field was added to avoid.

use std::sync::OnceLock;

use regex::Regex;

use crate::sse_events::{ChatEvent, RecordedCitation};

/// The exact marker grammar the frontend's `parseInline` matches
/// (`chat-media-markdown.tsx:1152`): `[n]` or `[n, m, ...]`. The frontend's
/// pattern also guards against an escaped `\[`, which does not apply here —
/// this pass reads the model's own finished answer, not user-authored
/// Markdown that might escape a literal bracket.
fn marker_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[(\d+(?:\s*,\s*\d+)*)\]").expect("static citation marker regex is valid"))
}

/// Splits `answer` into paragraph-ish segments (blank-line-separated blocks,
/// fence-aware so a blank line inside a ```code block``` does not split it),
/// each as a byte-offset span within `answer`. Mirrors the idea behind
/// `source_validation.rs::paragraphs` — a plain text block boundary is close
/// enough to a "claim boundary" for this purpose, and does not need to be a
/// real Markdown parse.
fn segments(answer: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start = 0usize;
    let mut in_fence = false;
    let mut block_has_content = false;
    let mut cursor = 0usize;
    for line in answer.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
        }
        let blank = trimmed.is_empty() && !in_fence;
        if blank {
            if block_has_content {
                spans.push((start, cursor));
                block_has_content = false;
            }
            // Leading/duplicate/just-closed-a-block blank lines all push the
            // NEXT segment's start forward past this line, so it never
            // carries an empty prefix and a just-closed span is never
            // re-opened at its own start.
            start = cursor + line.len();
        } else {
            block_has_content = true;
        }
        cursor += line.len();
    }
    if block_has_content {
        spans.push((start, answer.len()));
    }
    if spans.is_empty() {
        spans.push((0, answer.len()));
    }
    spans
}

/// The index of the segment containing byte offset `at`, clamped to the
/// last segment if `at` lands past every recorded span (can only happen from
/// an off-by-one at the very end of the text).
fn segment_index_for(spans: &[(usize, usize)], at: usize) -> usize {
    spans
        .iter()
        .position(|(start, end)| at >= *start && at < *end)
        .unwrap_or_else(|| spans.len().saturating_sub(1))
}

/// Anchors every citation this turn recorded to the first marker in
/// `answer` that cites it, returning one updated [`ChatEvent::Citation`] per
/// citation that got a match — same `id`, now carrying `claim_id`/`start`/
/// `end`. A marker index with no matching `RecordedCitation.number` is
/// silently skipped (never a panic, never a guessed match); a marker citing
/// several numbers (`[1, 2]`) gives every one of them the SAME `claim_id`
/// (they sit in the same sentence).
#[must_use]
pub fn anchor_citations(answer: &str, turn_id: &str, recorded: &[RecordedCitation]) -> Vec<ChatEvent> {
    if answer.is_empty() || recorded.is_empty() {
        return Vec::new();
    }
    let spans = segments(answer);
    let mut anchored: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut events = Vec::new();

    for captures in marker_regex().captures_iter(answer) {
        let Some(whole) = captures.get(0) else { continue };
        let Some(numbers) = captures.get(1) else { continue };
        let marker_start = whole.start();
        let marker_end = whole.end();
        let claim_id = format!("{turn_id}:claim-{}", segment_index_for(&spans, marker_start));

        for piece in numbers.as_str().split(',') {
            let Ok(n) = piece.trim().parse::<u32>() else { continue };
            let Some(citation) = recorded.iter().find(|c| c.number == Some(n)) else { continue };
            // First occurrence only — a later marker citing the same source
            // is left exactly as it already renders (the client resolves it
            // by its own regex match regardless of `claimId`).
            if !anchored.insert(citation.id.clone()) {
                continue;
            }
            events.push(ChatEvent::Citation {
                id: citation.id.clone(),
                title: citation.title.clone(),
                url: citation.url.clone(),
                snippet: citation.snippet.clone(),
                source_group_id: citation.source_group_id.clone(),
                source_group_label: citation.source_group_label.clone(),
                number: citation.number,
                claim_id: Some(claim_id.clone()),
                start: u32::try_from(marker_start).ok(),
                end: u32::try_from(marker_end).ok(),
                origin: citation.origin.clone(),
            });
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(id: &str, number: u32) -> RecordedCitation {
        RecordedCitation {
            id: id.to_owned(),
            title: format!("Title {id}"),
            url: format!("https://example.com/{id}"),
            snippet: "snippet".to_owned(),
            source_group_id: "web:example.com".to_owned(),
            source_group_label: None,
            number: Some(number),
            origin: "web".to_owned(),
        }
    }

    #[test]
    fn single_marker_gets_a_claim_id_and_offsets() {
        let answer = "Aquatiq delivers cleaning chemistry [1].";
        let events = anchor_citations(answer, "turn-1", &[recorded("c1", 1)]);
        assert_eq!(events.len(), 1);
        let ChatEvent::Citation { id, claim_id, start, end, .. } = &events[0] else {
            panic!("expected a citation");
        };
        assert_eq!(id, "c1");
        assert_eq!(claim_id.as_deref(), Some("turn-1:claim-0"));
        let marker_start = answer.find("[1]").unwrap();
        assert_eq!(*start, u32::try_from(marker_start).ok());
        assert_eq!(*end, u32::try_from(marker_start + 3).ok());
    }

    #[test]
    fn two_indexes_in_one_marker_share_the_same_claim_id() {
        let answer = "Two facts support this [1, 2].";
        let events = anchor_citations(answer, "turn-1", &[recorded("c1", 1), recorded("c2", 2)]);
        assert_eq!(events.len(), 2);
        let claim_ids: Vec<Option<String>> = events
            .iter()
            .map(|event| match event {
                ChatEvent::Citation { claim_id, .. } => claim_id.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(claim_ids[0], claim_ids[1]);
        assert!(claim_ids[0].is_some());
    }

    #[test]
    fn a_later_paragraph_gets_a_different_claim_id() {
        let answer = "First claim [1].\n\nSecond claim [2].";
        let events = anchor_citations(answer, "turn-1", &[recorded("c1", 1), recorded("c2", 2)]);
        assert_eq!(events.len(), 2);
        let ChatEvent::Citation { claim_id: first_claim, .. } = &events[0] else {
            panic!("expected a citation");
        };
        let ChatEvent::Citation { claim_id: second_claim, .. } = &events[1] else {
            panic!("expected a citation");
        };
        assert_ne!(first_claim, second_claim);
    }

    #[test]
    fn a_marker_index_with_no_matching_citation_is_skipped_not_panicked() {
        let answer = "This cites a source that was never recorded [7].";
        let events = anchor_citations(answer, "turn-1", &[recorded("c1", 1)]);
        assert!(events.is_empty(), "no matching number → no anchor event, never a panic");
    }

    #[test]
    fn the_same_citation_cited_twice_only_anchors_the_first_occurrence() {
        let answer = "First mention [1].\n\nSecond mention of the same source [1].";
        let events = anchor_citations(answer, "turn-1", &[recorded("c1", 1)]);
        assert_eq!(events.len(), 1, "only the first occurrence produces an anchor event");
        let ChatEvent::Citation { claim_id, .. } = &events[0] else {
            panic!("expected a citation");
        };
        assert_eq!(claim_id.as_deref(), Some("turn-1:claim-0"));
    }

    #[test]
    fn no_markers_at_all_yields_no_events() {
        let events = anchor_citations("No markers in this answer at all.", "turn-1", &[recorded("c1", 1)]);
        assert!(events.is_empty());
    }

    #[test]
    fn empty_answer_or_no_recorded_citations_yields_no_events() {
        assert!(anchor_citations("", "turn-1", &[recorded("c1", 1)]).is_empty());
        assert!(anchor_citations("Something [1].", "turn-1", &[]).is_empty());
    }
}
