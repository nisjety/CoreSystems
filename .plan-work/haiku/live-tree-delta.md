# Live tree delta: uncommitted work map (Space hybrid build plan, 2026-10-14)

Scope: the uncommitted and untracked work in the live tree `C:/dev/CoresSystem` (branch main, HEAD afb83928, nothing staged). Input: `.plan-work/live-tree-status.txt` (324 entries; re-checked against live `git status --porcelain`, still 324). Every read was from the live tree. Nothing in the live tree was written, and no build, test or install was run.

## Summary

- 324 entries: 197 modified (M), 122 untracked entries (U; 314 files after directory expansion), 5 deleted (D, MISSING).
- 232 entries are Space-relevant (classes A and B) and listed in section 2. Row states: M 161, U 71, MISSING 0. The other 92 entries are adjacent (C, 34) or not Space (N, 58); the N set is almost all product-demo and marketing work.
- The Space-relevant uncommitted work is the Z-series ZDR build (refusal catalogue Z-05a, acknowledgement Z-13, waiver and retention receipt Z-16 to Z-18, egress attestation Z-41), the Quarry option-B rerank (QB), and the Quarry and retrieval-engine changes. This is WS06 work already in progress. It touches the WS01, WS02, WS09, WS10 and WS17 hotspots (section 6).
- Largest risks: session-core `grpc.rs` (+817/-44) and `auth.rs` (+396); model-gateway `sse.rs` (+814/-69); model-gateway `http_routes.rs` (approval routes now gated by a continuation check); new `zdr_posture.rs` (1,259 lines) and `retention_receipt.rs` (1,735 lines); retrieval-engine `rerank.rs` (+835).
- A behaviour change to decide, not just review: the uncommitted Ingestion compose turns `QUARRY_EDGE__AUTOPROMPT` and `QUARRY_EDGE__SEMANTIC_RERANK` from default 1 (committed) to 0, and adds six Model Plane switches at 0 (the QB-T17 test asserts OFF).
- Integrity checks passed: no staged entries; no line-ending-only modifications; all 9 uncommitted .sha256 pins and all 4 SHA256SUMS manifests match their files; the two cross-plane token manifests are byte-identical. One real gap: `judgments.v1.jsonl` is 0 bytes (section 7).

## Legend

- State: M = MODIFIED-UNCOMMITTED (` M`); U = UNCOMMITTED-NEW (`??`, directories expanded with `git ls-files --others --exclude-standard`); D = MISSING (` D`, absent from the live tree). No row is COMMITTED.
- Class: A = named Space component from the brief (session-core, model-gateway, execution-core, auth-core, retrieval-engine, mp-contracts, Quarry, src/features/spaces, plus the compose and CI files of those components); B = Space-relevant by topic (ZDR, retention, refusal, acknowledgement, notification contract, ZDR compliance docs); C = adjacent (collision risk only); N = not Space.
- Size: M rows from `git diff --numstat` (+added/-removed); U rows from `wc -l` (directory rows give the directory). Line numbers are from the live files as read. Hunk headers came from `git diff -U0`; no large diff was read in full.

## 1. Counts by plane and service

| Plane | Service (after apps/) | M | U entries | U files | D | Entries | A/B | C | N |
|---|---|---|---|---|---|---|---|---|---|
| (repo root) | .githooks | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| (repo root) | .github | 3 | 1 | 1 | 0 | 4 | 3 | 0 | 1 |
| (repo root) | .gitleaks.toml | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| (repo root) | docs | 6 | 0 | 0 | 0 | 6 | 6 | 0 | 0 |
| (repo root) | scripts | 2 | 1 | 1 | 0 | 3 | 3 | 0 | 0 |
| Control Plane | auth-core | 2 | 0 | 0 | 0 | 2 | 2 | 0 | 0 |
| Data Plane v2 | (plane root) | 4 | 0 | 0 | 0 | 4 | 4 | 0 | 0 |
| Data Plane v2 | docs | 1 | 1 | 1 | 0 | 2 | 2 | 0 | 0 |
| Data Plane v2 | openapi | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 |
| Data Plane v2 | scripts | 2 | 6 | 6 | 0 | 8 | 3 | 0 | 5 |
| Data Plane v2 | services/retrieval-engine-rs | 22 | 11 | 19 | 0 | 33 | 33 | 0 | 0 |
| Data Plane v2 | services/usage-ledger-rs | 1 | 0 | 0 | 0 | 1 | 0 | 1 | 0 |
| Data Plane v2 | tests | 4 | 4 | 4 | 0 | 8 | 8 | 0 | 0 |
| Frontend Plane | verevonv3/apps/verevon-web | 6 | 30 | 99 | 5 | 41 | 0 | 11 | 30 |
| Frontend Plane | verevonv3/package.json | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/scripts | 0 | 5 | 5 | 0 | 5 | 0 | 0 | 5 |
| Frontend Plane | verevonv3/src/app | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/src/demo | 0 | 1 | 82 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/src/features | 25 | 8 | 8 | 0 | 33 | 11 | 16 | 6 |
| Frontend Plane | verevonv3/src/index.tsx | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/src/shared | 10 | 9 | 10 | 0 | 19 | 12 | 5 | 2 |
| Frontend Plane | verevonv3/src/styles | 1 | 0 | 0 | 0 | 1 | 0 | 1 | 0 |
| Frontend Plane | verevonv3/src/vite-env.d.ts | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/tsconfig.node.json | 1 | 0 | 0 | 0 | 1 | 0 | 0 | 1 |
| Frontend Plane | verevonv3/vite.demo.config.ts | 0 | 1 | 1 | 0 | 1 | 0 | 0 | 1 |
| Ingestion Plane | (plane root) | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 |
| Ingestion Plane | Quarry-v2 | 30 | 11 | 24 | 0 | 41 | 41 | 0 | 0 |
| Ingestion Plane | integration-corev2 | 14 | 10 | 26 | 0 | 24 | 24 | 0 | 0 |
| Model Plane | api | 1 | 2 | 2 | 0 | 3 | 3 | 0 | 0 |
| Model Plane | docs | 3 | 0 | 0 | 0 | 3 | 3 | 0 | 0 |
| Model Plane | proto | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 |
| Model Plane | rust/crates/mp-contracts | 2 | 4 | 5 | 0 | 6 | 6 | 0 | 0 |
| Model Plane | rust/services/execution-core | 8 | 1 | 1 | 0 | 9 | 9 | 0 | 0 |
| Model Plane | rust/services/inference-core | 2 | 2 | 2 | 0 | 4 | 4 | 0 | 0 |
| Model Plane | rust/services/model-gateway | 27 | 10 | 12 | 0 | 37 | 37 | 0 | 0 |
| Model Plane | rust/services/session-core | 11 | 4 | 5 | 0 | 15 | 15 | 0 | 0 |
| **Total** | | **197** | **122** | **314** | **5** | **324** | **232** | **34** | **58** |

Plane subtotals (entries): repo root 15; Control Plane 2; Data Plane v2 57 (M 35, U 22, A/B 51, C 1, N 5); Frontend Plane 106 (M 47, U 54, D 5; A/B 23, C 33, N 50); Ingestion Plane 66 (M 45, U 21; all A/B); Model Plane 78 (M 55, U 23; all A/B).

## 2. Space-relevant entries (rows = 232)

Row states: 161 M, 71 U, 0 D. Class A rows: 169. Class B rows: 63. Path column is repo-root relative with the exact on-disk spelling ("Model Plane", "Data Plane v2", "Quarry-v2"). Directory rows are marked dir.

| 1 | A | M | .github/workflows/dataplane-v2-ci.yml | +25/-0 | CI step for the CR-T15 rerank-input-context contract (RERANK_INPUT_INCLUDE_CONTEXT) in retrieval-engine. | - |
| 2 | B | M | .github/workflows/model-plane-rust-ci.yml | +50/-0 | New job runs the #[ignore]d session-core retention-receipt Postgres tests (Z-17) on a scratch database. | runs only in CI; not run here |
| 3 | A | U | .github/workflows/quarry-docs-contract.yml | 53 lines | New CI job runs the Quarry docs-contract test (Quarry docs, compose, token manifest). | - |
| 4 | B | M | docs/compliance/evidence/claims-vs-code.md | +8/-8 | Claim rows T2a and T5b reworded to match code (ZDR add-on, default-deny). | compliance evidence; re-verify after ZDR lands |
| 5 | B | M | docs/compliance/evidence/data-flow-map.md | +21/-21 | Rows G1 to G3 (privacy floor, residency, ZDR gate) reworded to current enforcement. | compliance evidence |
| 6 | B | M | docs/compliance/evidence/honest-limits.md | +6/-6 | Rows L11 and L12 reworded (privacy floor on answer calls; ZDR refused per document). | compliance evidence |
| 7 | B | M | docs/compliance/evidence/retention-and-erasure-evidence.md | +12/-12 | "Where the rows would go" (owner-only delete) and grep evidence N3 reworded. | compliance evidence |
| 8 | B | M | docs/compliance/evidence/subprocessors-from-code.md | +5/-5 | Subprocessor rows (brave-search, browserbase) reworded from code. | compliance evidence |
| 9 | B | M | docs/compliance/evidence/tools/doc-codes.json | +1/-0 | Adds doc code zdr_dense_unavailable pointing at retrieval_metadata.rs and a Data Plane file. | - |
| 10 | B | M | scripts/tests/zdr-contract-schema.mjs | +12/-12 | Refusal catalogue read from api/contracts/zdr-refusals.json instead of a hard-coded constant (Z-05a). | depends on the untracked catalogue (row 210) |
| 11 | B | M | scripts/tests/zdr-contract-sync.test.sh | +93/-2 | Sync test requires every matrix refusal code to be in the generated catalogue (Z-05a). | same catalogue dependency (row 210) |
| 12 | A | U | scripts/tests/quarry-model-plane-switches.test.mjs | 54 lines | QB-T17: asserts every Quarry Model Plane switch defaults OFF in the Ingestion compose. | passes only with the uncommitted compose (row 89) |
| 13 | A | M | apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json | +2/-3 | Token manifest: rerank client listed in used_by for the rerank token request (QB-T09); pending field removed. | byte-identical to the Quarry copy (row 102); pinned |
| 14 | A | M | apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json.sha256 | +1/-1 | Pin for the manifest above (verified: matches). | - |
| 15 | A | M | apps/Data Plane v2/.env.example | +3/-0 | Adds RERANK_INPUT_INCLUDE_CONTEXT=0 (CR-T15), off by default. | - |
| 16 | A | M | apps/Data Plane v2/Cargo.lock | +3/-0 | Lock: usage-ledger-rs path dependency and rustls, tokio-rustls for retrieval-engine. | lockfile |
| 17 | A | M | apps/Data Plane v2/DATA_PLANE_STATUS.md | +36/-0 | Status entry 2026-10-11: Quarry option-B web rerank route (POST /v1/rerank) built dark. | - |
| 18 | A | M | apps/Data Plane v2/docker-compose.yml | +53/-0 | Egress attestation env for ZDR-restricted content (Z-41); empty attests nothing, so a ZDR dense query answers lexically. | - |
| 19 | A | M | apps/Data Plane v2/docs/contextual-retrieval-runbook.md | +1/-1 | RERANK_INPUT_INCLUDE_CONTEXT row rewritten (values 0, 1, true, false). | - |
| 20 | A | M | apps/Data Plane v2/openapi/d4d5.yaml | +92/-0 | Adds a `notice` object on answers produced in a reduced form (Z-41). | contract change; Quarry and Model Plane consumers |
| 21 | A | M | apps/Data Plane v2/scripts/auth-regression-matrix.sh | +19/-0 | Adds DPV2_SERVICE_BEARER and quarry-edge service-principal rows (QB-T08). | - |
| 22 | A | M | apps/Data Plane v2/scripts/check-usage-contract.sh | +11/-1 | Usage contract check also covers the rerank contract fixtures (usage-ledger-v2). | - |
| 23 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/Cargo.toml | +6/-0 | usage-ledger-rs path dependency and rustls (test TLS stand-in). | - |
| 24 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/api/mod.rs | +45/-9 | Router merges POST /v1/rerank (RerankState) before the engine-wide layers, with its own body limit and rate gate. | shared router |
| 25 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/audit/mod.rs | +6/-0 | AuditCause lets a handler name its access-row cause (rerank:ok, rerank:skipped_zdr). | - |
| 26 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/authz/context.rs | +6/-0 | Sets req.service_id from the verified service principal (boundary only) for corpus confinement. | authz boundary |
| 27 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/authz/principal.rs | +8/-9 | Access-log cause derivation moved to AuditCause. | - |
| 28 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/authz/service_routes.rs | +111/-0 | Service corpus grants and confinement (QB-M1a); refuses space_scope for service principals (line 220; check at line 244). | Space authority boundary; keep user-only |
| 29 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/config.rs | +311/-0 | Web-rerank switches (on, processor, usage emit), bounded parsing, rerank input-context switch. | large |
| 30 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/embed/mod.rs | +356/-9 | Embedding client takes egress attestation; ZdrEgressRefused refuses dense egress for ZDR content before any call (Z-41). | large; WS14 hotspot |
| 31 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/grpc/retrieval_svc.rs | +128/-3 | gRPC retrieval refuses a reduced answer whose notice cannot ride the stream trailer (Z-41). | - |
| 32 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/lib.rs | +2/-0 | Exports egress_attestation and rerank_api. | - |
| 33 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/main.rs | +167/-3 | Boot wiring: egress attestation, rerank state, web-rerank config, usage ledger client. | hot file |
| 34 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/metrics/mod.rs | +99/-0 | Web-rerank metrics: outcome, duration, documents, search units, budget exceeded, cost failure. | - |
| 35 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs | +313/-52 | DenseEgress decision, rerank_requested_for_request, chunk-context attach, restrictive-posture paths. | hot file; WS14 and WS17 |
| 36 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/postprocess.rs | +14/-0 | Confined service candidates must be live documents in granted sources; wiki branch skipped for services. | service boundary |
| 37 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/types.rs | +118/-3 | chunk_context on candidates; RetrievalNotice type; request carries service_id. | - |
| 38 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/rate_limit/mod.rs | +30/-2 | Rate limiter for the rerank route (rerank_from_env, keyed check). | - |
| 39 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/redact.rs | +100/-0 | query_for_log withholds query text on retrieval.start under restrictive posture (ZDR reject or ephemeral; sovereignty). | - |
| 40 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/search/filters.rs | +15/-0 | Qdrant filter pins the `source` axis for confined service corpora (QB-M1a). | - |
| 41 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/src/search/rerank.rs | +835/-3 | Reranker client: provider scores and limits, breaker policy, chunk-context rerank document, web capacity. | largest Data Plane change |
| 42 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/tests/grpc_integration.rs | +2/-2 | Test signature follows the request change. | - |
| 43 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/tests/retrieve_contract.rs | +31/-1 | Imports rerank_requested_for_request; +31 lines of new assertions (not read in full). | - |
| 44 | A | M | apps/Data Plane v2/services/retrieval-engine-rs/tests/zdr_behavior.rs | +19/-3 | test_db_url_or_skip helper; expectations updated for the new posture. | - |
| 45 | A | M | apps/Data Plane v2/tests/e2e/isolated/fixture.mjs | +32/-1 | Adds the mint-service command for the quarry-edge service bearer (QB-T08). | - |
| 46 | A | M | apps/Data Plane v2/tests/e2e/run-isolated-mvp.sh | +4/-0 | Mints and uses the quarry-edge bearer with data:read only (QB-T08). | - |
| 47 | A | M | apps/Data Plane v2/tests/scripts/auth-regression-matrix-test.sh | +190/-7 | Service rows told apart by the bearer each call carries (QB-T08). | large test script |
| 48 | A | M | apps/Data Plane v2/tests/scripts/run-isolated-real-authority.sh | +9/-0 | quarry-edge registry row generated from the committed principal policy (QB-T08). | - |
| 49 | A | U | apps/Data Plane v2/docs/web-rerank-v1.md | 315 lines | Runbook for POST /v1/rerank (Quarry option B). | - |
| 50 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/src/egress_attestation.rs | 806 lines | Z-41 egress attestation: attested routes for ZDR-restricted content; no attestation means lexical-only answers. | new; large |
| 51 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/ | dir (5 files, 1,639 lines) | POST /v1/rerank (QB-T05a): wire schema, pure decision policy, replay cache, limits. | new route |
| 52 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/chunk_context_join.rs | 149 lines | CR-T15 rerank-input context join (needs TEST_DATABASE_URL). | DB test |
| 53 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/chunk_context_rerank.rs | 177 lines | CR-T15: what the reranker receives (localhost HTTP stand-in). | - |
| 54 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/fixtures/contracts/rerank-v1/ | dir | Golden rerank-v1 wire fixtures and SHA256SUMS. | - |
| 55 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/fixtures/contracts/usage-ledger-v2/ | dir | usage-ledger-v2 envelope fixture and its sha256. | - |
| 56 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/rerank_contract.rs | 2,326 lines | QB-T05a rerank contract tests; one live test (paid endpoint, #[ignore]). | live test |
| 57 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/rerank_provider_gate.rs | 584 lines | QB-T05b: provider gate shared by tenant retrieval and /v1/rerank. | - |
| 58 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/retrieval_start_log_redaction.rs | 429 lines | Asserts retrieval.start withholds query text under restrictive posture. | - |
| 59 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/service_scope.rs | 757 lines | QB-M1a corpus confinement on the real request path. | DB tests #[ignore]d |
| 60 | A | U | apps/Data Plane v2/services/retrieval-engine-rs/tests/zdr_dense_egress.rs | 376 lines | Z-41: ZDR retrieval without an attested embedding route answers lexically with a notice. | - |
| 61 | A | U | apps/Data Plane v2/tests/scripts/rerank-input-contract-test.sh | 110 lines | CR-T15 rerank-input contract script. | - |
| 62 | A | U | apps/Data Plane v2/tests/scripts/web-rerank-compose-contract-test.sh | 468 lines | Compose contract test for the web-rerank route. | - |
| 63 | A | U | apps/Data Plane v2/scripts/qb-preflight.sh | 899 lines | QB-T08 pre-flight script for the Quarry option-B rollout. | large script |
| 64 | A | U | apps/Data Plane v2/tests/scripts/isolated-plane-principals.mjs | 75 lines | QB-T08: generates the quarry-edge row of the isolated registry from the committed principal policy. | - |
| 65 | A | U | apps/Data Plane v2/tests/scripts/isolated-registry-parity-test.sh | 143 lines | QB-T08 parity test for the generated registry row. | - |
| 66 | A | M | apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx | +1/-1 | Passes the refusal's feature key into translateApiError (a ZDR refusal names its feature). | WS01 and WS07 hotspot |
| 67 | A | M | apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.test.tsx | +19/-1 | Test: a ZDR refusal names its feature in the room. | - |
| 68 | B | M | apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx | +9/-2 | ZDR refusal shown with its next step; onError receives the failure object. | WS09 |
| 69 | B | M | apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.test.tsx | +40/-0 | ZDR refusal tests for the run console. | - |
| 70 | B | M | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx | +185/-44 | ZDR ErrorNotice with connection-review action; prompt-suggestion and notification imports. | mixed (chat and ZDR); WS15 |
| 71 | B | M | apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.failure-actions.test.tsx | +34/-0 | Test: a ZDR refusal sends the reader to review the connection beside a retry. | - |
| 72 | B | M | apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.ts | +39/-0 | Maps zdr_* refusal codes to catalogue words and action inside turn errors. | - |
| 73 | B | M | apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts | +5/-2 | New retry action open_integrations for ZDR connection review. | - |
| 74 | B | M | apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts | +4/-0 | Carries the refusal feature key through the turn failure. | - |
| 75 | B | M | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts | +53/-0 | retrieval_notice handler and ZDR refusal shapes on the chat stream. | WS02 and WS09 |
| 76 | B | M | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.test.ts | +22/-0 | Test: retrieval_notice routed by code and wording key. | - |
| 77 | B | M | apps/Frontend Plane/verevonv3/src/shared/api/http.ts | +18/-6 | Reads the refused feature key from zdr_feature_not_supported bodies. | - |
| 78 | B | M | apps/Frontend Plane/verevonv3/src/shared/api/sse.ts | +2/-2 | SSE errors carry the feature into ApiError. | WS02 |
| 79 | B | M | apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts | +12/-0 | zdr_* refusals render from the catalogue seam (shared/api/zdr-refusals.ts). | - |
| 80 | B | M | apps/Frontend Plane/verevonv3/src/shared/api/notifications-client.ts | +11/-7 | Fields now recipient_id, organization_id, event_type, channel, archived, submitted_at (match committed notification-core tags). | WS11; corrects committed drift |
| 81 | B | U | apps/Frontend Plane/verevonv3/src/features/chat/components/chat-normalizers.zdr-refusal.test.ts | 66 lines | A turn refusal shows catalogue words in the reader's language. | - |
| 82 | B | U | apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.zdr-refusal.test.ts | 145 lines | Controller test for zdr_* refusals (feature and action). | - |
| 83 | B | U | apps/Frontend Plane/verevonv3/src/shared/api/chat-client.zdr-refusal.test.ts | 129 lines | Three refusal shapes: stream frame, AG-UI run error, HTTP answer. | - |
| 84 | B | U | apps/Frontend Plane/verevonv3/src/shared/api/http.zdr-refusal.test.ts | 40 lines | Feature-key extraction from refusal bodies. | - |
| 85 | B | U | apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts | 268 lines | Norwegian and English words for every typed ZDR refusal (client catalogue). | prerequisite (ZDR catalogue) |
| 86 | B | U | apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.test.ts | 178 lines | Catalogue test against the generated JSON. | - |
| 87 | B | U | apps/Frontend Plane/verevonv3/src/shared/api/contracts/ | dir (2 files) | Client copy of the generated catalogue; hash equals the Model Plane file. | keep in sync |
| 88 | B | U | apps/Frontend Plane/verevonv3/src/shared/i18n/errors.zdr-refusal.test.ts | 43 lines | Every JSON surface that reports a ZDR refusal reads its words from the seam. | - |
| 89 | A | M | apps/Ingestion Plane/docker-compose.yml | +36/-14 | AUTOPROMPT and SEMANTIC_RERANK change from default 1 (committed) to 0; six Model Plane switches (ANSWER_SYNTHESIS, AI_FORMATS, MODEL_AUDIO, SEARCH_SUGGEST, LLM_CLASSIFY_INTENT, PAGE_TITLE_ENRICH) are added at 0; RERANK_SHADOW added at 1. | BEHAVIOUR CHANGE; section 7 |
| 90 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/answer_routes.rs | +28/-2 | /v1/answer error names the QUARRY_EDGE__ANSWER_SYNTHESIS switch instead of the Model Plane URL. | - |
| 91 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/audio_routes.rs | +63/-0 | Audio route returns 501 unless the model_audio feature is on. | - |
| 92 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs | +167/-20 | Model Plane switches, rerank deadline, defaults(). | - |
| 93 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs | +57/-3 | Model Plane client used only when the ai_formats feature is on. | - |
| 94 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/lib.rs | +1/-0 | Exports model_plane_gate. | - |
| 95 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs | +132/-56 | Boot calls model_plane_gate::decide once over the config. | hot file |
| 96 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs | +1/-0 | Default ModelPlaneFeatures in route state. | - |
| 97 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs | +530/-81 | cacheable_search(zdr, rerank), params_signature, apply_score_policy (line 585). | large |
| 98 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/state.rs | +13/-8 | AppState carries ModelPlaneFeatures. | - |
| 99 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/test_support.rs | +1/-0 | Test state carries Model Plane features. | - |
| 100 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/support/mod.rs | +1/-0 | Test support: Model Plane features. | - |
| 101 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/zdr_claim_floor.rs | +10/-0 | ZDR claim-floor test enables model_audio. | - |
| 102 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json | +2/-3 | Rerank client added as a user of the rerank token request (QB-T09). | byte-identical to row 13 |
| 103 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json.sha256 | +1/-1 | Pin for the manifest above (verified: matches). | - |
| 104 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/fusion.rs | +452/-59 | Fusion rewritten around a Page accumulator and dedupe_key. | large |
| 105 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/hybrid.rs | +262/-30 | Hybrid search: vector timeout; ZDR mode from quarry_core. | - |
| 106 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs | +5/-1 | Exports data_plane_http and rerank modules. | - |
| 107 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/rerank.rs | +1,283/-293 | Web-rerank client; RerankReport and RerankStatus (applied, skipped, failed) with cacheability (QB option B). | largest Quarry change |
| 108 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/serp.rs | +54/-1 | SERP search returns the rerank report. | - |
| 109 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/smart_router.rs | +1/-59 | Dedupe helpers moved to fusion.rs. | - |
| 110 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/token_request_contract.rs | +14/-3 | Wire-contract test moved to the VectorQuery API (adds zdr: ZdrMode::Off); no rerank path in this file. | - |
| 111 | A | M | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/vector_index.rs | +603/-242 | Typed SkipReason and VectorOutcome; DataPlaneZdrMode sent on the vector leg. | large |
| 112 | A | M | apps/Ingestion Plane/Quarry-v2/docs/CROSS_PLANE_AUTH.md | +2/-2 | Cross-plane auth wording. | doc |
| 113 | A | M | apps/Ingestion Plane/Quarry-v2/docs/CROSS_PLANE_INTEGRATION.md | +3/-2 | Integration doc: rerank path. | doc |
| 114 | A | M | apps/Ingestion Plane/Quarry-v2/docs/Features.md | +3/-3 | Feature list: switches and rerank. | doc |
| 115 | A | M | apps/Ingestion Plane/Quarry-v2/docs/ONBOARDING_LIVE_CRAWL.md | +4/-3 | Live crawl onboarding wording. | doc |
| 116 | A | M | apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md | +90/-0 | Runbook additions for the rerank route and switches. | doc |
| 117 | A | M | apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md | +23/-1 | Self-host notes: switches and rerank env. | doc |
| 118 | A | M | apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml | +77/-1 | OpenAPI: rerank and Model Plane fields. | doc and contract |
| 119 | A | M | apps/Ingestion Plane/Quarry-v2/lab/evals/Cargo.toml | +4/-0 | Eval crate dependency for the web-rerank gate. | - |
| 120 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/model_plane_gate.rs | 401 lines | Decides the Model Plane features once at boot (ModelPlaneFeatures, decide). | - |
| 121 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs | 469 lines | /v1/search semantic rerank wired to the Data Plane (rerank-v1). | - |
| 122 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/data_plane_http.rs | 238 lines | One HTTP path for every Data Plane call (retrieve and rerank). | - |
| 123 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_rerank_contract.rs | 537 lines | Wire contract with the Data Plane rerank route. | - |
| 124 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_retrieve_contract.rs | 268 lines | Wire contract with the Data Plane retrieve route. | - |
| 125 | A | U | apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/ | dir | Golden fixtures for the two Data Plane contracts (rerank-v1, retrieve-v1). | - |
| 126 | A | U | apps/Ingestion Plane/Quarry-v2/docs/WEB_RERANK_GATE_RUNBOOK.md | 40 lines | Operator runbook for the web-rerank gate (QB-T14). | - |
| 127 | A | U | apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/ | dir | Judged web-result fixtures: queries and results present; judgments.v1.jsonl is 0 bytes. | FLAG: empty judgments (section 7) |
| 128 | A | U | apps/Ingestion Plane/Quarry-v2/lab/evals/src/web_rerank_gate.rs | 3,841 lines | QB-T14 gate: judged set, runner, calibration fit, regression gate. | large |
| 129 | A | U | apps/Ingestion Plane/Quarry-v2/lab/evals/web-rerank-baseline.json | 23 lines | Baseline for the web-rerank gate. | - |
| 130 | A | U | apps/Ingestion Plane/Quarry-v2/scripts/tests/ | dir | docs-contract.test.mjs (337 lines): docs and contract checks for option B and the Model Plane switches. | - |
| 131 | B | M | apps/Ingestion Plane/integration-corev2/README.md | +26/-0 | README: ZDR acknowledgement (Z-13) section and env table. | - |
| 132 | B | M | apps/Ingestion Plane/integration-corev2/cmd/api/main.go | +9/-0 | Boot checks acknowledgement wording pins; stops the service on mismatch when the routes are on. | - |
| 133 | B | M | apps/Ingestion Plane/integration-corev2/internal/api/codex_subscription.go | +8/-0 | Removing a ChatGPT subscription revokes its live acknowledgements in the same transaction. | - |
| 134 | B | M | apps/Ingestion Plane/integration-corev2/internal/api/server.go | +30/-0 | Wires the org-core posture reader for the acknowledgement API; nil fails closed. | - |
| 135 | B | M | apps/Ingestion Plane/integration-corev2/internal/auth/middleware.go | +56/-37 | Bearer parsing split into authenticateBearer; the new BearerOnly guard (server.go line 218) serves the user-scoped acknowledgement routes. | verify other routes unchanged; bearer_only_test.go (row 147) covers BearerOnly |
| 136 | B | M | apps/Ingestion Plane/integration-corev2/internal/config/config.go | +2/-0 | ZDR_ACKNOWLEDGEMENT_ENABLED switch, default false. | - |
| 137 | B | M | apps/Ingestion Plane/integration-corev2/internal/controlplane/clients.go | +32/-0 | org-core client EffectiveZDR: stored intent AND plan entitlement; any failure is an error (fail closed). | - |
| 138 | B | M | apps/Ingestion Plane/integration-corev2/internal/db/zdr_postgres_test.go | +17/-0 | Postgres ZDR lifecycle contract test on a disposable Postgres. | DB test |
| 139 | B | M | apps/Ingestion Plane/integration-corev2/internal/oauth/service.go | +4/-0 | Removing a model connection revokes its ZDR acknowledgement in the same transaction. | - |
| 140 | B | M | apps/Ingestion Plane/integration-corev2/internal/store/memory_zdr.go | +62/-0 | In-memory ZDR ledger: revocation on removal and organisation purge. | - |
| 141 | B | M | apps/Ingestion Plane/integration-corev2/internal/store/postgres_zdr.go | +112/-6 | Postgres ledger: acknowledgement insert re-reads the connection under lock (removal race). | - |
| 142 | B | M | apps/Ingestion Plane/integration-corev2/internal/store/postgres_zdr_test.go | +29/-0 | Mock-based tests for the Postgres ZDR store. | - |
| 143 | B | M | apps/Ingestion Plane/integration-corev2/internal/store/zdr.go | +73/-0 | Store interface gains PurgeOrganizationConnectionZDR (the only ledger delete). | - |
| 144 | B | M | apps/Ingestion Plane/integration-corev2/internal/zdr/assessor.go | +10/-0 | WordingVariants: required acknowledgement text variants per key and locale. | - |
| 145 | B | U | apps/Ingestion Plane/integration-corev2/internal/api/zdr_acknowledgement.go | 534 lines | ZDR acknowledgement API (Z-13): a member acknowledges an exact assessment and text. | new route |
| 146 | B | U | apps/Ingestion Plane/integration-corev2/internal/api/zdr_acknowledgement_test.go | 754 lines | Acknowledgement API tests with a fake org posture. | - |
| 147 | B | U | apps/Ingestion Plane/integration-corev2/internal/auth/bearer_only_test.go | 110 lines | Bearer-only path tests with a stub verifier. | - |
| 148 | B | U | apps/Ingestion Plane/integration-corev2/internal/config/zdr_acknowledgement_config_test.go | 30 lines | Acknowledgement routes dark by default; config switch test. | - |
| 149 | B | U | apps/Ingestion Plane/integration-corev2/internal/controlplane/org_zdr_posture_test.go | 97 lines | org-core posture route fixture that mirrors the real response. | - |
| 150 | B | U | apps/Ingestion Plane/integration-corev2/internal/store/memory_zdr_lifecycle_test.go | 16 lines | Memory store runs the shared lifecycle contract. | - |
| 151 | B | U | apps/Ingestion Plane/integration-corev2/internal/store/storetest/zdr_lifecycle_contract.go | 145 lines | Shared lifecycle contract (revocation, purge, admin listing) for memory and Postgres. | - |
| 152 | B | U | apps/Ingestion Plane/integration-corev2/internal/zdr/texts/ | dir | Versioned acknowledgement texts (en and no variants) and index.json. | pinned by wording_test.go (row 154) |
| 153 | B | U | apps/Ingestion Plane/integration-corev2/internal/zdr/wording.go | 272 lines | Loader for the versioned acknowledgement wording. | - |
| 154 | B | U | apps/Ingestion Plane/integration-corev2/internal/zdr/wording_test.go | 240 lines | Snapshot pins of every acknowledgement text, by locale, key and variant. | - |
| 155 | B | M | apps/Model Plane/api/contracts/README.md | +1/-0 | README lists the generated zdr-refusals.json and its regeneration command. | - |
| 156 | B | M | apps/Model Plane/docs/ZDR.md | +63/-1 | Decision 17, receipts and the thread-receipt gap; status table lists Z-05a, Z-11, Z-12/13, Z-14 and Z-15 to Z-18 as "Not built" (stale against the uncommitted code). | FLAG doc drift (section 7) |
| 157 | A | M | apps/Model Plane/docs/core-research/model-gateway.md | +9/-0 | Runbook for the Quarry cross-encoder switch (QB-T12): off until a fitted table is pinned. | - |
| 158 | B | M | apps/Model Plane/docs/sse-event-taxonomy.md | +4/-0 | Taxonomy adds error (ZdrError), plan_proposed and retrieval_notice events. | WS02 and WS09 |
| 159 | A | M | apps/Model Plane/proto/model_plane/v1/sessions.proto | +42/-0 | Adds SessionRetentionService (read-only waived-run counts per org) as its own service. | proto change |
| 160 | A | M | apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs | +65/-2 | Data Plane zdr_mode per turn: waived to reject, ZDR to ephemeral, persistent to disabled (matrix tests). | - |
| 161 | A | M | apps/Model Plane/rust/crates/mp-contracts/src/lib.rs | +2/-0 | Exports the retention and zdr_refusal modules. | - |
| 162 | B | M | apps/Model Plane/rust/services/inference-core/src/auth.rs | +402/-2 | Strict zdr claim; ZdrWaiver read from the token (Z-16); enforce_zdr_route and enforce_non_chat_zdr_posture (lines 311, 364). | inference-core (not in the named list) |
| 163 | B | M | apps/Model Plane/rust/services/inference-core/src/grpc.rs | +129/-2 | Modality refusals go through the single catalogue helper; waiver route posture check (Z-16). | - |
| 164 | A | M | apps/Model Plane/rust/services/execution-core/src/agent_route.rs | +253/-32 | ModelRoute resolve and validate take the caller; a waiver binds one route; ZDR run route checked against the waiver (Z-18). | - |
| 165 | A | M | apps/Model Plane/rust/services/execution-core/src/auth.rs | +608/-23 | AuthenticatedUser carries org_zdr and zdr_waiver; delegated bearers must match the caller's posture (POSTURE_MISMATCH); service tokens cannot carry a waiver. | large authz change |
| 166 | A | M | apps/Model Plane/rust/services/execution-core/src/grpc.rs | +76/-22 | Persistence-free refusals take typed feature keys (tool_execution, run_control, agent_run); caller passed to route checks. | - |
| 167 | A | M | apps/Model Plane/rust/services/execution-core/src/knowledge_tools.rs | +125/-6 | Knowledge search sends an explicit zdr_mode (ephemeral, reject or disabled) and waived filtering. | - |
| 168 | A | M | apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs | +200/-7 | Caller threaded through run_agent; waived steps use filtered retrieval; approval continuation under ZDR is now a typed refusal (approval_continuation), not removed. | FLAG WS10 |
| 169 | A | M | apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs | +18/-0 | execute_step paths carry the waived flag. | - |
| 170 | A | M | apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs | +536/-1 | Test double of session-core credential rules and receipts; gated by #[cfg(test)] (mod.rs line 9). | size note only |
| 171 | A | M | apps/Model Plane/rust/services/execution-core/src/workspace_promote.rs | +4/-3 | Document promotion under ZDR refused with the typed DURABLE_MUTATION_FORBIDDEN instead of free text. | - |
| 172 | A | M | apps/Model Plane/rust/services/model-gateway/src/auth.rs | +389/-26 | Strict zdr claim required on user tokens; ZdrWaiverClaim; AuthRejection. | - |
| 173 | A | M | apps/Model Plane/rust/services/model-gateway/src/browser_run.rs | +18/-11 | Browser run under ZDR refused as typed feature_not_supported("browser_run"). | sibling (b) browser grant path |
| 174 | A | M | apps/Model Plane/rust/services/model-gateway/src/chat_turn_registry.rs | +36/-0 | Turn registry records private_run, so the following turn knows the answer was private. | - |
| 175 | A | M | apps/Model Plane/rust/services/model-gateway/src/dataplane.rs | +170/-24 | Proxied retrieval: waived turns use reject mode; ZDR answers carry a notice; typed refusal mapping from the Data Plane. | - |
| 176 | A | M | apps/Model Plane/rust/services/model-gateway/src/finetune_routes.rs | +16/-15 | Fine-tune refresh skipped under ZDR; durable-mutation refusal typed. | - |
| 177 | A | M | apps/Model Plane/rust/services/model-gateway/src/gateway_metrics.rs | +10/-0 | Metric for the ZDR waiver contract version (recorded at boot in main.rs). | - |
| 178 | A | M | apps/Model Plane/rust/services/model-gateway/src/grpc.rs | +31/-16 | Refusal helpers take feature keys; effective_zdr replaced by posture. | - |
| 179 | A | M | apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs | +189/-10 | gRPC verifier requires the zdr claim; delegated user credentials must match the ingress posture. | - |
| 180 | A | M | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | +190/-47 | approve_plan (line 1393), reject_plan, update_todo_status, decide_approval (1496), cancel_run (1606), resume_run (1713) gated by the posture continuation check; blanket ZDR refusal removed from decide_approval, cancel_run and resume_run. | FLAG WS10: verify ZDR still refused on these routes |
| 181 | A | M | apps/Model Plane/rust/services/model-gateway/src/implicit_feedback.rs | +1/-0 | Feedback record sets private_run: false. | - |
| 182 | A | M | apps/Model Plane/rust/services/model-gateway/src/lib.rs | +3/-0 | Exports zdr_features, zdr_posture and relevance_calibration. | - |
| 183 | A | M | apps/Model Plane/rust/services/model-gateway/src/main.rs | +1/-0 | Records the ZDR waiver contract version at boot. | - |
| 184 | A | M | apps/Model Plane/rust/services/model-gateway/src/normalize.rs | +12/-5 | Connection-not-supported refusal typed from the catalogue instead of body-flag text. | - |
| 185 | A | M | apps/Model Plane/rust/services/model-gateway/src/quarry.rs | +534/-12 | Quarry search reply: RerankOutcome and RerankStatus, cross-encoder calibration read, web search item shape (QB-T12). | large |
| 186 | A | M | apps/Model Plane/rust/services/model-gateway/src/relevance.rs | +163/-7 | ScoreKind parsing; cross-encoder score read only through a calibration table; unknown kinds never read. | - |
| 187 | A | M | apps/Model Plane/rust/services/model-gateway/src/retrieval.rs | +659/-23 | data_plane_zdr_mode(zdr, waived) (line 901), retrieve_checked and the dense-unavailable refusal; space_decision parameter (line 1053) passed into retrieval. | large; Space decision plumbing |
| 188 | A | M | apps/Model Plane/rust/services/model-gateway/src/retrieval_metadata.rs | +232/-3 | RetrievalNotice {code, wording key} decoded from Data Plane metadata; the sentence never crosses the gateway. | - |
| 189 | A | M | apps/Model Plane/rust/services/model-gateway/src/retrieval_tools.rs | +2/-2 | Retrieval tool carries zdr_mode (Option of string) instead of a zdr bool. | - |
| 190 | A | M | apps/Model Plane/rust/services/model-gateway/src/session_flow.rs | +5/-0 | Typed catalogue refusals pass through the session flow unchanged. | - |
| 191 | A | M | apps/Model Plane/rust/services/model-gateway/src/sse.rs | +814/-69 | ZDR error and prepared-refusal streams, zdr_enforced_floor, ZDR turn settings, retrieval notice on grounding. | large; WS02 and WS09 hotspot |
| 192 | A | M | apps/Model Plane/rust/services/model-gateway/src/sse_events.rs | +202/-1 | ChatEvent: RetentionClass on usage frames; retrieval_notice and ZdrError events. | WS02 and WS09 taxonomy |
| 193 | A | M | apps/Model Plane/rust/services/model-gateway/src/state.rs | +5/-1 | AppState loads cross-encoder scores from env at boot (refused while the table is unfitted). | - |
| 194 | A | M | apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs | +370/-55 | One typed retrieval notice on reduced knowledge search under ZDR; rerank gate sentence in tool output. | - |
| 195 | A | M | apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs | +58/-226 | Request precheck reduced: feature decisions moved to zdr_posture and zdr_features; keeps the http() mapping and connection-name tests. | FLAG: -226 lines; verify no gate lost |
| 196 | A | M | apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | +271/-17 | End-to-end ZDR chain: refusal before any model call; answers under instructions and ceiling. | - |
| 197 | A | M | apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs | +102/-0 | ZDR credential refused before any plan read; waiver not refused at the gateway (session-core checks the receipt). | FLAG WS10 |
| 198 | A | M | apps/Model Plane/rust/services/model-gateway/tests/source_review_live.rs | +7/-2 | Live source review reads the zdr claim (ignored live test). | - |
| 199 | A | M | apps/Model Plane/rust/services/session-core/src/auth.rs | +396/-0 | VerifiedIdentity carries zdr_waiver; parse_zdr_waiver (line 921) refuses malformed waivers; authorize_owner_housekeeping (line 295; owner decision 17). | FLAG WS01 hotspot |
| 200 | A | M | apps/Model Plane/rust/services/session-core/src/dreaming.rs | +47/-5 | Grounding pool skips threads with a waived run (thread_has_waived_run, line 1253). | FLAG WS17 |
| 201 | A | M | apps/Model Plane/rust/services/session-core/src/grpc.rs | +817/-44 | start_run_inner waiver and receipt checks (line 3071); SessionRetentionGrpc (7389); owner housekeeping calls (5653 to 6007); ZDR owner presentation, archive and delete; permits_external_memory_indexing (4147). | FLAG WS01, WS02, WS09, WS12, WS13, WS15 hotspot; AppendMessage untouched |
| 202 | A | M | apps/Model Plane/rust/services/session-core/src/learning_events.rs | +66/-11 | withheld_reason (line 172): ZDR and waived runs are never announced; retention gate runs before the outbox insert. | FLAG WS17 and WS02 outbox |
| 203 | A | M | apps/Model Plane/rust/services/session-core/src/main.rs | +1/-0 | Registers SessionRetentionService. | - |
| 204 | A | M | apps/Model Plane/rust/services/session-core/src/memory_dedup.rs | +5/-0 | Dedupe SQL excludes memories whose source thread has a zdr_waived run (line 796). | FLAG WS17 |
| 205 | A | M | apps/Model Plane/rust/services/session-core/src/memory_grpc.rs | +107/-16 | Memory list is a no-op for ZDR callers; ZDR owner may delete their own memory; receipt-aware tests. | FLAG WS17 |
| 206 | A | M | apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs | +243/-6 | Proof bundle cites the run's retention receipt; plan and approval decisions follow the run receipt; typed refusals. | FLAG WS10 |
| 207 | A | M | apps/Model Plane/rust/services/session-core/src/orchestration_store.rs | +7/-1 | RunProvenanceRow carries retention_class, waiver_ack_id and waiver_connection_id. | - |
| 208 | A | M | apps/Model Plane/rust/services/session-core/src/run_service_grpc.rs | +10/-11 | ResolveRunOwner answers via retention_receipt::run_owner_matches; a posture mismatch is a typed refusal. | FLAG WS09 and WS10 |
| 209 | A | M | apps/Model Plane/rust/services/session-core/src/terminalization.rs | +108/-15 | Managed run start takes RunRetention (start_managed_run_with_retention_inner); idempotent start keyed on the receipt. | FLAG WS02 terminalization |
| 210 | B | U | apps/Model Plane/api/contracts/zdr-refusals.json | 221 lines | Generated refusal catalogue: 10 codes (zdr_unavailable to zdr_inference_failed). | prerequisite (ZDR catalogue) |
| 211 | B | U | apps/Model Plane/api/contracts/zdr-refusals.json.sha256 | 1 line | Pin for the catalogue (verified: matches). | - |
| 212 | A | U | apps/Model Plane/rust/crates/mp-contracts/src/retention.rs | 518 lines | Pure retention resolver resolve_turn: waiver, receipt and route decision. | prerequisite |
| 213 | A | U | apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | 1,006 lines | Typed refusal catalogue for every Model Plane service; generates zdr-refusals.json. | prerequisite |
| 214 | A | U | apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/ | dir (2 files; posture matrix 800 lines) | Golden posture matrix (sha matches its pin). | - |
| 215 | A | U | apps/Model Plane/rust/crates/mp-contracts/tests/zdr_retention_matrix.rs | 732 lines | Runs the shared posture matrix through the resolver. | - |
| 216 | A | U | apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs | 90 lines | Every execution-core ZDR refusal is typed. | - |
| 217 | B | U | apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs | 840 lines | Inference boundary under a connection-bound waiver (Z-16). | - |
| 218 | B | U | apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs | 60 lines | Every inference-core ZDR refusal is typed. | - |
| 219 | A | U | apps/Model Plane/rust/services/model-gateway/calibration/ | dir (2 files) | Cross-encoder calibration table in state "unfitted" (knots null); boot refuses the switch. | FLAG unfinished (section 7) |
| 220 | A | U | apps/Model Plane/rust/services/model-gateway/src/relevance_calibration.rs | 591 lines | Calibration of cross_encoder.v1 scores relayed by Quarry. | - |
| 221 | A | U | apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs | 365 lines | What a ZDR turn does with each invoke field and feature; typed refusal per unsupported feature. | - |
| 222 | A | U | apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs | 1,259 lines | Turn posture decided once from verified claims and route; continuation_gate (line 257) for plan, approval, todo and resume. | FLAG WS10 hotspot; large |
| 223 | A | U | apps/Model Plane/rust/services/model-gateway/tests/fixtures/auth-core/ | dir (2 files) | Golden Quarry token claims (auth-core contract) for gateway tests. | - |
| 224 | A | U | apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json | 800 lines | Gateway copy of the posture matrix (same hash as the mp-contracts copy). | - |
| 225 | A | U | apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json.sha256 | 1 line | Pin for the gateway copy (verified: matches). | - |
| 226 | A | U | apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs | 366 lines | ZDR feature table for every invoke field and feature string. | - |
| 227 | A | U | apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs | 240 lines | Posture matrix through the gateway decision. | - |
| 228 | A | U | apps/Model Plane/rust/services/model-gateway/tests/zdr_refusal_inventory.rs | 63 lines | Every gateway ZDR refusal is typed. | - |
| 229 | A | U | apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql | 80 lines | Retention receipt columns and constraints on runs (Z-17). | FLAG migration |
| 230 | A | U | apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql | 17 lines | Org-scoped index for waived runs (Z-21a counts). | FLAG migration |
| 231 | A | U | apps/Model Plane/rust/services/session-core/migrations/revert/ | dir (2 files: 0044 and 0045 .down.sql) | Down migrations; the directory sits inside migrations/ (sqlx reads the top level only). | verify no recursive loader |
| 232 | A | U | apps/Model Plane/rust/services/session-core/src/retention_receipt.rs | 1,735 lines | A run's retention receipt: posture at creation; run_owner_matches; require_stored_posture; DB tests #[ignore]d. | FLAG large; WS10 and WS01 dependency |

## 3. Adjacent (C) and not-Space (N) entries, not in section 2

Class C (34, collision risk only):
- Data Plane: `apps/Data Plane v2/services/usage-ledger-rs/src/call.rs` (M, +109): a rerank usage method on the usage-ledger path dependency of retrieval-engine.
- Frontend M (14): `src/features/chat/components/ChatPage.tsx` (work-period state), `ScheduleDialog.tsx` and `.test.tsx` (routines dialog seeds the goal), `src/features/core/components/CoreSidebar.tsx`, `sidebar/ChatSidebarOrganizer.tsx`, `sidebar/chat-sidebar-organizer.css` (ChatWorkLinks), `src/features/dashboard/home/DashboardComposer.tsx` (work-period picker), `DashboardUpdatesCard.tsx` and `.test.tsx` (workspace pages feed), `home-layout-state.test.ts` (feed key aquatiq to workspace), `src/shared/actions/home-updates.ts`, `src/shared/api/home-updates-client.ts` and `.test.ts` (feed rename), `src/styles/global.css` (+246: chat and error-notice styles).
- Frontend U (19): `verevon-web/plans/system-audits/program-2026-10-03/` (11 workflow scripts, including `run_session_db_tests.sh` and `run_quarry_tests.sh`), `src/features/chat/components/ChatUtilityPage.tsx`, `ChatWorkPeriodPicker.tsx`, `EmptyChatState.test.tsx`; `src/features/chat/lib/prompt-suggestions.ts` and `.test.ts`; `src/features/core/components/sidebar/ChatWorkLinks.tsx`; `src/shared/actions/chat-work-period.ts` and `.test.ts`.

Class N (58, not Space):
- Repo root (3): `.githooks/pre-commit` (+2, demo-bundle exclusion), `.github/workflows/verevonv3-ci.yml` (+55, demo-bundle gate), `.gitleaks.toml` (+9, allowlist for the eval corpus; see section 7).
- Data Plane U (5): `apps/Data Plane v2/scripts/eval-corpus/repo/apps/Infra Plane/WEBHOOK_SETUP.md`, `.../Ingestion Plane/END_TO_END_TESTS.md`, `QUICKSTART.md`, `README.md`, `TEST_DOCUMENTATION_INDEX.md` (copies of retrieval-eval corpus documents).
- Frontend M (18): verevon-web `next.config.ts`, `src/app/robots.ts`, `components/home/sections/ProductLoopProductDemos.tsx`, `ProductLoopSection.tsx`, `product-loop-progress.ts` and `.test.ts`; verevonv3 `package.json` (+2 demo scripts), `src/app/App.tsx` (scene route behind VITE_SCENE_MODE), `src/index.tsx` (scene runtime boot), `src/vite-env.d.ts`, `tsconfig.node.json`, `src/shared/api/information-client.ts` (weather glyphs), `src/features/core/components/CoreNavbar.tsx` (BASE_URL), `src/features/dashboard/home/DashboardCards.tsx`, `dashboard-cards.ts`, `KnowledgeComposer.tsx`, `browser-omnibox.ts` (VITE_DEMO), `dashboard-search-utils.ts` (VITE_DEMO).
- Frontend D (5, MISSING): `apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/dev/product-demo/page.tsx`, `.../dev/product-demo/product-demo.tsx`, `src/components/home/product-demo/demo-data.ts`, `product-demo.module.css`, `product-demo.tsx`.
- Frontend U (27): verevon-web `plans/product-demo/` and `plans/product-scenes/` (dirs), five `plans/system-audits/BUILD_PASS_*.md` files, `PRODUCT_CARD_FILMS_AUDIT_2026-10-11.md`, `public/product-demo/` and `public/product-scenes/` (dirs), `scripts/capture-scene.mjs`, `scripts/check-live-demo.mjs`, `scripts/scene-capture/` (dir), six `components/home/sections/ProductLoop*` files; verevonv3 `scripts/` (five demo tools), `src/demo/` (dir, 82 files), `vite.demo.config.ts`, `src/shared/api/information-client.weather.test.ts`.

## 4. Migrations

| Migrations directory | Uncommitted entries | Tracked highest | Next free (counting untracked) | Notes |
|---|---|---|---|---|
| apps/Model Plane/rust/services/session-core/migrations/ | U 0044_run_retention_receipt.sql; U 0045_zdr_waived_runs_org_index.sql; U revert/ (0044 and 0045 .down.sql) | 0043 (43 tracked, no duplicate numbers, no gaps) | **0046** | Confirms the brief (at least 0046). Reserve 0046 for the thread receipt if it is built (ZDR.md 5.11), since it is the next session-core migration. `sqlx::migrate!("./migrations")` is used at 38 committed call sites and reads the top level only, so `revert/` is not applied (verify there is no recursive loader). |

- Other directories with uncommitted entries: none. The status snapshot has no migration path outside session-core.
- Reference only (tracked highest + 1; no uncommitted entries): user-core 0030 (apps/Control Plane/user-core/migrations); org-core 0021; auth-core 0029; billing-core 0011; audit-core 0002 (migrations) and 0007 (internal/store/migrations); notification-core 0015 (apps/Application Plane/notification-core/migrations); conversation-core 0032; integration-corev2 0014 (internal/db/migrations); Quarry quarry-runtime 0010 and quarry-control 0015; imports-core 0005; capability-core 0016; cost-core 0011; sandbox-manager 0005. Data Plane infra/postgres/migrations uses 14-digit timestamps (no sequence). convex-core has no migrations directory (Convex schema.ts; no entries).
- Naming trap: `apps/Control Plane/session-core/migrations/` (7 tracked files, highest 005, next 0006) is a different directory from the Model Plane session-core. Do not number Space migrations there.

## 5. Uncommitted work that is a prerequisite or part of the Space goal

1. **ZDR refusal catalogue (Z-05a).** U `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs` (1,006 lines; typed refusals for every Model Plane service); U `apps/Model Plane/api/contracts/zdr-refusals.json` (10 codes; hash matches its pin); U client copy `apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json` (same hash); U `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts`; M `scripts/tests/zdr-contract-sync.test.sh` and `scripts/tests/zdr-contract-schema.mjs`. Every Space refusal (human post, agent run, approval) must use these codes. Prerequisite: yes (WS06).
2. **ZDR waiver and connection binding (Z-14, Z-16, Z-18).** M session-core `src/auth.rs` (`ZdrWaiver` line 64, `parse_zdr_waiver` line 921); M execution-core `src/auth.rs` and `src/agent_route.rs`; M model-gateway `src/auth.rs` and `src/grpc_auth.rs`; M inference-core `src/auth.rs`; U `inference-core/src/grpc/zdr_waiver_route_tests.rs`. A waiver is minted only behind Auth Core's kill switch, which is off by default (ZDR.md 5.11). Needed before personal agents act in ZDR-org rooms (WS17).
3. **Retention receipt (Z-17) and admin counts (Z-21a).** U `session-core/src/retention_receipt.rs` (1,735 lines); U migrations 0044 and 0045 (plus `revert/`); U `mp-contracts/src/retention.rs` (518 lines; the one resolver); M session-core `src/grpc.rs` (`start_run_inner` line 3071; `SessionRetentionGrpc` line 7389), `terminalization.rs`, `orchestration_store.rs`; M `apps/Model Plane/proto/model_plane/v1/sessions.proto` (SessionRetentionService). Prerequisite for WS10 (receipt-checked approvals and plan decisions), WS09 (run-owner answers) and any Space proof or audit view of ZDR runs.
4. **Owner decision 17 (ZDR.md 5.11).** Housekeeping (archive, delete, presentation controls, own memories) is open to ZDR owners; stored continuations (plans, todos, approvals, resume) need posture equality with the receipt. M session-core `src/auth.rs` (`authorize_owner_housekeeping` line 295), `src/grpc.rs` (call sites lines 5653 to 6007), `src/run_service_grpc.rs`; M model-gateway `src/http_routes.rs`; U `src/zdr_posture.rs` (`continuation_gate` line 257). Governs what a Space owner can do under ZDR (WS01 and WS12 thread housekeeping; WS10 approvals).
5. **Memory containment, partial (F-t1-012).** M session-core `src/memory_dedup.rs` (SQL excludes memories linked to a `zdr_waived` run, line 796); `src/dreaming.rs` (grounding pool line 586, `thread_has_waived_run` line 1253); `src/learning_events.rs` (`withheld_reason` line 172); `src/memory_grpc.rs`; `src/grpc.rs` (`permits_external_memory_indexing`, line 4147, replaces `permits_durable_memory` for the Letta sync). WS17 should build on this, not redo it.
6. **Egress attestation (Z-41) and reduced-answer notice (Z-41b).** U retrieval-engine `src/egress_attestation.rs` (806 lines) and `tests/zdr_dense_egress.rs`; M `src/embed/mod.rs` (`ZdrEgressRefused`), `docker-compose.yml`, `openapi/d4d5.yaml`; M model-gateway `src/retrieval_metadata.rs`, `src/sse_events.rs`, `src/tool_loop.rs`; M frontend `src/shared/api/chat-client.ts`. Required before any ZDR knowledge retrieval inside a Space room.
7. **Space authority guards already in the uncommitted tree.** M retrieval-engine `src/authz/service_routes.rs` refuses `space_scope` for service principals (error at line 220, check at line 244). The committed `src/space_scope.rs` already carries Space-scoped retrieval; the uncommitted change keeps it user-only. M model-gateway `src/retrieval.rs` (line 1053: `space_decision` is now passed into retrieval). Both follow the rule that Space access never widens an owner-plane ACL.
8. **Notification contract alignment (WS11).** M `apps/Frontend Plane/verevonv3/src/shared/api/notifications-client.ts` now uses `recipient_id`, `organization_id`, `event_type`, `channel`, `archived` and `submitted_at`. These match the committed notification-core JSON tags. It corrects a drift that already existed (the committed client used `userId` and `type`).
9. **Frontend ZDR presentation in Space.** M `src/features/spaces/components/SpaceRoomComposer.tsx` (+ test) passes the refusal feature key; M `src/features/agents/components/AgentRunConsole.tsx`; U `src/shared/api/zdr-refusals.ts`. Needed so WS01 and WS09 show ZDR refusals in words.
10. **Thread receipt (not built; open).** ZDR.md 5.11 says threads carry no receipt, so a waived `AppendMessage` is not checked against its thread's posture. Closing it needs a migration (next free 0046) and a stamp at thread creation. Not in the uncommitted tree. Prerequisite for WS01 human posts under ZDR.
11. **Not on the Space critical path (sibling a, Quarry and browser).** Quarry option-B web rerank (DP `rerank_api/` 1,639 lines and `rerank_provider_gate.rs`; Quarry `rerank.rs` +1,283/-293 and `data_plane_http.rs`; QB-T08 principal rows); cross-encoder calibration (QB-T12 and QB-T14; unfitted); browser-run ZDR refusal (`browser_run.rs`, WS20). Part of Space only through WS20 (room-scoped browser) and the shared service-principal rules.
12. **S4.x work.** No uncommitted entries. The status has no sandbox-manager, capability-core or process-registry paths; the S4.3 watch primitive has no code in the live tree.

## 6. Collisions with planned work (workstreams.md)

| Planned work | Uncommitted files it would collide with | Nature | Severity |
|---|---|---|---|
| WS01 Human posts (AppendMessage, member thread:append, SpaceRoomComposer) | session-core `src/grpc.rs` (+817/-44) and `src/auth.rs` (+396); `SpaceRoomComposer.tsx` | `VerifiedIdentity` gains `zdr_waiver`; `authorize_owner_housekeeping` sits beside `authorize_operation`; thread presentation, archive and delete changed. `AppendMessage` itself has 0 uncommitted hits (42 committed). | HIGH: same files, authorization model changed. Rebase on this tree first. |
| WS02 Realtime substrate (outbox, events, SSE) | session-core `src/learning_events.rs` (retention gate precedes the outbox insert), `src/terminalization.rs`; model-gateway `src/sse.rs` (+814/-69), `src/sse_events.rs` (+202); `docs/sse-event-taxonomy.md`; frontend `sse.ts`, `chat-client.ts` | Event taxonomy already grew (retrieval_notice, ZdrError, RetentionClass). A new Space outbox takes a session-core migration number after 0046. | MEDIUM-HIGH |
| WS06 ZDR safety (this tree is its ZDR slice) | all of section 5 | Treat as the baseline; do not re-plan these pieces. | n/a |
| WS09 Agent work visible to the room | model-gateway `sse.rs`, `sse_events.rs`; session-core `src/run_service_grpc.rs` (ResolveRunOwner now posture-checked); frontend `AgentRunConsole.tsx` | Same SSE and run-owner code paths. | HIGH |
| WS10 Approvals (durable resume, quarantine) | model-gateway `src/http_routes.rs` (approve_plan, reject_plan, update_todo_status, decide_approval, cancel_run, resume_run); `src/zdr_posture.rs` (continuation gate); session-core `src/orchestration_grpc.rs` (+243: proof bundle; decisions follow the run receipt); execution-core `src/runtime_loop/agent.rs` (approval continuation typed refusal) | Committed `approval_delivery.rs`, `approvals.rs` and the outbox workers are not modified, so the approval flow itself is untouched by this tree. | HIGH |
| WS11 Notifications, read state | frontend `notifications-client.ts` (contract change); `ChatMessages.tsx` (`listNotifications`) | Notification-core is not modified; the client now matches its committed tags. | MEDIUM |
| WS14 Attachments and file library (retrieval-engine on the data side) | retrieval-engine `src/api/mod.rs`, `src/main.rs`, `src/config.rs`, `src/pipeline/orchestrator.rs`, `src/embed/mod.rs`, `src/pipeline/types.rs` | Shared hot files, all uncommitted. | MEDIUM |
| WS17 Personal agents and audience-safe memory | session-core `src/memory_grpc.rs`, `src/memory_dedup.rs`, `src/dreaming.rs`, `src/learning_events.rs`, `src/grpc.rs` (`permits_external_memory_indexing`); model-gateway `src/zdr_posture.rs`; retrieval-engine `src/authz/service_routes.rs` | The waiver-based exclusion from memory already exists here; the plan must extend it, not add a second rule. | HIGH |
| WS20 Room-scoped agent browser | model-gateway `src/browser_run.rs` (ZDR browser runs refused as a typed feature); Quarry changes (sibling a) | Touches the browser grant path (sibling b). | MEDIUM |
| WS05 Security and enterprise identity | `apps/Control Plane/auth-core` fixture manifest (pinned; matches Quarry copy); integration-corev2 `internal/auth/middleware.go` (bearer parsing refactor, -37 lines) | Shared bearer and token contract. | LOW-MEDIUM |
| Migration numbering (all WS) | session-core migrations | Every planned session-core schema change starts at 0046 (thread receipt, WS02 outbox, WS01 revisions). Assign numbers explicitly to avoid two 0046 files. | MEDIUM |

## 7. Unfinished or suspicious entries

- `apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/judgments.v1.jsonl` is 0 bytes (U). The judged set for the QB-T14 gate is empty, so the gate and any calibration fit cannot run from it.
- `apps/Model Plane/rust/services/model-gateway/calibration/quarry-cross-encoder-v1.json` is in state "unfitted" (`knots` null). Its note says boot refuses `VEREVON_RELEVANCE_CROSS_ENCODER_SCORES` until a fitted table is committed. Expected; the plan must not assume cross-encoder scores.
- `apps/Model Plane/docs/ZDR.md` (M): the status table (lines 393 to 407) lists Z-00, Z-01, Z-05a, Z-11, Z-12/13, Z-14 and Z-15 to Z-18 as "Not built", while the uncommitted tree implements parts of each. Re-verify before quoting the table in the plan.
- session-core DB tests are `#[ignore]` (`grpc.rs`, `memory_grpc.rs`, `orchestration_grpc.rs`, `retention_receipt.rs`) and need a scratch Postgres. The CI job added to `model-plane-rust-ci.yml` runs only the receipt tests. Not verified here.
- Live-only or DB-only `#[ignore]` tests: retrieval-engine `tests/rerank_contract.rs` (one live test, paid endpoint), `tests/service_scope.rs` (DB tests); model-gateway `tests/source_review_live.rs` (live).
- `apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs` (536 lines) is compiled only under `#[cfg(test)]` (`runtime_loop/mod.rs` line 9). No runtime effect; a review-size note only.
- `apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs` lost 226 lines (moved to `zdr_posture` and `zdr_features`). Its remaining tests cover the `http()` mapping and connection names. Verify that every route it used to guard is still guarded; the matrix tests are untracked and were not run here.
- `apps/Ingestion Plane/docker-compose.yml` (M): behaviour change. The committed `QUARRY_EDGE__AUTOPROMPT` (`${QUARRY_EDGE_AUTOPROMPT:-1}`, line 130) and `QUARRY_EDGE__SEMANTIC_RERANK` (`:-1}`, line 135) default to 1, under the committed comment "Both default ON now" (lines 121 to 123). The uncommitted file defaults both to 0 (AUTOPROMPT at line 129). Six Model Plane switches are new entries at 0, and RERANK_SHADOW is a new entry at 1. The QB-T17 test asserts the Model Plane switches are OFF. Decide this on purpose: it turns the Quarry search-quality and semantic-rerank layers off by default in the stack.
- model-gateway `src/http_routes.rs`: `decide_approval`, `resume_run` and `cancel_run` lost the blanket `require_non_zdr_durable_mutation` call (the helper remains at line 823). The continuation gate now decides. Confirm that a ZDR credential is still refused on each route; `orchestration_http_test.rs` covers only the pre-plan refusal.
- `.gitleaks.toml` (M) cites `scripts/eval-corpus-manifest.json` and `scripts/eval-ingest-corpus.py` in its new comment. Both are absent from the live tree (MISSING). The 5 untracked eval-corpus copies (section 3) are therefore not backed by the manifest the comment describes.
- `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/program-2026-10-03/` (11 untracked scripts, C): tooling inside the marketing app's plans folder, not product code.
- Untracked product-demo and scene work (N, 19 verevon-web entries plus `src/demo/` 82 files and five demo scripts) is gated by `VITE_DEMO` and `VITE_SCENE_MODE`. Keep it out of any Space change set.
- `apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` uses `org_placeholder` and `user_placeholder` values. Test data only, not unfinished work.

## 8. Surprises and facts that change the plan

- Summary-object fields: `rows` = 232 Space rows; `modifiedUncommitted`, `uncommittedNew` and `missing` are the row-state counts (161, 71, 0). Whole-tree totals: 197 M, 122 U entries (314 files), 5 D (all verevon-web product-demo files, not Space).
- session-core `grpc.rs` INSERT (`space_id, space_decision_ref, recipient_audience_ref, ...`) appears as one removed and one added line at hunk `@@ -2989 +3114`. It is a re-wrap of an existing column list: the committed file already has 142 `space_id` references, so no new Space columns are added.
- `authorize_space_deletion_service` is an existing (committed) import. The uncommitted change only adds `authorize_scheduled_step_service` to that import (`grpc.rs`, hunk `@@ -34,7 +36,6`).
- The approval continuation under ZDR was not removed from execution-core `agent.rs`; it was retyped to a catalogue refusal (`approval_continuation`).
- `workspace_promote.rs` now refuses durable mutation under ZDR with the typed `DURABLE_MUTATION_FORBIDDEN` instead of a free-text error.
- The "workspace" name collides with the Space concept. The uncommitted dashboard renames the feed key `aquatiq` to `workspace` (SharePoint pages) in `homeUpdatesFeedSchema`. UNVERIFIED: whether a saved `aquatiq` layout falls back cleanly. The plan should say "Space" for rooms.
- The Control `auth-core` and Quarry `quarry-runtime/contracts` copies of `cross-plane-token-requests.json` are byte-identical, and both `.sha256` pins match. Quarry's rerank client is now a listed user (QB-T09).
- `zdr-refusals.json` (Model Plane) and its client copy share one hash (9c2b5458...) and the pin matches. The posture-matrix copies in mp-contracts and model-gateway are identical (a6ff6879...), and both pins match.
- Git: no staged entries; no line-ending-only modifications (`--ignore-cr-at-eol` leaves no zero-change file); the CRLF warnings are normalisation notices only.
- sqlx: `sqlx::migrate!("./migrations")` (38 committed call sites in session-core) reads the top level only, so `revert/` is not applied.
- Nothing in this tree touches the Space routes in the gateway (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs`), Convex (`apps/Application Plane/convex-core`), user-core, org-core, audit-core, capability-core or sandbox-manager. Those are untouched and remain COMMITTED as far as this snapshot shows.
