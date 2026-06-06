# CoreSystem — Architecture Map (as-built, verified 2026-06-05)

Companion to `apps/master-ownership-matrix.md` (that doc is the canonical *target*;
this captures the **actual** service dirs / ports / container names observed on disk).

## The pyramid (authority flows strictly downward; a plane never writes another plane's DB)

| Layer | Plane | Canonical dir | Owns |
|------|-------|---------------|------|
| L1 | **Control Plane** | `apps/Control Plane` | Identity, users, orgs, billing, entitlements, quotas, sessions — **authority root** |
| L2 | **Data Plane v2** | `apps/Data Plane v2` | Canonical knowledge: store, chunk, embed, index, hybrid retrieval, GraphRAG, LLM wiki, versioning |
| L3 | **Ingestion Plane** | `apps/Ingestion Plane` | Fetch/crawl/connectors/file-import — produces evidence, persists **through Data Plane contracts** |
| L4 | **Model Plane** | `apps/Model Plane` | Reasoning, agents, tool choice, synthesis, memory/wiki-update proposals, browser-agent planning |
| L5 | **Application Plane** | `apps/Application Plane` | Convex collaborative workspace + realtime sync (Affine docs) |
| L6 | **Frontend Plane** | `apps/Frontend Plane` | `velion` (v1) + `velionv2` UI + BFF |
| — | **Channel Plane** | `apps/Channel Plane` | **Docs-only stub today** — planned channel inbox (email/Slack/Teams) + voice/canvas UX |

## Decision rules (which plane owns a capability)
- fetch / browse / screenshot / crawl / replay / source evidence → **Quarry v2** (Ingestion)
- store / chunk / embed / index / retrieve / graph / version / maintain knowledge → **Data Plane v2**
- plan / reason / synthesize / agent / tool-choice / memory / wiki updates / research loops → **Model Plane**
- human-facing graph/wiki/IDE/CLI/canvas/voice UX → **App / Frontend**
- Language: **Rust** = latency/parsing/retrieval/browser/protocol-heavy · **Go** = durable workflow/registry/policy/scheduling/CRUD/grants/billing · **Python** = eval/lab/provider-ML glue only

## Services per plane (verified)

**Control Plane (L1):** `auth-core:3011` · `user-core:3012` · `org-core:8080` · `billing-core:3014` · `session-core` · `audit-core`.

**Data Plane v2 (L2):** `documents-api-go` (doc CRUD) · `index-engine-rs` (chunk/knowledge-units) · `embedding-engine-rs` (Qdrant) · `retrieval-engine-rs` (BM25+dense+rerank) · `graph-index-rs` (GraphRAG) · `wiki-store-go` (LLM wiki) · `data-orchestrator-go` (reindex/jobs) · `data-quality-go` · `quickwit-adapter-rs` · `retrieval-eval-py`. Infra: `dpv2-{postgres,redis,qdrant,nats,minio,quickwit}`. (`apps/Data Plane` = v1 Python legacy.)

**Ingestion Plane (L3):** `Quarry-v2` (web access; API `8090`, Rust edge) · `imports-core` (CSV/Excel/PDF/DOCX/JSON, `3025`) · `integration-core`+`integration-corev2` (OAuth/connectors: Notion/HubSpot/Salesforce/Odoo/Slack/Git, `3026`/`3126`) · `finspo-core` (SharePoint/M365) · `autocomplete-core` (quick lookups) · connector-runtime (Nango, `3003`). Shared: postgres/redis/nats/temporal/qdrant.

**Model Plane (L4):** Rust — `model-gateway:8080/9090` (public boundary, SSE, usage) · `session-core:8081` (thread/run/checkpoint authority, memory) · `inference-core:8082` (provider routing, fallback, prompt cache) · `execution-core:8083` (runtime loop, tools, permission gates, subagents). Go — `orchestrator-core:8084` (Temporal) · `capability-core:8085` (skill registry/policy/budget) · `sandbox-manager:8086` · `browser-broker:8087` (executes Quarry-policied browser actions) · `letta-bridge:8088` (memory) · `cost-core` · `bridge-core` (MCP/LSP). Holds Data Plane v2 clients (Retrieval/Knowledge/Graph/Wiki — **consumer only**). `apps/Model Plane v2` = legacy being deprecated (prior memory: build on v1).

**Application Plane (L5):** `convex-backend:3210/3211` · `convex-dashboard:6791` · `convex-gateway:3005→3000` · `convex-subscriber` (NATS bridge syncing Control org/user events) · `affine-runtime:3010`.

## Cross-plane contracts (non-negotiable)
1. No direct DB crossing — Model & Quarry never touch Data Plane Postgres/Qdrant directly (API only).
2. No independent embeddings — only Data Plane embeds/reranks (except isolated eval labs).
3. No agent bypass around Quarry policy — Model *proposes* browser actions; Quarry executes/rejects.
4. ZeroDataRetention propagates across planes.
5. Knowledge assets (graphs, wiki, embeddings, chunks, source logs) = Data Plane-owned.
6. Reasoning (autoresearch, graph synthesis, wiki agents, browser-agent planning) = Model Plane-owned.
7. UX (graph/wiki/IDE/CLI/channel inbox/voice/canvas) = App/Frontend-owned.

## Network / wiring (verified 2026-06-05 — all stacks share the external `inter-plane-bus`)
- Every stack declares `inter-plane-bus` as `external: true`, so cross-stack DNS by container name works. Each plane exposes only its **boundary** on the bus and keeps infra/DBs on its private net:
  - **Control Plane** → app services (`auth-service`/`user-service`/`org-core`/…) + `controlplane-nats` on the bus; Postgres/Redis on `controlplane-net` only.
  - **Model Plane** → `model-gateway` on the bus with a `model-gateway` network **alias**; session/inference/execution/etc. stay on `model-plane-network`.
  - **Data Plane v2** → engines on `dpv2-net` + bus.
  - **velionv2 (Frontend)** → on the bus; resolves `model-gateway:8080` and the control cores by container name.
- velionv2 chat BFF (`/api/chat/stream`) → `model-gateway:8080`; ingestions BFF (`/api/ingestions/*`) → Quarry edge. Local dev `.env` points at `localhost` host ports instead — both the dev and deployed paths resolve.
- Grounded chat now stays owner-correct: velionv2 forwards chat to Model Plane, and `model-gateway` consumes Data Plane retrieval/graph endpoints itself before streaming `grounding` + `citation` SSE back to the UI.
- Footgun to keep watching: a stack must declare `inter-plane-bus` as `external: true` (not create its own project-scoped net). All current stacks do.
