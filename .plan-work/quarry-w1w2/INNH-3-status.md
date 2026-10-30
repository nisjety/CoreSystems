# INNH-3: Structured extract returns 501 on live: document the enablement path and make the 501 hint exact

Status: not started
Lane: A (scrape/extract). Plan home: config + docs. Estimate: 1 d.
Verified by reading quarryv2-next. Nothing was built or run.

## What exists
The 501 exists, but its hint and the audit's "enablement path" are both WRONG, and the real path is not available in production.

The 501 (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs:245-254`): when `req.schema.is_some()` and the per-request runner is None, returns NOT_IMPLEMENTED with `ApiError::new("UNSUPPORTED", "structured extraction requires a Model Plane").with_hint("set MODEL_PLANE_URL, or omit `schema` to receive markdown")`. Wire body = {error, code:"UNSUPPORTED", hint} (api_error.rs:17-30 also has window, retry_after_seconds, next_actions; no `details`). Runner is None only when `state.model_plane_url` is None/empty or ModelPlaneClient::new fails (228-243). The 501 path has NO test.

What happens when you follow the hint (verified in code, not run):
1. There is NO `QUARRY_EDGE__AI_FORMATS` switch. The brief, the audit and plan WS16.S6 name it; grep finds it only in `scripts/tests/docs-contract.test.mjs:230` (pending) and plan prose. config.rs has no such field; `docker-compose.yml:181-184` says AI extract formats "have no switch: they run whenever MODEL_PLANE_URL is set (QB-T17, still pending)".
2. Setting `MODEL_PLANE_URL` (compose passes `QUARRY_EDGE__MODEL_PLANE_URL: ${MODEL_PLANE_URL:-}` at docker-compose.yml:180; blank is unset, config.rs:309-329, tests 433-491) removes the 501, but main.rs:405-412 then refuses to boot unless `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` is set (or cross_plane_auth_dev_bypass).
3. Each model call then mints manifest row `model-invoke` (mp_client.rs:121). That row is `granted:false` in `crates/quarry-runtime/contracts/cross-plane-token-requests.json` and the registry gives quarry-edge no model-gateway audience; the gateway also refuses service principals on /v1/invoke (docs/SELF_HOST.md:95-97 citing model-gateway auth.rs; auth.rs:1309 allows only /v1/ai/chat and /v1/ai/embeddings). Result: ErrorCode::Forbidden (mp_client.rs:179-201), which extract turns into a PER-ITEM `status:"error"` inside a 200 (extract_routes.rs:353-364), AFTER paying for the page fetch of every URL (up to 25, sequential).
4. The only way it works today is dev: static `QUARRY_EDGE__MODEL_PLANE_TOKEN` bearer (config.rs:86-87 "Development-only") against a gateway that accepts it. No production enablement path until WS16.S1/S2/S3 and decision X-7 (DR-15.1); all unbuilt (W2-closeout.md:122).

Docs today: RUNBOOKS.md has no extract/AI-format enablement section (extract appears only at 611 "WS5.B deferral" and 1225 "web policy"); SELF_HOST.md:95-110 documents page-title enrichment only; openapi.yaml:1309 says only "Structured extraction requires a Model Plane"; sdks/python/docs/ExtractApi.md:80 and sdks/typescript/docs/ExtractApi*.md:75 copy it; docker-compose.yml:181-184 and deploy/compose/README.md:94-95 describe the switches. The pending QB-T17 test text (docs-contract.test.mjs:22-23) lists AI_FORMATS among "seven switches", four of which do not exist.

## The gap (config + docs + a small code change)
1. Text: replace the hint with the truth for the no-Model-Plane case: error "structured extraction needs the Model Plane lane, which is not configured on this deployment", hint "omit `schema` and `prompt` to receive markdown. Operators: this needs QUARRY_EDGE__MODEL_PLANE_URL AND a granted model lane (see docs/RUNBOOKS.md, section Structured extract enablement); setting the URL alone is not enough", and `next_actions: ["retry without schema to receive markdown", "see RUNBOOKS Structured extract enablement"]` (ApiError already serializes next_actions). Never name a variable that does not exist in config.rs.
2. Behaviour (small, additive): a Model Plane `Forbidden` on the FIRST model call short-circuits the batch like the existing RateLimited short-circuit (extract_routes.rs:312-315, 345-352): return 501 UNSUPPORTED with hint "the Model Plane refused this service (lane not granted)" instead of N per-item errors, ideally BEFORE fetching remaining URLs (model call for URL 1, then stop). Stops wasted fetches and metering of failed items.
3. Docs: new RUNBOOKS.md section "Structured extract and AI formats: enablement" with three real states (a) no MODEL_PLANE_URL -> 501; (b) URL set but lane ungranted -> 501 after fix (today 200 with item errors); (c) dev static bearer works; plus the explicit statement that there is no production path until WS16.S1-S3 + X-7. Update SELF_HOST.md env table, openapi.yaml 501 description (1309) with both variants, CONTRACTS.md, compose comment (181-184), deploy/compose/README.md:94-95; correct the audit/brief claim that `QUARRY_EDGE__AI_FORMATS` gates extract. If item 1 creates that flag, document it separately as gating scrape formats only (or both, per the item-2 owner decision).
4. Tell the Innhenting session (their UI reads Quarry's 501 text verbatim) that the live 501 is not unlockable by a flag today.

## Files
`crates/quarry-edge/src/extract_routes.rs` (245-254, 345-352 area, tests); `crates/quarry-edge/src/api_error.rs` (only if a helper is wanted); `docs/RUNBOOKS.md` (new section); `docs/SELF_HOST.md` (95-110); `docs/openapi.yaml` (1309; 501 variants); `docs/CONTRACTS.md`; `apps/Ingestion Plane/docker-compose.yml` (181-184 comment); `deploy/compose/README.md` (94-95); `scripts/tests/docs-contract.test.mjs` (QB_T17 text; add env-var-exists test); `sdks/python/docs/ExtractApi.md`, `sdks/typescript/docs/ExtractApi*.md` (text only, optional; regeneration blocked).

## Flag
None. Wording, docs and one error-mapping change. Status stays 501 for the no-Model-Plane case. Must not touch any live flag; `QUARRY_EDGE__MODEL_PLANE_URL` stays unset on live.

## Contract change
501 body (existing ApiError shape, no new fields): `{"error":"structured extraction needs the Model Plane lane, which is not configured on this deployment","code":"UNSUPPORTED","hint":"omit `schema` and `prompt` to receive cleaned markdown; operators: see docs/RUNBOOKS.md 'Structured extract and AI formats: enablement'","next_actions":["retry without schema to receive markdown"]}`.
NEW second 501 variant (behaviour change for Forbidden-only batches, previously 200 with per-item `status:error`): same envelope, hint "the Model Plane refused this service; the extraction lane is not granted". openapi.yaml documents both with an example each. No change to 200/400/429 shapes. If the Forbidden short-circuit is rejected, ship (1) and (3) only and add a per-item stable error text/code to ExtractItem instead.

## Tests to add
extract_routes.rs unit tests: (a) schema set, model_plane_url None -> 501, body keys exactly {code,error,hint,next_actions}, hint contains no env var absent from config.rs, and the StubDriver recorded ZERO fetches (add a fetch counter to the stub if test_support lacks one); (b) prompt set (after item 2) and no Model Plane -> same 501; (c) wiremock Model Plane answers 403 on /v1/invoke: request returns 501 (new variant) after exactly ONE page fetch and ONE model call, metering recorder saw no `quarry.extract.url` for failed items; (d) wiremock 200: normal ok path; (e) wiremock 500 on URL 1 and 200 on URL 2: stays per-item (isolation preserved); (f) blank `QUARRY_EDGE__MODEL_PLANE_URL=""` still yields the no-runner 501 (config.rs tests 433-491 cover blank-to-None; reference, do not duplicate).
Node: in `scripts/tests/docs-contract.test.mjs` check that every `QUARRY_EDGE__[A-Z_]+` named in RUNBOOKS/SELF_HOST/openapi/compose comments exists in config.rs or a documented env read (the guard that would have caught the phantom AI_FORMATS); keep QB-T17 pending but fix its description.
Run: `cargo test -p quarry-edge extract_routes`; `node --test scripts/tests/docs-contract.test.mjs`.

## Risks
- Ops/security: the current hint tells an operator to set MODEL_PLANE_URL; doing that on a shared stack today flips every schema extract from a clean 501 to 200-with-errors after paid fetches and (if the lane were later granted without item 2's ZDR refusal) would send ZDR org page text to the Model Plane. The hint fix removes the foot-gun, so it is a safety fix.
- The Forbidden short-circuit changes status for batches that were failing anyway; keep it request-level only for Forbidden (not DriverFailed/timeouts, which stay per-item; per-source isolation is the route contract, extract_routes.rs:4-6).
- Do not echo the Model Plane URL, host or token detail in the error (hints are shown to end users by the gateway/UI).
- Docs: the audit at commit 14299e659 and the brief both assert a flag that does not exist; the new RUNBOOKS text must say so plainly so the Innhenting UI does not tell users to set it.

## Commits
(none yet)

## Tests (counts)
(none yet)

## Not verified
Code reading only; nothing built or run. The 403-from-gateway path (items 3 and 4 of "what happens") is derived from SELF_HOST.md and auth.rs reading, not exercised.
