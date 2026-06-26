# Velion — Phase Next Roadmap (Road to Full Functionality)

> **Status:** Drafted 2026-06-26 from a fresh five-cluster code audit (agentic/model-plane, trust/compliance/GDPR, customer-surfaces, knowledge/search/ingestion, platform/multitenancy/ops), reconciled against `VELION.md` and the shipped `docs/PHASE_0..4` + `PHASE_OWNERSHIP` plans.
> **Continues the existing numbering.** Shipped: Phase 0 (survival), Phase 1 (honest core loop), Phase 2 (wedge spine + leads), Phase 3 (residency-honest wedge), Phase 4 (data-driven UI), Ownership (fine-grained authz substrate). This document defines **Phase 5–9**.
> **The discipline that survived every prior phase stays load-bearing here:** no control is represented as in place without engineering confirmation, and no privacy/security/trust claim ships without its backing gate in the *same* release.

---

## 1. Definition of "full functionality"

Velion is at "full functionality" — shippable, production-grade, and trust-claimable — when **all** of the following hold:

- **The agent-runner is *credible in production*, not just operational.** The governed multi-step ReAct loop already runs end-to-end; full functionality additionally means every run carries **real cost telemetry** (cost-core ledger fed per inference) and **continuous quality/eval signal** (no permanently-null `costUsd`/`confidence` columns in the Agent Run Console). This is the explicit enterprise hook in `VELION.md`'s own competitive strategy.
- **Multi-tenant production is safe and observable.** Org isolation is enforced not only at the app/gateway layer but with a DB backstop (RLS active), schema evolution is versioned (a real `schema_migrations` ledger on every Control-Plane core, migrations applied on deploy), the gateway caches session validation and rate-limits fleet-wide, and there is per-tenant request/error/latency visibility on the identity/billing/session/audit hot paths and at the BFF edge.
- **Procurement-grade trust is *earned*, not just *built*.** The controls exist and are honestly gated today; full functionality means a public Trust Center backed by **at least one third-party attestation in flight** (SOC 2 II / ISO 27001 / ISO 42001 / EU AI Act / pentest), a **cross-plane erasure fan-out** that actually purges derived copies (so Art. 17 "erased everywhere" is true), a complete DSAR export, enforced CSP, and per-class retention sweeps on content-bearing stores.
- **There is at least one self-serve revenue surface that works out-of-the-box.** Either the Brreg lead-builder is sellable (a plan/add-on grants the `leads` entitlement instead of 402-ing every org) **or** an embeddable end-customer widget exists — ideally both. Today neither monetizes without manual operator intervention.
- **The product is honestly *claimable*.** Connector breadth matches the marketing copy (today only SharePoint actually ingests content), per-user "private-until-shared" passes the four-path honesty test before any "Private" badge renders, and ZDR is provably bound on every routable vendor path (not just the Azure-EU default).

---

## 2. Where we are

`VELION.md` self-rates **~72% production-real (backends ~85%)**, and this audit confirms that framing with two corrections in Velion's favour and several hard blockers against it. **In Velion's favour:** the multi-step governed agent loop is genuinely live end-to-end (the "MVP no-tool dispatch" label in `VELION.md`/`grpc.rs` is *stale* — `execution-core` runs a real capped ReAct loop), and two roadmap items already shipped (the **public Trust Center** on `velion-web`, and the **Data-Plane erasure subscriber**). **Against it:** the credibility layer the GTM leans on is not yet wired — **cost is hardcoded `null` on every run and there is no eval in the serving path** — and the trust *claims* are not yet *earned* (zero certifications, no real cross-plane derived-copy purge). The remaining work is heavily **wiring and operational hardening of already-built backends**, plus three genuinely net-new builds (Channel Plane / embeddable widget, connector content-sync workers, evaluation service).

---

## 3. The blocking gaps

Items where `blocksFullFunctionality = true`, across all five clusters:

| # | Gap | Cluster | Status | Effort |
|---|---|---|---|---|
| B1 | **Gateway session-validation cache (per-cookie)** — `validate_session_cookie` hits auth-core `/get-session` on *every* request; 429 fix only raised the limit | platform-multitenancy-ops | missing | **S** |
| B2 | **Leads billing entitlement seeding** — gateway fail-closed-gates `build_list`/CSV export on `leads`, but no plan grants it → every org 402s | knowledge-search-ingestion / platform | missing | **S** / **M** |
| B3 | **dpv2 migration application on deploy** — versioned migrator runs only via manual `make migrate-up`; `make up` skips it (root cause of missing `operating_maps`) | platform-multitenancy-ops | partial | **M** |
| B4 | **Control-Plane migration versioning** — org-core/billing-core re-apply every `*.up.sql` on each boot, no `schema_migrations` ledger; blocks RLS enablement | platform-multitenancy-ops | partial | **M** |
| B5 | **Per-run cost accounting (cost-core ledger feed)** — nothing writes to the ledger; `cost_usd` is `None` at both emission sites; budget posture computed against an unfed ledger | agentic-modelplane | partial | **L** |
| B6 | **Continuous evaluation / quality & drift monitoring** — no eval in the serving path; `confidence` SSE hardcoded `None`; eval-lab is offline-only | agentic-modelplane | missing | **XL** |
| B7 | **Cross-plane erasure fan-out** — only DP subscriber exists and it merely *re-owns* rows; MP/App/Ingestion have none → Art. 17 "erased everywhere" not achievable | trust-compliance-gdpr | partial | **L** |
| B8 | **Customer-facing DSAR completeness** — self-serve export/erase exists but scope is Control-Plane only; no public intake for non-account subjects; no step-up re-auth on erase | trust-compliance-gdpr | partial | **M** |
| B9 | **Per-user private-until-shared as a *claimable* guarantee** — substrate live but everything defaults to `org`, graph/ExecuteStep carry no `user_id`, Qdrant/Quickwit pre-filters inert; four-path test not green | trust-compliance-gdpr | partial | **L** |
| B10 | **Third-party certifications** (SOC 2 II / ISO 27001 / ISO 42001 / EU AI Act / pentest) — none held; headline procurement blocker | trust-compliance-gdpr | missing | **XL** |
| B11 | **Connector content ingestion breadth** — only SharePoint/finspo ingests content; Slack/Gmail/Notion/Google connect+discover but feed zero documents; `CreateDocument` has no callers | knowledge-search-ingestion | partial | **XL** |
| B12 | **Insights data accrual** — gateway/insight-core honest-empty but almost nothing *produces* scorecards; not a working feature until upstream surfaces emit metrics | knowledge-search-ingestion | partial | **L** |
| B13 | **Observability / metrics across planes** — no gateway `/metrics`, no OTEL tracing; Control/Application compose stacks have zero observability; only org-core exposes `/metrics` | platform-multitenancy-ops | partial | **L** |
| B14 | **Embeddable end-customer chat widget (`/embed`)** — no widget bundle, no `/embed` route, no public funnel; ChatbotStudio is preview-only | customer-surfaces | missing | **XL** |
| B15 | **Channel Plane runtime** (adapter-core / widget-core / public visitor conversation runtime) — docs-only, zero services | customer-surfaces | docs-only | **XL** |
| B16 | **Public / anonymous visitor gateway ingress** — every customer-surface domain is behind `require_session`; an anonymous visitor cannot talk to the agent | customer-surfaces | missing | **L** |
| B17 | **`agents.deploy_channel` runtime executor** — typed descriptor with no backend; owner plane (Channel) has no runtime | customer-surfaces | partial | **L** |
| B18 | **Inbound customer conversation ingress** — only `/internal/ingest/email` (internal-key, email-only) and it has *no in-repo caller*; no web-chat channel | customer-surfaces | partial | **L** |

> **Not blocking but on the path** (sequenced into the phases below for leverage/evidence): CSP enforcement (S), RLS activation (L), retention sweeps for non-audit stores (M), ZDR vendor propagation (M), distributed rate limiting (M), browser-observation as a default chat tool (M), `ExecuteStep` user_id (M), WorkflowBuilder persistence/execution (L), scrape anti-bot commercial proxy tier (M), GDPR policy-metadata propagation (L), Norway-East residency (L).

---

## 4. The phased roadmap

Sequenced by **dependency + leverage**: quick high-leverage ops fixes and the two cheap revenue/credibility unlocks first; then the trust/safety prerequisites that *gate* enterprise GA; then the credibility dashboards that *unlock the production narrative*; then the net-new external-facing surfaces; with breadth (connectors) and depth-claims (private-until-shared) threaded where their dependencies land.

---

### Phase 5 — "Stop the bleeding + unlock the cheap revenue" (ops hardening + leads monetization)

- **Goal:** eliminate the operational landmines that make multi-tenant prod unsafe-or-fragile, and flip the one revenue surface that is technically complete but commercially dormant.
- **Scope (audited items):**
  - **B1** Gateway per-cookie session-validation cache — reuse the already-present `ResultCache` (`cache.rs::store_for_secs`/`lookup_within`, currently used only for the 60s session-*context* at `upstream.rs:56`), short TTL (5–15s) keyed on a cookie hash, with care not to extend a revoked session beyond TTL. *This is the true 429 root cause; the rate-limit raise to 1000 only masked it.*
  - **B2** Leads `leads` entitlement — add `leads` to a plan default (`billing-core/internal/billing/defaults.go` `defaultEntitlementsByPlan`) or define a purchasable add-on SKU + Stripe price; wire `trial.go` effective-plan logic. Gateway/leads-core enforcement already exists.
  - **B3** dpv2 migration-on-deploy — add a migrate job / init-container to the dpv2 compose stack (or a service entrypoint that runs `tools/migrator/main.go up` before serving), idempotent + run-once.
  - **CSP enforce** — flip `nginx.conf:35` from `Content-Security-Policy-Report-Only` to enforce after collecting report-only violations and a load test (one-line rename per the in-file comment).
  - **Distributed rate limiting** *(stretch, pulls forward from ops)* — back `rate_limit.rs` token buckets with the gateway's existing Dragonfly connection (`GATEWAY_CACHE_REDIS_URL`) so throttles hold fleet-wide once the gateway scales horizontally.
- **Why now / sequencing:** these are S/M effort with outsized leverage — B1 fixes a real auth-fanout/scaling problem the 429 patch hid; B2 turns an already-built, Brreg-live-verified lead-builder into self-serve revenue with a one-line entitlement; B3 prevents the exact stale-schema 500s already seen in production (`operating_maps`). None depend on anything else and several are prerequisites for later phases (B1 hardens the auth path before public ingress; B3 establishes the deploy-time migration habit).
- **Dependencies:** none external. Dragonfly already connected.
- **Rough effort:** **S–M** (one focused sprint).

---

### Phase 6 — "Safe, versioned, observable multi-tenant platform" (GA infrastructure prerequisites)

- **Goal:** make multi-tenant production *defensibly safe* and *observable* — the non-negotiable substrate beneath any enterprise sale and beneath the certification evidence in Phase 8.
- **Scope (audited items):**
  - **B4** Control-Plane migration versioning — port dpv2's `schema_migrations` ledger + per-migration tx pattern (`tools/migrator/main.go`) to org-core, billing-core, user-core, session-core, audit-core. Replaces the fragile "re-exec all idempotent DDL on every boot."
  - **RLS activation** *(depends on B4)* — apply `org-core/migrations/008_rls_tenant_isolation`, wire `SET LOCAL app.current_org` (`DB.WithOrgScope`) on every request path, add policy DDL per table, load-test. Defense-in-depth the app layer cannot provide; required SOC 2 evidence.
  - **B13** Observability — add `/metrics` (promhttp / metrics exporter) to the gateway + billing/session/user/audit-core; add Prometheus + Grafana to the Control-Plane and Application-Plane compose stacks; standardize OTEL tracing with **org/tenant labels** at the BFF edge and on the identity/billing/session/audit hot paths.
- **Why now / sequencing:** B4 is the hard prerequisite for RLS (the org-core 008 migration explicitly documents the missing ledger as the reason RLS can't be enabled blind). RLS + per-tenant observability are exactly the controls a SOC 2 / ISO 27001 auditor expects to *see operating* during an observation window — so this phase must land **before** Phase 8 opens any cert window. It builds directly on Phase 5's deploy-time-migration habit.
- **Dependencies:** Phase 5 (B3 establishes the migration-on-deploy pattern this generalizes).
- **Rough effort:** **L** (B4 M, RLS L, observability L — parallelizable across cores).

---

### Phase 7 — "Credible-in-production agent" (cost + eval + telemetry that's no longer null)

- **Goal:** make the agent-runner *credible* for a regulated buyer — every run carries real cost and quality signal — which is the explicit competitive hook ("from pilot to production: observed, evaluated, governed, in-region").
- **Scope (audited items):**
  - **B5** Per-run cost accounting — have `inference-core`/`model-gateway` call cost-core `POST /api/v1/record` per inference (tokens + price from a new pricing table); populate the `Usage` SSE `cost_usd` (currently `None` at `sse.rs:721,1441`); add a gateway cost/usage domain + SPA cost dashboard. *Side-effect:* the already-built Budget/Balance/Genius **cost-aware downgrade** becomes real — today posture is `Unknown` against an unfed ledger so it never fires.
  - **B6** Continuous evaluation — wire an eval/quality signal onto the run stream (either lift `python/eval-lab-py` onto the serving path or a new lightweight scorer), populate the `confidence` SSE field (currently hardcoded `None` at `sse.rs:723`), add a gateway eval domain + an Ops/Quality SPA surface (accuracy/drift). Run-history (session-core `RunService`, live) is the data source.
  - **B12** Insights data accrual — wire metric producers into insight-core across conversation/execution/search/leads surfaces so the honest-empty insights surface starts rendering real values (gateway `insights.rs` already normalizes; only producers are missing).
  - *Telemetry payoff:* the **Agent Run Console** (`AgentRunConsole.tsx`, already renders cost/confidence columns) and the cost-dashboard go from structurally-null to live.
- **Why now / sequencing:** this is the "unlock the production narrative" phase the GTM leans on, and it depends on the data being trustworthy — Phase 6's per-tenant observability and stable schema make the cost/eval/insight tables safe to build on. B5 is a prerequisite for B6's cost-per-quality story and for meaningful budget enforcement. B12 reuses the same event-producer plumbing.
- **Dependencies:** Phase 6 (stable migrations + tenant-labeled telemetry); session-core RunService (live).
- **Rough effort:** **XL** (B6 XL, B5 L, B12 L).

---

### Phase 8 — "Trust, earned" (GDPR fan-out + DSAR + public-trust + certification path)

- **Goal:** turn Velion's *built* controls into *earned, claimable* trust — the unlock for trust-led enterprise deals and the differentiator against a Vanta-backed competitor.
- **Scope (audited items):**
  - **B7** Cross-plane erasure fan-out — add a content-purge subscriber per plane to `velion.gdpr.erasure.requested`: DP byte/chunk/embedding delete (today's subscriber only *re-owns* rows), MP run-history + conversation + inference-cache purge, App Convex/conversation-core purge, Ingestion CAS/crawl purge — each idempotent + audited. Makes Art. 17 "erased everywhere" *true*.
  - **B8** DSAR completeness — extend `BuildDSARExport` (user-core) to assemble MP run-history/conversations + DP documents via the same subscribers (export side); add a public/unauthenticated intake with identity verification for non-account subjects; enforce step-up re-auth on `DELETE /api/v1/privacy/erase`.
  - **GDPR policy-metadata propagation** — attach + carry the full 7-field envelope (purpose, lawful basis, retention, residency, privacy class, third-party processing, deletion scope) on every durable record + cross-plane NATS job, enforced at each plane's ingest boundary. Underpins per-class retention + erasure scoping.
  - **Retention sweeps for non-audit stores** — per-store janitor for MP run-history/conversations and Ingestion CAS/crawl, honoring per-class retention from the privacy taxonomy (audit-core already sweeps; others grow unbounded).
  - **ZDR vendor propagation** — confirm/contract ZDR+region with Anthropic & OpenAI, *or* hard-gate ZDR requests to the Azure-EU path only in the `FallbackChain`/model-router (today a downgrade/fallback could route a ZDR request to an unconfirmed vendor).
  - **B10 / public trust** — stand up the cert path (SOC 2 II / ISO 27001 / ISO 42001 / EU AI Act / pentest); enrich the *already-shipped* public Trust Center (`velion-web/src/app/trust`) with a machine-readable data-flow/architecture map and a wired request-access intake (reuse the DSAR intake plumbing). Lead on **ISO 42001 / EU AI Act** as the sovereign-AI wedge.
- **Why now / sequencing:** the certifications (B10, XL, months-long, external auditors) require the operational controls from Phases 6–8 *operating as evidence* — RLS active, retention sweeps running, CSP enforced, audit complete, erasure provable. So the cert window opens here, after the substrate is real. B7 and B8 share subscribers (build once, use for both erase and export). B9 (next) reuses the per-user_id keying this phase introduces on derived stores.
- **Dependencies:** Phase 6 (RLS, observability as cert evidence); Phase 5 (CSP enforce); the erasure publisher (live) + DP subscriber (live) as the pattern to replicate.
- **Rough effort:** **XL** (B10 XL is largely process/audit, not code; B7 L, B8 M, the rest M–L).

---

### Phase 9 — "Honest depth + the external-facing product" (private-until-shared claim + Channel Plane + widget)

- **Goal:** ship the two highest-ceiling surfaces — the *claimable* per-user privacy guarantee and the external visitor → agent path — that turn Velion from an internal workbench into a deployable end-customer product.
- **Scope (audited items):**
  - **B9** Private-until-shared as a claimable guarantee — thread `user_id` through `ExecuteStep` (proto change, tracked PR-4; regenerate mp-contracts) **and** graph-index gRPC; activate the Qdrant/Quickwit payload pre-filters (today only a retrieval post-filter exists); flip `CONTROL_PLANE_ENFORCEMENT` to strict in the *same* release; pass the four-path honesty test (dense/sparse/wiki/graph all gate on identity) **before** any "Private" badge or "AI only sees your data" copy renders. *Also closes the `ExecuteStep` viewer-scoping gap on the single-step path.*
  - **B15** Channel Plane runtime — net-new build: adapter-core / widget-core / public visitor conversation runtime (owns visitor identity bootstrap + public conversation runtime + channel adapters).
  - **B16** Public/anonymous visitor gateway ingress — a new public, rate-limited, origin-allowlisted ingress with a *visitor* session model distinct from `require_session`.
  - **B14** Embeddable end-customer chat widget (`/embed`) — the deployable JS bundle + origin-allowlisted public funnel.
  - **B17** `agents.deploy_channel` executor — the backend that actually publishes an agent role to a live channel (the typed descriptor exists; the runtime does not).
  - **B18** Inbound customer conversation ingress — wire a live email provider webhook (Gmail/IMAP/SES) into `conversation-ingest-rs` (today `/internal/ingest/email` has no in-repo caller) + a web-chat channel for the widget path.
  - **B11** Connector content ingestion breadth — per-provider content-sync workers (mirror finspo-core's delta/cursor model) for Slack/Gmail/Notion/Google + wire the orphaned `DataPlaneDocumentsClient.CreateDocument`; brings the marketed connector list in line with reality (today only SharePoint ingests content). *Can run in parallel — it depends only on the existing Data-Plane source-objects/documents contracts.*
  - *Adjacent depth* (pull in as capacity allows): **WorkflowBuilder persistence/execution** (workflow store + gateway domain + execution-core dispatch), **browser_agent as a default governed chat tool** (add to `offered_tool_defs` with HITL gating), **scrape commercial proxy tier** (configure Scrapfly/Bright Data keys for hard anti-bot targets), **Norway-East residency** (Tier 2 deployment + Schrems II assessment).
- **Why now / sequencing:** B9 must come after the per-user_id keying introduced in Phase 8's derived-store work and after the four read paths are observable (Phase 6). The external-facing build (B14–B18 + Channel Plane) is the largest net-new effort and rightly last — it depends on a hardened auth path (Phase 5 B1), safe multi-tenancy (Phase 6), and a credible governed agent (Phase 7) before exposing the agent to anonymous traffic. B11 is parallelizable throughout but is grouped here as the "claimable breadth" companion to B9's "claimable depth."
- **Dependencies:** Phase 6 (multi-tenant safety + observability before public exposure); Phase 7 (credible agent before anonymous traffic); Phase 8 (per-user_id keying for B9); Phase 5 B1 (hardened session path).
- **Rough effort:** **XL** (multiple XL items; the single largest phase — sequence B11 + B9 + Channel/widget as parallel tracks).

---

## 5. Quick wins (do first — S effort, high leverage)

1. **Gateway per-cookie session-validation cache (B1)** — `ResultCache` already exists and is unused for this; a short-TTL cookie-hash cache collapses ~10+ auth-core calls per page load to one. *The actual 429 root cause, not the masked one.*
2. **Seed the `leads` entitlement (B2)** — one entry in `defaultEntitlementsByPlan` (or an add-on SKU) turns an already-Brreg-live-verified, metered, audited lead-builder into self-serve revenue instead of a universal 402.
3. **dpv2 migration-on-deploy (B3)** — a migrate init-container on the dpv2 compose stack ends the class of stale-schema 500s already seen with `operating_maps`.
4. **CSP enforce** — collect report-only violations, tighten the policy, load-test, then the one-line `Content-Security-Policy-Report-Only` → `Content-Security-Policy` rename at `nginx.conf:35`.
5. **Correct the stale `execution-core` doc-comments** (`grpc.rs:224-241` "no-tool slice") and the `VELION.md` "MVP no-tool dispatch" line — the real multi-step ReAct loop is live; the docs under-sell a shipped capability.

---

## 6. Explicitly out of scope / cut

Restated verbatim from `VELION.md` — **cut, not deferred** — so the roadmap stays on the wedge (breadth is how a small team dies; the bet is depth):

- **Meeting notes**
- **Podcast / video repurposing**
- **AI phone answering**
- **An AI app / workflow *builder*** (note: this is distinct from finishing the existing **WorkflowBuilder persistence/execution**, which *is* in Phase 9 — the cut item is a net-new general-purpose app builder)
- **A Penpot-class design canvas**
- **SEO / content briefs**

Additionally **not pursued** as product features (per the cross-plane rules and prior plans): independent embeddings/reranking outside isolated eval labs; any agent bypass around Quarry browser-action policy; a Zendesk connector (roadmap-only, not in these phases); the OpenFGA/SpiceDB/Zanzibar authz extraction (revisit only when grant graphs gain nested-team depth — Option C hybrid stands).
