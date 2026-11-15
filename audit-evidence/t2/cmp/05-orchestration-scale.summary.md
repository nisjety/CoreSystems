# T2 / Category 5: Orchestration & scale (comparison summary)

There are 46 reference records from Apify (15, all DOC), Firecrawl (23, SRC), Aside (3, SRC) and Codex in-app browser (5, SRC). They are grouped into 17 clusters. All 46 records are assigned and none are excluded. Verevon is mapped against 20 code-verified baseline records. Detail is in `05-orchestration-scale.clusters.json`.

## Shared baseline (table-stakes)
These are table-stakes in both web-crawl benchmarks (Apify and Firecrawl). The browser products (Codex, Aside) have no crawl engine, so prevalence is judged against the web-only benchmarks.
- Durable request queue / URL frontier with leases (C-001). Verevon has shipped this.
- Crawl budgets and guardrails: page, depth, rate and cost caps (C-006). Verevon: partial.
- Crawl scoping and link filters with denial reasons (C-007). Verevon: stub.
- Sitemap, robots and index-assisted discovery (C-008). Verevon: partial.
- Executable schedules: cron, timezone, jitter, overlap and stale handling (C-010). Verevon: partial.

## Best-of-breed
| Capability | Best source | Why |
|---|---|---|
| Request queue (C-001) | Firecrawl NuQ (SRC), plus Apify API (DOC) | Postgres SKIP LOCKED with lock renewal, stall cap and pg_cron reaper. Apify adds the uniqueKey, forefront and lock-head API. |
| Autoscale and admission (C-003) | Apify AutoscaledPool | CPU, memory and event-loop driven step scaling. Firecrawl adds a cgroup-aware 80% admission gate. |
| Tenant concurrency and fairness (C-004) | Firecrawl | Work over the limit waits in a backlog with an expiry instead of being rejected. Priority drops as a tenant's jobs in flight grow. Returns 429 only when the backlog is full. |
| Scoping and denials (C-007) | Firecrawl Rust `filter_links` | Typed denial map, ReDoS caps, and fix-it messages written for LLM callers. |
| Sitemap kickoff (C-008) | Firecrawl | Parallel discovery from robots Sitemap lines, scoped, root and domain sitemaps, plus seeding from the cache index. |
| Schedules (C-010) | Firecrawl monitors, plus Apify timezone/input | Lease claim, sha256 jitter, skipped_overlap and skipped_no_credits states, stale recovery, plain-English cron. |
| Agent routines (C-011) | Aside | Agent-created cron, heartbeat (wakes the same session) and event routines, all through one tool. |
| Run lifecycle (C-013) | Apify (DOC) | Graceful abort, resurrect with the same storage, recorded origin, persistState and migration events. |
| Session caps and reaping (C-015) | Aside | 200 concurrent runs, 8 sub-agents, 5-tab warning, 15-minute idle tab close. |
| Turn-scoped tabs (C-016) | Codex | Tabs marked deliverable or handoff. One turnEnded cleanup per (session, turn), also run on interrupt or sub-agent stop. |

## Differentiators and unique ideas
- Firecrawl only:
  - Tenant backlog and fair share (C-004).
  - URL permutation dedupe and redirect race handling (C-009).
  - Worker ops: health, metrics and drain (C-014).
  - Broker prefetch and FoundationDB backend (C-002). Verdict: skip.
- Aside only: agent routines (C-011) and session caps (C-015).
- Codex only: turn- and sub-agent-scoped browser routes (C-016).
- Apify and Firecrawl: run chaining and completion-triggered workflows (C-012), run lifecycle (C-013), plan-scaled rate limits (C-005).

## Verevon's biggest gaps (priority tier)
1. **C-007 Crawl scoping (stub, significance 5).** The Go crawl workflow enqueues every link with no scope or robots check (`workflows.go:396-610`). The Rust `CrawlFrontier` with typed denials exists but nothing uses it. The gateway forwards only url, max_pages and ingest. Fix: wire it in (effort M).
2. **C-010 Schedules (partial, significance 5).**
   - Only change_monitor schedules run. Scrape, crawl and batch schedules are silently skipped.
   - Trigger and backfill return 501 because control has a nil Temporal client.
   - The gateway UI still offers these actions.
   - Fix: use Temporal schedules, which already provide overlap handling, jitter, backfill and timezones (effort M).
3. **C-004 Per-tenant concurrency and backlog (absent, significance 4).** There is only a read-only team-concurrency view. Add a per-org cap, a backlog and a priority penalty inside the existing Postgres queue. The Control Plane owns the numbers (effort M).
4. **C-015 Session reaping and caps (partial, significance 4).** There is no idle reaper and lease `ttl_s` is never enforced. Agent runs live in an in-process map. The lease pool holds 2 sessions. The fleet console is a stub. Abandoned runs keep their browser contexts and grants alive.

Recommended next:
- C-006: enforce `max_cost_usd`/`max_runtime_s` on the step path, and clamp crawls to the remaining entitlement.
- C-008: sitemap seeding for crawls.
- C-013: public crawl status, cancel and pause routes on top of the existing Temporal signals.
- C-012: tenant-scoped webhooks.
- C-003: memory-aware browser admission.
- C-009: URL canonicalisation.
- C-016: turn and tab marks keyed by Model Plane run/turn ids.
- C-011: agent routines (Model Plane).
- C-014: `/metrics` on edge.
- C-005: plan-scaled rate limits. Status unknown: check the Control Plane.

## Verevon strengths (keep)
- **C-001.** Postgres SKIP LOCKED frontier, Temporal durability and job idempotency keys. This is at parity with NuQ and avoids Firecrawl's two-store reconciler problem.
- **C-017.** Verevon has both a bulk crawl, map, batch and schedule engine and an interactive agent browser. Codex has no crawl, queue or scheduler (SRC), and neither does Aside. Keep bulk acquisition out of the agent tab loop and expose it to the Model as proposable jobs.
- **Per-run isolation.** Each agent run gets its own CDP context, behind broker grants, with step budgets. For multi-tenant use this is stronger than Codex's local partitions.

## Caveats
- **Apify evidence is documentation only.** All 15 Apify records are DOC. That includes the q5 items for the queue, AutoscaledPool and lifecycle. Confidence is not upgraded.
- **Firecrawl evidence is source code.** The Firecrawl and Aside/Codex records are SRC, taken from the repo or the shipped bundles.
- **C-005 and C-011 have status "unknown".** No baseline record covers them. A light grep found:
  - only a per-org vector budget in Quarry edge;
  - a Model Plane cron sweeper (`capability-core/internal/cron/sweeper.go`).
  A later step should verify both.
- **C-004 "absent" rests on a light grep from this step**, not on a baseline record. C-014 relies on the baseline narrative (`telemetry.rs:6-14`).
- **The gateway schedule UI being ahead of the backend is an inference.** It comes from comparing baseline records v-browser-app-040 and v-quarry-081.
- **Prevalence counts are low** because Codex and Aside are browser products, not crawl platforms. "table-stakes" here means present in both crawl benchmarks.
