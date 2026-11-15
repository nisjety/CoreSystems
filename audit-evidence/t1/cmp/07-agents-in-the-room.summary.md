# T1 / Category 7: Agents in the room (comparison)

Inputs: 358 reference records from 5 sources (buzz 71, grokbot 121, qm 106, slack 25, teams 35) and 42 Verevon baseline records. The records fall into 67 capability clusters. 356 records are assigned and 2 are excluded as trivia (qm-architecture-041 feature flags, qm-architecture-146 config epochs). Full detail: `07-agents-in-the-room.clusters.json`.

## Shared baseline (table-stakes, 4-5 of 5 sources)
- **In-room activity, plan and tool-call cards (C-018, 5/5).** Agent work streams into the room as semantic cards, not just a status line.
- **Agent as a labelled first-class member (C-001).** Each agent has its own identity, an automation marker and a profile.
- **Audience-safe disclosure (C-003).** An agent only uses what the whole audience may see.
- **Approval cards with scopes and durable resume (C-035).**
- **Presence and live working state (C-017).**
- **Scheduled and event-triggered agent work (C-054).**
- **Per-agent and per-room cost attribution (C-061).**
- **Context assembly and compaction (C-029).**
- **Pluggable harnesses (C-025).** The Slack count here comes from DOC records only.

Slack and Teams are mostly DOC/REA evidence of the product surface. Neither ships an agent computer, browser or memory (absence records slack-098, slack-shell-116, teams-109 and teams-110 are kept as negative evidence and are not counted toward prevalence). Prevalence for the "agent with a PC" clusters is therefore at most 3 of 5.

## Best of breed
| Capability | Best source | Why |
|---|---|---|
| Agent identity and owner provenance | Buzz | A signed principal in the same pipeline as people; owner attestation shown on every post (SRC). Teams has the best agent profile page (REA). |
| Authority model / turn origin | QM | A turn-origin enum gates credentials and sharing; human-only walls are pinned by parity tests (SRC). |
| Audience-safe disclosure | QM (mechanism), Teams (UX) | Labels at write time plus an audience fold at read time, and a context cut when the audience changes. Teams adds a private draft with "Allow to send to the group". |
| Personal and team agents | Grok Bot | Primary "chief of staff" bot, Personal and Team visibility with per-person sessions, and personal-to-team carry-over. |
| Addressing and trigger policy | Buzz | Author gate (owner, allowlist, anyone, nobody) that fails closed, rule triggers, and an agent address lock in the composer. |
| Ambient participation | QM | A cheap judge returns {act, reason}, standing orders and a bot ledger; silent by default and auditable. |
| Steer and interrupt | Buzz | Queue, steer, interrupt or owner-only interrupt, with explicit prompt framing (q5). |
| Multi-agent turn-taking | Grok Bot | Per-member turn request with nonce, deadline and lineage ids; outcomes speak, pass, skip or time out; loop detection with a shadow mode. |
| Activity cards | Buzz (render), Slack (wire) | Verb-object-outcome cards over a raw-event floor; Slack supplies the start/append/stop stream with task and plan states. |
| Memory | QM | Per-scope notebook with sensitivity and provenance; one disclosure view; revision history. |
| Approvals | QM (durable), Grok Bot (UX) | The pause ends generation, with once, session or always scopes and requester-only decisions; Grok Bot adds named-approver cards. |
| Secrets | QM | A broker pins host, method and path so the agent never sees the secret; a one-time secret-drop form keeps values out of chat. |
| Agent computer and lifecycle | Grok Bot | Hibernate and wake, pre-thaw on typing, presence-aware parking; QM adds durable home snapshots. |
| Live view and takeover | Grok Bot | Hand-off request with an instruction, Take control, then I'm done (noVNC). |
| Agent browser | QM (grant), Grok Bot (product) | A scoped browser-model grant, so the runner never holds a provider key; Grok Bot detects bot-check walls. |
| Room-behaviour evals | Buzz | Benchmarks that run the real product and grade thread placement, notifications and silence. |

## Differentiators worth carrying over
Grok Bot's turn protocol for multi-agent rooms (C-013). QM's ambient judge and standing orders (C-011). Buzz's steer/interrupt policies (C-012). Re-authorizing every scheduled fire and requiring recipient consent (C-055). Explicit room I/O, where an agent speaks only through a tool and silence is allowed (C-016). The credential broker (C-039). Teams' explicit per-room knowledge list, where removing a source deletes what was collected from it (C-031).

## Verevon's biggest gaps (priority tier)
1. **Every room post invokes the model (C-010, sig 5).** There is no human-only message and no author gate, and only one agent can be addressed per message. This blocks Slack/Teams-style rooms.
2. **No activity, tool or plan cards in the room (C-018, sig 5).** `AgentRunConsole` and `ChatLiveRunPanel` already render these, but only outside Spaces.
3. **No multi-agent rooms or delegation (C-013, sig 5).** UI-2c is blocked on an execution-core to gateway path.
4. **No personal agents (C-004, sig 5).** Personal Spaces cannot bind agents.
5. **Agent browser is not Space-scoped (C-049, sig 5).** `BrowserRunStartRequest` has no Space field. Takeover exists only on the dashboard (C-047).
6. **The agent PC is a narrow slice (C-042, sig 5).** It is credential-free and has no network, and it only covers code_interpreter. There is no lifecycle and no room view.
7. **Approvals have no scopes (C-035, sig 5).** They offer approve/deny only, and the baseline records disagree on whether a granted approval resumes.
8. **No room or team memory scope (C-030, sig 4).** It is blocked on a decision about which plane owns memory.

Recommended next: steer/cancel in the room (C-012), cost per Space (C-061), re-authorizing each schedule fire (C-055), the tool-policy editor and ceilings (C-036), the secrets broker (C-039), and review-then-share disclosure (C-003).

## Verevon strengths (keep)
- **Agents as Control service subjects on the room roster (C-001).** The UI also shows the honest third state "access granted, identity not published".
- **Authority model (C-002).** Control-signed decisions are short-lived and bound to the effect, revocation propagates to the Model Plane, and Space access never widens an owner-plane ACL. This is ahead of QM's act-as-the-human model.
- **Instruction hierarchy (C-032).** Platform > org > Space > user > agent, composed on the server.
- **Binding policy enforced at the gateway.** trigger_modes, approval_mode and allowed_tools are all checked there.
- **Recipient audience snapshots with revisions.** These are a stronger foundation for audience-safe disclosure than any reference app has.

## Caveats
- **28 clusters are "unknown".** No baseline record covers them, mostly Model Plane runtime internals and some room I/O and safety items. A later code check should confirm them.
- **Some statuses come from the baseline narrative, not base records.** Mid-stream queue/steer shipped, per-message model picker, audience-revision filtering.
- **The Slack and Teams evidence is weaker.** Slack is mostly DOC and Teams is REA locale strings plus DOC. For C-034, C-060, C-062 and C-067, confidence is not upgraded.
- **Prevalence counts may be inflated for some clusters.** C-025 counts Slack because Slack documents how to connect harnesses, but Slack does not host a harness itself.
- **Several Buzz and Grok Bot "absence" records are used only as negative evidence.** Buzz auto-approves tools (buzz-architecture-045) and has no agent browser.
- **Effort and significance are judgments (INF).** They are based on the architecture rules and the baseline evidence.
