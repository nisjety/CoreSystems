# S6 gateway-zdr-telemetry (saved by coordinator from the sweeper inline reply)

Tags: [C] committed and unchanged, [M] committed but modified in live tree, [U] untracked, [NEW] would be created.

Legend: GW = apps/Frontend Plane/verevonv3/apps/gateway/src; QE = apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src; QR = .../crates/quarry-runtime/src; QCO = .../crates/quarry-core/src; QCT = apps/Ingestion Plane/Quarry-v2/services/quarry-control; MPX = apps/Model Plane; BC = apps/Control Plane/billing-core; OC = apps/Control Plane/org-core; SPA = apps/Frontend Plane/verevonv3/src.

# S6 gateway-zdr-telemetry

**Cross-item overlaps (merge-conflict hotspots)**
1. QE/search_routes.rs [M]: F-t2-003 (key and admission 501-531, 576-578, 762-843) and F-t2-046 (SEARCH_QUERY meters 1006, 1267, 1459).
2. QE/routes.rs [M, 1-line diff]: F-t2-003 (scrape read guard 432-438, durable refusals 563-588), F-t2-046 (scrape meter metadata 488-509), F-t2-004 (internal_run_page takes ZDR from body only, 969).
3. QE/main.rs [M]: F-t2-004 (init_telemetry call), F-t2-046 (RuntimeLeasePool 323, 337), F-t2-014 (router and new tower layer).
4. QE/agent_routes.rs [C]: F-t2-014 (routes 3660-3728, SSE 2650-2706, DevtoolsQuery 1059-1066), F-t2-046 (step cost 2346-2366, no meter), F-t2-004 (emit_for_zdr callers 1643, 3637).
5. QCO/zdr.rs [C]: F-t2-003 (guard; WriteKind::Cache unwired), F-t2-004 (guard callers), F-t2-014 (live-only ZDR runs).
6. QE/auth.rs [C]: F-t2-004 (effective_zdr_mode 135-138), F-t2-046 (principal for surface tag, Claims 61-91), F-t2-003 (org claim).
7. GW/zdr.rs, GW/upstream.rs (authorized_org_id 324-329, x-zdr 1787-1789), GW/domains/knowledge/quarry.rs (CacheAdmission 212-282) [C]: F-t2-003 and F-t2-004.
8. GW/main.rs [C]: F-t2-004 (trace span 194-202, log target 168-192) and F-t2-046 (rate-limit layer 143-148).
9. GW/domains/browser.rs [C]: F-t2-014 (route table 369-449) and F-t2-003 (persistent-profile refusal 174-186, withheld artifact 3846-3871).
10. MPX/proto/model_plane/v1/events.proto [C], MPX/go/pkg/natsx/publisher.go [C], scripts/tests/zdr-contract-sync.test.sh [M], QR/contracts/cross-plane-token-requests.json [M]: F-t2-004 (header and envelope), F-t2-014 (Event envelope), F-t2-003 (matrix pins).

---

## F-t2-003 Close the gateway search-cache ZDR and tenant hole, then publish a per-feature ZDR capability matrix

### 1. Citation check (25 checked; skipped as subsumed: search.rs:195-231 and 205-232 inside 209-233, zdr.rs:44-135 inside 44-120, search_routes.rs:495-530 inside 501-531)

| Citation | Tag | Verdict | Note |
|---|---|---|---|
| GW/domains/search.rs:209-226 | [C] | OK | filters_sig 209-217, key 218-226 |
| GW/domains/search.rs:218-226 | [C] | OK | key = cache_key("search-web", [query, limit, include_answer, filters_sig]) |
| GW/domains/search.rs:285 | [C] | OK | state.cache.store |
| GW/domains/search.rs:286 | [C] | OK, minor | return line after store |
| GW/cache.rs:20-25 | [C] | OK | FRESH_SECS 20, STORE_SECS 23 |
| GW/cache.rs:1-30 | [C] | OK | module docs and constants |
| GW/cache.rs:194-200 | [C] | OK | cache_key is 194-201 |
| GW/zdr.rs:44-90 | [C] | OK | request_zdr 44-51; org_zdr_enabled 67-120, fails closed at 105-112 |
| GW/domains/knowledge/quarry.rs:92-118 | [C] | OK | admission 103; scrape-preview key 104 is target-only |
| GW/domains/knowledge/quarry.rs:213-280 | [C] | OK | CacheAdmission 212-217, impl 219-281 |
| GW/domains/browser.rs:3846-3870 | [C] | OK | BrowserArtifactAbsence 3847-3871 |
| QE/routes.rs:430-436 | [M] | OK | read guard `!zdr.is_active() && should_read` at 432-438 |
| QE/routes.rs:563-588 | [M] | OK | refuse_durable_job_under_zdr |
| QE/routes.rs:575-588 | [M] | OK | ErrorCode::Forbidden plus prose |
| QE/search_routes.rs:503-517 | [M] | OK | build_search_options; org_id from JWT at 517 |
| QE/search_routes.rs:762-780 | [M] | OK, incomplete | build at 762, key 773, ZDR read-skip 775-782; write-skip is at 831, outside the cited range |
| QE/search_routes.rs:762-790 | [M] | OK | |
| QCT/internal/janitor/janitor.go:40-62 | [C] | OK | OptionsFromEnv 41-54, Run 58-; provenance caveat 14-18 |
| QR/postgres_event_history.rs:31 | [C] | OK | struct PostgresEventHistory |
| QR/local_index.rs:14-32 | [C] | OK | org scoping; add_document rejects empty org at 294-303 |
| QR/local_index.rs:286-300 | [C] | OK | |

Claim checks:
- "search_web keys on query, limit, include_answer and filters only": OK.
- "stores with no ZDR check": OK. `zdr` does not occur in search.rs. search_web reads neither x-zdr nor body zdr. post_quarry (search.rs:640-660) sends only x-user-id and a bearer, so Quarry learns ZDR only from the user token floor (search_routes.rs:727).
- Cross-tenant exposure: confirmed by code path, not exercised. The gateway key has no org (218-226). Quarry's answer depends on the verified org (search_routes.rs:515-517, 773). A second org sending the same query and params gets the first org's results, answer and citations. Stale entries are also served when Quarry fails (search.rs:288-291), for up to 24 h (cache.rs:23).
- Quarry is ZDR-correct: reads are skipped (775), and writes are gated by cacheable_search (576-578: `!zdr && rerank.status.is_cacheable()`, used at 831).
- "No declared per-feature ZDR matrix": true for Quarry features (see Surprises for the Model Plane matrix).

### 2. Affected files

Phase 1 (gateway):
- GW/domains/search.rs [C, 1078 lines]: search_web 51-293 (lookup 227-233, key 218-226, store 285). Imports at 28-34 need `crate::zdr` and `upstream::authorized_org_id`. Route at 39.
- GW/cache.rs [C, 201]: cache_key 194-201 (add org and variant parts), lookup 78-89 (stale window), lookup_within 96-105, store 108-110.
- GW/domains/knowledge/quarry.rs [C, 1158]: CacheAdmission 212-282 (to move), resolve call 103, key 104 (add org and variant), store 199-201, tests 814-910.
- GW/zdr.rs [C, 206]: request_zdr 44-51, org_zdr_enabled 67-120. Existing callers: GW/domains/chat/shared.rs:111 and GW/domains/knowledge/quarry.rs:243, 246.
- GW/upstream.rs [C, 3024]: authorized_org_id 324-329. The `_state` parameter is unused.
- NEW shared module, for example GW/cache_admission.rs [NEW], plus a `mod` line in GW/main.rs [C] (mod list 24-39).

Phase 2 (Quarry matrix and tombstone):
- QE/search_routes.rs [M, 2708]: build_search_options 501-531; params_signature about 541-571 (uncommitted); cacheable_search 576-578; handler zdr derivation 727, key and read 773-782, write 831-843.
- QE/cache.rs [C, 236]: SearchCache::key 121-127 (org-scoped).
- QCO/zdr.rs [C, 103]: guard 56-65, WriteKind 35-54.
- QCO/privacy.rs [C, 178]: guard_third_party_processing and its denial strings.
- QE/routes.rs [M, 1564]: should_read, should_write, effective_ttl at 58-77; scrape cache 432-487; crawl and batch refusal calls 596, 687.
- NEW matrix golden QR/contracts/zdr-feature-matrix.json [NEW], pinned in scripts/tests/zdr-contract-sync.test.sh [M] (GOLDENS list 48-118).
- Tombstone: QCT/internal/store/pg/migrations/015_<name>.sql [NEW] (next free after 014_fleets.sql [C]; these migrations are single files with no down pair). Sweeper QCT/internal/janitor/janitor.go [C, 115]; wiring QCT/cmd/control/main.go:200 [C].
- No compose or env change is needed for phase 1.

### 3. Reusable but unwired
- QCO/zdr.rs `WriteKind::Cache` and `WriteKind::Index` (36-52) have no production caller. `grep "WriteKind::Cache\|WriteKind::Index"` across QCO, QR and QE returns only zdr.rs:48, 51, 75, 78, 88, 91. The search cache uses ad hoc `!zdr` and cacheable_search instead.
- For contrast, `zdr::guard` is wired for Ingest, Artifact and Event at ingest_client.rs:199, observation.rs:1436, page_renderer.rs:29-30, pipeline.rs:111, 186, 194, 601, 856.
- GW/domains/knowledge/quarry.rs `CacheAdmission` is wired for the scrape preview but private, so "promote to a shared module" is still open.
- GW/cache.rs `set_add` and `set_members` (142-171) carry `#[allow(dead_code)]` and say they are not yet wired.
- MPX/api/contracts/zdr-refusals.json [U] and MPX/rust/crates/mp-contracts/src/zdr_refusal.rs [U] (the Z-05a refusal catalogue). Consumers are Model Plane services only. Zero hits in QE, QR, QCO or GW.
- MPX/api/contracts/zdr-posture-matrix.json [C] is consumed by Model Plane readers and has no Quarry, search, scrape or browser rows.
- The QCT janitor [C] is wired but off by default (QUARRY_RETENTION_DAYS=0) and covers artifacts only.

### 4. Tests
Gateway (verevonv3-ci.yml job "Gateway — fmt + clippy + test", lines 109-141: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`):
- GW/domains/knowledge/quarry.rs:814 `a_zdr_request_neither_reads_nor_writes_the_local_preview_cache`, :842 `an_org_under_zdr_caches_no_page_content_at_all`, :855 `an_ordinary_request_in_a_non_zdr_org_still_caches`, :871 `the_callers_cache_mode_is_honoured_the_way_quarry_edge_honours_it`, :893 and :907 (policy defaults).
- GW/zdr.rs:154-196 (4 tests).
- GW/domains/search.rs:855-1065 has 9 tests, none on cache or ZDR. GW/cache.rs has no tests. This is a coverage gap.

Quarry (quarry-v2-ci.yml, "Cargo check + test (workspace)", `cargo test --workspace` at line 205; coverage floor at 304):
- QE/tests/zdr_claim_floor.rs:1072 `a_zdr_org_user_never_reads_or_writes_a_cache_and_a_registry_zdr_service_still_does`. It needs Redis (redis_connection at 1034). The skip path without Redis is [UNVERIFIED].
- QE/tests/cache_policy.rs:78-265 (Redis).
- QE/src/cache.rs:211 `key_is_org_scoped`.
- QCO/src/zdr.rs:71-102.

Commands:
- `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" zdr`
- `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test zdr_claim_floor a_zdr_org_user_never_reads -- --nocapture`
- `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core zdr`
- `bash scripts/tests/zdr-contract-sync.test.sh` (repo root; CI model-plane-rust-ci.yml:92-96)

### 5. Feature flags and env
- GATEWAY_CACHE_REDIS_URL: GW/config.rs:128. Unset or unreachable means no cache (cache.rs:45-75). Compose wiring found only in the Data Plane e2e compose (`apps/Data Plane v2/tests/e2e/isolated/real-authority-browser-compose.yml:97`); production wiring [UNVERIFIED].
- Org standing ZDR posture: org-core `metadata.interactiveRetention.zdr` (GW/zdr.rs:128-137), memoised for 60 s (zdr.rs:37).
- Token claim floor: QE/auth.rs:79-131 (interactive users only; service claim ignored).
- QUARRY_RETENTION_DAYS (default 0) and QUARRY_RETENTION_DRY_RUN (default true): janitor.go:43, 47.
- No search-cache kill switch found in QE.

### 6. Surprises
- Quarry's search-cache admission is uncommitted. The live file has `params_signature(..., rerank)` adding `pp=` and `rr=` to the key (diff hunks 541-571), and cacheable_search (576). HEAD has neither; its write gate is `if !zdr` at line 728. The audit's 762-780 citations match the live tree only. Coordinate with whoever owns that rework.
- A ZDR posture matrix already exists, but for Model Plane turns: MPX/api/contracts/zdr-posture-matrix.json [C] (refusal_codes, decision_rows, enforcement_rows, org_posture_rows). Its loader is scripts/tests/zdr-contract-schema.mjs [M], and its digest pin is at zdr-contract-sync.test.sh:102. The Quarry matrix should follow this shape rather than invent one.
- Out-of-catalogue codes: `zdr_persistent_profile_forbidden` (QE/agent_routes.rs:1474; GW/domains/browser.rs:180) and `ZDR_REQUIRED` (QE/search_routes.rs:704) are absent from zdr-refusals.json (0 hits). The catalogue's 10 codes include `zdr_feature_not_supported` with a {feature} slot, which is the natural refusal for unsupported Quarry features. Quarry otherwise refuses with ErrorCode::Forbidden and prose (routes.rs:577-587; zdr.rs:56-65).
- zdr-refusals.json and zdr_refusal.rs are both untracked [U]. A sibling session may land them, so the Z-05a catalogue is not yet stable.
- The gateway ignores the body zdr flag: search.rs:236-241 forwards no zdr field, so a temporary-chat search would still be cached.
- janitor.go:14-18 says the sweeper keys on artifact age alone and must skip artifacts that produced durable Data Plane documents before deletion is armed. It is not a safe tombstone base until that guard lands.

---

## F-t2-004 ZDR-safe telemetry: span sampler, late-ZDR registry, log filter and safe/sensitive log fields

### 1. Citation check (25 checked; the mp-telemetry and duplicate telemetry.rs ranges were folded in)

| Citation | Tag | Verdict | Note |
|---|---|---|---|
| QE/telemetry.rs:1-64 | [C] | OK | 64 lines; no zdr string |
| QE/telemetry.rs:25-45 | [C] | OK | init 25-56; OTLP branch only when OTEL_EXPORTER_OTLP_ENDPOINT is set (29-33) |
| QE/telemetry.rs:6-18 | [C] | OK | metrics crates inert (6-14) |
| QE/change_routes.rs:632-637 | [C] | OK, exact | url field at 635, inside the `if zdr.is_active()` branch (632), at debug level |
| QR/pipeline.rs:110-118 | [C] | MISLEADING | 109-112 is `#[cfg(test)] fn page_event_persistence_allowed`, test-only. 114-118 is IndexedPage docs. Production artifact guards are at 186, 194, 601, 856 |
| QE/routes.rs:303 | [M] | OK | `TraceLayer::new_for_http()`; line unchanged by the diff |
| pkg/quarryotel/quarryotel.go:36-93 | [C] | OK | Init 39-93; `ParentBased(AlwaysSample())` at 80 |
| QE/auth.rs:121-139 | [C] | OK | zdr_floor 121-123; effective_zdr 128-131; effective_zdr_mode 135-138 |
| QE/auth.rs:104-120 | [C] | OK | service zdr claim ignored (doc 105-119; is_service 101-103) |
| QE/auth.rs:60-139, 62-91 | [C] | OK | Claims struct 61-91 |
| QR/events.rs:137 | [C] | OK | emit_for_zdr 137-152 |
| QR/events.rs:125-153 | [C] | OK | |
| QR/page_renderer.rs:115-135 | [C] | OK | render_and_emit 116-128; URL-free debug at 126 |
| QR/page_extract.rs:470-500 | [C] | OK for content; purpose corrected | url logged at 466, 491, 496-499. Reached only via pipeline.rs:795-798, which is gated by model_hop_allowed (page_extract.rs:335-340). Not an active ZDR leak |
| MPX/go/pkg/natsx/publisher.go:36-60 | [C] | OK | Publish 56-60 drops zdr:true envelopes before any backend |
| publisher.go:57 | [C] | OK, exact | |
| MPX/rust/crates/mp-telemetry/src/lib.rs:1-80 | [C] | OK | 75 lines; no ZDR |
| GW/zdr.rs:1-60 | [C] | OK | module docs |
| GW/upstream.rs:1775-1790 | [C] | OK | actor headers 1776-1781; x-zdr 1787-1789 |
| GW/upstream.rs:1788 | [C] | OK, exact | |
| QE/telemetry.rs, quarryotel (duplicate ranges) | — | OK | |
| "about 295" (JSON) and "roughly 250" (card) tracing call sites | — | DRIFTED | live grep: 312 matching macro lines in 52 files, including test modules; 111 carry url, query or error fields (heuristic) |

Claim checks:
- "telemetry.rs installs JSON fmt and OTLP layers unconditionally": PARTLY FALSE. The JSON layer is unconditional; the OTLP layer is added only when the endpoint is set (telemetry.rs:29-33).
- "No zdr string in telemetry.rs or quarryotel": TRUE.
- "Go services call quarryotel.Init but no tracer.Start": TRUE. No tracer.Start, otelhttp or otel.Tracer call exists under services/ or pkg/. Callers: QCT/cmd/control/main.go and the orchestrator main.go.
- "Quarry posture comes from token floor plus body": TRUE (auth.rs:105-131; routes.rs:416; search_routes.rs:727).
- "x-zdr is set by the gateway (upstream.rs:1788) but Quarry reads token plus body": TRUE. A grep of QR, QE and QCO finds no `x-zdr` reader.

### 2. Affected files
- QE/telemetry.rs [C, 64]: init_telemetry 25-56 (fmt json at 27, OTLP 29-50, registry 52-55). Phase 1 adds a filter layer here.
- QE/change_routes.rs [C, 823]: emit_change_side_effects 623-639; remove url at 635.
- Tracing sweep across QE and QR. Files by macro-line count: QE/main.rs [M, 1569] 105 (includes tests); action_runtime.rs 17; QE/auth.rs [C, 1242] 18; chromiumoxide.rs (quarry-browser) 16; QR/pipeline.rs [C, 1503] 11; QE/search_routes.rs [M, 2708] 10; QR/rerank.rs 10; QR/vector_index.rs 10; QR/fallback_driver.rs 8; QE/routes.rs [M, 1564] 8; QR/publisher.rs [C, 227] 8; QE/change_routes.rs 7.
- QCO/zdr.rs [C, 103]: the posture guard is the natural carrier.
- QR/events.rs [C, 356]: emit_for_zdr 137-152.
- QR/page_renderer.rs [C, 240], QR/page_extract.rs [C, 888] (log lines 466-499), QR/pipeline.rs (title hop gate 795-798).
- pkg/quarryotel/quarryotel.go [C, 101]: Init 39-93. No test file.
- MPX/go/pkg/natsx/publisher.go [C, 81]: callers include capability-core and orchestrator-core.
- MPX/rust/crates/mp-telemetry/src/lib.rs [C, 75]: initialised by session-core, model-gateway, inference-core and execution-core main.rs.
- GW/main.rs [C, 2554]: tracing init 47-50; make_http_trace_span 194-202; request_target_for_log 168-192 (path only). GW/observability.rs [C, 59].
- Contract pin: QR/contracts/cross-plane-token-requests.json [M, 5-line diff] with its .sha256 and the pin list at zdr-contract-sync.test.sh:117. A new ZDR header contract would be a NEW file pinned the same way.
- Lint: none exists. Any lint would be [NEW].

### 3. Reusable but unwired
- QCO `guard(WriteKind::Event)` and page_renderer `guard` are wired.
- QR/pipeline.rs:109-112 `page_event_persistence_allowed` is test-only (`#[cfg(test)]`).
- quarryotel `Init` is wired (2 callers) but creates no spans, so it is effectively unwired for span export.
- mp-telemetry `init` is wired in 4 services.
- GW `request_target_for_log` is wired (TraceLayer) and tested (main.rs:240, 277).
- GW `observability::track_metrics` and `record_authenticated_request` are wired (middleware.rs:359), with a real Prometheus recorder installed in main. These are content-free counters that already survive ZDR, unlike Quarry's inert metrics crates.
- natsx ZDR suppression is wired in Model Plane Go services but has no CI.

### 4. Tests
- natsx: MPX/go/pkg/natsx/publisher_test.go:136 `TestPublisher_ZDREnvelopeNeverReachesAnyBackend` and :164 `TestPublisher_ZDRSuppressionIsExactlyZDRTrue`. No .github workflow runs `go test` for apps/Model Plane/go (grep).
- quarryotel: no tests. Not in quarry-v2-ci's Go matrix (services/quarry-control, services/quarry-orchestrator, pkg/quarrycontracts).
- telemetry.rs and mp-telemetry: no tests.
- Quarry ZDR behaviour: QE/tests/zdr_behavior.rs (5 tests, behaviour not logs), QE/tests/zdr_claim_floor.rs, QR/events.rs:288-300 (emit_for_zdr).
- Gateway: GW/main.rs:240 `callback_trace_target_redacts_oauth_oidc_and_saml_secrets`, :277 `nginx_access_log_uses_a_query_redacting_callback_target`, GW/zdr.rs:154-196, GW/domains/browser.rs:4572 `a_zdr_run_yields_a_clean_no_artifact_outcome_without_touching_quarry`.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test zdr`; `cd "apps/Model Plane/go/pkg/natsx" && go test ./... -run ZDR`.
- CI: quarry-v2-ci.yml (cargo test --workspace; llvm-cov 80%); verevonv3-ci.yml gateway job (cargo test); model-plane-rust-ci.yml (`cargo test --workspace --no-fail-fast`).

### 5. Feature flags and env
- OTEL_EXPORTER_OTLP_ENDPOINT: QE/telemetry.rs:29-33 (unset means fmt only); quarryotel.go:40 (unset means no-op provider); mp-telemetry lib.rs:35 (unset means no OTLP layer). No explicit exporter-disable flag exists. Compose passes it through, default empty, at apps/Ingestion Plane/docker-compose.yml lines 209, 276, 334, 871 [M].
- OTEL_EXPORTER_OTLP_INSECURE: quarryotel.go:64 (only "true").
- OTEL_SERVICE_NAME: mp-telemetry lib.rs:43.
- ENVIRONMENT: quarryotel.go:54 (default "dev").
- RUST_LOG: telemetry.rs:26 and mp-telemetry lib.rs:28 (default "info"); compose at docker-compose.yml:210, 825 (default info). The gateway's main.rs:48 uses `EnvFilter::from_default_env()` with no fallback directive [UNVERIFIED default level].
- No ZDR switch exists for telemetry. Posture sources are listed under F-t2-003.

### 6. Surprises
- The live exposure is narrow. Quarry's only known URL log in a ZDR branch is change_routes.rs:635, at debug level, which the default `info` filter suppresses. The page_extract URL logs are not reachable under ZDR. The bigger risk is future call sites: about 111 macro lines carry url, query or error fields.
- Quarry's metrics crates are inert (telemetry.rs:6-14), while the gateway has a live Prometheus recorder. "Content-free counters survive ZDR" is already true at the gateway, not in Quarry.
- The audit's proposed ZDR header is redundant. Quarry ignores x-zdr (no reader in QR, QE or QCO), and token plus body already carry posture.
- internal_run_page (QE/routes.rs:969) reads ZDR from the body only, with no token floor. Comments at 555-562 say this is deliberate for the orchestrator path.
- Nested worktree copies exist under Quarry-v2/.claude/worktrees/vigilant-banach-6933aa. Exclude them from grep counts and citations.

---

## F-t2-046 Complete web-usage metering: browser metrics, outcome and surface attributes, price table and plan keys

### 1. Citation check (25 checked; skipped as duplicates: usage.rs:1-140, QE/config.rs:51-57, QE/main.rs:323 dup)

| Citation | Tag | Verdict | Note |
|---|---|---|---|
| QR/usage.rs:50-94 | [C] | OK | UsageEvent 49-58; new 64-82; metrics 88-94 |
| QR/usage.rs:143-179 | [C] | OK | NatsUsageMeter::meter 143-180 |
| QR/usage.rs:88-94 | [C] | OK | five metric constants |
| QE/extract_routes.rs:345-361 | [M] | OK (HEAD 343-359) | quantity `targets.len()` at 352, after results, with no success check |
| QE/extract_routes.rs:348-351 | [M] | OK | UsageEvent::new arguments |
| QE/map_routes.rs:294-310 | [C] | OK | meter 295-310; SEARCH_QUERY at 300 |
| QE/routes.rs:490-505 | [M] | OK | meter 491-509; url at 500, unconditional on ZDR |
| QE/agent_routes.rs:700-761 | [C] | OK | cost helpers 701-762; no meter call in the file |
| QE/agent_routes.rs:2346-2362 | [C] | OK | fallback 2348-2351; receipt append guarded by `!entry.zdr.is_active()` at 2360 |
| QR/action_cost.rs:14-38 | [C] | OK | CostSource 12-18; flat table 20-37 |
| QE/main.rs:323 | [M] | OK (HEAD 312) | RuntimeLeasePool::new(4) |
| QE/main.rs:337 | [M] | OK (HEAD 326) | RuntimeLeasePool::new(2) |
| QR/lease_pool.rs:30-45 | [C] | OK | struct 33-39; new 41-50 |
| QE/auth.rs:62-91 | [C] | OK | Claims |
| QE/tests/zdr_claim_floor.rs:379-388 | [M] | OK | metered_zdr helper; diff hunks at 918 and 973 do not touch it |
| QE/config.rs:58-62 | [M] | OK in live tree, NOT IN HEAD | block added uncommitted (+12 at 51-62); HEAD has 0 hits for vector_retrieval_per_org_hourly_budget |
| QE/routes.rs:202-221 | [M] | OK | team routes |
| QCT/internal/resources/cycle24.go:189-258 | [C] | OK | MountTeam 186; routes 189-258 |
| docs/BILLING.md:46-64 | [C] | OK | taxonomy 46-54; idempotency 63-66 |
| docs/BILLING.md:116-125 | [C] | OK | future work 116-126 contains no browser metrics |
| BC/internal/billing/repository.go:371-380 | [C] | OK | RecordUsage |
| repository.go:463-512 | [C] | OK | metadata JSON persisted at 485-488; Lago enqueue 494-505 |
| BC/internal/billing/allowance_repository.go:189-210 | [C] | OK | recordAllowanceUsage |
| BC/internal/billing/defaults.go:5-80 | [C] | OK | entitlements 5-59; quotas 61-85; no browser keys |

Claim checks:
- "Five idempotent metrics": partly. Idempotency holds only for NATS redelivery. Extract (extract_routes.rs:345), map (map_routes.rs:294) and search (search_routes.rs:1264, 1456) mint a fresh `Id::new()` per call, and answer uses `answer-<nanos>` (answer_routes.rs:101-104). A client retry double-bills.
- "Extract bills every attempted target": TRUE (352).
- "agent_routes emits no UsageEvent": TRUE (no `.meter(` in that file).
- "Scrape metadata puts the URL in even under ZDR": TRUE.
- "No plan keys": TRUE in defaults.go and org-core types.go.
- "Two fixed pools": TRUE (4 and 2, hard-coded).
- "SPA usage client is unused": PARTLY. Four of five team helpers have no callers. getTeamCreditUsage is called only in quarry-client.test.ts:128.
- "Gateway global bucket 600 RPM": TRUE (rate_limit.rs:58).

### 2. Affected files
- QR/usage.rs [C, 298]: add metric constants at 88-94; keep the `quarry:<request_id>:<metric>` identity.
- QE/extract_routes.rs [M, 639]: meter 344-361. Split extract from SEARCH_QUERY, meter successes only.
- QE/map_routes.rs [C, 459]: meter 293-310.
- QE/agent_routes.rs [C, 3737]: step path 2346-2366. Add the step and minute meter here, with ids and counts only under ZDR. Receipts are not persisted under ZDR, so a receipt-derived meter goes silent.
- QE/routes.rs [M, 1564]: scrape meter metadata 493-509; remove url and fingerprint under ZDR.
- QE/answer_routes.rs [M, 379]: ANSWER_SYNTH id 101-104 (timestamp-based); meter 107-116 and 295-301.
- QE/search_routes.rs [M, 2708]: SEARCH_QUERY meters 1006, 1267, 1459.
- QR/action_cost.rs [C, 80]: quantity source for browser steps.
- QR/lease_pool.rs [C, 294]: a per-org semaphore goes in front of RuntimeLeasePool (struct 33-39).
- QE/main.rs [M, 1569]: pool sizes 323, 337. QE/config.rs [M, 501]: vector budget 51-62 (uncommitted).
- QE/auth.rs [C, 1242]: Claims 61-91 gives the principal for the surface tag.
- QE/model_plane_gate.rs [U, 401]: the AI-format gate for structured extraction; Model Plane cost is not metered.
- docs/BILLING.md [C, 126]: taxonomy table 46-54 needs extract and map rows.
- BC/internal/billing/defaults.go [C, 196]: add plan keys and quotas.
- BC/internal/billing/repository.go [C, 764]: RecordUsage 371-380; strip URL metadata under ZDR at 485-488.
- BC/internal/billing/allowance_repository.go [C, 347]: allowance policies 189-210.
- BC/internal/nats/subscriber.go [C, 347]: subscribes to `usage.>` at line 41, so new subjects need no subscriber change.
- OC/internal/org/types.go [C, 365]: defaultEntitlements 266-271; computed ZDR entitlement 273-274; quotas 318-347.
- NEW migrations if tables are needed: BC/migrations/0011_<name>.up.sql and .down.sql [NEW] (next free after 0010_allowance_revisions); OC/migrations/021_<name>.up.sql [NEW] (next free after 020).
- GW/rate_limit.rs [C, 577]: DEFAULT_RPM 58; rate_limit_key 269-289. GW/main.rs [C, 2554]: layer 143-148. GW/domains/leads.rs [C, 403]: entitlement gate 66-112 to copy for browser keys.
- SPA/shared/api/quarry-client.ts [C, 173]: team helpers 153-173. SPA/features/dashboard/home/KnowledgeComposer.tsx [M, 1999]: run view, dispatch counter at 1053.

### 3. Reusable but unwired
- Team helpers: QE routes (routes.rs:202-221 to resource_routes.rs:836-858 to cycle24.go) are wired; four of five SPA helpers have no callers.
- Allowance policies: wired (repository.go:495 to allowance_repository.go:189). Writers are in allowance_revisions.go.
- Entitlement gate: wired for leads only (leads.rs:80-112).
- Flat action-cost table: wired (agent_routes.rs:2350).
- Per-process vector budget (config.rs:58-62, uncommitted): wired, but the doc at lines 59-60 says it is per replica, not shared.
- Lago metric codes: no in-repo mapping. grep finds them only in usage.rs and BILLING.md, so each new metric needs an external Lago billable-metric code before it ships.
- Per-org fair-share: none. RuntimeLeasePool has only a global semaphore.

### 4. Tests
- Quarry (CI quarry-v2-ci.yml `cargo test --workspace`, line 205): QR/usage.rs tests at 195, 207, 271, 285; QE/tests/zdr_claim_floor.rs metered-usage harness at 231 and 379-388; QE/extract_routes.rs test module from 509.
- Billing-core (CI billing-core-service.yml `go test -v -p 1 ./...`, line 84): usage_test.go (TestRecordUsageRejectsUnstableIdentityBeforeStorage and others), internal/nats/usage_subscriber_test.go (5 tests), adapters/lago/adapter_test.go (TestReportUsageUsesStableEventIDAsTransactionID), allowance_postgres_test.go, lifecycle_postgres_test.go, http/usage_validation_test.go.
- Gateway (verevonv3-ci.yml): rate_limit.rs:410-572, including prefers_org_then_user_over_ip at 503 (tests the function in isolation).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime usage`; `cd "apps/Control Plane/billing-core" && go test ./internal/billing/ ./internal/nats/`; `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test rate_limit`.

### 5. Feature flags and env
- GATEWAY_RATE_LIMIT_RPM: GW/rate_limit.rs:131 (default 600, DEFAULT_RPM at 58).
- GATEWAY_CACHE_REDIS_URL: also supplies the shared Dragonfly for the limiter (main.rs:144-146).
- QUARRY_EDGE__AI_FORMATS: QE/config.rs:161-163, default false (config.rs:391). Compose default "0" at apps/Ingestion Plane/docker-compose.yml:134.
- QUARRY_EDGE__VECTOR_RETRIEVAL (config.rs:51-57, default false) and the per-org budget (config.rs:58-62, default at config.rs:294). Both uncommitted.
- QUARRY_BROWSERBASE_PROCESSOR_ID: QE/main.rs:324-325 (default "browserbase").
- QUARRY_EDGE__INTERNAL_SECRET: config.rs:235; run_page fails closed (routes.rs:940-967).
- Cargo feature browser-agent: default ON (QE/Cargo.toml:73-74); agent router no-op when off (agent_routes.rs:3734).

### 6. Surprises
- Rate-limit key: the org and user branch is effectively dead for authenticated traffic. The limiter layer (main.rs:147) is outer, and require_session is a route_layer on the browser router (browser.rs:448). The comment at rate_limit.rs:52-54 says it keys by client IP. This is inferred from layer order and the comment, so confirm by test [INF].
- Billing-core persists the scrape URL: repository.go:485-488 writes the metadata JSON to billing_usage_events.
- BILLING.md:52 says SEARCH_QUERY is emitted "on every successful /v1/search". Extract and map also emit it, so the doc is out of date.
- Structured extraction's Model Plane cost is not metered. extract_routes.rs:205-208 gates the path on QUARRY_EDGE__AI_FORMATS, and model_plane_gate.rs is untracked [U].
- The audit's "fix config.rs:58-62" points at uncommitted code. HEAD has no such block.
- The audit's pool citations (main.rs:323 and 337) are correct for live, not HEAD (312 and 326).

---

## F-t2-014 Surface one canonical agent run trace in the UI: typed events, receipts, live view, correlation envelope

### 1. Citation check (25 checked)

| Citation | Tag | Verdict | Note |
|---|---|---|---|
| MPX/proto/model_plane/v1/events.proto:8-60 | [C] | OK | Event 12-56; fields 1-13; `reserved 14 to 20` at 55; EventType from 59 |
| events.proto:112-130 | [C] | OK | run events 117-126; action events 128-132 |
| eventlog.proto:26-37 | [C] | OK | service 26-38; SubscribeEvents 37 |
| eventlog.proto:50-90 | [C] | OK | EventFilter 58-73; Cursor 78-81 |
| orchestration.proto:990-1040 | [C] | OK | StreamRunEventsRequest 995-1006; OrchestrationEvent 1010-1043; browser tags 17-22 at 1038-1043 |
| SPA ChatLiveRunPanel.tsx:360-380 | [C] | OK | onRunResumed 360-370; browser approval 371-381 |
| SPA BrowserTimelineDetail.tsx:1-50 | [C] | OK | consoleTone 15; networkTone 23 |
| SPA FleetRunConsole.tsx:1-60 | [C] | OK, wrong framing | fetches `/v1/fleets/...` at 28 and 35; mounted at SPA/app/App.tsx:216 |
| SPA KnowledgeComposer.tsx:1053 | [M] | OK | dispatchCount |
| SPA BrowserChrome.tsx:199 | [C] | OK | DevtoolsTab type |
| GW/domains/browser.rs:625-648 | [C] | OK | get_owner_timeline 625-650 |
| GW/domains/browser.rs:1819 | [C] | OK | devtools proxy 1815-1825 (function at 1784) |
| GW/domains/browser.rs:368-470 | [C] | OK | router 369-449; no receipts, egress, procedure or agent-events routes |
| QE/agent_routes.rs:1061-1072 | [C] | OK | DevtoolsQuery 1059-1066 has after_sequence and limit only |
| QE/agent_routes.rs:1739 | [C] | OK | RequestKind::new in set_browser_session_control |
| QE/agent_routes.rs:3662-3716 | [C] | OK | routes 3660-3728: egress 3680, receipts 3683, procedure 3685, events/stream 3724-3727 |
| QE/agent_routes.rs:2650-2706 | [C] | OK | agent_events_stream 2658; replay is a no-op at 2705 |
| QE/agent_routes.rs:930-960 | [C] | OK | AgentEventsQuery 936-942 |
| QCT/internal/httpx/httpx.go:26-30 | [C] | OK | RequestID 24-34; mounted at QCT/cmd/control/main.go:144 |
| QR/step_receipts.rs:100-125 | [C] | OK, approximate | BrowserProfileScope 96-104; BrowserTimelineEvent 110-118 |
| QR/step_receipts.rs:190-206 | [C] | OK | StepReceipt fields 186-210 |
| docs/gap-quarry.md:347 | [C] | OK | row 7 documents a Quarry-side canonical envelope |
| QE/fleet_routes.rs:216-223 (found) | [C] | — | /v1/fleets is routed in Quarry and merged at QE/routes.rs:285 |
| GW/domains/chat.rs:74-78 (found) | [C] | — | run events and replay proxies |
| GW/nginx.conf (found) | [C] | — | only `/api/` and `/health` are proxied to the gateway |

Claim checks:
- "Canonical Event envelope exists": TRUE (events.proto).
- "EventLog Query and Subscribe with opaque cursor exists": CONTRACT ONLY. No server implementation exists (see section 3).
- "Gateway proxies only timeline and devtools": IMPRECISE. It also proxies tabs, frames, frames/ws, artifacts, ai-runs, control and suggest (browser.rs:369-449).
- "Unrouted /v1/fleets": TRUE at the gateway, but misleading. Quarry serves it (fleet_routes.rs:216-223, merged at routes.rs:285). The SPA fetches a path that nginx never proxies, so FleetRunConsole cannot reach Quarry in any deployment.
- "No x-request-id intake": TRUE in GW and QE (grep finds nothing). QCT does intake it (httpx.go).
- "from_seq replay is a no-op": TRUE.
- "No run origin enum exists": [UNVERIFIED], not searched in this pass.
- "No producer sequence across planes": TRUE. But Quarry has its own envelope: QCO/event.rs:84-96 `Event` carries `seq: u64`, set by the sink (QR/events.rs:125), and gap-quarry.md:347 documents it. So three event vocabularies exist: MPX events.proto EventType, the MPX OrchestrationEvent oneof, and QCO event.rs EventType.
- "Browser action events are not in the canonical envelope": PARTLY FALSE. EVENT_TYPE_BROWSER_* (events.proto:140-146) exists and is produced by MPX/rust/services/execution-core/src/browser_events.rs, model-gateway browser_run.rs and sse.rs, and session-core grpc.rs.

### 2. Affected files
Model Plane (trace contract):
- MPX/proto/model_plane/v1/events.proto [C, 185]: Event 12-56 (fields 14-20 reserved at 55); EventType browser values 140-146.
- MPX/proto/model_plane/v1/orchestration.proto [C, 1289]: StreamRunEventsRequest 995-1006; OrchestrationEvent 1010-1043.
- MPX/proto/model_plane/v1/eventlog.proto [C, 116].
- MPX/proto/baseline.binpb [C]: CI runs `buf breaking ... --against baseline.binpb` (root .github/workflows/buf.yml:20-23), so any change must refresh the baseline.
- MPX/go/gen/model_plane/v1/events.pb.go [C] and eventlog_grpc.pb.go [C]: generated.
- MPX/rust/crates/mp-contracts/build.rs [C]: compiles the proto root (line 30).
- MPX/rust/crates/mp-eventlog/src/lib.rs [C, 31] with cursor.rs, filter.rs, idempotency.rs: unwired (section 3).
- MPX/rust/services/session-core/src/orchestration_grpc.rs [M], model-gateway/src/browser_run.rs [M], execution-core/src/grpc.rs [M]: uncommitted changes in the browser run path.
- NEW migration for run-trace persistence: MPX/rust/services/session-core/migrations/0046_<name>.sql [NEW]. The latest is 0045_zdr_waived_runs_org_index.sql, which is untracked [U], so treat 0045 as taken until it lands.

Quarry:
- QE/agent_routes.rs [C, 3737]: routes 3660-3728; DevtoolsQuery 1059-1066; AgentEventsQuery 936-942; replay no-op 2702-2705; RequestKind mints at 1739, 2666.
- QE/routes.rs [M, 1564]: merge of agent and fleet routers at 280-285; TraceLayer at 303, where a tower layer for x-request-id would mount.
- NEW tower layer QE/request_id.rs [NEW], modelled on QCT/internal/httpx/httpx.go:24-34.
- QE/fleet_routes.rs [C, 252]: /v1/fleets routes at 216-223.
- QCO/event.rs [C, 96]: Event 84-96, with `seq`.
- QR/step_receipts.rs [C, 818]: BrowserTimelineEvent 110-118; StepReceipt 186-210.
- QR/events.rs [C, 356]: emit_for_zdr 137-152 (callers at agent_routes.rs 1643, 3637; agent_loop.rs:117; pipeline.rs:267; observation.rs 388, 1000).
- QR/migrations/0010_<name>.sql [NEW] if receipts gain an origin or trace column (next free after 0009_browser_timeline_events.sql [C]).

Gateway:
- GW/domains/browser.rs [C, 6317]: router 369-449; add proxies for receipts, egress-receipts, procedure and the agent-events SSE, modelled on get_owner_timeline 625-650.
- GW/domains/chat.rs [C]: run-event proxies 74-78.
- GW/upstream.rs [C, 3024]: forwarding; no x-request-id is forwarded today.

SPA:
- SPA/features/chat/components/ChatLiveRunPanel.tsx [C, 713]: 360-381.
- SPA/features/dashboard/home/BrowserTimelineDetail.tsx [C, 348], BrowserChrome.tsx [C, 2172] (DevtoolsTab 199), KnowledgeComposer.tsx [M, 1999] (1053).
- SPA/features/agents/components/FleetRunConsole.tsx [C, 166]: the fetch paths 28 and 35 are the bug; the route is at SPA/app/App.tsx:216 [M].
- SPA/nginx.conf [C]: `location /api/` and `/health` only.

### 3. Reusable but unwired
- MPX/rust/crates/mp-eventlog [C, 31-line lib.rs]: cursor, filter and idempotency modules are unwired. grep for `mp_eventlog|mp-eventlog` in MPX/rust returns only its own lib.rs, its Cargo.toml and the workspace Cargo.toml. No service depends on it.
- EventLog gRPC server: none. Only generated MPX/go/gen/.../eventlog_grpc.pb.go exists. No `impl EventLog` or `EventLogServer` in non-generated MPX Rust or Go.
- QE/fleet_routes.rs: routed in Quarry, but no gateway proxy and no SPA path that reaches it.
- MPX/rust/services/execution-core/src/scrub.rs [C]: wired (executor.rs:94, grpc.rs:1015-1016).
- QCT httpx.RequestID: wired (cmd/control/main.go:144).
- Quarry receipt and egress routes (agent_routes.rs:3680-3687): wired in Quarry, unused by the SPA (grep for egress-receipts, procedure and receipts in SPA/ finds nothing).

### 4. Tests
- Gateway (verevonv3-ci.yml gateway job, `cargo test`): GW/domains/browser.rs:3995 `browser_session_read_proxies_the_quarry_owner_projection_without_cache_fallback`; :4052 `browser_timeline_read_proxies_quarry_without_legacy_replay_merge`; :4572 `a_zdr_run_yields_a_clean_no_artifact_outcome_without_touching_quarry`; :4751 `browser_http_operations_use_quarry_owner_denials_not_bff_cache`.
- Quarry: QE/tests/zdr_behavior.rs:67-202 (ZDR profile refusals). Quarry CI runs `cargo test --workspace` (quarry-v2-ci.yml:205).
- Model Plane (model-plane-rust-ci.yml): `cargo test --workspace --no-fail-fast` (line 119); session-core `zdr_receipt` (ignored, needs Postgres) at lines 162-164.
- Proto: root .github/workflows/buf.yml:20-23 runs `buf lint apps/Model Plane/proto` and `buf breaking`.
- SPA (verevonv3-ci.yml "SPA — lint + typecheck + test + build", `pnpm verify`, lines 192-193). SPA/features/agents/components/FleetRunConsole.test.tsx exists.
- Commands: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test browser`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge agent`; `cd "apps/Model Plane/rust" && cargo test --workspace`; `buf breaking "apps/Model Plane/proto" --against "apps/Model Plane/proto/baseline.binpb"`.

### 5. Feature flags and env
- Cargo feature browser-agent (QE/Cargo.toml:73-74, default ON): gates the agent router (agent_routes.rs:3734).
- Cargo feature postgres-queue (QE/Cargo.toml): gates durable history and change side effects (change_routes.rs:622).
- MODEL_PLANE_URL (QE/extract_routes.rs:207, 229): Model Plane path for AI extraction.
- x-request-id has no env switch; the proposed intake must cap length and charset (per the audit risks).

### 6. Surprises
- Three event vocabularies coexist (MPX EventType, MPX OrchestrationEvent, QCO EventType/Event). The audit's "extend model_plane.v1.Event" needs an explicit mapping from Quarry's Event.
- EVENT_TYPE_BROWSER_* values already exist in the canonical enum (events.proto:140-146), and producers exist. The envelope is closer to done than the audit states.
- FleetRunConsole is not a placeholder: it is mounted at App.tsx:216 and has a test. Its data path is broken because `/v1/fleets` never reaches Quarry through nginx.
- ZDR runs are ephemeral by construction: emit_for_zdr uses the ephemeral sink (events.rs:145-147), and natsx drops zdr:true envelopes (publisher.go:56-60). The trace design must keep that.
- Receipts are not appended under ZDR (agent_routes.rs:2360). Any trace built on receipts is empty for ZDR runs by design.
- Model Plane browser-run files are uncommitted (orchestration_grpc.rs, browser_run.rs, execution-core grpc.rs). The contract is moving.
- The run-start grant and bearer dependency (F-t2-001) was not checked in this pass.

<!-- COMPLETE -->
<!-- COMPLETE -->
