use crate::context_pins::ContextPin;
use crate::pipeline::types::*;

/// CAG — pack pinned permanent-memory facts FIRST (priority order, they are
/// the org's always-on context), then fill the remaining budget with retrieval
/// candidates via `pack_context`. Pins that do not fit are dropped whole (a
/// truncated pin is a corrupted fact); `total_tokens`/`budget_tokens` cover the
/// combined pack. With no pins this is exactly `pack_context`.
pub fn pack_context_with_pins(
    pins: &[ContextPin],
    candidates: &[ScoredCandidate],
    sources: &[SourceRef],
    budget_tokens: usize,
    format: &str,
) -> ContextPack {
    let mut pinned_facts = Vec::new();
    let mut pinned_tokens = 0usize;
    for pin in pins {
        let est = estimate_tokens(&pin.content);
        if pinned_tokens + est > budget_tokens {
            continue; // drop whole pins that don't fit; keep trying smaller ones
        }
        pinned_facts.push(ContextFact {
            knowledge_id: pin.pin_id.clone(),
            document_id: format!("pin:{}", pin.pin_id),
            text: pin.content.clone(),
            score: 1.0, // pinned = unconditionally relevant by operator decree
            source_title: pin.title.clone(),
            source_type: "pinned".to_string(),
            estimated_tokens: est,
        });
        pinned_tokens += est;
    }

    let remaining = budget_tokens.saturating_sub(pinned_tokens);
    let mut pack = pack_context(candidates, sources, remaining, format);

    let mut facts = pinned_facts;
    facts.append(&mut pack.facts);
    ContextPack {
        facts,
        total_tokens: pinned_tokens + pack.total_tokens,
        budget_tokens,
        format: format.to_string(),
    }
}

/// "Lost in the middle" mitigation (plan P1-8).
///
/// LLMs attend most reliably to the **start and end** of a long context and
/// degrade in the middle (Liu et al., *Lost in the Middle*). The packer fills in
/// descending relevance order, so before this the least-relevant surviving fact
/// always occupied the tail — one of the two positions the model reads best —
/// while the mid-ranked evidence was buried where it reads worst.
///
/// Opt-in via `LONG_CONTEXT_REORDER=true`, default **off**. This changes the
/// prompt the model sees and therefore its output, and there is no scored golden
/// set yet (P0.5 is seeded but not yet runnable), so it must not flip silently.
/// Turn it on together with a before/after eval.
fn long_context_reorder_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("LONG_CONTEXT_REORDER")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
    })
}

/// Interleaves a relevance-ordered list so relevance decreases toward the middle.
///
/// `[1,2,3,4,5,6]` (1 = most relevant) becomes `[1,3,5,6,4,2]`: rank 1 at the
/// head, rank 2 at the tail, the weakest facts buried in the centre.
///
/// Pure and order-only — every fact keeps its `score`, ids and token estimate, so
/// citation rendering and any score-sorted consumer are unaffected.
fn reorder_for_long_context(facts: Vec<ContextFact>) -> Vec<ContextFact> {
    if facts.len() < 4 {
        // Below four items there is no meaningful "middle" to protect, and
        // shuffling two or three facts only obscures the ranking.
        return facts;
    }
    let mut head = Vec::with_capacity(facts.len());
    let mut tail = Vec::with_capacity(facts.len() / 2);
    for (i, fact) in facts.into_iter().enumerate() {
        if i % 2 == 0 {
            head.push(fact);
        } else {
            tail.push(fact);
        }
    }
    tail.reverse();
    head.extend(tail);
    head
}

/// Pack candidates into a token-budgeted context bundle.
/// Uses 4-chars-per-token heuristic (matches cl100k_base for English).
pub fn pack_context(
    candidates: &[ScoredCandidate],
    sources: &[SourceRef],
    budget_tokens: usize,
    format: &str,
) -> ContextPack {
    let mut facts = Vec::new();
    let mut used_tokens = 0;

    for candidate in candidates {
        let est_tokens = estimate_tokens(&candidate.text);
        if used_tokens + est_tokens > budget_tokens {
            // Try partial fit
            let remaining = budget_tokens.saturating_sub(used_tokens);
            if remaining > 20 {
                let truncated_len = remaining * 4;
                let truncated_text: String = candidate.text.chars().take(truncated_len).collect();
                let source = find_source(sources, &candidate.document_id);
                facts.push(ContextFact {
                    knowledge_id: candidate.knowledge_id.clone(),
                    document_id: candidate.document_id.clone(),
                    text: format!("{}...", truncated_text),
                    score: candidate.final_score,
                    source_title: source.map(|s| s.title.clone()).unwrap_or_default(),
                    source_type: source.map(|s| s.r#type.clone()).unwrap_or_default(),
                    estimated_tokens: remaining,
                });
                used_tokens += remaining;
            }
            break;
        }

        let source = find_source(sources, &candidate.document_id);
        facts.push(ContextFact {
            knowledge_id: candidate.knowledge_id.clone(),
            document_id: candidate.document_id.clone(),
            text: candidate.text.clone(),
            score: candidate.final_score,
            source_title: source.map(|s| s.title.clone()).unwrap_or_default(),
            source_type: source.map(|s| s.r#type.clone()).unwrap_or_default(),
            estimated_tokens: est_tokens,
        });
        used_tokens += est_tokens;
    }

    // P1-8: place the strongest facts at both ends of the window.
    //
    // Deliberately AFTER the budget loop: the loop must consume candidates in
    // relevance order so that what fits is the best content. Only the surviving
    // set is reordered.
    if long_context_reorder_enabled() {
        facts = reorder_for_long_context(facts);
    }

    ContextPack {
        facts,
        total_tokens: used_tokens,
        budget_tokens,
        format: format.to_string(),
    }
}

/// §16.3.2 — real BPE token counting instead of the `len()/4` heuristic.
/// We cache one `CoreBPE` instance per process; `cl100k_base` matches all
/// current Azure / OpenAI text-embedding and chat models. If the env var
/// `RETRIEVAL_TOKENIZER` is set to `heuristic` we fall back to the old
/// estimator (useful in tests that don't want the 5 MB BPE table loaded).
fn estimate_tokens(text: &str) -> usize {
    if std::env::var("RETRIEVAL_TOKENIZER").as_deref() == Ok("heuristic") {
        return text.len() / 4 + 1;
    }
    static BPE: once_cell::sync::Lazy<Option<tiktoken_rs::CoreBPE>> =
        once_cell::sync::Lazy::new(|| tiktoken_rs::cl100k_base().ok());
    match BPE.as_ref() {
        Some(bpe) => bpe.encode_with_special_tokens(text).len(),
        // BPE table failed to load — degrade to the heuristic so we never
        // crash a request just because the tokenizer init failed.
        None => text.len() / 4 + 1,
    }
}

fn find_source<'a>(sources: &'a [SourceRef], doc_id: &str) -> Option<&'a SourceRef> {
    sources.iter().find(|s| s.document_id == doc_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_pins::ContextPin;

    fn pin(id: &str, content: &str, priority: i32) -> ContextPin {
        ContextPin {
            pin_id: id.to_string(),
            title: format!("pin {id}"),
            content: content.to_string(),
            priority,
        }
    }

    fn cand(id: &str, text: &str) -> ScoredCandidate {
        ScoredCandidate {
            knowledge_id: id.to_string(),
            document_id: format!("doc-{id}"),
            text: text.to_string(),
            dense_score: 0.5,
            sparse_score: 0.0,
            rerank_score: 0.5,
            final_score: 0.5,
            chunk_index: 0,
            metadata: std::collections::HashMap::new(),
        }
    }

    fn fact(id: &str, score: f32) -> ContextFact {
        ContextFact {
            knowledge_id: id.to_string(),
            document_id: format!("doc-{id}"),
            text: format!("text {id}"),
            score,
            source_title: String::new(),
            source_type: String::new(),
            estimated_tokens: 1,
        }
    }

    fn ids(facts: &[ContextFact]) -> Vec<String> {
        facts.iter().map(|f| f.knowledge_id.clone()).collect()
    }

    #[test]
    fn reorder_puts_strongest_facts_at_both_ends() {
        // Ranked most→least relevant.
        let input: Vec<ContextFact> = (1..=6)
            .map(|i| fact(&i.to_string(), 1.0 - (i as f32) * 0.1))
            .collect();
        let out = reorder_for_long_context(input);
        assert_eq!(
            ids(&out),
            vec!["1", "3", "5", "6", "4", "2"],
            "rank 1 at the head, rank 2 at the tail, weakest in the middle"
        );
        // The two positions the model reads best hold the two best facts.
        assert_eq!(out.first().unwrap().knowledge_id, "1");
        assert_eq!(out.last().unwrap().knowledge_id, "2");
    }

    #[test]
    fn reorder_is_a_permutation_and_preserves_scores() {
        let input: Vec<ContextFact> = (1..=7)
            .map(|i| fact(&i.to_string(), 1.0 - (i as f32) * 0.1))
            .collect();
        let before: std::collections::HashMap<String, f32> =
            input.iter().map(|f| (f.knowledge_id.clone(), f.score)).collect();
        let out = reorder_for_long_context(input);
        assert_eq!(out.len(), before.len(), "no facts added or dropped");
        let mut seen = ids(&out);
        seen.sort();
        assert_eq!(seen, vec!["1", "2", "3", "4", "5", "6", "7"]);
        for f in &out {
            assert_eq!(
                before[&f.knowledge_id], f.score,
                "reordering must not alter scores (citations depend on them)"
            );
        }
    }

    #[test]
    fn reorder_leaves_short_packs_alone() {
        // Under four facts there is no middle worth protecting; shuffling would
        // only obscure the ranking.
        for n in 0..4usize {
            let input: Vec<ContextFact> =
                (1..=n).map(|i| fact(&i.to_string(), 0.5)).collect();
            let expected = ids(&input);
            assert_eq!(ids(&reorder_for_long_context(input)), expected, "n={n}");
        }
    }

    #[test]
    fn reorder_is_off_by_default_so_pack_order_is_unchanged() {
        // Guards the opt-in contract: without LONG_CONTEXT_REORDER the packer
        // must emit strict relevance order.
        let cands: Vec<ScoredCandidate> = (1..=6)
            .map(|i| cand(&i.to_string(), "some text"))
            .collect();
        let pack = pack_context(&cands, &[], 500, "json");
        assert_eq!(ids(&pack.facts), vec!["1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn pins_pack_first_then_candidates_within_budget() {
        let pins = vec![pin("p1", "the org's standing policy fact", 1)];
        let cands = vec![cand("k1", "retrieved chunk text")];
        let pack = pack_context_with_pins(&pins, &cands, &[], 10_000, "json");

        assert!(pack.facts.len() >= 2, "pin + candidate must both fit");
        assert_eq!(pack.facts[0].source_type, "pinned");
        assert_eq!(pack.facts[0].knowledge_id, "p1");
        assert_eq!(pack.facts[0].document_id, "pin:p1");
        assert!(pack.facts.iter().skip(1).any(|f| f.knowledge_id == "k1"));
        assert!(pack.total_tokens <= pack.budget_tokens);
    }

    #[test]
    fn oversized_pin_is_dropped_whole_never_truncated() {
        // A pin that cannot fit is dropped entirely (a truncated pin is a
        // corrupted fact); a smaller later pin may still fit.
        let big = "x ".repeat(4000);
        let pins = vec![pin("big", &big, 1), pin("small", "short fact", 2)];
        let pack = pack_context_with_pins(&pins, &[], &[], 50, "json");

        assert!(pack.facts.iter().all(|f| f.knowledge_id != "big"));
        assert!(pack.facts.iter().any(|f| f.knowledge_id == "small"));
        assert!(pack.total_tokens <= 50);
    }

    #[test]
    fn no_pins_matches_plain_pack() {
        let cands = vec![cand("k1", "retrieved chunk text")];
        let with = pack_context_with_pins(&[], &cands, &[], 500, "json");
        let plain = pack_context(&cands, &[], 500, "json");
        assert_eq!(with.facts.len(), plain.facts.len());
        assert_eq!(with.total_tokens, plain.total_tokens);
        assert_eq!(with.budget_tokens, 500);
    }
}
