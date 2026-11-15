# Manus (manus.im) web product — public-documentation deep dive

Audit 3, facet "web-product". Researcher: Fable. Date of research: 2026-10-13. Sources: manus.im marketing pages, manus.im/blog (121 posts listed), manus.im/docs (Mintlify-style, llms.txt index), help.manus.im (Intercom help center, 8 collections), open.manus.ai API v2 docs, plus third-party coverage for pricing and release notes where official pages were JS-rendered and unscrapable. Provenance tags: DOC = official Manus documentation/blog/help; DOC-3P = third-party page; INF = my inference with stated basis. No REA tools were used (web-only facet). Companion file: web-manus.web-product.features.json (105 records). The Manus desktop app itself was inventoried separately in manus.inventory.md by another scout; this document covers only the public product surface.

Caveats on sourcing: manus.im/pricing, /updates and /trust are client-rendered and returned nav-only or 404 to the fetcher, so plan numbers come from the help center article "What is the current membership pricing" (official) cross-checked with layer3labs (3P, updated 2026-09-28). All performance numbers are vendor self-reports.

## 1. Product shape (what Manus is in October 2026)

Manus positions itself as a general-purpose agent that produces finished artifacts (slides, websites, apps, games, videos, documents, research) rather than a chat assistant. The tagline on the home page is "Less structure, more intelligence" and the banner is "Introducing Manus 2.0" [DOC https://manus.im/]. Timeline relevant to the benchmark:

- 2025-07-31 Wide Research; 2025-10-16 Manus 1.5 (web builder, Collab, Library); 2025-11-18 Browser Operator; 2025-12-01 Projects; 2025-12-15 Manus 1.6 Max (mobile dev, Design View); 2025-12-29 joined Meta.
- 2026: Meeting Minutes (01-12), Agent Skills standard (01-27), Project Skills (02-12), Telegram agents (02-16), My Computer desktop control (03-16), Slack agent (04-06), Cloud Computer (04-30), recommended connectors and self-updating Projects (05-05/06), Scheduled Tasks 2.0 (05-18), Hosting Modes (06-24), Branch (07-09), Auto-Publish (07-13), PowerPoint mode (07-14), Plan Mode (07-22), ElevenLabs (08-03), data deletion/backup service change (08-11 to 08-25), independence resumed (09-01), Manus 2.0 + Cue (09-28), Flex BYO-key (09-29), Video Editor and Game Dev posts (10-01), OPPO partnership (10-13). [DOC https://manus.im/blog]

The two-mode split is fundamental: Chat mode is free (no credits), accepts images/PDFs/text and can search; Agent mode consumes credits and runs in a sandbox with Manus 2.0 Lite / 2.0 / 2.0 Max profiles [DOC help 11711128]. Free users get Lite only.

## 2. Architecture as visible from the outside

### 2.1 Task is the universal unit
Every interaction is a "task": web sessions, API tasks, Slack/Telegram/LINE DM agents (an agent is a persistent main task that spawns `agent_subtask` children), scheduled runs (either continue in the same task or spawn a new one), and Wide Research sub-tasks. `task_url` always resolves into the web app (`https://manus.im/app/{id}`) [DOC open.manus.ai v2 task.create, agents-overview, blog manus-schedules]. This gives Manus one addressable, shareable, replayable object across all surfaces. INF: Verevon's Model Plane sessions/runs should be the same kind of single unit, with channel and schedule being adapters rather than separate entities.

### 2.2 One VM per session
Each task gets an isolated cloud VM (Ubuntu, root access, browser, filesystem, network). It sleeps when idle, wakes on demand, and is recycled after 7 days (Free) / 21 days (Pro) of sleep; on recycle only artifacts, attachments and Slides/WebDev project files are restored, not intermediate files [DOC blog manus-sandbox]. Wide Research allocates one VM per sub-agent [DOC blog introducing-wide-research]. Cloud Computer is a paid persistent VM (Standard $30/mo 2 vCPU/4 GB/70 GB; Advanced $50/mo 2 vCPU/8 GB/120 GB; Ubuntu Server 24.04, public IP, SSH or web terminal, deleted when subscription lapses) [DOC help 15392078, blog manus-cloud-computer]. Manus explicitly refuses to publish per-task VM specs ("performance is not fixed") [DOC help 11813695].

### 2.3 Run state machine (API v2)
Statuses: running / stopped / waiting / error, plus an independent tri-state `has_running_background_jobs` (true / false / omitted = unknown; the docs insist omitted must not be read as false). Waiting is routed by `waiting_for_event_type`: agent questions (`messageAskUser`, `cascadeAskUser`) answered by `task.sendMessage` with optional `question_expectation` {options, selection_mode single|multiple}; action confirmations (`gmailSendAction`, `deployAction`, `terminalExecute`, `googleCalendarDelete`, `metaMarketingAction`, `videoGenerate`, `webdevRunAction`, `webdevRequestSecrets`) answered by `task.confirmAction` with a per-type `confirm_input_schema`; and UI-only confirmations (connector CRUD, `selfConfigSaveAction`, Agent configuration cards) that the API can only skip. A stopped task can be re-awakened by a later message, schedule or external trigger [DOC open.manus.ai v2 task-lifecycle]. The `cascadeAskUser` event and the mention of "Profile Agent Cascade sub-lane connector confirmations" are the only public traces of how the 2.0 Cascade harness structures sub-lanes.

### 2.4 Context engineering (published harness internals)
The 2025-07-18 post is the most concrete public description of any vendor's agent harness [DOC blog Context-Engineering...]: KV-cache hit rate is the production metric (100:1 input:output tokens; cached vs uncached Sonnet input 10x cheaper); stable prompt prefix, append-only context, deterministic JSON serialization, explicit cache breakpoints; tools are never removed mid-task, instead a state machine masks logits using prefixed tool names (`browser_`, `shell_`) with Auto/Required/Specified prefill modes; the filesystem is the unbounded, restorable memory (drop a page body but keep the URL, drop a document but keep its sandbox path); a `todo.md` is rewritten each step so the plan sits at the attention-rich end of context; failed actions and stack traces are kept in context; serialization noise is injected to avoid few-shot imitation ruts; ~50 tool calls per typical task. Cascade (2.0) adds lazy loading of specialist capabilities with vendor-reported -23.2% tokens, -28.2% time, -32% cost in one configuration [DOC blog introducing-manus-2-0].

### 2.5 Skills
Manus adopted Anthropic's Agent Skills format (SKILL.md + scripts/references/templates) with three-level progressive disclosure: metadata (~100 tokens) at startup, instructions (<5k tokens) on trigger, resources on demand. Skills are invoked automatically or with `/SKILL_NAME`; "Build a Skill with Manus" packages a successful workflow into a `.skill` file; import from `.skill`/`.zip`/folder or a public GitHub repo; Manus can review imported skills for risk; Team Skill Library and per-Project skill libraries with a lock; only skills in a Project's library may fire in that Project [DOC blog manus-skills, manus-project-skills; help 14753565]. API: `skill.list`, `enable_skills`, `force_skills`; Open Apps may bundle up to 50 private skills [DOC open-app].

### 2.6 Connectors and permissions
About 70 OAuth connectors plus custom MCP/API connectors. Resolution order on a task: explicit list > Project default > user default; follow-ups can override or `clear_connectors`; revoked connectors are silently skipped [DOC v2 connectors]. Connectors are recommended inline when a task needs one, but enabling always requires user approval and any OAuth step is user-completed [DOC blog manus-recommended-connectors]. Connectors are automatically disabled when a task enters collaboration mode [DOC blog manus-sandbox]. Multi-account Gmail/Calendar with explicit sender selection at action time [DOC blog manus-google-multi-account].

Browser: the default cloud browser is watchable live with a take-over handshake (notification, user completes CAPTCHA/MFA, hands control back) and a logged-in-accounts panel [DOC docs/features/cloud-browser]. Browser Operator (My Browser) is a Chrome/Edge extension with per-task one-time authorization, a tab group named after the task, take-over by clicking in, stop by closing the tab, and a full action log [DOC docs/integrations/manus-browser-operator]. The API exposes this as a `needConnectMyBrowser` wait + `browser.onlineList` + confirmAction select/skip. My Computer (desktop) uses folder grants and command approvals (blog: Allow Once / Always Allow; help: only sensitive commands, scoped to task or path) [DOC blog manus-my-computer-desktop; help 14178443].

### 2.7 Public API and Open Apps
RPC-style v2 endpoints (`task.*`, `project.*`, `skill.list`, `agent.*`, `file.*`, `artifact.list`, `website.*`, `webhook.*`, `usage.*`, `connector.list`, `browser.onlineList`), OpenAPI spec, per-user rate limits (task.create 10/min). Structured output is a post-run extraction step against a strict JSON-schema subset. Open Apps are OAuth2 clients (team, trusted_team, trusted_public) with scopes `create_task`, `manage_all_tasks`, `create_project`, `use_connectors`, `use_my_browsers`; trusted_public clients can inject hidden context parts the user never sees [DOC open.manus.ai v2 docs]. An installable "Manus API skill" for Codex-style coding agents reads live docs [DOC manus-api-skill].

## 3. Data model (inferred from API and docs)

- User / Team (Owner, Super Admin, Admin, Member, Guest) with pooled credits and typed credit buckets (event, daily, monthly, add-on, free) consumed in that order; balance can go briefly negative [DOC help 11711097, 11711764].
- Project {name, master instruction, knowledge files, default connectors, skill library, sessions}. Instruction edits apply to existing sessions on next message; file edits only to new sessions [DOC blog manus-projects].
- Task {id (22 chars), title, agent_profile, project_id, locale, share_visibility, task_type (normal | agent_subtask), status, background jobs flag, events[], task_references[] (max 20, FIFO), structured_output_schema, website?}.
- Event stream: `status_update`, `assistant_message`, `tool_used {tool, action_id, params, result, status}`, `user_message {content, hidden_content}`, `structured_output_result`, waiting events by type.
- Artifact {id, task_id, filename, type in 10 classes, file_type, favorite, url (48h signed), cover_image, resource_uri} indexed in the Library [DOC artifacts].
- Website {website_id, checkpoints[] git-commit-backed with status, published_version_id, publish_status, visibility public|team|private with cap, site_urls (manus.space, subdomain, custom domains), auto_publish, hosting mode Autoscale|Reserved} [DOC website, blog hosting modes].
- Agent {id, nickname, description, main task_id} [DOC agents-overview].
- Knowledge entries (user memory) capped at 50 free / 100 pro [DOC help 11813718].
- Branch: a child session with an isolated snapshot of context up to a message, plus "Branched from" link [DOC blog manus-branch].

## 4. Notable design decisions

1. Chat is free, agency is metered. The credit boundary is the mode boundary, which makes the pricing legible and keeps acquisition cheap.
2. Full-instance sub-agents with no fixed roles (Wide Research) and strict controller-only communication; each sub-task capped at 50 credits; help center states 20 concurrent subtasks [DOC help 11960169]. Marketing talks about hundreds of items; documented limit is lower.
3. Consent-gated self-improvement. Self-updating Projects and "Build a Skill" let the agent propose instruction/file/skill changes, but nothing is written without approval, and scope is a single Project [DOC blog manus-projects-self-updating]. This is the Manus answer to Hermes-style learning.
4. Plan Mode is manual and document-centric: plan is a Markdown artifact the user can edit before and after confirmation, and it becomes the execution reference [DOC blog manus-plan-mode].
5. Persistent-vs-ephemeral compute is a product line, not a hidden detail: ephemeral sandbox (sleep/recycle) vs Cloud Computer (paid, persistent, public IP) vs My Computer (user hardware), and Manus picks per task.
6. "Editable project, not flat export" across artifact types: pptx with data-backed charts, image slides with region edits, timeline video projects, game tweak panels, visual edit of live web preview.
7. Mobile is a remote control for long-running work (Browser Operator and My Computer can be driven from a phone while the desktop runs; 2.0 adds live desktop streaming with voice).
8. Governance is per-task and visible: one-time browser grants, named tab groups, close-tab kill switch, action logs, typed confirmation schemas, skip-confirmations only as an explicit per-automation setting.

## 5. Best-in-class (relative to peers)

- Published context-engineering practice (KV-cache discipline, logit masking, file-as-memory, todo recitation, keep errors). No peer documents this as concretely. [DOC]
- Slides pipeline: three modes, editable image slides, native pptx with live chart data, inputs from sheets/PDF/transcripts/Wide Research. [DOC]
- Typed confirmation protocol in the public API (per-action input schemas, explicit UI-only confirmations, tri-state background-job signal). [DOC]
- Connector default resolution (explicit > project > user) and in-chat connector recommendation with hard user-approval. [DOC]
- Branching as a navigable session tree with isolated context copies. [DOC]
- Channel breadth with artifact delivery conversion (presentations and markdown to PDF, web projects as preview links) for Slack/Telegram/LINE/email. [DOC]

## 6. Weaknesses and limitations

- No pre-task cost estimate; the help center warns that agent-provided estimates are "possible AI hallucinations" [DOC help 13185575]. Third parties report 11-14 credits per active minute and widespread complaints about credit burn [DOC-3P layer3labs, eesel].
- Models are hidden behind Manus tiers; no model picker, no per-step model transparency (contrast: DeepSeek harness, T3 Code). Flex (BYO key) is the only place a user chooses a model and reasoning effort.
- Memory is thin: a capped knowledge list (50/100 entries) with a warning that too many entries degrade quality; no documented retrieval-backed long-term memory. Persistent memory exists only as agent main threads.
- Citations/source panels for research are not described anywhere public; the research story is about parallelism, not grounding UX.
- Video Editor and Game Dev are desktop-only (web users are redirected to download Studio); Manus itself says 2.0 "still has quite a few rough edges" [DOC blog introducing-game-developer].
- Documentation inconsistencies: approval granularity for My Computer differs between blog and help; video resolution listed as 1080x720 for 16:9; pricing page unscrapable; trust center 404 at time of audit; concurrency and schedule limits only in a help article.
- Platform risk: the 2026 regulatory deletion of post-acquisition data for affected regions (backup tool, restoration portal, refunds) shows how corporate events can translate into data loss for users [DOC help 16147831].
- Webhooks: no documented retry policy; signing documented on a separate page; one webhook per app.
- Website API cannot pin or roll back to an older checkpoint; republish defaults to public if visibility is omitted.
- Collaboration limits: connectors disabled in shared tasks; sandbox cookies cleared when the first collaborator joins.

## 7. What Verevon should copy, adapt, or avoid

Copy (high value, low risk):
- Run event schema with typed waits: questions with option lists, confirmations with input schemas, tri-state background-job flag, UI-only confirmation classes. This slots into Model Plane runs and the "Model proposes, Quarry executes or rejects" rule.
- Per-task grants with visible kill switch and action log for browser and local actions; connector default resolution explicit > space > user; connectors disabled when a run is shared (keeps ACLs from widening).
- Branching as first-class child runs with context snapshots and breadcrumbs.
- Consent-gated self-updating workspace context (propose instruction/file/skill diffs after a run; user approves). Pair with Hermes-style learning for the user model.
- Skills in the SKILL.md format with progressive disclosure, per-space skill libraries and lock; "package this run as a skill".
- Context-engineering rules in the Rust harness: stable prefix, append-only context, deterministic serialization, tool masking instead of removal, file-backed restorable compaction, todo recitation, keep errors in context.
- Wide-Research topology: controller + isolated subruns with per-subrun budget caps, no sibling communication.
- Task references: lazy, tool-driven access to earlier sessions with access checked at read time.
- Effort ladder (`lite`...`max-ultra`) as the user-facing control, with the real model visible underneath (Verevon can beat Manus on transparency here).
- Artifact Library with typed classification and export-format parameters; editable artifacts (pptx with chart data) rather than flat exports.

Adapt:
- Scheduled runs that continue in the same session vs spawn new ones: offer both, default to new session with references to the prior one to keep context bounded.
- Email and chat-channel ingress: cheap to add on the Ingestion Plane even while Channel Plane remains docs-only.
- Cloud Computer style persistent compute as a paid add-on only if Verevon hosts user workloads; otherwise keep sandboxes ephemeral with explicit sleep/recycle contracts.

Avoid:
- Hidden-model tiers without any per-step disclosure; Verevon's positioning (DeepSeek-harness transparency, T3 multi-model) argues for showing model, tokens and cost per step.
- Credit opacity without pre-run estimates; Verevon should ship a live cost meter and at least a range estimate.
- Capped flat knowledge lists as the memory story.
- Republish-defaults-to-public style foot-guns in any sharing API.
- Desktop-only creative editors; Verevon is web-first, so editors must run in the browser.

## 8. Open questions

- Which LLM vendors sit under Manus 2.0 Lite/Standard/Max and whether the model changes per step (only indirect evidence: Sonnet pricing used as an example in the 2025 post).
- Exact Cascade mechanics: how sub-lanes relate to the main timeline and whether sub-lanes are separate model contexts.
- Wide Research true concurrency and whether the 20-subtask cap is per plan.
- Whether citations/source attribution exist in research outputs and how they are rendered.
- Pricing page ground truth (official page could not be fetched): the $200/40,000 tier and Team per-seat credits are third-party reported.
- Enterprise claims (SOC 2 Type II, ISO 27001/27701) could not be verified at manus.im/trust (404 during audit).
- Cue's permission/security model (wallet limits, phone identity) is undocumented beyond marketing.

## 9. Evidence index (primary URLs)

- https://manus.im/ ; https://manus.im/blog ; https://manus.im/team
- Blog: introducing-manus-2-0, introducing-manus-flex, introducing-video-editor, introducing-game-developer, manus-plan-mode, manus-branch, manus-schedules, manus-cloud-computer, manus-my-computer-desktop, manus-projects, projects-connectors, manus-projects-self-updating, manus-project-skills, manus-skills, introducing-wide-research, manus-wide-research-solve-context-problem, Context-Engineering-for-AI-Agents-Lessons-from-Building-Manus, manus-1.5-release, manus-max-release, manus-design-view, manus-sandbox, manus-auto-publish, manus-hosting-web-builder, manus-ppt-slides, edit-slides-created-on-manus-with-nano-banana-pro, manus-agents-telegram, manus-meeting-minutes, manus-your-partner-on-slack, manus-app-publishing, manus-recommended-connectors, manus-browser-operator, manus-preferred-browser, manus-custom-domains, manus-advanced-seo, manus-make-a-copy-webdev, elevenlabs-connector, manus-google-multi-account, manus-resumes-independent-operations
- Docs: https://manus.im/docs/features/cloud-browser ; https://manus.im/docs/integrations/manus-browser-operator
- API: https://open.manus.ai/docs/llms.txt and v2 pages task.create, task-lifecycle, structured-output, task-references, agents-overview, webhooks-overview, rate-limits, hidden-application-context, open-app, website, artifacts, connectors, manus-api-skill
- Help: articles 11711128, 11711218, 11711198, 12135428, 11960169, 11813718, 11813695, 12059745, 14753565, 11711172, 11711239, 17190150, 17190151, 11711097, 11711111, 11711764, 13644620, 15392078, 16147831, 12520235, 13185575, 14178443, 14431752, 14495248; collections 13762621 (Feature), 13760762 (Billing), 13762619 (Events & Plan), 19704025 (Data Back Up)
- Third-party: https://www.layer3labs.io/guides/manus-ai-pricing (pricing cross-check, 2026-09-28); https://www.testingcatalog.com/manus-2-0-launches-with-studio-cloud-computer-and-cue.md ; APKMirror release notes for mobile 26.11.6 / 26.6.11
