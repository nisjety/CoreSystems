# INNH-8: Search answer provider switch (include_answer returns answer null; /v1/answer 501)

Status: not started
Lane: D (design/config). Plan home: none (config + docs, maybe small code). Estimate: 8 d total, of which 8a (Quarry side) about 2 d and 8b (cross-plane) about 5-7 d across three planes.
Verified by reading quarryv2-next. Nothing was built or run. Paths under `apps/Ingestion Plane/Quarry-v2` unless stated.

## What exists
WIRED CODE, ALREADY BUILT:
- `crates/quarry-edge/src/answer_routes.rs`: POST /v1/answer (:48) and POST /v1/answer/stream (:169). No pipeline gives 501 {error, code:UNSUPPORTED, hint} (:88-104, :197-214). Success meters ANSWER_SYNTH (:125-144). ZDR tombstones, ZDR cache skip on the stream route.
- `crates/quarry-edge/src/search_routes.rs`: include_answer is a request field (:90-92). On a cache miss it calls state.answer_pipeline.p.answer(..) with top_k 5 (:758-782). With no pipeline or on error the result is (None, None) and only a tracing::warn is logged (:778, :773). Response carries answer null with no reason. The null result is also written to the search cache (:792-803).
- `crates/quarry-edge/src/main.rs:1457-1498`: AnswerPipeline built only when BOTH `search` is Some AND cfg.model_plane_url non-empty; else logs "answer pipeline disabled (search or model_plane_url unset)" (:1494). build_model_client :1274-1290 uses the cross_plane_tokens provider, or a static bearer only with cross_plane_auth_dev_bypass.
- `crates/quarry-runtime/src/answer.rs` (AnswerPipeline, prepare/answer, citations with outcome, structured figures), `ai_formats.rs` query_inner/query_for_org (:205-232, model None so gateway picks), `mp_client.rs` invoke_with_org (:116-131) POSTs {base}/v1/invoke (:166) using manifest row `model-invoke`.
- Config: config.rs:84 model_plane_url (blank normalised to None :309-329), :87 model_plane_token.
- Compose `apps/Ingestion Plane/docker-compose.yml:174-184`: QUARRY_EDGE__MODEL_PLANE_URL=${MODEL_PLANE_URL:-} (empty = off). The comment says answer synthesis, AI formats, search suggest and audio have NO switch (QB-T17 pending; model_plane_gate.rs does not exist on this branch).
- Tests today: zdr_claim_floor.rs:622-672 and :1468-1490, zdr_tombstone.rs:761-897 (answer 501, failure, stream), web_policy_routes.rs:414, 2 unit tests in answer_routes.rs:368-393. Nothing asserts include_answer end to end, nothing asserts the failed-synthesis cache entry.
- Docs: TAVILY_PARITY.md:95-100, SELF_HOST.md:53 and :102, CROSS_PLANE_AUTH.md:12, ZDR_MATRIX.md:28, BILLING.md:53 and :138. `docs/openapi.yaml` has NO /v1/answer or /v1/answer/stream path: both in `scripts/openapi-route-allowlist.json:46-53` as "not yet described". SearchRequest.include_answer (openapi.yaml:6122) and SearchResponse.answer/citations (:6183) exist with no status or reason field.

WHAT THE "SWITCH" REALLY IS: not only MODEL_PLANE_URL. Four independent gates, at least three closed today:
1. QUARRY_EDGE__MODEL_PLANE_URL set (compose default empty).
2. A SearchProvider is configured (live search works, so open).
3. Control Plane registry grants quarry-edge the `model-gateway` audience. `apps/Control Plane/config/plane-service-principals.json:380-408` gives quarry-edge only browser-broker and data-plane. Quarry's manifest `crates/quarry-runtime/contracts/cross-plane-token-requests.json:40-80` lists model-invoke, model-stream, model-audio with granted:false; CROSS_PLANE_AUTH.md:12 says the mint is refused (decision D16, owner QB-T17). auth-core's copy: `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json`.
4. The Model Plane must accept the call. `apps/Model Plane/rust/services/model-gateway/src/auth.rs:1283-1320` (authorize_principal_route) lets a SERVICE principal reach only POST /v1/ai/chat and /v1/ai/embeddings (plus two internal routes). Quarry calls /v1/invoke, /v1/invoke/stream, /v1/ai/speech, /v1/ai/transcribe, so a minted token would still get 403. Also http_routes.rs:8796-8800 (/v1/invoke) and :2308-2312 (/v1/ai/chat) require VerifiedInferenceBearer, which for a service is a second header `x-inference-authorization` (aud inference-core, same sub/service_id/org, scope models:invoke) checked at auth.rs:969-990 and :1195-1260. Quarry's mp_client sends one Authorization header only. The scope strings look inconsistent (gateway models:invoke vs registry/auth-core inference:invoke); how that resolves is unverified.

The brief's flag `QUARRY_EDGE__AI_FORMATS=1` does not exist in the code (only in scripts/tests/docs-contract.test.mjs:230, a pending QB-T17 assertion). Current live behaviour: MODEL_PLANE_URL empty -> answer pipeline None, /v1/answer 501, include_answer answer null (silent).

## The gap
Two halves. Only the first is Quarry's alone.
8a. Quarry side (about 2 d):
- G1 Silent null. Add `answer_status` (and `answer_reason`) to /v1/search response; closed vocabulary: not_requested | ok | no_sources | unavailable_not_configured | unavailable_model_refused | unavailable_error.
- G2 The 501 hint is wrong/incomplete (says "configure a SearchProvider AND a Model Plane URL"). Name the exact missing gate from a boot-time AnswerPipelineStatus enum {Ready, NoSearchProvider, NoModelPlaneUrl, ModelClientInitFailed} stored on AppState. When the URL is set but the mint is refused (token_for_org returns Forbidden, service_tokens.rs:459) the error is a generic upstream failure; make it a typed 503/502 with reason model_plane_auth_refused, not a bare 501.
- G3 A failed synthesis is cached as answer null for the search TTL (search_routes.rs:792-803); after enabling the Model Plane include_answer queries keep returning null until TTL ends. Do not write the cache entry when synthesis was requested but failed, or key the cache on pipeline readiness.
- G4 include_answer synthesis is not metered; only SEARCH_QUERY with metadata include_answer:true (search_routes.rs:940-958). /v1/answer meters ANSWER_SYNTH. The include_answer path pays for a second search plus up to 5 page fetches plus one model call; meter ANSWER_SYNTH there too (not on cache hits).
- G5 Both answer routes need openapi.yaml entries, SDK regen and removal from openapi-route-allowlist.json (:46-53); add the 501/503 reason shape. Answer routes use the legacy ErrorBody {error, code, hint}, not the typed envelope; decide keep (document) or add `details.reason` additively.
- G6 Docs: a runbook section "Enable answer synthesis" listing the four gates in order with the live check; fix SELF_HOST.md:102, TAVILY_PARITY.md:95 (claim that MODEL_PLANE_URL alone enables it), and the stale compose comment.
- G7 Optional/recommended: `QUARRY_EDGE__ANSWER_SYNTHESIS` switch (default 0) in the pipeline-build condition at main.rs:1460 so setting MODEL_PLANE_URL for another consumer (autoprompt, rerank, titles) does not also turn on answer synthesis. Matching pending assertion docs-contract.test.mjs:223-236.
8b. Cross-plane enablement, NOT Quarry's alone (about 5-7 d across three planes, owner decision needed):
- Control Plane: add audiences model-gateway (scope models:invoke) and inference-core to quarry-edge in plane-service-principals.json with retentionByAudience model-gateway: zdr; update auth-core fixtures and contract specs.
- Model Plane: allow quarry-edge's route in authorize_principal_route, or move Quarry to /v1/ai/chat (non-persisting, honours `zdr`, supports structured_output_schema which also serves INNH-2, sets a model). Resolve models:invoke vs inference:invoke.
- Quarry: mp_client sends the second x-inference-authorization header (new manifest row), flips the model-invoke row to granted:true together with the registry grant, passes ZDR in the request (InvokeRequest mp_client.rs:32-40 has no zdr field; validate_input `let _ = zdr`, ai_formats.rs:302-309).

## Files
QUARRY (8a): `crates/quarry-edge/src/answer_routes.rs` (status-specific 501/503); `search_routes.rs` (answer_status field near :140-160, include_answer block :758-782, cache write :792-803, meter near :940); `main.rs` (:1457-1498 build and record AnswerPipelineStatus; optional gate); `state.rs` (:97-100); `config.rs` (optional field); `test_support.rs:75,130` and `tests/support/mod.rs:75` (AppState literals); `crates/quarry-runtime/src/answer.rs` (optionally typed reason); `docs/openapi.yaml`, `scripts/openapi-route-allowlist.json` (remove :46-53), `sdks/typescript` regenerated, `docs/CONTRACTS.md`, `docs/RUNBOOKS.md`, `docs/SELF_HOST.md`, `docs/TAVILY_PARITY.md`, `docs/BILLING.md`, `apps/Ingestion Plane/docker-compose.yml` (:174-184), `deploy/compose/docker-compose.yml:97-99`, `scripts/tests/docs-contract.test.mjs` (:223-236, only if G7 built).
CROSS-PLANE (8b, other owners): `apps/Control Plane/config/plane-service-principals.json`, `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` + contract specs; `apps/Model Plane/rust/services/model-gateway/src/auth.rs` (:1283-1320, :969-990), `http_routes.rs` (:136, :415); `crates/quarry-runtime/src/mp_client.rs`, `service_tokens.rs`, `contracts/cross-plane-token-requests.json`, `ai_formats.rs`. Gateway/SPA out of lane: the Innhenting session reads answer_status from the search response.

## Flag
No existing Quarry flag gates answer synthesis; today the only "flag" is QUARRY_EDGE__MODEL_PLANE_URL non-empty, which also switches on AI extract formats, search suggest and audio (QB-T17 gate absent). Proposed: `QUARRY_EDGE__ANSWER_SYNTHESIS` (default 0) in compose and EdgeConfig (`answer_synthesis: bool`, `#[serde(default)]`); config.rs falls back to ALL defaults if any field fails to parse, so add a parse test as D2-A did. Stay off and untouched: QUARRY_EDGE__AUTOPROMPT, SEMANTIC_RERANK, LLM_CLASSIFY_INTENT, PAGE_TITLE_ENRICH, ZERO_SAAS_SEARCH. LIVE RULE: do not set MODEL_PLANE_URL or any flag on the live stack; the 8a change is additive and dark by default.

## Contract change
openapi.yaml additions (additive).
1. POST /v1/search response, optional: `answer_status` enum [not_requested, ok, no_sources, unavailable_not_configured, unavailable_model_refused, unavailable_error] and `answer_reason` (short closed code, never provider text). `answer`/`citations` unchanged. include_answer false -> not_requested (or omitted).
2. POST /v1/answer: request {query, top_k?, country?, language?, zdr?}; 200 {query, answer, citations[{url,title,rank,provider,outcome?,structured?}], model, latency_ms, sources_used, sources_skipped, sources_snippet_fallback}. Errors: 400 empty query; 501 UNSUPPORTED {error, code, hint, reason in [no_search_provider, no_model_plane_url, model_client_init_failed]}; new 503 or 502 upstream-type for model_plane_auth_refused. Keep the legacy ErrorBody; document it in CONTRACTS.md (decide whether to add details.reason).
3. POST /v1/answer/stream: SSE frames `citations`, `delta`, `done {answer,cached}`, `error {message}`. Document as is.
4. ZDR: honored, bypasses answer cache; add the model-gateway ZDR lane note once 8b lands.
Enablement checklist for CONTRACTS.md and RUNBOOKS.md, in order: registry grant, model-gateway route allowance, manifest granted:true, MODEL_PLANE_URL, optional ANSWER_SYNTHESIS=1, recreate quarry-edge, check answer_status:ok.

## Tests to add
Rust, quarry-edge:
- `tests/answer_provider_status.rs` (new): router with answer_pipeline None and (a) no search, (b) search but no MP URL, (c) client init failure; assert /v1/answer 501 with exact reason and /v1/search include_answer answer null with answer_status unavailable_not_configured.
- Same file, wiremock Model Plane returning 403 for the mint: answer_status unavailable_model_refused and the typed error, not a 500. With 200 invoke: answer_status ok, answer, citations, model.
- Cache test: a failed synthesis is NOT cached (second request with the model healthy returns an answer).
- Usage test: include_answer synthesis meters ANSWER_SYNTH once, a cache hit does not, a ZDR run does not persist.
- ZDR: include_answer with zdr true skips cache and corpus write-back (extend zdr_claim_floor.rs near :622).
- Unit tests for AnswerPipelineStatus derivation (all four inputs) and the new config field parse (like blank_model_plane_url_is_unset).
Contract: openapi route allowlist test passes with /v1/answer documented and removed from allowlist; openapi-vs-router parity; SDK regen diff test; docs-contract.test.mjs un-pends the QB-T17 assertion for keys that now exist.
Cross-plane (8b, other owners): auth-core registry spec (quarry-edge may mint model-gateway models:invoke with lane zdr); model-gateway route test (quarry-edge service token reaches the allowed route, 403 elsewhere); Quarry mp_client wiremock test that the second delegated header is sent, plus the manifest/granted:true consistency test already in service_tokens.rs.
Live proof (needs approval, not in this lane): side-by-side stack with MODEL_PLANE_URL set, one include_answer query, answer_status ok.

## Risks
- ZDR: the Model Plane request carries no ZDR flag (mp_client.rs:32-40) and ai_formats.rs:302-309 discards zdr, relying on a contract. The registry lane for model-gateway must be `zdr` before enabling, otherwise a ZDR org's page text reaches a persistent lane. include_answer under ZDR must keep skipping the cache (search_routes.rs:730) and corpus write-back (answer.rs hook). /v1/answer under ZDR leaves a metadata-only tombstone (zdr_tombstone.rs); include_answer on /v1/search does not, so ZDR_MATRIX rows 'answer' and 'search' need a statement. search_suggest already has an open ZDR gap on the same hop (ZDR_MATRIX.md:27).
- Cost: one include_answer search does 2 provider searches (the route's search, then AnswerPipeline.prepare searches again), up to 5 page fetches (answer.rs MAX_SOURCE_CHARS 8k, MAX_TOTAL 40k chars, about 10k input tokens) and one model call. Unmetered today (G4). The failed-synthesis cache poisoning (G3) hides the switch being turned on.
- Security: the Model Plane route allowance for a service principal widens the gateway's service allowlist; keep to exact routes and models:invoke. SSRF/egress: the answer fetcher SimpleHttpMarkdownFetcher fetches arbitrary result URLs from the edge; confirm it goes through web policy and egress guard (answer candidate filter WS7.G is not done per RUNBOOKS.md:1230, :1242).
- Contract: the legacy ErrorBody differs from the typed envelope; any client parsing `error` as an object breaks if it changes, so only add fields.

## Commits
(none yet)

## Tests (counts)
(none yet)

## Not verified
Code reading only; Docker is off, nothing exercised. Unverified: the models:invoke vs inference:invoke scope mismatch in the deployed registry; whether the live model-gateway container is the build with these middleware rules.
