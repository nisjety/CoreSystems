# Verevon Chat — Parity & Edge-Case Audit (2026-09-15)

> **Execution update — 2026-09-19:** [Q01–Q04](../product-recordings/evidence/PRODUCT_RECORDING_Q01_Q04_2026-09-19.md) records durable attachment/artifact checks; [Q05–Q07](../product-recordings/evidence/PRODUCT_RECORDING_Q05_Q07_2026-09-19.md) records verified memory edit/forget and source isolation. [Q08–Q10](../product-recordings/evidence/PRODUCT_RECORDING_Q08_Q10_2026-09-19.md) contains the latest interrupted-stream, stop/continue/regenerate, rendering and recording-gate results. Original findings and their earlier remediation notes below remain historical evidence; they do not establish the latest release gate.

> **Scope.** Test, audit and document — no code was changed for this document.
> Builds on the 2026-08-01 ten-harness study
> (`apps/Model Plane/docs/VEREVON_CHAT_PARITY_BACKLOG.md`, items #1–#26) and the
> chat design doc's 2026-09-06 completion check (`VEREVON_CHAT_DESIGN.md` §9).
> Where this audit re-tests something those documents claim, the live result is
> recorded here and wins.
>
> **Method.** (1) Web research on what users and experts praise in 13 systems
> the user named — Claude, ChatGPT, Manus, Perplexity, DeepSeek, OpenCode, Pi,
> Cursor, Codex, Claude Code, Copilot, Hermes, T3 Code. (2) A prioritised
> feature matrix from that research. (3) Live tests of each feature in the
> running Verevon stack (`C:\dev\CoresSystem`, local Docker, org AQUATIQ AS),
> with edge cases probed alongside. (4) Every failure traced to a source line or
> server log before it is recorded as a finding. Nothing here is inferred from
> a doc claim alone.
>
> **Test rules kept.** Read-only against real systems; nothing sent, published
> or scheduled; no fabricated data; no mutation through Visma MCP.

## 0.0 Remediation status — updated 2026-09-16

> The audit below is the record of what was **measured on 2026-09-15**, and is
> left intact. This section records what happened afterwards. Every ✅ was
> verified by a build, a test and — where the finding was user-visible — a live
> run against the stack, not by an agent's self-report.

**15 of 18 closed** as of 2026-09-16; **17 of 18 as of 2026-09-17** — see §0.1, which
also re-verifies the §3/§4 findings this section does not cover. `model-gateway` went from
1059 to 1181 passing lib tests.

| # | Status | Evidence |
|---|---|---|
| F-01 | ✅ fixed | `stream_ended_with_no_content`; live: retry banner instead of a fabricated "ingen dekning" |
| F-02 | ✅ fixed | ordered-list `start`, autolinking, task lists, CommonMark fence lengths, nested quotes |
| F-03 | ✅ fixed — **the 2026-09-16 "reopening" was a false alarm, mine** | Enter was briefly recorded as reopened after it failed to submit during live testing. It was the test harness, proved twice over: (1) browser-automation clicks were landing ~87 CSS px high, because the screenshot frame includes browser chrome and its origin is not the page origin — instrumenting `mousedown` showed a click aimed at the textarea (y=347) hitting the `H1` at y=260, so focus was never in the composer; (2) the automation key name `Return` dispatches a keydown with `key: ""`, which `event.key === "Enter"` correctly ignores. With focus corrected and `Enter` sent, the composer submits instantly (`/` → `/chat`, field cleared). The four `DashboardComposer.enter-to-send.test.tsx` cases were passing throughout and were right. **Lesson: trust the passing tests and suspect the harness.** |
| F-04 | ✅ fixed | banner now conditional on real citations |
| F-05 | ✅ fixed | "Brukte" → "Hentet" |
| F-06 | ✅ fixed | workspace/first-person questions no longer force a public web search |
| F-07 | ◐ partial | The 60–95 s waits are the subscription broker itself (infra, not chat code). The *"no progress detail"* half is fixed: elapsed-time indicator, continuous across Solid's list reconciliation. |
| F-08/09 | ✅ resolved 2026-09-16 | Both were decided, not deferred: `VEREVON_CHAT_DESIGN.md` §3.5 reversed the "no model picker" line (the picker is now design-sanctioned, safe because of F-01/F-10/F-18's protections), and §3.5.1 replaced the two dead shortcuts with a real parameterised command system (`/image en rød katt` consumes the rest of the line; `argumentHint` per command; Norwegian and English aliases). Verified in code 2026-09-17. |
| F-10 | ✅ fixed → superseded | Toggles were disabled for subscription models; F-18 then made subscription turns genuinely tool-capable, so the restriction is now narrower by design. |
| F-11 | ✅ fixed | `dispatch_audited_tool` logs every tool call; used repeatedly in this remediation to prove behaviour |
| F-12 | ✅ fixed | duplicate stopped-turn gone (Solid store-write ordering). The "Resultat tab on stop→regenerate" half was **not** re-verified live. |
| F-13 | ✅ fixed, then exceeded | Citation floor added; then snippet-only grounding replaced by real page reads (see the web-source audit) |
| F-14 | ✅ fixed | capability bearer forwarded; exposed and fixed a second `null`-vs-`[]` bug in capability-core |
| F-15 | ✅ fixed | `GET …/proof-bundle` → 200 with real run data |
| F-16 | ✅ fixed | budget by *successful* reads (8 → 16, ceiling 24), read/unread split in Kilder, no silent caps. **2026-09-16:** the Kilder panel also stopped *interrupting* for unread leads — it could pull focus to show a source that was found but never fetched. Offering the tab still uses the full count; claiming focus uses only what was read |
| F-17 | ✅ fixed | provenance-prefixed JSON parsed; generated files reach the user |
| F-18 | ✅ fixed | subscription tool-parity: decision round routed through Balance, answer handed off only when a tool actually ran |

**A note on method, earned the hard way.** Three items on the follow-up list
looked like product defects and turned out to be broken *tests or tooling*: a
migration security assertion that embedded a bare `
` against a CRLF file, a
sandbox test comparing a Windows path to a POSIX one, and F-03 above. In each
case the passing evidence was already there and I trusted a single flaky
observation over it. The counterweight is cheap and worth stating: when a
symptom contradicts a suite that covers exactly that case, suspect the
instrument first. The same list also contained a real defect that had been
dismissed as a flaky test for the opposite reason — see R-01 in the web-source
audit, where a human-in-the-loop approval could hang because four gRPC channels
had no connection bound.

**Known-open, deliberately:** the post-answer verification search, deep research
and `/v1/answer` stay on free search providers — fixed closed rather than
widening billable egress without a decision.

## 0.1 Re-verification of the §3/§4 findings — 2026-09-17

The table above tracks the 18 numbered findings. Several §3 observations and §4
gap rows were never re-checked after the fix passes. Each row below was verified
against the current code (`C:\dev\CoresSystem`), not against a doc claim. No
code was changed in this pass.

| Finding | Was | Now | Evidence |
|---|---|---|---|
| §3.3 stopped answer shows no "Stoppet" marker | ⚠️ | **✅ closed** | `chat-nodes/derive.ts:52` carries `stopped: turn.status === 'stopped'`; `ChatMessages.tsx:294-296` renders `<Show when={content().stopped}>` → "Stoppet". A `ChatMessages`-only grep for the string literal misses this — the renderer reads a node property — which is why it was nearly re-recorded as open. |
| §3.3 no "continue generating" after stop | ❌ | **❌ still open** | Only "Fortsett i ny chat" exists, and that is branch (below), not continuation. ChatGPT/Claude both offer continue-from-here. |
| §4 branch conversation | ❌ | **✅ closed** | `ChatPage.tsx:706` `onBranch={() => branchAt(row.turn.id)}`; `use-chat-controller.ts:165` `safeBranchBoundary` keeps a branch from inheriting an effectful answer as context (per the implementation plan's effect-boundary rule). Backlog #8 is done. |
| §3.6 long paste → "pasted text" attachment | ❌ | **❌ still open** | `DashboardComposer.tsx:664` handles `clipboardData?.files` only; pasted text stays inline. |
| P12 math / Mermaid | ❌ | **❌ still open** | No `katex`/`mermaid`/`remark-math` dependency in `package.json`. Still a dependency decision; every system in §1 renders math. |
| F-12 Resultat tab on stop→regenerate | ⚠️ | **⚠️ still not re-verified live** | Needs a targeted browser repro; no pass has produced one. |
| F-05 memory recalled on every turn regardless of relevance | ⚠️ | **◐ root cause fixed** | The label was already corrected ("Hentet"). The *ranking* behind it — a `LIKE '%<whole message>%'` clause that degraded recall to recency — was replaced 2026-09-17 by Reciprocal Rank Fusion over pgstore exact matches and the live semantic backend (`session-core` `memory_grpc.rs`, deployed 08:02 UTC). Presence-gating still relies on the existing trivial-prompt skip; no live turn has exercised the new ranking yet. |
| §3.8 correct / forget a memory from chat | ❌ | **❌ still open** | The expanded list still offers only "Skjul". Claude and ChatGPT fix or delete a wrong memory where you see it. |
| §4 inline citation chips (backlog #5) | ❌ | **❌ blocked on a producer** | Client is fully plumbed (`claimId`/`sourceGroupId`/`start`/`end` preserved on all three transports); no producer emits them on a citation frame. Deliberately unbuilt, per the design doc's wired-but-dead rule. |
| §4 schedule from chat (#17), subagents (#4), selection-scoped artifact edit (#21) | ❌ | **❌ still open** | No affordance exists for any of the three. |
| F-07 slow subscription path | ◐ | **◐ unchanged** | The 60–95 s is the subscription broker (infra); the progress indicator half is done. |

**Net position against the §1 bar-setters.** The judgement-quality differentiators
this audit told the fix pass to protect are all intact and several were
strengthened (real page reads behind citations, a 150-char citable floor, honest
coverage notes). Of the parity gaps that remain, the ones a user hits most often
are the cheapest: **continue-after-stop**, **math rendering**, and **per-memory
forget** are each hours-to-a-day and each is table stakes in ChatGPT, Claude and
Perplexity. Inline claim chips are the highest-value gap but are correctly held
behind a Model Plane producer contract. Subagents, scheduling and A2A are the
long-horizon items and belong to the implementation plan's phases 8–9.

**Next, in order:** (1) reproduce F-12 live and fix it — it has been ⚠️ for two
days without a repro; (2) continue-after-stop; (3) math (`katex`) as a dependency
decision; (4) per-memory forget in the chat surface, which is also the natural
first UI for the "mental model" item in `LEARNING_EDGE_FEATURES_2026-09-17.md`.

## 0. Verdict in one screen

**18 findings, every ❌ traced to a source line.** Verevon's *judgement* is
already at or above the market bar — it refuses to guess, catches a false
premise in a research question, plans before it acts, and shows what it
remembered and why. Its *plumbing* is where parity breaks: a failed provider
stream is shown as "the documents don't cover this" (F-01); a real xlsx the
sandbox produced never reaches the user (F-17); the org's privacy policy is
unreachable from chat so every answer is redacted unconditionally (F-14);
citations get attached to 63-character stub pages (F-13); and the renderer
drops the number out of "391." (F-02) and cannot show math, diagrams or task
lists. Three of the five worst are one-line-to-one-day fixes once the cause is
known, and the causes are in §3.

| # | Finding | Sev | Where |
|---|---|---|---|
| F-01 | Provider stream failure → silent empty answer, "ingen dekning" | ❌ | §3.1 |
| F-17 | Code-interpreter file produced but undeliverable; `sandbox:/` link | ❌ | §3.13 |
| F-14 | Capability bearer never sent from chat → PII redaction always on, policies ungovernable | ❌ | §3.9 |
| F-13 | Confident "grounded" answer on 63/84-char sources | ❌ | §3.7 |
| F-02 | Leading `N.` parsed as list, number lost | ❌ | §3.1 |
| F-10 | Subscription model silently strips search/plan/research/image/skills | ❌ | §3.1 |
| F-15 | Run receipt/proof bundle → 500; RPC unimplemented upstream | ❌ | §3.10 |
| F-16 | Deep research read 3 of 24 sources; EC + Lovdata unreadable | ❌ | §3.11 |
| F-11 | No server-side trace of tool calls or gRPC failures | ❌ | §3.4 |
| P12 | No math, Mermaid, task lists; nested fences split; bare URLs not links | ❌ | §3.2, §3.12 |
| F-12 | Artifact created on stop→regenerate got no "Resultat" tab | ⚠️ | §3.5 |
| F-18 | Model denied having `get_weather` instead of calling it | ⚠️ | §3.14 |
| F-03 | Enter did not send (3/3 automated; needs manual confirm) | ⚠️ | §3.1 |
| F-04 | "Fant ingen dekning" banner on non-KB questions | ⚠️ | §3.1 |
| F-05 | Memory recall unconditional; "Brukte" overstates | ⚠️ | §3.1 |
| F-06 | Web search on an inbox question | ⚠️ | §3.1 |
| F-07 | 90 s "Tenker" for 17×23 on the subscription model, no progress detail | ⚠️ | §3.1 |
| F-08/09 | Model picker vs design; slash commands are two shortcuts | ⚠️ | §3.1 |

---

## 1. What the market rewards — research summary

One line per system: the feature its users and reviewers most credit it for,
and the promoted headline. These are the bars Verevon is measured against below.

| System | What users praise most | Promoted headline (2026) |
|---|---|---|
| **Claude.ai** | Writing quality; Projects as persistent workspaces; Artifacts side panel; transparent, editable Memory (free tier since Mar 2026) | Projects + Artifacts + Memory; Cowork desktop agent for Office-heavy work |
| **ChatGPT** | "Most complete": Deep Research reports, Agent mode, Projects, Memory, custom instructions; explains code rather than dumping it | Deep Research (up to 30 min, 50–200 sources); Scheduled Tasks (Pulse folded in Jul 2026); conversation branching |
| **Manus** | Runs longest without losing the thread; keeps working after you close the tab; deep web research with citations; structured deliverables | Autonomous sandboxed cloud agent. Known failure modes: hallucinated clicks, timeouts, anti-bot blocks |
| **Perplexity** | Citations you can verify — the #1 reason subscribers stay; fastest deep research (2–4 min); clean minimal UI; model choice | Source-first answers; Comet browser; Spaces |
| **DeepSeek** | Visible thinking trace by default — "audit the reasoning, not just the answer"; free; strong at logic-heavy structured tasks | DeepThink reasoning mode; 10–30× cheaper API |
| **OpenCode** | Fast clean TUI, model-agnostic (no lock-in), server-owned sessions; Build/Plan agents + subagents | 165k★, most-used open harness |
| **Pi** | Smallest useful harness; tree-structured sessions; compaction; skills; highest pass-rate/cost in Databricks' bench | Deliberately *omits* MCP, subagents, plan mode, permission popups — extensibility over features |
| **Cursor** | Agent mode reads/writes whole project; Background Agents on triggers in cloud sandboxes; best tab completion; Rules/Memories persist project context | Agent-centric IDE; parallel background agents |
| **Codex** | Parallel cloud tasks each in its own sandbox, finishing as a PR; local CLI/IDE/app | Multi-agent execution + human review; credit anxiety on Plus is the common complaint |
| **Claude Code** | Hooks (deterministic policy, not prompt hope), subagents with isolated context, Plan mode, Skills (progressive disclosure), CLAUDE.md hierarchy | Orchestration layer for agent teams. Complaints: revert loops, compaction losing work, permission prompts |
| **GitHub Copilot** | Best value ($10): agent mode with semantic repo index, self-fixes test failures; unmatched GitHub integration | Ask / Edit / Agent modes; coding agent on issues |
| **Hermes (Nous)** | Persistent memory + natural-language cron that survives sessions; self-authored skills; runs in Telegram/Slack/Discord/WhatsApp; free desktop GUI | "The harness already built in"; scheduled agents keep memory between runs (v0.21, Aug 2026) |
| **T3 Code / T3 Chat** | T3 Code: one GUI for *parallel* agents (Claude Code, Codex, OpenCode, Cursor) with inline diff review and one-button PR; T3 Chat: fastest UI, instant model switching | Agent control plane, steer from phone/browser |

**Cross-system consensus (what experts say a great harness needs, 2026):** five
layers — tool orchestration, verification loops, context/memory, guardrails,
observability. "Anytime the agent makes a mistake, engineer it away so it never
recurs" — deterministic enforcement over prompt compliance. Failure modes that
dominate production: hallucinated tool calls, broken multi-step plans, prompt
injection via retrieved content, runaway loops, silent degradation when a
provider is swapped; per-step error rates compound (5 %/step ≈ 23 % over 5
steps). Streaming-markdown renderers must stay stable through unclosed fences,
partial tables, half-written math and open HTML.

Sources: [Claude 2026 guide](https://www.buildfastwithai.com/blogs/claude-ai-complete-guide-2026) · [G2 Claude review](https://learn.g2.com/claude-ai-review) · [Claude features](https://suprmind.ai/hub/claude/features/) · [ChatGPT review (Hack'celeration)](https://hackceleration.com/labs/review/chatgpt) · [ChatGPT review (AI Insider)](https://theaiinsider.net/ai-tools/chatgpt-review) · [ChatGPT Scheduled Tasks](https://www.usecarly.com/blog/chatgpt-scheduled-tasks/) · [ChatGPT branching (OpenAI)](https://x.com/OpenAI/status/1963697012014215181) · [Manus review (Willo)](https://www.willo.ai/blog/manus-review) · [Manus review (Taskade)](https://www.taskade.com/blog/manus-ai-review) · [Best browser agents 2026](https://nesyona.com/articles/best-ai-browser-agents-2026) · [Perplexity review (G2)](https://learn.g2.com/perplexity-ai-review) · [Perplexity 2026 (Neuriflux)](https://neuriflux.com/en/blog/perplexity-ai-review-2026) · [Deep research showdown](https://www.tminusai.com/blog/deep-research-ai-showdown-2026) · [Deep research compared (Superkind)](https://superkind.ai/blog/ai-deep-research-tools) · [DeepSeek review (Neuriflux)](https://neuriflux.com/en/blog/deepseek-review-2026) · [DeepSeek vs ChatGPT](https://aimlapi.com/blog/deepseek-vs-chatgpt-in-2026-the-complete-model-breakdown) · [OpenCode review](https://aicoderscope.com/blog/opencode-review-2026/) · [Pi review (DEV)](https://dev.to/rosgluk/pi-coding-agent-review-minimal-hackable-ai-coding-cli-4ge8) · [Pi harness review](https://vibecodinghub.org/blog/pi-coding-harness-review) · [Cursor review 2026](https://softpicker.com/cursor-ai-review-2026/) · [Cursor rules docs](https://cursor.com/docs/rules) · [Codex review](https://replitreview.com/openai-codex-review/) · [Codex guide](https://www.innovatrixinfotech.com/blog/openai-codex-2026-what-it-can-do) · [Claude Code hooks/subagents/skills](https://ofox.ai/blog/claude-code-hooks-subagents-skills-complete-guide-2026/) · [Claude Code complaints (AppStuck)](https://www.appstuck.com/blog/claude-code-troubleshooting-10-errors-fixes-2026) · [Anthropic postmortem (InfoQ)](https://www.infoq.com/news/2026/05/anthropic-claude-code-postmortem/) · [Copilot agent mode](https://dev.to/stacknotice/github-copilot-agent-mode-complete-guide-2026-1p8k) · [Copilot review](https://www.nxcode.io/resources/news/github-copilot-review-2026-worth-10-dollars) · [Hermes review](https://www.mayhemcode.com/2026/09/hermes-agent-review-2026-features.html) · [Hermes cron docs](https://hermes-agent.nousresearch.com/docs/user-guide/features/cron) · [T3 Code explained](https://continuumcode.ai/guides/what-is-t3-code/) · [T3 Code (GLN)](https://gln75.com/en/blog/t3-code-open-source-app-parallel-ai) · [T3 Chat review](https://techfixai.com/t3-chat-ai-review/) · [Claude Cowork review](https://go9x.com/blog/claude-cowork-review) · [Canvas vs Artifacts](https://www.shareduo.com/blog/claude-artifacts-vs-chatgpt-canvas) · [Harness engineering (Faros)](https://www.faros.ai/blog/harness-engineering) · [Agents: good/bad/unknown](https://futureagi.com/blog/ai-agents-the-good-the-bad-and-the-unknown/) · [Streaming markdown renderers](https://dev.to/simonhe95/why-streaming-ai-chat-needs-a-renderer-for-incomplete-markdown-4754)

---

## 2. Prioritised feature matrix — what to test, in order

Priority = how many systems promote it × how central it is to Verevon's own
positioning (grounded, governed, Norwegian-first work assistant).

| # | Capability | Systems that promote it | Verevon claims (docs/code, pre-test) |
|---|---|---|---|
| P1 | Verifiable citations, inline, click-to-source | Perplexity, ChatGPT DR, Manus, Claude | Sources panel, confidence %; inline claim chips **not** rendered (design §9.5 item 5, producer-blocked) |
| P2 | Reliable answer on *any* question, incl. non-KB | all | "Spør" mode grounds first; behaviour on zero-coverage untested until now |
| P3 | Reasoning transparency (visible steps) | DeepSeek, ChatGPT, Perplexity | "Tenker", "N steg", Trace panel |
| P4 | Artifacts/Canvas: create, update, version, read back | Claude, ChatGPT | create/update/read_artifact tools; version nav |
| P5 | Deep research / long autonomous task | ChatGPT, Manus, Perplexity, Gemini | `deep_research` tool; "Grundig" effort |
| P6 | Memory + custom instructions + projects | Claude, ChatGPT, Cursor, Hermes | Dreaming memory ("Brukte N minner"); Spaces; no persona doc (backlog #14) |
| P7 | Edit / regenerate / version siblings / branch | ChatGPT | Regenerate + 1/N versions shipped 2026-08-01; branch: **no** (backlog #8) |
| P8 | Stop, resume after disconnect | OpenCode, Codex, Manus | Stop button; resumable streams shipped |
| P9 | Tool use with approval gates (HITL) | Cursor, Copilot, Codex, Claude Code | "Utfør" mode, Godkjenninger panel |
| P10 | Code execution + real file outputs | ChatGPT, Manus, Codex | `code_interpreter` (xlsx/docx/pdf) |
| P11 | Files in: PDF/DOCX/images; long paste | all | accept list: image/*, pdf, docx, txt, md, csv, json, html |
| P12 | Markdown fidelity: tables, math, diagrams, nested fences | all | Custom renderer; **no** KaTeX/mermaid dependency in `package.json` |
| P13 | Scheduled / proactive tasks from chat | Hermes, ChatGPT, Cursor BG agents | Infra exists; chat-facing "schedule this": **no** (backlog #17) |
| P14 | Slash commands | Claude Code, Hermes | Two: `/Last opp fil`, `/Generer bilde` |
| P15 | Model choice | T3, Perplexity, OpenCode | Picker present (design §3.5 says "no model picker") |
| P16 | Subagents / parallel agents | Claude Code, Codex, Cursor, T3 Code | Durable runs; no chat fan-out (backlog #4) |
| P17 | Web search grounding toggle | Perplexity, T3, ChatGPT | "Søk" toggle → `web_search` |
| P18 | Image generation | ChatGPT, Hermes | "Bilde" toggle |
| P19 | Voice | ChatGPT | Composer has "Stemmemodus" and "Stemmeinndata" buttons (backlog #26 said none — stale) |

---

## 3. Live test results and edge cases

Legend: ✅ works as the best systems do · ⚠️ works with a defect · ❌ fails ·
◻︎ not tested in this pass. Each ❌/⚠️ carries the evidence that established it.

### 3.1 Findings established so far

**F-01 ❌ Silent empty answer when the selected model's stream fails (P2, P8).**
Prompt: a self-contained formatting request ("Rendering-test, ikke bruk
verktøy…"). Result: **no answer text at all**; the only thing rendered was
*"Usikkert svar (10 % sikkerhet) — sjekket dokumentene — fant ingen dekning."*
The stored thread (`GET /api/v1/chat/threads/01M2H4EX753M2DCPR1DQ2AHVTA/messages`)
holds **one** message — the user's. Nothing was persisted for the assistant.
Root cause, traced:
- The composer's model was **"GPT 5.6 Terra Subscription"**, which inference-core
  routes to `openai-codex-subscription` (`infer_stream started … provider:
  "openai-codex-subscription"`).
- The broker returned an `error` event mid-stream: `WARN subscription broker
  stream failed code=subscription_broker_failed`
  (`inference-core/src/provider/codex_subscription.rs:416`). The adapter then
  sends a `done` chunk with `delta: ""` and `stop_reason: "stream_error"`.
- `FallbackChain::infer_stream` (`fallback.rs:1412`) had already returned the
  receiver, so **no failover** happened — the fallback loop only moves on for
  failures *before* the stream opens (429 / too-long / open error).
- model-gateway received "done, zero content", scored it (`verdict:
  "unrelated"`, `confidence: 0.1`, `kb_citations: 0`) and streamed only the
  verification banner. No `ChatEvent::Error` was emitted: model-gateway only
  emits `inference_stream_error` on a gRPC-level `Err` (`sse.rs:2719`), never
  on a well-formed `done` chunk whose `stop_reason` is `"stream_error"` — the
  adapter's failure is indistinguishable from a model that chose to say
  nothing. The SPA's `onError` handler (`chat-client.ts:1080`) never fired.
- Frequency: 2 subscription streams in the last 24 h, **1 failed** (50 %).
Why it matters: the user is told "the documents don't cover this" when the
truth is "the model never answered". Every system in §1 surfaces a provider
error and retries or falls back; none reports a provider outage as a knowledge
gap. This is the single worst finding of the audit.

**F-02 ❌ A leading number is swallowed by list parsing (P12).**
Prompt: "Hva er 17 * 23? Svar kort med bare tallet og en setning." Model output
(visible in the live-region status line): `391. Dette er produktet av 17 og
23.` Rendered message: **"1. Dette er produktet av 17 og 23."** — the DOM holds
`<ol start=null>` with one item, so the answer **391 was destroyed** and
replaced by a list marker "1.". CommonMark does make `391.` an ordered-list
opener, but every mature renderer preserves the start number (`<ol start="391">`)
and most chat UIs additionally refuse to start a list from a number > 1 at the
top of a message. Here the payload of the answer is lost, not just restyled.
Source: the block parser keeps only `ordered: boolean` per item and discards
the marker text (`chat-media-markdown.tsx:718–735`, `isOrderedListMarker`), and
the renderer emits a bare `<ol>` with no `start` (`ChatMessages.tsx:898`).

**F-03 ⚠️ Enter does not send (P11, keyboard).**
Reproduced 3/3 times via automation: text typed into the composer, Return
pressed, message stays in the box; the "Send melding" button works. Source
(`DashboardComposer.tsx:1529`) intends Enter-without-Shift to submit unless the
autocomplete popup is open (`:1491`). No popup was visible. Needs a manual
keyboard check to rule out a synthetic-event artefact; recorded as ⚠️ until then.

**F-04 ⚠️ "Fant ingen dekning" banner on questions that need no coverage (P2, P1).**
The control turn (17 × 23 = 391) rendered *"Usikkert svar (72 % sikkerhet) —
sjekket dokumentene — fant ingen dekning."* under a correct arithmetic answer.
The verifier is right that the KB has nothing on it, but presenting that as
"uncertain" on a question that could never be in the KB erodes trust in the
banner where it matters. Perplexity and ChatGPT only show source status when
sources were the point. A verdict of `unrelated` should read as "no sources
needed", not "uncertain".

**F-05 ⚠️ Memory recall fires on every turn, including ones it cannot help (P6).**
Both turns above showed *"Brukte 5 minner fra tidligere samtaler"* — on a
rendering test and on 17 × 23. Recall is a fixed-cost lookup, not relevance-
gated; the label makes it look like the model *used* memories to multiply.

**F-06 ⚠️ Unrelated web search on an inbox question (P17).**
Earlier in the session, "Søk i innboksen…" triggered a web search alongside the
inbox tool and pulled irrelevant sources (casino sites); the relevance filter
set them aside (relevance 0.00 < 0.30) so they were not cited, but the search
should not have run. Tool selection over-reaches when the question names an
internal source.

**F-07 ⚠️ Slow reasoning path with no progress detail (P3).**
17 × 23 took ~90 s on "GPT 5.6 Terra Subscription" showing only "Tenker" /
"Arbeider fortsatt…". DeepSeek and ChatGPT stream the reasoning; Verevon shows a
spinner label. The Trace panel exists but is not the default view while
waiting.

**F-08 ⚠️ Model picker exists against the design (P15).**
The composer shows a model picker ("Velg modell selv") with "Verevon Balance"
and "GPT 5.6 Terra Subscription"; `VEREVON_CHAT_DESIGN.md` §3.5 says *no model
picker*. The picker also let a user land on a provider with a 50 % failure
rate (F-01). Either the design or the picker is wrong; both cannot stand.

**F-09 ⚠️ Slash commands are two upload/image shortcuts, not commands (P14).**
`slashCommands` (`DashboardComposer.tsx:324`) = `/Last opp fil`, `/Generer
bilde`. Claude Code / Hermes style parameterised commands: absent (backlog #6
unchanged).

**F-10 ❌ Choosing the subscription model silently strips five capabilities (P5, P9, P15, P17, P18).**
`chat-client.ts:651,686–703`: when `provider === 'openai-codex-subscription'`
the client sends `browse_web: false`, `generate_image: false`, `plan_mode:
false`, `deep_research: false` and drops every explicit skill pick — while the
"Søk", "Bilde", "Utfør" and research controls in the composer still appear
active. A user who picks "GPT 5.6 Terra Subscription" and toggles Søk gets an
ungrounded answer with no indication anything was turned off. Combined with
F-01 (the same provider fails 50 % of streams) this model choice is a trap.
The market's rule (T3, Perplexity, OpenCode): when a model cannot do
something, the control is disabled *and says why*.

**Verified ✅ earlier this session (kept for completeness):** inbox tools read
real mail through a signed delegation (P9-adjacent, read-only); support-origin
threads are excluded from the chat list at three layers; Drafts tab renders;
regenerate keeps prior versions ("Forrige/Neste versjon" controls present).

### 3.2 Markdown fidelity (P12) — the rendering test, re-run on "Verevon Balance"

The same prompt that produced F-01 on the subscription model answered fully on
Verevon Balance in ~30 s, which also confirms F-01 was the provider, not the
prompt. Probed in the DOM after "Svar fullført":

| Element requested | Result | Evidence |
|---|---|---|
| Markdown table 3×3 | ✅ | 1 `<table>` |
| Inline + block LaTeX | ❌ shown as raw `$x = \frac{-b}{2a}$` / `$$E = mc^2$$` | 0 `.katex`/`math` nodes; no KaTeX/MathJax dependency in `package.json` |
| Mermaid diagram | ❌ shown as a code block `graph TD A[Start] --> B…` | no mermaid dependency; `<pre>` only |
| Nested fence (```` ```python ```` inside ```` ```markdown ````) | ⚠️ outer block closed at the inner fence — 3 `<pre>` blocks, one empty | `chat-media-markdown.tsx:601–606`: a block ends at the next line matching `/^```/`, regardless of fence length or language |
| Task list (`- [x]`, `- [ ]`) | ❌ rendered as plain bullets | 0 `input[type=checkbox]` |
| Two-level blockquote | ⚠️ one level only | 1 `<blockquote>`, 0 nested |
| Arabic + emoji line | ✅ | present, correct direction |
| First line `391. Fasit.` | ❌ **F-02 reproduced on a second model** | `<ol start=null>` — 391 lost again |

Every system in §1 renders math and task lists; ChatGPT, Claude and
Perplexity render Mermaid. For a product whose answers are frequently
financial (margins, VAT, unit conversions), unrendered math and a renderer that
can drop a leading number are user-visible correctness issues, not polish.

### 3.3 Stop mid-answer (P8)

Prompt: a 700-word essay, no tools. "Stopp svar" pressed ~11 s in, while the
model was still in its thinking phase (23 chars of body at 5 s).

| Check | Result |
|---|---|
| Stream stops promptly | ✅ `Stopp svar` gone within 3 s |
| Assistant bubble state after stop | ⚠️ **empty body**, no "Stoppet"/"Avbrutt" marker — the bubble shows only *"Brukte 5 minner fra tidligere samtaler"*; a reader cannot tell a stopped answer from an empty one. The controller does set `turn.status = 'stopped'` (`use-chat-controller.ts:1181,1202`), but `ChatMessages.tsx` never renders that status — the only reference to `'stopped'` in the components is the type union (`chat-types.ts:190`) |
| Continue-from-here affordance | ❌ none. The only related control in source is `Kjør som ny tur` (`ChatMessages.tsx:497`); there is no `Fortsett` for a stopped chat answer (grep of `i18n.tr('Stoppet|Avbrutt|Fortsett…')` finds only browser-run continues in `ChatLiveRunPanel.tsx`) |
| Partial text preserved when stop lands mid-text | ◻︎ re-tested in §3.4 with a later stop |

ChatGPT and Claude mark a stopped answer and offer "Continue generating";
OpenCode/Codex keep the partial as a first-class message. Verevon's stop is a
clean abort but leaves no trace of *why* the answer is short or absent —
compounding F-01, where an empty bubble already means "provider failed".

### 3.4 Regenerate, version siblings, and instruction adherence (P7, P4)

"Generer på nytt — beholder dette svaret som versjon 1" on the stopped turn
from §3.3.

| Check | Result |
|---|---|
| Regenerate keeps the prior answer | ✅ switcher shows `1/2` ↔ `2/2`; "Forrige versjon" returns to v1 |
| v1 (the stopped, empty answer) is labelled as stopped | ❌ v1 renders as an empty bubble with only the memory line — same defect as §3.3 |
| 700-word essay lands somewhere readable | ✅ the model created a document artifact *"Historien til norsk fiskeoppdrett"* (Work feed: `Tool: Create artifact … (document, v1)`) and said so in chat |
| Explicit instruction "ikke bruk verktøy" honoured | ⚠️ overridden — an artifact tool ran anyway. Defensible product behaviour for 700 words, but the user's instruction was not acknowledged; ChatGPT/Claude would keep prose inline when told to |
| Artifact discoverable from the workspace | ⚠️ the workspace tablist offered only **"Arbeid 1"** — no artifact/result tab appeared; the document is reachable only via the Work-feed entry (checked further in §3.5) |

**F-11 ⚠️ Tool calls are invisible in model-gateway logs (observability).**
While the artifact tool ran, `docker logs model-plane-model-gateway-1` (all
levels, 8 min) contained **zero** lines mentioning a tool, a dispatch or an
artifact; the only record was the Work feed in the browser. The harness
consensus in §1 names observability as one of the five load-bearing layers,
and F-01 was diagnosable only through inference-core's logs — a tool failure
in model-gateway would today leave no server-side trace at all.

### 3.5 Artifacts (P4)

Fresh thread, Verevon Balance: *"Lag et dokument i arbeidsflaten (artifact)
med en tekst på cirka 150 ord om hva Aquatiq AS leverer."*

| Check | Result |
|---|---|
| Artifact created and surfaced | ✅ workspace tabs became **Arbeid 4 · Resultat 1 · Kilder 2**; "Resultat" opens the document |
| Grounded before writing | ✅ `knowledge_search` ran; two sources; text matches aquatiq.com's five service areas |
| Artifact panel affordances | ✅ title, kind ("Markdown-dokument"), size, age, **Kopier**, **Last ned** |
| Length control | ⚠️ 125 words for "cirka 150" — acceptable |
| Language quality | ⚠️ one mangled compound, *"mattprygghetskompetanse"* (→ mattrygghetskompetanse) in the chat summary |
| Selection-scoped edit / shortcut menu / version restore | ❌ none in the panel — backlog #21 unchanged. ChatGPT Canvas' headline strength (highlight a span → "shorten this") has no equivalent |
| Artifact from a **regenerated** turn surfaced | ⚠️ inconsistent — see F-12 |

**F-12 ⚠️ An artifact created on a regenerate-after-stop never got a "Resultat" tab.**
In §3.4 the model created *"Historien til norsk fiskeoppdrett"* (5 476 chars,
Work feed confirms `Tool: Create artifact`) during a regenerate of a **stopped**
turn, and the workspace tablist stayed at **"Arbeid 1"** — the document was
unreachable except as a line in the Work feed, while the chat said "ligger i
sidepanelet". A clean regenerate of the §3.5 artifact turn (no prior stop)
kept **Resultat 1** and produced `2/2` correctly, so plain regenerate is not
the trigger. Observed once; the reproducible precondition appears to be
stop → regenerate. `onArtifact` attaches the artifact to `assistantId` and the
tab is offered when `artifactItems().length > 0` (`ChatPage.tsx:267,310`),
so the likely fault is the stopped turn's id/version snapshot diverging from
the id the regenerate stream writes to. Needs a targeted repro before a fix.

### 3.6 Composer input edge cases (P11)

| Check | Result | Evidence |
|---|---|---|
| Enter sends | ⚠️ did not, 3/3 via automation (F-03) | `DashboardComposer.tsx:1529` |
| Long text paste → "pasted text" attachment (ChatGPT/Claude behaviour) | ❌ pasted text stays inline in the textarea; only pasted **images** become attachments | `onPaste` at `:1676` handles `clipboardData.files` only; no long-paste logic anywhere in `src/` |
| Accepted attachment types | ✅ `image/*, .pdf, .docx, .txt, .md, .markdown, .csv, .json, .html` | `:2091` |
| Attachment size / count limit shown to user | ◻︎ no `MAX_*`/size guard found in the composer — untested live | grep of `MAX_\|maxFiles\|too large` finds only the textarea height cap |
| Slash commands | ⚠️ two shortcuts only (F-09) | `:324` |
| Voice input / voice mode buttons present | ✅ "Stemmeinndata", "Stemmemodus" | button inventory §3.3 |

### 3.7 Grounding and citations (P1, P2)

Fresh thread: *"Hva er Aquatiqs fem hovedområder ifølge kunnskapsbasen? Svar
kort og vis kildene…"*

| Check | Result |
|---|---|
| Refuses to guess when coverage is thin | ✅ *"Jeg kan ikke svare på spørsmålet uten å gjette"* — names the only docs found and offers two concrete next steps (search aquatiq.com live, or upload). This is the behaviour Perplexity is praised for and ChatGPT is criticised for lacking |
| Banner semantics here | ✅ *"Usikkert svar (66 %) — sjekk kildene"* is right for this case (contrast F-04) |
| Inline citation chips in the answer | ❌ none — `parseInline(text, citations)` receives citations but no `sup`/chip nodes were rendered; sources live only in the "Kilder" tab (backlog #5 / design §9.5 item 5, producer-blocked) |
| Sources tab | ✅ "Kilder 1" opened; cards are real `<a href target=_blank>` links (`ChatPanels.tsx:427`) |
| **KB inventory behind the answers** | ⚠️ `GET /api/v1/knowledge/documents` returns **3 documents in total** for the org: `skills-list.pdf`, `https://www.aquatiq.com/`, `https://aquatiq.com/`. The two web entries are the root page twice (the model itself called them "viderekobling-sider"). Every grounded answer in this audit — including §3.5's confident "five service areas… hentet direkte fra aquatiq.com" — rests on these three items |
| Consistency across turns with identical sources | ⚠️ §3.5 wrote a confident five-area company description from the same two root pages that §3.7 judged insufficient. Same corpus, opposite confidence — the difference was the task framing (write vs. answer), not the evidence |
| `skills-list.pdf` as an org "source" | ⚠️ a Verevon-internal skills catalogue is indexed in the customer's knowledge base and gets offered as a citation for company questions |

Net: the grounding *machinery* behaves well (honest refusal, real links, verifier
banner) but the corpus it stands on is three documents, one of which does not
belong there. That is the item already tracked as "B1 re-crawl aquatiq.com" in
the end-to-end plan; this audit adds the measurement.

### 3.8 Memory (P6)

"Vis hva jeg husker" on the §3.7 turn expands the memory line inline.

| Check | Result |
|---|---|
| Transparency: shows what was recalled | ✅ five entries, each labelled `USER · utledet` (inferred): works in Norwegian · works in IT · works at AQUATIQ AS · based in Norway · prefers project plans as drafts without calendar events or notifications |
| Provenance labelling | ✅ "utledet" vs stated is exactly the distinction Claude's memory UI makes |
| Relevance gating | ⚠️ the same five profile facts are recalled on **every** turn — 17 × 23, a rendering test, a KB question — because they are the only memories that exist after the 2026-09-14 wipe. The label "Brukte 5 minner" therefore reads as "used" when the truth is "retrieved" (F-05) |
| Edit / delete a memory from the chat | ❌ the expanded list offers only "Skjul"; no per-entry correct/forget control. Claude and ChatGPT let you fix or delete a wrong memory where you see it. (Deletion exists elsewhere — the 2026-09-14 wipe went through the memory API — so this is a chat-surface gap, not a platform gap) |
| Explicit "remember this" then recall in a new thread | ◻︎ not exercised in this pass — the Dreaming extractor runs asynchronously after the turn, so a same-session check would be a timing test rather than a feature test |

**F-13 ❌ Confident "grounded" answer whose sources are empty stubs (P1).**
Data Plane `documents` for the org (query on `data-plane-v2-postgres-1`,
`dataplane.documents`, `deleted_at is null`):

| title / source | status | content chars | indexed |
|---|---|---|---|
| `skills-list.pdf` (sharepoint) | indexed | 4 993 | 2026-09-06 |
| `https://www.aquatiq.com/` (quarry) | indexed | **63** | 2026-09-04 |
| `https://aquatiq.com/` (quarry) | indexed | **84** | 2026-09-04 |

Sixty-three characters cannot describe five service areas. §3.5's document —
*"basert på informasjon hentet direkte fra aquatiq.com"*, with **Kilder 2**
attached — was written from the model's own knowledge of Aquatiq and then
presented as sourced. The verifier's confidence read 82 %. §3.7 later got the
same two stubs and correctly said there was nothing there. The failure is not
that the model knows Aquatiq; it is that the UI attached citations to text the
citations do not support. This is the exact failure mode Perplexity is trusted
for avoiding, and it is the one a "sjekk kildene" banner cannot catch when the
user *does* check and finds a real company URL.

### 3.9 Effectful requests without an execution mode (P9)

Prompt (still in "Spør"): *"Send en e-post til synnove.venas@aquatiq.com nå…
Bare send den."*

| Check | Result |
|---|---|
| Does not send; says why | ✅ *"Jeg kan ikke sende e-post direkte – jeg har ikke tilgang til e-postverktøy i denne samtalen"* — true (no send tool exists on the chat path) |
| Offers the useful fallback | ✅ a copy-ready draft |
| No approval UI appears | ✅ correct for Spør mode; "Utfør" is tested in §3.10 |
| **F-14 ⚠️ Over-redaction of user-supplied PII** | the draft's `Til:` line reads **`[redacted-email]`** — the address the user *typed into the question* was stripped from the answer. model-gateway logged `WARN capability-core bearer absent while resolving PII policy; redacting` (`moderation.rs`): when the policy lookup has no bearer it fails closed and redacts everything that looks like PII, including the user's own input echoed back. Fail-closed is the right default for third-party data; for an address the user just pasted it makes the draft unusable. **Scale:** the warning fired on every chat turn in the audit window (3 in 6 min = 3 turns), so the org's PII policy is never actually consulted on the chat path — redaction is unconditionally on, and any answer that legitimately contains an e-mail, phone or personal name is being silently degraded. **Root cause (source):** the BFF has the plumbing — `shared.rs:610` forwards `x-capability-authorization` when given a bearer, and `required_capability_token()` (`shared.rs:257`) mints one — but every chat-stream call site passes **`None`** for it (`streams.rs:115, 144, 188, 245`). The org's PII policy is therefore unreachable from chat by construction, not by outage. The same missing bearer also blocks the **injection-defense** policy lookup (`WARN … resolving injection_defense policy; screening stays on`, 9 times during the deep-research run) — that one fails safe (screening on), but it means the org can neither relax nor tune either policy from Control Plane for chat |
| Banner | ⚠️ *"fant ingen dekning"* on a drafting task — F-04 again |

### 3.10 "Utfør" — plan-first agentic turn with approval gate (P9, P3)

Fresh thread, **Utfør** mode, Verevon Balance: *"Hent frakttilbud for en pakke
på 66 kg… Vis planen din først, og ikke book noe."*

| Check | Result |
|---|---|
| Plan before action | ✅ a five-step plan in chat (look up both companies' registered addresses · fetch quotes for 66 kg/60×40×40 · use B2B segment and compare carriers · show price, lead time, terms · book nothing). Nothing executed — the plan honoured "ikke book noe" |
| Approval gate | ✅ *"Dette var en plan – ingenting er utført. Velg hvor mye agenten får gjøre, og skriv hvorfor"* with two scopes — **skrive i arbeidsområdet** / **utføre alt, også utgående handlinger** — a free-text justification, and **Godkjenn**. This is the Claude Code / Codex permission model expressed as product, and stronger than Cursor's per-command prompts because the scope is chosen once and recorded with a reason |
| Durable run surfaced | ✅ right panel: "Arbeid — Plan (lagret av arbeidskjøringen)", "Spor" tab, "Kjør"/status chip |
| Approve is gated on a reason | ✅ **Godkjenn** is `disabled` until the justification field is filled |
| Run details | ❌ **F-15** *"Noen kjøringsdetaljer kunne ikke hentes akkurat nå"* — the BFF logged `GET /api/v1/orchestration/runs/01M2H5WM4PQH4F29BYW7DJVCHQ/proof-bundle → 500 Internal Server Error` (53 ms) for this run — the **proof bundle** (the run's evidence/receipt), not the run record itself. The panel degraded gracefully, but a fresh, healthy run's evidence endpoint returning 500 is a defect, not a transient. Trace: BFF `orchestration.rs:387` → model-gateway `GET /v1/orchestration/runs/:id/proof-bundle` (`http_routes.rs:858`) → gRPC `orchestration_client.get_run_proof_bundle` → `grpc_status_to_http` where every code not explicitly mapped becomes **500** (`http_routes.rs:5559`, `_ => INTERNAL_SERVER_ERROR`). Neither model-gateway nor orchestrator-core logged a line for the run id (0 mentions in 15 min). **Root cause (source):** orchestrator-core never implemented the RPC — `handlers.go:17` embeds `mpv1.UnimplementedOrchestrationCoreServiceServer`, its own comment at `:105` says `GetRunProofBundle` answers `codes.Unimplemented`, and the only `ProofBundle` references outside generated code are in `handlers_test.go` (a stub client). Unimplemented falls into model-gateway's catch-all and becomes 500. The SPA is calling a contract that exists in the `.proto` and nowhere else — the "wired-but-dead" class the design doc's §2.3 was written to catalogue |
| Raw identifiers in UI | ⚠️ *"Opprettet av qw4BbrWqet889ngwIlFrFWjqVwRvQT4s"* — a subject id where a name belongs |
| Banner | ⚠️ *"Usikkert svar (72 %) — Ingen kilder ble brukt, så bekreft det selv"* under a plan that needs no sources (F-04 family) |
| Not approved in this audit | by design — the approval would have started real carrier calls and a BRREG lookup; read-only quotes were already verified earlier in the session |

### 3.11 Deep research (P5, P1, P3)

Fresh thread, **Spør** + **Dyp research** toggle, Verevon Balance. Question: EU
Regulation 2023/2006 (GMP for food-contact materials) and what it means for a
Norwegian supplier of cleaning chemistry. Wall time ≈ 2.5 min.

| Check | Result |
|---|---|
| Runs as a durable run with live progress | ✅ "Executing" card, tabs grew live: Arbeid 11 → 12, Kilder 24, Resultat 1 → 2, Spor |
| Multi-query fan-out | ✅ Work feed: *"3 relevante av 24 unike kilder fra 6 av 6 delspørsmål"* — six sub-questions, 24 unique sources |
| Report as an artifact | ✅ *"Rapport klar: 4913 tegn, 3 kilder sitert"* in Resultat; chat carries a structured summary |
| Honesty about evidence | ✅ **exemplary** — the summary leads with *"Kildedekningen var svak – kun 3 av 24 funne kilder lot seg lese, og ingen scoret høyt på relevans"* and the run card says *"Checked against 24 web sources shown in Kilder"*. Perplexity/ChatGPT rarely volunteer this |
| Source readability | ❌ **F-16** only **3 of 24** sources were used. The Kilder tab labels one card *"lest"* and the rest *"irrelevant"*; the report's own note says *"21 kilder ble satt til side som irrelevante"* and separately names two that **could not be read**: *"EUs egen side om matkontaktmaterialer (European Commission Food Safety) og Lovdatas fullstendige tekst"* — the two most authoritative sources for this exact question. The candidate pool also contained facebook.com ×2, linkedin.com ×2 and formulaswiss.com, so the search stage surfaced weak candidates and the gate then (correctly) removed them. *Correction during the audit:* a first log grep appeared to show 6 × 403 / 6 × 429 from Quarry; those matches were digits inside request ids (`req_…-403d-…`) on `status=200` lines, and were discarded. Because model-gateway logs nothing per fetch (F-11), the fetch-vs-gate split for the 21 cannot be established from the server side |
| Per-source status in Kilder | ✅ each card carries a status ("lest" / "irrelevant") — Perplexity does not show why a source was dropped |
| Trace tab (P3) | ⚠️ *"Kvitteringen kunne ikke hentes akkurat nå"* (F-15 again — the proof bundle), then **75 events** that read almost entirely as `step · updated` with timestamps — an audit trail with no verbs. DeepSeek/ChatGPT show what was thought or searched; this shows that *something* updated 70 times |
| Report quality (read in full via the "Last ned" data-URI) | ✅ **strong**: 657 words, 11 headings, 12 numbered references `[n]`, an up-front *"Viktig forbehold om kildedekning"*, a *"Kunne ikke verifiseres"* section, and a *"Dekningsnotat"*. Best of all it caught the **false premise in the question**: it explains that 2023/2006 binds *producers of materials and articles*, that it is *"ikke dokumentert"* the regulation directly covers cleaning-chemistry suppliers, and points to the biocide regulation and the hygiene package instead. Claude Research is praised for exactly this synthesis; here it is live |
| Reference integrity | ⚠️ the report cites `[2]` and `[3]` but its source list has no `[1]`, while the run card says "3 kilder sitert" — one source was counted but not listed |
| Explanation of exclusions | ⚠️ the chat says 21 sources *"lot seg ikke lese"* (unreadable) while the report's note says they were *"satt til side som irrelevante"* (relevance-gated). Both are partly true (403/429 **and** low relevance), but the two surfaces give different reasons |
| Inline citations in the chat summary | ⚠️ one inline link (regjeringen.no), no numbered references; numbered refs exist only inside the artifact and are not hyperlinked (backlog #5) |
| Export | ✅ "Last ned" is a `data:text/markdown` link with the full report; "Kopier" present |
| Speed vs market | ✅ 2.5 min sits between Perplexity (2–4 min) and ChatGPT (up to 30 min) |
| Raw user id on the run card | ⚠️ same as §3.10 |

### 3.12 Web search grounding and small composer edge cases (P17, P11)

| Check | Result |
|---|---|
| Whitespace-only message | ✅ Send is `disabled` with six spaces in the box; clicking it does nothing, no empty turn is created |
| "Søk på nett" + a live-fact question (*Norges Banks styringsrente nå, sist endret, kilde med dato*) | ✅ **verified correct**: answer said 4,25 %, decided 12 Aug 2026 (unchanged), next decision 24 Sep 2026, citing the Norges Bank meeting page. Fetching that page independently during the audit confirms every figure and date. Kilder 5 |
| Cited URL in the answer body is clickable | ❌ the source was printed as plain text (*"URL: https://www.norges-bank.no/…"*) — 0 `<a>` in the message. The inline parser only linkifies `[text](url)` (`chat-media-markdown.tsx:927`); bare URLs are not autolinked. Every competitor autolinks |
| Banner | ✅ *"86 % sikkerhet"* with no "ingen dekning" clause — because web citations existed. Consistent with F-04's diagnosis: the clause tracks *KB* coverage only |
| Search toggle state visible | ✅ `aria-pressed="true"` on "Søk på nett" |

### 3.13 Code execution with a real file output (P10)

Prompt: *"Lag en Excel-fil (xlsx) med to kolonner … 12 rader … gi meg filen til
nedlasting."* Verevon Balance, same thread as §3.12.

| Check | Result |
|---|---|
| Tool ran and produced the file | ✅ Work feed: `Tool: Code interpreter` → `{"stdout":"Fil lagret: maned_verdi.xlsx","exit_code":0,"files":[{"name":"maned_verdi.xlsx","mime":"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet","bytes":5022,"content_b64":"UEsDB…"}]}` — a genuine 5 KB workbook |
| File delivered to the user | ❌ **F-17** no download card, no "Resultat" tab (tabs stayed *Arbeid 2 · Kilder 5*), `a[download]` count 0. The only route to the bytes is the base64 in the Work feed's tool-result text |
| Model's own link | ❌ the answer says *"📥 [Last ned maned_verdi.xlsx](sandbox:/maned_verdi.xlsx)"* — a ChatGPT-style `sandbox:/` URL that means nothing here; it was also rendered as raw brackets, not as a link |
| Table preview in chat | ✅ the 12 rows are rendered as a markdown table |
| Contrast with the design | model-gateway has a unit test (`tool_loop.rs` ≈6435–6470) asserting that a tool result with `files` yields **one `Artifact` (kind "spreadsheet", `data:` URI) + one `Attachment`**, and the SPA has `onAttachment` → "Last ned" (`ChatMessages.tsx:1687`). The contract exists on both ends; on this live turn neither event arrived |
| **Root cause (source, two planes)** | The persisted assistant turn carries only `confidence` — no recorded artifacts — so the events were never emitted. `code_interpreter_events` (`tool_loop.rs:340`) starts with `serde_json::from_str(&outcome.output)` and returns nothing on a parse failure. `outcome.output` is execution-core's `response.output` verbatim (`tool_loop.rs:1871`), and execution-core renders every result **with a provenance prefix**: `provenance.rs:266` → `"[source: org-internal]\n{…json…}"` (visible verbatim in the Work feed). The prefix that protects the model from spoofed provenance makes the JSON unparseable for the file extractor, so: no artifact, no attachment, no base64-to-note rewrite — the raw base64 goes back to the model, which does what ChatGPT-trained models do with a file it cannot link: invent `sandbox:/`. The unit test passes because its fixture is bare JSON. This is the "silent degradation when a producer changes its output shape" failure the harness literature warns about, across a plane boundary |

ChatGPT's code interpreter, Manus and Codex all hand the file back as a
download; here the user is told the file exists and cannot get it.

**Side observation (P3).** The Work panel's "Teknisk aktivitet" block reads
*"KONTEKSTVINDU 751 / 4 096 tokens (18 %)"* on a Verevon Balance turn. Whatever
that 4 096 measures (a per-tool result budget, most likely), a reader will take
it as the model's context window and conclude Verevon runs on a 4k model. The
label needs to say what it is.

### 3.14 Tool availability, language switch, multi-part questions (P2, P3)

| Check | Result |
|---|---|
| Weather for a city the tool does not cover (*"Hva er været i Lillehammer? Bruk værverktøyet."*) | ⚠️ **F-18** the model answered *"Jeg har dessverre ikke tilgang til et værverktøy i denne samtalen – ingen slik funksjon er tilgjengelig"* — false. `get_weather` is a built-in advertised on every chat turn (`tool_loop.rs:3356`), and the inline allowlist denies only `save_memory`, `browser_agent` and MCP names (`inline_tool_allowed`, `tool_loop.rs`). The right move was to call it, receive the tool's own "Lillehammer is not covered, use web_search" rejection, and relay that. Instead it denied having the capability. It did **not** invent a forecast, and it pointed to yr.no — so no hallucination, but a capability lie. The yr.no URL was again plain text (§3.12) |
| Language switch mid-thread ("Switch to English from now on") | ✅ answered in English immediately; verifier banner and memory line stay Norwegian (UI chrome, acceptable) |
| Multi-part question (3 parts in one message) | ⚠️ only the **last** part (0.1 + 0.2 in Python — answered correctly with IEEE 754 explanation) was addressed; the two recall parts were dropped without acknowledgement. Re-tested with an explicitly numbered two-part question — see §3.15 |

### 3.15 Context recall across a long thread (P6, P2)

Same 10-message thread, explicitly numbered two-part question in English.

| Check | Result |
|---|---|
| Recall of an earlier fact + its source | ✅ *"4.25%, held unchanged at the meeting on 12 August 2026, sourced from Norges Bank (norges-bank.no), published 12 August 2026"* — exact |
| Recall of earlier requests | ✅ names the xlsx request precisely (columns, 12 rows, values 1–12); counts the weather request as the second "thing to create" — a fair reading of the thread |
| Honesty about its own failure | ✅ *"I could not fulfill the second one as no weather tool was available"* — consistent with F-18 (consistently wrong about the tool, but not covering it up) |
| Numbered parts vs. prose parts | the same recall questions were dropped in §3.14 when embedded in prose and answered here when numbered — an instruction-following edge worth a regression case |

### 3.16 Not exercised in this pass (and why)

| Capability | Why not | How to test next |
|---|---|---|
| File attachments (PDF/DOCX in) — P11 | the in-app browser used for the audit has no file-chooser upload | manual: attach `skills-list.pdf`, ask for a summary with page references; then a 30-page PDF to find the size guard that the code does not show |
| "Remember this" → recall in a new thread — P6 | the Dreaming extractor is asynchronous; a same-session check tests timing, not the feature | state a fact, wait for the next extraction cycle, open a new thread, ask |
| Approve the Utfør plan — P9 | approval would have started real carrier calls and a BRREG lookup | approve with scope "skrive i arbeidsområdet" on a read-only plan and check the receipt/Spor |
| Branch conversation, share link, scheduled task from chat — P7, P13 | no UI affordance exists (backlog #8, #17, #22 unchanged) | — |
| Voice mode / voice input — P19 | buttons exist; audio cannot be driven from the automation | manual |
| Image generation — P18 | not attempted (cost, and no parity question was open) | "Generer bilde" with a simple prompt; check attachment delivery, given F-17's mechanism |
| Prompt injection via a fetched page or an inbox message | would require planting content in the org's real inbox or a controlled page | use a controlled page under the team's own domain; injection-defense screening is on (F-14 note) |
| Context-window compaction / `reattach_context` | needs a thread far longer than this audit produced | a 60+ turn thread, then a question about turn 3 |

---

## 4. Gap list vs the market

Grouped by what the best system does; status is what this audit **observed**,
not what a doc claims.

| Capability (bar-setter) | Verevon today | Status |
|---|---|---|
| Answer arrives or an error does (every system) | a provider stream failure becomes an empty bubble labelled "fant ingen dekning" (F-01) | ❌ worst finding |
| Citations you can verify (Perplexity) | real links in Kilder; but citations attached to 63-char stub pages (F-13); no inline chips; bare URLs not linkified | ⚠️ machinery good, corpus and presentation weak |
| Honest "I don't know" (Perplexity, Claude) | §3.7 refusal, §3.11 caveats and false-premise catch | ✅ **better than most** |
| Deep research (ChatGPT DR, Perplexity, Gemini) | 6 sub-queries, 24 sources, 2.5 min, structured report with numbered refs, coverage note | ✅ format & honesty; ❌ read rate 3/24 (F-16) |
| Reasoning transparency (DeepSeek, ChatGPT) | "Tenker / N steg", Work feed, Trace with 75 `step · updated` events; receipt 500 (F-15) | ⚠️ present but low-information |
| Artifacts (Claude) | create/read/update, versions, Kopier/Last ned, artifact-as-report | ✅ core; ❌ file outputs (F-17); ⚠️ stop→regenerate tab (F-12); ❌ selection edit (Canvas) |
| Code interpreter with downloads (ChatGPT, Manus) | code runs, file exists, user cannot get it (F-17) | ❌ |
| Plan-first + scoped approval (Claude Code, Codex, Cursor) | five-step plan, scope choice, mandatory reason, Godkjenn gated | ✅ **stronger than Cursor** |
| Memory with transparency (Claude, ChatGPT) | shows what was recalled with "utledet" provenance; no inline correct/forget; recalled every turn (F-05) | ⚠️ |
| Projects / custom instructions (ChatGPT, Claude, Cursor rules) | Spaces exist; no persona document (backlog #14) | ⚠️ |
| Edit / regenerate / versions / branch (ChatGPT) | regenerate + 1/N ✅, edit ("Rediger") present, branch ❌ | ⚠️ |
| Stop / continue (OpenCode, ChatGPT) | stop works; no stopped marker, no continue (§3.3) | ⚠️ |
| Markdown fidelity (all) | tables ✅; math ❌, mermaid ❌, task lists ❌, nested fences ⚠️, leading-number swallow ❌ (F-02), autolink ❌ | ❌ |
| Model choice that is safe (T3, Perplexity) | picker exists against design; subscription model strips 5 capabilities silently (F-10) and fails 50 % (F-01) | ❌ |
| Web search grounding (Perplexity, T3) | verified-correct live answer (§3.12) | ✅ |
| Tool honesty (all agents) | denied an available tool (F-18); over-reached with web search on an inbox question (F-06) | ⚠️ |
| Slash commands (Claude Code, Hermes) | two upload/image shortcuts (F-09) | ❌ |
| Scheduled / proactive (Hermes, ChatGPT Tasks) | none from chat (backlog #17) | ❌ |
| Subagents / parallel (Claude Code, Codex, T3 Code) | none from chat (backlog #4) | ❌ |
| Long-paste handling (ChatGPT, Claude) | text stays inline; only images become attachments | ⚠️ |
| Observability of the agent loop (harness consensus) | no server-side tool-call logs; two 500s undiagnosable from logs (F-11, F-15) | ❌ |
| Privacy policy enforcement (enterprise) | PII redaction and injection-defense policies unreachable from chat; redaction unconditionally on (F-14) | ❌ (fails safe, but ungovernable) |

---

## 5. Priorities for the fix pass

Ranked by user harm × frequency × how cheap the fix is now that the cause is
known. Effort: S = hours–a day, M = days, L = a week+.

1. **F-01 — a failed provider stream must be an error, not an empty answer.** (S)
   inference-core already emits `stop_reason: "stream_error"`; model-gateway
   should treat a `done` with that reason (or zero content after a non-empty
   prompt) as `ChatEvent::Error { retryable: true }`, and the fallback chain
   should be allowed to retry the *next* provider when the failure arrives
   before the first content delta. Then the SPA's existing `onError` shows it.
2. **F-14 — forward the capability bearer on chat streams.** (S) Four `None`
   arguments in `streams.rs`; `required_capability_token()` already exists.
   Unblocks the org's PII and injection-defense policies for chat and stops
   redacting the user's own input.
3. **F-17 — parse the provenance-prefixed tool result.** (S) Either strip the
   `[source: …]` header before `serde_json::from_str` in
   `code_interpreter_events`, or have execution-core carry provenance in a
   field rather than in the text. Add a fixture with the real prefix to the
   unit test that currently passes on bare JSON. Also refuse/rewrite
   `sandbox:/` links.
4. **F-02 + P12 renderer — preserve `<ol start>`, autolink bare URLs, render
   task lists, track fence length.** (S–M) The leading-number swallow destroys
   answers; the rest is parity. Math and Mermaid are a dependency decision (M).
5. **F-10 + F-08 — make the model picker safe or remove it.** (S) If a provider
   cannot search/plan/research, disable those controls with a reason; hide a
   provider with a failing broker. Reconcile with design §3.5.
6. **F-13 — do not cite what was not read.** (M) Attach citations only to
   sources whose retrieved content exceeds a minimum, and mark a
   parametric-knowledge answer as such. Re-crawl aquatiq.com (plan item B1) and
   remove `skills-list.pdf` from the customer corpus.
7. **F-15 — implement or un-wire `GetRunProofBundle`.** (S to un-wire, M to
   implement) A UI that calls an unimplemented RPC on every run is the
   design doc's own anti-pattern.
8. **F-11 — log tool calls and gRPC failures in model-gateway.** (S) One INFO
   line per dispatch (name, call id, ms, ok/err) and one WARN with the gRPC
   code on every `grpc_status_to_http` non-2xx.
9. **§3.3 stop UX — render `status === 'stopped'`, offer "Fortsett".** (S)
10. **F-04 / F-05 — banner and memory copy.** (S) "Ingen dekning" only when the
    question needed coverage; "Hentet 5 minner" not "Brukte".
11. **F-16 — source readability in deep research.** (M) Instrument fetch
    outcomes per source (the audit could not separate blocked from irrelevant),
    then decide on retries/alternate fetchers for the authoritative hosts.
12. **F-18 / F-06 — tool selection.** (M, prompt + eval) Regression cases:
    unsupported-city weather must *call* the tool; an inbox question must not
    web-search.
13. **Parity backlog items still open and confirmed by this audit:** inline
    citation chips (#5), branch (#8), slash commands (#6), persona doc (#14),
    schedule-from-chat (#17), subagents (#4), selection-scoped artifact edits
    (#21), long-paste attachment. Ranked in the 2026-08-01 backlog; nothing
    here changes that order.

**What is genuinely strong and should be protected during the fix pass:** the
honest-refusal behaviour (§3.7, §3.11), the plan-first approval gate (§3.10),
verified-correct web grounding (§3.12), memory transparency with provenance
(§3.8), and deep research's coverage notes. These are differentiators the
market leaders do not consistently have.

_Sections 3.4+ are appended as each capability in §2 is exercised._

---

## 0.2 §0.1's four remaining items — implemented and independently verified, 2026-09-17

All four items named in §0.1's "Next, in order" were picked up: F-12 reproduced live and fixed,
continue-after-stop, math/Mermaid rendering, and per-memory forget. A separate, adversarial
verification pass re-ran everything rather than trusting the implementers' own reports.

**F-12 — root cause was one level up the stack from the original hypothesis, confirmed live.**
The original note guessed "the stopped turn's id/version snapshot diverging from the id the
regenerate stream writes to." The real cause: `readSseStream` swallows `AbortError` instead of
rejecting, so a manually aborted stream's `await streamChat(...)` still resolves normally, just
after `handleStop()` already wrote `status: 'stopped'`. Since `settled` was never set, execution
unconditionally fell through to `stopStreaming(undefined)`, silently reverting the status back to
`undefined` moments after the user stopped it — exactly the kind of turn-state instability a
stop→regenerate sequence depends on. Fixed in `use-chat-controller.ts` by branching on
`controller.signal.aborted`, mirroring the pattern the `catch` block just below it already used.
Live re-verification: reproducing the exact stop→regenerate sequence showed no ghost duplicate
"Stoppet" bubble across ~2.5 minutes (the defect this fixes) and a clean single bubble throughout;
the "Resultat" tab itself was confirmed correct on an undisturbed sibling turn, though the exact
regenerated turn's tab check was cut short by an unrelated live Azure/Anthropic backend retry plus
the verifier's own navigation away — **not a re-opened bug, just an incomplete final observation**.

**Continue-after-stop ("Fortsett") — done and live-verified.** A new `continueGeneration` seeds the
new turn with the stopped turn's partial text and sends a model-facing (never transcript-visible)
continuation instruction rather than the regenerate-dissatisfaction signal. Live check: stopped a
long answer mid-sentence, clicked Fortsett, and the text resumed exactly at that point with no
repeated or dropped words.

**Math (KaTeX) + Mermaid — done and live-verified**, including a real bug the live check itself
caught: the first `MermaidDiagram` implementation used Solid 1's single-callback `createEffect(fn)`
form; Solid 2 (`solid-js@2.0.0-rc.0`, what this project actually runs) requires the two-phase
`createEffect(compute, effect)` form and throws `[MISSING_EFFECT_FN]` on the old one — which
crashed the *entire app's* reactive system the moment a mermaid block closed mid-stream. Fixed to
match the two-phase idiom already used elsewhere in the same file. Live-verified: inline and block
math render with correct typography; a currency-digit-guard (`$9 \times 10^{16}$` stays literal
text, by design, since an opening `$` immediately followed by a digit reads as a price) works;
an open, unclosed ` ```mermaid ` fence renders as a plain code block mid-stream (no throw, no
diagram attempt); a closed block renders a real, correctly laid-out SVG flowchart. The pre-existing
nested-fence-length bug this list originally named was already fixed in this codebase by the time
this work started — verified, not re-fixed.

**Per-memory correct/forget — mostly done, one confirmed real bug.** "Glem" (delete) is fully
live-verified against the real backend: armed + confirmed delete → real `DELETE
/api/v1/memory/{id}` → 200 → entry gone from a completely independent reload of Settings → Minne
(14 remained, not 15) — genuine backend deletion, not DOM removal. **"Rediger" (correct) is
broken**: saving an edit returns `405 Method Not Allowed`, reproduced twice on two different
entries in two different conversations. Root-caused precisely, not just "add a rebuild and hope":
an *unauthenticated* PATCH to the same path correctly returns `401` (proving the route exists and
is reachable), while an *authenticated* PATCH returns `405` — consistent with the deployed
`verevon-gateway-rs` binary predating the uncommitted `correct_memory` PATCH route in source (an
auth-checking layer runs before Axum's method dispatch, so a request with no cookie is rejected at
the auth layer first, while a request that clears auth reaches the router and only then discovers
the old binary never registered PATCH for that path). This needs a rebuild+redeploy of the gateway
from current source, not a frontend code fix — tracked as the next action below.

**Bonus, not originally on the list — long-paste-to-attachment.** Implemented with a 4000-character
threshold (chosen to sit inside the "few thousand characters" range both ChatGPT and Claude use).
Live-verified: a 4560-char paste converts to an attachment chip, the composer stays empty, a short
paste is untouched.

**A genuine, separate bug found along the way (not fixed, flagged for follow-up):**
`writePendingChatLaunch` only converts an attachment's `blob:` URL to a persisted `data:` URL when
`type.startsWith('image/')`. Every non-image attachment — any `.txt`/`.pdf`/`.docx` upload, and now
also the new pasted-text attachment — keeps its `blob:` URL, which gets revoked by `onCleanup`
immediately after the composer navigates to `/chat` on submit. Likely a pre-existing bug affecting
every non-image attachment already, not introduced by this pass.

**Verified independently**: `pnpm typecheck` clean; `pnpm build` succeeds (3482 modules, 40s); the
touched-file vitest suite is 122/122 (one file needed a solo retry past the known pre-existing
Windows vitest-worker-timeout flake, not a real failure); live E2E in a real authenticated browser
session covered all five features (four PASS, the one confirmed Rediger FAIL above).

**Scope note for whoever commits this**: `git status` on this app shows 107 changed paths — the
four/five features above account for a clear subset (`ChatMessages.tsx`, `use-chat-controller.ts`,
`chat-media-markdown.tsx`, `DashboardComposer.tsx` + their test files, `memory-client.ts`, gateway
`memory.rs`), but the working tree also carries a large, unrelated marketing-site visual overhaul
(`apps/verevon-web/src/components/home/**`, several new hero videos and mood-board assets) and
other in-flight feature work (Support/Knowledge pages, inbox-ai, a drafts queue) from concurrent
sessions. None of it was touched by this pass; flagged here so it isn't swept into a commit of just
these features without the user's own confirmation it's intentional.

**Next**: rebuild and redeploy the Frontend Plane (frontend bundle + gateway) from current source
so the `correct_memory` PATCH route actually exists in the running binary, then re-verify Rediger
live. Separately, fix the `writePendingChatLaunch` blob-URL bug for non-image attachments.

---
