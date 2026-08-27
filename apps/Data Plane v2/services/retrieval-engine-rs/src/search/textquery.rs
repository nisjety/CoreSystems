//! Shared full-text query construction for every Postgres FTS surface.
//!
//! This exists because the same defect was found independently in three places,
//! and a fourth was one edit away from it: **Postgres' `plainto_tsquery` and
//! `websearch_to_tsquery` both AND bare words together.** A natural-language
//! question therefore required EVERY one of its words to appear in the target
//! text, which for short text is essentially never.
//!
//! Measured on the live corpus:
//!
//! | surface | AND (as shipped) | OR (fixed) |
//! |---|---|---|
//! | `knowledge_units.content_tsv` (sparse arm) | 0 rows | 1,161 rows |
//! | `graph_entities.entity_text` (graph seeds + 1-hop) | 0 rows | 3 rows |
//!
//! The graph case is the starker one: entity texts are short phrases like
//! `"embeddings"`, so a five-word question could not match an entity even when
//! an entity by exactly that name existed. Both graph tiers and the
//! contradictions claim search returned nothing while looking perfectly healthy
//! — the same silent-zero signature as the reranker and the Quickwit arm.
//!
//! Use [`fts_disjunction`] for the bind parameter and `websearch_to_tsquery` in
//! the SQL. Do not reach for `plainto_tsquery` on user text again.

/// Terms are OR-joined and matched with `websearch_to_tsquery`, which is
/// *total* on user input — it never raises a syntax error, so no sanitized term
/// combination can fail the query. Verified: `a or or or -- \\ ) | & ! or b`
/// parses to `'a' | 'or' | 'b'`.
///
/// Returns an empty string when nothing survives sanitization; callers should
/// treat that as "no text predicate" rather than binding it, since
/// `websearch_to_tsquery('simple', '')` matches nothing and emits a NOTICE.
#[must_use]
pub fn fts_disjunction(query: &str) -> String {
    sanitize_terms(query).join(" or ")
}

/// Reduce a user query to bare terms: strip anything that could carry query
/// syntax (`:` field selectors, `/` paths, wildcards, parentheses) and cap the
/// count so one enormous query cannot build an unbounded disjunction.
#[must_use]
pub fn sanitize_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter_map(|term| {
            let cleaned: String = term
                .chars()
                .filter(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
                .collect();
            (!cleaned.is_empty()).then_some(cleaned)
        })
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The regression this module exists for: a multi-word question must become
    // a disjunction, because both tsquery builders AND bare words.
    #[test]
    fn a_question_becomes_a_disjunction() {
        let got = fts_disjunction("how do stale embeddings get detected");
        assert_eq!(got, "how or do or stale or embeddings or get or detected");
        assert!(!got.contains(" and "));
    }

    #[test]
    fn query_syntax_is_stripped_from_terms() {
        let got = fts_disjunction("alpha site:ignored ../bad beta");
        // `.` is deliberately KEPT (version strings, file extensions, hostnames
        // are real query terms), so `../bad` reduces to `..bad` rather than
        // `bad`. Harmless as a tsquery term; the path separator is what mattered.
        assert_eq!(got, "alpha or siteignored or ..bad or beta");
        assert!(!got.contains(':'), "field selectors must not survive: {got}");
        assert!(!got.contains('/'), "path separators must not survive: {got}");
    }

    // `websearch_to_tsquery` tolerates operator keywords as ordinary words, so a
    // user typing "or"/"and" cannot break the query — it just becomes a term.
    #[test]
    fn operator_keywords_survive_as_terms() {
        assert_eq!(fts_disjunction("cats or dogs"), "cats or or or dogs");
    }

    #[test]
    fn punctuation_only_input_yields_no_predicate() {
        assert!(fts_disjunction("??? !!! :::").is_empty());
        assert!(fts_disjunction("").is_empty());
    }

    #[test]
    fn term_count_is_capped() {
        let long = (0..100)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(sanitize_terms(&long).len(), 32);
    }
}
