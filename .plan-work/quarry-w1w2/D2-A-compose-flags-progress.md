# D2-A-compose-flags progress (2026-10-28)

Unit: decision 3.12 DATAPLANE-OPTION-B (flags-off Quarry compose, MODEL_PLANE_URL opt-in, QB-T17 kept pending, option B not landed) and decision 3.8 ADR-6 (the scrape-actions line in both compose files). Owner order 2026-10-28, list A. Branch quarryv2-next, local commits only.

## Commits (local, branch quarryv2-next, no push)

| Commit | Subject |
|---|---|
| 8a8bf5c1b | fix(quarry-edge): read an empty MODEL_PLANE_URL as unset (config.rs, 4 tests) |
| 675903067 | fix(quarry-compose): flags-off Model Plane, MODEL_PLANE_URL opt-in, scrape actions off (both compose files) |
| 88fb4afe3 | test(docs-contract): QB-T17 todo states its real reason (assertions unchanged) |
| c457cdc51 | docs(quarry-compose): state the flags-off defaults the compose now sets |
| (this commit) | docs(plan-work): D2-A-compose-flags progress, depth and hand-over notes |

## What changed (summary of the four code and docs commits)

| File | Change |
|---|---|
| apps/Ingestion Plane/docker-compose.yml | QUARRY_EDGE__MODEL_PLANE_URL `${MODEL_PLANE_URL:-}` (was `http://model-gateway:8080`); AUTOPROMPT and SEMANTIC_RERANK default `0` (were `1`); LLM_CLASSIFY_INTENT and PAGE_TITLE_ENRICH added at `0`; QUARRY_EDGE__SCRAPE_ACTIONS `${QUARRY_EDGE_SCRAPE_ACTIONS:-off}` next to SCRAPE_CAPTURE; the comment block that said "both default ON" is rewritten |
| apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml | The same opt-in MODEL_PLANE_URL (previously not wired at all), the four switches at `0`, SCRAPE_ACTIONS `off` |
| apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs (CRLF) | `blank_model_plane_url_is_unset` (empty or blank URL becomes None), called from `EdgeConfig::from_env`; module `compose_switch_tests` with four tests |
| apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs | Only the QB-T17 todo reason text. The assertions are unchanged (not narrowed) |
| deploy/compose/README.md, deploy/compose/.env.example, docs/SELF_HOST.md, docs/ONBOARDING_LIVE_CRAWL.md | Compose-default statements brought in line with the new defaults |
| .plan-work/quarry-w1w2/depth/D2-A-compose-flags.md | Findings not fixed (caller audit, trap notes, stack session) |

Not touched, by design: option B (the Data Plane rerank), the ADR-6 gate text in the plan, the root docker-compose.yml legacy block, the live stack, anything under sdks/ or docs/openapi.yaml.

## Checks run (what was actually seen)

- Quarry node: `node --test scripts/tests/*.test.mjs` from apps/Ingestion Plane/Quarry-v2, before and after the edits: 44 tests, 32 pass, 0 fail, 12 todo, rc=0 (after = before). The 12 todos are the 11 option-B assertions and the QB-T17 assertion. Log: scratchpad/D2-A-compose-flags/after1-quarry-node.log.
- Compose validation (under with-lock docker):
  - Standalone deploy/compose/docker-compose.yml: `docker compose -f docker-compose.yml config -q` rc=0.
  - Ingestion base, scratch copy: `config -q` rc=0; base plus docker-compose.production.yml: `config -q` rc=0. The worktree lacks the gitignored env files (env_file entries for Quarry-v2, imports-core, integration-corev2, shipping-core, finspo-core, autocomplete-core), so the two files were copied to scratchpad/D2-A-compose-flags/validate-ingestion with empty stand-ins and placeholder values for the required variables (exported to the process only; nothing written to the repo). Log: scratchpad/D2-A-compose-flags/check-compose-ingestion-1.log.
  - Rendered `quarry-edge` values (docker compose config --format json): MODEL_PLANE_URL "" when unset, "http://model-gateway:8080" when the test URL is set; AUTOPROMPT, SEMANTIC_RERANK, LLM_CLASSIFY_INTENT, PAGE_TITLE_ENRICH all "0"; SCRAPE_ACTIONS "off". The same holds for the production overlay and the standalone compose.
- Rust (qtest, docker lock): `cargo test -p quarry-edge --lib compose_switch`: 4 passed, 0 failed (log scratchpad/D2-A-compose-flags/qtest-config-1.log).
- Rust regression (qtest): `cargo test -p quarry-edge --lib`: 631 passed, 0 failed. `cargo check --workspace --all-targets`: Finished, rc=0 (log scratchpad/D2-A-compose-flags/qtest-regress-1.log).
- Root suite: `node --test scripts/tests/*.test.mjs` from the repo root (includes the grant-contract compose render, under the docker lock): 143 tests, 143 pass, 0 fail, 0 todo, rc=0, after the edits (log scratchpad/D2-A-compose-flags/root-node-after1.log). Same count as earlier today (D2-A-openapi-null-progress.md).

## Traps found

- An empty MODEL_PLANE_URL reaches the edge as an empty string. Most consumers filter empties; the cross-plane boot gate (main.rs:406-412) does not. Fixed at load in config.rs, so every consumer sees None.
- The compose renders env_file entries from gitignored files; a worktree cannot validate apps/Ingestion Plane/docker-compose.yml as-is (see the depth note, item 9).
- `${MODEL_PLANE_URL:-}` leaves the Ingestion data-plane URL untouched, so the boot gate is not hit there. The standalone compose has no data-plane URL, so it would be hit without the config fix.
- Four Model Plane features (answer synthesis, AI formats, search suggest, audio) still run whenever the URL is set. There is no switch for them. QB-T17 stays pending.
- The worktree has other agents' edits in main.rs, lib.rs, fixture_site.rs, step_receipts.rs (strays, not committed here) and in quarry-transform robots.rs (the robots unit). Commits stage only this unit's paths.

## Left open (details in depth/D2-A-compose-flags.md)

1. Four Model Plane features still run whenever the URL is set (QB-T17 gate absent).
2. The stream citations the SPA merges are no longer sent when the Model Plane is absent (a feature decision).
3. The gateway labels Quarry's 501 as "Model gateway returned 501" (model_error).
4. The root docker-compose.yml quarry-edge block uses names the binary never reads (legacy).
5. The plan's X-20 gate text (lines 386, 514) still describes the old gates; the ADR-6 ruling is recorded, the rewrite is not done (outside this unit's spec).
6. Stack session: recreate quarry-edge and confirm no Model Plane mints (decision 3.12 step 4). Not done; no live stack touched.
7. Option B (Data Plane rerank) not landed, by decision.
