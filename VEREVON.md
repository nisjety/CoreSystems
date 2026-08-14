# Verevon

> **Local Docker scope — 2026-08-03.** Current implementation work is local
> Docker work. A deployment claim means the updated images have been rebuilt,
> the local stack is running with migrations/configuration applied, and the
> relevant end-to-end flow has been verified. It does not mean Verevon has been
> released to external customers.

> **Current execution ledger — 2026-08-14.** For the current reconciled status
> and ordered implementation work across the six Verevon documents, use
> [verevon-roadmap.md](verevon-roadmap.md#current-execution-ledger--2026-08-14).
> The dated material below remains evidence and historical context; it is not
> a newer status assertion when it conflicts with that ledger.

> **Scope-and-operations update — 2026-08-14.** Verevon's next product
> contract is a first-class **Space**: one person, room, project, or case has
> one explicit collaboration authority context, agent context, work projection,
> activity history, and presence across supported surfaces. This is an active
> execution program, not a shipped claim. The canonical plan is
> [Verevon v3 × QM: comparison and adoption plan](apps/Frontend%20Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md).
> Space access never widens an owner-plane resource ACL: effective access is
> the intersection of current Space authority, active recipient audience, and
> the owner plane's current resource decision.

> **Local implementation checkpoint — 2026-08-14.** Commit `fa2d1eff` adds a
> source/test-backed owner-approved scheduled-run preparation contract across
> Control, Capability Core, Session Core, and Orchestrator. Control issues a
> short-lived, single-fire decision; Capability Core verifies it and passes its
> bearer only to Session Core; Session Core creates or reuses the exact
> service-owned deterministic thread; Orchestrator preserves that supplied
> thread. The canonical local launcher rebuilt and started the selected
> CoreSystem stacks without rotating existing credentials. This is **not** a
> release certificate or end-to-end scheduled-effect proof: live Control →
> Session → Temporal → receipt, revocation, ZDR, and rollback evidence remain
> open in the roadmap's R/S gates.

> **Quarry web/browser checkpoint — 2026-08-14.** Verevon's agent web
> capability is Quarry-native; Firecrawl is neither a runtime dependency nor
> an execution fallback. Model Plane plans and replans, BrowserBroker issues
> exact grants, and Quarry acquires pages, executes or rejects browser actions,
> and returns evidence and receipts. Local Chromium now has source- and
> runtime-backed per-session DNS-pinned egress plus fail-closed coverage for
> private/metadata destinations, HTTP redirects, frames, XHR/fetch, images,
> and script-triggered navigation. That promotion is deliberately local-driver
> only: Browserless, Browserbase, and Kernel remain ineligible for governed
> agent runs until they prove equivalent request-level containment. Artifact
> upload/download, dialog approval replay, and native AX/OOPIF stale-target
> behavior remain gated; provider action cost is unknown, so a requested
> `max_cost_usd` must be rejected rather than treated as enforced.

> **Production-readiness correction — 2026-07-13.** The 2026-07-10 “live” sections below are historical, not a current release certificate. Model Plane `model-gateway:9090` and `inference-core:9092` are absent while HTTP health remains green, so default inference/chat, tools that depend on inference, and Data Plane query embedding are currently unavailable. Live cost/session/capability boundaries remain unauthenticated, semantic memory search is down, and no Verevon Visma runtime integration exists. Significant authenticated/tenant-scoped source fixes, exact audience issuance, and the ordinary invoke caller graph pass source tests but are not deployed; approval/browser/Letta/background callers, a verified ZDR provider route, compatibility, and rollback gates remain incomplete. See [MODEL_PLANE_STATUS.md](apps/Model%20Plane/MODEL_PLANE_STATUS.md) and the [2026-07-13 audit](apps/Model%20Plane/docs/core-research/plane-audit-2026-07-13.md).

> **Product model correction — 2026-07-22.** Verevon is broader than customer support. The support inbox is a concrete wedge and proof workflow, not the category. Verevon is an organization intelligence-and-action platform: it connects internal knowledge, Norwegian public-data context, web evidence, business systems, agents, tools, and governed execution in one workbench. External customer-facing agents through Channel Plane and meeting intelligence are future source/distribution capabilities, not current runtime claims.

> **Status update — 2026-08-01.** The 2026-07-13 note above is itself now historical and must not be read as current: `model-gateway` and `inference-core` are **up and live**, not absent — this session drove real, streaming chat end-to-end (live Azure providers, tool loop, GraphRAG-fused retrieval) for hours against the running stack. Two chat-parity gaps closed this session: **resumable streams** (`#47`) — a client disconnect (closed tab, reload, network drop) no longer cancels the run; the producer detaches and keeps generating, persists the assistant message, and finishes the resume buffer, so a reconnect replays the complete answer instead of a stuck or truncated one — and **edit/regenerate version navigation** (`#49`), a client-side 1/N switcher over the final exchange's prior answers. Building the latter also surfaced and fixed a real, previously-undiscovered bug: Regenerate had been _appending_ a duplicate answer instead of replacing it since the feature shipped. Both are live-verified, not just source-reviewed; see `apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md`. Zero Data Retention is now precisely documented (not just "partial"): enforcement is complete and verified across all 6 Verevon-side durable boundaries (session-core threads/messages, Dreaming/agent-memory, response cache, implicit feedback, provider-side prompt cache, NATS/audit envelopes) — the ONE remaining gap is provider attestation (`AZURE_OPENAI_ZDR_CONFIRMED` / an Anthropic equivalent), which is a signed-contract-plus-operator-flip task, not an engineering gap; see `apps/Model Plane/docs/ZDR.md`. Also since 2026-07-22: agent-memory semantic recall was fixed (an embedding-dimension mismatch silently zeroed every search), the Auth Core service-principal registry was reconciled to the live fleet (14/14, no drift), and a 10-harness competitor study (Claude Code, Codex, OpenCode, ChatGPT, Perplexity, Manus, Hermes, OpenClaw, Pi, plus expert consensus) produced a ranked chat-parity backlog — see `apps/Model Plane/docs/VEREVON_CHAT_PARITY_BACKLOG.md`. This note does not re-certify anything below it that this session did not touch (IDOR, RLS, insight-core wiring, etc.) — those stand as last verified.

> **Architecture synthesis — 2026-08-03.** This document is synchronized with the latest code-aware improvement sources:
>
> - `/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026.md`
> - `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane/docs/MODEL_PLANE_IMPROVEMENTS_2026.md`
>
> The resulting authority split is explicit: **Quarry captures and verifies web evidence; Data Plane owns durable knowledge and organizational memory; Model Plane plans, reasons, selects capabilities, and proposes memory updates; Application/Control own human intent, identity, and review surfaces.** Mem0 is treated as a Data Plane design donor and benchmark target, not as Verevon's canonical memory authority.

**The Norwegian AI workbench that turns grounded intelligence into approved action.**

Verevon is both a product and the AI worker at its center: an EU/Norway-first, source-grounded, observable, and governed AI workbench for work across an organization. It connects company knowledge and business systems with Norwegian public data, web research, search, agents, tools, monitoring, inboxes, tickets, social operations, and briefs. A human can inspect and approve consequential actions before they are sent, published, changed, or stored. Grounding, approvals, residency, and retention are runtime properties that must be verified per request; they are not implied by the presence of a backend service. Verevon is delivered as a single workbench built on the **CoreSystem** multi-plane platform, with customer support as one useful wedge rather than the product definition.

## Historical live status — 2026-07-10

The Dockerized CoreSystem stack had 91 running containers and no unhealthy containers during the live verification. The authenticated Verevon v3 cross-plane Playwright smoke suite passed 6/6. The following distinctions are authoritative over older “live” wording in this document:

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

The market for "AI in your business" has settled into four paradigms. Verevon deliberately competes in only one of them:

| Paradigm                                 | Representative players              | What Verevon maps to                      |
| ---------------------------------------- | ----------------------------------- | ----------------------------------------- |
| **1. AI in the support inbox**           | Intercom Fin, Zendesk, Gorgias      | conversation-core-go (live)               |
| **2. Build-a-bot-on-your-docs**          | Chatbase, Zapier                    | not the bet — too shallow                 |
| **3. Observable autonomous task runner** | Relevance AI, Rox — **and Verevon** | execution-core + Agent Run Console (live) |
| **4. Answers / enterprise search**       | ayfie, partly Mimir                 | insight-core, Quarry search               |

**Verevon's category is #3 — the runner.** The strategic instruction is explicit: _beat Relevance/Rox, not Intercom._ A chatbot answers; Verevon **does the work** — observable and governed — and shows you every step.

### Verevon's wedge

Verevon's defensible position rests on the intersection of five things almost no single competitor combines:

- **Approvable execution.** Inline Approve/Reject gates on every risky tool call. Relevance offers only a coarse "Paused"; most competitors gate nothing. This is the governance moat.
- **Cost-awareness as an up-front control.** The Budget/Balance/Genius intent layer chooses the model _before_ it runs, downgrading under budget pressure. Relevance shows credits _after_; Chatbase/Zapier make you pick a raw model.
- **Deeper grounding.** Data Plane v2 GraphRAG + wiki + dense/sparse vectors + visual/multimodal embeddings + source-traces, versus "add a URL."
- **Owned browse + act.** Quarry executes real `browser_action`/`observation` timeline events under policy.
- **EU/Norway residency + ZDR + audit — _with_ actions.** ayfie/Mimir have the home-turf advantage but cannot _act_.

### The commercial frame

GTM leads **commercially** with the Norwegian wedge — Brreg-grounded intelligence, in-region data, GDPR posture — and demos the **observable agent-runner second**, as the moat and upsell. The four uncopyable pillars:

1. **Brreg/Enhetsregisteret-native resolution** — entities ground to the real Norwegian register (live-verified against AQUATIQ AS), not a hallucinated global record.
2. **EU/Sweden-Central data residency** — non-EU paths gated off by default.
3. **A wired GDPR/audit trust posture** — 2FA/SSO/passkey, cross-plane "Used by AI?" tool-action trail.
4. **The observable, approvable, cost-aware, grounded agent-runner** — a category where Verevon is ahead of point competitors.

**Target customer:** Norwegian and EU enterprises and SMBs. The proof story is one end-to-end demo — _Brreg resolution → monitor → brief → approve → act-in-region_ — put in front of a Norwegian design partner. Lighthouse dogfood customer: AQUATIQ AS.

### Product capability model

Verevon should be understood as five connected layers:

1. **Sources** — company documents, connected systems, websites, web evidence, Norwegian public data, and eventually channels such as meetings.
2. **Knowledge and context** — retrieval, GraphRAG, wiki, citations, source traces, entity resolution, and bounded live lookups.
3. **Reasoning and action** — chat, search, research, agents, tool calls, browser actions, MCP, plugins, and integrations.

> **2026-08-03 architecture clarification.** The original wording above is retained. The current ownership model adds the following precision:

> 2. **Knowledge and context** — retrieval, GraphRAG, wiki, citations, source traces, entity resolution, governed organizational memory, temporal validity, contradiction/supersession state, and bounded live lookups.
> 3. **Reasoning and action** — chat, search, research, agents, procedures/skills, tool calls, browser actions, MCP, plugins, and integrations.

4. **Governance** — permissions, policies, cost controls, approval gates, audit, retention, residency, and reversibility.
5. **Work surfaces** — dashboard, chat, knowledge, inbox, tickets, agent runs, social, studio, insights, and eventually externally deployed agents.

The core loop is **connect → understand → search → decide → act → approve → audit**. Customer support is one complete example of this loop; it is not the only one.

---

## What it can do

Verevon v3 has a **typed UX action registry**
(`apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts`) — each action carries an
ID, owner plane, risk, approval/reversibility metadata, and Zod input/output
schemas. It is valuable product metadata, but it is **not yet the one
executable cross-plane contract**: the BFF and owner planes still have
independent dispatch/validation paths, and Model eligibility is intentionally
actor-filtered. The target is one versioned owner-approved manifest with
actor-specific views: every eligible human or agent request receives the same
envelope/idempotency/receipt semantics, while human-only authority is never
advertised to or forgeable by the Model. Until S2 is complete, a registry entry
or tool description is not evidence that the underlying owner operation can
execute.

### Grounded answers — chat & inbox

- **Source-grounded chat** _(live, corrected 2026-08-01)_ — durable threads/transcripts over the Model Plane (`/v1/invoke`, streaming, **resumable** as of 2026-08-01, cancel), with live provider inference. **Normal chat now always requests knowledge grounding** (`knowledge_search`/`fetch_url`) — fixed 2026-07-28; it previously only attached when a client declared explicit tool specs, which plain chat never did, so the knowledge base was silently unreachable from ordinary chat. Every request also carries the caller's verified org/user identity as system context, so "we/our" resolves to the org and "I/my" to the user. A per-turn confidence badge is real (fixed 2026-07-29 — it had been hardcoded at 72% for every tool-grounded answer, showing a false "uncertain" caveat on every correct tool-sourced reply). A response cache now serves repeat questions from the real chat path (previously wired to a code path real traffic never used) — live-verified 7.7s→1.1s on a cache hit, zero tokens billed. Extracted memories show a provenance badge ("Inferred by Verevon") distinguishing what the user stated from what Verevon inferred. Thread pinning is real and server-owned (fixed 2026-08-01 — it was previously client-only dead code, and a sort-before-truncate bug had been silently deleting pinned threads past position 40).
- **Shared / team inbox** _(live)_ — conversation-core-go powers inboxes, conversations, messages, internal notes, status, and assignment, plus an `ai-actions` **HITL review queue** where the agent drafts replies for human approval. _(Backend is conversation-core-go, not Zendesk.)_
- **Ticketing** _(live)_ — create, classify, update, assign, link-resource, resolve.

### Knowledge & ingestion

- **Knowledge base** _(live)_ — a rich `LiveKnowledgePayload` fans out across documents, integrations, finspo, graph, retrieval, and Quarry; documents flow into Data Plane v2 retrieval/graph and an LLM wiki. The relationship graph (Graf tab) is an interactive 3D force-graph — the same battle-tested WebGL engine (with Canvas-2D fallback) the onboarding flow uses, not a bespoke layout — so entity clusters stay readable as the graph grows instead of piling nodes into an illegible mass.
- **Website crawl & scrape** _(live)_ — `/v1/map` discover → page-picker selection → durable `/v1/batch` with run-events SSE; **private-by-default** with selective ingest. Scrape-preview is hardened against bot-walls and timeouts. Content extraction is DOM-based readability — ARIA-landmark-aware (not just tag-name matching) with a responsive-duplicate collapse pass — so modern component-framework sites with no semantic `<nav>`/`<main>` and CSS-only mobile/desktop dual-rendering still yield clean chunks instead of nav-menu clutter, duplicated sections, and raw CDN image URLs.
- **Visual / multimodal grounding** _(live)_ — every successfully-ingested page, crawled or uploaded, is rendered to an image by Quarry's own headless Chromium (no external browser API/Browserbase dependency), stored content-addressably, and embedded via Cohere Embed v4 into a dedicated Qdrant collection that hybrid retrieval fuses in (`w_visual`) alongside BM25/dense/rerank. Governed by the same Zero Data Retention gate as text: a ZDR request is never rasterized, stored, or embedded.
- **Imports & connectors** _(live, subset)_ — Slack, Gmail, Notion, OneDrive, Outlook via integration-corev2.
- **SharePoint (Microsoft 365)** _(live)_ — finspo-core's Graph delta-sync captures real document **content**, not just metadata, into Data Plane v2. A source scopes to a whole document library, a single subfolder via a drill-down picker, or a site's Pages as its own source kind (Pages has no delta feed, so it does a full re-list pass, extracting `canvasLayout` text web parts to plain text).

### Organizational memory and verified outcomes

- **Memory provenance is live; Memory Intelligence is the next architecture layer.** Model Plane may extract or propose typed memory candidates, but Data Plane owns canonical memory records, embeddings, sparse/entity/temporal indexes, validity, supersession, contradiction links, retention, and deletion proof. Caller metadata never grants memory scope; Control Plane authority does.
- **Append-only evidence, governed active beliefs.** Verevon should preserve every observation and correction while maintaining an explicit active-state projection: `candidate → active → disputed → superseded/expired/revoked`. New facts must not merely compete with stale contradictions in vector ranking.
- **Verified agent outcomes become stronger memory than agent statements.** A proposal or approval is not a completed fact. Provider receipts plus deterministic postcondition verification may create a `provider_verified` memory candidate tied to its proof bundle.
- **The quality target is a verified organizational outcome.** Consequential runs should converge on a Verevon Proof Bundle containing evidence, procedure/plan version, authority and approvals, capability/browser receipts, observed effects, verification, cost, latency, residency, and retention. This bundle powers audit, replay, evaluation, customer proof, and safe learning.

### Search & browse

- **Web search** _(live)_ — Quarry-v2 SmartSearchRouter behind the gateway: web (with Exa-style filters — topic, time-range, include/exclude domains, exact-match), find-similar, images, video (SearXNG), answer-with-citations (SSE), did-you-mean + related queries + entity knowledge panel.
- **Precision layer** _(live)_ — Model-Plane LLM rerank with query-relevant highlights (degrade-safe), and autoprompt query rewriting for research/comparative intents.

### Norwegian public-data context

`information-core` adds a bounded, provenance-bearing Norwegian context layer for live read-only lookups. The locally verified tranche includes Kartverket address/property location, SSB metadata, Entur journeys, Storting representatives, Norges Bank series, MET weather, Statens vegvesen traffic/NVDB, NVE warnings, Riksantikvaren features, and Miljødirektoratet observations. News, weather, traffic, and address context can enrich ordinary work and agent runs without pretending to be company knowledge. Lovdata, DATEX II, Frost, eInnsyn, full Matrikkel/Grunnbok, Folkeregisteret, Maskinporten/Altinn data, and other restricted sources remain credentialed, source-only, blocked, or post-MVP according to their access and privacy requirements. See the [Norway data-source completion](apps/Application%20Plane/information-core/docs/norway-data-sources-completion-2026-07-21.md) and [audit](apps/Application%20Plane/information-core/docs/norway-data-sources-roadmap-audit-2026-07-20.md).

### Autonomous agent runs — with HITL approval & cost-awareness

- **Multi-tool agent loop** _(live, corrected 2026-08-01)_ — execution-core's real web, knowledge, shipping, provider, social, Brreg, weather, traffic, news, tracking, and MCP paths are no longer "blocked by inference": inference is proven live (see the status update above), and a P0 agent-loop-starvation bug is fixed — `MAX_TOOL_ROUNDS` was 3, leaving zero rounds to act on a self-describing MCP server's own tool-discovery calls; now 12 (configurable), with tool-result truncation, a 4× larger answer budget, and honest signaling when the tool phase is cut short. Live-verified before/after: the same question went from "3 calls and a question back to the user" to "self-corrects, grounded answer." **Delegated subagents were completely faked before 2026-07-28** (a canned "spawned" string, no execution) — they now really run (isolated context, depth-1 cap, budget charged to the parent).
- **Third-party tool connectivity via MCP** _(live, new 2026-07-28)_ — a real OAuth 2.1 + Dynamic Client Registration client lets model-gateway connect a third-party MCP server from nothing but its URL; tokens are encrypted at rest; real MCP Streamable HTTP transport replaced a prior "bridge" that was never built (no genuine remote MCP server could work at all before). Live-verified end-to-end: a real Visma Net ERP query answered through plain chat. Caveat: HTTPS-only (stdio/legacy SSE remain quarantined), only the streaming chat path, and token refresh is not yet load-tested under concurrent use.
- **Code interpreter + canvas artifacts** _(live, new 2026-07-30, one deploy gap open)_ — execution-core runs real sandboxed Python (and `sh`) via bwrap; canvas artifacts are genuine generated files (xlsx/docx/pdf via pandas/openpyxl/python-docx/reportlab), not a stub, with a real pageable version history and working downloads. **Open gap:** the capability-health scope these tools attest through is not yet granted in the deployed service-principal registry, so the self-attestation heartbeat can still be silently refused in production pending an operator/admin registry change.
- **Observable + approvable runs** _(real guarded path; current boundary unsafe)_ — `PermissionMode::Ask` pauses before a risky tool and persists approval. The running session-core gRPC approval surface is unauthenticated, so HITL is not globally safe. **A second, more specific gap, found 2026-08-02 in the gateway's own code comments**: model-gateway's `GetApprovalRequest`/`DecideApprovalRequest` carry no `org_id` at all, so the gateway's orchestration proxy (`apps/gateway/src/domains/orchestration.rs`) — despite verifying the caller's session and user id correctly — cannot enforce org-ownership on an approval beyond what model-gateway itself checks, which today is nothing. **Any authenticated user of the platform, from any org, who knows or guesses an `approval_id` can read or decide (approve/reject) another org's pending risky-tool-call approval.** Self-flagged in the source as a known, deliberately out-of-scope-for-the-proxy gap awaiting a session-core/model-gateway fix. Tenant/CAS containment and inline MCP denial are source-only fixes. Cron/workflow-fired runs are now visible (read-only) in the Agent Run Console (2026-08-01).
- **Smart model selection** _(live, corrected 2026-08-01)_ — `VerevonMode::Budget/Balance/Genius` blends heuristic task complexity with the org's cost-core budget posture. **This was completely inert from when it was built until 2026-07-29**: the budget-check call carried no Authorization header, 401'd on every single call, and permanently pinned posture to `Unknown` — no request was ever actually downgraded or throttled by budget. Now forwards the caller's own delegated token; confirmed live (`posture:"Healthy"` observed in logs).
- **Durable orchestration reachable end-to-end** _(fixed 2026-08-01, superseding earlier drafts of this line)_ — 7 registered Temporal workflows had zero production callers as of 2026-07-31 morning; by end of day, cron→run→learning is closed and live-verified (a real cron-fired run, claimed, completed, and consumed by the learning-review path with no human in the loop). This specifically unblocks the 3 `DeniesZDR` workflows — memory consolidation, skill promotion, feedback promotion — previously refused by identity (an internal/shared-secret caller can never satisfy a ZDR-denial gate; only a signed JWT carrying the `zdr` claim can, and that audience didn't exist in Auth Core until this fix).

### Onboarding intelligence

- **AI-personalized plan recommendation** _(live)_ — weighs connector count, sources, websites, employees (Brreg or operator head-count), website agent-brief, and governance signals → trial/team/enterprise, via an AI remote path with a local heuristic fallback. Brreg verification and graph-preview run live in onboarding.

### Insights & monitoring

- **Insights / analytics** _(partial — honest-empty)_ — insight-core overview + connectors; the SPA renders an **honest empty state** when upstream has no data — no fabricated metrics.
- **Monitoring / change-watch** _(live)_ — Quarry-v2 versioned change detection: check, latest, history, schedules, with changed-paragraph diff highlights.
- **Leads / Brreg lead builder** _(live, gated)_ — filtered Enhetsregisteret search, enrichment, governed `build_list`, saved org-scoped lists, CSV export; **company data only, never natural-person PII**; metered, audited, entitlement-gated. _(End-to-end use requires the "leads" billing entitlement.)_

### External agents and meeting intelligence

- **Channel Plane** _(docs-only future)_ — the intended runtime for deploying Verevon agents to websites, Shopify, WooCommerce, WordPress, and other external channels, with visitor identity, public conversations, handoff, and monitoring inside the Verevon workspace. No Channel Plane runtime exists today.
- **Meeting intelligence** _(docs-only future)_ — meetings are intended to become another source type: self-hosted transcription, diarized transcript documents, slide/keyframe evidence, graph entities, minutes, and approved follow-up actions. It reuses Ingestion, Data, Model, and Application Plane contracts; it is not a separate deployed product today.

---

## Where it works

### Surfaces

- **Internal AI agent + HITL inbox** _(live)_ — the production loop: agent drafts, human approves, action executes.
- **Internal knowledge work** _(live)_ — chat, search, knowledge base, studio.
- **Agent Run Console** _(live)_ — `/agents/runs`, observable and approvable.
- **Website chatbot** _(partial → roadmap)_ — `agents.deploy_channel` and a `chatbot/runtime` config endpoint exist, but there is **no live embeddable end-customer widget** yet; the public `/embed` funnel is a scoped, origin-allowlisted roadmap item.

SPA routes: `/chat`, `/inbox`, `/tickets`, `/knowledge` (+ `/shared`), `/ingestions`, `/agents` (+ `/runs`), `/insights/*`, `/leads`, `/social/*`, `/studio/*`, `/settings`, `/onboarding`.

### Integrations & connectors

Slack, Gmail, Notion, OneDrive, Outlook (live) via integration-corev2; **SharePoint (live, content-capturing)** via finspo-core Graph delta-sync — whole-library, single-folder, or site-Pages scope. Providers are returned dynamically by integration-core, not hardcoded. _(Zendesk connector is roadmap.)_

### Norwegian-market fit

- **Brreg/Enhetsregisteret company lookup** — live, classified `public_non_personal`, proven against AQUATIQ AS (org 983 515 827).
- **Non-PII lead-builder** (leads-core) — company-only, metered, audited.
- **Bokmål UI** _(partial)_ — `i18n/no.ts` is the single-source-of-truth seam; strings migrate incrementally.
- **EU model routing** — Azure OpenAI Sweden Central default.
- **Norwegian public-data context** — bounded, source-attributed lookups across official Norwegian providers, with restricted and credentialed sources kept behind explicit access gates.

---

## The platform underneath — CoreSystem

CoreSystem is a multi-plane monorepo with **downward-flowing authority.** Each plane owns one thing and crosses boundaries only through contracts.

### Control Plane — _identity & governance_

Authority root for identity, users, orgs, billing, sessions, audit, quotas, and entitlements. Services: auth-core (NestJS JWT/session/token authority), user-core, org-core, billing-core, session-core, audit-core.

### Data Plane v2 — _durable knowledge_

The **only** owner of documents, chunks, embeddings, retrieval, GraphRAG, LLM wiki, and source traces. Services: documents-api-go, index-engine-rs, embedding-engine-rs (text + visual/Cohere Embed v4), retrieval-engine-rs (hybrid BM25 + dense + rerank + visual fusion), graph-index-rs, wiki-store-go, data-orchestrator-go, data-quality-go, quickwit-adapter-rs.

> **2026-08-03 architecture clarification.** The original wording above is retained. The current ownership model adds the following precision:

> The **only** owner of documents, chunks, embeddings, retrieval, GraphRAG, LLM wiki, source traces, and canonical organizational memory (provenance, validity, supersession, contradiction state, retention, and derived-index deletion). Services: documents-api-go, index-engine-rs, embedding-engine-rs (text + visual/Cohere Embed v4), retrieval-engine-rs (hybrid BM25 + dense + rerank + visual fusion), graph-index-rs, wiki-store-go, data-orchestrator-go, data-quality-go, quickwit-adapter-rs.

### Ingestion Plane — _evidence capture_

Captures evidence and persists durable knowledge **only through Data Plane contracts.** Quarry-v2 is the source of truth: quarry-edge, quarry-runtime, quarry-browser (CDP action runtime), quarry-control, quarry-orchestrator (Temporal). Plus imports-core, integration-corev2, finspo-core, autocomplete-core.

> **2026-08-03 architecture clarification.** The original wording above is retained. The current ownership model adds the following precision:

> Captures and verifies web evidence and persists durable knowledge **only through Data Plane contracts.** Quarry-v2 owns runtime selection, typed browser actions, observations, adaptive target memory, challenge classification, deterministic postconditions, change intelligence, and browser proof receipts—not organizational knowledge or agent planning. Quarry-v2 is the source of truth: quarry-edge, quarry-runtime, quarry-browser (CDP action runtime), quarry-control, quarry-orchestrator (Temporal). Plus imports-core, integration-corev2, finspo-core, autocomplete-core.

### Model Plane — _reasoning & execution_

Owns reasoning, sessions/runs, inference, the execution loop, capabilities, sandboxes, browser grants, and cost. Rust: model-gateway, session-core, inference-core, execution-core. Go: orchestrator-core (Temporal), capability-core, sandbox-manager, browser-broker, cost-core, bridge-core.

> **2026-08-03 architecture clarification.** The original wording above is retained. The current ownership model adds the following precision:

> Owns reasoning, sessions/runs, inference, the execution loop, capability and procedure/skill selection, memory-write proposals, adaptive model/context routing, sandboxes, browser grants, and cost. It does not own canonical knowledge or browser execution. Rust: model-gateway, session-core, inference-core, execution-core. Go: orchestrator-core (Temporal), capability-core, sandbox-manager, browser-broker, cost-core, bridge-core.

### Application Plane — _realtime, context & collaboration_

Owns collaborative/realtime workspace projections, notifications, bounded contextual lookups, conversations, social, and leads — **not** identity, billing, durable knowledge, ingestion, or reasoning. Convex stack, affine-core/runtime, conversation-core-go, information-core, notification-core, leads-core, insight-core, and social-core.

### Frontend Plane — _Verevon + BFF seam_

Verevon v3 is a SolidJS SPA whose **only** path to the planes is the Rust BFF gateway (`verevonv3/apps/gateway`, deployed as verevon-gateway-rs). The gateway exposes ~50 per-plane domain modules (auth, chat, knowledge, ingestions, agents, leads, ownership, privacy, billing, search, orgs, settings, social, insights, monitoring, mcp, finetune, …) plus the onboarding flow, normalizes everything to typed envelopes, and **must never leak upstream secrets or raw OAuth tokens.**

### Channel Plane — _docs-only future_

Reserved for future external-agent deployment (adapter-core, widget-core, public visitor runtime, and channel adapters). **No runtime exists today — the plane is documentation only; do not market or build against it as if deployed.**

### Tech stack by job

- **Rust** — latency/CPU/parsing/retrieval/browser/protocol hot paths (Quarry runtime, Data engines, Model gateway/session/inference/execution, the BFF gateway).
- **Go** — durable workflow, CRUD, registry, policy, scheduling, grants, billing, operator control.
- **TypeScript/TSX** — Verevon frontend + BFF, NestJS auth-core, Convex functions.
- **Python** — imports, labs, evals, provider glue only (no production hot path).
- **Infra** — Postgres, Redis/Dragonfly, NATS/JetStream (the cross-plane event spine on a shared `inter-plane-bus`), Qdrant, MinIO, Temporal, Quickwit, Convex.

### Non-negotiable cross-plane rules

1. **No direct database crossing** — Model and Quarry consume Data Plane APIs, never its Postgres/Qdrant.
2. **No independent embeddings/reranking** outside isolated eval labs.
3. **Model proposes, Quarry executes (or rejects)** browser actions — no agent bypass around Quarry policy.
4. **Zero Data Retention propagates** across every content-persisting boundary.
5. **GDPR policy metadata** (purpose, lawful basis, retention, residency, privacy class, third-party processing, deletion scope) travels with durable records and jobs.
6. **Memory scope is authority, not metadata** — Control resolves the subject/org scope; Data Plane enforces it and owns every durable and derived copy.
7. **No false success** — an agent statement, browser click, or provider 2xx is not completion until the expected external state is independently verified or explicitly marked unknown.
8. **Every consequential outcome is reconstructable** — runs carry a Runtime Evidence Manifest and may emit a Verevon Proof Bundle linking source, model/harness, capability/browser runtime, authority, effects, verification, cost, and residency.

---

## Ultimate position — Norway-first, full GDPR & data residency

This is the strongest part of the story, and the one held to the strictest honesty gate: the compliance controls **are** the product.

### EU/Norway data residency

- **Residency Tier 1 (live)** — primary inference _and_ embeddings run on **Azure OpenAI Sweden Central** (EU/EEA). Every `StartRun` stamps a `residency` value (default `swedencentral`). App data, conversations, run-history, audit, and the vector store are EU/EEA Verevon-controlled stores.
- **Non-EU TTS gated off by default (live)** — text-to-speech historically ran in East US 2. `speech.rs` now **defaults to the EU endpoint and refuses to use a configured non-EU TTS endpoint** unless `MODEL_PLANE_ALLOW_NON_EU_TTS=true` (default `false`); when the flag is off, a set `AZURE_OPENAI_TTS_ENDPOINT` is ignored and voice stays on the EU `AZURE_OPENAI_ENDPOINT`.
- **Residency Tier 2 (Norway East)** _(roadmap)_ — documented but conditional. **Honesty bound:** we do **not** claim "data stays in Norway" until a norwayeast deployment is evaluated.
- **Sovereignty caveat (disclosed):** EU residency is **not** data sovereignty. A US-headquartered subprocessor (including Microsoft Azure) remains reachable under the US CLOUD Act regardless of physical location. This is logged as a disclosed residual risk in the Schrems II transfer assessment with supplementary measures — never claimed as immunity.

### Zero Data Retention (partial at model layer)

The inference cache correctly skips reads and writes for `req.zdr`, and current gateway/inference source makes issuer-required ZDR monotonic for unary/SSE infer/embed. That is not end-to-end ZDR: session replay classification, compaction, memory, traces, tool I/O, Data Plane grounding, external bridges, provider eligibility, and non-infer/embed modality contracts remain unproved or incomplete. A request whose issuer requires ZDR must not be described as ZDR if Verevon retains run history/content under ordinary retention. Only the Azure OpenAI Sweden Central path is presently classified ZDR+EEA; no compliant live end-to-end proof exists. The newer visual-RAG page-image arm follows the same discipline — rendering, CAS write, and embed-event emission are all skipped outright when the request is ZDR — one more modality correctly gated, not a closure of the broader gap above.

### In-infra / 0-SaaS search (partial)

Quarry-v2 SmartSearchRouter tries a free in-infra chain first — **Tantivy → Stract → SearXNG** — and only falls back to paid external SERPs (Brave/Serper) as a last resort. Omit the paid keys and you get **zero external egress** by routing order and config. _(A hard policy-enforced "0-SaaS / no-egress" toggle is roadmap; today it is configuration-driven.)_

### The GDPR control set

- **Right-to-erasure (partial)** — `gdpr_hard_delete_user` and `gdpr_anonymize_user` primitives are live in user-core (with handlers and tests) and now _called_ by user-core and org-core via admin/owner-gated, audited, `confirm:true` endpoints, emitting a `verevon.gdpr.erasure.requested` event. The cross-plane subscribers that delete derived copies in Model/Data/Application/Ingestion are **roadmap**.
- **DSAR / export (partial)** — an Art. 15 export path exists internally; a customer-facing intake endpoint is being built (not yet GA).
- **Retention janitors (partial)** — audit-core runs a real sweep purging audit + usage events older than `AUDIT_RETENTION_DAYS` (365) every 24h; org-core has a purge cron. Other TTLs (run-history, conversations, crawl artefacts, telemetry, backups) are _proposed_ — sweeps TBC.
- **Audit log incl. cross-plane `tool_action` (live)** — audit-core dual-subscribes to the Model-Plane NATS; `GET /api/v1/audit?event=tool_action` returns `{event, plane:model, details:{data_category, zdr, tool}}`, feeding the Trust Center "Used by AI?" column.

### Tenant isolation & per-user ownership

- **Org-axis isolation (live)** — membership re-check guards fail closed on mutation routes; the gateway derives org from the session (IDOR-clean).
- **Postgres RLS backstop (partial)** — delivered but **inert by default**, enabled only via an explicit `ALTER DATABASE … SET app.org_rls_enabled='true'` opt-in at the database layer. Defense-in-depth, not yet active; deep RLS + per-core keys are a SOC2-era fast-follow.
- **Per-user private-until-shared (partial — substrate live, claim not yet shippable)** — a single grant authority (`resource_grants`), `documents.owner_id` + `visibility ∈ {private, org, shared}`, filter-at-source on GET/LIST, and a retrieval **post-filter** gating dense + sparse + wiki are all live. But everything still defaults to `org`, graph grounding and the ExecuteStep gRPC carry no `user_id`, and the Qdrant/Quickwit pre-filters are inert. **Absolute rule:** no "Private" badge or "the AI only sees your data" copy renders until all release-gate conditions hold, including a four-path honesty test.
- **Privacy data classification (live)** — a 6-class taxonomy (`public_non_personal`, `customer_private`, `personal`, `sensitive_personal`, `credential_or_secret`, `zdr_ephemeral`) governs storage/indexing/sharing, with **default-deny** third-party processing for protected classes.

### Trust Center, SSO & 2FA (live)

The Trust Center surfaces App / Permissions / Data fetched / **Used by AI?** / Retention / Last sync / Disconnect, fed by integrations + the audit `tool_action` aggregation. SSO sign-in and 2FA enrollment are wired with gateway proxies. Gateway rate-limiting (token-bucket, 429 + Retry-After, org→user→IP) and security headers (via nginx + tower-http) are live; **CSP is Report-Only today** (partial). _(Trust Center per-connection Retention is a placeholder.)_

### Where Verevon's trust wedge is real — and where it must be earned

**Be precise (and honest).** The lazy claim _"US vendors leak your data, we don't"_ does **not** survive contact with a mature competitor. [wonderful.ai](https://trust.wonderful.ai), for example, offers **EU subprocessor regions** (AWS/GCP/Azure/OpenAI listed "USA, EU") and holds **SOC 2 Type II + ISO 27001/17/18/27799 + ISO/IEC 42001 + an EU AI Act risk assessment** on a public **Vanta** Trust Center. And Verevon itself runs on **Azure** — so "your data stays in the EU" is achievable by both, and _neither escapes the US CLOUD Act by region alone_ (we disclose this above).

Verevon's **defensible** trust differentiators are therefore narrower and sharper than "we're more compliant":

- **ZDR enforced by default at the model layer** (in code) — not an after-the-fact contract clause.
- **EU-resident _by default_** (Sweden Central) — not "EU region available on request."
- **Norwegian-native** grounding (Brreg/Enhetsregisteret + Bokmål) — a regional fit a US platform won't build.
- **0-SaaS / in-infra search** — queries needn't leave our infrastructure.
- **Per-action approval + per-category "Used by AI?" audit** — concrete governance, not a generic "oversight" tile.
- **Product-led accessibility** for the mid-market that enterprise-sales-only competitors ignore.

Against the Norwegian incumbents (ayfie/Mimir) the edge is simpler: home-turf trust, but they **cannot act**.

### Trust & certifications — the honest gap and the roadmap

The uncomfortable truth a _trust-led_ positioning must own: **today Verevon has the controls but not the credentials.** The controls above are real and largely live, but we hold **no third-party certifications yet** and have **no public, prospect-facing Trust Center** — only the in-app one. On procurement-grade trust, a Vanta-backed competitor with SOC 2 Type II + ISO 42001 currently _out-documents_ us. Closing this is a priority, not a footnote.

| Trust asset                                                                      | Status                    | Plan                                                                                                                             |
| -------------------------------------------------------------------------------- | ------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| In-app Trust Center ("Used by AI?", retention, audit)                            | **live**                  | keep; feed from the cross-plane `tool_action` trail                                                                              |
| **Public** Trust Center (subprocessors, controls, data-flow map, request-access) | **roadmap**               | stand one up (Vanta/SafeBase/Drata or custom on `verevon-web`); publish a tighter, EU-default subprocessor list                  |
| SOC 2 Type II                                                                    | **roadmap**               | open the observation window; continuous controls                                                                                 |
| ISO 27001                                                                        | **roadmap**               | ISMS scoped to the platform                                                                                                      |
| **ISO/IEC 42001** (AI management) + **EU AI Act** readiness                      | **roadmap — and a wedge** | _more native to a sovereign EU AI platform than to a US one_; aim to be the ISO-42001 / EU-AI-Act-ready Norwegian agent platform |
| Independent penetration test (published)                                         | **roadmap**               | web-app/API pentest report                                                                                                       |

Until these land, sell the **defensible** differentiators above honestly — and treat the certification path + a public Trust Center as the unlock for trust-led enterprise deals.

---

## Maturity & honesty

Verevon is **real, not a demo.** Reality score: **~72% production-real (backend planes ~85%).** The SolidJS SPA reaches every plane only through the Rust BFF gateway, with zero mock modules and ~40 typed gateway clients. The framing is deliberate: **the engine is built — most remaining high-leverage work is WIRING already-built backends through the gateway, not net-new construction.** The honesty gate is load-bearing — we never represent a control as in place without engineering confirmation.

| Status        | Capability / control                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Live**      | Source-grounded chat; shared inbox + HITL ai-actions queue; ticketing; knowledge base + ingestion (clean DOM-based readability extraction; visual/multimodal grounding via Cohere Embed v4, local Chromium render, ZDR-gated; 3D relationship graph); SharePoint real-content sync with whole-library/folder/site-Pages scoping; private-by-default Quarry crawl; web search (rerank, highlights, autoprompt, find-similar, did-you-mean, entity panel, video); multi-tool agent loop + Agent Run Console + approvable HITL gate; Budget/Balance/Genius selection + cost-core budget; AI-personalized onboarding + Brreg verification; Brreg company_lookup; monitoring/change-watch; EU residency (Sweden Central) + residency stamp; non-EU TTS gated off; model-layer ZDR; erasure primitives (now called); audit incl. cross-plane tool_action; org-axis isolation; privacy data-classification; in-app Trust Center + SSO + 2FA; gateway rate-limiting; social publishing; router-policy + fine-tune; per-user ownership substrate |
| **Partial**   | Customer-facing chatbot deploy (config only, no live widget); deeper multi-step run engine (MVP dispatch live); leads (end-to-end needs the billing entitlement); insights (honest-empty until data accrues); scrape anti-bot infra; Norway-East residency; ZDR across all vendors; in-infra-first search (config, not hard gate); cross-plane erasure fan-out; customer-facing DSAR intake; RLS backstop (inert/gated); private-until-shared as a _claimable_ guarantee; CSP enforcing; Bokmål coverage; ZDR/GDPR-metadata propagation; Trust Center retention column                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| **Roadmap**   | Embeddable end-customer chat widget (`/embed`); Brreg-seeded lead/list builder as the net-new monetization bet; real WorkflowBuilder persistence/execution; multi-tenant X-Org-ID fully consulted across all cores; "0-SaaS / no-egress" hard toggle; retention sweeps for non-audit stores; Zendesk connector; **public Trust Center**; **third-party certifications** (SOC 2 Type II, ISO 27001, ISO/IEC 42001, EU AI Act assessment) + independent pentest                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| **Docs-only** | Channel Plane runtime (adapter-core, widget-core, public visitor conversation runtime); compliance documentation pack (drafted, counsel-review pending)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

**Cut, not deferred** — meeting notes, podcast/video repurposing, AI phone answering, an AI app/workflow builder, a Penpot-class design canvas, SEO/content briefs. Breadth is how a small team dies; the wedge is depth.

---

## Competitive strategy — what we take from the horizontal platforms (Wonderful.ai)

[Wonderful.ai](https://www.wonderful.ai/) is the sharpest exemplar of the _horizontal, US-style, production-led_ enterprise agent platform: **"Get AI into production, everywhere it matters"**, run across Support · Sales · HR · back-office, on a named layered stack (Management / Orchestration / Business-Context / Infrastructure) that "compounds across every deployment." We will **not** out-broaden or out-fund it. We win by sharpening our wedge — **sovereignty + depth + governed action + accessibility** — _and_ by borrowing its two strongest narrative moves, tailored to Verevon.

### Two narratives to adopt — tailored to Verevon specifics

**1 — "Run _sovereign_ AI across your operations": breadth as vision, depth as commitment.**
Wonderful's _"Run AI across your enterprise"_ (customer · employee · back-office) is a bigger-TAM story than our support+knowledge focus. We do **not** chase that breadth now — a small team dies on breadth (see Cut list). But the same engine we already ship — the typed **action registry**, **execution-core** agent loop, and **Model Plane** — generalises beyond support. So we adopt breadth as a **horizon vision**, not a roadmap commitment: _"operationalise sovereign AI across customer, knowledge, and back-office operations — in-region."_ Every **shipped** capability stays deep in the support/knowledge wedge; the breadth lives on a "where this goes" slide. **Verevon's twist Wonderful can't copy:** the breadth is _sovereign_ — EU/Norway-resident, Brreg-grounded, ZDR — so "AI across your operations" means _across your operations without your data leaving the region._

**2 — "From pilot to production: observed, evaluated, governed, in-region."**
Wonderful leads with the genuinely-hard enterprise problem — getting agents _into production_ and keeping them good (monitoring, evaluation, optimization at scale) — which reads as mature and credible. We have the spine to claim this honestly: the **Agent Run Console** (observable runs), **cross-plane `tool_action` audit**, **HITL approval gates**, and **cost-core**. So we lead with _production-credibility_, not a feature list. **Verevon's twist:** for us "production" also means _provably in-region + ZDR + fully auditable_ — production a regulated Norwegian/EU buyer can actually sign off on. **Honest gap (see Maturity):** continuous **evaluation/quality + cost dashboards** are roadmap — we build them next _because_ this is the credible enterprise hook, not as an afterthought.

### Leverage map — Wonderful idea → what we already have → the Verevon move

| Wonderful idea                                                                                              | What Verevon already has                                                         | The Verevon move                                                                                                                                                                                                                       |
| ----------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **"Get AI into production"** production-first narrative                                                     | Agent Run Console, `tool_action` audit, HITL, cost-core                          | Lead messaging with _pilot → production, observed & governed, **in-region**_                                                                                                                                                           |
| Named layered platform, _"a foundation that compounds across every deployment"_                             | CoreSystem planes + action registry + Model Plane + Trust Center                 | Publish a **customer-facing** layer diagram — **Trust/Residency · Agent-Runner · Grounding · Skills/Business-Context · Norwegian-Context** — tagline _"a sovereign foundation that compounds with every system you connect"_           |
| **Business Context Layer** = "reusable agent skills grounded in your systems, knowledge, processes"         | capability-core + skills + action-registry + Data Plane GraphRAG                 | Productise **"reusable grounded skills"**; sell the compounding moat — _every system you connect makes every agent smarter_                                                                                                            |
| Built-in **observability + evaluation + optimization**                                                      | Model Plane feedback/eval primitives _(partial)_, audit                          | Ship a first-class **Ops/Quality surface** — accuracy/drift eval + cost dashboards _(roadmap → next)_                                                                                                                                  |
| **Secure & Compliant / Visibility & Oversight** readiness grid                                              | Trust Center, SSO, 2FA, roles, retention, residency                              | **Out-specify on governance** — approve/reject _per action_ + _"Used by AI?"_ per-category trail — but **do not claim to out-comply**: Wonderful is _not_ generic here (SOC 2 II + ISO 27001/42001 + EU AI Act on a public Vanta page) |
| **Public Vanta Trust Center + formal certs** (SOC 2 II, ISO 27001/17/18, **ISO 42001**, EU AI Act, pentest) | In-app Trust Center + live controls, but **no certs / no public trust page yet** | **Close the gap** — stand up a public Trust Center + start the cert path; lead on **ISO 42001 / EU AI Act** as a sovereign-AI wedge (see _Trust & certifications roadmap_)                                                             |
| **Multi-model, multi-cloud** (no lock-in)                                                                   | inference-core multi-provider fallback + Budget/Balance/Genius                   | State it plainly, then one-up: _"multi-model flexibility that never leaves the EU"_                                                                                                                                                    |
| Premium **"critical organizations"** enterprise brand                                                       | `verevon-web` marketing site (in build)                                          | Adopt a premium, restrained aesthetic; lead with _"built for organizations that can't compromise on where their data lives"_                                                                                                           |
| Sales-led **"Get in touch"** only                                                                           | AI-personalised onboarding (product-led)                                         | Run **both** motions — product-led self-serve for the mid-market Wonderful ignores + enterprise "get in touch" for large NO/EU accounts                                                                                                |

**The frame:** _Wonderful = "AI everywhere, at scale" (broad · US · production-led). Verevon = "sovereign AI that acts — governed and grounded — for Norway/EU" (deep · regional · trust-led)._ Same category energy; we win on **where the data lives, how governed the action is, and who we'll actually serve.**
