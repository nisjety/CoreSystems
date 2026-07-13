# Velion

> **Production-readiness correction — 2026-07-13.** The 2026-07-10 “live” sections below are historical, not a current release certificate. Model Plane `model-gateway:9090` and `inference-core:9092` are absent while HTTP health remains green, so default inference/chat, tools that depend on inference, and Data Plane query embedding are currently unavailable. Live cost/session/capability boundaries remain unauthenticated, semantic memory search is down, and no Velion Visma runtime integration exists. Significant authenticated/tenant-scoped source fixes, exact audience issuance, and the ordinary invoke caller graph pass source tests but are not deployed; approval/browser/Letta/background callers, a verified ZDR provider route, compatibility, and rollback gates remain incomplete. See [MODEL_PLANE_STATUS.md](apps/Model%20Plane/MODEL_PLANE_STATUS.md) and the [2026-07-13 audit](apps/Model%20Plane/docs/core-research/plane-audit-2026-07-13.md).

**The Norwegian AI workbench that turns grounded intelligence into approved action.**

Velion is both a product and the AI worker at its center: an EU/Norway-first, source-grounded, observable, and governed autonomous agent for customer support and knowledge work. It is designed to extract data, monitor change, build briefs, route work through an inbox, and publish, with a human approving every consequential step. Grounding, approvals, residency, and retention are runtime properties that must be verified per request; they are not implied by the presence of a backend service. Velion is delivered as a single workbench — chat, search, knowledge, inbox, agent runs, insights, and a trust center — built on the **CoreSystem** multi-plane platform, with a Norwegian-native spine (Brreg/Enhetsregisteret resolution, Bokmål, EU model routing) at its core.

## Historical live status — 2026-07-10

The Dockerized CoreSystem stack had 91 running containers and no unhealthy containers during the live verification. The authenticated Velion v3 cross-plane Playwright smoke suite passed 6/6. The following distinctions are authoritative over older “live” wording in this document:

- Normal chat is live model inference, but it does not request tools, RAG, or knowledge grounding by default. It cannot discover shipping-core, MCP, Control, or Data capabilities unless Browse, an explicit action, or Plan mode is selected.
- Browse mode successfully used live web search/fetch and emitted citations. Knowledge search was reachable but returned an honest empty result for the seeded organization.
- Shipping is mixed live/demo. Bring production rating returned real prices, but shipping-core dropped Bring delivery-time fields and returned zero transit time. Four carriers are explicit mocks; DHL/UPS/FedEx are configured for test/sandbox environments, not production proof.
- One organization-scoped “visma mcp” record exists, but it is configured as `stdio` with an HTTPS URL and no tool allowlist. It was not executed. No in-repository Visma adapter was found.
- The frontend action registry, direct Model Gateway tools, and execution-core Plan tools are separate runtime catalogs. The same human/AI action contract is a target, not yet an end-to-end guarantee.
- Critical blockers remain: unauthenticated shipping/import/Data routes, MCP command/SSRF exposure, Quarry bearer bypass in the running environment, and incomplete ZDR propagation.

The detailed live evidence is recorded in the 2026-07-10 addenda in each plane’s core-research audit.

---

## The thesis & category

### An intelligence-to-action workbench, not a chatbot

The market for "AI in your business" has settled into four paradigms. Velion deliberately competes in only one of them:

| Paradigm | Representative players | What Velion maps to |
|---|---|---|
| **1. AI in the support inbox** | Intercom Fin, Zendesk, Gorgias | conversation-core-go (live) |
| **2. Build-a-bot-on-your-docs** | Chatbase, Zapier | not the bet — too shallow |
| **3. Observable autonomous task runner** | Relevance AI, Rox — **and Velion** | execution-core + Agent Run Console (live) |
| **4. Answers / enterprise search** | ayfie, partly Mimir | insight-core, Quarry search |

**Velion's category is #3 — the runner.** The strategic instruction is explicit: *beat Relevance/Rox, not Intercom.* A chatbot answers; Velion **does the work** — observable and governed — and shows you every step.

### Velion's wedge

Velion's defensible position rests on the intersection of five things almost no single competitor combines:

- **Approvable execution.** Inline Approve/Reject gates on every risky tool call. Relevance offers only a coarse "Paused"; most competitors gate nothing. This is the governance moat.
- **Cost-awareness as an up-front control.** The Budget/Balance/Genius intent layer chooses the model *before* it runs, downgrading under budget pressure. Relevance shows credits *after*; Chatbase/Zapier make you pick a raw model.
- **Deeper grounding.** Data Plane v2 GraphRAG + wiki + dense/sparse vectors + source-traces, versus "add a URL."
- **Owned browse + act.** Quarry executes real `browser_action`/`observation` timeline events under policy.
- **EU/Norway residency + ZDR + audit — *with* actions.** ayfie/Mimir have the home-turf advantage but cannot *act*.

### The commercial frame

GTM leads **commercially** with the Norwegian wedge — Brreg-grounded intelligence, in-region data, GDPR posture — and demos the **observable agent-runner second**, as the moat and upsell. The four uncopyable pillars:

1. **Brreg/Enhetsregisteret-native resolution** — entities ground to the real Norwegian register (live-verified against AQUATIQ AS), not a hallucinated global record.
2. **EU/Sweden-Central data residency** — non-EU paths gated off by default.
3. **A wired GDPR/audit trust posture** — 2FA/SSO/passkey, cross-plane "Used by AI?" tool-action trail.
4. **The observable, approvable, cost-aware, grounded agent-runner** — a category where Velion is ahead of point competitors.

**Target customer:** Norwegian and EU enterprises and SMBs. The proof story is one end-to-end demo — *Brreg resolution → monitor → brief → approve → act-in-region* — put in front of a Norwegian design partner. Lighthouse dogfood customer: AQUATIQ AS.

---

## What it can do

Capabilities are modeled as a **typed action registry** (`velionv3/src/shared/actions/action-registry.ts`) — each action carries id, owner-plane, risk, `requiresApproval`, reversibility, and Zod input/output schemas. The same descriptors become Model-Plane tool specs (`agent-tools.ts`), so **the human UI and the AI agent share one action contract.**

### Grounded answers — chat & inbox

- **Source-grounded chat** *(opt-in / partially verified)* — durable threads/transcripts over the Model Plane (`/v1/invoke`, streaming, resume, cancel), with live provider inference. Grounding is available through Browse/tools/knowledge paths; normal chat does not request it and must not be described as automatically source-grounded.
- **Shared / team inbox** *(live)* — conversation-core-go powers inboxes, conversations, messages, internal notes, status, and assignment, plus an `ai-actions` **HITL review queue** where the agent drafts replies for human approval. *(Backend is conversation-core-go, not Zendesk.)*
- **Ticketing** *(live)* — create, classify, update, assign, link-resource, resolve.

### Knowledge & ingestion

- **Knowledge base** *(live)* — a rich `LiveKnowledgePayload` fans out across documents, integrations, finspo, graph, retrieval, and Quarry; documents flow into Data Plane v2 retrieval/graph and an LLM wiki.
- **Website crawl & scrape** *(live)* — `/v1/map` discover → page-picker selection → durable `/v1/batch` with run-events SSE; **private-by-default** with selective ingest. Scrape-preview is hardened against bot-walls and timeouts.
- **Imports & connectors** *(live, subset)* — Slack, Gmail, Notion, SharePoint, OneDrive, Outlook via integration-corev2; SharePoint also via finspo-core Graph delta-sync.

### Search & browse

- **Web search** *(live)* — Quarry-v2 SmartSearchRouter behind the gateway: web (with Exa-style filters — topic, time-range, include/exclude domains, exact-match), find-similar, images, video (SearXNG), answer-with-citations (SSE), did-you-mean + related queries + entity knowledge panel.
- **Precision layer** *(live)* — Model-Plane LLM rerank with query-relevant highlights (degrade-safe), and autoprompt query rewriting for research/comparative intents.

### Autonomous agent runs — with HITL approval & cost-awareness

- **Multi-tool agent loop** *(historically verified Plan-mode engine; currently blocked by inference)* — execution-core contains real web, knowledge, shipping, provider, social, Brreg, weather, traffic, news, tracking, and MCP paths. Catalogs differ across modes, and no current live end-to-end success may be inferred from the catalog.
- **Observable + approvable runs** *(real guarded path; current boundary unsafe)* — `PermissionMode::Ask` pauses before a risky tool and persists approval. The running session-core gRPC approval surface is unauthenticated, so HITL is not globally safe. Tenant/CAS containment and inline MCP denial are source-only fixes.
- **Smart model selection** *(live)* — `VelionMode::Budget/Balance/Genius` blends heuristic task complexity with the org's cost-core budget posture, downgrading a tier when constrained and falling back to Azure model-router when exhausted.

### Onboarding intelligence

- **AI-personalized plan recommendation** *(live)* — weighs connector count, sources, websites, employees (Brreg or operator head-count), website agent-brief, and governance signals → trial/team/enterprise, via an AI remote path with a local heuristic fallback. Brreg verification and graph-preview run live in onboarding.

### Insights & monitoring

- **Insights / analytics** *(partial — honest-empty)* — insight-core overview + connectors; the SPA renders an **honest empty state** when upstream has no data — no fabricated metrics.
- **Monitoring / change-watch** *(live)* — Quarry-v2 versioned change detection: check, latest, history, schedules, with changed-paragraph diff highlights.
- **Leads / Brreg lead builder** *(live, gated)* — filtered Enhetsregisteret search, enrichment, governed `build_list`, saved org-scoped lists, CSV export; **company data only, never natural-person PII**; metered, audited, entitlement-gated. *(End-to-end use requires the "leads" billing entitlement.)*

---

## Where it works

### Surfaces

- **Internal AI agent + HITL inbox** *(live)* — the production loop: agent drafts, human approves, action executes.
- **Internal knowledge work** *(live)* — chat, search, knowledge base, studio.
- **Agent Run Console** *(live)* — `/agents/runs`, observable and approvable.
- **Website chatbot** *(partial → roadmap)* — `agents.deploy_channel` and a `chatbot/runtime` config endpoint exist, but there is **no live embeddable end-customer widget** yet; the public `/embed` funnel is a scoped, origin-allowlisted roadmap item.

SPA routes: `/chat`, `/inbox`, `/tickets`, `/knowledge` (+ `/shared`), `/ingestions`, `/agents` (+ `/runs`), `/insights/*`, `/leads`, `/social/*`, `/studio/*`, `/settings`, `/onboarding`.

### Integrations & connectors

Slack, Gmail, Notion, SharePoint, OneDrive, Outlook (live); SharePoint also via finspo Graph delta-sync. Providers are returned dynamically by integration-core, not hardcoded. *(Zendesk connector is roadmap.)*

### Norwegian-market fit

- **Brreg/Enhetsregisteret company lookup** — live, classified `public_non_personal`, proven against AQUATIQ AS (org 983 515 827).
- **Non-PII lead-builder** (leads-core) — company-only, metered, audited.
- **Bokmål UI** *(partial)* — `i18n/no.ts` is the single-source-of-truth seam; strings migrate incrementally.
- **EU model routing** — Azure OpenAI Sweden Central default.

---

## The platform underneath — CoreSystem

CoreSystem is a multi-plane monorepo with **downward-flowing authority.** Each plane owns one thing and crosses boundaries only through contracts.

### Control Plane — *identity & governance*
Authority root for identity, users, orgs, billing, sessions, audit, quotas, and entitlements. Services: auth-core (NestJS JWT/session/token authority), user-core, org-core, billing-core, session-core, audit-core.

### Data Plane v2 — *durable knowledge*
The **only** owner of documents, chunks, embeddings, retrieval, GraphRAG, LLM wiki, and source traces. Services: documents-api-go, index-engine-rs, embedding-engine-rs, retrieval-engine-rs (hybrid BM25 + dense + rerank), graph-index-rs, wiki-store-go, data-orchestrator-go, data-quality-go, quickwit-adapter-rs.

### Ingestion Plane — *evidence capture*
Captures evidence and persists durable knowledge **only through Data Plane contracts.** Quarry-v2 is the source of truth: quarry-edge, quarry-runtime, quarry-browser (CDP action runtime), quarry-control, quarry-orchestrator (Temporal). Plus imports-core, integration-corev2, finspo-core, autocomplete-core.

### Model Plane — *reasoning & execution*
Owns reasoning, sessions/runs, inference, the execution loop, capabilities, sandboxes, browser grants, and cost. Rust: model-gateway, session-core, inference-core, execution-core. Go: orchestrator-core (Temporal), capability-core, sandbox-manager, browser-broker, cost-core, bridge-core.

### Application Plane — *realtime & collaboration*
Owns collaborative/realtime workspace projections and notifications only — **not** identity, billing, knowledge, ingestion, or reasoning. Convex stack, affine-core/runtime, conversation-core-go, information-core, notification-core, leads-core.

### Frontend Plane — *Velion + BFF seam*
Velion v3 is a SolidJS SPA whose **only** path to the planes is the Rust BFF gateway (`velionv3/apps/gateway`, deployed as velion-gateway-rs). The gateway exposes ~50 per-plane domain modules (auth, chat, knowledge, ingestions, agents, leads, ownership, privacy, billing, search, orgs, settings, social, insights, monitoring, mcp, finetune, …) plus the onboarding flow, normalizes everything to typed envelopes, and **must never leak upstream secrets or raw OAuth tokens.**

### Channel Plane — *docs-only*
Reserved for future external-agent deployment (adapter-core, widget-core, public visitor runtime). **No runtime exists today — the plane is documentation only; do not build against it.**

### Tech stack by job

- **Rust** — latency/CPU/parsing/retrieval/browser/protocol hot paths (Quarry runtime, Data engines, Model gateway/session/inference/execution, the BFF gateway).
- **Go** — durable workflow, CRUD, registry, policy, scheduling, grants, billing, operator control.
- **TypeScript/TSX** — Velion frontend + BFF, NestJS auth-core, Convex functions.
- **Python** — imports, labs, evals, provider glue only (no production hot path).
- **Infra** — Postgres, Redis/Dragonfly, NATS/JetStream (the cross-plane event spine on a shared `inter-plane-bus`), Qdrant, MinIO, Temporal, Quickwit, Convex.

### Non-negotiable cross-plane rules

1. **No direct database crossing** — Model and Quarry consume Data Plane APIs, never its Postgres/Qdrant.
2. **No independent embeddings/reranking** outside isolated eval labs.
3. **Model proposes, Quarry executes (or rejects)** browser actions — no agent bypass around Quarry policy.
4. **Zero Data Retention propagates** across every content-persisting boundary.
5. **GDPR policy metadata** (purpose, lawful basis, retention, residency, privacy class, third-party processing, deletion scope) travels with durable records and jobs.

---

## Ultimate position — Norway-first, full GDPR & data residency

This is the strongest part of the story, and the one held to the strictest honesty gate: the compliance controls **are** the product.

### EU/Norway data residency

- **Residency Tier 1 (live)** — primary inference *and* embeddings run on **Azure OpenAI Sweden Central** (EU/EEA). Every `StartRun` stamps a `residency` value (default `swedencentral`). App data, conversations, run-history, audit, and the vector store are EU/EEA Velion-controlled stores.
- **Non-EU TTS gated off by default (live)** — text-to-speech historically ran in East US 2. `speech.rs` now **defaults to the EU endpoint and refuses to use a configured non-EU TTS endpoint** unless `MODEL_PLANE_ALLOW_NON_EU_TTS=true` (default `false`); when the flag is off, a set `AZURE_OPENAI_TTS_ENDPOINT` is ignored and voice stays on the EU `AZURE_OPENAI_ENDPOINT`.
- **Residency Tier 2 (Norway East)** *(roadmap)* — documented but conditional. **Honesty bound:** we do **not** claim "data stays in Norway" until a norwayeast deployment is evaluated.
- **Sovereignty caveat (disclosed):** EU residency is **not** data sovereignty. A US-headquartered subprocessor (including Microsoft Azure) remains reachable under the US CLOUD Act regardless of physical location. This is logged as a disclosed residual risk in the Schrems II transfer assessment with supplementary measures — never claimed as immunity.

### Zero Data Retention (partial at model layer)

The inference cache correctly skips reads and writes for `req.zdr`, and current gateway/inference source makes issuer-required ZDR monotonic for unary/SSE infer/embed. That is not end-to-end ZDR: session replay classification, compaction, memory, traces, tool I/O, Data Plane grounding, external bridges, provider eligibility, and non-infer/embed modality contracts remain unproved or incomplete. A request whose issuer requires ZDR must not be described as ZDR if Velion retains run history/content under ordinary retention. Only the Azure OpenAI Sweden Central path is presently classified ZDR+EEA; no compliant live end-to-end proof exists.

### In-infra / 0-SaaS search (partial)

Quarry-v2 SmartSearchRouter tries a free in-infra chain first — **Tantivy → Stract → SearXNG** — and only falls back to paid external SERPs (Brave/Serper) as a last resort. Omit the paid keys and you get **zero external egress** by routing order and config. *(A hard policy-enforced "0-SaaS / no-egress" toggle is roadmap; today it is configuration-driven.)*

### The GDPR control set

- **Right-to-erasure (partial)** — `gdpr_hard_delete_user` and `gdpr_anonymize_user` primitives are live in user-core (with handlers and tests) and now *called* by user-core and org-core via admin/owner-gated, audited, `confirm:true` endpoints, emitting a `velion.gdpr.erasure.requested` event. The cross-plane subscribers that delete derived copies in Model/Data/Application/Ingestion are **roadmap**.
- **DSAR / export (partial)** — an Art. 15 export path exists internally; a customer-facing intake endpoint is being built (not yet GA).
- **Retention janitors (partial)** — audit-core runs a real sweep purging audit + usage events older than `AUDIT_RETENTION_DAYS` (365) every 24h; org-core has a purge cron. Other TTLs (run-history, conversations, crawl artefacts, telemetry, backups) are *proposed* — sweeps TBC.
- **Audit log incl. cross-plane `tool_action` (live)** — audit-core dual-subscribes to the Model-Plane NATS; `GET /api/v1/audit?event=tool_action` returns `{event, plane:model, details:{data_category, zdr, tool}}`, feeding the Trust Center "Used by AI?" column.

### Tenant isolation & per-user ownership

- **Org-axis isolation (live)** — membership re-check guards fail closed on mutation routes; the gateway derives org from the session (IDOR-clean).
- **Postgres RLS backstop (partial)** — delivered but **inert by default**, enabled only via an explicit `ALTER DATABASE … SET app.org_rls_enabled='true'` opt-in at the database layer. Defense-in-depth, not yet active; deep RLS + per-core keys are a SOC2-era fast-follow.
- **Per-user private-until-shared (partial — substrate live, claim not yet shippable)** — a single grant authority (`resource_grants`), `documents.owner_id` + `visibility ∈ {private, org, shared}`, filter-at-source on GET/LIST, and a retrieval **post-filter** gating dense + sparse + wiki are all live. But everything still defaults to `org`, graph grounding and the ExecuteStep gRPC carry no `user_id`, and the Qdrant/Quickwit pre-filters are inert. **Absolute rule:** no "Private" badge or "the AI only sees your data" copy renders until all release-gate conditions hold, including a four-path honesty test.
- **Privacy data classification (live)** — a 6-class taxonomy (`public_non_personal`, `customer_private`, `personal`, `sensitive_personal`, `credential_or_secret`, `zdr_ephemeral`) governs storage/indexing/sharing, with **default-deny** third-party processing for protected classes.

### Trust Center, SSO & 2FA (live)

The Trust Center surfaces App / Permissions / Data fetched / **Used by AI?** / Retention / Last sync / Disconnect, fed by integrations + the audit `tool_action` aggregation. SSO sign-in and 2FA enrollment are wired with gateway proxies. Gateway rate-limiting (token-bucket, 429 + Retry-After, org→user→IP) and security headers (via nginx + tower-http) are live; **CSP is Report-Only today** (partial). *(Trust Center per-connection Retention is a placeholder.)*

### Where Velion's trust wedge is real — and where it must be earned

**Be precise (and honest).** The lazy claim *"US vendors leak your data, we don't"* does **not** survive contact with a mature competitor. [wonderful.ai](https://trust.wonderful.ai), for example, offers **EU subprocessor regions** (AWS/GCP/Azure/OpenAI listed "USA, EU") and holds **SOC 2 Type II + ISO 27001/17/18/27799 + ISO/IEC 42001 + an EU AI Act risk assessment** on a public **Vanta** Trust Center. And Velion itself runs on **Azure** — so "your data stays in the EU" is achievable by both, and *neither escapes the US CLOUD Act by region alone* (we disclose this above).

Velion's **defensible** trust differentiators are therefore narrower and sharper than "we're more compliant":

- **ZDR enforced by default at the model layer** (in code) — not an after-the-fact contract clause.
- **EU-resident *by default*** (Sweden Central) — not "EU region available on request."
- **Norwegian-native** grounding (Brreg/Enhetsregisteret + Bokmål) — a regional fit a US platform won't build.
- **0-SaaS / in-infra search** — queries needn't leave our infrastructure.
- **Per-action approval + per-category "Used by AI?" audit** — concrete governance, not a generic "oversight" tile.
- **Product-led accessibility** for the mid-market that enterprise-sales-only competitors ignore.

Against the Norwegian incumbents (ayfie/Mimir) the edge is simpler: home-turf trust, but they **cannot act**.

### Trust & certifications — the honest gap and the roadmap

The uncomfortable truth a *trust-led* positioning must own: **today Velion has the controls but not the credentials.** The controls above are real and largely live, but we hold **no third-party certifications yet** and have **no public, prospect-facing Trust Center** — only the in-app one. On procurement-grade trust, a Vanta-backed competitor with SOC 2 Type II + ISO 42001 currently *out-documents* us. Closing this is a priority, not a footnote.

| Trust asset | Status | Plan |
|---|---|---|
| In-app Trust Center ("Used by AI?", retention, audit) | **live** | keep; feed from the cross-plane `tool_action` trail |
| **Public** Trust Center (subprocessors, controls, data-flow map, request-access) | **roadmap** | stand one up (Vanta/SafeBase/Drata or custom on `velion-web`); publish a tighter, EU-default subprocessor list |
| SOC 2 Type II | **roadmap** | open the observation window; continuous controls |
| ISO 27001 | **roadmap** | ISMS scoped to the platform |
| **ISO/IEC 42001** (AI management) + **EU AI Act** readiness | **roadmap — and a wedge** | *more native to a sovereign EU AI platform than to a US one*; aim to be the ISO-42001 / EU-AI-Act-ready Norwegian agent platform |
| Independent penetration test (published) | **roadmap** | web-app/API pentest report |

Until these land, sell the **defensible** differentiators above honestly — and treat the certification path + a public Trust Center as the unlock for trust-led enterprise deals.

---

## Maturity & honesty

Velion is **real, not a demo.** Reality score: **~72% production-real (backend planes ~85%).** The SolidJS SPA reaches every plane only through the Rust BFF gateway, with zero mock modules and ~40 typed gateway clients. The framing is deliberate: **the engine is built — most remaining high-leverage work is WIRING already-built backends through the gateway, not net-new construction.** The honesty gate is load-bearing — we never represent a control as in place without engineering confirmation.

| Status | Capability / control |
|---|---|
| **Live** | Source-grounded chat; shared inbox + HITL ai-actions queue; ticketing; knowledge base + ingestion; private-by-default Quarry crawl; web search (rerank, highlights, autoprompt, find-similar, did-you-mean, entity panel, video); multi-tool agent loop + Agent Run Console + approvable HITL gate; Budget/Balance/Genius selection + cost-core budget; AI-personalized onboarding + Brreg verification; Brreg company_lookup; monitoring/change-watch; EU residency (Sweden Central) + residency stamp; non-EU TTS gated off; model-layer ZDR; erasure primitives (now called); audit incl. cross-plane tool_action; org-axis isolation; privacy data-classification; in-app Trust Center + SSO + 2FA; gateway rate-limiting; social publishing; router-policy + fine-tune; per-user ownership substrate |
| **Partial** | Customer-facing chatbot deploy (config only, no live widget); deeper multi-step run engine (MVP dispatch live); leads (end-to-end needs the billing entitlement); insights (honest-empty until data accrues); scrape anti-bot infra; Norway-East residency; ZDR across all vendors; in-infra-first search (config, not hard gate); cross-plane erasure fan-out; customer-facing DSAR intake; RLS backstop (inert/gated); private-until-shared as a *claimable* guarantee; CSP enforcing; Bokmål coverage; ZDR/GDPR-metadata propagation; Trust Center retention column |
| **Roadmap** | Embeddable end-customer chat widget (`/embed`); Brreg-seeded lead/list builder as the net-new monetization bet; real WorkflowBuilder persistence/execution; multi-tenant X-Org-ID fully consulted across all cores; "0-SaaS / no-egress" hard toggle; retention sweeps for non-audit stores; Zendesk connector; **public Trust Center**; **third-party certifications** (SOC 2 Type II, ISO 27001, ISO/IEC 42001, EU AI Act assessment) + independent pentest |
| **Docs-only** | Channel Plane runtime (adapter-core, widget-core, public visitor conversation runtime); compliance documentation pack (drafted, counsel-review pending) |

**Cut, not deferred** — meeting notes, podcast/video repurposing, AI phone answering, an AI app/workflow builder, a Penpot-class design canvas, SEO/content briefs. Breadth is how a small team dies; the wedge is depth.

---

## Competitive strategy — what we take from the horizontal platforms (Wonderful.ai)

[Wonderful.ai](https://www.wonderful.ai/) is the sharpest exemplar of the *horizontal, US-style, production-led* enterprise agent platform: **"Get AI into production, everywhere it matters"**, run across Support · Sales · HR · back-office, on a named layered stack (Management / Orchestration / Business-Context / Infrastructure) that "compounds across every deployment." We will **not** out-broaden or out-fund it. We win by sharpening our wedge — **sovereignty + depth + governed action + accessibility** — *and* by borrowing its two strongest narrative moves, tailored to Velion.

### Two narratives to adopt — tailored to Velion specifics

**1 — "Run *sovereign* AI across your operations": breadth as vision, depth as commitment.**
Wonderful's *"Run AI across your enterprise"* (customer · employee · back-office) is a bigger-TAM story than our support+knowledge focus. We do **not** chase that breadth now — a small team dies on breadth (see Cut list). But the same engine we already ship — the typed **action registry**, **execution-core** agent loop, and **Model Plane** — generalises beyond support. So we adopt breadth as a **horizon vision**, not a roadmap commitment: *"operationalise sovereign AI across customer, knowledge, and back-office operations — in-region."* Every **shipped** capability stays deep in the support/knowledge wedge; the breadth lives on a "where this goes" slide. **Velion's twist Wonderful can't copy:** the breadth is *sovereign* — EU/Norway-resident, Brreg-grounded, ZDR — so "AI across your operations" means *across your operations without your data leaving the region.*

**2 — "From pilot to production: observed, evaluated, governed, in-region."**
Wonderful leads with the genuinely-hard enterprise problem — getting agents *into production* and keeping them good (monitoring, evaluation, optimization at scale) — which reads as mature and credible. We have the spine to claim this honestly: the **Agent Run Console** (observable runs), **cross-plane `tool_action` audit**, **HITL approval gates**, and **cost-core**. So we lead with *production-credibility*, not a feature list. **Velion's twist:** for us "production" also means *provably in-region + ZDR + fully auditable* — production a regulated Norwegian/EU buyer can actually sign off on. **Honest gap (see Maturity):** continuous **evaluation/quality + cost dashboards** are roadmap — we build them next *because* this is the credible enterprise hook, not as an afterthought.

### Leverage map — Wonderful idea → what we already have → the Velion move

| Wonderful idea | What Velion already has | The Velion move |
|---|---|---|
| **"Get AI into production"** production-first narrative | Agent Run Console, `tool_action` audit, HITL, cost-core | Lead messaging with *pilot → production, observed & governed, **in-region*** |
| Named layered platform, *"a foundation that compounds across every deployment"* | CoreSystem planes + action registry + Model Plane + Trust Center | Publish a **customer-facing** layer diagram — **Trust/Residency · Agent-Runner · Grounding · Skills/Business-Context · Norwegian-Context** — tagline *"a sovereign foundation that compounds with every system you connect"* |
| **Business Context Layer** = "reusable agent skills grounded in your systems, knowledge, processes" | capability-core + skills + action-registry + Data Plane GraphRAG | Productise **"reusable grounded skills"**; sell the compounding moat — *every system you connect makes every agent smarter* |
| Built-in **observability + evaluation + optimization** | Model Plane feedback/eval primitives *(partial)*, audit | Ship a first-class **Ops/Quality surface** — accuracy/drift eval + cost dashboards *(roadmap → next)* |
| **Secure & Compliant / Visibility & Oversight** readiness grid | Trust Center, SSO, 2FA, roles, retention, residency | **Out-specify on governance** — approve/reject *per action* + *"Used by AI?"* per-category trail — but **do not claim to out-comply**: Wonderful is *not* generic here (SOC 2 II + ISO 27001/42001 + EU AI Act on a public Vanta page) |
| **Public Vanta Trust Center + formal certs** (SOC 2 II, ISO 27001/17/18, **ISO 42001**, EU AI Act, pentest) | In-app Trust Center + live controls, but **no certs / no public trust page yet** | **Close the gap** — stand up a public Trust Center + start the cert path; lead on **ISO 42001 / EU AI Act** as a sovereign-AI wedge (see *Trust & certifications roadmap*) |
| **Multi-model, multi-cloud** (no lock-in) | inference-core multi-provider fallback + Budget/Balance/Genius | State it plainly, then one-up: *"multi-model flexibility that never leaves the EU"* |
| Premium **"critical organizations"** enterprise brand | `velion-web` marketing site (in build) | Adopt a premium, restrained aesthetic; lead with *"built for organizations that can't compromise on where their data lives"* |
| Sales-led **"Get in touch"** only | AI-personalised onboarding (product-led) | Run **both** motions — product-led self-serve for the mid-market Wonderful ignores + enterprise "get in touch" for large NO/EU accounts |

**The frame:** *Wonderful = "AI everywhere, at scale" (broad · US · production-led). Velion = "sovereign AI that acts — governed and grounded — for Norway/EU" (deep · regional · trust-led).* Same category energy; we win on **where the data lives, how governed the action is, and who we'll actually serve.**
