<a id="F-t1-037"></a>
### F-t1-037 · Space tool cockpit and reliable tool connections: per-room MCP enablement, per-binding allowlist, credential mode and scoped consent, health states and person-bound sign-in links

*Verevon today: Partial · Effort: L · Plane: Model Plane (registry and runtime) with Control, Ingestion and Frontend · Wave: W3 · Confidence: Medium*

**What and why.** Let a Space manager enable org-approved MCP (Model Context Protocol) servers for one room, with a per-binding tool allowlist, a per-user or shared credential choice and a consent card bound to the exact Space, agent and tool. Make tool use fail visibly and recover with per-server health, backoff, call budgets and a room-visible "needs sign-in" card.

**Reference practice.** An MCP registry is table stakes among the agent references (3 of 5); consent links for connected apps appear in 2 of 5. QM's server record carries credentialScope shared or per_user, a toolAllowlist and a readOnly flag, and a consent link completes only for the signed-in principal [SRC]. Grok Bot probes health every 10 s, bounds calls at 2 min idle and 60 min total, and returns a typed error union with needs_auth and retry_after hints [REA].

**Verevon today and approach.** The registry, OAuth 2.1 refresh, three-attempt retry and argument validation exist (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:327-343, tool_retry.rs:1-60), but tool exposure is resolved per org and requesting user with no Space input (runtime_registries.rs:715-765), so a room turn is offered that member's private and shared tools. A per-binding allowed_tools list is stored, enforced and displayed (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4500-4570) but nothing writes it. Slice 1 (M): default-deny MCP tools in Space turns, enable-per-room as a human-only Control decision, and a write path for allowed_tools. Then add Retry-After, per-server state and idle versus total budgets to the existing retry and refresh. Consent cards and person-bound links follow once cards exist; the connect-session callback completes by state, so a principal check must be added.

**Dependencies and risks.** S3.4 broker and S3.2 sandbox lease; a shared credential lets any member act as its owner, so default read-only with a warning; never widen a private server by Space linkage.

<a id="F-t1-038"></a>
### F-t1-038 · Agents and AI answers over room history under the asker's authority: search_space_history and open_space_entry tools, cited answers over messages and Space knowledge

*Verevon today: Partial · Effort: L · Plane: Model Plane (tools, index, answer composition) with Control Plane · Wave: W3 · Confidence: Medium*

**What and why.** Give room agents search_space_history and open_space_entry tools over a Session Core message index, run under the authority of the human who triggered the turn, then cited answers over messages and documents and an "Ask agent about this" action. Agents today cannot read room history beyond the current transcript.

**Reference practice.** A history tool appears in 3 of 5 references, delegated-authority search and cited answers in 2 of 5 each. QM's sessions.search returns session and entry references without widening read access, and the history tool reopens one entry by sequence even after compaction [SRC]. Slack mints an action token from the triggering event and limits results to what the user can access; this rests on public docs [DOC]. Grok Bot adds scopes and a rule to trust the newest result [REA].

**Verevon today and approach.** Grounded document retrieval is shipped: knowledge_search maps to cap.retrieval.query (apps/Model Plane/rust/services/execution-core/src/capability_policy.rs:780-800) and the gateway carries a Control retrieval decision in space_context (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:215-300). Messages are not covered: Session Core scans with no full-text index (apps/Model Plane/rust/services/session-core/src/grpc.rs:5503-5596) and the gateway search routes cover only web, similar, suggest, images, videos, suggestions and answer, none over Space messages (search.rs:39-45). Stage it: (a) "Ask agent about this" as a composer action that quotes the selected turn into a reply plus mention (S); (b) a tsvector index and SearchSpaceMessages RPC filtered by the audience ceiling; (c) the two tools in capability_policy.rs, scoped to the current Space, using a model.thread.read decision added to space_context; (d) cited answers last. Current-Space scope makes every hit readable by all members, which removes the private-quote problem.

**Dependencies and risks.** [F-t1-018](#F-t1-018) index and deep links; ZDR (Zero-Data-Retention) threads have nothing to index; retrieved messages are untrusted input.

<a id="F-t1-039"></a>
### F-t1-039 · Entry references and permalinks: one entity router for message, thread and run links, deep links honoured, Space-role pins, visibility-preserving fork, quote and forward

*Verevon today: Partial · Effort: L · Plane: Frontend Plane (route table) with Model Plane (entry refs, fork) and Application Plane (pins) · Wave: W2 · Confidence: High*

**What and why.** Define one reference to a room entry (space_ref, thread_id, seq) resolved server-side under the viewer's audience revision, and one route table that notifications, push, agent-posted links and share handlers all use. Build Space-role pins, a fork that copies only what the forking viewer can see, quoted replies with tombstones, and forward last.

**Reference practice.** Deep links are table stakes (4 of 5); pins appear in 3 of 5; fork, quote and forward in 2 of 5 each. Teams normalises toast clicks, deep links and lifecycle events into one (entity, action) command and fails closed on membership [REA]. QM pages older history until the target sequence loads, then highlights it, and records forkedFrom and forkBoundarySeq on fork [SRC]. Buzz renders known hosts as entity chips [SRC].

**Verevon today and approach.** Activity rows already emit /spaces/{id}?thread_id=...#chat (apps/Frontend Plane/verevonv3/src/features/spaces/lib/activity-grammar.ts:186-212) but only the hash tab is read (components/SpaceCockpit.tsx:120-125) and App.tsx registers just /spaces/:spaceId (src/app/App.tsx:181-182). Pin is an author-only thread flag (apps/gateway/src/domains/spaces.rs:2183-2310) and the timeline sorts by time only (SpaceRoomTimeline.tsx:137-141). Every message carries a durable message_id and audience snapshot (apps/Model Plane/rust/services/session-core/migrations/0025_message_space_audience_context.sql:1-30) that the Space client drops (src/shared/api/spaces-client.ts:606-660). Slice by cost: make the existing links scroll and highlight, validated like chat-thread-deep-link.ts (S); pass message_id through and add /spaces/:id/m/:entry (S to M); Space-role pins in an Application table with timeline lifting (M); fork as a Session Core RPC denied for ZDR (L); quote with a server-resolved tombstone (M). Demote forward.

**Dependencies and risks.** [F-t1-001](#F-t1-001) shared threads, [F-t1-035](#F-t1-035) typed notification refs; reveal no titles to non-members; an agent pin tool first needs a model-eligibility.ts entry.

<a id="F-t1-040"></a>
### F-t1-040 · Per-thread serialized, leased agent run queue and member-safe mid-run steering for multi-member rooms

*Verevon today: Partial · Effort: L · Plane: Model Plane · Wave: W2 · Confidence: High*

**What and why.** Several people can post to one thread while an agent turn runs. Characterise what happens today, add a per-thread active-run guard with a visible "agent is busy" state, and let members steer a running turn at the next tool-round boundary instead of being locked out.

**Reference practice.** A durable per-conversation run queue appears in 2 of 5 references, mid-run steering in 1 of 5. QM allows one running run per session via a unique partial index, claims with SKIP LOCKED, heartbeats the lease, parks a run after repeated crashes, and classifies an incoming message as engage, steer or drop with steers read at step boundaries [SRC]. Grok Bot uses a Temporal-based durable harness instead [REA].

**Verevon today and approach.** Steering is already built and Space-aware except in the room composer: model-gateway queues input under a fresh Control append decision (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:7397-7540), the BFF route mints it (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/json_handlers.rs:164-215), but SpaceRoomComposer only refuses to send while submitting (src/features/spaces/components/SpaceRoomComposer.tsx:372-378). The registry is in-memory, single-replica and bound to the requesting user (queued_input.rs:28-32). No per-thread gate exists: start_run_inner inserts with ON CONFLICT (id) only (apps/Model Plane/rust/services/session-core/src/grpc.rs:3071-3140). Reduce to: (a) a two-member concurrency test, including whether the assistant-reply slot rule (grpc.rs:2487-2490) refuses a second reply; (b) a per-thread guard at run start or a gateway 409 plus a queued-turn state; (c) wire the composer to the existing queue route, then member steering by explicit policy with [F-t1-032](#F-t1-032); (d) reuse managed-run recovery as the reaper. Drop the tool ledger: interactive runs cannot restart.

**Dependencies and risks.** ZDR Spaces cannot steer (queue route refuses ZDR tokens); member steering must not widen any owner-plane ACL; multi-replica steering needs request-id routing over NATS.

<a id="F-t1-041"></a>
### F-t1-041 · One command layer on the typed action registry: Cmd/Ctrl+K palette and quick switcher, platform slash commands, shortcut sheet, then a unified 'To:' picker

*Verevon today: Partial · Effort: M · Plane: Frontend Plane · Wave: W2 · Confidence: High*

**What and why.** Wrap the typed ActionDescriptor registry and navigation commands in one command registry with hotkeys, one key handler, Ctrl/Cmd+1-9 Space switching and a shortcut sheet. Rebuild Cmd/Ctrl+K as a palette over rooms, threads, agents, people and actions, and add platform slash commands beside skills. A registry shared by human clicks and model-initiated actions is a base no reference has.

**Reference practice.** A command registry with hotkeys is table stakes (4 of 5); slash commands 3 of 5; a quick switcher is recorded in 2 of 5 and under-counted. Grok Bot registers every shell action as a command with id and hotkey consumed by one hotkey layer and a mod+K palette, and reveals numeric shortcuts on a held modifier [REA]. QM uses the same registry shape [SRC]. Slack defines the keyboard model and the ephemeral versus in-channel slash contract [REA].

**Verevon today and approach.** The Cmd/Ctrl+K overlay is a knowledge search plus local chat-history filter, not web search (apps/Frontend Plane/verevonv3/src/features/core/components/CoreNavbar.tsx:122-132), with no focus trap, arrow keys or Enter handling (docs/UI_UX_AUDIT_2026-09-29.md:71). The room "/" opens a skill picker only (src/features/spaces/components/SpaceRoomComposer.tsx:213-275), and the gateway accepts only personal and room Spaces (apps/gateway/src/domains/spaces.rs:2871-2873), so a "To:" picker has nothing to find. Ship the switcher (rooms, loaded threads, agents, roster people) and the keyboard model first, keeping knowledge and local history as palette providers. ActionDescriptor has no hotkey or when field, so add them in a thin wrapper; run actions only through ActionCommand preview and the gateway dispatcher, where input is derivable or after a schema-driven prompt. Split the "To:" picker into its own XL item blocked on a DM kind and people directory.

**Dependencies and risks.** [F-t1-055](#F-t1-055) org switcher keys, [F-t1-009](#F-t1-009) cross-room lists; requiresApproval actions never run from the palette; recent items obey isLocalRetentionAllowed under ZDR.

<a id="F-t1-042"></a>
### F-t1-042 · Read state, presence and personal follow-ups: per-thread read frontier, mark unread, global presence and agent availability, saved items, reminders and snooze

*Verevon today: Partial · Effort: L · Plane: Application Plane (convex-core) with Model Plane (run-state feed) · Wave: W2 · Confidence: High*

**What and why.** Add mark-unread, a first-unread divider and jump-to-unread to the room read marker; a global status with text and expiry plus agent availability; and refs-only saved items and reminders that fire into the Inbox. Per-thread frontiers and receipts wait for shared threads.

**Reference practice.** Read state is table stakes (4 of 5), presence 3 of 5, reminders 3 of 5. Buzz keeps a server-side read frontier with manual-unread overrides and encrypts a reminder's target and note to its author so the server schedules on not_before alone [SRC]. Teams marks read optimistically, has the richest availability model with DND suppressing notifications, and offers shared room reminders [REA].

**Verevon today and approach.** A durable, cross-device room-level marker ships (apps/Application Plane/convex-core/convex/spaceReadMarkers.ts:1-30), designed per room on purpose, and advanceMarker never moves backwards (spaceReadMarkers.ts:78-84); there is no mark-unread, divider or receipts. Presence is room-scoped online and typing with a 30 s TTL (convex/spacePresence.ts:1-60); the only global status is a user-core profile availability value with no text or expiry (apps/Control Plane/user-core/internal/http/handlers.go:225-262) that no Space surface reads; agent "working" is inferred in the browser (src/features/spaces/lib/space-live-work.ts:100-140). Reminders exist only as a 24 h ticket snooze (src/features/inbox/components/InboxPage.tsx:1057-1073). Three slices: (1) a mark-unread mutation plus divider and jump from the SpacePage arrival snapshot (S to M); (2) extend the availability value with text and expiry and project agent working or waiting from a Model Plane feed, not a cross-plane read; (3) refs-only reminders fired by the Convex scheduler into the Inbox (M). Drop priority re-notify; notification-core has no such concept.

**Dependencies and risks.** [F-t1-001](#F-t1-001) before per-thread frontiers, [F-t1-009](#F-t1-009) Inbox Reminders slice; rows stay content-free; global status must not leak across organisations; reminder targets are rechecked at fire time.

<a id="F-t1-043"></a>
### F-t1-043 · Room moderation: report, queue, tombstones, user block, and timeout or ban that also stops the person's agents

*Verevon today: Partial · Effort: XL · Plane: Application Plane (reports, sanctions) with Control Plane (suspension) and Model Plane (tombstone write) · Wave: W3 · Confidence: High*

**What and why.** Add Report on every room message, a private queue for owners and managers, signed actions (dismiss, delete with a visible tombstone, timeout, remove, ban) and a per-user block list. A ban must also stop the person's agents, or it becomes a bypass. Without this, Spaces cannot open to larger teams or guests.

**Reference practice.** A moderation workflow appears in 2 of 5 references, a ban cascade to owned agents in 1 of 5. Buzz keeps a reports table with the reporter hidden from the author, writes a room-visible tombstone, filters delivery through a block list, and checks the ban verdict at authentication for the principal and its owner, failing closed [SRC]. Teams has the best end-user side: report a concern, block, chat-request expiry [REA].

**Verevon today and approach.** Only coarse removal exists: remove_space_member (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:3433-3546) reaches Control, which publishes membership_changed (apps/Control Plane/user-core/internal/nats/shared_publisher.go:196-215), and session-core records the revocation (apps/Model Plane/rust/services/session-core/migrations/0032_space_membership_revocations.sql:1-20); no in-flight run is cancelled. space_memberships has no suspended_until (apps/Control Plane/user-core/migrations/017_space_authority.up.sql:53-64) and agents carry no owner link in Control. Room posts are Session Core threads with owner-bound presentation writes, so the tombstone cannot live in convex-core: put the hide and tombstone write in Session Core behind a Space-authorised moderation decision, keep Application records content-free (refs and hash), and implement timeout and ban as Control membership state on the membership_changed path made durable (today plain NATS that tolerates missed events). Every post today is an AI turn, so the first reports target agent output and prompts.

**Dependencies and risks.** [F-t1-022](#F-t1-022) room roles, [F-t1-011](#F-t1-011) agent ownership, [F-t1-031](#F-t1-031) audit; removal must propagate to Data Plane indexes and notification payloads; a fail-closed ban check must alert rather than silently lock people out.

<a id="F-t1-044"></a>
### F-t1-044 · Credential custody and outward actions: secrets broker, per-person connectors, draft-first sends, bounded cross-room reach

*Verevon today: Partial · Effort: XL · Plane: Ingestion Plane (broker) with Control (grants), Application (drafts) and Frontend · Wave: W3 · Confidence: Medium*

**What and why.** Let room agents use credentials without seeing them: a broker that attaches secrets server-side with host, method and path pinning, a one-time secret-drop form that bypasses chat and the model, and Control grants (once, session, standing) bound to the Space audience. Add per-person connector accounts, draft-first outbound messages where the human stays the sender, and bounded cross-room posts.

**Reference practice.** Secrets custody and room-agent connectors each appear in 3 of 5 references, draft-first sends in 2 of 5, cross-room reach in 1 of 5. QM's broker checks capability claims, host, method and path prefix, uses manual redirects, caps responses at 5 MB and writes a receipt; secret drop is a one-time form with 7-day expiry; reach is capped at 5 cross-conversation posts per turn [SRC]. Grok Bot ships plugins with per-person OAuth accounts and draft cards with always, needs-approval or disabled settings [REA].

**Verevon today and approach.** Absent: the broker, secret drop and Space-bound grants (session-core migration 0028 adds a credential_grant_ref column no code reads), per-room connector bindings (bindings carry only allowed_tools, trigger_modes and approval_mode; apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4483-4580), and cross-Space posts (apps/Model Plane/rust/services/session-core/src/grpc.rs:2495 refuses them). Already shipped: the human-only versus model-executable classification with a pinned test (src/shared/actions/catalog-manifest.test.ts:23-49), the personal-connection owner rule (apps/Ingestion Plane/integration-corev2/internal/api/connection_access.go:22-73), the human_approved_ai_action attestation (apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:347-430) and the Inbox draft and outbound-intent flow. Split into four: A broker, secret drop and grants in integration-corev2 on egress.SafeClient (L); B connector cockpit and bindings (M); C draft-first sends on conversation-core ai actions, not a new Draft object (M); D cross-room posts under a Control decision for the target Space (M). Drop the route-classification sub-item.

**Dependencies and risks.** [F-t1-028](#F-t1-028) egress boundary, [F-t1-010](#F-t1-010) grant records; recheck grants at use and every scheduled fire; durable decisions refuse ZDR, so standing grants need an explicit rule; only three action kinds resume after approval.

<a id="F-t1-045"></a>
### F-t1-045 · Agent runtime guardrails for shared rooms: member-text screening, shell command policy, room approvals, secret masking, DLP

*Verevon today: Partial · Effort: M · Plane: Model Plane (execution-core, model-gateway, capability-core) · Wave: W3 · Confidence: Medium*

**What and why.** In a multi-human room, colleagues' messages are untrusted input to someone's personal agent, and agent output posted to the room can leak credentials and personal data. Extend the existing screening and redaction to both. Shell parsing, browser approvals in rooms and value-based credential masking follow once rooms can reach those capabilities.

**Reference practice.** Secret and PII (personally identifiable information) redaction is table stakes (4 of 5); command policy 2 of 5; provenance-tagged screening and inline DLP (data loss prevention) 1 of 5 each. QM tags every chunk with provenance, screens external chunks under a deadline, parses commands with tree-sitter-bash against an org floor returning allow, ask or deny with a reason code, and masks injected secrets in raw, URL-encoded and base64 forms [SRC]. Grok Bot pairs a shell AST with typed block reasons [REA]. Teams offers inline DLP with report or override [REA].

**Verevon today and approach.** TrustClass is a function of the tool name with no class for room-member text (apps/Model Plane/rust/services/execution-core/src/provenance.rs:20-58); shell is gated by a tool-name keyword with no parser (execution-core/src/permission/mod.rs:195-262); pii_filter redacts provider-bound input, not agent posts (model-gateway/src/moderation.rs:570-605); scrub.rs is pattern-based (execution-core/src/scrub.rs:40-70). The room composer never requests agentic or browser features and the sandbox is credential_free with egress disabled (execution-core/src/sandbox.rs:355-368), so there is nothing to mask yet. Ship now: (a) a RoomMember trust class from author subject versus invoking subject at prompt assembly, since Session Core records each turn's author (session-core/src/grpc.rs:2386-2395); (e) pii_filter and masking on agent replies in the model-gateway output path or Session Core append, not an Application path. Hold (b), (c), (d) until S3.2, S3.4 and room agentic access land.

**Dependencies and risks.** S2.7 provenance envelope, [F-t1-005](#F-t1-005) typed refusals; false positives on colleagues' text; stateless screening under ZDR; room approvals stay bound to the initiating human.

<a id="F-t1-046"></a>
### F-t1-046 · Files become agent input and room knowledge: per-turn re-authorized attachments as model input, extraction with summaries and typed refusals, and agent-proposed pinned knowledge

*Verevon today: Partial · Effort: L · Plane: Model Plane (prompt builder, routing) with Data Plane v2 (extraction, proposals) and Control (import contract) · Wave: W3 · Confidence: Medium*

**What and why.** Make an uploaded file usable by a room agent: extract and index it into the Space retrieval binding with typed refusals, then pass attachments to providers per turn after re-authorising each against the invoking member's current ACL, storing references only. Later let an agent propose a summary that a human approves before it is pinned.

**Reference practice.** Files as agent input and as room knowledge each appear in 2 of 5 references; crystallisation in 1 of 5, planned only. QM lists attachments newest-first under caps (10 documents, 8 MB), authorises each against the current ACL, dedupes by sha256, extracts with heap and time caps, picks native file input or text per provider, emits truncation notices and stores only references [SRC]. Grok Bot stages uploads with background captions and deletes on discard [REA]. Slack's auto-summary rests on public docs [DOC].

**Verevon today and approach.** The room composer has no attach control (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx); Chat's ephemeral extract and durable document routes are not wired to Spaces (apps/gateway/src/domains/chat/documents.rs:1-60). Room agents read only through the retrieval binding (apps/Data Plane v2/infra/postgres/migrations/20260813130000_space_retrieval_bindings.sql:1-30), and Control's import decision is personal-Space only (apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go:261-272), so no durable room upload can be authorised today. wiki-store SubmitProposal and ReviewProposal exist (apps/Data Plane v2/services/wiki-store-go/internal/grpcserver/server.go:343-380) but no room offers them. Sequence: first a Control shared-import contract plus a Data Plane upload into the retrieval binding, so files become knowledge agents can already read; then per-turn native attachment input; then propose-and-review pinning on wiki proposals. Do not start with per-turn references in session-core.

**Dependencies and risks.** [F-t1-013](#F-t1-013) owns the composer attach pipeline (confirm to avoid double counting), [F-t1-015](#F-t1-015) canvas; ZDR refuses the attachments field (model-gateway/src/zdr_features.rs:286-298) and durable import, so ZDR rooms need the ephemeral route or a typed refusal; attachment text is untrusted.

<a id="F-t1-047"></a>
### F-t1-047 · Webhooks that wake room agents (Space-bound inbound endpoints)

*Verevon today: Partial · Effort: L · Plane: Ingestion Plane (receive, verify) with Model Plane capability-core (trigger) and Control (fire decision) · Wave: W3 · Confidence: High*

**What and why.** Let a Space manager create a secret webhook endpoint bound to a room and an agent, so a pull request, a payment dispute or an alert starts work in the room. Ingestion verifies, dedups, filters and caps the delivery; a Control-decided unattended turn consumes the payload as untrusted data. The same plumbing unblocks per-agent email ([F-t1-075](#F-t1-075)).

**Reference practice.** 2 of 5 references have it. QM's WebhookReceiver runs per-provider verifiers (GitHub, Slack, Stripe, Linear, generic HMAC) with constant-time compare, derives a delivery id for dedup, filters by JSON path, caps the payload at 16,000 characters in a wake envelope wrapped as untrusted content, and dispatches it like a cron fire under the endpoint owner's authority; the secret is shown once [SRC]. Buzz adds outbound calls with SSRF protection [SRC].

**Verevon today and approach.** integration-corev2 already receives org-level provider webhooks with signature checks for a fixed catalogue, dedup, storage and a NATS reference publish (apps/Ingestion Plane/integration-corev2/internal/api/server.go:981-1100, 2408); conversation-core's inbox is the only subscriber. Nothing is Space-bound, and the wake path is not ready: Control's schedule-fire decision requires a personal Space and refuses ZDR (apps/Control Plane/user-core/internal/spaces/schedule_fire_decision.go:83-101), the scheduled step is one tool-free 1024-token call (apps/Model Plane/rust/services/execution-core/src/grpc.rs:498-528), and the gateway reports watches_not_built (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:1622-1627). Model the endpoint as a webhook source kind of the S4.3 watch primitive (Space and member authority, bounded predicate, cursor, unscreened trust level) rather than a new trigger beside schedules, keeping an endpoint-id route family in integration-corev2 with one-time secrets. Add the Control shared-room fire decision first. Ship notify-and-summarise through the tool-free lane and gate tool use on S4.1 effects closing. Not a quick win.

**Dependencies and risks.** Control shared-room fire decision (critical path), S4.4 delivery; payloads are attacker-controlled and never get tool access in the same turn; ZDR rooms drop payloads; at-least-once delivery needs idempotent fire keys.
