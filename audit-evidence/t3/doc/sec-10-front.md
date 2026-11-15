# Model Plane and the Verevon chat and agent pages: audit of Claude, Hermes, DeepSeek harness, T3 Code, Manus, Codex, Antigravity, Cursor and the web chat products

*As of 2026-10-14. Scope: the Model Plane (Rust and Go) and the Verevon chat and agent pages in the Frontend Plane, compared with eight installed or open-source reference tools and four public web chat products. Read-only audit; nothing in the product or the reference apps was changed.*

## How to read this document

**Tiers.** Every recommendation sits in one of three tiers.

- **PRIORITY** = do first: gaps that block or visibly undercut the vision, table-stakes capabilities every comparable reference has and Verevon lacks or half has, real safety, security or data-governance risks, and high-leverage unique capabilities with acceptable effort.
- **RECOMMENDED** = clear evidenced value that supports the vision but does not block it, larger efforts that need sequencing, and differentiators worth adopting after the priority set.
- **NICE TO HAVE** = polish, niche, speculative or modest value.

**Provenance tags.** Every factual statement about a reference carries a tag: [REA] reverse-engineered with REA (the reverse-engineering analysis toolset) from the shipped app; [SRC] read in source code or shipped files; [DOC] documentation only, public or internal; [INF] inference, with the basis stated. The four web products (claude.ai, chatgpt.com, perplexity.ai, manus.im) rest on public documentation only, so their claims are [DOC] unless stated otherwise.

**Status vocabulary for "Verevon today".** Absent (nothing in code), Partial (some of the capability exists in code), Stub (a placeholder or dark module exists), Planned (a plan or decision exists, no code). "Shipped" appears only in the strengths and common-ground sections. Effort uses S, M, L and XL. Waves W1 to W5 are the build order set out in section 9. "OD" numbers refer to the open decisions in the chat gap-closure plan (CHAT_GAP_CLOSURE_PLAN_2026-10-01.md). Ask, Plan and Do are Verevon's answer, planning and acting chat modes.

**Ids and anchors.** Recommendations are numbered F-t3-001 to F-t3-098, ordered by tier and rank. Each has an anchor with the same name, so [F-t3-001](#F-t3-001) jumps to its card. Prevalence is written "n of N references", where N is normally 12, the number of reference sources in this audit; categories that could be assessed on fewer sources say so (11 for multimodal and research, 9 for coding). Numbering follows the pre-verification tier order, so F-t3-016 keeps its number after moving to Recommended.

**Verification.** Every "Verevon today" status, gap description and effort was verified by reading the Verevon code at commit afb83928 (2026-10-10). Every Priority item was also re-checked on the reference side by a second, independent pass that re-opened the cited evidence and tried to refute it. Where verification changed a claim, the verified version is the one printed here; Appendix C lists the corrections and Appendix D the tier and effort changes.

<!-- TOC -->

## 1. Executive summary

### Goal

Verevon's chat and agent pages aim to be one web product that combines what manus.im, perplexity.ai, chatgpt.com and claude.ai offer: chat, code, images, research, computer use and connected apps, all driven by the Model Plane. This audit compares that target against the Claude desktop app (the overall reference to be re-created in Rust), Hermes Agent (memory and self-tailoring), the DeepSeek harness (transparency and customisation), T3 Code (clean multi-model UI), Manus (computer control), Codex (plugins and ChatGPT-model capabilities), Antigravity (Gemini capabilities) and Cursor (model harnessing, not IDE features), plus the four web products. It answers which capabilities every reference shares, who implements each best, what each does uniquely well, and where Verevon stands. The output is 98 recommendations, tiered, sequenced into waves and tied to the architecture decisions they need.

### Where Verevon stands today

The code-verified baseline holds 500 capability records across 13 categories: 290 shipped, 122 partial, 57 absent, 9 stub and 22 planned [SRC]. The picture is uneven. The strongest categories are Transparency, observability and control (26 of 33 shipped, 2 absent), Chat experience (56 of 77 shipped), Safety, permissions and governance (25 of 37 shipped, nothing absent) and Research and web (22 of 34 shipped, nothing absent). Verevon already matches the references on the chat substrate: streaming, rendering, web search with citations, context compaction, subagents, typed contracts and identity. Its signed fail-closed capability gate, autonomy ladder, Zero Data Retention (ZDR) design and credential isolation are stronger than any reference.

The weakest categories are Computer and environment control (11 of 31 shipped, 9 absent), Coding capability (4 of 10 shipped), Multimodal and creation (18 of 41 shipped, 16 partial) and Models and routing (25 of 41 shipped, 10 absent). Agent runtime is the largest category in the reference data (420 records) and the most unfinished in practice: 43 of 77 shipped but 22 partial, with the gaps sitting in the loop itself. A recurring pattern across categories is "built, but dark": finished or nearly finished code that is not deployed, not wired or not proven.

### Headline findings

1. **The agent loop is the product, and it is not finished.** Plain chat is close to parity, but Do runs start without their instruction stack, history and memory, cannot resume in place after a pause, cannot ask the user a typed question and cannot run on a schedule with tools. See [F-t3-001](#F-t3-001), [F-t3-002](#F-t3-002), [F-t3-011](#F-t3-011), [F-t3-018](#F-t3-018).
2. **A large share of the gaps are already built but dark.** The audit outbox fix is built and not deployed, the ZDR design is complete pending one attestation, MCP (Model Context Protocol) calls are refused by the gate and connector reads are approval-gated as if they were writes, browser grants are placeholders and the OpenAI cache order is built and off. Finishing beats building. See [F-t3-004](#F-t3-004), [F-t3-005](#F-t3-005), [F-t3-010](#F-t3-010), [F-t3-003](#F-t3-003), [F-t3-008](#F-t3-008), [F-t3-022](#F-t3-022).
3. **Governed autonomy is the differentiator, but the declarative layer users and admins see is missing.** Declared tool side-effect classes, scoped approvals with cause codes, named modes and an org policy registry, stored consent for unattended runs and per-site browser policy are what the references show in parts (driving approvals from a declared side-effect class is a Verevon synthesis) and Verevon lacks. See [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006), [F-t3-032](#F-t3-032), [F-t3-018](#F-t3-018).
4. **The conversation data model is thinner than in the leading references.** Message versions and forks, full-text search, agent recall of past chats, paging, projects on top of Spaces and sharing are partial or absent. See [F-t3-012](#F-t3-012), [F-t3-013](#F-t3-013), [F-t3-046](#F-t3-046).
5. **The path to "acts like a PC" runs in steps.** A governed agent browser first, then a per-thread sandbox computer behind a backend seam, then computer use inside that sandbox; user-desktop control stays parked. See [F-t3-008](#F-t3-008), [F-t3-009](#F-t3-009), [F-t3-035](#F-t3-035), [F-t3-024](#F-t3-024).
6. **Verevon learns already, but without review, scope or undo.** Manus stages learned skills as proposals the user accepts or rejects, and Hermes stages unattended memory edits (its skill approval gate is opt-in and off by default); Verevon's loop writes org-matchable skills while the promotion gate is deliberately unbuilt. Governance first, then tailoring. See [F-t3-007](#F-t3-007), [F-t3-020](#F-t3-020), [F-t3-061](#F-t3-061).
7. **Deliverables, not only answers.** Verified Office files through executable SKILL.md bundles, decks, an editable canvas and research cited to passages are where the web products and Claude desktop set the bar (Office skills are table-stakes; decks, the canvas and passage-level citations are common or differentiators). See [F-t3-014](#F-t3-014), [F-t3-038](#F-t3-038), [F-t3-031](#F-t3-031), [F-t3-053](#F-t3-053).
8. **Nobody owns model entitlements.** model-gateway refuses the OrgPolicy allowed_models field because no service owns it; Control Plane should decide and Model Plane enforce. See [F-t3-023](#F-t3-023), [F-t3-065](#F-t3-065).
9. **Verevon is honest by contract but not yet visible.** Live cost with a pre-run estimate, typed lifecycle events, readable long runs and a trajectory ledger are where DeepSeek, Manus and Antigravity lead. See [F-t3-028](#F-t3-028), [F-t3-049](#F-t3-049), [F-t3-048](#F-t3-048), [F-t3-034](#F-t3-034).
10. **Open standards keep Verevon in the ecosystem the references share.** SKILL.md, MCP that executes, MCP Apps, plugins with supply-chain pins and an outward MCP endpoint. See [F-t3-014](#F-t3-014), [F-t3-045](#F-t3-045), [F-t3-059](#F-t3-059), [F-t3-089](#F-t3-089).

### Priority recommendations

| ID | Title | Verevon today | Effort | Plane | Wave |
|---|---|---|---|---|---|
| [F-t3-001](#F-t3-001) | One server-assembled run context for Do, Plan and scheduled runs: instruction stack, thread history, persona, memory and run notes | Partial | M | Model Plane | W1 |
| [F-t3-002](#F-t3-002) | Durable agent loop: checkpoint every round and pause, resume in place after approval, answer or crash, close dangling tool calls, effect ledger | Partial | L | Model Plane | W1 |
| [F-t3-003](#F-t3-003) | Declared tool side-effect classes, a never-auto set and per-tool allow/ask/never policy, so MCP tools become callable through the signed gate | Partial | L | Model Plane | W1 |
| [F-t3-004](#F-t3-004) | Deliver the AI-action audit trail, then make it tamper-evident with a compliance query API and SIEM export | Partial | L | Model Plane, Control Plane | W1 |
| [F-t3-005](#F-t3-005) | Unblock Zero Data Retention: attest one provider and run the live proof | Partial | M | Model Plane, Control Plane | W1 |
| [F-t3-006](#F-t3-006) | Approvals v2: scoped decisions, typed cause codes, decision provenance, denial memory and step-up for high-risk effects | Partial | L | Model Plane, Frontend Plane | W2 |
| [F-t3-007](#F-t3-007) | Govern and prove the skill-learning loop: learned skills start as owner-scoped proposals, a seeded eval proves the loop, and a learning ledger supports undo | Partial | L | Model Plane, Control Plane, Frontend Plane | W1 |
| [F-t3-008](#F-t3-008) | Agent browser in chat and Do mode on real broker grants, with per-site read/act consent, admin allow/block lists and model-blind logins | Partial | XL | Model Plane, Ingestion Plane, Control Plane | W2 |
| [F-t3-009](#F-t3-009) | Per-thread sandbox computer behind a SandboxBackend seam: microVM backend, persistent thread workspace, typed executor protocol and visible lifecycle | Partial | XL | Model Plane, Control Plane | W2 |
| [F-t3-010](#F-t3-010) | Connected apps that work in chat: prove Gmail, Outlook, Drive and Calendar reads end to end, add Ask-mode reads, @-connector scoping, citations, then approved writes | Partial | L | Model Plane, Ingestion Plane, Control Plane | W2 |
| [F-t3-011](#F-t3-011) | Finish human-in-the-loop: plan approve-then-continue, a typed ask_user tool, and docked approval, plan, question and queue trays | Partial | L | Model Plane, Frontend Plane | W2 |
| [F-t3-012](#F-t3-012) | Conversation history foundation in session-core: durable message tree with versions and fork, full-text search, a recall tool, paging and thread organisation | Partial | L | Model Plane, Frontend Plane | W2 |
| [F-t3-013](#F-t3-013) | Make Spaces the chat project: instructions, knowledge, members and a retrieval indicator in Ask and Do | Partial | L | Application Plane, Model Plane, Data Plane | W2 |
| [F-t3-014](#F-t3-014) | Verified Office deliverables through executable SKILL.md bundles: docx, xlsx, pptx and pdf skills with structural checks, render-and-look and an in-panel previewer | Partial | XL | Model Plane, Frontend Plane | W2 |
| [F-t3-015](#F-t3-015) | Attention model for long-running agents: closed run-state vocabulary, attention-ordered work list, typed alerts with Web Push, a live shell stream and an installable PWA | Partial | XL | Model Plane, Application Plane, Frontend Plane | W2 |
| [F-t3-017](#F-t3-017) | Attachments v2: fail closed on unsupported media, durable reference-based uploads, and sandbox analysis of the user's files | Partial | L | Model Plane, Data Plane, Frontend Plane | W1 |
| [F-t3-018](#F-t3-018) | Scheduled runs become governed, tool-using Do runs with a stored approval contract, non-delegable tools, result delivery and origin attribution | Partial | L | Model Plane, Control Plane, Application Plane | W2 |
| [F-t3-019](#F-t3-019) | Real Agents and Automations hubs: versioned agent definitions, named agents, automations with run now and pause, and a working Fleet console | Partial | XL | Model Plane, Application Plane, Frontend Plane | W3 |
| [F-t3-020](#F-t3-020) | User and org memory controls: on, pause, reset and org policy | Partial | L | Model Plane, Control Plane, Frontend Plane | W1 |
| [F-t3-021](#F-t3-021) | Transcript performance: benchmark first, then block-level streaming render, a virtualised timeline and paged transcript reads | Partial | M | Frontend Plane, Model Plane | W1 |
| [F-t3-022](#F-t3-022) | Prompt-cache discipline: measured hit rate, stable/context/volatile prompt tiers with an append-only tail, lineage-scoped cache keys and compaction transactions | Partial | M | Model Plane | W1 |
| [F-t3-023](#F-t3-023) | Org and role model entitlements: Control Plane decides, Model Plane enforces at admission | Absent | L | Control Plane, Model Plane, Frontend Plane | W1 |

### Decisions leadership must make

Ten topics need an ADR (Architecture Decision Record) or an owner ruling before implementation starts, because they touch standing Verevon rules: plane ownership, ZDR propagation, Space authority, "the Model proposes and Quarry executes" and "no LLM on the permission hot path".

1. Sandbox computer backend and thread-scoped workspace: microVM per thread, snapshot retention and destruction, cost metering ([F-t3-009](#F-t3-009), [F-t3-024](#F-t3-024), [F-t3-035](#F-t3-035), [F-t3-043](#F-t3-043)).
2. Computer-use scope: desktop control only inside Verevon's sandbox VM; user-desktop control stays parked ([F-t3-024](#F-t3-024), [F-t3-026](#F-t3-026)).
3. Spaces as the chat project container, instruction precedence and multi-author authority, open decision OD19 ([F-t3-013](#F-t3-013), [F-t3-001](#F-t3-001), [F-t3-060](#F-t3-060), [F-t3-012](#F-t3-012)).
4. Agent-browser domain policy ownership and model-blind credentials, OD13 ([F-t3-008](#F-t3-008), [F-t3-026](#F-t3-026), [F-t3-050](#F-t3-050)).
5. Unattended authority for scheduled, goal and trigger runs: stored consent with expiry and renewable delegated credentials, owner ruling 2026-09-28, OD8, D10 ([F-t3-018](#F-t3-018), [F-t3-037](#F-t3-037), [F-t3-047](#F-t3-047), [F-t3-002](#F-t3-002)).
6. Share model, public links and artifact hosting origin under ZDR and residency ([F-t3-046](#F-t3-046), [F-t3-045](#F-t3-045), [F-t3-031](#F-t3-031)).
7. Learned and distributed behaviour: a three-tier skill library (vendor, org, user), the promotion gate and the plugin supply chain ([F-t3-007](#F-t3-007), [F-t3-058](#F-t3-058), [F-t3-059](#F-t3-059), [F-t3-014](#F-t3-014)).
8. Model entitlements and provider credential ownership: allowed_models has no owner and bring-your-own keys need a custodian ([F-t3-023](#F-t3-023), [F-t3-065](#F-t3-065), [F-t3-042](#F-t3-042)).
9. LLM reviewers on the permission path: only a tighten-only design (deny or escalate, never grant) could be compatible with the standing rule ([F-t3-097](#F-t3-097)).
10. Connector content in shared Spaces and ZDR propagation for connector and web reads ([F-t3-010](#F-t3-010), [F-t3-056](#F-t3-056), [F-t3-027](#F-t3-027), [F-t3-055](#F-t3-055)).

The open questions with the widest effect: which provider surface is attested first for ZDR and when Z-40 closes commercially; self-hosted microVMs or an attested managed provider for the sandbox computer, scoped to the thread, the Space or both; whether user-desktop control is permanently out of scope or only parked; how delegated credentials are renewed for runs that pause for hours (OD8); whether Ask mode may read connected apps (F12); whether Vertex AI in an EU region meets the residency and ZDR bar, which gates the Gemini family ([F-t3-041](#F-t3-041)); when Do mode is enabled on the product route (OD2); and when the Channel Plane moves from docs-only to build.

### What changed during verification

Verification covered all 98 recommendations. Before it, the tiers held 23 Priority, 45 Recommended and 30 Nice-to-have items; after it, 22, 46 and 30. The one tier change is [F-t3-016](#F-t3-016), moved from Priority to Recommended because the Redis buffer, drain and durable receipts already exist and the item is a scale-readiness hardening rather than a precondition for durable runs. Verdicts were keep for 13 items, reword for 81 and demote for 4; no item was dropped as already built and none remains unverified or low-confidence. Effort was revised on 40 items, mostly upward, because the code showed bundles that span several planes ([F-t3-008](#F-t3-008), [F-t3-014](#F-t3-014) and [F-t3-037](#F-t3-037) went to XL) and sometimes downward where parts already exist ([F-t3-026](#F-t3-026), [F-t3-044](#F-t3-044), [F-t3-066](#F-t3-066)). The verified Verevon status across the 98 items is 78 Partial, 16 Absent, 2 Stub and 2 Planned. On the reference side, 23 Priority items were re-checked: 2 confirmed outright and 21 partially confirmed with corrections, and 14 items had a wrong prevalence count in the first draft. The most consequential corrections: no reference shows a crash-durable approval pause, so durable resume is a Verevon synthesis built on DeepSeek checkpoints; Manus's side-effect taxonomy drives replay deduplication, not approvals; and Claude's hash-chained audit log is a local per-session file, not a hosted compliance store.
