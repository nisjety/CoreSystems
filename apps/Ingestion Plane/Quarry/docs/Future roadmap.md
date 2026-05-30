# Quarry Future Roadmap

**Project:** Quarry  
**Scope:** Backend-only roadmap for getting Quarry from strong MVP to the target state: best-in-class self-hosted internal ingestion platform with enterprise control, durable state, predictable runtime behavior, and LLM-ready outputs.  
**Status:** Strategic roadmap with implementation detail and verified shipped foundations  
**Last Updated:** 2026-04-08

---

## 1. Executive Summary

Quarry should not try to become a clone of Firecrawl or Apify.

Quarry should instead become the best system for these jobs:

1. Self-hosted internal knowledge ingestion.
2. Durable, repeatable crawling and extraction.
3. Change-aware content capture and auditability.
4. Enterprise-grade control of schedules, policies, retention, and outputs.
5. Clean, LLM-ready content and metadata with low caller effort.

### Product Positioning

**What Quarry should beat Firecrawl on:**

- Self-hosted deployment.
- Internal ingestion and compliance-heavy environments.
- Persistent change tracking.
- Operator transparency.
- Control of schedules, retention, and org-scoped data boundaries.

**What Quarry should borrow from Firecrawl:**

- Clean default outputs.
- Fewer knobs for the common case.
- Better runtime feedback.
- Better one-call ergonomics.
- Better output packaging for AI pipelines.

**What Quarry should borrow from Apify:**

- Durable storage primitives.
- Request queue semantics.
- Repeatable schedules.
- Structured job inputs and outputs.
- Clear resource lifecycle and list surfaces.

**What Quarry should not copy:**

- An Actor marketplace as a first-order product investment.
- A dashboard-first strategy.
- A second orchestration platform replacing Temporal.
- GraphQL-first design.

---

## 2. External Inputs Used For This Roadmap

This roadmap is based on Quarry’s current codebase plus current public docs as of April 2026.

### Firecrawl docs used

- Firecrawl positions itself around search, scrape, and interact in one API.
- `scrape` supports clean markdown, HTML, raw HTML, screenshots, links, JSON, branding, images, audio, actions, location settings, caching, batch scraping, and zero data retention.
- `crawl` supports path filters, depth, subdomains, externals, sitemap modes, `scrapeOptions`, real-time watcher updates, page webhooks, and paginated result retrieval.
- Firecrawl explicitly documents cache semantics, result expiration, and nondeterminism at higher concurrency.

### Apify docs used

- Apify storage is built around three persistent primitives:
  - **Dataset** for run results.
  - **Key-value store** for arbitrary blobs, JSON, HTML, files, and checkpoints.
  - **Request queue** for URL processing and crawl progress control.
- Actors are structured, serverless programs with JSON input, output schema, storage, scheduling, metadata, and publishing options.
- Schedules exist as a first-class platform surface.

### Temporal Go docs used

- Temporal Go SDK schedules support:
  - create
  - list
  - describe
  - update
  - pause
  - unpause
  - trigger
  - delete
  - backfill
- Temporal recommends schedules over cron jobs.
- Start delay exists for one-time future execution.

### gqlgen docs used

- gqlgen is schema-first and type-safe.
- It supports code generation, explicit resolvers, query complexity controls, subscriptions, dataloaders, and GraphQL-specific config.
- It fits Quarry’s current Go service architecture better than introducing a new ORM just to support GraphQL.

---

## 3. Non-Negotiable Architecture Decisions

### 3.1 Keep REST canonical

REST remains the system of record for operators, automation, and backend integrations.

GraphQL can be added later as an overlay for:

- composite queries
- subscriptions
- internal dashboards
- multi-resource drill-down views

### 3.2 Keep Temporal as the primary workflow engine

Quarry already has the right durable workflow backbone in `internal/temporal/`.

Do not replace it with River or Asynq for the primary crawl and extraction lifecycle.

Use River or Asynq ideas for ergonomics only:

- queue inspection concepts
- job API shape
- retention semantics
- lightweight auxiliary job classes if needed later

### 3.3 Keep Postgres + Redis split responsibilities

**Postgres should own:**

- durable metadata
- named stores
- checkpoints
- schedules
- baselines
- job and event history
- audit trails
- queryable state

**Redis should own:**

- hot cache
- short-lived distributed coordination
- rate limiting
- ephemeral stream fan-out
- lock leasing where low latency matters

### 3.4 Optimize for enterprise ingestion first

Quarry’s roadmap should prioritize:

- internal docs and help center ingestion
- knowledge base syncs
- pricing and policy page monitoring
- repeatable scheduled crawls
- change-aware refresh loops

Public-web anti-bot escalation matters, but it should not displace the internal-ingestion control plane work.

---

## 4. Current Quarry Reality

### Strengths already present

- Fiber-based REST API in `internal/api/`.
- Crawl engine in `internal/crawl/`.
- Session subsystem in `internal/session/`.
- Async jobs and state in `internal/jobs/` and `internal/asyncjobs/`.
- SSE infrastructure in `internal/sse/`.
- Cache layer in `internal/cache/`.
- Durable workflow surface in `internal/temporal/`.
- Browser session REST resources already exist on top of the browser runtime.
- Browser profile capture and restore already exist through `ProfileStore`, with in-memory storage as the default implementation.
- Change tracking APIs and tracker primitives already exist.
- Change tracking already computes git-style diffs, JSON comparisons, and optional Redis-backed latest-baseline retention.
- Immediate and scheduled execution already exist as routing choices.
- NATS event publishing and worker queue plumbing already exist.
- Usage metering, quota, and team-activity surfaces already exist.
- Transformation and post-run pipeline chains already exist.
- Pagination detection is already implemented as a scrape output format.
- Semantic chunking, SPA auto-escalation, 403/429 escalation, and browser auto-scroll are already implemented in the scrape pipeline.
- Webhook delivery is already wired across crawl, extract, search, research, and agent flows.
- Structured extraction already exists through `/v2/extract` prompt/schema execution.
- PDF, DOCX, and XLSX parsing already exist in the document pipeline, with OCR fallback for PDFs when AI Core is configured.
- Crawl storage already supports memory, Redis, and Postgres backends.
- Search, batch, extraction, security, and transformation packages already separated.

### Gaps already confirmed

- Quick scrape cache-hit behavior does not materially outperform cold scrape.
- Crawl progress is not rich enough for the frontend.
- Resource list endpoints are incomplete and inconsistent across async resource kinds.
- Durable profile storage is incomplete.
- Named stores are not first-class resources and existing stores are not registered behind one seam.
- Job persistence is fragmented across `jobs.Store`, `crawl.Store`, extraction storage, and profile storage.
- Schedule resources are not first-class Quarry API resources, and current `scheduleAt` support is separate from true Temporal schedules.
- Crawl determinism, resumability, and queue inspection need to be made explicit.
- Event transport exists, but durable event history and subject governance do not.
- Per-domain crawl policy, robots-aware throttling, and request-queue style replay semantics are still incomplete as first-class product surfaces.
- GraphQL is absent.
- Benchmarks and scoreboards are not formalized as a release gate.

### Work already started

The first operational maturity slice has already begun:

- `GET /v1/crawl/jobs` now exists.
- The frontend dashboard now consumes that endpoint.

### Verification update from earlier parity notes

Older assessment documents in the repo were useful for direction, but they are no longer fully current.

The following claims from those notes were verified as true in the current codebase:

- Pagination detection exists in `internal/scraper/pagination.go` and is exposed through the `pagination` scrape format.
- Semantic chunking, SPA detection and escalation, 403/429 escalation, and browser auto-scroll are implemented in the current scraper and driver stack.
- Browser sessions already capture and restore cookies and local storage through `internal/session/manager.go` and `internal/session/profiles.go`.

The following older claims are now stale and should be treated as already shipped foundations, not roadmap gaps:

- Webhooks are not merely typed; they are validated and delivered across multiple async surfaces, with verified test coverage for crawl, extract, and search flows.
- Structured extraction is not missing; `/v2/extract` already supports prompt- and schema-driven AI extraction.
- PDF extraction is not missing; document parsing already exists for PDF, DOCX, and XLSX, with OCR fallback for PDFs.
- Change tracking is not just scaffolding; it already supports latest-baseline persistence, git-style diff output, JSON comparison, and optional Redis-backed retention.

The roadmap should therefore continue to treat the remaining work as convergence and durability work:

- durable multi-instance `ProfileStore`
- versioned change history and refresh scheduling
- named store registry and persistence convergence
- first-class schedule resources
- durable event history and event governance
- richer per-domain crawl policy and resumability controls

This roadmap assumes that work continues from there.

---

## 5. Target End State

At the end of this roadmap, Quarry should expose:

1. Full REST transparency for every long-running resource.
2. Named durable stores for runs, snapshots, checkpoints, artifacts, and profile state.
3. First-class schedules backed by Temporal.
4. Durable crawl and extraction history.
5. Deterministic crawl policies and resumable execution.
6. Clean LLM-ready outputs with strong defaults.
7. Org-scoped governance, retention, and auditable state.
8. Optional GraphQL overlay for read-heavy operational workflows.
9. A benchmark suite and success-rate scoreboard that gates claims.

---

## 6. Phase Overview

| Phase | Name | Primary Outcome | Depends On |
|------|------|-----------------|------------|
| 0 | Product boundary and success criteria | Clear target state and release gates | None |
| 1 | Seam inventory and REST normalization | Normalize existing resources before adding new ones | 0 |
| 2 | Store registry and persistence convergence | Extend existing stores behind unified seams | 1 |
| 3 | Queue, checkpoint, and resumability | Durable repeatable crawl execution | 2 |
| 4 | Schedule convergence and control plane | Unify `scheduleAt`, Temporal workflows, and future schedule resources | 1, 2 |
| 5 | Profile durability and browser state | Replace only the in-memory profile seam | 2 |
| 6 | Event contract, governance, and operator visibility | Unify SSE, WebSocket, NATS, and durable event history | 1, 2, 3 |
| 7 | Output normalization and transformer profiles | Extend existing transform and pipeline chains into product defaults | 1, 2 |
| 8 | Determinism and runtime policies | Predictable repeatable crawls | 2, 3, 6 |
| 9 | Change tracking and refresh loops | Promote the existing tracker into a durable versioned subsystem | 2, 4, 8 |
| 10 | Presets and packaged workflows | Reusable vertical recipes | 7, 8, 9 |
| 11 | GraphQL overlay | Composite reads and subscriptions on top of canonical REST/event seams | 1, 6 |
| 12 | Benchmarking, release governance, and rollout | Measurable competitiveness | 6, 7, 8, 9 |

---

## 7. Phase 0 — Product Boundary And Success Criteria

### Objective

Lock the product boundary so implementation work stays focused.

### Deliverables

1. Written target statement for Quarry.
2. Competitive rules of engagement.
3. Release scorecard definition.
4. Architectural rules that prevent platform drift.

### Steps

#### 0.1 Define the official Quarry promise

Quarry promise:

> “Quarry is the enterprise-grade, self-hosted ingestion backend for structured crawling, extraction, change tracking, and repeatable AI-ready content capture.”

#### 0.2 Define what Quarry is not

Quarry is not:

- an open marketplace of third-party actors
- a hosted-only black-box crawler
- a dashboard-led product initiative
- a GraphQL-first platform

#### 0.3 Define release gates

No phase is considered complete unless it includes:

- API contract definition
- storage definition
- tests
- metrics
- operational runbook impact
- rollback strategy

#### 0.4 Define roadmap metrics

Top-level product metrics:

- crawl success rate
- extraction completeness
- change detection precision
- warm-cache latency reduction
- retry rate
- block rate
- operator recovery time
- schedule failure recovery rate

### Internal packages touched

- `docs/`
- `internal/api/`
- `internal/jobs/`
- `internal/temporal/`
- `internal/crawl/`

---

## 8. Phase 1 — Resource Model And REST Normalization

### Objective

Start from the resources Quarry already exposes and normalize them into one discoverable, listable, and governable control plane.

### Firecrawl and Apify insight applied

- Firecrawl is clean because the main surfaces are obvious.
- Apify is usable because every durable concept is visible and operable.

Quarry needs both traits.

### Existing surfaces to normalize rather than replace

- crawl
- search
- extract
- research
- agent
- batch scrape
- browser sessions
- job event streams
- team governance and usage surfaces

### Target resources

- `/v1/crawl/jobs`
- `/v1/search/jobs`
- `/v1/extract/jobs`
- `/v1/research/jobs`
- `/v1/agent/jobs`
- `/v1/batch/scrape/jobs`
- `/v1/sources`
- `/v1/snapshots`
- `/v1/stores`
- `/v1/request-queues`
- `/v1/schedules`
- `/v1/browser/sessions`
- `/v1/browser/profiles`
- `/v1/artifacts`
- `/v1/benchmarks`
- `/v1/team/credit-usage`
- `/v1/team/token-usage`
- `/v1/team/concurrency`
- `/v1/team/queue-status`
- `/v1/team/activity`

### Required contract rules

Each long-running resource must support, where applicable:

- create
- get
- list
- update
- delete
- cancel
- stream
- history

### Steps

#### 1.0 Build a compatibility map first

Before adding new handlers, document which resources already support:

- create
- get
- list
- cancel
- stream
- history
- schedule
- quota/metering

This phase should extend existing routes before introducing replacement paths.

#### 1.1 Standardize resource envelopes

Add consistent JSON shapes for:

- list responses
- status responses
- errors
- lifecycle actions
- pagination metadata
- next links or cursors

#### 1.2 Add common filter model

Support:

- `orgId`
- `sourceId`
- `kind`
- `status`
- `createdBefore`
- `createdAfter`
- `updatedBefore`
- `updatedAfter`
- `limit`
- `cursor` or `offset`
- `sort`

#### 1.3 Normalize handler layout in `internal/api/`

Add or extend handler functions such as:

- `v1CrawlJobs`
- `v1SearchJobs`
- `v1ExtractJobs`
- `v1ResearchJobs`
- `v1AgentJobs`
- `v1SchedulesList`
- `v1SourcesList`
- `v1SnapshotsList`
- `v1BrowserProfilesList`

#### 1.4 Extend `jobs.Store`

Add functions to `internal/jobs/store.go`:

- `ListByKind(ctx, kind, filter)`
- `ListPage(ctx, filter)`
- `CountByKind(ctx, kind, filter)`
- `ListHistory(ctx, jobID)`

Also add a normalization layer so typed async resources from `internal/asyncjobs/` map cleanly onto the shared REST control plane.

### Dependencies

**Internal:**

- `internal/api/`
- `internal/jobs/`
- `internal/platform/`

**External:**

- no new external dependency required

### Acceptance criteria

- Frontend and operators can discover resources without already knowing IDs.
- Every list endpoint has pagination and filters.
- API contracts are consistent across crawl, extract, search, and batch.

---

## 9. Phase 2 — Store Registry, Durable Named Stores, And Persistence Convergence

### Objective

Extend Quarry’s existing store seams into a unified registry and converge fragmented persistence interfaces before adding more storage concepts.

### Apify insight applied

Apify’s durability advantage comes from separating:

- result storage
- arbitrary object storage
- request queue state

Quarry should implement equivalent resources.

### Current Quarry seams to extend

- `jobs.Store`
- `crawl.Store`
- extraction store in `internal/jobs/`
- `session.ProfileStore`
- result cache and schema cache in `internal/api/`
- tracker storage in `internal/tracker/`
- pipeline and artifact persistence seams

The first step is not a net-new storage subsystem. The first step is a registry and interface cleanup so these stores can be addressed consistently.

### Quarry storage primitives to create

1. **Dataset store** equivalent: `snapshots` and `results`.
2. **Key-value store** equivalent: `artifacts`, `checkpoints`, `profile blobs`, `raw payloads`.
3. **Request queue** equivalent: crawl frontier and resumable discovery queue.

### Proposed Quarry resource model

#### `Store`

Represents a named durable namespace.

Fields:

- `id`
- `orgId`
- `name`
- `kind` (`results`, `artifacts`, `queue`, `profiles`, `snapshots`)
- `retentionPolicy`
- `createdAt`
- `updatedAt`
- `metadata`

#### `Snapshot`

Represents a durable persisted content result.

Fields:

- `id`
- `storeId`
- `sourceId`
- `runId`
- `url`
- `checksum`
- `version`
- `capturedAt`
- `contentRef`
- `metadataRef`

#### `Artifact`

Represents arbitrary durable output.

Fields:

- `id`
- `storeId`
- `contentType`
- `size`
- `checksum`
- `blobRef`
- `createdAt`

### Storage implementation plan

#### Store registry layer

Add a new registry package to register and retrieve concrete stores by name and capability.

Initial responsibilities:

- register existing stores at startup
- expose typed accessors for handlers and workers
- support future retention and tenant routing policy by store kind

#### Postgres tables to add

- `quarry_stores`
- `quarry_snapshots`
- `quarry_artifacts`
- `quarry_profile_blobs`
- `quarry_queue_checkpoints`

#### Internal packages to extend

- `internal/stores/`
- `internal/jobs/`
- `internal/db/`
- `internal/objectstore/`
- `internal/cache/`

### Functions to add

In `internal/stores/registry.go`:

- `RegisterStore`
- `Store`
- `StoresByCapability`
- `ListRegisteredStores`

In `internal/jobs/postgres_store.go` or adjacent repositories:

- `CreateStore`
- `GetStore`
- `ListStores`
- `UpdateStore`
- `DeleteStore`
- `CreateSnapshot`
- `ListSnapshots`
- `GetLatestSnapshotByURL`
- `CreateArtifact`
- `GetArtifact`

In existing persistence packages, converge common lifecycle methods so job-oriented stores stop diverging by resource type.

### Dependencies

**Internal:**

- `internal/db/`
- `internal/objectstore/`
- `internal/jobs/`

**External:**

- `github.com/jackc/pgx/v5`
- existing object store dependency already used by Quarry

### Acceptance criteria

- Named stores are queryable and org-scoped.
- Snapshots survive restart and multi-instance deployment.
- Artifacts can be retained independently from runs.

---

## 10. Phase 3 — Queue, Checkpoint, And Resumability

### Objective

Make crawl execution resumable, inspectable, and durable.

### Firecrawl and Apify insight applied

- Firecrawl exposes runtime progress, but retains results only for a limited API window.
- Apify exposes a request queue as a first-class durable primitive.

Quarry should keep durable queue state under its own control.

### What Quarry needs

1. Durable frontier storage.
2. Retry-safe checkpointing.
3. Resume from partial runs.
4. Pause and resume long crawls.
5. Durable page-discovery decisions.

### Implementation design

#### Queue state entities

- `request queue`
- `request item`
- `checkpoint`
- `run frontier state`
- `retry ledger`

#### Postgres tables to add

- `quarry_request_queues`
- `quarry_queue_items`
- `quarry_run_checkpoints`
- `quarry_retry_events`

### Internal packages to extend

- `internal/crawl/store.go`
- `internal/crawl/runner.go`
- `internal/temporal/workflows.go`
- `internal/temporal/activities.go`
- `internal/asyncjobs/`

### Functions to add

In `internal/crawl/store.go`:

- `CreateQueue`
- `EnqueueURL`
- `LeaseNextURLs`
- `AckURL`
- `NackURL`
- `ListQueueItems`
- `SaveCheckpoint`
- `LoadCheckpoint`

In `internal/crawl/runner.go`:

- `ResumeRunFromCheckpoint`
- `PersistDiscoveredURL`
- `PersistPageResult`
- `ApplyDeterministicOrdering`

In `internal/temporal/workflows.go`:

- `ResumeCrawlWorkflow`
- `CheckpointCrawlWorkflow`
- `PauseCrawlWorkflow`
- `ReplayFailedPageWorkflow`

### Dependencies

**Internal:**

- `internal/crawl/`
- `internal/temporal/`
- `internal/jobs/`

**External:**

- existing Temporal Go SDK
- Postgres

### Acceptance criteria

- A crawl can be resumed after worker restart.
- Queue state is inspectable through REST.
- Page discovery is durable and auditable.

---

## 11. Phase 4 — Schedule Convergence And Control Plane

### Objective

Unify Quarry’s current delayed-start and Temporal workflow behavior behind one schedule model, then expose first-class schedule resources.

### Temporal insight applied

Temporal Go schedules already provide the lifecycle Quarry needs. Quarry should expose them directly with Quarry semantics instead of hiding them behind internal workflow wiring.

### Current seams to converge

- `scheduleAt` delayed execution in async dispatch
- `ScheduledExecutor` workflow dispatch
- Temporal workflow rehydration and signal support
- future recurring schedules and backfills

The first delivery here is convergence, not replacement.

### Resource model

`/v1/schedules`

Supported lifecycle:

- create
- list
- describe
- update
- pause
- unpause
- trigger
- backfill
- delete

### Required fields

- `id`
- `orgId`
- `targetKind` (`crawl`, `extract`, `search`, `batch`)
- `targetConfig`
- `spec`
- `timezone`
- `overlapPolicy`
- `catchupWindow`
- `pauseOnFailure`
- `state`
- `notes`

### API handlers to add

In `internal/api/`:

- `v1SchedulesCreate`
- `v1SchedulesList`
- `v1SchedulesGet`
- `v1SchedulesUpdate`
- `v1SchedulesDelete`
- `v1SchedulesPause`
- `v1SchedulesUnpause`
- `v1SchedulesTrigger`
- `v1SchedulesBackfill`

### Temporal integration functions

In `internal/temporal/client.go`:

- `CreateSchedule`
- `DescribeSchedule`
- `ListSchedules`
- `UpdateSchedule`
- `PauseSchedule`
- `UnpauseSchedule`
- `TriggerSchedule`
- `BackfillSchedule`
- `DeleteSchedule`

Also add a compatibility step that routes existing `scheduleAt` requests through the same Temporal-backed schedule path or start-delay path so Quarry stops maintaining two independent scheduling behaviors.

### Dependencies

**Internal:**

- `internal/temporal/`
- `internal/api/`
- `internal/platform/`

**External:**

- `go.temporal.io/sdk/client`

### Acceptance criteria

- Schedules are fully controllable through Quarry’s API.
- Operators never need direct Temporal access for routine schedule work.
- One-time delayed starts and recurring schedules both work.

---

## 12. Phase 5 — Profile Durability And Browser State

### Objective

Keep the current browser session REST surface and replace only the in-memory profile seam with durable multi-instance persistence.

### Current seam

- `internal/session/manager.go`
- `internal/session/profiles.go`
- `internal/session/types.go`
- existing `/v1/browser` create, list, get, execute, and delete routes

### Required work

#### 5.1 Replace `InMemoryProfileStore`

Add durable implementations:

- `PostgresProfileStore`
- optional `RedisProfileCache`

#### 5.2 Persist browser state artifacts

Persist:

- cookies
- local storage
- session storage snapshots when possible
- auth session metadata
- last-used viewport
- last-known domain scope

#### 5.3 Add profile resources

REST surfaces:

- `/v1/browser/profiles`

The existing `/v1/browser` session routes should remain canonical for live sessions.

### Functions to add

In `internal/session/profiles.go`:

- `SaveProfile`
- `LoadProfile`
- `DeleteProfile`
- `ListProfilesByOrg`

In `internal/session/manager.go`:

- `PersistSessionState`
- `RestoreSessionState`
- `ListSessionProfiles`

### Dependencies

**Internal:**

- `internal/session/`
- `internal/objectstore/`
- `internal/jobs/`

**External:**

- Postgres
- optional Redis for hot restore path

### Acceptance criteria

- Sessions and profiles survive API restarts.
- Multi-instance Quarry nodes can reuse org-scoped profile state safely.
- Operators can list and delete profiles explicitly.

---

## 13. Phase 6 — Event Contract, Governance, And Operator Visibility

### Objective

Extend Quarry’s existing SSE, WebSocket, NATS, and metering surfaces into one governed event contract with durable event history.

### Firecrawl insight applied

Firecrawl documents page-level updates, watcher output, and webhooks clearly. Quarry needs similar clarity while keeping more durable internal history.

### Current seams to extend

- `internal/sse/StreamManager`
- `/v1/jobs/:id/events`
- `/v1/jobs/:id/ws`
- cross-plane NATS publishers
- team usage, concurrency, queue, and activity endpoints

This phase should not replace the transports. It should standardize payloads, retention, and subject governance across them.

### Event model to support

- `crawl.started`
- `crawl.discovered`
- `crawl.queued`
- `crawl.page.started`
- `crawl.page.completed`
- `crawl.page.failed`
- `crawl.checkpoint.saved`
- `crawl.paused`
- `crawl.resumed`
- `crawl.completed`
- `crawl.failed`
- `extract.started`
- `extract.completed`
- `search.completed`

### Payload requirements

Every event payload should include:

- `runId`
- `orgId`
- `kind`
- `stage`
- `status`
- `completed`
- `total`
- `discovered`
- `queued`
- `retries`
- `blocks`
- `eta`
- `timestamp`

### Internal packages to extend

- `internal/sse/streaming.go`
- `internal/api/jobs_stream.go`
- `internal/temporal/activities.go`
- `internal/jobs/`
- `internal/nats/`
- `internal/platform/`
- `internal/workerqueue/`

### Functions to add

- `PublishStructuredProgress`
- `PublishTerminalEvent`
- `RecordJobHistoryEvent`
- `ListJobEvents`
- `ReplayJobEventWindow`
- `BuildEventEnvelope`
- `ResolveSubjectName`
- `RecordUsageEvent`

### Acceptance criteria

- The frontend no longer simulates crawl phases.
- Job progress is visible after reconnect or restart.
- SSE, webhooks, and future GraphQL subscriptions share the same canonical event model.
- Subject naming, quota events, and usage events follow one documented contract.

---

## 14. Phase 7 — Output Normalization, Transformer Profiles, And Product Defaults

### Objective

Extend Quarry’s existing transform and pipeline chains into named output profiles and Firecrawl-like defaults without hiding control from operators.

### Firecrawl insight applied

Firecrawl succeeds because the common path is easy:

- clean markdown
- metadata always included
- consistent output packaging
- multi-format one-call behavior

### Output targets Quarry must support consistently

- markdown
- html
- raw html
- links
- screenshot
- structured json
- main-content normalized text sections
- extraction metadata
- change metadata

### Required improvements

#### 7.0 Start from the existing chains

Quarry already has:

- content transformers in `internal/transform/`
- post-run pipeline steps in `internal/pipeline/`

This phase should package those seams instead of replacing them.

#### 7.1 Standard output envelope

All scrape and crawl results should normalize around:

- `content`
- `metadata`
- `links`
- `sections`
- `artifacts`
- `llm`
- `diagnostics`

#### 7.2 Strong defaults

Default behaviors for common ingestion should include:

- `onlyMainContent=true` equivalent for docs-focused modes
- normalized metadata completeness
- stable chunk boundaries
- canonical source URL rules
- content fingerprinting

#### 7.3 Cache semantics

Fix the quick path cache bug and document cache contract explicitly:

- `maxAge`
- `minAge`
- `storeInCache`
- change-tracking bypass semantics

### Internal packages to extend

- `internal/cache/manager.go`
- `internal/transform/`
- `internal/pipeline/`
- `internal/scraper/`
- `internal/extractor/`

### Functions to add

- `NormalizeOutputEnvelope`
- `NormalizeMetadata`
- `BuildLLMSections`
- `ComputeContentFingerprint`
- `ReadCacheWithPolicy`
- `WriteCacheWithPolicy`
- `RegisterOutputProfile`
- `ResolveOutputProfile`

### Dependencies

**Internal:**

- `internal/cache/`
- `internal/transform/`
- `internal/scraper/`

**External:**

- existing Redis client

### Acceptance criteria

- Warm-path latency is measurably lower than cold-path latency.
- Common docs and knowledge pages produce clean markdown without caller tuning.
- Output envelopes are stable across modules.

---

## 15. Phase 8 — Determinism And Runtime Policies

### Objective

Make repeatability an explicit product feature.

### Firecrawl insight applied

Firecrawl explicitly documents nondeterminism at concurrency. Quarry should do better for enterprise runs by making determinism controllable.

### Controls to expose

- `maxConcurrency`
- `delay`
- `sitemap`
- `crawlEntireDomain`
- `allowSubdomains`
- `allowExternalLinks`
- `ignoreQueryParameters`
- `orderingMode`
- `retryPolicy`
- `blockPolicy`
- `proxySessionStrategy`
- `robotsMode`

### Policy model

Add a reusable `RunPolicy` object with:

- discovery policy
- fetch policy
- retry policy
- block policy
- extraction policy
- checkpoint policy

### Internal packages to extend

- `internal/crawl/spec.go`
- `internal/crawl/runner.go`
- `internal/driver/`
- `internal/session/`
- `internal/security/`

### Functions to add

- `NormalizeRunPolicy`
- `ApplyOrderingPolicy`
- `ApplyRetryPolicy`
- `ApplyBlockPolicy`
- `SelectProxySession`
- `RecordDeterminismInputs`

### Acceptance criteria

- The same run config produces stable results under controlled conditions.
- Operators can dial between speed and determinism.
- Deterministic mode is available for scheduled internal knowledge syncs.

---

## 16. Phase 9 — Change Tracking And Refresh Loops

### Objective

Promote Quarry’s existing change tracker into a first-class durable versioned subsystem.

### Quarry advantage to preserve

This is one of the areas where Quarry should clearly beat Firecrawl.

### Current seam to extend

Quarry already has:

- `/v1/change/check`
- `/v1/change/latest`
- `Track`
- `Compare`
- `GetLatest`

The gap is durable history, versioning, and schedule-driven refresh, not the absence of change tracking.

### Required capabilities

- baselines stored by source and URL
- versioned snapshots
- semantic diff metadata
- schedule-driven refresh
- webhook or event emission on change
- retention rules per store

### Resource surfaces

- `/v1/change/check`
- `/v1/change/latest`
- `/v1/change/history`
- `/v1/snapshots`
- `/v1/sources/:id/refresh`

### Internal packages to extend

- `internal/tracker/`
- `internal/crawl/`
- `internal/jobs/`
- `internal/temporal/`

### Functions to add

- `SaveBaseline`
- `LoadBaseline`
- `CompareSnapshot`
- `CreateDiffRecord`
- `ScheduleRefreshRun`
- `EmitChangeDetectedEvent`
- `ListChangeHistory`
- `PromoteTrackedResultToSnapshot`

### Acceptance criteria

- Quarry can show previous and current versions for a tracked page.
- Scheduled refresh loops can drive re-ingestion automatically.
- Changes can be filtered by source and severity.

---

## 17. Phase 10 — Presets And Packaged Workflows

### Objective

Replace “figure out the right flags” with opinionated presets for the use cases Quarry cares about.

### Why this matters

Firecrawl wins many first-use experiences because the default call is enough. Quarry needs the same outcome for its target workloads.

### Presets to ship first

1. `docs-site`
2. `help-center`
3. `pricing-monitor`
4. `knowledge-base-sync`
5. `ecommerce-catalog`
6. `policy-and-legal-tracker`

### Preset definition contents

- crawl policy
- extraction formats
- metadata normalization rules
- chunking strategy
- anti-bot strategy
- change tracking defaults
- retry policy
- retention defaults

### Internal packages to extend

- `internal/modules/`
- `internal/crawl/spec.go`
- `docs/PRESETS.md`

### Functions to add

- `ResolvePreset`
- `MergePresetWithOverrides`
- `ValidatePresetCompatibility`

### Acceptance criteria

- Common ingestion tasks require fewer manual options.
- Presets are documented, versioned, and testable.

---

## 18. Phase 11 — GraphQL Overlay

### Objective

Add GraphQL for composite reads and subscriptions, while keeping REST canonical.

### gqlgen decision

Use `github.com/99designs/gqlgen`.

Reasons:

- schema-first
- type-safe
- code generation
- explicit resolvers
- complexity controls
- subscriptions support

### Scope order

#### 11.1 Read-only query layer first

Expose:

- jobs
- schedules
- stores
- snapshots
- browser sessions
- artifacts
- benchmark results
- health summaries

#### 11.2 Subscriptions second

Expose:

- job progress
- crawl page events
- change-detected events
- schedule status events

#### 11.3 Mutations last

Only after REST parity:

- pause schedule
- trigger backfill
- delete profile
- replay run

### Files and packages to add

- `internal/graphql/`
- `internal/graphql/schema.graphqls`
- `internal/graphql/resolvers/`
- `gqlgen.yml`

### Functions to add

- `NewGraphQLServer`
- `QueryJobs`
- `QuerySchedules`
- `SubscribeJobEvents`
- `MutateSchedulePause`

### Dependencies

**External:**

- `github.com/99designs/gqlgen`

### Acceptance criteria

- GraphQL read models work without breaking REST.
- Complexity limits and auth boundaries are enforced.
- Subscriptions reuse Quarry’s canonical event model.

---

## 19. Phase 12 — Benchmarking, Governance, And Rollout

### Objective

Make quality measurable and tie roadmap completion to scorecards instead of impressions.

### Benchmark corpus

The benchmark suite should cover:

- static documentation sites
- docs with heavy nav trees
- SPA-heavy public pages
- help center sites
- pricing pages
- ecommerce product and catalog pages
- authenticated-like flows where legally testable
- block-prone domains

### Metrics to record

- cold latency
- warm latency
- cache hit rate
- completed pages
- failed pages
- retries
- block events
- output cleanliness
- chunk stability
- diff precision
- schedule reliability

### Internal packages to add or extend

- `scripts/benchmarks/`
- `internal/jobs/`
- `internal/tracker/`
- `docs/`

### Functions to add

- `RunBenchmarkSuite`
- `ComputeBenchmarkScore`
- `PublishInternalScorecard`
- `CompareReleaseBenchmarks`

### Acceptance criteria

- Every release candidate runs benchmark comparisons.
- Quarry can show where it beats Firecrawl and where it still trails.
- The roadmap can be closed only when scorecards validate the claims.

---

## 20. Dependency Matrix

### Existing dependencies Quarry should continue to use

- Fiber for REST API.
- pgx and Postgres for durable metadata.
- Redis for cache and coordination.
- Temporal for durable workflows and schedules.
- Existing object store abstraction for durable artifact blobs.

### New dependency to add

- `github.com/99designs/gqlgen` for the GraphQL overlay.

### Dependencies Quarry should not adopt as core replacements

- River as main workflow engine.
- Asynq as main crawl orchestration engine.
- Ent as a migration path only for GraphQL.

These may remain reference implementations or be introduced only for narrowly scoped auxiliary jobs if a later use case proves Temporal is the wrong fit.

---

## 21. Execution Order By Horizon

### Horizon A — Foundation

1. Finish REST normalization.
2. Build the seam compatibility map.
3. Add missing list surfaces.
4. Add durable profile storage.
5. Add durable event history and canonical event envelopes.
6. Fix quick scrape cache path.

### Horizon B — Enterprise Maturity

1. Add the store registry and converge persistence interfaces.
2. Add named stores.
3. Add request queue and checkpoint persistence.
4. Converge `scheduleAt` and Temporal scheduling.
5. Add deterministic run policies.
6. Promote the existing tracker into durable versioned change history.

### Horizon C — Product Leverage

1. Ship output profiles and presets.
2. Add GraphQL queries and subscriptions.
3. Publish scorecards.
4. Add enterprise governance and release runbooks.

---

## 22. Immediate Next Implementation Slices

The next slices should be executed in this order:

1. Build the resource compatibility map and fill list/history gaps for search, extract, research, agent, and batch resources.
2. Add `jobs.Store` filtering and counting primitives instead of ad hoc in-memory iteration.
3. Introduce a store registry and converge job-oriented persistence seams.
4. Add durable `ProfileStore` implementation in `internal/session/profiles.go` while keeping the current `/v1/browser` session API.
5. Add canonical event envelopes plus durable event history for SSE, WebSocket, and NATS-driven lifecycle updates.
6. Converge `scheduleAt` delayed execution with the Temporal-backed scheduling path.
7. Investigate and fix quick scrape cache-hit behavior in `internal/cache/manager.go` and related scrape path wiring.

---

## 23. Definition Of Done For The Roadmap

This roadmap is complete only when Quarry can demonstrate all of the following:

1. Every operationally relevant resource is discoverable and controllable through REST.
2. Named durable stores exist and survive restart and multi-instance operation.
3. Scheduled crawls and extracts are fully controllable via Quarry APIs.
4. Browser profile state is durable and org-scoped.
5. Runtime progress is structured, durable, and reusable across SSE, webhooks, and GraphQL subscriptions.
6. Quick scrape cache behavior materially reduces warm-path latency.
7. Deterministic mode and repeatable refresh loops exist for enterprise ingestion.
8. Presets reduce common setup complexity.
9. GraphQL read and subscription surfaces exist without replacing REST.
10. Benchmarks prove Quarry’s target leadership in self-hosted internal ingestion, change tracking, and enterprise control.

---

## 24. Final Product Rule

When roadmap tradeoffs appear, choose the option that improves:

- durability
- transparency
- repeatability
- operator control
- LLM-ready output quality

Choose against the option that mainly improves:

- novelty
- marketplace breadth
- UI polish before control plane maturity
- additional orchestration layers without clear need

That is how Quarry reaches the desired goal without losing focus.