# Verevon Space: final recommendations (Audit 1, task T1)

Date: 2026-10-14. Inputs: 12 category consolidations (144 items, `cons/*.items.json`), 12 comparison summaries (`cmp/*.summary.md`), the table-stakes, differentiator and strength extracts (`final/_inputs.*.json`), and the Verevon code baselines (`v-space-*.baseline.md`). The machine-readable result is `final/items.json`.

Provenance tags: **SRC** = read in source or shipped files; **REA** = reverse engineering of a shipped app; **DOC** = documentation; **INF** = inference. Evidence quality differs by source and matters when reading this report:
- buzz and QM evidence is mostly SRC (their repositories).
- Grok Bot is SRC from shipped (often minified) bundles, sometimes medium confidence.
- Slack is mostly DOC (help centre and API docs) plus some SRC from the desktop shell.
- Teams is mostly REA (locale strings and chunks) plus DOC. It shows that a feature exists, not how deep it goes.
- Verevon status comes from code-verified baselines (SRC) at commit afb83928 (2026-10-10). "Shipped" means present in source, not deployed. The release gates are still open, and dev has no `space_effect_policies` rows, so shared features are largely unverified live.

## 1. What changed in calibration

The 12 category leads proposed 54 priority, 69 recommended and 21 nice-to-have items. Seen across all categories, many of those were one capability described from different planes: the same push stream, the same inbox, the same composer. They were also too many to be a "do first" list.

Result: **96 final items**. 24 priority, 45 recommended, 27 nice-to-have, plus 1 dropped.
- All 144 source items are covered exactly once, checked with node.
- 41 items keep `needsVerification: true`.
- 17 items are quick wins.

**Main merges.** Every merged item keeps its source item ids and cluster ids.
- Human posting, shared threads and the thread panel (01-01 + 12-01) became **F-t1-001**.
- The realtime substrate (09-01, 09-02, 09-03) became **F-t1-003**.
- Visible agent work (07-02, 09-04, 12-03) became **F-t1-004**.
- Cross-room attention (05-02, 05-03, 09-05, 12-02) became **F-t1-009**.
- Notifications and their controls (05-01, 05-04) became **F-t1-008**.
- Attachments and the file library (01-04, 08-03) became **F-t1-013**.
- Edit/delete and reactions (01-05, 01-06) became **F-t1-017**.
- Search (04-01, 04-02) became **F-t1-018**.
- Personal agents and the agent access list (07-04, 02-03) became **F-t1-011**.
- Audience-safe memory (10-02, 07-08) became **F-t1-012**.
- Governance admin, admin policy and flags (02-05, 10-09, 11-07) became **F-t1-006**.
- SSO/SCIM (10-04, 02-09) became **F-t1-023**.
- The agent browser and computer view (07-06, 12-08) became **F-t1-020**.
- The command layer (04-04, 11-05, 12-07, 06-05) became **F-t1-041**.
- The composer (01-08, 12-05, 08-08) became **F-t1-032**.
- The timeline (09-07, 12-06) became **F-t1-034**.
- Permalinks (01-11, 11-03) became **F-t1-039**.
- PWA and push (11-01, 05-05, 05-08) became **F-t1-035**.
- Automations (06-06, 07-10, 03-08) became **F-t1-036**.
- The tool cockpit (06-02, 06-08) became **F-t1-037**.

**Main demotions out of priority, with reasons:**
- **Multi-agent rooms (F-t1-025) and the persistent agent computer (F-t1-028).** Both are XL. They depend on addressing, cards, cost caps and confinement. The room browser (F-t1-020) delivers the "agent with its own browser" part of the vision sooner.
- **Calls (F-t1-029) and meeting intelligence (F-t1-050).** These need a product decision first, because huddles are a documented non-goal. They also need a bought SFU and a push substrate.
- **Typed cards and forms (F-t1-026) and the tool cockpit (F-t1-037).** These are enablers that follow approvals.
- **PWA and push (F-t1-035).** In-app notifications and Home come first.
- **Moderation (F-t1-043).** Rooms are internal until guests arrive.
- **Org audit trail (F-t1-031).** This stays recommended, but it carries a W1 verification step: the audit read path may let any member read all org events. If that is confirmed, fix it as a bug immediately.

**Promotions:**
- Room content hardening (F-t1-030, CSP and the silent 403) was raised to significance 4 and made a W1 quick win. Basis (INF): agent- or member-authored markdown can load images from any https host, which is a known exfiltration channel for prompt-injected agents.
- Client resilience (F-t1-024) and room administration (F-t1-022) stay priority because users notice these at once and they are cheap.

**Dropped (1): R-t1-03-12, agent outbound phone calls.**
- It is outside Space scope.
- The only evidence is single-source and medium confidence (Grok Bot strings).
- Its reusable idea, consequence-stated wording on approval cards, was folded into F-t1-010.

## 2. Final tiers at a glance

### Priority (do first, 24)
| Rank | Id | Item | Wave | Effort | Notes |
|---|---|---|---|---|---|
| 1 | F-t1-001 | Human-first room: human-only posts, shared threads, thread panel in an AuxPane | W1 | L | planned in part (S1.7) |
| 2 | F-t1-002 | Addressed agent wake-up and trigger policy, ambient participation later | W2 | L | planned in part |
| 3 | F-t1-003 | Realtime substrate: event projection, resumable SSE, delivery-time authz | W1 | XL | intent in SPACE_PAGE_AUDIT R-4 |
| 4 | F-t1-004 | Agent work visible to the whole room (shared run stream, cards, strip, steer) | W2 | XL | planned in part (S2.5, S4.5) |
| 5 | F-t1-005 | ZDR binding in every plane | W1 | M | quick win |
| 6 | F-t1-006 | Space governance admin (effect-policy UI first) | W1 | L | verify |
| 7 | F-t1-007 | Identity-bound @mentions | W2 | M | quick win |
| 8 | F-t1-008 | Space notifications with levels and follow/mute | W2 | L | |
| 9 | F-t1-009 | Cross-room attention: aggregate stream and Home "needs you" | W2 | L | |
| 10 | F-t1-010 | Approvals with scopes and durable resume, tool policy | W2 | L | verify resume |
| 11 | F-t1-011 | Personal agents, access list and add policy, private assistant | W3 | L | |
| 12 | F-t1-012 | Audience-safe agent context and memory | W3 | L | verify |
| 13 | F-t1-013 | Room attachments and file library | W2 | L | verify |
| 14 | F-t1-014 | Agent artifacts and viewers in the room | W1 | M | quick win, verify |
| 15 | F-t1-015 | Room canvas | W3 | L | planned (S3.1) |
| 16 | F-t1-016 | DMs and group DMs | W2 | L | verify |
| 17 | F-t1-017 | Edit/delete as revisions, reactions, message toolbar | W2 | L | |
| 18 | F-t1-018 | Permission-checked message search | W2 | L | after F-t1-005 |
| 19 | F-t1-019 | Agent self-API on spaces.* actions | W3 | M | quick win |
| 20 | F-t1-020 | Room-scoped agent browser with live view and takeover | W3 | L | planned (browser_run.rs note) |
| 21 | F-t1-021 | Agent computer confinement (real egress, caps) | W2 | L | verify |
| 22 | F-t1-022 | Room administration basics and org-room roster sync | W1 | M | quick win |
| 23 | F-t1-023 | SSO GA and real SCIM with agent suspension | W2 | L | verify |
| 24 | F-t1-024 | Client resilience: error boundaries, version gating | W1 | S | quick win |

### Recommended (45)
These items have evidenced value but do not block the vision. Ranked:
1. F-t1-025 multi-agent rooms
2. F-t1-026 typed cards and request_input/secret drop
3. F-t1-027 agent operations (cost, budgets, evals)
4. F-t1-028 persistent per-agent computer
5. F-t1-029 room calls on a bought SFU
6. F-t1-030 room content hardening
7. F-t1-031 org audit trail
8. F-t1-032 composer for real work
9. F-t1-033 reliable send and client store
10. F-t1-034 scalable timeline
11. F-t1-035 PWA and content-free push
12. F-t1-036 room automations
13. F-t1-037 tool cockpit
14. F-t1-038 agent history search and cited answers
15. F-t1-039 permalinks and entity router
16. F-t1-040 per-thread run queue
17. F-t1-041 command layer
18. F-t1-042 read state, presence and reminders
19. F-t1-043 moderation
20. F-t1-044 credential custody
21. F-t1-045 runtime guardrails
22. F-t1-046 files as agent input and knowledge
23. F-t1-047 webhooks that wake agents
24. F-t1-048 room directory and self-join
25. F-t1-049 people and agent directory
26. F-t1-050 meeting intelligence with consent
27. F-t1-051 retention and shared-Space deletion
28. F-t1-052 typed entries, system messages and link previews
29. F-t1-053 guests and link invites
30. F-t1-054 teams and sub-groups
31. F-t1-055 sidebar organisation
32. F-t1-056 agent task tracker
33. F-t1-057 artifact sharing
34. F-t1-058 typed records and Lists
35. F-t1-059 DND and quiet hours
36. F-t1-060 delivery ledger and digest
37. F-t1-061 catch me up
38. F-t1-062 UI quality baseline
39. F-t1-063 agent avatar and hover card
40. F-t1-064 room templates and case Spaces
41. F-t1-065 governed skills and plugins
42. F-t1-066 agents attend external meetings
43. F-t1-067 duplex voice
44. F-t1-068 screen sharing
45. F-t1-069 search operators

### Nice to have (27)
These are polish, long-tail or speculative items, all in W5:
- F-t1-070 header extras
- F-t1-071 agent templates and catalogue
- F-t1-072 team layer and promotion path
- F-t1-073 Tauri desktop shell
- F-t1-074 provenance footer
- F-t1-075 agent external channels
- F-t1-076 public API
- F-t1-077 compliance export and holds
- F-t1-078 latency observability
- F-t1-079 report flow
- F-t1-080 ZDR-safe client persistence
- F-t1-081 agent long tail
- F-t1-082 messaging long tail
- F-t1-083 federated search
- F-t1-084 recording
- F-t1-085 live meeting AI
- F-t1-086 call polish
- F-t1-087 snapshot share
- F-t1-088 app publish lifecycle
- F-t1-089 multi-window
- F-t1-090 shell conveniences
- F-t1-091 realtime ops backlog
- F-t1-092 delivery health
- F-t1-093 multi-org
- F-t1-094 assurance long tail
- F-t1-095 long-tail UX
- F-t1-096 reverse channel to the user's machine

## 3. Common ground: what every reference shares

The table-stakes extract lists 72 capabilities that at least 3 of the 5 sources share. Here they are condensed into what a Space must do to feel like Teams, Slack or buzz with agents inside.

**Human chat core** (buzz, Slack, Teams, plus Grok Bot and QM for most):
- rooms where people post without invoking a model
- threads that any member can continue
- identity-bound mentions
- reactions
- edit and delete
- attachments
- DMs
- a rich composer with drafts
- per-thread read state

**Attention:**
- a typed event-to-notification pipeline
- unread counts across rooms
- a personal cross-room inbox
- per-conversation notification levels

**Discovery:**
- permission-aware message search
- scoped in-room search
- a room directory with self-join
- a people directory

**Live transport:** a push channel to every member, with a polling safety net. All five sources have it.

**Agents in the room:**
- agents as labelled members (shipped in Verevon)
- in-room activity, plan and tool cards (5 of 5)
- approval cards with scopes and durable resume
- audience-safe disclosure
- scheduled and event-triggered work
- per-agent and per-room cost

**Artifacts:**
- a shared room canvas
- a room file library with an owning store
- artifact sharing with roles

**Org and governance:**
- role change
- guests
- SSO and SCIM
- an admin console
- server-side tenant binding (shipped in Verevon)

**Shell:**
- entity deep links
- a command palette and shortcuts
- feature flags and rings
- a native desktop shell with protocol links and auto-update (all five sources, but Verevon is web-first)

**Calls and meetings** (Teams, Slack, buzz only):
- room calls
- transcription with an AI recap

### Verevon against the common ground

**Already shipped. Keep these and do not regress them:**
- Agents as first-class, labelled room members (C-07-001).
- Server-resolved tenant binding (C-10-001).
- Server-managed chat posture with ZDR on top (C-09-037).
- Accessibility of the Space surface (C-11-022).
- Empty and zero states that tell "empty" apart from "unavailable" (C-12-015).

**Half there (partial).** These are the cheapest table stakes to close:

| Capability | What exists | Final item |
|---|---|---|
| Human-first rooms and shared threads | A Session Core append with Space decision fields exists, but it is owner-only and every post calls the model | F-t1-001 |
| Mentions | @agent is re-resolved server-side; @person is text only | F-t1-007 |
| Unread counts | Durable per-member read markers exist; the sidebar dots are browser-local | F-t1-009 |
| Activity inbox | Room-scoped Activity with a Needs-attention lens | F-t1-009 |
| Notification levels | notification-core has per-event by per-channel preferences | F-t1-008 |
| Room roles | Control has roles, but there is no change route | F-t1-022 |
| Scoped search | Client-side title filter only | F-t1-018 |
| Approval cards | Approve/deny only, and resume is disputed | F-t1-010 |
| File library | Read-only Knowledge tab | F-t1-013 |
| SSO and SCIM | SSO behind a global flag; the SCIM stub fakes success | F-t1-023 |
| Deep links | Space and tab links work; ?thread_id is ignored | F-t1-039 |
| MCP registry | Org-level only | F-t1-037 |
| Scheduled agent work | Per-fire reauthorization is shipped; no room UI | F-t1-036 |
| Admin console and flags | Org settings and plan entitlements only | F-t1-006 |
| Command palette | Typed action registry exists; Cmd+K opens web search | F-t1-041 |
| Rich composer, drafts, read state | Plain textarea; room-level marker | F-t1-032, F-t1-042 |
| Calendar | Read action exists in integration-corev2 | F-t1-036 |
| Meeting recap | A text-only Meeting Scribe template | F-t1-050 |
| Live working state | 6 s poll status | F-t1-004 |
| Audience-safe disclosure | Decisions bind audience revision; memory is not bound | F-t1-012 |

**Lacking (absent):**

| Capability | Final item |
|---|---|
| DMs | F-t1-016 |
| Reactions, edit and delete | F-t1-017 |
| Attachments | F-t1-013 |
| Message search | F-t1-018 |
| Push channel | F-t1-003 |
| Activity and tool cards in the room | F-t1-004 |
| Event-to-notification pipeline | F-t1-008 |
| Room canvas | F-t1-015 |
| Room directory and self-join | F-t1-048 |
| Guests | F-t1-053 |
| Per-room cost | F-t1-027 |
| Calls | F-t1-029 |
| Search operators | F-t1-069 |
| Native shell | F-t1-073, after F-t1-035 |
| Local cache | F-t1-080, deliberately low because of ZDR |

**Status unknown, still to verify in code:**

| Capability | Final item |
|---|---|
| Secret and PII redaction | F-t1-045 |
| Telemetry consent | F-t1-005 |
| Reminders | F-t1-042 |
| Notification sounds | F-t1-035 |
| Context compaction | F-t1-012 |

Pluggable harnesses (C-07-025) were judged not to fit: Verevon runs its own Model Plane runtime, and outside harnesses would bypass ZDR and Model Plane authority.

### Best-of-breed table
| Capability | Best source | Why (mechanism) | Provenance | Final item |
|---|---|---|---|---|
| Human-first posting and threads | buzz (+ QM semantics) | A post is a plain append. Agents wake only on mention or subscription. Thread counters are kept atomically at ingest. QM lets any member continue a thread and adds an engagement rule. | SRC | F-t1-001 |
| Addressing and trigger policy | buzz | respond_to and subscribe fields that fail closed, rule triggers, and an address lock in the composer | SRC | F-t1-002 |
| Ambient participation | QM | A cheap judge returns {act, reason}, with standing orders and a stored ledger | SRC | F-t1-002 |
| Mentions | buzz | Recipients are structured ids, not text. Namesakes get qualified labels, and an ambiguous name blocks the send. | SRC | F-t1-007 |
| Edit/delete and reactions | buzz; QM for agent-visible revisions | Owner-of-agent retract. Soft-deletes stay in audit. Revisions are appended for the agent. | SRC | F-t1-017 |
| Attachments | buzz / Slack | Content-hash storage with metadata stripped (buzz). Three-step upload with a malware scan (Slack). | SRC / DOC | F-t1-013 |
| Push channel | Grok Bot | One multiplexed, sequenced stream with cursor_too_old and generation frames | SRC | F-t1-003 |
| Delivery-time authorization | buzz | One fail-closed fan-out chokepoint shared by local and cross-node paths | SRC | F-t1-003 |
| Live run stream for all viewers | QM | AG-UI SSE with offset-keyed deltas and a snapshot for late joiners | SRC | F-t1-004 |
| Activity cards | buzz (render), Slack (stream wire) | Verb-object-outcome cards; start/append/stop with plan states | SRC / DOC | F-t1-004 |
| Cross-room inbox | Teams; Grok Bot for the agent needs-you queue | About 60 typed activity kinds with slices; one queue for approvals, questions and hand-offs | REA / SRC | F-t1-009 |
| Notification pipeline | Slack | One validated payload through DND and display gates, with a trace id | SRC (shell) | F-t1-008 |
| Content-free push | buzz | The push is only a wake signal; the client fetches the content | SRC | F-t1-035 |
| Message search | buzz | A generated tsvector. Sensitive kinds get a NULL vector. Every hit is re-authorized. | SRC | F-t1-018 |
| Search as the asker | Slack (QM for the mechanism) | An action token from the triggering event; results are never stored | DOC / SRC | F-t1-038 |
| Approvals | QM (durable), Grok Bot (UX) | The pause ends generation. Once/session/always scopes. Requester-only decisions. Named approvers. | SRC | F-t1-010 |
| Personal and team agents | Grok Bot | Primary bot, Personal and Team visibility, an access list with roles, clone-to-team | SRC | F-t1-011 |
| Audience-safe memory | QM (mechanism), Teams (UX) | Labels at write, an audience fold at read, a context cut on audience change, a private draft with Share | SRC / REA | F-t1-012 |
| Agent browser and takeover | Grok Bot (product), QM (grant) | Hand-off with an instruction, Take control, bot-wall detection, a scoped browser-model grant | SRC | F-t1-020 |
| Agent computer | Grok Bot (lifecycle), QM (capability tokens, egress proxy) | Hibernate and pre-thaw; durable home snapshots | SRC | F-t1-028, F-t1-021 |
| Multi-agent turn-taking | Grok Bot | Per-member turn request with nonce and deadline; speak/pass/skip/timeout outcomes; loop guard in shadow mode | SRC | F-t1-025 |
| Agent self-API | QM (+ buzz CLI) | A discoverable API catalogue with run-bound capability tokens | SRC | F-t1-019 |
| Forms and secret drop | QM | A one-time form whose value goes to the keychain; the transcript records only that it was supplied | SRC | F-t1-026 |
| Room canvas | Slack (product), buzz (revisions) | Rich blocks and comments; append-only revisions with a server-side expected-head check | DOC / SRC | F-t1-015 |
| Agent artifacts in the room | Grok Bot | Analyses are routed into a canvas; lazy viewers for PDF, DOCX, XLSX and Mermaid | SRC | F-t1-014 |
| Room roles and lifecycle | buzz | Pure guard functions, a last-owner rule, archive and TTL rooms | SRC | F-t1-022 |
| Admin policy | Teams | Precedence direct > group > global, policy packages, locked settings | DOC | F-t1-006 |
| SSO and SCIM | Teams / Slack | Admin centre federation and IdP-synced groups | DOC | F-t1-023 |
| Moderation | buzz | Private reports to a queue, signed actions, tombstones, a ban that also stops the person's agents | SRC | F-t1-043 |
| Retention | Slack | Org, workspace, channel and DM rules with an org lock | DOC | F-t1-051 |
| Calls | Slack (product), Grok Bot (client state machine), Teams (engine) | The huddle belongs to the room; explicit call phases | DOC / SRC | F-t1-029 |
| Meeting recap and consent | Teams | Notes, tasks and chapters with citations; AI on/off, privacy mode, admin-locked settings | REA | F-t1-050 |
| Command palette | Grok Bot (registry), QM (palette) | Every action is a registered command with an id and hotkey | SRC | F-t1-041 |
| Deep links | Teams (router), QM (?seq= paging) | One (entity, action) command path for toasts and links | REA / SRC | F-t1-039 |
| Timeline windows | buzz | Server windows with reply summaries and exact composite cursors | SRC | F-t1-034 |
| Error recovery | Teams | A diagnosis-specific error curtain | SRC | F-t1-024 |

## 4. Differentials by source

### buzz (Slack-like plus agents; mostly SRC)
- **Agents are plain room members with an explicit wake policy.**
  - What buzz does: a binding has respond_to and subscribe fields, rule triggers and an address lock. Mentioning an agent you own that is not in the room offers to invite it.
  - What Verevon gains: rooms where people talk and agents answer only when addressed (F-t1-002, F-t1-007).
- **Authorization at every delivery.**
  - What buzz does: one fan-out chokepoint, with subscribe-then-register ordering.
  - What Verevon gains: push without a side door around Control (F-t1-003).
- **Owner-of-agent edit and retract, plus moderation that cascades to the banned person's agents.**
  - What Verevon gains: humans stay accountable for their agents (F-t1-017, F-t1-043).
- **Typed room records with prev-revision CAS that never touch unread counts, and room templates with an agent roster.**
  - What Verevon gains: one substrate for Lists, tasks and case rooms (F-t1-058, F-t1-064).
- **Selection-to-context hand-off with untrusted-data framing and a byte-identical pre-send preview.**
  - What Verevon gains: safe "discuss this with an agent" (F-t1-032).
- **Server timeline windows, a tested scroll-anchoring policy, and performance budgets measured while simulated agents flood the room.**
  - Where this lands: F-t1-034, F-t1-078.
- **Room-behaviour eval suites** that grade thread placement, notifications and silence (F-t1-027).
- Caveat: buzz's low-noise notification defaults and its knowledge crystallization are DOC intent and not enforced in its own client.

### Grok Bot (personal agents with their own PC and browser; SRC from bundles)
- **Personal and team agents.**
  - What Grok Bot does: a primary "chief of staff" bot, an access list (owner, manager, collaborator) and publishing to the team.
  - What Verevon gains: the Grok-Bot half of the vision (F-t1-011, F-t1-072).
- **The agent's computer and browser as a product.**
  - What Grok Bot does: hibernate and pre-thaw, a live preview card that expands to an interactive modal, a hand-off request with an instruction, Take control and I'm done, and bot-wall detection.
  - What Verevon gains: an agent that "works like a human with its own PC", governed by Quarry (F-t1-020, F-t1-028).
- **Multi-agent turn protocol** with nonce, deadline, lineage ids, pass and skip outcomes, and loop detection in shadow mode (F-t1-025).
- **Agent needs-you queue** with a reason per item (F-t1-009).
- **Voice front and worker split**, where a fast voice persona delegates through send_task, and **attending Zoom or Meet as a visible guest** with approval-gated chat drafts. Both are medium confidence (F-t1-067, F-t1-066).
- **Send journal with honest delivery states, typed previews with retired-card fallbacks, per-feature minimum client versions, and a coded error registry** (F-t1-033, F-t1-052, F-t1-024, F-t1-079).
- **Cost by flow and by bot with hard limits** (F-t1-027).
- **Zero-retention mode that blocks persistent features** (F-t1-005).

### QM (Teams-like agent harness for companies; SRC)
- **Mechanisms for safe agents in shared rooms:**
  - an audience fold with write-time labels and a context cut when the audience changes (F-t1-012)
  - durable approvals with scopes and tighten-only tool policy (F-t1-010)
  - a credential broker and one-time secret drop (F-t1-044, F-t1-026)
  - an authorising egress proxy with per-turn capability tokens (F-t1-021, F-t1-028)
- **The agent can do what humans can.**
  - What QM does: a discoverable API catalogue with run-bound tokens.
  - Where this lands: F-t1-019.
- **Ambient participation**: a cheap judge, standing orders and a ledger. Explicit room I/O, where silence is allowed (F-t1-002).
- **Live run stream any viewer can join** (AG-UI with offset deltas), a cross-room session-state stream, and a run queue with SKIP LOCKED claims and leases (F-t1-004, F-t1-009, F-t1-040).
- **Automation depth**: crons with destinations, watches, loops, fire logs, recipient consent and pause-while-unread (F-t1-036). Also webhook wake with per-provider verifiers (F-t1-047).
- **Reactions and edits as agent input** (F-t1-017).
- **Attachments re-authorized every turn** (F-t1-046).
- **Conversation kind decides scope** (F-t1-016).
- **Visibility-preserving fork** (F-t1-039).
- **Publish agent-built apps** with immutable versions and pointer-flip rollback (F-t1-088).

### Slack (human chat incumbent; mostly DOC, shell SRC)
- **Huddles as part of the room**, with notes and canvas (F-t1-029).
- **Canvas and Lists as the room's working documents** (F-t1-015, F-t1-058).
- **Search as the asking human.**
  - What Slack does: an action token from the triggering event, per-content scopes, and results that may not be stored. AI answers carry citations.
  - What Verevon gains: the template for F-t1-038, where Verevon's signed decisions are an even stronger token.
- **Notification engineering:**
  - one validated payload, DND with VIP break-through, and toasts with inline reply (F-t1-008, F-t1-059, F-t1-035)
  - "why am I not notified" self-diagnosis (F-t1-092)
- **Granular retention, audit-log export to a SIEM, and posting permissions** (F-t1-051, F-t1-031, F-t1-006).
- **Operator grammar and keyboard model**: quick switcher, unread navigation, F6 region cycling (F-t1-069, F-t1-041).

### Teams (enterprise incumbent; mostly REA and DOC)
- **Meeting AI governance**: per-meeting AI on/off, a privacy mode that blocks every agent, ephemeral answers, admin-locked settings, and a recap visible only to people who can see the transcript (F-t1-050).
- **A private assistant beside the room**, with "Allow to send to the group" (F-t1-011, F-t1-012).
- **An explicit per-room knowledge list**, where removing a source deletes what was collected from it (F-t1-012).
- **About 87 typed system notices**, including agent added or removed (F-t1-052).
- **Live components**: the message holds a pointer and the component keeps its own ACL (F-t1-052).
- **Policy administration** with precedence, packages and admin allowlisting of apps, agents and models (F-t1-006).
- **Guest tiers** (F-t1-053).
- **Compliance**: eDiscovery, legal hold and information barriers (F-t1-077).
- **Strict CSP** with Trusted Types and SRI (F-t1-030).
- **A thin native shell** with a versioned RPC bridge, an error curtain and pop-out windows (F-t1-073, F-t1-024, F-t1-089).

## 5. Strengths to protect (do not regress)

Verevon is equal to or ahead of every reference here (SRC unless noted). Each new item must reuse these strengths, not route around them.

1. **Signed authority.** Ed25519-signed, two-minute Control decisions bind subject, org, Space, audience revision, privacy policy, action, payload digest and idempotency key. Effect policies deny by default. This is stronger than QM's roster version, buzz's membership cache, Grok Bot's grant ids and Slack's action token.
   - Every new transport (push, web push, CLI, external channels, webhooks) must go through this one chokepoint (F-t1-003, F-t1-035, F-t1-047, F-t1-075).
2. **Space access never widens an owner-plane ACL.** Effective access is the Space decision AND the audience AND the owner-plane resource grant. Audience snapshots are immutable.
   - Search, file library, canvas, sharing and guests must keep the intersection (F-t1-018, F-t1-013, F-t1-015, F-t1-057, F-t1-053).
3. **Zero-Data-Retention as a fail-closed flag with typed refusals.** ZDR attempts are content-free and async ZDR delivery is refused. No reference has an equivalent.
   - The gap is enforcement inside Session Core (F-t1-005).
   - Any push, cache or offline tier must carry references, not content (F-t1-003, F-t1-035, F-t1-080).
4. **Agents as Control service subjects on the roster.**
   - Joined Control-first.
   - The honest state "access granted, identity not published" is shown.
   - Turn attribution comes from the verified caller only.
   - Human roster edits cannot remove agents.
5. **Binding policy enforced at the gateway**, with safe create-in-room defaults (mention-only, no tools, require confirmation). The layered instruction hierarchy (platform > org > Space > user > agent) is composed on the server. Equal to or ahead of QM.
6. **Per-fire reauthorization of scheduled work** (fresh decision, fire key, template digest) and a Watch primitive bound to one resource and one member. This is stronger than QM; F-t1-036 should build the product on it.
7. **The model-executable admission bar**: an owner-issued receipt, idempotent fencing and forged-call denial proven on both sides. F-t1-019 must open spaces.* actions through it, not around it.
8. **Honest-state UX enforced in code.**
   - The unavailable[] contract tells empty, could-not-load and not-published apart.
   - An unobserved room counts as unknown.
   - A stop recorded on the server is shown differently from a local-only stop.
   - Destructive actions arm in place.
   - Per-plane deletion receipts.
   - Extend this to every new chip, count and card. Also fix the silent 403 (F-t1-030).
9. **One event log with several lenses** in Activity: verb-object-outcome grammar ordered by salience. Approvals are a typed timeline row. This is equal to or ahead of buzz and QM.
10. **Hybrid retrieval and policy-gated grounding.**
    - Retrieval: dense + keyword + graph + rerank with RRF fusion.
    - Grounding: a 403 degrades a turn to ungrounded rather than failing it.
    - Agent memory tools: save_memory and recall_memory.
11. **Plane isolation and the same-origin Rust BFF.** No browser credential, no cross-plane DB access, the Model proposes and Quarry executes. Browser-action risk gates and the sandboxed HTML preview with a no-network CSP are agent-room controls that Slack and Teams do not need.
12. **Infrastructure to reuse rather than rebuild:**
    - durable Convex outboxes with leases
    - own-turn SSE resume (seq, Last-Event-ID)
    - durable read markers
    - notification-core's delivery state machine (sent_unconfirmed, unknown)
    - the realtime voice route with ZDR fail-closed
    - the calendar read action
    - the verified provider webhook pipeline in integration-corev2
    - wiki-store's append-only versions with propose-and-approve
    - the /chat artifact panel
    - the atomic CAS promote in the Space workspace
    - the mid-run steer that classifies redirect versus follow-up
    - the typed ActionDescriptor registry
13. **Bilingual UI, a code-keyed error catalogue and Space accessibility** (aria-live activity rows, roving focus, reduced motion). These are ahead of buzz and QM, which are English only.
14. **The six-tab cockpit** (Chat, Work, Knowledge, Activity, Agent, Members) shows more agent working context than Slack or Teams. Keep it as the default when adding tabs (F-t1-070).

## 6. Sequencing: waves, quick wins and existing plans

### Waves
- **W1. Foundations and quick wins.**
  - Items: F-t1-001, 003, 005, 006, 014, 022, 024, 030, 031, 033, 063.
  - Goal: people can post and reply without a model turn, on a substrate that pushes. ZDR binds in Session Core. Admins can switch policies on. Cheap visible fixes land.
  - The two L/XL items here (F-t1-001, F-t1-003) are foundations everything else waits on, so they start first even though they finish late in the wave.
- **W2. A real multi-human room.**
  - Items: F-t1-002, 004, 007, 008, 009, 010, 013, 016, 017, 018, 021, 023, 032, 034, 039, 040, 041, 042, 048, 049, 052, 055, 057, 062.
- **W3. Agents as colleagues.**
  - Items: F-t1-011, 012, 015, 019, 020, 026, 027, 035, 036, 037, 038, 043, 044, 045, 046, 047, 051, 056, 059, 060, 061.
- **W4. Cooperation at scale and enterprise depth.**
  - Items: F-t1-025, 028, 029, 050, 053, 054, 058, 064, 065, 066, 067, 068, 069.
- **W5. Horizon.** All nice-to-have items, built on demand.

**Key dependency chains:**
- **Room features.** F-t1-001 (human append, shared threads) unlocks F-t1-007, F-t1-017, F-t1-018, F-t1-016 and F-t1-052. F-t1-002 (addressing) also builds on it.
- **Live features.** F-t1-003 (substrate) unlocks F-t1-004, F-t1-009, F-t1-034, F-t1-035 and F-t1-029.
- **Search.** F-t1-005 (ZDR in Session Core) must land before F-t1-018 indexes anything.
- **Agent actions.** F-t1-010 (resume) comes before F-t1-019, F-t1-026, F-t1-036 and F-t1-047, because all of them pause and resume runs.
- **Personal agents.** F-t1-012 (memory audience) is needed before F-t1-011 can put a personal agent in a shared room.
- **Agent computer.** F-t1-021 (confinement) and F-t1-027 (cost caps) come before F-t1-028 (persistent computer) and F-t1-025 (multi-agent).
- **Policy.** F-t1-006 (policy rows and admin) is needed to verify anything shared live.

### Quick wins (effort S/M, significance at least 4)
F-t1-005, 007, 014, 019, 022, 024, 030, 031, 033, 034, 041, 047, 055, 056, 057, 059, 061.

These smaller slices inside larger items are also near-free and belong in W1:
- Read the `?thread_id` param that Activity already emits (F-t1-001, F-t1-039).
- Generate the idempotency key in the composer so a retry does not create a second, billed turn (F-t1-033).
- Pass the persisted avatarColor through the gateway (F-t1-063).
- Schedule `syncAllOrganizationRooms` (F-t1-022).
- Make the SCIM stub stop returning fake success (F-t1-023).
- Fix the silent 403 on shared reads at SpaceRoomTimeline.tsx:201 (F-t1-030).
- Wire the existing queue and steer backend into the Space composer (F-t1-032).

### Items already planned in Verevon docs, and whether the plan matches best practice
- **S1.7 shared-thread completion** (F-t1-001, F-t1-039 fork). It matches QM's multi-author semantics. It misses buzz's core point: a human-only post with no model turn. Add that to S1.7.
- **Offer-to-add in space-defenition.md, and S5.2 standing orders** (F-t1-002). Both match buzz and QM. Standing orders need a Control-owned, versioned policy floor with a kill switch.
- **SPACE_PAGE_AUDIT R-4 delivery/projection outbox and the SpacePage poll comment** (F-t1-003). Intent only, no code. This matches buzz and Grok Bot only if delivery-time authorization is added; the plan does not mention it.
- **S2.5 Activity integration and S4.5 Work cockpit** (F-t1-004). They match on cancel and steer. The plan has no shared-audience run stream, which is the core gap.
- **S2.7 screening envelope and S3.6 human-only grant mutation** (F-t1-010). They match QM. Scoped grants and descriptor-backed continuation are not yet planned concretely.
- **S3.1 slice 2, Data-owned revisioned Space documents with CAS** (F-t1-015). This matches buzz's model, and the wiki-store backend largely exists.
- **S3.1 split memory ownership** (F-t1-012). It is open and needs the ADR in section 7.
- **browser_run.rs next-step note, S3_3 design §3.5** (F-t1-020). It matches QM's grant model. Grok Bot's hand-off and takeover UX is not in the plan.
- **S3.2/S3.3/S4.2/S4.3 sandbox lease, durable workspace, process registry and Watch** (F-t1-028), plus S3_2_SANDBOX_LEASE_CLOSEOUT (F-t1-021). They match QM. Lifecycle UX (hibernate, pre-thaw) from Grok Bot is not planned. Egress is still advisory.
- **S3.4 credential broker and S3.6 cockpit** (F-t1-037, F-t1-044, F-t1-026). These match QM exactly.
- **S1.5 Space-bound document imports** (F-t1-013). This matches QM's owner-scope model. The current /chat upload must not be reused as is.
- **S4.1, S4.3, S4.4 and S4.5** (F-t1-036, F-t1-060, F-t1-059). These match QM. S4.4 quiet hours match Slack. The delivery ledger exists only as a named gap.
- **S6.1 deletion, export and retention proof** (F-t1-051). It matches buzz's saga. Slack-level per-room retention rules are not planned.
- **UI-2c multi-agent in SPACE_AGENT_SCOPE_PLAN** (F-t1-025). It is narrower than Grok Bot (one hop, same room, human present) and has no pass/skip protocol or loop guard. It is acceptable as a first slice if those are added.
- **Meeting intelligence phases 1 to 4 in the Channel Plane docs** (F-t1-050, F-t1-066). The source-type design is sound. Capture ownership (Channel Plane versus Ingestion) is unresolved.
- **S5.1 principal links ADR adopted, S5.3 blocked** (F-t1-023, F-t1-075, F-t1-093). This matches QM. It is blocked on a Channel Plane that is docs-only.
- **S5.4 governed app publish** (F-t1-088). It matches QM's deployments.
- **Conflict with the brief.** `docs/space-defenition.md` lists huddles as a non-goal and rejects "chat parity". The brief asks for a Teams + Slack + Grok Bot hybrid. The tiers here follow the brief. Calls (F-t1-029) need the product decision recorded first.

## 7. Architecture decisions that need an ADR first

These conflict with, or stretch, plane ownership, ZDR or Space authority. The full list with item ids is under `adrNeeded` in items.json.

1. **Ownership of human room messages and shared-thread append authority** (F-t1-001, 002, 003, 016, 017).
   - Decide which plane is the source of truth for human posts.
   - Add a Control space:post/thread:append decision and a turn_origin field.
   - Define the DM kinds.
2. **Realtime delivery tier under ZDR and Space authority** (F-t1-003, 004, 009, 035, 080).
   - Only references and metadata travel on the wire.
   - Authority is checked at subscribe and at every delivery.
   - Push is a content-free wake.
   - ZDR orgs get no content cache.
3. **Agent memory ownership, audience labels and declassify** (F-t1-012, 011, 072).
4. **Personal agents as service subjects** (F-t1-011, 043, 023).
   - Admission derived from the owner.
   - Access lists.
   - Revocation, ban and deprovisioning cascade to the person's agents.
5. **Multi-agent delegation and whose authority applies** (F-t1-025, 019, 038).
   - A delegate re-resolves its authority through Control.
   - When several humans trigger a run, the narrowest authority applies.
6. **Agent computer network boundary, persistence and local execution** (F-t1-021, 028, 020, 096).
   - Egress goes through Quarry with an audience-intersected allowlist.
   - The durable home has to work with ZDR.
   - The reverse channel to a user's own machine conflicts with "Quarry executes".
7. **Calls and meetings** (F-t1-029, 050, 066, 067).
   - Reverse the huddle non-goal.
   - Buy the SFU.
   - Model a meeting as a sub-resource of a Space rather than a child Space kind (otherwise ADR-0001 needs an amendment).
   - Decide who owns the capture worker.
8. **Retention, legal hold and ZDR precedence** (F-t1-051, 077, 005).
9. **Message search index placement and exclusion rules** (F-t1-018, 038).
10. **External surfaces and guests** (F-t1-075, 053, 093, 023). This covers the Channel Plane, linked identities and restricting agents when external participants are present.

## 8. Cross-cutting themes

1. **From an AI prompt box to a room.** The largest gap is not a missing feature but a model: every post is a model turn, threads are owner-only and agents answer everything.
   - The fix is one human-first entry model plus explicit wake rules (F-t1-001, F-t1-002).
   - Most of the human chat table stakes then hang off it: mentions, reactions, edit, DMs and search.
2. **Make it live, without leaking.** A push substrate is a precondition for most items (F-t1-003). Verevon's differentiator is to do it with delivery-time authorization and refs-only frames under ZDR.
3. **Agents' work must be visible and steerable by the room**, not only by the sender (F-t1-004, F-t1-010, F-t1-020). The UI pieces already exist in /chat and the dashboard; they are not wired into Spaces.
4. **Personal agents need audience-safe memory before they enter shared rooms** (F-t1-011, F-t1-012, F-t1-021). This is where the Grok-Bot vision meets Verevon's authority rules. Verevon's foundation is stronger than any reference's, but memory is the hole.
5. **Attention across rooms**: notifications, a Home "needs you" inbox and unread counts that work for rooms that are not open (F-t1-008, F-t1-009, F-t1-035).
6. **Make shipped governance usable.** Deny-by-default policies without admin tooling, ZDR that Session Core ignores, and a SCIM stub that fakes success are each worse than a missing feature, because they look done (F-t1-005, F-t1-006, F-t1-023).
7. **Reuse before build.** Much of the backlog is wiring what exists: the /chat artifact panel, queue and steer, AG-UI resume, read markers, notification-core preferences, calendar read, the webhook pipeline, wiki versions and the action registry.
8. **Shared artifacts as the room's memory**: canvas, files, Lists and cards on Data-Plane-owned, revisioned objects under one grant authority (F-t1-015, F-t1-013, F-t1-057, F-t1-058).

## 9. Main architecture risks

- **Push tier bypass.** A realtime tier that caches decisions too long, fans out content, or skips the per-delivery check would widen access and break ZDR. This is the highest-impact risk because everything rides on it (F-t1-003).
- **Index and copy sprawl.** Search indexes, notification payloads, previews, digests and caches are all second copies of content. Each needs ZDR exclusion and deletion and retention hooks (F-t1-018, F-t1-008, F-t1-060, F-t1-051).
- **Cross-room leakage through agents.** Personal agents and multi-agent delegation can carry context from a private room to a wider one through memory, compaction or tool results. Until F-t1-012 lands, keep personal agents out of shared rooms (F-t1-011, F-t1-025).
- **Runaway cost and loops.** Multi-agent rooms, ambient participation and unattended triggers multiply model calls. Today Session Core reports 0 tokens and there is only an org-level hard stop, so per-room budgets and loop guards must come first (F-t1-027, F-t1-025, F-t1-002).
- **Sandbox egress is advisory.** Processes that ignore proxy variables, a passthrough fallback when bubblewrap fails, and no resource caps all need fixing before agent computers get network or persistence (F-t1-021, F-t1-028).
- **Approvals that do not resume.** The baseline records conflict on whether a granted approval resumes the run. If it does not, every human-in-the-loop flow is broken (F-t1-010).
- **Plane-ownership drift.** Several items are tempting to build in the gateway or in Convex for speed (human posts, reactions, canvas, meeting records). The ADRs in section 7 must settle ownership first.
- **Stale docs.** Several docs contradict the code. For example, SPACE_PAGE_AUDIT §18 on sidebar dots, and the description of the Application Plane as "realtime sync" when the Spaces UI uses no Convex subscription. Plans must be re-checked against the code.
- **Evidence risk.** Much of the Slack and Teams benchmark is DOC or REA (it shows a feature exists, not how it works). Grok Bot's meeting-bot and phone records are medium confidence. Items that rest mainly on these are marked medium or low confidence.

## 10. Open questions for the team

1. **Product stance.** Does Verevon accept the hybrid brief over the "no chat parity" and "huddles are a non-goal" lines in `space-defenition.md`? This decides F-t1-029, F-t1-050 and F-t1-068.
2. **Message ownership.** Which plane owns human room messages and reactions: Session Core in the Model Plane, or an Application Plane store? Is there one source of truth for a thread that both people and agents write to?
3. **Approval resume.** Does a granted approval resume the paused run today on the live-stream path? The baseline records disagree.
4. **Audit reads.** Can a normal org member read all org audit events through audit-core? Trace this in W1.
5. **Sandbox in production.** Does the execution-core sandbox fail closed in production, or fall back to a passthrough when bubblewrap is unavailable?
6. **Memory ownership.** Who owns agent memory (S3.1)? Must Letta-backed memory be audience-labelled, or excluded from shared rooms?
7. **Multiple triggering humans.** Whose authority applies when one agent turn is triggered by several humans, or when an agent delegates to another?
8. **Default room agent.** Should the always-on default "Verevon" assistant remain once human-only posting exists, or become an explicit binding with mention-only wake?
9. **ZDR orgs.** What can they get in push, previews, search and digests? Is a refs-only experience acceptable to them?
10. **Calls.** Buy or self-host the SFU (LiveKit OSS or similar)? Is a meeting a sub-resource of a Space or a child Space kind?
11. **Channel Plane.** Is it scheduled? External channels, linked identities and cross-org rooms all wait on it.
12. **Desktop shell.** Is a Tauri shell wanted after the PWA, or is the web plus PWA enough?
13. **Unknown statuses.** Which of the 41 items marked for verification are already partly built? Examples: the run queue, secret redaction, telemetry consent, context compaction, and SSE behaviour behind customer proxies. A focused code pass before W2 planning would firm up their tiers.
