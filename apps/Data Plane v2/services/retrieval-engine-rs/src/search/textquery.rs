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

/// Terms that look like an exact thing the user is hunting — an identifier, a
/// path, a config key, a version — as opposed to the prose they wrapped it in.
///
/// # Why the lexical arms need this
///
/// OR-joining every term fixed "multi-word questions match nothing", then swung
/// straight into the opposite failure: a rare, high-value term OR'd with common
/// words gets swamped by them. Measured on the live corpus, hunting a path:
///
/// | Quickwit query | hits | target in top 5 |
/// |---|---|---|
/// | `"where" OR "is" OR "internal/jobs/executor.go" OR "called"` | 1,517 | 0/5 |
/// | `"internal/jobs/executor.go"` alone | 14 | 5/5 |
/// | `+("internal/jobs/executor.go") AND ("where" OR "is" OR "called")` | 14 | 5/5 |
///
/// End to end that cost 0.115 nDCG: the same 23 mined identifiers wrapped in
/// `where is X defined and how is it used` scored 0.8473 against 0.9622 bare,
/// with two becoming unfindable.
///
/// Boosting was tried first and does nothing — Quickwit accepts `term^10` and
/// ranks identically. Requiring the term is what works.
///
/// # Why requiring is the right call for the LEXICAL arm specifically
///
/// It trades recall for precision, which would be wrong for a single-arm search
/// and is right here: results are fused. The lexical arm exists to nail exact
/// tokens; the dense arm already supplies the fuzzy, semantic, typo-tolerant
/// half. Letting each arm do what it is good at beats making both mediocre.
///
/// Shape is deliberately "at least one distinctive term", not "all of them": a
/// user naming two identifiers usually wants either, and requiring both can
/// yield nothing.
///
/// Heuristic, and intentionally a narrow one — every rule below fires on
/// something a person would expect an exact match for:
/// * contains `_` or `/` — `snake_case`, `SCREAMING_CASE`, paths
/// * contains a digit **and** a letter — `embed-v-4-0`, `PLAN_2026`, `v4`. The
///   letter is required so a bare number does not anchor: "top 10 results" must
///   not become a required match on `10`, and a year on its own is prose.
/// * has a short alphabetic extension after a `.` — file names
///
/// A plain long word is NOT distinctive: prose is exactly what the dense arm
/// handles, and promoting ordinary words here would re-create the flood.
#[must_use]
pub fn distinctive_terms(query: &str) -> Vec<String> {
    sanitize_terms(query)
        .into_iter()
        .filter(|term| is_distinctive(term))
        .collect()
}

fn is_distinctive(term: &str) -> bool {
    if term.contains('_') || term.contains('/') {
        return true;
    }
    if term.chars().any(char::is_numeric) && term.chars().any(char::is_alphabetic) {
        return true;
    }
    // `foo.rs`, `config.yaml` — a short alphabetic tail after a dot. Guards
    // against treating an abbreviation or a sentence-ending word as a filename.
    match term.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (2..=4).contains(&ext.len())
                && ext.chars().all(|ch| ch.is_ascii_alphabetic())
        }
        None => false,
    }
}

/// The FTS predicate for the lexical arms: anchor on the distinctive terms when
/// the query has any, else fall back to the full disjunction.
///
/// Ordinary terms are dropped rather than OR'd in alongside, because
/// `websearch_to_tsquery` cannot express "required plus optional" — an OR of
/// prose words can only add rows, never reorder them, and ranking is a separate
/// `ts_rank` concern. Quickwit keeps the ordinary terms as an optional clause,
/// where they legitimately influence BM25 order; see `build_quickwit_query`.
#[must_use]
pub fn fts_anchor_disjunction(query: &str) -> String {
    let distinctive = distinctive_terms(query);
    if distinctive.is_empty() {
        fts_disjunction(query)
    } else {
        distinctive.join(" or ")
    }
}

/// Reduce a user query to bare terms: strip anything that could carry query
/// syntax (`:` field selectors, wildcards, parentheses) and cap the count so one
/// enormous query cannot build an unbounded disjunction.
///
/// # `/` is preserved, and stripping it silently broke every path query
///
/// `/` was stripped here as "path syntax". But Postgres' `simple` parser emits a
/// file path as ONE lexeme *containing the slashes* — `to_tsvector('simple',
/// 'internal/jobs/executor.go')` is `'internal/jobs/executor.go'` — so removing
/// the separators produced a term that cannot match anything that was indexed.
/// Both lexical arms returned zero for any path-shaped query, measured on the
/// live 1,164-chunk corpus:
///
/// | query as sent | Postgres FTS | Quickwit |
/// |---|---|---|
/// | `src/api/mod.rs` (raw) | 10 rows | 153 hits |
/// | `srcapimod.rs` (slashes stripped) | **0** | **0** |
/// | `zzz/qqq/nope.rs` (control) | — | 0 hits |
///
/// The control matters: 153 is real signal, not a loose match, so the stripped
/// form was discarding genuine hits rather than avoiding false ones. Same silent
/// -zero signature as the AND-vs-OR defect above, and it survived the mined
/// lexical golden set because that set contains only `snake_case`/`SCREAMING`
/// identifiers and no path-shaped queries at all — the gate measured the case
/// that worked.
///
/// Safe for every consumer, verified rather than assumed:
/// * Postgres — `websearch_to_tsquery` is total on user input, and a path
///   survives OR-joining intact: `'src/api/mod.rs' | 'retrieval'`.
/// * Quickwit — `build_quickwit_query` wraps every term in `quote_term`
///   unconditionally, so a `/` is inside a quoted phrase and cannot act as
///   syntax.
///
/// `:` stays stripped: it is a Quickwit field selector, and unlike `/` there is
/// no measured recall behind keeping it.
#[must_use]
pub fn sanitize_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter_map(|term| {
            let cleaned: String = term
                .chars()
                .filter(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
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
        // `.` and `/` are deliberately KEPT — version strings, file extensions,
        // hostnames and paths are all real query terms — so `../bad` survives
        // whole. Only `:` is removed, because it is a Quickwit field selector.
        assert_eq!(got, "alpha or siteignored or ../bad or beta");
        assert!(
            !got.contains(':'),
            "field selectors must not survive: {got}"
        );
    }

    /// A file path must reach the query builders INTACT.
    ///
    /// This previously asserted the opposite. Stripping `/` was believed to be
    /// defensive, but Postgres' `simple` parser indexes a path as one lexeme
    /// containing its slashes, so the stripped term matched nothing: measured
    /// 10 rows -> 0 on Postgres FTS and 153 hits -> 0 on Quickwit for
    /// `src/api/mod.rs`, with a nonsense path confirming the 153 was real
    /// signal. Every path-shaped query silently returned zero on both lexical
    /// arms. Keep this test pointing the way it does now.
    #[test]
    fn file_paths_survive_intact() {
        assert_eq!(
            fts_disjunction("src/api/mod.rs"),
            "src/api/mod.rs",
            "a lone path must not be mangled"
        );
        assert_eq!(
            fts_disjunction("where is internal/jobs/executor.go called"),
            "where or is or internal/jobs/executor.go or called",
            "a path mixed into prose must survive the disjunction"
        );
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
    fn identifier_shapes_are_distinctive_and_prose_is_not() {
        // The anchor terms a person expects an exact match on.
        for term in [
            "RERANK_TOP_K",
            "idx_ku_content_tsv_gin",
            "src/api/mod.rs",
            "internal/jobs/executor.go",
            "embed-v-4-0",
            "config.yaml",
            "QM_INSPIRED_IMPROVEMENT_PLAN_2026",
        ] {
            assert!(is_distinctive(term), "{term} should anchor the lexical arm");
        }
        // Prose must NOT be promoted — that would rebuild the flood this exists
        // to stop. Note `defined.` keeps its dot after sanitization, so the
        // filename rule has to reject a sentence-ending word.
        for term in ["where", "is", "defined", "called", "retrieval", "defined."] {
            assert!(!is_distinctive(term), "{term} must stay ordinary");
        }
    }

    #[test]
    fn an_anchored_query_drops_the_prose() {
        // The measured failure: the path buried under `where`/`is`/`called`.
        assert_eq!(
            fts_anchor_disjunction("where is internal/jobs/executor.go called"),
            "internal/jobs/executor.go"
        );
        // Two anchors stay a disjunction — either is a legitimate answer.
        assert_eq!(
            fts_anchor_disjunction("compare RERANK_TOP_K and W_BM25 defaults"),
            "RERANK_TOP_K or W_BM25"
        );
    }

    #[test]
    fn pure_prose_keeps_the_full_disjunction() {
        // No anchor present, so behaviour is unchanged — this is the path the
        // 87-query natural-language set measures, and it must not move.
        assert_eq!(
            fts_anchor_disjunction("how does retrieval combine dense and sparse"),
            // Note the literal "and" from the question survives as a term —
            // `websearch_to_tsquery` treats operator keywords as ordinary words.
            "how or does or retrieval or combine or dense or and or sparse"
        );
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
