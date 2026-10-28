# D2-A-compose-flags: findings not fixed here (2026-10-28)

Unit: decision 3.12 DATAPLANE-OPTION-B (flags-off compose, MODEL_PLANE_URL opt-in, QB-T17 kept pending) and decision 3.8 ADR-6 (the scrape-actions line in the compose), list A of the owner's 2026-10-28 order. Option B is not landed (it lands later from main, per 3.12).

## 1. Callers that expect /v1/answer to degrade (the first step of the unit)

Model Plane status today: every Model Plane call is refused. The quarry-edge token mint has no model-gateway audience, so Auth Core answers 403 (cross-plane-token-requests.json notes), which service_tokens.rs:459 maps to Forbidden. So no caller can see a working synthesis today either.

| Caller | Path with MODEL_PLANE_URL empty (after this unit) | Before this unit (URL set, calls refused) | Effect |
|---|---|---|---|
| quarry-edge /v1/answer | 501 UNSUPPORTED (answer_routes.rs:88-100) | Sources are fetched, then the synthesis call fails on the mint refusal (Forbidden, service_tokens.rs:459) and answer.rs:607-622 has no degrade on that path | Error to error. No 200 degraded answer existed to lose. The only 200 without synthesis is the citations-only answer when no source text was fetched (answer.rs:611-622). |
| quarry-edge /v1/answer/stream | 501 JSON (answer_routes.rs:197-210) | 200 SSE: a `citations` frame, then an `error` frame (answer_routes.rs:322) | Citations no longer arrive. See item 2. |
| gateway POST /api/v1/search/answer/stream (search.rs:627) | Relays 501 (upstream.rs:1851-1863 passes any upstream status of 400 or more through) | Relays the SSE stream | The body is `{error:{code:"model_error", message:"Model gateway returned 501"}}`. The label is wrong for a Quarry 501. Not fixed: gateway change, outside this unit. |
| SPA streamSearchAnswer (src/shared/api/search-client.ts, DashboardSearchPanel.tsx:333-357) | readSseStream throws on !response.ok (sse.ts:60-65), so onError runs and the panel clears answerStreaming | Same handler on the error frame | Silent in both cases: the panel shows no answer and no error. Only the citations change (item 2). |
| gateway search (/api/v1/search/web with includeAnswer, search.rs:81, 259) and scripts/evidence/ws4a-deploy-probes.sh:143 | 200 without `answer` (search_routes.rs:778 `None => (None, None)`) | 200 without `answer` when synthesis fails (search_routes.rs:760-778) | No change. This is the degrade the callers expect. |
| quarry-edge /v1/search/suggest (search_routes.rs:1512-1522) | 200 empty | 200 empty when the call fails | No change. Already degrade-safe. |
| quarry-edge /v1/extract with no `schema` (extract_routes.rs:228) | Cleaned markdown | Cleaned markdown | No change. |
| quarry-edge /v1/extract with `schema` (extract_routes.rs:249) | 501 "structured extraction requires a Model Plane" | Structured call, refused | Error to error, by design. |
| quarry-edge /v1/audio (audio_routes.rs:92-103) | 501 UNSUPPORTED | Proxy call, refused | Error to error, by design (the deploy README already documents this). |

Nothing outside the SPA and the gateway calls quarry-edge /v1/answer in the tracked code (the Model Plane, Application Plane and Data Plane have no caller). One planned caller exists on paper: apps/Ingestion Plane/autocomplete-core/docs/DESIGN.md says autocomplete-core will summarise sources through /v1/answer. It has no code yet, so it will meet the 501 once it is built. The decision text said "/v1/answer changes from a degraded answer to 501". That is not what happens: the degraded 200 answer never existed while the Model Plane refused every call. The recorded change is error to a different error, plus the stream item below.

## 2. The stream citations the SPA merges are no longer sent (not fixed)

With a Model Plane configured but refusing, /v1/answer/stream sends the `citations` frame before synthesis fails. DashboardSearchPanel.tsx:346-348 merges those citations into the web results (mergeCitations). With MODEL_PLANE_URL empty the route returns 501 before any frame, so answer-only citations are not merged. The effect is small: the citations are a subset of the search results the panel already shows. Fixing it needs an edge mode that returns the citations without synthesis when no Model Plane is wired. That is a feature decision, not a trivial change. Owner or frontend decision.

## 3. Four Model Plane features have no switch (QB-T17, still pending)

Answer synthesis (main.rs:1460-1495), AI extract formats (extract_routes.rs:228), search suggest (search_routes.rs:1518) and audio (audio_routes.rs:92) run whenever QUARRY_EDGE__MODEL_PLANE_URL is set. Setting the URL therefore turns them on, with no flag. The compose comment says so. The QB-T17 gate (crates/quarry-edge/src/model_plane_gate.rs) is not on this branch (STATUS-2026-10-24.md section 7.4). The docs-contract QB-T17 assertion still pins seven switches, four of which do not exist, so it stays pending with its real reason (scripts/tests/docs-contract.test.mjs).

## 4. Empty-string trap (fixed at the source)

`${MODEL_PLANE_URL:-}` makes compose set QUARRY_EDGE__MODEL_PLANE_URL to an empty string. Most consumers already filter empties (main.rs:1302, 1340, 1428, 1462, 1654; search_routes.rs:1518; extract_routes.rs:229; audio_routes.rs:93). The cross-plane boot gate tests `is_some()` (main.rs:406-412) and does not filter. If an empty value reaches the gate as Some(""), the gate demands QUARRY_EDGE__QUARRY_SERVICE_API_KEY. The Ingestion compose is not hit, because its data-plane URL is always set. The standalone compose would be hit: it had no Model Plane wiring, and its service key defaults to empty, so the edge would refuse to boot. (Whether the config crate keeps an empty value at all is not tested here; the normalisation below covers both cases.) config.rs now normalises an empty or blank URL to None at load (blank_model_plane_url_is_unset, called from EdgeConfig::from_env, the only production loader, main.rs:209). Four unit tests cover it (see the progress file).

## 5. The compose's "0" values and the defaults fallback

EdgeConfig::from_env falls back to all-defaults when any field fails to deserialize (config.rs `unwrap_or_else`). A bad value for one switch would silently drop the whole configuration. The new test parses the four switches the compose now sets to "0" through the same config-crate source, so this path is covered for them. Any future switch that is added to the compose should get the same check.

## 6. Root docker-compose.yml: legacy quarry-edge block (not changed)

The repo-root docker-compose.yml defines a quarry-edge service under the x-legacy-service anchor (lines 910-975). It sets names that EdgeConfig never reads, for example QUARRY_EDGE__MODEL_PLANE_BASE_URL, QUARRY_EDGE__MODEL_PLANE_BEARER_TOKEN and QUARRY_EDGE__DATA_PLANE_DOCUMENTS_BASE_URL (the fields are model_plane_url and data_plane_url). It does set QUARRY_EDGE__LLM_CLASSIFY_INTENT from QUARRY_LLM_CLASSIFY_INTENT, default false, which the binary does read. So that stack never configures a Model Plane URL. Owner decision: retire the legacy block or fix its names. Not in this unit's two compose files.

## 7. ADR-6 plan text still describes the old gates (not done)

The ruling is ratified (plan section "Owner decisions of 2026-10-28", the ADR-6 / X-20 row). The plan's X-20 text still says "User principals only until scrape:interact is granted" and "W2 enable" (docs/plans/quarry-browser-build-plan-2026-10-14.md:386 and :514), and the ledger at :3740 is a plain env table. Rewriting the gates in the plan is list A item 8 of the decision round. It is not in this unit's spec (the spec asks for the compose line only), so it is left for the owner or the docs unit.

## 8. Stack session (not done here, by design)

Changing the compose defaults changes a running stack only when quarry-edge is recreated. Decision 3.12 step 4 (recreate quarry-edge, confirm that no Model Plane mints happen) is a stack action. No live stack was touched. The stack session should also confirm the standalone's new MODEL_PLANE_URL opt-in with a real service key, because the edge will refuse to boot with a URL and no QUARRY_SERVICE_API_KEY (that is the intended gate).

## 9. The Ingestion compose needs gitignored env files (validation note)

`docker compose config` on apps/Ingestion Plane/docker-compose.yml fails in this worktree: the per-service env files (Quarry-v2/.env, imports-core/.env, integration-corev2/.env, shipping-core/.env, finspo-core/.env, autocomplete-core/.env) are gitignored and exist only in the main checkout. The validation therefore ran on a scratch copy of the two compose files with empty stand-ins for those files and placeholder values for the required (:?) variables. The copy, the placeholders and the rendered switch values are in the progress file.

## 10. Test counts that did not change

Quarry scripts/tests: 44 tests, 32 pass, 0 fail, 12 todo (the same counts as before the unit). The 12 todos are 11 option-B assertions and the QB-T17 assertion. The OPTION_B pending markers are untouched.
