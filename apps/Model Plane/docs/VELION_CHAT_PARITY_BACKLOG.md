# Verevon Chat — Competitive Parity Backlog (2026-08-01)

> Chat is the platform: every UI in Verevon is an API the model can drive, powered by
> the model-gateway chat loop. This document maps what the leading AI chat/coding
> harnesses do best onto concrete parity work for Verevon chat. The goal is not to
> copy but to reach parity where it makes Verevon better.
>
> Method: 10 harnesses researched from their **actual source** (where open) and
> expert commentary (where closed), each report adversarially scoped against what
> Verevon already ships, then synthesized into a ranked backlog. Harnesses covered:
> Claude Code, OpenAI Codex CLI, OpenCode (sst/opencode), ChatGPT, Perplexity,
> Manus.im, Nous Hermes agent, OpenClaw, Pi — plus the 2025–2026 expert consensus
> (Anthropic "Building Effective Agents" / "Writing Tools for Agents" / "Effective
> Context Engineering"; HumanLayer 12-Factor Agents; Cognition "Don't Build
> Multi-Agents"; Manus "Context Engineering"; Simon Willison; swyx/Latent Space).
>
> Effort key: S = days, M = 1–2 weeks, L = multi-week.

# VEREVON CHAT — PARITY BACKLOG

Synthesis of 9 harness reports + expert consensus into a ranked, deduplicated build plan. Every item is a thing Verevon is missing or only partially has. Citations name the harness and source that establish the bar.

---

## 1. RANKED BACKLOG

Ranking weights: convergence across harnesses, whether it's a Verevon self-declared gap, whether it unblocks other items, enterprise/strategic fit, and effort. Effort is S (days) / M (1–2 wk) / L (multi-week).

### #1 — Server-authoritative chat sessions + stream-resume across reconnect
- **Best-in-class:** OpenCode (server-owned sessions + Event Bus, persisted message parts replayed on reconnect — deepwiki.com/sst/opencode/2.1-session-management); OpenAI Codex (on-disk `rollout` + `codex resume` — github.com/openai/codex docs/config.md); consensus (12-factor "pause/resume/trigger from anywhere" — github.com/humanlayer/12-factor-agents).
- **Why it matters:** This is Verevon's own #1 known-weak item (transport buffers exist; SPA merge + producer detach unfinished). It is also the structural prerequisite for checkpoint/rewind, branch, edit-versions, and share links — build it once, four items unblock.
- **Verevon coverage:** partial. Durable runs already do this (durable runId + live panel attach); ordinary chat turns do not.
- **Effort:** M.
- **First step:** In `apps/gateway`, make chat turns persist canonical message-parts (text deltas + tool events) keyed by threadId to the same durable store the run-events stream uses. Change the SolidJS SPA transport to replay-from-buffer then re-subscribe on reconnect, mirroring the agent-panel's durable-run attach. Apply the proven run-attach model to plain chat.

### #2 — Declarative hook + permission-policy layer at the gateway (make HITL load-bearing)
- **Best-in-class:** Claude Code (PreToolUse allow/deny/ask + input rewrite, PostToolUse output rewrite, Stop gate — docs.anthropic.com/en/docs/claude-code/hooks); Codex (`execpolicy` Starlark allow/prompt/forbidden with CI match/not_match fixtures — github.com/openai/codex execpolicy/README.md; managed-hooks-only for admins); OpenCode (opencode.json ask/allow/deny + wildcard bash patterns — deepwiki.com/sst/opencode/5.2-permission-system).
- **Why it matters:** Verevon's own audit memory repeatedly flags "HITL approval decorative on live writes." A declarative policy the gateway enforces *before dispatch* converts the model's cooperation into a hard boundary — and turns ZDR redaction / cross-tenant scoping into config, not hope. This is an enterprise moat, not just parity.
- **Verevon coverage:** partial (one hardcoded slice: risky-tool HITL; capability attestation).
- **Effort:** M.
- **First step:** Add a hook dispatch point around every tool call in the model-gateway multi-round loop that consults a per-org policy doc returning allow/deny/ask + optional rewritten tool input. Key rules by action-contract ID (`shipping.send:ask`, `visma.mutate:ask`, `web_search:allow`). Add match/not_match fixtures in CI so scope broadening fails the build (Codex pattern). Make the existing HITL gate *read* this policy.

### #3 — Skills-as-files registry (authorable, progressively disclosed, self-patching)
- **Best-in-class:** Claude Code (SKILL.md, name+description ~100 tokens always loaded, body/scripts on demand, script code never enters context — docs.anthropic.com/.../agent-skills/overview); Manus, OpenClaw, Codex all ship the same format; Hermes auto-authors + self-patches skill files ("backpropagation for prompts, not weights" — agentic-ai.readthedocs.io/.../hermes-agent).
- **Why it matters:** Seven of nine reports converge here. Lets Verevon add domain capabilities (Norway data commons, Visma flows, shipping, compliance playbooks) at near-zero standing context cost, as versioned files instead of system-prompt bloat. Verevon's existing "skill learning" is a Wilson-score *quality* loop — a different thing; it produces scores, not retrievable procedures.
- **Verevon coverage:** partial (Wilson loop + RUN_COMPLETED→skill learning, but no skill *files*).
- **Effort:** M.
- **First step:** Define a SKILL manifest (name, trigger/description, markdown body, optional script deps) stored per-org in Data Plane. Inject descriptions always; serve bodies on demand via the same tool-spec path as `knowledge_search`; run bundled scripts in the execution-core bwrap sandbox so code never enters context. Put the description metadata in the `src/shared/actions` action-contract convention. Close the loop by having the Dreaming/skill-learning path *emit* a skill file, not just a score.

### #4 — In-chat subagent / fan-out orchestration (isolated context, summary-return)
- **Best-in-class:** Claude Code (Task tool, isolated context window, returns only a summary — docs.anthropic.com/.../sub-agents); Manus "Wide Research" parallel sub-agents (manus.im/blog/Context-Engineering); Anthropic multi-agent research system (read-heavy fan-out + re-synthesis, ~15× tokens — anthropic.com/engineering/multi-agent-research-system). **Guardrail:** Cognition says single-thread write work; parallelize read/gather only, always pass full trace (cognition.ai/blog/dont-build-multi-agents).
- **Why it matters:** Verevon's explicit stated gap ("no subagent/parallel-agent orchestration in chat"). Quarantines verbose exploration so it never floods the main thread's budget. Deep research already has this shape — formalize it.
- **Verevon coverage:** partial (durable RunAgent + parallel tool dispatch + agent console, but no chat-invokable fan-out).
- **Effort:** L.
- **First step:** Expose a `spawn_subagent` tool (as a `src/shared/actions` contract) that starts a child RunAgent with its own context + restricted tool set + cost ceiling, streams into a child panel, and returns only a compacted summary to the parent turn. Scope the first version to **read-only** research subagents (stays on the safe side of the Cognition/Anthropic split); the missing piece is the summary-return contract + parent-context isolation.

### #5 — Inline citation chips + multi-altitude source browser + retrieval quality gate
- **Best-in-class:** Perplexity (domain chips with +N grouping, 1/N quick-verify popover, favicon stack, peer Sources tab, "check sources" on text selection, ~0.7 rerank abstain-and-requery fail-safe — aiuxplayground.com/teardowns/perplexity/citations; ziptie.dev/blog/how-perplexity-ai-answers-work).
- **Why it matters:** Grounding is Verevon's core differentiator; Perplexity is the bar every reviewer measures against. Verevon already computes citations + a graduated confidence score but *presents* rather than *gates* on it, and lacks the inline-chip UX. Cheap because the payload exists.
- **Verevon coverage:** partial.
- **Effort:** S (chips/browser), M (quality gate).
- **First step:** Add a chat-message renderer that attaches inline domain chips at claim boundaries (group co-supporting sources with +N) with a hover popover paginating 1/N over the existing citation payload — new UI contract in `src/shared/actions`. Then add a threshold branch in the grounding orchestrator: below confidence X, re-issue one broadened sub-query or return an explicit "weak sources" banner instead of citing as solid.

### #6 — Slash / custom-command system with composer autocomplete
- **Best-in-class:** Claude Code (`.claude/commands/*.md`, `$ARGUMENTS`/`$N` substitution, per-invocation allowed-tools — docs.anthropic.com/.../slash-commands); Hermes (slash-command autocomplete); consensus.
- **Why it matters:** Verevon's explicit stated gap. Gives power users deterministic, parameterized entry points (`/reconcile-order`, `/crawl-and-summarize`) that pre-authorize exactly the tools that workflow needs for one turn — cheaper and more predictable than model intent inference. Follow-up chips are model-suggested next-turns — orthogonal.
- **Verevon coverage:** no.
- **Effort:** S.
- **First step:** Add an org/user-keyed command registry; parse leading `/name args` in the SPA composer, autocomplete from the `src/shared/actions` registry, expand a stored template with typed args + a one-turn tool allowlist, dispatch straight to the action.

### #7 — Checkpoint / rewind of conversation (+ reversible workspace state)
- **Best-in-class:** Claude Code (file snapshot before every prompt, `/rewind` restores code/conversation/both, summarize-from-here — docs.anthropic.com/.../checkpointing); OpenCode (git-style per-response snapshots, /undo /redo restore files AND messages); Manus recoverable state.
- **Why it matters:** Verevon's stated gap. Makes bold multi-step agent actions low-risk. Note CC's own limitation — it does *not* track bash/side-effect mutations; Verevon should scope checkpoints to reversible projections and mark provider writes as non-rewindable.
- **Verevon coverage:** no.
- **Effort:** M (rides on #1).
- **First step:** Once sessions are server-authoritative (#1), snapshot conversation + Application-Plane workspace projection per user turn as an event-sourced reducer; expose restore-conversation / restore-both. Mark external provider writes non-rewindable.

### #8 — Edit-message version siblings (1/2 switcher) + branch-conversation
- **Best-in-class:** ChatGPT (edit produces navigable version siblings; "Branch conversation" forks an independent thread, original intact — tech.yahoo.com; openai.com); OpenCode (revert removes subsequent messages from active projection).
- **Why it matters:** Editing a prompt after a bad answer is the single most common corrective gesture. Verevon's stated gap. Verevon *already tracks* edit-resubmit as a dissatisfaction signal — persisting the sibling instead of discarding it enriches that signal AND gives the switcher for free.
- **Verevon coverage:** no.
- **Effort:** M.
- **First step:** Model turns as a sibling array per user-message node (not a flat list); render prev/next arrows on edited messages. Add `branch_conversation(messageId)` action (server-owned, same pattern as the recent server-owned pin work) that clones thread state up to that message into a new threadId; seed the branch context via existing compaction.

### #9 — Persistent per-run workspace as filesystem-memory + restorable-by-reference compaction
- **Best-in-class:** Manus (filesystem is "ultimate context"; compress by dropping a page body but keeping its URL/path — manus.im/blog/Context-Engineering); Hermes (SQLite + files as externalized memory); consensus (structured note-taking / external memory).
- **Why it matters:** Removes the context-window ceiling and makes compaction lossless-by-reference instead of lossy — long grounded/research runs stop degrading when trimmed.
- **Verevon coverage:** partial (execution-core sandbox + file artifacts exist, but not a persistent per-run workspace the model reads/writes across steps).
- **Effort:** M.
- **First step:** Promote the bwrap sandbox to a persistent per-run workspace exposed as `workspace.read/write/list` action contracts. Change compaction to store dropped tool-result bodies (web_search/fetch_url/knowledge_search) to files, replacing them in-context with a path/URL stub the model can re-open.

### #10 — Model-managed plan/todo recitation into the context tail
- **Best-in-class:** Manus (continuously-rewritten `todo.md` recited into recent tokens to fight lost-in-the-middle — manus.im/blog); consensus (recitation, memory files).
- **Why it matters:** Surfacing a plan to the *user* (Verevon has this via plan mode + run console) is not the same as reciting it back into the *model* each turn. Cheap, no architecture change, directly attacks goal-drift on ~50-call loops.
- **Verevon coverage:** partial.
- **Effort:** S.
- **First step:** Persist the plan as `workspace/todo.md`; have the model-gateway loop re-append the current checklist (done/remaining) as the last context block before each model call; render the same file in the run console so UI and model read one source of truth.

### #11 — Code-mode: Verevon tools callable from inside the sandbox (collapse N tool rounds into one script)
- **Best-in-class:** Codex "code mode" (github.com/openai/codex code-mode/src/lib.rs); Hermes Python-RPC "zero-context-cost turns."
- **Why it matters:** Verevon memory notes tools are dispatched sequentially and bounded by MAX_TOOL_ROUNDS. One authored script can loop/filter/join across many tools without each intermediate result re-entering context — big token + latency savings.
- **Verevon coverage:** partial (run_code exists but can't call Verevon's own tools).
- **Effort:** M.
- **First step:** Expose an in-sandbox RPC client (`knowledge_search`, `web_search`, `fetch_url`, provider actions) over a local socket the gateway proxies with the caller's scoped auth; keep the #2 HITL/policy gate at the sandbox boundary.

### #12 — Post-mutation / external-verifier feedback loop
- **Best-in-class:** OpenCode (LSP `didChange`→diagnostics fed back into context same turn — deepwiki.com/sst/opencode); Willison "close the loop" (simonwillison.net/2025/Sep/30/designing-agentic-loops); consensus.
- **Why it matters:** Verevon's grounding is a read-side loop; there's no post-write verification, so the model can claim "done" without checking. Cuts hallucinated success.
- **Verevon coverage:** partial.
- **Effort:** M.
- **First step:** For run_code, feed real type-check/LSP diagnostics back before the next round. For business actions, add an automatic post-action re-fetch (e.g. confirm the Visma order status actually changed) and inject the result.

### #13 — Projects / Spaces: scoped containers (threads + files + instructions + own memory tier)
- **Best-in-class:** ChatGPT Projects (help.openai.com/.../11146739); Perplexity Spaces (default model + file set + custom instructions + collaborators — perplexity.ai/hub/blog).
- **Why it matters:** Prevents context bleed; long-running engagements resume without re-explaining. A cleaner unit than org-wide KB for a specific engagement — high leverage for an enterprise multi-plane product.
- **Verevon coverage:** partial (org/workspace + KB + context-packs, but no first-class project container scoping its own memory).
- **Effort:** L.
- **First step:** Introduce a `project` resource in the gateway with resource_grants-based sharing (existing private-until-shared authz); scope Data Plane retrieval and Dreaming-loop memory writes to `project_id`; default new chats inside a project to its instructions. Compose from LiveKnowledgePayload pattern on the Application Plane.

### #14 — User/org-authored persona + custom-instructions document (CLAUDE.md-style, always-on)
- **Best-in-class:** ChatGPT Custom Instructions (distinct from auto-memory); Claude Code CLAUDE.md hierarchy (managed→user→project→local); Hermes SOUL.md; OpenClaw AGENTS.md+SOUL.md.
- **Why it matters:** Verevon's memory is model-*written* (Dreaming loop). There's no explicit, deterministic, editable steering slot separate from inferred memory. Pairs with the shipped identity-context work (we/us→org, I/my→user): org-level instructions for "us," user-level for "I."
- **Verevon coverage:** partial.
- **Effort:** S.
- **First step:** Add a per-user/per-org persona + instructions document (stored in Data Plane, editable in settings), injected into the InvokeRequest system prompt via a `src/shared/context-packs` context-pack, with clear precedence (platform→org→user→chat), kept separate from the learned `agent_memory` tier.

### #15 — Source-scope "focus" selector in the composer
- **Best-in-class:** Perplexity Focus modes (hard source filter applied at retrieval, upstream of synthesis — perplexity.ai; testingcatalog.com).
- **Why it matters:** Legible control over the evidence pool with zero prompt-craft; the gateway already fans out to Quarry/SearXNG/Exa (web) and DP retrieval + GraphRAG (internal) — this is a UI toggle + a scope param, not new plumbing.
- **Verevon coverage:** partial.
- **Effort:** S.
- **First step:** Add a focus selector in the chat composer mapping to a source-scope flag on the InvokeRequest / `knowledge_search` tool call: "Org knowledge only" / "Web only" / "Both" / Exa-filtered academic/social.

### #16 — Lightweight inline step-trace on ordinary answers
- **Best-in-class:** Perplexity (collapsed plain-language "Searched X, read Y, ranked Z," expandable — langchain.com/breakoutagents/perplexity); ChatGPT collapsible reasoning summary.
- **Why it matters:** Verevon has the *heavy* path (deep research + live panel + durable runId) but no light inline variant for normal turns — casual readers see a clean answer, auditors expand. SSE tool-round events already exist.
- **Verevon coverage:** partial.
- **Effort:** S.
- **First step:** When the multi-round tool loop runs, stream a collapsed summary block above the answer, expandable to the actual sub-queries/tool calls — a compact renderer over data Verevon already streams.

### #17 — Proactive cron / scheduled runs surfaced in chat, delivered to Inbox
- **Best-in-class:** Hermes NL cron scheduler; OpenClaw heartbeat/cron tool delivering to a channel (github.com/openclaw/openclaw).
- **Why it matters:** Converts request/response chat into a standing teammate ("every Monday summarize new competitor pages"). The infra exists (Model Plane cron dispatch with minted JWT; Inbox inbound channels) but chat doesn't expose it.
- **Verevon coverage:** partial.
- **Effort:** M.
- **First step:** Add a chat-facing "schedule this" action that persists an NL task + cadence, dispatches a durable RunAgent via the existing cron path, and posts the run summary into the thread / Inbox (conversation-core) channel.

### #18 — Background-worker delegation with completion notification
- **Best-in-class:** OpenClaw (spawn detached, monitor via process tools, ping user on done/fail via `message send`); Manus async cloud execution.
- **Why it matters:** "Kick it off and tell me on Slack/Inbox when done" — lets a chat turn start long work without holding the stream open.
- **Verevon coverage:** partial (durable RunAgent + panel + HITL, but no completion notification binding).
- **Effort:** S.
- **First step:** Bind each durable run to a notification target (Inbox conversation-core channel); emit a completion/failure message when the run resolves.

### #19 — KV-cache-stable loop discipline + cache-hit metric + mask-don't-remove tools
- **Best-in-class:** Manus (stable append-only prefix, no mid-loop tool churn, deterministic serialization, logit-mask tools by state — manus.im/blog); consensus.
- **Why it matters:** Loops are ~100:1 input:output; a one-token prefix change invalidates cache for everything after. Verevon wires Anthropic prompt caching but the loop discipline is the other half (and the in-process prompt cache dies on deploy per memory).
- **Verevon coverage:** partial.
- **Effort:** S (discipline) / M (masking).
- **First step:** Freeze the system-prompt + ToolSpec[] block at run start (don't add/remove tools mid-run); move volatile data (timestamps, request ids) out of the cached prefix into the tail; add a cache-hit-rate metric next to the existing cost/token badge. Masking (tool_choice / constrained decoding by run phase) is a follow-on where model-gateway supports it.

### #20 — Full-text (FTS) search over all past chat history
- **Best-in-class:** Hermes (SQLite FTS5 + LLM summarization for cross-session recall — agentic-ai.readthedocs.io).
- **Why it matters:** Complements Verevon's semantic tiers (Dreaming/Letta) with exact-term recall ("the invoice number from that March thread") that embeddings miss.
- **Verevon coverage:** partial (semantic memory yes; user-facing FTS over chat history no).
- **Effort:** S.
- **First step:** Add an FTS index (Postgres tsvector or Quickwit — already in-stack) over stored turns behind a `recall_conversations` tool returning summarized hits with thread links + provenance badges.

### #21 — Canvas: selection-scoped inline edits + shortcut menu + version restore
- **Best-in-class:** ChatGPT Canvas (highlight-a-span edit, writing/coding shortcut menus, version back-button — openai.com/index/introducing-canvas).
- **Why it matters:** Moves from "regenerate the whole artifact" to surgical edits. Verevon already ships canvas/artifacts — this is an extension, not a new surface.
- **Verevon coverage:** partial.
- **Effort:** M.
- **First step:** Extend the artifacts pipeline with (a) selection-scoped edit actions passing highlighted range + instruction, (b) a fixed shortcut menu (length/tone/reading-level; review/comment/fix-bugs), (c) a version stack with restore.

### #22 — Governed read-only share / replay link
- **Best-in-class:** OpenCode `/share` (opencode.ai/docs/share); Manus shareable replays.
- **Why it matters:** One-click review/collaboration. But multi-tenant + ZDR make naive sharing dangerous — must hard-enforce tenant scoping and refuse ZDR/temporary sessions.
- **Verevon coverage:** unclear (pin/filter exist; no evident public share).
- **Effort:** M.
- **First step:** Server-rendered, read-only, default-off, revocable share link for a chat or agent run, generated from the durable run-events stream, refusing ZDR/temporary sessions.

### #23 — Tool-response engineering (concise/detailed enum, resolve UUIDs→names, pagination) + tool-level evals
- **Best-in-class:** Claude Code / Anthropic (~1/3 tokens with concise responses; semantic IDs over UUIDs — anthropic.com/engineering/writing-tools-for-agents); consensus (evals-driven ACI).
- **Why it matters:** Verevon measures cost per turn but doesn't systematically shape tool outputs. "Every UI is an API" gives a huge, confusable tool surface — the highest-ROI place to tune.
- **Verevon coverage:** partial.
- **Effort:** M.
- **First step:** Add a `response_format` (concise|detailed) param to high-volume gateway tools, strip low-signal identifiers, resolve internal UUIDs to names before returning; add a tool-level eval suite in the existing eval CI that runs whenever a `src/shared/actions` contract changes.

### #24 — Human-takeover of a live browser session
- **Best-in-class:** Manus (seize browser/VS-Code mid-run to clear login/CAPTCHA, hand back — help.manus.im).
- **Why it matters:** Rescues long browser runs instead of failing them. Stronger than approve/deny alone.
- **Verevon coverage:** partial (panel is observe + approve, not take-the-wheel).
- **Effort:** L.
- **First step:** Where Verevon drives a browser (Quarry-v2 / browser grants), add an interactive takeover mode streaming the live session to the panel, routing user input back into the same session, pausing the loop while the human acts.

### #25 — Multi-provider auth-profile failover in the FallbackChain
- **Best-in-class:** OpenClaw (BYO-provider OAuth + credential rotation/failover).
- **Why it matters:** A provider/credential outage should fail over, not error the turn. Lower priority given Verevon's SaaS model, but the failover half is worth mirroring.
- **Verevon coverage:** partial (model ladder + Azure, no explicit rotation).
- **Effort:** S.
- **First step:** Give the inference-core FallbackChain explicit multi-provider auth-profile rotation.

### #26 — Advanced voice mode (speech-native, screen/video share)
- **Best-in-class:** ChatGPT Advanced Voice Mode (venturebeat.com; gptprompts.ai).
- **Why it matters:** Genuinely different modality, strong for field/ops. **Lowest priority** for an enterprise BFF product and heaviest lift.
- **Verevon coverage:** no.
- **Effort:** L.
- **First step:** If pursued, add a realtime STT-in/TTS-out transport with barge-in feeding the same `/v1/invoke/stream` loop; gate behind ZDR/EU-residency since audio/video is content-persisting.

---

## 2. THE 3–5 HIGHEST-LEVERAGE ITEMS TO BUILD FIRST

1. **#1 Server-authoritative sessions + stream resume (M).** It's Verevon's own #1 gap *and* the load-bearing foundation: checkpoint/rewind (#7), branch/edit-versions (#8), and share links (#22) all sit on top of it. Reuses the durable-run attach model Verevon already proved. Build this first or the four downstream items each reinvent persistence.

2. **#2 Declarative hook + permission-policy layer (M).** Directly fixes the recurring "HITL decorative on live writes" landmine that appears across Verevon's own audit memory. It's the single biggest enterprise-trust differentiator in the whole backlog — Codex/Claude Code/OpenCode all treat this as core, and Verevon's multi-tenant + ZDR posture makes it more valuable here than anywhere. Converts model cooperation into a hard boundary.

3. **#3 Skills-as-files registry (M).** The most-converged pattern in the research (7/9 reports). Unlocks compounding domain capability (Norway data commons, Visma, compliance) at near-zero standing context cost, and gives the Dreaming/skill-learning loop a real artifact to emit instead of an opaque score. Also the natural home for #6 slash-commands and #14 persona docs.

4. **#5 Inline citation chips + quality gate (S/M).** Defends Verevon's *core* strategic differentiator (grounded, cited answers) against the acknowledged bar (Perplexity) — cheaply, because the citation + confidence payload already exists. High visible-quality-per-effort. The abstain-and-requery gate prevents the one failure mode a cited-answer product cannot afford: confidently wrong.

5. **#4 In-chat subagent fan-out (L).** Verevon's explicit stated gap and the frontier capability that separates "chat" from "agent platform." Higher effort, but reuses durable RunAgent + parallel dispatch. Scope v1 to read-only research subagents to stay on the safe side of the Cognition/Anthropic multi-agent split.

**Sequencing note:** #1 → then #2 and #3 in parallel → #5 as a quick win alongside → #4 last. #6 (slash commands, S) is a cheap rider on #3 and worth slotting in early for visible user value.

---

## 3. WHAT VEREVON ALREADY DOES AS WELL AS / BETTER THAN THE FIELD — DO NOT REGRESS

Confirmed by the reports' own "what_verevon_does_better" sections across all 9 harnesses:

- **First-class grounded retrieval** — DP retrieval + GraphRAG + RRF hybrid + rerank + citations + **graduated confidence score** over governed multi-tenant data. *No* competing harness has a first-class citation/confidence/knowledge-graph-traversal layer; Perplexity is web-first with no graph arm and no exposed confidence gate. This is Verevon's moat.
- **Cost-aware model routing** — intent×budget → verevon-budget/balance/genius ladder + real BPE tokenizer + per-turn cost/token badge. Every harness only *preaches* token economics (Manus/consensus); Verevon operationalizes them. Perplexity/ChatGPT/Manus hide cost entirely.
- **Semantic response caching (CAG, user-scoped, ZDR-gated)** on top of Anthropic prompt caching. No competitor has a cross-turn semantic answer cache.
- **Enterprise multi-tenant, multi-plane architecture** — explicit authority boundaries behind a Rust BFF, resource_grants authz, EU/Norway residency, ZDR propagation through content-persisting boundaries. Every code harness (Claude Code, Codex, OpenCode, OpenClaw, Hermes) is single-user/local with no tenancy or residency model. This is the enterprise wedge.
- **Governed autonomy** — observable + approvable durable agent runs (Manus-style panel attached to a durable runId, streaming during tool rounds) with HITL gates. Manus/Hermes are flagged in their *own* docs as guardrails-lagging.
- **Learning loop from implicit dissatisfaction signals** — regenerate/edit-resubmit/correction/near-duplicate → Wilson-score skill-quality, plus Dreaming extraction with provenance-badged memory. No competitor closes this feedback loop; most stop at offline evals.
- **Parallel tool dispatch within a round + token-by-token SSE streaming during tool execution.** Hermes is explicitly one-function-at-a-time; OpenClaw is turn/message-oriented with no mid-turn token streaming.
- **Owning the loop in the gateway** (consensus rated this `yes`) — keep the loop in the Rust BFF; resist pushing it into a third-party agent SDK (12-factor).
- **Platform-scale ACI** — "every UI is an API" via typed action contracts across six planes is a broader, more governed agent-computer interface than the single-repo filesystem+shell surface every code harness exposes.
- **Native business-action surface** — Visma MCP, shipping, provider actions, multi-channel inbox, image/xlsx/docx/pdf. Code harnesses act on a repo; Verevon acts on real business systems.

**Regression risks to watch while building the backlog:** #19's cache discipline must not break the existing cost badge; #2's policy layer must not weaken the streaming-during-tool-rounds behavior; #4 subagents must preserve per-turn cost accounting; any share/proactive/voice feature (#17, #22, #26) must not bypass ZDR/residency propagation.

---

## 4. INCONCLUSIVE — NEEDS A HUMAN CALL

1. **Multi-agent architecture stance (sharpest disagreement in the research).** Cognition says single-thread write work, never parallel writers, always pass full trace (cognition.ai/blog/dont-build-multi-agents); Anthropic says fan-out works for read-heavy research at ~15× tokens (anthropic.com/engineering/multi-agent-research-system). The field's reconciliation — parallelize read/gather only — is a *recommendation*, not a Verevon decision. **Call needed:** does Verevon allow subagents to take *write/side-effecting* actions, or hard-restrict fan-out to read-only research? This gates the #4 design and its cost ceiling.

2. **Voice mode (#26) — build or drop.** The report itself calls it "heaviest lift and arguably lowest priority for an enterprise BFF product." Needs a product/GTM call on whether field/ops voice is a real Verevon buyer need before any engineering.

3. **Public share links (#22) — coverage genuinely unknown.** OpenCode report marked Verevon's share capability `unclear`. Someone must confirm whether any share surface exists today, and legal/compliance must rule on whether *any* cross-tenant share is acceptable under Verevon's residency/ZDR posture before building.

4. **Keeping errors in context (#Manus pattern) — needs an audit, not a build yet.** Manus keeps failed actions + stack traces in context so the model self-corrects; the report marked Verevon `unclear` here. **Call needed:** audit the MAX_TOOL_ROUNDS path + prompt-too-long recovery to confirm tool errors are preserved verbatim (not swallowed, silently retried, or compacted away first). If they're being trimmed, that's a latent quality bug, not a feature — but it needs a human to read the loop before deciding.

5. **Checkpoint scope for irreversible side effects (#7).** Claude Code explicitly does *not* rewind bash/side-effect mutations. Verevon must decide the exact boundary: which Application-Plane projections are snapshot-and-restore, and how non-rewindable provider writes (Visma/shipping) are marked in the UI so "rewind" never implies a rollback that didn't happen. This is a correctness/trust decision, not just engineering.

6. **Projects vs. org-KB boundary (#13).** Whether a first-class `project` container should get its *own isolated memory tier* (ChatGPT model) or simply scope existing org memory by `project_id` is an architecture decision with authz + Dreaming-loop implications — needs a design call before committing to L effort.

---

# Appendix — per-harness source notes

### Claude Code

**Does best:**
- File-based, version-controllable extensibility. Subagents, hooks, skills, and commands are all plain files in .claude/ with YAML frontmatter — they diff, review, and ship with the repo. WHY it works: the team's automation is code, so behavior is reproducible across machines/CI and auditable in PRs rather than living in a UI database.
- Progressive disclosure via Skills. Only name+description (~100 tokens) sits in context until a task matches; the body, reference files, and scripts load on demand, and script code never enters context (only stdout). WHY it works: you can install dozens of capabilities with near-zero standing context cost, so the model stays focused and cheap until a capability is actually needed.
- Isolated-context subagent delegation. A subagent runs in its own context window, is auto-selected by matching its `description`, and returns only a summary to the parent. WHY it works: verbose exploration (grep/read loops, research) is quarantined so it never pollutes the main thread's context budget, and tool/permission restrictions can be enforced per-agent.
- Deterministic lifecycle hooks. ~25 events let user code allow/deny/ask/modify tool calls (PreToolUse), rewrite tool output (PostToolUse), block completion (Stop), or inject context (SessionStart/UserPromptSubmit). WHY it works: policy that must ALWAYS happen (secret redaction, format-on-save, block rm -rf, completion checks) is enforced by the harness deterministically instead of relying on the model to remember.
- Checkpoint + rewind of code AND conversation. Every prompt snapshots edited files; /rewind restores code, conversation, or both, with summarize-from-here to reclaim context. WHY it works: it makes ambitious multi-file changes low-risk — you can explore, fail, and revert to a known-good state without git gymnastics, which encourages bolder autonomy.
- Tool-design discipline (from the 'Writing tools for agents' guidance). Namespaced tools, semantic identifiers over UUIDs, and a concise/detailed response_format enum for token control, all tuned against evals. WHY it works: high-signal, right-sized tool responses measurably raise task success and cut token cost versus dumping raw API payloads at the model.

**Patterns Verevon lacks / partial:**
- Hooks: deterministic, user-owned lifecycle interception (PreToolUse allow/deny/ask + input rewrite, PostToolUse output rewrite/redaction, Stop completion-gate, SessionStart context injection).
- Skills-as-files with progressive disclosure: name+description always loaded, body + bundled scripts loaded only when the task matches.
- Isolated-context subagent delegation via a Task-style tool, auto-routed by each agent's description, returning only a summary.
- Checkpoint + rewind of conversation AND workspace state, with 'summarize from here' to reclaim context.
- Slash / custom commands: user-authored reusable prompts with $ARGUMENTS substitution and per-invocation allowed-tools grants.
- Graduated permission modes (plan/read-only, acceptEdits, auto-with-classifier, dontAsk, bypass) layered over ordered deny→ask→allow rules.
- Tool-response engineering: concise/detailed response_format enum, semantic IDs over UUIDs, formats chosen by eval, tools optimized against a benchmark.
- CLAUDE.md-style always-loaded hierarchical project/org memory (managed → user → project → local), distinct from a learned memory store.

### OpenAI Codex CLI (github.com/openai/codex — the `codex-rs` Rust workspace)

**Does best:**
- OS-level, defense-in-depth sandboxing of model-generated commands (Seatbelt on macOS; bwrap + per-thread seccomp on Linux; Windows sandbox; Landlock fallback). WHY it works: Codex runs untrusted commands on the user's real machine, so it treats the OS sandbox — not the model's good behavior — as the safety boundary, and layers mechanisms so no single failure is fatal.
- A coherent graduated permission model: `approval_policy` × `sandbox_mode` with on-request escalation. WHY: one mental model scales from read-only analysis to workspace-write to full-access, and 'ask-to-escalate' avoids the all-or-nothing trap where users disable safety wholesale to get work done.
- Network-off-by-default with allowlisted egress via a proxy bridge (proxy-routed seccomp mode). WHY: most command execution never needs the network, so denying it by default kills the largest exfiltration/supply-chain vector, while the proxy path re-enables only vetted destinations and stays fail-closed even in full-access managed sessions.
- Declarative, testable exec policy (`execpolicy`: Starlark `prefix_rule` → allow/prompt/forbidden, with `justification` and `match`/`not_match` examples validated at load). WHY: command permissions become auditable data, not scattered code; the strictest match wins, and the embedded unit-tests stop a policy from silently drifting.
- Filesystem-based, version-controllable extensibility: `AGENTS.md` + `skills/` (SKILL.md) + `hooks` + `plugin`/`connectors` + `prompts`. WHY: every extension point is a discoverable file the user or org can commit and review; skills are progressively disclosed, and hooks (PreToolUse/SessionStart/AfterAgent, handler types Prompt/Command/Agent, FailedAbort halts the op) let orgs enforce policy the model cannot bypass.
- Open, cleanly-factored Rust core with a stable app-server protocol. WHY: one audited core (protocol/core/exec-server/sandboxing/app-server) is reused by TUI, headless exec, IDE, and cloud — so behavior and safety are identical across surfaces, and third parties can drive it without reimplementing the loop.
- Durable, resumable sessions (`rollout` persistence + `codex resume`). WHY: conversations and their tool history are written to disk as they happen, so a crash or reconnect resumes deterministically instead of losing the run.

**Patterns Verevon lacks / partial:**
- Graduated session permission policy (approval_policy × sandbox_mode) with on-request escalation, replacing per-tool HITL gates as the primary model.
- Network-off-by-default in the code sandbox with allowlisted egress via a proxy bridge (seccomp proxy-routed mode).
- Declarative, testable command-permission policy engine (execpolicy: allow/prompt/forbidden prefix rules with justifications and load-time match/not_match tests).
- Skills-as-files (SKILL.md + scripts) discovered from disk, with progressive disclosure and a skill-creator.
- Lifecycle hooks (PreToolUse / SessionStart / AfterAgent) with Command/Prompt/Agent handlers and a FailedAbort that halts the operation; managed-hooks-only enforceable by admins.
- Code mode: the model writes a program that calls tools through a runtime session instead of emitting many individual JSON tool calls.
- Durable, resumable sessions via on-disk rollout + `codex resume`, and conversation checkpoint/rewind.

### OpenCode (sst/opencode)

**Does best:**
- Headless agent engine + typed multi-client SDK: the agent runs as a local server and the UI is just a client. Stainless generates a type-safe SDK from the server's OpenAPI spec, so TUI, desktop, web, VS Code and any script drive the exact same API. WHY it works: it makes 'every UI is an API' literally true and lets sessions outlive any single client.
- Server-owned sessions with Event-Bus multi-client sync: message parts are persisted (Drizzle/SQLite) and broadcast over SSE; sessions survive terminal/SSH disconnect and any client can re-subscribe and replay. WHY: reconnection is trivial because the server, not the client, is the source of truth.
- Provider-agnostic routing via the Vercel AI SDK + models.dev: drop in an API key for 75+ providers (incl. Ollama and OpenAI-compatible self-host), pick a model per-agent, with per-provider system prompts and transformations. WHY: zero lock-in and near-free cost to add a model.
- LSP feedback loop: after each edit the language server's diagnostics are fed straight back into the model's context. WHY: grounds edits in compiler/type truth instead of the model guessing, catching errors within the same turn.
- Declarative, layered permission policy: ask/allow/deny per tool with wildcard argument patterns, evaluated in order and overridable per-agent and per-project via opencode.json. WHY: governance is config-as-code and auditable, not buried in imperative UI callbacks.
- Agents-as-files + subagents: primary/subagent roles defined as Markdown+frontmatter (own model, tools, permissions, prompt), with a TaskTool that runs a subagent in an isolated child session. WHY: composable specialists with isolated context windows, cheaply authored and version-controlled.
- Workspace+conversation checkpoint/rewind: git-style snapshots per response let /undo and /redo restore files AND messages together. WHY: fearless experimentation — you can rewind the whole workspace state, not just the chat text.

**Patterns Verevon lacks / partial:**
- Headless agent server + typed client SDK generated from an OpenAPI contract (Stainless-style), with server-owned sessions that any client drives.
- Central Event Bus + persisted message parts → reconnect-safe multi-client streaming (client re-subscribes and replays from storage).
- Declarative layered permission policy: ask/allow/deny per tool + wildcard argument patterns, resolved global→project→per-agent.
- Model-invokable subagents via a Task tool: each spawns an isolated child session with its own model/tools/prompt/budget and returns a summary.
- Agents + Skills + custom commands as version-controlled files (Markdown + frontmatter), progressively disclosed and exposed to the model as tools; plus lifecycle hooks and a slash-command palette.
- Workspace + conversation checkpoint/rewind: snapshot state per response so undo/redo restores side effects and messages together; plus ChatGPT-style edit-version navigation.
- LSP-style external-verifier feedback loop: after a mutating tool runs, automatically re-observe with an authoritative checker and feed the result back into context within the same turn.

### ChatGPT (OpenAI product)

**Does best:**
- Canvas — a side-by-side editable document/code pane the model and user co-edit. WHY it works: it separates the durable artifact from the ephemeral chat, supports targeted inline edits (highlight a section, ask for a change to just that span), writing shortcuts (adjust length, reading level, polish, add final touches), code shortcuts (review/add comments/add logs/fix bugs/port language), and a version back-button — turning 'chat that regenerates the whole thing' into 'surgical editing of a persistent object.'
- Layered, user-inspectable memory. WHY it works: three distinct tiers — user-authored Custom Instructions (explicit steering), model-written Saved Memories (auto-captured, but viewable and deletable in Settings), and Reference Chat History (implicit retrieval over all past chats) — give both automatic personalization AND user control/transparency. The 'you can see and edit exactly what it remembers' property is what builds trust.
- Projects as scoped containers. WHY it works: a Project bundles related chats + uploaded files + custom instructions + its own isolated memory, and is shareable with per-member chat/edit access. It solves 'context bleed' — the project only references what's explicitly in it — and lets long-running work resume without re-explaining, which flat chat history cannot do.
- Non-linear conversation: edit-message version siblings + Branch conversation. WHY it works: users can fork a thread at any message into an independent branch (original preserved) or edit a past message to explore an alternate path, so exploration doesn't destroy prior state. This turns a linear transcript into a tree the user can navigate.
- Custom GPTs — packaged, shareable assistants built conversationally. WHY it works: a non-technical user configures instructions + knowledge + actions in a few chat prompts and gets a reusable, distributable assistant. It converts one-off prompting into durable, named tools others can invoke.
- Summarized reasoning display for o-series/thinking models. WHY it works: a collapsible, post-processed thinking trace lets users see the model's plan and catch when it's going wrong, building trust — even though experts note it's a summary, not the raw chain-of-thought, which some find disappointing.
- Advanced Voice Mode with screen/video share. WHY it works: speech-native, low-latency, interruptible conversation plus live camera/screen input makes the assistant usable hands-free and context-aware of the user's physical/on-screen environment.

**Patterns Verevon lacks / partial:**
- Branch conversation — fork a new independent thread from any past message while the original stays intact
- Edit-message version siblings with a 1/2 switcher
- Projects — a scoped container bundling chats + files + custom instructions + isolated per-project memory, shareable with chat/edit roles
- User-authored Custom Instructions distinct from auto-captured memory
- Custom GPTs — conversationally-built, packaged, shareable assistants (instructions + knowledge + actions)
- Canvas-grade targeted inline edits + writing/coding shortcut menus + version restore
- Collapsible summarized reasoning/thinking trace for reasoning-tier models
- Advanced Voice Mode — speech-native, interruptible, with live screen/camera share

### Perplexity (Pro Search answer engine + Spaces)

**Does best:**
- Inline, publisher-attributed citations as a first-class UX, not a footnote dump. Claims end with rounded domain chips ('northjersey +3') that group multiple sources with a +N count; clicking a chip opens a quick-verify popover with 1/N pagination through the underlying sources (aiuxplayground teardown). This makes every sentence auditable inline while scanning — the reason Perplexity is the reference standard for trust-visible answers.
- Multiple, redundant paths to the source set at different altitudes. A favicon stack + source count on the answer bar gives breadth at a glance; a Links/Sources tab sits as a peer beside Answer and Images for a full source browser; a sources sidebar opens without a tab switch; and 'check sources' appears on text selection for a local claim audit. Users can verify at whatever grain they want without re-querying.
- Plan/execute Pro Search with a visible, plain-language step trace. Perplexity found users tolerate longer waits when intermediate progress is shown, so it renders the plan executing step-by-step with expandable steps — trust signal without log noise, and the decomposition genuinely improves multi-step answers (LangChain).
- Focus modes as a one-click hard source filter. Because the filter is applied at retrieval (not prompt), the same query in Academic vs Social vs Web returns a materially different, domain-appropriate answer. This is cheap, legible source steering — the user controls the evidence pool before the model ever sees it.
- Retrieval-quality-first design with an abstain/re-query fail-safe. The reported ~0.7 rerank threshold that discards everything and re-queries rather than cite weak sources encodes 'don't answer from bad evidence' as a system behavior, not a model hope. Analysts summarize it as: retrieval quality, not LLM capability, is the bottleneck.
- Spaces as persistent, shareable, model-configurable knowledge hubs. A Space bundles uploaded files + web + custom instructions + a default model, and threads run inside it collaboratively — turning one-off chats into a reusable, scoped research context (airespo; Perplexity blog).

**Patterns Verevon lacks / partial:**
- Inline publisher-attributed citation chips with +N grouping and a hover/click quick-verify popover (1/N through the grouped sources).
- A dedicated Sources/Links surface at multiple altitudes: favicon stack + count on the answer bar, a peer Sources tab, a slide-in sources sidebar, and 'check sources' on text selection.
- Focus modes: a one-click selector that hard-filters the retrieval source pool (web / internal-KB-only / academic / social / video) BEFORE the model sees evidence.
- Lightweight, expandable plain-language step trace on ordinary answers (collapsed 'research steps', not a full agent-run console).
- Retrieval quality gate: an explicit confidence threshold that abstains and re-queries rather than answering from weak evidence.
- Spaces: persistent, shareable containers binding a file set + web scope + custom instructions + a default model, with threads living inside.

### Manus.im — autonomous general AI agent (closed-source; researched via Manus's own "Context Engineering" blog, product/help pages, and independent technical analysis)

**Does best:**
- KV-cache-first context design: a stable append-only prefix, no mid-loop tool-definition churn, and deterministic serialization. Works because agent loops are ~100:1 input:output and cached tokens are ~10x cheaper — so cache hit rate dominates both latency and cost.
- Filesystem-as-memory with restorable compression: the agent reads/writes real files as unbounded externalized memory and shrinks context by dropping content while keeping a pointer (URL/path). Works because you can never predict which observation matters 10 steps later, so lossy in-context compression is risky — a restorable pointer is not.
- todo.md recitation as attention control: rewriting the plan into the end of context every step. Works because over a ~50-call loop the model drifts; reciting the goal into recent tokens biases attention back onto the objective without architectural changes.
- Keeping errors/failed actions in context instead of scrubbing them. Works because the failure trace is the evidence the model uses to shift its prior away from the bad action — error recovery is a core signal of real agentic behavior.
- 'Mask, don't remove' for a large/MCP-inflated tool space: gate tools by logit masking on a state machine rather than mutating the tool list. Works because it preserves the KV-cache and avoids schema violations/hallucinated calls that arise when context references now-undefined tools.
- Live transparent workspace + human takeover: a real-time browser/VS-Code/terminal panel the user can watch AND seize control of mid-task. Works because long autonomous runs hit walls (logins, CAPTCHAs, judgment calls); letting the human grab the wheel rescues the run instead of failing it.

**Patterns Verevon lacks / partial:**
- Filesystem-as-memory: give the agent a durable per-task workspace it reads/writes across steps, and compress context by dropping bulky observations while keeping a restorable pointer (URL kept when page body is dropped; sandbox path kept when doc body is dropped).
- Plan recitation into the context tail: maintain a todo list as a real file and re-inject the updated, checked-off plan at the END of context every turn.
- KV-cache-stable loop discipline: stable append-only prefix, never mutate tool definitions or earlier turns mid-run, deterministic JSON serialization (stable key order, no volatile timestamps in the prefix).
- Mask, don't remove: gate a large/MCP-inflated tool space by a state machine + logit/constrained-decoding masking rather than dynamically loading/unloading tools.
- Wide Research: fan out dozens of parallel sub-agents, one per item, for large batch work, then aggregate.
- Anti-few-shot variation: inject small structured variation (serialization templates, phrasing, ordering) on repetitive action/observation sequences.
- File-based Agent Skills: reusable procedures stored as SKILL.md (+ scripts) that the runtime discovers and executes on demand.
- Async cloud execution + shareable replay: the task keeps running after the user disconnects, notifies on completion, and produces a replayable/shareable link of the whole run.
- Human takeover of the live workspace: user can seize the agent's browser/VS-Code/terminal mid-run to clear a blocker (login, CAPTCHA, judgment call), then hand control back.

### Hermes Agent (Nous Research) — primary referent, plus the Hermes-2-Pro/Hermes-3 function-calling model format. Pi (Inflection) covered as a secondary/ambiguous referent (a consumer EQ chat model, not a tool-use harness).

**Does best:**
- Skills as self-authored, self-patching Markdown files (Hermes Agent): after a complex task (5+ tool calls) it writes a reusable SKILL.md and later PATCHES it when it proves outdated/wrong — 'backpropagation for prompts, not weights.' WHY it works: the improvement unit is an inspectable, portable file loaded contextually (RAG over skills) rather than crammed into every prompt, so competence compounds without prompt bloat and survives model swaps.
- Cross-session persistent memory + dialectic user model (Hermes Agent): SQLite + FTS5 full-text session search with LLM summarization ('recall a conversation from three months ago') plus a Honcho user model of projects/preferences/working-style. WHY: it kills the 're-explain yourself every session' tax; memory is server/local-owned and model-agnostic, so it persists across provider switches.
- A dead-simple, verifiable tool protocol (Hermes models): ChatML with <tools>/<tool_call>/<tool_response> XML wrapping strict pydantic-validated JSON, a running-summary accumulated every iteration, and code_interpreter as the fallback when no declared tool fits. WHY: XML delimiters are trivially parseable mid-stream, pydantic rejects malformed calls, and the running summary keeps a long recursive loop coherent without re-reading full history.
- Unattended, multi-surface operation (Hermes Agent): one running process bridges CLI + Telegram/Slack/WhatsApp/Signal with continuous cross-surface context, plus a natural-language cron scheduler that delivers recurring results to any connected platform. WHY: the agent becomes a long-lived teammate you reach from anywhere and that works while you're away, not a tab you must babysit.
- Concurrency without context bloat (Hermes Agent): subagent spawning for parallel workstreams, and Python RPC scripts that call tools directly to collapse multi-step pipelines into 'zero-context-cost turns.' WHY: fan-out parallelism plus scripting many tool calls in one code turn cuts both latency and token cost versus a serial tool-by-tool loop.
- Steerable, file-defined persona + (Pi) genuine EQ: Hermes ships a SOUL.md persona file that migrates with you; Pi shows how far tone-adaptation, emotional-arc tracking across a session, and proactive goal check-ins can carry a product. WHY: a user-editable persona file makes voice/behavior a first-class, versionable setting instead of a buried system prompt.

**Patterns Verevon lacks / partial:**
- Skills-as-files: auto-author a Markdown skill after a successful multi-tool task, self-patch it on later failure, load contextually
- Slash-command / custom-command system with autocomplete in the chat composer
- Subagent spawning for parallel workstreams, exposed in chat
- Tools-callable-from-sandboxed-code (Python RPC) to collapse multi-tool pipelines into one 'zero-context-cost' turn
- Natural-language cron scheduler that runs unattended and delivers to a messaging surface
- One agent, many surfaces: continuous cross-surface conversation context (CLI/Telegram/Slack) from a single process
- FTS5 full-text search over ALL past conversations with LLM summarization for recall
- User-editable persona/steerability file (Hermes SOUL.md) + Pi-style tone adaptation
- Explicit running-summary accumulation + GOAP (Goal/Actions/Observation/Reflection) scratchpad across the tool loop

### OpenClaw (personal AI assistant / agent runtime by the OpenClaw Foundation) — with a note on Nous Research's Hermes Agent as its sibling/competitor

**Does best:**
- Messaging-channel-first ubiquity: the chat surface is a real 25+ channel inbox (WhatsApp/Telegram/Slack/Signal/iMessage/SMS/WebChat), so the agent meets the user wherever they already are and can both receive and initiate on the same channel. Why it works: the channel adapter IS the UI, so there is no separate app to open and proactive messages land in a place the user already watches.
- Portable Skills format (SKILL.md files with frontmatter + progressive disclosure, installable from a community catalog). Why it works: capabilities are added as data/files in the workspace, not code changes to the core, so users and the community extend the agent without touching the runtime — and the model only pays context cost for a skill when it is actually engaged.
- Proactive autonomy via a heartbeat/cron scheduler and first-class cron tool. Why it works: the agent can wake on an interval and act unprompted (reminders, checks, follow-ups), turning it from a request/response chatbot into a standing assistant.
- One local-first Gateway as the single control plane over sessions, channels, tools and events, paired with a declarative per-tool sandbox permission model (host for the owner's `main` session; Docker/SSH/OpenShell sandbox with allow/deny lists for everyone else). Why it works: one authority means trust decisions (who can run browser/cron/channel actions) are configured in one place and scale from single-user-full-trust to multi-party-least-privilege.
- Coding-agent delegation to Claude Code/Codex/OpenCode as monitored background workers in isolated git worktrees, with trusted/untrusted ref classification and mandatory completion notifications. Why it works: long code tasks run detached and safely (worktree isolation prevents cross-contamination; untrusted PRs never get a permission-bypassed worker), and the user is pinged on a channel when done instead of babysitting a stream.
- Multi-agent routing: inbound channels/accounts/peers map to distinct agents each with their own workspace and session. Why it works: it gives clean isolation and per-context memory/persona without a heavyweight orchestration layer — each 'agent' is just a workspace + session binding.

**Patterns Verevon lacks / partial:**
- Skills-as-files: an authorable, installable SKILL.md format (frontmatter + body, progressive disclosure) that the model loads on demand, with a community/registry install path.
- Self-improving loop that emits a retrievable skill artifact (Hermes' 'Reflective Phase': after a task, extract the pattern, write a skill file, query the skill library next time).
- Proactive heartbeat/cron scheduler exposed IN chat (agent wakes itself on an interval and initiates a message on the user's channel).
- Background-worker delegation with a captured notification route + process monitoring (spawn a long task detached, watch via process/session tools, ping the user on completion/failure via message send).
- Subagent / parallel background workers spawned from the main chat agent, each isolated (own workspace/session), with the parent monitoring and aggregating.
- Git-worktree isolation + trusted/untrusted ref classification + diff/apply review for code-modifying agent tasks.
- Declarative per-tool sandbox allow/deny keyed on session trust (owner/main session runs on host; others run sandboxed with an explicit allow/deny tool list).
- Editable persona/context workspace files (AGENTS.md + SOUL.md injected into the prompt) that the user directly controls.
- Bring-your-own-provider with OAuth subscription sign-in + auth-profile rotation/failover across providers.

### EXPERT CONSENSUS — the 2025-2026 agent-harness playbook (Anthropic "Building Effective Agents" + "Writing Tools for Agents" + "Effective Context Engineering"; HumanLayer's 12-Factor Agents; Cognition/Devin "Don't Build Multi-Agents"; Manus "Context Engineering"; Simon Willison "Designing Agentic Loops"; swyx/Latent Space on Claude Code)

**Does best:**
- Owning the loop end-to-end (12-factor): because the prompt, context assembly, and control flow are explicit code rather than framework magic, the agent is debuggable, interruptible, resumable, and reliable in production — the single biggest predictor of a harness that survives contact with real users.
- Treating context as the scarce resource (Anthropic context-engineering + Manus): active compaction, right-altitude prompts, smallest-high-signal-token curation, and KV-cache-friendly append-only prefixes are what keep long agentic sessions coherent and cheap instead of degrading as the window fills.
- ACI / tool design as the highest-leverage investment (Anthropic writing-tools + BEA): high-leverage, well-named, token-efficient tools with human-readable returns measurably raise task completion — 'small improvements in descriptions yield large gains,' and tools should be built and tuned against evals, not vibes.
- Closing the loop so the agent can verify itself (Willison): giving the model a sandbox to run code, read output, and check its own work turns a one-shot generator into a self-correcting agent — the difference between 'wrote plausible code' and 'produced working code.'
- Human-in-the-loop as a native tool + guardrails on irreversible actions (12-factor factor 7, Anthropic guardrails): the durable pattern for trust is the agent explicitly requesting approval as a tool call, not the UI bolting on a confirm dialog.
- Knowing when NOT to add complexity (Anthropic simplicity principle + Cognition single-thread): use a workflow before an agent, a single thread before multi-agent; only pay the reliability/token cost of autonomy and parallelism where the task provably needs it.

**Patterns Verevon lacks / partial:**
- Context as a finite budget: right-altitude system prompt + smallest high-signal token set (Anthropic context-engineering)
- Structured note-taking / external memory + recitation (Manus todo.md, Anthropic memory files, filesystem-as-context)
- ACI / tool design: high-leverage, distinctly named/namespaced, token-efficient, human-readable returns (Anthropic writing-tools)
- Evals-driven tool + agent development: Prototype -> Evaluate -> Collaborate on real tasks (Anthropic writing-tools)
- Give the agent a sandbox to verify its own work — close the loop (Willison 'designing agentic loops')
- Multi-agent orchestrator ONLY for read-heavy parallelizable work, with fan-out + re-synthesis, budgeting ~15x tokens (Anthropic multi-agent research)
- KV/prompt-cache-friendly context: stable append-only prefixes, mask (don't delete) tools (Manus)
- Checkpoint / rewind of conversation + workspace state (Claude Code via Latent Space; Manus recoverable state)
- Slash-commands / custom commands / skills-as-files / hooks (Claude Code harness patterns, Latent Space)
