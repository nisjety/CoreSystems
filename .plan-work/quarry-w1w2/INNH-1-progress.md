# INNH-1 progress

Run of 2026-10-31. Docker down; nothing built or run; nothing committed.

- [x] Read brief, INNH-1-status.md, plan WS16 (2890-3018), X-7 / ADR-4 records, decision rounds.
- [x] Read code: quarry-core scrape.rs (ScrapeFormat 279-335, parse_formats 754-839, ScrapeLimits 913-948, ResolvedScrapeOptions 952-1000), quarry-runtime ai_formats.rs, mp_client.rs, quarry-edge scrape_actions.rs (flag pattern, route deadline), scrape_contract.rs limits(), routes.rs stream refusals, extract_routes.rs runner build, main.rs model_plane_token (dev bypass only), Model Plane gateway ai_chat handler + VerifiedInferenceBearer.
- [x] Wrote INNH-1-design.md.
- [x] Wrote depth/INNH-1.md (depth items, not for this wave).
- [ ] Next (not this run): owner decisions OD-1..OD-7, then build units U1 -> U2 -> U3 per the design.

Files written by this run (all new, LF, uncommitted):
- .plan-work/quarry-w1w2/INNH-1-progress.md
- .plan-work/quarry-w1w2/INNH-1-design.md
- .plan-work/quarry-w1w2/depth/INNH-1.md

## Run of 2026-10-31 (U1 contract, quarry-core)
- [x] ai_spec.rs (specs, parser, schema checks, canonical hash, report types) + 13 unit tests.
- [x] scrape.rs: `ScrapeFormat::Ai`, not Copy, parse_formats / parse_object_format, `ai_formats_enabled`, `ai_specs()`, canonical key, `ScrapeResponse` fields, 9 new tests.
- [x] routes.rs: 5 `None` fields so quarry-edge still compiles (U3 replaces).
- [x] zdr_matrix.rs basis text; ZDR golden + sha256 + ZDR_MATRIX.md regenerated.
- [x] Schema goldens regenerated; openapi.yaml, CONTRACTS.md, ERROR_REASONS.md; parity test (+4).
- [x] Rust (qtest) and node tests green; committed locally as "feat(quarry-core): ... (INNH-1 U1)".
- [ ] Next: U2 (quarry-runtime: mp_client chat_for_org, ai_scrape.rs, ai_verify.rs, pipeline ai_source) then U3 (quarry-edge wiring, flags, compose, SELF_HOST, RUNBOOKS, docs-contract test, `enforced_by` rows in zdr_matrix.rs). Read INNH-1-design.md section 10 and INNH-1-status.md "U1 as built" (deviations) first.
