# INNH-7 progress

Run of 2026-10-31 (lane A, worktree innh-lane-a, branch innh/lane-a). Continue from the tree: every step below leaves a checkable change.

Working note: the scratchpad is shared with other lanes; this lane's helper scripts are prefixed innh7_. Files here are CRLF in the worktree (index LF, core.autocrlf=true); edits were made EOL-preserving by script.

- [x] Read INNH-7-status.md, the brief, routes.rs scrape_with, cache.rs, scrape.rs, pipeline inline/artifact code.
- [x] quarry-core: CachePolicy.mode serde default (+ CacheMode default ReadWrite); ScrapeOptions.max_age + OPTION_KEYS 11 + read/validate (0..=2_592_000_000, feature_disabled while off); ResolvedScrapeOptions.max_age_ms; result_cache_admissible(); EffectiveScrapeOptions max_age / cached_at / cache_age_ms + served_from_cache(); unit tests.
- [x] quarry-edge: PageCache::result_key / get_result / put_result + CachedScrape (cache.rs); new scrape_cache.rs (eligibility, age, storable, rehydrate from artifacts); scrape_contract.rs flag QUARRY_EDGE__SCRAPE_MAX_AGE (off); routes.rs read before the driver, write after success; pipeline::offer_inline made pub.
- [x] Docs: openapi.yaml (ScrapeRequest.max_age, CachePolicy, effective_options), CONTRACTS.md 9.1, ERROR_REASONS.md, RUNBOOKS.md, GAP.md:61 corrected, compose files (default off).
- [x] ZDR matrix row scrape_max_age (Degraded, warning max_age_ignored_zdr) + scrape basis; goldens regenerated.
- [x] Tests written: tests/scrape_max_age.rs (13 cases), tests/scrape_max_age_off.rs (3), unit tests in cache.rs, scrape_cache.rs, scrape_contract.rs, quarry-core.
- [x] Rust run (qtest-lane a, Redis started in the container): core 284 + schemas; edge lib; scrape_max_age 14, scrape_max_age_off 3, cache_policy 10, ai_formats 17, ai_formats_off 3, zdr_feature_matrix 14, zdr_gap_rows 9: all green. Counts in INNH-7-status.md.
- [x] Node contract tests: 60 tests, 48 pass, 0 fail, 12 todo (pre-existing).
- [x] Status file, depth lines, committed locally on innh/lane-a (two commits: code+docs, then plan-work files).
