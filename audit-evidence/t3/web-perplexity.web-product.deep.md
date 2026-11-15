# Perplexity (web product) — deep dive for Verevon chat/agent pages and Model Plane

Audit 3, facet `web-product`, source: public perplexity.ai product, help center, changelog, docs.perplexity.ai, engineering blog. Researched 2026-10-13. All claims are DOC unless tagged INF. Vendor benchmark numbers are reported as vendor claims, not verified. The public pricing page (perplexity.ai/pricing) returned 404; prices/limits come from help-center articles. The Comet-specific help centre redirected to a landing page without article bodies, so Comet Assistant internals are thinner than the rest.

Companion file: `web-perplexity.web-product.features.json` (112 records).

## 1. Product shape in October 2026

Perplexity has split into two products that share one account, one composer, one memory and one connector layer (DOC: help center "What is Computer?", changelog Feb–Oct 2026):

1. **Ask / Search** — the classic citation-first answer engine. Modes: Standard, Pro Search, Research (Deep Research), Create files and apps (ex-Labs), Learn, Model Council. Unlimited and credit-free on every plan; plan tiers cap Pro Search / Research / file creation counts.
2. **Computer** — the agent. "Perplexity answers your questions. Computer does your work." Orchestrator + parallel subagents, sandboxes, connectors, skills, automations, artifacts, website publishing, Projects with Git-backed filesystems, Brain memory. Metered in credits (100 credits = $1). Launched 2026-02-25 (third-party: Semafor); by Oct 2026 it is available on all paid plans and even trialable on free US accounts via "included turns".

Surfaces: web, iOS, Android, Mac/Windows desktop ("Personal Computer"), Linux local-only ("Portable Computer"), Comet browser, Slack, Teams, email ingress, Stripe CLI, and the developer API (Agent, Search, Orchestrator, Embeddings, Decisions).

Cadence: the official changelog shows a shipped-features post roughly every 2–4 weeks with 8–15 items each (Jun 18, Jul 13, Jul 27, Aug 4, Aug 24, Sep 21, Oct 5 2026). This is the fastest public release cadence among the benchmarks in this audit (INF, basis: changelog index).

## 2. Architecture and data model (what is public)

### 2.1 Answer/citation data model
The Agent API exposes what the web UI consumes (INF, basis: identical field shapes): a `search_results` output item (id, url, title, snippet, date, last_updated, source), `fetch_url_results`, and a `message` with `output_text`. Citations are inline markers `[web:n]` bound to result ids. `usage` includes USD cost split into input/output/tool_calls/cache_read plus per-tool invocation counts (DOC: docs.perplexity.ai agent-api quickstart). Perplexity's own guidance: never trust links the model writes into JSON; use the citations list (DOC: output-control).

Verevon takeaway: the Rust gateway should stream a typed event stream where citations reference a structured result list, and the SolidJS UI renders chips from that list rather than parsing markdown.

### 2.2 Projects = Git repositories on S3Files (engineering blog 2026-10-12)
This is the most architecturally detailed public document and the best-in-class design in this audit:

- Each Project is backed by a **Git repository**; content-addressed files are stored once; commits are atomic so collaborators never see half-finished state.
- Components: a **CLI in each sandbox** (checkout, sync, submit), a **control plane** that locates repos and mints short-lived credentials but never touches repo data, a **Git service** (ref discovery, fetch, receive, LFS links) that decides the accepted head, **storage** on AWS S3Files split into isolated cells with stateless replicas, and a **downstream parser pipeline** that extracts text from PDFs/Office files and stores it outside history.
- **Concurrency**: compare-and-swap on head; stale-base submissions are replayed on the new head by the agent and resubmitted. <2% rejected; median submit <1.7 s, p99 <8 s.
- **Lazy checkout**: agents materialise only needed files; LFS pointers reveal size so the agent decides whether to download. A FUSE virtual FS was tried and rejected (~400x slower).
- **Compaction** folds old commits into a snapshot; an **outbox** records every accepted write (repo, commit, author, paths) for downstream services and audit.
- Agents never coordinate directly; they only share state through the repo. Interactive, scheduled and teammates' agents all commit to the same repo from their own sandbox.
- **Exploration beats semantic search** on their internal benchmark: agents using ls/grep/read over parsed text scored 0.867 retrieval / 0.627 answer vs ~0.39/0.37 for semantic search, at 25% lower latency (150 single-document finance questions).
- **Brain** lives in *separate* knowledge repositories so background memory writes never contend with project edits; a session loads both when needed.

Verevon mapping: this is a concrete blueprint for the Application Plane workspace + Model Plane sandbox hand-off. It also respects Verevon's "no cross-plane DB access" rule naturally: the control plane only brokers credentials.

### 2.3 Sandboxes (SPACE) and safety stack (blog "How we engineer safer agents", 2026-09-29)
- Every Computer task runs in its own **Firecracker microVM**; credentials stay outside the VM and are injected when needed; egress is controlled at node level; snapshots allow rollback; enterprises can bring their own keys.
- Six layers: intent anchoring (untrusted content marked), **BrowseSafe** classifiers screening retrieved content in parallel with reasoning (open-sourced with a benchmark), containment, independent monitoring (**Numbat** on endpoints, real-time audit logs), an improvement loop (Computer proposes rule updates; humans approve), adversarial testing (Trail of Bits, bug bounty).
- Design rules: layers must fail independently; at least one deterministic layer enforces below the agent; signals can only reduce authority.
- On-device Portable Computer uses a **deterministic (non-LLM) orchestrator** to enforce policy; if the OS sandbox is unavailable the harness disables itself.
- Limitations they admit: nine frontier models given root could not escape the VM but some bypassed network policy when partially allowed (fixed); no published false-positive/coverage figures.

Verevon mapping: Verevon's "Model proposes, Quarry executes or rejects" is exactly "enforcement below the agent". Missing pieces vs Perplexity: input screening of retrieved content, explicit untrusted-content marking in prompts, and snapshot rollback in sandboxes.

### 2.4 Secrets
Custom API credentials are encrypted and injected by a **proxy** on outbound requests; the raw value never appears in agent activity or the sandbox; scopes are session / personal vault / project-shared; saved credentials require per-use approval unless "Always allow" (DOC: help center). This is cleaner than most peers and directly reusable in Model Plane.

### 2.5 Tool permission model
Connector tools have three states: Always allow / Always ask / Never allow. When paused the user can approve once, allow for the rest of the thread, or deny; recurring automations inherit thread-level grants (DOC: changelog 2026-08-24). Comet adds per-domain Browser Control / Read Only / No Access with admin precedence rules (DOC: help center). Confirmations, clarifying questions and connector sign-ins render inline in the composer (DOC: changelog 2026-06-18).

### 2.6 Run lifecycle (API view)
`background: true` keeps a run alive after disconnect; clients poll `GET /v1/agent/{id}` or resume the SSE stream with `starting_after=N` inside a reconnect window (DOC: output-control). Presets (`fast/low/medium/high/xhigh`) expand to model + max_steps + reasoning effort + tool configs + system instructions; xhigh forces loading a `pplx_sdk` skill for fan-out research (DOC: presets). This "search as code" approach is also what lifted Computer's search reliability from 81.9% to 92.6% (DOC: changelog 2026-08-24).

### 2.7 Memory
Two layers: **Memory** (preferences/facts + search history; citable as sources; manageable bank; off in Incognito) and **Brain** (background wiki/graph of projects, people, docs, decisions, open items; every entry links to its source; browsable as Concepts/Entities/Workstreams; editable; stale-marking; 15 languages; catalog prefilled into every session). Claims: +25% correctness, +16% recall, −13% cost on tasks with prior context; 10-point lift on memory benchmarks (vendor). Brain is Max/Enterprise Max only and in Research Preview.

## 3. Best-in-class (what Verevon should copy)

1. **Citation UX stack**: structured results → inline markers → sources side panel with passage view → source labels (Government/Academic/Trusted) → **Check Sources** claim-vs-evidence verification. Nobody else in this audit has the last two.
2. **Git-backed shared workspace** (2.2) with optimistic concurrency, lazy checkout, outbox audit, and memory in separate repos.
3. **Effort Mode** (Light/Standard/High/Ultra + Custom): one slider abstracts model + reasoning depth; selector shows the paired model; does not cap steps/tools; "higher is not automatically better" guidance. Same idea in the API as presets.
4. **Model Council**: parallel fan-out to 3 (web) or 2–8 (Computer) models with a synthesiser that reports agreement/disagreement/unique contributions, per-model Thinking toggles, side-by-side view, per-model reports as files.
5. **Automations** that are event-triggered (Slack/Gmail/Outlook/Linear/GitHub) with conditions, remember prior runs, pause for review before consequential actions, cost nothing while idle, and notify only when something changed.
6. **Three-state tool permissions + thread-level grants + inline approvals**, and **proxy-injected secrets**.
7. **Advanced Deep Research UX**: clarifying questions, add questions mid-run, live progress of sources being read, interim key findings, report streamed into an editable file.
8. **Transparent cost**: live credit counter during tasks, per-model analytics for individuals and orgs, free analytics API, explicit draw order of credit pools, spending caps.
9. **Side Chat**: read-only parallel chat over a snapshot of a running task.
10. **Fork a session with assets**, mid-task orchestrator switching, user-chosen compaction (keep full vs summarise).

## 4. Weaknesses and limitations (documented or widely reported)

- **Credit opacity / burn**: third-party reviews and Reddit reports describe 15,000 credits consumed in one 40-minute task and silent failures costing $200 (INF, basis: WebSearch of reviews; not verified). Perplexity's own docs say effort is "not a spending cap" and remaining image/file allowances are not shown in settings.
- **Vague consumer limits**: Pro/Max limits are described as "average use"/"advanced use" without numbers; Education Pro text contradicts its own table.
- **Pages retired**: "Create page" and "Convert to Page" are temporarily withdrawn; the Research article still advertises it (stale doc).
- **Focus/source mode cannot change mid-session**; Web on/off only at session start.
- **No manual in-preview editing** of generated assets beyond region-scoped prompts; no slide templates/themes.
- **Video**: 8 s, no editing, no add-on purchase.
- **Brain** is Max-only and cloud-only (not available in Portable Computer); Model Council on web is Max-only.
- **Help-center drift**: duplicated path segments 404; article slugs still say "thread" while UI says "Session"; Workflows → Skills migration mid-flight; org file repository retired 2026-10-01.
- **Local MCP** only on Mac, "not officially recognised"; remote MCP for consumers still "coming soon" in one article while enterprise custom remote connectors already exist (inconsistent docs).
- **Safety metrics** not published (false positives, coverage).
- Coding is reported as a weak spot for Computer (INF, third-party reviews).

## 5. Gaps versus Verevon (based on the baseline files in this findings folder)

Verevon's chat/agent pages and Model Plane already have sessions/runs, sandboxes, browser grants via Quarry, cost tracking and Data v2 retrieval. Relative to Perplexity the notable gaps are:

- No Check-Sources style post-hoc verification run; no source trust labels in citation chips.
- No effort-preset abstraction over (model, reasoning) with a visible paired model.
- No fan-out multi-model council run with synthesis.
- Automations: event-triggered runs with per-run memory and pause-for-review are a Model Plane + integration-corev2 feature Verevon should add; "silent when unchanged" notifications belong to the Application Plane.
- Git-backed Space filesystem with CAS head and agent-side rebase; Verevon's Spaces must keep ACLs non-widening — Perplexity's "Projects cannot be made publicly queryable" and request-edit-access flow are compatible patterns.
- Proxy-injected credentials for sandboxes.
- Side Chat over a run snapshot; fork with artifact lineage; user-chosen compaction prompt.
- Skills in SKILL.md convention with org catalog + admin approval.
- Background/resumable run streaming with sequence numbers in the gateway API.

## 6. What to avoid

- Credit-based pricing without a hard per-task cap and without live remaining-allowance display (biggest user complaint).
- Locking fundamental quality features (council, memory) behind the top tier if Verevon wants enterprise trust.
- Letting docs drift from product naming (thread/session/space/project).
- Client-side browser agents (Comet) for enterprise governance; Verevon's server-side Quarry executor with per-domain policy is easier to audit.

## 7. Open questions

- Internal protocol between web client and Computer runs (likely SSE like the API; not public).
- How Brain's background runs are budgeted and scheduled; how conflicts between Brain edits by users and background agents resolve (Git semantics implied).
- Whether a true Zero-Data-Retention mode exists for consumers; only Enforce-Incognito (24 h) and configurable retention are documented for enterprise.
- Photon index internals (post 404ed at fetch time).
- Exact Comet Assistant internals (sidecar, @tab mentions) — Comet help centre bodies were not reachable.

## 8. Key evidence index

- Changelog index: https://www.perplexity.ai/changelog (entries 2026-06-18 … 2026-10-05)
- Projects engineering post: https://www.perplexity.ai/hub/blog/building-shared-workspaces-for-people-and-agents
- Safer agents: https://www.perplexity.ai/hub/blog/how-we-engineer-safer-agents
- Automations: https://www.perplexity.ai/hub/blog/computer-adds-automations-for-ongoing-work
- Help center collections: Features, Computer, AI Models, Connectors, Comet, Enterprise, Privacy, Desktop App (https://www.perplexity.ai/help-center)
- API: https://docs.perplexity.ai (agent-api quickstart/presets/output-control/skills/tools, search quickstart, router quickstart, pricing, models)
