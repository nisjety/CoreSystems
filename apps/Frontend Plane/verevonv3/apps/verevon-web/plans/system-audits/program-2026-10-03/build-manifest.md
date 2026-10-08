# Build manifest: contextual retrieval (CR), Quarry option B (QB), zero-retention (ZR/ZDR)

Generated 2026-10-03 for C:/dev/CoresSystem. Machine-readable twin: build-manifest.json (same content, full per-task detail).

> This manifest was produced read-only. No repo file was edited, nothing was committed, built, restarted or called.

## 0. How to read this

- **ID convention.** Design task ids are prefixed by design: CR-T<n> = contextual retrieval (T1..T18), QB-T<nn> = Quarry option B (T01..T17), Z-<nn> = zero retention (Z-00..Z-60). Splits add a/b suffixes (CR-T9a, QB-T05a, Z-05a, ...). Tasks added by this manifest use QB-M1a/M1b/N1/N2/M, CR-N1, X-TRUST, X-DOCS-FINAL (QB-M1 and QB-M10 keep the ids the quarry reviewer used in the surviving text).
- **Review coverage.** The review JSON handed to this step was cut off inside contextual T9, quarry T09 and zdr Z-12. Tasks before the cut carry the reviewers' corrections and notes (corrections folded in); tasks after the cut were reviewed here against the raw designs, the three critiques, the owner defaults and the repository (file existence and git status checked read-only). Every task has a review_source field.
- **Chains** run their tasks strictly in order; chains run in parallel with each other. **Gates** are exact conditions between chains/stages (start = needed to begin, done = needed to declare the task done, deploy = needed before deploy/enable/live proof).
- **Control Plane wave** is built by another workflow and is not re-planned here (section 3).

## 1. Totals

| Item | Count |
|---|---|
| Design tasks reviewed (contextual 18 + Quarry 17 + ZDR 31) | 66 |
| Covered by the Control Plane wave (CR-T1, CR-T2, QB-T01, Z-02) | 4 |
| Merged into other tasks (QB-T07, QB-T15, QB-T16, Z-07) | 4 |
| Deferred design tasks (QB-T06, CR-T18, Z-50, Z-51) | 4 |
| Split parents replaced by children (CR-T9, QB-T05, QB-T13, Z-05, Z-21) | 5 |
| Design tasks kept as they are (in chains) | 49 |
| Split children in chains | 10 |
| Extracted child (QB-T09a) | 1 |
| Tasks added by this manifest (QB-M1a, QB-M1b, QB-N1, QB-N2, QB-M, CR-N1, X-TRUST, X-DOCS-FINAL) | 8 |
| **Task entries in chains** | **68** |
| Extra deferred follow-ups (not design tasks) | 7 |
| Chains / gates / deploy stages / live proofs | 13 / 40 / 15 / 14 |
| Operator actions / really-open owner decisions | 8 / 8 |
| Size mix (S=1, M=2, L=3 points) | M:30, L:18, S:20 = 134 points |
| By plane | model:15, data:22, quality:1, docs:5, control:3, ingestion:11, frontend:10, infra:1 |

## 2. Chains (sequential inside, parallel across)

| Chain | Name | Tasks in order |
|---|---|---|
| C1 | dp-cr | CR-T6 -> CR-N1 -> CR-T5 -> CR-T7 -> CR-T8 -> QB-M1b -> CR-T11 -> CR-T10 -> CR-T9a -> CR-T13 -> CR-T9b |
| C2 | dp-graph | CR-T12 |
| C3 | dp-retrieval | QB-T03 -> QB-T04 -> QB-N2 -> QB-M1a -> QB-T05a -> QB-T05b -> QB-T08 -> Z-41 -> QB-M -> CR-T15 |
| C4 | mp-gateway | CR-T3 -> Z-10 -> Z-00 -> Z-01 -> Z-05a -> Z-06 -> Z-11 -> Z-15 -> Z-16 -> QB-T12 |
| C5 | mp-session-exec | Z-17 -> Z-18 -> Z-21a |
| C6 | mp-cost-core | CR-T4 |
| C7 | cp-contracts | CR-T17 -> Z-14 -> Z-32 |
| C8 | ing-integration | Z-12 -> Z-13 |
| C9 | ing-quarry | Z-08 -> QB-T02 -> QB-T09a -> QB-T10 -> QB-N1 -> QB-T09 -> QB-T17 -> QB-T11 -> QB-T14 |
| C10 | fe | Z-04 -> Z-03 -> Z-09 -> Z-05b -> Z-19 -> Z-20 -> Z-21b -> Z-31 -> Z-42 -> Z-60 -> X-TRUST |
| C11 | quality-ctx | CR-T14 |
| C12 | docs | QB-T13a -> CR-T16 -> Z-30 -> QB-T13b -> X-DOCS-FINAL |
| C13 | ops | Z-40 |

### C1 dp-cr: Data Plane contextual retrieval core

- Scope: embedding-engine-rs, inference-client-rs (new), usage-ledger-rs (new), event-envelope-rs, quickwit-adapter-rs (parity endpoint), data-orchestrator-go, data-quality-go, Data Plane migrations/init.sql/compose/.env.example/Makefile/Cargo workspace
- Why this order: Owner ruling 2 (CR works perfectly) first: migration, CI wiring, shared client, contextualizer v2 + batch pipeline (one change set), the source payload stamp for Quarry, policy, metering foundation (consumer first), reconciler, defaults flip last, budget gate (alert-only).
- Runs in parallel with: C2 after its gate, C3, C4, C5, C6, C7, C8, C9, C11 after gate

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T6 Postgres migration: CR provenance and status, org_processing_policy, coverage view, legacy backfill | M | - | - | - | - | G-BACKUP |
| 2 | CR-N1 CI wiring: real-migrator Postgres contract tests that fail instead of silently skipping | S | CR-T6 | - | - | - | - |
| 3 | CR-T5 Data Plane shared crate inference-client-rs (token client, tenant posture, bearer cache, typed statuses) | M | - | - | G-T3-CODE | - | G-CP-P0B |
| 4 | CR-T7 Contextualizer v2: outcomes, ctx-v2 prompt, windowing, validation, usage capture, floors | L | CR-T5, CR-T6 | - | - | - | G-CP-DEPLOYED, G-T3-DEPLOYED, G-OPS-ORG |
| 5 | CR-T8 Batch pipeline: write-ahead context, vector coherence, reuse, inline deadline, cross-document concurrency | L | CR-T7 | - | - | - | G-T6-APPLIED |
| 6 | QB-M1b Embedding-engine: stamp documents.source into the Qdrant payload and backfill existing points (M1b) | M | CR-T8 | - | - | - | G-T8-GREEN |
| 7 | CR-T11 Org processing policy admin API, coverage endpoint and release gates | M | CR-T6, CR-T8 | - | - | - | - |
| 8 | CR-T10 Usage ledger v2: shared event crate, durable stream, cost_events columns, forwarder to cost-core, ZDR numeric-only, graph signer | L | CR-T8, CR-T6 | - | - | - | G-CP-P1 |
| 9 | CR-T9a Context reconciler, backfill/repair CLI, legacy verification pass and Postgres-vs-Quickwit parity repair (T9a) | L | CR-T8, CR-T11, CR-T10 | - | - | - | G-T8-GREEN |
| 10 | CR-T13 Defaults and config: CR ON by default, boot-time contract, .env.example, compose contract, dedicated deployment alias, stale-comment fixes | S | CR-T7, CR-T9a | - | - | - | G-T8-GREEN |
| 11 | CR-T9b Reconciler budget gate: alert-only by default, optional stop mode (T9b) | M | CR-T9a, CR-T10, CR-T11 | - | - | - | - |

### C2 dp-graph: Data Plane graph-index hardening

- Scope: graph-index-rs (state machine, backoff, poison cap, metering emission, removal of the azure_openai extractor)
- Why this order: Separate chain only because graph-index is free of embedding-engine edits once CR-T5 (client conversion) and CR-T10 (signer) are done; gated on those.
- Runs in parallel with: everything else after its start gates

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T12 graph-index hardening: durable per-chunk state, backoff and poison cap, typed errors, metering, tenant posture, version stamps | L | CR-T5, CR-T6, CR-T10 | CR-T5, CR-T6, CR-T10 | G-T5-DONE, G-T10-CRATE | - | G-CP-P1, G-T10-CONSUMER-DEPLOYED |

### C3 dp-retrieval: Data Plane retrieval-engine: Quarry rerank (option B), then ZDR egress guard, then CR rerank input

- Scope: retrieval-engine-rs, its compose block, openapi/d4d5.yaml, auth-regression-matrix scripts
- Why this order: Plan: QB first (auth, route table, source scope, rerank_api, redaction), then Z-41 (shared egress attestation, same files), then the CR rerank-input change which is measured and flag-gated.
- Runs in parallel with: all other chains

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | QB-T03 Retrieval engine: principal-aware auth with a default-deny service route allowlist (fixes the 403) | M | - | - | - | - | - |
| 2 | QB-T04 Retrieve contract for non-user callers: SourceRef.url, wire golden, strict zdr_mode | S | - | - | - | - | - |
| 3 | QB-N2 Retrieval engine: redact the query in the retrieval.start log when the mode is restrictive (N2) | S | QB-T04 | - | - | - | - |
| 4 | QB-M1a Retrieval engine: server-enforced Quarry corpus scope for service principals (M1a) | M | QB-T03, QB-T04, QB-N2 | - | - | - | G-M1B-BACKFILL |
| 5 | QB-T05a Data Plane rerank capability: POST /v1/rerank core, policy, limits, idempotency, metrics, interim v1 cost event (T05a) | L | QB-T03, QB-M1a | - | - | - | G-CP-P1, G-PROCESSOR-APPROVED-PROOF |
| 6 | QB-T05b Shared rerank provider permit pool, token bucket and circuit breaker (T05b) | M | QB-T05a | - | - | - | G-EVAL-NOT-DROPPED |
| 7 | QB-T08 Config, compose, OpenAPI, runbook, auth matrix rows and dark ZDR attestation | S | QB-T03, QB-T05a | - | - | - | - |
| 8 | Z-41 ZDR-capable query embedding path or explicit lexical degrade | L | QB-T05a | - | - | - | G-ATTEST |
| 9 | QB-M Switch the web-rerank cost producer to the v2 ledger builder: feature web-rerank, units = billed search units (QB-M) | S | QB-T05a, CR-T10 | CR-T10 | G-T10-CRATE | - | G-T4-DEPLOYED-PRICED, G-T10-CONSUMER-DEPLOYED |
| 10 | CR-T15 Retrieval: rerank input with chunk context (measured, flag-gated) | M | CR-T14, QB-T05a, QB-T05b | CR-T14 | - | - | G-T14-CTX-CELLS |

### C4 mp-gateway: Model Plane: inference-core, model-gateway, mp-contracts, ZDR enforcers and Quarry calibration

- Scope: inference-core, model-gateway, mp-contracts, api/contracts goldens (execution-core only for Z-05a hunks)
- Why this order: Typed statuses first (they ship alone and unblock CR); then the enforcer stop-gaps (Z-00, Z-01, Z-05a), the resolver (Z-11) and the waiver enforcers. Enforcers ship before any minter. QB-T12 is last because tool_loop.rs is shared with Z-15 and is dirty.
- Runs in parallel with: C5 after G-MP-CONTRACTS; every non-Model chain

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T3 inference-core: typed ZDR/residency/tier statuses, per-service counters, shared wire fixtures | M | - | - | - | - | - |
| 2 | Z-10 Shared golden fixtures: posture matrix and connection capability contract | S | CR-T3 | - | - | - | - |
| 3 | Z-00 Stop the silent subscription re-route under token ZDR | S | - | - | - | - | - |
| 4 | Z-01 Strict zdr claim and equal posture across delegated bearers | S | - | - | - | - | - |
| 5 | Z-05a Typed ZDR refusal catalogue and refusal sites in the Model Plane, including the silent-drop closure (Z-05a, absorbs Z-07) | L | CR-T3, Z-10 | - | - | - | - |
| 6 | Z-06 ZDR stream provenance and usage frame in the Model Plane (metering half deferred as Z-06b; SPA hunks moved to Z-20) | M | Z-05a | - | - | - | - |
| 7 | Z-11 mp-contracts retention resolver | M | Z-10, Z-05a | - | - | - | - |
| 8 | Z-15 model-gateway waiver enforcement and stamping | L | Z-00, Z-01, Z-05a, Z-06, Z-11 | - | - | - | - |
| 9 | Z-16 inference-core route-binding backstop | M | Z-01, Z-11 | - | - | - | - |
| 10 | QB-T12 Model Plane consumers: score_kind, rerank status, calibration in relevance.rs | M | QB-T09, QB-T14, Z-15 | QB-T09, QB-T14 | G-T14Q-CALIBRATION, G-T09-DEPLOYED | - | - |

### C5 mp-session-exec: Model Plane: session-core and execution-core waiver work

- Scope: session-core (receipt, derived-job exclusions, migration 0043, aggregate RPC), execution-core (waiver-aware Do and children)
- Why this order: Starts when the mp-contracts API is stable (Z-05a + Z-11 merged); runs in parallel with Z-06/Z-15/Z-16 because the crates are disjoint.
- Runs in parallel with: C4 (after gate), C1..C3

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | Z-17 session-core retention receipt and derived-job exclusions | L | Z-11 | Z-11 | G-MP-CONTRACTS | - | - |
| 2 | Z-18 execution-core waiver-aware Do and children | M | Z-11, Z-17, Z-05a | Z-11, Z-05a | G-MP-CONTRACTS | - | - |
| 3 | Z-21a session-core read-only aggregate RPC of waived runs per connection and user (Z-21a) | S | Z-17 | - | - | - | - |

### C6 mp-cost-core: Model Plane: cost-core ingest v2, budget exclusion and pricing (Go)

- Scope: cost-core (ledger, postgres, server, cmd, pricing, migrations 0002 seed and 0008)
- Why this order: Blocked on the other session's uncommitted cost-core work; a single-task chain so a stall here blocks nothing else except the forwarder enablement.
- Runs in parallel with: everything

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T4 cost-core: feature and units on the ledger, org-level service rows, catalogue rows, feature aggregation | M | - | - | G-CORECOST-OTHER-SESSION | G-T10-GOLDEN | G-CP-P1 |

### C7 cp-contracts: Control Plane (ours, after the CP wave): cross-plane manifest spec, ZDR decision endpoint, launcher wiring

- Scope: auth-core new spec/fixtures/controllers, config/plane-calls.manifest.json, launcher/compose flag wiring
- Why this order: Everything here edits files the CP wave also owns; it starts only after the CP wave has merged its P0/P1 code (gates), by hunk, never extending the untracked other-session spec.
- Runs in parallel with: all non-Control chains

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T17 Cross-plane dependency manifest, real-policy contract spec and live smoke matrix | M | CR-T3 | CR-T3 | G-CP-P1-CODE | - | G-CP-DEPLOYED |
| 2 | Z-14 Auth Core decision endpoint, connection-bound waiver claim and kill switch (principal rows are CP wave P1) | L | Z-10, Z-13, CR-T17 | Z-10, Z-13 | G-CP-P0A-CODE, G-CP-P1-CODE | - | G-CP-DEPLOYED |
| 3 | Z-32 Launcher, compose and flag wiring | S | Z-14 | - | G-CP-P1-CODE | - | - |

### C8 ing-integration: Ingestion: integration-corev2 ZDR capability and acknowledgement ledger

- Scope: integration-corev2 (migration 0013, store, assessor, ack API, texts)
- Why this order: Consent first, per the plan (integration-corev2, then Quarry); independent crate.
- Runs in parallel with: C9 and everything else

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | Z-12 integration-corev2 schema, capability assessor and backfill | L | Z-10 | Z-10 | G-GOLDEN-Z10 | - | - |
| 2 | Z-13 Acknowledgement API with audit, revocation and admin list | L | Z-12 | - | - | - | - |

### C9 ing-quarry: Ingestion: Quarry-v2 (Docker-only builds)

- Scope: Quarry-v2 crates quarry-runtime, quarry-edge, lab/evals, docs; Ingestion compose quarry-edge block
- Why this order: Quarry is one Cargo workspace built only in Docker, so it is one chain. Z-08 owns the Claims.zdr/effective_zdr helper so QB-T09 only consumes it; T09a plumbing, then the vector leg (needs retrieve-v1 goldens), then the rerank client (needs rerank-v1 goldens).
- Runs in parallel with: all other chains after their gates

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | Z-08 Quarry takes the token zdr claim as a floor | M | - | - | - | - | - |
| 2 | QB-T02 Quarry cross-plane token-request manifest and its Auth Core contract | M | - | - | - | - | - |
| 3 | QB-T09a Quarry: SearchProvider::search_with_report plumbing, RerankReport and the cache rule (T09a) | M | QB-T02, Z-08 | - | - | - | - |
| 4 | QB-T10 Quarry vector leg: correct request, ZDR/sovereign skip, scoped corpus, strict mirror, timeouts | M | QB-T02, QB-T09a, QB-T03, QB-T04, QB-M1a | QB-T03, QB-T04, QB-M1a | G-QB-RETRIEVE-CODE | - | G-M1B-BACKFILL, G-QB-A-DEPLOYED |
| 5 | QB-N1 Quarry: rrf_fuse keeps engines and fusion_score; canonical URL dedup (N1) | S | QB-T10 | - | - | - | - |
| 6 | QB-T09 Quarry: Data Plane rerank client replaces the LLM reranker, with explicit status, ZDR and cache rules | L | QB-T09a, QB-T05a, QB-T10, Z-08 | QB-T05a | G-QB-A-CODE | - | G-QB-A-DEPLOYED, G-CP-P1 |
| 7 | QB-T17 Decide and fix Quarry's other Model Plane dependents (autoprompt, answer, ai-formats, page-title) | M | QB-T02, QB-T09 | - | - | - | - |
| 8 | QB-T11 Quarry docs, OpenAPI, SDKs and compose cleanup | S | QB-T09, QB-T17 | - | - | - | - |
| 9 | QB-T14 Judged web-result set, runner, calibration and regression gate | L | QB-T05a, QB-T11 | QB-T05a | - | - | G-QB-A-DEPLOYED, G-OPS-LABEL |

### C10 fe: Frontend: gateway (BFF), SPA, e2e, trust re-verification

- Scope: apps/gateway (Rust BFF), src/ SPA, tests/e2e, verevon-web trust files, conversation-core client (Z-03)
- Why this order: One chain because WorkspaceSettingsPage.tsx, chat-client.ts, privacy-tier.ts, organization-client.ts and gateway zdr.rs are shared; early tasks (Z-04, Z-03, Z-09, Z-05b) have no waiver dependency and run while upstream work lands. Z-60 (live proof) then X-TRUST (re-verify claims) close the chain.
- Runs in parallel with: all

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | Z-04 Gate the org ZDR switch on an attested provider | M | - | - | - | - | - |
| 2 | Z-03 One org retention-posture read model: migrate all readers outside the Control Plane (BFF, conversation-core, SPA) | M | Z-10 | Z-10 | G-CP-P0A-CODE | - | G-CP-P0A, G-OD-LAPSE |
| 3 | Z-09 Fix the browser retention purge (F8) | S | Z-03 | - | - | - | - |
| 4 | Z-05b SPA: localized rendering of every ZDR refusal code with actions (Z-05b) | S | Z-05a | Z-05a | - | - | - |
| 5 | Z-19 BFF turn retention resolution and connection routes | L | Z-14 | Z-14 | G-Z14-CODE | - | G-Z14-DEPLOYED |
| 6 | Z-20 Frontend prompts, composer chip, answer line and admin record | L | Z-19, Z-06, Z-05b | Z-06 | - | - | - |
| 7 | Z-21b BFF and SPA: org-admin counts of waived runs beside the acknowledgement list (Z-21b) | S | Z-21a, Z-19, Z-20 | Z-21a | - | - | - |
| 8 | Z-31 e2e helper and new ZDR specs | M | Z-19, Z-20 | - | - | - | - |
| 9 | Z-42 Interactive-only ZDR scope wording and /trust text | S | Z-30, Z-21b | Z-30 | - | - | - |
| 10 | Z-60 Live proof on Terra with a ZDR-on test org | M | Z-15, Z-16, Z-17, Z-18, Z-19, Z-20, Z-21b, Z-31, Z-32, Z-14, Z-13, Z-12 | Z-15, Z-16, Z-17, Z-18, Z-32, Z-14, Z-13, Z-12 | G-ENFORCERS-BEFORE-MINTERS, G-OPS-ORG-ZDR, G-OWNER-ZDR-PROOF | - | - |
| 11 | X-TRUST Trust claims re-verification after the live proofs (trust-edits part b) | M | Z-42, Z-60 | - | G-PROOFS-GREEN | - | - |

### C11 quality-ctx: Quality harness for contextual retrieval

- Scope: Data Plane scripts/eval-*, golden sets, quality_eval_runs store (data-quality-go internal/eval), Makefile hunk
- Why this order: Starts after CR-T11 (same Go module) and the CR core proof; needs owner approval OD8 for the paid re-seed.
- Runs in parallel with: everything

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | CR-T14 Quality harness: committed corpus ingest, A/B cells, coverage gate, persisted results, ctx-v2 promotion gate | L | CR-T8, CR-T9a, CR-T11 | CR-T8, CR-T9a, CR-T11 | G-T11-DONE | - | G-T8-GREEN, G-OWNER-OD8, G-OPS-ORG |

### C12 docs: Docs: ownership matrix rules 15 to 18, ledger, compliance evidence

- Scope: master-ownership-matrix.md, CODEBASE_INFORMATION_SYSTEM.md, GDPR_SUMMARY.md, ledger.md, ZDR.md, honest-limits and the other evidence docs, runbooks
- Why this order: Rule numbers are written in the order the plan states (15 QB, 16 CR, 17 ZR, 18 QB); drafted early as proposed, finalised after the proofs by X-DOCS-FINAL. One docs commit per wave (the orchestrator commits, never agents).
- Runs in parallel with: all code chains

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | QB-T13a Ownership documents and decision ledger: rows 70-71, rule 15, GDPR note, ledger entry (T13a) | S | - | - | - | - | - |
| 2 | CR-T16 Ownership documents, decision ledger, compliance evidence and runbook | M | - | - | - | - | - |
| 3 | Z-30 Ownership, ledger, GDPR and compliance document updates | M | CR-T16 | - | - | - | - |
| 4 | QB-T13b Ownership documents: rule 18 (service-principal route allowlist, exception to rule 9) (T13b) | S | QB-T13a, CR-T16, Z-30 | - | - | - | - |
| 5 | X-DOCS-FINAL Docs finalisation after the live proofs: ledger state implemented, measured numbers, evidence re-verification | S | QB-T13b, CR-T16, Z-30 | - | G-PROOFS-GREEN | - | - |

### C13 ops: Operator/commercial action: ZDR provider attestation

- Scope: release path variables only (AZURE_OPENAI_ZDR_*); no code
- Why this order: Owner default D-A: attestation is data and an operator step; modelled as a chain so its gate (G-ATTEST) is explicit.
- Runs in parallel with: everything

| # | Task | Size | Depends on | Cross-chain waits | Start gates | Done gates | Deploy gates |
|---|---|---|---|---|---|---|---|
| 1 | Z-40 Attest a ZDR provider route (operator action) | M | - | - | - | - | - |

## 3. Control Plane wave boundary

A Control Plane wave (Auth Core org-core read path, tenant retention posture, registry unification, the principal bundle) is ALREADY being built by another workflow. Its tasks are not re-planned here.

- **CR-T1**: Auth Core tenant-aware retention posture (fail-closed) = P0a/P0b, including the org-core read credential/route (critic BLOCKER #1) and the 503 retention_posture_unavailable semantics.
- **CR-T2**: data-orchestrator to cost-core cost:write principal, graph-index event signer principal row, launcher mint and credential validation = P1. The Data Plane-side residual (compose env, config.go, graph-events key mount) is moved into CR-T10.
- **QB-T01**: quarry-edge data:rerank grant and who may hold it = P1 (plus the tenancy model and bootstrap registry unification P0c).
- **Z-02**: Auth Core retention resolver stale-if-error / no failure caching = P0a.
- **Z-03 (principal part)**: org-core retention-posture read route and Auth Core resolver; the remaining readers outside Control stay as task Z-03.
- **Z-14 (principal part)**: the separate BFF retention-decision principal, audience control-policy, scope retention:zdr-decide and launcher env wiring are P1; the decision controller/claim/kill switch stay as task Z-14.
- **Z-32 (registry part)**: reconciling scripts/bootstrap_runtime_environment.sh with the committed registry is P0c.

Assumptions to verify at the gates:
- P1 includes a production-valid tenancy model (no allowAnyOrg in production file mode) for quarry-edge, data-orchestrator, embedding-engine, graph-index and the BFF decision principal.
- The org-core retention read route returns {zdrIntent, entitled} (or the effective flag) and its shape is final before Z-03/Z-14 start.
- Tenant-principal tokens are capped at 60 s TTL and a 503 retention_posture_unavailable is returned on resolution failure.
- The Data Plane-side residuals of CR-T2 are NOT covered by the wave (they are in CR-T10); confirm with the wave owner.

## 4. Gates between chains and stages

| Gate | Kind | Exact condition | How verified | Blocks |
|---|---|---|---|---|
| G-CP-P0A-CODE | external-code | The org-core internal retention read route ({zdrIntent, entitled}) and the Auth Core resolver/503 semantics are merged in the working tree and their route shape is readable. | git diff of apps/Control Plane/org-core and auth-core shows the route; ask the CP wave owner for the final shape. | start: Z-03, Z-14 |
| G-CP-P0A | external-deploy | auth-service reaches org-core with service headers and gets 200 for the live orgs; with org-core down a tenant mint returns 503 reason retention_posture_unavailable and issues no token. | Free probe from inside the auth-service container: status codes only. | deploy: Z-03 |
| G-CP-P0B | external-deploy | The embedding-engine/graph-index principals mint tenant tokens: zdr:true for a ZDR test org, zdr:false for a non-ZDR org; token TTL is capped at 60 s; the old validator would reject a tenant registry, so the registry flip happened strictly after this image. | Mint through the service container's own env; print only status and the decoded zdr claim. | deploy: CR-T5 |
| G-CP-P1-CODE | external-code | config/plane-service-principals.json carries: quarry-edge data:rerank on data-plane; data-orchestrator to cost-core cost:write; a SEPARATE BFF retention-decision principal (audience control-policy, retention:zdr-decide); tenant retention for embedding-engine and graph-index; the production-valid tenancy model. | Read the committed JSON; the vendored slices in consuming trees match by sha256. | start: CR-T17, Z-14, Z-32 |
| G-CP-P1 | external-deploy | The registry bundle of G-CP-P1-CODE is live in auth-service (launcher re-assembled PLANE_SERVICE_PRINCIPALS_JSON, auth-service recreated): quarry-edge mints data:rerank, data-orchestrator mints cost-core cost:write, tenant retention rows for embedding-engine/graph-index are active only AFTER the tenant-capable auth-core image (G-CP-P0B). | Mint status for each principal/audience/scope; scopes only, no tokens printed. | deploy: CR-T4, CR-T10, CR-T12, QB-T05a, QB-T09 |
| G-CP-DEPLOYED | external-deploy | P0a, P0b, P0c and P1 are all deployed: (1) auth-service reaches org-core (200); (2) tenant mint works and a ZDR test org mints zdr:true; (3) registry parity spec (committed JSON = launcher assembly = bootstrap) is green; (4) quarry-edge mints data:rerank, data-orchestrator mints cost-core cost:write, the BFF decision principal exists; (5) no allowAnyOrg remains in production file mode; (6) interactive tokens cannot reach /v1/rerank. | LP-0 and LP-1 free probes. | deploy: CR-T7, CR-T17, Z-14 |
| G-T3-CODE | task-code | apps/Model Plane/api/contracts/infer-statuses.json exists, generated from the code under test, sha256 recorded. | Sync test green. | start: CR-T5 |
| G-T3-DEPLOYED | task-deploy | GET :8082/metrics returns the Prometheus counters (not the 37-byte stub) and a refused ZDR Infer carries x-mp-provider-error=zdr_unavailable. | LP-MODEL-A. | deploy: CR-T7 |
| G-CORECOST-OTHER-SESSION | external-code | cost-core migrations 0006/0007, internal/azurecost and the provider_used attribution are committed; the live cost_entries table has provider_used; git status shows ledger.go, store.go, server.go, main.go and tests clean. | git status on apps/Model Plane/go/services/cost-core; psql column check. | start: CR-T4 |
| G-T10-GOLDEN | task-code | data-orchestrator-go/testdata holds the exact JSON body the forwarder posts to /api/v1/cost/record, with sha256; CR-T4 vendors it. | sha256 equal in both trees. | done: CR-T4 |
| G-T10-CRATE | task-code | usage-ledger-rs compiles, event-envelope-rs has the cost subject and graph scope, SUBJECTS.md and check-subjects.sh pass. | cargo check --workspace; scripts/check-subjects.sh. | start: CR-T12, QB-M |
| G-T10-CONSUMER-DEPLOYED | task-deploy | data-orchestrator durable consumer survives a restart without losing an event (publish while down, restart, exactly one row); producers still have USAGE_EMIT_ENABLED=0. | Kill/restart drill on the test org. | deploy: CR-T12, QB-M |
| G-T4-DEPLOYED-PRICED | task-deploy | cost-core accepts feature and units (unknown field no longer 400) AND sourced pricing rows for embed-v-4-0, text-embedding-3-large and Cohere-rerank-v4.0-pro are deployed (OPS-PRICE); otherwise the 3.00/15.00 default would overprice embeddings about 25x. | Zero-token record round trip; /api/v1/pricing lists the rows. | deploy: QB-M |
| G-T5-DONE | task-code | inference-client-rs is a workspace member, embedding-engine and graph-index use it, both inference_auth.rs copies are deleted. | cargo clippy --workspace --all-targets -- -D warnings. | start: CR-T12 |
| G-BACKUP | operator | scripts/backup.sh run before the migration is applied live. | Backup artifact present. | deploy: CR-T6 |
| G-T6-APPLIED | task-deploy | Migration applied; counts: 206 legacy, 1 pending, 0 incoherent; the idempotent backfill is re-run at the CR-T8 deploy. | LP-SQL. | deploy: CR-T8 |
| G-T8-GREEN | proof | On the throwaway org: every eligible unit ends with a non-NULL context_status; vector input equals compose(persisted context, text); a forced re-embed with unchanged inputs makes 0 LLM calls; payload unchanged until QB-M1b; only then the reconciler is enabled and the default flipped. | LP-CR. | deploy: CR-T9a, CR-T13, CR-T14, QB-M1b |
| G-T11-DONE | task-code | policy API, coverage view endpoint and gates exist (data-quality-go module free for CR-T14). | Go tests green. | start: CR-T14 |
| G-OPS-ORG | operator | A real org-core organisation on the pro plan (CR proof org; QB test org with an ingested Quarry page) exists; the tenant posture recognises it (unknown orgs are refused). Needs owner approval for the writes. | Mint for the org succeeds; org visible in org-core. | deploy: CR-T7, CR-T14 |
| G-OPS-ORG-ZDR | operator | A dedicated pro org with two members; ZDR intent switched on for it ONLY through the operator seed path (direct org-core PATCH with an operator credential); member A has completed the ChatGPT device sign-in on the Terra route. | Z0/Z1 free probes mint zdr:true for the org. | start: Z-60 |
| G-OWNER-OD8 | owner-approval | Owner OK in chat for about 1,164 chunk calls (about 0.15 USD gpt-4o-mini) plus Cohere embed cost and the nightly eval. | Chat record. | deploy: CR-T14 |
| G-OWNER-ZDR-PROOF | owner-approval | Owner OK for the writes (test org, ZDR on that org, ZDR_WAIVER_ENABLED window) and for the owner-interactive Terra device sign-in. | Chat record. | start: Z-60 |
| G-OD-LAPSE | owner-approval | Owner confirms: an org whose plan lapses keeps ZDR (intent=true) until an admin turns it off (default applied) or chooses intent AND entitlement. | Chat record; the Z-10 matrix row matches. | deploy: Z-03 |
| G-OPS-LABEL | operator | LLM pre-judge on the Terra subscription plus human adjudication of all disagreements and a 20% sample; Cohen kappa >= 0.6; at least 2 judges per query. | Fixture-integrity test. | deploy: QB-T14 |
| G-M1B-BACKFILL | task-deploy | QB-M1b deployed; counts by source in Qdrant equal Postgres counts by documents.source for all existing points (207 live). Until then the Quarry vector leg stays disabled (QUARRY_EDGE__VECTOR_RETRIEVAL=0). | Backfill CLI dry run then real run; count compare. | deploy: QB-M1a, QB-T10 |
| G-QB-RETRIEVE-CODE | task-code | QB-T03, QB-T04 and QB-M1a are merged and tests/fixtures/contracts/retrieve-v1 goldens (response and request) exist with a sha256 manifest. | cargo test; sha256 manifest. | start: QB-T10 |
| G-QB-A-CODE | task-code | QB-T03, QB-T04, QB-M1a and QB-T05a are merged; tests/fixtures/contracts/retrieve-v1 and rerank-v1 goldens exist with sha256 manifests. | cargo test; sha256 manifests. | start: QB-T09 |
| G-QB-A-DEPLOYED | task-deploy | retrieval-engine image deployed with WEB_RERANK_ENABLED=0 and WEB_RERANK_PROCESSOR_APPROVED=false; auth-regression-matrix rows: no token 401, user token on /v1/rerank 403, service token without scope 403, service token on admin routes 403, quarry-edge token on /v1/retrieve 200 (zdr_mode ephemeral). | LP-AUTH-MATRIX; qb-preflight.sh. | deploy: QB-T09, QB-T10, QB-T14 |
| G-PROCESSOR-APPROVED-PROOF | proof-env | The orchestrator sets WEB_RERANK_ENABLED=1 and WEB_RERANK_PROCESSOR_APPROVED=true in the LIVE-PROOF environment only (owner ruling 3: paid reranking is fine). The code and compose defaults stay false; the production default waits for OD-LEGAL. | Env in the recreated container. | deploy: QB-T05a |
| G-EVAL-NOT-DROPPED | proof | scripts/eval-gate.py exits 0 against the committed baseline before and after QB-T05b. | eval-gate.py. | deploy: QB-T05b |
| G-T14Q-CALIBRATION | task-code | QB-T14 produced results and the score-to-relevance knots (monotone, <=8 knots) from the live DP /v1/rerank run. | Committed baseline file. | start: QB-T12 |
| G-T09-DEPLOYED | task-deploy | quarry-edge image with QB-T09 is live, shadow mode run, no Model Plane 403s in its logs. | LP-QUARRY. | start: QB-T12 |
| G-T14-CTX-CELLS | proof | CR-T14 cells show a gain beyond the rerank noise floor with the reranker on (three runs each; 429 noise); otherwise RERANK_INPUT_INCLUDE_CONTEXT stays off. | quality_eval_runs. | deploy: CR-T15 |
| G-MP-CONTRACTS | task-code | Z-05a (zdr_refusal + all refusal sites incl. execution-core hunks) and Z-11 (retention resolver, WAIVER_CONTRACT_VERSION, waived-retrieval helper) are merged. | cargo check -p mp-contracts and dependants. | start: Z-17, Z-18 |
| G-GOLDEN-Z10 | task-code | api/contracts/zdr-posture-matrix.json and connection-zdr-capabilities.json exist with sha256 pins; the sync script passes. | scripts/tests/zdr-contract-sync.test.sh. | start: Z-12 |
| G-Z14-CODE | task-code | Decision endpoint and waiver claim code merged (flag default off). | auth-core jest. | start: Z-19 |
| G-Z14-DEPLOYED | task-deploy | The decision route answers waiver_disabled; no waiver token can be minted. | Free probe. | deploy: Z-19 |
| G-ENFORCERS-BEFORE-MINTERS | ordering | Z-00, Z-01, Z-05a, Z-15, Z-16, Z-17, Z-18 are deployed and model-gateway, inference-core, session-core and execution-core each report WAIVER_CONTRACT_VERSION; the Z-32 launcher check passes. Only then may the Auth Core flag be turned on (S14). Roll back in reverse order. | Launcher check; /metrics gauges. | start: Z-60 |
| G-ATTEST | operator | Z-40 done: /v1/models shows a zdr feature on a covered model; readyz reports attested capability honestly. | Catalogue read. | deploy: Z-41 |
| G-PROOFS-GREEN | proof | Every applicable live proof passed with its exact call counts recorded. | Proof evidence notes. | start: X-TRUST, X-DOCS-FINAL |

## 5. Deploy stages in order (with the launcher stage copy each needs)

Stage ids are logical and ordered by the cross-plane order of the plan (Control, Data, Model, Application, Ingestion, Frontend) with two deliberate exceptions: Model-A (typed statuses and ZDR stop-gaps) ships before the Data live proofs, and Model-C (ZDR enforcers) ships dark before any minter. The orchestrator may split or merge adjacent stages of the same plane (the launcher rebuilds a whole plane each time; --skip-build recreates with new env). Each task's deploy_stage names the earliest stage that can carry it. Roll back in reverse order.

Launcher stack order: 0 Control, 1 Data Plane v2, 2 Model, 3 Application, 4 Ingestion, 5 Frontend, 6 Infra. A stage copy is build-verevon-services.<plane>-only.tmp.sh next to apps/Frontend Plane/verevonv3/build-verevon-services.sh with the LAST_STACK_INDEX= line changed by sed; run with --from <plane> (add --skip-build for recreate-only); detach long builds with nohup and watch the log with Monitor; delete the copy afterwards. Never hand-build compose.

### S0 (control): Control Plane wave (external workflow: P0a org-core read path + Auth Core 503, P0b tenant posture, P0c registry unification, P1 principal bundle)

- Tasks: CR-T1, CR-T2, QB-T01, Z-02
- Launcher: Control-only launcher copy: copy apps/Frontend Plane/verevonv3/build-verevon-services.sh to build-verevon-services.control-only.tmp.sh, sed the LAST_STACK_INDEX= line to 0, run --from control (stack order: 0 Control, 1 Data Plane v2, 2 Model, 3 Application, 4 Ingestion, 5 Frontend, 6 Infra); delete the copy afterwards. Mint any newly required credential first with apps/Control Plane/scripts/run-control-plane.sh config > /dev/null or validation fails before anything is created. Long builds: nohup + Monitor.
- Flags and ordering: auth-core image before any registry change to tenant; never hand-build docker compose.
- Owner chain: CP wave workflow (not this manifest)

### S1 (model): Model-A: typed statuses and ZDR stop-gaps (dark, additive)

- Tasks: CR-T3, Z-00, Z-01, Z-05a, Z-06, Z-11
- Launcher: Model-only copy: build-verevon-services.model-only.tmp.sh with LAST_STACK_INDEX=2, run --from model (about 4 min when cached; --skip-build recreate about 2 min). Check git status for the Model Plane tree first: other sessions' half-finished edits (model-gateway sse.rs +2136, normalize.rs +434, http_routes.rs, execution-core, session-core, cost-core) are baked into the image. On build exit 101 rerun with COMPOSE_PARALLEL_LIMIT=1 to see the real error.
- Flags and ordering: No env change. After any Model Plane proto change, cargo check the Data Plane v2 workspace too (graph-index-rs and embedding-engine-rs build Model Plane proto messages as struct literals).
- Owner chain: C4

### S2 (model): Model-B: cost-core ingest v2 (feature, units, budget exclusion) and sourced pricing rows

- Tasks: CR-T4
- Launcher: Same model-only copy (--from model, LAST_STACK_INDEX=2). Migration 0008 applies at container boot via docker-entrypoint.sh (idempotent).
- Flags and ordering: Must precede enabling the Data Plane forwarder. Do NOT start until the other session's cost-core work is committed and deployed (G-CORECOST-OTHER-SESSION).
- Owner chain: C6

### S3 (data): Data-1: CR core images (all dark) and migrations

- Tasks: CR-T6, CR-T5, CR-T7, CR-T8, CR-T11, CR-T10, CR-T9a, CR-T12
- Launcher: Data-only copy: LAST_STACK_INDEX=1, run --from data. The Data stage hook apply_dataplane_migrations applies the migrations (context_provenance, cost_events_v2, graph_extraction_state); verify_ownership_phase runs after. Consumers/verifier (data-orchestrator, data-quality) are in the same image rebuild as the producers, so producers ship with emission dark.
- Flags and ordering: CONTEXT_RECONCILER_ENABLED=0, CONTEXT_REFRESH_ON_PROMPT_CHANGE=0, USAGE_EMIT_ENABLED=0, USAGE_FORWARDER_ENABLED=0, posture env still persistent, ctx-v2 behind the org policy pin. Take a knowledge_units backup first (G-BACKUP). Re-run the idempotent legacy backfill at this deploy.
- Owner chain: C1 + C2

### S4 (data): Data-1b: enablement by env (recreate with --skip-build), in this order, plus the Qdrant source payload stamp (QB-M1b image and backfill) after the CR core proof

- Tasks: QB-M1b
- Launcher: Data-only copy with --from data --skip-build (about 2 min per recreate).
- Flags and ordering: (1) posture env to tenant for embedding-engine/graph-index ONLY after G-CP-P0B and the registry flip (MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE ~compose 759, MODEL_PLANE_INFERENCE_RETENTION_POSTURE ~984); (2) USAGE_EMIT_ENABLED=1 after G-T10-CONSUMER-DEPLOYED; (3) CONTEXT_RECONCILER_ENABLED=1 with a cap after LP-CR is green; (4) USAGE_FORWARDER_ENABLED=1 only after G-T4-DEPLOYED-PRICED. Run the Qdrant source backfill CLI (QB-M1b) here (G-M1B-BACKFILL).
- Owner chain: orchestrator

### S5 (data): Data-2: retrieval-engine QB-A (route table, retrieve contract, source scope, redaction, /v1/rerank dark)

- Tasks: QB-T03, QB-T04, QB-N2, QB-M1a, QB-T05a, QB-T08
- Launcher: Data-only copy (--from data, LAST_STACK_INDEX=1); one rebuild for all of QB-T03/T04/N2/M1a/T05a (+ T08 compose env recreate).
- Flags and ordering: WEB_RERANK_ENABLED=0, WEB_RERANK_PROCESSOR_APPROVED=false. Run scripts/qb-preflight.sh and the auth matrix (G-QB-A-DEPLOYED). Container name data-plane-v2-retrieval-engine-1. Only in the live-proof environment: WEB_RERANK_ENABLED=1 and WEB_RERANK_PROCESSOR_APPROVED=true (owner ruling 3).
- Owner chain: C3

### S6 (data): Data-3: late items (defaults flip, budget gate, provider pool, ZDR egress guard, v2 rerank metering, CR rerank-input flag off)

- Tasks: CR-T13, CR-T9b, QB-T05b, Z-41, QB-M, CR-T15, CR-T14
- Launcher: Data-only copy (--from data).
- Flags and ordering: CR default ON from code/compose (the live .env is never edited in place; new knobs only in .env.example); RERANK_INPUT_INCLUDE_CONTEXT stays 0 until G-T14-CTX-CELLS; isolated stacks set CR off explicitly.
- Owner chain: C1 + C3 + C11

### S7 (model): Model-C: ZDR enforcers (dark: nothing mints a waiver yet)

- Tasks: Z-15, Z-16, Z-17, Z-18, Z-21a
- Launcher: Model-only copy (--from model, LAST_STACK_INDEX=2); session-core migration 0043 applies at start.
- Flags and ordering: Enforcers ship before minters. Each service must report WAIVER_CONTRACT_VERSION (G-ENFORCERS-BEFORE-MINTERS). AGENTIC_SUBSCRIPTION_ENABLED is passed in the launcher env only (compose default off): do not silently reset it.
- Owner chain: C4 + C5

### S8 (ingestion): Ingestion-1: integration-corev2 ZDR capability and acknowledgement

- Tasks: Z-12, Z-13
- Launcher: Ingestion-only copy: LAST_STACK_INDEX=4, run --from ingestion; migration 0013 applies at start; run the idempotent backfill.
- Flags and ordering: Personal ack routes are bearer-only; no internal-key bypass.
- Owner chain: C8

### S9 (ingestion): Ingestion-2: Quarry-v2 (ZDR floor, manifest tokens, vector leg, rerank client, defaults off)

- Tasks: Z-08, QB-T02, QB-T09a, QB-T10, QB-N1, QB-T09, QB-T17, QB-T11, QB-T14
- Launcher: Compile check first: cd "apps/Ingestion Plane/Quarry-v2" && docker build -f deploy/compose/Dockerfile.edge --target build -t quarry-edge-compilecheck:local . (about 20 min cold; confirm Compiling quarry-runtime/quarry-transform/quarry-edge lines appear); unit tests via deploy/compose/Dockerfile.test; deploy with the Ingestion-only copy (LAST_STACK_INDEX=4, --from ingestion). quarry-edge needs a recreate (not a restart) for new env.
- Flags and ordering: QUARRY_EDGE__RERANK_SHADOW=1 for the smoke run then off; QUARRY_EDGE__AUTOPROMPT=0 (D-C) and the other three Model Plane dependents off; QUARRY_EDGE__VECTOR_RETRIEVAL stays 0 until G-M1B-BACKFILL. Compose is dirty (integration-api hunk): quarry-edge block only.
- Owner chain: C9

### S10 (control): Control-2: Auth Core decision endpoint (flag off), launcher flag wiring, manifest spec

- Tasks: CR-T17, Z-14, Z-32
- Launcher: Control-only copy (--from control, LAST_STACK_INDEX=0) after the CP wave has merged its files.
- Flags and ordering: ZDR_WAIVER_ENABLED=false (kill switch). Mint any newly required credential first with run-control-plane.sh config > /dev/null.
- Owner chain: C7

### S11 (application): Application: conversation-core posture reader (Z-03 part)

- Tasks: Z-03
- Launcher: Application-only copy: LAST_STACK_INDEX=3, run --from application.
- Flags and ordering: Keeps raw intent for the purge/skip use unless the Control wave exposes the effective flag; pinned by a Go test with the real Control response fixture.
- Owner chain: C10

### S12 (frontend): Frontend: gateway first, then SPA and trust files

- Tasks: Z-04, Z-03, Z-09, Z-05b, Z-19, Z-20, Z-21b, Z-42, X-TRUST
- Launcher: Frontend-only copy: LAST_STACK_INDEX=5, run --from frontend (gateway image). CAUTION: --from model also rebuilds the frontend plane from the working tree and bakes in other sessions' half-finished edits. SPA edits are NOT live until the container restarts (frontend-plane-verevonv3-frontend-1 runs Vite dev over a bind mount, no hot reload): run git status on apps/Frontend Plane/verevonv3 first, then docker restart (about 6 s), or verify on the :5199 host preview (preview verevonv3-dev).
- Flags and ordering: Do not restart while another session has half-done edits in the tree.
- Owner chain: C10

### S13 (model): Model-D: Quarry score calibration in model-gateway

- Tasks: QB-T12
- Launcher: Model-only copy (--from model).
- Flags and ordering: Only after G-T14Q-CALIBRATION; absent score_kind stays byte-identical.
- Owner chain: C4

### S14 (control): ZDR proof window: turn the waiver flag on (only after G-ENFORCERS-BEFORE-MINTERS), prove, then decide to leave on

- Tasks: Z-60
- Launcher: Control-only copy with --from control --skip-build and the ZDR_WAIVER_ENABLED=true env for the window only.
- Flags and ordering: Kill switch: flip the flag back; tokens revert to org posture on the next mint (at most 300 s for tokens already issued). Terra only.
- Owner chain: orchestrator + C10

## 6. Live proofs in order (cheapest first), pass criteria, tiny-usage notes

### LP-0 (free): CP wave probes

- After: G-CP-DEPLOYED
- Pass criteria:
  - auth-service reaches org-core with service headers and gets 200
  - a dedicated ZDR test org mints zdr:true and a non-ZDR org zdr:false
  - an org-core outage returns 503 retention_posture_unavailable and no token
  - a fresh bootstrap and the launcher produce the same registry (parity spec)
  - quarry-edge mints data:rerank; interactive tokens cannot reach /v1/rerank; no allowAnyOrg remains in production file mode
- Usage note: Free. Project registry output through a node filter; never print env var values or key names with values (two earlier probes leaked a credential prefix).

### LP-MODEL-A (free): Model-A: typed statuses and ZDR stop-gaps

- After: S1
- Proves: CR-T3, Z-00, Z-01, Z-05a, Z-06
- Pass criteria:
  - GET :8082/metrics returns counters (not the 37-byte stub)
  - ListModels works
  - an Infer with request.zdr=true is refused before any provider call with x-mp-provider-error=zdr_unavailable and the provider-call counter does not move
  - a normal Terra chat turn still works; no 401 spike in model-gateway/inference-core logs
- Usage note: Free; no grpcurl on this machine: use a node http2 probe in the style of apps/Model Plane/scripts/smoke-wave10.js.

### LP-COST (free): cost-core ingest v2

- After: S2
- Proves: CR-T4
- Pass criteria:
  - one zero-token record for the TEST org with a data-orchestrator token appears via /api/v1/cost/aggregate?feature=...
  - /api/v1/budget/check for that org is unchanged after the Data Plane row is recorded
  - per-producer counts of model-gateway rows unchanged
- Usage note: The zero-token test row stays; use the test org, never a customer org.

### LP-SQL (free): CR-T6 migration

- After: S3
- Proves: CR-T6
- Pass criteria:
  - 206 legacy, 1 pending, 0 incoherent
  - row counts unchanged
  - embedding_model still NULL
  - up then down then up clean on a disposable DB first
- Usage note: SQL only.

### LP-CR (tiny-paid): Contextual retrieval proof (CR-T7/T8/T9a/T10/T11/T12)

- After: G-OPS-ORG, G-CP-DEPLOYED, G-T3-DEPLOYED, S3
- Proves: CR-T5, CR-T7, CR-T8, CR-T9a, CR-T10, CR-T11, CR-T12
- Pass criteria:
  - 207 of 207 units have a non-NULL context_status
  - Quickwit versus Postgres drift is 0 after the reconcile (named documents re-announced, not rebuilt)
  - a forced re-embed with unchanged inputs makes 0 provider calls (counter check)
  - stop_reason handled with a captured real Azure response (stop)
  - a ZDR org makes 0 provider calls and records skipped_zdr
  - graph-index restart makes no new call for an empty document
- Usage note: Tiny paid usage, state the counts: 3 gpt-4o-mini calls (about 0.002 USD) shared by T7/T8/T10; 1 call (about 0.0002 USD) to heal the raw chunk (T9a); 1 extraction call (about 0.0005 USD) for T12; the reembed-switch makes 3 Cohere embed calls and the legacy verification about 206 Cohere embed calls (about 60k tokens, well under a cent). Zero new LLM calls on the unchanged-input pass.

### LP-METER (free): Metering end to end

- After: LP-CR, G-T10-CONSUMER-DEPLOYED, G-T4-DEPLOYED-PRICED
- Proves: CR-T10, CR-T4
- Pass criteria:
  - one CR call gives exactly one cost-core row with feature, org and exact tokens
  - replaying it still leaves one row
  - the chat budget aggregate is unchanged
  - killing data-orchestrator mid-run loses no event after restart
- Usage note: Uses the LP-CR calls: no extra spend.

### LP-AUTH-MATRIX (free): Retrieval-engine auth matrix

- After: G-QB-A-DEPLOYED
- Proves: QB-T03, QB-T08
- Pass criteria:
  - no token 401
  - user token on /v1/rerank 403
  - service token without data:rerank 403
  - service token on admin routes 403
  - quarry-edge token on /v1/retrieve 200 with zdr_mode ephemeral (no Cohere embed, no persisted trace)
  - dpv2_authz_denials_total{cause=denied:no_membership} stops increasing for service:quarry-edge
- Usage note: Free; the matrix prints labels and statuses only.

### LP-RERANK (tiny-paid): One paid rerank call

- After: LP-AUTH-MATRIX, G-PROCESSOR-APPROVED-PROOF
- Proves: QB-T05a, QB-M
- Pass criteria:
  - status ok with Cohere scores
  - the same request_id retried makes no second provider call
  - a ZDR request returns skipped in input order with zero provider calls and cost-event count 0
  - dpv2_web_rerank_total shows exactly the expected calls
  - the real Azure Foundry Cohere v2 response is committed as a golden (synthetic public strings only)
- Usage note: Tiny: one 10-document call (one search unit) plus free probes; report the exact provider-call count (expected 1).

### LP-QUARRY (tiny-paid): Quarry search end to end

- After: S9, LP-RERANK, G-M1B-BACKFILL
- Proves: QB-T09, QB-T10, QB-N1, Z-08, QB-T17
- Pass criteria:
  - no 403 in quarry-edge logs
  - vector hits carry real URLs and only source=quarry
  - engines survive fusion
  - scores appear only when the request opts in (score_kinds) and only after QB-T12
  - a ZDR user token search makes zero DP calls (retrieval_runs count unchanged)
  - a service token with zdr:true but body zdr:false still runs non-ZDR
  - kill-switch drills (SEMANTIC_RERANK=0, WEB_RERANK_ENABLED=0, scope removal) return results in original order with the right marker
- Usage note: Tiny: one paid 10-doc rerank through Quarry and one Cohere Embed query for the vector check; state the counts.

### LP-EVAL (owner-approved-paid): Eval gates (contextual and web rerank)

- After: G-OWNER-OD8, LP-CR, LP-QUARRY, G-OPS-LABEL
- Proves: CR-T14, QB-T14
- Pass criteria:
  - CR and rerank cells do not drop below the committed baseline (contextual nDCG@10 0.7732)
  - web rerank: nDCG@10 >= +0.04 over engine order with CI lower bound > 0, no category drop > 0.03, added p95 <= 900 ms
  - degraded-rate measured against the shared-deployment baseline (dedicated capacity is a default-ON precondition)
- Usage note: Owner approval OD8: about 1,164 chunk calls (about 0.15 USD) plus Cohere embed; web rerank about 80 paid calls plus a 200-call soak (under a dollar at plausible prices; price unverified). State exact counts.

### LP-MODEL-D (subscription): Quarry calibration in the Model Plane

- After: S13
- Proves: QB-T12
- Pass criteria:
  - a Terra-subscription chat turn with web_search carries score, the gate keeps on-topic sources, the coverage line mentions rerank when skipped; repeat on a ZDR test org and see the skipped marker
- Usage note: Terra only (gpt-5.6-terra via openai-codex-subscription), never Verevon Balance.

### LP-ZR-FREE (free): ZDR free probes Z0 and Z1 (before any Terra call)

- After: G-OPS-ORG-ZDR, G-OWNER-ZDR-PROOF, S10, S12
- Proves: Z-60
- Pass criteria:
  - Z0: standard chat on the ZDR test org is refused with a typed reason; no run row, no broker audit line, no non-Terra provider in inference-core logs
  - Z1: stale digest 409, other member 403, X-Internal-API-Key refused, client zdr:false ignored, direct token mint has no waiver, the ZDR test org mints zdr:true
  - the org flag is unchanged
- Usage note: Free; no model call.

### LP-ZR-TERRA (subscription): ZDR waiver proof on Terra (Z2 to Z6)

- After: LP-ZR-FREE, G-ENFORCERS-BEFORE-MINTERS
- Proves: Z-60
- Pass criteria:
  - Z2 one Terra Ask after acknowledgement: run row zdr_waived with ack id, thread persisted, usage frame and answer line, audit events, only openai-codex-subscription in logs, derived jobs skipped
  - Z3 second member without acknowledgement refused and member A on another model refused (never org-wide)
  - Z4 Do with plan_mode under waiver (children register with lineage)
  - Z5 revoke then refused; catalog version bump refused until re-ack
  - Z6 Temporary chat plus Terra refused
  - a non-ZDR org sees no change
- Usage note: About 7 to 10 Terra rounds on the owner subscription; zero Verevon Balance spend; evidence lines (ids, counts, codes, timings) only. Do mode still needs AGENTIC_SUBSCRIPTION_ENABLED and is not claimed closed.

### LP-TRUST (free): Trust claims against proven behaviour

- After: LP-CR, LP-RERANK, LP-QUARRY, LP-ZR-TERRA
- Proves: X-TRUST, X-DOCS-FINAL
- Pass criteria:
  - trust-claims tests and check-citations.mjs pass against the evidence
  - every pending sentence is rewritten to what the proofs show; gaps are stated
- Usage note: Free.

## 7. Owner decisions that are REALLY open after applying the defaults file

### OD-LAPSE

- Question: Plan-lapse semantics for org ZDR: when an org's plan (entitlement) lapses but ZDR intent is on, does the org stay ZDR? Today Auth Core says no (intent AND entitlement), the BFF fails true and conversation-core reads raw intent.
- Default applied so the build can proceed: Stricter reading: intent=true stays ZDR until an admin turns it off; Z-10 matrix row and Z-03 follow it. Changes Auth Core's current behaviour, so the CP wave owner must be told.
- Blocks: Z-03; Z-10 matrix row; CP wave resolver semantics
- Source: Z-03 review

### OD-ZDR-INGEST

- Question: Scope of org ZDR over ingestion and indexing (design OD1 / OD-Z9): must Data Plane text embedding (Cohere), page-image embedding and rerank also be gated by the same org-ZDR oracle in this release, or is org ZDR interactive-content-only with the gap stated in the trust copy?
- Default applied so the build can proceed: Build proceeds with option A (interactive content only, state the gap) plus Z-41 (lexical degrade notice) and DEF-CR-EMBED-ZDR-ORACLE as a scheduled follow-up. The critic says the oracle must ship in the same release or the trust copy must state the gap: the owner must choose.
- Blocks: X-TRUST wording; a follow-up task for the embed/page-image oracle
- Source: critic-contextual #18, critic-zdr OD-Z9

### OD-LEGAL

- Question: Legal and counsel sign-offs: (1) Art. 28 processor basis for sending query text and candidates to Azure/Cohere before WEB_RERANK_PROCESSOR_APPROVED=true becomes a production default (GDPR_SUMMARY also requires allow_third_party_processing); (2) wording of the ZDR acknowledgement texts NO/EN including the supported text and vendor claims (OD-Z12); (3) counsel review of the processor-instruction exception for user-owned connections.
- Default applied so the build can proceed: Proof environments run with the approval flag set per owner ruling 3 and with placeholder acknowledgement text; production defaults stay false/placeholder until sign-off.
- Blocks: production default of WEB_RERANK_PROCESSOR_APPROVED; ZDR_WAIVER_ENABLED outside the proof window
- Source: quarry O2, zdr OD-Z12

### OD-PRICES

- Question: The price sheet for embed-v-4-0, text-embedding-3-large and Cohere-rerank-v4.0-pro (per search unit): none exists in the repo and none may be invented.
- Default applied so the build can proceed: Forwarder stays dark and cost_usd null until sourced rows are supplied; unknown models would otherwise price at 3.00/15.00.
- Blocks: G-T4-DEPLOYED-PRICED; forwarder enablement; USD figures in the cost view
- Source: quarry O5, integrator

### OD-PER-ORG-OFF

- Question: May a per-org CR "off" (skipped_policy) or an enforcing budget (skipped_budget) be used on CUSTOMER orgs, given the ruling that CR is not switched off?
- Default applied so the build can proceed: Operator-only, audited, reason required, never silent; used for the eval baseline orgs and incidents; no org-admin UI; budget default alert-only (D-E).
- Blocks: CR-T11 admin API scope of use; CR-T9b stop mode
- Source: critic-contextual open question 3

### OD-TEMP-CHAT

- Question: Waiver scope: may a Temporary chat (explicit per-turn ZDR) be waived by a personal subscription, and may the waiver apply to unattended runs (scheduled, support, inbox, browser, speech)?
- Default applied so the build can proceed: No to both: the waiver is for org-default ZDR on interactive owner-present chat turns only; Temporary chat plus Terra refuses (matches normalize.rs today).
- Blocks: Z-10 matrix rows; Z-19/Z-20 behaviour
- Source: zdr OD-Z1, critic-zdr

### OD-EVAL-APPROVAL

- Question: Approval of the paid eval re-seed (about 1,164 chunk calls, about 0.15 USD plus Cohere embed) and the standing nightly eval (OD8); approvals for the live-proof writes (throwaway orgs, ZDR on the test org, ZDR_WAIVER_ENABLED window, owner-interactive ChatGPT device sign-in).
- Default applied so the build can proceed: None: these are approvals, requested per step in chat.
- Blocks: G-OWNER-OD8; G-OPS-ORG; G-OWNER-ZDR-PROOF
- Source: contextual OD8, zdr section 11

### OD-BREADCRUMB

- Question: Include the optional deterministic tier-1 breadcrumb context (title/heading path) for every org including ZDR orgs (CR-T18)?
- Default applied so the build can proceed: Deferred: decide after CR-T14 shows the baseline.
- Blocks: CR-T18
- Source: contextual OD11

### Settled by the defaults file or by the designs' recommended option (build proceeds; owner may overrule)

- **D-A**: No provider is attested; the org ZDR switch is gated on an attested provider (Z-04) with an operator-only seed; attestation is data not code (defaults file).
- **D-B**: Derived jobs (memory indexing, dreaming, learning review) are skipped on a waived run; the consent text names it (defaults file; Z-13, Z-17).
- **D-C**: Quarry autoprompt, answer synthesis, ai-formats and page-title default OFF; the proper design is scheduled (QB-T17 part 2) (defaults file).
- **D-D**: Rerank capacity: shared Foundry deployment with a limiter now; dedicated before general availability (defaults file; QB-T05b).
- **D-E**: Data Plane background-LLM budget: configurable per org and global, default alert-only, owner supplies the figure later (defaults file; CR-T9b).
- **OD3**: Metering architecture: usage ledger v2 on a durable stream, data-orchestrator forwarder, cost-core feature/units (accepted by the integrator).
- **OD4**: Dedicated Azure deployment alias for background LLM (e.g. gpt-4o-mini-bg): operator portal action; default is to share the deployment with caps and the circuit breaker until it exists; non-blocking.
- **OD5**: Pin background document calls to provider_hint azure-openai and min_residency eu (label-based until AZURE_OPENAI_DEPLOYMENT_TYPE is read from the real deployment; never set it blindly).
- **OD7**: org_processing_policy is operator-only now (data:quality:admin); UI for org admins later.
- **OD9**: Production principal model (allowAnyOrg refused in production): CP wave P1.
- **OD10**: Keep gpt-4o-mini for CR; ctx-v2 ships only if the eval gate passes.
- **O3**: Quarry vector leg searches Quarry-origin documents only.
- **O6**: Rerank wire format HTTP/JSON only now; gRPC when a Model Plane caller exists.
- **O7**: Rerank is not gated by allow_paid_providers (it is bounded by limits and deadlines).
- **O11**: Judged set labelled by an LLM pre-judge on the Terra subscription plus human adjudication (labels never feed runtime ranking).
- **O12**: Shadow period only for the smoke run and live proof.
- **OD-Z3**: Acknowledgement lifecycle: no time expiry; invalid on digest change, text version bump, connection removal or owner revoke.
- **OD-Z4**: No admin lock (owner ruling 5: we do not stop them); audit plus admin visibility only.
- **OD-Z5**: cannot_verify is treated as non-ZDR (waiver needed); an admin assertion for own API keys is deferred (Z-51).
- **OD-Z7**: ZDR turn metering: content-free count rows, recorded in the ledger (matrix row 87); deferred as DEF-Z-06b.
- **OD-Z8**: Do mode under REAL ZDR is refused with a typed reason in v1; waived runs get normal children.
- **OD-Z11**: The acknowledgement ledger lives in integration-corev2; the Control to Ingestion read edge is recorded in the contract matrix.

## 8. Operator and owner actions (not code)

- **OPS-ORG**: Provision the throwaway proof orgs through real org-core (pro plan, members); tenant posture refuses unknown orgs and the old eval fixture orgs are gone (needs: owner approval for writes; unblocks: LP-CR, CR-T7, CR-T14, QB-T10 live)
- **OPS-PRICE**: Supply the price sheet for embed-v-4-0, text-embedding-3-large and Cohere-rerank-v4.0-pro (per search unit) (needs: owner/operator; unblocks: G-T4-DEPLOYED-PRICED)
- **OPS-SECRETS**: Rotate GRAPH_INDEX_GDPR_NATS_PASSWORD (printed in a critic transcript) and CODEX_SUBSCRIPTION_INTERNAL_API_KEY (first 40 characters printed); disclose the roughly 6 hex characters of one principal credential exposed by an auth-service env probe (needs: owner call (critics were read-only); unblocks: n/a)
- **OPS-LABEL**: Judged-set labelling: LLM pre-judge on Terra plus a human adjudicator (all disagreements and a 20% sample, kappa >= 0.6) (needs: a human adjudicator; unblocks: QB-T14)
- **OPS-ALIAS**: Optional: dedicated Azure deployment alias for background LLM (OD4) and reading the real AZURE_OPENAI_DEPLOYMENT_TYPE (needs: Azure portal; unblocks: quota isolation; the EU residency claim becoming provable)
- **OPS-ATTEST**: Z-40: obtain the signed no-retention approval and set AZURE_OPENAI_ZDR_* through the release path (needs: owner/operator contract action; unblocks: G-ATTEST)
- **OPS-CAPACITY**: Dedicated rerank deployment/quota before default-ON (D-D) (needs: Azure portal; unblocks: QB-T14 degraded-rate gate for default-ON)
- **OPS-OTHER-SESSION**: The other session must commit and deploy its cost-core work (migrations 0006/0007, azurecost, provider_used) (needs: the other session / owner; unblocks: G-CORECOST-OTHER-SESSION)

## 9. Shared files and crates touched from more than one chain

- `apps/Data Plane v2/docker-compose.yml` [CR-T5, CR-T10, CR-T9a, CR-T12, CR-T13, QB-T08]: One service block per task, hunks only, re-read before each edit (CRLF file). Blocks: CR-T5 posture env names (embedding-engine ~759, graph-index ~984); CR-T10 data-orchestrator block, key mounts and the cost stream; CR-T9a embedding-engine reconciler env; CR-T12 graph-index block (remove the Azure key and endpoint ~988); CR-T13 embedding-engine CR defaults; QB-T08 retrieval-engine block (lines ~361-538) ONLY. Order within chain C1: T5, T10, T9a, T13; CR-T12 (C2) starts after CR-T10 and touches only the graph-index block; QB-T08 (C3) touches only the retrieval-engine block. The orchestrator serialises the commit (plan: one commit, one block per service).
- `apps/Data Plane v2/services/graph-index-rs/src/extractor.rs` [CR-T5, CR-T10, CR-T12]: Strict order CR-T5 (client conversion), CR-T10 (signer wiring only), then CR-T12 (hardening); CR-T12 depends_on both, so graph-index is never edited concurrently.
- `apps/Data Plane v2/services/graph-index-rs/src/config.rs` [CR-T5, CR-T12]: CR-T5 then CR-T12 (depends_on CR-T5).
- `apps/Data Plane v2/services/graph-index-rs/Cargo.toml` [CR-T5, CR-T12]: CR-T5 then CR-T12.
- `apps/Data Plane v2/Makefile` [CR-T5, CR-T14]: CR-T5 edits the cross-plane-env list (~line 46) INCLUDING the env names owned by CR-T10 (DATA_ORCHESTRATOR_COST_SERVICE_API_KEY and the graph-events key paths); CR-T14 (depends_on CR-T8/T9a/T11) adds eval targets later.
- `apps/Data Plane v2/openapi/d4d5.yaml` [CR-T11, QB-T04, QB-T08]: Distinct sections (data-quality admin/coverage endpoints; SourceRef.url and zdr_mode docs; /v1/rerank): one editor at a time per section, hunks only, CRLF; check line endings with node.
- `apps/Data Plane v2/tests/scripts/run-isolated-real-authority.sh` [CR-T13, QB-T08]: Distinct regions: QB-T08 generates the quarry-edge principal row in the hand-written registry (~line 164) from config/plane-service-principals.json and adds a parity test; CR-T13 only adds the explicit CR-off environment. Hunks only, LF shell script; QB-T08 (C3) normally lands before CR-T13 (C1, late).
- `apps/Data Plane v2/DATA_PLANE_STATUS.md` [QB-T08, CR-T16]: Distinct sections (web-rerank vs contextual retrieval); hunks only (CRLF .md).
- `apps/Model Plane/docs/ZDR.md` [CR-T16, Z-30, Z-40]: Docs chain order CR-T16 (stale Anthropic attestation statement), then Z-30 (rewrite to the current state); Z-40 (operator) only appends the attestation record section after Z-30.
- `apps/Ingestion Plane/docker-compose.yml` [QB-T17, QB-T11, Z-32]: Dirty (another session's integration-api hunk ~line 458+). QB-T17 then QB-T11 edit ONLY the quarry-edge block (~121-147) by hunk; Z-32 edits only its own env lines (after the CP wave) and never the integration-api hunk of the other session.
- `apps/Model Plane/rust/services/execution-core/src/grpc.rs` [Z-05a, Z-18]: Z-05a (C4) first; Z-18 (C5) starts only after G-MP-CONTRACTS (Z-05a and Z-11 merged). Dirty (+133, another session): hunks only.
- `apps/Model Plane/rust/services/execution-core/src/agent_route.rs` [Z-05a, Z-18]: Same as grpc.rs; the file is untracked (another session): hunks only, never replace it.
- `apps/Model Plane/deploy/docker-compose.yml` [Z-32, Z-40]: Dirty. Z-32 adds the WAIVER_CONTRACT_VERSION/flag wiring blocks; Z-40 sets the AZURE_OPENAI_ZDR_* values through the release path (ZDR_RETENTION_PATH=zdr-provider-route-attested), editing compose only for variable wiring, after Z-32.

Crate ownership exceptions (each ordered by dependency so no crate is edited by two chains at the same time):
- `dp:graph-index-rs` (C1 (CR-T5, CR-T10); C2 (CR-T12)): CR-T12 depends_on CR-T5 and CR-T10 (gates G-T5-DONE, G-T10-CRATE), so graph-index is only ever edited by one chain at a time; after CR-T12 starts chain C2 owns the crate exclusively.
- `dp:data-quality-go` (C1 (CR-T10, CR-T11); C11 (CR-T14)): CR-T14 depends_on CR-T11 (gate G-T11-DONE); C1 does not touch data-quality-go after CR-T11.
- `cp:auth-core` (C3 (QB-T03); C7 (CR-T17, Z-14)): QB-T03 adds NEW files only (a jest spec and fixtures that regenerate the two Auth Core goldens); it never edits an existing auth-core file. The existing files belong to the CP wave and then C7 (hunks, after the wave merges). The new spec imports ConvexTokenService (read-only dependency).
- `mp:execution-core` (C4 (Z-05a); C5 (Z-18)): Z-18 starts only after G-MP-CONTRACTS (Z-05a merged), so the crate is not edited concurrently.

## 10. Deferred, merged, split

### Deferred
- **QB-T06** Extractive passage highlights. Reason: SERP snippets are 70 to 274 characters (median 138-172) so a 320-character window equals the whole snippet; the old LLM reranker never worked (403 since 2026-09-23), so no consumer ever got highlights. Remove the highlights field from /v1/rerank v1. Revisit: Data Plane vector chunks (up to 4000 chars) or fetched page text become reranker inputs: design as profile web-v2 with offsets, Unicode scalar indexes and a measured precision gate.
- **CR-T18** Deterministic tier-1 breadcrumb context. Reason: Optional and eval-gated; needs a chunker change and migration; owner decision OD-BREADCRUMB; the CR core must be proven and the CR-T14 baseline exist first. Revisit: CR-T14 cells exist and the owner decides.
- **Z-50** Own-API-key connection kind (XL). Reason: No such connection exists; the design defers it; the proof is out of scope for the Terra-only rule and needs an explicit owner decision to prove on the owner's own key. The record model is generic so it plugs in later. Revisit: Owner asks for own-API-key connections.
- **Z-51** Customer-asserted ZDR basis for own keys. Reason: Depends on Z-50 and on OD-Z5 acceptance. Revisit: After Z-50.
- **DEF-Z-06b** Metering of ZDR chat turns (content-free envelope class in model-gateway state.rs/budget.rs and cost-core decodeUsageMessage). Reason: Needs cost-core ingest v2 deployed and the OD-Z7 convention recorded; no ZDR turn can occur before an attestation (Z-40) anyway; the cost-core file is dirty. Revisit: CR-T4 deployed and Z-40 done.
- **DEF-QB-M10** ZDR-compliant rerank path (self-hosted text cross-encoder profile web-v1-sovereign, or an attested route modelled on ZdrAttestation). Reason: Skip-with-marker is the interim behaviour (ZDR/sovereign orgs get skipped in input order). Exit criteria: (a) permits_rerank() true only from attestation DATA with evidence fields, or (b) a self-hosted cross-encoder profile with no egress, proven by the same policy matrix and a zero-egress test. Revisit: G-ATTEST or a sidecar decision (owner).
- **DEF-CR-EMBED-ZDR-ORACLE** Org-ZDR oracle for Data Plane text embedding (Foundry Cohere), page-image embedding and rerank. Reason: Owner decision OD-ZDR-INGEST; until decided the trust copy must state the gap (index-time text embedding is gated only by the per-document restricted classification). Revisit: OD-ZDR-INGEST answered or an attested embedding resource exists (Z-41).
- **DEF-CR-RE-CLIENT** Converge the retrieval-engine token client (embed/service_auth.rs, 650 lines, the third copy) into inference-client-rs. Reason: QB edits retrieval-engine; converge afterwards to avoid collisions. Revisit: After QB-T05a/T08/Z-41 are deployed and stable.
- **DEF-QB-VECTOR-METERING** Metering of Quarry-triggered query embeds (Cohere Embed) through usage ledger v2. Reason: Only rerank cost events exist today; needs the metering foundation first. Revisit: CR-T10 deployed.
- **DEF-QB-T17-PART2** Proper org-bound, metered model path for Quarry autoprompt/answer/ai-formats/page-title. Reason: Owner default D-C: switched off now; the proper design is a scheduled task recorded in the ledger as OPEN with an owner and date. Revisit: Scheduled design task.
- **DEF-ZR-GENERIC-CONNECTION-ROUTES** Owner-only checks on the generic integration-corev2 connection read routes (GET /api/v1/connections, /:id, /:id/consents). Reason: Any org member can read another member's codex-subscription connection through them (personalProviderKeys holds only microsoft and google). Z-13 makes only the ZDR routes owner-only. Revisit: Record in the ledger; schedule with the Chat Wave-2 personal-connections owner-only ruling.

### Merged
- **QB-T07** into QB-T05a, QB-M, CR-T10. Duplicates the contextual metering foundation (two column sets on cost_events, the same strict Go verifier, a non-durable core-NATS consumer). Interim v1 rerank cost event ships inside QB-T05a; the v2 producer is QB-M after CR-T10. Its 20261003120000 migration is NOT created.
- **QB-T15** into deploy_stages, live_proofs, QB-T08, QB-T11. Rollout and live proof are the stage and proof tables of this manifest; its pre-flight script is scripts/qb-preflight.sh in QB-T08; the post-proof compose default flips are post-proof steps of QB-T08 (Data Plane) and QB-T11 (Ingestion).
- **QB-T16** into CR-T4, CR-T10. Same four cost-core files and the same producer allowlist/units shape: one cost-core change set (CR-T4) and one forwarder (CR-T10).
- **Z-07** into Z-05a. Same sse.rs refusal block, same typed catalogue and the same test table (reviewer: merge).

### Split (parent replaced by children that are in chains)
- **CR-T9** into CR-T9a, CR-T9b. Reviewer: reconciler/parity/legacy verification (T9a) vs budget gate after T10/T11 (T9b); decouples the CR fix from metering.
- **QB-T05** into QB-T05a, QB-T05b. Reviewer: route/policy/limits/idempotency/metrics/v1 event vs shared provider permit pool and breaker that touches tenant retrieval.
- **QB-T13** into QB-T13a, QB-T13b. Matrix rule numbers must be written in the plan order 15 (QB), 16 (CR), 17 (ZR), 18 (QB).
- **Z-05** into Z-05a, Z-05b. Model Plane catalogue/refusal sites vs SPA rendering: different planes and chains.
- **Z-21** into Z-21a, Z-21b. session-core aggregate RPC (Model chain) vs BFF/SPA (Frontend chain).
- **QB-T09a** extracted from QB-T09. Reviewer: SearchProvider::search_with_report plumbing and cache rule shared with QB-T10, sequenced first.

## 11. Rules for every implementer

1. Never commit, stash, reset, revert or checkout: the tree is shared with other sessions (396 modified/untracked entries at the time of writing). Edit hunks in place, re-read the file before each edit, never overwrite a dirty file wholesale.
2. Never build images or restart containers from an agent: the orchestrator deploys through the launcher stage copies (apps/Frontend Plane/verevonv3/build-verevon-services.sh; never a hand-built docker compose).
3. Line endings per the standing rule: CRLF for .rs, .go, .md and Python; LF for .ts, .tsx, .js, .json, .yml, .sh. Check each neighbouring file with node (count \r\n against \n), never grep; the Edit tool keeps a file's endings, heredoc appends do not (mixed files). New migrations and goldens are LF.
4. Cite functions or anchors, not line numbers: the dirty files shift. Re-run node docs/compliance/evidence/tools/check-citations.mjs on every evidence doc that cites an edited file (inference-core grpc.rs 11 citations, cost-core 20, embedding-engine provider/mod.rs, contextualize.rs comments, extractor.rs:286-297) and re-pin moved anchors; evidence files are untracked (trust-edits track).
5. Contract tests must apply the REAL rules (real policy file, real middleware, real session-core credential checks, real tokens captured from the real mint, a captured real Azure/Cohere response), never permissive doubles. The A6 proof found six defects that stubs had hidden.
6. Goldens and manifests live INSIDE the consuming build context (retrieval-engine: services/retrieval-engine-rs/tests/fixtures/contracts/; Quarry: crates/quarry-runtime/...; auth-core: src/auth/fixtures/; Data Plane: services/<crate>/tests/fixtures/ or testdata) and are vendored byte for byte with a sha256 pin on BOTH sides plus a repo-level drift check. Specs must FAIL, not skip, when REQUIRE_CONTRACT or CI is set (the describeIfPolicy no-op trap).
7. Postgres-backed tests run as SET LOCAL ROLE dataplane_app inside the real pg-org-scope helper: the services connect as a superuser and RLS is inert otherwise. Cross-org workers run on the unscoped connection with explicit org_id predicates.
8. Live tests run on gpt-5.6-terra via openai-codex-subscription, never Verevon Balance or Claude. Paid reranking, embedding and contextualisation calls are intrinsic to those features: keep live volumes tiny and state the exact number of calls made.
9. Never print secrets or tokens. Reading the live registry must go through a node filter that projects audiences and scopes; two earlier probes leaked a credential prefix. Report any exposure.
10. Quarry-v2 compiles only in Docker: docker build -f deploy/compose/Dockerfile.edge --target build -t quarry-edge-compilecheck:local . (and Dockerfile.test for cargo test); cargo check on the host dies on libclang. Confirm the Compiling quarry-runtime/quarry-transform/quarry-edge lines appear.
11. Data Plane Rust: CI runs cargo fmt --check and cargo clippy --workspace --all-targets -- -D warnings with RUSTFLAGS=-D warnings (watch dead_code on new modules); new modules are declared in BOTH lib.rs and main.rs where the crate has both.
12. Cargo workspace discipline: create a new member crate directory with a compiling skeleton BEFORE adding it to the workspace Cargo.toml; refresh Cargo.lock minimally (cargo update -p <new crate>), never regenerate it; only chain C1 edits the Data Plane workspace Cargo.toml.
13. After any Model Plane proto change, cargo check the Data Plane v2 workspace too (graph-index-rs/embedding-engine-rs build Model Plane proto messages as struct literals). On a launcher build exit 101 rerun with COMPOSE_PARALLEL_LIMIT=1.
14. Shared files outside a task's chain (compose files, Makefile, .env.example, SUBJECTS.md, ledger.md, matrix) are edited by hunk, one service block per task; the shared compose-security-contract-test.sh is edited ONLY by CR-T13 and CR-T5; other tasks add new sibling scripts.
15. Scan for NUL-filled files and CRLF damage after any abrupt shutdown or reboot (a reboot kills workflows and empties the auth session store: token endpoint 401 means check boot time first).
16. Enforcers before minters; kill switch before enablement: ZDR_WAIVER_ENABLED stays off until G-ENFORCERS-BEFORE-MINTERS; the registry flips to tenant only after the auth-core image has the tenant code; ship consumers/verifiers before producers; ship cost-core ingest v2 before the forwarder.
17. Where a design note and a review disagree, the review/manifest note wins; where an owner ruling or default applies, it wins over both.

## 12. Task entries

Each entry keeps id, design, title, plane, files, depends_on, size and the reviewer notes with the critic corrections folded in. The JSON adds critic_corrections_applying, files_missing_or_wrong and cross_design_overlap for every task.

### Chain C1 dp-cr

#### CR-T6: Postgres migration: CR provenance and status, org_processing_policy, coverage view, legacy backfill

- design: contextual (T6) | plane: data | size: M | chain: C1 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy [G-BACKUP]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS. The legacy backfill as written contradicts the integrator plan and the critic.
- files:
  - apps/Data Plane v2/infra/postgres/migrations/20261003130000_context_provenance.sql (new)
  - apps/Data Plane v2/infra/postgres/migrations/20261003130000_context_provenance.down.sql (new)
  - apps/Data Plane v2/tests/ (new disposable-Postgres contract tests run through the real tools/migrator as dataplane_app)
- files removed from the design list:
  - apps/Data Plane v2/infra/postgres/init.sql
  - apps/Data Plane v2/services/index-engine-rs/src/builder/mod.rs
  - apps/Data Plane v2/tools/migrator
  - apps/Data Plane v2/tests/scripts/compose-security-contract-test.sh
- corrections (folded in):
  - Legacy backfill must NOT assert coherence: add legacy to the status CHECK, set embedded_context_hash NULL for rows that have a chunk_context, mark eligible rows without context pending. v_context_coverage counts legacy separately and vector_incoherent must exclude NULL-hash rows. Expected live result: 206 legacy, 1 pending, NOT 206 ready.
  - embedding_model backfill: do not hard-code a model in SQL; stamp it when the CR-T9a verification pass re-embeds (the only moment the truth is known). Canonical hash bytes: encode(sha256(convert_to(chunk_context,'UTF8')),'hex') over the stored, trimmed string.
  - Timestamps: latest applied is 20260907120000; quarry T07's 20261003120000 is dropped, so use 20261003130000_context_provenance (+ .down.sql); CR-T10 takes the next free timestamp. The migrator applies in version order, each file in one transaction (no CREATE INDEX CONCURRENTLY).
  - init.sql is not the convention for knowledge_units: chunk_context and context_prompt_version are absent from init.sql (baseline only; verified); later columns arrive by migration through the migrate service. Do not mirror (init.sql mirroring applies to cost_events only). index-engine builder test schemas and compose-security-contract-test.sh need no change unless builder SQL starts referencing the new columns.
  - v_context_coverage must be created WITH (security_invoker = true) (Postgres 16 live); a view owned by the superuser otherwise bypasses RLS for dataplane_app.
  - org_processing_policy is NOT protected automatically (the RLS migration says a new table must be added explicitly): create the same policy (org_id = current_setting('app.current_org', true), USING and WITH CHECK). Default privileges already grant dataplane_app. The data-quality org purge (data-quality-go internal/gdpr/postgres_purger.go lists tables explicitly) is edited in CR-T11 (same chain), not here, to keep data-quality-go in one task set.
  - Between migration and the CR-T8 deploy the old code keeps writing chunk_context with status NULL: re-run the idempotent backfill at CR-T8 deploy, or make the reconciler selector treat NULL status plus a context as legacy.
  - No image rebuild for the migration itself: the migrate service bind-mounts infra/postgres/migrations (the Data stage launcher hook apply_dataplane_migrations applies it). Take a backup of knowledge_units first (scripts/backup.sh).
  - Do not edit the already-applied 20260824120000 migration to fix its stale comment: use COMMENT ON COLUMN statements in this migration (comment-only), keeping applied files immutable.
- cross-design overlap: Quarry T07 also wrote a cost_events migration at 20261003120000 (dropped). CR-T10 uses a later timestamp (20261003140000_cost_events_v2).
- deploy: S3 - data: migration only (docker compose run --rm migrate up through the launcher; the Data stage hook apply_dataplane_migrations does this). No image rebuild, no config change. Deployable alone: nothing reads the columns until CR-T7/CR-T8.
- live proof (none): SQL only: expect 206 legacy, 1 pending, 0 incoherent, row counts unchanged, embedding_model still NULL; up then down then up clean on a disposable DB first.
- notes_for_implementer: Write the migration idempotently (the fresh-DB path applies it after init.sql; use ADD COLUMN IF NOT EXISTS with the CHECK declared inline on the column, CREATE TABLE IF NOT EXISTS, CREATE OR REPLACE VIEW ... WITH (security_invoker = true), DROP/CREATE POLICY guarded in a DO block). Columns as designed plus status legacy; context_attempts default 0 (fast default, no rewrite); partial index idx_ku_context_work. Statuses: NULL means unknown; legacy is a real status. The down migration drops view, table, columns. Do NOT fabricate coherence. Contract tests with REAL rules: run the real migrator (tools/migrator) against a disposable Postgres 16 with every migration in order, then assert as the dataplane_app role inside the real pg-org-scope helper (SET LOCAL ROLE dataplane_app plus app.current_org) that org A cannot read or write org B rows in org_processing_policy and through the coverage view (this proves security_invoker); fixture covering every status plus an incoherent row; CHECK rejects an unknown status; backfill statement run twice changes nothing. CI note: dataplane-v2-ci.yml only runs Postgres-gated Rust tests for retrieval-engine-rs against an empty database; these tests silently skip without CR-N1 (CI wiring), so land CR-N1 right after this task.

#### CR-N1: CI wiring: real-migrator Postgres contract tests that fail instead of silently skipping

- design: manifest | plane: data | size: S | chain: C1 #2 | kind: ci
- depends_on: CR-T6
- gates: start [] done [] deploy []
- review source: added by this manifest
- verdict: NEW (the "CI wiring task" the contextual review referenced under missing tasks).
- files:
  - .github/workflows/dataplane-v2-ci.yml (add a Postgres 16 service job; REQUIRE_CONTRACT=1)
  - apps/Data Plane v2/tests/scripts/run-postgres-contract-tests.sh (new; applies every migration through tools/migrator, then runs the gated crate tests as dataplane_app)
- corrections (folded in):
  - dataplane-v2-ci.yml only runs Postgres-gated Rust tests for retrieval-engine-rs against an EMPTY database: the CR status-machine, reconciler, policy and graph-state tests would silently skip. Wire a job that applies all migrations with the real migrator and runs the gated tests of embedding-engine-rs, graph-index-rs and data-quality-go, with REQUIRE_CONTRACT=1 so absence fails.
  - CI runs cargo clippy --workspace --all-targets -- -D warnings and RUSTFLAGS=-D warnings: keep the new job consistent.
  - Do not touch the Makefile (CR-T5/CR-T14 own it); add a new script and register it in the workflow.
- cross-design overlap: None.
- deploy: none
- live proof (none): CI run shows the gated tests executed (counts) rather than skipped.
- notes_for_implementer: Add the workflow job and the runner script; assert the number of executed gated tests is non-zero. Pre-existing tests keep running unchanged.

#### CR-T5: Data Plane shared crate inference-client-rs (token client, tenant posture, bearer cache, typed statuses)

- design: contextual (T5) | plane: data | size: M | chain: C1 #3 | kind: code
- depends_on: none (design had: T1, T3)
- gates: start [G-T3-CODE] done [] deploy [G-CP-P0B]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS. Two real conflicts with the integrator plan (token TTL vs cache margin, closed error enum lacks the 503 posture error).
- files:
  - apps/Data Plane v2/services/inference-client-rs/Cargo.toml (new)
  - apps/Data Plane v2/services/inference-client-rs/src/lib.rs (new)
  - apps/Data Plane v2/services/inference-client-rs/src/token.rs (new)
  - apps/Data Plane v2/services/inference-client-rs/src/status.rs (new)
  - apps/Data Plane v2/services/inference-client-rs/tests/real_policy.rs (new)
  - apps/Data Plane v2/Cargo.toml
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/inference_auth.rs (delete)
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/mod.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/video_caption.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/config.rs
  - apps/Data Plane v2/services/graph-index-rs/src/inference_auth.rs (delete)
  - apps/Data Plane v2/services/graph-index-rs/src/extractor.rs
  - apps/Data Plane v2/services/graph-index-rs/src/config.rs
  - apps/Data Plane v2/docker-compose.yml
  - apps/Data Plane v2/services/embedding-engine-rs/Cargo.toml
  - apps/Data Plane v2/services/graph-index-rs/Cargo.toml
  - apps/Data Plane v2/Cargo.lock (minimal refresh for the new member only)
  - apps/Data Plane v2/.env.example (line ~88 posture env documentation)
  - apps/Data Plane v2/Makefile (cross-plane-env list ~line 46; also add the env names owned by CR-T10: DATA_ORCHESTRATOR_COST_SERVICE_API_KEY and the graph-events key paths)
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/video_caption.rs (third call site, listed in the design file list already; must be converted)
  - apps/Data Plane v2/services/inference-client-rs/tests/fixtures/ (vendored sha256-pinned: T3 status golden, Control Plane registry slice, CP 503 contract)
- files removed from the design list:
  - apps/Data Plane v2/services/embedding-engine-rs/Dockerfile
  - apps/Data Plane v2/services/graph-index-rs/Dockerfile
- corrections (folded in):
  - TTL conflict: the integrator caps tenant-principal tokens at 60s (replacing the posture-version cache). "Refresh when under 60s TTL remain" then refreshes on every call. Use margin = min(60s, ttl/4) (15s for a 60s token), so reuse is at most 45s, and document that as the staleness bound.
  - Never hold one bearer across a whole document: fetch from the cache per provider call (large documents outlive 60s; today one bearer per document silently expires mid-document).
  - Cache key must be (org_id, reason): the client asserts claims.reason equals the requested reason and reasons now differ per feature (contextual-retrieval, graph-extraction, graph-community-summary, video-caption).
  - The closed decoder enum lacks the CP wave's 503 reason retention_posture_unavailable. Add PostureUnavailable (maps to pending, retry 5 minutes, never skip, never send). TokenResponse currently has deny_unknown_fields: relax it so the CP wave can add response fields without breaking every client.
  - RetentionPosture gains Tenant: accept claims.zdr either value, expose bearer.zdr(); keep persistent and zdr strict. Ship clients first (env still persistent), flip posture env to tenant, then the registry row; never flip the registry before auth-core has the tenant code (a registry with tenant makes the old validator reject the whole file).
  - Env names differ per service and must be handled separately: embedding-engine MODEL_PLANE_EMBEDDING_INFERENCE_RETENTION_POSTURE (compose ~759), graph-index MODEL_PLANE_INFERENCE_RETENTION_POSTURE (~984), .env.example:88, Makefile cross-plane-env list (line 46).
  - The Dockerfiles listed are unnecessary (COPY services/ services/ already picks up a new crate and the crate needs no Model Plane proto); the embedding-engine and graph-index Cargo.toml and the workspace Cargo.lock are what is missing.
  - The real-policy test must read a vendored, sha256-pinned copy of the principal registry slice (the Control Plane file is dirty and outside the Data Plane build context) and must FAIL, not skip, when REQUIRE_CONTRACT or CI is set.
  - The retrieval-engine third copy (embed/service_auth.rs, 650 lines) stays out of this task (QB edits retrieval-engine); follow-up convergence is DEF-CR-RE-CLIENT after QB-A.
  - Evidence anchors: embedding-engine provider/mod.rs is cited at 175-181 and 496-501; editing its imports shifts them. Re-run check-citations.mjs.
  - Audit AZURE_OPENAI_API_KEY in the embedding-engine env (compose ~827): remove it if nothing but the deleted direct path used it (see CR-T12 for graph-index).
- cross-design overlap: None direct. QB T08/T15 edit the retrieval-engine compose block; keep one block per service per commit in docker-compose.yml. CR-T5 touches graph-index (extractor.rs, config.rs): it must be complete before CR-T12 (chain C2) starts (gate G-T5-DONE).
- deploy: S3 - data: image rebuild of embedding-engine and graph-index (launcher). Config: posture env flip in compose/.env only AFTER the control stage (auth-core image first, then registry): enablement step S4.
- live proof (none): Free (mint only, status and decoded zdr claim printed). Replace "audit event count increases by 1" with an in-service cache hit/miss counter or log line; the audit count cannot be read cheaply and a 60s TTL makes the window short.
- notes_for_implementer: Build crate services/inference-client-rs (workspace member; create the directory with a compiling skeleton BEFORE adding the member line to the workspace Cargo.toml): token.rs (mint, validate_bundle, org+reason keyed cache with margin min(60s, ttl/4), single-flight per key so concurrent chunks do not stampede auth-core), status.rs (decode tonic::Status into ZdrUnavailable, ResidencyUnavailable, RateLimited{retry_after}, Transient, Auth, InvalidArgument, TooLong, PostureUnavailable, Other using code plus x-mp-provider-error and x-mp-retry-after-ms; never match on message text; log code and kind). Convert embedding-engine (provider/mod.rs, contextualize.rs, video_caption.rs) and graph-index (extractor.rs); delete the two inference_auth.rs copies; fix graph-index's anyhow!(status).context(..) so the gRPC code is logged. Keep the 300s MAX lifetime check, add the 60s-capped case. Traps: reason is asserted equal on the claim; TokenResponse deny_unknown_fields; standalone_startup (APP_ENV=standalone) lets credentials be absent and must keep working; CR/graph must treat PostureUnavailable as defer, not as failure. Contract tests with REAL rules: (1) validate_bundle matrix on real RS256 JWTs captured from the real auth-core mint (not hand-built claims) for persistent, zdr and tenant with both claim values (the tenant cases can only be captured after CP wave P0b is deployed: until then those fixtures are pending and the tests fail under REQUIRE_CONTRACT; code can proceed, but do not call the task proven before the tenant tokens are captured), wrong reason, wrong org, TTL 60 and 300, expiry mid-use refresh; (2) decoder tests that consume the sha256-pinned copy of the CR-T3 golden, so the client is tested against what the server emits; (3) a test that loads the vendored registry slice and asserts audience inference-core, scope inference:invoke, retention compatible with the compose posture env, failing when the file is absent and REQUIRE_CONTRACT is set; (4) a compose contract that asserts the two env names above equal the registry value. Data Plane CI runs cargo clippy --workspace --all-targets -- -D warnings and cargo fmt --check: keep both clean.

#### CR-T7: Contextualizer v2: outcomes, ctx-v2 prompt, windowing, validation, usage capture, floors

- design: contextual (T7) | plane: data | size: L | chain: C1 #4 | kind: code
- depends_on: CR-T5, CR-T6 (design had: T5, T6)
- gates: start [] done [] deploy [G-CP-DEPLOYED, G-T3-DEPLOYED, G-OPS-ORG]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS. Cannot ship without CR-T8 (the outcome type ripples into the batch pipeline): review as two units, deploy as one.
- files:
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/contextualize.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/context/mod.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/context/prompt.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/context/window.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/context/validate.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/context/outcome.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/config.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/main.rs
  - apps/Data Plane v2/services/embedding-engine-rs/Cargo.toml (sha2 workspace dependency for input_hash)
  - apps/Data Plane v2/services/embedding-engine-rs/src/stream/mod.rs (constructor change only; the rest of the file belongs to CR-T8)
- corrections (folded in):
  - stop_reason: the design says require end_turn. Real Azure returns stop and length (inference-core openai.rs keeps the raw finish_reason). Accept {stop, end_turn}, treat {length, max_tokens} as truncated, content_filter may arrive as an HTTP error instead. Capture one REAL Azure response (during the live proof) as the golden; the repo's MockProvider uses end_turn and would hide this exact defect.
  - There is no cache-control or no-store field in InferRequest (checked inference.proto). Drop that requirement. The inference-core cache is in-memory, 5 minutes, tenant scoped and skipped for ZDR; a hit is free, so just skip metering when provider_timing.response_cache_hit is true.
  - Floors: provider_hint azure-openai (underscore and hyphen both match) and min_residency eu pass today only because AZURE_OPENAI_REGION=swedencentral is set; deployment type is unset so the claim is label-based. Do NOT set AZURE_OPENAI_DEPLOYMENT_TYPE without reading the real Azure deployment (a wrong value is a false residency claim). Add a contract test over the real compose env so a missing region variable cannot hard-fail every call silently, and make the retry honour x-mp-retry-after-ms (the azure hint removes failover during Azure 429s).
  - Bearer: take it per provider call from the CR-T5 cache; request.zdr comes from bearer.zdr(); PostureUnavailable (CP 503) maps to pending with next attempt in 5 minutes, never to skip and never to a send.
  - Prefix-cache proof is a measurement, not a gate: Azure prefix caching needs a 1,024-token prefix and is best effort, so record cache_read_input_tokens and do not fail the proof when 0.
  - Add sha2 (workspace dependency) to embedding-engine-rs/Cargo.toml for input_hash; stream/mod.rs:100 constructs the Contextualizer and changes with the new outcome API (CR-T8 owns the rest of that file).
  - The comments "Deliberately NOT tightened here" (contextualize.rs:414-423) and "This path never carries restricted content" (:400-404) are anchors in docs/compliance/evidence AND in the Frontend trust-claims.ts (anchor string). Rewriting them breaks check-citations.mjs and trust-data.test.ts: update both in the same release or keep the anchor text.
- dirty/untracked collisions: none (Data Plane tree clean); adjacent: apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-claims.ts (untracked, trust-edits track) anchors on this file
- cross-design overlap: None direct in files. The trust-edits track (Frontend trust-claims.ts, docs/compliance/evidence) anchors on contextualize.rs comments.
- deploy: S3 - data: embedding-engine image rebuild (together with CR-T8). ctx-v2 stays dark behind env or org-policy pin; no config flip.
- live proof (tiny): Needs a real throwaway org (see OPS-ORG; the old eval fixture orgs are gone and the tenant posture refuses unknown orgs). 3 gpt-4o-mini calls (about 0.002 USD) shared with CR-T8 and CR-T10. Not a chat test, so Terra is not involved (the CR model is the feature's own paid gpt-4o-mini). Say how many calls were made.
- notes_for_implementer: Build src/context/{mod,prompt,window,validate,outcome}.rs and reduce provider/contextualize.rs to the client. ContextOutcome::{Ready{text,usage,model_used,provider_used,residency,request_id}, SkippedZdr, ResidencyUnavailable, SkippedPolicy, SkippedBudget, Pending{retry_after}, Transient{retry_after}, Permanent{class}} mapped only from CR-T5's typed decoder; error_class values are the closed enum from the design (add zdr_org vs zdr_document so a restricted document and a ZDR org are distinguishable). Validation: normalise whitespace, strip tags, 12-600 chars, stop reason as above (length: cut at last full sentence if 20 chars or more, else invalid_output). Window: whole document up to 32,000 chars, else fixed 32,000-char sections with 4,000 overlap and a constant header (title plus first 2,000 chars); all slicing char-safe. Metering fields are only captured here (usage, cache_read, response_cache_hit); emission is CR-T10. Per-document and per-batch deadlines stay well under JetStream ack_wait 60s and the 120s envelope TTL. Traps: do not log prompts, contexts or provider bodies; Ok(None) for an empty answer must become an explicit invalid_output outcome (today it is silent); gpt-5 family models need reasoning settings, do not swap models here. Contract tests with REAL rules: prompt goldens (ctx-v1 byte-identical to today; ctx-v2 with a Norwegian chunk); outcome mapping table driven from the pinned CR-T3 golden; validation using the captured real Azure response (finish_reason stop, plus a length case); a retry test proving the delay equals the retry-after trailer; a counting double proving a ZDR bearer makes zero provider calls; a compose-env test that the effective residency floor and region variable are set together.

#### CR-T8: Batch pipeline: write-ahead context, vector coherence, reuse, inline deadline, cross-document concurrency

- design: contextual (T8) | plane: data | size: L | chain: C1 #5 | kind: code
- depends_on: CR-T7 (design had: T7)
- gates: start [] done [] deploy [G-T6-APPLIED]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS. Core of the owner's "CR must work" ruling; deploy with CR-T7 as one change set.
- files:
  - apps/Data Plane v2/services/embedding-engine-rs/src/batch/mod.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/stream/mod.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/qdrant_writer/mod.rs
- corrections (folded in):
  - process_batch deliberately mixes orgs in one buffer and runs on the superuser pool (RLS inert); every new UPDATE/CAS must carry an explicit org_id predicate (the existing persist_chunk_context and mark_units_done key on knowledge_id only), and group bearer, policy and posture by org.
  - Keep D18: the batch pipeline must not write embedding_status=failed; terminal failure marking stays in stream::run_consumer. Context failures are context_status values, never embedding failures.
  - Write embedding_model and embedded_context_hash in the same statement as mark done; embedded_context_hash is sha256 of the stored trimmed chunk_context and NULL for a raw vector (a unit embedded raw while its context is still pending must not claim coherence).
  - Stale-context detection: exact-reuse chunks keep ids and vectors and get no units.created event (index-engine builder/mod.rs:91-135), and documents has no content hash column. Do not read documents.content for every ready row: select candidate documents with documents.updated_at newer than the unit's context_generated_at, then compare input_hash.
  - Inline deadline (8s per document, 20s per batch) must stay far below ack_wait 60s and the 120s envelope TTL; a redelivered message must regenerate nothing (write-ahead context plus input_hash reuse).
  - documents.indexed (JetStream DATAPLANE_GRAPH, consumed durably by quickwit-adapter) is published after the context write already (step 3b before step 4); keep that order, it is the lexical sync. Do not add a new subject (see CR-T9a).
  - Reuse this code from the reconciler: build BatchItems from DB rows and call the same pipeline, one implementation only.
  - The Qdrant payload must stay unchanged in this task (the live proof asserts it); QB-M1b adds the source key afterwards (update that assertion there, not here).
- cross-design overlap: None. CR-T10 also edits batch/mod.rs (producer hook): CR-T8 first (same chain C1, T10 later).
- deploy: S3 - data: embedding-engine image rebuild (with CR-T7). CONTEXT_RECONCILER_ENABLED and CONTEXT_REFRESH_ON_PROMPT_CHANGE stay 0 at first deploy. Re-run the idempotent CR-T6 backfill at this deploy.
- live proof (tiny): No extra spend beyond CR-T7's 3 calls, BUT the re-embed via reembed-switch makes Cohere embed calls (3 chunks): the assertion is zero new LLM calls, not zero embed calls. State the count.
- notes_for_implementer: Rewrite contextualize_items and process_batch around the invariants: read persisted (status, input_hash, chunk_context) and reuse on hash match; generate within the deadline using bounded cross-document concurrency; then in order: UPDATE context plus provenance plus status ready (write-ahead, CAS on input_hash, explicit org_id); embed compose(persisted context, text); upsert Qdrant; mark done with embedding_model and embedded_context_hash; set pending with context_next_attempt_at=now for anything unfinished and embed it raw; publish documents.indexed; a failed regeneration never overwrites a previous ready context. ZDR: restricted document or ZDR bearer gives skipped_zdr with zero provider calls and a distinct error_class. Traps: process_batch is called from stream/mod.rs:379 with items from several orgs; JetStream may redeliver; the cost event key embed.cost is shared across (org,user,zdr) groups in a batch (CR-T10 fixes the idempotency, do not touch it here). Contract tests with REAL rules: Postgres 16 with the real migrator-applied schema, a fake provider and fake Qdrant that only record calls, assertions I1 (vector input equals compose(persisted context, text)), I2 (provenance in the same statement), I3 (every eligible unit ends non-NULL), I4 (a provider sleeping past the deadline leaves that tenant's units pending while another tenant's items embed in the same batch; use tokio::time::pause), I7 (second run makes zero provider calls), failure injection between write-ahead, embed, upsert and mark-done each converging after retry, regeneration failure keeping the old ready context and vector; run the reconciler-facing queries inside the real pg-org-scope helper as dataplane_app to prove the explicit org predicates.

#### QB-M1b: Embedding-engine: stamp documents.source into the Qdrant payload and backfill existing points (M1b)

- design: manifest | plane: data | size: M | chain: C1 #6 | kind: code
- depends_on: CR-T8
- gates: start [] done [] deploy [G-T8-GREEN]
- review source: added by this manifest
- verdict: NEW (reconstructed by this manifest; second half of the M1 headline "stamped source payload").
- files:
  - apps/Data Plane v2/services/embedding-engine-rs/src/qdrant_writer/mod.rs (payload gains source)
  - apps/Data Plane v2/services/embedding-engine-rs/src/batch/mod.rs (payload build; documents.source join if absent)
  - apps/Data Plane v2/services/embedding-engine-rs/src/bin/qdrant_backfill_source.rs (new bounded admin CLI: Qdrant set_payload by document_id, --dry-run, --max-points)
  - apps/Data Plane v2/services/embedding-engine-rs/Cargo.toml and Dockerfile ([[bin]] and COPY)
- corrections (folded in):
  - CR-T8 asserts the Qdrant payload is unchanged; this task changes it AFTER CR-T8 is deployed and green and updates that assertion here, not in CR-T8.
  - Backfill the existing 207 points (set_payload keyed by document_id) with a dry run first; no re-embedding and no provider call. Verify a point count by source before and after.
  - Sparse index (Quickwit) is not changed by this task.
- cross-design overlap: Same crate and files as CR-T8/CR-T10: sequential in chain C1.
- deploy: S4 - data: embedding-engine image; run the backfill CLI once after deploy.
- live proof (none): Free: counts by source in Qdrant equal Postgres counts by documents.source; no provider calls (state 0).
- notes_for_implementer: Stamp source from documents.source at write time; add the backfill CLI with --dry-run/--max-points; tests with a fake Qdrant that only records calls: new points carry source, the backfill is idempotent and only touches points whose payload lacks source, org_id is never changed.

#### CR-T11: Org processing policy admin API, coverage endpoint and release gates

- design: contextual (T11) | plane: data | size: M | chain: C1 #7 | kind: code
- depends_on: CR-T6, CR-T8 (design had: T6)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest).
- files:
  - apps/Data Plane v2/services/data-quality-go/internal/handler/quality.go
  - apps/Data Plane v2/services/data-quality-go/internal/handler/policy.go (new)
  - apps/Data Plane v2/services/data-quality-go/internal/policy/store.go (new)
  - apps/Data Plane v2/services/data-quality-go/internal/gates/checker.go
  - apps/Data Plane v2/services/data-quality-go/internal/gdpr/postgres_purger.go (purge org_processing_policy rows; moved from CR-T6)
  - apps/Data Plane v2/services/embedding-engine-rs/src/policy.rs (new; wired into the pipeline)
  - apps/Data Plane v2/services/embedding-engine-rs/src/main.rs (module wiring only)
  - apps/Data Plane v2/openapi/d4d5.yaml (the openapi directory holds one file)
- corrections (folded in):
  - Per-org off is an operator-only, audited, reason-required control (admin scope data:quality:admin, org id validated). Owner ruling 2: CR is NOT switched off; off exists for the eval A/B baseline orgs and incidents, always with an explicit skipped_policy status and marker, never silent, no org-admin UI (OD7 option a). Whether it may be used on customer orgs is OD-PER-ORG-OFF.
  - embedding-engine policy reader: 30s cache; hard fallback to inherit/ON only when the table is unreachable AND the global flag is on (never when the org policy says off). A missing row means ON.
  - The sparse_index_in_sync gate cannot read Quickwit from data-quality-go (no access): consume the read-only parity endpoint added by CR-T9a through a vendored response golden; until the endpoint exists the gate reports unavailable (never pass).
  - contextual_coverage_ok counts status legacy as eligible-but-not-ready until the verification pass runs; excludes explicit skipped_policy/skipped_zdr rows; threshold 99% configurable.
  - org_processing_policy RLS is created in CR-T6; here add the table to the org purge in postgres_purger.go (the purger lists tables explicitly) with its test; tests that touch RLS must run as dataplane_app.
  - The openapi directory contains the single file d4d5.yaml; edit that file, not the directory. data:quality:admin already exists in handler/quality.go and routes_auth_test.go; reuse it, do not add a scope.
- cross-design overlap: CR-T14 (chain C11) later edits data-quality-go internal/eval/store.go: gated after this task.
- deploy: S3 - data: data-quality-go and embedding-engine images.
- live proof (none): No paid call. Set the throwaway eval org to off and confirm a synthetic ingest makes zero provider calls (inference-core counter from CR-T3 unchanged) and units are skipped_policy; set back to inherit and confirm they become pending then ready via CR-T9a. Query the coverage endpoint for the live org and see 0 ready + 206 legacy + 1 pending, then 207/207.
- notes_for_implementer: Add PUT/GET /v1/admin/orgs/{org}/processing-policy (scope data:quality:admin, audited, org id validated), GET /v1/quality/context-coverage backed by v_context_coverage, and two release gates: contextual_coverage_ok and sparse_index_in_sync. Tests first: authorization (admin scope required, cross-org write refused), RLS isolation as dataplane_app, default row semantics (absent row means ON), the kill switch honoured by the embedding-engine policy reader (off means zero provider calls), cache expiry, and gate unit tests in both directions (a fixture with 98% coverage fails; a skipped_policy org passes; a Quickwit count mismatch fails; an unavailable parity endpoint is unavailable not pass).

#### CR-T10: Usage ledger v2: shared event crate, durable stream, cost_events columns, forwarder to cost-core, ZDR numeric-only, graph signer

- design: contextual (T10) | plane: data | size: L | chain: C1 #8 | kind: code
- depends_on: CR-T8, CR-T6 (design had: T2, T4, T6)
- gates: start [] done [] deploy [G-CP-P1]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest: the original review was cut off). Largest cross-cutting task; verifier and consumer must ship before any producer.
- files:
  - apps/Data Plane v2/Cargo.toml (workspace member usage-ledger-rs; create the skeleton directory first)
  - apps/Data Plane v2/Cargo.lock (minimal refresh)
  - apps/Data Plane v2/services/usage-ledger-rs/Cargo.toml (new)
  - apps/Data Plane v2/services/usage-ledger-rs/src/lib.rs (new)
  - apps/Data Plane v2/services/event-envelope-rs/src/lib.rs (closed subject-per-scope table lib.rs:432-455: add the cost stream subject and a graph scope)
  - apps/Data Plane v2/infra/nats/SUBJECTS.md
  - apps/Data Plane v2/scripts/check-subjects.sh
  - apps/Data Plane v2/services/embedding-engine-rs/src/batch/mod.rs (producer hook; fixes the shared embed.cost idempotency key at ~185-216)
  - apps/Data Plane v2/services/graph-index-rs/src/extractor.rs (signer wiring only; metering emission is CR-T12)
  - apps/Data Plane v2/services/graph-index-rs/src/main.rs (signer wiring only)
  - apps/Data Plane v2/services/data-orchestrator-go/internal/cost/consumer.go
  - apps/Data Plane v2/services/data-orchestrator-go/internal/cost/verifier.go
  - apps/Data Plane v2/services/data-orchestrator-go/internal/cost/forwarder.go (new)
  - apps/Data Plane v2/services/data-orchestrator-go/internal/cost/consumer_security_test.go
  - apps/Data Plane v2/services/data-orchestrator-go/cmd/main.go
  - apps/Data Plane v2/services/data-orchestrator-go/internal/config/config.go (residual from CR-T2: DATA_ORCHESTRATOR_COST_SERVICE_API_KEY)
  - apps/Data Plane v2/services/data-quality-go/internal/cost/query.go
  - apps/Data Plane v2/infra/postgres/migrations/20261003140000_cost_events_v2.sql (new; unique timestamp after CR-T6)
  - apps/Data Plane v2/infra/postgres/migrations/20261003140000_cost_events_v2.down.sql (new)
  - apps/Data Plane v2/infra/postgres/init.sql (cost_events mirror only)
  - apps/Data Plane v2/docker-compose.yml (data-orchestrator block, key mounts, stream; one block per service)
  - apps/Data Plane v2/docker-compose.cross-plane.yml (residual from CR-T2)
  - apps/Data Plane v2/services/data-orchestrator-go/testdata/ (authored forwarder request golden, sha256-pinned; CR-T4 vendors it)
- corrections (folded in):
  - Verifier changes first: per-producer event types become a SET (today one event_type per producer: embedding for embedding-engine, rerank for retrieval-engine, verifier.go:21-33,111-135,232-262), strict decode kept (DisallowUnknownFields), deterministic idempotency key = hash of sorted request ids (no document ids).
  - The shared envelope crate has a closed subject-per-scope table (event-envelope-rs lib.rs:432-455): add DATAPLANE_COST subject and a graph scope there, plus SUBJECTS.md and check-subjects.sh (lint).
  - Durability: today Consumer.Start uses nc.Subscribe on core NATS dataplane.cost.ledger (non-durable): producers publish to a new JetStream stream DATAPLANE_COST with PubAck, data-orchestrator consumes with a durable consumer; dual-publish to core NATS for exactly one release; keep schema_version 1 accepted.
  - Existing bug (inference from code, not exercised): one embed.cost idempotency key is shared by all (org,user,zdr) groups in a batch (embedding-engine batch/mod.rs:185-216) while the persist step prefixes only the org (consumer.go:149-164), so a second user in the same org is silently dropped. Fix with per-(org,user,zdr,batch) keys in this task.
  - ZDR numeric-only: existing convention drops ZDR events (consumer.go:88-90, embedding-engine drop). Resolved without the owner (matrix row 87 allows ZDR/cost metadata only): record content-free count rows with zdr=true and record the convention in the ledger (CR-T16). Skip metering when response_cache_hit is true.
  - Forwarder (data-orchestrator): loop posts unforwarded rows to cost-core /api/v1/cost/record with per-org tokens (principal data-orchestrator, cost:write on audience cost-core, provided by CP wave P1). Idempotency key must embed the org (cost-core unique index is global). Ship DARK: USAGE_FORWARDER_ENABLED=0; do not send provider_used until the other session's cost-core build is deployed; send feature and units only after CR-T4 is deployed (DisallowUnknownFields).
  - Residual from CR-T2 (CP wave covers Control-side registry rows, launcher mint and validators; this task owns the Data Plane side): data-orchestrator config.go and compose env for DATA_ORCHESTRATOR_COST_SERVICE_API_KEY; the graph-events RSA key pair (private key mounted into graph-index as signer, public key into data-orchestrator as verifier). Production event keys cannot live under .secrets/event-keys (DATA_PLANE_STATUS.md:90: the production preflight rejects that path) and event keys are provisioned by hand through .env and the Makefile list, not by the launcher: document, do not assume. The Makefile cross-plane-env hunk for these names is made by CR-T5 (same chain).
  - The migration timestamp is 20261003140000 (after CR-T6's 20261003130000); quarry T07's 20261003120000 migration does not exist. cost_events DDL lives in BOTH init.sql and migrations: mirror it.
  - Rust crates and Go modules edited here: no overlap with graph-index hardening (CR-T12 owns store/stream/reconcile); CR-T10 touches graph-index only for the signer so CR-T12 can emit.
- dirty/untracked collisions: none in the Data Plane tree; adjacent: cost-core dirty files (CR-T4) and CP wave principal files (read-only)
- cross-design overlap: Quarry T07/T16 merged here (producer/units dimension, deterministic keys). QB-M later switches the rerank producer to the v2 builder. Z-06b (ZDR turn metering in model-gateway) deferred.
- deploy: S3 - data: data-orchestrator-go and data-quality-go images (consumer/verifier FIRST), then producers (embedding-engine hook, graph-index signer) with USAGE_EMIT_ENABLED=0; emission enabled at S4 after the consumer is verified. Forwarder enabled only after cost-core CR-T4 is deployed and priced (S4).
- live proof (none): Uses the CR-T7/T8/T12 paid calls (no extra spend): after the synthetic ingest, cost_events rows (feature contextual-retrieval) exist with exact input/output/cache_read tokens; forwarded_at set within seconds; cost-core GET /api/v1/cost/aggregate?feature=contextual-retrieval for the test org matches the sums. Kill data-orchestrator mid-run and confirm no event is lost after restart.
- notes_for_implementer: Create the shared usage-ledger-rs crate FIRST (schema_version 2: org, feature, model_used, provider_used, input/output/cache_read tokens, calls, cost_basis, estimated, idempotency key; no document ids; built directly from InferResponse), with a compiling skeleton in place before adding the workspace member line. Then the Go verifier/consumer (set of event types per producer, third issuer graph-index with its key), the durable JetStream consumer, the migration and the forwarder (dark). Author the forwarder request golden (exact JSON body posted to cost-core /api/v1/cost/record) under data-orchestrator-go/testdata with its sha256; CR-T4 vendors and verifies it. Tests first: signature/replay/scope tests for the new issuer (extend consumer_security_test.go); a ZDR-flagged event is persisted with counters only; JetStream durability (publish while the consumer is down, restart, exactly one row); forwarder tests against an httptest cost-core stand-in that applies the REAL rules (unknown field -> 400, org-embedded key, scope cost:write) plus the golden; token cache per org; event-builder tests proving exact token fields are copied from a captured real InferResponse and that cached responses emit zero; second user in the same org not dropped. Re-run check-citations.mjs for every evidence doc citing the edited Go/Rust files.

#### CR-T9a: Context reconciler, backfill/repair CLI, legacy verification pass and Postgres-vs-Quickwit parity repair (T9a)

- design: contextual (T9); parent: CR-T9 | plane: data | size: L | chain: C1 #9 | kind: code
- depends_on: CR-T8, CR-T11, CR-T10 (design had: T8, T10, T11)
- gates: start [] done [] deploy [G-T8-GREEN]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: REWORK of the original T9 (split into T9a here and T9b budget gate). The new core-NATS subject is the wrong mechanism, the repair path named is unreachable, the budget default contradicts an owner default, and legacy rows need a verification pass the task never scheduled.
- files:
  - apps/Data Plane v2/services/embedding-engine-rs/src/context_reconciler.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/bin/context_backfill.rs (new)
  - apps/Data Plane v2/services/embedding-engine-rs/src/bin/backfill_reannounce.rs (reuse the signed documents.indexed publisher; ensure a --document-id flag)
  - apps/Data Plane v2/services/embedding-engine-rs/src/main.rs
  - apps/Data Plane v2/services/embedding-engine-rs/Cargo.toml ([[bin]] context-backfill)
  - apps/Data Plane v2/services/embedding-engine-rs/Dockerfile (cargo build --bin context-backfill and COPY, or the binary never reaches the image)
  - apps/Data Plane v2/docker-compose.yml (reconciler env, embedding-engine block only)
  - apps/Data Plane v2/services/quickwit-adapter-rs/src/api.rs (read-only parity endpoint; the adapter owns Quickwit access, data-quality has none)
  - apps/Data Plane v2/scripts/rebuild-quickwit.sh (document as UNUSABLE: no bearer, CLEAR=true default; do not extend)
- corrections (folded in):
  - Drop dataplane.knowledge.units.contextualized (core NATS, non-durable, needs a new verifier scope in event-envelope-rs and a SUBJECTS.md row). The durable lexical path already exists: documents.indexed rides JetStream DATAPLANE_GRAPH, quickwit-adapter has a durable pull consumer and re-indexes the whole document with its chunk_context (rebuild.rs index_document_knowledge_units). The reconciler re-publishes documents.indexed (same signed publisher as backfill-reannounce) after healing a document. That also removes event-envelope-rs/src/lib.rs, SUBJECTS.md and check-subjects.sh from this task.
  - The Quickwit repair via rebuild is unreachable: /admin/rebuild needs scope data:search:rebuild plus a second approval scope, no principal in plane-service-principals.json holds them (the spec says never grant it to users), scripts/rebuild-quickwit.sh sends no bearer and defaults to CLEAR=true (destructive), and quickwit_admin_jobs has 0 rows ever. Repair with the backfill-reannounce path per document (the unit count missing is 155, not 150: 151 private SharePoint, 3 org SharePoint, 1 Quarry).
  - Re-announcing documents.indexed also triggers graph-index extraction (re-pays LLM calls until CR-T12 lands) and meilisearch clear-then-reindex: until CR-T12 is deployed announce only named documents (--document-id), never the whole org (whole-org announce only after CR-T12 is deployed).
  - Budget default must be alert-only (owner default D-E: configurable per org and global, default alert-only, no stop). skipped_budget exists only when budget_mode=stop is explicitly set. The design's $5/month stop default is superseded. The budget gate itself is CR-T9b.
  - Add the legacy verification pass: status legacy (CR-T6) is moved to ready by re-embedding the persisted context (zero LLM calls), stamping embedding_model and embedded_context_hash. Cost: one Cohere embed per legacy unit (206 live, roughly 60k tokens, well under a cent); state the call count in the proof.
  - Cross-org lease selection (FOR UPDATE SKIP LOCKED, org fair share) must run on the unscoped connection like the other cross-org workers; per-unit work then runs through pg-org-scope for the org. Select stale candidates through documents.updated_at newer than context_generated_at, not by reading every document.
  - The parity gate must use the same eligibility predicate as quickwit-adapter (embedding_status=done, document not deleted, zdr_classification in internal/public/sensitive), dedup by knowledge_id (this Quickwit rejects the cardinality aggregation; delete-by-query is eventual: measured 5,215 docs for 1,164 chunks), and read counts after merges.
  - Add the new [[bin]] context-backfill to embedding-engine-rs/Cargo.toml and the embedding-engine Dockerfile (cargo build --bin ... and COPY) or the binary never reaches the image.
  - Per-org off and skipped_budget conflict with the owner ruling that CR is not switched off: only operator-set, audited, and never silently skip (critic lean: throttle or defer).
- cross-design overlap: None direct. The ZDR design's derived-job exclusions (memory, dreaming) are Model Plane, not here.
- deploy: S3 - data: embedding-engine image rebuild (new bin and loop); quickwit-adapter image rebuild for the parity endpoint; compose env for the reconciler (CONTEXT_RECONCILER_ENABLED=0 first, enabled with a cap at S4).
- live proof (tiny): 1 gpt-4o-mini call (about 0.0002 USD) for the raw chunk, about 206 Cohere embed calls for the legacy verification (well under a cent), 0 further LLM calls. Run with --max-units and --dry-run first; announce documents by id only until CR-T12 is deployed. The Quickwit repair is a re-announce of the affected documents, NOT a rebuild.
- notes_for_implementer: Build src/context_reconciler.rs (spawned from main.rs): lease rows with FOR UPDATE SKIP LOCKED and context_lease_until, order by org fair share then document, apply the same pipeline as CR-T8 (reuse or generate, re-embed when embedded_context_hash differs from the hash of the persisted context, upsert, CAS update), then re-publish documents.indexed for each healed document. Per-org and global concurrency, jittered backoff honouring x-mp-retry-after-ms (cap 5 minutes), circuit breaker, kill-switch and org-policy re-check on every pass, retry ladder 1m/5m/30m/2h/12h then failed_permanent after 8 attempts with an alert (a posture or credential problem must not burn attempts). skipped_zdr rows are re-checked at most hourly by minting and reading bearer.zdr(). Prompt-version refresh only when CONTEXT_REFRESH_ON_PROMPT_CHANGE=1 and within an explicit cap. bin/context_backfill.rs is a thin wrapper (--org-id --dry-run --max-usd --max-units --target-version) and the dry run prints counts and estimated spend, no content. Parity: a function (and a read-only adapter endpoint with a stable JSON shape the CR-T11 gate vendors a golden of) that counts expected (the quickwit-adapter eligibility predicate) versus Quickwit chunks deduped by knowledge_id and context_body presence; compare after merges. The legacy verification pass moves legacy to ready with zero LLM calls. Tests first (disposable Postgres, run through the real migrator, as dataplane_app): the selector returns exactly the right rows for every status/policy/ZDR combination; two concurrent reconcilers never process the same unit (lease); an interrupted run resumes without repeating generation; a second full run makes zero provider calls; prompt refresh is off unless the flag is set and cost-capped when on; dry-run output contains counts and estimated spend but no content; the parity check detects a missing and a context-less document.

#### CR-T13: Defaults and config: CR ON by default, boot-time contract, .env.example, compose contract, dedicated deployment alias, stale-comment fixes

- design: contextual (T13) | plane: data | size: S | chain: C1 #10 | kind: code
- depends_on: CR-T7, CR-T9a (design had: T7)
- gates: start [] done [] deploy [G-T8-GREEN]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest).
- files:
  - apps/Data Plane v2/docker-compose.yml
  - apps/Data Plane v2/.env.example
  - apps/Data Plane v2/services/embedding-engine-rs/src/config.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/provider/contextualize.rs
  - apps/Data Plane v2/tests/scripts/compose-security-contract-test.sh
  - NOTE: the DATA-15 correction in docs/gap-data.md is made by CR-T16 (docs chain), not here
  - apps/Data Plane v2/tests/e2e/run-isolated-mvp.sh (set CR off explicitly: isolated stacks have no inference-core)
  - apps/Data Plane v2/tests/scripts/run-isolated-real-authority.sh (CR off env block only; QB-T08 edits its principal registry at ~164)
  - apps/Data Plane v2/tests/scripts/run-isolated-real-authority-browser.sh (CR off env)
  - apps/Data Plane v2/tests/scripts/isolated-mvp-contract-test.sh and real-authority-contract-test.sh (check only: keep green)
- files removed from the design list:
  - apps/Data Plane v2/infra/postgres/migrations/20260824120000_knowledge_unit_contextual_retrieval.sql
  - apps/Data Plane v2/docs/gap-data.md
- corrections (folded in):
  - Flipping CR to ON by default breaks the isolated stacks (tests/e2e/run-isolated-mvp.sh and the real-authority runners): they have no inference-core and the boot check would crash them. They must set CR off explicitly (via the incident kill switch plus its reason, or a dedicated test env).
  - Do not edit the already-applied 20260824120000 migration to fix its comment; the comment fix is a COMMENT ON COLUMN in CR-T6. Never edit the live git-ignored .env in place: document every new knob in .env.example.
  - The kill switch requires an explicit reason variable (CONTEXT_RETRIEVAL_KILL_SWITCH_REASON), logged at startup and reported by /readyz; the switch is incident-only (owner ruling 2: CR is not switched off).
  - Default model: gpt-4o-mini via provider hint azure-openai, EU residency floor; the dedicated background alias (OD4) is an operator action: support both, default to the shared deployment with caps and the circuit breaker until the alias exists.
  - The compose-security-contract-test.sh edit for CR defaults is owned here; other tasks (QB-T08, CR-T17) must add sibling scripts instead of editing that file.
  - contextualize.rs comment rewrites must keep the trust anchors ("Deliberately NOT tightened here", "This path never carries restricted content") or update trust-claims.ts and the evidence docs in the same release.
  - Order: flip the default only after the CR live proof is green and the reconciler exists (rollout step 7).
- cross-design overlap: compose-security-contract-test.sh also edited by QB-T08 and CR-T17 (they add sibling scripts instead).
- deploy: S6 - data: embedding-engine recreate with new compose defaults (via launcher; the live .env deliberately lacks CR keys for the proof).
- live proof (none): No paid call. Recreate embedding-engine through the launcher with the live .env deliberately lacking CR keys and confirm CR is still ON from the new defaults (startup log line). Flip the kill switch with a reason in a scratch environment and confirm the process logs it and /readyz reports it.
- notes_for_implementer: Flip CONTEXTUAL_RETRIEVAL_ENABLED to true by default in config.rs and compose, give the model a default, add every CR/reconciler/budget/usage knob to .env.example with its meaning, make startup fail loudly when CR is enabled without a model or credential (already true) AND when the global flag is off without an explicit kill-switch reason, rewrite the stale comments (OFF by default, contextualized_text), correct DATA-15 in gap-data.md. Tests first: config tests assert default ON and an explicit-reason requirement for turning it off; the compose contract asserts the compose default, the model default, the new env keys and that .env.example documents each; a docs lint ensures no file still says OFF by default for CR.

#### CR-T9b: Reconciler budget gate: alert-only by default, optional stop mode (T9b)

- design: contextual (T9); parent: CR-T9 | plane: data | size: M | chain: C1 #11 | kind: code
- depends_on: CR-T9a, CR-T10, CR-T11 (design had: T8, T10, T11)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW (split from T9 on the reviewer's suggestion). Budget gate after the metering foundation and policy table exist.
- files:
  - apps/Data Plane v2/services/embedding-engine-rs/src/context_reconciler.rs
  - apps/Data Plane v2/services/embedding-engine-rs/src/policy.rs
- corrections (folded in):
  - Owner default D-E: budget configurable per org and global; default alert-only (no stop); the owner supplies the figure later; do not invent a number.
  - skipped_budget only when budget_mode=stop is set explicitly by an operator; throttle or defer instead of silently skipping (critic lean); every skip is an explicit status plus a marker.
  - Spend is read from cost_events (feature contextual-retrieval) and the org_processing_policy budget; this is a Data Plane read, not cost-core. Do not couple to the cost-core chat budget (CR-T4 excludes non-chat features from that aggregate).
- cross-design overlap: None.
- deploy: S6 - data: embedding-engine image rebuild (late Data stage); env: budget figures unset => alert-only.
- live proof (none): No paid call: alert fires on a synthetic threshold in the throwaway org; no stop occurs in default mode; stop mode proven only on the throwaway org.
- notes_for_implementer: Add the budget check to the reconciler selector and the inline pipeline (shared function). Modes: alert_only (default; emit metric ctx_budget_exceeded_total and a structured log, keep processing), throttle (cap concurrency), stop (set skipped_budget with marker; only when budget_mode=stop is set on the org policy or globally). Tests: default mode never stops; stop mode stops before the next provider call; an org with no policy row inherits global; spend aggregation excludes other features; kill switch honoured.

### Chain C2 dp-graph

#### CR-T12: graph-index hardening: durable per-chunk state, backoff and poison cap, typed errors, metering, tenant posture, version stamps

- design: contextual (T12) | plane: data | size: L | chain: C2 #1 | kind: code
- depends_on: CR-T5, CR-T6, CR-T10 (design had: T5, T6, T10)
- gates: start [G-T5-DONE, G-T10-CRATE] done [] deploy [G-CP-P1, G-T10-CONSUMER-DEPLOYED]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest).
- files:
  - apps/Data Plane v2/services/graph-index-rs/src/extractor.rs
  - apps/Data Plane v2/services/graph-index-rs/src/reconcile.rs
  - apps/Data Plane v2/services/graph-index-rs/src/stream.rs
  - apps/Data Plane v2/services/graph-index-rs/src/store.rs
  - apps/Data Plane v2/services/graph-index-rs/src/community.rs
  - apps/Data Plane v2/services/graph-index-rs/src/config.rs
  - apps/Data Plane v2/infra/postgres/migrations/<timestamp>_graph_extraction_state.sql (new)
  - apps/Data Plane v2/docker-compose.yml (graph-index block: remove AZURE_OPENAI_API_KEY and endpoint, ~988)
  - apps/Data Plane v2/services/graph-index-rs/Cargo.toml
  - apps/Data Plane v2/tests/scripts/ sibling compose-contract script (graph key mount absent, no Azure key): do NOT edit compose-security-contract-test.sh
- files removed from the design list:
  - apps/Data Plane v2/infra/postgres/init.sql
- corrections (folded in):
  - Delete the azure_openai extractor backend (GRAPH_EXTRACTION_PROVIDER=azure_openai selects AzureExtractor: config.rs:19-25, extractor.rs:34,45,348) and drop AZURE_OPENAI_API_KEY and the endpoint from graph-index's compose env (~988); today flipping that one variable bypasses the token, the ZDR claim, metering and typed errors. Add a compose contract test (sibling script).
  - init.sql is not the convention for a new table (mirroring applies to cost_events only): do not edit init.sql; the graph_extraction_state migration needs a fixed unique timestamp after CR-T10's (20261003150000_graph_extraction_state).
  - RLS on graph_extraction_state; tests as SET LOCAL ROLE dataplane_app (services connect as a superuser and RLS is inert otherwise).
  - Use the shared client (tenant bearer, typed statuses, per-feature reason) from CR-T5; CR-T5 already converted graph-index to it: here add state, poison cap, backoff and metering emission through usage-ledger-rs (CR-T10 wired the signer).
  - PostureUnavailable (CP 503) means pending/defer, never a failure and never a skip; ZDR tenant gives skipped_zdr with zero provider calls and a marker; the typed gRPC code must appear in the structured log (graph-index logged %e and hid it: the 8 live "model-plane inference failed" lines).
  - DLQ publish regardless of verifier mode; stamp model and prompt version into provenance; poison cap 6 attempts, backoff 5m to 6h; empty result durable across restart (live repro: document cefaf22d); community summarisation obeys budget (alert-only default).
  - Trust anchors: extractor.rs:286-297 is cited by the trust copy; update trust-claims.ts and the evidence docs in the same release (check-citations.mjs).
  - Rotation: GRAPH_INDEX_GDPR_NATS_PASSWORD was printed in a critic transcript: operator action OPS-SECRETS, not code.
- cross-design overlap: Depends on CR-T10 (signer, ledger crate). Re-announcing documents.indexed triggers graph extraction: CR-T9a announces named documents only until this task is deployed.
- deploy: S3 - data: graph-index image rebuild; migration applied by the Data stage hook; emission env stays dark until S4.
- live proof (tiny): One paid call (about 0.0005 USD): extract one org-visible chunk in the throwaway org; verify the usage event (feature graph-extraction), state row ready with model and prompt version, and that restarting graph-index makes no new call for an empty document. Confirm the live org's cefaf22d document is now durably empty (no repeats).
- notes_for_implementer: Add graph_extraction_state (per org+chunk: status pending/ready/empty/skipped_zdr/skipped_policy/skipped_visibility/skipped_budget/failed_transient/failed_permanent, attempts, next_attempt_at, prompt_version, model, extracted_at, entity_count, error_class) with RLS. Replace the in-memory Suppressed set and the NOT EXISTS candidate scan with state-driven selection, exponential backoff (5m to 6h) and a poison cap of 6 attempts. Tests first, built on the live repro: a chunk that fails 10 times in a row stops being retried after the cap and is reported failed_permanent; backoff table; an empty result is durable across a simulated restart (zero provider calls on the next pass); a ZDR bearer or skipped policy makes zero calls and records the marker; community summarisation obeys budget; the typed gRPC code appears in the log fields (assert on the structured event); DLQ publish happens with a verifier configured; the compose contract proves graph-index has no Azure key. Visibility rule unchanged.

### Chain C3 dp-retrieval

#### QB-T03: Retrieval engine: principal-aware auth with a default-deny service route allowlist (fixes the 403)

- design: quarry (T03) | plane: data | size: M | chain: C3 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: KEEP, with corrections. The right fix for D1 and independent of P1 (quarry-edge already holds data:read), but the corpus-scope part moves to QB-M1a, and the test strategy must exercise the real admission function.
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/authz/context.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/authz/principal.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/authz/service_routes.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/authz/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/api/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/grpc/interceptor.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/audit/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/metrics/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/service_principal_auth.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/fixtures/auth-core/data-plane-service-token-claims.golden.json
  - apps/Data Plane v2/services/retrieval-engine-rs/src/lib.rs (module declarations)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/main.rs (the bin re-declares every module)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs (skip viewer-grant resolution for service principals; granted_docs site ~997)
  - apps/Control Plane/auth-core/src/auth/ new jest spec + fixtures regenerating the two goldens from the real ConvexTokenService.issuePlaneToken and buildDataPlaneDecision (NEW files only; do not edit the dirty specs)
- files removed from the design list:
  - apps/Data Plane v2/contracts/ (any repo-root contracts/ directory: not in the Data Plane Docker context)
- corrections (folded in):
  - The api module is declared only in main.rs (mod api;), not in lib.rs, so an integration test cannot reach auth_middleware. Put the WHOLE admission decision (service branch, membership branch via the PolicyClient trait, header-org check) into a lib function authz::principal::admit(claims, matched_route, org_header, policy) and make api::auth_middleware a thin wrapper; also move decode_verified_claims/strict_validation_for (api/mod.rs ~240-330) into authz so tests run the real JWT validation including required claim zdr. Any new module must be declared in BOTH lib.rs and main.rs.
  - auth_middleware writes audit rows through crate::audit::record_access(&state.pool, ...) (PgPool, org-scoped transaction), so a hermetic test of the wrapper needs an AuditSink trait; keep the DB-backed impl as a thin adapter around record_access.
  - Key the route table on axum MatchedPath (available in route_layer middleware), not the raw URI path, and test trailing-slash, percent-encoded and case variants; /v1/retrieval/{trace_id} must be denied.
  - The shared Claims struct is also used by the gRPC JwtInterceptor. Adding principal_type must make gRPC REJECT service principals explicitly (typed Status::permission_denied, with a test) instead of relying on the membership lookup failing; a comment is not an enforcement.
  - Auth Core service tokens carry sub=service:quarry-edge, service_id=service:quarry-edge (same value), principal_type=service, scopes, reason, optional jti and optional acting_user_id, no user_id (convex-token.service.ts:629-635). Treat acting_user_id presence as an ambiguous shape and refuse it (401) for this route table.
  - Service principals must skip the user-core visibility facade: orchestrator.rs ~997 and api/mod.rs resolve_viewer_grants call HttpVisibilityClient.visible_documents with the service bearer (a refused, fail-open call with latency). Set granted=[] for principal_type=service and test with a counting double that asserts zero facade calls.
  - Retrieval result cache keys are partitioned by viewer_scope_token(viewer, grants); a service principal gets its own partition, but any server-forced scope or mode_mix (QB-M1a) must be applied BEFORE the cache key is computed, otherwise two requests with different client filters collide.
  - Record a ledger/matrix exception: skipping the membership check for service principals deviates from matrix rule 9 (Identity and membership are Control Plane-owned); QB-T13b writes rule 18 for it.
- dirty/untracked collisions: None: apps/Data Plane v2 has no uncommitted changes.; Read-only dirty references: auth-core convex-token.service.ts and plane-token.controller.ts (take the claim shape from them, do not edit).
- cross-design overlap: No other design edits retrieval-engine auth. CR-T15 and Z-41 later touch orchestrator.rs/config.rs: sequence QB-T03, QB-T04, QB-N2, QB-M1a, QB-T05a before them.
- deploy: S5 - data: retrieval-engine image rebuild through the launcher (batch QB-T03+T04+N2+M1a+T05a into one rebuild with WEB_RERANK_ENABLED unset/0). No compose change here.
- live proof (none): Free and cheaper than designed: drive the admission proof with a minimal valid body carrying zdr_mode ephemeral (or sovereign_required true). That restricts egress, so no Cohere embed and no persisted trace: a free proof of 200-vs-403 and of the audit row. Do NOT assert "cause ok" on a default-mode call before QB-T10, because Quarry's body still has the invalid zdr_mode "off" (422). Counters: dpv2_authz_denials_total{cause=denied:no_membership} stops increasing for service:quarry-edge; one audit row with the service id.
- notes_for_implementer: Build: (1) Claims gains principal_type, service_id, user_id, acting_user_id, reason as Option<String> with serde default (keep zdr: bool required). (2) authz::principal::admit(): user path = today's behaviour byte-for-byte (header org must equal claim org, PolicyClient membership); service path = identity-shape validation (principal_type==service, service_id==sub, sub starts service:, no user_id, no acting_user_id, non-empty scopes; any violation 401), then service_routes lookup by (method, MatchedPath): POST /v1/retrieve, /v1/retrieve/hybrid, /v1/knowledge/search need scope data:read for service:quarry-edge; /v1/rerank (registered by QB-T05a) needs data:rerank; every other route 403 service_principal_route_not_allowed, counted via record_authz_denial(org, denied:service_route_not_allowed) and audited. Header org mismatch stays 403. admin_read_all stays false (the token has no org:data:read_all). Service principals get granted_docs=[] without calling user-core. gRPC interceptor explicitly refuses principal_type=service. (3) Contract tests with REAL rules in default cargo test (no Postgres): sign tokens with the dev-dependency rsa key and run them through the moved real JWT validator; build the token claims from a golden produced by Auth Core's real issuePlaneToken (service principal, data-plane audience); drive the PolicyClient double from a second golden produced by Auth Core's real exported buildDataPlaneDecision(null, data.read) for a service: subject (not_member), NOT a hand-written Policy(true) double. Cases: golden service token with data:read admitted on /v1/retrieve (fails on today's main: 403); same token denied on /v1/admin/cleanup/orphans, /v1/context/pins, /v1/cache/semantic/store, /v1/internal/space-retrieval-bindings, /v1/retrieve/graph, /v1/retrieval/{trace_id}; data:read-only token denied on /v1/rerank; user token (interactive golden) denied on /v1/rerank; user path unchanged; five ambiguous shapes 401 incl. acting_user_id; x-org-id mismatch 403; zero visibility-facade calls; gRPC refusal; trailing slash/encoded variants denied. Add a jest test in Auth Core (new file, not the dirty specs) that regenerates both goldens and asserts the committed copies match; record sha256 in a .sha256 file and compare in the Rust test. Line endings: .rs CRLF, .json LF; check each file with node. Never print tokens or the registry.

#### QB-T04: Retrieve contract for non-user callers: SourceRef.url, wire golden, strict zdr_mode

- design: quarry (T04) | plane: data | size: S | chain: C3 #2 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: KEEP (small), with corrections to the golden location and to what populates url.
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/types.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/cache/mod.rs (compile check; SourceRef is cached)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/context_pack/mod.rs (compile check)
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/retrieve_contract.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/fixtures/contracts/retrieve-v1/ (response and request goldens; vendored by hash into Quarry)
  - apps/Data Plane v2/openapi/d4d5.yaml (CRLF; check with node)
- corrections (folded in):
  - Populate SourceRef.url only for documents.source = 'quarry' (CASE WHEN source='quarry' THEN metadata->>'url' END) and only when it is an http(s) URL. Selecting metadata->>'url' for every document would add an unreviewed tenant-metadata field (SharePoint and other connector URLs) to every /v1/retrieve response for every caller and to the Dragonfly retrieval cache (cache/mod.rs:100 stores Vec<SourceRef>).
  - Wire goldens must live inside the Rust build context: services/retrieval-engine-rs/tests/fixtures/contracts/retrieve-v1/. A repo-root apps/Data Plane v2/contracts/ directory does not exist and is not copied by the retrieval-engine Dockerfile (it copies only Cargo.toml, Cargo.lock, proto/, services/).
  - Add the new field with #[serde(default, skip_serializing_if = "Option::is_none")] so cached entries and persisted traces written by the old build still deserialize; SourceRow (the sqlx FromRow struct next to join_sources) needs the same column.
  - Quarry needs the full wire shape, so the golden must be produced by serializing a real RetrievalResponse (not hand-edited JSON); also emit a request golden for the body Quarry sends (vendored back from QB-T10) and deserialize it into the real RetrievalRequest/ZdrMode so the literal "off" can never recur.
  - The gRPC Source message stays untouched; note the HTTP/gRPC drift in docs.
- dirty/untracked collisions: None (Data Plane tree clean).
- cross-design overlap: CR-T15 later adds chunk_context to ScoredCandidate in the same types.rs and edits orchestrator.rs (different structs; QB-T04 first). Z-41 also edits orchestrator.rs/config.rs.
- deploy: S5 - data: same retrieval-engine image rebuild as QB-T03 (batch them).
- live proof (tiny): Free if done with zdr_mode reject instead of the design's paid embed: reject restricts egress so no Cohere embed runs, the sparse arm still returns the org-visible quarry document when the query text matches, and sources[].url can be compared with the stored metadata url. A normal-mode call costs one Cohere Embed query (tiny) if the owner prefers the dense path; state the count.
- notes_for_implementer: Add url: Option<String> to SourceRef and to SourceRow; extend the join_sources SQL with the CASE expression above; wiki rows keep None. Do not add a zdr_mode alias. Tests (default cargo test, no DB): a serialization golden of a real RetrievalResponse built in the test: sources[].url present when set and absent when None, ScoredCandidate serializes no score and no url (pins what Quarry must mirror), deserialize {"zdr_mode":"off"} fails with the strict enum, and deserialize the vendored Quarry request body golden succeeds. Unit test that mode_mix.rerank=false yields no text rerank (use the existing text_rerank_allowed/rerank_requested helpers). DB-gated companion (TEST_DATABASE_URL, #[ignore]) asserts the SQL returns url only for source=quarry. Write the golden with LF endings (.json eol=lf per .gitattributes) and record its sha256 so Quarry's vendored copy (crates/quarry-runtime/tests/fixtures/dp-contracts/retrieve-v1/) can be checked by both sides. Update openapi/d4d5.yaml (CRLF file; check with node) to document accepted zdr_mode values (disabled, reject, ephemeral), the sovereign_required default (absent means strict true in the engine) and the additive url field.

#### QB-N2: Retrieval engine: redact the query in the retrieval.start log when the mode is restrictive (N2)

- design: manifest | plane: data | size: S | chain: C3 #3 | kind: code
- depends_on: QB-T04
- gates: start [] done [] deploy []
- review source: added by this manifest
- verdict: NEW (critic-quarry missing task: redact query text in retrieval.start for restrictive modes).
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs (~1086)
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/ (log-capture test)
- corrections (folded in):
  - retrieval.start logs the raw query text at INFO next to zdr_mode (orchestrator.rs:~1086; live log lines confirm), ZDR requests included: this breaks the owner's ZDR rule in operator logs. Redact the query when zdr_mode is reject or ephemeral or sovereign_required is true; keep a length and hash for correlation.
  - The same applies to any other span/log field carrying the query on the restrictive path (check with the log-capture test).
- cross-design overlap: Same orchestrator.rs hunks as QB-T04 and QB-M1a: sequential in chain C3.
- deploy: S5 - data: retrieval-engine image (with QB-T03/T04/T05a).
- live proof (none): Free: a restrictive-mode retrieve leaves no query text in the retrieval-engine log (log capture).
- notes_for_implementer: Log-capture test first: with zdr_mode ephemeral/reject/sovereign the INFO line contains no query text; a non-restrictive call is unchanged.

#### QB-M1a: Retrieval engine: server-enforced Quarry corpus scope for service principals (M1a)

- design: manifest | plane: data | size: M | chain: C3 #4 | kind: code
- depends_on: QB-T03, QB-T04, QB-N2
- gates: start [] done [] deploy [G-M1B-BACKFILL]
- review source: added by this manifest
- verdict: NEW (reconstructed by this manifest from the surviving headline of the quarry review: "forced dense-only scope plus a stamped source payload (new task M1)"; the full M1 text was in the truncated part of the review).
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/authz/service_routes.rs (forced scope per service principal)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs (apply forced filters and mode before the cache key and before the overfetch trim)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/search/filters.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/postprocess.rs (visibility gate ~499: SQL predicate on documents.source as defence in depth)
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/service_scope.rs (new; real Postgres and Qdrant, TEST_DATABASE_URL gated plus a hermetic table test)
- corrections (folded in):
  - The integrator plan's fix (SQL predicate in the post-fusion gate only) is not enough: the gate runs AFTER the overfetch trim, the sparse arm takes no filters at all (filters.sources is never read by sparse, keyword or postprocess), and Qdrant has no source payload key (live scroll of dataplane_knowledge_embedv4: keys knowledge_id, document_id, org_id, chunk_index, text, document_date). Net effect today: the dense arm returns zero candidates for every Quarry search and the sparse arm stays unscoped (live: 1 sharepoint, 1 chat-upload and 2 quarry org-visible documents).
  - Server-enforced per service principal: the route table (QB-T03) forces filters.sources=[quarry] and a dense-only mode_mix for service:quarry-edge on /v1/retrieve and /v1/retrieve/hybrid; the client filter is a hint only and can never widen. Force BEFORE the cache key is computed (QB-T03 correction) and before the overfetch trim.
  - Dense arm filters on the stamped payload key source (QB-M1b stamps new points and backfills the existing 207); until QB-M1b is deployed and backfilled the dense arm returns zero for Quarry, so Quarry's vector leg stays disabled (G-M1B-BACKFILL). Keep the post-fusion SQL predicate (documents.source=quarry within the org) as defence in depth, never as the only control.
  - Sparse and graph arms are not run for the forced scope (they cannot be scoped). Fallback if payload stamping is rejected: resolve a bounded document-id allow-list from Postgres per request and pass it to the dense arm; record the scale limit.
  - Verify which mode_mix switches exist today in pipeline/types.rs (rerank is known from the QB-T04 review; sparse/graph/wiki/pins switches must be confirmed) and add any missing switch; the forced dense-only mix must be applied server-side whatever the client sends.
  - This also makes the rerank input premise true: only quarry-origin text reaches /v1/rerank (QB-T05a rule: Quarry sends only quarry-origin or public text).
- cross-design overlap: QB-M1b (embedding-engine payload stamp, chain C1) is the other half; QB-T10 (Quarry) consumes the scoped contract.
- deploy: S5 - data: retrieval-engine image rebuild with QB-T03/T04/T05a; effect only after QB-M1b backfill.
- live proof (none): Free after the backfill: a Quarry-principal retrieve with zdr_mode ephemeral cannot return a SharePoint or chat-upload chunk; with a non-ZDR call (one Cohere Embed query, state the count) dense hits are source=quarry only.
- notes_for_implementer: Add the forced-scope structure to service_routes (per principal: forced filters, forced mode_mix, allowed arms). Tests with REAL rules: mixed-corpus fixture (quarry, sharepoint, chat-upload documents) in real Postgres and Qdrant with the stamped key: a quarry-edge principal can never retrieve a non-quarry chunk through any arm or any client filter (including filters.sources=[sharepoint], empty filters, and sources omitted); a user principal behaves exactly as before; the forced scope is part of the cache key partition; the post-fusion gate rejects a non-quarry candidate even if the dense filter is bypassed (inject one).

#### QB-T05a: Data Plane rerank capability: POST /v1/rerank core, policy, limits, idempotency, metrics, interim v1 cost event (T05a)

- design: quarry (T05); parent: QB-T05 | plane: data | size: L | chain: C3 #5 | kind: code
- depends_on: QB-T03, QB-M1a (design had: T01, T03)
- gates: start [] done [] deploy [G-CP-P1, G-PROCESSOR-APPROVED-PROOF]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: KEEP, SPLIT into T05a (route, schema, policy, limits, idempotency, deadline, metrics, v1 cost event) and T05b (shared provider permit pool, token bucket and circuit breaker that also touches the tenant retrieval path). Corrections are mandatory.
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/mod.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/schema.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/policy.rs (new; pure, table-tested)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/limits.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/cache.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/search/rerank.rs (per-request timeout, typed errors, rerank_texts)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rate_limit/mod.rs (parameterised limiter constructor)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/audit/mod.rs (AuditSink adapter)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/config.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/main.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/lib.rs (declare rerank_api in BOTH lib.rs and main.rs)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/api/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/metrics/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/rerank_contract.rs (new)
  - apps/Data Plane v2/services/retrieval-engine-rs/tests/fixtures/contracts/rerank-v1/ (goldens; inside the Docker context; vendored by hash into Quarry)
- files removed from the design list:
  - apps/Data Plane v2/contracts/rerank-v1/ (not in the Docker context)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/metering.rs (replaced by a CostSink inside rerank_api)
- corrections (folded in):
  - Goldens: services/retrieval-engine-rs/tests/fixtures/contracts/rerank-v1/ (inside the build context), vendored into Quarry by hash; not apps/Data Plane v2/contracts/rerank-v1/.
  - Declare rerank_api in BOTH lib.rs and main.rs (the bin re-declares modules); watch dead_code lints under cargo clippy -D warnings (Makefile:155 and CI).
  - RerankClient::with_endpoint builds reqwest::Client::new() with no timeout. rerank_texts must use a per-request timeout derived from the remaining deadline, or the tokio timeout only abandons the future while the socket stays open.
  - PerOrgLimiter has only from_env() (env DPV2_RATE_LIMIT_PER_ORG_*). A rerank-class limiter needs a parameterised constructor (prefix, rps, burst) added to rate_limit/mod.rs; reuse the Lua token bucket and the fail-open convention.
  - Idempotency/metering key must include a content hash: rerank:{org}:{request_id}:{blake3(query|profile|texts)} (a caller-chosen request_id reused with different content would otherwise under-record). The 120 s moka cache is per replica; document that.
  - Metering can ship now without waiting for the ledger foundation: emit the EXISTING v1 rerank cost event from rerank_api (count = documents_scored, estimated_tokens = chars/4, user_id None, zdr false, deterministic key above, signed with the existing retrieval signer). The Go verifier already accepts event_type rerank from the retrieval producer. QB-M later moves to the v2 builder (feature=web-rerank, units = search units). If the provider response carries meta.billed_units.search_units, capture it instead of estimating.
  - Add a per-org daily web-rerank unit budget, alert-only by default (matches owner default D-E): an agentic loop can issue unbounded searches and each is a paid call; the 5 rps limiter alone is about 432k calls per org per day.
  - The processor gate: keep WEB_RERANK_PROCESSOR_APPROVED default false in code and compose; the orchestrator sets it true only in the live-proof environment under owner ruling 3 (paid reranking is fine). Legal confirmation (O2) stays open for the production default. State the rule in docs: only service:quarry-edge may call, and Quarry must send only quarry-origin or public text (vector hits are tenant chunks up to 4000 chars, so the QB-M1a source scope is what makes the "public text" premise true).
  - The startup validator must be route-level: if RERANK_ENDPOINT is empty or not on an Azure host (*.azure.com, *.ai.azure.com) the web route answers status skipped reason disabled/misconfigured (200 with marker) and logs ERROR once; it must never abort the engine or touch tenant retrieval. Today the code default is public Cohere when RERANK_ENDPOINT is unset (config.rs:344-346).
  - The in-process axum Cohere mock is itself a permissive double: capture ONE real Azure Foundry Cohere v2 response during the live proof, commit it as a golden (synthetic public test strings only) and add an #[ignore] live contract test gated by env.
  - Do not shard caller documents; the existing rerank_parallel path (PARALLEL_SPLIT_AT=50, chunks of 32) must not be used for /v1/rerank; the shared pipeline path keeps its behaviour unchanged in T05a.
  - The attestation hook is a trait with a default implementation returning false ("no attestation"); Z-41 owns the shared egress_attestation module and wires the real permits_rerank() (attestation is data, owner default D-A). Do NOT create rerank_api/attestation.rs.
- dirty/untracked collisions: None (Data Plane tree clean). Within the Data Plane, CR-T15 and Z-41 edit search/rerank.rs, config.rs, orchestrator.rs later: land this first.
- cross-design overlap: CR-T15 (rerank input with chunk context) edits the same RerankClient/orchestrator; the metering foundation (CR-T10) later replaces the v1 event via QB-M; Z-41 shares the attestation module with the ZDR rerank path.
- deploy: S5 - data: retrieval-engine image rebuild; compose env (WEB_RERANK_ENABLED=0, WEB_RERANK_PROCESSOR_APPROVED=false, limits) is added by QB-T08 in the retrieval-engine block only.
- live proof (tiny): Free probes: skipped zdr, skipped disabled, 401/403 matrix, replay. One paid 10-document call (one search unit) for the ok path and the real-response golden; report the exact number of provider calls (expected 1) from dpv2_web_rerank_total and the cost event. Synthetic public strings only, never tenant text.
- notes_for_implementer: Build POST /v1/rerank as a separate axum sub-router merged in router_inner with its own state RerankState{config, Option<RerankClient> cloned from pipeline.reranker, CostSink trait (signer+NATS impl, in-memory test impl), AuditSink, parameterised limiter, moka cache}; auth layer = QB-T03 admit(); RequestBodyLimitLayer 1 MiB on that sub-router only. Request: request_id 8..64, query 1..1024, documents<=100 with unique ids and text 1..4000 (longer is truncated at a char boundary and listed in truncated_ids), top_n, profile web-v1 (allowlist resolved server-side to RERANKER_MODEL; a caller-supplied model is rejected), zdr_mode REQUIRED (disabled|reject|ephemeral; absent or off is 400), sovereign_required optional (absent means strict true like the engine), deadline_ms 100..8000 default 2000. NO highlights field in v1 (QB-T06 deferred). Org is taken only from the signed claim; x-org-id header mismatch 403. Decision order in policy.rs (pure, table-tested): enabled? -> processor approved? -> effective zdr (token zdr OR body restrictive) -> sovereign -> circuit -> input size; every non-ok outcome is HTTP 200 with status skipped|degraded, a reason and the INPUT order with scored:false and no scores; only 400/401/403/413 and 429 from our own limiter (with Retry-After) are errors. One provider call per request (no sharding), rerank_texts(query, texts, top_n, remaining_deadline) with per-request timeout, typed errors (RateLimited, Provider, Timeout), 429 retry only if Retry-After fits the remaining budget, NaN/Inf coerced to 0.0, out-of-range or duplicate provider indexes ignored, errors sanitized (sanitized_rerank_status_error). Persist nothing: no retrieval_runs/trace row, cache values hold scores and offsets only, no query or document text in logs, errors or spans (log capture test). Metrics dpv2_web_rerank_total{outcome,reason}, _duration_seconds, _documents; access_audit_log row per call with cause rerank:ok|skipped_zdr|degraded_timeout (no ids, no text). Contract tests with REAL rules in default cargo test: real admit() + the real golden service token from QB-T03 (not a permissive double) for 401/403 cases (user token on /v1/rerank 403, no scope 403, data:read-only 403); policy matrix zdr x sovereign x processor x enabled x circuit asserting provider call count EXACTLY 0 and cost-event count 0 for every restrictive cell (the mock provider counts calls); idempotent replay returns the identical body with provider calls 1; limits (101 docs 400, duplicate ids 400, 1025-char query 400, body over 1 MiB 413, zdr_mode missing 400). Files are CRLF .rs; goldens LF .json; compose/openapi in Data Plane v2 are CRLF (check each file with node; .gitattributes is itself dirty).

#### QB-T05b: Shared rerank provider permit pool, token bucket and circuit breaker (T05b)

- design: quarry (T05); parent: QB-T05 | plane: data | size: M | chain: C3 #6 | kind: code
- depends_on: QB-T05a (design had: T01, T03)
- gates: start [] done [] deploy [G-EVAL-NOT-DROPPED]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: NEW (split from T05 on the reviewer's suggestion). Separate PR; touches the tenant retrieval path so it needs an eval run.
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/search/rerank.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/limits.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/config.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/metrics/mod.rs
- corrections (folded in):
  - Shared provider permit pool (RERANK_PROVIDER_MAX_INFLIGHT reserving 2 permits for tenant retrieval), token bucket (RERANK_PROVIDER_RPS), circuit breaker (5 failures in 30 s opens 30 s). The shared pipeline path keeps its behaviour unless the pool is saturated.
  - Capacity default D-D: a shared Foundry deployment with a limiter now; dedicated capacity before general availability (critic #16: about 19% of queries are lost to 429s today, so a dedicated deployment is a precondition for default-ON, not "before GA").
  - scripts/eval-gate.py baseline must not drop: run the Data Plane eval before and after.
- cross-design overlap: Touches the tenant retrieval reranker path (QB-T05a, CR-T15).
- deploy: S6 - data: retrieval-engine image rebuild.
- live proof (none): No extra paid calls beyond the eval run; observe permit saturation metrics.
- notes_for_implementer: Add the permit pool and breaker behind config; tests with a mock provider proving tenant retrieval keeps 2 reserved permits under web saturation, the breaker opens after 5 failures in 30 s and half-opens, and the token bucket rate-limits. Run scripts/eval-gate.py before and after; the baseline must not drop.

#### QB-T08: Config, compose, OpenAPI, runbook, auth matrix rows and dark ZDR attestation

- design: quarry (T08) | plane: data | size: S | chain: C3 #7 | kind: code
- depends_on: QB-T03, QB-T05a (design had: T03, T05)
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: SHRINK: keep compose/config, OpenAPI, runbook, regression-matrix rows; MOVE attestation.rs out (shared module with Z-41); add the missing test files.
- files:
  - apps/Data Plane v2/docker-compose.yml (retrieval-engine block 361-538 only; CRLF)
  - apps/Data Plane v2/openapi/d4d5.yaml
  - apps/Data Plane v2/docs/web-rerank-v1.md (new)
  - apps/Data Plane v2/DATA_PLANE_STATUS.md
  - apps/Data Plane v2/scripts/auth-regression-matrix.sh (LF)
  - apps/Data Plane v2/tests/scripts/auth-regression-matrix-test.sh (the script's own 131-line self-test)
  - apps/Data Plane v2/tests/scripts/run-isolated-real-authority.sh (registry gains a quarry-edge row GENERATED from config/plane-service-principals.json, not hand-typed; add a parity test)
  - apps/Data Plane v2/tests/scripts/web-rerank-compose-contract-test.sh (new sibling; do NOT edit compose-security-contract-test.sh)
  - apps/Data Plane v2/scripts/qb-preflight.sh (new, read-only pre-flight script that replaces the design's QB-T15 pre-flight)
- files removed from the design list:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/attestation.rs (moved to Z-41's shared egress_attestation module)
- corrections (folded in):
  - attestation.rs would be a second, retrieval-engine-local copy of the Model Plane ZdrAttestation validator (inference-core provider/zdr.rs); retrieval-engine has no attestation code today. Z-41 needs the same shape for the embedding egress guard. Build ONE shared module under retrieval-engine (egress_attestation) owned by Z-41, and have rerank_api read a single permits_rerank() bool that is false until an attestation exists (owner default D-A: no provider is attested; attestation is data, not code).
  - scripts/auth-regression-matrix.sh runs against an isolated Compose project with bearers supplied by env (DPV2_USER_BEARER etc.). Its registry is the hand-written inline JSON in tests/scripts/run-isolated-real-authority.sh:164 (retrieval-engine, embedding-engine, browser-fixture-seeder only; no quarry-edge). The new service rows cannot mint a quarry-edge token unless that registry gains a quarry-edge row copied from the committed policy (and must not become another permissive copy). Add run-isolated-real-authority.sh and tests/scripts/auth-regression-matrix-test.sh to the file list.
  - The Data Plane compose retrieval-engine block is lines 361-538 (RERANK_ENDPOINT at 486, RERANKER_MODEL 493, RERANK_TOP_K 538); compose, openapi and DATA_PLANE_STATUS.md are CRLF in the working tree while the matrix scripts are LF.
  - Add the compose contract in a NEW sibling script: WEB_RERANK_ENABLED defaults to 0, WEB_RERANK_PROCESSOR_APPROVED defaults to false and RERANK_ENDPOINT has no public Cohere fallback for the web route.
  - Quarry T15's tasks are folded in: the pre-flight script is scripts/qb-preflight.sh (read-only: registry has data:rerank, DP migration applied, image has /v1/rerank (401 without a token), WEB_RERANK_ENABLED state, quarry-edge env, baseline metrics dpv2_authz_denials_total and cost_events counts). After the live proof, flipping the compose defaults (WEB_RERANK_ENABLED to 1) is a post-proof step of THIS task, executed by the orchestrator.
- dirty/untracked collisions: None (all listed Data Plane files clean).
- cross-design overlap: Z-40/Z-41 (attestation and attestation-aware egress guard), CR-T13 (compose defaults), CR-T17 (smoke matrix). One block per service in the Data Plane compose.
- deploy: S5 - data: compose/env change needs a recreate of retrieval-engine (no extra rebuild if batched with QB-T05a); the matrix and scripts need no deploy.
- live proof (none): Free: the matrix prints only labels and statuses. Expected new rows: no token 401, user token on /v1/rerank 403, service token without scope 403, service token on admin routes 403, quarry-edge token on /v1/retrieve 200 (use zdr_mode ephemeral so the 200 costs nothing).
- notes_for_implementer: Add the compose env WEB_RERANK_ENABLED (default 0), WEB_RERANK_ALLOWED_SERVICES (default service:quarry-edge), WEB_RERANK_PROCESSOR_APPROVED (default false), WEB_RERANK_MAX_INFLIGHT, WEB_RERANK_PER_ORG_RPS/BURST, WEB_RERANK_ORG_DAILY_UNITS (alert-only), RERANK_PROVIDER_* to the retrieval-engine block only; document kill switches (WEB_RERANK_ENABLED=0, scope removal, Quarry flag), quota ownership and the missing price sheet in docs/web-rerank-v1.md; update openapi/d4d5.yaml with /v1/rerank and the 200-with-marker contract. Extend the matrix script with the service-principal rows using a new DPV2_SERVICE_BEARER env validated like the others (valid_bearer), and extend its dry-run self-test to assert the new rows and their expected statuses (401, 403, 403, 403, 200) are encoded; extend run-isolated-real-authority.sh's registry with a quarry-edge row generated from config/plane-service-principals.json (not hand-typed scopes) and add a test that fails if the two disagree. No tokens, org ids or bodies in output.

#### Z-41: ZDR-capable query embedding path or explicit lexical degrade

- design: zdr (Z-41) | plane: Data | size: L | chain: C3 #8 | kind: code
- depends_on: QB-T05a (design had: Z-40)
- gates: start [] done [] deploy [G-ATTEST]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest; build does NOT depend on Z-40, only the dense-arm proof does)
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/embed/mod.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/config.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/egress_attestation.rs (new shared module; QB-T05a reads permits_rerank())
  - apps/Data Plane v2/services/retrieval-engine-rs/src/lib.rs (module declaration)
  - apps/Data Plane v2/services/retrieval-engine-rs/src/main.rs (module declaration)
- corrections (folded in):
  - Make the Cohere Foundry and direct-Azure egress guards attestation-aware (same evidence-bound ZdrAttestation shape as inference-core provider/zdr.rs) in ONE shared module owned here (egress_attestation), exposing permits_embed() and permits_rerank(); QB-T05a reads permits_rerank() through a trait that defaults to false.
  - With no attestation: ZDR retrieval returns lexical-only results plus an explicit notice field the gateway surfaces; with attestation the dense arm runs. The index and query embedding model must match. Existing egress-guard tests keep failing closed when unattested.
  - Owner default D-A: attestation is data, not code. Docs conflict (sovereign-rag-blueprint-reconciliation.md:79-82) recorded, relied on neither.
  - Sequenced after the QB retrieval-engine tasks (same orchestrator.rs/config.rs/types.rs) and before CR-T15.
- cross-design overlap: QB-T05a/T05b/T08, CR-T15 (same files).
- deploy: S6 - data: retrieval-engine image.
- live proof (none): Z-60 optional with a ZDR token (needs the test org and, for the dense arm, an attestation).
- notes_for_implementer: Orchestrator tests: ZDR with no attestation returns lexical results plus notice; with attestation the dense arm runs; existing egress-guard tests keep failing closed when unattested.

#### QB-M: Switch the web-rerank cost producer to the v2 ledger builder: feature web-rerank, units = billed search units (QB-M)

- design: manifest | plane: data | size: S | chain: C3 #9 | kind: code
- depends_on: QB-T05a, CR-T10
- gates: start [G-T10-CRATE] done [] deploy [G-T4-DEPLOYED-PRICED, G-T10-CONSUMER-DEPLOYED]
- review source: added by this manifest
- verdict: NEW (the reviewer's "QB-M": the rerank-producer follow-up after the metering foundation; replaces quarry T07 and T16).
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/rerank_api/mod.rs (CostSink implementation)
  - apps/Data Plane v2/services/retrieval-engine-rs/Cargo.toml (usage-ledger-rs dependency)
- corrections (folded in):
  - After CR-T10 lands, switch the rerank producer from the interim v1 event (QB-T05a) to the v2 builder with feature web-rerank (distinct from the tenant rerank) and units = billed search units (meta.billed_units.search_units when present, otherwise count = documents_scored).
  - Pricing: a Cohere-rerank-v4.0-pro row in cost-core's catalogue is needed; the price is NOT in the repo (OPS-PRICE); store null cost_usd until supplied so it is not priced at the 3.00/15.00 default. Until then the chat budget aggregate is unaffected (nothing is forwarded; CR-T4 excludes non-chat features anyway).
  - Skip metering entirely for skipped/degraded/ZDR outcomes; deterministic key rerank:{org}:{request_id}:{content_hash}.
- cross-design overlap: Quarry T07/T16 merged into CR-T10/CR-T4/QB-T05a/QB-M.
- deploy: S6 - data: retrieval-engine image after the consumer is verified and cost-core is priced.
- live proof (tiny): One paid rerank call: exactly one cost-core row with feature web-rerank, org and units; replay leaves one row (shares the LP-RERANK call).
- notes_for_implementer: Replace the CostSink implementation; keep the interface; tests assert the event shape against the CR-T10 golden and the deterministic key.

#### CR-T15: Retrieval: rerank input with chunk context (measured, flag-gated)

- design: contextual (T15) | plane: data | size: M | chain: C3 #10 | kind: code
- depends_on: CR-T14, QB-T05a, QB-T05b (design had: T14)
- gates: start [] done [] deploy [G-T14-CTX-CELLS]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest). Sequenced last in the retrieval-engine chain: QB first, then CR.
- files:
  - apps/Data Plane v2/services/retrieval-engine-rs/src/search/rerank.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/search/sparse.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/types.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/pipeline/orchestrator.rs
  - apps/Data Plane v2/services/retrieval-engine-rs/src/config.rs
- corrections (folded in):
  - Plan: QB lands first in retrieval-engine (auth, route table, rerank_api, redaction), then CR: same files (rerank.rs, orchestrator.rs, types.rs, config.rs). Do not start before QB-T05a/QB-T05b are merged.
  - chunk_context is carried through the candidate and the source join; QB-T04 adds SourceRef.url in the same types.rs/orchestrator.rs: do not drop or reorder it. The retrieval cache key (cache/mod.rs) must include the flag so cached entries never mix modes.
  - RERANK_INPUT_INCLUDE_CONTEXT default off; with the flag off the rerank request body is byte-identical to today. Context never appears in the response payload or to graders.
  - Reranking stays on the paid Cohere route; never a subscription route (owner ruling 3). Ship enabled only if the CR-T14 cells show a gain beyond the rerank noise floor (three runs each; 429 noise).
  - Norwegian multibyte fixture: truncation char-boundary safe.
  - Dependency on CR-T14 is on its HARNESS code (cells exist), not on its live run: the rerank-input-with-context cell can only be measured after this flag exists and both are deployed (LP-EVAL); the flag stays off until G-T14-CTX-CELLS.
- cross-design overlap: QB-T05a/T05b (same RerankClient/orchestrator), Z-41 (orchestrator/config). Sequence: QB first, Z-41, then this.
- deploy: S6 - data: retrieval-engine image rebuild (flag off); enable per eval decision via env.
- live proof (tiny): No extra paid generation; rerank calls are part of the CR-T14 eval run. Compare rerank-on cells with and without the flag three times each and record the decision in the evidence doc.
- notes_for_implementer: Carry chunk_context on retrieval candidates through the source join and, behind RERANK_INPUT_INCLUDE_CONTEXT (default off), build rerank documents as compose_contextualized(context, text). Tests first: flag off byte-identical; flag on equals compose(context,text) and a candidate without context unchanged; context never in the response payload; Norwegian multibyte fixture; cache-key includes the flag.

### Chain C4 mp-gateway

#### CR-T3: inference-core: typed ZDR/residency/tier statuses, per-service counters, shared wire fixtures

- design: contextual (T3) | plane: model | size: M | chain: C4 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS. Independent of the Control Plane wave, small, additive, safe to ship first.
- files:
  - apps/Model Plane/rust/services/inference-core/src/grpc.rs
  - apps/Model Plane/rust/services/inference-core/src/main.rs
  - apps/Model Plane/rust/services/inference-core/src/http_health.rs
  - apps/Model Plane/rust/services/inference-core/src/metrics.rs (new)
  - apps/Model Plane/rust/services/inference-core/src/lib.rs (export metrics)
  - apps/Model Plane/api/contracts/infer-statuses.json (new, generated from the code under test)
  - scripts/tests/ repo-level golden sync test shared with Z-10 (new)
  - NOTE (not a file of this task): the sha256-pinned copy of the golden in the Data Plane consumer is vendored by CR-T5
- files removed from the design list:
  - apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs
  - apps/Model Plane/proto/contract-fixtures/infer-statuses.json
  - apps/Model Plane/rust/services/inference-core/tests/status_contract_test.rs
- corrections (folded in):
  - Golden location: apps/Model Plane/api/contracts/infer-statuses.json (the directory Z-10 also uses), not apps/Model Plane/proto/contract-fixtures/ (does not exist). A sha256-pinned copy is vendored into the Data Plane consumer by CR-T5, plus a repo-level sync test in scripts/tests/.
  - Do NOT edit provider/fallback.rs: it is dirty (+107 lines inside mod resolution_tests, another session) and belongs to ZDR only. Counters go in a new src/metrics.rs.
  - Untyped sites are more than the task says: reject_unattested_zdr_modality (grpc.rs ~172-187), the create_embedding error map (~374-380: only ResidencyViolation typed; ZdrUnavailable and TierUnavailable fall to INTERNAL) and provider_error_to_status (~1465-1467). Type all of them through one helper.
  - provider_error_to_status and typed_status are private and the trailer tests are inline (grpc.rs mod tests ~1732): keep the contract tests inline or export a doc-hidden accessor; tests/status_contract_test.rs cannot call them.
  - Metrics cardinality: model label from the response model_used (unknown on failure), never from the caller-controlled request.model; service label only for service principals; no org or user label.
  - Freeze the kind strings now (zdr_unavailable, residency_unavailable, tier_unavailable): Z-05a maps them to its refusal catalogue and Z-16 adds zdr_waiver_route_mismatch next to them.
  - The live proof needs no T1 ZDR-claim token: effective_zdr is token OR request flag (auth.rs:193), so any valid service bearer with request.zdr=true is refused before any provider call because no provider is ZDR-attested.
- cross-design overlap: ZDR Z-05a (inference-core grpc.rs refusal sites), Z-16 (grpc.rs plus fallback.rs backstop), Z-10 (shared golden location and sync test). No Quarry overlap. Same chain (C4) so grpc.rs edits are sequential: CR-T3 first, then Z-01 (auth.rs), Z-05a, Z-16.
- deploy: S1 - model: inference-core image rebuild through the launcher (Rust). No config change. Deploy before the Data Plane live proof so typed kinds exist; the Data Plane decoder must still accept an old server (message-only status).
- live proof (none): Free: GET /metrics (more than the 37-byte stub), one ListModels, one Infer with request.zdr=true refused before any provider call. No grpcurl on this machine: use a node http2 probe in the style of apps/Model Plane/scripts/smoke-wave10.js and read the trailers.
- notes_for_implementer: Build (1) one helper in grpc.rs that maps ProviderError::{ZdrUnavailable,ResidencyViolation,TierUnavailable} to Status::failed_precondition plus trailer x-mp-provider-error = zdr_unavailable | residency_unavailable | tier_unavailable (keep the content-free message and the same gRPC code so old callers behave identically) and use it in provider_error_to_status, reject_unattested_zdr_modality and the create_embedding map. (2) New src/metrics.rs following model-gateway gateway_metrics.rs: install the recorder in main.rs, pass the PrometheusHandle to http_health::serve, render it from the /metrics handler. Counters inference_requests_total{service,model,provider,outcome} and inference_tokens_total{service,model,provider,direction}; outcome includes zdr_unavailable so no provider call shows as a response counter that does not move; do not double count the cache token fields (input_tokens already folds them in). (3) Golden JSON generated from the code under test (every ProviderError, tonic code, kind trailer, retry-after trailer), not hand-written, with its sha256 asserted in a repo-level sync test shared with Z-10. Traps: grpc.rs lines 172-187, 329-358 and 351-357 are cited by docs/compliance/evidence (11 citations to this file; the checker is line plus anchor), so add new code at the end of the file or in the new module and re-run node docs/compliance/evidence/tools/check-citations.mjs on every evidence doc, re-pinning moved lines; never edit fallback.rs; AuthenticatedPrincipal::for_test is cfg(test) pub(crate) so only inline unit tests can build a ZDR-claim principal. Real-rule tests: drive the real FallbackChain with a provider whose capabilities come from the real ZdrAttestation configuration path (unattested), a ZDR-claim principal, assert FAILED_PRECONDITION, kind trailer and provider-call count 0; assert counters on the rendered Prometheus text, not a mock recorder.

#### Z-10: Shared golden fixtures: posture matrix and connection capability contract

- design: zdr (Z-10) | plane: Model | size: S | chain: C4 #2 | kind: code
- depends_on: CR-T3
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections
- files:
  - apps/Model Plane/api/contracts/zdr-posture-matrix.json
  - apps/Model Plane/api/contracts/connection-zdr-capabilities.json
  - scripts/tests/zdr-contract-sync.test.sh
  - vendored sha256-pinned copies in each consuming tree: Control Plane auth-core fixtures, integration-corev2 testdata, Model Plane rust tests, BFF tests, SPA (created by their consumer tasks)
  - apps/Model Plane/api/contracts/infer-statuses.json shares this directory (created by CR-T3)
- corrections (folded in):
  - apps/Model Plane/api/contracts/ does not exist (api/ holds openapi.yaml and sdk/); creating it is fine, but the integrator rule is that goldens are vendored and sha256-pinned in each consuming tree and specs must FAIL, not skip, when REQUIRE_CONTRACT/CI is set. The byte-identical copy test is necessary but not enough: pin the sha256 in each consumer.
  - Matrix rows must include: plan lapsed (OD-LAPSE answer), Temporary chat (explicit zdr), Space-scoped turn, other connection, other member, revoked ack, stale digest, text version below minimum, unknown provider, decision outage (Auth Core/org-core/integration-corev2 each), non-owner, token set with mixed zdr. Each row's expected code must be a code from the Z-05a catalogue, so write the matrix after the code list is fixed.
  - connection-zdr-capabilities.json is the single source for capability: integration-corev2 embeds it (go:embed); the cross-check against inference-core's codex supports_zdr=false must live in a RUST test reading the vendored JSON and instantiating the real provider capabilities (Go cannot link Rust).
  - Script test location: scripts/tests holds both -test.sh and .test.sh names; follow the neighbouring names and wire it into whatever runner executes those. Share the sync script with CR-T3 rather than adding a second one.
- dirty/untracked collisions: None (all new).
- cross-design overlap: Quarry QB-T03 and contextual CR-T5/CR-T17 add their own golden fixtures with the same vendoring convention; share the sync script.
- deploy: none - repo contract/test only.
- live proof (none): None needed; free.
- notes_for_implementer: Write the JSON files and the sync script first. The matrix is data: {id, inputs:{org_zdr, entitled, explicit_zdr, connection, capability, ack, digest_match, text_version_ok, space_scoped, outage:null|auth_core|org_core|integration_core}, expect:{decision: zdr|persistent|waived|refuse, code}}. Loader tests in every language reject unknown fields and rows without a code. Vendored copies carry a sha256 file; a repo script fails if any copy differs or if REQUIRE_CONTRACT is set and a consumer test skipped. Never evaluate rows with a local reimplementation: each plane's test must call that plane's real resolver (Auth Core TS, mp-contracts Rust, BFF Rust, integration-corev2 Go).

#### Z-00: Stop the silent subscription re-route under token ZDR

- design: zdr (Z-00) | plane: Model | size: S | chain: C4 #3 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (small stop-gap; ship first, no hard prerequisites)
- files:
  - apps/Model Plane/rust/services/model-gateway/src/normalize.rs
  - apps/Model Plane/rust/services/model-gateway/src/sse.rs
  - apps/Model Plane/rust/services/model-gateway/src/http_routes.rs
  - apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs (new small module holding refuse_subscription_under_token_zdr; or later mp-contracts retention.rs)
- corrections (folded in):
  - normalize()/normalize_stream()/normalize_with() in model-gateway/src/normalize.rs take only &InvokeRequest and have NO claims. Making them claims-aware changes the signature of 38 call sites inside normalize.rs's own tests, in a file that already carries +434 uncommitted lines from the Do-on-Terra session. Do not change those signatures. Add a pure pre-check (refuse_subscription_under_token_zdr(claims_zdr, &req) -> Option<Refusal>) in a new small module and call it at the two entries: the stream entry just before crate::normalize::normalize_stream (sse.rs ~919) and http_routes.rs ~8811 just before normalize::normalize(&req).
  - The design names the refusal code zdr_connection_not_acknowledged. No acknowledgement path exists until Z-13/Z-14/Z-19, so that code tells the user to do something impossible. Use a neutral Z-05a catalogue code now (zdr_connection_not_supported) and reserve zdr_connection_not_acknowledged for the resolver output in Z-15.
  - Keep SUBSCRIPTION_STRICT_REFUSAL / SUBSCRIPTION_AGENTIC_REFUSAL byte-identical (tests and clients match on them; tests/subscription_failure_contract.rs is dirty from another session).
  - Verified facts: zdr_direct_stream (sse.rs ~8097) calls direct_infer(..., "", "", ...) so provider_hint and subscription_connection_id are dropped; the unary persistence-free branch (http_routes.rs ~8812-8860) already forwards both and inference-core refuses, so on the unary path the only change is a typed refusal instead of the generic mapping through invoke_inference_error. Pass provider_hint/connection through zdr_direct_stream as a backstop even though the pre-check refuses first.
  - The existing test sse.rs ~560-571 (auxiliary_calls_preserve_subscription_identity_and_privacy) builds a route with zdr:true on the Terra connection, a state normalize/inference-core refuse. Leave it, but add the real-rule test the design asks for (signed Claims{zdr:true} + subscription request -> typed refusal, no inference call).
- dirty/untracked collisions: apps/Model Plane/rust/services/model-gateway/src/normalize.rs (M, +434); sse.rs (M, +2136); http_routes.rs (M, +242)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/model-gateway/src/normalize.rs; modified: Model Plane/rust/services/model-gateway/src/sse.rs; modified: Model Plane/rust/services/model-gateway/src/http_routes.rs
- cross-design overlap: Same files as Z-05a, Z-06, Z-15 (all ZDR; chain C4 serialises them). Quarry T12 edits model-gateway quarry.rs/relevance.rs/tool_loop.rs only.
- deploy: S1 - model: model-gateway image rebuild through the launcher (model-plane-only launcher copy). No config change.
- live proof (none): No live proof possible: no ZDR org exists and a ZDR token cannot be minted until the operator seed exists. Cost zero. The proof is the contract test now and Z-60 probe Z0 later.
- notes_for_implementer: Build a pure pre-check, not a signature change. Test first: a real Claims{zdr:true} (built by deserialising a token-shaped JSON, not by a struct literal with defaults) plus InvokeRequest{provider:openai-codex-subscription, subscription_connection_id:c1, zdr:false} must return a typed refusal before any inference client is touched; today it reaches zdr_direct_stream and is served or refused on the default ZDR chain with the route silently dropped. Add the same case for the unary route (http_routes.rs ~8811). Then pass provider_hint/subscription_connection_id through zdr_direct_stream into direct_infer so inference-core also refuses (belt and braces), and add a FallbackChain-level test with an attested Azure double proving a Terra-pinned request never lands on Azure. Use the Z-05a catalogue code for the refusal (zdr_connection_not_supported until Z-15 swaps in resolver output). Non-ZDR callers must be byte-identical: add a test that a non-ZDR Terra request still reaches inference unchanged. Edit hunks in place in sse.rs/http_routes.rs (shared dirty files), keep CRLF/LF as the file has it (check with node, not grep), never stash or checkout.

#### Z-01: Strict zdr claim and equal posture across delegated bearers

- design: zdr (Z-01) | plane: Model | size: S | chain: C4 #4 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections
- files:
  - apps/Model Plane/rust/services/model-gateway/src/auth.rs
  - apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs
  - apps/Model Plane/rust/services/inference-core/src/auth.rs
  - apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs (test signer; modified by another session)
  - apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs (line ~127 zdr:false signer; modified by another session)
  - apps/Model Plane/rust/services/model-gateway/src/session_terminal_auth.rs (verify it carries zdr; read-only check unless it does not)
  - apps/Model Plane/rust/services/model-gateway/tests/source_review_live.rs (reads claims zdr with unwrap_or(false))
- corrections (folded in):
  - Three Claims structs need the serde default removed, not two: model-gateway/src/auth.rs (HTTP, zdr at line 58), model-gateway/src/grpc_auth.rs (gRPC, line ~329) and inference-core/src/auth.rs (line ~76). Keep the field required (no Option) as in session-core/execution-core.
  - Posture equality across delegated bearers must apply to USER bearers only (verify_delegated_user_bearer, which all nine delegated audiences use). Do NOT add it to verify_delegated_service_inference_bearer: the registry sets retention per audience, so a service principal legitimately holds different zdr values by audience (live registry: the retrieval-engine row is control-policy=zdr and inference-core=persistent; model-gateway row is quarry=zdr, session-core=zdr). Test against the real registry rows, not invented ones.
  - Pre-check inventory before removing the default: list every place that signs a JWT consumed by these two services without a zdr claim. Auth Core always sets it (convex-token.service.ts issuedTokenZdr at ~543/642 and token-exchange verification requires typeof zdr === boolean); the model-gateway dev bypass stub sets zdr:false (auth.rs ~763). Check test signers in model-gateway/tests (e2e_invoke_chain_test.rs mints with zdr; orchestration_http_test.rs line 127 has zdr:false; source_review_live.rs reads claims zdr with unwrap_or(false)); session_terminal_auth.rs mints via Auth Core internal-token: confirm it carries zdr. Any signer lacking the claim must be fixed in the same change.
  - The design's free live proof ("a hand-made token lacking zdr gets 401") proves nothing: a token you can hand-make is not signed by Auth Core's key and fails on signature first. The real proof is a unit test that signs a token with a test RS256 key through the real decode path with the claim missing, plus a live regression smoke (normal turn and ListModels still succeed, no 401 spike in model-gateway/inference-core logs).
- dirty/untracked collisions: None of the three source files is dirty. tests/e2e_invoke_chain_test.rs and tests/orchestration_http_test.rs (model-gateway) are modified by another session.
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs; modified: Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs
- cross-design overlap: Contextual CR-T3 and Z-16 edit inference-core (auth.rs is Z-01/Z-16 only; grpc.rs/fallback.rs are CR-T3/Z-05a/Z-16): chain C4 serialises them.
- deploy: S1 - model: rebuild model-gateway and inference-core images (launcher model stage). Safe in any deploy order because Auth Core already emits zdr on every token.
- live proof (none): Free: regression smoke only after the rebuild (one normal chat turn on Terra costs nothing in Verevon Balance; a ListModels call is free). No 401 spike in logs.
- notes_for_implementer: Remove #[serde(default)] on zdr in the three Claims structs. First write the failing tests: sign a token with a test RS256 key, omit zdr, and assert the real verifier rejects it (HTTP extractor in model-gateway auth.rs, the gRPC verifier in grpc_auth.rs, inference-core authenticate); fixtures should be generated from Auth Core's real issuePlaneToken output (export one captured token body into a fixture) so the test uses the true claim shape. Then add posture equality to verify_delegated_user_bearer (compare claims.zdr with model_claims.zdr; mismatch -> 403 with a typed posture_mismatch body). Do not touch the service-inference variant. Fix every test signer that omits zdr in the same commit. Watch the live skew case: Auth Core's org-posture cache means two bearers minted a second apart can differ when an org toggles ZDR; the 403 is the correct fail-closed result and the BFF should surface it as retryable. Prepare for Z-14 by leaving room for an optional zdr_waiver claim (do not add it here).

#### Z-05a: Typed ZDR refusal catalogue and refusal sites in the Model Plane, including the silent-drop closure (Z-05a, absorbs Z-07)

- design: zdr (Z-05); parent: Z-05 | plane: Model | size: L | chain: C4 #5 | kind: code
- depends_on: CR-T3, Z-10
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (reuse existing typed-status machinery; do not invent a second one). Z-07 is merged into this task.
- files:
  - apps/Model Plane/rust/crates/mp-contracts/src/lib.rs (one-line hunk: pub mod zdr_refusal; dirty by 1 line)
  - apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs (new; needs the workspace serde dependency in mp-contracts/Cargo.toml, or a serde-free catalogue)
  - apps/Model Plane/rust/services/model-gateway/src/sse.rs
  - apps/Model Plane/rust/services/model-gateway/src/sse_events.rs (SSE Error frame gains an optional feature; modified +216)
  - apps/Model Plane/rust/services/model-gateway/src/http_routes.rs
  - apps/Model Plane/rust/services/model-gateway/src/browser_run.rs
  - apps/Model Plane/rust/services/execution-core/src/grpc.rs
  - apps/Model Plane/rust/services/execution-core/src/agent_route.rs (untracked, other session)
  - apps/Model Plane/rust/services/execution-core/src/subscription_refusal.rs (untracked, other session; the existing copy-and-pin pattern to extend)
  - apps/Model Plane/rust/services/inference-core/src/grpc.rs (typed refusals through CR-T3's helper; new code at the end of the file)
  - apps/Model Plane/api/contracts/zdr-refusals.json (new JSON export used as a fixture by Rust and the SPA)
- corrections (folded in):
  - Server-side bilingual text duplicates the SPA's i18n (i18n.tr(no, en)) and will drift. Make the Rust catalogue carry stable code + feature + English fallback message + retryable + a small action enum, export it as a JSON fixture (Z-10 style), and have the SPA own Norwegian/English rendering with a test that every code in the fixture has an SPA entry (Z-05b). The product requirement (typed, human-readable NO/EN) is met in the SPA.
  - Wire shape is undefined in the design. Today the surfaces differ: SSE Error frame has only {code,message,retryable} (sse_events.rs ChatEvent::Error, dirty); browser_run.rs returns 412 {error: string}; the speech route returns 412 {error:{code:zdr_unavailable,message}} (the SPA's speech-client.test.ts and VoiceModeDialog.read-aloud.test.tsx pin it, do not break); execution-core and inference-core use message-only failed_precondition. Define one shape per transport (SSE frame adds feature; JSON body {error:{code,message,feature}}; gRPC uses the existing x-mp-provider-error trailer).
  - inference-core already has the mechanism: typed_status and PROVIDER_ERROR_KIND_TRAILER = x-mp-provider-error (grpc.rs ~1343-1450), and execution-core has the copy-and-pin pattern in subscription_refusal.rs pinned by tests/subscription_refusal_contract.rs. Extend those; do not add a parallel trailer. CR-T3 builds typed ZDR/residency/tier statuses and the shared fixture (api/contracts/infer-statuses.json): consume or extend it instead of creating a competing one.
  - The 14 reject_unattested_zdr_modality call sites in inference-core grpc.rs (lines ~196-979) are the enumerable inventory the table test needs; also sse.rs ~982 (zdr_feature_not_supported), ~8136 (zdr_unavailable), ~8149 (zdr_inference_failed), browser_run.rs ~307, execution-core grpc.rs ~652 (enforce_persistence_free_execution) and agent_route.rs validate_run_route. Name them in the test, not a count.
  - Z-07 merged: the refusal list at sse.rs ~968-986 checks only attachments, generate_image, deep_research and the agentic feature. Verified silent drops: browse_web, web-only/knowledge+web scope, client-declared tools, structured_output_schema, plan_mode, artifact/office/file outputs and any other InvokeRequest field the ZDR branch ignores. Do not write per-feature unit tests from memory: enumerate every InvokeRequest field and feature string in a table test classifying each as honored or refused under ZDR, with a compile-time guard (destructure InvokeRequest without a .. rest pattern) so a new field breaks the build; return zdr_feature_not_supported with the feature name. Refusing web search under ZDR is the honest v1 (Quarry's free SearXNG chain still forwards queries to external engines, honest-limits L13); zdr_direct_stream stays tool-free; non-ZDR callers unchanged.
  - Catalogue codes: zdr_unavailable, zdr_feature_not_supported (with feature), zdr_connection_not_supported, zdr_connection_not_acknowledged, zdr_explicit_request_not_supported_by_connection, zdr_decision_unavailable, zdr_waiver_route_mismatch, zdr_posture_mismatch. Frozen kind strings from CR-T3 (zdr_unavailable, residency_unavailable, tier_unavailable) stay.
- dirty/untracked collisions: mp-contracts/src/lib.rs (M, 1 line); model-gateway/src/sse.rs (M +2136); http_routes.rs (M +242); sse_events.rs (M +216); execution-core/src/grpc.rs (M +133); execution-core/src/agent_route.rs (untracked)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/crates/mp-contracts/src/lib.rs; modified: Model Plane/rust/services/model-gateway/src/sse.rs; modified: Model Plane/rust/services/model-gateway/src/sse_events.rs; modified: Model Plane/rust/services/model-gateway/src/http_routes.rs; modified: Model Plane/rust/services/execution-core/src/grpc.rs; untracked: Model Plane/rust/services/execution-core/src/agent_route.rs; untracked: Model Plane/rust/services/execution-core/src/subscription_refusal.rs
- cross-design overlap: Contextual CR-T3 (inference-core grpc.rs, infer-statuses.json): same typed-status area, serialised in chain C4. The SPA mapping is Z-05b.
- deploy: S1 - model: mp-contracts (library), model-gateway, execution-core, inference-core image rebuilds (launcher model stage).
- live proof (none): Free where reachable on a non-ZDR stack; mostly proven by the table test, then Z-60. No paid calls.
- notes_for_implementer: Create mp-contracts zdr_refusal.rs with the stable codes and a JSON export used as a fixture by Rust and the SPA. Write the table test first: enumerate every refusal site named above, assert code, English message, non-empty feature where applicable, retryable flag, and that the gRPC refusals carry the x-mp-provider-error trailer. Add a source-scan test failing if a new ZDR refusal returns an untyped Status::failed_precondition. Keep the speech route's {error:{code,message}} shape. Add the silent-drop table test over InvokeRequest fields (with the compile-time guard). Edit shared dirty files by hunk only. Re-run check-citations.mjs for evidence docs citing inference-core grpc.rs.

#### Z-06: ZDR stream provenance and usage frame in the Model Plane (metering half deferred as Z-06b; SPA hunks moved to Z-20)

- design: zdr (Z-06) | plane: Model | size: M | chain: C4 #6 | kind: code
- depends_on: Z-05a
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: split: (a) provenance and usage frame is sound and stays here; (b) metering and org budget depends on contextual metering work and is deferred (DEF-Z-06b); the SPA hunks move to Z-20.
- files:
  - apps/Model Plane/rust/services/model-gateway/src/sse.rs (direct_infer returns a struct; zdr_direct_stream emits one usage frame; ~7520 and ~8097)
  - apps/Model Plane/rust/services/model-gateway/src/sse_events.rs
- files removed from the design list:
  - apps/Model Plane/rust/services/model-gateway/src/state.rs
  - apps/Model Plane/rust/services/model-gateway/src/budget.rs
  - apps/Model Plane/go/services/cost-core/internal/consumers/consumer.go
  - apps/Frontend Plane/verevonv3/src/shared/api/privacy-tier.ts
  - apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts
- corrections (folded in):
  - Wrong file: apps/Model Plane/go/services/cost-core/internal/consumers/consumer.go is the generic JetStream durable consumer. The envelope rules live in cost-core/cmd/main.go decodeUsageMessage: producer allowlist (usageEnvelopeProducer = model-gateway, ~line 53/430) and "user attribution is required" (~440). cmd/main.go is modified by another session and also edited by CR-T4: part (b) is deferred.
  - Wrong premise for the budget half: model-gateway budget.rs check_budget requires a non-empty user_id and a user-bound cost bearer, so an org-level, no-user budget check before the ZDR branch cannot work until cost-core gains org-level service rows (CR-T4/CR-T10). The no-user envelope would be rejected by cost-core today.
  - DynPublisher::publish in model-gateway/src/state.rs (~line 40) is the correct place that drops envelope.zdr before publish; allowlist one content-free envelope class there, nothing else (part b).
  - direct_infer (sse.rs ~7520) returns only the answer String; the provenance half needs it to return provider_used, residency and token counts (pb::InferResponse fields 8, 9, 5, 6 exist). Do not hard-code tier zdr_contractual: stamp from the response, floored by the request's requirement.
  - OD-Z7 convention: existing suppression of usage for ZDR requests is documented (ledger.md:33; consumer drops ZDR events). The integrator resolved the Data Plane equivalent (matrix row 87 allows ZDR/cost metadata only; emit content-free count rows with zdr=true and record it in the ledger); apply the same convention uniformly to model-gateway in the deferred part.
  - The ZDR answer line cannot be proven live until a provider is attested (Z-40); the waived path only exercises the persistent stream. Do not claim ZDR provenance proven from Z-60.
- dirty/untracked collisions: model-gateway/src/sse.rs (M +2136); sse_events.rs (M +216); cost-core cmd/main.go, ledger.go, store.go, server.go (M) plus untracked migrations 0006/0007 and internal/azurecost/ (part b only)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/model-gateway/src/sse.rs; modified: Model Plane/rust/services/model-gateway/src/sse_events.rs
- cross-design overlap: CR-T4/CR-T10 and quarry T16 (cost-core envelope shape, budget aggregate exclusions) for part (b). Must not add a third envelope kind.
- deploy: S1 - model: model-gateway image (part a). Part (b) only after the cost-core Model stage ships.
- live proof (none): (a) free once an attested provider exists, otherwise unprovable. (b) deferred. A ZDR chat on attested Azure is a paid Azure call that conflicts with the standing Terra-only rule: needs an explicit owner OK for one minimal call. Do not attempt on Verevon Balance.
- notes_for_implementer: Implement (a): change direct_infer to return a small struct (content, provider_used, residency, input/output tokens); zdr_direct_stream emits one usage frame with provider, residency, privacy tier derived from the response (floored at zdr_contractual only because the ZDR chain reaches attested providers), and retention posture; no content in it. The SPA answerProcessingNote work (privacy-tier.ts, chat-client.ts usage frame) is done in Z-20 together with retention_class; pinned wording tests (privacy-tier.answer-line.test.ts, untracked) are extended there, not loosened. Part (b) is DEF-Z-06b.

#### Z-11: mp-contracts retention resolver

- design: zdr (Z-11) | plane: Model | size: M | chain: C4 #7 | kind: code
- depends_on: Z-10, Z-05a (design had: Z-10, Z-05)
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections
- files:
  - apps/Model Plane/rust/crates/mp-contracts/src/lib.rs
  - apps/Model Plane/rust/crates/mp-contracts/src/retention.rs
  - apps/Model Plane/rust/crates/mp-contracts/src/dataplane_posture.rs (helper data_plane_zdr_mode(zdr, waived): waived retrieval defaults to reject, i.e. filtering, so restricted documents never go to the personal account)
- corrections (folded in):
  - mp-contracts/Cargo.toml has serde_json, prost, prost-types and tonic only. A claims view with Deserialize needs the workspace serde dependency; or keep the resolver strictly on plain structs (ClaimsView{zdr,org_zdr,waiver}) and let each plane map its own claims, which is the more robust shape and keeps the crate I/O- and serde-free.
  - lib.rs is dirty with one line (pub mod agentic_subscription). Add pub mod retention; and pub mod zdr_refusal; as one-line hunks; do not rewrite the file.
  - Also host here: WAIVER_CONTRACT_VERSION (for the launcher gauge, Z-32), the helper data_plane_zdr_mode(zdr, waived) for retrieval (critic-zdr #9: waived retrieval runs in ZdrMode::Disabled today, and Reject is the mode that filters restricted documents) and the canonical route-id constants reused from agentic_subscription::PROVIDER_ID.
  - Property test: no input combination yields Persistent for a ZDR token without a verified waiver; add "a waiver with a connection or provider different from the request is Refuse, never Zdr".
- dirty/untracked collisions: apps/Model Plane/rust/crates/mp-contracts/src/lib.rs (M, 1 line)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/crates/mp-contracts/src/lib.rs
- cross-design overlap: None.
- deploy: S1 - model: library; takes effect when Z-15/16/17/18 rebuild.
- live proof (none): None (library).
- notes_for_implementer: Pure functions only. resolve_turn(claims: &ClaimsView, route: &RouteView) -> Resolution {Zdr, Persistent, Waived{connection_id, ack_id}, Refuse(code)} and a route_allowed_under_waiver(waiver, route_kind, provider, connection) used by gateway, inference-core, execution-core, session-core. Run the whole golden matrix through it as the first test. No I/O, no env reads (switches arrive as arguments as in agentic_subscription.rs). Export WAIVER_CONTRACT_VERSION: u32 and the waived-retrieval zdr_mode helper.

#### Z-15: model-gateway waiver enforcement and stamping

- design: zdr (Z-15) | plane: Model | size: L | chain: C4 #8 | kind: code
- depends_on: Z-00, Z-01, Z-05a, Z-06, Z-11 (design had: Z-00, Z-01, Z-05, Z-11)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Model Plane/rust/services/model-gateway/src/auth.rs
  - apps/Model Plane/rust/services/model-gateway/src/normalize.rs
  - apps/Model Plane/rust/services/model-gateway/src/sse.rs
  - apps/Model Plane/rust/services/model-gateway/src/http_routes.rs
  - apps/Model Plane/rust/services/model-gateway/src/sse_events.rs
  - apps/Model Plane/rust/services/model-gateway/src/session_flow.rs
  - apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs
  - apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs (third Claims struct)
  - apps/Model Plane/rust/services/model-gateway/src/dataplane.rs (effective_retrieval_zdr_mode ~42: waived runs retrieve in reject mode)
  - apps/Model Plane/rust/services/model-gateway/src/retrieval.rs (waived retrieval)
  - apps/Model Plane/rust/services/model-gateway/src/retrieval_tools.rs (waived retrieval)
  - apps/Model Plane/rust/services/model-gateway/src/state.rs (expose WAIVER_CONTRACT_VERSION for the launcher gauge)
- corrections (folded in):
  - Claims gain org_zdr and zdr_waiver in BOTH the HTTP (auth.rs) and gRPC (grpc_auth.rs) Claims; every effective_zdr site calls the mp-contracts resolver. The source-scan test needs a NAMED inventory (critic-zdr #7: 72 references in four services: sse.rs 29, http_routes.rs 15, grpc.rs 8, finetune_routes.rs 5, inference-core 7, others), not a count.
  - A waiver requires provider openai-codex-subscription with the bound connection and no explicit zdr, else 403 zdr_waiver_route_mismatch; unknown waiver versions refused.
  - Helper-call compatibility (critic-zdr #13): subscription_scoped_request is used at 4 sites (sse.rs ~7679, 7743, 7796, 7883) plus the summarizer at ~5435; add a route inventory test for EVERY gateway-to-inference call on a waived turn; the existing test at sse.rs ~560-571 is a permissive double (zdr:true with the Terra route is a state normalize.rs:297 and codex_subscription.rs:624 refuse).
  - Waived retrieval (critic-zdr #9): a waived turn mints zdr=false, so retrieval runs in ZdrMode::Disabled by default and restricted-classification documents would flow to the personal OpenAI account. Waived runs call the Data Plane with zdr_mode=reject (filtering) through the Z-11 helper; the acknowledgement text says retrieved org knowledge and tool results go to OpenAI (honest-limits L18). Tool and Quarry calls receive the run's resolved posture (the Quarry body zdr is the run-level effective ZDR after the BYO downgrade: false on a waived run, true on a real ZDR run).
  - Usage frame carries retention_class and reason from the verified claim; bearer posture equality includes the waiver id; expose WAIVER_CONTRACT_VERSION for the Z-32 launcher check.
  - Do mode: a waived Terra Do run still needs AGENTIC_SUBSCRIPTION_ENABLED on (off in the live stack), plan_mode read-only and the gateway setting req.zdr=false (execution-core agent_route.rs ~99-148): not claimed closed.
  - Space ZDR is not applied in v1 (no space id in the decision request; honest-limits L14): state it.
  - Plan: tool_loop.rs (+628, other session) is avoided until that session commits. If it has not, put the tool/Quarry posture pass-through in a small separate module and call it with a one-line hunk in tool_loop.rs.
  - Dirty files: normalize.rs, sse.rs, http_routes.rs, sse_events.rs, tool_loop.rs (other sessions): hunks only; cite function names, not lines.
- dirty/untracked collisions: model-gateway normalize.rs (M), sse.rs (M), http_routes.rs (M), sse_events.rs (M), tool_loop.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/model-gateway/src/normalize.rs; modified: Model Plane/rust/services/model-gateway/src/sse.rs; modified: Model Plane/rust/services/model-gateway/src/http_routes.rs; modified: Model Plane/rust/services/model-gateway/src/sse_events.rs; modified: Model Plane/rust/services/model-gateway/src/tool_loop.rs
- cross-design overlap: QB-T12 (tool_loop.rs) runs after this in chain C4.
- deploy: S7 - model: model-gateway image; dark until Auth Core mints waivers (flag off).
- live proof (none): Z-60 probes Z2 and Z6.
- notes_for_implementer: Source-scan test that no raw claims.zdr or effective_zdr remains outside the resolver (named inventory); golden matrix through real normalize and the stream entry; mismatch cases refused; usage frame test; auxiliary-call test (title, follow-ups) stays on the connection; waived retrieval uses reject mode (counting double). Hunks only in the shared dirty files.

#### Z-16: inference-core route-binding backstop

- design: zdr (Z-16) | plane: Model | size: M | chain: C4 #9 | kind: code
- depends_on: Z-01, Z-11 (design had: Z-01, Z-11)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Model Plane/rust/services/inference-core/src/auth.rs
  - apps/Model Plane/rust/services/inference-core/src/grpc.rs
  - apps/Model Plane/rust/services/inference-core/src/provider/codex_subscription.rs
  - apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs
- corrections (folded in):
  - Owner of fallback.rs for ZDR (integrator: fallback.rs +107 test-only hunk by another session, ZR only): edit by hunk after re-reading; CR-T3 never touched it.
  - AuthenticatedPrincipal carries the waiver; to_internal_request and EVERY modality entry deny any call whose provider and connection differ from the bound ones (PermissionDenied zdr_waiver_route_mismatch). Enumerate the entries by name (the 14 reject_unattested_zdr_modality sites in grpc.rs, lines ~196-979); AuthenticatedPrincipal::for_test is cfg(test) pub(crate) so only inline unit tests can build a waiver principal.
  - The subscription provider stays supports_zdr=false so an explicit request.zdr=true is still refused (effective_zdr is token OR request, auth.rs:193).
  - Test with a broker double enforcing org and user scope like integration-corev2 validateCodexSubscriptionInference: waiver token plus provider azure refused, embeddings refused, bound connection allowed, other member's connection refused.
  - grpc.rs lines are cited by docs/compliance/evidence (11 citations): re-run check-citations.mjs; codex_subscription.rs is dirty (adjacent).
- dirty/untracked collisions: inference-core provider/fallback.rs (M +107); provider/codex_subscription.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/inference-core/src/provider/codex_subscription.rs; modified: Model Plane/rust/services/inference-core/src/provider/fallback.rs
- cross-design overlap: CR-T3 and Z-05a edit inference-core grpc.rs earlier in the same chain.
- deploy: S7 - model: inference-core image; dark until a waiver is minted.
- live proof (none): Free: waiver token (test only) against a non-bound route returns the denial (Z-60).
- notes_for_implementer: Make the waiver binding a first-class check at the inference-core boundary (the backstop if the gateway is bypassed). Mutation check: revert the binding check and the named test must fail.

#### QB-T12: Model Plane consumers: score_kind, rerank status, calibration in relevance.rs

- design: quarry (T12) | plane: model | size: M | chain: C4 #10 | kind: code
- depends_on: QB-T09, QB-T14, Z-15 (design had: T09, T14)
- gates: start [G-T14Q-CALIBRATION, G-T09-DEPLOYED] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP, shrink to the clean files (authored by this manifest from the quarry reviewer headline #5).
- files:
  - apps/Model Plane/rust/services/model-gateway/src/quarry.rs
  - apps/Model Plane/rust/services/model-gateway/src/relevance.rs
  - apps/Model Plane/rust/services/model-gateway/src/relevance_calibration.rs (new module for the calibration table)
  - apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs (coverage line ONLY; dirty +628 from another session; minimal hunk after that session commits)
  - apps/Model Plane/rust/services/model-gateway/src/deep_research.rs (only if its cross-sub-query max needs the calibrated value)
- corrections (folded in):
  - Calibration can live entirely in the clean files quarry.rs and relevance.rs plus a new module: tool_loop.rs and deep_research.rs both re-read the already-projected tool output; the dirty tool_loop.rs (+628/-28, other session) is needed only for the coverage line.
  - Absent score_kind keeps the legacy LLM semantics (byte-identical scoring regression test); score and score_kind are consumed only when Quarry returns them (opt-in negotiation from QB-T09).
  - Chain rule: model-gateway belongs to chain C4; this runs after Z-15 (also edits tool_loop.rs and sse.rs); highlights are deferred so no highlight handling (QB-T06): relevance.rs HIGHLIGHT_ONLY_CREDIT stays untouched.
  - Calibration table (monotone piecewise-linear, <=8 knots) comes from the QB-T14 judged run; re-fit DEFAULT_PROVIDER_WEIGHT/threshold only if the judged data requires; do not invent knots.
  - Live proof must run on gpt-5.6-terra via openai-codex-subscription (standing rule), never Verevon Balance.
- dirty/untracked collisions: apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/model-gateway/src/tool_loop.rs
- cross-design overlap: Z-15 (tool_loop.rs, sse.rs); same chain C4.
- deploy: S13 - model: model-gateway image (model-only launcher copy) after the judged run.
- live proof (none): A Terra-subscription chat turn with web_search for a factual Norwegian question: tool output carries score, the gate keeps on-topic sources and the coverage line mentions rerank when skipped; repeat on a ZDR test org and see the skipped marker surfaced. Terra only; no balance spend.
- notes_for_implementer: quarry.rs parses top-level score_kind and rerank {status, reason}; relevance.rs applies a per-kind calibration before the 0.6 blend; tool_loop.rs coverage line surfaces rerank status. Property tests on the calibration (monotone, f(0)=0, f(1)=1, NaN ignored, absent stays absent); a golden test deserializing the Quarry response goldens (ok, skipped zdr, degraded); a regression test that legacy responses without score_kind score byte-identically; a judged-set derived test that the calibrated gate keeps >=95% of relevant (grade>=2) and drops >=70% of irrelevant results.

### Chain C5 mp-session-exec

#### Z-17: session-core retention receipt and derived-job exclusions

- design: zdr (Z-17) | plane: Model | size: L | chain: C5 #1 | kind: code
- depends_on: Z-11 (design had: Z-11)
- gates: start [G-MP-CONTRACTS] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest; owner default D-B applies)
- files:
  - apps/Model Plane/rust/services/session-core/src/auth.rs
  - apps/Model Plane/rust/services/session-core/src/terminalization.rs
  - apps/Model Plane/rust/services/session-core/src/grpc.rs
  - apps/Model Plane/rust/services/session-core/src/learning_events.rs
  - apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs
  - apps/Model Plane/rust/services/session-core/migrations/0043_run_retention_receipt.sql
  - apps/Model Plane/rust/services/session-core/src/dreaming.rs (skip dreaming and the Letta mirror sync, sync_persisted_candidates_to_letta, for zdr_waived runs)
  - apps/Model Plane/rust/services/session-core/src/dream_extractor.rs (skip for zdr_waived)
  - apps/Model Plane/rust/services/session-core/src/memory_grounding.rs (memory indexing/grounding skip for zdr_waived; verify the call path)
- corrections (folded in):
  - Owner default D-B: on a waived run in an org that has ZDR on, content-derived background jobs that send run content to OTHER processors (memory indexing/mirror, dreaming extraction, learning review) are SKIPPED, regardless of their own switches; the consent text names this (Z-13). The run itself is persisted like a normal run.
  - Migration 0043_run_retention_receipt is free (latest 0042_thread_persona_toggle) but claim it; nullable retention columns default persistent so old rows read as persistent; the receipt is immutable; child runs inherit; the run proof bundle cites the receipt; audit event run_started_zdr_waived is durable.
  - session-core auth.rs:668 compares zdr across delegated bearers (homogeneous token set); ZDR start forces an empty parent_run_id and a fresh thread and ZDR credentials cannot write orchestration events: waived runs follow the normal path (their bearer is zdr=false).
  - Learning review and dreaming use paid models (claude-sonnet-4-5/haiku; both off by compose default after the paid-model leak); agent-memory uses azure/gpt-4o-mini: all skipped on zdr_waived.
  - Real-Postgres tests gated on DATABASE_URL (precedent refuses_zdr_runs_foreign_threads) must be wired to run (CR-N1 pattern) or they silently skip; session_core_rules_double.rs (untracked, execution-core) must call the shared resolver (Z-11), not copied rules.
  - Dirty: session-core auth.rs, grpc.rs, orchestration_grpc.rs: hunks only.
- dirty/untracked collisions: session-core auth.rs (M), grpc.rs (M), orchestration_grpc.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/session-core/src/auth.rs; modified: Model Plane/rust/services/session-core/src/grpc.rs; modified: Model Plane/rust/services/session-core/src/orchestration_grpc.rs
- cross-design overlap: Z-18 (execution-core) and Z-21a (session-core aggregate RPC) follow in chain C5.
- deploy: S7 - model: session-core image; migration 0043 applies at start; dark until a waiver is minted.
- live proof (none): Z-60 probe Z2 (SELECT the receipt, aggregates only; no content).
- notes_for_implementer: VerifiedIdentity.zdr_waiver from the claim; start_managed_run records retention_class and waiver ids (immutable receipt); child runs inherit; learning events, dreaming and memory mirror skip retention_class zdr_waived; run proof bundle cites the receipt; audit event run_started_zdr_waived; real-Postgres tests: waived start persists thread and receipt, a waiver token cannot start a run claiming another connection, learning and memory skipped for zdr_waived, ZDR start still metadata-only; migration reversible.

#### Z-18: execution-core waiver-aware Do and children

- design: zdr (Z-18) | plane: Model | size: M | chain: C5 #2 | kind: code
- depends_on: Z-11, Z-17, Z-05a (design had: Z-11, Z-17)
- gates: start [G-MP-CONTRACTS] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Model Plane/rust/services/execution-core/src/auth.rs
  - apps/Model Plane/rust/services/execution-core/src/grpc.rs
  - apps/Model Plane/rust/services/execution-core/src/agent_route.rs
  - apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs
  - apps/Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs
  - apps/Model Plane/rust/services/execution-core/src/knowledge_tools.rs (waived retrieval zdr_mode=reject through the Z-11 helper; 2 zdr_mode sites)
- corrections (folded in):
  - Read the waiver from the execution bearer; validate_run_route accepts req.zdr=false with a matching waiver and refuses a mismatch; enforce_persistence_free_execution unchanged for real ZDR; children keep the shared ModelRoute and register normally because their session bearer is zdr=false.
  - extend session_core_rules_double.rs (untracked) by calling the shared mp-contracts resolver and session-core's real rules, not copied rules (critic-zdr #13): waived parent and child register with lineage, ZDR parent registers none, waiver for another connection refused; revert-each-guard mutation checks.
  - Waived Do run still needs AGENTIC_SUBSCRIPTION_ENABLED (off in the live stack; the launcher env turned it on only for the A6 proof), plan_mode read-only and gateway req.zdr=false (agent_route.rs ~99-148): do not claim the 2026-10-02 finding closed by construction.
  - Z-05a edited grpc.rs and agent_route.rs earlier (gate G-MP-CONTRACTS); several files are dirty or untracked from other sessions: hunks only.
- dirty/untracked collisions: execution-core src/grpc.rs (M +133), src/agent_route.rs (untracked), src/runtime_loop/agent.rs (M), src/runtime_loop/session_core_rules_double.rs (untracked)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/execution-core/src/grpc.rs; untracked: Model Plane/rust/services/execution-core/src/agent_route.rs; modified: Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs; untracked: Model Plane/rust/services/execution-core/src/runtime_loop/session_core_rules_double.rs; modified: Model Plane/rust/services/execution-core/src/knowledge_tools.rs
- cross-design overlap: Z-05a (same files, earlier).
- deploy: S7 - model: execution-core image; dark until a waiver is minted.
- live proof (none): Z-60 probe Z4 (Do with plan_mode under waiver: children and lineage register, panel frames); Terra only.
- notes_for_implementer: Implement against the shared resolver; the double applies session-core's real rules; mutation checks per guard.

#### Z-21a: session-core read-only aggregate RPC of waived runs per connection and user (Z-21a)

- design: zdr (Z-21); parent: Z-21 | plane: Model | size: S | chain: C5 #3 | kind: code
- depends_on: Z-17 (design had: Z-17, Z-19)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW split from Z-21 (Model Plane half).
- files:
  - apps/Model Plane/rust/services/session-core/src/grpc.rs (dirty)
  - apps/Model Plane/proto/ session-core service proto (new RPC; counts only)
- corrections (folded in):
  - Counts and last-used date only (no content), scope-gated for the BFF.
  - A proto change: after any Model Plane proto change, cargo check the Data Plane v2 workspace as well (graph-index-rs and embedding-engine-rs build Model Plane proto messages as struct literals and break on new fields; with --from control a build error 101 hides behind parallel logs: rerun with COMPOSE_PARALLEL_LIMIT=1). Adding an RPC (not a message field they construct) is expected to be safe: verify.
- dirty/untracked collisions: session-core src/grpc.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/rust/services/session-core/src/grpc.rs
- cross-design overlap: Z-21b consumes it.
- deploy: S7 - model: session-core image.
- live proof (none): Z-60 probe Z2: count equals 1.
- notes_for_implementer: Real-Postgres aggregate test; response schema has no content field.

### Chain C6 mp-cost-core

#### CR-T4: cost-core: feature and units on the ledger, org-level service rows, catalogue rows, feature aggregation

- design: contextual (T4) | plane: model | size: M | chain: C6 #1 | kind: code
- depends_on: none (design had: T2)
- gates: start [G-CORECOST-OTHER-SESSION] done [G-T10-GOLDEN] deploy [G-CP-P1]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: APPROVE WITH CORRECTIONS, but BLOCKED until the other session's cost-core work is committed and deployed. Missing the budget-exclusion fix the integrator requires. Merged with quarry T16.
- files:
  - apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go
  - apps/Model Plane/go/services/cost-core/internal/postgres/store.go
  - apps/Model Plane/go/services/cost-core/internal/server/server.go
  - apps/Model Plane/go/services/cost-core/internal/server/server_test.go
  - apps/Model Plane/go/services/cost-core/internal/pricing/pricing.go
  - apps/Model Plane/go/services/cost-core/migrations/0008_usage_feature.up.sql (new)
  - apps/Model Plane/go/services/cost-core/migrations/0008_usage_feature.down.sql (new)
  - apps/Model Plane/go/services/cost-core/cmd/main.go
  - apps/Model Plane/go/services/cost-core/cmd/main_test.go
  - apps/Model Plane/go/services/cost-core/migrations/0002_model_pricing.up.sql (catalogue seed rows together with defaultSeed)
  - apps/Model Plane/go/services/cost-core/internal/ledger/ledger_test.go
  - apps/Model Plane/go/services/cost-core/internal/postgres/store_test.go
  - apps/Model Plane/go/services/cost-core/internal/postgres/store_integration_test.go
  - apps/Model Plane/go/services/cost-core/internal/pricing/pricing_test.go (add catalogue/seed parity check)
  - vendored sha256-pinned copy of the forwarder request golden authored by CR-T10 (Go internal/ packages cannot be imported across modules)
- corrections (folded in):
  - All six named Go files carry another session's uncommitted work (ledger.go +82, store.go +124, server.go +98, main.go +19, tests) plus untracked migrations 0006/0007, internal/azurecost/ and docs/. Do not start until that lands and its image is deployed (the cost-core live image predates its working tree; live cost_entries still lacks provider_used). Edit in place; the dirty files are LF in the working copy, keep that.
  - Add the budget fix (integrator): handleBudgetCheck, org-wide branch (empty user_id), calls Aggregate(AggregateFilter{OrgID}) and sums every row including producer rows. inference-core intent.rs and model-gateway budget.rs both call /api/v1/budget/check, so background spend would downgrade chat. Add an exclusion/chat-only predicate to AggregateFilter in BOTH stores (in-memory ledger.Store.matches and Postgres buildWhere). /cost/aggregate keeps counting everything and gains feature filter and group-by. Check model-gateway verevon_actions.rs:452 (consumes /cost/aggregate).
  - Pricing: model_pricing is per-token only. No embed-v-4-0 or Cohere-rerank-v4.0-pro price exists in the repo and none may be invented (owner or operator supplies, see OPS-PRICE). A record with tokens and cost 0 and no basis is priced from the catalogue, so unknown models fall to the default 3.00/15.00 (about 25x for embeddings). Keep the forwarder dark until sourced rows are deployed. Prefer cost-core-side unit pricing (Data Plane sends units, not USD) over explicit cost_usd computed in the Data Plane.
  - Migration 0008 must be idempotent: docker-entrypoint.sh re-applies every migrations/*.up.sql on every boot (ADD COLUMN IF NOT EXISTS, CREATE ... IF NOT EXISTS). The catalogue rows in the 0002 seed and defaultSeed change together, so migrations/0002_model_pricing.up.sql belongs in the file list.
  - decodeRequestJSON uses DisallowUnknownFields: feature, units (and provider_used) must be live in cost-core before any sender; gate the forwarder with a flag, ship it dark.
  - cost_entries has a GLOBAL unique index on non-empty idempotency_key (not per org): forwarded keys must embed the org (the Data Plane already stores org-prefixed keys in cost_events).
  - Both the in-memory ledger.Store and the Postgres Store, ValidateEntry bounds, entryResponse and usageResponse need the new fields; ledger_test.go, store_test.go and store_integration_test.go are missing from the list.
  - Quarry T16 merge: producer allowlist becomes a set (cmd/main.go usageEnvelopeProducer ~53, ~430) only if a NATS producer envelope is still required; the chosen path is the HTTP /api/v1/cost/record forwarder, so NO new NATS producer class is added here. Z-06b (ZDR content-free envelope class) is deferred.
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/go/services/cost-core/internal/ledger/ledger.go; modified: Model Plane/go/services/cost-core/internal/postgres/store.go; modified: Model Plane/go/services/cost-core/internal/server/server.go; modified: Model Plane/go/services/cost-core/internal/server/server_test.go; modified: Model Plane/go/services/cost-core/cmd/main.go; modified: Model Plane/go/services/cost-core/cmd/main_test.go
- cross-design overlap: Quarry T16/T07 (same four cost-core files: units/unit_kind, producer allowlist) merged here. ZDR Z-06b touches cost-core cmd/main.go decodeUsageMessage (metadata-only metering): deferred, keep out of this task.
- deploy: S2 - model: cost-core image rebuild (Go); migration 0008 applies at container boot via docker-entrypoint.sh. No env change. Must precede enabling the Data Plane forwarder.
- live proof (none): Free: one zero-token record for a TEST org with a data-orchestrator token, read back via /api/v1/cost/aggregate?feature=...; then prove /api/v1/budget/check for that org is unchanged after the Data Plane row is recorded. The zero-token test row stays in that org: use the test org, never a customer org.
- notes_for_implementer: Build: optional validated feature (bounded charset) and non-negative units on recordRequest, ledger.Entry, ValidateEntry, both stores, cost_entries (migration 0008, nullable or defaulted, IF NOT EXISTS), entryResponse and usageResponse; a feature filter plus group-by on handleAggregate; an exclusion predicate used ONLY by the org-wide branch of handleBudgetCheck so chat budgets keep seeing chat rows (feature NULL or empty). Catalogue: add sourced rows to migrations/0002 and defaultSeed together (pricing_test has no parity check, add one); do not invent prices. If rerank is priced per search unit, add a per-unit rate in the catalogue and let cost-core compute from units. Traps: the unique idempotency index is global; unknown JSON fields 400 (that is the deploy-order reason); do not change model-gateway rows (assert per-producer counts unchanged); the dirty files must be merged in place with no stash or checkout. Contract tests with REAL rules: post the exact forwarder JSON (a vendored golden authored by CR-T10, sha256-pinned here and in the Data Plane tree) through the real Handler with the real CostAuthorizer and a generated JWKS service token holding cost:write; same body plus an unknown field returns 400; a user principal is refused; replay returns 202 without a second row; producer-only rows (no user_id) validate; add a store integration test (TEST_DATABASE_URL) that records a feature row and asserts BudgetCheck totals exclude it while Aggregate includes it, and that re-running every migration file twice succeeds.

### Chain C7 cp-contracts

#### CR-T17: Cross-plane dependency manifest, real-policy contract spec and live smoke matrix

- design: contextual (T17) | plane: control | size: M | chain: C7 #1 | kind: code
- depends_on: CR-T3 (design had: T1, T2, T3, T4)
- gates: start [G-CP-P1-CODE] done [] deploy [G-CP-DEPLOYED]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest). One Control-side manifest spec for CR and QB; the Quarry TS half (QB-T02) merges here.
- files:
  - apps/Control Plane/config/plane-calls.manifest.json (new)
  - apps/Control Plane/auth-core/src/auth/plane-calls-manifest.contract.spec.ts (new; also hosts the Quarry rows from QB-T02's vendored manifest)
  - apps/Control Plane/auth-core/src/auth/fixtures/ (vendored, sha256-pinned copies: quarry cross-plane-token-requests.json, plane-service-principals.json slice)
  - apps/Data Plane v2/scripts/plane-call-smoke.sh (new)
  - apps/Data Plane v2/tests/scripts/plane-calls-contract-test.sh (new sibling; do NOT edit compose-security-contract-test.sh)
- corrections (folded in):
  - Do not edit the untracked, other-session plane-service-principals-policy.contract.spec.ts (describeIfPolicy no-ops when the registry file is absent from the auth-core build context): write a NEW spec that FAILS (expect.fail) when REQUIRE_CONTRACT or CI is set and the registry or a vendored fixture is missing. auth-core's build context is ./auth-core: vendor everything it reads under auth-core/src/auth/fixtures/ with sha256 pins.
  - Quarry rows come from the Quarry manifest (crates/quarry-runtime/contracts/cross-plane-token-requests.json) vendored here by hash; the three model-gateway/models:invoke rows are granted:false and the spec asserts the real registry REFUSES them, flipping only when a model path is granted (D16 is not blocked).
  - Rows required: embedding-engine/graph-index to inference-core (retention tenant, inference:invoke), data-orchestrator to cost-core (cost:write held by exactly one principal, only on audience cost-core), quarry-edge data:read, documents:write+org:data:read_all (ingest) and data:rerank (separate requests), the BFF retention-decision principal (added when Z-14 lands), model-gateway rows granted:false. Rows not yet granted before P1 are pending, and the spec fails only under REQUIRE_CONTRACT.
  - The smoke script performs a real mint per row (status only), prints no tokens or org ids, and reads the live registry only through a node filter that projects audiences and scopes (two earlier probes leaked a credential prefix).
  - Cross-plane compose posture assertions go in a new sibling script, not in the shared compose-security-contract-test.sh.
- dirty/untracked collisions: apps/Control Plane/auth-core/src/auth/plane-service-principals-policy.contract.spec.ts (untracked, other session): do not edit
- cross-design overlap: Quarry T02 TS half merged here. CP wave P1 owns the registry; Z-14 later adds its rows.
- deploy: S10 - No image of its own: spec and scripts only; the smoke matrix runs against the live stack during S10 and again before the paid proofs.
- live proof (none): No paid call. Run the matrix on the live stack after the CP wave and the Data/Model foundations; every row green before the paid proofs of CR-T7/CR-T8/CR-T12 run (it would have caught all six A6 defect classes).
- notes_for_implementer: Commit a machine-readable manifest of every cross-plane call these designs rely on (caller, audience, scopes, retention posture, feature, the file that makes the call, granted flag). The spec loads the REAL registry and manifest and asserts each granted call is authorized by the real authorizePlaneServicePrincipal, no registry grant lacks a manifest owner, retention values match, granted:false rows are refused. Spec first: it must fail on today's tree for the missing data-orchestrator row, the unmanifested quarry-edge grants and the missing tenant posture; the smoke script gets a dry mode that asserts it never prints secrets (grep the output).

#### Z-14: Auth Core decision endpoint, connection-bound waiver claim and kill switch (principal rows are CP wave P1)

- design: zdr (Z-14) | plane: Control | size: L | chain: C7 #2 | kind: code
- depends_on: Z-10, Z-13, CR-T17 (design had: Z-03, Z-10, Z-13)
- gates: start [G-CP-P0A-CODE, G-CP-P1-CODE] done [] deploy [G-CP-DEPLOYED]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest; principal part covered by the Control Plane wave)
- CP wave boundary: CP wave P1 owns the registry rows: the SEPARATE BFF retention-decision principal (not verevon-gateway/MAIL_TASK_GATEWAY_BRIDGE_SERVICE_API_KEY), audience control-policy, scope retention:zdr-decide, launcher env wiring and the bootstrap/registry parity (P0c). This task contains only the decision controller/service, claim stamping and the kill switch.
- files:
  - apps/Control Plane/auth-core/src/auth/zdr-waiver-decision.controller.ts (new)
  - apps/Control Plane/auth-core/src/auth/zdr-waiver-decision.service.ts (new)
  - apps/Control Plane/auth-core/src/auth/plane-token.controller.ts (dirty; after the CP wave merges)
  - apps/Control Plane/auth-core/src/auth/model-plane-token.controller.ts (dirty; the second token caller, critic-contextual #2)
  - apps/Control Plane/auth-core/src/auth/convex-token.service.ts (dirty; after the CP wave merges)
  - apps/Control Plane/auth-core/src/auth/zdr-waiver-decision.contract.spec.ts (new; do not extend the untracked other-session policy spec)
  - apps/Control Plane/auth-core/src/auth/fixtures/zdr-posture-matrix.json (vendored sha256-pinned from Z-10)
- corrections (folded in):
  - Decision: POST /api/retention/zdr-decision verified with verifyPlaneServiceToken on aud=control-policy and scope retention:zdr-decide; resolves org posture through the CP wave's read path, reads connection posture as the SESSION USER from integration-corev2 (Z-13), applies the rule, returns a 60 s signed decision and audits with a durable PubAck. Audit failure refuses issuance.
  - Token mint accepts x-retention-decision, verifies it against the cookie session (a decision for user A cannot mint for user B; expired or wrong-org decisions fail) and stamps zdr=false, org_zdr, zdr_waiver (connection-bound, versioned; enforcers refuse unknown versions), TTL capped at 300 s. Handle BOTH token controllers (plane-token.controller.ts and model-plane-token.controller.ts). internal-token and token-exchange routes keep rejecting any x-zdr or body zdr and ignore the decision header; interactive routes ignore client-supplied zdr.
  - Kill switch ZDR_WAIVER_ENABLED default off; with the flag off the endpoint answers waiver_disabled and mints nothing. The flag may only be turned on after every enforcer reports the waiver contract version (Z-32 launcher check). Enforcers before minters.
  - Single unknown-posture rule: org-core unreachable with no last-known posture means 503 retention_posture_unavailable and no waiver token (consistent with the CP wave; never mint persistent for a decision).
  - The token set must be homogeneous: every mint site carries the same decision (BFF mint sites are Z-19); execution-core and session-core compare zdr across delegated bearers and fail closed.
  - Plan-lapse semantics follow OD-LAPSE (default: intent=true stays ZDR until an admin turns it off); the golden matrix row is the single answer.
  - Edit the dirty auth-core files only after the CP wave has merged its P0 changes (the plan: P0 first, then the ZR waiver), by hunk; no stash or checkout.
- dirty/untracked collisions: auth-core plane-token.controller.ts, model-plane-token.controller.ts, convex-token.service.ts (M); plane-service-principals.json (M), run-control-plane.sh (M); plane-service-principals-policy.contract.spec.ts (untracked)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Control Plane/auth-core/src/auth/plane-token.controller.ts; modified: Control Plane/auth-core/src/auth/model-plane-token.controller.ts; modified: Control Plane/auth-core/src/auth/convex-token.service.ts
- cross-design overlap: CP wave P0/P1 (same files); CR-T17 (manifest spec lists the new row).
- deploy: S10 - control: auth-service image via the control-only launcher copy (LAST_STACK_INDEX=0) with ZDR_WAIVER_ENABLED=false. A newly required credential must be minted first with run-control-plane.sh config (output to /dev/null) or the launcher fails validation.
- live proof (none): Z-60 probes Z0/Z1 (free): ZDR test org mints zdr:true; a direct token mint carries no waiver; the decision for another user is refused; kill switch off returns waiver_disabled.
- notes_for_implementer: Policy contract: write the golden-matrix spec first against the real decision function; specs that internal-token and token-exchange still reject x-zdr and body zdr, interactive routes ignore them, a decision for user A cannot mint for user B, expired or wrong-org decisions fail, audit failure refuses issuance. Mutation checks for each guard (A6 style: revert a single enforcement point and a named test must fail). Pin who holds retention:zdr-decide through the CR-T17 manifest spec.

#### Z-32: Launcher, compose and flag wiring

- design: zdr (Z-32) | plane: Infra | size: S | chain: C7 #3 | kind: code
- depends_on: Z-14 (design had: Z-14)
- gates: start [G-CP-P1-CODE] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest; registry reconcile is CP wave P0c)
- files:
  - apps/Control Plane/docker-compose.yml
  - apps/Control Plane/scripts/run-control-plane.sh
  - apps/Control Plane/config/README.md
  - apps/Model Plane/deploy/docker-compose.yml
  - apps/Ingestion Plane/docker-compose.yml
  - apps/Model Plane/scripts/tests/release-runtime-partition-test.sh (style for the launcher check test)
  - apps/Frontend Plane/verevonv3/build-verevon-services.sh (only if the launcher check lives there)
- files removed from the design list:
  - scripts/bootstrap_runtime_environment.sh
- corrections (folded in):
  - Reconciling the second registry in scripts/bootstrap_runtime_environment.sh (14-principal inline registry) is CP wave P0c (registry unification): removed here.
  - Keep: ZDR_WAIVER_ENABLED (Auth Core, default off), integration core URL for Auth Core, BFF decision credential env (from P1), WAIVER_CONTRACT_VERSION wiring; the launcher check: the Auth Core flag cannot be on unless gateway, inference-core, session-core and execution-core report the waiver contract version (enforcers before minters).
  - All the compose and launcher files are dirty (other sessions): hunks only; AGENTIC_SUBSCRIPTION_ENABLED is currently passed in the launcher env only and compose defaults stay off: do not change that here (gap-closure status).
  - The launcher lives at apps/Frontend Plane/verevonv3/build-verevon-services.sh; the release partition tests live in apps/Model Plane/scripts/tests/.
- dirty/untracked collisions: Control Plane docker-compose.yml, scripts/run-control-plane.sh, config/README.md (M); Model Plane deploy/docker-compose.yml (M); Ingestion Plane docker-compose.yml (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Control Plane/docker-compose.yml; modified: Control Plane/scripts/run-control-plane.sh; modified: Control Plane/config/README.md; modified: Model Plane/deploy/docker-compose.yml; modified: Ingestion Plane/docker-compose.yml
- cross-design overlap: CP wave P1 (same files); chain C7 sequential after Z-14.
- deploy: S10 - control: launcher run with ZDR_WAIVER_ENABLED=false; the flag stays off until the proof window (S14).
- live proof (none): Free: launcher dry run; the flag stays off until the proof window.
- notes_for_implementer: Launcher script tests (release-runtime-partition style) failing when the flag is on and a plane lacks the contract version; deployment-contract spec for new env.

### Chain C8 ing-integration

#### Z-12: integration-corev2 schema, capability assessor and backfill

- design: zdr (Z-12) | plane: Ingestion | size: L | chain: C8 #1 | kind: code
- depends_on: Z-10 (design had: Z-10)
- gates: start [G-GOLDEN-Z10] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (side table, no cascade FK, forward-only migrations)
- files:
  - apps/Ingestion Plane/integration-corev2/internal/db/migrations/0013_model_connection_zdr.sql (new; forward-only)
  - apps/Ingestion Plane/integration-corev2/internal/db/migrate_upgrade_test.go (bump latestMigration from 0012_ to 0013_)
  - apps/Ingestion Plane/integration-corev2/internal/store/types.go
  - apps/Ingestion Plane/integration-corev2/internal/store/postgres.go
  - apps/Ingestion Plane/integration-corev2/internal/store/memory.go
  - apps/Ingestion Plane/integration-corev2/internal/store/atomic_audit.go
  - apps/Ingestion Plane/integration-corev2/internal/zdr/assessor.go (new)
  - apps/Ingestion Plane/integration-corev2/internal/zdr/testdata/connection-zdr-capabilities.json (vendored sha256-pinned copy; go:embed)
  - apps/Ingestion Plane/integration-corev2/internal/api/codex_subscription.go (dirty; hunks only; ensure capability fields are not writable through generic upsert/patch)
- corrections (folded in):
  - Migration 0013 is free (latest is 0012_email_sync_history_backfill.sql), but internal/db/migrate_upgrade_test.go:17 pins latestMigration = 0012_...; bump it. The runner (internal/db/migrate.go) applies embedded forward-only SQL: there are NO down migrations, so the design's "migration up and down" test cannot exist; test apply, idempotent re-apply and upgrade from a 0012 schema fixture.
  - Data model: the integrator and critic chose a SIDE table for capability (not columns on integration_connections) and an append-only ack ledger with NO ON DELETE CASCADE foreign key (the 0004_platform_primitives.sql pattern cascades). Connections are soft-deleted (MarkConnectionDeleted sets deleted_at) so the ledger must not depend on the connection row. The design's "FK ON DELETE CASCADE" is wrong.
  - UpsertConnection is read-modify-write (the poll route in codex_subscription.go ~122 can write back a stale digest): capability fields live in the side table and are not writable through generic upsert or patch; only integration-corev2 computes the digest (Auth Core compares it as an opaque string, avoiding Go/TypeScript canonical-JSON drift).
  - The capability contract is vendored from api/contracts/connection-zdr-capabilities.json (go:embed); the cross-check against inference-core's codex supports_zdr=false lives in a Rust test (Go cannot link Rust).
  - The 5 live connections (2 active, 2 pending, 1 needs_refresh; consents table empty) are backfilled not_supported with no acknowledgements; the backfill is idempotent; a digest change revokes dependent acks and writes an audit row in the same transaction.
- dirty/untracked collisions: apps/Ingestion Plane/integration-corev2/internal/api/codex_subscription.go (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Ingestion Plane/integration-corev2/internal/api/codex_subscription.go
- cross-design overlap: Z-13 continues in the same package and files; chain C8 sequential.
- deploy: S8 - ingestion: integration-corev2 image rebuild; migration 0013 applies at start; run the idempotent backfill.
- live proof (none): Free: backfill on the live DB, SELECT aggregates show 5 rows not_supported.
- notes_for_implementer: Add the side table integration_connection_zdr (capability supported/not_supported/cannot_verify, basis, basis_ref, checked_at, digest) and the append-only integration_connection_zdr_acknowledgements ledger (no FK cascade; partial unique on the active ack per connection and digest). Assessor loads the shared capability contract; assess at connect start and at connected; idempotent backfill of the 5 live connections; digest change revokes dependent acks and audits. Postgres-backed tests as postgres_audit_outbox_test.go: apply, re-apply, upgrade from a 0012 fixture, backfill idempotency, digest invalidation in the same transaction as the audit outbox row; a cross-plane test (Rust) that the contract table equals inference-core provider capabilities (codex supports_zdr=false).

#### Z-13: Acknowledgement API with audit, revocation and admin list

- design: zdr (Z-13) | plane: Ingestion | size: L | chain: C8 #2 | kind: code
- depends_on: Z-12 (design had: Z-12)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest from critic-zdr #3, #9, #15 and the owner defaults)
- files:
  - apps/Ingestion Plane/integration-corev2/internal/api/zdr_acknowledgement.go
  - apps/Ingestion Plane/integration-corev2/internal/api/server.go
  - apps/Ingestion Plane/integration-corev2/internal/api/codex_subscription.go
  - apps/Ingestion Plane/integration-corev2/internal/zdr/texts
  - apps/Ingestion Plane/integration-corev2/internal/zdr/texts/ (versioned immutable NO and EN text files with sha256; drafted `supported` text included, placeholders pending counsel sign-off OD-LEGAL)
- corrections (folded in):
  - Ack and posture routes are bearer-only (NOT internalOrBearerAuth): enforce principal.UserID == connection.UserID with no internal-key bypass (scopedCodexSubscriptionConnection skips the owner check for internal-key calls, codex_subscription.go:~455) and add a test that sends X-Internal-API-Key and is refused. The generic connection routes (GET /api/v1/connections, /:id, /:id/consents, server.go:318-437) check org only: do not add ack data to them; the personal-connection exposure is recorded as a follow-up for the generic routes.
  - Auth Core reads the posture as the session user (new Control-to-Ingestion read edge, record it in the contract matrix via Z-30); a user-bound posture read must return the opaque digest, text version and ack state only.
  - The ack text must say retrieved org knowledge and tool results are sent to OpenAI (honest-limits L18) and that content-derived background jobs on a waived run (memory indexing, dreaming extraction, learning review) are SKIPPED (owner default D-B: the consent text names this). Draft the `supported` text too (the owner wording says the user is told when the connection DOES support ZDR; none was drafted) and a stronger warning when org ZDR is on (it contradicts the org setting). Both languages; versioned, immutable, digest-pinned; legal sign-off is OD-LEGAL (placeholders until then).
  - Disconnect revokes (connection_removed) in the same transaction; org purge removes rows; replay is idempotent; stale digest 409; non-owner 403; wrong org 403; existing consent endpoints untouched; NATS event and audit written atomically (outbox).
  - server.go and codex_subscription.go are dirty (other session): hunks only.
- dirty/untracked collisions: apps/Ingestion Plane/integration-corev2/internal/api/server.go (M); internal/api/codex_subscription.go (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Ingestion Plane/integration-corev2/internal/api/server.go; modified: Ingestion Plane/integration-corev2/internal/api/codex_subscription.go
- cross-design overlap: Z-14 reads the posture endpoint (cross-chain gate G-Z13-CODE).
- deploy: S8 - ingestion: integration-corev2 image rebuild via the Ingestion stage copy (LAST_STACK_INDEX=4).
- live proof (none): Z-60 probes Z1 and Z5 (free): other member 403, stale digest 409, X-Internal-API-Key refused.
- notes_for_implementer: Routes under /api/v1/model-connections/:id: GET zdr (assessment, ack state, composed text for the locale), POST zdr-acknowledgement (owner only, digest and text version checked, text_snapshot and sha stored, audit and NATS event in one transaction), DELETE (owner revoke), GET admin list per org (counts-only fields plus ids), and a user-bound posture read for Auth Core. Tests first for: non-owner 403, wrong org 403, stale digest 409, replay idempotent, text snapshot and sha stored, disconnect revokes, org purge removes rows, internal-key refused, existing consent endpoints untouched; wording snapshot test pinning both languages.

### Chain C9 ing-quarry

#### Z-08: Quarry takes the token zdr claim as a floor

- design: zdr (Z-08) | plane: Ingestion | size: M | chain: C9 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (narrowed by the integrator: claim applies to user tokens only)
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/routes.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/answer_routes.rs (~87, ~182, ~188)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/extract_routes.rs (~234)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs (~1436, body.zdr)
- corrections (folded in):
  - Integrator ruling: read the token zdr claim ONLY for principal_type=user. Service tokens to the quarry audience (model-gateway and execution-core hold retentionByAudience quarry=zdr in the live registry) are always zdr:true, so reading the claim for service principals would force ZDR on every web search of every org, disable paid providers and bypass caches. For service callers the run's posture comes from the body flag, set by the gateway/execution-core from the run's resolved posture (false on a waived run).
  - quarry-edge Claims (auth.rs) has NO zdr field at all (verified). Add #[serde(default)] zdr: Option<bool>. About a dozen Claims struct literals in auth.rs tests and dev_bypass_stub_claims() need the field. Pure serde default means a missing claim is None, not an auth failure.
  - The file list is incomplete although the approach says search, answer, extract, map and agent routes: the ZDR sites are search_routes.rs (const fn effective_zdr at ~144, used at ~628), routes.rs (scrape ~414, crawl/batch ~922, scrape_stream ~1025), answer_routes.rs (~87, ~182, ~188), extract_routes.rs (~234) and agent_routes.rs (~1436, body.zdr). Add answer_routes.rs, extract_routes.rs, agent_routes.rs.
  - tests/zdr_behavior.rs spawns the app with QUARRY_EDGE_AUTH_DEV_BYPASS=1, a stub identity with no zdr claim: a permissive double for this change. The contract test must sign real RS256 tokens through require_auth (auth.rs has real-JWKS tests to copy) with principal_type user and zdr:true plus body zdr:false, and a service token (zdr:true by registry) with body zdr:false that must still run non-ZDR.
  - "Require zdr from service callers on /v1/search" (400 zdr_required) is QB-T09's item, not this one; do not implement it twice. This task owns Claims.zdr and the effective_zdr helper across all five route families so QB-T09 only consumes them. Remove the existing const fn effective_zdr(requested: bool).
  - Quarry crates build and test only in Docker: docker build -f deploy/compose/Dockerfile.edge --target build . (compile) and Dockerfile.test (tests); never cargo check on the host (libclang).
- dirty/untracked collisions: None of the Quarry-v2 files is dirty.
- cross-design overlap: Quarry QB-T09a/T10/T09 (search_routes.rs, auth.rs): same chain C9, this task first.
- deploy: S9 - ingestion: rebuild the quarry-edge image through the Ingestion launcher stage copy (Docker-only build). No config change.
- live proof (none): Free: one web search under a non-ZDR user token with allow_paid_providers:false returns as before (SearXNG free chain; no paid call). The ZDR path needs a ZDR test org.
- notes_for_implementer: Add zdr: Option<bool> to Claims and a helper effective_zdr(claims, requested) = requested || (claims.principal_type == user && claims.zdr == Some(true)); replace the const fn at the five route families. First write the failing tests with real signed tokens: (1) user token zdr:true + body zdr:false -> paid providers off, cache bypassed, ZDR behaviours on; (2) service token (zdr:true by registry row, built from the real registry JSON entry) + body zdr:false -> non-ZDR behaviour unchanged (the regression the critic caught); (3) user token zdr:false + body zdr:true -> still ZDR (monotonic). Extend tests/zdr_behavior.rs only for body-level cases; put the token cases in auth-level tests that go through require_auth with a test JWKS.

#### QB-T02: Quarry cross-plane token-request manifest and its Auth Core contract

- design: quarry (T02) | plane: ingestion | size: M | chain: C9 #2 | kind: code
- depends_on: none (design had: T01)
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: KEEP, SHRINK AND RELOCATE. Keep the Rust half (manifest-driven token requests) in the Quarry crate; the TypeScript half merges into the single Control-side manifest spec of CR-T17 (no second spec file).
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json (new; inside the Docker build context)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/service_tokens.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/vector_index.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/ingest_client.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/mp_client.rs (lines ~122 and ~230)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/audio_routes.rs (line ~133)
- corrections (folded in):
  - Manifest location is wrong: Quarry-v2/contracts/ is not in either Docker build context (Dockerfile.edge and Dockerfile.test copy only Cargo.toml, Cargo.lock, .cargo, crates, lab), so include_str! of it would not compile in the image. Put it at crates/quarry-runtime/contracts/cross-plane-token-requests.json (include_str!("../contracts/...") from src/service_tokens.rs).
  - The manifest is described as the single source for EVERY token request Quarry makes, but the code also builds Model Plane requests at mp_client.rs:122 and :230 and audio_routes.rs:133 (all models:invoke on audience model-gateway). The Rust "enumerate every constructor" test will fail on them and a TS test that refuses any model-gateway entry contradicts them. Fix: list them as rows with granted:false (feature flag named), the TS spec asserts the registry REFUSES them, and the row flips only when a model path is granted.
  - The Auth Core spec cannot read Quarry-v2 files from the auth-core Docker context and the existing policy spec silently no-ops when its file is absent (describeIfPolicy). Use a vendored copy under apps/Control Plane/auth-core/src/auth/fixtures/ pinned by sha256 on both sides (CR-T17 vendors it), and make the spec FAIL, not skip, when REQUIRE_CONTRACT or CI is set.
  - Do not create a second spec mechanism: CR-T17 already plans plane-calls.manifest.json plus a real-policy spec. Quarry's manifest is the machine-consumed source for the quarry-edge rows and CR-T17 vendors it. The proposed plane-service-principals-callers.contract.spec.ts is NOT created.
  - ServiceTokenRequest::new is private and the type is built in 5 production call sites (ingest_client.rs:110, vector_index.rs:145, the new rerank client, mp_client x2, audio_routes). Add a pub(crate) from_manifest(name) constructor and delete the inline literals; keep the sort/dedup/validate behaviour (reason 3..500 chars, scopes non-empty).
- dirty/untracked collisions: None of the Quarry files is dirty.; apps/Control Plane/config/plane-service-principals.json is dirty (read-only reference).; plane-service-principals-policy.contract.spec.ts is untracked (do not edit).
- cross-design overlap: Overlaps CR-T17 (cross-plane manifest + real-policy spec + smoke matrix) and CP wave P1. QB-T02 owns the Quarry manifest file and the Rust loader; CR-T17 owns the one Control-side spec.
- deploy: S9 - ingestion: Rust refactor with no behaviour change; ships inside the quarry-edge image together with QB-T09a/T10/T09. Compile check: docker build -f deploy/compose/Dockerfile.edge --target build; unit tests via deploy/compose/Dockerfile.test.
- live proof (none): Free. Compare manifest rows to the projected live registry (audiences and scopes only, through a node filter; never print env var values or key names with values). Zero mismatches expected after CP wave P1.
- notes_for_implementer: Create crates/quarry-runtime/contracts/cross-plane-token-requests.json with one row per token request: {name, audience, scopes[], reason, granted(bool), used_by(file), feature_flag?}. Rows: vector-retrieve (data-plane, [data:read], "retrieve verified organization evidence"), ingest (data-plane, [documents:write, org:data:read_all], "persist verified Quarry evidence"), web-rerank (data-plane, [data:rerank], "rank web search candidates"), and three granted:false rows for the Model Plane callers (models:invoke on model-gateway: mp_client invoke, mp_client stream, audio). In service_tokens.rs add a TokenRequestSpec loaded with include_str! and ServiceTokenRequest::from_manifest(name) (private new() stays private); replace the literals at ingest_client.rs:110, vector_index.rs:145, mp_client.rs:122/230 and audio_routes.rs:133. Rust tests (real code, not doubles): (a) for every manifest row the production constructor yields identical audience/scopes/reason; (b) a source scan test (include_str! of the five source files) asserts no ServiceTokenRequest::data_plane( or model_plane( literal remains outside the manifest loader; (c) scopes sorted/deduped, reason 3..500 chars; (d) the vector-retrieve row never contains org:data:read_all and ingest and rerank are separate requests. The Quarry Rust test recomputes the sha256 of its manifest and compares it to a pinned constant (CR-T17 pins the same hash on the auth-core side) so editing one side fails both. Line endings: .rs CRLF, .json/.ts LF (check each neighbour with node, not grep). The implementer cannot build images: hand the orchestrator the Dockerfile.test command for unit tests and the Dockerfile.edge --target build command for the compile check.

#### QB-T09a: Quarry: SearchProvider::search_with_report plumbing, RerankReport and the cache rule (T09a)

- design: quarry (T09); parent: extracted from QB-T09 | plane: ingestion | size: M | chain: C9 #3 | kind: code
- depends_on: QB-T02, Z-08 (design had: T02, T03, T05)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW (the reviewer's T09a: plumbing shared with T10 and T09, sequenced first).
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/serp.rs (SearchProvider::search_with_report default method)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/rerank.rs (RerankReport / outcome types)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs (CachedSearch carries the report; serde default)
- corrections (folded in):
  - Cache rules: write-back happens inside the handler (search_routes.rs ~688-715) for whatever the provider returned. Cache only status applied or rerank-not-configured; never skipped (disabled, processor_not_approved, circuit_open...) or degraded; add a rerank epoch (profile, enabled, shadow, top_n) to params_signature; carry the rerank report in CachedSearch (serde default) so a cache hit reports the same status.
  - T02/T09/T10 share the report/cache mechanism and must be sequenced: QB-T02, QB-T09a, QB-T10, then QB-T09.
- cross-design overlap: search_routes.rs is also edited by Z-08, QB-T10 and QB-T09: sequential in chain C9.
- deploy: S9 - ingestion: part of the quarry-edge image.
- live proof (none): None alone (plumbing).
- notes_for_implementer: Add the default search_with_report (calls search, returns an empty report) so existing providers compile unchanged; add RerankReport {status, reason, profile}; CachedSearch carries it with serde default so entries written by the old build deserialize. Tests: a degraded or skipped report is never cached; an applied report is; params_signature changes with the rerank epoch; old CachedSearch JSON deserializes.

#### QB-T10: Quarry vector leg: correct request, ZDR/sovereign skip, scoped corpus, strict mirror, timeouts

- design: quarry (T10) | plane: ingestion | size: M | chain: C9 #4 | kind: code
- depends_on: QB-T02, QB-T09a, QB-T03, QB-T04, QB-M1a (design had: T02, T03, T04)
- gates: start [G-QB-RETRIEVE-CODE] done [] deploy [G-M1B-BACKFILL, G-QB-A-DEPLOYED]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP with corrections (authored by this manifest from the design plus critic-quarry #1, #9, #10).
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/vector_index.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/hybrid.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_retrieve_contract.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/dp-contracts/retrieve-v1/ (vendored goldens + sha256)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/zdr.rs or quarry-core zdr map (typed ZdrMode{Off,On} -> disabled|reject|ephemeral)
- corrections (folded in):
  - The client filter filters.sources=[quarry] is only a hint: the corpus scope is enforced server-side by QB-M1a (forced scope, dense only) and requires the stamped payload (QB-M1b). Do not rely on it.
  - Typed map from Quarry ZdrMode{Off,On} to DP {disabled,reject,ephemeral} with a golden of accepted values; the literal "off" never leaves Quarry. ZDR or sovereign skips the vector leg entirely (marker skipped_zdr) with zero requests.
  - Strict mirror of the real wire (QB-T04 pins it): candidates carry document_id and text but NO score and NO url; the url comes from sources[].url (documents.source=quarry only); drop hits without a resolvable url; rank by returned order. Wiremock tests load only the vendored retrieve-v1 goldens, no invented aliases (the current tests use source_url, uri, score).
  - Vector leg needs a kill switch (QUARRY_EDGE__VECTOR_RETRIEVAL), a per-org budget and a joined timeout: today vector_index is built whenever DATA_PLANE_URL is set (main.rs ~1058-1075), is shared with find-similar and each non-ZDR search pays a Cohere Embed query call. Metering of query embeds through usage ledger v2 is deferred (DEF-QB-VECTOR-METERING).
  - 1500 ms client timeout (reqwest::Client::new() has none today) and a joined timeout in HybridSearchProvider so lexical results are never delayed; auth/contract failures ERROR (loud), outages WARN. Apply the same context to /v1/search/similar (search_routes.rs ~1260-1270).
  - Token request data:read only from the manifest (never org:data:read_all). Viewer-grant skipping is on the Data Plane side (QB-T03).
- cross-design overlap: QB-N1 (rrf_fuse fix) edits hybrid/fusion next.
- deploy: S9 - ingestion: part of the quarry-edge image; the vector leg flag stays OFF until QB-M1b has been deployed and backfilled.
- live proof (tiny): Live /v1/search for a test org that has an ingested quarry page: results contain a vector-origin hit with its real URL fused with lexical results; quarry-edge shows no hybrid warning; the DP audit table shows the service-principal retrieve with cause ok; a ZDR search shows zero DP retrieve calls (retrieval_runs count unchanged). One Cohere Embed query for the non-ZDR call (state the count).
- notes_for_implementer: Change VectorIndex::retrieve to take a context (org, zdr, sovereign, limit). Request body: zdr_mode disabled (mapped), sovereign_required = zero_saas_search, filters.sources [quarry] (hint), mode_mix.rerank false, top_n = limit; token request data:read only from the manifest. Strict response mirror per above. Tests: exact request body asserted against the vendored golden (valid zdr_mode, filters.sources, mode_mix.rerank=false, no org:data:read_all in the token request); ZDR and sovereign make zero requests; a hit with no url is dropped; vector 422/403 produce the loud degraded reason; timeout leaves lexical results intact and bounded latency; fused results contain real URLs deduplicated against lexical hits.

#### QB-N1: Quarry: rrf_fuse keeps engines and fusion_score; canonical URL dedup (N1)

- design: manifest | plane: ingestion | size: S | chain: C9 #5 | kind: code
- depends_on: QB-T10
- gates: start [] done [] deploy []
- review source: added by this manifest
- verdict: NEW (critic-quarry #4 and missing task: fix rrf_fuse metadata loss and URL normalization).
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/fusion.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/hybrid.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/serp.rs (read: engines at 50-67)
- corrections (folded in):
  - rrf_fuse (fusion.rs:29, ~59-90) rebuilds each result from url/title/snippet with ..Default::default(), dropping engines and fusion_score. relevance.rs uses engines for the engine-agreement bonus (relevance.rs:44-46, 864-872): once the vector leg returns any hit, all results in that response would lose the signal.
  - Dedup is by raw URL string: canonical stored URLs will not merge with SERP URLs. Normalise (scheme, host case, trailing slash, tracking params) with a test using real engines populated.
- cross-design overlap: None beyond hybrid.rs (QB-T10).
- deploy: S9 - ingestion: part of the quarry-edge image.
- live proof (none): Free: a fused result keeps its engines list (checked through the Quarry response).
- notes_for_implementer: Carry engines and fusion_score through the fused result; merge engines when the same URL appears in several legs; add canonical URL normalisation; tests with realistic multi-engine fixtures.

#### QB-T09: Quarry: Data Plane rerank client replaces the LLM reranker, with explicit status, ZDR and cache rules

- design: quarry (T09) | plane: ingestion | size: L | chain: C9 #6 | kind: code
- depends_on: QB-T09a, QB-T05a, QB-T10, Z-08 (design had: T02, T03, T05)
- gates: start [G-QB-A-CODE] done [] deploy [G-QB-A-DEPLOYED, G-CP-P1]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: KEEP, SPLIT: T09a (plumbing, separate entry) then T09 proper. Many corrections.
- files:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/rerank.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/data_plane_http.rs (new; shared client with timeouts, manifest-driven token requests, one refresh on 401)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/lib.rs (drop the ModelPlaneSearchReranker export at ~157)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs (wiring ~1091-1127 and ~1097-1127)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/search_routes.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs (new)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_rerank_contract.rs (new)
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/dp-contracts/rerank-v1/ (vendored goldens + sha256 manifest checked by both trees)
- files removed from the design list:
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/serp.rs
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs
- corrections (folded in):
  - Goldens: the Data Plane rerank-v1 goldens are not readable from the Quarry Docker context. Vendor them into crates/quarry-runtime/tests/fixtures/dp-contracts/rerank-v1/ with a sha256 manifest that the Data Plane test also checks; run them via Dockerfile.test (cargo test) because Dockerfile.edge --target build only compiles.
  - ZDR rule (plan-confirmed): the Claims.zdr field and the effective_zdr helper are owned by Z-08 (done earlier in this chain, user principals only); here only consume them: effective_zdr(claims, requested) = requested.unwrap_or(false) OR (user principal AND claims.zdr == Some(true)). Service principals MUST send zdr: SearchRequest.zdr becomes Option<bool> and a service principal without it gets 400 zdr_required. Verified callers already send it: model-gateway quarry.rs Body always serialises zdr, execution-core web_tools search_body always includes it. Do NOT read the claim for service tokens: model-gateway and execution-core hold retentionByAudience quarry=zdr, so every chat search would become ZDR.
  - Scores and shadow: emit score and score_kind only when the request opts in with score_kinds containing cross_encoder.v1 (default: reorder only), otherwise uncalibrated Cohere scores would hit relevance.rs (0.6 * provider + 0.4 * lexical against a 0.30 keep threshold). Shadow mode (call, keep order) emits no score and no highlights.
  - Cache rules come from QB-T09a (applied or not-configured only; rerank epoch in params_signature; report carried in CachedSearch).
  - Wiring: the wrapper is built in main.rs ~1097-1127 only when cfg.model_plane_url is set; the new gate is cfg.data_plane_url + cross_plane_tokens + cfg.semantic_rerank (no MODEL_PLANE_URL). Delete ModelPlaneSearchReranker, build_rerank_prompt, strip_code_fences, RerankEntry, with_model, config.semantic_rerank_model and the lib.rs:157 export; keep semantic_rerank_top_n.
  - Skip rules: org "onboarding" and the global org are real (ONBOARDING_ORG_SENTINEL in auth-core plane-token.controller): a rerank for an org that is not a tenant skips with reason input_not_tenant; rerank is never attempted for them. Highlights are deferred (QB-T06): no window/highlight settings and no QUARRY_EDGE__RERANK_WINDOW.
  - Rerank client behaviour: one DP call per search with deadline from RerankContext{org_id, zdr, sovereign, request_id, deadline}; circuit breaker opens after 5 failures; 401/403/404/422 log at ERROR once per minute; invalid or duplicate indexes ignored; degraded results never cached.
  - No code path may construct a Model Plane token for rerank (grep-style test over the manifest).
- dirty/untracked collisions: None of the Quarry files is dirty.
- cross-design overlap: Z-08 (auth.rs, search_routes.rs, claims) and QB-T10 (search_routes.rs) in the same chain: Z-08, QB-T02, QB-T09a, QB-T10, QB-T09.
- deploy: S9 - ingestion: quarry-edge image through the Ingestion launcher stage copy; compile check first: docker build -f deploy/compose/Dockerfile.edge --target build -t quarry-edge-compilecheck:local . ; unit tests via Dockerfile.test. First deploy with QUARRY_EDGE__RERANK_SHADOW=1 for the smoke run, then shadow off.
- live proof (tiny): Through the live stack: a real /v1/search for the test org returns rerank.status=applied with scores only when the request opts in; quarry-edge logs search.rerank outcome=applied and no Model Plane 403; the same request with zdr:true returns rerank.status=skipped reason=zdr with unchanged order and no DP call (dpv2_web_rerank_total unchanged); WEB_RERANK_ENABLED=0 yields skipped/degraded with the original order. One paid 10-document rerank call; state the count.
- notes_for_implementer: Add data_plane_http.rs (shared client with timeouts, manifest-driven token requests, one refresh on 401). Rewrite rerank.rs: keep SearchReranker and RerankingSearchProvider, add DataPlaneSearchReranker, RerankContext, RerankReport/outcome (from QB-T09a). Delete ModelPlaneSearchReranker, prompt builder, fence stripping, JSON entry parsing and tests, the main.rs model-client block and config.semantic_rerank_model. search_routes: sovereign = zero_saas_search, skip input_not_tenant orgs, SearchResponse.rerank and top-level score_kind (opt-in), never cache degraded results, add the rerank profile to params_signature, emit the search.rerank tracing event. Optional QUARRY_EDGE__RERANK_SHADOW. Wiremock tests load ONLY the rerank-v1 goldens produced by QB-T05a: ok reorders head by DP order and keeps the tail; skipped/degraded keep order and mark the response; ZDR (body or user-claim) makes zero DP calls; a service principal whose token zdr is true but body zdr false DOES call the DP (guards the trap) using claims built from the registry policy; invalid/duplicate indexes ignored; deadline respected; circuit breaker; degraded not cached and the cache signature changes with the profile; 401/403/404/422 log at ERROR once per minute; onboarding/global orgs skip with reason. Regression test that no code path constructs a Model Plane token for rerank.

#### QB-T17: Decide and fix Quarry's other Model Plane dependents (autoprompt, answer, ai-formats, page-title)

- design: quarry (T17) | plane: ingestion | size: M | chain: C9 #7 | kind: code
- depends_on: QB-T02, QB-T09 (design had: T02)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP, scope set by owner default D-C (authored by this manifest).
- files:
  - apps/Ingestion Plane/docker-compose.yml
  - apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs
- files removed from the design list:
  - apps/Control Plane/config/plane-service-principals.json
- corrections (folded in):
  - Owner default D-C: autoprompt, answer synthesis, ai-formats and page-title cannot mint a model-plane token today (403). Until a proper org-bound, metered design exists their defaults are switched OFF in config so they stop failing; the registry is NOT edited (no models:invoke grant).
  - Add a loud startup error when a feature is flagged on and has no model path, instead of silently degrading; keep the three model-gateway/models:invoke rows granted:false in the Quarry manifest (QB-T02) so flipping a row later is the only change.
  - Commit 932b30d2 flipped AUTOPROMPT and SEMANTIC_RERANK from 0 to 1 together: AUTOPROMPT back to 0; SEMANTIC_RERANK stays 1 (it now uses the Data Plane).
  - Part 2 (the proper org-bound, metered model path: principal with audience model-gateway, scope models:invoke, per-request ZDR from the body and a Model Plane cost trail) is a design task recorded in the ledger as OPEN with an owner and date (DEF-QB-T17-PART2); it is not built in this manifest and must not become a forgotten stopgap.
  - Edit the Ingestion compose quarry-edge block only (dirty integration-api hunk by another session); main.rs is also edited by QB-T09 (this runs after it in chain C9).
- dirty/untracked collisions: apps/Ingestion Plane/docker-compose.yml (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Ingestion Plane/docker-compose.yml
- cross-design overlap: QB-T11 (same compose block) runs after this.
- deploy: S9 - ingestion: quarry-edge recreate with the new defaults (env change needs a recreate, not a restart).
- live proof (none): Research/comparative search no longer logs an Auth Core 403 for autoprompt; a flagged-on feature with no model path logs the loud startup error.
- notes_for_implementer: Set the defaults off in config.rs/compose for AUTOPROMPT and the other three Model Plane dependents; add the startup validation; behaviour test that a flagged-on feature with no model path fails loudly at startup rather than silently degrading.

#### QB-T11: Quarry docs, OpenAPI, SDKs and compose cleanup

- design: quarry (T11) | plane: ingestion | size: S | chain: C9 #8 | kind: code
- depends_on: QB-T09, QB-T17 (design had: T09)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP with corrections (authored by this manifest).
- files:
  - apps/Ingestion Plane/docker-compose.yml
  - apps/Ingestion Plane/Quarry-v2/docs/Features.md
  - apps/Ingestion Plane/Quarry-v2/docs/CROSS_PLANE_AUTH.md
  - apps/Ingestion Plane/Quarry-v2/docs/CROSS_PLANE_INTEGRATION.md
  - apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml
  - apps/Ingestion Plane/Quarry-v2/docs/PARITY_IMPLEMENTATION_PLAN.md
  - apps/Ingestion Plane/Quarry-v2/sdks/generate.sh
- corrections (folded in):
  - Ingestion compose is dirty (uncommitted integration-api hunk by another session, ~line 458+): edit ONLY the quarry-edge block (~121-147), by hunk. QB-T17 edits the same block first (AUTOPROMPT default off), so this task runs after it in chain C9.
  - Highlights are deferred (QB-T06): do not add RERANK_WINDOW or highlight docs; document RERANK_DEADLINE_MS and RERANK_SHADOW only. Keep QUARRY_EDGE__SEMANTIC_RERANK default 1 and TOP_N; delete QUARRY_EDGE__SEMANTIC_RERANK_MODEL.
  - CROSS_PLANE_AUTH.md rows 9-10 rewritten to the real grants (data-plane scopes data:read, data:rerank, documents:write; no model-plane token); Features.md:39,179 stale; openapi SearchResponse.rerank and SearchResult score/score_kind (no highlights); regenerate SDKs (sdks/generate.sh).
  - After the live proof, the post-proof compose default flips for the Ingestion block (rerank shadow off) are done here by the orchestrator.
  - A docs-contract test in the Quarry repo greps compose, config.rs and docs for removed identifiers (SEMANTIC_RERANK_MODEL, ModelPlaneSearchReranker, "reranker can call Model Plane") and fails if any remain.
- dirty/untracked collisions: apps/Ingestion Plane/docker-compose.yml (M, integration-api hunk by another session)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Ingestion Plane/docker-compose.yml
- cross-design overlap: Z-32 also edits the Ingestion compose (different block, CP wave ordering); QB-T17 edits the quarry-edge block before this task.
- deploy: S9 - ingestion: compose env change needs a recreate (not a restart) of quarry-edge through the Ingestion launcher stage copy.
- live proof (none): docker compose config for the Ingestion stack shows the new env and no removed variable; quarry-edge starts and logs "semantic rerank: enabled" with the DP client, not a model client.
- notes_for_implementer: Compose comments rewritten (no LLM, no per-query LLM cost), SEMANTIC_RERANK_MODEL deleted, RERANK_DEADLINE_MS and RERANK_SHADOW added; docs and openapi updated; SDKs regenerated; the docs-contract test added and green.

#### QB-T14: Judged web-result set, runner, calibration and regression gate

- design: quarry (T14) | plane: ingestion | size: L | chain: C9 #9 | kind: code
- depends_on: QB-T05a, QB-T11 (design had: T05, T06)
- gates: start [] done [] deploy [G-QB-A-DEPLOYED, G-OPS-LABEL]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP with corrections (authored by this manifest; highlight arm F removed because QB-T06 is deferred).
- files:
  - apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/queries.v1.json
  - apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/results.v1.jsonl
  - apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/judgments.v1.jsonl
  - apps/Ingestion Plane/Quarry-v2/lab/evals/src/web_rerank_gate.rs
  - apps/Ingestion Plane/Quarry-v2/lab/evals/web-rerank-baseline.json
  - apps/Ingestion Plane/Quarry-v2/lab/evals/Cargo.toml
- corrections (folded in):
  - Remove arm F (highlight precision) and the highlight gate (QB-T06 deferred); keep arms A-E (engine order, lexical baseline, Cohere web-v1 title+snippet, plus host, window sweep removed with highlights).
  - Acceptance gate: nDCG@10 >= +0.04 over engine order with CI lower bound > 0, no category drop > 0.03, added p95 <= 900 ms. The "degraded <= 2% over a 200-call soak" gate is unrealistic on the shared deployment (about 19% of queries lost to 429s, eval-gate.py:33-35): measure the shared baseline and make a dedicated deployment/quota a precondition for default-ON (D-D); a +0.04 gate on 80 queries needs a power check.
  - Labelling is an operator action (OPS-LABEL): LLM pre-judge on the Terra subscription (judging is not reranking, O11) plus human adjudication of all disagreements and a 20% sample, kappa >= 0.6; labels must never feed runtime ranking. Frozen SERP fixtures are public data (about 400 KB).
  - lab/evals is part of the Quarry Docker contexts: keep this task in chain C9 so Quarry Docker builds never meet a half-edited lab crate. The labelling data files (fixtures) can be produced earlier by the operator without cargo.
  - Calibration artefacts (score-to-relevance knots, provider weight) are emitted and committed for QB-T12; do not invent knots.
- cross-design overlap: Feeds QB-T12 (calibration). Chain C9 end.
- deploy: S9 - No image: lab runner and fixtures; runs against the live DP /v1/rerank.
- live proof (tiny): Run arm C against the live DP /v1/rerank (about 80 paid rerank calls plus the soak of 200 calls; at any plausible Cohere price under a dollar, price itself unverified); say the exact call counts. Commit the results and the calibration table consumed by QB-T12.
- notes_for_implementer: Build the 80-query set (50 no, 30 en; categories per design section 8), freeze the top-20 SearXNG results per query, implement the runner (nDCG@10/MRR/P@3/noise@5, per-category deltas, paired bootstrap, degraded-rate soak), emit calibration artefacts, commit a baseline with tolerances derived from measured noise and a 10-query smoke mode for CI. Unit tests for the metrics (hand-computed nDCG/MRR), the bootstrap (seeded determinism), the gate (fails on a synthetic regression and on a degraded-rate breach, passes on baseline), the calibration fit (monotone knots); a fixture-integrity test (every judged url exists in results, every query has >=2 judges).

### Chain C10 fe

#### Z-04: Gate the org ZDR switch on an attested provider

- design: zdr (Z-04) | plane: Frontend | size: M | chain: C10 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (add an operator-only seed path; gate in the BFF only)
- files:
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orgs/settings.rs
  - apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
  - apps/Frontend Plane/verevonv3/src/shared/api/organization-client.ts
  - apps/Frontend Plane/verevonv3/docs/ or the existing ZDR runbook location: operator seed runbook for the test org (direct org-core PATCH with an operator credential) and its test
- corrections (folded in):
  - The org switch gate belongs in the BFF (verevon-gateway is the only holder of org:settings:write:self); org-core cannot read the Model Plane catalogue. Because the gate is BFF-only, a direct org-core PATCH still bypasses it. That is the intended operator-only seed path for the Z-60 test org (owner default D-A), but the design has no task for it: write the runbook and a test proving the BFF returns 409 zdr_no_attested_provider while org-core PATCH with an operator credential still works.
  - BFF catalogue read: reuse the existing list_models handler's token minting (shared::model_token + shared::required_inference_token, then GET {model_gateway_url}/v1/models). The catalogue is deployment-level so any admin's tokens work. Fail closed on enable only: if the catalogue is unreachable return 503 zdr_gate_unavailable; disabling is always allowed.
  - Drop "and no grounding path when documents exist" from the gate: the BFF cannot know that. Gate on at least one chat model advertising the zdr feature (inference-core provider/mod.rs ~684 derives it from supports_zdr); grounding degradation is Z-41's explicit notice.
  - The tooltip over-promise the design cites is already fixed by the uncommitted trust-edits job (WorkspaceSettingsPage.tsx zdrTooltip now says the content still leaves Verevon and goes to the provider; pinned by in-product-trust-copy.test.ts). Z-04 should only add the scope sentence in coordination with Z-42, not rewrite the tooltip.
  - Reuse the composer's predicate (chat-client.ts ~2501-2520, whether ANY catalogue model can serve a no-retention turn) so the toggle and the composer cannot disagree.
- dirty/untracked collisions: apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx (M; trust-edits tooltip hunk at ~1311)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
- cross-design overlap: Trust-edits job (same file, tooltip) and Z-42. No Quarry/contextual overlap.
- deploy: S12 - frontend: rebuild gateway image, SPA change, restart the frontend container (no hot reload; check git status for other sessions' half-done edits first or verify on the :5199 host preview).
- live proof (none): Free, no write: the toggle stays disabled on the live stack because nothing is attested; PATCH with zeroDataRetention:true returns 409. Do NOT toggle any live org.
- notes_for_implementer: In settings.rs, when zeroDataRetention is true: require_org_admin first (existing), then fetch /v1/models with the admin's model+inference bearers and check for any model whose features include zdr; else return 409 {error:{code:zdr_no_attested_provider}} and make no org-core call. Disabling passes straight through. Frontend: disable the toggle with the reason, banner when ZDR is on but nothing is attested (attestation lapsed at REVIEW_BY), keep zdrLocked (plan) logic separate. Tests: Rust wiremock with a catalogue double shaped like the REAL /v1/models body (capture one from the stack; do not invent the shape), 409 path asserts zero org-core requests, 503 on catalogue failure, disable always allowed. Playwright: toggle disabled on the live stack. Write the operator seed runbook (direct org-core PATCH with an operator service credential, test org only, never an existing org) and a Go or script test that the seed path still works so Z-60 is not blocked by Z-04.

#### Z-03: One org retention-posture read model: migrate all readers outside the Control Plane (BFF, conversation-core, SPA)

- design: zdr (Z-03) | plane: Control | size: M | chain: C10 #2 | kind: code
- depends_on: Z-10 (design had: Z-10)
- gates: start [G-CP-P0A-CODE] done [] deploy [G-CP-P0A, G-OD-LAPSE]
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections (non-Control parts only; org-core read path and Auth Core resolver are covered by the Control Plane wave)
- CP wave boundary: The org-core internal retention read route ({zdrIntent, entitled} with narrow scopes org:retention:read:any for auth-core and a BFF scope, mapped in serviceScopesForRequest) and the Auth Core resolver/503 semantics are CP wave P0a. This task migrates only the readers OUTSIDE Control.
- files:
  - apps/Frontend Plane/verevonv3/apps/gateway/src/zdr.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/tickets.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/inbox.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/ai.rs
  - apps/Application Plane/conversation-core/conversation-core-go/internal/clients/orgcore_client.go
  - apps/Frontend Plane/verevonv3/src/shared/api/organization-client.ts
- files removed from the design list:
  - apps/Control Plane/org-core/internal/http/server.go
  - apps/Control Plane/org-core/internal/http/handlers.go
  - apps/Control Plane/org-core/internal/org/service_enhanced.go
  - apps/Control Plane/auth-core/src/auth/interactive-retention-policy.ts
- corrections (folded in):
  - Do not re-plan the org-core endpoint. The Control wave builds the org-core internal route and Auth Core's service-header read path. Z-03 only migrates the readers outside Control: BFF zdr.rs, tickets.rs, inbox.rs, knowledge/quarry.rs, ai.rs, conversation-core, SPA.
  - Plan-lapse semantics are an unresolved owner-visible choice hidden in the design. effective = intent AND entitlement (Auth Core today) would WEAKEN the BFF (today fails true, reads intent) and conversation-core (raw intent): an org whose plan lapses silently stops being ZDR. Default recommendation: do not silently drop ZDR on lapse; return {intent, entitled, lapsed} and let every reader treat intent=true as ZDR until an admin turns it off (stricter reading), and encode that single answer in the Z-10 golden row "plan lapsed". Needs one owner confirmation (OD-LAPSE) because it changes Auth Core's current behaviour; tell the CP wave owner.
  - BFF readers: zdr.rs org_zdr_enabled reads the full org record through proxy_json with verevon-gateway delegated credentials (it works today, no scope problem). inbox.rs (~335, ~416) and tickets.rs (~247) read metadata.interactiveRetention from the org JSON they already fetched, so migrating them means one extra call or adding a retention block to the org GET response. quarry.rs and ai.rs go through crate::zdr::org_zdr_enabled already.
  - conversation-core's ZDREnabledOrgIDs (clients/orgcore_client.go) enumerates /internal/orgs (list, 500 per page) and filters raw metadata; it has no per-org call. A per-org posture endpoint does not fit it. Either keep raw intent for that purge/skip use (intent is the conservative value) with an explicit comment and test, or add the effective flag to the list response.
  - SPA organization-client.ts reads intent for the toggle display (getOrganizationAISettings.zdr). Keep intent there; only consumers that gate behaviour need the effective value.
  - One unknown-posture rule: Known(bool)/Unknown with the BFF failing TRUE for write gates; the 60 s BFF cache is for successes only; a failed lookup is never cached.
- dirty/untracked collisions: None of the listed non-Control files is dirty today. Control wave edits will land in org-core and auth-core; conversation-core and BFF files are clean.
- cross-design overlap: Heavy with the Control wave (P0a/P0b). Contextual critic #18 asks for an org-ZDR oracle for embed, page-image and rerank (DEF-CR-EMBED-ZDR-ORACLE): coordinate the route shape rather than adding a second one.
- deploy: S11+S12 - frontend (BFF gateway image rebuild + SPA, frontend container restart) and application (conversation-core image rebuild, S11). The org-core side is the control stage and is covered elsewhere.
- live proof (none): Free: read the posture for the 3 live orgs (all false) and compare each reader. Needs the Control wave deployed first or the live read 401s.
- notes_for_implementer: Wait for the Control wave's route shape, then implement one shared BFF function (zdr.rs) that returns Known(bool)/Unknown and keep fail-true for callers that gate persistence; migrate tickets.rs, inbox.rs, quarry.rs, ai.rs onto it. For conversation-core decide intent-vs-effective explicitly (see correction) and pin it with a Go test that uses the real Control response fixture. Golden row first: plan lapsed (intent true, entitlement false) evaluated by org-core, Auth Core, BFF Rust and conversation-core must give the same answer; today Auth Core says false and the BFF says true. Do not touch the SPA toggle's intent display. Verify with a real org-core instance or fixture with service auth enforced; do not mock fetch/reqwest with a permissive double.

#### Z-09: Fix the browser retention purge (F8)

- design: zdr (Z-09) | plane: Frontend | size: S | chain: C10 #3 | kind: code
- depends_on: Z-03 (design had: Z-03)
- gates: start [] done [] deploy []
- review source: reviewer (surviving part of the review JSON), corrections folded in
- verdict: sound-with-corrections
- files:
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/history.rs
  - apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts
  - apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-retention.ts
- corrections (folded in):
  - BFF org_zdr_enabled fails TRUE on any org-core error. If ThreadsResponse.retention.zdr is filled from it, a single org-core blip makes applyServerRetention purge every user's localStorage copy of conversations (chat-client.ts ~2120-2135, chat-retention.ts). Return retention only when the posture was positively read (2xx or fresh cache); on lookup failure omit the field so the SPA treats it as undefined (current behaviour, no purge). This needs the Known/Unknown return from the shared posture function (Z-03).
  - The SPA side already reads retention.zdr (verified), so the SPA change is test-only unless Z-20 touches it; chat-client.ts and chat-retention.ts are dirty from the trust-edits job.
  - Line endings: chat-retention.ts and privacy-tier.ts show CRLF will be replaced by LF warnings in git (working copy CRLF vs repo LF). Check with node, preserve what the file has, never normalise.
- dirty/untracked collisions: src/shared/api/chat-client.ts (M +59); src/features/chat/lib/chat-retention.ts (M +32)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/shared/api/chat-client.ts; modified: Frontend Plane/verevonv3/src/features/chat/lib/chat-retention.ts
- cross-design overlap: Trust-edits job (same SPA files).
- deploy: S12 - frontend: gateway rebuild plus SPA, frontend container restart.
- live proof (none): Free: with a ZDR test org the threads listing returns retention.zdr:true and the browser copy clears; with the live non-ZDR orgs it returns false and nothing changes. Needs the test org.
- notes_for_implementer: Add retention: {zdr: bool} to ThreadsResponse only when the posture is Known; add a handler test for the field and for omission on org-core failure (use a wiremock org-core returning 500 and assert the key is absent). Add a chat-retention vitest asserting purge on zdr:true and NO purge when the key is absent. Do not change the fail-true behaviour of org_zdr_enabled used by write gates.

#### Z-05b: SPA: localized rendering of every ZDR refusal code with actions (Z-05b)

- design: zdr (Z-05); parent: Z-05 | plane: Frontend | size: S | chain: C10 #4 | kind: code
- depends_on: Z-05a
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW split from Z-05 (the SPA half).
- files:
  - apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts (error handling; ChatErrorEvent gets an optional feature; modified +59 by the trust edits)
  - apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts (new; vendored sha256-pinned copy of api/contracts/zdr-refusals.json plus NO/EN text and actions)
- corrections (folded in):
  - Map each code to localized text and the right action (open Integrations, choose another model) via i18n.tr(no,en); a test asserts every code in the vendored fixture has an SPA entry; keep the speech route {error:{code,message}} shape tests green.
  - chat-client.ts is dirty (+59, trust edits): edit by hunk only; preserve CRLF/LF as the file has it (check with node).
- dirty/untracked collisions: src/shared/api/chat-client.ts (M +59)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/shared/api/chat-client.ts
- cross-design overlap: Z-09, Z-20 edit chat-client.ts after this in chain C10.
- deploy: S12 - frontend: SPA only (restart the frontend container after git status check).
- live proof (none): None possible until a ZDR path exists; vitest only.
- notes_for_implementer: Vitest first: every code renders NO and EN text and an action; unknown code falls back to a safe generic message; ChatErrorEvent.feature is rendered when present.

#### Z-19: BFF turn retention resolution and connection routes

- design: zdr (Z-19) | plane: Frontend | size: L | chain: C10 #5 | kind: code
- depends_on: Z-14 (design had: Z-14)
- gates: start [G-Z14-CODE] done [] deploy [G-Z14-DEPLOYED]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Frontend Plane/verevonv3/apps/gateway/src/zdr.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/shared.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/json_handlers.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/audience_tokens.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations/model_subscriptions.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations/model_connections.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/integrations/model_connections.rs (new; assessment, acknowledgement, revoke, admin list proxies)
- corrections (folded in):
  - Replace apply_org_zdr_posture with resolve_turn_retention: only when the body names a subscription connection AND the org is ZDR, call the Auth Core decision (control-policy token from the SEPARATE BFF decision principal provided by CP wave P1), pass x-retention-decision on EVERY audience mint through one get_audience_token path (chat/shared.rs ~150-338 and ~1389, json_handlers.rs ~444, audience_tokens.rs): the token set must be homogeneous; never write zdr=false; ignore client zdr:false; map refusals to typed 409 and 503 (Auth Core down means 503 and no upstream call).
  - Non-ZDR orgs make no decision call (asserted by request count). Explicit zdr:true (Temporary chat) is passed as explicit_zdr; Temporary chat plus a personal subscription refuses (default, OD-TEMP-CHAT). Space ZDR is not applied in v1.
  - New proxy routes for assessment, acknowledgement, revoke and admin list; admin list uses require_org_admin; owner-only enforcement is in integration-corev2 (Z-13), the BFF only forwards the user bearer.
  - wiremock tests against a decision fixture signed with a test key and verified through the real JWKS path (not a permissive double).
  - Unknown-posture rule from Z-03 (Known/Unknown; fail true for write gates).
  - Dirty: integrations.rs and model_subscriptions.rs (other sessions): hunks only.
- dirty/untracked collisions: gateway domains/integrations.rs (M); domains/integrations/model_subscriptions.rs (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/apps/gateway/src/domains/integrations.rs; modified: Frontend Plane/verevonv3/apps/gateway/src/domains/integrations/model_subscriptions.rs
- cross-design overlap: Z-03 (zdr.rs earlier in chain C10).
- deploy: S12 - frontend: gateway image via the frontend-only launcher copy (LAST_STACK_INDEX=5, --from frontend); caution: --from model also rebuilds the frontend plane from the working tree.
- live proof (none): Z-60 probes Z0-Z2.
- notes_for_implementer: Add resolve_turn_retention and the single mint path; wiremock tests: all tokens in a turn carry the same decision, a client x-zdr:false and body zdr:false change nothing, explicit zdr:true is passed as explicit_zdr, Auth Core down gives 503 and no upstream call, non-ZDR orgs make no decision call.

#### Z-20: Frontend prompts, composer chip, answer line and admin record

- design: zdr (Z-20) | plane: Frontend | size: L | chain: C10 #6 | kind: code
- depends_on: Z-19, Z-06, Z-05b (design had: Z-19, Z-06)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Frontend Plane/verevonv3/src/shared/api/chatgpt-subscription-client.ts
  - apps/Frontend Plane/verevonv3/src/shared/api/model-connections-client.ts
  - apps/Frontend Plane/verevonv3/src/features/integrations/components/ChatGptSubscriptionPanel.tsx
  - apps/Frontend Plane/verevonv3/src/shared/components/AiModelPicker.tsx
  - apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx
  - apps/Frontend Plane/verevonv3/src/shared/api/privacy-tier.ts
  - apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts
  - apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx
  - apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
  - absorbs the SPA hunks of Z-06: src/shared/api/privacy-tier.ts (answerProcessingNote shows retention_class; CRLF in the working copy) and src/shared/api/chat-client.ts (usage frame servedBy)
  - apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-claims.ts and its pin test (the mustSay pins about the ChatGPT route turn false for waived runs)
- corrections (folded in):
  - Add-flow dialog after connected and an inline dialog on zdr_connection_not_acknowledged (render the server text, send digest and version, retry the turn); composer chip when org ZDR is on and the model or connection is not ZDR-capable; stronger warning when the org has ZDR on (it contradicts the org setting); the user can continue; ZDR is turned off ONLY for runs on that connection (never org-wide, never other members, never other connections).
  - Draft the `supported` text (the connection DOES support ZDR) in NO and EN; the drafts are placeholders pending counsel sign-off (OD-LEGAL).
  - Trust drift (critic-zdr #14): trust-claims.ts marks zdr-is-a-gate-not-a-guarantee and terra-route-disclosed as pending, and the mustSay pins "ChatGPT-ruten støtter ikke ZDR, og en ZDR-forespørsel mot den avvises" and "sender forespørselen bare til en leverandør med registrert ZDR-godkjenning" turn false for waived runs; trust-claims-pending-reverification.md lists 26 sentences (rows 14, 23, 24, 25 touch dataClasses, the privacy-tier tooltips, answerProcessingNote and VEREVON.md) and says the prompt wording must be pinned in a test. The trust edits and this task must land together; claims stay true to the code and the live proof (X-TRUST).
  - The privacy-tier.answer-line tests are extended, not loosened (three classes, both languages); the inbox picker uses the same assessment; accessibility checks on the dialog.
  - Dirty SPA files (chat-client.ts +59, privacy-tier.ts +149 CRLF, ChatMessages.tsx, DashboardComposer.tsx, WorkspaceSettingsPage.tsx, chatgpt-subscription-client.ts): hunks only; preserve line endings.
  - The frontend container has no hot reload; check git status for other sessions' half-done edits before restarting it, or verify on the :5199 host preview.
- dirty/untracked collisions: chatgpt-subscription-client.ts, DashboardComposer.tsx, privacy-tier.ts, chat-client.ts, ChatMessages.tsx, WorkspaceSettingsPage.tsx (all M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/shared/api/chatgpt-subscription-client.ts; modified: Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx; modified: Frontend Plane/verevonv3/src/shared/api/privacy-tier.ts; modified: Frontend Plane/verevonv3/src/shared/api/chat-client.ts; modified: Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx; modified: Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
- cross-design overlap: Trust-edits track; Z-21b and Z-42 edit WorkspaceSettingsPage.tsx after this in chain C10.
- deploy: S12 - frontend: SPA; restart the frontend container after the git status check.
- live proof (none): Z-60 screenshots of the dialog, chip and answer line.
- notes_for_implementer: Vitest first: privacy-tier.answer-line tests for the three classes and both languages; panel flow with ZDR on and off; composer chip; refusal-to-dialog-to-retry; Playwright with route mocks; accessibility checks on the dialog.

#### Z-21b: BFF and SPA: org-admin counts of waived runs beside the acknowledgement list (Z-21b)

- design: zdr (Z-21); parent: Z-21 | plane: Frontend | size: S | chain: C10 #7 | kind: code
- depends_on: Z-21a, Z-19, Z-20 (design had: Z-17, Z-19)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW split from Z-21 (Frontend half).
- files:
  - apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orgs/settings.rs
  - apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
- corrections (folded in):
  - Non-admins get 403; no content field exists in the response schema; per-connection and per-user counts and last-used date beside the acknowledgement list (the org admin sees the record, no lock is built: OD-Z4 default).
- dirty/untracked collisions: WorkspaceSettingsPage.tsx (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
- cross-design overlap: Z-04, Z-20, Z-42 edit WorkspaceSettingsPage.tsx: same chain C10 order Z-04, Z-20, Z-21b, Z-42.
- deploy: S12 - frontend: gateway and SPA.
- live proof (none): Z-60 probe Z2: count equals 1.
- notes_for_implementer: BFF handler test that non-admins get 403 and the response schema has no content field.

#### Z-31: e2e helper and new ZDR specs

- design: zdr (Z-31) | plane: Frontend | size: M | chain: C10 #8 | kind: code
- depends_on: Z-19, Z-20 (design had: Z-19, Z-20)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound (authored by this manifest)
- files:
  - apps/Frontend Plane/verevonv3/tests/e2e/product-subscription-route.ts
  - apps/Frontend Plane/verevonv3/tests/e2e/zdr-connection-waiver.spec.ts
  - apps/Frontend Plane/verevonv3/tests/e2e/browser-workspace-zdr.spec.ts
- corrections (folded in):
  - Relax the helper's "zdr must not be true" guard to "zdr true only with an acknowledged connection"; keep the rule that tests never fall back to Balance or Claude (standing rule: live tests on gpt-5.6-terra via openai-codex-subscription).
  - The first run must fail at Z0 against a build without Z-19; evidence lines are ids, counts, codes and timings only (no content), as in A6.
- cross-design overlap: None.
- deploy: none
- live proof (none): Run as part of Z-60.
- notes_for_implementer: Probes Z0-Z6 per design section 11 with evidence lines; no tokens or cookies printed.

#### Z-42: Interactive-only ZDR scope wording and /trust text

- design: zdr (Z-42) | plane: Frontend | size: S | chain: C10 #9 | kind: code
- depends_on: Z-30, Z-21b (design had: Z-30)
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx
  - apps/Frontend Plane/verevonv3/apps/verevon-web/src/app/trust
  - docs/compliance/evidence/trust-page-edits.diff
- corrections (folded in):
  - The tooltip over-promise is already fixed by the uncommitted trust-edits job (WorkspaceSettingsPage.tsx zdrTooltip says the content still leaves Verevon and goes to the provider; pinned by in-product-trust-copy.test.ts): only ADD the scope sentence (ZDR covers interactive chat content; it does not cover connector ingestion, search engines or derived processing) and the per-connection rule, coordinated with Z-04.
  - Public text goes through the trust-edits approval; apps/verevon-web/src/app/trust is dirty and docs/compliance/evidence/trust-page-edits.diff is untracked: coordinate.
  - Snapshot tests of the tooltip in both languages; claims-vs-code row updated.
- dirty/untracked collisions: WorkspaceSettingsPage.tsx (M); apps/verevon-web/src/app/trust (dirty); docs/compliance/evidence/trust-page-edits.diff (untracked)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx; dir-has-changes: Frontend Plane/verevonv3/apps/verevon-web/src/app/trust; untracked: docs/compliance/evidence/trust-page-edits.diff
- cross-design overlap: Trust-edits job; Z-04.
- deploy: S12 - frontend: SPA and verevon-web.
- live proof (none): None.
- notes_for_implementer: Add the sentence, keep the pinned wording tests green.

#### Z-60: Live proof on Terra with a ZDR-on test org

- design: zdr (Z-60) | plane: Frontend | size: M | chain: C10 #10 | kind: live_proof
- depends_on: Z-15, Z-16, Z-17, Z-18, Z-19, Z-20, Z-21b, Z-31, Z-32, Z-14, Z-13, Z-12 (design had: Z-15, Z-16, Z-17, Z-18, Z-19, Z-20, Z-21, Z-31, Z-32)
- gates: start [G-ENFORCERS-BEFORE-MINTERS, G-OPS-ORG-ZDR, G-OWNER-ZDR-PROOF] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/Frontend Plane/verevonv3/tests/e2e/zdr-connection-waiver.spec.ts
  - apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/ZDR_WAIVER_TERRA_PROOF.md
- corrections (folded in):
  - Free probes first: Z0 (refusal without acknowledgement: no run row, no broker audit line, no non-Terra provider in inference-core logs) and Z1 (stale digest 409, other member 403, X-Internal-API-Key refused, client zdr:false ignored, direct token mint has no waiver, the ZDR test org mints zdr:true) must pass before ANY Terra call.
  - Terra only: gpt-5.6-terra via openai-codex-subscription, never Verevon Balance and never Claude; about 7 to 10 Terra rounds; say the count.
  - Test org: a dedicated pro org with two members, provisioned for real (OPS-ORG); member A completes the ChatGPT device sign-in (owner-interactive); org ZDR switched on for that org ONLY through the operator seed path (Z-04 runbook: a direct org-core PATCH with an operator credential), because the BFF gate refuses while nothing is attested. Never toggle an existing org.
  - Z4 (Do under waiver) needs AGENTIC_SUBSCRIPTION_ENABLED on for the window (live compose default is off; launcher env only) and plan_mode; do not claim it closed beyond what the probe shows.
  - ZDR_WAIVER_ENABLED is on only for the proof window and only after the launcher check passes (S14); afterwards decide to leave it on once the evidence note passes.
  - Evidence note path plans/system-audits/ZDR_WAIVER_TERRA_PROOF.md: ids, counts, codes and timings only; trust claims are re-verified against it (X-TRUST).
- cross-design overlap: Z-31 spec; X-TRUST after.
- deploy: S14 - No image: flag window.
- live proof (subscription): About 7 to 10 Terra rounds on the owner subscription (zero Verevon Balance spend); evidence lines only.
- notes_for_implementer: Probe list fixed before the run: Z0 refusal without ack; Z1 enforcement checks; Z2 one Terra Ask after acknowledgement (run row zdr_waived with ack id, thread persisted, usage frame and answer line, audit events, only openai-codex-subscription in logs); Z3 second member without acknowledgement refused and member A on another model refused (proves never org-wide); Z4 Do with plan_mode under waiver; Z5 revoke then refused, catalog_version bump then refused until re-ack; Z6 Temporary chat plus Terra refused; the org flag is unchanged; a non-ZDR org sees no change.

#### X-TRUST: Trust claims re-verification after the live proofs (trust-edits part b)

- design: manifest | plane: Frontend | size: M | chain: C10 #11 | kind: docs
- depends_on: Z-42, Z-60
- gates: start [G-PROOFS-GREEN] done [] deploy []
- review source: added by this manifest
- verdict: NEW (plan wave D and live proof 9; owner ruling 6: trust edits completed with only claims that are true on the code and the live stack).
- files:
  - apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-claims.ts
  - apps/Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-data.ts (+ trust-data.test.ts, TrustCenter.test.tsx)
  - docs/compliance/evidence/trust-claims-pending-reverification.md and the pinned evidence docs (check-citations.mjs)
  - apps/Frontend Plane/verevonv3/VEREVON.md (row 25)
- corrections (folded in):
  - Part (a) sentences that do not depend on shipped behaviour may start now in parallel by the trust-edits track; part (b) here covers the ZDR-, rerank- and contextual-retrieval-dependent sentences (26 pending sentences; rows 14, 23, 24, 25 among them): each states only what the proofs show. The mustSay pins about the ChatGPT route and the privacy tooltips are updated to the per-connection rule.
  - Anchors: contextualize.rs "Deliberately NOT tightened here" and "This path never carries restricted content" comments and extractor.rs:286-297 are anchors in the evidence docs and in trust-claims.ts: they were kept or updated by CR-T7/CR-T13/CR-T12; re-run check-citations.mjs and trust-data.test.ts.
  - Where the proof is missing (e.g. ZDR chat itself, until an attestation exists; embed/page-image/rerank for ZDR orgs, DEF-CR-EMBED-ZDR-ORACLE) the copy states the gap instead of claiming it.
- dirty/untracked collisions: trust-claims.ts and trust-data.test.ts, TrustCenter.test.tsx, trust-recipients.ts (untracked); TrustCenter.tsx and trust-data.ts (modified): trust-edits track
- working tree right now (git status, read-only, other sessions' uncommitted work): untracked: Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-claims.ts; modified: Frontend Plane/verevonv3/apps/verevon-web/src/components/trust/trust-data.ts
- cross-design overlap: Trust-edits track; Z-20, Z-42.
- deploy: S12 - frontend: verevon-web; restart as needed after git status check.
- live proof (none): Trust-claims tests pass against the proven behaviour (plan live proof 9).
- notes_for_implementer: For each pending sentence, cite the proof evidence (id) or rewrite; pin the wording in a test; do not publish a claim the proofs do not show.

### Chain C11 quality-ctx

#### CR-T14: Quality harness: committed corpus ingest, A/B cells, coverage gate, persisted results, ctx-v2 promotion gate

- design: contextual (T14) | plane: quality | size: L | chain: C11 #1 | kind: code
- depends_on: CR-T8, CR-T9a, CR-T11 (design had: T8, T9, T11)
- gates: start [G-T11-DONE] done [] deploy [G-T8-GREEN, G-OWNER-OD8, G-OPS-ORG]
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest).
- files:
  - apps/Data Plane v2/scripts/eval-ingest-corpus.py (new)
  - apps/Data Plane v2/scripts/eval-corpus-manifest.json (new)
  - apps/Data Plane v2/scripts/eval-attribution-study.sh
  - apps/Data Plane v2/scripts/eval-gate.py
  - apps/Data Plane v2/scripts/eval-baseline.json
  - apps/Data Plane v2/scripts/eval-nightly.sh
  - apps/Data Plane v2/scripts/eval-fixture-membership.sql
  - apps/Data Plane v2/scripts/golden-v2.json
  - apps/Data Plane v2/services/data-quality-go/internal/eval/store.go
  - apps/Data Plane v2/Makefile
- corrections (folded in):
  - The pinned corpus manifest must be a FROZEN snapshot (sha256 of snapshot files committed under the repo), not live repo paths that change every commit; the golden judgments key on document ids (relevant_ids) and the two orgs use different id sets (golden-v2.json and golden-v2-ctx.json): regenerate the judgments (eval-build-golden-set.py) or use deterministic document ids.
  - The corpus is the repo's own 95 markdown files ingested by an unrecorded process (retrieval-fusion-evidence-2026-08-26.md:1123-1129): record the ingest in the script; ingestion goes through documents-api with the corpus-seeder principal.
  - The throwaway eval orgs need real provisioning (old fixture orgs are gone; tenant posture refuses unknown orgs): OPS-ORG. Baseline vs contextual split uses org_processing_policy off vs inherit (operator-only).
  - Reranker noise: about 19% of queries are lost to 429s (scripts/eval-gate.py:33-35): run rerank-on cells three times; attribution with the reranker off first.
  - Owner approval OD8 required for the paid re-seed (about 1,164 chunk calls, about 0.15 USD plus Cohere embed) and the standing nightly eval.
  - data-quality-go internal/eval/store.go is the same module as CR-T10/CR-T11: this task starts after CR-T11 (gate). The Data Plane Makefile hunk comes after CR-T5's.
  - Line endings per the standing rule: Python and .md CRLF; .sh, .json, .yml LF (verify each neighbour with node).
- cross-design overlap: Feeds CR-T15 (rerank input with context) and the optional breadcrumb (deferred CR-T18).
- deploy: S6 - No image; scripts and a data-quality-go change (image rebuild with the late Data stage).
- live proof (owner-approved): With owner approval (OD8): re-seed the eval orgs (about 1,164 chunk calls, about 0.15 USD, outside the handful of tiny proof calls), run baseline vs contextual-ctx-v1 and record the new committed baseline (nDCG@10 0.7732 contextual baseline from the 2026-08-26 evidence must not be undercut by the CR and rerank cells); then ctx-v1 vs ctx-v2 on the canary policy and decide promotion by the gate. Say how many calls were made.
- notes_for_implementer: Add a deterministic corpus ingest (frozen manifest of path+sha256) that creates org-corpus-baseline and org-corpus-contextual; cells for ctx-v1, ctx-v2, a model candidate, rerank-input-with-context (needs CR-T15 flag) and (if built) breadcrumb; attribution with the reranker off, confirm with rerank on three times; coverage and parity gates in eval-gate.py; persist each run's summary in quality_eval_runs; extend the golden set with Norwegian-morphology and mixed-language queries; nightly job. Gate unit tests first, verified in BOTH directions: exit 1 on a regression past tolerance, on coverage below 99%, on too few scored queries; exit 0 on the committed baseline; the ingest script is idempotent; the manifest check fails on a changed file hash.

### Chain C12 docs

#### QB-T13a: Ownership documents and decision ledger: rows 70-71, rule 15, GDPR note, ledger entry (T13a)

- design: quarry (T13); parent: QB-T13 | plane: docs | size: S | chain: C12 #1 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: KEEP with corrections: split so that rule numbers are written in the order the plan states (15 here, 18 in QB-T13b).
- files:
  - apps/master-ownership-matrix.md
  - apps/CODEBASE_INFORMATION_SYSTEM.md (:119, :159)
  - apps/GDPR_SUMMARY.md (note near :106-108)
  - apps/Model Plane/docs/decisions/ledger.md
  - apps/Frontend Plane/verevonv3/docs/WEB_SOURCE_QUALITY_AUDIT_2026-09-15.md (A3 status)
- corrections (folded in):
  - Matrix rows to amend are 70-71 (row 69 is Embedding generation, critic-quarry #12), plus the owner rule at :155 (rule 2) and a new rule 15: "Data Plane owns rerank of caller-supplied candidates, including public web results" (Quarry and Model Plane are callers, subject to ZDR, sovereign and processor gates; direct provider HTTP from the Data Plane is by design).
  - GDPR_SUMMARY.md note: processor approval AND allow_third_party_processing=true are both required (critic-quarry #8: GDPR_SUMMARY.md:106-107); record that Quarry's PrivacyPolicy.allow_third_party_processing is not carried on the search path today.
  - Ledger entry state proposed at the start, implemented via X-DOCS-FINAL; append at the top in ONE hunk (ledger.md is append-only, newest first, 3,453 lines).
  - Ownership docs must land in the same change set as QB-T05a so the matrix is never contradicted by the code.
- cross-design overlap: Same files as CR-T16, Z-30 and QB-T13b: single docs chain C12, order QB-T13a (15), CR-T16 (16), Z-30 (17), QB-T13b (18).
- deploy: none
- live proof (none): n/a (documentation); confirm every file changed by QB-T05a/QB-T09 is referenced by the ledger entry.
- notes_for_implementer: Run scripts/lint-resource-taxonomy.sh and any matrix checks; add a grep check in the QB-T11 docs-contract test that the matrix names POST /v1/rerank.

#### CR-T16: Ownership documents, decision ledger, compliance evidence and runbook

- design: contextual (T16) | plane: docs | size: M | chain: C12 #2 | kind: code
- depends_on: none
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: APPROVE WITH CORRECTIONS (authored by this manifest). Docs chain, written in matrix-rule order.
- files:
  - apps/master-ownership-matrix.md
  - apps/CODEBASE_INFORMATION_SYSTEM.md
  - apps/Model Plane/docs/decisions/ledger.md
  - apps/Model Plane/docs/ZDR.md
  - apps/Data Plane v2/docs/contextual-retrieval-runbook.md (new)
  - apps/Data Plane v2/docs/retrieval-fusion-evidence-2026-08-26.md
  - apps/Data Plane v2/docs/gap-data.md
  - apps/Data Plane v2/DATA_PLANE_STATUS.md
  - docs/compliance/evidence/data-flow-map.md
  - docs/compliance/evidence/honest-limits.md
  - docs/compliance/evidence/subprocessors-from-code.md
  - docs/compliance/evidence/claims-vs-code.md
  - apps/Data Plane v2/docs/contextual-retrieval-runbook.md is new (already listed)
- corrections (folded in):
  - Matrix rules are numbered 15 to 18 (rules 1-14 already exist at lines 152-168): CR contributes rule 16 (tenant retention posture for background LLM work and metering; fold in "every paid provider call is metered under an org-attributed feature" and "ZDR is never lowered org-wide"). Written AFTER QB-T13a (rule 15) and BEFORE Z-30 (rule 17) and QB-T13b (rule 18). The design's rules 10 and 11 do not exist. Lines 69 (embedding generation), 87 (cost ledger) and 155 (owner rule text) are correct anchors.
  - honest-limits L19 (honest-limits.md:44) is about usage on the ChatGPT-account route, NOT Data Plane background calls: do not claim it "changes once metering lands"; add new limit rows instead. Correct L12, G13, G14 and R15.1 in data-flow-map.
  - The compliance evidence files are untracked (trust-edits track: data-flow-map.md, honest-limits.md, subprocessors-from-code.md, claims-vs-code.md). Coordinate; run node docs/compliance/evidence/tools/check-citations.mjs on each edited file and re-pin moved anchors. Model Plane ledger.md is 3,453 append-only lines, newest first: insert entries in ONE hunk at the top.
  - Draft early (state proposed), finalise with measured numbers via X-DOCS-FINAL after the live proofs.
  - Also fix the stale ZDR.md statement about the Anthropic attestation knob and DATA-15 in gap-data.md.
- dirty/untracked collisions: docs/compliance/evidence/*.md are untracked (trust-edits track)
- working tree right now (git status, read-only, other sessions' uncommitted work): untracked: docs/compliance/evidence/data-flow-map.md; untracked: docs/compliance/evidence/honest-limits.md; untracked: docs/compliance/evidence/subprocessors-from-code.md; untracked: docs/compliance/evidence/claims-vs-code.md
- cross-design overlap: Same files as QB-T13a/b and Z-30: single docs chain C12, sequential.
- deploy: none
- live proof (none): No paid call. Re-run the live read-only queries cited in the evidence files and confirm each statement against the stack after the rollout (X-DOCS-FINAL).
- notes_for_implementer: Apply the document changes of design section 13 with the corrections above: new matrix rows (Contextual retrieval; Background provider metering), amended rows (cost ledger line 87, embedding generation line 69), rule 16, ledger entries (append newest first, with state proposed): CR permanently on and bounded-inline plus reconciler; tenant retention posture; metering architecture and rejected alternatives; graph state machine. Runbook with the incident procedures (429 storm, stuck backlog, ZDR org report, prompt rollout/rollback, budget exhausted, kill switch and purge, Quickwit parity repair via documents.indexed re-announce NOT rebuild, cost spike triage). Run check-citations.mjs first, then after every edit.

#### Z-30: Ownership, ledger, GDPR and compliance document updates

- design: zdr (Z-30) | plane: Docs | size: M | chain: C12 #3 | kind: code
- depends_on: CR-T16
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: sound-with-corrections (authored by this manifest)
- files:
  - apps/master-ownership-matrix.md
  - apps/CODEBASE_INFORMATION_SYSTEM.md
  - apps/GDPR_SUMMARY.md
  - apps/Model Plane/docs/decisions/ledger.md
  - apps/Model Plane/docs/ZDR.md
  - apps/Control Plane/docs/core-research/auth-core.md
  - apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md
  - docs/compliance/evidence/honest-limits.md
  - docs/compliance/ai-data-policy.md
  - docs/CROSS_PLANE_CONTRACT_MATRIX.md
- corrections (folded in):
  - Rule 17 (per-connection waiver, amends rule 4 / contract 4): written after rule 16 (CR-T16) and before rule 18 (QB-T13b). Also: the Auth Core to integration-corev2 read edge in the contract matrix and CONTROL_PLANE_OWNERSHIP.md, the GDPR_SUMMARY processor exception for user-owned connections, ZDR.md rewrite to the current state, honest-limits L17-L19 updates plus a new limit for waived runs, and honest-limits line 45 / open item 5 (admin lock OD-Z4) updated: the owner's ruling means no lock is built and the line must not show "owner to confirm".
  - Claims no stronger than the code; trust page text is delivered with the trust-edits job and X-TRUST. docs/compliance/evidence/honest-limits.md is untracked (trust-edits): coordinate, run check-citations.mjs.
  - Draft early (state proposed); finalise via X-DOCS-FINAL after Z-60.
- dirty/untracked collisions: docs/compliance/evidence/honest-limits.md (untracked)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Control Plane/docs/core-research/auth-core.md; untracked: docs/compliance/evidence/honest-limits.md
- cross-design overlap: Docs chain C12.
- deploy: none
- live proof (none): None.
- notes_for_implementer: Doc test that every task code and refusal code named in docs exists in source; the citation checker passes on updated evidence.

#### QB-T13b: Ownership documents: rule 18 (service-principal route allowlist, exception to rule 9) (T13b)

- design: quarry (T13); parent: QB-T13 | plane: docs | size: S | chain: C12 #4 | kind: code
- depends_on: QB-T13a, CR-T16, Z-30
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: NEW split (plan section 2: rule 18 is last).
- files:
  - apps/master-ownership-matrix.md
  - apps/Model Plane/docs/decisions/ledger.md
- corrections (folded in):
  - Rule 18: the Data Plane retrieval engine admits service principals only through a default-deny route allowlist and skips the membership check for them; this is an exception to rule 9 (identity and membership are Control Plane-owned); record documents-api-go's existing precedent.
- cross-design overlap: Docs chain C12 order.
- deploy: none
- live proof (none): n/a.
- notes_for_implementer: One small hunk in the matrix and one ledger line; numbering 15, 16, 17, 18 verified.

#### X-DOCS-FINAL: Docs finalisation after the live proofs: ledger state implemented, measured numbers, evidence re-verification

- design: manifest | plane: docs | size: S | chain: C12 #5 | kind: docs
- depends_on: QB-T13b, CR-T16, Z-30
- gates: start [G-PROOFS-GREEN] done [] deploy []
- review source: added by this manifest
- verdict: NEW (CR-T16/QB-T13a/Z-30 are drafted early and finalised here).
- files:
  - apps/Model Plane/docs/decisions/ledger.md (flip entries from proposed to implemented)
  - apps/Data Plane v2/docs/contextual-retrieval-runbook.md, retrieval-fusion-evidence-2026-08-26.md addendum, DATA_PLANE_STATUS.md
  - docs/compliance/evidence/data-flow-map.md, honest-limits.md, subprocessors-from-code.md, claims-vs-code.md (re-run the live read-only queries cited; check-citations.mjs)
- corrections (folded in):
  - Replace targets with measured numbers from the proofs; every statement checked against the stack; document the open items (embed/page-image ZDR oracle gap, ZDR chat needs an attestation, rerank ZDR skip marker) so nothing is overclaimed.
- cross-design overlap: Docs chain C12.
- deploy: none
- live proof (none): Re-run read-only queries after rollout.
- notes_for_implementer: Ledger append in one hunk; citations re-pinned.

### Chain C13 ops

#### Z-40: Attest a ZDR provider route (operator action)

- design: zdr (Z-40) | plane: Model | size: M | chain: C13 #1 | kind: operator_action
- depends_on: none
- gates: start [] done [] deploy []
- review source: authored by this manifest from the raw design, the critics and repo checks (the review JSON was truncated)
- verdict: operator action, not code (authored by this manifest; owner default D-A)
- files:
  - apps/Model Plane/docs/ZDR.md
  - apps/Model Plane/deploy/docker-compose.yml
  - apps/Model Plane/scripts/release-artifact.sh (the real path; the repo-root path in the design does not exist)
- files removed from the design list:
  - scripts/release-artifact.sh
- corrections (folded in):
  - Path correction: the release script is apps/Model Plane/scripts/release-artifact.sh (ZDR_RETENTION_PATH is referenced by its tests under apps/Model Plane/scripts/tests/).
  - Owner default D-A: no model provider is attested as ZDR-capable today; this is a commercial/operator step. The attestation is DATA (AZURE_OPENAI_ZDR_* evidence fields: resource, approval ref, effective date, review-by, reviewer, digest, set through the release path ZDR_RETENTION_PATH=zdr-provider-route-attested), never a code flip; build everything so it is data.
  - Conflict to record, relying on neither: sovereign-rag-blueprint-reconciliation.md:79-82 calls Embed v4 on Azure-EU DataZone "ZDR" while the phased plan lists contractual ZDR as a TODO.
  - Without an attestation the BFF switch gate (Z-04) keeps the org ZDR toggle disabled and ZDR chat itself is unusable: that is intended.
- dirty/untracked collisions: apps/Model Plane/deploy/docker-compose.yml (M)
- working tree right now (git status, read-only, other sessions' uncommitted work): modified: Model Plane/deploy/docker-compose.yml
- cross-design overlap: Z-41 and QB egress guard consume the attestation as data.
- deploy: none
- live proof (none): Free: catalogue shows zdr on the covered model; first ZDR chat on the test org succeeds (owner OK needed for one minimal paid Azure call; never on Verevon Balance).
- notes_for_implementer: Owner/operator obtains the signed no-retention approval, records the fields and sets them only through the release path; verify /v1/models shows a zdr feature and /readyz reports attested capability honestly. Existing zdr.rs validation tests plus a readiness test.
