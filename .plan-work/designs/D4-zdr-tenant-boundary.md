# D4 zdr-tenant-boundary: close the gateway cache tenant/ZDR hole, publish Quarry's ZDR matrix, make telemetry ZDR-safe

Prepared 2026-10-15 against the LIVE tree `C:/dev/CoresSystem` (HEAD afb83928 + uncommitted work). Audit source: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Path tags: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked (exists only as uncommitted work), `[NEW]` would be created by this plan. Tags come from `tools/provenance.sh`. All paths are relative to the repo root unless they start with `GW/`, `QE/`, `QR/`, `QCO/`, `QCT/`, `MPX/`:

- `GW/` = `apps/Frontend Plane/verevonv3/apps/gateway/`
- `QE/` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/`
- `QR/` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/`
- `QCO/` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-core/`
- `QCT/` = `apps/Ingestion Plane/Quarry-v2/services/quarry-control/`
- `MPX/` = `apps/Model Plane/`

## 1. Items resolved and wave

| Item | Scope of this design | Wave | Effort (verified) |
|---|---|---|---|
| F-t2-003 phase 1 | Gateway search-cache + scrape-preview cache: org scope, ZDR admission, fail-closed, purge/incident evidence | W1, ship first | S |
| F-t2-003 phase 2 | Quarry per-feature ZDR matrix with refusal codes aligned to the Model Plane catalogue; metadata-only ZDR run tombstone + janitor | W1 (matrix) / W3 (tombstone) | M |
| F-t2-004 phase 1 | Remove known ZDR log/meter leaks, ZDR carrier + event filter, log-field lint (Quarry and gateway) | W1 | S |
| F-t2-004 phase 2 | Sampler, late-ZDR registry, shared helper, Go `quarryotel`: summarised in 4.12 only, designed in D10 (with F-t2-029) | W2 | M (not here) |
| ADR topic 4 (ZDR semantics) | Decision table with recommended defaults (section 3.3) | before any slice that changes a refusal | n/a |

Totals for the slices below: about 27 engineer-days (gateway hotfix S-1..S-3 4.75 d, telemetry phase 1 S-4..S-6 7 d, matrix and catalogue S-7..S-9 10 d, tombstone S-10 5 d). The hotfix (slices S-1, S-2) is independent of everything else and should ship alone and first.

## 2. Verified current state

### 2.1 Gateway search cache (F-t2-003 phase 1)

- `GW/src/domains/search.rs` [C] `search_web` (51-293). Keyword path: `filters_sig` (209-217), `cache::cache_key("search-web", [query, limit, include_answer, filters_sig])` (218-226), `state.cache.lookup(&key)` (228), `state.cache.store(&key, &cached_payload)` (285), stale fallback when Quarry errors (288-291). The file never mentions `zdr` and never calls `authorized_org_id` in `search_web` (only `search_suggestions` does, 552). **Confirmed: no org, no ZDR check, 4 h fresh / 24 h stale** (`GW/src/cache.rs` [C] 20-23).
- The cached value is `{mode, results, answer, citations}` (279-284). Each `results[]` item still carries Quarry's per-result `provider` string (`QR/src/serp.rs` [M] `SearchResult.provider`, line 32), and the local Tantivy provider names itself `tantivy_local` (`QR/src/local_index.rs` [C] 597). `answer` is non-null only when Quarry's org-scoped answer pipeline ran (`QE/src/search_routes.rs` [M] 807-813 passes `org_id: Some(claims.org_id.clone())`).
- Quarry itself is org-scoped: `build_search_options` takes the org from the verified JWT (`QE/src/search_routes.rs` 501-531), its own `SearchCache::key(org, query, params)` is `quarry:search:{org}:{blake3}` (`QE/src/cache.rs` [C] 121-127), and it skips cache reads under ZDR (775-782) and writes through `cacheable_search(zdr, &rerank)` (576-578, used at 831). The Tantivy corpus refuses empty-org writes and returns zero results for an org-less query (`QR/src/local_index.rs` module doc). **So a gateway key without org collapses Quarry's per-org answer for every org onto one entry: confirmed by code path, not exercised.**
- `GW/src/zdr.rs` [C] already has the right primitives: `request_zdr` (44-51: header OR body flag), `org_zdr_enabled` (67-120), which returns `true` (assume ZDR) when the org is empty (70-75) or org-core errors (105-112) and memoises a successful answer for 60 s (37, 78, 115-118). `GW/src/upstream.rs` [C] `authorized_org_id` (324-329) reads the live membership decision and returns empty for session-only flows.
- The gateway never forwards its ZDR decision on search: `post_quarry` (640-692) sends `x-user-id` + the audience bearer, and the payload built at 236-259 has no `zdr`. The same holds for the URL-fetch path (116-136) and `search_answer_stream` (610). Quarry learns ZDR only from the user-token floor (`QE/src/auth.rs` [C] 121-138). So a temporary-chat search (`x-zdr` header or body `zdr:true`, which the SPA never sets on `/search/web` today: `src/shared/api/search-client.ts` 85-113) would be cached by the gateway AND by Quarry (not ZDR on Quarry's side unless the org is).
- Scrape preview: `GW/src/domains/knowledge/quarry.rs` [C] `scrape_preview` (62-203). Key `cache_key("scrape-preview", [target])` (104): target only, not org, not request variant. The private `CacheAdmission` (212-282) already denies read and write for `request_zdr` or `org_zdr_enabled` (237-250) and honours the caller's `cache.mode` (256-281); tests at 814-912. Its body is forwarded verbatim to Quarry (126-133), so formats/render/signals/headers (once F-t2-005/023 add them) would change the page returned but not the key.
- `ResultCache` (`GW/src/cache.rs` 25-182) is a no-op when `GATEWAY_CACHE_REDIS_URL` is unset (45-75). Hash is std `DefaultHasher` (SipHash-1-3 with zero keys, 64-bit) in `cache_key` (194-201): fine for non-tenant keys, weak as the only tenant separator, and not guaranteed stable across Rust releases.

**[DRIFT] The exposure is deployment-conditional, which the audit does not say.** The canonical compose passes NO `GATEWAY_CACHE_REDIS_URL` to the gateway (`apps/Frontend Plane/verevonv3/docker-compose.yml` [C] 132-133 says caching "degrades safely ... when no frontend-owned cache contract is configured"; grep of `docker-compose.yml` and `docker-compose.production.yml` [C] for `REDIS|CACHE` finds only comments). The variable IS set in the Data Plane e2e compose (`apps/Data Plane v2/tests/e2e/isolated/real-authority-browser-compose.yml` line 97, Dragonfly DB 4, the same Dragonfly instance as the Data Plane) and in the gitignored local `apps/Frontend Plane/verevonv3/.env` line 26 (value not read). The compliance pack already states "the cache exists only when `GATEWAY_CACHE_REDIS_URL` is set" (`docs/compliance/evidence/retention-and-erasure-evidence.md` [M] W2 row). Whether any production gateway sets it is **[UNVERIFIED]** and is the first thing to establish (section 4, S-2 step 0). The code path is real either way; severity depends on that answer.

**[DRIFT, minor]** the compose comment says caching degrades "to in-process storage"; there is no in-process cache, `ResultCache` is simply disabled (`cache.rs` 38-40, 45-48). Only the rate limiter has an in-process fallback.

### 2.2 What "ZDR-correct" already means in Quarry (protect it, audit section 8)

- Posture = verified token floor OR body flag, raise-only: `Claims::zdr_floor` (121-123), `effective_zdr` (129-131), `effective_zdr_mode` (135-138) in `QE/src/auth.rs` [C]; the service-principal `zdr` claim is deliberately ignored (104-120). The gateway sets `x-zdr` at `GW/src/upstream.rs` 1787-1789 but Quarry has no reader of that header (S6). No new carrier is needed, and none is proposed.
- Durable crawl/batch refused with `ErrorCode::Forbidden` + prose (`QE/src/routes.rs` [M] 563-588, callers 596 and 687). ZDR scrape cache skip (routes.rs 432, 480). `QCO/src/zdr.rs` [C] `guard(zdr, WriteKind)` (56-65) is wired for Artifact, Event, Ingest (`QR/src/pipeline.rs` [C] 601, 856; `ingest_client.rs` 199; `page_renderer.rs` 29-30); `WriteKind::Cache` and `WriteKind::Index` have no production caller.
- Agent runs: persistent profile or `profile_id` refused at start (`QE/src/agent_routes.rs` [C] 527-533, 1469-1476: HTTP 400, code only in the prose prefix `zdr_persistent_profile_forbidden:`); resume of a ZDR checkpoint refused 403 (1347-1353); receipts not appended (2360); events via `emit_for_zdr`.

### 2.3 Audit claims that are wrong or understated

| Claim | Verdict | What the code does |
|---|---|---|
| ADR topic 4 / card: "crawl, batch and schedules are refused under ZDR (routes.rs:563-588)" | **[CLAIM-FALSE] for schedules and change monitors** | `refuse_durable_job_under_zdr` is called for crawl (596) and batch (687) only. `QE/src/schedule_routes.rs` [C] `create_schedule` (239-330) has no ZDR reference at all (grep `zdr` in the file: 0 hits); `QE/src/change_routes.rs` [C] `check`, `latest`, `history`, `snapshot`, `refresh` and the internal `record_check` have none either, except that `emit_change_side_effects` suppresses the event and webhook when the body declares `zdr` (623-639). `QE/src/profile_routes.rs`, `resource_routes.rs`, `fleet_routes.rs`, `map_routes.rs` [C] have no ZDR handling either. A ZDR-org user can create a change monitor today; the schedule target URL is stored by quarry-control, and `record_check` persists baseline (org, URL, fingerprint) and diff artifact rows whatever `zdr` says (change_routes.rs 548-600). I found no code in `QCT/` or the orchestrator that sets `zdr` on the internal record request (grep `zdr` in `services/quarry-orchestrator`: 0 hits; in `QCT/`: only `internal/resources/change_webhook.go`, a refusal gate), so ZDR-org monitor change events and webhooks very likely fire [INF: confirm with the gap test in S-7]. |
| F-t2-004 card / S6: "the live exposure is narrow: one debug-level URL log at change_routes.rs:635, suppressed by the default `info` filter" | **[CLAIM-FALSE] (understated)** | `QR/src/pipeline.rs` [C] 821-829 logs `tracing::info!(url = %resp.final_url, driver, title = %extraction.title, excerpt_chars, word_count, "page extracted")` for every HTML page, with no ZDR condition, although `self.zdr` is in scope two statements earlier (806-817 gates the model hop on it and logs a URL at debug). Default filter is `info` (`QE/src/telemetry.rs` [C] 26; compose `RUST_LOG` default info). So **a ZDR scrape logs the page URL and the extracted title at INFO today.** Further URL-bearing lines on the ZDR-reachable scrape path: `QR/src/fallback_driver.rs` [C] 202, 211 (info), 242, 266 (warn) `url = %url`; `QR/src/browser_driver.rs` [C] 262 (info) `url = %url`; `QR/src/fetch.rs` [C] 263, 285 (warn) `host = %host` plus `error = %err`. |
| "reqwest errors embed URLs" (card) | Confirmed, and worse for search | `QR/src/serp.rs` [M] `SearXNGSearch::search` (595-618) issues `GET {base}/search?q=<query>` and converts failures with `format!("searxng: {e}")` (586, 618). reqwest 0.12 `Display` includes the request URL unless `Error::without_url()` is used (grep `without_url` in all crates and the gateway: 0 hits). `smart_router.rs` [M] 543 and `serp.rs` 350 then `warn!(error = %e, ...)`. SearXNG is permitted under ZDR (only paid providers are excluded), so **a SearXNG failure on a ZDR search logs the user's query at WARN.** [INF from reqwest behaviour: prove with a unit test that a refused connection to `http://127.0.0.1:1/search?q=SECRET` yields an error string containing `SECRET`; that test is part of S-6]. 186 `format!("...{e}")` sites exist across 41 files; the reqwest-converting subset (serp.rs 18, fetch.rs 5, mp_client.rs 6, http3.rs 2, ingest_client.rs 3, robots_cache.rs 2, data-plane clients) is about 35-45 sites. |
| "about 295" / "roughly 250" tracing call sites | Drift | Live recipe (rg multiline, section 4.5): 310 macro lines in 51 files under `Quarry-v2/crates`; 80 in 22 files under `GW/src`. The url/query/error-ish subset is 111 (S6 heuristic) to 161 (broader recipe in 4.6) in Quarry; `QE/src/main.rs` alone has 105 macro lines (mostly startup and tests). Exclude nested `Quarry-v2/.claude/worktrees/*`. |
| Scrape usage meter is ZDR-clean | **[CLAIM-FALSE]** (F-t2-046 owns the fix; recorded because it is a ZDR sink) | `QE/src/routes.rs` [M] 491-509 meters `scrape.page` with metadata `{url: out.url.final_url, fingerprint, user_id, ...}` unconditionally; billing-core persists the metadata JSON (`apps/Control Plane/billing-core/internal/billing/repository.go` 485-488, per S6). |

### 2.4 Uncommitted (`[M]`/`[U]`) files in this area and whether in-flight diffs conflict

| File | State | In-flight change | Conflict with this work |
|---|---|---|---|
| `QE/src/routes.rs` | [M] | one hunk at line 1329 (tests region) | none for 303 (TraceLayer) or 488-509 (meter); hotspot with F-t2-014/046/023 by line range only |
| `QE/src/search_routes.rs` | [M] | rerank-aware `params_signature`, `cacheable_search`, SEARCH_QUERY meters (other initiative) | S-7 only reads it; do not edit before it commits |
| `QE/src/state.rs`, `QE/src/config.rs`, `QE/tests/support/mod.rs` | [M] | QB-T17 Model Plane gate fields (`AppState.model_plane`, config switches), `base_state` literal | S-8 appends one config field (append-only); S-10 adds an `AppState` field and a `base_state` literal entry: both collide with these hunks, so S-10 lands after they commit |
| `QE/src/lib.rs`, `QE/src/main.rs` | [M] | one `pub mod` line; large model-plane-gate hunks in main.rs (244-253, 984-1232) | S-5 adds one `pub mod zdr_log;` line to lib.rs (trivial rebase) and edits only `telemetry.rs`; main.rs `mod` list is hand-duplicated and needs `mod zdr_log;` only if `telemetry.rs` references it as `crate::zdr_log` (use `quarry_edge::zdr_log` instead, as main.rs:1508 already does for `fleet_routes`) |
| `QR/src/serp.rs`, `QR/src/smart_router.rs` | [M] | serp.rs hunks at 21, 92, 289-308, 1083; smart_router.rs hunks at 71-77, 859 | the `without_url` edits (serp.rs 383-786) and the 543 log line are outside those hunks; low risk |
| `scripts/tests/zdr-contract-sync.test.sh`, `scripts/tests/zdr-contract-schema.mjs` | [M] (95 + 24 lines) | new goldens rows (QB-T09, QB-T10, usage-ledger, SPA refusals) | **wait**: S-7/S-9 append rows to GOLDENS/VENDORED/CONSUMERS; do it after that diff commits or as an append-only hunk at the end of each list |
| `QE/tests/zdr_claim_floor.rs` | [M] | two 5-line `model_plane:` blocks (audio tests, QB-T17) | none: S-7 adds a NEW test file and copies the token fixture rather than editing this file |
| `QR/contracts/cross-plane-token-requests.json` (+ `.sha256`), `QR/src/token_request_contract.rs` | [M] | QB-T02 manifest tweak | none; S-7 adds a sibling contract and copies the loader pattern |
| `MPX/api/contracts/zdr-refusals.json` (+ `.sha256`), `MPX/rust/crates/mp-contracts/src/{zdr_refusal,retention}.rs`, `.../tests/zdr_retention_matrix.rs`, `lib.rs` [M] | [U] / [M] | the Z-05a/Z-05b/Z-10/Z-17 ZDR refusal-taxonomy initiative | **shapes S-7/S-9; see 2.5** |
| `MPX/rust/services/model-gateway/src/{zdr_features,zdr_posture}.rs`, `.../tests/{zdr_feature_table,zdr_posture_matrix,zdr_refusal_inventory}.rs`, `.../tests/fixtures/zdr-posture-matrix.json` | [U] | Model Plane field-by-field ZDR treatment table and decision function | pattern to mirror; no file overlap |
| `MPX/rust/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `.../src/retention_receipt.rs` | [U] | Model Plane run retention receipt (`runs.retention_class`) | naming alignment for S-10 only; Quarry's tombstone is in quarry-control, never session-core |
| `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts` (+ tests, vendored golden) | [U] | SPA texts for the catalogue codes; closed `ZdrFeatureKey` union; pinned sha const `ZDR_REFUSALS_CONTRACT_SHA256` | S-9 (new feature keys) forces edits here; wait |
| `docs/compliance/evidence/retention-and-erasure-evidence.md`, `MPX/docs/ZDR.md`, `MPX/api/contracts/README.md` | [M] | evidence/doc updates | S-3 updates the W2 row of the evidence pack; append-only edit |
| `apps/Ingestion Plane/integration-corev2/internal/{zdr,store,api}/zdr*` | [M]/[U] | connection ZDR capability, acknowledgement, wording | no overlap (connections are not Quarry features) |
| `.github/workflows/verevonv3-ci.yml` | [M] | demo-product job | none; gateway job (fmt, clippy, test) unchanged and runs the new gateway tests |
| All gateway files in 2.1 (`search.rs`, `cache.rs`, `zdr.rs`, `knowledge/quarry.rs`, `upstream.rs`, `main.rs`, `config.rs`, `gdpr_nats.rs`) | [C] | none | none; slice S-1 is conflict-free against the working tree |

### 2.5 The in-flight Model Plane ZDR-refusal taxonomy, and exactly what overlaps Quarry's matrix

What exists (read in full where noted):

- `MPX/api/contracts/zdr-refusals.json` [U] (generated; sha256 `9c2b5458...6d1e`): contract `model-plane.zdr-refusals` v1. **10 codes** (`zdr_unavailable`, `zdr_feature_not_supported` (takes a `feature`), `zdr_connection_not_supported`, `zdr_connection_not_acknowledged`, `zdr_explicit_request_not_supported_by_connection`, `zdr_decision_unavailable` (retryable), `zdr_waiver_route_mismatch`, `zdr_posture_mismatch`, `zdr_durable_mutation_forbidden`, `zdr_inference_failed`), each with `action` and `retryable`, a closed **30-key feature vocabulary** (`web_search`, `browser_run`, `browser_run_control`, `scheduled_step`, `agent_run`, `structured_output`, ... none of them Quarry-specific), and a `wire` block (SSE `error` frame, JSON `{error:{code,message,retryable,action,feature?}}`, gRPC trailers `x-mp-provider-error` / `x-mp-zdr-feature`).
- `MPX/rust/crates/mp-contracts/src/zdr_refusal.rs` [U] (1006 lines): `code::*`, `ENTRIES`, `FEATURES`, `Refusal` (constructors only; `Refusal::feature_not_supported(key)` debug-asserts the key is in `FEATURES`), golden regenerated with `MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal`.
- `MPX/rust/services/model-gateway/src/zdr_features.rs` [U]: `Treatment::{Honored, Refused{code,feature}, Inert(reason), PerMember}` per request field; `invoke_field_names` destructures `InvokeRequest` without `..` so a new field stops the build until classified; `tests/zdr_feature_table.rs` checks one row per field.
- `MPX/api/contracts/zdr-posture-matrix.json` [C] (50 KB, authored, 44 decision rows, 27 enforcement rows; org states x connections x waiver) plus `mp-contracts/src/retention.rs` [U] (`resolve_turn`, pure) and `tests/zdr_retention_matrix.rs` [U]: Model Plane TURN posture. Has no Quarry, search, scrape or schedule rows.
- `session-core` migration 0044 [U]: `runs.retention_class IN ('zdr','persistent','zdr_waived')` with an immutability trigger; `retention_receipt.rs` [U]: stamped at run start, child inherits.
- CI wiring: committed `scripts/tests/zdr-contract-sync.test.sh` knows `zdr-refusals.json` only through a conditional at line 288; the GOLDENS row, the vendored SPA copy and the loader changes are in the uncommitted diff. A Model Plane or Quarry CI run that depends on the catalogue is not green from the committed tree alone.

What overlaps the Quarry matrix, precisely:

1. **Refusal codes**: three catalogue codes are exactly what Quarry needs and already carry stable client behaviour in the SPA: `zdr_durable_mutation_forbidden` (create/change/delete durable state: crawl, batch, schedules, monitors, profiles), `zdr_feature_not_supported` + feature (a named feature cannot run), `zdr_decision_unavailable` (the gateway cannot establish posture; retryable). `zdr_posture_mismatch` fits the service-vs-body posture disagreement. The Quarry codes that are NOT in the catalogue: `zdr_persistent_profile_forbidden` (`QE/src/agent_routes.rs` 1474; `GW/src/domains/browser.rs` [C] 180) and `ZDR_REQUIRED` (`QE/src/search_routes.rs` 704, a 400 request-validation code: a service caller omitted `zdr`).
2. **Feature vocabulary**: only `web_search`, `scheduled_step` (label "scheduled runs"), `browser_run`, `agent_run`, `structured_output`, `document_analysis` are semantically usable for Quarry rows. Quarry rows for durable crawl, batch, persistent profile, session resume and screenshot/PDF have no key. A new key requires: `FEATURES` in `zdr_refusal.rs`, regeneration of the golden, a new sha in the sync script GOLDENS row, refreshed vendored copies (model-gateway fixtures, SPA `src/shared/api/contracts/zdr-refusals.json`), the SPA `ZdrFeatureKey` union and `ZDR_REFUSALS_CONTRACT_SHA256`, and the SPA `zdr-refusals.test.ts`. All of those files are uncommitted today.
3. **Matrix shape**: `Treatment` (Honored / Refused{code,feature} / Inert) is the pattern; Quarry needs two more treatments the audit's ADR names (`inline_only`, plus `degraded`). The completeness idea (a new field stops the build until classified) maps to a new route stops the test until classified.
4. **Receipt vocabulary**: `retention_class` with values `zdr` / `persistent` is the word the Model Plane run receipt uses; the Quarry tombstone should use the same column name and values (no `zdr_waived`: Quarry has no waiver concept).
5. **Posture semantics**: raise-only (`Claims::effective_zdr` == "a caller can raise the posture, never lower it", `zdr.rs` docs in both planes). Same rule; no code can be shared across workspaces (Quarry-v2 and Model Plane are separate cargo workspaces), so consumption is by vendored JSON golden + sha pin, the repo's established pattern (`scripts/tests/zdr-contract-sync.test.sh` GOLDENS / VENDORED / CONSUMERS).
6. **Not overlapping**: connection capabilities and waivers (integration-corev2, `connection-zdr-capabilities.json`), model-turn decision rows, Data Plane `zdr_dense_egress.rs`, SPA chat refusal handling.

What must wait until that initiative commits (hard list, repeated in section 5): (a) vendoring `zdr-refusals.json` into Quarry and gating its sha (S-9), because the file is generated and may still change; (b) adding Quarry feature keys to the catalogue; (c) any append to `zdr-contract-sync.test.sh`; (d) the gateway BFF mapping of Quarry's `details.zdr_refusal` onto the SPA's `ZdrRefusalCode` handling; (e) citing the catalogue as a CI-gated contract anywhere in Quarry docs. What can proceed now: S-1..S-6 (no overlap) and S-7 using the catalogue's code STRINGS as a local closed list (the strings are the contract, not the file).

## 3. Design decisions

Each decision: options, recommendation, rationale. Decisions marked **ADR-GATED** need a user decision; the audit ADR topic number (section 9.3) is named. Anything unmarked can be built now.

### 3.1 Cache key and scope: org id as a plain key segment, strong digest, scope tag in the payload

- Options: (a) add the org id to the existing `cache_key(parts)` hash input; (b) a distinct key shape with the org as a readable segment and a SHA-256 digest of the variant parts; (c) also add the user id.
- **Recommendation: (b), org only.** Key = `verevon:gw:{ns}:o:{org_id}:{sha256_hex(len-prefixed parts)}` (mirrors Quarry's own `quarry:search:{org}:{blake3}`, `QE/src/cache.rs` 121-127). The org segment is the validated `authorized_org_id` (non-empty, at most 128 chars, `[A-Za-z0-9_-]` only, so it cannot contain `:` or glob characters); anything else means "no cache". The stored envelope becomes `{cached_at, scope, data}` and `lookup_scoped` returns a miss (plus a `scope_mismatch` counter and an `error!` log with ids only) if `scope` differs from the requesting org: defence in depth against a future key-construction regression.
- Rationale: org-keyed matches the result's real scope: Quarry's search and answer results are org-scoped, not user-scoped (`SearchCache::key(org, query, params)`; the Tantivy filter is `org_id`), so adding the user would only cut the hit rate. A readable org segment is what makes org erasure possible (S-3: `SCAN MATCH verevon:gw:*:o:{org}:*`); the compliance pack today says result keys "are hashed and cannot be found by subject". SHA-256 (the crate already has `sha2`, `hex`: `GW/Cargo.toml` [C]) removes the reliance on a 64-bit zero-key SipHash for tenant separation. Empty org (session-only flow) never caches: `org_zdr_enabled` already treats an empty org as ZDR (zdr.rs 70-75), so the rule is consistent.
- Legacy `cache_key` stays for non-tenant keys (`org-zdr`, `session-validation`, `session-context`, `browser-run-owner`). Adjacent note for its owner, not fixed here: `session-validation` is keyed by a 64-bit SipHash of the cookie header (`middleware.rs` 465).

### 3.2 One shared admission module, handlers cannot reach the content cache around it

- Options: (a) copy `CacheAdmission` into `search.rs`; (b) promote it to a shared module and keep calling raw `state.cache.lookup/store` from handlers; (c) promote it AND make the admitted handle the only way to read or write content caches.
- **Recommendation: (c).** New `GW/src/cache_admission.rs` [NEW] holds `Admission` (the promoted `CacheAdmission` plus `scope`), `Denied` reasons, `Admission::resolve(...)`, `Admission::key(ns, parts)` and `ResultCache::{get_admitted, put_admitted}` extension methods. `resolve` order: `request_zdr` (cheap, no I/O) -> `org_posture` (new tri-state wrapper in `zdr.rs`: `Zdr | NotZdr | Unknown`, with `org_zdr_enabled` = `!NotZdr`, so existing callers and tests are untouched) -> scope validation -> request-shape deny list (`headers`, `actions`, `profile`, `profile_id`, `session`, `session_id`, `cookies`, `authorization`, `proxy`, and `ingest: true`: Firecrawl's `hasCustomRequestContext` rule, which also covers fields F-t2-005/023 will add) -> caller `cache` policy (`CacheAdmission::from_policy`, unchanged). Raw `ResultCache::lookup`/`store` remain only for identity/posture caches; a source-scan test (`cache_admission_only`, section 4 S-1) fails if `domains/**` calls them.
- Rationale: the original bug (BI-2 comment in `quarry.rs` 94-102) was a second admission policy invented in a second handler. The same drift happened again one handler over. F-t2-023 and F-t2-048 need the same gate; they should call `Admission`, not re-derive it. The module is gateway-local: Quarry's own admission (`should_read`/`should_write`/`cacheable_search`) stays where it is and the matrix (3.7) is what keeps the two layers aligned.

### 3.3 ZDR semantics per feature (ADR topic 4, ADR-GATED)

Definitions: **refused** = typed refusal before any persistence or egress; **inline-only** = the result is returned in the response (or live stream) and nothing is stored; **hash-only** = a digest is stored, content is not; **metadata-only** = counts, timings, outcome codes, no target or content; **honored** = runs as usual and writes nothing durable. Rule for the whole table: existing refusals are never loosened. Rows marked `tighten` change behaviour for ZDR orgs today and need the ADR sign-off; rows marked `leak` close a leak without changing a contract.

| # | Feature | Today (verified) | Recommended default | Options rejected and why |
|---|---|---|---|---|
| 1 | Gateway search-web cache | stores for every org, key has no org, no ZDR gate (`search.rs` 218-285) `leak` | **refused**: no read, no write, no stale; non-ZDR orgs get an org-scoped entry | hash-only is meaningless for a cache |
| 2 | Gateway scrape-preview cache | ZDR denied (`quarry.rs` 237-250); key is target only | **refused** under ZDR (keep); non-ZDR key = org + request variant | |
| 3 | Quarry `SearchCache` (`/v1/search`, answer) | read skipped, write skipped under ZDR (`search_routes.rs` 775-782, 831) | **refused** (keep) | |
| 4 | Quarry `PageCache` (scrape) | read/write skipped under ZDR (`routes.rs` 432, 480); key is `quarry:page:{fingerprint}`, **not org-scoped** (`cache.rs` 31-33) | **refused** under ZDR (keep). Org scoping belongs to F-t2-048; flagged here as an adjacent tenant finding | |
| 5 | URL cache (F-t2-048, future) | none | **refused** | hash-only URL cache serves nothing and records a visited-URL oracle |
| 6 | Change monitors (`POST /v1/schedules` with `preset`) | not refused; target URL stored by control, baseline and diff rows persisted, ZDR-org events/webhooks likely fire (2.3) `tighten` | **refused** at create with `zdr_durable_mutation_forbidden` | hash-only: the URL itself is the retained datum, so impossible. inline-only (client keeps the previous fingerprint and calls `/v1/change/check`) is a possible later ZDR variant; not built |
| 7 | Schedules (cron, `schedule_at`) | not refused `tighten` | **refused**, same code | |
| 8 | Durable crawl, batch | refused 403 (`routes.rs` 563-588) | **refused** (keep). Lifting needs the orchestrator to carry posture to `run_page` (comment at 555-562) | |
| 9 | Telemetry: log fields | URL, title, host, error text on the ZDR scrape path (2.3) `leak` | **content-free fields only** in ZDR scope: ids, enum outcomes, status, durations, counts; sensitive-named fields dropped; errors logged as a class code | strip-and-keep (hash a URL) rejected: a hash of a known URL is a membership oracle |
| 10 | Telemetry: spans / OTLP | Quarry: tower-http spans only (debug level), export only if `OTEL_EXPORTER_OTLP_ENDPOINT`; Go: `Init` but no spans | **drop at the sampler** + exporter kill switch (phase 2, D10) | |
| 11 | Telemetry: counters | gateway Prometheus is live; Quarry metrics crates inert | **honored**, content-free labels only (no URL, query, title; no per-user label) | |
| 12 | Scrape result delivery | ZDR run skips the artifact (`pipeline.rs` 681-683), so the caller gets no content (S4) | **inline-only** (F-t2-005): content in the response body, nothing persisted | refusing scrape under ZDR would be a regression for the strongest ZDR path |
| 13 | Scrape-time screenshot / PDF (F-t2-023) | not offered at scrape | **refused until F-t2-023 ships an inline path, then inline-only** with a size cap | storing and returning a reference is retention |
| 14 | Browser-agent screenshots, DOM snapshots (live view) | artifacts withheld (`browser.rs` 3846-3870); step receipts not appended (`agent_routes.rs` 2360) | **inline/live-only** (keep) | |
| 15 | Persistent profile at run start | refused 400, code only in prose (`agent_routes.rs` 1469-1476) | **refused** (keep), code mapped to the catalogue | |
| 16 | Profile CRUD `/v1/profiles*` | no ZDR check at all; `GET` returns raw cookies (F-t2-020) `tighten` | **refused** for ZDR callers, `zdr_durable_mutation_forbidden` | a ZDR caller has no legitimate stored profile |
| 17 | Run history, events, timeline | live subscribers only via `emit_for_zdr`; no replay | **inline/live-only** (keep) plus the metadata-only tombstone (3.9) | |
| 18 | Run resume, checkpoints | ZDR checkpoints refused 403 (`agent_routes.rs` 1347-1353) | **refused** (keep) | |
| 19 | Artifacts (CAS/S3/FS) | withheld by `zdr::guard(Artifact)` | **refused** (keep) | |
| 20 | Data Plane ingest (`ingest: true`) | denied by `zdr::guard(Ingest)` (`ingest_client.rs` 199) | **refused** (keep) | |
| 21 | Usage metering metadata | `url` + `fingerprint` included under ZDR (`routes.rs` 491-509) `leak` | **metadata-only**: metric, quantity, org, request id, `zdr: true`; drop `url`, `fingerprint` | owner: F-t2-046, slice S-4 does the scrape site |
| 22 | Change webhook | suppressed only when the body declares `zdr`; control refuses ZDR bodies (`change_webhook.go` 75-85) | **refused** (keep; becomes unreachable once row 6 is refused) | |
| 23 | AI formats, model hops | title hop refused under ZDR (`page_extract.rs` 335-343); AI formats allowed with no `zdr` on the invoke (`ai_formats.rs` 302-309) | **refused until the invoke request carries ZDR (F-t2-019)**; gap row | `tighten` if AI formats are live for ZDR orgs; flag only |
| 24 | Paid search providers | excluded under ZDR (`build_search_options` 501-531) | **refused** (keep) | |
| 25 | Run tombstone | none | **metadata-only**, no URL, no URL digest, immutable, default 30 days | hash-only rejected (membership oracle) |

Open product point inside this table: rows 6, 7, 16 tighten. Today a ZDR org can create monitors and schedules. If any such org relies on that, the decision is "refuse and migrate" versus "grandfather with a warning". The recommendation is refuse, because the audit's own ADR text and the ZDR contract describe these as refused already.

### 3.4 Fail-closed on the search path: skip the cache, do not refuse the search

- Options: (a) refuse `/search/web` with `zdr_decision_unavailable` when org-core cannot answer; (b) serve the search but bypass the cache (read, write, stale).
- **Recommendation: (b).** `org_zdr_enabled`'s own contract (zdr.rs 55-65) is that nothing the caller does with the posture is load-bearing for the answer: Quarry derives ZDR from the token floor on its side. Refusing search on an org-core blip would turn a cache concern into an availability incident. The gateway forwards `zdr: true` to Quarry whenever it knows (`request_zdr` or `OrgPosture::Zdr`, new in S-1) so Quarry's own caches and paid-provider exclusion follow the body flag too (closes the temporary-chat gap in 2.1). Counter `gateway_result_cache_total{ns,outcome}` records `bypass_zdr`, `bypass_zdr_unknown`, `bypass_no_scope`, `bypass_request_shape`, `hit`, `stale_hit`, `miss`, `store`, `scope_mismatch` (labels bounded; no org or query label; allowed under ZDR).

### 3.5 Stale-on-error stays, but only inside the scope

The 24 h stale fallback (`search.rs` 288-291) is an availability feature. It stays for admitted non-ZDR requests inside the same org scope. It is never served across scopes (the key and the scope tag both prevent it) and never for ZDR.

### 3.6 Incident response: purge, evidence, and the honest answer to open question 3

- Open question (audit 10.2 item 3): "was any org-private result ever served across orgs"? **It cannot be answered retrospectively from the cache**: entries carry no org tag, cache hits are not logged (`cache.rs` has no logging or metrics on `lookup`), the gateway request span logs path only (`main.rs` 194-202), and Dragonfly keeps no per-key hit counter. What is answerable: (1) whether any gateway ever had the cache on, and since which image revision (`GET /version` returns `SOURCE_REVISION`, main.rs 213-219; env of the running container); (2) how many cached entries today contain org-derived content (`answer != null` or any `results[].provider == "tantivy_local"`), a bound on what could have been served; (3) the global hit rate (Dragonfly `INFO stats`: `keyspace_hits`, `keyspace_misses`, which include rate-limit and session keys, so coarse) and, from the gateway's own request count versus Quarry's `search.upstream` event count over the same window, the number of requests that never reached Quarry. If (1) says the cache was never enabled outside dev, the answer is "no exposure, structurally possible but never reachable". If it was enabled, the honest answer is "exposure window up to 24 h of entries at any time; cross-org serving cannot be proven or disproven; treat org-derived entries as potentially served" and the incident record says so.
- **Recommendation**: run the read-only forensics script BEFORE deploying S-1 (so the snapshot is of the old state), deploy S-1 everywhere, then purge the two legacy namespaces (new code never reads the old key shape, so the purge is hygiene and closes the window for old replicas still writing during a rolling deploy). Never `FLUSHDB`: the Dragonfly is shared with the Data Plane in the e2e compose, and DB 4 also carries the rate-limit counters and the session-validation cache. The counters added in S-1 make the question answerable going forward (`stale_hit`, `hit`, `scope_mismatch`).

### 3.7 The matrix: Rust table as source of truth, generated golden, route-scan completeness test

- Options: (a) hand-maintained JSON (the audit explicitly says no); (b) derive from the live axum `Router` (axum 0.7/0.8 does not expose a route list); (c) a `zdr_route(path, handler, posture)` registration wrapper used at every `.route(` (the true "route attribute"); (d) a const table in `quarry-core` plus a test that scans the source for `.route("...")` registrations and fails on any route without a row or any row without a route.
- **Recommendation: (d) now, (c) as a later refinement.** (c) edits `routes.rs` [M] (the busiest conflict file in the Quarry plan) and the agent/fleet routers for little extra safety. (d) gives the same guarantee ("a new route stops CI until classified", the property `zdr_feature_table.rs` has for `InvokeRequest` fields) without touching them.
- Shape: `quarry-core/src/zdr_matrix.rs` [NEW]: `pub enum Treatment { Honored, InlineOnly, Degraded { warning }, Refused { code, feature }, Inert { reason } }` (variant spellings follow the Model Plane `Treatment`), `pub struct Row { id, routes: &[&str], treatment, enforcement: &str, status: Enforced | Gap { issue } }`, `pub const ROWS`. The golden `QR/contracts/zdr-feature-matrix.json` is GENERATED from `ROWS` (`QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix`) and pinned like the other goldens (sha file + sync-script row), the same pattern as `zdr-refusals.json` ("generated, never written by hand"). `status: gap` rows assert today's (bad) behaviour in the behavioural test, so fixing a gap forces the matrix to flip: no silent drift in either direction.

### 3.8 Align with the Model Plane catalogue instead of a second taxonomy

- **Recommendation**: Quarry refusals keep Quarry's envelope (`{data, meta, error:{code, message, details, retryable}}`, `QCO/src/error.rs` 63-70) and HTTP status (403 for `ErrorCode::Forbidden`), keep the existing message text (existing tests assert the message contains "ZDR", `zdr_claim_floor.rs` 842-870), and ADD `details.zdr_refusal = {code, feature?, action}` with a code from the Model Plane catalogue: `zdr_durable_mutation_forbidden` for rows 6, 7, 8, 15, 16, `zdr_feature_not_supported` + feature for rows 13, 23, `zdr_decision_unavailable` from the gateway where applicable. `zdr_persistent_profile_forbidden` stays as `details.legacy_code`. `ZDR_REQUIRED` is request validation, listed in the matrix `out_of_catalogue` allowlist.
- Phase S-7 uses only the 10 existing codes and the existing feature keys (`scheduled_step` for schedules/monitors, `web_search`, `structured_output`). New Quarry feature keys (`durable_crawl`, `browser_profile`, `session_resume`, `page_screenshot`) are an S-9 follow-up PR into the Model Plane catalogue after that initiative commits; until then rows 8, 15, 16 use the featureless `zdr_durable_mutation_forbidden`.
- The gateway BFF maps `details.zdr_refusal` to the SPA's `ZdrRefusalCode` handling (`zdr-refusals.ts` [U]) so a Quarry refusal renders the same Norwegian/English text as a model refusal. That mapping waits for `zdr-refusals.ts` to commit.
- Quarry cannot import `mp-contracts` (separate cargo workspaces). Consumption is a vendored byte-identical copy of `zdr-refusals.json` with its `.sha256` pin, registered in `zdr-contract-sync.test.sh` VENDORED + CONSUMERS, and a Rust test (modelled on `QR/src/token_request_contract.rs` [M]) that loads it and asserts every `code` and `feature` named by `ROWS` exists in it.

### 3.9 Tombstone: metadata-only, allowlist schema, quarry-control owns it (ADR-GATED, topic 4, plus contract review)

- Options: (a) none (status quo: a ZDR run leaves no record at all: no support diagnosis, no metering reconciliation); (b) derive from the usage meter event; (c) a dedicated signed internal call from the edge to quarry-control that writes one row.
- **Recommendation: (c), behind an off-by-default flag, after the contract review.** (b) is rejected because the meter's metadata is exactly where the URL leak is (row 21), and F-t2-046 owns that shape.
- Row (all columns allowlisted, `DisallowUnknownFields` on the Go side): `run_id`, `org_id`, `kind` (`scrape|search|answer|extract|agent_run`), `retention_class` (`'zdr'`, same word as `runs.retention_class` in Model Plane migration 0044), `outcome` (enum), `driver`, `started_at`, `ended_at`, `step_count`, `cost_units`, `error_class`. **No URL, no URL digest, no query, no title, no user id, no error text, no fingerprint.** The body must not use the key `zdr`: control's existing change-webhook gate rejects any body with a truthy `zdr` (`change_webhook.go` 75-85) and that gate must stay; the tombstone uses a different route and `retention_class`.
- Retention: janitor sweep keyed on `ended_at`, default 30 days, env `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS`; plan-based retention from Control Plane (`apps/Control Plane`) is a later input fetched through its API, never read from its database. The existing artifact janitor is NOT the base: it is default-off and its own header says it is not safe for deletion until a provenance guard lands (`janitor.go` 14-18), so the tombstone sweeper is a separate function with its own two-key arm.
- Whether a metadata-only record is compatible with each customer's ZDR contract and DPA is a legal/compliance question, not an engineering one. It is the reason this slice is gated.

### 3.10 ZDR carrier for logs: a span field plus a marker layer; no new header

- Options: (a) tokio task-local; (b) span field `zdr` + a marker layer + a per-layer event filter; (c) per-call-site gating only; (d) a new `x-zdr` header or NATS header.
- **Recommendation: (b).** (d) is rejected on evidence: Quarry derives posture from token floor OR body flag and deliberately ignores a service claim (`auth.rs` 104-120), has no reader of `x-zdr` (S6 grep), and Model Plane NATS already drops `zdr:true` envelopes (`natsx/publisher.go` 56-60): a third carrier would go unread. (a) loses context on `tokio::spawn` exactly like spans do but, unlike a span field, cannot serve the OTLP layer or the phase-2 sampler. (c) is what produced the leak: the `page extracted` line sits two statements from a ZDR check and still logs the URL.
- Mechanics: (1) the HTTP span declares `zdr = tracing::field::Empty`; (2) a tiny middleware after `require_auth` records `zdr = true` when `claims.zdr_floor()`; (3) each handler that resolves posture calls `ZdrMode::mark_current_span()` (new, `quarry-core/src/zdr.rs`) at the same line it already calls `effective_zdr_mode` (routes.rs 416, 1072; search_routes.rs 727, 1928-1992; answer_routes.rs 89, 187; extract_routes.rs 237; agent_routes.rs 1443, and each step handler from `entry.zdr`); (4) `ZdrMarkLayer` (sticky: once true it stays true; children inherit) sets a span extension; (5) a `DynFilterFn` on the JSON fmt layer drops an EVENT when its span scope is ZDR and its callsite field names (`meta.fields()`) intersect `SENSITIVE_FIELDS`. Events with only safe fields still pass, so operational visibility survives.
- Also required: the tower-http `TraceLayer::new_for_http()` default span records the full request URI including the query string (`GET /v1/change/latest?url=...`) and the JSON fmt layer prints span fields on every event (`with_span_list` default). Replace with a `make_span_with` that records method and path only, as the gateway already does (`GW/src/main.rs` 168-202 `request_target_for_log`).
- Fields: `SENSITIVE_FIELDS = url, uri, source_url, target_url, target, query, q, selector, title, host, domain, origin, snippet, text, markdown, excerpt, error, err, reason_text`. Replacement for error text: `error_class` (`zdr_log::class(&QuarryError)` returns the `ErrorCode` name; `class_reqwest(&reqwest::Error)` returns `timeout|connect|status|decode|request`).
- Late-ZDR (posture learned after events were already emitted) cannot be undone for logs; for spans it is the registry of phase 2. Mitigation for logs: mark at the earliest decision point (the floor middleware) and keep content-bearing fields off request-path logs entirely (the lint, 3.11).

### 3.11 Lint: source-scan test with a ratchet baseline, plus a Layer behaviour test

- Options: (a) clippy `disallowed_macros`/`disallowed_methods` (can ban `tracing::info!` wholesale, cannot see field names); (b) a dylint custom lint (new toolchain pin in a repo whose Quarry crates only build in a Docker container on Windows hosts); (c) a source-scan test in the idiom the gateway already uses (`GW/tests/ssrf_forward_not_fetch.rs`, `sse_relay_no_allowlist.rs`) with a baseline that may only shrink (the idiom of auth-core's `scripts/lint-ratchet.mjs`); (d) a Layer test proving the filter works.
- **Recommendation: (c) + (d).** (c) runs in the existing `cargo test --workspace` (Quarry) and `cargo test` (gateway) with no new CI job. Rules: R1 a sensitive field name in a tracing macro; R2 `error = %e`/`?e` whose value is not wrapped by `zdr_log::class*` (the type is not visible to a scan, so any raw error display is flagged); R3 an interpolated message literal (`{` in the message string); R4 `tokio::spawn` that is not `.in_current_span()` or `.instrument(...)` inside request-path files (carrier loss). Exemption: a `// zdr-log-ok: <reason>` comment on the line above the macro (reason mandatory, count reported). The baseline file lists `file::rule -> count`; the test fails when a count rises (new violation) AND when it falls without the baseline being lowered (ratchet), and prints the recipe to regenerate.
- Not recommended: clippy (cannot express the rule), dylint (cost), a runtime-only guard (it cannot see call sites that never ran in a test).

### 3.12 What this design deliberately does not do

No ZDR header or NATS header; no `FLUSHDB`; no change to Quarry's org-keyed `SearchCache` (audit section 8: protect it); no second refusal taxonomy; no cross-plane database access (the tombstone is in quarry-control's own Postgres, the Control Plane retention input is an API read); no new refusal behaviour for rows 6, 7, 16, 23 before the ADR; no hash-only anything.

## 4. Slices (PR-sized, in build order)

Order: S-2 step 0-2 (read-only evidence) -> **S-1** -> S-2 step 3-5 (purge) -> S-3 | S-4 -> S-5 -> S-6 | S-7 -> S-8 | S-9 (waits) | S-10 (waits). S-1 and S-4 touch disjoint trees and can be built in parallel by two people.

Common commands (from CLAUDE.md and the S9 sweep):

- Gateway, CI parity (`.github/workflows/verevonv3-ci.yml` [M] job "Gateway", lines 132-141): `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`. CLAUDE.md form: `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (CI omits `--all-targets`; run both).
- Quarry: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test --workspace` (CI: `quarry-v2-ci.yml` [C] line 205). On a Windows host the Rust crates do not build natively: use the `rust:1` container with `quarry-cargo`/`quarry-target` cache volumes and `bash -c` (memory `quarry-rust-tests-in-docker`; a full `-p quarry-core -p quarry-runtime -p quarry-edge` run takes over 10 minutes, so run targeted tests while iterating). Go: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`.
- Contracts: `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`; `node --test scripts/tests/zdr-contract-schema.test.mjs` (CI `contract-goldens.yml` [C] lines 66-74).
- "Deployment evidence" in each slice means what the repo counts as a deployment claim: rebuilt local images, a running stack, and a verified end-to-end flow with a stated observable assertion. Stack recipes: gateway image and frontend restart per the Ingestion Plane deploy recipe in project memory (`ingestion-plane-deploy-recipe`, env-file chain order, gateway image swap, frontend restart); Quarry via `cd "apps/Ingestion Plane" && make up` (override the macOS cache paths in the Makefile, S9 2.1) and `make test-endpoints`. The stock compose does NOT give the gateway a cache: every cache evidence run must set `GATEWAY_CACHE_REDIS_URL` explicitly to a Redis-compatible instance reachable from the gateway container.

---

### S-1: Gateway content-cache admission, org scope, ZDR posture forwarding (F-t2-003 phase 1)

**Goal.** No gateway content cache entry can be read by another org, none is written or read for ZDR (request, org, unknown, or no org), the scrape-preview key includes org and request variant, and Quarry is told the gateway's ZDR decision.

**File-level changes.**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `GW/src/cache_admission.rs` | `Admission`, `Denied`, `CacheScope`, `ScopedKey`, `Admission::resolve/key/is_zdr`, `ResultCache::{get_admitted, put_admitted}` | promoted from the private `CacheAdmission`; adds scope, request-shape deny list, `Span::current().record("zdr", ...)` hook (used by S-5), metrics calls | [NEW] |
| `GW/src/cache.rs` | `ResultCache` | backend enum `Disabled \| Redis \| Memory` (`Memory` is `#[cfg(test)]`, with `in_memory()`, `len()`, `put_raw(key, scope, data, cached_at)`); `lookup_scoped`/`store_scoped` (envelope `{cached_at, scope, data}`); `scoped_key(ns, org, parts)` (SHA-256, length-prefixed); `connect` reads `GATEWAY_RESULT_CACHE_CONTENT` once; raw `lookup`/`store` doc-marked "identity and posture caches only" | [C] |
| `GW/src/zdr.rs` | `OrgPosture`, `org_posture()` | tri-state `Zdr \| NotZdr \| Unknown`; `org_zdr_enabled` becomes `!matches!(.., NotZdr)` so callers (`chat/shared.rs:111`, old `quarry.rs:246`) and the 4 existing tests are unchanged; new tests for `Unknown` | [C] |
| `GW/src/domains/search.rs` | `search_web` (51-293), `search_answer_stream` (589-622) | resolve `Admission` once after `quarry_token`; key via `adm.key("search-web", [query, limit, include_answer, filters_sig])`; `get_admitted` / `put_admitted`; stale fallback only when admitted; add `"zdr": true` to the `/v1/search`, `/v1/scrape` (both payloads, 116-136) and `/v1/answer/stream` payloads when ZDR; drop `target = %target` from the warn at 154-158 | [C] |
| `GW/src/domains/knowledge/quarry.rs` | `scrape_preview` (62-203); `CacheAdmission` (212-282) removed; tests 814-912 moved | key `adm.key("scrape-preview", [target, variant_digest(body)])` where `variant_digest` = SHA-256 of the canonical (recursively key-sorted) body minus `url`, `cache`, `zdr`; deny list passed to `resolve`; `target` removed from logs 148-151, 169, 187 | [C] |
| `GW/src/observability.rs` | `record_result_cache(ns, outcome)` | `metrics::counter!("gateway_result_cache_total", "ns" => .., "outcome" => ..)`; no org/user label | [C] |
| `GW/src/main.rs` | `mod cache_admission;` | one line in the mod list (24-39) | [C] |
| `GW/tests/cache_admission_only.rs` | `content_cache_is_only_reached_through_admission` | dependency-free source scan (idiom of `tests/ssrf_forward_not_fetch.rs`): `cache.lookup(` / `cache.store(` forbidden under `src/domains/**` and `src/domains/search.rs`; allowlist `cache.rs`, `cache_admission.rs`, `upstream.rs` (session-context, user+org key), `middleware.rs`, `zdr.rs`, `domains/browser.rs` (run owner key), `gdpr_nats.rs` | [NEW] |

**API / data model / migration.** No HTTP change. Redis key shape changes: `verevon:gw:{ns}:o:{org}:{sha256}` (new) versus `verevon:gw:{ns}:{16 hex}` (legacy, never read again). Stored envelope gains `scope`. Counter `gateway_result_cache_total{ns,outcome}` is new. No SQL.

**Flags, rollout, rollback.** Correctness fix: always on. Emergency lever `GATEWAY_RESULT_CACHE_CONTENT=off` (default on) disables the two content caches without touching session or posture caches. Rollout: the whole point is that old gateways must stop serving the legacy keys, so roll all gateway replicas, then run S-2 purge. **Rollback caution:** rolling back to the previous image re-opens the hole; if a rollback is needed, roll back with `GATEWAY_CACHE_REDIS_URL` unset (cache disabled), never with it set.

**Tests.**
- Unit, `cache_admission.rs`: `scoped_keys_differ_by_org_and_variant`; `an_org_id_with_separator_or_glob_characters_is_not_a_scope`; `a_scope_tag_mismatch_is_a_miss_not_a_hit` (entry written under org A's key but tagged org B); `org_lookup_failure_denies_both_directions` (org-core mock 500); `no_authorized_org_denies`; `request_shape_with_headers_actions_profile_or_ingest_denies`; the six moved tests (`a_zdr_request_neither_reads_nor_writes_the_local_preview_cache`, `an_org_under_zdr_caches_no_page_content_at_all`, `an_ordinary_request_in_a_non_zdr_org_still_caches`, `the_callers_cache_mode_is_honoured_the_way_quarry_edge_honours_it`, `no_policy_keeps_the_previous_defaults`, `max_age_s_narrows_the_freshness_window`).
- Handler tests in `search.rs` (call `search_web` directly with `State`/`Extension`; wiremock for Quarry and org-core; `ResultCache::in_memory()`): `search_web_isolates_cached_results_by_org` (the cross-tenant test: org A caches, org B with the same query reaches Quarry, mock `.expect(2)`, B receives B's body); `search_web_same_org_second_request_is_a_cache_hit`; **`a_zdr_org_search_leaves_no_cache_entry_and_tells_quarry`** (the audit's one-test quick win: `cache.len() == 0`, second request reaches Quarry again, forwarded body has `"zdr": true`); `explicit_zdr_header_or_body_skips_cache_for_a_non_zdr_org`; `org_core_outage_skips_the_cache_but_not_the_search`; `stale_entry_is_not_served_across_orgs_when_quarry_fails`; `stale_entry_is_served_to_the_same_org_when_quarry_fails` (availability regression guard).
- `knowledge/quarry.rs`: `scrape_preview_key_differs_by_org`, `scrape_preview_key_differs_by_render_variant`.
- Run: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" cache_admission`, then `... search_web`, `... zdr`, `... --test cache_admission_only`, then the full CI line above.

**Definition of done.** All tests green including clippy `-D warnings`; no `cache.lookup/store` in content handlers (scan test); the audit's two named quick wins hold (ZDR org leaves no entry; org A cannot read org B's entry); metrics visible on `/metrics`; S-2 purge executed and recorded.

**Deployment evidence required.** Rebuild the gateway image (prod overlay needs `SOURCE_REVISION` and `BUILD_DATE`; `GET /version` must show the new revision) and run it with `GATEWAY_CACHE_REDIS_URL` pointing at a scratch Redis. Flow: (1) as org A user `POST /api/v1/search/web {"query":"d4-probe-<uuid>","includeAnswer":true}` twice. Assert: Quarry's log shows exactly one `search.upstream` for the probe; `redis-cli --scan --pattern 'verevon:gw:search-web:o:<orgA>:*'` returns 1 key. (2) As org B the same query. Assert: a second `search.upstream` appears; `...o:<orgB>:*` has 1 key; the two keys differ. (3) Set org B's `metadata.interactiveRetention.zdr=true` through the org settings API, wait 61 s (posture memo, `zdr.rs` 37), repeat twice. Assert: two more `search.upstream` events, no new key under org B, `/metrics` shows `gateway_result_cache_total{ns="search-web",outcome="bypass_zdr"} 2`, and Quarry's `search.result_shape` event for those calls has `cache_hit=false`. (4) Stop org-core and repeat as org A (non-ZDR, memo expired): assert `bypass_zdr_unknown` increments and no key changes.

**Effort.** S, 3 engineer-days (1.5 code, 1 tests incl. the in-memory backend, 0.5 evidence run).

**Risks and mitigations.** Lower cross-org hit rate (accepted by the audit). Every `AppState { .. }` literal is untouched because the switch lives inside `ResultCache` (five literals exist: `main.rs:312`, `browser.rs:4915`, `chat/shared.rs:745`, `orchestration.rs:632`, `config.rs:146`). Forwarding `zdr:true` makes Quarry stricter for temporary-chat searches: intended, covered by the handler test. `org_posture` adds one org-core call on a cold 60 s memo for every cached-path request (the scrape preview already paid this): bounded by the memo.

---

### S-2: Incident response: evidence, purge, hit-rate (answers open question 3)

**Goal.** Establish whether the cache was ever on, snapshot what an org-derived entry population looked like BEFORE it is destroyed, purge legacy entries after S-1 is everywhere, and leave a record.

**Steps and files.**

| Step | Action | Artifact | Tag |
|---|---|---|---|
| 0 | For every running gateway (dev, staging, any production): count `GATEWAY_CACHE_REDIS_URL` in the container env WITHOUT printing it (`docker inspect <gw> --format '{{range .Config.Env}}{{println .}}{{end}}' \| grep -c '^GATEWAY_CACHE_REDIS_URL='`; the value holds a password), read `GET /version` for `SOURCE_REVISION`/`BUILD_DATE`, and the deployment history of the env file. Output: a table `env x cache on/off x since revision`. If production never had it on, close as "reachable code, never reachable in production" and still ship S-1. | `docs/runbooks/gateway-search-cache-tenant-incident.md` | [NEW] (directory may need creating) |
| 1 | Read-only snapshot per Redis that has it: `apps/Frontend Plane/verevonv3/scripts/gateway-cache-forensics.sh`: `redis-cli --scan --pattern 'verevon:gw:search-web:*'` and `'verevon:gw:scrape-preview:*'`, then per key `GET` piped to `jq` that emits ONLY aggregates: entry count, age histogram from `cached_at`, `answer != null` count, count of entries with any `results[].provider == "tantivy_local"` (the org-private provider name, `local_index.rs` 597) and the distinct provider set (so unknown org-private provider names, e.g. the Data Plane vector leg when `QUARRY_EDGE__VECTOR_RETRIEVAL` is on, show up). Never print result URLs, titles, snippets or answers. | `scripts/gateway-cache-forensics.sh` | [NEW] |
| 2 | Hit-rate: `redis-cli INFO stats` (`keyspace_hits`, `keyspace_misses`, coarse because rate-limit and session keys share the instance) and the precise window comparison: count `POST /api/v1/search/web` lines in the frontend nginx access log (`docker logs <frontend>`) versus count of `search.upstream` events in the Quarry edge log over the same window; the difference is requests that never reached Quarry. | in the runbook | [NEW] |
| 3 | Deploy S-1 to every gateway replica. | | |
| 4 | Purge hygiene: `scripts/gateway-cache-purge.sh` (dry-run by default, `--apply`): `SCAN` for `verevon:gw:search-web:*` and `verevon:gw:scrape-preview:*`, SKIP keys containing `:o:`, `UNLINK` in batches of 100. Never `FLUSHDB`/`FLUSHALL` (shared Dragonfly in the e2e compose; DB 4 also holds rate-limit and session-validation keys). | `scripts/gateway-cache-purge.sh` | [NEW] |
| 5 | Verify: legacy key count 0; new-shape count unchanged by the purge; `gateway_result_cache_total` shows `store`/`hit` for admitted traffic. Record findings and the sign-off in the runbook. | | |

**Flags/rollback.** None; read-only until `--apply`. The purge is irreversible by design but only removes a re-derivable cache.

**Tests.** Script self-test against a scratch Redis seeded with legacy-shaped and new-shaped keys (`redis-cli SET verevon:gw:search-web:deadbeefdeadbeef '{"cached_at":1,"data":{"results":[{"provider":"tantivy_local"}],"answer":"x"}}'` and a `...:o:org-x:<64 hex>` key): forensics reports 1 legacy entry with answer and tantivy_local; purge dry-run lists 1; `--apply` removes it; the `:o:` key survives.

**Definition of done.** Runbook filled in with step 0 results; forensics output attached to the incident record (aggregates only); purge executed; open question 3 answered in writing with one of the two honest outcomes in 3.6.

**Deployment evidence.** The script run against the evidence stack from S-1 (seed legacy keys with an old-image run or by hand), showing dry-run, apply, and the surviving new-shape keys.

**Effort.** S, 1 engineer-day. **Risk.** The purge script's glob must exclude the new shape; covered by the self-test. Forensics reads cached payloads that may contain org-private snippets: the script must not write them anywhere (pipe-only, aggregate output), and whoever runs it needs the same access they already have to Redis.

---

### S-3: Org erasure reaches the gateway result cache (F-t2-003 follow-on, compliance)

**Goal.** An organisation erasure event removes that org's gateway content-cache entries, now that the key makes them findable.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `GW/src/cache.rs` | `ResultCache::delete_prefix(pattern)` | `SCAN MATCH` + `UNLINK` batches; no-op when disabled | [C] |
| `GW/src/gdpr_nats.rs` | `handle_message`, `Action::PurgeOrg` (326-337) | after deleting the `org-zdr` key, delete `verevon:gw:*:o:{org}:*` when the org id passes `CacheScope` validation (an id that fails validation could never have been stored) | [C] |
| `docs/compliance/evidence/retention-and-erasure-evidence.md` | row W2 | replace "Other keys are hashed and cannot be found by subject" for result keys with the org-prefix purge; keep the 24 h expiry note | [M] append-only edit |

**Tests.** `an_org_erasure_purges_that_orgs_result_cache_entries_only` (in-memory backend: org A and org B entries plus A's `org-zdr`; after `PurgeOrg{A}` only B's remain), `an_org_id_with_glob_characters_purges_nothing`. **Flags.** none. **DoD.** tests green, evidence row updated. **Evidence.** publish an organisation erasure message (`verevon.gdpr.erasure.requested`, the consumer's subject per `QCT/cmd/control/main.go` comment and `gdpr_nats.rs`) on a stack that has the shared NATS broker and the gateway's consumer env, with org A and org B keys present: A's keys gone, B's intact. If the shared broker is not available on the evidence stack, record that the proof is the unit test plus a direct `delete_prefix` call, and say so. **Effort.** S, 0.75 day. **Risk.** `SCAN` on a large shared keyspace: bounded by the pattern and `COUNT 500` batching; the event is rare.

---

### S-4: Close the known ZDR leaks in logs and meter metadata (F-t2-004 phase 1a)

**Goal.** Remove every leak that is proven reachable from a ZDR request today, with a test that would have caught the INFO-level one.

| Path | Symbol / lines | Change | Tag |
|---|---|---|---|
| `QE/src/change_routes.rs` | `emit_change_side_effects` 632-637 | drop `url`; keep `org_id` (an identifier, not content) | [C] |
| `QR/src/pipeline.rs` | 821-829 `"page extracted"` | `if self.zdr.is_active()` log only `driver`, `word_count`, `excerpt_chars`; otherwise unchanged. Same treatment for 583 (`url` on a branding serialisation warn) | [C] |
| `QR/src/serp.rs` | Brave, Serper, SearXNG adapters (the `format!("brave: {e}")`, `"serper: {e}"`, `"searxng: {e}"`, images/videos variants, lines 383-786) | `e.without_url()` before formatting, so the error text never embeds the request URL or query | [M] edits outside the in-flight hunks (21, 92, 289-308, 1083) |
| `QE/src/routes.rs` | scrape meter metadata 491-509 | when `zdr.is_active()` omit `url` and `fingerprint` from the metadata (keep `zdr: true`, status, metric, quantity). F-t2-046 owns the wider metering rewrite of this block: land whichever comes first and rebase | [M] one hunk |
| `GW/src/domains/search.rs` 154-158, `GW/src/domains/knowledge/quarry.rs` 148-151, 169, 187, `GW/src/domains/knowledge/products.rs` 96 | `target = %target` | removed (status, code and elapsed remain); the gateway has no request id on these lines, so add `request_id` only if S-5 introduces one | [C] |
| `QE/tests/support/zdr_harness.rs` | token fixture, `Harness`, `RecordingDriver`, `RecordingMeter` | copy of the private helpers in `zdr_claim_floor.rs` (which stays untouched; de-duplicate after that file commits), declared with `#[path = "support/zdr_harness.rs"] mod zdr_harness;` so `support/mod.rs` [M] is not edited | [NEW] |
| `QE/tests/zdr_log_capture.rs` | see tests | own test binary so a global capturing subscriber is safe | [NEW] |

**API/data/migration.** None. The scrape meter's metadata shape changes for ZDR only (billing-core persists whatever it receives; fewer keys, no schema change).

**Flags, rollout, rollback.** None; each is a log or metadata removal. Rollback = revert.

**Tests.**
- `zdr_log_capture.rs::a_zdr_scrape_logs_no_url_and_no_title_at_the_default_level`: install a `tracing_subscriber::fmt().json()` subscriber with `EnvFilter("info")` and an in-memory writer, run the real router with the golden ZDR-org token against the recording driver (`https://example.com/zdr-probe-<uuid>`), assert the captured text contains neither the probe URL, the host, nor the page title. Control in the same file, `a_non_zdr_scrape_still_logs_page_extracted_with_the_url`, proves the capture works and that the event exists. This test FAILS on the current tree (pipeline.rs 821): that is the point.
- `zdr_log_capture.rs::scrape_meter_metadata_under_zdr_has_no_url_or_fingerprint` (extends the `metered_zdr` idea from `zdr_claim_floor.rs` 379-388 to key names).
- `serp.rs` unit test `search_provider_errors_do_not_embed_the_query`: `SearXNGSearch::new("http://127.0.0.1:1")`, search for `SECRET-QUERY-<uuid>`, assert the returned `QuarryError.message` does not contain it; plus a probe test documenting the reqwest behaviour (`reqwest::get("http://127.0.0.1:1/?q=SECRET").await.unwrap_err().to_string()` contains `SECRET`), so a reqwest upgrade that changes it is noticed.
- Gateway: existing tests plus a source-scan assertion added in S-6.
- Run: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test zdr_log_capture`, `cargo test -p quarry-runtime serp`, `cargo test -p quarry-edge --test zdr_claim_floor` (must stay green).

**Definition of done.** The capture test is green for ZDR and the control proves the line still exists for non-ZDR; no `url`/`title` field in the ZDR branch of `change_routes.rs`; provider errors carry no query.

**Deployment evidence.** Rebuild the Quarry edge image (`make up`), run with `RUST_LOG=info` (the default). As a ZDR-org user (token floor) `POST /v1/scrape {"url":"https://example.com/zdr-probe-<uuid>"}`; assert `docker logs <quarry-edge> 2>&1 \| grep -c "zdr-probe-<uuid>"` is 0. As a standard-org user the same: assert the count is at least 1 (the line is still there for ops). With SearXNG pointed at a dead host, a ZDR-org `/v1/search` must log the provider failure without the query string.

**Effort.** S, 1.5 engineer-days. **Risks.** `pipeline.rs` is the F-t2-005/023 hotspot (formats, inline return): keep the diff to the one log statement. Removing the URL from the non-ZDR `serp` error text loses a debugging aid: `provider` and `status` remain, and the class code is added in S-5.

---

### S-5: ZDR carrier and event filter for logs (F-t2-004 phase 1b)

**Goal.** Posture travels with the request span; events whose callsite carries a sensitive field name are dropped when the span scope is ZDR; the request span no longer prints the query string.

**Quarry changes.**

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QCO/src/zdr.rs` | `ZDR_SPAN_FIELD`, `ZdrMode::mark_current_span()` | `tracing::Span::current().record("zdr", self.is_active())`; no-op without the declared field; tests | [C] |
| `QE/src/zdr_log.rs` | `ZdrMarkLayer`, `SENSITIVE_FIELDS`, `event_filter()`, `make_request_span`, `mark_floor` middleware, `class`, `class_reqwest`, `build_json_layer(writer)` | marker layer (sticky, children inherit); `DynFilterFn` using the span scope and `meta.fields()`; path-only request span with `zdr = field::Empty`; middleware recording `zdr = true` when `claims.zdr_floor()` | [NEW] |
| `QE/src/lib.rs` | `pub mod zdr_log;` | one line (file is [M] with one in-flight line) | [M] |
| `QE/src/telemetry.rs` | `init_telemetry` 25-56 | fmt layer built by `zdr_log::build_json_layer`, OTLP layer gets the same filter; `QUARRY_ZDR_LOG_FILTER=off` emergency lever; references `quarry_edge::zdr_log` (as `main.rs:1508` does for `fleet_routes`), so `main.rs`'s hand-written `mod` list is untouched | [C] |
| `QE/src/routes.rs` | 303, 286 | `TraceLayer::new_for_http().make_span_with(crate::zdr_log::make_request_span)`; insert `.layer(middleware::from_fn(crate::zdr_log::mark_floor))` before the `require_service_route_scope` layer (286) so it runs after `require_auth` | [M] two small hunks |
| posture decision points | `routes.rs` 416, 1072; `search_routes.rs` 727, 1928-1992; `answer_routes.rs` 89, 187; `extract_routes.rs` 237; `agent_routes.rs` 1443 and each step handler that loads `entry.zdr` | one added line `zdr.mark_current_span()` next to each existing `effective_zdr_mode` call | [M]/[C] mixed; one line each |

**Gateway changes.**

| Path | Change | Tag |
|---|---|---|
| `GW/src/zdr_log.rs` | the same marker layer and filter, about 80 lines (a documented contract, not shared code: each plane owns its telemetry init, per the F-t2-004 constraints) | [NEW] |
| `GW/src/main.rs` | tracing init 47-50 becomes `registry().with(EnvFilter::from_default_env()).with(ZdrMarkLayer).with(fmt::layer().json().with_filter(event_filter()))`; `make_http_trace_span` 194-202 gains `zdr = tracing::field::Empty`; `mod zdr_log;` | [C] |
| `GW/src/cache_admission.rs`, `GW/src/domains/chat/shared.rs` (call site of `request_zdr`, line 111) | `Span::current().record("zdr", true)` when the resolved posture is ZDR | [NEW]/[C] |

**API/data.** None. New env `QUARRY_ZDR_LOG_FILTER` (default on). Field vocabulary `error_class` added to logs.

**Tests.**
- `zdr_log.rs` unit tests with a capturing writer: `a_zdr_span_drops_events_with_sensitive_fields`, `a_zdr_span_keeps_events_with_only_safe_fields`, `a_non_zdr_span_keeps_everything`, `a_late_record_true_drops_later_events_only`, `children_inherit_the_marker`, `a_later_record_false_cannot_lower_it`, `the_request_span_never_prints_the_query_string`, `the_floor_middleware_marks_a_zdr_org_user_before_the_handler_runs`.
- `QE/tests/zdr_log_capture.rs` extended: `a_zdr_search_with_a_failing_searxng_logs_no_query` (the `smart_router.rs` 543 / `serp.rs` 350 path now dropped by the filter even if an error string slipped through), `a_zdr_agent_start_logs_no_url`.
- Gateway: `zdr_log` unit tests; handler test that a ZDR search emits no event containing the query.
- Run: `cargo test -p quarry-core zdr`, `cargo test -p quarry-edge zdr_log`, `cargo test -p quarry-edge --test zdr_log_capture`; gateway `cargo test ... zdr_log`.

**Definition of done.** Filter tests green on both planes; one real ZDR request per route family in the capture test shows no sensitive-named field in the captured output; `telemetry.rs` and gateway init use the filtered layers.

**Deployment evidence.** Rebuild the Quarry edge and gateway images; with `RUST_LOG=debug` (worst case) run the S-4 probe flow for scrape, search (SearXNG failing), and an agent run start; assert the probe marker, the host and the query are absent from `docker logs` for the ZDR identities and present for the standard identity at debug level (control). Also with `RUST_LOG=debug`, `GET /v1/change/latest?url=https://example.com/zdr-probe-<uuid>` must not print the URL in any span field.

**Effort.** S/M, 3 engineer-days (2 Quarry, 1 gateway). **Risks.** `DynFilterFn` reading the span scope inside a per-layer filter: prove with the first unit test before wiring (if `lookup_current` is unavailable inside `enabled` on the pinned `tracing-subscriber` 0.3.x, fall back to `Layer::on_event` + a wrapping `FormatEvent` that skips the line). Events emitted in `tokio::spawn`ed tasks lose the span: rule R4 in S-6 catches new ones; existing ones (`pipeline.rs` writeback, ingest) already only log ids. The filter drops the whole event, so a ZDR-scope `error!(error = %e, ...)` becomes invisible until it is rewritten to `error_class`: S-6 converts the request-path ones. Hotspots: `routes.rs` 286/303 with F-t2-014 (request-id layer), `telemetry.rs` with F-t2-029, `main.rs` 47-50 (gateway) with F-t2-014/046.

---

### S-6: Reqwest URL stripping sweep and the log-field discipline lint (F-t2-004 phase 1c)

**Goal.** The ~250-300 call-site sweep becomes a ratchet: today's violations are listed, new ones fail CI, and the reqwest error-text class is closed at its source.

| Path | Change | Tag |
|---|---|---|
| `QE/tests/log_field_discipline.rs` | scans `../*/src/**/*.rs` (all Quarry crates; excludes `tests/`, files after a `#[cfg(test)]` line, nested `.claude/worktrees`) for rules R1-R4 (3.11); `// zdr-log-ok: <reason>` exemption; prints counts per `file::rule`; `LOG_FIELD_BASELINE_UPDATE=1` regenerates | [NEW] |
| `QE/tests/fixtures/log-field-baseline.json` | initial baseline (expected size: about 110-160 `file::rule` counts, dominated by `quarry-edge/src/main.rs`, `quarry-browser/src/chromiumoxide.rs`, `quarry-runtime/src/{pipeline,fallback_driver,action_runtime,publisher,rerank,vector_index}.rs`); it may only shrink | [NEW] |
| `GW/tests/log_field_discipline.rs`, `GW/tests/fixtures/log-field-baseline.json` | same test for the gateway (80 macro lines in 22 files; sensitive sites are `upstream.rs` `url`/`error` pairs at 1080-1085, 1146-1151, 1238-1243, 1605-1610, 1848-1853, 2891, 2928 and the `domains/auth/public.rs` `error = %err` lines) | [NEW] |
| `QR/src/{fetch,mp_client,http3,ingest_client,robots_cache,data_plane_http,...}.rs` and `QE/src/{audio_routes,handoff,...}.rs` | `without_url()` at every reqwest-to-`QuarryError`/log conversion (about 35-45 sites; list with `rg -l "reqwest" crates/*/src`, then `rg -n "format!\(\s*\"[^\"]*\{e\}" <file>` per file) | [C]/[M] |
| request-path `error = %e` sites on routes a ZDR caller reaches (`routes.rs`, `search_routes.rs`, `pipeline.rs`, `fetch.rs`, `fallback_driver.rs`) | `error_class = zdr_log::class(&e)` instead of raw `error = %e` | [M]/[C] |

**Recipes (exact).**
- Count macros (excludes nested worktrees and `target/`): `rg -c --glob '*.rs' -e '\b(?:tracing::)?(?:trace|debug|info|warn|error)!\s*\(' "apps/Ingestion Plane/Quarry-v2/crates"` (live: 310 lines, 51 files; gateway `GW/src`: 80 lines, 22 files).
- Candidate sensitive sites: `rg -U --glob '*.rs' -e '\b(?:tracing::)?(?:trace|debug|info|warn|error)!\((?:[^;]{0,400}?)(?:\b(?:url|uri|query|source_url|target_url|selector|snippet|title|text|markdown|host|domain|origin|path)\b\s*(?:=|,|\))|\b(?:error|err|e)\b\s*=\s*[%?]|[%?]e\b|[%?]err\b)' "apps/Ingestion Plane/Quarry-v2/crates"` (live: 161 matches in 41 files; S6's narrower heuristic gave 111).
- Field-only view: `rg -n --glob '*.rs' -e '^\s*(?:url|source_url|target_url|uri|query|selector|snippet|title|host|domain|origin|text)\b\s*=\s*[%?&]?' <crate dir>`.
- reqwest sites missing `without_url`: `rg -n --glob '*.rs' -e 'without_url' crates` (live: 0 hits in Quarry and in the gateway).

**Tests.** The lint test itself, plus its fixtures (`log_field_lint_flags_a_new_url_field`, `..._accepts_a_zdr_log_ok_comment_with_a_reason`, `..._rejects_an_empty_reason`, `..._ratchet_fails_when_a_count_falls_without_the_baseline_being_lowered`) as unit tests over strings. Run: `cargo test -p quarry-edge --test log_field_discipline`; gateway `cargo test --test log_field_discipline`. They run inside CI's existing `cargo test --workspace` and gateway `cargo test`.

**Definition of done.** Baseline committed; the three S-4 leaks are absent from it; a deliberately added `tracing::info!(url = %u, ...)` fails CI; every request-path raw `error = %e` reachable by ZDR callers is converted.

**Deployment evidence.** CI run on the PR showing the lint test executed and a red run on a throwaway commit that adds a violation (attach the failing log), then green after revert. Behaviour evidence is S-5's.

**Effort.** M-ish, 2.5 engineer-days (1.5 lint and baseline, 1 `without_url` sweep). **Risks.** Regex scan false positives: the `zdr-log-ok` exemption with a reason is the escape hatch, and the baseline absorbs the existing set; false negatives (a field built through a helper) are the accepted trade-off for a dependency-free scan, stated in the test's header as the gateway tests do. Large mechanical diff in many [C]/[M] files: do the `without_url` sweep as its own PR so reviewers can skim it.

---

### S-7: Quarry ZDR feature matrix v1: table, route-scan completeness, behaviour tests (F-t2-003 phase 2a)

**Goal.** A declared per-route ZDR treatment that CI keeps true in both directions: a new route without a classification fails; a documented refusal that is not enforced fails; a known gap is pinned so fixing it forces the matrix to change. Uses only the Model Plane catalogue's existing code strings (3.8). Documents today's behaviour; changes none.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QCO/src/zdr_matrix.rs` | `Treatment { Honored, InlineOnly, Degraded{warning}, Refused{code, feature}, Inert{reason} }`, `Row { id, routes, treatment, enforcement, status: Enforced \| Gap{issue} }`, `ROWS`, `to_json()`, `to_markdown()` | the source of truth; one row per feature in 3.3 plus one row per route family it does not cover (the 85 literal `.route(` registrations: `routes.rs` 56, `agent_routes.rs` 22, `fleet_routes.rs` 4, `queue_routes.rs` 3) | [NEW] |
| `QCO/src/lib.rs` | `pub mod zdr_matrix;` | one line | [C] |
| `QR/contracts/zdr-feature-matrix.json` (+ `.sha256`) | generated golden | `QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix` writes both; same pin pattern as `cross-plane-token-requests.json` (`QR/src/token_request_contract.rs` [M] compares the sha file) | [NEW] |
| `apps/Ingestion Plane/Quarry-v2/docs/ZDR_MATRIX.md` | generated human table | written by the same generator; the test fails if the checked-in file differs from `to_markdown()` | [NEW] |
| `QE/tests/zdr_feature_matrix.rs` | tests | below; reuses `tests/support/zdr_harness.rs` from S-4 | [NEW] |
| `scripts/tests/zdr-contract-sync.test.sh` | GOLDENS row for the matrix golden | append-only, last row; if the [M] diff (95 lines) has not committed, ship S-7 with the Rust-side sha pin only and add the row afterwards | [M] |

**API/data.** None on the wire. Golden JSON shape: `{contract: "quarry.zdr-feature-matrix", version: 1, catalogue: {contract: "model-plane.zdr-refusals"}, treatments: [...], rows: [{id, routes, treatment, code?, feature?, http?, enforcement, status, issue?}], out_of_catalogue: ["ZDR_REQUIRED"]}`.

**Tests (`QE/tests/zdr_feature_matrix.rs`).**
- `every_registered_route_has_a_matrix_row`: scan `QE/src/*.rs` with `\.route\(\s*"([^"]+)"` (multi-line aware), normalise `:id` to `{id}`; every path must match a row's `routes` pattern. `every_row_matches_a_registered_route`: stale rows fail.
- `the_golden_and_markdown_are_what_the_table_generates`.
- `every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list`: a local list of the 10 code strings and 30 feature keys for now (replaced by the vendored file in S-9; the strings are the contract).
- `enforced_refusals_hold`: for each `Refused` + `Enforced` row, a golden ZDR-org token (`user_in_zdr_org`) gets the row's HTTP status and a message containing "ZDR", and a standard-org token does not (crawl, batch, persistent profile at agent start, resume).
- `a_service_principal_with_body_zdr_gets_the_same_refusals`: the `service_model_gateway` golden token with body `zdr: true` (the shape of an agent web tool call) is refused on the same rows, and the same token with `zdr: false` is not (the service `zdr` claim is ignored by design, `auth.rs` 104-120).
- `gap_rows_currently_violate`: for each `Gap` row, assert today's behaviour so a fix must flip the row: ZDR-org user `POST /v1/schedules {preset:"hourly", target_ref}` returns 200 and reaches the control double (gap, 3.3 rows 6-7); `/v1/profiles` create and `GET` succeed (row 16); `POST /v1/change/snapshot` and `/v1/change/refresh` are not refused (row 6); the scrape meter metadata (if S-4 not yet landed) contains `url` (row 21).
- Run: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core zdr_matrix && cargo test -p quarry-edge --test zdr_feature_matrix`.

**Flags/rollout/rollback.** None; test and documentation only. **Definition of done.** Both directions enforced by CI; `ZDR_MATRIX.md` readable by support; each gap row names an owner (F-t2-016/018/020/030/046/019, or S-8).

**Deployment evidence.** The behavioural tests ARE the end-to-end flow against the real router, real RS256 verification and golden Auth Core claims (the `zdr_claim_floor.rs` harness idea). Plus one live check on the running stack (`make up`): with a real ZDR-org token, `POST /v1/crawl` returns 403 and `POST /v1/schedules {preset}` returns 200 (the documented gap), matching the matrix.

**Effort.** M, 6 engineer-days (2 table and generator, 2 route scan and behaviour tests, 1 harness reuse, 1 review of the 85-route classification). **Risks.** Classification review is the cost: a wrong `Honored` row is a false assurance, so each `Honored` row names the code that makes it true (`enforcement`). Route-scan regex misses routes added through helpers (`nest`, loops); the test also fails if the count of registered literals drops, and the header documents the limit, as the gateway's scan tests do. Hotspot: none in source (no edits to `routes.rs`).

---

### S-8: Enforce the gap rows with catalogue-coded refusals (F-t2-003 phase 2b, ADR-GATED topic 4)

**Goal.** After the ADR: ZDR callers cannot create durable resources through the routes the matrix lists as gaps, and every Quarry ZDR refusal carries a machine code from the Model Plane catalogue.

| Path | Change | Tag |
|---|---|---|
| `QE/src/zdr_refusal.rs` | `refuse(claims, requested, request_id, row_id)` looks the row up in `ROWS` and builds `QuarryError::new(ErrorCode::Forbidden, <existing-style message containing "ZDR">)` with `details = {"zdr_refusal": {code, feature?, action}, "legacy_code"?: ...}`; emits a content-free `zdr.refused` event (`row`, `org_id`) | [NEW] |
| `QE/src/routes.rs` | `refuse_durable_job_under_zdr` (563-588) delegates to it, message text unchanged | [M] one hunk |
| `QE/src/schedule_routes.rs` | refuse `create_schedule` (239), `trigger`, `backfill`, `unpause` for ZDR callers; allow `list`, `get`, `pause`, `delete` so a ZDR org can wind down | [C] |
| `QE/src/change_routes.rs` | refuse `snapshot` and `refresh` (the persisting routes); internal `record_check` refuses to persist a baseline when its body declares `zdr: true` (today it only suppresses side effects, 623-639) | [C] |
| `QE/src/profile_routes.rs` | refuse all `/v1/profiles*` for ZDR callers | [C] |
| `QE/src/agent_routes.rs` | profile refusal (1469-1476) and resume refusal (1347-1353) gain `details.zdr_refusal` and `legacy_code: "zdr_persistent_profile_forbidden"`; HTTP statuses unchanged | [C] |
| `QE/src/config.rs` | `zdr_refuse_durable_resources: bool` (`QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`), default **false** in the first release | [M] append-only |
| `QCO/src/zdr_matrix.rs` | flip the affected rows from `Gap` to `Enforced` | [NEW from S-7] |

**Rollout.** Stage A (dark): flag off, the refusal path runs in observe mode and emits `zdr.would_refuse{row}`; read it for a week to learn which ZDR orgs would be affected. Stage B: after the ADR sign-off and customer notice, default the flag on. Rollback: set the flag false. Rows that were already refused (crawl, batch, profile at run start, resume) are NOT behind the flag: they only gain `details`.

**Tests.** The `gap_rows_currently_violate` cases invert (`zdr_org_cannot_create_a_schedule`, `..._monitor`, `..._profile`, `..._snapshot`) with `details.zdr_refusal.code == "zdr_durable_mutation_forbidden"`; standard-org requests unchanged (200); wind-down routes still 200 for ZDR callers; existing `zdr_claim_floor.rs::durable_crawl_and_batch_are_refused_...` stays green (message still contains "ZDR"); `a_refused_request_never_reaches_the_control_plane` (control double records zero requests, the assertion already used at `zdr_claim_floor.rs` 893-896).

**DoD.** Matrix has no `Gap` row for schedules, monitors, profiles; flag default on in compose after sign-off. **Evidence.** On the running stack: ZDR-org `POST /v1/schedules` returns 403 with the catalogue code in `error.details.zdr_refusal.code`; quarry-control `GET /v1/schedules` for that org stays empty; standard org still creates a schedule (200).

**Effort.** M-ish, 2.5 engineer-days. **Risks.** Behaviour change for ZDR orgs that use monitors today (open question 2): the staged flag is the mitigation. **Pre-existing schedules of an org that turns ZDR on later keep firing**: Quarry has no org-posture lookup and must not read Control Plane data; see section 9 question 3 (needs a Control Plane signal or a per-fire check owned by F-t2-018).

---

### S-9: Consume the Model Plane catalogue; add Quarry feature keys (F-t2-003 phase 2c). **WAITS** for the Z-05a/Z-05b initiative to commit

| Path | Change | Tag |
|---|---|---|
| `QR/tests/fixtures/zdr-refusals/zdr-refusals.json` (+ `.sha256`) | byte-identical vendored copy of `MPX/api/contracts/zdr-refusals.json` | [NEW] |
| `QE/tests/zdr_feature_matrix.rs` | the closed list from S-7 is replaced by loading the vendored copy (modelled on `QR/src/token_request_contract.rs`); `every_code_and_feature_the_matrix_names_is_in_the_vendored_catalogue` | [NEW from S-7] |
| `scripts/tests/zdr-contract-sync.test.sh` | VENDORED row `<quarry copy>\|apps/Model Plane/api/contracts/zdr-refusals.json`, its `.sha256` row, CONSUMERS row for quarry-edge | [M] |
| New feature keys (`durable_crawl`, `browser_profile`, `session_resume`, `page_screenshot`) | a PR INTO the Model Plane catalogue: `FEATURES` in `zdr_refusal.rs`, regenerate the golden (`MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal`), new sha in the sync GOLDENS row, refresh every vendored copy (model-gateway fixtures, SPA `src/shared/api/contracts/zdr-refusals.json`), SPA `ZdrFeatureKey` union, `ZDR_REFUSALS_CONTRACT_SHA256`, `zdr-refusals.test.ts` | owned by the Model Plane initiative: [U]/[M] files |
| `GW` BFF | map `error.details.zdr_refusal` from Quarry onto the SPA's refusal handling | [C], after `zdr-refusals.ts` [U] commits |

**Tests/gate.** `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`, `node --test scripts/tests/zdr-contract-schema.test.mjs`, the matrix test. **Evidence.** The sync script reports the new rows green; a Quarry refusal rendered in the SPA with the catalogue's Norwegian text. **Effort.** S, 1.5 engineer-days Quarry side plus the Model Plane side's PR. **Risk.** The catalogue is generated and unreleased; if its digest changes before commit, the vendored copy must be re-vendored: that is why this slice waits.

---

### S-10: Metadata-only ZDR run tombstone with a retention janitor (F-t2-003 phase 2d). ADR-GATED (topic 4) + contract review

**Goal.** A ZDR run leaves a content-free, immutable record (counts, timings, outcome) for support and billing reconciliation, deleted after a default 30 days, without any URL, digest, query, title, user id or error text.

| Path | Symbol | Change | Tag |
|---|---|---|---|
| `QCT/internal/store/pg/migrations/015_zdr_run_tombstones.sql` | table `zdr_run_tombstones` | `run_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, kind TEXT CHECK (kind IN ('scrape','search','answer','extract','agent_run')), retention_class TEXT NOT NULL DEFAULT 'zdr' CHECK (retention_class = 'zdr'), outcome TEXT CHECK (outcome IN ('ok','error','refused','cancelled','timeout')), driver TEXT, started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ NOT NULL, step_count INT NOT NULL DEFAULT 0, cost_units NUMERIC(18,6) NOT NULL DEFAULT 0, error_class TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT now()`; indexes on `(org_id, ended_at DESC)` and `(ended_at)`; an `UPDATE`-refusing trigger (the immutability pattern of Model Plane migration 0044). 015 is the next free number after `014_fleets.sql` [C]; the directory holds single files, no down pairs | [NEW] |
| `QCT/internal/store/store.go`, `resources_v2.go`, `resources_v2_mem.go`, `pg/resources_v2.go` | `ZdrTombstones()` accessor with `Put` (idempotent `ON CONFLICT (run_id) DO NOTHING`), `DeleteBefore(cutoff, limit)`, `ListByOrg` | memory and Postgres implementations | [C] |
| `QCT/internal/resources/zdr_tombstone.go` | `MountZdrTombstones` | `POST /v1/internal/zdr-tombstones`, HMAC-verified exactly like the change webhook group (`change_webhook.go`), body cap 4 KiB, `json.Decoder.DisallowUnknownFields`, rejects any string value containing `://`; distinct from the change webhook, which must keep rejecting bodies with a truthy `zdr` | [NEW] |
| `QCT/internal/janitor/tombstones.go`, `QCT/cmd/control/main.go` (mount near `MountTeam` 190, start next to `janitor.Run` 200) | `RunTombstones` | own sweeper, default 30 days (`QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS`), `QUARRY_ZDR_TOMBSTONE_DRY_RUN`; NOT the artifact janitor (its header, `janitor.go` 14-18, says it is unsafe to arm without a provenance guard) | [NEW]/[C] |
| `QCT/internal/store/gdpr_purge.go` | include `zdr_run_tombstones` in the org-erasure purge | [C] |
| `QE/src/zdr_tombstone.rs`, `QE/src/state.rs`, `QE/tests/support/mod.rs` (`base_state`) | `TombstoneSink` (HTTP, signed with `internal_signer`), `AppState.tombstones: Option<Arc<dyn TombstoneSink>>`; emitted at run end by scrape, search, answer, extract handlers and at agent `close_run` (the ZDR `AgentCompleted` point, `agent_routes.rs` ~3630-3640); fail-open (never fails or delays the request), flag `QUARRY_EDGE__ZDR_TOMBSTONE` default **off** | [NEW]/[M]/[M] (state.rs and support/mod.rs are in another initiative's diff: land after it commits) |
| `QR/contracts/zdr-tombstone.schema.json` (+ `.sha256`), vendored copy `QCT/internal/resources/testdata/zdr-tombstone.schema.json` | the allowlist, one definition checked by both the Rust emitter test and the Go validator test; GOLDENS + VENDORED rows in the sync script | [NEW] / [M] script |

**API/data.** `POST /v1/internal/zdr-tombstones` (internal, signed). Row shape above; naming (`retention_class`, value `zdr`) aligned with `runs.retention_class` in Model Plane migration 0044 [U].

**Tests.** Go: `TestZdrTombstoneRejectsAnyFieldOutsideTheAllowlist`, `TestZdrTombstoneRejectsUrlLookingValues`, `TestZdrTombstoneIsIdempotentOnRunID`, `TestZdrTombstoneRowCannotBeUpdated` (Postgres, skipped without a DB as the existing `pg_test.go` tests are), `TestSweepDeletesOnlyPastRetention`, `TestOrgErasureRemovesTombstones`. Rust: `a_zdr_scrape_emits_exactly_one_tombstone_without_any_url`, `a_standard_scrape_emits_none`, `a_tombstone_failure_never_fails_the_request`, `the_emitted_body_validates_against_the_schema`. Run: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`; `cargo test -p quarry-edge zdr_tombstone`.

**Flags/rollout/rollback.** Edge flag default off; control route is signature-protected and inert without the edge. Enable per environment after the contract review. Rollback: flag off; rows age out via the janitor (or `DELETE` by hand; rows are metadata).

**DoD.** Contract review recorded (who, what the contract says); allowlist schema pinned in the sync script; tombstone survives only the retention window; org erasure removes it. **Evidence.** Stack with control and edge, flag on: one ZDR scrape and one ZDR agent run; `psql -c "select * from zdr_run_tombstones"` shows two rows with the expected columns; `pg_dump --table zdr_run_tombstones \| grep -c "zdr-probe-<uuid>"` is 0; set retention to 0 days for a test and show the sweeper deleting them; a standard scrape adds no row.

**Effort.** M, 5 engineer-days (1.5 Go store and route, 1 janitor and purge, 1.5 Rust emitter, 1 tests and evidence). **Risks.** A new egress path under ZDR: mitigated by the allowlist schema, `://` rejection, fail-open, flag off by default. Legal compatibility with each customer's ZDR terms is unknown (section 9 question 4). `AppState` and `support::base_state` edits collide with the in-flight QB-T17 work.

---

### 4.11 F-t2-004 phase 2: summary only (designed in D10 with F-t2-029)

Phase 2 needs spans, which today barely exist (S6: tower-http debug spans only, Go services call `quarryotel.Init` but never start a span). It adds: a `ZdrSampler` (the opentelemetry_sdk `ShouldSample` returning `Drop` when the parent context or attributes carry `zdr=true`; Go mirror in `pkg/quarryotel/quarryotel.go` 39-93, which uses `ParentBased(AlwaysSample())` at line 80); a bounded FIFO late-ZDR trace registry (Firecrawl uses a Set capped at 100,000 with oldest-first eviction; it is FIFO, not LRU) checked by a `SpanProcessor::on_end`; an explicit exporter kill switch (proposal `QUARRY_TELEMETRY_EXPORT=off`, today the only off-switch is leaving `OTEL_EXPORTER_OTLP_ENDPOINT` unset); and a documented safe/sensitive field contract mirrored in Rust and Go, not shared code. **What S-4..S-6 hand to D10:** the `zdr` span field and `ZdrMarkLayer` (the sampler reads the same carrier), the `SENSITIVE_FIELDS` constant (keep it in one file, pinned by a golden so the Go side can vendor it), the `error_class` vocabulary, and the lint baseline. Keep content-free counters outside the filter; the gateway already has them, Quarry's metrics crates stay inert until F-t2-029 installs a recorder (`telemetry.rs` 6-18).

### 4.12 Adjacent findings recorded here, owned elsewhere

- Quarry `PageCache` is keyed by fingerprint only (`QE/src/cache.rs` 31-33) and read with the caller-supplied `prev_fingerprint` (`routes.rs` 432-437): not org-scoped. F-t2-048 (flagged "to verify" in the audit): verified as code; exercise it before scoping that item.
- `session-validation` cache key is a 64-bit zero-key SipHash of the cookie header (`GW/src/middleware.rs` 465). Low risk (the attacker cannot choose a victim's cookie), but it is the same weakness class as 3.1.
- `ai_formats.rs` 302-309 sends ZDR requests to the Model Plane with an invoke request that has no `zdr` field (S4): F-t2-019.
- The gateway's `docker-compose.yml` comment about in-process caching (132-133) is wrong; fix when touching the file.

## 5. Dependencies and ordering

**Hard.**

- S-2 purge (steps 3-5) requires S-1 deployed to every gateway replica; S-2 steps 0-2 (read-only) must run BEFORE S-1 deploys, or the evidence of the old state is gone.
- S-3 requires S-1 (key shape, `ResultCache` API). S-6 requires S-5 (`error_class`, `SENSITIVE_FIELDS`) and S-4 (so the baseline does not enshrine the known leaks). S-8 requires S-7 (rows, helper) and the ADR 4 decision. S-9 requires the Model Plane Z-05a/Z-05b initiative to have committed `zdr-refusals.json`, `zdr_refusal.rs`, `zdr-refusals.ts` and the sync-script diff. S-10 requires ADR 4, the contract/legal review, S-7 (rows), and the QB-T17 `state.rs`/`support/mod.rs` hunks to have committed.
- F-t2-004 phase 2 and F-t2-029 (spans) require S-5 (audit 9.4 note 8: ship the log-field sweep first).

**Soft.**

- S-7 benefits from S-4 (shared test harness) but can copy it.
- F-t2-023 (scrape-time actions, formats) lists "F-t2-003 cache admission" as a dependency: it consumes `Admission` and its deny list from S-1, adding `formats`, `headers`, `actions`, `profile` handling in one place. F-t2-048 (org-scoped URL cache) consumes S-1's scope/key module and matrix row 5. F-t2-011 (free human browsing) lists F-t2-003: satisfied by S-1 (a human browsing session's ZDR gating uses `request_zdr`/`org_posture`).
- ADR 4 rows 6-8 gate F-t2-016 (scoped crawl), F-t2-018 (schedules that execute) and F-t2-030 (monitor quality): they inherit S-8's refusal rather than redefine it.
- F-t2-005 (inline return) turns row 12 from "unusable" into "inline-only"; F-t2-020 owns profile CRUD behaviour (row 16); F-t2-019 owns the AI-format ZDR gap (row 23); F-t2-046 owns the metering shape (row 21; S-4 only strips the scrape site).

**Merge-conflict hotspots (files and line ranges shared with other workstreams).**

| File | Lines | This work | Other work |
|---|---|---|---|
| `QE/src/routes.rs` [M] | 286-289 and 303 (router layers) | S-5 (floor middleware, span) | F-t2-014 (x-request-id layer at the TraceLayer) |
| | 488-509 (scrape meter) | S-4 (strip url, fingerprint) | F-t2-046 (metering rewrite) |
| | 345-375 (`ScrapeRequest`) | none | F-t2-005, F-t2-023 |
| | 563-588 (refusal helper) | S-8 | F-t2-016 |
| `QR/src/pipeline.rs` [C] | 821-829 | S-4 | F-t2-005/019/023 (formats, extraction) |
| `QE/src/telemetry.rs` [C] | 25-56 | S-5 | F-t2-029 (spans, recorder) |
| `GW/src/main.rs` [C] | 24-39 (mods), 47-50 (tracing init), 194-202 (span) | S-1, S-5 | F-t2-014, F-t2-046 (rate-limit layer 143-148) |
| `GW/src/domains/search.rs` [C] | 51-293, 589-622 | S-1 | F-t2-011/028 if they touch the search surface [UNVERIFIED] |
| `GW/src/domains/knowledge/quarry.rs` [C] | 62-282, tests 814-912 | S-1 | F-t2-005/023 (scrape body), F-t2-048 |
| `QE/src/schedule_routes.rs` [C] | 239-330 | S-8 | F-t2-018 (schedules that execute): same function, coordinate order |
| `QE/src/profile_routes.rs` [C] | all handlers | S-8 | F-t2-020 (cookie reads, encryption) |
| `QE/src/agent_routes.rs` [C] | 1347-1353, 1469-1476, 1443, ~3630 | S-5 (one-line marks), S-8, S-10 | F-t2-013/014/022 (grant, trace, policy) |
| `scripts/tests/zdr-contract-sync.test.sh` [M] | GOLDENS, VENDORED, CONSUMERS tails | S-7, S-9, S-10 | the in-flight 95-line diff of the Z-series initiative |
| `QE/src/state.rs`, `QE/tests/support/mod.rs`, `QE/src/config.rs` [M] | `AppState`, `base_state`, config tail | S-8 (config), S-10 (state) | QB-T17 Model Plane gate |
| `QCT/cmd/control/main.go` [C] | 186-215 | S-10 | F-t2-017/018/031 |
| `QR/src/serp.rs`, `smart_router.rs` [M] | 383-786, 543 | S-4 (`without_url`), S-6 | rerank initiative hunks at 21, 92, 289-308, 1083 / 71-77, 859 |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- **Control Plane** owns identity, orgs, billing, sessions, audit, quotas, entitlements. This work only READS the org's ZDR posture through the existing org-core API call (`zdr.rs` `org_posture`), stores none of it, and consumes the existing org-erasure event (`gdpr_nats.rs`). A later plan-based tombstone retention is read through a Control Plane API, never its database. One Control Plane-side sink is flagged, not fixed here: billing-core persists the scrape meter metadata including the URL (`repository.go` 485-488), so S-4 strips it at the producer and F-t2-046 owns any billing-core hardening.
- **Data Plane v2** is untouched. Ingest stays refused under ZDR (`zdr::guard(Ingest)`). No Data Plane table or retrieval path is read.
- **Ingestion Plane** owns the matrix and the tombstone. The tombstone lives in quarry-control's own Postgres (`015_...sql`); the edge reaches it through the existing signed internal call pattern. No cross-plane DB access anywhere in this design.
- **Frontend Plane** owns the BFF cache and its admission; the fix stays in the gateway and does not move cache logic into Quarry or the SPA. Mapping Quarry refusals onto the SPA's refusal UX is BFF normalisation (allowed) and waits on the catalogue commit.
- **Model Plane** owns the refusal catalogue and the run retention receipt. Quarry consumes the catalogue as a vendored, sha-pinned golden and invents no second taxonomy; adding Quarry feature keys is a PR into Model Plane's contract with its owner's approval (S-9). The Quarry tombstone shares the `retention_class` vocabulary but not storage.
- **Model proposes, Quarry executes or rejects**: unaffected and reinforced. Every matrix refusal is enforced inside Quarry's own handlers (token floor or body flag), whatever the Model proposed; model-gateway's own refusals remain the first line, Quarry the last. No new Model-facing capability is added. One reconciliation is flagged in section 7: model-gateway refuses model-driven browser runs under ZDR while Quarry allows a direct UI ZDR agent run.
- **ZDR must propagate through every content-persisting boundary**, state per boundary: gateway caches (S-1, fixed; fails closed); Quarry caches (already correct, protected, matrix rows 3-4); logs (S-4..S-6; ZDR scrape logs URL and title at INFO today, closed by S-4); spans and OTLP (phase 2 sampler, D10); usage meter metadata (S-4 scrape site, F-t2-046 the rest; the meter is a ZDR sink that leaks the URL today); schedules and change monitors (NOT refused today: S-8, ADR-gated); profile CRUD (not refused today: S-8); run history, receipts, artifacts, ingest, webhooks (already withheld); the new tombstone (the only deliberate retention, metadata-only by allowlist, gated by review).
- **Rules at risk today**: (1) ZDR propagation is broken at the gateway search cache, the INFO log line, the scrape meter, schedules/monitors and profile CRUD; (2) "no tenant sees another tenant's data": the gateway cache path is cross-tenant wherever the cache is enabled, and Quarry's fingerprint-keyed `PageCache` is not org-scoped (adjacent, F-t2-048); (3) plane ownership: S-9 edits a Model Plane contract from the Ingestion side, which needs the owner's approval; the e2e compose points the gateway cache at the Data Plane's Dragonfly (DB 4), an infrastructure coupling that is not DB crossing but makes `FLUSHDB` forbidden and pattern-scoped purge mandatory.

## 7. Cross-session interfaces

**(a) Verevon Space (shared workspaces; agent PC/browser in Spaces; ZDR-safe telemetry).**

| Shared piece | Proposed owner | What it means |
|---|---|---|
| Space-level ZDR as a carrier | Space session computes it; D4 guarantees the gateway honours it | Quarry's token floor knows only org posture; a Space with its own ZDR must reach Quarry as the body `zdr: true` (raise-only) and the gateway as `x-zdr`/body `zdr`. S-1 already treats `request_zdr` as ZDR for caching and forwards `zdr: true`. The matrix needs a row for "service or delegated caller with body zdr" (the `service_model_gateway` golden token exists in `zdr_claim_floor.rs`) |
| Cache scope beyond org | Space session decides if a Space-private result exists | `CacheScope` carries `org` now; design it extensible to `o:{org}:s:{space}`. Default: no Space segment; adding one is a decision by whoever makes search or retrieval Space-scoped |
| ZDR-safe telemetry for Space PC/browser sessions | D4 (S-5) provides the mechanism, D10 the phase 2 | Space-hosted runs flow through Quarry spans and the gateway: they get the `zdr` span marker for free; Space code outside the gateway and Quarry (Application Plane, Convex functions) is NOT covered by the lint and needs its own field discipline |
| Tombstone content | Space session | Tombstone carries `org_id` only; no Space id by default (a Space id can identify a project). Add only if the Space session needs per-Space support diagnostics, and then by decision |

**(b) Model Plane and chat/agents.**

| Shared piece | Proposed owner | What it means |
|---|---|---|
| Refusal catalogue (`zdr-refusals.json`, feature keys) | Model Plane initiative | Quarry adds four feature keys via a PR after their commit (S-9). Until then Quarry uses only existing codes and keys |
| Model-driven browser runs under ZDR | Model Plane | model-gateway refuses `browser_run` under issuer-enforced ZDR; Quarry allows a direct UI ZDR agent run (live-only, no receipts, no artifacts). Both are consistent today because they are different callers; the matrix documents both. If Model Plane ever allows ZDR browser runs, Quarry's enforcement (token floor or body flag, no persistent profile, no resume) is already the last line |
| Agent web tools as service principal | Model Plane (`execution-core`, `model-gateway`) | Their token's `zdr` claim is deliberately ignored by Quarry; posture arrives as the body flag set from the run's resolved posture. The matrix behavioural tests cover the service-body case so a regression in either side shows |
| Run retention receipt vs tombstone | Model Plane owns `runs.retention_class`; Quarry owns the tombstone | Same column name and value (`zdr`); no join, no shared table. A Model Plane receipt may carry a Quarry run id (an id, not content) |
| Telemetry adoption | Model Plane (`mp-telemetry`, initialised by session-core, model-gateway, inference-core, execution-core) | Adopt the documented contract (drop events with `SENSITIVE_FIELDS` in ZDR scope, `error_class`), not shared code; their natsx already drops `zdr:true` envelopes |
| SPA rendering of a Quarry refusal | Model Plane/chat lane owns `zdr-refusals.ts` | The gateway BFF maps `details.zdr_refusal` to that handling after it commits |

**What the user must reconcile:** who adds the four Quarry feature keys and when the Z-series initiative commits; whether Model Plane will ever allow model-driven ZDR browser runs; how Space ZDR reaches Quarry and the gateway; whether a Space id may appear in a cache key or tombstone.

## 8. What can proceed WITHOUT an ADR/decision, and what is blocked

**Proceed now, no decision needed:** S-1 (gateway admission and scope), S-2 (evidence, purge: needs ops access, not a decision), S-3, S-4, S-5, S-6, S-7 (documents today's behaviour and pins the gaps; changes no behaviour). Together about 18 engineer-days; S-1 and S-2 first.

**Blocked:**

| Slice | Blocked by |
|---|---|
| S-8 (enforce gap rows: schedules, monitors, profile CRUD; AI-format row) | ADR topic 4 (rows 6, 7, 16, 23 tighten behaviour for ZDR orgs), customer impact check, the pre-existing-schedule question (section 9 item 3) |
| S-9 (vendor the catalogue, add keys) | Model Plane Z-05a/Z-05b/Z-10 initiative committing `zdr-refusals.json`, `zdr_refusal.rs`, `zdr-refusals.ts`, the sync-script diff |
| S-10 (tombstone) | ADR topic 4 option choice, contract/legal review, QB-T17 hunks in `state.rs`/`support/mod.rs` committing |
| F-t2-004 phase 2 | F-t2-029 spans; designed in D10 |
| Matrix rows 12 and 13 reaching "inline-only" | F-t2-005 (inline return) and F-t2-023 (inline screenshot/PDF) |

## 9. Open questions for the user (real blockers only)

1. **Was `GATEWAY_CACHE_REDIS_URL` ever set on a gateway that is not a developer machine, and since which image revision?** The canonical compose does not set it (2.1); the e2e compose and a local `.env` do. This decides whether the incident record says "reachable in production for N days" or "never reachable outside dev". Does not block S-1.
2. **ADR topic 4, rows 6, 7, 16 (and 23): may Quarry start refusing change monitors, schedules, profile CRUD and AI formats for ZDR orgs?** Today a ZDR org can create monitors and schedules. Recommendation: refuse (the audit's ADR text already assumes they are refused), staged behind a flag with an observe-only week. Need: yes or no, and whether any ZDR customer currently depends on monitors.
3. **Who pauses schedules and monitors that predate an org turning ZDR on?** Quarry has no org-posture lookup and must not read Control Plane data. Options: an org-ZDR-change signal from Control Plane that quarry-control reacts to (an event contract is needed and does not exist [UNVERIFIED]); or the orchestrator checking posture on every fire (F-t2-018's scope). Needs an owner; blocks calling the schedule row "enforced".
4. **Is a metadata-only run tombstone acceptable under the ZDR contract and DPA, and what default retention (proposal 30 days)?** Legal or compliance answer; blocks S-10 only.
5. **Who adds Quarry's four feature keys to the Model Plane catalogue and after which commit?** Blocks S-9 and the SPA rendering of Quarry refusals.
6. **Model Plane: will model-driven browser runs ever be allowed under ZDR?** Does not block this plan; it changes the wording of one matrix row and the reconciliation in section 7.

## 10. INTERFACE SUMMARY

**Files touched.** *Gateway* (`apps/Frontend Plane/verevonv3/apps/gateway`): new `src/cache_admission.rs`, `src/zdr_log.rs`, tests `cache_admission_only.rs`, `log_field_discipline.rs`; edits `cache.rs`, `zdr.rs`, `domains/search.rs`, `domains/knowledge/{quarry,products}.rs`, `observability.rs`, `gdpr_nats.rs`, `main.rs`; scripts `gateway-cache-forensics.sh`, `gateway-cache-purge.sh`; runbook. *Quarry* (`Quarry-v2`): quarry-core `zdr.rs`, new `zdr_matrix.rs`; quarry-edge new `zdr_log.rs`, `zdr_refusal.rs`, `zdr_tombstone.rs`, tests, edits `telemetry.rs`, `routes.rs`, `schedule_routes.rs`, `change_routes.rs`, `profile_routes.rs`, `agent_routes.rs`, `config.rs`, `state.rs`; quarry-runtime `pipeline.rs`, `serp.rs`, reqwest `without_url` sweep; quarry-control migration `015_zdr_run_tombstones.sql`, store, route, janitor, `gdpr_purge.go`. *Shared*: `scripts/tests/zdr-contract-sync.test.sh` rows, compliance evidence row W2, Model Plane catalogue keys (their PR).

**New contracts.** Redis key `verevon:gw:{ns}:o:{org}:{sha256}` with `scope` tag; counter `gateway_result_cache_total{ns,outcome}`; gateway forwards `zdr:true` on search, fetch, answer; Quarry error `details.zdr_refusal{code,feature?,action}` with catalogue codes; goldens `zdr-feature-matrix.json`, `zdr-tombstone.schema.json`; `POST /v1/internal/zdr-tombstones`; table `zdr_run_tombstones`; span field `zdr`; flags `GATEWAY_RESULT_CACHE_CONTENT`, `QUARRY_ZDR_LOG_FILTER`, `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`, `QUARRY_EDGE__ZDR_TOMBSTONE`, `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS`.

**Dependencies.** S-1 then purge; S-3 after S-1; S-5 after S-4; S-6 after S-5; S-8 after S-7 and ADR 4; S-9 after the Model Plane Z-series commit; S-10 after ADR 4, legal review, QB-T17 commit; D10 after S-5.

**Effort.** S-1 3 d, S-2 1 d, S-3 0.75 d, S-4 1.5 d, S-5 3 d, S-6 2.5 d, S-7 6 d, S-8 2.5 d, S-9 1.5 d, S-10 5 d: about 27 d.

**ADR gates.** Topic 4 (ZDR per feature) gates S-8 and S-10; recommended defaults in 3.3 (refuse durable resources, inline-only scrape, metadata-only tombstone, no hash-only).

**Parallelisable with.** S-1 with S-4; S-7 with S-5/S-6; the Space and Model Plane lanes (they consume S-1/S-5 contracts).

**Conflicts with.** `routes.rs` (F-t2-014/046/005), `pipeline.rs` (F-t2-005/023), `telemetry.rs` (F-t2-029), `schedule_routes.rs` (F-t2-018), `profile_routes.rs` (F-t2-020), `zdr-contract-sync.test.sh` and `state.rs` (in-flight Z-series and QB-T17 work).

**Audit corrections that change planning:** schedules/monitors are NOT refused under ZDR; a ZDR scrape logs URL and title at INFO; the gateway cache exposure is deployment-conditional (stock compose has no cache).

<!-- COMPLETE -->
