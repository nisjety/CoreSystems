use crate::pipeline::types::*;

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
