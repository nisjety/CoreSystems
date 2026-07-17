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
