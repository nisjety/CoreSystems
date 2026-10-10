# Program summary and status (2026-10-07)

What has been done and completed, what is deployed, what is built but not deployed, what is blocked, and what comes next. Written when the 53-task build finished; nothing new was launched after the owner's instruction to stop and summarise.

## 1. Where we are in one paragraph

Over the last week the chat system went from "built but not proven" to proven on the ChatGPT Terra route (Do mode, plan proposals, knowledge and web search, wide research), four planes were redeployed, and a larger program started from the owner's rulings of 2026-10-03: make contextual retrieval perfect (stays on), move reranking into the Data Plane with Quarry as a caller (option B), make zero retention (ZDR) work per org with per-connection informed consent, and apply the trust edits. That program found and fixed real defects that had existed for weeks (an org's ZDR setting could never reach its users' tokens; the AI-action audit trail had never delivered an event). The Control Plane part is deployed and live-proven. The other 23 build tasks that passed adversarial review are in the working tree and are NOT deployed. 26 tasks are blocked and have not started; 3 were implemented but not reviewed; 1 has to be redone. Weekly usage stands at about 65 percent, so the rest is paced over the next window and week.

## 2. The whole arc, by date

| Dates | What happened |
|---|---|
| 2026-09-24 to 09-30 | Product journeys qualified on Terra: all four passed five consecutive private rehearsal passes on build 1ff7c6a268d6; first-turn p95 43.0 to 105.7 s, follow-up p95 45.3 to 57.7 s, no timeouts (evidence: product-recordings/evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md). Section-only revision and finer review batches shipped. |
| 2026-10-01 | Parity audit: the chat system does NOT yet have parity with the reference systems (10 of 62 gap rows, 33 of 75 chat-doc items). A gap-closure plan was written and Wave 0 started (paid-model leaks closed, Do on Terra built dark). Live proof A6 on Terra (details in A6_DO_MODE_TERRA_PROOF_2026-10-01.md): loop, plan proposals, knowledge search, web search and a three-way wide research fan-out all work, every round on the subscription route, no cost-core entries; six defects found by live runs and fixed. Batch 3 (17 tasks: connectors, charts, Office extractors, drain, planner JSON, compliance evidence) reviewed. |
| 2026-10-02 | Items 1 to 5 of the owner's list done: Quarry search scopes granted to execution-core, J3 (mail-to-task principals, dark), J13/J14/G12, pytest and the H1 tests (71 of 71), paid-path check. Control, Model, Ingestion and Frontend planes deployed. Journey smoke test on the final build passed 4 of 4. Final regression sweep clean apart from one known pre-existing test. |
| 2026-10-03 | Owner asked why Quarry has its own reranker (no recorded reason; the ownership rules say it should not exist) and gave rulings for contextual retrieval, rerank, Quarry option B, ZDR consent and the trust edits. Three designs, adversarial critiques, an integrator plan, and a build manifest (68 task entries, 13 chains, 40 gates). Trust edits applied and audited. Two pre-existing live defects discovered (section 5). |
| 2026-10-04 to 10-06 | Usage limits cut the build twice; agents died mid-task and the build was resumed with a resume notice. Control Plane spine finished (W0-1 to W0-4). Audit-trail fix built. |
| 2026-10-06 to 10-07 | Control Plane wave deployed (org-core, then auth-core) and live-proven. The 53-task build ran to completion of what it could do (section 4). |

## 3. Deployed and live-proven

**Control Plane wave (deployed 2026-10-06 evening; org-core first, then auth-core, with the Control Plane's own runner, billing-core deliberately not touched).**

- org-core serves a new internal route GET /internal/orgs/{id}/retention-posture that returns { zdrIntent, entitled } computed server-side, behind a new narrow scope org:retention:read:any that only Auth Core holds.
- Auth Core resolves an org's ZDR posture through that route. A known ZDR org is never downgraded during an org-core outage (last known answer is served: 24 hours for ZDR orgs, 15 minutes for non-ZDR orgs); an org with no known answer gets HTTP 503 retention_posture_unavailable and no token; failures are never cached.
- New tenant retention posture for background workers, with a 60 second token cap; one principal registry for development and production (19 principals, including data-orchestrator and a separate BFF retention-decision principal; quarry-edge holds data:rerank; execution-core holds orgIds [global]).
- Live proof: a real token mint returns 200 with zdr false for the test org; the three live Pro orgs resolve intent false and entitled true (the old entitlements path wrongly said not entitled because the cache omitted the feature); an unknown org returns 404 organization not found; no credential rejection anywhere in the stack in the five minutes after the deploy.
- A trap was avoided before deploying: the stored principal registry had drifted (retrieval-engine and conversation-core credentials differed from the running registry and from what those services present). The running credentials were restored into the stored registry first (hashes only, backup deleted), so the deploy only added principals and rotated nothing.
- Not yet proven live: a ZDR-on org minting zdr true, and the org-core outage producing 503. Both need a Pro test org with ZDR switched on (operator step).

**Earlier deployments of this week** are listed in A6_DO_MODE_TERRA_PROOF_2026-10-01.md and CHAT_GAP_CLOSURE_PLAN_2026-10-01.md (section 0): Do on Terra behind a switch (on in the dev stack only through the launcher environment; compose defaults are off), typed subscription failures, agentic stream follow, run budgets, capability attestation for knowledge and web search, child-run registration fix, drain on redeploy, planner JSON schema, per-answer processing line, connector and chart work, J1 and J2.

## 4. The 53-task build (contextual retrieval, Quarry option B, zero retention)

Result: 23 reviewed and shipped (22 after the reviewer fixed defects), 3 implemented without a review, 1 failed, 26 not started. Reviewers made no "do not ship" call. All of it is in the working tree and is NOT deployed. Run: workflow wf_0dd51c33-48a, 19.5M agent tokens, 7,939 tool calls, about 22 hours; detail per task in the workflow journal.

### 4.1 Built and reviewed (not deployed)

| Task | What | Chain | Review |
|---|---|---|---|
| CR-T6 | Postgres migration: CR provenance and status, org_processing_policy, coverage view, legacy backfill | C1 dp-cr | ship, reviewer fixed defects |
| CR-N1 | CI wiring: real-migrator Postgres contract tests that fail instead of silently skipping | C1 dp-cr | ship |
| CR-T5 | Data Plane shared crate inference-client-rs (token client, tenant posture, bearer cache, typed statuses) | C1 dp-cr | ship, reviewer fixed defects |
| CR-T7 | Contextualizer v2: outcomes, ctx-v2 prompt, windowing, validation, usage capture, floors | C1 dp-cr | ship, reviewer fixed defects |
| CR-T8 | Batch pipeline: write-ahead context, vector coherence, reuse, inline deadline, cross-document concurrency | C1 dp-cr | ship, reviewer fixed defects |
| QB-M1b | Embedding-engine: stamp documents.source into the Qdrant payload and backfill existing points (M1b) | C1 dp-cr | ship, reviewer fixed defects |
| CR-T11 | Org processing policy admin API, coverage endpoint and release gates | C1 dp-cr | ship, reviewer fixed defects |
| CR-T10 | Usage ledger v2: shared event crate, durable stream, cost_events columns, forwarder to cost-core, ZDR numeric- | C1 dp-cr | ship, reviewer fixed defects |
| CR-T9a | Context reconciler, backfill/repair CLI, legacy verification pass and Postgres-vs-Quickwit parity repair (T9a) | C1 dp-cr | ship, reviewer fixed defects |
| CR-T13 | Defaults and config: CR ON by default, boot-time contract, .env.example, compose contract, dedicated deploymen | C1 dp-cr | ship, reviewer fixed defects |
| CR-T9b | Reconciler budget gate: alert-only by default, optional stop mode (T9b) | C1 dp-cr | ship, reviewer fixed defects |
| CR-T12 | graph-index hardening: durable per-chunk state, backoff and poison cap, typed errors, metering, tenant posture | C2 dp-graph | ship, reviewer fixed defects |
| QB-T03 | Retrieval engine: principal-aware auth with a default-deny service route allowlist (fixes the 403) | C3 dp-retrieval | ship, reviewer fixed defects |
| CR-T14 | Quality harness: committed corpus ingest, A/B cells, coverage gate, persisted results, ctx-v2 promotion gate | C11 quality-ctx | ship, reviewer fixed defects |
| CR-T3 | inference-core: typed ZDR/residency/tier statuses, per-service counters, shared wire fixtures | C4 mp-gateway | ship, reviewer fixed defects |
| Z-10 | Shared golden fixtures: posture matrix and connection capability contract | C4 mp-gateway | ship, reviewer fixed defects |
| Z-08 | Quarry takes the token zdr claim as a floor | C9 ing-quarry | ship, reviewer fixed defects |
| QB-T02 | Quarry cross-plane token-request manifest and its Auth Core contract | C9 ing-quarry | ship, reviewer fixed defects |
| Z-04 | Gate the org ZDR switch on an attested provider | C10 fe | ship, reviewer fixed defects |
| QB-T13a | Ownership documents and decision ledger: rows 70-71, rule 15, GDPR note, ledger entry (T13a) | C12 docs | ship, reviewer fixed defects |
| CR-T16 | Ownership documents, decision ledger, compliance evidence and runbook | C12 docs | ship, reviewer fixed defects |
| Z-30 | Ownership, ledger, GDPR and compliance document updates | C12 docs | ship, reviewer fixed defects |
| QB-T13b | Ownership documents: rule 18 (service-principal route allowlist, exception to rule 9) (T13b) | C12 docs | ship, reviewer fixed defects |

### 4.2 Implemented, review not done (the usage limit killed the reviewer)

| Task | What | Chain | State |
|---|---|---|---|
| QB-T04 | Retrieve contract for non-user callers: SourceRef.url, wire golden, strict zdr_mode | C3 dp-retrieval | implemented; review killed by the usage limit |
| Z-00 | Stop the silent subscription re-route under token ZDR | C4 mp-gateway | implemented; review killed by the usage limit |
| Z-12 | integration-corev2 schema, capability assessor and backfill | C8 ing-integration | implemented; review killed by the usage limit |

### 4.3 Failed (redo)

| Task | What | Chain | State |
|---|---|---|---|
| QB-T09a | Quarry: SearchProvider::search_with_report plumbing, RerankReport and the cache rule (T09a) | C9 ing-quarry | implementer killed by the usage limit; nothing usable |

### 4.4 Not started (blocked behind the tasks above)

| Task | What | Chain | Blocked |
|---|---|---|---|
| QB-N2 | Retrieval engine: redact the query in the retrieval.start log when the mode is restrictive (N2) | C3 dp-retrieval | waits for: QB-T04 |
| QB-M1a | Retrieval engine: server-enforced Quarry corpus scope for service principals (M1a) | C3 dp-retrieval | waits for: QB-T04, QB-N2 |
| QB-T05a | Data Plane rerank capability: POST /v1/rerank core, policy, limits, idempotency, metrics, interim v1 cost even | C3 dp-retrieval | waits for: QB-M1a |
| QB-T05b | Shared rerank provider permit pool, token bucket and circuit breaker (T05b) | C3 dp-retrieval | waits for: QB-T05a |
| QB-T08 | Config, compose, OpenAPI, runbook, auth matrix rows and dark ZDR attestation | C3 dp-retrieval | waits for: QB-T05a, QB-T05b |
| Z-41 | ZDR-capable query embedding path or explicit lexical degrade | C3 dp-retrieval | waits for: QB-T05a, QB-T08 |
| QB-M | Switch the web-rerank cost producer to the v2 ledger builder: feature web-rerank, units = billed search units  | C3 dp-retrieval | waits for: QB-T05a, Z-41 |
| CR-T15 | Retrieval: rerank input with chunk context (measured, flag-gated) | C3 dp-retrieval | waits for: QB-T05a, QB-T05b, QB-M |
| Z-01 | Strict zdr claim and equal posture across delegated bearers | C4 mp-gateway | waits for: Z-00 |
| Z-05a | Typed ZDR refusal catalogue and refusal sites in the Model Plane, including the silent-drop closure (Z-05a, ab | C4 mp-gateway | waits for: Z-01 |
| Z-06 | ZDR stream provenance and usage frame in the Model Plane (metering half deferred as Z-06b; SPA hunks moved to  | C4 mp-gateway | waits for: Z-05a |
| Z-11 | mp-contracts retention resolver | C4 mp-gateway | waits for: Z-05a, Z-06 |
| Z-15 | model-gateway waiver enforcement and stamping | C4 mp-gateway | waits for: Z-00, Z-01, Z-05a, Z-06, Z-11 |
| Z-16 | inference-core route-binding backstop | C4 mp-gateway | waits for: Z-01, Z-11, Z-15 |
| QB-T10 | Quarry vector leg: correct request, ZDR/sovereign skip, scoped corpus, strict mirror, timeouts | C9 ing-quarry | waits for: QB-T09a, QB-T04, QB-M1a |
| QB-N1 | Quarry: rrf_fuse keeps engines and fusion_score; canonical URL dedup (N1) | C9 ing-quarry | waits for: QB-T10 |
| QB-T09 | Quarry: Data Plane rerank client replaces the LLM reranker, with explicit status, ZDR and cache rules | C9 ing-quarry | waits for: QB-T09a, QB-T05a, QB-T10, QB-N1 |
| QB-T17 | Decide and fix Quarry's other Model Plane dependents (autoprompt, answer, ai-formats, page-title) | C9 ing-quarry | waits for: QB-T09 |
| QB-T11 | Quarry docs, OpenAPI, SDKs and compose cleanup | C9 ing-quarry | waits for: QB-T09, QB-T17 |
| QB-T14 | Judged web-result set, runner, calibration and regression gate | C9 ing-quarry | waits for: QB-T05a, QB-T11 |
| QB-T12 | Model Plane consumers: score_kind, rerank status, calibration in relevance.rs | C4 mp-gateway | waits for: QB-T09, QB-T14, Z-15, Z-16 |
| Z-17 | session-core retention receipt and derived-job exclusions | C5 mp-session-exec | waits for: Z-11 |
| Z-18 | execution-core waiver-aware Do and children | C5 mp-session-exec | waits for: Z-11, Z-17, Z-05a |
| Z-21a | session-core read-only aggregate RPC of waived runs per connection and user (Z-21a) | C5 mp-session-exec | waits for: Z-17, Z-18 |
| Z-13 | Acknowledgement API with audit, revocation and admin list | C8 ing-integration | waits for: Z-12 |
| Z-05b | SPA: localized rendering of every ZDR refusal code with actions (Z-05b) | C10 fe | waits for: Z-05a |

### 4.5 Held by design (not in this run)

CR-T4 (cost-core ingest v2: another session is editing cost-core with uncommitted migrations 0006 and 0007), Z-03, Z-14, CR-T17, Z-32 (need the Control Plane code, now done, plus their own prerequisites; they can start in the next run), Z-09, Z-19, Z-20, Z-21b, Z-31, Z-42 (depend on Z-14), X-TRUST and X-DOCS-FINAL (after live proofs), Z-40 (operator action: attest a ZDR-capable provider), Z-60 (live ZDR proof on Terra with a ZDR test org).

## 5. Defects found that existed before this work

| Defect | Status |
|---|---|
| An org's ZDR setting could never reach interactive tokens: org-core answered 401 to Auth Core's lookup, the resolver fell back to zdr false, and the entitlement list omitted the zero-retention feature when its cache was warm. | Fixed and deployed (Control Plane wave). |
| The AI-action audit trail ("Brukt av AI?") never delivered an event: 4,992 events from 2026-09-04 to 10-01 died on "no stream found for given subject" (stream provisioner password mismatch on an optional bus), and reservation events were published on the wrong subject (2,477 of them). | Fix built and reviewed (stream manifest, publisher subject, runtime guard, idempotent requeue). NOT deployed. The stream itself exists live since 2026-10-04. Replay of the stuck rows waits for the deploy. |
| Quarry's own reranker prompts a general model and has failed with 403 on every search since it became the default; Quarry's vector retrieval also got 403. | Option B in progress: service-token admission on the retrieval engine (QB-T03) is built and reviewed; the rerank endpoint (QB-T05a) is blocked. |
| Contextual retrieval was on only through an ignored .env file, unmetered, ignored org ZDR, failed silently and left a mixed index (206 of 207 chunks contextualised, lexical index at 57 of 207). | Provenance migration, shared inference client, contextualizer v2, write-ahead pipeline, reconciler, org policy API, graph-index hardening and the quality harness are built and reviewed. NOT deployed. |
| No model provider is attested as ZDR-capable on this stack, so a ZDR request is refused everywhere. | Open: operator and commercial step (Z-40). |
| Credential drift in the stored principal registry (two principals). | Worked around before the deploy. Origin not found. |

## 6. Incidents and hygiene

- During QB-T03 an implementer's dry run reached the real docker by mistake, read the quarry-edge credential into a shell variable (never printed) and minted 8 service tokens (300 s lifetime, audience data-plane) for a non-existent org. They expired. The engine saw no request. The script now needs an explicit CONFIRM_LIVE flag. Auth Core's token-issuance events could not be counted.
- Secrets that appeared in agent transcripts (local files only): GRAPH_INDEX_GDPR_NATS_PASSWORD and the first 40 characters of CODEX_SUBSCRIPTION_INTERNAL_API_KEY. Rotation recommended, together with the quarry-edge credential from the incident above. Nothing was rotated.
- Usage-limit cut-offs killed agents mid-edit twice. Resumed tasks inspect and finish or remove only their own partial edits. Other sessions' uncommitted work (cost-core, billing-core, insight-core, conversation-core, parts of Frontend) is in the same tree and was never touched.

## 7. What is proven and what is not

Proven live: the Control Plane wave (section 3); Do mode loop, plan proposals, knowledge and web search, wide research on Terra (A6); drain on redeploy; planner JSON; per-answer processing line.
Reviewed and unit/contract tested only, never run against the live stack: everything in section 4.1 (reviewers ran the Postgres suites on throwaway databases; several need a live proof with a few paid calls or a Pro test org); the audit-trail fix; the trust edits.
Not run at all: the ZDR consent flow end to end, the Data Plane rerank endpoint, Quarry calling it.

## 8. Trust page

The trust edits (J13) are applied in the working tree and audited sentence by sentence, but not deployed. 30 sentences about ZDR, rerank and contextual retrieval are listed in docs/compliance/evidence/trust-claims-pending-reverification.md and must be re-verified against the final behaviour before anything is published; a drift test fails if one goes stale. Until the audit-trail replay passes, the page must keep saying the trail is not verified end to end.

## 9. Owner decisions and operator steps still open

- Price sheet for embeddings and Cohere rerank (none in the repo; none invented).
- Counsel sign-off on the ZDR acknowledgement wording and the Art. 28 basis for sending text to Azure and Cohere.
- The Azure ZDR attestation (operator), and a Pro test org with ZDR switched on (operator) for the live ZDR proofs.
- OD-LAPSE (does a plan lapse drop ZDR; default applied: no), per-org contextual retrieval off or budget stop on customer orgs, Temporary chat and unattended runs under a waiver (default: no), owner approval of the paid evaluation re-seed.
- Data Plane background-LLM budget figure (default: alert only).
- Rotate the three credentials in section 6.
- The other session's cost-core work must be committed and deployed before CR-T4 and the metering forwarder.

## 10. Next steps, in order

1. Resume the build: workflow wf_0dd51c33-48a with the same script (scratchpad copy archived in program-2026-10-03/build-dag-A2.workflow.js). It re-runs the four killed agents (QB-T09a implement; review of QB-T04, Z-00, Z-12) and then unblocks the 26 waiting tasks. Estimated 55 to 65 agent runs, roughly 18 to 22M agent tokens: start it in a fresh 5-hour window and expect to resume once more. Weekly usage is about 65 percent now; the week resets on 2026-10-12 at 23:00 UTC.
2. Quiet-tree model plane deploy (session-core audit fix, inference-core typed statuses and metrics), then the audit-trail replay: canary of 10 rows, then the rest (steps in apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md).
3. Data Plane deploy in the manifest order: the provenance migration (the migrate one-shot), usage consumers before producers, then the embedding-engine pipeline, reconciler and graph-index; per-service rather than whole-plane where another session has uncommitted work in the same plane.
4. The rerank endpoint and Quarry as a caller, the ZDR enforcers before the minters, the consent API, then the Frontend.
5. Live proofs with tiny paid usage; chat only on Terra. Then X-TRUST re-verification and the final trust text.

## 11. Where things are

- Archived planning artifacts (this folder: program-2026-10-03/): owner-rulings-and-defaults.md, design-plan.txt, design-contextual.json, design-quarry.json, design-zdr.json, critic-contextual.txt, critic-quarry.txt, critic-zdr.txt, build-manifest.json, build-manifest.md, gen_dag_workflow.cjs, build-dag-A2.workflow.js, dag-a2-final.json.
- Evidence and plans: A6_DO_MODE_TERRA_PROOF_2026-10-01.md, CHAT_GAP_CLOSURE_PLAN_2026-10-01.md (section 0 status), CHAT_PARITY_AUDIT_2026-10-01.md, docs/compliance/evidence/ (claims-vs-code.md, honest-limits.md, trust-claims-pending-reverification.md, live reads), apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md, apps/Control Plane/config/contracts/README.md.
- Deploy method: stage copies of build-verevon-services.sh; for single Control Plane services use scripts/run-control-plane.sh up -d --build --no-deps <service>; always diff the stored principal registry against the running one first; mint new credentials with run-control-plane.sh config and read its stderr.
