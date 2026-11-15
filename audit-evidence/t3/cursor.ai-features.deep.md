# Cursor 3.17.21: how it harnesses models (deep dive, facet "ai-features")

Audit 3, source "Cursor IDE", facet "ai-features". Read-only analysis of the installed program files (Windows, Anysphere Cursor 3.17.21, product commit 8f2a112cb2845a97b75fd932ea5c470579ca4060, build 2026-08-25, VS Code base 1.128.0, Electron 40). Companion file: cursor.ai-features.features.json (146 records, ids cursor-ai-features-001 to 146). In this note "#NNN" means that record. Nothing was run, installed or modified outside the scratchpad; no per-user data directory was read.

Sibling facet: cursor.harness-architecture.deep.md and .features.json were written in parallel by another analyst (141 records, strongest on loop, wire contracts, state and sandbox internals). Overlaps (compaction, hooks, privacy matrix, sandbox, MCP meta-tools) were cross-checked and are consistent; this file puts more weight on feature breadth, product surfaces (Tab, Bugbot, cloud agents, automations, Glass UI), client flag defaults and the Verevon mapping.

Provenance tags: SRC = read directly in shipped files (minified or beautified bundles, protobuf descriptors, JSON schemas, manifests); REA = observed through an REA tool; DOC = documentation (none used); INF = my inference, with the basis stated. Records are SRC unless noted; 144 of 146 are SRC, one (#122) is REA and one (#146, Origin) is INF from flag names. Many SRC facts are read from minified code, so names, fields, strings and constants are reliable while behaviour is inferred; where a record leans on inference its notes say so.

## 0. Method, evidence key, REA log and honesty section

Resume status. A previous attempt left only the scout's inventory (cursor.inventory.md) and a sibling "harness-architecture" scratch folder (tmp/cursor-h: beautified extension bundles, notes.md, raw lists). No features.json or deep.md existed. I used the inventory and the sibling notes as leads only, re-derived or spot-checked every fact I relied on against the bundles, and extended coverage. I checked all 620 evidence anchors in the final JSON with a script (every X#name token must appear in the named bundle; every BEAUT line must exist; every APP path must exist): 0 problems, and 72 anchors were re-labelled to the bundle that actually contains them.

Evidence key (also written into the notes of record #001):
- OUT = scratchpad/tmp/cursor/app/out, a byte copy of install resources/app/out. MAIN = OUT/main.js; EXTH = OUT/vs/workbench/api/node/extensionHostProcess.js; DESK = OUT/vs/workbench/workbench.desktop.main.js; GLASS = OUT/vs/workbench/workbench.glass.main.js.
- "X#name" = a protobuf typeName or enum, a service typeName, a command or setting key, a component module name or another searchable identifier in bundle X. I extracted protobuf field lists with small scripts (scratchpad/tmp/aif/msg.js, enum.js, svc3.js) that read the makeMessageType, makeEnum and service method tables.
- BEAUT/<file>:<line> = a line in scratchpad/tmp/cursor-h/beaut/<file>, esbuild-beautified copies of extension bundles: exec_main.js = extensions/cursor-agent-exec/dist/main.js (the agent loop, prompts, tools), host_main.js = cursor-agent-host/dist/main.js, mcp.js = cursor-mcp/dist/main.js, retrieval.js = cursor-retrieval/dist/main.js, shadow.js = cursor-shadow-workspace/dist/extension.js, alwayslocal.js = cursor-always-local/dist/main.js, commits.js = cursor-commits/dist/main.js, agentworker.js = cursor-agent-worker/dist/main.js, browserauto.js = cursor-browser-automation/dist/extension.js. Line numbers refer to these copies, not to the shipped minified files.
- RAW/NN = scratchpad/tmp/cursor_raw/NN_*.txt (scout extractions); APP = install resources/app; SCRATCH = scratchpad.

REA usage (real calls, this attempt):
1. inspect_analysis_view on retained evidence ev_131baecbd4b87236080aeca6460d94c92670cfa033ad709586026fd80ca9d189 (the sibling's analysis of 12 cursor-* extensions), modules page, 40 nodes. It confirmed the module inventory: cursor-agent-exec, cursor-agent-host, cursor-local-agent-runtime, cursor-shadow-workspace, cursor-browser-automation, cursor-commits and cursor-resolver bundles plus piscina and better-sqlite3. REA: used in #122.
2. trace_application_feature, seed kind=module value "cursor-retrieval" (contains) on the same evidence: 2 asset nodes matched, 6 nodes and 4 edges traced, no feature-level relations.
3. trace_application_feature, seed kind=string value "mcp.elicitationRequest": no match, coverage "partial". REA states this is not proof of absence, and I read the string directly in mcp.js:31814.
4. analyze_javascript_application (format directory, detail summary, record-and-continue, 1 GB heap) on out/vs/code/electron-utility/conversationSearch: 1 JS file, 273,998 bytes, 64,110 AST nodes, 7 findings, evidence ev_d646141a04b19ed23b6a4255411fd657d8af310a78eeac159d026badbb03857e. The semantic graph was partial because the analyzer worker exhausted its heap, so every behaviour of #065 is read from source.
Earlier REA runs by others (scout and sibling): analyze_javascript_application on the whole out/ (ev_e36e1eedc725e9df78ee44d90a7f13cdf3ba1e1ce9c07c23da21c91423856fa2), on the 12 extensions (ev_131baecb...), inspect_pe_resources on Cursor.exe and cursorsandbox.exe. Net assessment: REA gave a process and module inventory and parse confirmation but its semantic graph does not reach inside these large bundles and its string seeds missed literals I could read directly. Nearly all findings therefore come from reading the shipped code and protobuf descriptors. Tools used by me: analyze_javascript_application, inspect_analysis_view, trace_application_feature.

Corrections to the scout inventory (verified):
- aiserver.v1.ConversationsService has only GetCommitMetrics and GetCommitMetricsByFilePath, so it is not conversation search. Search is a local SQLite FTS5 utility process (#065).
- "Canvas" was marked unconfirmed. It is real: an agent SDK with typed React components ships in extensions/cursor-agent-exec/dist/agent-sdk/cursor/canvas (#099).
- The "restoreWorkspaceFiles" log lines in the desktop bundle are a mock cloud-environment progress animation, not checkpoints. Checkpoints are ComposerCheckpointService and a checkpoint storage service (#036).
- "No explicit prompt-injection defense" needs refinement: there are prompt-level cautions and a quarantine note around replayed transcripts (#092). "No ZDR literal" stands: the 16 "zdr" matches in the desktop bundle are minified identifiers (zDr), not product code.
- Chat "Memories" exists only as legacy RPCs (#071), not as a current feature.

## 1. Executive summary

Cursor is a VS Code fork whose AI layer has grown into three products sharing one agent runtime: the IDE, a separate agent-first window called Glass, and cloud agents (plus Grok Bot, a separate desktop agent built from the same @anysphere packages, #123). What matters for Verevon is not the editor but how the harness is built.

1. One typed agent protocol (agent.v1) separates the model loop from tool execution: the loop asks the client or a VM to run shell, read, write, grep, MCP, computer use, hooks and subagents, so the same agent runs on a laptop, a cloud VM or a private worker (#024). Conversation state is content-addressed, encrypted blobs held by the client and pulled by the server through a KV side channel (#120).
2. Context is engineered as a cache-stable prefix plus volatile reminders (#047), compacted with cache-preserving self-summaries and vendor-native paths (#032), and large outputs, terminals, transcripts and MCP tool descriptors are turned into files the agent can grep (#035, #053, #064).
3. Extensibility is unusually complete and deliberately cross-vendor: rules with four types (#058), skills with a token-budgeted catalogue (#057), plugins that carry skills, rules, commands, subagents, hooks and MCP servers (#056), 21 hook events with fail-closed and prompt hooks (#060), and readers for Claude, Codex and agents layouts (#061).
4. MCP is treated as infrastructure: an isolated process, MCP Apps, OAuth refresh leases, progressive disclosure through files and meta-tools, per-server network modes and sandboxing for stdio servers (#050 to #055).
5. Governance is a first-class design axis: layered approval modes and an auto-review classifier (#080, #081), a Rust sandbox with a protected-path list and model-requested escalation (#083 to #085), a privacy-mode matrix and a typed RedactedString (#087, #088), index path encryption (#090) and an admin object of about 70 controls (#093).
6. Cloud agents are a large product surface (212 RPCs, #037) with environment-as-code (#038), prebuilt images (#126), automations with an LLM admission filter (#040), event subscriptions (#125), project and multitask orchestration (#124, #027) and named long-lived agents (#066).
7. Tab completion is the signature model feature: streamed multi-line edit prediction, next-edit cursor jumps, delta file sync and a very detailed fate and evaluation loop (#001 to #006). It is IDE-specific, but its context contract and tuning approach are instructive.
8. Weak spots: a vast legacy API surface and flag sprawl, no filesystem sandbox on Windows, prompt-level injection defence, no automatic memory write in the agent path, and no deep research or document generation (sections 5 and 7).

## 2. Architecture

### 2.1 Layers and processes (SRC, REA for the inventory)

- Shells: the classic workbench renderer (workbench.desktop.main.js, 41 MB), the Glass renderer (workbench.glass.main.js, 49 MB, about 1,100 React component modules and about 110 glass_* flags), an automations UI bundle (8.9 MB) and a CLI (out/cli.js). Electron main (out/main.js) coordinates them with utility processes: mcpProcess, conversationSearch, sharedProcess, extensionMonitor, processExplorer, watcher (#122).
- Agent plane inside extension hosts: cursor-agent-host (AgentHostService: sessions, forks, interrupts, queued turns; chooses the backend loop or a managed local loop behind agent_host_local_loop, #024), cursor-agent-exec (the @anysphere/agent code: prompts, loop, tools, hooks, sandbox, canvas, MCP meta-tools), cursor-local-agent-runtime, cursor-shadow-workspace (#010), cursor-retrieval (index, Crepe grep, #011, #012), cursor-mcp, cursor-browser-automation (#110), cursor-commits (#018), cursor-always-local (JSON schemas, git worker) and cursor-agent-worker (#043).
- Native helpers in resources/helpers: crepectl.exe (Rust n-gram index, 5.3 MB), cursorsandbox.exe (Rust sandbox and proxy, 3.5 MB), node.exe (87 MB), plus node-pty and ripgrep.
- Backend contracts compiled into the client: aiserver.v1 (about 65 services, from a 4,556-name message list) and agent.v1 (9 services). AiService alone has about 200 methods and records the product's history (StreamCpp, StreamBugBot, StreamUiBestOfNJudge, StreamStt, CalculateAutoSelection, RunWebSearch, PotentiallyGenerateMemory, EvaluatePromptHook). BackgroundComposerService has 212 methods and DashboardService 593 (INF from its method names: the web dashboard API). ChatService still carries a legacy loop (StreamUnifiedChatWithTools, ConvertOALToNAL) next to the new agent loop (NAL), which shows a migration in progress.

### 2.2 Data model (SRC)

- Conversation state (agent.v1.ConversationStateStructure): ids of root prompt messages, turns, todos, summary and summary archives, plans, file states (v2), subagent states and threads, goal state, completed ask-question ids, durable skill blocks, durable custom mode id, token details and context-usage tree. Blobs are SHA-256 content-addressed; only new blobs are uploaded; the server asks the client for blobs through KvServerMessage (get_blob, set_blob); stores are layered (cached, write-through, encrypted with AES-GCM and a 12-byte IV, nearby, lazy reference) with a fallback key for rotation (#120).
- ComposerData (client): per-chat record in SQLite (state.vscdb; a composerHeaders table with indexes) with checkpoint ids, git checkpoint, worktree and plan references, best-of-N flags, context-usage tree, share id, queue items, blob and speculative-summarization keys (#036, #019, #101).
- Project-local state: .cursor/ holds rules, skills, commands, agents, plans, worktrees, mcp.json, hooks.json, permissions.json, sandbox.json, environment.json and a projects/<p>/ folder with canvases, mcps (tool descriptors), agent-tools (large outputs), agent-transcripts and notes (#035, #053, #099, #136).

### 2.3 Protocols (SRC)

- AgentClientMessage: run_request, exec results, kv results, conversation_action, interaction_response, heartbeat, prewarm_request. AgentServerMessage: interaction_update (text, thinking, tool-call lifecycle, summaries, prompt_suggestion, response_comparison, context_injection_state, routed_model), exec requests (about 40 kinds), conversation_checkpoint_update, kv requests, interaction_query, ttft_breakdown (#024, #048, #049).
- Transport: Connect RPC over HTTP/2 with SSE and polling fallbacks and idempotent variants (#121). Remote configuration is parsed with Zod schemas with fallbacks (MCP reconnect, model lifecycle, transcript grouping, onboarding actions).
- Feature flags: a registry of 641 typed client flags (182 with a fallback default of on) sits under the Statsig client; checkFeatureGate returns the registry default only when the experiment service is uninitialized or errors, and about 20 flags are named kill switches (#145). Fallback defaults are quoted in the notes of records that depend on flags, because they show which features are core (enable_multitask_mode, editor_bugbot, context_visualizer: on) and which are still staged (custom_agents_enabled, agent_goal_continuation, full_self_driving, instant_grep_indexing: off).

## 3. Feature areas

### 3.1 Tab completion and inline assist (#001 to #007, category Coding capability)

Tab is a streamed edit predictor, not a completion list. The request (StreamCppRequest) carries the current file, three encodings of edit history, diagnostics, scored context items, LSP suggestions and parameter hints, additional files and delta file-sync updates; the response streams text with a range_to_replace, begin and done edit markers, a confidence and an optional cursor_prediction_target (#001, #002, #004). Behaviour is tuned from the server (radii, debounce, ghost text vs diff) and filtered on the client with six named heuristics (#003). Guardrails: no Tab in .env-style files or user-disabled languages, and hooks for Tab reads and writes (#005). A 40-kind session event timeline and labelled evaluation categories feed training and evaluation (#006). Cmd+K inline edit and terminal command generation are older, selection-scoped companions (#007).

Takeaway: the interesting part for a non-IDE product is the context contract (recent edits, diagnostics, structured symbols, delta sync) and the discipline of remote tuning plus named suppression rules.

### 3.2 The agent loop (Composer): modes, tools, subagents (#024 to #049)

- Modes: AGENT, ASK, PLAN, DEBUG, TRIAGE, PROJECT, MULTITASK, CUSTOM, with mode reminders and a model-initiated switch_mode that the user accepts (#025). Plan mode writes <name>_<8hex>.plan.md with frontmatter and todos and offers Build in Parallel (#026). Debug mode is hypothesis-driven with an NDJSON log sink on localhost ports 7242 to 7942 (#022). Multitask mode turns the foreground agent into a coordinator that delegates to async subagents and ends its turn (#027). Project mode is an orchestrator that may not edit files, with phase gates, exclusive file claims, a parallel review panel and a single-writer progress.md (#124). Multi-agent synthesis runs one subagent per model in separate worktrees and merges (#097); best-of-N applies one result and undoes the rest (#019).
- Tools: about 75 tool-call types, including read, write or StrReplace or ApplyPatch (chosen by vendor, #008), grep, glob, semantic search, shell with block_until_ms, notify_on_output and required_permissions (#035, #085), web search and fetch, MCP meta-tools, todos, goals, plans, ask_question, image generation, computer use, record_screen, PR management, CI findings, reflect and subagent control.
- Subagents: explore, shell, computerUse, browserUse, debug, vmSetupHelper, watchVideo, cursorBlameLearning, ciInvestigator, bugbot, security-review, coordinator-agent, worker-agent, a general default and user-defined agents from .cursor/agents (#028, #029). Nesting depth is controlled by omitting the Task tool.
- Hygiene: a streaming repetition detector with false-positive guards, bounded retries for max-token and empty responses, exponential backoff with jitter (#033); streaming tool execution that dispatches calls as soon as arguments complete (#034); checkpoints and resume (#036); a typed interaction-query channel for every human decision (#046); a context-injection lifecycle for queued messages (#031).

### 3.3 Context assembly and management (#032, #035, #047, #114, #134)

The first user message holds user_info, rules, a budgeted skills catalogue, subagent descriptions, git status, transcripts location and MCP servers; each turn adds user_query, attached files, recent files, rules context and reminders; model switches add a reminder that earlier turns came from another model (#047, #075). Compaction uses the same model, system prompt, tools and prefix so the provider cache keeps hitting, appends a summary request and re-injects plan, todos and skills; vendor-native compaction (Anthropic beta or explicit, OpenAI and xAI prompts), background pre-computation with start and persist thresholds, and a transcript-truncation fallback wrapped in an injection warning complete it (#032). The product exposes the result as a context-usage tree with sources and token counts (#114). Request context is hydrated progressively with per-source completion flags so the run can start early (#134).

### 3.4 Codebase indexing and search (#011 to #014)

A native Merkle client hashes embeddable files, computes a SimHash vector and handshakes with the server, which can copy a similar existing index; only changed subtrees and files upload (50 concurrent uploads, auto-index cap 10,000 files, 600 s period). Paths are AES-256-CTR encrypted with a per-repo key whose plaintext reaches the server only when privacy mode is off (#090). Semantic search offers custom embedding models (three Qwen variants, Voyage code and TEXT_EMBEDDINGS_LARGE_3, probably OpenAI's large model, INF) with rerank; exact search uses Crepe, a Rust sparse n-gram index with an explicit ripgrep fallback taxonomy and a "fallback storm" alert (#012). Git history is indexed too (PR history search, commit graph).

### 3.5 Rules, skills, plugins, hooks, MCP, commands (#050 to #063)

The shape is summarised in section 1, items 3 and 4. Notable design decisions: rule type derives from frontmatter (alwaysApply, globs, description, otherwise manual); skills are discovered across .cursor, .agents, .claude and .codex and listed within 2 percent of the context budget; plugins pin sources by commit, carry typed variables with write-only secrets and declare minimum client versions for both Cursor and Grok Bot ("sand"); team admins push rules, hooks, marketplaces and required plugins (#055, #058, #060). About 33 built-in slash commands cover authoring (create-rule, create-skill, create-hook, create-subagent), scheduling (schedule, loop, subscribe), review (review, review-bugbot, review-security), goals and environment setup (#059).

### 3.6 Memory and learning (#064 to #071, #136)

There is no automatic memory store in the agent path. Learning relies on: transcripts as searchable files plus a past_conversation_explorer subagent and a prompt that asks "always, or just this case?" (#064); a local FTS5 index for chat search (#065); notes folders for other agents (#136); named agents with SELF.md and SOUL.md (#066); per-automation memory files with compare-and-set (#067); Bugbot rules learned from reactions with example PRs and path scopes (#068); skills from demonstration (#069). Legacy chat memories and a knowledge base remain as RPCs only (#071). Compared with Hermes this is explicit and file-based rather than a learning loop.

### 3.7 Background and cloud agents, automations (#037 to #045, #124 to #127)

BackgroundComposerService covers lifecycle (create, attach, pause, resume, wake, archive), pods and snapshots, environment versions and builds (manual, recurring, config-change; failure types), follow-ups and reordering, PR operations (merge, auto-merge, thread resolution), Slack, GitHub and Linear streaming, artifacts and sharing, VM usage, keyrings and private workers. Agents start from 34 origin values (editor, web, iOS, API, SDK, CLI, Slack, Teams, Linear, Jira, GitHub, GitLab, Bitbucket, CI autofix, Bugbot autofix, automations, other agents, Grok Bot). Automations combine triggers (cron, Slack, Linear, PagerDuty, Sentry, Teams, Git, webhook), actions, prompts, model, memory and deploy specs, with an LLM filter that records its decision and rationale per run (#040). Environments are declared in .cursor/environment.json and prebuilt (#038, #126). Cloud agents follow a written contract for git, testing and final messages (#098) and, in automation runs, deliver results through channel tools rather than the final channel (#135).

### 3.8 Review bots and PR flows (#015 to #021, #144)

Bugbot has effort tiers (default, high, auto, frontier), autofix modes (off, manual, auto-PR, auto-merge), seat or token billing, feedback reactions, learned and manual repository rules, and live status with iterations, tokens and cost (#015, #068). In the IDE the agent can launch a Bugbot-style subagent, or a security review, as a single-shot task with a fixed prompt form, and auto-run on commit is a setting (#016). The PR workspace shows threads, checks and stacks, with footers for Bugbot, automation and self-driving comments and an "add to chat" action (#144). FSD is an autonomous PR agent with findings, auto-apply configs and team policy (#041, inferred).

### 3.9 Models and routing (#072 to #079, #096, #134)

A managed catalogue with lifecycle states and upgrade targets; parameterised models (effort and thinking as id/value pairs); Max mode behind a confirmation; Auto routing that reports the routed model and collects feedback; BYOK for API keys, Azure and Bedrock (several credential chains) and a key-helper script for local providers; a customer LLM gateway with per-provider routing; per-subagent model overrides; per-family prompt variants and tool shapes; rolling Anthropic cache breakpoints (max four), prefix-preserving summaries, prewarm and cache warm calls; middleware for image resizing, effort level, continuation and trailing-empty cleanup. Slugs in the bundle include Cursor's own composer-1 to 2.5 (the local agent's model resolver falls back from composer-2 to grok-composer-2, INF on purpose), Claude, GPT-5.x, Grok 4.x, Gemini 2.5 and 3, and kimi-k2p5 variants (presence does not prove availability).

### 3.10 Privacy, safety and governance (#080 to #095, #138)

Approvals: allowlist, unrestricted or manual, plus auto-review with a shadow mode, merged from user, project and admin layers with natural-language allow and block instructions; shell commands are parsed into executables, redirects and substitutions before allowlist checks (#080 to #082). Execution: a Rust sandbox with workspace policies and a proxy, a hard-coded list of files the agent can never write even in a writable workspace, and per-command escalation requests (#083 to #085); network egress policy spans local, MCP and cloud (#086). Privacy: five modes decide per (field class, purpose) what may be stored, logged or trained on; strings are wrapped in a RedactedString that cannot be serialised implicitly; each request states its mode in a header and the client can ask the server whether it is on a privacy pod; the index path key is withheld in privacy mode (#087 to #090). Admin: one settings object of about 70 controls with locks and ceilings (#093). Secrets in cloud runs use grant-based keyrings with redaction and commit scanning (#091). A competitor-usage analytics task reads other tools' history metadata, and its client fallback default is on (#138). The sandbox is force-disabled on Windows by a flag that also falls back to on (#083).

## 4. What is best in class (among the peer set, from client evidence)

- Privacy by construction: the (mode, class, purpose) matrix and RedactedString (#087, #088), index path encryption (#090) and client-held encrypted state (#120).
- Protected paths in a writable sandbox and model-requested escalation (#084, #085).
- MCP as infrastructure: progressive disclosure, OAuth refresh leases, per-server network modes (#052 to #054).
- Hooks: the broadest event set with fail-closed and team push (#060).
- Compaction and caching engineering (#032, #076) and the typed runtime protocol (#024).
- Context inspector tree with sources (#114).
- Native search engineering: Crepe with fallback taxonomy, Merkle index reuse (#011, #012).
- Automations as a product (#040) and the enterprise admin object (#093).
- Config-as-forms, anchored trays and canvases as UI patterns (#117, #139, #099).

## 5. Weaknesses and limitations

- Legacy and sprawl: about 640 distinct client feature-flag names (111 prefixed glass_, 44 sand_, 34 cloud_agent_, six origin_ flags for an unreleased hosting surface, #146); overlapping old and new loops (OAL and NAL); services with hundreds of methods; multiple retired surfaces still shipped (memories, AI lint, apply model, notepads). This makes behaviour hard to reason about and likely hard to maintain.
- Windows sandbox is proxy-only with no filesystem isolation (#083); Linux depends on a kernel preflight.
- Injection defence is mostly prompts and approvals (#092); no detector or provenance tagging was found.
- Privacy claims are contracts the client cannot prove: PrivacyCheck returns a server flag, and session events, edit history and prompt-quality uploads are gated by mode, but enforcement is server-side (#089, #006). A hidden analytics task reads other tools' history metadata and its client fallback default is on (#138).
- No automatic memory write, no deep research product and no document, slide or sheet generation were found; artifacts are code canvases (#099).
- Cloud and review features depend on the vendor backend (self-hosting only through private workers, #043), and many behaviours (Tab, Bugbot, routing, FSD) are server-side and unobservable.
- Prompt-heavy design: modes, Multitask, Project and synthesis behaviours rely on long prompts; without evals the same design could behave differently in another harness.
- Quality ratings are from client evidence only; I cannot judge suggestion or review quality.

## 6. What Verevon should copy, adapt or avoid

Constraints from the Verevon baselines that shape adoption: plane ownership (the Model proposes browser actions, Quarry executes), two separate loops (the chat loop is read-only; every effect goes through execution-core's capability, hook, permission, HITL pipeline), capability-before-dispatch with fail-closed defaults, ZDR that must propagate and forbids persistence-bearing features, Space access never widening an owner ACL, no registry for a single implementation, and licence gating (proprietary code may contribute shapes only, so everything below is a design idea to re-implement, not code to port).

### 6.1 Copy or adapt (priority order, mapped to documented Verevon gaps)

| Priority | Verevon gap | Cursor design to adopt | Records |
|---|---|---|---|
| P0 | MCP tools cannot be invoked | Tool descriptors as files plus generic call tool, schema-first rule, per-server capability binding, network modes, refresh-lease OAuth | #053, #054, #052, #050 |
| P0 | Hooks not applied in Do mode | Event list, fail-closed flag, updated_input and additional_context contract, team hooks | #060 |
| P0 | Do mode is context-blind | First-message context block, four rule types, AGENTS.md and SKILL.md folders, org-required rules | #047, #058, #057 |
| P0 | ZDR must propagate | (mode, class, purpose) matrix, typed redacted strings in Rust, client-held encrypted state, privacy attestation call | #087, #088, #120, #089 |
| P0 | No rewind or checkpoints | Per-message file checkpoints with conversation state at that point | #036, #120 |
| P1 | Weak sandbox and approvals | Policy shape, protected-path list, model-requested escalation, shell parsing, auto-review with shadow mode, permissions file | #083 to #085, #081, #082, #080 |
| P1 | No server-side chat search, share, import | FTS index process with recency-weighted ranking, share links with fork, Claude import modal | #065, #101, #137 |
| P1 | Scheduled runs are a single call | Workflow objects with triggers, LLM admission filter, run history, memory files, subscriptions that park a run, channel-tool delivery | #040, #125, #067, #135 |
| P1 | Plugins inert, no marketplace | Manifest accepting Cursor and Claude layouts, commit pinning, team marketplace with required installs | #056, #055 |
| P1 | Compaction and latency | Cache-preserving self-summary, background pre-compute, progressive hydration, prewarm | #032, #134, #076 |
| P2 | Transparency | Context-usage tree, TTFT breakdown, request ids, run analytics | #114, #115, #130 |
| P2 | Reasoning and effort controls | Parameterised model catalogue with lifecycle and upgrade targets, per-family prompts | #072, #075 |
| P2 | No per-session VM, no parallel agents | Environment as code, prebuilt images, worktrees with quotas, private workers, project and multitask orchestration | #038, #126, #112, #043, #124, #027 |
| P2 | Model comparison absent | Best-of-N with undo, multi-model synthesis, sampled response comparison | #019, #097, #049 |
| P3 | Artifacts read-only | Typed canvas SDK with compile diagnostics, selection-scoped edit, design mode | #099, #007, #140 |

### 6.2 Avoid or handle with care

- Reading other products' local files for analytics, even metadata-only (#138); it conflicts with Verevon's trust positioning.
- A single mega-service and flag sprawl (#121 weaknesses); keep Model Plane contracts small and versioned.
- Windows-style partial sandboxes presented as sandboxes (#083); say clearly what is isolated.
- Prompt-only safety rules (no force push, ignore linked threads); enforce them in capability policy and hooks as well (#092, #098).
- Mode proliferation in one loop: adopt Project, Multitask and Plan behaviours in Do mode only, keeping the read-only chat loop separate.
- Features needing server-side retention (memories, checkpoints, cloud snapshots) for ZDR callers: gate them as Verevon already gates Do mode.
- Treat "privacy mode" as a promise unless attested: add an attestation call and tests (#089).

### 6.3 Mapping to the audit taxonomy

Counts and mean quality by category (146 records): Coding capability 25 (3.8), Agent runtime 31 (4.0), Tools, plugins and extensibility 14 (4.3), Memory, learning and personalization 9 (3.3), Models and routing 10 (3.9), Safety, permissions and governance 17 (4.1), Chat experience 8 (3.5), Multimodal and creation 3 (3.3), Research and web 2 (3.0), Computer and environment control 6 (3.8), Transparency, observability and control 4 (4.3), UI/UX and product design 11 (3.6), Platform architecture 6 (3.8). Quality distribution: 18 records at 5, 95 at 4, 29 at 3, 4 at 2. Uniqueness: 57 unique, 63 uncommon, 26 common. Confidence: 113 high, 30 medium, 3 low. Cursor's strongest categories are extensibility, safety and agent runtime; its weakest are research, multimodal creation and memory.

## 7. Open questions

1. How does the server decrypt or use client-held blobs, and is the blob key sent per request (#120)? That decides how strong the "stateless server" story is.
2. Does NO_STORAGE equal contractual zero data retention, and is it attested anywhere (#087, #089)?
3. The client fallback for enable_ex_hs is on (#138) and the live value is server-side: is the history-reading task active in production, and what else uses the same pattern?
4. What are the actual Tab models and thresholds (#003), and do session events upload in privacy mode (#006)?
5. How do Auto routing and Bugbot modes choose models or depth (#073, #015)?
6. Are legacy memories retired for good, and is GetUserInstructions wired (#070, #071)?
7. What is Full Self Driving's real behaviour and default state (#041)?
8. How is the Grok Bot lineage (shared packages, aliases) governed, and what is shared server-side (#123)?
9. Which flags are default-on in production? Statsig values are server-side and invisible here.
10. Will the Windows sandbox gain filesystem isolation, and is the macOS sandbox profile stricter than the Linux one (#083)?

## 8. Limits of this audit

- Client-side only: server logic (models, Bugbot, routing, Tab, FSD) was not observable. Names of RPCs and fields are weaker evidence than UI strings, prompts and implemented code; records say which one they rest on.
- Minified code was read with targeted scripts and beautified copies; I did not run the app, any installer or any project code, and I did not read user data directories. Secret-bearing values (client keys, endpoints) were not recorded.
- REA coverage was partial (see section 0). Spot-checks of sibling notes were limited to the claims I used.
- Quality ratings are relative and judged from evidence of design maturity and execution, not from outcomes.
