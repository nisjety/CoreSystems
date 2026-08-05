# Aqencia: AI Company Intelligence Platform(renamed to verevon)

**Status:** 🚧 Active Development  
**Phase:** Product 1 — Foundation Platform  
**Architecture:** CoreSystem Multi-Plane  
**Latest Update:** Onboarding pipeline fully implemented ✅

---

## Overview

Aqencia is an AI-powered company intelligence workspace that automatically learns how an organization works.

Users simply sign in with their company account, connect their data sources, and Aqencia builds a searchable, collaborative knowledge layer powered by AI.

- **No manual setup**
- **No training required**
- **No prompt engineering**

### Core Philosophy

Traditional AI tools require users to:
- Upload documents manually
- Configure knowledge bases
- Train assistants
- Manage integrations

**Aqencia removes setup entirely:** Sign in → Connect → System learns your company automatically

What Agecia Does

Agecia combines:

automated company discovery

document ingestion

semantic knowledge indexing

collaborative AI chat

real-time team workspace

into a single platform.

Primary Use Case (V1)
Company Intelligence Assistant

After onboarding, teams can ask:

“What did we spend in Q3?”

“Summarize our onboarding policy.”

“What products are underperforming?”

“Create marketing content based on our website.”

“Explain our internal process for refunds.”

Agecia retrieves verified company knowledge and reasons over it using AI.

---

## System Architecture

Aqencia runs on **CoreSystem**, a five-plane distributed architecture:

### 🖥️ Frontend Plane
**Hosts user-facing applications, each with its own application-level event backbone when needed**

| App | Role | Shared App NATS |
|-----|------|------------------|
| **Triodelab Website** | Marketing site, brand surface, public pages | None |
| **Aqencia** | Workspace app: dashboard, settings, integrations, inbox, chat | `verevon-nats` |
| **Quarry App** | Future dedicated Quarry product UI | `quarryapp-nats` (future) |
| **ChatGate App** | Future dedicated ChatGate product UI | `chatgateapp-nats` (future) |

**Rule:** each backend plane keeps its own plane-local NATS broker for internal service communication.  
**Application composition rule:** when multiple planes surface into a frontend application, they also connect to that app's frontend-plane NATS broker.

### 🧭 Control Plane
**Owns identity and organization authority**

| Service | Responsibility |
|---------|-----------------|
| **Auth-Core** | OAuth (Microsoft/Google/GitHub), Better Auth sessions, user authentication |
| **User-Core** | User profiles, memberships, onboarding flags |
| **Org-Core** | Organization metadata, entitlements, onboarding state, member management |
| **Billing-Core** | Plans, quotas, usage tracking, subscription lifecycle |

### 📦 Data Plane
**Transforms company data into AI-ready knowledge**

| Component | Purpose |
|-----------|---------|
| **Documents Service** | Document CRUD, metadata management, versioning |
| **Knowledge Index** | Metadata indexing, full-text search, filtering |
| **Embedding Worker** | Chunking, embedding generation (batch), async processing |
| **Qdrant Vector DB** | Vector storage, filtered search, collection management |
| **Retrieval Service** | RAG gateway: vector search + Cohere reranking |

**Pipeline:** Documents → Chunking → Embeddings → Vector Index → Retrieval API

*AI accesses company data only via Retrieval Service with org isolation.*

### 🧠 Reasoning Plane
**Provides intelligence and structured reasoning**

| Service | Specialization |
|---------|-----------------|
| **AI-Core** | LLM orchestration, prompt templates, response generation |
| **Reasoning-Core** | Chain-of-Thought, Tree-of-Thought, agent orchestration |
| **Memory Systems** | Conversation history, user context, session management |

### 🌐 Ingestion Plane
**Acquires external company knowledge**

| Service | Capability |
|---------|-----------|
| **Quarry** | Enterprise web crawler/scraper, scheduled jobs, multi-format output |
| **Integration-Core** | Aqencia-owned integrations API: connect sessions, connection model, provider registry, webhook intake, and orchestration contracts |
| **Integration-Worker** | Dedicated background execution: sync job processing, webhook retries, state transitions, and event fan-out |
| **Integration-Credentials** | Encrypted credential storage, refresh locking, token lifecycle, and provider access material under Aqencia control |
| **Imports API** | File uploads, URL imports, bulk operations |
| **Temporal Workflows** | Job scheduling, async processing, retry logic |
| **NATS** | Event streaming, state synchronization across planes |

#### Aqencia Integrations Strategy

- **Aqencia owns the integration product surface**: users connect services inside Aqencia, not in a third-party UI
- **Integration-Core is the boundary**: provider auth, token lifecycle, connection state, sync orchestration, and webhook intake live behind Aqencia APIs
- **Integration-Worker executes asynchronously**: sync jobs and webhook retries run outside the API process and communicate over NATS + durable DB state
- **Integration-Credentials owns secrets**: encrypted tokens, refresh locks, and provider credential lifecycle are first-party Aqencia concerns
- **Full fork ownership**: Aqencia absorbs only the runtime code paths and patterns it needs; external connector product surfaces are removed
- **TypeScript first, Go later only if justified**: the runtime fork starts in the source ecosystem of the absorbed code to avoid translation risk; isolated workers can move to Go later if profiling supports it
- **Events stay first-class**: integration state changes and sync lifecycle updates publish to NATS for the rest of CoreSystem
- **Knowledge Layer is the destination**: connected systems enrich documents, retrieval, chat context, inbox workflows, and future agents

### ⚡ Application Plane
**Human collaboration layer**

| Technology | Role |
|------------|------|
| **Convex** | Real-time state: chat, collaborative conversations, presence, live inbox updates |
| **Novu (self-hosted)** | Unified notification delivery: In-App Inbox, Push, Email, SMS, Chat—all multi-channel workflows orchestrated by Novu |
| **Novu Event Bridge** | NATS→Novu adapter; subscribes to `notifications.>` events and triggers Novu workflows |
| **Real-time Sync** | Live ingestion progress, document status updates |

*Note: Convex mirrors backend events and is not a source of truth for persistent data. Novu (self-hosted on `aquatiq-local` network) owns notification delivery orchestration across all channels.*

#### App-Level Event Composition

- **Plane-local events stay in-plane**: Control, Data, Ingestion, Reasoning, and Application each keep their own NATS broker for internal choreography
- **Frontend-plane events compose applications**: Aqencia currently uses `verevon-nats` as the shared app-level event backbone
- **Shared NATS is app-scoped, not global-by-default**: future frontend applications like Quarry App and ChatGate App should get their own shared app broker
- **Marketing stays separate**: the public Triodelab website should remain a plain marketing surface without the application-level NATS broker

#### Notification Event Bus Convention

Any CoreSystem service that needs to deliver a notification publishes to the NATS `notifications.*` namespace:

| NATS Subject | Novu Workflow | When |
|---|---|---|
| `notifications.team.invite.sent` | `team-invite-sent` | Org member invitation created |
| `notifications.user.mentioned` | `user-mentioned` | `@mention` in chat or document |
| `notifications.crawl.completed` | `crawl-completed` | Quarry crawl job finishes |
| `notifications.document.indexed` | `document-indexed` | Vector indexing complete |

Payload schemas: `apps/Application Plane/notification-service/src/events/schemas.ts`
Workflow IDs: `apps/frontend-plane/agencia/src/lib/novu/workflows.ts`

#### Port Reference (Self-Hosted Novu)

| Container | Host Port | Purpose |
|---|---|---|
| `novu-api` | 3200 | REST API / SDK endpoint |
| `novu-ws` | 3202 | WebSocket for In-App Inbox |
| `novu-dashboard` | 4000 | Management UI |
| `novu-mongodb` | 27018 | MongoDB (external debug only) |
| `novu-event-bridge` | 3140 | Health check |

#### First-Time Setup (after `docker-compose up`)

```bash
# 1. Open Novu dashboard and create an account
open http://localhost:4000

# 2. Copy the API key from Settings → API Keys
#    Set in .env and docker-compose overrides:
#      NOVU_SECRET_KEY=<key>
#      NEXT_PUBLIC_NOVU_APP_ID=<application-identifier>

# 3. Create the four workflows in the Novu dashboard (or via API):
#      team-invite-sent, user-mentioned, crawl-completed, document-indexed
#    Each workflow must have at least an In-App step.

# 4. Set NOVU_ENABLED=true in apps/frontend-plane/agencia/.env and restart the frontend.
```

---

## Onboarding Pipeline (V1): Implementation Complete ✅

### Architecture

Aqencia implements a **guided 6-step onboarding** that automatically provisions company resources:

```
1. Profile Setup              (user-core)
   ↓ Store user profile
2. Organization Setup         (org-core)
   ↓ Create or join organization
3. Website Configuration      (quarry)
   ↓ Start discovery crawl
4. Data Source Connect        (Aqencia Integrations)
   ↓ Microsoft Teams, SharePoint, etc.
5. Team Invitation            (org-core member management)
   ↓ Invite teammates, assign roles
6. Complete                   (user-core onboarding flag)
   ↓ AI workspace ready
```

### Step Completion Status

| Step | Feature | Frontend | Backend | Testing |
|------|---------|----------|---------|---------|
| **1** | Profile (name, timezone, job title) | ✅ | ✅ PATCH `/users/me` (user-core) | ✅ E2E verified |
| **2** | Organization (create or join) | ✅ | ✅ POST `/orgs` + invite acceptance | ✅ E2E verified |
| **3** | Website (Quarry crawl) | ✅ | ✅ POST `/api/ingestion/crawl` + SSE stream | ✅ E2E verified |
| **4** | Connect (Aqencia Integrations) | ✅ Dedicated page + catalog | 🟨 Runtime foundation | 🟨 Smoke-tested |
| **5** | Team (invite & roles) | ✅ | ✅ POST `/orgs/:id/members/invite` | ✅ E2E verified |
| **6** | Complete | ✅ | ✅ POST `/users/onboarding/complete` (user-core) | ✅ E2E verified |

### Component Details

#### Step 1: Profile ✅
- **User Input:** First name, last name, job title, timezone
- **Backend Calls:** 
  - `PATCH /api/user/users/me` → user-core
  - Auto-provisioning via OAuth (user created on first sign-in)
- **State Management:** localStorage + user-core persistence
- **Error Handling:** Validation on client + server, graceful degrade if backend unavailable

#### Step 2: Organization ✅
- **Create Path:**
  - Input: org name (auto-derives slug: lowercase, alphanumeric+dash)
  - Optional: Norwegian org number (Brreg lookup for verification)
  - Backend: `POST /api/org/orgs` → org-core
  - Creator becomes owner
  
- **Join Path:**
  - Input: invitation code
  - Backend: `POST /api/auth/organization/accept-invitation` → auth-core (Better Auth)
  - User added as member
  
- **State Management:** localStorage + `POST /internal/orgs/:orgId/onboarding/state` (backend state endpoint)
- **Validation:** Org name required, slug uniqueness check

#### Step 3: Website ✅
- **User Input:** Company website URL (e.g., https://company.com)
- **Quarry Integration:**
  - Request: `POST /api/ingestion/crawl` (next.js proxy)
  - Forwards to: `POST /quarry-api:8090/v1/crawl` (mode: scheduled, maxDepth: 2)
  - Response: `{jobId}` to track crawl status
  
- **Progress Tracking:**
  - SSE stream: `GET /api/ingestion/crawl/:jobId/stream`
  - Real-time phase updates: scanning → crawling → processing
  - User sees live progress, can skip or wait for completion
  
- **State Management:** localStorage (url + jobId) + backend onboarding state with crawl metadata
- **Error Handling:** Network retry, fallback phase simulation, graceful skip option

#### Step 4: Connect (Aqencia Integrations) 🟨
- **Product Boundary:** Aqencia owns the integration UI and workspace-facing APIs. Users never manage integrations in a separate product UI.
- **Runtime Direction:** Integration-Core fully internalizes only the connector backend pieces needed for:
  - Microsoft Graph API (SharePoint, OneDrive)
  - Teams API (presence, channel data)
  - Outlook API (calendar events, emails)
  - Google Workspace (Gmail, Calendar)
  - Slack (messages, channels)
- **Architecture:** OAuth token exchange, incremental scoping, webhook ingestion, sync fan-out, token refresh, and NATS event publishing all sit behind Integration-Core
- **Fork Rule:** Aqencia ports useful upstream runtime patterns and deletes hosted dashboards, connect UIs, billing surfaces, and provider-platform vocabulary
- **Frontend:** Dedicated `/integrations` page acts as Aqencia's own integration catalog and connection surface; integration status and notifications stay inside Aqencia's design system
- **Status:** Aqencia-owned frontend catalog is live; runtime fork and storage migration are in progress

#### Step 5: Team Invitation ✅
- **User Input:** Email list + role assignment (admin/member/viewer)
- **Invitation Flow:**
  - Request: `POST /api/org/orgs/:orgId/members/invite`
  - Backend (org-core):
    - Check if user exists in user-core `GET /api/users/by-email/:email`
    - If found → add to org immediately (status: active)
    - If not found → create pending invite (status: invited, store email in DB)
  - Response: `{invitation_id, status}`
  
- **Member Management:**
  - List: `GET /api/org/orgs/:orgId/members` → `{members: [], count: 0}`
  - Remove: `DELETE /api/org/orgs/:orgId/members/:userId` → `{ok: true}`
  - Update Role: `PUT /api/org/orgs/:orgId/members/:userId/role`
  
- **Auto-Join:** When invited user signs up → matches `invited_email` in DB → auto-joins with correct role
- **State Management:** localStorage + org-core member tables
- **Error Handling:** Invalid email format, duplicate invite, graceful skip if network fails

#### Step 6: Complete ✅
- **Action:** Mark onboarding as complete
- **Backend Calls:**
  - User: `POST /api/user/users/onboarding/complete` → user-core (sets `onboardingComplete: true`)
  - Unlock: AI workspace, full chat interface, document search
  
- **Post-Completion:**
  - Quarry crawl continues in background (documents indexed automatically)
  - AI assistant primed with company knowledge
  - Team workspace ready for collaboration
  
- **State Management:** Clear localStorage, redirect to dashboard
- **Error Handling:** Continue even if user-core call fails (local state sufficient)

---

## Product Features (V1)
## Product Features (V1)

| Feature | Status | Description |
|---------|--------|-------------|
| **Company Discovery** | ✅ | Crawl company website automatically, extract structure and content, build site map |
| **AI Workspace** | ✅ | Organization-wide AI chat, shared conversations, source citations from knowledge base |
| **Collaborative Teams** | ✅ | Invite members, assign roles (admin/member/viewer), shared context |
| **Knowledge Retrieval** | ✅ | Semantic search via Qdrant, metadata filtering, Cohere reranking |
| **Live Progress Tracking** | ✅ | Real-time ingestion status, document indexing progress, error reporting |
| **Multi-Org Isolation** | ✅ | All queries scoped to org_id, zero data leakage across organizations |

---

## Example User Journey

```
User signs in with work email (OAuth)
  ↓
System detects tenant, creates/resolves organization
  ↓
Guided profile setup (name, timezone, job title)
  ↓
Organization configuration (create or join)
  ↓
Website URL entered for Quarry crawl
  ↓
Team members invited with role assignment
  ↓
Onboarding marked complete
  ↓
Background: Documents indexed automatically
  ↓
AI assistant ready for questions
  ↓
Team collaborates in shared workspace
```

---

## Repository Structure

```
CoreSystem/
│
├── control-plane/
│   ├── auth-core/              (Better Auth + OAuth)
│   ├── user-core/              (Profiles, onboarding flags)
│   ├── org-core/               (Organizations, members, invites)
│   └── billing-core/           (Plans, quotas, usage)
│
├── data-plane/
│   ├── documents-service/      (CRUD, metadata)
│   ├── retrieval-service/      (RAG + reranking)
│   ├── embedding-worker/       (Async chunking + vectors)
│   ├── knowledge-index/        (FTS indexing)
│   └── qdrant/                 (Vector storage)
│
├── reasoning-plane/
│   ├── ai-core/                (LLM orchestration)
│   ├── reasoning-core/         (CoT, reasoning agents)
│   └── memory-systems/         (Context & history)
│
├── ingestion-plane/
│   ├── quarry/                 (Web crawler/scraper)
│   ├── integration-core/       (Aqencia-owned integrations engine: auth, tokens, webhooks, syncs, and events)
│   ├── imports-api/            (File/URL ingestion)
│   └── temporal/               (Workflow scheduler)
│
├── application-plane/
│   ├── convex/                 (Real-time sync + chat)
│   └── notification-service/   (NATS→Novu event bridge)
│
└── frontend-plane/
    ├── agencia/                (Current Aqencia app: onboarding, dashboard, settings, chat)
    ├── triodelab-web/          (Future dedicated marketing/public site)
    ├── quarry-app/             (Future dedicated Quarry product app)
    └── chatgate-app/           (Future dedicated ChatGate product app)
```

*Current repository note:* the Aqencia app now lives at `apps/frontend-plane/agencia/`. The Triodelab marketing site is split into `apps/frontend-plane/triodelab-web/`.

---

## Design Principles

### 1. Plane Isolation ✅
- Services never cross planes directly
- All inter-plane communication via REST/event APIs
- Each plane independently scalable

### 2. Event-Driven System ✅
- Real-time state synchronization via NATS
- Event sourcing for audit trails
- Asynchronous processing reduces latency

### 3. AI Safety ✅
- AI only accesses approved knowledge through Retrieval Service
- No direct database access from reasoning plane
- All queries scoped to org_id (tenant isolation)

### 4. Multi-Tenant by Default ✅
- Every operation requires org_id parameter
- Row-level security enforced in backend
- Zero cross-org data leakage

### 5. Replaceable Intelligence ✅
- Models, embeddings, LLMs pluggable without architecture changes
- Currently: OpenAI embeddings + Claude/GPT4 for reasoning
- Can swap to Cohere, Mistral, on-premise models

---

## Future Products Enabled

Because Aqencia runs on CoreSystem, the same backend enables:

- 🔎 **Quarry** — Standalone enterprise web crawler (Firecrawl alternative)
- 💬 **AI Workspace Platform** — Generalized collaborative AI for any data source
- 📈 **Marketing AI Generator** — Dynamic content creation from company knowledge
- 🧑‍💼 **HR Intelligence Assistant** — Employee onboarding, policy assistant, benefits Q&A
- 🛍 **Ecommerce AI Copilot** — Product recommendations, customer support automation
- 📊 **Business Analytics AI** — Predictive insights, anomaly detection, reporting

---

## Development Status

| Component | Status | Notes |
|-----------|--------|-------|
| **Control Plane** | ✅ Stable | All services production-ready |
| **Data Plane** | ✅ Operational | Full RAG pipeline, vector search + reranking |
| **Reasoning Plane** | ✅ Operational | LLM integration, structured reasoning |
| **Ingestion Plane** | ✅ Production Ready | Quarry stable, scheduled crawls working |
| **Application Plane** | ✅ Integrated | Convex real-time sync operational; Novu self-hosted notification stack deployed |
| **Frontend** | ✅ Onboarding Complete | 6-step flow, team management, chat UI |
| **E2E Pipeline** | ✅ Verified | Full test covering all 6 planes |

---

## Vision

**Aqencia becomes the operating system layer between organizations and AI.**

- Every company gets an intelligent assistant trained on their actual data
- No setup, no training, no integration nightmares
- Knowledge layer forms the foundation for AI-powered workflows
- Future: agents autonomously completing workflows based on company knowledge

---

## Current Goal

**Deliver the first production-ready AI company workspace where:**

1. A company signs in and immediately gains an intelligent assistant trained on their organization
2. Team members can collaborate in shared AI conversations with source citations
3. New data sources (websites, documents, integrations) automatically enrich the knowledge base
4. The AI adapts to company processes, terminology, and culture
5. Every action is audited, every fact is traceable, every query respects tenant boundaries
