## 2. What every reference shares

Across the five references (Teams, Slack, Grok Bot, Buzz and QM) 72 capabilities were classed as shared by at least 3 of 5 before reference-side verification; after correction a few rows count partial evidence and one (privacy-respecting telemetry) falls to 1 of 5, kept here for traceability. They fall into three groups: the human chat core that Teams, Slack and Buzz ship and that Grok Bot and QM lean on, the agent-in-the-room layer that Buzz, Grok Bot and QM ship, and the client and governance plumbing that every native app carries. Verevon is at parity on five of the 72 (agents as labelled members, server-resolved tenant binding, server-managed chat with zero-data-retention on top, accessibility, and empty states), is partial on 38, lacks 27 outright, has one stub (SCIM) and one unverified (pluggable harnesses); the largest gaps are the human chat core and the realtime push substrate. Prevalence is written as "n of 5 references"; where verification corrected a count, the corrected count is used and the cluster count is not repeated. ZDR means zero-data-retention; SFU means selective forwarding unit; SSO is single sign-on; SCIM is the System for Cross-domain Identity Management; MCP is the Model Context Protocol; PWA is progressive web app; CSP is Content Security Policy.

### 2.1 Messaging core

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Shared rooms with human-first posting (agents as ordinary members) | 3 of 5 native (Buzz, Slack, Teams); QM via Slack | Partial | [F-t1-001](#F-t1-001) |
| Threads: reply-in-thread, multi-author threads, thread panel | 5 of 5 (Grok Bot threads are bot task threads) | Partial | [F-t1-001](#F-t1-001) |
| Direct messages and group DMs | 4 of 5 | Absent | [F-t1-016](#F-t1-016) |
| Mentions bound to exact identities, with group and broadcast handles | 4 of 5 | Partial | [F-t1-007](#F-t1-007) |
| Emoji reactions | 5 of 5 | Absent | [F-t1-017](#F-t1-017) |
| Edit and delete messages (audit, tombstones) | 4 of 5 (QM handles edits made on its Slack surface) | Absent | [F-t1-017](#F-t1-017) |
| Custom emoji, GIFs and shortcode completion | 4 of 5 | Absent | [F-t1-017](#F-t1-017) |
| Rich-text composer (formatting, code, lists, Enter behaviour) | 4 of 5 | Partial | [F-t1-032](#F-t1-032) |
| Persistent drafts | 4 of 5 | Absent | [F-t1-032](#F-t1-032) |
| Attachments and file upload pipeline | 5 of 5 | Absent | [F-t1-013](#F-t1-013) |
| Read state: per-thread unread, mark unread, read receipts | 4 of 5 | Partial | [F-t1-042](#F-t1-042) |
| Voice notes and audio or video clips with transcripts | 4 of 5 | Absent | [F-t1-082](#F-t1-082) |

The baseline is a room where a post is a plain append, agents wake only on mention or subscription, any member can continue a thread, and messages carry reactions, revisions, attachments and a per-thread read frontier; Buzz ships all of this in source and Slack and Teams document it [SRC, DOC]. Verevon is behind on nearly every row: every Space post is a model turn because `SpaceRoomComposer.submitMessage` always calls `streamChat` (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx:375-425), a non-owner reply is refused in Session Core by `authorize_thread_owner(Mutate)`, Space kinds are limited to personal and room in the gateway create route (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:2850-2900), and `sessions.proto` offers only `AppendMessage` plus whole-thread deletes, so there is no edit, delete or reaction store (apps/Model Plane/proto/model_plane/v1/sessions.proto:18-204) [SRC]. The one point of parity is agent mentions: the browser sends only `mentioned_agent_ref` and the gateway re-resolves it against the Control roster, which is the identity-binding pattern Buzz uses on the client [SRC]. The composer is a two-row textarea cleared before a send starts, so a failed send loses the draft [SRC].

### 2.2 Org and membership

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Role hierarchy, permission checks and role change | 5 of 5 graded roles; 3 of 5 with record-level evidence (Buzz, Slack, QM) | Partial | [F-t1-022](#F-t1-022) |
| Room visibility (open or private), directory, browse and self-join | 3 of 5 (Buzz, Teams, QM) | Absent | [F-t1-048](#F-t1-048) |
| Guests and external users | 4 of 5 | Partial | [F-t1-053](#F-t1-053) |
| Admin console and org admin operations | 3 of 5 | Partial | [F-t1-006](#F-t1-006) |
| Multi-account and multi-workspace switching in one client | 4 of 5 | Partial | [F-t1-055](#F-t1-055) |

Every reference grades members, and Buzz adds an owner or admin guard on elevated roles, a sole-owner rule, archive and TTL rooms, all checked under a per-channel advisory lock [SRC]. Control already stores and converges viewer, editor, manager and owner roles with an owner guard (apps/Control Plane/user-core/internal/spaces/repository.go:646-675), but Application can only grant editor and the gateway has only add and remove member routes, so there is no role change, rename, topic, archive or restore [SRC]. Discovery is absent at the schema level: `registered_spaces` has no visibility column and `space_memberships` has no join sequence (apps/Control Plane/user-core/migrations/017_space_authority.up.sql:10-66) [SRC]. Guests exist only as org-level email invitations with a 48 h default expiry in auth-core [SRC].

### 2.3 Calls and meetings

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Group audio or video calls and huddles started from a room | 3 of 5 (Slack, Teams, Buzz) | Absent | [F-t1-029](#F-t1-029) |
| Meeting transcription and AI recap into a searchable artifact | 3 of 5 | Partial | [F-t1-050](#F-t1-050) |
| Calendar, meeting scheduling and calendar-triggered agent work | 4 of 5 | Partial | [F-t1-036](#F-t1-036) |

The three references with room calls (Slack, Teams and Buzz) start a call from the room and keep it there; Slack documents the huddle as part of the channel, Buzz models it as a short-lived child channel and Teams carries the deepest engine, while Grok Bot contributes a client call state machine [DOC, SRC, REA]. Verevon has no call, huddle, SFU or WebRTC code in any plane, and `docs/space-defenition.md` lists huddles as a non-goal (apps/Frontend Plane/verevonv3/docs/space-defenition.md:24) [SRC]. Only dictation-sized speech-to-text exists, capped at 25 MB of audio and a 60 s take, with no diarisation [SRC]. A calendar read action exists in integration-corev2, and capability-core's sweeper reauthorizes every scheduled slot through Control, but there is no room UI to create a schedule [SRC].

### 2.4 Search and discovery

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Permission-aware full-text message search (index plus per-hit re-authorization) | 4 of 5 (per-hit re-authorization verified only in Buzz) | Absent | [F-t1-018](#F-t1-018) |
| Scoped in-conversation search and Ctrl+F routing | 3 of 5 (Buzz, Teams, Slack partial) | Absent | [F-t1-018](#F-t1-018) |
| Search operator grammar (from:, in:, has:, before:/after:) | 3 of 5 | Absent | [F-t1-069](#F-t1-069) |
| People directory and people search | 5 of 5 | Partial | [F-t1-049](#F-t1-049) |
| Room directory: browse and join | 3 of 5 | Absent | [F-t1-048](#F-t1-048) |

Buzz's search is the reference mechanism: a stored generated `tsvector` column with a GIN index, a NULL vector for sensitive kinds, `ts_rank_cd` ordering and an access predicate applied to every hit, while QM confirms the same `tsvector` approach over session entries [SRC]. Teams supplies the richest scope list, but only at the level of UI strings, and its search backend cannot be observed [REA]. Verevon has no search over Space messages, threads or runs: the gateway search router serves only web, similar, suggest, images, videos and answer (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/search.rs:30-45), and Ctrl/Cmd+K opens a knowledge search over Data v2 retrieval plus a client-side title filter [SRC]. On people, org-core already serves a member search endpoint that the gateway never proxies [SRC].

### 2.5 Notifications and attention

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Event-to-notification pipeline: typed signal classes, normalised payload, deep links | 4 of 5 | Absent | [F-t1-008](#F-t1-008) |
| Per-conversation notification levels and per-event preferences | 3 of 5 | Partial | [F-t1-008](#F-t1-008) |
| Personal cross-room activity inbox with typed events | 3 of 5 | Partial | [F-t1-009](#F-t1-009) |
| In-app unread counts, cross-room aggregates and mark-all-read | 3 of 5 | Partial | [F-t1-009](#F-t1-009) |
| OS badge, tray and taskbar cues | 3 of 5 | Absent | [F-t1-035](#F-t1-035) |
| Per-event notification sounds | 4 of 5 | Absent | [F-t1-035](#F-t1-035) |
| Reminders and snooze on messages | 3 of 5 | Absent | [F-t1-042](#F-t1-042) |

Slack validates one schema-checked payload (team, channel, message and thread ids plus a trace id) and runs it through client-side display and DND gates, and Teams exposes about 60 titled activity types in its feed [SRC, REA]. No reference ships an implemented notification rate limiter; Grok Bot carries a disabled config schema only [SRC]. Verevon emits only run-completion notifications to users who registered a per-run watcher, `mention.received` exists only as a seeded preference row with no producer, and the sidebar unread and working dots come from a browser-only store cleared when the reader leaves the room (apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-live-work.ts:1-146) [SRC]. A durable per-member, per-room read marker does exist in Convex (apps/Application Plane/convex-core/convex/spaceReadMarkers.ts:1-153), which is the right base for a server-side aggregate [SRC]. Nothing for out-of-tab attention exists: no manifest, service worker, Notification or Push API, or `setAppBadge` call [SRC].

### 2.6 Apps and extensibility

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Public developer API, SDKs and rate-limit contract | 3 of 5 | Partial | [F-t1-076](#F-t1-076) |
| Platform slash commands and app command registry | 3 of 5 | Partial | [F-t1-041](#F-t1-041) |
| MCP server registry with shared or per-user credentials | 3 of 5 | Partial | [F-t1-037](#F-t1-037) |
| Workflow automation (builder, triggers, steps, run trace) | 3 of 5 | Partial | [F-t1-036](#F-t1-036) |

The shared shape is a registry of tools and commands that admins can allow or block, plus an automation builder with a run trace; QM ships the deepest automation model (fire keys, outcomes, loops with gates) and Teams the most explicit allowlisting [SRC, DOC]. Verevon's MCP registry is real, with private, shared and org scope, OAuth 2.1 with token refresh and a three-attempt retry, but tool exposure is resolved per org and requesting user with no Space input (apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs:715-765) [SRC]. Org-scoped API keys are minted, listed and revoked end to end, and a typed action registry exists, so the public API and command layer are extensions rather than new builds [SRC].

### 2.7 Agents in the room

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Agent as a first-class, labelled room member with owner provenance | 4 of 5 | Shipped | none needed |
| In-room activity, plan and tool-call cards | 3 of 5 shipped (Buzz, QM, Grok Bot); Slack DOC and Teams streaming flags | Partial | [F-t1-004](#F-t1-004) |
| Agent presence and live working state | 4 of 5 | Partial | [F-t1-004](#F-t1-004) |
| In-room approval cards with grant scopes and durable pause and resume | 3 of 5 (Grok Bot, QM, Teams) | Partial | [F-t1-010](#F-t1-010) |
| Audience-safe disclosure in shared rooms | 1 shipped (QM) plus 2 partial (Teams, Slack) | Partial | [F-t1-012](#F-t1-012) |
| Context assembly and compaction | 4 of 5 | Partial | [F-t1-012](#F-t1-012) |
| Scheduled and event-triggered agent work | 4 of 5 | Partial | [F-t1-036](#F-t1-036) |
| Agent cost and usage: per-agent and per-room attribution, budgets | 4 of 5 | Absent | [F-t1-027](#F-t1-027) |
| Pluggable agent harnesses (bring your own runtime) | 4 of 5 | Unknown | none needed (does not fit the Model Plane runtime) |

This is Verevon's strongest category. Room agents exist end to end: create-in-room, bind an org agent, pause, resume and revoke, agents held in Control as service subjects, and a binding that already stores `triggerModes`, `allowedTools` and `approvalMode` enforced by the gateway [SRC]. The gap is visibility and scope: inside a Space only the sender streams the turn, because Session Core authorizes run events with `authorize_run_owner(Read)` and the gateway proxies them with the caller's token, so other members see a 6 s poll (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/streams.rs:169-215) [SRC]. QM is the only reference with shipped audience intersection and labelled memory, whereas Verevon's Session Core extracts memory candidates from every Space post into user-level `agent_memory` with no space, audience or label column [SRC]. The cost ledger has tenant, actor, run, request, model and producer dimensions and no Space or agent column (apps/Model Plane/go/services/cost-core/internal/ledger/ledger.go:40-50) [SRC]. Pluggable harnesses were judged not to fit because an outside runtime would bypass ZDR and Model Plane authority [INF].

### 2.8 Collaborative artifacts

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Room canvas: one shared, agent-readable and agent-writable document per Space | 4 of 5 (Teams as meeting notes, a weak analogue) | Absent | [F-t1-015](#F-t1-015) |
| Room file library stored by an owner plane with provenance, grants and quotas | 4 of 5 | Partial | [F-t1-013](#F-t1-013) |
| Artifact sharing: per-person and general access, view or edit roles, access requests | 4 of 5 | Partial | [F-t1-057](#F-t1-057) |

Slack documents the canvas product model and Buzz ships the write-safety mechanism: a transaction that takes a per-channel advisory lock, compares the head with the expected revision and only then inserts, so a rejected save is rolled back [DOC, SRC]. Verevon has no Space-owned document and no Canvas tab (`SpaceTabId` has six tabs, apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceCockpit.tsx:43); wiki-store has pages, append-only versions and diffs but `CreateVersion` takes no expected-head precondition (apps/Data Plane v2/services/wiki-store-go/internal/repo/wiki_repo.go:359-398) [SRC]. The file library is a read-only Knowledge tab with owner-plane plumbing only for connector imports, and Control `resource_grants` already supports an edit role that the gateway share route hard-codes away [SRC].

### 2.9 Realtime and sync architecture

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Persistent push channel delivering room events to every member | 5 of 5 (Grok Bot's server tail is flag-gated) | Absent | [F-t1-003](#F-t1-003) |
| Stream health watchdog with polling safety net | QM in full; Teams and Grok Bot partial | Partial | [F-t1-003](#F-t1-003) |
| Local replica or cache for instant paint and restart | 4 of 5 | Absent | [F-t1-080](#F-t1-080) |
| Server-managed chat, no E2EE for messages, selective owner-private encryption | 4 of 5 | Shipped | none needed |

Every reference pushes room events to every member; Buzz is the only one with a single fail-closed fan-out chokepoint shared by local and cross-node paths and a subscribe-time access check, and QM is the only one with a verified watchdog that resyncs on reconnect [SRC]. No reference was shown polling room messages while push is healthy [SRC]. Verevon has no room-level push: `SpacePage` polls the thread projection every 6 s while the tab is visible (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpacePage.tsx:273-338), the gateway has no Space stream route (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:649-738), and Convex has no per-Space event table or sequence [SRC]. The run-event SSE with `Last-Event-ID` and the AG-UI stream exist outside Spaces and are the natural base [SRC]. The local replica is deliberately low priority because of ZDR [INF].

### 2.10 Governance, security and compliance

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Server-resolved tenant binding and multi-tenant isolation | 4 of 5 | Shipped | none needed |
| Enterprise SSO, SCIM provisioning and identity federation | 3 of 5 for SSO plus Buzz partial; SCIM reference is Slack | Stub | [F-t1-023](#F-t1-023) |
| Secret and PII redaction in logs, telemetry and agent output | 4 of 5 | Partial | [F-t1-045](#F-t1-045) |
| Privacy-respecting telemetry, consent and diagnostics | 1 of 5 consent-deferred client analytics (Grok Bot); QM is a counter-example | Partial | [F-t1-005](#F-t1-005) |

Teams is not a SCIM reference, because it relies on Entra ID provisioning; Slack documents SCIM behind its Business+ and Enterprise plans, and QM's actor gate only filters Slack ingress [DOC, SRC]. QM sends unredacted error context to Sentry when enabled, which is an anti-pattern, not a model [SRC]. Verevon's only SCIM code is a fake-success intake in the Ingestion Plane that stores a webhook event and answers 201 or 204 without creating or changing anyone (apps/Ingestion Plane/integration-corev2/internal/api/server.go:1225-1290), and SSO sits behind a global flag [SRC]. Tool-result screening and persisted-output scrubbing exist, but `TrustClass` is a pure function of the tool name with no class for room-member text [SRC]. Session Core already enforces ZDR on the caller's token claim; only the Space decision's own `zero_data_retention` flag is decoded and discarded [SRC].

### 2.11 Client shell

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Native desktop shell wrapping the web client | 5 of 5 | Absent | [F-t1-073](#F-t1-073) |
| Narrow, typed and validated web-to-native bridge | 4 of 5 | Absent | [F-t1-073](#F-t1-073) |
| OS protocol handler with single-instance link forwarding | 5 of 5 | Absent | [F-t1-073](#F-t1-073) |
| Auto-update with release channels and safe install timing | 4 of 5 | Absent | [F-t1-073](#F-t1-073) |
| Signed packaging and multi-format distribution | 4 of 5 | Absent | [F-t1-073](#F-t1-073) |
| Entity deep-link routing and URL-synced view state | 4 of 5 | Partial | [F-t1-039](#F-t1-039) |
| Feature flags, remote config, preview rings and experiments | 4 of 5 | Partial | [F-t1-006](#F-t1-006) |
| In-app keyboard shortcuts and command palette | 4 of 5 shortcuts; palette 2 of 5 | Partial | [F-t1-041](#F-t1-041) |
| Theming, high contrast, org branding and density | 4 of 5 | Partial | [F-t1-062](#F-t1-062) |
| Accessibility: screen readers, keyboard regions, reduced motion | 4 of 5 | Shipped | none needed |

All five references are native desktop apps; Teams shows the thinnest model, a native shell around a web UI with a versioned RPC over capability modules, and Buzz is the closest stack fit with Tauri 2 and Rust-owned state [REA, SRC]. Verevon is a Vite SPA with no manifest, service worker, Tauri, Electron or MSIX files and no desktop sign-in or updater endpoint (apps/Frontend Plane/verevonv3/index.html:1-16) [SRC]. Because PWA and content-free push come first ([F-t1-035](#F-t1-035)), the shell is placed in the horizon wave [INF]. Space and tab deep links work but `?thread_id` is ignored, and a Cmd/Ctrl+K overlay exists without focus trap, arrow keys or Enter handling [SRC].

### 2.12 UX patterns and layout

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Primary navigation: rail plus sidebar with Home, Activity, Agents, Files destinations | 4 of 5 | Partial | [F-t1-009](#F-t1-009) |
| Personal sidebar organisation: star, sections, sort, drag-drop | 4 of 5 (Buzz, Grok Bot, QM, Slack) | Partial | [F-t1-055](#F-t1-055) |
| Contextual right-hand pane with one header contract and resize | 3 of 5 (Grok Bot, Teams, Slack) | Partial | [F-t1-001](#F-t1-001) |
| Thread side panel, pop-out thread window, inline replies from inbox | 3 of 5 (Slack DOC, Buzz, Grok Bot; Teams as inline replies) | Absent | [F-t1-001](#F-t1-001) |
| Typed transcript rows for mixed human and agent rooms | 3 of 5 | Partial | [F-t1-004](#F-t1-004) |
| Empty states, zero states and suggested starters | 3 of 5 | Shipped | none needed |

The shared layout is a rail, a sidebar the user can reorganise, a main timeline with typed rows and one contextual right pane; Buzz puts Agents, Workflows and the inbox as first-class destinations behind one shared right panel, and Grok Bot's pane shows a close linger, inert-while-closing and focus hand-off, but no keyboard resize or persisted width [SRC]. Verevon's sidebar has fixed groups (Channels with the org room pinned first, Personal Space, All conversations) and a text filter, with no stars, sections, ordering, hide or mute (apps/Frontend Plane/verevonv3/src/features/core/components/sidebar/CoreSidebarSpacesPanel.tsx:18-21) [SRC]. Buzz's 15 render classes and Grok Bot's telemetry row kinds are the models for typed rows; the approval row and `{agentRef, ownerRef}` union in the recommendation are Verevon synthesis [SRC, INF].

## 3. Best of breed: who does each thing best

| Capability area | Best reference | Why it is best (one sentence) | Recommendation |
|---|---|---|---|
| Human-first posting and threads | Buzz; QM for the engagement rule | A post is a plain append tagged with the channel id, thread counters are kept in the same transaction at ingest, and QM lets any member continue a thread with a bot-stake rule for un-mentioned replies [SRC]. | [F-t1-001](#F-t1-001) |
| Addressing and trigger policy | Buzz | A binding carries respond_to and subscribe fields that fail closed, first-match-wins rule triggers, and a composer address lock [SRC]. | [F-t1-002](#F-t1-002) |
| Ambient participation | QM | A cheap judge returns an act-or-stay-silent decision with a reason, standing orders and a stored ledger, a mechanism no other reference has [SRC]. | [F-t1-002](#F-t1-002) |
| Mentions | Buzz | The composer resolves names to subject ids, qualifies namesakes and blocks an ambiguous send, and the relay indexes mention rows [SRC]. | [F-t1-007](#F-t1-007) |
| Edit, delete and reactions | Buzz; QM for agent-visible revisions | Owner-of-agent retract keeps humans accountable for their agents, and QM appends revision entries and parses reactions for the agent (capped at 5 per reply) [SRC]. | [F-t1-017](#F-t1-017) |
| Attachments | Buzz; Slack for the pipeline | Buzz stores content-addressed media with metadata stripped, and Slack documents a three-step upload with a malware scan [SRC, DOC]. | [F-t1-013](#F-t1-013) |
| DMs and group DMs | Buzz; Teams for history choice | Buzz identifies a DM by a participant hash so a second open returns the existing DM, and Teams lets the adder choose how much history a newcomer sees [SRC, REA]. | [F-t1-016](#F-t1-016) |
| Push channel | Grok Bot | One multiplexed, sequenced stream with updated_seq cursors, cursor_too_old and generation frames, flag-gated and mid-migration in the shipped build [SRC]. | [F-t1-003](#F-t1-003) |
| Delivery-time authorization | Buzz | One fail-closed fan-out chokepoint shared by local and cross-node paths, with access checked before subscribe registration [SRC]. | [F-t1-003](#F-t1-003) |
| Stream watchdog | QM | SSE open and 30 s silence timeouts with a resync on every reconnect [SRC]. | [F-t1-003](#F-t1-003) |
| Live run stream for all viewers | QM | AG-UI-shaped SSE per run with offset-keyed text deltas and a snapshot for late joiners, authorized by run visibility [SRC]. | [F-t1-004](#F-t1-004) |
| Activity and tool cards | Buzz | Fifteen render classes turn tool events into verb-object-outcome cards with a working strip and owner badge [SRC]. | [F-t1-004](#F-t1-004) |
| Cross-room inbox | Teams; Grok Bot for the needs-you queue | Teams exposes about 60 titled activity types with slices, and Grok Bot keeps one queue for approvals, questions and hand-offs [REA, SRC]. | [F-t1-009](#F-t1-009) |
| Cross-room state aggregate | QM | One server-filtered session-state stream reports working, awaiting approval or idle per thread with participants [SRC]. | [F-t1-009](#F-t1-009) |
| Notification pipeline | Slack | One schema-checked payload with a trace id runs through display and DND gates in the desktop shell [SRC]. | [F-t1-008](#F-t1-008) |
| Auto-follow rules | Teams | Threads are followed when mentioned, authored or participated, stated explicitly [REA]. | [F-t1-008](#F-t1-008) |
| Content-free push | Buzz | The push is only a wake signal; the client fetches the content, which is the only pattern compatible with ZDR [SRC]. | [F-t1-035](#F-t1-035) |
| Message search | Buzz | A stored generated tsvector with a GIN index, NULL vectors for sensitive kinds, and re-authorization of every hit [SRC]. | [F-t1-018](#F-t1-018) |
| Search scope UX | Teams | The richest scope list (channel, chat, community, thread, post, people), evidenced as UI strings only [REA]. | [F-t1-018](#F-t1-018) |
| Search as the asker | Slack; QM for the mechanism | An action token from the triggering event bounds the agent's search to the asker's reach and results are never stored [DOC, SRC]. | [F-t1-038](#F-t1-038) |
| Approvals | QM; Grok Bot for the card UX | QM stores the request, ends generation at the pause, lets only the requester decide with once, session or always scopes and re-runs the turn; Grok Bot orders levels never, ask, always with effective = min(user, team ceiling) [SRC]. | [F-t1-010](#F-t1-010) |
| Personal and team agents | Grok Bot | A primary bot, OWNER or TEAM visibility, an access list with USER and MANAGER roles, and clone-to-team [SRC]. | [F-t1-011](#F-t1-011) |
| Private assistant beside the room | Teams | A private pane with an escalate-to-group step, evidenced for meeting chats [REA]. | [F-t1-011](#F-t1-011) |
| Audience-safe context and memory | QM | Write-time labels, an audience fold at read and a context cut when the audience changes are the only shipped implementation; Buzz's version is a draft design [SRC, DOC]. | [F-t1-012](#F-t1-012) |
| Agent browser and takeover | Grok Bot; QM for the grant | Hand-off with an instruction, take control, bot-wall detection and a live preview card, plus QM's scoped browser-model capability token [SRC]. | [F-t1-020](#F-t1-020) |
| Agent computer lifecycle | Grok Bot; QM for tokens and egress | Hibernate and pre-thaw with cost-aware wake, and QM's per-turn capability tokens with an authorising egress proxy [SRC]. | [F-t1-028](#F-t1-028) |
| Multi-agent turn-taking | Grok Bot | Per-member turn requests with nonce and deadline, speak, pass, skip and timeout outcomes, and a loop guard in shadow mode [SRC]. | [F-t1-025](#F-t1-025) |
| Agent self-API | QM | A discoverable API catalogue and a per-turn capability token (48 h default) re-verified by every handler [SRC]. | [F-t1-019](#F-t1-019) |
| Forms and secret drop | QM | A one-time form whose value goes to the keychain while the transcript records only that it was supplied [SRC]. | [F-t1-026](#F-t1-026) |
| Credential custody | QM; Grok Bot for per-person OAuth | A secrets broker with route classification and cross-person asks, and Grok Bot's shared plugin with per-person OAuth and draft cards [SRC]. | [F-t1-044](#F-t1-044) |
| Automations | QM | Crons with destinations, watches, loops with gates, fire keys and outcomes, runAs and recipient consent [SRC]. | [F-t1-036](#F-t1-036) |
| Run queue | QM | A unique one-running-per-session index, SKIP LOCKED claims, lease heartbeat and a reaper with crash-loop parking [SRC]. | [F-t1-040](#F-t1-040) |
| Agent cost | Grok Bot | Cost by flow and by bot with hard limits [SRC]. | [F-t1-027](#F-t1-027) |
| Room canvas | Slack for the product; Buzz for write safety | Rich blocks with comments, and append-only revisions guarded by an advisory-lock compare against the expected head [DOC, SRC]. | [F-t1-015](#F-t1-015) |
| Agent artifacts and viewers | Grok Bot | A lazy viewer registry for PDF, DOCX, XLSX, code and Mermaid is confirmed; the canvas-first rule is flag-gated and inherited [SRC]. | [F-t1-014](#F-t1-014) |
| Room roles and lifecycle | Buzz | Elevated roles granted only by an owner or admin, a sole-owner rule, archive and TTL rooms, all under a per-channel advisory lock [SRC]. | [F-t1-022](#F-t1-022) |
| Room directory and self-join | Buzz | An open channel directory in source, with Teams and QM as the other shipping examples [SRC]. | [F-t1-048](#F-t1-048) |
| Admin policy | Teams | Precedence direct over group over global, policy packages and admin allowlisting of apps, agents and models [DOC]. | [F-t1-006](#F-t1-006) |
| Feature flags and rings | Teams | Layered providers resolved by ring, tenant and account with a local override [REA]. | [F-t1-006](#F-t1-006) |
| SSO and SCIM | Teams for SSO; Slack for SCIM | Teams ships a native client broker against its own identity cloud, and Slack documents SCIM provisioning on its upper plans [REA, DOC]. | [F-t1-023](#F-t1-023) |
| Guests | Teams; QM for expiry | Teams defines guest tiers, and QM makes a guest expiry mandatory [REA, SRC]. | [F-t1-053](#F-t1-053) |
| Moderation | Buzz | Private reports to a queue, signed actions, tombstones and a ban that also stops the person's agents [SRC]. | [F-t1-043](#F-t1-043) |
| Retention | Slack | Org, workspace, channel and DM rules with an org lock [DOC]. | [F-t1-051](#F-t1-051) |
| Calls | Slack for the product; Grok Bot for the client; Teams for the engine | The huddle belongs to the room, Grok Bot ships explicit call phases, and Teams SlimCore carries the lifecycle depth [DOC, SRC]. | [F-t1-029](#F-t1-029) |
| Meeting recap and consent | Teams | Notes, tasks, topics and chapters with citations back to the recording, AI on and off, a privacy mode and recap visibility derived from transcript access [REA]. | [F-t1-050](#F-t1-050) |
| Command layer | Grok Bot; QM for the palette | Every shell action is a registered command with an id and hotkey consumed by one hotkey layer and a mod+K palette [SRC]. | [F-t1-041](#F-t1-041) |
| Deep links | Teams; QM for paging | One entity-and-action command path serves toasts and links, and QM pages with a ?seq= cursor [REA, SRC]. | [F-t1-039](#F-t1-039) |
| Timeline windows | Buzz | Server-computed windows with reply summaries, composite cursors and a tested scroll-anchoring policy [SRC]. | [F-t1-034](#F-t1-034) |
| Reliable send | Grok Bot | A send journal with queued, prepared, dispatching and accepted-awaiting-echo stages and a stale-while-revalidate resource state machine [SRC]. | [F-t1-033](#F-t1-033) |
| Sidebar organisation | Buzz; Teams for system sections | Per-group sort, synced sections and stars, drag-and-drop and a more-unread pill, with Teams adding computed system sections [SRC, REA]. | [F-t1-055](#F-t1-055) |
| Error recovery | Teams; Buzz for the root boundary | A diagnosis-specific native error curtain with Restart and Repair, and Buzz's root React error boundary with Reload [SRC]. | [F-t1-024](#F-t1-024) |
| Desktop shell | Teams for the model; Buzz for the stack | A thin native shell around a web UI with a versioned RPC over capability modules, and Buzz's Tauri 2 shell with Rust-owned state [REA, SRC]. | [F-t1-073](#F-t1-073) |

Taken together, the references split the work cleanly. Buzz is the best overall model for the human chat core and its substrate: posting, threads, mentions, reactions, DMs, search, delivery-time authorization, timeline windows, roles and moderation, all readable in source [SRC]. QM is the best model for safe agents in shared rooms: audience-bound memory, durable approvals, the self-API, credential custody, automations, the run queue and the live run stream [SRC]. Grok Bot is the best model for the personal agent as a product: the primary bot and access list, the agent's computer and browser with takeover, turn-taking, the send journal, the command registry and cost by bot [SRC]. Slack supplies the product definitions for canvas, huddles, retention, notification engineering and search-as-the-asker, mostly as documentation [DOC], and Teams supplies the enterprise envelope: the activity feed, admin policy and rings, meeting intelligence with consent, deep-link routing and the thin native shell, mostly as reverse-engineered strings [REA]. Verevon's own strengths, Control-signed decisions, ZDR enforcement in Session Core and agents as service subjects, are not matched by any reference and should remain the frame into which these patterns are fitted [SRC, INF].
