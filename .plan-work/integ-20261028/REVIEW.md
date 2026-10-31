# Review of the conflict resolution (integ/main-20261028)

Intended location: `C:/dev/CoresSystem/.claude/worktrees/integ-20261028/.plan-work/integ-20261028/REVIEW.md`. A PreToolUse hook stops this session from writing into another worktree, so the file is here. Copy it into place, or rerun the step from a session that is allowed to write in the integ worktree.

Reviewed: 2026-10-28, read-only. Worktree `integ-20261028`, branch `integ/main-20261028` (HEAD = quarryv2-next `bd61260ee`), staged = live snapshot `0b3dee61f` merged in, merge base `afb839283`.
Method: for each of the 24 conflicted files (list from `git merge-tree --name-only quarryv2-next 0b3dee61f`), (1) `git diff afb839283 0b3dee61f`, (2) `git diff afb839283 quarryv2-next`, (3) the staged file. Every +/- line of (1) and (2) was checked for presence in (3), and every difference was explained. Defaults were checked against "Owner decisions of 2026-10-28" in `docs/plans/quarry-browser-build-plan-2026-10-14.md`. I also checked 14 files that both sides changed and git merged without a conflict, looking for semantic clashes.

## Summary

- No hunk of either side is lost by accident in the 24 files. Every difference from (1) or (2) is a deliberate resolution, and each one is correct: option B replaces `ModelPlaneSearchReranker`, the branch's error_class lines were moved onto live's code or exempted with a reason, `Option<bool>` became `bool`, `req.zdr.unwrap_or(false)`, and a single digest was regenerated for the pins.
- No compose or config default contradicts the owner decisions. MODEL_PLANE_URL is empty (opt-in). AUTOPROMPT, SEMANTIC_RERANK, LLM_CLASSIFY_INTENT and PAGE_TITLE_ENRICH are 0. The four QB-T17 switches are 0. SCRAPE_ACTIONS is off, CRAWL_ROBOTS is observe, ROBOTS_UNREACHABLE is disallow and BUDGET_ENFORCEMENT is shadow. The binary defaults are off too.
- One merge result will break CI: the spec gained live's search fields, but the branch's committed SDK docs gate was not regenerated (H1).
- Only 16 of the 24 conflicted files have resolution notes. The other 8 have none: the Ingestion compose, five Quarry docs, openapi.yaml and docs-contract.test.mjs. I reviewed those 8 here and found them correct.
- `BUILD.md` does not exist yet, so there are no Build test numbers. See "Test numbers" at the end.

## Findings, by severity

### H1. The committed Quarry SDK docs no longer match openapi.yaml, so spa-verify fails
- Files: `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` (live hunks) against `apps/Ingestion Plane/Quarry-v2/sdks/typescript/docs/*` (branch) and the branch gate `.github/workflows/verevonv3-ci.yml:196` ("Check the committed SDK docs match the spec").
- Hunks: live added the `/v1/search` `'400'` response, `SearchRequest.zdr` and `score_kinds`, `SearchResultItem.score`, `SearchResponse.score_kind` and `rerank`, and a new `RerankReport` schema. The merge kept all of them (merged-vs-branch = live's 77/1 lines). The branch's gate runs `sdks/generate.sh` (generator 7.22.0), then `git status --porcelain` on the docs. `SearchRequest.md`, `SearchResponse.md` and `SearchResultItem.md` have none of the new fields, and `RerankReport.md` does not exist. So the gate reports drift and spa-verify goes red. A change to openapi.yaml triggers it (paths filter, lines 17 and 30).
- Fix: run `apps/Ingestion Plane/Quarry-v2/sdks/generate.sh` (needs Docker) and commit the changed and new pages under `sdks/typescript/docs` (expect SearchRequest, SearchResponse, SearchResultItem and a new RerankReport). Then run the Python smoke tests (`sdks/python/tests`) as the job does. Do it before the merge commit, or as the next commit on the branch.

### M1. Option B and the QB-T17 gate land now, but the decision text says "later"
- Decision DATAPLANE-OPTION-B (2026-10-28): "Flags-off compose now ... QB-T17 stays pending; option B lands later from main after the search_suggest ZDR and token-retention review."
- Merged state: `DataPlaneSearchReranker`, `data_plane_http.rs`, `model_plane_gate.rs` and the docs-contract assertions are all in, and the `pending(...)` markers (`OPTION_B`, `QB_T17`) are removed (`scripts/tests/docs-contract.test.mjs`, merged = live's file plus the branch's whole-file YAML check). The user asked for this merge, and every flag stays 0, so nothing is switched on. But the review gate named in the decision (search_suggest ZDR and token retention) is not recorded anywhere.
- Fix: no code change. Add a ledger line to the plan or the decision document. It should say that option B code and the QB-T17 gate arrived through the merge of main on 2026-10-28 with every flag off, and that `QUARRY_EDGE_SEMANTIC_RERANK` and `QUARRY_EDGE_SEARCH_SUGGEST` stay 0 until that review is done.

### M2. The web policy does not cover the new paid Data Plane calls (a semantic clash git could not see)
- Files: `crates/quarry-edge/src/search_routes.rs` (branch WS7.E: `web_policy_edge::paid_provider` sets `opts.allow_paid_providers`, and a refusal becomes `policy_downgrades: ["search.paid_provider"]`) and live's `RerankingSearchProvider` / `DataPlaneVectorIndex` (a paid cross-encoder score per search, and a paid query embedding).
- Effect: if an org's policy refuses `search.paid_provider`, the SERP SaaS providers are dropped but the rerank and vector calls still run and bill, and `policy_downgrades` does not mention them. The catalogue (`crates/quarry-core/contracts/web-policy-catalogue.v1.json`) has no key for either. Nothing is live today, because SEMANTIC_RERANK and VECTOR_RETRIEVAL default off.
- Fix: before either flag is turned on, decide whether `search.paid_provider` (or a new catalogue key) governs them. If it does, carry `paid.allowed` into `RerankContext` and `VectorQuery`, skip with a reason (for example `policy`), and add the reason to `RerankReport`'s documented reasons.

### L1. SELF_HOST.md still names the semantic reranker as a model-invoke caller
- File: `apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md`, section "Quarry → Model Plane", paragraph 1 (branch hunk, kept as is): "intent classification, autoprompt, the semantic reranker, page titles, AI formats and answer synthesis through `model-invoke`".
- This contradicts option B. CROSS_PLANE_INTEGRATION.md in the same merge says rerank makes no Model Plane call. docs-contract passes only because its regex matches `model plane|model-plane|model-gateway|models:invoke` and not `model-invoke`.
- Fix: delete "the semantic reranker, " from that list. Optionally add `model-invoke` to the phrase regex in `docs-contract.test.mjs` ("no current document says the semantic reranker calls the Model Plane").

### L2. The Quarry standalone compose still treats SEMANTIC_RERANK as a Model Plane switch
- File: `apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml:99-101`. This is a branch-only file, so git saw no conflict. The comment reads "Model Plane switches, off ... Each one also needs MODEL_PLANE_URL" and lists `QUARRY_EDGE__SEMANTIC_RERANK`. After option B, rerank needs DATA_PLANE_URL plus the cross-plane tokens. The file also has no `RERANK_DEADLINE_MS` / `RERANK_SHADOW` and none of the four QB-T17 switches (the binary defaults them to false, so this is harmless).
- Fix: move SEMANTIC_RERANK under its own "Data Plane rerank (option B)" comment, and add the four QB-T17 switches at `0` so they match the Ingestion compose.

### L3. The token manifest still ties SEMANTIC_RERANK to model-invoke
- File: `crates/quarry-runtime/contracts/cross-plane-token-requests.json`, row `model-invoke`, `feature_flags` includes `QUARRY_EDGE__SEMANTIC_RERANK`. This came from live (the snapshot carries the same value) and is not a merge loss.
- Fix (optional, its own commit): remove the flag from that row, then regenerate both `.sha256` pins, the `zdr-contract-sync.test.sh` GOLDENS digest and the Auth Core copy, as `CROSS_PLANE_INTEGRATION.md` "Changing a row" describes.

### L4. Raw error text kept at 10 rerank/vector log sites (exempted, not converted)
- Files: `crates/quarry-runtime/src/rerank.rs` (4), `vector_index.rs` (4), `data_plane_http.rs` (2).
- The branch's rule (WS4.E/F, `tests/log_field_discipline.rs` R2) converts raw `error = %e` to `error_class`. Its own conversions in `ModelPlaneSearchReranker` (rerank.rs) and `e.without_url()` (vector_index.rs) were dropped together with the code live removed. The merge kept live's `error = %error` with `// zdr-log-ok:` reasons instead. I checked the reasons and they hold: `rerank()` returns `skipped` for zdr, sovereign and tenant before any call, `retrieve()` skips ZDR before `send()`, and the returned errors are fixed URL-free strings. Only `hybrid.rs` was converted (`error_class = error.code.wire_name()`). The baseline (`tests/fixtures/log-field-baseline.json`) holds only `browser_driver.rs::R1`, so any site I missed fails the ratchet.
- Fix: none required. Build must run `cargo test -p quarry-edge --test log_field_discipline`. If you want the branch's stricter style, use `error_class = crate::zdr_log::class(&error)` and drop the exemptions.

### L5. With the compose defaults, every boot logs a WARN about RERANK_SHADOW
- File: `apps/Ingestion Plane/docker-compose.yml` (live hunk, kept): `QUARRY_EDGE__RERANK_SHADOW` defaults to `1` while `QUARRY_EDGE__SEMANTIC_RERANK` defaults to `0`. `main.rs` (`if cfg.rerank_shadow && !cfg.semantic_rerank`) then warns "has no effect" on every start. This is live's design, not a merge loss.
- Fix (optional): leave it as is, or set the shadow default to 0 until the rerank is enabled. In that case update RUNBOOKS.md and the SELF_HOST.md table in the same change, because docs-contract pins docs and compose to agree.

### L6. Live's new spec fields use `nullable: true` in an OpenAPI 3.1 file
- File: `docs/openapi.yaml`: `SearchRequest.zdr`, `SearchResponse.score_kind`, `RerankReport.reason`, `RerankReport.profile`. OpenAPI 3.1 ignores `nullable`. The branch's convention (OPENAPI-NULL, `openapi-no-bare-null.test.mjs`) is `type: [x, 'null']`. This does not contradict the decision, which names only `ProfileCookieView.value` (kept as `[string, 'null']`), but generated models will treat these fields as non-null.
- Fix: rewrite the four fields as type arrays (`type: [boolean, 'null']`, `type: [string, 'null']` with `null` in the enum for `score_kind`) before the H1 regeneration.

### L7. The SPA error-catch ratchet was widened to admit live's raw-message sites
- File: `apps/Frontend Plane/verevonv3/src/shared/i18n/error-catch-sites.test.ts` (staged). Adds `demo/ScenePage.tsx` 2, `demo/install.ts` 1 and `AgentRunConsole.tsx` 1, and raises `shared/api/chat-client.ts` from 5 to 6. The new chat-client site (`streamConnectionFailure`, about line 1786) passes `err.message` only for ZDR codes. `AgentRunConsole` and `use-chat-controller` render those codes through `zdrRefusalPresentation`, so the raw text is not shown for them.
- Fix: none required. Record it as a widened ratchet in the next SPA i18n round.

### L8. Small text inaccuracies from the merge
- `src/shared/api/http.ts`, extractError doc comment: "it is returned only when present, so every other error keeps its exact shape". After the merge `hints.feature` is always set (null when absent). Fix the sentence.
- `docs/CROSS_PLANE_INTEGRATION.md`, ZDR section: "the seed then goes to the Data Plane without a posture, so find-similar is not ZDR-gated". Live's `VectorQuery` now carries `zdr` and `retrieve()` skips a ZDR query. The sentence was already stale in both parents. Fix it.
- `src/features/dashboard/home/browser-omnibox.ts` (resolved to the branch version): live's demo guard is satisfied because no build makes a third-party URL. But `resolveOmniboxIntent` is never passed `demo: import.meta.env.VITE_DEMO === '1'` (`BrowserChrome.tsx:967`, `OmniboxField.tsx:126`), so a demo build with `VITE_BROWSER_OMNIBOX_SEARCH=1` would try a Verevon search. `vite.demo.config.ts` does not set that flag, so this is informational only.

### L9. Two zdr-contract-sync failures caused by CRLF working copies (not the merge)
- `crates/quarry-runtime/contracts/telemetry-field-contract.json.sha256` and `apps/Data Plane v2/services/usage-ledger-rs/tests/fixtures/usage-envelopes.v2.json.sha256` are `i/lf w/crlf`. The script reads a trailing CR, so it reports 106 passed, 2 failed (I reproduced this). The digests are equal.
- Fix: add `*.sha256 text eol=lf` to `.gitattributes` (or check out these two files again). CI on Linux is not affected.

## The 24 conflicted files, one by one (verdict on hunk preservation)

| File | Live hunks in merge | Branch hunks in merge | Notes |
|---|---|---|---|
| auth-core `.../cross-plane-token-requests.json.sha256` | n/a (regenerated) | n/a (regenerated) | `f73f86a2...` = sha256 of the merged JSON (checked), LF, same bytes as the Quarry copy. No stale digest (`30ec31e3`, `40bd3586`) is left outside `.plan-work`. |
| Quarry `contracts/cross-plane-token-requests.json.sha256` | same | same | same |
| verevonv3 `browser-omnibox.ts` | intent kept (no third-party URL) | all | = HEAD. See L8. |
| verevonv3 `shared/api/http.ts` | all (`feature` moved into hints) | all | No 4-arg positional `ApiError(…, 'feature')` remains in src or tests. |
| verevonv3 `shared/api/sse.ts` | effect kept through `apiErrorFromResponse` | all | = HEAD |
| verevonv3 `shared/i18n/errors.ts` | all (catalogue check in `describe()`) | all | `translateApiError` delegates to `describe`; `LocalizedError` still wins. |
| verevonv3 `vite-env.d.ts` | all | all | |
| quarry-edge `config.rs` | all | all except one assert, retyped to `!cfg.page_title_enrich` | |
| quarry-edge `lib.rs` | all | all | |
| quarry-edge `main.rs` | all | all | Gate wiring checked: every MP caller reads `mp`/`state.model_plane` (audio, extract, suggest, answer, page title, autoprompt, intent). Reranker is the outermost SearchProvider, so `search_with_report`/`rerank_epoch` are not lost by a wrapper. |
| quarry-edge `search_routes.rs` | all | all except `effective_zdr(req.zdr)`, changed to `.unwrap_or(false)` | Tombstone wrapper and `ZDR_REQUIRED` agree. `policy_downgrades` is set in all four constructors and the live test. |
| quarry-runtime `hybrid.rs` | all | the error_class rule applied to live's line | |
| quarry-runtime `rerank.rs` | all + 4 exemptions | 2 log lines dropped with the removed code | Deliberate (option B). See L4. |
| quarry-runtime `vector_index.rs` | all + 4 exemptions | `without_url()` dropped with the removed body | Deliberate. Returned errors are URL-free. |
| docs `CROSS_PLANE_AUTH.md` | all (rows) | "(D16, QB-T17)" and "every model-gateway call degrades" folded in | docs-contract row tests pass. |
| docs `CROSS_PLANE_INTEGRATION.md` | all | all (browser-grant row, P2 audience, REQUIRE_CONTRACT CI paragraph) | |
| docs `ONBOARDING_LIVE_CRAWL.md` | all | all (rewritten into one bullet) | |
| docs `RUNBOOKS.md` | all (90 lines) | all (1810 lines) | Identical union. |
| docs `SELF_HOST.md` | all | all | See L1. |
| docs `openapi.yaml` | all | all | Guards pass (internal-guard, no-bare-null, route-coverage, challenge/scrape parity). See H1, L6. |
| lab `evals/Cargo.toml` | all | all | |
| `scripts/tests/docs-contract.test.mjs` | all (assertions live) | the `parseYamlSubset` whole-file checks kept; `pending` markers removed on purpose | 19/19 pass. |
| `apps/Ingestion Plane/docker-compose.yml` | all (QB-T17 switches, option-B rerank block, deadline, shadow) | all (MODEL_PLANE_URL opt-in, comment) | Branch's `SEMANTIC_RERANK_MODEL` removed, as option B requires. See L5. |
| `scripts/tests/zdr-contract-sync.test.sh` | all GOLDENS/VENDORED/CONSUMERS rows + `ZDR_REFUSALS_CATALOGUE` env | all rows + header paragraph | One digest row for the manifest (f73f86a2). No duplicate rows. Z-05b CONSUMERS row is listed once. |

## Spot-check: files both sides changed, merged by git without a conflict

| File | Result |
|---|---|
| quarry-edge `extract_routes.rs` | Live's `ai_formats_on` gate sits inside the branch's `extract_run` wrapper, after the WS7.E URL gate. No clash. |
| quarry-edge `answer_routes.rs` | Live's 501 hint and test call `answer()`, which is now the branch's tombstone wrapper. No clash. |
| quarry-edge `state.rs`, `routes.rs`, `test_support.rs`, `tests/support/mod.rs` | `model_plane` field added on both struct literal paths. Compiles (the notes record `cargo check` rc 0). |
| quarry-edge `tests/zdr_claim_floor.rs` | Live's `model_audio: true` for the posture tests. Branch changes are elsewhere. No clash. |
| quarry-runtime `serp.rs` | New `search_with_report`/`rerank_epoch` defaults. The only forwarding wrapper is `RerankingSearchProvider` (outermost). No branch-added wrapper drops the report. |
| quarry-runtime `smart_router.rs` | `dedupe_key` moved to `fusion.rs` by live. The branch's edits do not touch it. |
| quarry-runtime `token_request_contract.rs` | Live's `VectorQuery`/`VectorOutcome` test and the branch's browser-grant rows coexist. `EXPECTED_REQUESTS` in `service_tokens.rs` lists all 7 rows. |
| verevonv3 `App.tsx` | Live's scene/chat routes and the branch's fleet flag are in different places. No clash. |
| verevonv3 `CoreSidebar.tsx`, `ChatSidebarOrganizer.tsx`, `CoreNavbar.tsx` | Live adds `ChatWorkLinks` and BASE_URL. The branch converts catch sites to `translateApiError`. No clash. |
| verevonv3 `DashboardComposer.tsx`, `KnowledgeComposer.tsx` | Live's work-period picker and example hosts are separate from the branch's `LocalizedError` edits. No `vg.no` placeholder is left in that file. |
| `zdr-refusals.ts` / `.json` / `.test.ts` | Byte-identical on both sides (vendored from model-plane `5ae7fe13`). |
| `.github/workflows/verevonv3-ci.yml` | Live's `demo-product-build` job and the branch's SDK gate are both kept. See H1. |

## Owner decisions of 2026-10-28: default check

- DATAPLANE-OPTION-B: flags-off compose holds (the four flags are 0, MODEL_PLANE_URL is empty). See M1 for the "lands later" wording.
- X-10 / ROBOTS-RFC: `CRAWL_ROBOTS=observe`, `ROBOTS_UNREACHABLE=disallow`. Live does not touch these.
- X-12 / TAKEOVER-BUDGET: `BUDGET_ENFORCEMENT=shadow`. Untouched.
- D16 Q2: `profile_owner_enforce` defaults to Observe and the user_private default is untouched. Live has no profile changes.
- X-19, X-9, OPENAPI-INTERNAL, BILLING-IDEMPOTENCY: live does not touch these. Live's spec additions are public `/v1/search` fields, not internal routes, and the internal-guard test passes.
- ADR-6 / X-20: `SCRAPE_ACTIONS=off` in both composes.
- MP-GATEWAY-AUTH: live gives the verevonv3 gateway no Model Plane credential. Gateway search uses the user's `quarry` audience token, so the new `ZDR_REQUIRED` (service callers only) does not hit the SPA. execution-core's `web_tools.rs` and model-gateway's `quarry.rs` already send `zdr`.
- OPENAPI-NULL: `ProfileCookieView.value` is `type: [string, 'null']`. See L6 for the new fields.

## Test numbers

`BUILD.md` was not present in `.plan-work/integ-20261028/` when this review ran, so there are no Build-phase numbers. Recorded or run:

- Run by me (read-only): `node --test` on all six `Quarry-v2/scripts/tests/*.test.mjs`: docs-contract 19/19, openapi-challenge-parity 8/8, openapi-internal-guard 9/9, openapi-no-bare-null 2/2, openapi-route-coverage 10/10, openapi-scrape-parity 5/5. `bash scripts/tests/zdr-contract-sync.test.sh`: 106 passed, 2 failed (CRLF, L9).
- From the group notes, not re-run: `browser-grant-contract.test.mjs` 32/32. Vitest on `src/shared/api`, `src/shared/i18n` and the omnibox: 52 files pass, 0 failing tests, and 1 file fails to load (`quarry-client.test.ts`, which needs the generated SDK). `tsc -b` has 33 errors, equal to the quarryv2-next baseline. `cargo check -p quarry-edge -p quarry-runtime --all-targets` rc 0, and `cargo check -p quarry-evals --bins` rc 0.
- Still to run in Build: `log_field_discipline` (L4), `rerank_behavior`, `web_policy_routes`, the docs-contract Rust side (`token_request_contract`, `service_tokens`), the `cargo test --workspace` lane, the auth-core manifest spec, and spa-verify's SDK gate after the H1 fix.
