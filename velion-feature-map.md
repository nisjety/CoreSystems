# Velion Feature Map — Honest Deep-Dive Audit

> **2026-07-20 correction (same-day, later pass):** the "one confirmed
> cross-tenant IDOR (`x-velion-org-id`, 17+ call sites) is open" claim in the
> paragraph below is stale. A fresh source-level re-audit of
> `apps/gateway/src/middleware.rs` and `upstream.rs` on `main` (not the dirty
> working tree) found the header IDOR closed: `x-velion-org-id`/`x-org-id` are
> globally stripped at ingress (`STRIPPED_HEADERS`, applied as an axum layer
> before any handler), org is derived only from the live-verified session
> membership (`upstream::authorized_org_id`), that accessor is used at 107 call
> sites across 31 domain files (a superset of the 17+ sites this claim cites),
> and dedicated regression tests (`strips_forged_org_scoping_headers`,
> `stripped_headers_includes_velion_org_id`, plus IDOR-regression tests in
> `main.rs`) assert a forged header is never forwarded. This does not touch the
> separate, still-open `onboarding/graph-preview ?org_id=` query-param IDOR
> described elsewhere in this doc and in `FRONTEND_PLANE_ROADMAP.md` — that is a
> different vector and was not re-verified in this pass.

> **2026-08-01 correction (chat engine + two live gates closed).** §1.2's "one live probe settles it" and §2 item 2's "verify with a real inference call" are done: this session drove real, streaming chat end-to-end for **hours** against the running stack (live Azure providers, tool loop, GraphRAG-fused retrieval) — not a smoke test, direct interactive use. This is a manual proof, not a CI gate — §1.2's "smoke test in CI" done-enough-gate item remains open. Two chat-parity gaps also shipped and were live-verified: **resumable streams** (a disconnect no longer cancels the run — the producer detaches, keeps generating, persists, and finishes the resume buffer, so a reconnect replays the complete answer) and **edit/regenerate version navigation** (a client-side 1/N switcher over an exchange's prior answers), the latter surfacing and fixing a real standing bug where Regenerate silently appended a duplicate answer instead of replacing it. §1.2's "Semantic memory degraded (`DEGRADED_SEMANTIC_UNVERIFIED`)" line is also stale — fixed (an embedding-dimension mismatch was silently zeroing every search; readiness now correctly reports degraded only when the backend has never returned a hit). ZDR is more precisely scoped than §1.2's "may block external inference" framing: the interactive-retention-policy exception §6.9 already resolved on 2026-07-21 is a different gap from the one that remains — enforcement is complete and verified across all 6 Velion-side durable boundaries, and the sole open piece is **provider attestation** (a contract-plus-operator-flip, not code); see `apps/Model Plane/docs/ZDR.md`. Full detail on the two shipped features: `apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md`. New artifact: a 10-harness competitor study (Claude Code, Codex, OpenCode, ChatGPT, Perplexity, Manus, Hermes, OpenClaw, Pi + expert consensus) — `apps/Model Plane/docs/VELION_CHAT_PARITY_BACKLOG.md`. This note certifies only Model Plane chat-engine liveness and the two shipped features — it does not re-verify IDOR, RLS, insight-core wiring, credential rollout, image provenance, or anything else in §2/§3, which stand as last audited.

**Date:** 2026-07-20
**Method:** Full read of all 26 listed docs (6 parallel research streams), code-level audit of velionv3 (routes, feature folders, API clients, all ~49 gateway domains), live competitor research (11 companies), and a 4-perspective council (pragmatic CTO, security/compliance, product/GTM, delivery-risk) on sequencing. No code was changed. This is an audit, not a promise.

**How to read this:** Features are sorted by how far along they actually are — verified against code, not doc claims. Each feature ends with a "done-enough gate": what must be true before it works correctly and the team moves to the next thing. The cross-cutting blockers section matters more than any single feature — read it first if you read only one section.

---

## 0. The one-paragraph truth

Velion v3 is an organization intelligence-and-action workbench, not only a customer-support assistant. Its product loop connects company documents and systems, Norwegian public-data context, websites and web evidence to a durable knowledge layer; lets people and agents search, reason, call tools, and prepare work; and keeps consequential execution observable and approval-aware. **9 of 11 frontend surfaces are genuinely wired to real gateway domains with a documented honesty contract** (neutral empty states, self-labeled previews, no fabricated numbers), while the underlying planes provide deeper retrieval, graph, ingestion, agent, inbox, ticket, social, and contextual-data capabilities. The support inbox is a concrete wedge, not the category. Channel Plane external agents and meeting intelligence expand the same substrate in the future, but are docs-only today. **Runtime correction (verified live 2026-07-20):** the docs' 2026-07-13 "hot path down / Docker stopped" claims are stale — `docker ps` shows **94 containers up, zero unhealthy/restarting**, including model-gateway, inference-core, all of Quarry, shipping-core, velion-gateway-rs, and the SPA; the Lagring volume has 116 GiB free. What remains true regardless of green health: **image provenance is unproven** (no SHA tags / `/version` endpoints, so runtime cannot be correlated to the hardened source), health checks are shallow, the credential rollout ritual is undone, and production promotion still needs deep smoke tests, tenant/security evidence, ZDR decisions, and scope discipline. Treat the docs' ~72% "production-real" figure as dated and verify runtime claims against the live host in both directions.

### 0.1 Product model correction — 2026-07-22

Velion has five connected capability layers:

1. **Sources** — company documents, connected systems, websites, web evidence, Norwegian public data, and future sources such as meetings and public conversations.
2. **Knowledge and context** — hybrid retrieval, GraphRAG, wiki, citations, source traces, entity resolution, change monitoring, and bounded live lookups.
3. **Reasoning and action** — chat, search, research, agents, browser actions, tool calls, MCP, plugins, and system integrations.
4. **Governance** — permissions, policies, cost controls, approval gates, audit, retention, residency, and reversibility.
5. **Work surfaces** — dashboard, chat, knowledge, inbox, tickets, agent runs, social, studio, insights, and future external channels.

The core loop is **connect → understand → search → decide → act → approve → audit**. Customer support is one complete proof workflow for this loop, not the definition of the product.

---

## 1. Feature-by-feature map (sorted: furthest along first)

### 1.1 Knowledge Base — furthest along, and the crown jewel

**Vision:** internal database that connects data and makes it available to Velion's AI (mnemon / cognee / contextual.ai class).

**What verifiably exists**
- UI: `src/features/knowledge/` — overview, operating map, **3D force-graph** of live knowledge nodes, chunks/retrieval inspector, wiki pages, document management, imports/upload, share dialogs via `ownership-client`. All backed by `knowledge-live-client` + `knowledge-client` (~1,270 LOC combined), zero mocks.
- Backend (Data Plane v2, source + isolated verification): hybrid retrieval (Qdrant dense + Quickwit/Postgres sparse with fallback), retrieval traces with actor attribution, context packing, **GraphRAG** (entities/relationships/claims/communities in `graph-index-rs`), full wiki store (versions, backlinks, proposals, diffs, source logs), knowledge-unit extraction pipeline over NATS, document ACL gRPC in user-core for per-user ownership/sharing. retrieval-engine: 212 tests pass; real-authority browser E2E 2/2 — one user sees only their own knowledge.
- This is exactly the "connections between data + source traces" ambition — it is not aspirational, it exists in source.

**Honest gaps**
- `AUTHCTX_ENFORCE=0`: documents-api verifies JWTs in **observe mode** — header trust is still live in practice.
- The Model Plane embedding gRPC hop is broken/worked-around: embedding + retrieval fall back to **direct Azure HTTP**, which violates the "no independent embeddings outside Model Plane" architecture rule and blocks a self-hosted RAG story.
- Strict mutation telemetry for Postgres/Qdrant/Quickwit/MinIO missing (release-evidence gap); several job/eval packages 27–53 % coverage; `retrieval-eval-py` is an empty service (decide: build or delete).
- Cross-plane GDPR erasure: event emitted, **Data Plane purge subscriber partially unbuilt** — erasure does not yet actually purge chunks/embeddings/traces.
- Nothing here is deployed to a production-shaped environment.

**Competitor bar (Ayfie):** clickable per-answer sources ✅ (traces exist), permission-aware retrieval ✅ (in source), EU residency statement ✅ (Sweden Central), on-prem ❌ (not needed for SMB pilot).

**Done-enough gate:** deployed with `AUTHCTX_ENFORCE=1`, embedding hop through inference-core proven (or the Azure fallback consciously accepted and documented for the pilot), one real org's crawl+docs corpus retrievable with sources in <15 min from signup.

---

### 1.2 Home Dashboard (AI composer / Search / Crawl) + Chat page — UI done, engine not plugged in

**Vision:** an organization intelligence workbench that combines internal knowledge, Norwegian public-data context, web search, agentic research, crawling, and approved system actions.

**What verifiably exists**
- Composer (`DashboardComposer.tsx`, part of the 22-file / ~11.9k-LOC dashboard feature): streaming chat, thread history, voice dictation (`audio-client`), live model catalog from `/api/v1/models`, composer settings, tools opt-in.
- Chat page: `streamChat`/`resumeStream` SSE with `tool_call`/`tool_result`/`attachment` event families, thread history, attachments both directions, model selector with 3 Velion modes + live catalog, feedback endpoint. `buildChatWireBody` **does** emit tools now — the old "frontend sends no tools" gap is closed.
- Search tab: `/search/web`, images, videos, find-similar, suggest, **streaming `/answer`** — the Exa-style surface is wired.
- Crawl tab: page discovery → subset pick → crawl jobs, in-app browser chrome via `browser-client` (profiles/sessions) — the "see what to scrape" browser exists.
- Backend truth: the Model Plane agent loop is **real, not nomenclature** — `execute_step` dispatches shell, browser_agent, subagents, web_search/knowledge_search, shipping, social, provider actions, MCP proxy; server-side HITL pauses risky tools *before* execution.

**Honest gaps**
- **Engine status (corrected 2026-08-01):** model-gateway and inference-core are up, healthy, and **proven live** — this is no longer a probe-it-yourself gate. A real chat turn was driven end-to-end for hours (streaming, live Azure providers, tool loop, GraphRAG-fused retrieval) during the 2026-08-01 session. Still open: a CI-gated smoke test (this was a manual/interactive proof, not an automated one).
- **All-ZDR posture:** resolved on 2026-07-21 (§6.9) — the undocumented interactive-retention-policy exception is removed, `zdr:false` is the real default, resolved live per org. A *separate*, still-open gap: no provider is currently attested for ZDR (`AZURE_OPENAI_ZDR_CONFIRMED` defaults false, azure-anthropic has no attestation knob), so a user who explicitly requests ZDR (per-turn toggle or org-wide switch) is correctly refused rather than silently downgraded — see `apps/Model Plane/docs/ZDR.md` for the exact gap and the path to closing it.
- Post-approval continuation is a named P0: an approved risky action has no restartable dispatcher/receipt — HITL can pause but resuming is not durable.
- Search quality: SearXNG effectiveness is **unproven**; "full Google" is not a 3-dev goal — a good grounded answer engine over web + own KB is.
- ~~Semantic memory degraded (`DEGRADED_SEMANTIC_UNVERIFIED`)~~ **fixed 2026-08-01** — root cause was an embedding-dimension mismatch (config silently defaulted to 1536 while embeddings are 3072) that zeroed every search; readiness now correctly distinguishes a genuinely-empty index from a healthy one. Cost ledger in-memory (unchanged).
- ~~MCP execution disabled for MVP; Visma integration is vaporware in-product (zero source matches)~~ **completely reversed 2026-07-28.** A real OAuth 2.1 + Dynamic Client Registration client lets model-gateway connect a third-party MCP server from nothing but its URL; the prior "bridge" transport was never real MCP at all (no genuine remote server could have worked) and was replaced with real MCP Streamable HTTP. **Live-verified: a real Visma Net ERP query answered through plain chat.** Caveats: HTTPS-only (stdio/legacy SSE quarantined), streaming path only, token refresh not load-tested under concurrent use.
- **New this week — a P0 agent-loop-starvation bug, fixed 2026-07-28.** `MAX_TOOL_ROUNDS` was 3; a self-describing MCP server (like the Visma one above) spends its first rounds on tool-discovery calls, leaving zero rounds to act on results — the model would work out a fix and still have to hand it back to the user. Now 12 (configurable), plus tool-result truncation (was unbounded), a 4× larger answer budget (1024→4096 tokens), and honest signaling when the tool phase is cut short. Live-verified before/after on the same question.
- **New this week — cost-aware model selection (Budget/Balance/Genius) was completely inert until 2026-07-29.** The budget-check call carried no auth header and 401'd on every single call since it was built, permanently pinning cost posture to `Unknown` — no request was ever actually downgraded by budget pressure. Any claim that this capability was "live" before this date should be read as source-real-but-not-enforced. Fixed and confirmed live (`posture:"Healthy"` observed).
- **New this week — the per-answer confidence badge was hardcoded at 72% for every tool-grounded answer, fixed 2026-07-29.** The scorer's only path to a higher score required KB citations, which tool-sourced answers never carry — so every correct Visma/tool answer showed a false "Usikkert svar" (uncertain) caveat. Fixed by adding tool-success as its own grounding signal.
- **New this week — delegated subagents were completely faked before 2026-07-28.** `execution-core`'s subagent dispatch returned a canned "spawned" string with no actual execution. Now really runs (isolated context, depth-1 cap, budget charged to the parent) — cannot yet pause for human approval mid-delegation.
- **New this week — a response cache now reaches real chat traffic (2026-08-01).** The semantic response cache existed and worked but every call site was wired into a gRPC path real chat traffic never uses; now wired into the actual streaming invoke path. Live-verified: a repeat question went 7.7s→1.1s, zero tokens billed. Scoped out: turns using tools, citations, or structured output (replay-fidelity, not staleness).
- **New this week — chat pinning is server-owned and no longer destructive (2026-08-01).** Pinning was previously client-only dead code with zero callers. Also found and fixed in the same pass: a sort-before-truncate bug was silently deleting any pinned thread past position 40 in the history list — a real, previously-undiscovered data-loss bug, not just a missing feature.
- **New this week — memory provenance is now user-visible (2026-08-01).** Extracted memories carry a `stated`/`inferred`/`unknown` provenance field; the SPA shows an "Inferred by Velion" badge only on `inferred` rows, so a user can tell what they actually said from what the AI concluded.
- ChatPage.tsx 3,685 lines / DashboardComposer.tsx 2,349 lines — maintainability debt against your own 800-line rule.

**Competitor bar (ChatGPT/Perplexity/Chatbase):** streaming grounded chat with citations ✅ in source; time-to-value (crawl → useful answer < 15 min) is the Chatbase bar and is achievable with what exists; "combined ChatGPT+Claude+Manus+Perplexity" is **not** the pilot bar — grounded Norwegian answers with sources is.

**Done-enough gate:** inference hot path deployed and pinned to a commit; ZDR exception/tier decided and documented; one smoke test that performs a real chat turn + a real grounded answer in CI; approval→continuation works for at least the preset agent actions used in the demo.

---

### 1.3 Inbox — most complete Application Plane surface

**Vision:** Outlook + Intercom + Gorgias + Mimir parity, omnichannel.

**What verifiably exists**
- UI freshly overhauled (July 2026, 5 phases): sandboxed-iframe email rendering, resizable saved panels, Details tab on real data, real macros, and — the important part — `inbox-ai.ts` making **real model-plane calls** (`/api/v1/chat/invoke`) for draft/summarize/intent/ask with grounded sources.
- Backend: conversation-core-go (293 tests) — inboxes, queue, conversations, messages, notes, status, assignment, tags, search, and a real **HITL ai-actions review/approve/reject queue**. Channel-agnostic model (free-form `channel` string).
- Inbound pipeline proven channel-agnostic: Gmail/Outlook + Slack live earlier; Teams/Slack/X/Discord pollers landed 2026-07-18 (36/36 green; synthetic Teams → 202 → auto-inbox proven).

**Honest gaps**
- Same deployment gap: hardened source vs stale running images; conversation migrations unapplied; approval-event durable outbox, delivered/submitted provider callbacks, and stranded-row reconciliation missing.
- Full Inbox AI/HITL E2E is **blocked on Model Plane health** + lack of authorized WhatsApp/Messenger/Novu sandbox recipients; the old audit finding "WA/Messenger replies silently never send" lives in this hole.
- ZDR/retention unproven across message bodies/attachments/AI drafts; tenant retention policy absent.
- Known follow-ups from the UI overhaul: attachments, edit-draft-before-approve, backend prefs sync.
- Parity honesty: no CSAT, no collision detection, no per-channel connect UX for a customer to self-serve their own Gmail/Outlook.

**Competitor bar (Zendesk/Intercom/Gorgias/Mimir):** unified inbox + AI drafts + human approve is *met in source and is your differentiator* (approvable beats "90 % auto-resolved" for pilot trust). Full helpdesk parity (CSAT, SLA depth, voice, WFM) is **not** the pilot bar — do not chase it.

**Done-enough gate:** deployed; a pilot org can connect its own email account self-serve; AI draft → human edit → approve → **actually sends and is confirmed delivered** on email (one channel done honestly beats six half-channels); reconciliation for stuck sends exists.

---

### 1.4 Ticketing — real and distinct, further than expected

**Vision:** Zendesk + Intercom + Zammad class.

**What verifiably exists**
- Separate `/tickets` system (not just an inbox view): tickets, ticket views, macros, **SLA policies, automation rules**, per-org resources — `tickets-client` (415 LOC) + `tickets.rs` (333 LOC), all backend-driven.
- Zammad exists only as a non-integrated bootstrap stack on a separate compose file — the first-party path is conversation-core + tickets domain, which is the right call.

**Honest gaps**
- Depth untested against real volume; queue taxonomy is a static label array; no CSAT, no customer-facing portal/help-center, no routing engine; shares conversation-core's deployment/callback gaps.
- No evidence of E2E under real multi-agent load.
- **Code-health audit, 2026-08-02** (`TicketingPage.tsx`, `tickets-client.ts`): every ticket data resource swallows fetch errors into an empty-array fallback (`optionalTicketResource`), which makes the page's own "Ticketing is unavailable" error UI **permanently unreachable** — a real backend outage renders as a plausible-but-false "No tickets in this queue." Separately, a validated `tickets.*` action contract already exists in `src/shared/actions` (update/assign/resolve/link_resource, wired through the gateway dispatcher) but `TicketingPage.tsx` never uses it — it calls `tickets-client.ts`'s raw fetch wrappers directly with no schema validation, so two divergent mutation paths exist for the same resource, violating this repo's own "add the action contract before wiring a UI operation" rule. Also found: 10 of 21 `tickets-client.ts` exports (SLA-policy/macro/automation-rule/view create+update) have zero importers — the RulesWorkspace tab only ever renders these read-only, so that management surface was scaffolded but never wired to a control; `resource_url` renders into an anchor `href` with no protocol allow-list (a `javascript:` URI would pass the Zod schema, which only checks `.trim()`); `TicketingPage.tsx` is 866 lines against the project's own 800-line ceiling; and status/priority/severity fields are typed `TicketLiteral | string`, which defeats compile-time checking since `string` absorbs the whole union.

**Competitor bar (Zendesk):** assignment, status, tags, macros, SLA — **already present in source**, which is genuinely ahead of schedule. Help center + CSAT are pilot-era, reporting is Insights' job.

**Done-enough gate:** deployed alongside Inbox; one full lifecycle (create from inbound → assign → SLA clock → macro reply → resolve) demonstrated with real data; decide Zammad's fate (recommend: delete the stack, keep learnings).

---

### 1.5 Ingestion / Crawler page — real product, dead host; "SEO" does not exist

**Vision:** URL management, scheduling, crawler profiles, SEO monitoring, Google/Meta ads tracking → rename to SEO.

**What verifiably exists**
- The crawler/ingestion product is real: `VelionIngestionsPage` (1,137 LOC) → sources, **crawler profiles**, **schedules**, runs, evidence, actions; monitoring/change-watch client. Quarry-v2: scrape/crawl/batch (100+ URLs), URL security scanning, change tracking, webhook delivery, browser pooling, SSRF/DNS guards, production HMAC enforcement — 652 tests green.
- imports-core: Notion/HubSpot/Salesforce/Odoo/REST connectors + PDF/DOCX/CSV/JSON/HTML parsers with signed identity and fail-closed quota.

**Honest gaps**
- ~~The host is down~~ **Corrected 2026-07-20:** Quarry edge/control/orchestrator, imports-api, integration workers all up and healthy; 116 GiB free on the volume. The July disk outage is resolved but the 278 GiB Docker data image remains a standing risk — add a disk alert.
- Schedule trigger/backfill Temporal wiring stubbed; `/v1/sources` schema TODO; artifact store has `unimplemented!()` paths; SearXNG unproven; imports-core coverage 48 %.
- **There is zero SEO code anywhere.** No rank tracking, no ads integration, no traffic analytics, no AI-visibility tracking. The "SEO page" is 100 % future work, and the market bar has moved to AI-answer visibility (Peec: prompts × models × sentiment × cited sources, €85+/mo).

**Competitor bar (Peec):** classic SEO is a losing catch-up game; a *Norwegian-language AI-visibility lite* (~25 prompts × 2 models with cited-source reports) would be a genuinely open niche — but only **after** the pilot.

**Done-enough gate (crawler):** host disk reclaimed, Docker up, scheduled re-crawl of one real site proven, change-watch firing into knowledge freshness. **SEO gate:** consciously deferred — write it on the roadmap as a post-pilot bet, stop calling it a current feature.

---

### 1.6 Social — strongest gateway domain, strategically parked

**Vision:** merged into Studio "in a smart way."

**What verifiably exists**
- The largest gateway domain (`social.rs`, 3,088 LOC): posts, calendar, drafts-from-inbox, adapters/accounts, approvals workflow, campaigns, evergreen, competitor-watch, trends, metrics, commerce catalogs. social-core backend healthy with worker-enforced HITL at execution time. Real LinkedIn/X/IG/FB/TikTok integrations in source.

**Honest gaps**
- "Live authenticated UI/provider proof still pending" — no verified end-to-end post to a real account in the current pass. Historical audit flagged fabricated trends/competitor-watch and demo-account fallbacks (some cleaned; verify before showing).
- Social domain test coverage 41.7 %.
- The Studio merge is architecture-roadmap (Studio Plane Phase 4), i.e., far away.

**Done-enough gate:** park it. Before it appears in any demo: one real authenticated post to one real account, and every remaining fabricated fallback removed. Do not spend the merge-into-Studio effort pre-pilot.

---

### 1.7 Settings / Integrations — broad and real

**What verifiably exists**
- Providers/connections/connect-sessions/sync-jobs (real OAuth connect flows), cron, MCP servers, plugins, skills, privacy/Trust Center, 2FA, passkeys, org members/roles, billing, audit log — 13 files, ~5.4k LOC, all real clients, admin-gated.

**Honest gaps**
- SSO + org-security cards honestly unwired (render neutral — good). Better-Auth oRPC has placeholder paths (consent persistence, passkeys backends, admin stats). Billing has no live checkout proof; Nexi creds populated but unexercised in this stack.

**Done-enough gate:** already good enough for closed demo. Pilot gate: the 2-3 providers your wedge needs (email + website crawl) connect self-serve without a dev.

### 1.7a Norwegian public-data context — deployed bounded lookup layer

**What verifiably exists**
- `information-core` is a real Application Plane service for bounded, read-only, provenance-bearing contextual lookups rather than a second knowledge corpus or ERP runtime.
- The rebuilt local artifact verified Kartverket address/property location, SSB metadata, Entur journeys, Storting representatives, Norges Bank SDMX, MET weather, Statens vegvesen traffic/NVDB, NVE warnings, Riksantikvaren features, and Miljødirektoratet observations.
- Responses carry a canonical source envelope with provider, dataset, source URL, retrieval time, quality/status and coverage semantics. This allows Velion to distinguish measured, forecast, partial, stale, unavailable, and source-only results.
- These sources can enrich search, agent runs, logistics, property/context preflight, regulatory research, market intelligence, and Norwegian company work without being confused with the organization's private knowledge.

**Honest gaps**
- Lovdata, DATEX II, and Frost remain source-only until provider credentials are provisioned and live acceptance passes.
- eInnsyn, full Matrikkel/Grunnbok, Folkeregisteret, Maskinporten/Altinn data, closed AIS, and other restricted datasets require explicit purpose, authorization, legal basis, retention, and delegation decisions.
- Durable feeds, versioned corpora, and recurring catalog/source ingestion belong in Ingestion/Data Plane, not in the bounded lookup service.
- The July audit found stale or over-broad assumptions around Bring tracking, Kartverket, Lovdata, eInnsyn, Doffin, and Frost; product claims must use the current `deployed_verified`, `source_only`, `blocked`, and `discovery` states.

**Done-enough gate:** each source is independently feature-flagged, source-attributed, bounded, rate-limited, tenant-safe, ZDR/GDPR-reviewed, and consumed through a named product workflow rather than exposed as an unqualified list of APIs.

---

### 1.8 Agents — real runs console, fake builder; the biggest honesty gap in the product

**Vision:** n8n + Intercom Fin + Chatbase + workflow builders.

**What verifiably exists**
- `/agents/runs` **Agent Run Console is real**: runs list/detail, SSE timeline, plans/todos/lineage via orchestration-client. Server-side HITL is real and enforced (execution-core pauses risky tools pre-execution; durable approvals in session-core). Cost (`/agents/cost`) and eval (`/agents/quality`) pages wired.
- **Durable Temporal workflows are now reachable end-to-end (fixed 2026-07-31, previously they were NOT)** — deep task, wide research, memory consolidation, skill promotion, and feedback promotion had zero production callers as of that morning (registered, tested, deployed, never once invoked outside a test file). By end of day: a real cron-fired run is claimed, completed, and consumed by the learning-review path with no human in the loop — live-verified against real DB rows, not just code inspection. This specifically unblocks the 3 ZDR-denying workflows (memory/skill/feedback promotion), which were refused by caller *identity* (an internal/shared-secret caller can never satisfy a ZDR-denial gate) until a mintable JWT audience for the orchestrator was added.
- **Delegated subagents were completely faked before 2026-07-28** — returned a canned "spawned" string with no execution. Now really run (isolated context, depth-1 cap, budget charged to the parent); cannot yet pause for human approval mid-delegation.
- **Real code interpreter + canvas artifacts, new 2026-07-30.** Execution-core runs real sandboxed Python (and `sh`) via bwrap; canvas artifacts are genuine generated files (xlsx/docx/pdf via pandas/openpyxl/python-docx/reportlab — not a stub), with a real pageable version history (a `‹ v2/3 ›` stepper) and working downloads (a prior bug served files as a blocked top-level `data:` link).
- **Cron-fired runs are now visible (read-only) in the console (2026-08-01)** — the SPA-side companion to the durable-orchestration fix above.

**Honest gaps**
- `AgentsPage`, `WorkflowBuilder`, `WorkflowCanvas`, `ChatbotStudio`, `ChatbotPlayground` import **no API clients** — they render 861 LOC of hardcoded blueprint data. The code says it itself: "Phase 4 honesty sweep: … has no backend yet." There is no create/save/deploy-agent call in the product.
- Post-approval continuation P0 (see 1.2). Cost ledger in-memory. Sandbox/browser-broker stores in-memory.
- **Capability catalog, corrected 2026-07-30:** "all 27 rows unavailable pending health attestation" is now half-true. execution-core added a real self-attestation heartbeat for `cap.command.shell`/`cap.command.sandbox` specifically (the code interpreter/canvas capabilities above) — but the scope that heartbeat needs is **not yet granted in the deployed service-principal registry**, so it can still be silently refused in production pending an operator/admin change. The other 25 rows, including `cap.browser.open`, have no attestor anywhere in the repo and remain permanently unavailable.

**Competitor bar (Fin/Chatbase/Mimir):** the bar is an agent that **acts in customer systems under guardrails** — not a canvas. Your HITL + audit + cost primitives are the differentiator; the n8n canvas is commodity UI on top.

**Done-enough gate:** ship **3–5 preset approvable agents** (e.g., "besvar kunde-epost med kilder", "oppsummer nye henvendelser", "hold kunnskapsbasen oppdatert fra nettsiden") running on the existing runs console + HITL. **Hide the builder/studio/playground routes until then** — a static prototype in a live demo is a credibility grenade. Build the canvas only when pilot users ask to modify presets.

---

### 1.9 Insights — honest but thin, backend least mature

**Vision:** analytics on everything Velion does + benefit reporting.

**What verifiably exists**
- Real but minimal: `/insights/connectors` + `/insights/overview` scorecards with an enforced honesty contract (no fabricated values, `live` never attaches to an unproduced number). 7 section routes exist. Raw material is genuinely there: audit-core (append-only audit + usage + summaries), cost-core paths (in source), run events, inbox metrics.

**Honest gaps**
- insight-core is the least mature Application service ("no material change; daily-brief path not proven"). No charting depth, no report builder, no "value delivered" story. The old "insights 404" is fixed in wiring but the content is skeletal.

**Competitor bar:** every inbox competitor ships resolution/deflection/CSAT dashboards. Your open angle is **cost-per-resolution transparency** — you already meter cost; nobody local shows it.

**Done-enough gate (pilot, not demo):** three honest numbers on one page — conversations handled, AI drafts accepted %, cost per resolution — computed from real audit/usage rows. That is the whole pilot requirement. The "real reports" engine is post-pilot.

---

### 1.10 Studio — small real canvas today, big plan on paper

**What verifiably exists**
- A real block-based content canvas (`studio-client` → `/api/v1/studio/projects`, create/save/export-to-social-draft). Persistence just moved past gateway-memory prototype status per docs — verify which is live.
- Studio Plane = a README (2026-07-19). Zero services. 8-phase plan (revenue/market/campaigns/create/work/operations), explicitly gated, explicitly "don't create empty services."

**Done-enough gate:** none pre-pilot. Keep the canvas as-is (it feeds Social drafts, that's useful). The Studio Plane plan is good *paper* — resist starting Phase 0 until the wedge is sold. The "social merges into studio" idea lands at Studio Phase 4, which is several gates away by its own doc.

---

### 1.11 SEO (as a named surface) — does not exist

Restated for the sorted list: no code, no route, no client, no backend. Everything under 1.5's SEO gate applies. Rename the ambition, not the ingestions page: the crawler page is a crawler page; a future "AI-visibility" page is a new product bet.

### 1.12 External agents and meeting intelligence — future source/distribution layers

**Channel Plane** is docs-only. Its future role is to deploy Velion agents into websites, Shopify, WooCommerce, WordPress, and other public channels, bootstrap visitor identity, run external conversations, and route handoffs into the internal inbox. No current Channel Plane services, APIs, migrations, or tests exist.

**Meeting intelligence** is also docs-only. The intended design treats a meeting as another source type: transcript segments become Data Plane documents, slides/keyframes become visual evidence, people/decisions/action items become graph entities, and Model Plane reasoning can produce minutes and approved follow-up actions. Capture, self-hosted ASR/diarization, and sovereign visual embedding remain future work; the concept reuses existing planes rather than creating a separate meeting product.

---

## 2. Cross-cutting blockers — why nothing demos *today*

These block **every** feature above and are the actual roadmap. *(Corrected 2026-07-20 against the live host — items 2 and 3 as originally doc-claimed were stale.)*

1. **Deployment-reality gap → now a *provenance* gap (still the #1 item).** The stack IS up (94 healthy containers, model-gateway + inference-core + Quarry included), but nothing proves the running images match the hardened source — no SHA tags, no `/version` endpoints, no rollback artifacts, and July showed health can stay green over a broken path. Fix: git-SHA image tags, `/version` endpoints, CI that builds+deploys the slice from main, deep smoke tests (one real chat turn, one real retrieval, one real inbox send — not port checks). Until then, "healthy" is a claim, not evidence.
2. **~~Model Plane hot path down~~ RESOLVED, now fully proven (2026-08-01).** Container-level resolution (2026-07-20) plus a real live inference call (2026-08-01 — hours of streaming chat, tool loop, GraphRAG retrieval). The ZDR posture question is also resolved (§6.9, 2026-07-21): `zdr:false` is the real default, no undocumented exception. The one remaining ZDR item is provider attestation, not policy — see `apps/Model Plane/docs/ZDR.md`.
3. **~~Ingestion host dead~~ RESOLVED.** Docker up, all Quarry/imports/integration workers running, 116 GiB free on Lagring. Keep a disk-usage alert; the 278 GiB Docker image that caused the July outage is still the standing risk.
4. **Credential rollout.** Control (58 creds + 10 files preflight) and Data plane scoped-broker provisioning are operator-owned tasks nobody has run. For one demo environment: consolidate/pre-seed (you already proved zero-rotation recreate for Control on 2026-07-17) rather than doing the full production ritual.
5. **Cross-tenant IDOR + observe-mode auth.** One fix at the BFF chokepoint (derive org from session, never from header) covers the demo; RLS activation + enforce-mode + negative-test suite in CI is the pilot gate.
6. **CI is red.** velionv3 `pnpm test` fails (loadStudioWorkspace unhandled rejections); Quarry-v2 workspace tests fail to compile (`DataPlaneIngestRequest` constructors). A team cannot gate deploys on a suite it ignores.
7. **Ops sustainability (structural).** Six planes / ~10 stateful infra systems is a 20–30-engineer topology run by 3 people, and its failures are already ops failures (disk, stale images, stopped daemons). Not demo-blocking, but the council's minority-report warning (below) deserves a real team decision.

---

## 3. Competitor bar — the minimum to be credible in Norway

| Surface | Who sets the bar | Pilot-minimum bar | Velion vs bar |
|---|---|---|---|
| Grounded assistant | Ayfie (NO ent.), Chatbase | Crawl site + docs → cited Norwegian answers in <15 min; EU residency in writing | **Met in source**; blocked on deployment + ZDR decision |
| Inbox + AI | Intercom, Gorgias, Mimir | Email unified, AI draft + human approve, delivery confirmed | Met in source for email; delivery/callbacks missing |
| Ticketing | Zendesk | Assign/status/tags/macros/SLA | **Met in source** — surprising strength |
| Agents | Fin, Mimir, Chatbase Actions | ≥3 real actions in customer systems w/ guardrails + audit | HITL/audit real; preset agents needed; builder not required |
| Knowledge/RAG | Ayfie | One corpus feeds search + agent, source traces, permissions | **Met in source** — differentiated (graph + wiki + traces) |
| SEO/AI-visibility | Peec | Prompt-set tracking across models (post-pilot niche in Norwegian) | Does not exist — deliberate deferral |
| Analytics | all inbox players | Deflection/acceptance/cost per resolution | Thin; cost transparency is the open angle |
| GTM mechanics | Cobrief, Taito | Norwegian UI, flat self-serve org pricing (Trial 0 kr, Hobby 299 kr/mnd, Standard 999 kr/mnd, Pro 1499 kr/mnd — not per-seat), free trial, viewer seats | Shipped: flat per-org monthly tiers live in onboarding + billing settings; per-inquiry usage rates shown in paywall/billing copy are display-only today, not enforced (Lago has zero plans configured) |

**Market shape:** inbox/helpdesk = red ocean (don't lead with it). Norwegian SMB self-serve grounded assistant = near-empty (Ayfie is enterprise/sales-led). The *combination* is the moat, but it's a **retention** story — acquisition needs one wedge.

**Correction 2026-07-20 — boost.ai was missing from this list, and it's a real Norwegian threat.** Sandnes-founded conversational AI platform (general knowledge — not independently re-verified today beyond the live site fetch below): "The conversational AI platform regulated industries trust." Hybrid NLU+GenAI architecture, chat + **voice** channels, three use cases (self-service, internal support, **agent-assist**), enterprise/demo-gated motion (no public pricing), case study with Acorn Insurance (UK). This is a bigger direct threat than Ayfie *specifically in the AI-support-agent space* — Ayfie is enterprise search, boost.ai's whole product is conversational support automation, sold to Nordic banks/insurance/telco/public sector for years. **Curated counter:** take their hybrid-reliability framing ("predictable, governed AI" — matches your visible per-action HITL approval, arguably stronger since it's provably approved not just architecturally hybrid) and their explicit "agent-assist" naming for what inbox-ai already does; skip voice (no wedge evidence, real build cost) and skip chasing their enterprise incumbency (their gap is precisely no self-serve/no public pricing — that's still open ground for a Norwegian SMB self-serve motion).

---

## 4. Council verdict — how to tackle it

Four independent perspectives evaluated: (A) deploy-first, (B) security-first, (C) feature-depth-first, (D) vertical-slice-first.

**Unanimous points**
- **C (feature-depth-first) rejected 4/4.** Building the Agents builder or Insights depth while the inference path isn't deployed is "polishing a car with no engine" / "how the platform dies." The wishlist is ~8 companies' roadmaps for 3 devs.
- **D (vertical-slice-first) wins 4/4** — but only as a *container* for A and B: the slice must be genuinely deployed (A's discipline) and minimally safe (B's demo-tier fixes), not demo-hacked.
- Security is **tiered, not first**: a single-org closed demo makes the cross-tenant IDOR literally unexercisable, so demo-tier security = deploy hardened images + kill header-derived org at the BFF + real HMACs + remove any remaining fabricated trust UI. Full IDOR closure + active RLS + enforce-mode + reachable DSAR/erasure + proven-or-withdrawn ZDR = the **hard gate between closed demo and open pilot**, verified by an automated cross-tenant negative test in CI.
- Preset agents over builder: ship 3–5 approvable preset agents on the existing runs console; build the canvas only on demonstrated pilot pull.

**Pros/cons in brief**
- **A deploy-first** — Pro: the deployment-reality gap is the #1 verified failure; nothing else is real until code and runtime are the same system. Con: "deploy all 6 planes" is months of ops with no user payoff; deploy the *slice*, adopt the *discipline* (SHA tags, /version, smoke, env manifests) everywhere.
- **B security-first** — Pro: trust IS the product; a tenancy breach in pilot is company-ending given the positioning. Con: hardening code that isn't even deployed is "security theater squared"; full-estate hardening before any user starves you of feedback.
- **C feature-depth-first** — Pro: closes the only real feature gap (Agents builder) and the visible thin spot (Insights). Con: enters two red oceans, grows blast radius on an unsound boundary, and delays user contact indefinitely. Rejected.
- **D vertical-slice-first** — Pro: converts source progress into the only currency that matters (a thing a stranger can use); forces deploy+security fixes where they're needed; matches the near-empty Norwegian wedge. Con: requires accepting that most of the built surface stays hidden for a while; risk of duct-tape shortcuts that don't generalize — mitigated by the gates below.

**Minority report (delivery-risk voice, worth a team discussion, not a demo blocker):** the 6-plane topology is unsustainable for 3 people long-term. Consider collapsing to 2–3 deployables (gateway+app tier, worker tier), one Postgres with schemas+RLS, keep NATS+Qdrant, defer Temporal/Quickwit/Convex/MinIO where substitutable — keeping plane boundaries as *modules*, not *deployments*, and re-splitting when headcount justifies it.

---

## 5. The recommended sequence (order + gates, no dates)

**The wedge:** *"Norwegian grounded answers from your company's own knowledge — with sources — and an inbox that drafts replies from it, which a human approves."* Crawl → Knowledge → Chat-with-citations → Inbox AI draft → HITL approve → send. This is simultaneously your most-finished path, your architecture's spine, and the open market gap.

**Phase 0 — Demo-safety + truth sprint.**
Reclaim Ingestion host disk; deploy current hardened source of the slice's services (Control auth path, Data retrieval, Model gateway+inference, Quarry edge/control, conversation-core, gateway) with git-SHA tags and `/version`; decide the ZDR exception for pilot inference; fix the two red test suites; wire CI to block on them + one deep smoke test; derive org from session at the BFF (kill `x-velion-org-id` trust at the chokepoint); sweep any remaining fabricated trust UI.
*Gate: a teammate on a clean machine completes crawl → cited answer → inbox draft → approve → send, with zero dev intervention, twice in a row.*

**Phase 1 — Wedge polish on the slice only.**
Hide: Agents builder/ChatbotStudio/Playground, SEO ambitions, Social, Insights sections beyond overview, Studio (keep canvas reachable but off-nav if rough). Ship 3–5 preset approvable agents on the runs console. Seed the demo org with a real Norwegian corpus (Aquatiq). Fix edit-draft-before-approve in Inbox (it's on your own follow-up list and it's core to the wedge). Add the three honest Insight numbers (handled / accepted % / cost per resolution).
*Gate: scripted 20-minute demo runs 5× consecutively without failure; crawl-to-first-cited-answer < 15 minutes.*

**Phase 2 — Closed demo (10 friendly users, single org).**
Instrument everything; weekly fix cycles; collect the "which preset agents do they want to modify" signal that decides whether the builder ever gets built.
*Gate: ≥5 of 10 complete the core loop unassisted; no P0 open for a week; at least one user says they'd pay at the shipped flat-tier price (Trial 0 kr / Hobby 299 kr/mnd / Standard 999 kr/mnd / Pro 1499 kr/mnd — per-org, not per-seat; note per-inquiry usage pricing shown in paywall copy is display-only today, not enforced).*

**Phase 3 — Pilot hardening (the security gate).**
Full IDOR closure across all call sites + RLS active + enforce-mode documents auth + automated cross-tenant negative tests in CI; erasure/DSAR reachable end-to-end (build the Data Plane purge subscriber); ZDR proven for the default path or the claim withdrawn from all materials; per-service credential split + rotation of the exposed Data Plane credential; provider delivery callbacks + stuck-send reconciliation for email; DPA/RoPA/sub-processor list/incident runbook on paper.
*Gate: internal cross-tenant pen-test passes; a GDPR erasure demonstrably propagates; a second org onboards with zero shared-credential exposure.*

**Phase 4 — Open pilot (real Norwegian companies).**
Self-serve onboarding (email connect + site crawl without a dev), NOK pricing published, Norwegian UI pass, feedback loop.
*Gate: 10 external users active weekly; deflection/acceptance metrics honest on the Insights page.*

**Phase 5 — Expansion, strictly pull-driven.**
In rough order of expected pull: Agents canvas (if preset-modification demand is real) → Insights depth/reports → more inbox channels (WA/Messenger with sandbox recipients) → Social live-proof + Studio Phase 1 → Norwegian AI-visibility ("SEO") as a new bet → Zendesk-parity ticketing extras. Each only when a paying user asks.

**Explicitly not now:** n8n-parity builder, full Google-class search, SEO suite, Studio Plane services, Channel Plane, meeting notes/voice (already cut), enterprise items both plane docs correctly defer (mTLS/SPIFFE, HA/DR, multi-region, SOC2 — sequence *after* revenue, though the cert path is the enterprise unlock later).

---

## 6. Ecosystem addendum (2026-07-20) — one platform, how many products?

Question raised: merge avelis (SEO), agenci (support widget), and the Aquatiq integration services into Velion? Run them as a Microsoft-style product family? Or a hybrid? Verified findings first, then the recommendation.

### 6.1 What the codebases actually are

**avelis** (`apps/Frontend Plane/avelis`) — a ~6-source-file Next.js 16 shell: correct CoreSystem-native auth (calls auth-core `/api/v2/auth/getSession`), clean Dockerfile, 4 tests — and **zero product features**. `/` renders a heading. No SEO, crawl, keyword, or lighthouse code exists. It is ~1 day of good scaffolding wearing a product name.

**The six `/Volumes/Lagring/services`** — a legacy "QualAI"-era fleet, none wired to CoreSystem (Clerk auth or their own gateway headers; own Kafka/RabbitMQ/Mongo islands):
- **DiscoveryBot** — the real asset: tested Scrapy crawler, 7 spiders (incl. `SEODiscoverySpider`), link health, security headers, site-tree building, Celery scheduling. Production-shaped.
- **GrammarService** — deep, DI-architected **Norwegian grammar engine** (Ordbank data, norBERT, GPT adapter) with real unit+integration tests. Genuinely differentiated for a Norwegian product.
- **ReadabilityService** — spellcheck (NO+EN Hunspell), Lix readability, AI sentiment/summarization sub-services. Working, bloated (committed venv, nested duplicate GrammarService).
- **TextService / CnCRatioService** — WCAG/contrast/content-to-code analyzers. Working prototypes, stubbed or Clerk auth.
- **ScreenshotService** — empty husk (2 files). Delete.

Net: **real SEO/content-analysis capability exists — DiscoveryBot + GrammarService + Readability — but 0 % of it is integrated**; the gap is plumbing (auth-core + gateway + NATS instead of Clerk/QualAI/Kafka), not capability.

**agenci** (`apps/Frontend Plane/agenci`) — a **working near-MVP product**, not a prototype: ~42k LOC Turborepo (Next.js 15 + Convex + Clerk + Stripe + Firecrawl + OpenAI + Vapi voice), Norwegian throughout, real billing webhooks, real RAG (`@convex-dev/rag`, per-org namespaces), embeddable widget + anonymous visitor sessions + bookings with GDPR auto-delete. Active product-focused git history. Weaknesses: ~zero tests, no CI, no containers. **It shares zero code with CoreSystem** — and it is functionally a working realization of the Channel Plane vision (external widget + visitor identity + public conversation runtime) on a completely different stack.

**Aquatiq integrasjonen** — a Python/Rust **ETL-to-warehouse fleet** (BigQuery/Azure SQL/PowerBI/GCS), not an action broker: moment (~2.3k LOC, 26 REST routes), contifico (~2k), socrm (~8.3k, real Azure-AD OAuth), currency (reference data), odoo (54-line stub), plus two excellent Rust engines — visma_v2 (11 crates, ~11k LOC, tenant-id-per-row multi-tenant datamart sync) and xero_v2 (8 crates, stateless CLI → GCS raw landing). Architecture mismatch with integration-corev2 (synchronous OAuth broker + named-action executor): **the API knowledge ports, the runtimes don't.** moment/contifico/socrm → feasible corev2 adapters (low→medium effort); currency → plain internal service; visma_v2/xero_v2 → belong in a *bulk-sync worker tier*, not the actions surface; odoo → nothing to port. ⚠ Flagged: committed GCP private keys (3 locations), gateway `AUTH_ENABLED=false`, default JWT secret, hardcoded Redis password — none of this may carry over.

### 6.2 The three options

**Option 1 — merge everything into Velion: rejected.**
Pros: one brand, one auth, one KB, the "combination" moat. Cons are decisive: three foreign stacks would need full rewrites (agenci's Convex/Clerk backend cannot sit behind velion-gateway-rs; the QualAI fleet needs re-auth + re-bus; the ETL fleet is architecturally different from corev2); it re-inflates the scope cancer the council killed 4-0; it buries agenci's near-term revenue under a migration; and it adds ~10 more nav items to a product whose pilot problem is focus. This is how all three products end up 70 % done.

**Option 2 — Microsoft-style ecosystem (Velion / Avelis / Agenci as sibling products): partially right, honestly premature.**
Pros: matches the reality that agenci already IS a separate product with separate GTM (PLG widget SaaS vs. workspace); brands can win/fail independently; the support-widget market (Chatbase/Fin) and the workspace market want different pricing and onboarding. Cons: Microsoft's ecosystem works because of a shared platform (identity, Graph, billing) — today these three share **nothing**: two auth systems (auth-core vs Clerk), two billing systems (Lago/Nexi vs Stripe), three data stores. Announcing an "ecosystem" of three unintegrated products is a story, not an architecture — and 3 devs cannot give three products real roadmaps simultaneously. Avelis as a "product" doesn't exist yet at all.

**Option 3 — the hybrid (recommended): one platform, staggered fronts, harvest capabilities not runtimes.**

1. **Velion stays the flagship and the only product the team actively builds.** Nothing merges into it now. The wedge and phase gates in §5 are unchanged.
2. **Agenci ships as-is, as a separate revenue product — deliberately NOT ported.** It is the closest thing to money in the whole portfolio (working billing, PLG motion, Norwegian, near-launch). Porting it onto the planes now would kill that. Cap its cost: minimal maintenance, add the missing basics (a smoke-test CI, error budgets), and set one hard rule — **no shared customer data or cross-product promises until it's on shared identity.** Long-term (pull-driven, post-pilot): agenci either becomes the Channel Plane's frontend fed by Data/Model planes, or stays standalone and its widget/visitor/booking patterns get harvested into a native Channel Plane. Decide with revenue data, not now.
3. **Avelis: do not invest pre-pilot.** It's an empty shell and the SEO bet is Phase 5 by this map's own sequencing. When (if) the SEO wedge is validated: harvest **capabilities** into CoreSystem workers behind the gateway — GrammarService + Readability as a Norwegian content-quality engine (genuinely differentiated, nobody local has it), DiscoveryBot's SEO analysis (link health, headers, site tree) as an analysis layer *on top of Quarry* (Quarry stays the only crawler — don't run two). Whether the resulting surface lives at `/seo` inside velionv3 or as the avelis app is a branding decision to make then; the plumbing is identical. Delete ScreenshotService.
4. **"Integration Plane": adopt the idea as two tiers, skip the ceremony.** Tier 1 = integration-corev2 as-is (actions/OAuth broker; port moment → socrm as adapters when a paying Velion/Aquatiq workflow needs them). Tier 2 = a new **bulk-sync worker tier** in the Ingestion Plane for warehouse-grade provider sync — this is where visma_v2/xero_v2's engines and hard-won invariants (tenant-id-per-row, checkpoint-after-validated-write, raw-landing) belong. Whether Aquatiq's warehouse fleet stays a client-specific deployment or generalizes is a business call; either way rotate the committed GCP keys **now** and never import the `AUTH_ENABLED=false` gateway pattern.

**The principle underneath:** CoreSystem's planes ARE the "Microsoft Graph" of this ecosystem — one identity, one knowledge substrate, one model runtime, one audit spine. Products (velionv3 today, agenci later, avelis maybe) are *fronts* on that platform. The ecosystem becomes real the day a second front consumes shared identity — not the day a slide says "suite." Sequence: Velion pilot first, agenci earns independently, everything else is harvested on pull.

### 6.3 Team-learned engineering notes (2026-07-20, post-build hindsight)

Three things the team observed while building Velion, all correct and worth locking in as standing decisions:

- **SolidJS + Vite over Next.js for every app-shell surface.** Fine-grained reactivity (signals → direct DOM, no VDOM diff) suits update-heavy UIs — chat streams, dashboards, live crawl views — better than React's reconciliation model, and a Solid SPA is one long-lived shell so client state survives route changes without extra plumbing. **One caveat, not a refutation:** any surface that needs public Google-indexed pages (agenci's marketing site; a future avelis marketing page) shouldn't go pure client-rendered SPA — use SolidStart or a plain static page for that surface specifically, kept separate from the app shell. Don't let an SEO product lose its own SEO.
- **Corrected 2026-07-20 — agenci's real numbers change the framing entirely.** Live 3 months, zero users, ~400 NOK/month — there is no vendor bill to watch, so the "cost-crossover trigger" originally written here doesn't exist yet; strike it. **The actual stated goal is not to run agenci as a product — it's to sell it to a buyer organization**, on the pitch that orgs prefer to own their stack rather than depend on vendor sprawl. That pitch is in tension with agenci's own architecture (Convex+Clerk+Stripe+Firecrawl+Vapi+OpenAI is exactly the vendor sprawl such a buyer wants to avoid owning). Resolution: **agenci is the proof-of-concept, not the deliverable.** It de-risks the sale (a live, working demo of the widget+RAG+billing pattern) — the actual thing sold to an "own-our-stack" buyer is a CoreSystem-native rebuild of the same UX (Clerk→auth-core, Convex→Data Plane, Firecrawl→Quarry, raw OpenAI→Model Plane). Sell the demo, deliver the substrate.
- **On "AI agent leverage removes the 3-dev constraint":** true for code production, not for the actual bottlenecks this audit found. The blockers were image provenance, one IDOR, an unmade ZDR policy decision, and scope focus — none are dev-hour-bound; AI throughput doesn't relax any of them, and if code is cheap to produce, deciding what's worth building matters *more*, not less.
- **Avelis's current Next.js code is not an asset to preserve — it was going to need a full rewrite regardless of sequencing.** This removes any cost to waiting: nothing decays by deferring avelis to its turn (§5 Phase 5). **Confirmed template to follow:** velionv3 already runs the exact split needed — `apps/velion-web` (Next.js 16, public/marketing/SEO-indexed pages), `src` (SolidJS 1.9 + Vite 8, the authenticated app shell), `apps/gateway` (shared Rust BFF). Avelis should copy this shape exactly rather than reinvent it: an `avelis-web` Next front for anything that must rank in Google, a Solid app-shell for the workspace UI, both behind the *same* `velion-gateway-rs` (new domains, not a fourth gateway). Pull DiscoveryBot/GrammarService/Readability in as new Go/Rust workers behind that gateway; do not port their Node/Python/Flask runtimes, and treat the current avelis repo as a spec to read once, not code to migrate.
- **SEO placement lean (open question, not yet decided by GTM data):** build it as a Velion-native module/tab first, not a separate branded app — it costs zero marginal integration (Quarry already crawls, KB already stores, gateway already exists) and strengthens the combination moat instead of fragmenting it. Only fork it into a separately-branded product if a distinct non-Velion buyer segment (e.g. SEO agencies who'd never buy an AI workspace) is confirmed to want it standalone.

### 6.4 North star — what Velion is if every feature is stripped away

Strip chat, inbox, ticketing, agents, KB, social, insights, crawler down to zero. What's left is the plane substrate, and it has three properties no competitor currently combines:
1. **Grounded retrieval with real source traces + a knowledge graph** (Ayfie has this alone — enterprise/on-prem/sales-led, no self-serve inbox).
2. **Server-enforced, visible, approvable agent execution** — a paused tool call with cost and source attached, waiting on a human, not a vendor's black-box resolution-rate claim.
3. **EU/Norway residency and audit as a plane *contract*** (ZDR posture, cross-plane audit-core, GDPR metadata propagation by architectural rule) — not a compliance PDF bolted on after the fact.

No competitor has all three: Ayfie has #1 without #2; Intercom/Zendesk/Gorgias have automation without #2's transparency or #3's EU-native architecture; Chatbase/agenci-shaped RAG-widget products have #1 cheaply but own none of the pipeline underneath, so they can't show a customer the actual evidence chain. **This is the moat — and it is currently a capability the source is built for, not yet a proof the running system has demonstrated.** The IDOR, observe-mode auth, unproven end-to-end ZDR, and past fabricated-trust-UI findings mean §5 Phase 3 (the security gate) is not bureaucratic caution — it is the literal work of manufacturing proof of the one thing meant to make Velion un-copyable. Skip it and the features still work; the moat just isn't real yet.

**2026-07-20 concrete finding — Application Plane's Convex already has half of "rebuild agenci" designed and working.** `apps/Application Plane/convex-core/convex/schema.ts`'s `agents` table (tagged Wave 9 / `ui-ux-velion-gap.md §19`) already has `publicEnabled`/`publicSecret` (rotated per-agent embed secret), `embedTheme`, `knowledgeBindings` (per-agent RAG scoping into the real Data Plane retrieval engine), `retrievalConfig` (dense/BM25/graph/wiki weights), and `profile: "chat"|"deployed_agent"`. `convex/agents.ts` has real working mutations `enablePublicEmbed`/`disablePublicEmbed`/`getEmbedConfig`. Missing: the public HTTP ingress (`/api/embed/...` — no gateway route exists), an anonymous visitor-identity table (agenci's `contactSessions`), and the widget loader/UI. **Verdict: finish the Convex side (it's the "smarter backend" already routing to real retrieval), add only the public ingress as Channel Plane's first real component (visitor bootstrap + secret validation + write into existing `conversations`/`messages`), and port agenci's real `apps/embed`+`apps/widget` loader code + its good consent-capture UX verbatim.** Land it as `publicEnabled=true` on an existing Studio blueprint (e.g. "Build your own chatbot"/"Service agent"), not a sixth app — one more channel into the existing Inbox/HITL substrate.

**Corollary — do not let "combine Velion + Agenci" collapse into "build Intercom, but Norwegian and smaller."** Intercom already IS an omnichannel inbox + RAG agent + KB + analytics, at a scale and price no 3-person team beats on checklist parity (§3 table). A merged Velion+Agenci only earns its keep if it leads with what Intercom structurally cannot be: native Norwegian language quality (not translated), EU-native residency (not a US company's EU add-on), and visibly approvable execution (not a trust-us resolution rate). Test every "should we build this" question against §6.4's three properties — if the answer doesn't lean on grounding, approvability, or EU-native trust, it's parity work, not moat work.

### 6.5 "Velion Support" — the curated Intercom-equivalent (2026-07-20)

Decision: package the existing Inbox + Ticketing + AI-draft-HITL + embeddable-widget work (§1.3, §1.4, §6.4's agenci-inside-Velion plan) as a named, curated Intercom-equivalent — not a feature-parity clone. **Precision matters for the pitch:** the wedge is not "GDPR forbids Intercom" (false — Intercom offers EU residency/SCCs, and overclaiming this is the exact trust-misrepresentation failure mode flagged earlier in this audit). The real, defensible wedge is US CLOUD Act reach + Schrems-II-era sovereignty anxiety + Intercom's per-seat cost ($55–115+/agent on AI-capable tiers) — a combination Ayfie already proves converts at Norwegian enterprise scale (Oslo Børs-listed AYFIE; Telenor Norge selected it as GenAI platform) even though Ayfie has no inbox/ticketing/support-agent product at all. Velion's position: Ayfie's EU trust posture, with an actual support product Ayfie doesn't have.

**Take (already real or one step away):** shared omnichannel inbox (conversation-core-go); AI drafts with human approval (`inbox-ai.ts` → real Model Plane calls — the actual differentiator vs. Fin's "trust our resolution rate," since you show the paused tool call, source, and cost); embeddable website widget over RAG (§6.4's Convex + Channel-Plane-ingress plan, agenci's widget loader/consent UX ported in); ticketing with macros/SLA/automation rules (already real via tickets-client); copilot-style draft suggestions in the reply composer (extend inbox-ai; "edit-draft-before-approve" already on the Inbox follow-up list).

**Skip for v1 (real Intercom features, wrong fight for a 3-person team):** outbound/proactive messaging, banners, tours, checklists (saturated growth-tooling market); voice/phone; public help-center article pages; n8n-style workflow/routing builder (already decided: preset agents over canvas, same call applies here).

**Adopt the philosophy, not the mechanism:** Intercom's Fin prices per-resolution ($0.99) — don't copy the exact model, but "cost per resolved conversation, shown transparently" is already half-built via cost-core/Insights and is a claim neither Intercom (opaque black box) nor Ayfie (no support product) can make.

---

### 6.6a Cross-competitor pattern check (2026-07-20) — the doctrine is the region's playbook, not a compromise

Checked 6 Scandinavian/adjacent AI companies for shared structure: Ayfie, boost.ai (Sandnes-founded conversational AI, hybrid NLU+GenAI, chat+voice, "regulated industries trust," enterprise/demo-gated), Semine (Norwegian AP-invoice automation — ERP-integrated into SAP/NetSuite/D365/Visma Net/Xledger, approval routing by amount/cost-centre/entity, 10,000+ companies, Norwegian-airline+OBOS logos, Gartner+Deloitte recognized), Simplifai (Norwegian agentic AI for insurers, back-office-specific), Sana Labs (Swedish "superintelligence for work" knowledge/learning platform, Strava/Robinhood/Asics logos), DeepL (German, translation-only, the outlier).

**Meta-pattern, zero exceptions among the 5 Nordic ones: one narrow wedge, never a combined suite.** Ayfie=search only, boost.ai=conversational support only, Semine=AP only, Simplifai=insurance back-office only, Sana=knowledge/learning only. Zero functional overlap between any of them. **This is not a 3-person-team compromise — it is how every company in this exact market actually goes to market.** Two sub-patterns: (A) narrow workflow automation bolted onto existing systems of record — Semine/Simplifai/boost.ai integrate into the ERP/CRM/contact-center you already run rather than replacing it; Velion's architecture (own inbox+own KB+own agents+own search, replacing many tools at once) is structurally the *opposite* of pattern A, reinforcing why shipping one wedge deep (§5) rather than all 11 surfaces matters beyond team-capacity reasons. (B) horizontal bring-your-own-data AI layer — Ayfie/Sana, still single-function (search vs. learning). DeepL doesn't fit either pattern — not workflow-embedded, not vertical, pure point-utility on raw quality; treat it as the **quality benchmark** for any "native Norwegian language" claim, not a GTM peer.

**Universal thread, no exceptions:** trust/compliance/governance is the first marketing message, features second, across all 6 — confirming §6.5's "visible approval, EU-native" framing matches the actual regional category convention rather than being an invented differentiator. Also steal: Norwegian-named social proof as a trust signal (Semine names "Norwegian" the airline + OBOS + Deloitte's Norway-specific ranking) — do the same once Velion has more logos than Aquatiq.

**Sharpened 2026-07-20 — added trymimir.com and isolated the exact mechanism: two trust levers, every winning company has at least one, agenci has neither.** Mimir's homepage has no GDPR/security page at all (checked, zero results) — it doesn't need one, it's Oslo-based, sovereignty is implicit. Instead it markets the other lever explicitly: *"Complete control by default — choose between fully automated replies or AI drafts your team approves."* Cross-referencing all 7 (Ayfie, boost.ai, Semine, Simplifai, Sana, DeepL, Mimir): every one offers either **(a) an explicit sovereignty/compliance claim** (needed when the buyer can't assume it by default — Ayfie's Telenor partnership, boost.ai's security headline box, DeepL's "securely") **or (b) an explicit approval/control mechanic** (works even without loud compliance marketing, because national origin already implies sovereignty — Mimir's automated-vs-approve toggle, Semine's approval routing, boost.ai's "hybrid control" does both).

**This is the precise mechanism behind why agenci doesn't sell as currently built** (sharpening §6.2/6.3's earlier "sell the demo, deliver the substrate" call): agenci fails both levers. Sovereignty — its real audited stack is Clerk (US auth) + Convex (US BaaS) + OpenAI direct + Firecrawl (US crawl API) + Vapi (US voice) + Stripe, every byte of a customer conversation/RAG document routed through foreign-owned vendors by default with no EU-residency guarantee found in the code — this is an infrastructure fact, not a marketing gap, and cannot be fixed by better copy. Approval mechanic — agenci has escalate/resolve conversation states (human handoff exists) but nothing built or marketed as an explicit server-enforced draft-then-approve gate the way Mimir's toggle or Velion's own HITL does. Velion's planes already clear both bars (execution-core's real server-side approval pause beats a UI toggle; even with the Ayfie/Telenor sovereignty gap noted above, Velion's posture still beats raw OpenAI/Clerk/Convex). Confirms: rebuilding agenci's capability on Velion's substrate isn't abstract "own your stack" preference — it is the literal fix for the only two things every winning company in this exact market sells on, and agenci alone in this set has neither.

### 6.6 Curated-wedge doctrine, applied per competitor + backend layer (2026-07-20)

**Operating principle (explicit, team-stated):** Velion cannot and should not chase full feature parity with any competitor — pick the one best-in-class feature per category that users actually want, make it better on the axis that matters (grounding / approvability / trust), and consciously skip the rest.

**Correction first — the Ayfie+Telenor AI Factory partnership (press release, 2026-02-23) closes the sovereignty gap Velion hasn't closed.** Their offer: data stored and processed **in Norway, on Norwegian-owned-and-operated infrastructure** (Telenor AI Factory's sovereign AI cloud), RAG answers with citations, fast integration, model choice run in a closed environment, renewable-energy capacity. CEO quote: *"Many want generative AI but stop at confidentiality/privacy/digital-sovereignty requirements... a fully-Norwegian foundation, on Norwegian servers."* **Velion runs on Azure OpenAI Sweden Central — an EU/Nordic region, but Azure is a US hyperscaler regardless of region.** This is a real, currently-unclosed gap, not just competitive color: Ayfie can now make a sovereignty claim Velion cannot. Do not oversell "EU residency" as equivalent to this. Long-term open question worth tracking: a Norwegian-infrastructure partnership of Velion's own if sovereignty becomes an RFP gate. What still holds regardless: Ayfie is search/chat only — it has no acting, approvable agent product, no inbox, no ticketing. That gap is Velion's regardless of who wins on sovereignty.

**Per-competitor curated counter:**

| Competitor | Their one real pull | Velion's curated counter | Explicitly don't chase |
|---|---|---|---|
| Ayfie | Grounded answers + (now) full NO sovereignty via Telenor | Grounded answers *plus* an acting, approvable agent (Ayfie can't act) | On-prem enterprise motion; matching Telenor-grade sovereignty via engineering alone |
| Mimir | AI that acts in merchant backend systems | Same acting pattern with a visible approval step they don't show | Their 90% auto-resolve number — a safe 30-40% with an audit trail wins trust harder |
| Cobrief | NOK self-serve GTM, free viewer seats | Copy the pricing/GTM mechanics, not the tender vertical | Building anything tender/anbud-related |
| Taito | — | Price anchor only (€8-10/seat/mo = EU SMB seat expectation) | No surface overlap |
| Wonderful | Native-language quality, forward-deployed teams | Native Norwegian quality without the enterprise-only price floor | Their services/consulting layer |
| Attio | Agents that update records, not just chat | If CRM-lite ever ships (Studio Phase 3+): "agent acts on the record" | Building a general CRM now |
| Peec | AI-answer visibility tracking | Norwegian-language version, later, as a Velion module (§6.4) | Classic SEO (rank tracking/backlinks) — already lost to Ahrefs/Semrush |
| Intercom | Fin resolution-rate + composer copilot | Same shape, SMB-priced, visible paused-approved action instead of a trust-us number (§6.5) | Outbound/tours/banners, voice, help-center pages |
| Chatbase | Crawl-to-agent in <15 min, free tier | Match the time-to-value number exactly — it's testable | Multi-model marketplace — one well-routed model beats a picker |
| Gorgias | Per-ticket pricing, no seat tax | Consider per-resolution/per-ticket pricing as an SMB option | Deep Shopify-specific commerce actions unless asked |
| Zendesk | Ticketing completeness (SLA/macros/routing/CSAT) | Already matched in source (§1.4) — hold, don't extend | Full enterprise suite breadth (WFM/QA/voice) |

**Backend/infra layer — same doctrine, one level down:**
- **Model Plane vs. Claude Code / Hermes / Phi-style agent tooling:** don't build a general coding agent. Finish what's ~90% built — a durable, observable, **approval-gated** tool-use loop (execution-core already dispatches real tools + real HITL). Take Claude Code's lesson (reliable resume after a pause — the named P0), Phi's lesson (cheap small models for high-volume narrow tasks: triage, intent classification), Hermes's lesson (reliable function-calling from smaller/open models keeps cost-per-resolution down under ZDR constraints). Moat = tiered, cost-aware, approval-gated execution, not matching any one tool feature-for-feature.
- **Quarry v2 vs. Firecrawl / Apify:** Quarry already outputs clean LLM-ready markdown/JSON like Firecrawl — don't add complexity chasing it. From Apify, take the lesson not the breadth: rock-solid anti-bot resilience on the handful of sources a Norwegian SMB actually needs (own site, Brreg, Proff, LinkedIn company pages), not a general scraping marketplace.
- **Data Plane vs. GraphRAG / VisionRAG:** GraphRAG is already built (graph-index-rs) and differentiates against every competitor above — none have a knowledge graph. Priority isn't adding VisionRAG next (correctly already off, no MVP blocker) — it's closing the one real architecture violation: the embedding hop still falls back to direct Azure instead of routing through inference-core. That's the difference between "we own the whole grounded pipeline" being true vs. half-true.

## 6.7 Phase 0 execution log (2026-07-20, implementation pass)

Ran the audit-before-implementing discipline this doc itself prescribed. Result: **6 of 10 Phase 0 items were already fixed by the team before this pass ran** — confirms the "you may have already built this" warning completely, and means Phase 0's real remaining scope was much smaller than §5 assumed.

**Already fixed, verified by direct code/test inspection, not by this session:**
- IDOR closed: `x-velion-org-id`/`x-org-id` stripped globally at gateway ingress, org derived only from live session membership (`authorized_org_id()`, 107 call sites/31 files), regression-tested. Pre-existing commit `d8e11459`.
- documents-api-go: `AUTHCTX_ENFORCE=1` live and fail-closed by default in code too.
- velionv3 tests green (406/406 + 4/4); Quarry-v2 compiles + tests green (408/408).
- Fabricated trust UI: gone, regression-tested against reintroduction.
- Bonus finding: **inbox draft-edit-before-send already works** (was thought to be an open gap) — the real remaining gap is narrower, just the ticket-classification review queue lacking an edit step.

**Real gaps closed or in progress this session:**
- `/version` + git-SHA image labels: **done on all 5 slice services** — gateway (reference), model-gateway, quarry-edge, quarry-control, inference-core. All compile clean (independently re-verified: `go build`/`go vet`/`cargo check` all exit 0). Nothing staged or committed — left for review (one file, Ingestion `docker-compose.yml`, mixes the fix with an unrelated pre-existing tuning change and was deliberately left untouched).
- Quarry-v2 CI: added `cargo test --workspace` alongside the existing `cargo check --workspace` in `quarry-v2-ci.yml` — real test-gating closed.
- **Live smoke test still blocked — needs a human, not more automation.** Both automated attempts (chat/retrieval/inbox + IDOR-forgery-with-real-session) confirmed the code is right by inspection but could not complete a live HTTP proof: dev-bypass is correctly off on live containers (`ALLOW_DEV_AUTH_BYPASS=false`), and `local@velion.dev`'s documented fallback password returned 401 (the seed script requires an operator-supplied `SEED_DEV_PASSWORD` with no code default). Closing this gate requires the team to supply the real seed password, temporarily flip the bypass flag, or do one manual login as the actual proof — this is intentionally not something an agent should self-authorize.
- Editor note: a gopls "BrokenImport"/"UnusedImport" panel appeared against the quarry-control edit — verified false alarm (independent `go build`/`go vet` both clean, `os.Getenv` genuinely used twice). Root cause: quarry-control has its own `go.mod` with no top-level `go.work` linking it, so gopls loses all external-package resolution when the workspace root isn't scoped to that module — pre-existing monorepo quirk, not a regression.
- Docs synced: 12 files got dated correction banners (not rewrites) — `STATUS.md`'s stale "30/30 gates 100%" claim, Data Plane's stale observe-mode claim, Model/Ingestion's stale down-host claims, this doc's own IDOR claim.
- Minor: `WorkspaceSettingsPage.tsx` hardcodes placeholder org form defaults ("aquatiq-as" etc.) — cosmetic, not a trust fabrication, noted for later.

## 6.8 Phase 1 execution log (2026-07-20)

5 parallel workstreams, independently verified (typecheck/lint/full test suite, plus a manual `go build`/`go vet`/`go test` re-check on insight-core after a scary-looking but false gopls diagnostic — same missing-`go.work` artifact class as Quarry-v2 in §6.7, confirmed harmless both times).

**Shipped and verified:**
- **4 preset approvable agents** on the real Agent Run Console (`draft-reply-with-sources`, `summarize-new-inquiries`, `refresh-knowledge-base`, `triage-urgent-tickets`) — wired through the real `createSelectedAgentToolSpecs`/`streamChat` path so selecting a preset actually puts tool ids on the model-plane request (previously `runTask()` read `blueprintId` for display only and discarded it). HITL inherited from `action-registry.ts`, not reimplemented — no bypass. `AgentsPage`/`WorkflowBuilder`/`ChatbotStudio`/`ChatbotPlayground` and their honesty-enforced tests untouched.
- **2 of 3 Insight numbers wired to real data**: "conversations handled" (pure relabel of the already-flowing `inbox.tickets_resolved` scorecard) and "AI draft acceptance %" (real Go change in insight-core's `metric_subscriber.go` splitting one undifferentiated count into `ai_actions_approved`/`ai_actions_rejected` keyed on the actual `decision` field, 6 new tests). "Cost per resolution" correctly left in the honest-empty state — investigated both push and pull designs and found cost-core has no per-surface cost dimension and insight-core has zero existing integration with it; faking a ratio (org-wide cost ÷ tickets) would misattribute spend, so it wasn't built. Real follow-up: give cost-core's `Entry`/`AggregateFilter` a surface dimension, then wire insight-core to it.
- **Inbox review-queue edit — now fully functional end-to-end (fixed 2026-07-20, later same-day pass).** `AiActionReviewPanel.tsx` lets a reviewer edit AI-suggested category/intent/priority inline before approving, and the edit now actually takes effect: conversation-core-go's `ReviewAIAction` merges the whitelisted edited fields (`category`/`priority`/`severity`/`intent`/`team_id`/`team_name` — the frontend currently sends the first 3) into `payload.suggested_fields` via `jsonb_set`, atomically within the same transaction as the approve decision — no new DB round-trip. Double-gated on `decision=='approved'` at both the service layer and defense-in-depth in the SQL itself, so reject and no-edit-approve are provably unchanged (same `execCalls` count as before). `ai_action_executor.go`'s `promote()` needed zero changes — it already does a fresh DB read after the reviewed event fires, so it naturally observes the merge. 8 new tests, wire shape (`edited_fields`/`category`/`intent`/`priority`) confirmed byte-for-byte match frontend-to-backend, independently re-verified twice.
- **Nav hidden for the demo**: Agents builder section + 5 of 6 Insights sub-routes (kept Overview). Social and Studio were freshly re-verified as real (not stale-audit-assumed) and left visible. Single reversible flag (`DEMO_MODE_HIDE_UNFINISHED_NAV` in `sidebar-navigation.ts`) — flip to `false` for the open pilot, nothing else changes.
- **Caught and fixed a cross-workstream regression myself**: hiding the whole "agents" sidebar section also removed the only click-path to `/agents/runs` — where the new preset agents live. Root cause was deeper than expected: `CoreSidebar.tsx` special-cases `section.id === 'agents'` to render an entirely separate `AgentsExpandedSidebarPanel` component that never reads `sidebar-navigation.ts`'s generic items at all (it's a hardcoded role/feature switcher for the mock builder only, with no route navigation). Fix: repointed the Overview quick-link (`overview-agents`, which *does* render generically) from the mock `/agents` to the real `/agents/runs`, and un-hid it. Verified: `tsc --noEmit` clean, 13/13 targeted tests pass.

**Correctly not built — genuinely blocked, not a shortfall:**
- **Demo-data seed (Aquatiq crawl)**: thorough, honest investigation found no legitimate service-level ingestion path exists — the only two `quarry`-audience service principals are hardcoded to ZDR retention, and ZDR tokens are architecturally blocked from the durable-ingest write path; Aquatiq AS is confirmed provisioned as the intended non-ZDR demo org, but that path requires a real interactive session with no service-principal equivalent. Quarry-edge's own dev-bypass is on but was correctly declined — it maps to a stub identity unrelated to any real org, so using it wouldn't even land data in the right place. Verified ground truth directly: Data Plane v2's `documents` table has 0 rows for every org. **Needs a human**: same shape as the Phase 0 smoke-test blocker — someone logs in as a real Aquatiq-AS member once and triggers the crawl.
- ⚠️ **Security hygiene note**: investigating the above read live service-principal HMAC credentials via `docker exec` on local dev containers (already on disk in `.env.generated-secrets`, not newly exposed externally, but they passed through a subagent's tool output this session — worth being aware of).

**Net effect:** the wedge (crawl → KB → cited answer → inbox HITL) now also has preset agents and two real Insight numbers sitting on top of it, with every genuine remaining gap named precisely rather than papered over. Two items need a human, not more engineering: the Phase 0 smoke-test credential, and one manual Aquatiq crawl trigger for demo data.

## 6.9 Phase 2 + Phase 3 execution log (2026-07-20)

**Phase 2 — shipped:** a real, minimal feedback-collection widget (floating pill on every authenticated route → gateway → Inbox, tagged `demo-feedback`, idempotent, RBAC-gated). Reuses existing Inbox machinery, no new backend service. Verified end-to-end (Go/Rust/TS builds + tests), not yet checked against a live running stack.

**Phase 3 — fresh audit against 8 named gates, not trusting the original roadmap's characterization (matches the Phase 0 pattern: several items were further along or further behind than documented):**

**Closed, real, verified against fresh disposable Postgres:**
- Query-param IDOR (`onboarding/graph-preview?org_id=`) — confirmed already closed by the same commit (`d8e11459`) that fixed the header IDOR; added the missing regression test.
- **New tenant-isolation gap found and closed**: `org-core`'s `internal/rbac/repository.go` (role/permission-editor endpoints) had zero RLS scoping and zero per-request authz — its own comment admitted it trusted the frontend to check permissions. Now routed through `WithOrgScope`, backstopped by migration 013's fail-closed RLS policies. Verified against a real RLS-enforcing Postgres (not mocked).
- Two new cross-tenant regression tests: documents-api-go `Get`-by-known-id-from-another-org (404s correctly), user-core ACL grant isolation. Both genuinely pass against real Postgres.
- Inbox stuck-send reconciliation: a real sweep (5 min interval, 15 min stale threshold) now exists — atomically flips `sending`→`unknown` and the linked AI-action, using an index that existed for this exact purpose since migration 004 but was never queried until now.

**RLS reality check (materially better than the original doc, with one real caveat):** org-core's RLS is NOT "inert/gated/zero-callers" as previously documented — migrations 009/011/013 are ungated and fail-closed, `WithOrgScope` has 20 real call sites, and real Postgres integration tests exist proving cross-org isolation. The caveat: org-core's app connection role is Postgres **superuser**, which unconditionally bypasses RLS regardless of `FORCE ROW LEVEL SECURITY` — the entire enforcement burden rests on `WithOrgScope`'s `SET LOCAL ROLE` correctly firing on every sensitive call site. It does, for the 20+1 (rbac) paths checked. A handful of documented cross-org-by-design exceptions remain (admin list-all, GDPR erasure) — confirmed intentional, not oversights.

**Open — needs a human decision, not more engineering:**
- ✅ **RESOLVED 2026-07-21**: the undocumented `AUTH_CORE_INTERACTIVE_RETENTION_POLICY_JSON` ZDR exception (Aquatiq AS + one other org, no decision record) is removed. Stated business intent — ZDR is opt-in, paid, plan-gated, never the standard — is now the actual default: `interactive-retention-policy.ts`'s `DEFAULT_POSTURE` is `zdr:false`, and the effective posture is resolved live per org against org-core (stored self-serve toggle intent AND org-core's own plan-entitlement check, both required; any org-core failure fails closed to `zdr:false` without blocking login). See `apps/Control Plane/docs/core-research/auth-core.md`'s 2026-07-21 addendum for detail.
- Data Plane v2's erasure/DSAR purge subscriber: confirmed it only transfers ownership, never purges content, and this is a deliberate documented scope boundary, not a bug — needs a product/legal decision (does hard-delete purge only private documents or everything the user owns regardless of visibility? does anonymize purge any content at all?) before any code gets written.
- Data Plane credential rotation: confirmed still genuinely open (not just stale docs) — needs operator secret-manager access, out of agent scope by design.

**Open — structural, real but not urgent:**
- **CI does not actually gate any cross-tenant test today.** The workflows that would run them point at nonexistent paths (`services/user-service/**`, `services/org-service/**` don't exist in this repo) or live in a nested `.github/workflows/` folder GitHub Actions never reads (only repo-root `.github/workflows/` executes). Tests are real and pass locally; nothing blocks a merge on them yet.
- **Bonus find, unrelated to this pass**: `documents-api-go`'s idempotent-create path has a real bug — a repeated create with the same idempotency key returns the *second* title instead of reusing the first row. Reproduced twice against real Postgres, confirmed pre-existing (untouched by today's diff). Worth a follow-up fix.

## 6.10 Chat-engine verification + two chat-parity features (2026-08-01)

Independent pass, not a continuation of the same session as §6.7-6.9. Scope: verify the Model Plane hot path live (the standing gate §1.2/§2 had left open since 2026-07-20), then implement and live-verify two named chat-parity gaps from the competitive backlog.

**Verified live, not just by inspection:**
- Real, sustained interactive chat against the running stack: streaming answers via live Azure providers, tool loop, GraphRAG-fused retrieval, over multiple hours. This is the "one live probe" every prior pass (§1.2, §2 item 2) deferred to a human — it is now done, manually, not yet as a CI gate.
- Zero Data Retention re-confirmed at the precision this doc's ZDR mentions lacked: enforcement is complete and verified across all 6 Velion-side durable boundaries (session-core threads/messages, Dreaming/agent-memory extraction, response cache, implicit feedback, provider-side prompt cache, NATS/audit envelopes). The sole remaining gap is provider attestation (`AZURE_OPENAI_ZDR_CONFIRMED` / an Anthropic equivalent) — a signed-contract-plus-operator-flip task, not an engineering one. Full detail and a step-by-step production-readiness guide: `apps/Model Plane/docs/ZDR.md`.

**Shipped and live-verified (both were designed in an earlier pass, rejected on first design by adversarial review for data-corruption risk, and implemented from the corrected design once a live session became available — see `apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md`):**
- **Resumable streams.** The gateway producer used to treat a client disconnect (closed tab, reload, network drop) identically to a deliberate cancel — the run terminalized `Cancelled`, the answer never persisted, and a reconnect found nothing to replay. It now detaches instead: generation continues, the assistant message persists, and the run terminalizes `Completed`, so a reconnect replays the complete answer. Verified two ways: a deterministic Rust test that drops the SSE receiver mid-stream (fails against the pre-fix code) and a live browser test (reload mid-generation → full answer, no stuck spinner).
- **Edit/regenerate version navigation.** A client-only 1/N switcher over the final exchange's prior answers — session-lifetime, not persisted, so a nested version tree (the flaw that sank the first design) is structurally impossible. Live-verified with a non-deterministic prompt (paged through 3 real versions, each restoring its exact question+answer pair). Building it surfaced a real, previously-undiscovered bug: Regenerate had been silently *appending* a duplicate answer instead of replacing it since the feature shipped — fixed in the same change.

**New artifact — competitor research, not yet acted on beyond the above two items:** a 10-harness study (Claude Code, Codex, OpenCode, ChatGPT, Perplexity, Manus, Hermes, OpenClaw, Pi + expert consensus) produced a ranked chat-parity backlog. Top 5: server-authoritative sessions + resume (of which resumable streams above is the first slice), a gateway hook/permission layer, skills-as-files, citation chips + a quality gate, subagent fan-out. `apps/Model Plane/docs/VELION_CHAT_PARITY_BACKLOG.md`.

**Also fixed this session, adjacent to the chat engine:** agent-memory semantic recall (an embedding-dimension mismatch was silently zeroing every search — §1.2's "Semantic memory degraded" line is now stale, see above) and the Auth Core service-principal registry reconciled to the live fleet (14/14, no drift — relevant to this doc's credential-rollout concern in §2 item 4, though the broader rollout ritual itself was not re-run).

**Explicitly not touched by this pass:** IDOR/RLS/tenancy depth, insight-core wiring, Agents builder, Social live-proof, image provenance/SHA tags, credential rollout ritual, CI gating. All of §1-§6.9 stands as last verified on the dates given.

## 6.11 The rest of the week (2026-07-28 → 2026-07-31), read from diffs not messages

§6.10 covered one day's work by one pass. This entry covers the ~90 commits from the four days before it, reviewed by reading the actual diffs (not just commit subjects) specifically to catch cases where a commit message undersells or oversells what changed — several did.

### MCP server integration (2026-07-28) — went from "cannot work" to live-verified in one day
The starting state was worse than "gated": the existing `http` MCP transport POSTed to `/tools/list`/`/tools/call` paths that were never real MCP — a "bridge" that was never built. **No genuine remote MCP server could work at all.** The day's chain of commits fixed this in layers: a real OAuth 2.1 + Dynamic Client Registration client (connect a server from nothing but its URL, no pre-registration), AES-256-GCM-encrypted token storage, real MCP Streamable HTTP transport, tool auto-discovery (no manual allowlist), and — a separate, earlier restriction — MCP tools had been explicitly blocked from plain chat entirely (only execution-core's governed agent surface could call them); that restriction was lifted the same day. Several live-user bugs were found and fixed along the way (a missing `Bearer` prefix, a catalog upsert silently keeping the wrong server id, an internal token route wrongly gated by the per-user JWT check). **End state, confirmed live: a real Visma Net ERP query answered through plain chat.** Caveats that remain: HTTPS Streamable HTTP only (stdio/legacy SSE stay quarantined), only the streaming chat path (not the non-streaming JSON invoke), and token refresh was not load-tested under concurrent requests.

### The agent-loop-starvation P0 (2026-07-28)
`MAX_TOOL_ROUNDS` was 3. A self-describing MCP server (like Visma, above) spends its first rounds on `list_skills`/`get_skill` discovery calls before its first real query — leaving zero rounds to act on results or self-correct, so the model would work out the right fix and still have to hand it back to the user instead of applying it. Raised to 12 (configurable, ceiling 32). Fixing this alone would have worsened two latent issues, so both were fixed in the same change: tool results were being inlined into the prompt **untruncated** (now capped at 8k chars with an explicit "INCOMPLETE" marker), and every streamed answer was capped at 1024 tokens regardless of what the tool phase produced (now 4096). Two more defects surfaced while verifying: context assembly spent its whole budget on chat history first and dropped retrieval/knowledge/goal context entirely once exhausted (now proportioned); and a failed inference round previously streamed as a confident-but-silently-ungrounded answer (now tells the model the tool phase was cut short, so it says so instead of guessing). **Live-verified before/after**: the same question went from "3 tool calls and a question back to the user" to "12 steps, self-corrects, grounded answer."

### Chat identity, grounding, and shipping (2026-07-28) — no caveats, all live
- Plain chat previously **never requested knowledge grounding** — `buildChatWireBody` only requested the `tools` feature when the client declared explicit tool specs, which plain chat never does, so `knowledge_search`/`fetch_url` never attached regardless of what the user asked. Now always requested.
- Every chat request now carries the caller's verified org/user identity as system context, so "we/our" resolves to the org and "I/my" to the user — live-verified.
- `shipping.get_quotes` is now a real chat-callable builtin, not only an Agent-Console action — reached this state in two steps the same week (an earlier commit added it Agent-Console-only; a later one wired the tool-loop bearer and fixed a dotted-tool-name bug that had been silently breaking the *entire* request under the provider's tool-naming rules).
- Sole-org auto-activation on sign-in also landed this day (Better Auth was never setting `activeOrganizationId` for a single-org user).

### Reliability/security wave (2026-07-29)
- **Tool-routing chain, a real production bug**: the complexity scorer gave "tools offered" one point short of the threshold that routes to a tool-capable model, so an ordinary question with tools attached got answered by a model that declined to call any of them — and separately, even once floored onto the right model, the *answer* call re-resolved the model from scratch with tools withheld, so a 14-tool-call investigation got answered by a model that had never seen the tool definitions exist. Both fixed (hard floor + model carryover), plus a hardcoded Azure deployment id that 404'd whenever an operator's real deployment name differed, plus a real ordered provider fallback ladder (there was none before — one 429 killed the whole turn).
- **Cost-aware routing (Budget/Balance/Genius) was completely inert since it was built** — see §1.2's honest-gaps update, this is the single most consequential correction this pass found.
- **The confidence badge was hardcoded at 72%** — see §1.2.
- Two proactive security fixes with no actual exposure window: a cross-tenant cache key missing org/user (a timing-based existence oracle, never a content leak) and a bearer token sitting on a struct with `#[derive(Debug)]` (would have logged the raw JWT on the next `{:?}` call/panic — caught before any such log line existed).

### Hybrid-agent Wave 1 (2026-07-30) — see §1.8's update for the code-interpreter/canvas detail and the capability-attestation caveat.

### Durable orchestration + a research-quality pass (2026-07-31, the biggest single day) — see §1.8's update for the headline (7 workflows, zero callers → live cron-to-completion in one day). Also landed the same day:
- A Manus-style live agent panel in chat (reused the `BrowserChrome` devtools/replay UI that already existed for Knowledge, now mounted in chat too). Known limitation: chat-initiated runs don't yet register a gateway browser session, so real frames still 404 pending further wiring — the panel renders, the live view doesn't always have something to show yet.
- Deep-research quality: fetched pages were never actually surfaced as readable text (now fixed), a new relevance gate stops citing sources that can't answer the question instead of citing them anyway, and web-search queries are now built properly instead of from a raw conversational sentence.
- A real token-budget billing-accuracy fix: prompt budgets were charged via `len()/4` (byte-based), undercounting Norwegian/JSON payloads by 12–26% (risking silent provider truncation) and overcounting English by 60% (trimming grounding unnecessarily) — replaced with a real BPE tokenizer.
- ZDR posture was completely missing from the Go event leg's envelope (unlike the Rust producer, which already carried it) — completions were silently unmarked; fixed with an explicit tri-state field rather than defaulting to a guess.
- The velionv3 dev server had been OOM-crashing near its container heap cap with a healthy-looking container and an empty browser console — looked exactly like a UI bug, was actually the dev server dying silently. Heap raised, confirmed settled post-fix.

### Signal-quality loop: implicit dissatisfaction + quarantine (2026-08-01, adjacent to §6.10's chat-parity work but distinct)
A new control loop for the skill/capability registry: Regenerate, edit-resubmit, near-duplicate, and explicit-correction are now all implicit dissatisfaction signals (Regenerate and edit-resubmit were dead code client-side until this day — only explicit correction could fire before), scored with a Wilson-95%-lower-bound estimate per skill (explicit ratings full weight, implicit signals discounted). Below a threshold, a skill is **quarantined** — meaning its injection into future prompts is disabled via a dedicated RPC, not that any memory or content is deleted. Hysteresis prevents flapping; capped at 5 quarantines per sweep. Wired end-to-end, but recovery from quarantine is deliberately manual for now (the system can't yet distinguish a policy quarantine from a human deliberately disabling a skill).

**Net effect of the full week**: several previously-documented "live" capabilities (cost-aware routing, the confidence badge, MCP tool execution, delegated subagents, durable Temporal workflows) were either completely inert or outright faked, and are now real and live-verified — this is a materially different state than any prior pass in this document described, not an incremental refinement.

## 7. Appendix

**Doc trust guide** (for future audits): trust 2026-07-13+ correction banners as the current layer. `apps/STATUS.md` (2026-04-23) is the most misleading doc in the repo — "30/30 gates closed" measures crate tests, not runtime, and predates the discovery that the hot path was down; update or delete it. Feb-2026 "Production Ready v1.0.0" READMEs (Ingestion, Model) are formally retracted by their own July docs. `docs/core-research/mock-backed-surfaces.md` is flagged stale. Eight stale Ingestion docs await sign-off in `STALE_DOC_DELETION_REGISTER.md`.

**Known red tests to fix in Phase 0:** velionv3 `pnpm test` (loadStudioWorkspace unhandled rejection when session lacks `orgs`); Quarry-v2 workspace compile (`DataPlaneIngestRequest` missing `initiator_user_id`/`visibility`).

**Things that are better than you think** (keep morale honest too): the frontend honesty contract is genuinely unusual and good; ticketing SLA/macros already exist; the HITL gate is real server-side enforcement, not decoration; GraphRAG + wiki + source traces is a real cognee-class substrate; the inbound pipeline is proven channel-agnostic; the gateway has zero mocks across ~49 domains. The team's instinct that "features don't work" is a *deployment* problem wearing a feature costume.
