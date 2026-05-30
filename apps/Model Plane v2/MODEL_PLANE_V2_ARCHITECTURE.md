# ⛔ DEPRECATED — DO NOT USE ⛔

> **This entire directory is dead.** Model Plane v2 was an exploratory Python
> reskin of the Go+Rust Model Plane v1. As of 2026-05-21 it is **decommissioned**
> and will not be revived.
>
> **Canonical home:** `apps/Model Plane/` (Go services + Rust inference-core,
> gRPC throughout, single-source-of-truth `proto/model_plane/v1/*.proto`).
>
> **Why decommissioned:** v2 duplicated capabilities that already exist in v1
> (`structured_output_schema` is already plumbed end-to-end through
> `InferRequest` → `OpenAI`/`Anthropic` providers in
> `rust/services/inference-core/src/provider/`). Maintaining two stacks in two
> languages doubled the surface for no functional gain.
>
> **Migration map (v2 → v1):**
>
> | v2 (Python, dead)                                 | v1 (Go/Rust, canonical)                                              |
> |---------------------------------------------------|----------------------------------------------------------------------|
> | `agent-core/app/adapters/quarry_client.py`        | `go/pkg/quarry/` (Go shared lib)                                     |
> | `agent-core/app/tools/builtins/web_fetch.py`      | `gateway.proto: ModelGateway.Fetch` RPC                              |
> | `agent-core/app/tools/builtins/extract_structured.py` | `gateway.proto: ModelGateway.ExtractStructured` RPC              |
> | `tools/registry.py` + `ALL_BUILTINS`              | `capability-core` (Go) registry                                      |
> | LLM coercion (forced tool-call in Python)         | Native `structured_output_schema` on `InferRequest`/`InvokeRequest` |
>
> Do not add new code here. Do not import from here. Anything still landing in
> v2 after this date is a bug and should be redirected to v1.

---

# Model Plane v2 Architecture (historical, for reference only)

**Last Updated:** 2025-07-08

## Pyramid Placement

The **Model Plane v2** sits at **Layer 4** of the CoreSystem pyramid. It is the
AI reasoning and agent-orchestration layer. It receives identity and org context
from the Control Plane (L1), retrieves documents and embeddings from the Data
Plane (L2), and ingests raw content via the Ingestion Plane (L3). It exposes AI
capabilities upward to the Application Plane (L5) and Frontend Plane (L6).
Authority flows strictly downward — Model Plane v2 **never** writes to Control,
Data, or Ingestion Plane databases.

### Authority Rules

- ✅ **Canonical owner** of reasoning pipelines, agent orchestration, execution
  graphs, capability definitions, and LLM routing.
- ✅ **Canonical owner** of its own databases: `reasoning-v2-postgres`,
  `redis` (DBs 3–6), Temporal task queues, MinIO artifacts.
- ✅ **May read** Data Plane embeddings and documents via gRPC / REST.
- ✅ **May read** Control Plane identity tokens to authorize requests.
- ❌ Does **not** own user identity, org hierarchy, or billing (→ Control Plane).
- ❌ Does **not** own document storage, embeddings, or retrieval indexes (→ Data Plane).
- ❌ Does **not** own raw-content ingestion or import jobs (→ Ingestion Plane).
- ❌ Does **not** own collaborative workspaces or real-time sync (→ Application Plane).

> **Note:** `documents-worker` has been **REMOVED** from Model Plane v2. Its
> responsibilities are now handled by the Data Plane v1 embedding pipeline.

---

## 🏗️ Service Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                      MODEL PLANE v2  (Layer 4)                  │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────┐     │
│  │   ai-core    │  │ agent-core-v2│  │ execution-core-v2 │     │
│  │  :8001/:50051│  │  :8002/:50053│  │      :8003        │     │
│  │  (gateway)   │  │  (orchestr.) │  │   (exec engine)   │     │
│  └──────────────┘  └──────────────┘  └───────────────────┘     │
│                                                                 │
│  ┌──────────────────┐  ┌──────────────┐                        │
│  │ capability-core-v2│  │  llm-worker  │                        │
│  │      :8004       │  │    :8005     │                        │
│  │  (capabilities)  │  │  (LLM exec)  │                        │
│  └──────────────────┘  └──────────────┘                        │
│                                                                 │
│  ┌────────────────────────────────────────────────────────┐     │
│  │               Infrastructure                           │     │
│  │  reasoning-v2-postgres :55433 │ redis :6390            │     │
│  │  nats 2.10-alpine :4227/:8227 │ minio :9000/:9001     │     │
│  │  temporal-postgres                                     │     │
│  └────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────┘
```

---

## 📦 Service Responsibilities

### 1. **ai-core** (Port 8101 → 8001 external, gRPC 50061 → 50051)

| Attribute | Value |
|-----------|-------|
| **Domain** | Public AI gateway — 10-layer reasoning pipeline |
| **Technology** | Python (FastAPI + gRPC) |
| **Database** | Redis DB 6 |
| **Memory** | 512 MB |

**Owns:**
- ✅ AI reasoning pipeline orchestration (10-layer configurable pipeline)
- ✅ Multi-provider LLM routing (OpenAI, Anthropic, Azure OpenAI, Google)
- ✅ Azure Speech, Translator, and Document Intelligence integration
- ✅ Request validation and rate limiting at the AI gateway boundary

**Events Published:** `ai.request.completed`, `ai.pipeline.error`
**Events Subscribed:** —

**Key Environment:**
- `AGENT_CORE_URL` — delegates agent tasks to agent-core-v2
- Multi-provider API keys (OPENAI, ANTHROPIC, GOOGLE, AZURE)

---

### 2. **agent-core-v2** (Port 8102 → 8002 external, gRPC :50053)

| Attribute | Value |
|-----------|-------|
| **Domain** | Agent orchestration and workflow management |
| **Technology** | Python (FastAPI + gRPC) |
| **Database** | reasoning-v2-postgres, Redis |
| **Memory** | 384 MB |

**Owns:**
- ✅ Agent lifecycle management (create, invoke, pause, resume, terminate)
- ✅ Temporal workflow orchestration (`velion` namespace)
- ✅ MinIO artifact storage for agent sessions
- ✅ Dual-NATS connectivity (velion-nats internal + control-plane-nats for billing)

**Events Published:** `usage.{org}.llm` (via control-plane-nats for billing)
**Events Subscribed:** Agent invocation requests via velion-nats

**Key Environment:**
- `DATA_PLANE_*` URLs — reads documents/embeddings from Data Plane
- `NATS_URL` (velion-nats) + `CONTROL_PLANE_NATS_URL` (billing events)
- `TEMPORAL_ADDRESS`, `MINIO_ENDPOINT`

---

### 3. **execution-core-v2** (Port 8103 → 8003 external)

| Attribute | Value |
|-----------|-------|
| **Domain** | Execution engine for agent task graphs |
| **Technology** | Python (FastAPI) |
| **Database** | Redis DB 3 |
| **Memory** | 384 MB |

**Owns:**
- ✅ Task graph execution and step tracking
- ✅ Artifact persistence (MinIO: artifacts + workspace buckets)
- ✅ Execution state management

**Events Published:** `execution.step.completed`, `execution.failed`
**Events Subscribed:** Execution requests via NATS (dual-NATS)

**Key Environment:**
- `NATS_URL`, `MINIO_ENDPOINT`, `MINIO_ACCESS_KEY`, `MINIO_SECRET_KEY`

---

### 4. **capability-core-v2** (Port 8104 → 8004 external)

| Attribute | Value |
|-----------|-------|
| **Domain** | Capability registry and tool management |
| **Technology** | Python (FastAPI) |
| **Database** | Redis DB 4 |
| **Memory** | 384 MB |

**Owns:**
- ✅ Capability definitions and versioning
- ✅ Tool registry for agent consumption
- ✅ Capability discovery and matching

**Events Published:** `capability.registered`, `capability.updated`
**Events Subscribed:** Capability queries via NATS (dual-NATS)

**Key Environment:**
- `NATS_URL`, `INTERNAL_API_KEY`

---

### 5. **llm-worker** (Port 8105 → 8005 external)

| Attribute | Value |
|-----------|-------|
| **Domain** | Stateless LLM execution worker |
| **Technology** | Python (FastAPI) |
| **Database** | Redis DB 5 |
| **Memory** | 512 MB |

**Owns:**
- ✅ Stateless LLM API execution (prompt → completion)
- ✅ Rate limiting enforcement (RPM=60, TPM=100K)
- ✅ Provider-specific request formatting

**Events Published:** `llm.completion.done`
**Events Subscribed:** LLM execution requests via NATS (velion-nats only)

**Key Environment:**
- `NATS_URL` (velion-nats only — no control-plane-nats)
- Multi-provider API keys

---

## 🔧 Infrastructure

| Component | Image / Version | Port(s) | Purpose |
|-----------|----------------|---------|---------|
| reasoning-v2-postgres | postgres:16 | 55433 → 5432 | Persistent store for agent-core-v2 |
| redis | redis:7-alpine | 6390 → 6379 | Shared cache / state (DBs 3–6) |
| nats | nats:2.10-alpine | 4227 → 4222, 8227 → 8222 | JetStream messaging (velion-nats) |
| minio | minio/minio | 9000, 9001 | Object storage for artifacts |
| temporal-postgres | postgres:16 | — | Temporal server backing store |

### Networks

| Network | Purpose |
|---------|---------|
| `reasoning-v2-network` | Internal mesh for all Model Plane v2 services |
| `control-plane-network` | Read-only access to Control Plane NATS for billing events |
| `dataplane-network` | Read-only access to Data Plane retrieval and document APIs |

---

## Does NOT Own

| Capability | Canonical Owner | How Model Plane v2 Accesses |
|---|---|---|
| User identity & sessions | Control Plane (auth-core, session-core) | Validates JWT tokens |
| Org hierarchy & billing | Control Plane (org-core, billing-core) | Publishes `usage.{org}.llm` events |
| Document storage & embeddings | Data Plane (documents-service, embedding-worker) | REST / gRPC calls via DATA_PLANE URLs |
| Retrieval & vector search | Data Plane (retrieval-service, knowledge-index) | REST / gRPC calls |
| Raw content ingestion | Ingestion Plane (Quarry, imports-core) | Does not access directly |
| Collaborative workspaces | Application Plane (convex-backend, affine) | Does not access directly |
| UI rendering | Frontend Plane (velion) | Serves API responses to frontend |

---

## Cross-Plane Contract Rules

1. **Identity is read-only.** Model Plane v2 validates Control Plane JWT tokens
   but never issues, rotates, or stores them.
2. **Billing is event-based.** Usage reporting flows via NATS
   `usage.{org}.llm` events to Control Plane — never by direct DB writes.
3. **Data Plane access is API-only.** All document and embedding access goes
   through Data Plane REST/gRPC endpoints. Model Plane v2 never connects to
   Data Plane PostgreSQL or Qdrant directly.
4. **Artifact ownership is local.** MinIO buckets and Temporal workflows are
   owned exclusively by Model Plane v2 services.
5. **documents-worker is REMOVED.** All embedding and document processing is
   delegated to the Data Plane pipeline. Model Plane v2 does not run its own
   embedding workers.
