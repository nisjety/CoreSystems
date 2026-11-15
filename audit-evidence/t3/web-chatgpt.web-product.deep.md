# ChatGPT (chatgpt.com) web product — public-documentation deep dive

Audit 3, facet "web-product". Researcher: Fable. Research date: 2026-10-13. Companion file: `web-chatgpt.web-product.features.json` (155 records). Provenance tags: DOC = official OpenAI help center / developer docs / release notes; DOC-3P = credible third-party coverage; INF = my inference with stated basis. No REA tools were used (web-only facet).

Sourcing caveats: `help.openai.com` and `openai.com` return HTTP 403 to the WebFetch tool, so the help center was read through the Browser pane (page text of ~45 articles, including the full 218k-character ChatGPT release notes). `openai.com/chatgpt/pricing` could not be loaded at all; plan prices come from help-center articles. Developer docs (`developers.openai.com`, `learn.chatgpt.com`) were fetched directly. Third-party sources (VentureBeat, NeuralTrust, Kompozy, Unite.ai, Wikipedia, memoryplugin.com, aiinsider.in) were used only for context, rollout dates and implementation hints that OpenAI has not documented; they are tagged DOC-3P. All performance numbers are vendor self-reports.

## 1. Product shape in October 2026

ChatGPT is no longer one chat box. Since 2026-07-09 it is three experiences under one account [DOC WORK]:

- **Chat**: fast conversational answers with per-message allowances. Since 2026-10-07 it runs GPT-6 (Sol for paid, Luna for Free/Go) with **Intelligent UI**, i.e. answers composed from text plus native interactive components [DOC IUI, MODELS].
- **Work**: an agent "for longer, multi-step work and finished deliverables" (documents, spreadsheets, decks, reports, Sites). It is built on the Codex harness, runs in the cloud (or locally in the desktop app with sync), and is metered by tokens on an allowance shared with Codex, the Office add-ins and Workspace Agents [DOC WORK, RATE].
- **Codex**: software development; desktop app, CLI, IDE extension, Codex Cloud, GitHub/GitLab/Linear integrations [DOC CODEXPLAN].

Around them sit: **dots** (always-on personal agents with their own cloud computer, 2026-09-29) [DOC DOT]; **Space / Pages / Teams** (collaborative documents and groups replacing Library for Pro/Business/Enterprise) [DOC SPACE, TEAMS]; **Plugins** (the single packaging unit for skills + apps + extensions, replacing the App Directory and, soon, custom GPTs) [DOC PLUG]; **Scheduled tasks** including event-triggered ones [DOC TASKS]; **Workspace Agents** for Business/Enterprise [DOC WAGENTS]; **Sites** (hosted apps) [DOC SITES]; **Voice** (full-duplex GPT-Live-1 inside the chat) [DOC VOICE]; vertical spaces **Health** and **Finances** [DOC RN].

Retirements that matter for the benchmark: the screenshot-driven "ChatGPT agent"/Operator (replaced by Work + cloud browser) [DOC AGENT]; the Atlas browser (2026-08-09, folded into desktop app + extensions) [DOC RN]; canvas on current models (replaced by writing blocks and code blocks) [DOC RN 2026-05-28]; group chats (2026-07-09) [DOC RN]; Pulse (folded into scheduled tasks, 2026-06-17) [DOC RN]; custom GPTs (retirement announced 2026-09-11, migration to plugins) [DOC RN].

Timeline highlights (all DOC RN unless noted): Feb 2026 interactive code blocks, deep research redesign with editable plan; Mar 2026 File Library, GPT-5.4, interactive learning modules, Codex plugins; Apr 2026 Images 2.0, composer model picker, Fast answers, GPT-5.5; May 2026 memory sources, Office add-ins, Codex remote; Jun 2026 improved memory, Lockdown Mode for all, six-level picker, Scheduled page; Jul 2026 Work, unified desktop app, Sites beta, GPT-Live-1, Health; Aug 2026 GPT-5.6 slider, local work sync (Agent Security), event-triggered tasks, cloud-browser sign-in, Google Drive in Library; Sep 2026 Astra, Images 2.5, Library sharing, Box/Dropbox/SharePoint, DevDay (dots, Space, Pages, Pro 500, Ultrafast, plugin extensions, Meetings, Sign in with ChatGPT GA, MCP-event automations); Oct 2026 GPT-6 + Intelligent UI for everyone, audio uploads, Composer predictions.

## 2. Architecture as visible from the outside

### 2.1 One session store, two execution planes
Chat and cloud Work chats are stored in the cloud and sync across web, mobile and desktop; local Work/Codex chats run on the computer but "messages and task context may be stored in the cloud, even when work runs locally" [DOC WORK]. With **local work sync**, a conversation is cloud-coordinated and each *turn* has an execution location: if the computer is offline, the next turn starts in a cloud container without local access; a running turn never migrates [DOC AGSEC]. INF: this is the cleanest public statement of a hybrid runtime, and it maps well onto Verevon's Model Plane sessions/runs with a per-run executor binding.

### 2.2 The Codex harness and its protocol
Work, Codex, dots, Workspace Agents and the public **Agents API** all sit on the same harness. The **Codex app-server** documents it [DOC learn.chatgpt.com/docs/app-server]: JSON-RPC 2.0 over stdio/WebSocket/Unix socket; `initialize` with `clientInfo` (used for compliance logs); `thread/start|resume|fork`, `turn/start` with per-turn overrides (model, effort, cwd, sandbox policy, outputSchema), `turn/steer` (requires `expectedTurnId`), `turn/interrupt`; items `agentMessage, plan, reasoning, commandExecution, fileChange, mcpToolCall, dynamicToolCall, collabToolCall, webSearch, imageView, contextCompaction, enteredReviewMode`; server-initiated `item/permissions/requestApproval`, `mcpServer/elicitation/request` (form or URL mode), `item/tool/requestUserInput` with `autoResolutionMs`; sandbox policies `readOnly | workspaceWrite | dangerFullAccess | externalSandbox`; goals (`thread/goal/*`); `thread/compact/start`; app/plugin/marketplace/MCP/config/filesystem method groups; `-32001` overload with backoff. The **Agents API** (beta) exposes the hosted version: sessions with `none | openai_hosted | self_hosted` environments, streaming or webhooks, `multi_agent.max_concurrent_subagents`, harness-managed compaction and recovery; not ZDR-eligible, US-only residency [DOC developers.openai.com agents-api].

INF: Verevon's Rust gateway can adopt this vocabulary (thread/turn/item, server-initiated approval requests, per-turn sandbox policy) and remain interoperable with ACP-style clients.

### 2.3 Generative UI pipeline
Intelligent UI: GPT-6 is trained to compose text, visuals and interactive elements; some components keep state within a thread; Pro effort (Astra) and Work/Voice do not support it; users can select "Simple" layout [DOC IUI, MODELS]. OpenAI shipped the plumbing first: interactive charts (Jun 2026), progressive rendering of interactive content while generating (Aug 2026), then the model-driven composition (Oct 2026) [DOC RN]. Third-party reporting describes "a library of native components, designed to arrive in chunks while the model writes, plus a compiler that transforms the interface as it is generated" [DOC-3P aiweekly/TechRepublic]. INF (medium): the renderer accepts a streaming component AST; this is what Verevon's SolidJS frontend should expose (AG-UI-like events), not markdown post-processing.

### 2.4 Plugin system = MCP + skills + UI extensions
- **Manifest**: `plugin.json` with `$schema https://agent-plugins.org/schemas/1.0.0/plugin.schema.json`, `extensions.com.openai` (interface, apps, hooks, onboardingSkill), `mcp.json` for servers (transport, url, auth none|oauth|mixed|api_key), `skills/<name>/SKILL.md`, optional `hooks/hooks.json` (trust review; ineligible for the public directory). Marketplaces are JSON catalogs (`.agents/plugins/marketplace.json`), importable from GitHub with daily sync; Claude Code manifests and `.claude-plugin/marketplace.json` are readable [DOC developers.openai.com/plugins/build/plugins].
- **UI**: MCP Apps standard; resource `text/html;profile=mcp-app` referenced by `_meta.ui.resourceUri`; JSON-RPC over postMessage (`ui/initialize`, `tools/call`, `ui/update-model-context`); `window.openai` compatibility layer (toolInput/Output, widgetState, requestDisplayMode inline|fullscreen|pip, uploadFile, requestCheckout); CSP allowlists `_meta.ui.csp`; dedicated domain required [DOC plugins/build/chatgpt-ui, plugins/reference].
- **Extensions**: `_meta["openai/ui"].entrypoints` typed `global` (sidebar app), `thread` (conversation panel), `file` (viewer/editor by extension); extended forms via `requestedSchema`; composer mentions (desktop) [DOC plugins/build/extensions].
- **Events**: MCP 2.0 (2026-07-28) `events/list|subscribe|unsubscribe`; webhook delivery signed with Standard Webhooks HMAC (`whsec_` secrets), challenge verification, `refreshBefore`, cursors, 256 KiB cap, no retries on 410/413; consumed by Work and dots as task runs with batching [DOC plugins/build/mcp-events].
- **Governance**: developer mode; tools frozen at approval with diff-on-refresh; per-action enable; new actions off by default; Secure MCP Tunnel for private servers; Pro accounts limited to read/fetch MCP [DOC DEVMODE].

### 2.5 Agents as first-class objects
- **dots**: goal + connected apps + custom rules (act / act if pre-approved / ask / hand off) + own cloud computer (browser, network, computer-use toggles) + own memories + schedules + Slack/Teams identity + optional local computer access; auto-review checks consequential steps; reset deletes everything [DOC DOT, DOTSADMIN]. Enterprise model controls do not apply to dots [DOC DOTSADMIN].
- **Workspace Agents**: builder (prompt -> plan -> build), preview, model + reasoning, tools (apps, custom MCPs, image gen, web search), files (512 MB each, 10 GB per agent), skills, channels (ChatGPT directory, Slack, API), schedules, end-user vs agent-owned auth, write actions default Always ask, **Connector Action Constraints** (NL rules compiled to restrictions), can-chat/can-edit/owner roles, version history, API trigger `POST /v1/workspace_agents/{id}/trigger` -> 202 + `conversation_url`, run states queued/in_progress/suspended/completed/failed [DOC WAGENTS, developers.openai.com/workspace-agents].
- **Teams** have a service account and configured connections; **team tasks** run under it [DOC TEAMS, TTASKS].

## 3. Data model (as documented or inferable)

- Account -> Workspace (Business/Enterprise/Edu/Healthcare) with seat types (Standard/Premium, Codex-only, view-only), tenant-level identity (verified domains, SSO, SCIM), groups, custom roles (Default/On/Off, additive, Lockdown roles override) [DOC RBAC, ADMINQ].
- Conversation {mode Chat|Work, model+effort+speed, project?, temporary (personalized?), messages, items, memory sources, shared link snapshot}.
- Project {name, icon, color, instructions, files (plan-limited), sources (Drive/Slack links, saved responses, text), memory mode default|project-only, members with chat|edit} [DOC PROJ].
- Space {personal/shared, folders, pages (nested subpages, comments, uploaded files inherit page ACL, linked files do not)} and Team {members, owner, service account, connections, tasks} [DOC SPACE, TEAMS].
- Library {files uploaded/created, favorites/folders, storage quota 500 MB / 4 GB / 20 GB / 100 GB, sharing Viewer/Editor} [DOC LIB].
- Memory {improved summary (editable), legacy saved memories (search, sort, prioritization, versions), reference-chat-history flag, project scopes, dot memories} [DOC MEM].
- Scheduled task {title, instructions, trigger (schedule|date|event Gmail/Slack/GitHub), conditions, mode/model, notifications, runs, shared snapshot link} with limits 3/5/10/15 active and 30/h, 720/day for event runs [DOC TASKS].
- Plugin {manifest, skills, apps (connections per account, permissions Always ask | read | low-risk | all), templates, extensions, install policy Available|Installed, sharing invite|link|directory}.
- Site {versions tied to commits, deployments (every deployment URL is production), audience, D1/R2 bindings, secrets, custom domain, automations, hosted MCP plugin} [DOC SITES, learn.chatgpt.com/docs/sites].
- Usage {per-plan allowances per experience, credits, token history uncached/cached/output, per-chat lifetime credits} [DOC USAGE, RATE].

## 4. Notable design decisions

1. **Chat is per-message, agency is per-token.** Chat Instant is "unlimited" on every plan (abuse-guarded), Thinking/Pro are per message (10/50 credits), while Work/Codex/Office/Agents share a token-metered allowance with published cached-input rates and speed multipliers (Fast 2.5x allowance / 2x credits; Ultrafast 8x / 6x) [DOC RATE, PRO]. This makes agent cost legible without exposing tokens to chat users.
2. **Effort as a single slider, not a model zoo.** Instant/Medium/High/Extra High/Pro; auto-escalation retired for Plus/Pro on 2026-09-14 ("ChatGPT can still switch automatically for safety") [DOC MODELS, RN]. Free/Go get a Think button on Luna. Business keeps an admin-controlled auto-routing toggle.
3. **Transparency as product features**: memory Sources per response, deep-research activity history, usage breakdowns by model/effort/speed with per-chat credits, Personal Analytics plugin, published reasoning-time changes, Elevated Risk labels, Model Test for admins [DOC MEM, DR, USAGE, RISK, ENTM]. OpenAI also documents what is not counted (subagents, background work) [DOC USAGE].
4. **Security profiles instead of only prompts**: Lockdown Mode (outbound-network minimization, per-chat override, admin role), graduated app permissions, auto-review model, custom rules with mandatory human hand-offs, credential-isolating sign-in form with a phishing-review model, takeover without screenshots [DOC LOCK, PERM, CLOUDB, AGENT].
5. **Everything becomes a plugin.** Connectors -> apps -> plugins; GPTs -> plugins; skills follow the Agent Skills format; marketplaces are JSON catalogs; Claude plugins are importable. One directory for ChatGPT and Codex [DOC PLUG, plugins docs].
6. **Collaboration is document-centric, not chat-centric.** Group chats were retired; shared projects, Pages (real-time co-editing with "each collaborator uses their own ChatGPT") and Teams replaced them; sharing never exposes private chats or memory, but anything written onto a page is visible [DOC SPACESHARE].
7. **Agents get identities.** dots have handles and Slack/Teams identities; teams and workspace agents run on service accounts; Sign in with ChatGPT extends the ChatGPT identity to third-party apps and lets apps spend the user's plan allowance [DOC DOT, TEAMS, siwc].
8. **Regional gating is explicit.** Many agentic features launch excluding EEA/UK/CH and arrive later (dots, Computer Use, Computer History, Sites public publishing, Health) [DOC RN, DOT].

## 5. What is best-in-class (relative to Manus, Claude, Perplexity, Codex-style tools)

- **Intelligent UI** (model-composed interactive answers) — nobody else ships this at consumer scale [DOC IUI].
- **Memory Sources + editable memory summary** — most transparent personalization UX on the market [DOC MEM].
- **Graduated, per-connection app permissions with Elevated Risk labelling and Lockdown Mode** — more granular than Manus (Allow Once/Always) and more explicit than most [DOC PERM, LOCK, RISK].
- **Deep research controls**: editable plan, site scoping (restrict vs prioritize), interruptible runs, activity history, Markdown/Word/PDF export [DOC DR].
- **Steer vs Queue follow-ups and goal objects** in the harness [DOC CODEXPLAN, app-server].
- **Event-triggered automations on a documented MCP Events protocol** [DOC mcp-events].
- **Desktop browser annotations** (click/drag a region, preview style changes, site-defined custom annotations) [DOC BROWSER].
- **Rate card and usage analytics** (cached vs uncached tokens, per-chat credits, Personal Analytics) [DOC RATE, USAGE].
- **Plugin packaging openness** (portable manifest, Claude compatibility, GitHub marketplaces) [DOC plugins/build/plugins].

## 6. Weaknesses and limitations (documented)

- **dot memories are opaque**: users cannot view, edit or delete individual dot memories; only Reset deletes them; disconnecting an app does not remove data already obtained [DOC DOT]. Enterprise model governance does not apply to dots [DOC DOTSADMIN].
- **Cost attribution gaps**: per-chat usage excludes subagents, separate tasks, images and background activity [DOC USAGE].
- **Compliance blind spots**: agent-internal actions (virtual computer use, app requests, chain of thought) are not in Compliance logs; shared task links/snapshots are not logged or exported [DOC AGENT, TASKS, SHARE].
- **ZDR does not survive agency**: local work sync, Agents API and hosted sandboxes are not ZDR; Sites have no residency [DOC AGSEC, agents-api, SITES].
- **Shared links lack ACLs and expiry** [DOC SHARE].
- **Multiplayer agent editing** is last-save-wins with conflict refresh, not real-time merge [DOC WAGENTS].
- **Frequent churn**: model names, pickers and products changed many times in 12 months (six-option picker June 2026, slider August, GPT-6 October; canvas, Atlas, agent mode, group chats, Pulse, GPTs retired). Users and integrators pay the migration cost.
- **Browser agents are blocked by many sites**; OpenAI maintains allowlisting guides [DOC CLOUDB].
- **Intelligent UI exclusions**: not in Work, Voice, Astra, or old desktop apps; "design judgment" admitted as work in progress [DOC IUI; DOC-3P].
- **Pricing opacity**: official pricing page unreachable in this audit; allowances are described qualitatively ("more usage") and change (Pro 200 allowance cut for new subscribers) [DOC PRO].

## 7. What Verevon should copy, adapt, or avoid

Copy:
- A typed, streaming component channel from the Model Plane to the SolidJS renderer (Intelligent UI parity), with a user-selectable "Simple" mode (features 002, 004, 147).
- Memory Sources on every answer plus an editable synthesized user profile with provenance (090, 091); project/space-scoped memory where "shared implies isolated" (093).
- Permission matrix per connection (read / low-risk / all) with approval cards, Elevated Risk labels, and a Lockdown profile that minimizes outbound paths; this directly serves Verevon's ZDR and no-ACL-widening rules (117, 118, 119).
- Harness vocabulary from the app-server protocol: thread/turn/item, steer/queue, goals, compaction as an explicit item, server-initiated approval/elicitation (010, 056, 059, 142).
- Scheduler with Trigger/Condition/Prompt, monitoring semantics and documented run limits; MCP Events as the integration contract between integration-corev2 and Model Plane sessions (044, 050, 051).
- Service-account execution for team automations and agent-owned connections (018, 053, 149).
- Published rate card with cached-token rates, per-run cost including subagents (fix OpenAI's gap) (103, 109).
- Plugin packaging compatible with `plugin.json` / SKILL.md / `.claude-plugin` marketplaces so the ecosystem imports for free (064, 066).
- Annotation-driven feedback on live previews (078).

Adapt:
- "Chat free / agent metered" boundary: Verevon could meter per token in the agent page while keeping chat per message.
- Deep research plan editing and site scoping as a standard research mode (042).
- Auto-review model between proposal and execution fits "Model proposes, Quarry executes or rejects" (055).
- Natural-language action constraints compiled into enforceable rules at the Quarry boundary (120, 121).

Avoid:
- A separate persona store (GPTs) later folded into plugins; make personas plugins from day one (076).
- Chat-room style multi-user conversations without a document anchor (152).
- Opaque per-agent memory (096).
- Screenshot-only browser agents as a separate mode (151).
- Shipping agentic features that silently break ZDR; Verevon's propagation rule should make this impossible (123).

## 8. Open questions

- How Intelligent UI components are specified (schema, streaming format) and whether third-party plugins can contribute components to the model's palette. Only secondary sources describe a component library and compiler.
- Internal structure of Work's planning/subagent orchestration and how Ultra "may run additional agents"; cost attribution for those agents.
- What the auto-review model sees (full context vs action summary) and its latency.
- Whether dots' custom rules are enforced by policy code or by the model; OpenAI says rules "cannot override built-in safeguards" but does not describe the enforcement layer.
- Exact allowances per plan (message counts, token budgets) — only ranges (e.g. Business Standard 5-45 Astra local messages per 5 h) are published.
- How Space pages are stored and merged (CRDT vs OT) and whether Pages will expose an API.
- Whether the MCP Events webhook model will be adopted upstream in the MCP spec (OpenAI references a draft).
- Reliability/uptime commitments for "always-on" dots; none are published.

## Evidence index (short names used above)
WORK https://help.openai.com/en/articles/20001275-chatgpt-work-and-codex; IUI https://help.openai.com/en/articles/20001598-intelligent-ui-in-chatgpt; MODELS https://help.openai.com/en/articles/20001354-gpt-6-and-other-models-in-chatgpt; RN https://help.openai.com/en/articles/6825453-chatgpt-release-notes; DOT https://help.openai.com/en/articles/20001530-getting-started-with-your-dot; DOTSADMIN https://help.openai.com/en/articles/20001554-manage-dots-in-chatgpt-workspaces; SPACE https://help.openai.com/en/articles/20001549-getting-started-with-space-in-chatgpt; SPACESHARE https://help.openai.com/en/articles/20001544-chatgpt-space-sharing-data-and-controls; TEAMS https://help.openai.com/en/articles/20001541-teams-in-chatgpt; TTASKS https://help.openai.com/en/articles/20001540-creating-and-managing-team-tasks-in-chatgpt; PLUG https://help.openai.com/en/articles/20001256-plugins-in-chatgpt; TASKS https://help.openai.com/en/articles/10291617-scheduled-tasks-in-chatgpt; WAGENTS https://help.openai.com/en/articles/20001143-chatgpt-workspace-agents-for-enterprise-and-business; SITES https://help.openai.com/en/articles/20001339-creating-and-using-chatgpt-sites; VOICE https://help.openai.com/en/articles/20001274-chatgpt-voice; AGENT https://help.openai.com/en/articles/11752874-chatgpt-agent; CODEXPLAN https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan; AGSEC https://help.openai.com/en/articles/20001548-agent-security-and-local-work-sync-in-chatgpt; MEM https://help.openai.com/en/articles/8590148-memory-in-chatgpt; PROJ https://help.openai.com/en/articles/10169521-projects-in-chatgpt; DR https://help.openai.com/en/articles/10500283-deep-research-in-chatgpt; LOCK https://help.openai.com/en/articles/20001061-lockdown-mode; PERM https://help.openai.com/en/articles/20001495-managing-app-permissions-in-chatgpt; RISK https://help.openai.com/en/articles/20001062-elevated-risk-labels; CLOUDB https://help.openai.com/en/articles/20001280-using-cloud-browser-in-chatgpt; BROWSER https://help.openai.com/en/articles/20001277-using-the-built-in-browser-in-the-chatgpt-desktop-app; DEVMODE https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt; RBAC https://help.openai.com/en/articles/11750701-managing-feature-access-with-role-based-access-control-in-chatgpt; ADMINQ https://help.openai.com/en/articles/20001264-chatgpt-enterprise-admin-quickstart; COMP https://help.openai.com/en/articles/9261474-openai-compliance-platform-for-enterprise-and-edu-customers; RATE https://help.openai.com/en/articles/11481834-chatgpt-rate-card-business-enterpriseedu-credit-based-pricing; PRO https://help.openai.com/en/articles/9793128-about-chatgpt-pro-tiers; USAGE https://help.openai.com/en/articles/20001478-reviewing-work-and-codex-usage-and-using-personal-analytics-in-chatgpt-desktop; SHARE https://help.openai.com/en/articles/7925741-sharing-conversations-and-scheduled-tasks-in-chatgpt; LIB https://help.openai.com/en/articles/20001052-using-library-to-manage-files-in-chatgpt; ENTM https://help.openai.com/en/articles/11165333-chatgpt-enterprise-and-edu-models-and-limits; app-server https://learn.chatgpt.com/docs/app-server; plugin docs https://developers.openai.com/plugins (build/plugins.md, build/chatgpt-ui, reference, build/extensions.md, build/mcp-events.md, build/skills.md, guides/security-privacy.md); agents-api https://developers.openai.com/api/docs/guides/agents-api/overview and /architecture; siwc https://developers.openai.com/siwc; sites https://learn.chatgpt.com/docs/sites; changelog https://learn.chatgpt.com/docs/changelog; DOC-3P: https://venturebeat.com/technology/openai-launches-dots-always-on-ai-agent-coworkers-and-chatgpt-space-where-they-can-collaborate-with-human-teams, https://neuraltrust.ai/blog/chatgpt-agent-security, https://www.kompozy.io/reviews/chatgpt-work, https://aiweekly.co/alerts/openai-launches-chatgpt-intelligent-ui-alongside-gpt-6, https://en.wikipedia.org/wiki/ChatGPT.
