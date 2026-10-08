# CoreSystem Master Ownership Matrix

> **Runtime note — 2026-07-13:** the ownership rules below remain canonical,
> but they do not certify current availability. Model Plane gateway/inference
> gRPC is absent in the running stack. Authenticated additive source contracts
> and the ordinary invoke caller graph pass tests but are not deployed; the safe
> rebuild gate is documented in
> [Model Plane status](Model%20Plane/MODEL_PLANE_STATUS.md).

> **Application authority note — 2026-07-13:** Control Plane is the source of truth
> for identity and exact organization membership. Application Plane may maintain
> revocable projections, owns the canonical notification request API and application
> delivery ledgers, and must fail closed when Control authority is unavailable.
> Frontend/gateway and Ingestion workers are clients; neither caller-supplied scope
> nor a local projection can grant access. See the dated Application Plane
> [audit](Application%20Plane/docs/core-research/plane-audit-2026-07-13.md).

> Generated: 2026-05-06  
> Scope: CoreSystem Infra, Quarry v2, Data Plane, Model Plane, App/Shell, and future reference tooling.
> Rule: **Core Infra routes local traffic. Quarry captures evidence. Data Plane knows. Model Plane reasons. App Shell presents.**
> Privacy contract: [`GDPR_SUMMARY.md`](./GDPR_SUMMARY.md).

## 0. Decision Rules

> **Learning extension (2026-09-24):** `apps/learning-plane` owns human-learning
> course definitions/revisions, assignments, attempts, formative progress,
> compatibility validation and teaching-assistance intent/receipts. Data still
> owns source materials/assets/retrieval; Model owns reasoning, speech execution and agent
> runs; Control owns identity, authorization, canonical audit and the planned shared
> WhisperFlow service API/job metadata/processing policy; Application
> owns realtime projections/notifications; Support remains transport-only.
> `apps/Frontend Plane/learn-core` owns Teachme's Next.js marketing, Vite/Solid 2
> dashboard and native C#/.NET Windows companion. These are foundation sources,
> not runtime acceptance. See [ADR-001](learning-plane/docs/ADR-001-learning-plane.md).


| Question | Owner |
|---|---|
| Does it fetch, browse, screenshot, render, crawl, replay, or produce deterministic source evidence? | **Quarry v2** |
| Does it store, chunk, embed, index, retrieve, graph, version, or maintain durable knowledge? | **Data Plane** |
| Does it plan, reason, synthesize, act as an agent, choose tools, write memory/wiki updates, or run research loops? | **Model Plane** |
| Does it expose human-facing graph/wiki/IDE/CLI/canvas/voice UX? | **App Shell** |
| Does it terminate or route local HTTP traffic without reading or owning plane data? | **CoreSystem Infra Plane** |
| Is it latency-sensitive, CPU/memory-sensitive, parsing-heavy, retrieval-heavy, browser-runtime-heavy, or protocol-heavy? | **Rust** |
| Is it durable workflow, registry, policy, scheduling, resource CRUD, grants, billing, or operator control? | **Go** |
| Is it eval/research/model-lab/provider-specific ML glue only? | **Python** |
| Does it rendezvous or relay a live remote-desktop session between a support agent's browser and a customer's machine? | **Support Plane** (unmodified RustDesk `hbbs`/`hbbr`); the browser-side protocol is `@verevon/remote-core` in the Frontend Plane |

## 1. Canonical Plane Matrix

| Tool / feature | Quarry v2 | Data Plane | Model Plane | App Shell | Rust | Go | Python |
|---|---|---|---|---|---|---|---|
| Static fetch / TLS impersonation | **Owner** | — | — | — | `quarry-tls`, `quarry-runtime` | — | lab benchmark only |
| Browser actions | **Owner** | — | Consumer via agent contract | UI inspector | `quarry-browser`, `ActionRuntime` | browser lease admin | browser-use lab only |
| Browserbase sessions | **Owner** | — | Requests browser capability | session viewer link | Browserbase CDP driver | profile/grant lifecycle | no |
| Kernel browsers / kernel-images | **Owner** | — | Requests runtime lease | live view / replay links | CDP adapter / VM session driver | lease, quota, policy | no |
| Browserless | **Owner** | — | Requests browser capability | session viewer link | CDP-over-WS driver | config/admin | no |
| Stagehand `goto/observe/act/extract` low-level browser operations | **Owner for execution + observations** | optional source artifact ingest | **Owner for natural-language planning** | debug/playback UI | action/observation protocol | grant/job lifecycle | reference SDK only |
| Stagehand Agent / autonomous workflow | Executes browser steps only | stores resulting evidence if ingested | **Owner** | task UI | browser observation execution | Temporal orchestration | prototype only |
| Browser Use / computer-use screenshot agents | Executes screenshots/actions | stores evidence artifacts only | **Owner** for VLM/action decisions | live view | screenshot/action runtime | grants/quotas | prototype only |
| Firecrawl-like scrape formats | **Owner** for html/raw/markdown/links/images/screenshot/pdf/chunks/change/meta | optional ingest | summary/json/query generation only | API docs | transforms | job/control | no |
| AI `summary` / `json` / `query` formats | Captures source + stores artifacts | retrieval facts | **Owner** | display | bridge/output validation | jobs/cost | provider-specific only |
| `audio` format | evidence/reference only | optional artifact metadata | **Owner** speech provider routing | playback UX | artifact handling | async job/event flow | possible SDK glue |
| Static branding extraction | **Owner** | optional metadata storage | visual fallback only | display | CSS/HTML parser | catalog/policy | no |
| Rendered/visual branding | captures screenshot/DOM | stores metadata if promoted | **Owner** visual interpretation | display | screenshot capture | async job | vision lab/provider |
| Data Plane document ingest from Quarry | Sends markdown/chunks/meta | **Owner** | consumes retrieval later | status display | payload shaping | lifecycle orchestration | no |
| Documents service | writes via API only | **Owner** | reads only via API | doc admin UI | heavy transforms only | **documents-api-go** | legacy/prototype |
| Chunking / knowledge-unit creation | source producer only | **Owner** | consumer only | inspection UI | **index-engine-rs** | reindex orchestration | eval only |
| Embedding generation | no | **Owner**, including the paid provider calls made to prepare an embedding (contextual retrieval, next row), each metered under rule 16 | no independent embedding | status UI | **embedding-engine-rs** | batch control | provider SDK fallback only |
| Contextual retrieval (a model-written situating context stored beside each chunk) | no | **Owner**: whether it runs, with which model and prompt, and its status, repair and budget; the context is derived personal data stored with the chunk and erased with its row, though not yet purged from the keyword indexes (`docs/compliance/evidence/honest-limits.md` L35 and L22) | routing only: serves the one completion call through `inference-core` and never decides tenant retention (rule 16) | no screen is planned: operators read the coverage endpoint and the runbook queries, and there is no organisation-admin control | **embedding-engine-rs** (the pipeline; the reconciler is planned), **index-engine-rs** (units) | operator policy and coverage API in `data-quality-go` (planned) | eval only |
| Vector retrieval / rerank / source join | **caller only**: reads the vector leg for its own Quarry-origin corpus and sends caller-supplied candidates (web results and those Quarry-origin vector hits) to `POST /v1/rerank`; never reranks itself (rule 15) | **Owner** for tenant knowledge and for caller-supplied candidates (rule 15) | consumer only; never reranks itself | search UI | **retrieval-engine-rs** | control/rebuild | eval only |
| Hybrid retrieval (BM25 + dense + rerank) | **caller only for the Data Plane legs**: fuses Data Plane vector hits with its own web results and its own local page index (Tantivy over pages Quarry itself scraped); owns no tenant knowledge index and does not rerank (rule 15) | **Owner** | chooses retrieval mode | search UI | retrieval engine | policy/config | eval only |
| Graphify-style corpus graph | captures raw sources | **Owner** graph extraction/storage | graph-aware context use | graph viewer | AST/graph extraction | graph registry/jobs | graphify reference/lab |
| GraphRAG indexing | no | **Owner** | consumes graph context | operator rebuild UI | graph extraction/retrieval | rebuild schedules | prototype/reference |
| GraphRAG query synthesis | no | returns graph context | **Owner** synthesis | answer UI | context packing | task policy | eval |
| LLM Wiki durable pages/versioning/source log | source capture only | **Owner** wiki storage/index | proposes updates | wiki UX | page diff/index | lifecycle/policy | lab |
| LLM Wiki maintenance agents | no | stores accepted pages | **Owner** contradiction/stale/orphan agents | review UI | synthesis helpers | workflows/approval | eval |
| Logseq-like graph/page UX | no | backs page/graph APIs | provides suggested edits | **Owner** | local transforms if needed | app API control | no |
| Autoresearch loops | no | stores experiment artifacts/results if needed | **Owner** | research dashboard | execution steps | Temporal loops/budgets | experiment runtime |
| OpenClaw gateway/channels | no | no | routing decisions | **Owner for UX** | runtime execution | channel/gateway control | no |
| Hermes tools/toolsets/MCP/cron | no | memory persistence only | **Owner** | CLI/channel UX | runtime tool execution | capability/orchestrator | no |
| Codex/Claude Code-style local coding shell | no | source/project knowledge only | **Owner** coding agent | **Owner** CLI/IDE | execution core | bridge/control | no |
| TOON compact transport | can encode artifacts/observations | encode retrieval facts/wiki tables | encode tool/context payloads | display/debug | **codec-rs** | registry policy | no |
| Caveman terse mode | no | no | response-format policy | **UX owner** | formatting runtime | config policy | no |
| MCP registry | no | no | **Owner** capability use | shell integrations | runtime adapter | **capability-core-go** | no |
| Plugin registry | no | no | **Owner** runtime policy | shell/admin UI | plugin execution boundary | **capability-core-go** | no |
| Tasks / cron | no | reindex/refresh jobs only | **Owner** for agent tasks | task UI | step execution | **orchestrator/task-core-go** | no |
| Cost ledger / budgets | ZDR/cost metadata only | emits exact usage for its own paid calls (embed, rerank, contextual retrieval, graph extraction) under an org-attributed feature and forwards it to cost-core; keeps no second ledger of record (rule 16) | **Owner** inference/agent cost | billing display | metering hooks | **cost-core-go**: ledger of record; the chat budget counts chat rows only | no |
| Background provider metering | no | **Owner** of the usage event (exact tokens from the provider response, one idempotent event per call group, no document ids) and of its delivery: durable stream, then the forwarder in `data-orchestrator-go` | provides the exact usage in the response and the typed refusal; owns no Data Plane ledger | cost page reads cost-core | `usage-ledger-rs` (shared event crate, planned) | **data-orchestrator-go** (consumer; the forwarder is planned), **cost-core-go** (ledger of record) | no |
| Own-connection ZDR: capability record, acknowledgement and per-run waiver (rule 17; designed, not built) | reads the run's `zdr` body flag, and an interactive user's own token claim only as a floor; never reads or issues a waiver | takes the run's posture from its verified token claim and never decides a waiver (a waived run retrieves as a non-ZDR run with restricted documents filtered) | **Owner of enforcement** of the Control Plane's decision: route binding in model-gateway, inference-core and execution-core, the receipt on the run in session-core; never decides the posture | acknowledgement prompt, composer chip, answer line, admin record | model-gateway, inference-core, execution-core, session-core, the shared resolver in `mp-contracts` | **integration-corev2** (capability record, acknowledgement ledger); the only issuer is Auth Core (Control Plane, TypeScript) | no |
| Evals / scoreboards | capture benchmarks | retrieval benchmarks | agent/model benchmarks | dashboards | benchmark targets | CI/go test harness | **lab/evals only** |

## 2. Service Target Structures

### Quarry v2 target

| Service / crate | Language | Owner scope |
|---|---|---|
| `quarry-edge` | Rust | Public REST/SSE, request normalization, ZDR guards, cache admission, Model/Data bridge client |
| `quarry-runtime` | Rust | PageRunner, DriverPlan waterfall, security policy, deterministic execution |
| `quarry-browser` | Rust | Chromiumoxide, Browserless, Browserbase, Kernel CDP adapters, action runtime, observations |
| `quarry-transform` | Rust | Markdown/html/links/images/attributes/chunks/diff/branding/static outputs |
| `quarry-security` | Rust | DNS guard, URL signatures, blocklist hints, SSRF enforcement |
| `quarry-control` | Go | Jobs, runs, schedules, profiles, presets, webhooks, resource CRUD |
| `quarry-orchestrator` | Go | Durable crawl/batch/scrape workflows, BFS, checkpoints, pause/resume/backfill |
| `lab/evals` | Python | Firecrawl/Browserbase/Kernel benchmark adapters only |

### Data Plane target

| Service | Language | Owner scope |
|---|---|---|
| `documents-api-go` | Go | Document CRUD, metadata, org-scoped lifecycle, status, internal ingest |
| `index-engine-rs` | Rust | Chunking, normalization, AST/code graph extraction, dedupe/fingerprint, knowledge units |
| `embedding-engine-rs` | Rust | Batch embedding, backpressure, retries, provider parity, Qdrant upsert/delete |
| `retrieval-engine-rs` | Rust | Hybrid retrieval, query embedding, ANN, BM25/sparse, rerank, source join, context packaging |
| `graph-index-rs` | Rust | Graphify/GraphRAG extraction, entity/edge/community summaries, provenance labels |
| `wiki-store-go` | Go | Wiki pages, versions, source log, maintenance log, permissions, retention |
| `data-orchestrator-go` | Go | Reindex, rebuild, refresh, compaction, graph/wiki jobs, queue inspection |
| `retrieval-eval-py` | Python | RAG evals, chunking/rerank experiments, offline scoring only |

### Model Plane target

| Service | Language | Owner scope |
|---|---|---|
| `model-gateway` | Rust | Public invoke, stream, structured output schema passthrough, ZDR/cross-plane guards |
| `session-core` | Rust | Threads, runs, checkpoints, memory index, compaction, context assembly |
| `inference-core` | Rust | Provider routing, streaming, structured outputs, prompt cache, speech/vision/doc provider traits where native |
| `execution-core` | Rust | Tool loop, hooks, browser-agent observation/action protocol, secret scrub, subagents |
| `orchestrator-core` | Go | Temporal workflows, autoresearch, wide research, tasks/cron, approvals, recovery |
| `capability-core` | Go | Tools, commands, skills, plugins, MCP, model routing, safety, memory adapters |
| `browser-broker` | Go | Trusted browser grants, validate/revoke, session lease permissions |
| `letta-bridge` | Go | Optional long-term memory block bridge; not thread or retrieval source of truth |
| `cost-core` | Go | Token/cost ledger, budgets, usage events |
| `graph-lab-py` / `eval-lab-py` | Python | LangGraph/LangChain/Deep Agents/eval prototyping; no production hot path |

### Support Plane target

| Component | Language / runtime | Owner scope |
|---|---|---|
| `hbbs` / `hbbr` | Rust, upstream `rustdesk/rustdesk-server` run **unmodified** (AGPL-3.0 — run-as-a-process, never copied; see `packages/remote-core/docs/licensing.md`) | Peer rendezvous and relay only; no Verevon identity, session or billing state |
| Caddy `proxy` | Docker | TLS termination for the two plaintext WebSocket ports (21118/21119) — the only internet-facing entry for browsers |
| `readiness` | busybox sidecar | TCP-probes all four ports; the upstream image is `FROM scratch` with no shell, so in-container healthchecks cannot work |
| `@verevon/remote-core` | TypeScript (Frontend Plane, `packages/remote-core`) | Clean-room RustDesk wire protocol, crypto, codecs, permission model, AI frame sampling; framework-agnostic, no UI |
| Gateway `remote_support` domain | Rust (Frontend Plane gateway) | Serves connection config only (`/api/v1/remote-support/config`); the live session never transits the gateway |

### CoreSystem Infra Plane target

| Component | Language / runtime | Owner scope |
|---|---|---|
| `core-infra-gateway` | Go | Local operator self-status only; no cross-plane API, database, cache, Docker socket, or gRPC surface |
| Traefik | Docker | Local loopback ingress, explicit routes, rate limits, and edge security headers |
| NGINX | Docker | Local static landing surface only |
| `coresystem-edge` | Docker network | Routeable HTTP services only; never databases, brokers, queues, or object stores |

## 3. Non-negotiable Cross-Plane Contracts

1. **No direct database crossing.** Model and Quarry never connect to Data Plane Postgres/Qdrant directly.
2. **No independent embeddings or reranking.** Model Plane and Quarry never embed or rerank independently except in isolated eval labs. Reranking is a Data Plane capability that they call (rule 15), never a model prompt of their own.
3. **No agent bypass around Quarry policy.** Model Plane can propose browser actions; Quarry executes or rejects them.
4. **ZDR propagates across planes.** If `zeroDataRetention=true`, Quarry rejects or keeps ephemeral-only any Data/Model enrichment that would persist content. **Amended by rule 17:** the one exception is a per-run, acknowledged waiver for a user's own connection, issued only by the Control Plane and never organisation-wide. Quarry does not read or honour it. For a service caller it takes the run's flag from the request body, which the Model Plane sets from the run's resolved posture; from an interactive user's own token it reads the `zdr` claim only as a floor, which a waiver token (it carries `zdr: false`) never raises.
5. **GDPR policy metadata travels with data.** Durable records and processing jobs carry or link to purpose, lawful basis, retention, residency, privacy class, third-party processing allowance, and deletion scope.
6. **Knowledge assets are Data Plane-owned.** Graphs, wiki pages, embeddings, chunks, source logs, contradiction indexes live in Data Plane.
7. **Reasoning is Model Plane-owned.** Autoresearch, graph-aware synthesis, wiki maintenance agents, browser-agent planning live in Model Plane.
8. **UX is App/Shell-owned.** Logseq-like page/graph editing, CLI/TUI, IDE bridge, channel inbox, voice/canvas surfaces live above the core planes.
9. **Identity and membership are Control Plane-owned.** Sensitive callers resolve the exact user/organization decision through the canonical Control authority; active-organization headers, session hints, and Application projections are not grants. **Bounded exception by rule 18:** a first-party service principal is not a member and is admitted to the Data Plane retrieval engine by a default-deny route allowlist, never by this decision; interactive users are unchanged.
10. **Application projections only narrow access.** Convex, notification, conversation, and other Application services may cache or mirror membership for availability, but a denial/removal must revoke the projection and an authority outage must never create or widen access.
11. **Notification intake is Application Plane-owned.** The canonical contract is `POST /api/v1/notification-requests`; Ingestion support workers and the Frontend gateway own their client calls and must propagate authentication, tenant, retention, and delivery failures honestly.
12. **Provider execution never invents approval authority.** Conversation owns its durable human intent or approved-AI action; Ingestion Integration owns provider execution and the single-use receipt. An effectful call requires a tenant-bound service bearer plus a short-lived signature over the exact durable authorization, actor, tenant, provider effect, payload digest, and idempotency key. Every other issuer, including Model Plane, fails closed until it implements an equivalent durable contract.
13. **Core Infra is traffic-only.** It may expose a local HTTP route to a plane-owned public gateway, but it never shares or administers a plane database, cache, broker, object store, tenant scope, provider credential, or user authority. A service on `coresystem-edge` is not granted access to another plane.

14. **Support Plane is transport-only for remote sessions.** It rendezvous-matches and relays end-to-end-encrypted RustDesk sessions and grants nothing: who may reach which customer device, and any durable record that a session happened, are Control/Application concerns. Until an owning core persists remote-session records there is deliberately **no** action-registry contract for "start remote session" — a dispatcher with nothing behind it would be fabricated state. Control permissions (mouse/keyboard) come from the customer's machine per session and are never inferred from screen-view permission.

15. **Rerank of caller-supplied candidates is Data Plane-owned.** The Data Plane owns reranking of tenant knowledge and of candidate documents a caller supplies with the request, including public web search results (`POST /v1/rerank` on `retrieval-engine-rs`). Quarry and Model Plane are callers: they never rerank independently and never prompt a general-purpose model to order results. The provider is the platform's own paid reranker; reranking never runs on a subscription or bring-your-own route. The Data Plane calling that provider directly over HTTPS is by design and is not a bypass of rule 2 or of the Model Plane gate preferred in [`GDPR_SUMMARY.md`](./GDPR_SUMMARY.md); these gates apply instead. The call honours the run-level ZDR mode the caller sends and never relaxes it: a restrictive mode or a sovereign requirement returns an explicit `skipped` marker with the caller's own order, never a silent reorder and never provider egress. The processor gate (approved processor **and** `allow_third_party_processing=true`) applies as `GDPR_SUMMARY.md` states, including its recorded gaps. Only a named service principal holding the dedicated rerank scope may call it. Ownership, not availability: the decision and its implementation state are recorded in the [decision ledger](Model%20Plane/docs/decisions/ledger.md) entry of 2026-10-03.

16. **Background content processing follows the tenant's retention posture, and every paid provider call is metered under an org-attributed feature.** Work the Data Plane does on a tenant's content that no user requested (contextual retrieval, graph extraction and community summaries, and the index-time embedding and rerank calls) resolves the organisation's zero-retention (ZDR) posture when its service token is minted, through the Control Plane: the `tenant` retention posture of the service-principal registry, with Auth Core as the only authority. It fails closed. When the posture cannot be resolved the mint returns 503 `retention_posture_unavailable`, no token is issued, and the Data Plane defers the work: it never sends and never gives up for good. A ZDR organisation's content goes only to a route attested for ZDR; otherwise the unit carries an explicit marker (`skipped_zdr`), never a silent fallback to the raw chunk. **ZDR is never lowered organisation-wide** by a background job, a policy table or an operator switch. A per-run downgrade for a user's own connection is decided on the consent path of the interactive run and never by background work, and background work never uses a user's subscription or bring-your-own connection. The Data Plane sets the provider and residency floors on these calls itself; the Model Plane routes and does not decide tenant retention. Every paid provider call these jobs make is recorded as exact usage (organisation, feature, model, provider, tokens) by the Data Plane and forwarded to cost-core, the ledger of record; a paid call with no usage record is a defect. Background rows are attributed to the organisation and excluded from the chat budget, so they cannot downgrade a person's chat model. Contextual retrieval itself stays on: the deployment flag is an incident-only kill switch, and day-to-day control is an operator-set, audited per-organisation policy that never acts silently. **Recorded gap:** the same posture oracle is not yet applied to index-time text embedding, page-image embedding and rerank, which are guarded per document and per request until the owner decides that scope (`docs/compliance/evidence/honest-limits.md`, L33). Ownership, not availability: the decisions and their implementation state are in the [decision ledger](Model%20Plane/docs/decisions/ledger.md) entries of 2026-10-03 (contextual retrieval always on, the tenant retention posture, background provider metering, the graph extraction state machine).

17. **A user's own connection may lower zero retention (ZDR) for one run, and only through a signed, acknowledged waiver bound to that connection.** It amends rule 4 and nothing else. A member who connects their own subscription or key (today only the ChatGPT account route, `openai-codex-subscription`; no own-key connection exists) and whose organisation has ZDR on is told, when the connection is added and again at first use, whether it can provide ZDR. If it cannot, they may continue after an informed acknowledgement, and ZDR is then off for the runs that name that connection, never for the organisation, never for another member, never for another connection or model. *Authority.* Auth Core is the only issuer. It re-reads the organisation's posture (the resolver of rule 16), the connection's capability record and the stored acknowledgement itself. It signs a short decision only for a service-attested request from a dedicated BFF principal that can ask and can assert nothing, and it stamps a connection-bound `zdr_waiver` claim on the tokens of that one turn. A client can never send `zdr: false` (the effective posture is the OR of the signed claim and the request flag, so a client's `false` never lowers it, and Auth Core's service mint routes refuse a client `zdr` outright), and the organisation's setting is never written. *Record.* The connection's ZDR capability and the append-only acknowledgement ledger belong to integration-corev2 (Ingestion Plane), written in the connection's own audit transaction. Auth Core reads the connection's posture as the session user, a Control Plane to Ingestion Plane read edge recorded in [`CROSS_PLANE_CONTRACT_MATRIX.md`](../docs/CROSS_PLANE_CONTRACT_MATRIX.md). *Enforcement.* model-gateway, inference-core and execution-core honour a waiver only for the exact connection it names and refuse every other route, they never re-route. session-core records an immutable receipt (retention class `zdr_waived`) on the run. The Data Plane and Quarry take their posture from the run's verified claim and body flag and never decide a waiver. A waived run is a normal persisted run. Retrieval for it filters restricted-class documents so they never reach the personal account, and its content-derived background jobs that would send run content to other processors (memory indexing, dreaming extraction, learning review) are skipped. *Never.* Never an explicit Temporary chat or a Space policy, never scheduled, unattended, support, inbox, browser or speech work, never a background job (rule 16), never an operator or admin switch. *Not decided here.* No admin lock exists: the owner ruled on 2026-10-03 that members are not stopped, so the record is audit and admin visibility only. Whether a member may lawfully override the organisation's instruction to its processor is a question for counsel ([`GDPR_SUMMARY.md`](./GDPR_SUMMARY.md)). *Order.* Enforcers ship before the issuer, and the kill switch `ZDR_WAIVER_ENABLED` (default off) before enablement. **State: designed, not built.** Ownership, not availability: the decision and its implementation state are in the [decision ledger](Model%20Plane/docs/decisions/ledger.md) entry of 2026-10-03 (own-connection ZDR).

18. **A first-party service principal is admitted to the Data Plane retrieval engine by a default-deny route allowlist, not by organisation membership: a bounded exception to rule 9.** A caller whose verified token says `principal_type=service` is not an organisation member (Auth Core's answer for a `service:` subject is `not_member`), so the membership decision that gates an interactive user can never admit it. `retrieval-engine-rs` therefore admits a service on its HTTP surface only through a table in code, reviewed with the handler it admits, that names for each service the exact method and route it may call and the scope its token must carry. Anything not named is refused: 403 `service_principal_route_not_allowed`, or `service_principal_scope_missing` when the route is named and the scope is not. The key is the router's matched route pattern, never the raw path, so a route added to the router stays closed to every service until someone lists it. *What stays Control's.* Identity and tenancy. The token is signed by Auth Core, which mints one only for a registered principal and for an organisation its registry row allows (a fixed list or the org-core directory, never a blanket "any organisation" in production). The engine refuses a contradictory identity with 401 (a service id that differs from the subject, a user or acting user on a service token, no scopes), refuses an `x-org-id` header that differs from the token's organisation, and pins the request to the token's organisation. *What a service never gets.* No membership authority. Its request context keeps only the scopes the table can name (`data:read`, `data:rerank`), so an organisation-wide visibility scope that its registry ceiling holds is dropped at admission; it has no per-user grants, and the user-core visibility facade is never asked about it; a route that exists for services only (`POST /v1/rerank`, rule 15) refuses a user before any membership lookup; and the gRPC surface, which authorises by membership, refuses a service outright. The interactive user path is unchanged. *Precedent.* `documents-api-go` already admits a verified service principal on a per-route scope (`RequireServiceScope`) with no membership lookup, and accepts a service identity only with a service id equal to the subject, no user id and at least one scope. Rule 18 states that practice as a contract and narrows it in the retrieval engine from a scope per route group to a named service, route and scope. Any other Data Plane service that admits a service principal must do so on the same terms: a verified identity of that shape and a scope the Control registry issued for that route, never a header or a body field. No other service has been audited against this yet (see the ledger entry). *Not decided here.* Which scopes a principal holds (the Control registry; the `data:rerank` grant to `service:quarry-edge` belongs to the Control Plane principal work) and any change to rule 9 for users. Ownership, not availability: the decision, the rows of the table and its implementation state are in the [decision ledger](Model%20Plane/docs/decisions/ledger.md) entry of 2026-10-06 (service principals and the exception to rule 9).

## 4. Research Notes Used

- Kernel: cloud browser sessions expose CDP URLs, VM isolation, live view, replay, long-lived session states, and scaling; treat as browser runtime substrate for Quarry.
- Browserbase: Search/Fetch/Browsers/Contexts plus live view and recordings; treat as cloud-browser backend for Quarry and browser-agent substrate for Model Plane.
- Stagehand: primitives `goto`, `observe`, `act`, `extract`, and `agent`; split low-level execution to Quarry and natural-language planning to Model Plane.
- Graphify: multimodal folder-to-knowledge-graph pipeline with AST pass, LLM extraction, NetworkX/Leiden clustering, provenance labels; canonical Data Plane graph extraction reference.
- GraphRAG: LLM extraction of entities/relationships/claims, community detection/summaries, vector store outputs; Data Plane indexing and Model Plane synthesis consumer.
- LLM Wiki: persistent markdown wiki between raw sources and answers; Data Plane owns the durable wiki, Model Plane owns maintenance agents.
- Autoresearch: program-file-driven bounded experiment loop; Model Plane orchestration pattern.
- Logseq: graph/page human knowledge UX; App Shell pattern, not a core plane owner.
- TOON: compact, schema-aware JSON-equivalent prompt encoding; Rust codec in runtime/context layers.
