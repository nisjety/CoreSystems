# claude.ai (web product) — deep dive for Audit 3 (Model Plane + Verevon chat/agent pages)

Source: public web only (claude.com product/pricing pages, support.claude.com help center incl. the Release notes article, anthropic.com news, code.claude.com docs, plus a few press items). Researched 2026-10-13. Provenance tags: DOC = read in official documentation/help/product pages; INF = my inference (basis stated). No REA or SRC in this facet. Companion file: `web-claudeai.web-product.features.json` (127 records).

Key URLs (abbreviated below):
- REL = https://support.claude.com/en/articles/12138966-release-notes
- ONE = https://support.claude.com/en/articles/16761823-claude-cowork-and-chat-are-one-claude
- ARCH = https://support.claude.com/en/articles/14479288-claude-cowork-architecture-overview
- ENT = https://support.claude.com/en/articles/17444462-set-up-and-manage-the-new-unified-claude-experience-for-your-enterprise-organization
- ART = https://support.claude.com/en/articles/9487310-what-are-artifacts-and-how-do-i-use-them
- PRICING = https://claude.com/pricing

---

## 1. What claude.ai is in October 2026 (DOC)

Since 2026-09-16 Anthropic has collapsed "Chat" and "Cowork" (its agentic knowledge-work product, launched as a Max-only macOS research preview on 2026-01-12) into one conversation surface: the user types in one composer and Claude decides whether to answer directly or to spin up an agentic task that searches, reads files, runs code and returns editable files (ONE, REL). This is the single most important fact for Verevon: the reference product is no longer "a chat app with an agent mode" but "one conversation that escalates to a sandboxed run when needed". The old Cowork docs still exist and describe the underlying machinery (ARCH), and the Enterprise article (ENT) exposes most of the runtime contract.

Timeline that matters (REL):
- 2025-08/09: file creation (xlsx/pptx/docx/pdf) in a sandbox; memory on Team/Enterprise; incognito; Chrome extension experimental.
- 2025-11-24: automatic context compaction.
- 2025-12-18: Skills for organisations + open Agent Skills standard; Chrome gets desktop control, workflow recording.
- 2026-01: Cowork research preview; interactive MCP apps in chat (2026-01-26, press).
- 2026-02: scheduled tasks, plugin marketplace, Analytics API, self-serve Enterprise.
- 2026-03: interactive visuals inline; computer use research preview; mobile interactive apps; memory for free users.
- 2026-04: Cowork GA + OpenTelemetry; Claude Design (Labs); RBAC.
- 2026-06/07: Claude Tag (Slack agent), Claude Science, edit drafts in place, Cowork on web/mobile with cloud sessions, topic memory, monthly recap, M365 write tools.
- 2026-08: memory shared with cloud Cowork; skill/plugin scanning.
- 2026-09: unified experience; Docs/Slides/Design as typed artifacts on all plans; Projects redesign with parallel cloud threads (Claude Code first); plugin developer portal; 5.5 model family.
- 2026-10: Dashboards + Motion beta; monthly API credits for Max/Team; Haiku 5.5.

## 2. Architecture (DOC unless marked)

### 2.1 Execution modes
ARCH documents two modes with the same architecture on every plan:
- Cloud session (default): agent loop + code execution in a per-session sandbox on Anthropic-managed infrastructure, created at session start, destroyed at end, no state shared across sessions or orgs, separated from Anthropic corporate/research/training networks. Default-deny reachability to private/link-local/metadata/Anthropic-internal addresses; all egress through a mandatory proxy with an allowlist; only session-scoped tokens that expire within hours live inside.
- Local session (desktop): native agent loop on the device with an application-layer permission system for folders and egress; shell/code in a Linux VM under Apple Virtualization.framework or Hyper-V with its own egress filter, syscall restrictions and per-session user; if the VM dies, file and web tools keep working and shell reports "workspace unavailable".

ENT adds the lifecycle for the unified experience: each conversation that needs agentic work gets its own Anthropic-managed VM; it stops after a few idle minutes and its disk is saved as an encrypted snapshot; sandboxes expire 30 days after creation and the conversation continues in a fresh environment. Retention follows the conversation (30-day minimum custom retention) and cloud sessions are deleted with it.

INF (basis: ARCH + ENT): the "conversation" is the durable object; "sessions/sandboxes" are attachable, snapshot-able leases. This is exactly the sessions/runs split the Model Plane already has; the snapshot-restore-with-TTL is the piece to confirm Verevon has.

### 2.2 Desktop bridge
Cloud sessions reach local folders and the browser through the Claude Desktop app over an "Anthropic-brokered connection" (protocol unnamed). Every local tool call is checked against the member's folder permissions before running; offline app means no device access. Admins can require Trusted Devices and disable remote control (ARCH, ENT). This is the same shape as CoresSystem's "Model proposes, Quarry executes or rejects": the executing side holds the policy.

### 2.3 Connectors and tool gateway
Connectors are MCP. Authorization is server-side so connector tokens never enter the sandbox; org tool policies (Always allow / Needs approval / Blocked, per tool or category) are enforced "outside the sandbox on every call" (ARCH, ENT, https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities). Custom remote MCP servers must be reachable from Anthropic's cloud (public URL, allowlist Anthropic IPs). Local MCP servers and desktop extensions (MCPB) run only in local sessions via the app, and MDM keys `isLocalDevMcpEnabled` / `isDesktopExtensionEnabled` can block them.

Tool loading is explicitly context-managed: the per-conversation "Tool access" setting offers Auto (Claude chooses), Always available (all connectors loaded; recommended under 10), and On demand (Claude searches for relevant connectors first; recommended at 10+ or near length limits) (https://support.claude.com/en/articles/13730515-manage-claude-s-tool-access). INF: this is deferred tool schemas plus a tool-search step, the same mechanism Claude Code uses (ToolSearch).

Interactive MCP apps (MCP Apps, SEP-1865, first official MCP extension) let connectors render live UIs inside the chat: Canva, Figma, Slack, Asana, Hex, Amplitude, monday.com, Box, Clay, Salesforce at launch 2026-01-26 (press: thenewstack.io, sdtimes.com; marketplace marks them "Interactive").

### 2.4 Artifacts runtime
Artifacts moved from "code blocks rendered beside the chat" (legacy, still viewable/publishable/embeddable) to typed templates on 2026-09-16: Docs, Slides, Design, Dashboards (beta), Motion (beta) (ART). The runtime offers three capabilities that Verevon should copy almost verbatim:
1. Artifacts may call Claude; viewers sign in with their own account and usage is charged to the viewer's plan; creator pays nothing; no API keys.
2. Artifacts may read/write connected apps with per-tool approval; tools that require per-action approval are excluded; each viewer connects their own apps; owners can disable org-wide.
3. Personal or shared storage, 20 MB text per artifact; a confirmation appears the first time shared storage is used; unpublishing a legacy artifact permanently deletes its data.
Shared artifacts bind to the viewer's connectors, not the creator's; a viewer lacking access sees an error in that part (live-artifacts article). Export paths: Docs to DOCX/PDF/MD/Google Docs; Slides to PPTX/PDF/Google Slides; Design to ZIP/PDF/PPTX/HTML or Canva/Miro/Vercel/Netlify; Motion to MP4.

Docs is a real-time multi-user editor where Claude is a collaborator: edits attributed to a person or to Claude, @Claude in a comment performs the edit and replies in-thread, Claude leaves its own comments while drafting, tabs, version history (https://support.claude.com/en/articles/16923645-get-started-with-claude-docs). INF: a CRDT/OT document store with an agent participant; Verevon's Application Plane realtime projections are the natural host.

### 2.5 Memory
Memory is stored as individual categorized topics as you chat (replaced the 24-hour summary on 2026-07-10). Settings > Memory lists every topic for edit/delete; chat commands ("remember/forget") also work; pause keeps entries but stops use; reset deletes everything including project memory. Each project has its own memory space and project summary. Sensitive topics (health, politics, religion, gender identity) are excluded unless the user opts in, with a notice on save; government IDs, financial account numbers, criminal history and immigration status are never stored. Incognito chats bypass memory/history/search; "+" > Memory off excludes a single chat. Memory is shared with cloud Cowork; local-only tasks keep separate memory. Team/Enterprise: owner enables org-wide, member opts in, owners cannot read members' memory; turning off org-wide deletes all memory; not available under HIPAA/public-sector/custom retention (MEM article; REL).

### 2.6 Models and routing
Model picker + effort (low/medium/high/xhigh/max) + thinking toggle are orthogonal; 5.x models cannot have thinking disabled in the app; per-model defaults marked; Enterprise admins restrict models/effort per role and set defaults; "Efficient mode" forces Sonnet/Haiku at low effort (model article; ENT). Automatic safety fallback: classifiers scan the request and everything the model reads (memory, connector output, search, files); flagged categories switch Fable to Opus 5 / Opus 4.8 with a visible notice and per-response model label; users can disable switching, in which case the chat pauses (https://support.claude.com/en/articles/15363606-...). Thinking is shown as a summary, and "distillation attacks (including attempts to extract summarized thinking)" are a blocked category, which explains why raw reasoning is never exposed.

### 2.7 Observability and governance
- OpenTelemetry event export (Team/Enterprise): full prompt text, tool/MCP name + parameters + success + duration, file paths touched, skills/plugins invoked, approval decisions (approved / user-rejected / automatic), API request model/tokens/cost/errors; `prompt.id` correlates a turn; OTLP HTTP JSON or protobuf; headers encrypted at rest (https://support.claude.com/en/articles/14477985-...).
- Compliance API (Enterprise): activity events, chat data, file content across Claude, Cowork, Code, M365 (beta), Science (beta); excludes Claude Code cloud sessions and Bedrock/Vertex.
- Audit logs: 180-day export, entity/event model, content excluded.
- Inference hooks (Enterprise beta): every prompt, tool response and uploaded file text is POSTed to a customer server which returns allow/deny before generation; runs inside Anthropic infra; covers all surfaces and tool calls (https://support.claude.com/en/articles/16059458-inference-hooks-overview).
- Permission modes: Manual (default; asks before each action), Auto (automated safety checks per action, pauses when needed), Skip all approvals; admins can disable Auto and Skip; permanent deletion always asks; computer use asks per app (SAFE, ONE, ENT, Chrome permissions guide).
- Network egress tiers for the sandbox: off / package managers / package managers + domains / all; defaults: consumer on with network, Team package-managers-only, Enterprise off. Egress rules do not govern web search/fetch, MCP or Chrome.

## 3. Data model (INF, basis: documents above)

- Account -> Organisation (roles, custom roles, groups, spend limits) -> Member.
- Conversation (chat or task; the durable unit; retention clock; may own a VM lease with snapshot; may belong to a Project; has permission mode; has memory flag; may be incognito).
- Project (instructions, knowledge files with cache + auto-RAG, own memory namespace, sharing ACL; new Projects beta = one conversation + parallel cloud threads with separate branches/repo copies).
- Artifact (typed: doc/slides/design/dashboard/motion/legacy; owner; sharing level; comments; version history for Docs; optional personal/shared KV store; optional capabilities: claude calls, connector tools).
- Design system (artifact reused by all chats; tokens + components; org default; admin publish rights).
- Memory topic (category, text, created-from conversation; project-scoped or global; sensitive flag).
- Scheduled task (name, prompt, approval mode, cadence, model, working folder; runs = sessions).
- Connector (directory or custom MCP; auth mode; per-tool policy; org-managed auth via IdP roles/scopes).
- Skill (folder, SKILL.md frontmatter name/description/dependencies; source: Anthropic / custom / org-provisioned / partner / plugin).
- Plugin (skills + connectors + commands + sub-agents + hooks + MCP servers; marketplace; statuses installed-by-default / available / required / unavailable).
- Usage (5-hour session window, weekly window, Fable 50% cap, credits, bundles, monthly API credits, resets).

## 4. Protocols and standards (DOC)
- MCP for connectors; MCP Apps (SEP-1865) for interactive UIs; MCPB for desktop extensions; Agent Skills spec (agentskills.io) for skills; OTLP (HTTP JSON/protobuf) for telemetry; OAuth with Claude's published client identity or dynamic registration or customer client; IdP-managed authorization (Okta first) for org connectors; GitHub App + credential proxy for Code on the web; Git worktrees for parallel local sessions; `.claude/launch.json` for browser preview in Code Desktop. The desktop bridge protocol is not named.

## 5. Best-in-class (what Verevon should copy)
1. One composer, no modes; escalation to a sandboxed run is implicit (ONE). Combined with the message-box permission selector (Manual/Auto) this is the cleanest agent UX among the references.
2. Sandbox contract: per-conversation VM, idle stop, encrypted snapshot, 30-day expiry, default-deny network + proxy allowlist, short-lived scoped tokens, connector execution outside the sandbox (ARCH, ENT). Model Plane should match this line by line.
3. Artifact capabilities: viewer-paid AI calls, viewer-scoped connectors, personal/shared storage with explicit confirmation (ART). It turns artifacts into tiny apps without a developer platform.
4. Topic memory with full inspectability, sensitive-topic policy and hard never-store list (MEM). This is the most trustworthy memory design among the references (Hermes is more adaptive, Claude is more auditable).
5. Tool access modes / on-demand tool loading (tool-access article) and skills with progressive disclosure (SKILL.md metadata -> body -> referenced files).
6. Telemetry taxonomy (prompt.id, tool_parameters, approval decisions) and inference hooks; this is the enterprise transparency Verevon's DeepSeek-harness reference wants, delivered as org controls.
7. Safety-fallback routing with visible labels (model-switch article): policy-driven routing that is honest in the UI.
8. Record-a-skill from screen recording (custom skills article): the product literally learns procedures from the user.
9. Dashboards showing each chart's SQL and refresh time (dashboards article).
10. Claude in Chrome specifics: workflow recording, console/network/DOM access, tab groups, 1Password biometric handoff so the model never sees secrets.

## 6. Weaknesses and limitations (DOC unless marked)
- No native image generation (help article is explicit); visuals are HTML/SVG only. Verevon with Gemini/OpenAI image models can beat this.
- Branching is not available in the unified experience; even in the legacy path the tree is invisible (third-party extensions exist). Verevon can ship a real branch map.
- Incognito chats fall back to the legacy experience (no files/code), and ZDR / HIPAA / CMEK orgs cannot use the unified experience, memory, Code Review or live-artifact editing. Claude's privacy-maximal configurations lose most agent features. A ZDR-first Model Plane is a genuine differentiator.
- No folders/pins/archive for conversations; sidebar cannot be hidden; theming is minimal (light/dark/system, one accessibility font).
- Voice is STT -> text model -> TTS (INF from "your speech fills the chat input"), not a realtime speech model; Fable not available in voice.
- Many agentic features require Claude Desktop to be open (built-in browser, computer use, local files) even when the session is in the cloud; web/mobile are thin for local work.
- Search does not cover old Cowork tasks; "Add from GitHub" missing in unified experience; Dispatch closed to new users; Design has no version history; Docs has no comment-only role and charts don't auto-update.
- Documentation drift: the help centre has duplicate/overlapping articles (two artifact articles, Cowork articles describing a mode selector that is gone) and several inconsistencies (e.g. project instructions "paid only" vs projects on free). Treat dated statements accordingly.
- Usage model complexity: 5-hour + weekly windows, Fable 50% cap, credits, bundles, resets, monthly API credits; users need a dedicated page to understand it.
- Prompt injection: documented as non-zero residual risk in browser, computer use and scheduled tasks; computer use has no sandbox at all.
- UI language list lacks Norwegian (11 languages).

## 7. Gaps versus Verevon chat/agent pages and Model Plane (INF, basis: task context description of CoresSystem; no code read in this facet)
Likely present in Verevon already: sessions/runs, sandboxes, browser grants via Quarry, cost tracking, Space-scoped ACL, ZDR propagation. Likely gaps to verify in the code audits:
- Per-conversation sandbox snapshot/restore with TTL and idle stop.
- Typed artifact runtime with viewer-paid AI calls and viewer-scoped connector access; shared KV storage per artifact.
- Topic memory with sensitive-topic policy, per-Space memory, import from other assistants.
- Tool access modes / deferred tool loading and skills loader following agentskills.io.
- Scheduled tasks created conversationally with a confirm step; per-run session records.
- Permission modes (Manual/Auto/Skip) with org caps and classifier-backed Auto.
- OTel export with prompt.id correlation and approval-decision events; pre-inference policy hook.
- Per-message model/effort label and safety-fallback notices.
- Real-time collaborative doc with AI as attributed collaborator and @AI-in-comment edits.
- Interactive MCP Apps host.

## 8. What to avoid
- Shipping a mode switch (Chat vs Agent); Anthropic removed theirs within eight months.
- Letting privacy configurations (ZDR) disable the agent runtime; design the sandbox so it works without retention.
- Invisible branching; hidden tool payloads in shares are right, hidden branches are not.
- Two artifact runtimes (legacy vs typed) coexisting for long; version the artifact contract from day one.
- Routing cloud sessions through a user's desktop for browser work when a server-side browser (Quarry) exists; Claude needs the desktop open for its built-in browser, which hurts web/mobile.

## 9. Open questions (could not be resolved from public sources)
- The desktop bridge protocol and how local tool calls are authenticated/attested.
- How the Auto-mode action classifier is structured (per-tool? per-site?) and its latency/cost ("uses more of your usage limit" is stated).
- Thread coordination in the new Projects beta (context passing, merge policy) — only press coverage.
- Dashboard refresh cadence and caching.
- Whether thinking summaries are produced by the same model or a separate summarizer.
- Exact token accounting for artifacts that call Claude on the viewer's plan.
- Mythos availability in the consumer app (listed on product page; only observed behind Claude Security).

## 10. Pricing/limits snapshot (DOC, PRICING, fetched 2026-10-13)
Free $0 (Sonnet/Haiku, 5 projects, web search, file creation, memory, connectors, artifacts, voice); Pro $20/mo or $17 annual (Opus, Claude Code, Chrome, M365, research, unlimited projects, Dashboards beta, scheduled tasks, 1M context); Max $100/$200 (5x/20x, Fable at 50% of weekly, $100/$200 monthly API credits); Team $20-25 standard / $100-125 premium seats (SSO, admin connector controls, no training by default, pooled API credits up to $500); Enterprise $20/seat + usage (RBAC, SCIM, audit logs, Compliance API, custom retention, IP allowlisting, HIPAA option, Claude Security beta). Usage bundles: $50/$250/$1000 at 10/20/30% discount, up to $2000/mo individual, $3000/mo Team.
