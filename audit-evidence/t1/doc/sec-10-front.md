# Verevon Space: audit of Teams, Slack, Grok Bot, Buzz and QM

*Audit T1, as of 2026-10-14. Scope: the Verevon Space (a chat room shared by people and agents) compared with Microsoft Teams, Slack, Grok Bot, Buzz and QM. Status: Read-only audit; nothing in the product or the reference apps was changed.*

## How to read this document

**Tiers.** Every recommendation sits in one of three tiers.

- **PRIORITY** = do first: gaps that block or visibly undercut the vision, table-stakes capabilities every comparable reference has and Verevon lacks or half has, real safety, security or data-governance risks, and high-leverage unique capabilities with acceptable effort.
- **RECOMMENDED** = clear evidenced value that supports the vision but does not block it, larger efforts that need sequencing, and differentiators worth adopting after the priority set.
- **NICE TO HAVE** = polish, niche, speculative or modest value.

**Provenance tags.** A tag in square brackets ends every factual sentence or paragraph about a reference app, repo or product. [REA] = observed through the REA reverse-engineering tool on a shipped app. [SRC] = read directly in source code or shipped files (bundles, manifests, strings, locale files). [DOC] = documentation only, public or internal. [INF] = inference; the basis is stated. Where a claim rests only on documentation or inference, the sentence says so.

**Verevon today.** Each item carries a status for Verevon at commit afb83928 (2026-10-10): **Absent** (nothing exists), **Partial** (part of the capability exists in code), **Stub** (an endpoint or placeholder exists but does not do the work), **Planned** (a design or plan document exists, no code). "Shipped" (used in the baseline counts, the common-ground tables and the strengths) means present in source, not deployed; "Unknown" (one common-ground row) means the capability was not verified either way. Every status was verified by reading the Verevon code; verified data supersedes earlier estimates.

**Effort.** S, M, L, XL are relative sizes after verification; they count the work to close the stated gap, not the whole feature area.

**Ids and anchors.** Recommendations are numbered F-t1-001 to F-t1-096. Each heading carries an anchor of the same name, so a link such as [F-t1-003](#F-t1-003) jumps to the item. Verevon claims cite code paths relative to the CoresSystem repository, for example apps/Model Plane/rust/services/session-core/src/grpc.rs:2498.

**Prevalence.** "n of 5 references" counts how many of the five sources show a capability.

**Abbreviations.** SSE = Server-Sent Events; SSRF = server-side request forgery; AG-UI = the agent-UI event stream protocol; MCP = Model Context Protocol; SSO = single sign-on; SCIM = System for Cross-domain Identity Management; SFU = selective forwarding unit (media server for calls); ASR = automatic speech recognition; BFF = backend-for-frontend (the Verevon Rust gateway); NATS = the message bus between planes; HITL = human in the loop; SIEM = security information and event management; TTL = time to live; DM = direct message. Quarry is Verevon's Ingestion Plane crawl and browser executor; Letta is the external agent-memory service that Session Core mirrors memory to. S2.7, S3.1, S3.4 to S3.6, S4.5 and S5.1 are section numbers of the Verevon QM adoption plan (apps/Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md).

<!-- TOC -->

## 1. Executive summary

### 1.1 Goal

A Verevon Space should work like Teams and Slack for people (rooms, groups, direct messages, many people in one conversation) and like Grok Bot for agents (personal agents that work like a colleague with their own computer and browser), inside Verevon's authority model. This audit compares the five references with the Verevon code, finds what they all share (table stakes), which one does each thing best, what sets each apart, and where Verevon is behind or ahead. The result is 96 recommendations in three tiers, five delivery waves, ten architecture decisions that need an ADR (Architecture Decision Record) first, and the open questions leadership must answer.

### 1.2 Where Verevon stands today

The code-verified baseline holds 227 capability records across 12 categories: 102 shipped, 51 partial, 63 absent, 7 stub and 4 planned. [SRC]

The strongest categories are the ones a chat product does not usually lead with. Governance, security and compliance has 12 of 20 records shipped and only 2 absent. UX patterns and layout has 8 of 11 shipped. Agents in the room has 21 of 42 shipped, the largest shipped count anywhere, and org and membership has 16 of 30. Client shell has 5 of 8 shipped, although Verevon is web-only and has no desktop shell. [SRC]

The weakest categories are the human-chat basics. Calls and meetings has nothing shipped: 3 absent, 3 partial, 1 planned. Messaging core has 16 shipped against 14 absent, 8 partial and 1 stub; the absent records include human-only posts, direct messages, reactions, edit and delete, and attachments, and message search is absent under Search and discovery. Collaborative artifacts has 3 of 9 shipped. Notifications and attention has 5 of 17 shipped with 6 absent, and realtime and sync has 7 of 15 shipped with no room-level push. [SRC]

The honest picture: Verevon has an authority-first agent cockpit with stronger governance than any reference, sitting on a conversation model that is still an AI prompt box rather than a room. Signed Control decisions, agents as service subjects on the roster, audience-bound reads and fail-closed Zero-Data-Retention (ZDR) are ahead of all five references. [SRC] What is missing is most of what people expect from Teams or Slack on day one, plus live visibility of agent work for everyone in the room.

### 1.3 Headline findings

1. **Every Space post is a model turn.** The composer always calls streamChat, the gateway has no POST route for a human message, and a non-owner reply is refused twice in Session Core because append authority is checked against the thread owner, although Control already issues a member-capable thread:append decision. [SRC] This is the single largest gap; most human-chat table stakes hang off it ([F-t1-001](#F-t1-001), [F-t1-002](#F-t1-002)).
2. **There is no room-level push.** The Space page polls every 6 seconds while visible, the gateway has no Space stream route, and Session Core's outbox publishes only audit and run-event subjects. [SRC] A sequenced, resumable, authorization-at-delivery substrate is the precondition for live agent work, notifications and calls ([F-t1-003](#F-t1-003)).
3. **Only the sender sees an agent work.** Run events are authorized as run-owner reads and proxied with the caller's own token, so other members get a status row and the approval card; the Work tab is read-only. [SRC] The building blocks (AG-UI stream, step cards, queue and cancel routes, browser live-frame URL) exist outside Spaces and need a room-audience decision ([F-t1-004](#F-t1-004), [F-t1-020](#F-t1-020)).
4. **Approvals resume, but only for three action kinds.** A durable continuation descriptor and delivery worker resume execute_provider_action, book_shipment and tickets.create; shell, browser and MCP tools fail closed, the decide path returns a quarantine placeholder, and there are no once/session/always grant scopes. [SRC] This settles the earlier dispute over resume ([F-t1-010](#F-t1-010)).
5. **Agent memory is not Space-aware.** Session Core extracts memory candidates from every Space post and stores them as user-level memory owned by the thread owner, mirrored to Letta; agent_memory has no space, audience or label column. [SRC] Personal agents must stay out of shared rooms until this is fixed ([F-t1-012](#F-t1-012), [F-t1-011](#F-t1-011)).
6. **The human-chat basics are absent, not partial.** Direct messages, reactions, edit and delete, attachments and message search do not exist in the Space code. [SRC] Reactions and attachments are in 5 of 5 references, and DMs, edit and delete, and message search in 4 of 5; F-t1-013 is rated Partial only because connector imports already carry Space provenance, not because room attachments exist ([F-t1-016](#F-t1-016), [F-t1-017](#F-t1-017), [F-t1-013](#F-t1-013), [F-t1-018](#F-t1-018)).
7. **Shipped governance looks done but is not usable.** The deny-by-default effect policy can only be written through a service-principal PUT and an operator script, with no admin route or UI and plain-string denials; the SCIM (System for Cross-domain Identity Management) endpoint answers success without creating or changing anyone; and any org member can read all org audit events because audit-core never consults the delegated role. [SRC] The audit read gap is confirmed and should be fixed as a bug ([F-t1-006](#F-t1-006), [F-t1-023](#F-t1-023), [F-t1-031](#F-t1-031)).
8. **ZDR is stronger than the first pass assumed.** Session Core already refuses every non-read scope for ZDR callers and gates durable and Letta memory; only the Space decision's own ZDR flag is discarded, and that path is unreachable today. [SRC] The item moved from Priority to Recommended and from M to S ([F-t1-005](#F-t1-005)).
9. **The agent sandbox is confined but uncapped.** Filesystem confinement, seccomp and dropped capabilities are real and fail closed, and no Space agent code path has network at all; what is missing is CPU, memory and pid caps and a per-room egress allowlist through Quarry. [SRC] ([F-t1-021](#F-t1-021))
10. **Attention across rooms does not exist.** Notification-core has preferences and a delivery state machine, but no Space event produces a notification, sidebar unread dots are browser-local, and there is no Home inbox. [SRC] ([F-t1-008](#F-t1-008), [F-t1-009](#F-t1-009))

### 1.4 Priority recommendations (23)

| Id | Title | Verevon today | Effort | Plane | Wave |
|---|---|---|---|---|---|
| [F-t1-001](#F-t1-001) | Human-first room conversation: human-only posts, member replies in shared threads, and a thread side panel in one contextual right pane | Partial | L | Model Plane, Control Plane, Frontend Plane | W1 |
| [F-t1-002](#F-t1-002) | Addressed agent wake-up: several agents per message, per-binding respond_to/subscribe trigger policy, offer-to-add, then optional ambient participation | Partial | L | Frontend Plane, Application Plane, Model Plane, Control Plane | W2 |
| [F-t1-003](#F-t1-003) | Space realtime substrate: typed seq-numbered event projection, resumable per-Space SSE through the gateway, and authorization at subscribe and at every delivery | Absent | XL | Model Plane, Application Plane, Frontend Plane, Control Plane | W1 |
| [F-t1-004](#F-t1-004) | Agent work visible to the whole room: shared live run stream, activity/plan/tool cards, an ambient activity strip, typed transcript rows and steer/cancel | Partial | XL | Model Plane, Frontend Plane | W2 |
| [F-t1-006](#F-t1-006) | Space governance admin: effect-policy admin UI with safe presets and typed denials first, then posting rules, agent/model allowlist, locked switches, autonomy posture and rollout flags | Partial | XL | Control Plane, Frontend Plane, Model Plane | W1 |
| [F-t1-007](#F-t1-007) | Identity-bound @mentions for people and agents with mention records, role-gated group handles, invite-from-mention and ranked @/# pickers | Partial | M | Frontend Plane, Model Plane, Application Plane | W2 |
| [F-t1-008](#F-t1-008) | Space notifications: typed signals for mentions, replies, approvals and agent completions, refs-only payload, rate limits, per-room levels and thread follow/mute | Partial | XL | Application Plane, Model Plane, Control Plane | W2 |
| [F-t1-009](#F-t1-009) | Cross-room attention: a server-side unread/working aggregate stream, then a Home 'needs you' inbox with Agents and Activity destinations | Partial | L | Application Plane, Model Plane, Frontend Plane | W2 |
| [F-t1-010](#F-t1-010) | Approvals with grant scopes and durable resume, tool permission policy with ceilings, and quarantine-and-release | Partial | L | Model Plane, Control Plane, Frontend Plane | W2 |
| [F-t1-011](#F-t1-011) | Personal agents with an owner-controlled access list and add policy, a private assistant beside the room, and configurable room bindings | Partial | XL | Control Plane, Application Plane, Model Plane, Frontend Plane | W3 |
| [F-t1-012](#F-t1-012) | Audience-safe agent context and memory: source labels, audience-bound memory, owner-signed declassify, review-then-share and explicit room/team knowledge scopes | Partial | L | Model Plane, Data Plane, Control Plane | W3 |
| [F-t1-013](#F-t1-013) | Room attachments and file library: paste, drop and attach through presigned Data Plane documents with Space provenance, scan and typed refusals, plus a Files view | Partial | L | Data Plane, Frontend Plane, Control Plane | W2 |
| [F-t1-014](#F-t1-014) | Bring agent-produced artifacts and the shared viewer registry into the Space room | Partial | M | Frontend Plane, Model Plane | W1 |
| [F-t1-015](#F-t1-015) | Room canvas: a shared, revisioned Space document that humans and agents co-author | Absent | L | Data Plane, Control Plane, Frontend Plane, Application Plane, Model Plane | W3 |
| [F-t1-016](#F-t1-016) | Direct messages and group DMs on Space authority, with conversation kind deciding agent scope and history window | Absent | L | Control Plane, Application Plane, Model Plane | W2 |
| [F-t1-017](#F-t1-017) | Message lifecycle and reactions: edit and delete as revision entries (owner-of-agent retract, revisions reach the agent), emoji reactions and a per-message action toolbar | Absent | L | Model Plane, Application Plane, Control Plane, Frontend Plane | W2 |
| [F-t1-018](#F-t1-018) | Permission-checked message search: Session Core index with per-hit re-authorization and ZDR exclusion, in-room scoped search on Ctrl/Cmd+F and open-in-context | Absent | L | Model Plane, Frontend Plane, Control Plane | W2 |
| [F-t1-019](#F-t1-019) | Let room agents call Space actions themselves (agent self-API with run-bound grants) | Partial | L | Model Plane, Control Plane, Frontend Plane | W3 |
| [F-t1-020](#F-t1-020) | Room-scoped agent browser: space_ref on browser runs, a live preview card that expands to an interactive view, hand-off and take-over with a control indicator | Absent | L | Ingestion Plane, Model Plane, Control Plane, Frontend Plane | W3 |
| [F-t1-021](#F-t1-021) | Agent computer confinement: real network enforcement and resource caps, audience-bound egress, one SSRF-safe connector | Partial | M | Model Plane, Ingestion Plane, Control Plane | W2 |
| [F-t1-022](#F-t1-022) | Room administration basics: change and choose member roles with an owner guard, rename, topic, archive and restore, and keep the organization room roster in sync | Partial | L | Control Plane, Application Plane, Frontend Plane | W1 |
| [F-t1-023](#F-t1-023) | Enterprise identity: stop the fake-success SCIM stub, real SCIM 2.0 with deprovisioning that suspends a person's agents, per-org SSO enforcement and domain verification; linked identities later | Stub | XL | Control Plane | W2 |
| [F-t1-024](#F-t1-024) | Client resilience: root and per-region error boundaries, lazy-chunk recovery, a boot watchdog, and a build id with stale-tab reload and contract guard | Absent | M | Frontend Plane | W1 |

### 1.5 Decisions leadership must make

Ten topics need an ADR before the related items start, because they stretch plane ownership, ZDR or Space authority:

1. **Ownership of human room messages and shared-thread append authority.** Which plane is the source of truth for human posts, reuse of the existing member-capable Control thread:append decision, a turn_origin field, DM kinds ([F-t1-001](#F-t1-001), [F-t1-002](#F-t1-002), [F-t1-003](#F-t1-003), [F-t1-016](#F-t1-016), [F-t1-017](#F-t1-017)).
2. **Realtime delivery under ZDR and Space authority.** Refs and metadata only on the wire, authority at subscribe and every delivery, content-free wake push, no content cache for ZDR orgs ([F-t1-003](#F-t1-003), [F-t1-004](#F-t1-004), [F-t1-009](#F-t1-009), [F-t1-035](#F-t1-035), [F-t1-080](#F-t1-080)).
3. **Agent memory ownership, audience labels and declassification** ([F-t1-012](#F-t1-012), [F-t1-011](#F-t1-011), [F-t1-072](#F-t1-072)).
4. **Personal agents as service subjects**: owner-derived admission, access lists, cascade on suspension ([F-t1-011](#F-t1-011), [F-t1-043](#F-t1-043), [F-t1-023](#F-t1-023)).
5. **Multi-agent delegation and whose authority an agent acts under** ([F-t1-025](#F-t1-025), [F-t1-019](#F-t1-019), [F-t1-038](#F-t1-038)).
6. **Agent computer network boundary, persistence and local execution** ([F-t1-021](#F-t1-021), [F-t1-028](#F-t1-028), [F-t1-020](#F-t1-020), [F-t1-096](#F-t1-096)).
7. **Calls and meetings**: reverse the documented huddle non-goal, buy the SFU, choose the meeting model ([F-t1-029](#F-t1-029), [F-t1-050](#F-t1-050), [F-t1-066](#F-t1-066), [F-t1-067](#F-t1-067)).
8. **Retention, legal hold and ZDR precedence** ([F-t1-051](#F-t1-051), [F-t1-077](#F-t1-077), [F-t1-005](#F-t1-005)).
9. **Message search index placement and exclusion rules** ([F-t1-018](#F-t1-018), [F-t1-038](#F-t1-038)).
10. **External surfaces and guests**: Channel Plane, linked identities, external audiences ([F-t1-075](#F-t1-075), [F-t1-053](#F-t1-053), [F-t1-093](#F-t1-093), [F-t1-023](#F-t1-023)).

The most important open questions, beyond those ADRs:

- **Product stance.** Does Verevon accept the hybrid brief over the "no chat parity" and "huddles are a non-goal" lines in docs/space-defenition.md? This decides calls, meeting intelligence and screen sharing. [DOC]
- **Default room agent.** Should the always-on default assistant remain once human-only posting exists, or become an explicit binding with mention-only wake?
- **ZDR orgs.** Is a refs-only experience in push, previews, search and digests acceptable to them?
- **Channel Plane and desktop shell.** Is the Channel Plane scheduled, and is a Tauri shell wanted after the PWA (Progressive Web App)?
- **Sandbox in production.** Does execution-core fail closed when bubblewrap is unavailable? Verification confirmed that unsandboxed spawn is refused for the read-only and workspace-write policies; the production behaviour of the fallback path still deserves a focused check. [SRC]

Two earlier open questions are now answered by verification: approvals do resume for three idempotent action kinds and fail closed for the rest ([F-t1-010](#F-t1-010)), and the org audit read path is confirmed open to any org member ([F-t1-031](#F-t1-031)). [SRC]

### 1.6 What changed during verification

Two independent passes followed consolidation: a Verevon-side pass that read the code behind every item, and a reference-side pass that tried to refute the reference claims. All 96 items were checked; none remain unverified and none are low confidence. The Verevon-side pass kept 9 items as written, reworded 78 because the code showed a more precise gap, a better place to build or reusable parts, and gave 9 a demote verdict. Five of those changed tier: [F-t1-005](#F-t1-005) moved from Priority to Recommended, and [F-t1-029](#F-t1-029), [F-t1-066](#F-t1-066), [F-t1-068](#F-t1-068) and [F-t1-069](#F-t1-069) moved from Recommended to Nice to have. The tiers went from 24/45/27 to 23/42/31 (Priority/Recommended/Nice to have). Effort was revised on 29 items, almost all upward, because bundled items span more planes than first estimated; the verified statuses are 68 Partial, 21 Absent, 4 Stub and 3 Planned. The reference-side pass re-examined the 24 items that were Priority before verification (F-t1-001 to F-t1-024; Recommended and Nice-to-have reference claims were not re-checked), confirmed 2 in full and 22 in part, and corrected prevalence counts on 17 items; the corrected counts are used throughout and the refuted claims were removed.
