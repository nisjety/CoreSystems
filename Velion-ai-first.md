# Velion — AI-First Audit & Roadmap

> **Current correction — 2026-07-13.** The historical verification below is superseded for Model Plane runtime claims. Gateway/inference gRPC are absent in the running stack despite green HTTP health; live cost/session/capability auth, semantic memory, MCP/Visma, ZDR, and rollback gates fail secure-MVP acceptance. The guarded agentic approval path is real and durable through session-core, but the live approval RPC boundary is unauthenticated. Source now passes exact-audience ordinary invoke-chain tests, yet approval/browser/Letta callers and every live gate remain open; source containment is not deployment proof. See [MODEL_PLANE_STATUS.md](apps/Model%20Plane/MODEL_PLANE_STATUS.md).

> **Live verification update 2026-07-10.** Docker health is green (91 running containers, no unhealthy containers) and the authenticated v3 cross-plane Playwright smoke suite passes 6/6. This does **not** mean the AI-first contract is complete: normal chat sends zero tools and zero grounding features; Browse mode can search/fetch live web sources; Plan mode exposes a separate execution catalog. A live organization MCP record named `visma mcp` is enabled but is misconfigured as `stdio` with an HTTPS URL and was not executed. Shipping quotes are mixed live/demo, and Bring’s live delivery-time fields are currently dropped by the adapter.
>
> **Reopened blockers from live tests:** unauthenticated shipping/import/Data routes; MCP stdio/HTTP SSRF and unauthenticated gRPC exposure; Quarry bearer bypass and disabled control HMAC in the running stack; ZDR persistence gaps; direct chat tool-loop approval bypass; and source/image deployment drift. The previous “remediation landed and verified” statement below is now historical and must not be treated as a current release certificate.

> **Update 2026-07-07 — remediation LANDED on main.** The follow-up audits of 2026-07-05
> (integration surfacing; isolation/KB/boundaries) and the five-stream remediation program
> they triggered are merged and verified: Stream 1 Model Plane trust chain (server-pinned
> Auto/Ask posture, durable approval binding replacing the hardcoded marker, operation-aware
> read/write risk gating incl. `browser_agent`, `ExecuteStep` user_id cross-user-leak fix,
> browser planner through the cost-aware intent layer) · Stream 2 Data/Ingestion knowledge
> integrity (GDPR/ZDR metadata survives the ingest wire, SharePoint/Notion real content
> capture, GitHub/Slack sync worker, embedding + graph extraction routed through Model Plane —
> live-smoke-tested e2e) · Stream 3a Inbox (WhatsApp/Messenger send-before-persist — the
> silent reply-drop false-success bug is dead; tickets.* dispatchers wired; `inbox.draft_reply`
> registry orphan removed) · Stream 3b Social (metrics/catalog gateway routes + UI, REAL
> ApprovalState check before publish/schedule, leads CSV formula-injection guard) · Stream 3c
> Workspace/Agents (AgentRunConsole shows real tool args, backend-less `security.*`/
> `agents.deploy_channel` registry entries dropped, eval-lab-py "IMPLEMENTED" claim corrected).
> Additionally landed the same day: org-core RLS verified ACTIVE on the live DB (functional
> probe), full Instagram DM channel (object-aware webhook branching + `instagram.messages.send`),
> conversation-core ZDR tripwire, eval-harness MVP scope (`docs/EVAL_HARNESS_MVP.md`).
> Verification: gateway 160/160 (clean-room at merged HEAD), model-gateway 285/285,
> execution-core 117/117, quarry-runtime 392/392, all Go suites green. The 2026-06-19 IDOR
> and fabricated-trust-UI findings below were already fixed earlier and remain closed
> (regression-tested). Still open: shared INTERNAL_API_KEY split (proposal:
> `apps/Model Plane/docs/internal-api-key-split-proposal.md`), Gmail/Outlook inbound +
> Snapchat publisher + Shopify context card + eval harness (approved build wave), Discord
> bot (phase after). Sections below are retained as the 2026-06-19 baseline snapshot.

> **Date:** 2026-06-19 · **Scope:** Velion v3 (main app) + every CoreSystem plane and core.
> **Method:** Structural audit via codegraph (3,076 files / 56k symbols indexed) + context-mode, a 16-agent parallel audit workflow (2.48M tokens, 325 tool calls), an 18-area competitive benchmark mapping, and a 5-persona advisory **council** with chair synthesis. Findings captured to MemPalace (KG + diary + drawer), context-mode index, and Logseq.
> **Product thesis:** *An intelligence-to-action workbench for the Norwegian market — extract data, monitor change, generate briefs, route through inbox/tickets, and publish, all after human approval.*

---

## 0. TL;DR — Start Here

**Velion is real, not a demo.** The SolidJS SPA reaches every backend plane *only* through the Rust BFF (`velion-gateway-rs`); there are **zero mock modules** under `src`; ~40 typed gateway clients back the feature slices. **Reality score: ~72% production-real (backend planes ~85%).** The engine is built — the remaining high-leverage work is mostly **wiring already-built backends through the gateway**, not net-new construction.

**But the differentiator is also the attack surface.** A trust/residency-positioned product currently ships with (a) a **critical cross-tenant data-leak (IDOR)** and (b) **fabricated "Configured/Verified" trust UI**. Both must be fixed *before* any second tenant or pilot.

### The reconciled sequence (council verdict)

| Phase | Window | Do this | Why |
|---|---|---|---|
| **🔴 Survival sprint** | Days 1–21 | **1.** Fix the `x-velion-org-id` IDOR across all 6 trusted-key gateway domains + delete `org_id_from_headers`. **2.** Delete every fabricated trust/social/run-id state → honest empty/error states. **3.** Minimal CI PR-gate (cross-tenant regression test + no-fabricated-state lint) + reconcile DB_PASSWORD drift. | Blocks the *existence* of a second tenant and the trust brand. Ships nothing user-facing until done. The safe `authorized_org_id` pattern already exists in-repo → days, not weeks. |
| **🟡 Honest core loop** | Days 22–60 | **4.** Wire 3 already-built backends: `insight-core` (Market Intelligence), `conversation-core-go` AI-action HITL approve/reject, Quarry `/v1/change`+`/v1/schedules` (monitoring). **5.** Wire `user-core` GDPR erasure/DSAR + the "Used by AI?" audit trail with real UI. | Pure last-mile gateway glue over real backends. Makes the *monitor → brief → approve* spine LIVE and the Trust Center **provable**. |
| **🟢 Monetize the wedge** | Days 61–90 | **6.** Build **Brreg-seeded lead/list-building** (Enhetsregisteret by industry/region/size → enrich → export) on the now-safe platform. Put one end-to-end *"Brreg resolution → monitor → brief → approve → act in-region"* story in front of a Norwegian design partner. | The single net-new bet worth funding — monetizes the uncopyable Norwegian wedge. Growth bet, not survival bet. |

**Headline positioning (council):** *"The Norwegian AI workbench that turns **Brreg-grounded intelligence into approved action** — every step grounded and auditable, every byte kept in-region, and nothing published, sent, or stored without a human yes."* Lead commercially with the **Norwegian wedge**; demo the **observable agent-runner** second as the moat/upsell.

**Cut, don't defer:** meeting notes, podcast/video repurposing, AI phone answering, AI app/workflow builder, Penpot-class design canvas, SEO/content briefs. Only STT/TTS primitives exist; each is a separate company's worth of work that shares no wedge with the core. Breadth is how a small team dies.

---

## 1. System Map — Planes & Cores (with reality status)

CoreSystem is a 6-plane monorepo. Velion v3 is the only product surface; it reaches every plane exclusively through `velion-gateway-rs` (Rust/axum BFF at `apps/Frontend Plane/velionv3/apps/gateway`, ~70 domain modules, single binary on `:3185`).

| Plane | Cores | Status | One-line reality |
|---|---|---|---|
| **Control Plane** | auth-core, user-core, org-core, billing-core, session-core, audit-core | `mostly-real` | Production-grade identity/billing/audit. Better Auth (2FA/SSO/OAuth, RS256 tokens), Stripe+Lago billing, Brreg, cross-plane tool_action audit. **Gap: tenant isolation is single-perimeter (RLS inert).** |
| **Data Plane v2** | data-orchestrator-go, data-quality-go, documents-api-go, embedding/graph/index/quickwit/retrieval engines (rs), wiki-store-go, retrieval-eval-py | `mostly-real` | 9/10 services real (Postgres+Qdrant+Quickwit+MinIO+NATS). **Doc-lifecycle freshness gap is now CLOSED.** `retrieval-eval-py` is empty. **Gap: documents-api JWT enforce-mode unimplemented (503).** |
| **Ingestion Plane** | Quarry-v2 (4-process), imports-core, integration-corev2, autocomplete-core, finspo-core, support-worker | `mostly-real` | Quarry-v2 real: crawl/scrape/map/batch/search/find-similar/**change**, SSRF-guarded, Exa/Tavily parity. **Gap: durable request-queue is in-memory only; `/v1/sources` empty (schema unbuilt); anti-bot walls.** |
| **Model Plane** | model-gateway, inference-core, execution-core, session-core, orchestrator/cost/capability/sandbox/browser-broker/letta/bridge, lsp/mcp bridges, python labs | `real code / live-broken` | The provider/tool/run code and durable session approval store are real. Current gateway/inference gRPC outage breaks the hot path; live approval/cost/capability boundaries are unauthenticated, and source fixes await compatible caller credentials and rollback gates. |
| **Application Plane** | conversation-core (go+rs), convex-core, information-core, notification-core, **insight-core**, **social-core**, zammad-foundation | mixed | conversation-core-go = real inbox+ticketing (`:3160`, replaced Zammad). social-core = real publishing. insight-core = real but unwired. **convex-core entirely unwired + webhook HMAC is a placeholder.** |
| **Channel Plane** | — | `empty / future` | Docs-only; zero runtime. The deep-dive's "deferred placeholder" verdict is accurate. Where the embed-widget gap will eventually close. |

### The two undocumented cores (you flagged these specifically)

- **`insight-core`** (Application Plane, Go/gin) — **`partial`**. A clean, well-tested workspace-analytics service: org-scoped Overview/rollups/scorecards/connector-gaps across Social, Inbox, Agents, Campaigns + GA4/Search-Console connector slots. **It is NOT empty docs — but it is CRITICALLY unwired:** no `domains/insights.rs` in the gateway, not in any docker-compose, in-memory only, and no producers POST metric events. The v3 `insights` feature already calls `/api/v1/insights/overview` + `/connectors` — which **404 today**. → *Wiring this is the single highest-leverage move (Market Intelligence goes from dead nav item to live in days).*
- **`social-core`** (Application Plane, Go, `:3162`) — **`mostly-real`**. Genuine social publishing: connected accounts (synced from integration-core), campaigns, per-platform previews/char-limits, human approvals, scheduling, durable publish-job queue. Real HTTP integrations for **LinkedIn / X / Instagram / Facebook / TikTok**. Wired via gateway `social.rs`. **Gaps:** gateway serves *fabricated* demo accounts/posts as a silent fallback when the core is down; `trends`/`evergreen`/`competitor-watch` are BFF-fabricated, not backed by social-core.

---

## 2. Velion v3 Frontend — Feature Reality

Feature-sliced SolidJS+Vite+TS SPA (246 files). TS is strict (`noUncheckedIndexedAccess`, `verbatimModuleSyntax`); `pnpm typecheck` and `pnpm lint` pass; **zero `any`, zero `@ts-ignore`, zero prop-destructuring reactivity bugs.** The `docs/core-research/mock-backed-surfaces.md` doc is **stale** — the mock module, hardcoded graphrest client, and "empty" shared dirs it cites no longer exist.

| v3 feature | Backend | Status |
|---|---|---|
| chat | model-gateway invoke + SSE | ✅ real e2e |
| inbox / tickets | conversation-core-go | ✅ real (AI-action HITL flow not yet wired through gateway) |
| knowledge | Data Plane retrieval/docs/wiki/graph | ✅ real (⚠ IDOR via `x-velion-org-id`) |
| agents / runs | execution-core + orchestration HITL | ✅ runs real; ⚠ **WorkflowBuilder is presentation-only (no persistence/execution)** |
| search | Quarry-v2 (web/similar/images/answer) | ✅ real (Exa-style filters, find-similar) |
| ingestions | Quarry-edge runs/schedules/sources/evidence | ✅ real |
| onboarding | org/Brreg/website/graph/recommend | ✅ real e2e |
| settings | user-core (prefs/api-keys) | ✅ real; ⚠ **SSO/SCIM/Org-Security sections are static mockups** |
| billing | billing-core (Stripe+Lago) | ✅ real (org-scoped) |
| insights | insight-core | ❌ **404 — backend not wired** |
| social | social-core | ✅ publishing real; ⚠ demo-data fallback + fabricated trends/competitor-watch |
| finetune / router-policy | session-core routing_policy + Azure | ✅ real |
| studio | gateway-local in-memory canvas | ⚠ in-memory only (survives only while gateway up) |

**Code-quality fixes:** Tailwind utility classes are **no-ops** in v3 (no Tailwind) yet appear across live `agents/` + `studio/` surfaces → those render unstyled. 3 blanket `eslint-disable solid/*` directives hide **99 real Solid reactivity problems** (`Array#map` instead of `<For>`, signals read outside tracked scope). 10 files exceed 800 lines (`ChatPage.tsx` 3685, `DashboardComposer.tsx` 2349). `inbox-demo-data.ts` should be replaced with real conversation-core data.

---

## 3. Critical & High Fixes (the FIX list)

| # | Severity | Issue | Fix |
|---|---|---|---|
| 1 | 🔴 **critical** | **Cross-tenant IDOR via `x-velion-org-id`.** SPA sets it from browser state; gateway does *not* strip it (CORS allows it, `config.rs:210`) and forwards it as the authoritative org to internal-key-trusted cores: `knowledge/*`, `integrations`, `finetune`, `router_policy`, `actions`, `search` (17+ call sites, `knowledge/shared.rs:12`). User in org A reads org B's docs/retrieval/wiki/imports/integrations/finetune. | Strip `x-velion-org-id` at ingress; replace every read with `authorized_org_id(state, user)` (`upstream.rs:115`, already used by billing/audit/inbox/tickets/agents_runs); delete `org_id_from_headers`; remove from CORS allowlist; add a cross-tenant regression test (org A→org B id = 403). **Blocks any multi-tenant sale.** |
| 2 | 🟠 high | **Trust misrepresentation.** SSO/SCIM/Org-Security settings render fabricated `Configured/Verified` (`WorkspaceSettingsPage.tsx:856-1000`); gateway serves fake demo social accounts on upstream failure; trends/competitor-watch BFF-fabricated; non-LIVE actions return synthetic `run_`/`audit_` IDs whose event stream 404s. | Delete fabricated state → honest empty/disconnected/error. Wire `SsoSection` to auth-core's real OIDC ORPC routes or gate behind "coming soon". Lint that fails on `demo_*`/`fallback_*` in non-test paths. |
| 3 | 🟠 high | **Tenant isolation is single-perimeter.** org-core RLS migration is inert (gated off + permits rows when GUC unset); `WithOrgScope` has **zero callers (dead code)**; billing/audit/session/user/documents-api have no row-level enforcement; single shared `INTERNAL_API_KEY`. | Gateway `authorized_org_id` (fix #1) is sufficient for first pilots. Per-core keys + live RLS + documents-api JWT enforce-mode = SOC2/enterprise fast-follow. Do not mistake "safe pattern exists" for "tenancy enforced in depth". |
| 4 | 🟠 high | **documents-api-go auth unimplemented.** JWKS verification missing → enforce mode returns 503; observe mode trusts unverified `X-Org-ID`. A direct caller (bypassing gateway) could assert any org. | Implement fail-closed JWKS-verified JWT enforcement before treating the document write path as production. |
| 5 | 🟠 high | **GDPR erasure/DSAR unreachable in-product.** user-core has real hard-erase/anonymize/Art.15 endpoints but no gateway route, no client, no UI. | Add gateway proxy + v3 account/privacy surface. Table-stakes for EU enterprise. |
| 6 | 🟠 high | **Convex webhook signature = placeholder.** `convex/http.ts:172-184` returns true when secret unset; no real HMAC; convex-core is entirely unwired to v3. | Implement HMAC-SHA256 fail-closed *or* declare convex-core legacy and remove its compose services. Do not expose `/webhooks/*` publicly until then. |
| 7 | 🟡 medium | **Gateway config default port swaps + DB_PASSWORD drift.** `config.rs` defaults `SESSION_CORE_URL=:3013` (listens :3017) and `BILLING_CORE_URL=:3017` (billing :3014) — masked only by compose. session-core defaults `DATABASE_PASSWORD=controlplane_pass` while live DB uses hex → recreating CP cores out-of-band fails auth. | Fix hardcoded defaults; single-source `${DB_PASSWORD}` for every core; reconcile `.env`. |
| 8 | 🟡 medium | **Dev-bypass flags.** `http.ts`/`sse.ts` inject `Bearer dev-bypass` when build flag true; gateway/quarry-edge accept it in dev. | Assert `VITE_ALLOW_DEV_AUTH_BYPASS=false` (and `QUARRY_EDGE_AUTH_DEV_BYPASS`) in release artifacts. |

---

## 4. Competitive Benchmark Mapping (Norwegian market)

Mapping the 18 product areas against Velion's actual cores. **Readiness** = live / partial / scaffold / missing. **Priority** = the team's focus horizon.

| Product area | Competitors | Velion mapping | Readiness | Priority |
|---|---|---|---|---|
| AI inbox/support | Mimir, Intercom-Fin, Gorgias, Zendesk | conversation-core-go + gateway inbox/tickets | **live** | now |
| Social publishing | Buffer, Postiz | social-core (real LinkedIn/X/IG/FB/TikTok) | **live** | now |
| Web data extraction | Browse AI, Apify, Firecrawl, Diffbot | Quarry-v2 `/v1/scrape,crawl,batch,map,extract,browsers,profiles` | **live** | now |
| Internal tools/agents | Retool, appsmith | execution-core governed multi-tool loop + Agent Run Console | **live** | now |
| Trust/security layer | Bitwarden, DocuSign | Control Plane (auth/org/billing/audit) + GDPR controls | **live** | next |
| Knowledge workspace | AFFiNE, Ayfie | Data Plane wiki+graph+retrieval+operating-map | **live** | next |
| **Market intelligence** | Feedly, **Ayfie** | **insight-core** (real) | **scaffold** ⚠ *404 at v3 boundary* | **now** |
| **Competitor & price monitoring** | Prisync, Visualping | **Quarry `/v1/change`+`/v1/schedules`** (real, unsurfaced) | **partial** | **now** |
| Website chatbot | Chatbase, CustomGPT | ChatbotStudio + chat backend (no `/embed` delivery) | partial | now→later |
| Vertical data feeds | Apify, Diffbot | Quarry schedules+profiles+batch + Data Plane | partial | next |
| **Lead/list building** | **Clay**, twenty | org-core Brreg lookup (single-company only today) | **missing** | next (**the net-new bet**) |
| AI transformation templates | Conducting.ai | agent-blueprints + router-policy modes | partial | later |
| Design/canvas | Penpot | Studio (in-memory gateway canvas) | partial | later |
| SEO/content briefs | Surfer, Clearscope | — (assemblable from search + Search Console + LLM) | missing | later |
| Podcast/video repurposing | Castmagic, OpusClip, Descript | only STT/TTS primitive | missing | **cut** |
| Meeting notes | Granola | only transcription primitive | missing | **cut** |
| AI phone answering | telephony APIs | none (no SIP/voice loop) | missing | **cut** |
| AI app/workflow builder | v0, Replit, Bolt, Lovable | none (orchestration ≠ user automations) | missing | **cut** |

---

## 5. The Norwegian Wedge (why Velion can win locally)

Velion's defensible, **uncopyable** combination — no global competitor (Clay, Apify, Intercom-Fin, Buffer) ships it:

1. **Brreg / Enhetsregisteret-native company resolution** — `org-core/internal/brreg/client.go` (`SearchByName`/`LookupByOrgNr`) + `execution-core info_tools.rs` exposes it as an agent tool, **LIVE-verified against AQUATIQ AS**. Every company/entity reference grounds to the real Norwegian register, not a hallucinated global record.
2. **EU / Sweden-Central data residency** — Azure `eastus2` TTS deliberately gated OFF to keep content in-region. (Do *not* over-claim "data stays in Norway" until a `norwayeast` deployment is evaluated.)
3. **A wired GDPR/audit trust posture** — auth-core 2FA/SSO/passkey, audit-core's **cross-plane per-data-category "Used by AI?" tool_action trail** (genuinely real and reachable), tenant scoping, CSP.
4. **The category:** an **observable, approvable, cost-aware, grounded autonomous agent-runner** — real end-to-end and the one area where Velion is *ahead* of point competitors. Pair it with the AI-first operating model (intent → plan → grounded retrieve → execute reversible → **approve risky** → audit → feedback) and **manual parity** (every AI action also doable by hand).

---

## 6. Council Verdict — Ordered Priorities

Five advisors (Pragmatic Eng Lead, Security/Compliance Hawk, GTM/Commercial, Product/Category Strategist, Contrarian) + chair synthesis.

1. **[now]** Fix the cross-tenant IDOR across all 6 trusted-key gateway domains; delete `org_id_from_headers`. *Unanimous #1 — blocks the existence of a second tenant.*
2. **[now]** Delete every fabricated trust/security/social/run-id state → honest states. *Existential brand risk; deletion is cheaper than wiring.*
3. **[now]** Minimal CI PR-gate (cross-tenant regression test + no-fabricated-state lint) + reconcile DB_PASSWORD drift. *Otherwise #1/#2 silently regress.*
4. **[next]** Wire the 3 built backends that complete the honest core loop: insight-core, conversation-core-go HITL approve/reject, Quarry change/schedules. *Pure glue with the safe pattern while it's fresh — this is the monitor→brief→approve spine.*
5. **[next]** Wire user-core GDPR erasure/DSAR + the "Used by AI?" audit trail with real UI. *Makes Trust Center claims provable; legally covers the lead-builder.*
6. **[later]** Build Brreg-seeded lead/list-building. *The single net-new bet — growth, not survival; ships only after the loop is honest, wired, tenant-safe, DSAR-covered.*
7. **[later]** Port v1 embeddable website-chatbot (`/embed/[agentId]/config+/stream`) behind its own scoped key + origin allowlist + SSRF review. *Cheap funnel, but a public-surface IDOR multiplier; not the headline.*
8. **[later]** Data-layer defense-in-depth: per-core API keys + RLS on documents/knowledge/audit; fix documents-api JWT enforce-mode. *SOC2-era fast-follow, not a first-pilot gate.*
9. **[later → cut]** Cut the greenfield wishlist outright (meeting notes, video repurposing, AI phone, app builder, design canvas, SEO briefs).

**Genuine disagreements (kept, not papered over):**
- **GTM headline:** lead with the Brreg+residency *wedge* (3 advisors + contrarian — closes fast, locally credible, uncopyable, doesn't trigger security scrutiny the product currently fails) vs. lead with the *agent-runner category* (Product Strategist). → **Chair: lead commercially with the wedge; demo the agent-runner as the moat.**
- **Embeddable chatbot priority:** most rank it high (cheapest revenue) vs. Product Strategist (commodity, dilutes positioning). → **Chair: keep but demote to "later" with its own security review.**
- **Tenancy depth:** Hawk + Contrarian want per-core keys + RLS before any enterprise talk vs. Pragmatist + GTM (premature for first pilots). → **Chair: gateway `authorized_org_id` suffices for first pilots; deep RLS is a SOC2-era fast-follow.**
- **Which security defect is worse:** Hawk argues fabricated "Verified" UI (bad faith) > IDOR (fixable bug). → **Chair: both now-tier, same sprint.**

---

## 7. Detailed Backlog (synthesis lists)

### ▶ START (new, ordered by leverage *after* the survival sprint)
- **Wire insight-core through the gateway** (`domains/insights.rs` + `insight_core_url` + `main.rs` merge; translate `x-velion-org-id`→`x-org-id` server-side; Overview→metrics adapter). *S.* Flips Market Intelligence from permanent 404 to live.
- **Surface Quarry `/v1/change` as a gateway monitoring domain.** *M.* Ships Visualping/Prisync-class Competitor & Price Monitoring; replaces fabricated `derived_competitor_watch`.
- **Wire conversation-core-go AI-action HITL** (review/approve/reject via inbox.rs/tickets.rs + SPA client). *S.* The literal "after human approval" promise.
- **Expose user-core GDPR erasure + DSAR** through gateway + v3 account/privacy surface. *M.*
- **Brreg-seeded lead/list-builder** (search Enhetsregisteret → enrich → export). *L.* The defensible net-new bet — **build last**.
- **Embeddable website-chatbot `/embed`** delivery. *M.* Cheap funnel — behind its own scoped key.

### ↻ CONTINUE (in-flight, finish it)
- Convert agents **WorkflowBuilder/WorkflowCanvas** to real persistence/execution (orchestration.rs + agents_runs.rs exist) — or explicitly label it a design preview.
- Finish the **Data Plane→Model Plane gRPC embedding hop** + freshness-chain & ZDR regression tests (embeddings still on a direct-Azure workaround).
- Land **Quarry durable backends** (Redis/Postgres request queue, `quarry_sources` schema, Temporal schedule trigger/backfill) — gates monitoring at scale.
- Finish **Model Plane HITL boundary hardening**: durable session-core approval already exists and the guarded path blocks before execution; deploy authenticated tenant/actor pinning, requested-state CAS, caller migration, and live bypass regression tests.
- Convert **Tailwind no-op classes** to semantic CSS + remove the 3 blanket `eslint-disable solid/*` (fix the 99 hidden Solid issues).

### ✗ FIX → see §3 (IDOR, fabricated trust, RLS depth, documents-api JWT, GDPR reachability, convex HMAC, config ports/DB drift, dev-bypass).

### · DEFER / CUT
Podcast/video repurposing · meeting notes · AI phone answering · AI app/workflow builder · Penpot-class design canvas · SEO/content briefs · Channel Plane embed runtime · convex-core realtime (or retire) · `norwayeast` localisation · distributed gateway rate limiter · DPA/sub-processor Trust Center panel (pair with processor registry when SSO/DSAR land).

---

## 8. Top Risks

1. **Security/sales blocker:** the IDOR fails any enterprise security review — fix before selling multi-tenant. The safe pattern already exists in-repo (contained fix).
2. **Tenancy depth:** data-layer isolation is a single gateway perimeter (RLS inert, dead code; shared internal key; documents-api auth unimplemented). No defense-in-depth for the multi-tenant claim.
3. **Trust misrepresentation:** fabricated "live" SSO/SCIM/social/run-id state on a product whose entire differentiation is observability/trust — existential with EU buyers.
4. **Compliance procurement gap:** GDPR/DSAR unreachable + SSO is a mockup — table-stakes for Norwegian/EU deals (the transparency story behind it is genuinely strong).
5. **Strategic focus:** leverage is in *wiring + hardening*, not chasing greenfield media/builder categories. Lead GTM with the real wedge.
6. **Operational fragility:** no root build/test orchestration; partial CI (most planes ungated); Quarry coverage CI calls non-existent Make targets; DB_PASSWORD drift breaks clean recreate.

---

## 9. Appendix — Method & Evidence

- **codegraph:** 3,076 files, 56,012 nodes, 130,319 edges indexed (Go 716, Rust 709, TS 887, TSX 573, Py 178).
- **Audit workflow:** 16 specialist agents (one per plane + both undocumented cores + 5 v3 dimensions + competitive map + build health + synthesis), 2.48M tokens, 325 tool calls, ~83 min. Full JSON indexed in context-mode (`source: "Velion v3 CoreSystem audit 2026-06-19"`).
- **Council:** 5 personas + chair, 469k tokens. Code-confirmed the IDOR and the safe pattern's existence.
- **Memory updated:** MemPalace (9 KG facts + diary `claude-auditor`/wing `Velion` + a 5-chunk synthesis drawer in room `audit-2026-06-19`); context-mode index; Logseq page *"Velion AI-First Audit 2026-06-19"*.
- **Build/test baseline:** 199 Go `*_test.go`, 119 Rust integration tests (+397 `#[cfg(test)]` files), 92 TS tests (37 in velionv3 via Vitest), 82 Python `test_*.py`. velionv3 has a `pnpm verify` gate (lint+typecheck+test+build).
- **Key reference docs:** `apps/CODEBASE_INFORMATION_SYSTEM.md`, `apps/master-ownership-matrix.md`, `apps/GDPR_SUMMARY.md`, `docs/self-owned-systems.md`, `apps/STATUS.md`, `apps/Frontend Plane/velionv2/ux-gap.md` (competitive bible, 2026-06-01), and per-core `docs/core-research/*`.

*Generated by an exhaustive multi-agent audit on 2026-06-19.*
