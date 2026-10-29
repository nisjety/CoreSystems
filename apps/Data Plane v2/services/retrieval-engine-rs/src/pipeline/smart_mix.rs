//! Smart hybrid — query-adaptive mode-mix suggestion.
//!
//! Classifies the query text with cheap, deterministic lexical heuristics and
//! suggests blend-weight overrides for the arms that fit it: lexical/ID-like
//! queries lean BM25, natural-language questions lean dense, entity/relational
//! queries raise the graph arm, visual-intent queries raise the visual arm.
//!
//! Precedence is strictly LAST-resort-before-static-defaults:
//! `request mode_mix > agent_retrieval_configs > smart hybrid > config w_*`.
//! An explicit caller or agent config always wins; a neutral query returns all
//! `None` so the static defaults apply unchanged. Everything is renormalized by
//! `ModeMixWeights::resolve`, and the resolved mix is persisted on the trace —
//! so smart decisions stay fully auditable.
//!
//! English + Norwegian cues (the deployment's user base).

use crate::pipeline::types::ModeMixWeights;

const QUESTION_WORDS: &[&str] = &[
    // en
    "who",
    "what",
    "when",
    "where",
    "which",
    "why",
    "how",
    "is",
    "are",
    "does",
    "do",
    "can",
    "should",
    "explain",
    "describe",
    "summarize",
    "compare",
    // no
    "hvem",
    "hva",
    "når",
    "hvor",
    "hvilken",
    "hvilke",
    "hvordan",
    "hvorfor",
    "er",
    "kan",
    "bør",
    "forklar",
    "beskriv",
    "oppsummer",
    "sammenlign",
];

const RELATIONAL_CUES: &[&str] = &[
    // en (single tokens; multiword cues below)
    "related",
    "relationship",
    "relation",
    "connected",
    "connection",
    "linked",
    "between",
    "associated",
    "network",
    // no
    "relatert",
    "relasjon",
    "koblet",
    "kobling",
    "mellom",
    "tilknyttet",
    "forbindelse",
];

const RELATIONAL_PHRASES: &[&str] = &[
    "works at",
    "works with",
    "depends on",
    "who knows",
    "part of",
    "member of",
    "reports to",
    "jobber i",
    "jobber med",
    "jobber hos",
    "avhenger av",
    "del av",
    "medlem av",
];

const VISUAL_CUES: &[&str] = &[
    // en
    "image",
    "images",
    "diagram",
    "diagrams",
    "figure",
    "figures",
    "chart",
    "charts",
    "screenshot",
    "picture",
    "pictures",
    "photo",
    "photos",
    "drawing",
    "layout",
    "slide",
    "illustration",
    "blueprint",
    // no — including definite/plural inflections (Norwegian suffixes the
    // token-set match would otherwise miss; explicit forms beat suffix
    // stemming, which false-positives on English words like "charter").
    "bilde",
    "bildet",
    "bilder",
    "bildene",
    "figur",
    "figuren",
    "figurer",
    "diagrammet",
    "skjermbilde",
    "skjermbildet",
    "tegning",
    "tegningen",
    "tegninger",
    "tegningene",
    "illustrasjon",
    "illustrasjonen",
    "plantegning",
    "plantegningen",
    "plantegninger",
];

/// A token that looks like an identifier/code/SKU rather than prose — the
/// queries where exact lexical (BM25) match beats semantic similarity.
fn looks_code_like(token: &str) -> bool {
    let t = token.trim_matches(|c: char| c.is_ascii_punctuation());
    if t.len() < 2 {
        return false;
    }
    let has_digit = t.chars().any(|c| c.is_ascii_digit());
    let has_alpha = t.chars().any(|c| c.is_alphabetic());
    // Mixed letters+digits ("INV2039", "SKU-1234" after trim), path/underscore
    // tokens, or short ALL-CAPS codes ("GDPR", "ISO", "MVA").
    (has_digit && has_alpha)
        || t.contains('_')
        || t.contains('/')
        || (t.len() <= 10
            && t.chars().filter(|c| c.is_alphabetic()).count() >= 2
            && t.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
}

/// A mid-sentence token that reads as a proper noun ("Ada", "Aquatiq").
fn looks_proper_noun(token: &str) -> bool {
    let mut chars = token.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            token.len() >= 2 && chars.all(|c| c.is_lowercase() || c == '-' || c == '\'')
        }
        _ => false,
    }
}

/// Suggest blend-weight overrides for `query`. Neutral queries return all-`None`
/// (static defaults apply). Pure and deterministic — safe to call per-request.
pub fn smart_mode_mix(query: &str) -> ModeMixWeights {
    let trimmed = query.trim();
    let lower = trimmed.to_lowercase();
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let lower_tokens: Vec<String> = tokens
        .iter()
        .map(|t| {
            t.trim_matches(|c: char| c.is_ascii_punctuation())
                .to_lowercase()
        })
        .collect();

    let code_like = tokens.iter().any(|t| looks_code_like(t)) || trimmed.contains('"');
    let question = trimmed.ends_with('?')
        || lower_tokens
            .first()
            .is_some_and(|t| QUESTION_WORDS.contains(&t.as_str()));
    let relational = lower_tokens
        .iter()
        .any(|t| RELATIONAL_CUES.contains(&t.as_str()))
        || RELATIONAL_PHRASES.iter().any(|p| lower.contains(p))
        || tokens
            .iter()
            .skip(1)
            .filter(|t| looks_proper_noun(t))
            .count()
            >= 2;
    let visual = lower_tokens
        .iter()
        .any(|t| VISUAL_CUES.contains(&t.as_str()));

    let mut mix = ModeMixWeights::default();
    if code_like && tokens.len() <= 6 && !question {
        // Short ID/code lookup — exact lexical match dominates. Also raise
        // the keyword arm (Meilisearch): typo-tolerant exact-ID/code lookup
        // is precisely this arm's reason to exist (see its module docs), so
        // a code-like query should lean on it more than the flat static
        // default. 0.15 mirrors the ~3x boost `w_graph`/`w_visual` get for
        // their own trigger conditions below, scaled off `w_keyword`'s own
        // 0.05 static default.
        mix.w_bm25 = Some(0.45);
        mix.w_dense = Some(0.35);
        mix.w_keyword = Some(0.15);
    }
    if question && !code_like {
        // Natural-language question — semantic similarity dominates.
        mix.w_dense = Some(0.55);
    }
    if relational {
        // Entity/relationship intent — raise the graph arm.
        mix.w_graph = Some(0.3);
    }
    if visual {
        // Visual intent — raise the page-image arm (a no-op skip when the
        // visual embedder isn't configured, so this never breaks text-only).
        mix.w_visual = Some(0.2);
    }
    mix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_neutral(m: &ModeMixWeights) -> bool {
        m.w_dense.is_none()
            && m.w_bm25.is_none()
            && m.w_graph.is_none()
            && m.w_wiki.is_none()
            && m.w_visual.is_none()
            && m.w_keyword.is_none()
    }

    #[test]
    fn id_code_queries_lean_bm25_and_raise_the_keyword_arm() {
        for q in ["INV-20394", "order SKU_9931 status", "GDPR ISO 27001"] {
            let m = smart_mode_mix(q);
            assert_eq!(m.w_bm25, Some(0.45), "{q}");
            assert_eq!(m.w_dense, Some(0.35), "{q}");
            // This is the exact query shape the keyword arm exists for
            // (typo-tolerant exact-ID/code lookup) — it must be boosted
            // above its flat static default, not left at `None`.
            assert_eq!(m.w_keyword, Some(0.15), "{q}");
        }
    }

    #[test]
    fn natural_questions_lean_dense_english_and_norwegian() {
        for q in [
            "how does the retrieval pipeline decide which arm to run?",
            "hvordan fungerer faktureringen for bedriftskunder",
            "explain the onboarding flow",
        ] {
            let m = smart_mode_mix(q);
            assert_eq!(m.w_dense, Some(0.55), "{q}");
            assert!(m.w_bm25.is_none(), "{q}");
        }
    }

    #[test]
    fn relational_queries_raise_graph() {
        for q in [
            "how is Ada connected to the Analytical Engine project?",
            "hvem jobber med leveranser hos Aquatiq",
            "relationship between suppliers and contracts",
        ] {
            let m = smart_mode_mix(q);
            assert_eq!(m.w_graph, Some(0.3), "{q}");
        }
    }

    #[test]
    fn visual_intent_raises_visual_arm() {
        for q in [
            "show the network diagram for the warehouse",
            "finn tegningen av produksjonslinjen",
        ] {
            let m = smart_mode_mix(q);
            assert_eq!(m.w_visual, Some(0.2), "{q}");
        }
    }

    #[test]
    fn neutral_queries_defer_to_static_defaults() {
        for q in ["quarterly revenue summary", "leveringsbetingelser standard"] {
            assert!(is_neutral(&smart_mode_mix(q)), "{q}");
        }
    }

    #[test]
    fn signals_compose_question_plus_relational() {
        let m = smart_mode_mix("who works at Aquatiq Solutions together with Berit Hansen?");
        assert_eq!(m.w_dense, Some(0.55));
        assert_eq!(m.w_graph, Some(0.3));
    }

    #[test]
    fn resolve_renormalizes_smart_output() {
        // Whatever the heuristics emit must renormalize to a valid blend.
        let m = smart_mode_mix("how is Ada connected to the diagram of INV-1?");
        let r = m.resolve(
            0.45,
            0.2,
            0.2,
            0.1,
            0.05,
            0.05,
            crate::search::fusion::DEFAULT_RRF_K,
        );
        let sum = r.w_dense + r.w_bm25 + r.w_graph + r.w_wiki + r.w_visual + r.w_keyword;
        assert!((sum - 1.0).abs() < 1e-5, "sum = {sum}");
    }

    #[test]
    fn resolve_renormalizes_a_code_like_query_with_the_keyword_boost_applied() {
        let m = smart_mode_mix("INV-20394");
        let r = m.resolve(
            0.45,
            0.2,
            0.2,
            0.1,
            0.05,
            0.05,
            crate::search::fusion::DEFAULT_RRF_K,
        );
        let sum = r.w_dense + r.w_bm25 + r.w_graph + r.w_wiki + r.w_visual + r.w_keyword;
        assert!((sum - 1.0).abs() < 1e-5, "sum = {sum}");
        // Raw shares: dense .35, bm25 .45, graph .2, wiki .1, visual .05,
        // keyword .15 → total 1.30; keyword's resolved share is 0.15/1.30.
        assert!((r.w_keyword - (0.15 / 1.30)).abs() < 1e-4);
    }
}
