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

## Run of 2026-10-31 (U2 engine and client, quarry-runtime)
- [x] mp_client.rs: `AiChat*` types, `ModelHopError`, `chat_for_org` (retry rule, 60 s timeout, both tokens in dev bypass, Dynamic and no-credential give `LaneNotGranted`).
- [x] ai_verify.rs: fold, `FoldedText::find` (char offsets), `is_grounded`, `verify_highlights` + 8 unit tests.
- [x] ai_scrape.rs: engine (nonce framing, json local + one model repair, summary, question, highlights, budget, 429 short-circuit, ZDR and empty-page guards, content-free logs) + 10 unit tests.
- [x] pipeline.rs: `InlineRequest.ai_source_max_chars`, `InlineParts.ai_source` + 3 tests; routes.rs 2-line literal fix so quarry-edge compiles.
- [x] tests/ai_scrape.rs: 41 tests against a strict axum double. lib 1052 pass, log_hygiene 5 pass, quarry-edge `cargo check --tests` clean.
- [x] Committed locally as "feat(quarry-runtime): ... (INNH-1 U2)".
- [ ] Next: U3 (quarry-edge): `scrape_contract/ai_formats.rs` (env flags via OnceLock, 403 `extract_ai_scope_required`, 501 `model_plane_unavailable` / `model_lane_not_granted`, engine built from `state.model_plane_url` / `model_plane_token` / `QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN`, static 16-permit semaphore passed with `with_process_limit`, `run_ai_formats`), routes.rs (set `ai_source_max_chars` from `AiScrapeConfig.max_input_chars`, call the engine after the fetch, fill `ScrapeResponse.json/summary/answer/highlights/ai` from `AiFields` and `AiReport`, stream refusal), scrape_actions.rs 125 s deadline, tests, compose, SELF_HOST, RUNBOOKS, docs-contract test, `enforced_by` rows in zdr_matrix.rs. Pass `claims.effective_zdr_mode(req.zdr).is_active()` as `AiScrapeInput.zdr`; `AiFields.highlights`/`answer`/`json`/`summary` map 1:1 onto the response fields; the page source is `InlineParts.ai_source`.
