<a id="F-t1-059"></a>
### F-t1-059 · Do not disturb, quiet-hours schedules and urgent break-through, visible to agents

*Verevon today: Stub · Effort: M · Plane: Application Plane · Wave: W3 · Confidence: High*

**What and why.** Give each person a do-not-disturb (DND) state: a manual pause, a recurring schedule with time zone, a VIP list and a rate-limited urgent break-through. While DND is on, non-critical signals from non-VIP actors go to the inbox only and flush when it ends. Agents work around the clock and will page people at night unless this gate ships before push ([F-t1-035](#F-t1-035)); an agent that can ask "may I interrupt now" can defer a non-urgent ask instead of blocking a run.

**Reference practice.** 2 of 5 references have DND. Slack combines a manual pause, a schedule, admin default hours, a VIP override and a once-a-day "notify anyway" break-through [DOC]. Teams adds urgent messages and priority contacts that pass DND and automatic DND in meetings [REA]. Buzz and QM have none [SRC].

**Verevon today and approach.** Control user-core already stores a per-user QuietHours setting (enabled, start, end; default off) and a manual availability status, but nothing enforces or exposes them (apps/Control Plane/user-core/internal/http/handlers.go:999-1014, 225-262). notification-core's only gate is a per-event opt-out with no time window, and a suppressed request creates no inbox row (apps/Application Plane/notification-core/internal/notification/service.go:326-360) [SRC]. Extend the user-core setting with time zone, days, dnd_until and VIP, project it into notification-core by event (identity_sync pattern), and add a tri-state gate (deliver, inbox-only, defer) after the preference check. Urgency is set by the trusted producer with a per-agent daily cap; agents get only a narrow "interruptible now" read bound to the run's subject.

**Dependencies and risks.** Needs Space producers from [F-t1-008](#F-t1-008); the deferred flush rides on the S4.4 ledger ([F-t1-060](#F-t1-060)). Never disclose the schedule to agents or members.

<a id="F-t1-060"></a>
### F-t1-060 · Finish the S4.4 delivery ledger and add a content-free missed-activity digest

*Verevon today: Partial · Effort: L · Plane: Application Plane · Wave: W3 · Confidence: High*

**What and why.** Finish notification-core's delivery ledger (bounded retries, max-age expiry, Zero-Data-Retention (ZDR) proof, delivery errors reported back to the producing agent run) and add a per-user hourly or daily digest that pulls absent humans back when agents are blocked on them. The ledger is the base for push, email and scheduled output without lost or duplicated messages.

**Reference practice.** Outbox 2 of 5 references, digest 2 of 5. QM's DeliveryStore does enqueue, claim-pending and ack with idempotency keys, max-age expiry and five retries, reports undeliverable errors to the source session, and its morning digest auto-pauses when the person is inactive [SRC]. Buzz has a content-free mention outbox [SRC]; Teams offers a missed-activity email [DOC].

**Verevon today and approach.** A leased delivery-attempt state machine with request-level idempotency keys exists, but the worker is off by default, has no bounded-retry or max-age policy, and rejects ZDR requests outright (apps/Application Plane/notification-core/internal/notification/service.go:296-302, delivery_worker.go:91-94). The Space ledger is a named gap (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:1616-1627), and insight-core's daily-brief scheduler is disabled because an organisation is not a recipient [SRC]. Treat finishing the ledger as planned S4.4 work and build no parallel Space ledger. Make the digest a separate Application Plane item: a per-user scheduler modelled on the briefs scheduler that composes counts and links from Application-owned state, not a capability-core schedule. A ZDR-org digest needs an explicit content-free request class first; links must resolve through normal membership checks.

**Dependencies and risks.** Signals from [F-t1-008](#F-t1-008) and the inbox from [F-t1-009](#F-t1-009); email through Novu is a content-persisting boundary whose provider proof is still open.

<a id="F-t1-061"></a>
### F-t1-061 · 'Catch me up' recap of what I missed, scoped to my own read access

*Verevon today: Partial · Effort: M · Plane: Model Plane (recap call); Frontend Plane (entry point) · Wave: W3 · Confidence: Medium*

**What and why.** Add a "Catch me up" action on the room header and the Spaces list that summarises unread messages and agent runs since the viewer arrived, prioritises mentions and items needing action, offers scopes (unread, last 7 days, custom) and mark-as-read on completion. For a returning person in an agent-heavy room this answers "what did the agents do and what needs me".

**Reference practice.** 2 of 5 references. Teams does a cross-thread recap prioritised by relevance and marks chats read when dismissed [REA]. Slack offers unread, last-7-days and custom-range scopes plus a recurring-summary workflow step [DOC].

**Verevon today and approach.** The feature is absent; the only related thing is the Meeting Scribe template, which summarises when mentioned and posts to the whole room (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceCreateAgentDialog.tsx:35-60). The substrate is shipped: a per-member room read marker (apps/Application Plane/convex-core/convex/spaceReadMarkers.ts:1-60), a client arrival snapshot and unread set (apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-unread.ts:1-45), and viewer-scoped transcript and work reads under Control decisions (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:2607-2640) [SRC]. Build it as a gateway-composed stateless summarise call over threads read under the viewer's own decisions, bounded in thread count, using the arrival snapshot as the window start. Keep the result viewer-private and ephemeral; never route it through a room agent, which would post it. Do not put it in the Inbox, which is the support-ticket inbox.

**Dependencies and risks.** Read markers from [F-t1-009](#F-t1-009) already exist at room level. A ZDR turn is a single stateless call, which fits; a persisted private result raises effort to L.

<a id="F-t1-062"></a>
### F-t1-062 · Space UI quality baseline: locale-correct activity grammar and dates, server-held preferences, shared announcer, reduced motion, density and high contrast, failure screens

*Verevon today: Partial · Effort: M · Plane: Frontend Plane (with Control Plane for preferences) · Wave: W2 · Confidence: High*

**What and why.** Move the hard-coded Norwegian activity labels and forced nb-NO dates onto the i18n catalogue and viewer locale, pass the locale to Model Plane turns, read the server-held preferences at bootstrap, add a shared screen-reader announcer with row narration, forced-colors and prefers-contrast rules, a density attribute, and a root error boundary with a cannot-reach page. These are the places where an otherwise strong bilingual, accessible Space UI would regress as rooms grow.

**Reference practice.** Localisation 3 of 5 references, theming and high contrast 4 of 5, friendly activity copy 2 of 5. Grok Bot keys two hidden live regions by a counter so repeated messages announce and makes outgoing panes inert until animation ends [REA]. Teams ships 63 locales with a regional fallback map and start-up milestones; Slack has a cannot-connect page with log download [REA]. Buzz gates motion and offers three density levels [SRC].

**Verevon today and approach.** RUN_STATUS labels and verbs are Norwegian literals (apps/Frontend Plane/verevonv3/src/features/spaces/lib/activity-grammar.ts:118-137) and both date formatters force nb-NO (lib/space-thread-presentation.ts:73,90); there is no forced-colors rule in global.css and no error boundary (apps/Frontend Plane/verevonv3/src/shared/lib/create-resource-compat.ts:5) [SRC]. Two sub-items already exist: every Space animation is gated by prefers-reduced-motion, and user-core serves language, timezone, date format and compactMode through /api/v1/preferences (apps/Control Plane/user-core/internal/http/server.go:228) [SRC]. Drop those; make the frontend read the existing preferences (localStorage only as cache), route labels and dates through useI18n().tr and localeIntl(), wire compactMode as the density attribute, and spike Solid 2 boundary semantics before the fallback page.

**Dependencies and risks.** The timezone-to-region mapping in user-core handlers.go may be a bug (low confidence); check it before Space code depends on it. Diagnostic exports must exclude secrets and ZDR content.

<a id="F-t1-063"></a>
### F-t1-063 · Agent identity in the room: surface the persisted avatar colour (then shape) and add a hover card that leads with blocking state

*Verevon today: Partial · Effort: S · Plane: Frontend Plane (gateway passthrough); Application Plane for shape later · Wave: W1 · Confidence: High*

**What and why.** Render the avatar colour that the Application Plane already stores on every agent post, card, roster row, sidebar entry and mention picker, and add a hover or focus card for agents and members that leads with what blocks a human (awaiting approval), then a draft, then the last entry. Agents must be instantly distinguishable from people and never look like a colleague.

**Reference practice.** Avatars and hover card 1 of 5 references each. Grok Bot builds a deterministic non-human character from 14+ shapes by 11 colours, reuses the colour as an accent in agent rows, and orders its hover card as awaiting reason, draft, last entry, opened on delay and on keyboard focus [REA]. Buzz has a BotIdenticon and an agent badge naming the owner [SRC].

**Verevon today and approach.** The create dialog collects one of ten colours, the gateway forwards it and Convex stores and projects it (apps/Application Plane/convex-core/convex/spaceAgents.ts:194, 441), but the gateway composition field map omits avatarColor (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:4096-4160), the SpaceAgent type has no field (apps/Frontend Plane/verevonv3/src/shared/api/spaces-client.ts:704-733) and cards render an initial (SpacePage.tsx:911-915) [SRC]. Slice 1 (S, no plane work): add the field to the map and type, and render one shared AgentAvatar with the current initial or Sparkles glyph as fallback. The hover card is S to M on @kobalte/core hover-card, which the pinned alpha ships, reading the live-run projection and AWAITING_APPROVAL state. Shapes and uploads are a later Application Plane follow-up.

**Dependencies and risks.** Colour is presentation only and must never enter an authority decision; the card shows other members only "waiting on owner", never approval details or controls. An agent Control authorises but Application has not described keeps a neutral avatar.

<a id="F-t1-064"></a>
### F-t1-064 · Room creation flows: templates with an agent roster and seed artifacts, clone, project and case Spaces bound to work objects, invite-on-mention

*Verevon today: Partial · Effort: L · Plane: Application Plane (templates, bound Spaces) with Control Plane (kinds, decisions) · Wave: W4 · Confidence: Medium*

**What and why.** Let a room be created from a server-side template (instructions plus an agent roster with binding policy) or cloned, offer to add a non-member or bind an unbound agent when someone mentions them, and later create case and project Spaces bound to tickets and projects. Templates with an agent roster turn recurring work into ready-made human-plus-agent rooms, something neither Slack nor Teams has.

**Reference practice.** Templates 2 of 5 references (Buzz, Teams), resource-bound rooms 1 of 5 (QM), mention-to-invite 1 of 5 (Buzz). Buzz instantiates a template by creating the room and binding each agent through the normal decision, and on send returns non-member mentions so the client can offer an invite through the normal add-member path [SRC]. QM keeps channel-derived project members separate so unlinking removes only them [SRC]. Slack channel templates that pre-seed lists rest on docs only [DOC].

**Verevon today and approach.** Named room creation takes a name only and refuses project and case kinds (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs:2873) although Control and the Convex schema accept them; the only templates are three client-side prompt presets, and the composer offers only bound agents and roster people as mention candidates (apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx:184-203) [SRC]. A room is born pending_registration, so "one transaction" is really a resumable saga: create, await registration, set instructions, bind each agent through its own Control confirmation. Split into templates (M), invite-on-mention (M), case and project kinds after an ADR defines owner and lifecycle, and seed artifacts after [F-t1-058](#F-t1-058).

**Dependencies and risks.** Templates must never pre-grant tools, a mention never grants, and the offer must not reveal whether a private candidate exists. Owner-plane ACLs stay authoritative for bound Spaces.

<a id="F-t1-065"></a>
### F-t1-065 · Governed Space skills and plugin bundles (ecosystem-compatible manifests)

*Verevon today: Partial · Effort: XL · Plane: Model Plane (skill store, plugin registry); Control Plane (publish decisions); Ingestion Plane (git pack import) · Wave: W4 · Confidence: Medium*

**What and why.** Give skills a Space scope and a review lifecycle (draft, reviewed, published, archived; any edit invalidates review), with a Control decision on who may publish. Later, import skills from Claude-style and Cursor-style plugin bundles. A team can then curate room-specific know-how safely, which is how agents improve in a shared room.

**Reference practice.** Governed skills 2 of 5 references, plugin bundles 2 of 5. QM runs draft to reviewed to published to archived with signed manifests verified on resolve, visibility filtering per principal and scope, and git packs pinned to a ref [SRC]. Grok Bot reads both .cursor-plugin and .claude-plugin marketplace manifests, pins git sources to a 40-hex sha, declares a variables schema and registers each component with its subsystem [SRC].

**Verevon today and approach.** Skills exist with org and user scope only (apps/Model Plane/rust/services/session-core/migrations/0031_agent_skills_ownership.sql:1-25), are resolved server-side and picked per turn with the composer's / picker, but have no Space scope, lifecycle or signing. The plugin registry is a record store: rows default disabled, the wire type carries a manifest URL and nothing loads one (apps/Model Plane/go/services/capability-core/internal/api/plugins.go:1-70) [SRC]. Split it. Governed Space skills (L): extend session-core agent_skills, the live store the resolver reads, with scope space, review-state columns and immutable reviewed versions, plus a new Control publish decision. Ecosystem plugins: there is no plugin host, so defer the fan-out; start only with importing static skill markdown as draft Space skills requiring review, sha-pinned, refusing hooks and MCP components.

**Dependencies and risks.** S3.5 must first pick the canonical skill record. A skill body is prompt text, so the review gate must cover the exact stored body; Space-level plugin activation waits for a record to write to.

<a id="F-t1-067"></a>
### F-t1-067 · Duplex voice for agents: a voice front that delegates to the Space agent, then agents as attributed participants in room calls

*Verevon today: Partial · Effort: L · Plane: Model Plane · Wave: W4 · Confidence: High*

**What and why.** Phase A: inside a Space, a person holds a realtime voice conversation with a fast voice persona whose only tools delegate, send_task handing real work to the existing agent run (still behind approval gates) and a history search over Space threads, with per-agent voice, speed and language. Phase B: a server-side participant per invited agent hears a room call and answers with an attributed track. This is the defining hybrid moment: humans and agents in one voice room, and an agent tasked hands-free.

**Reference practice.** 1 of 5 references each. Grok Bot's realtime session exposes send_task, recall and search_conversations tools, announces worker results as async updates, allows one active voice chat per user and keeps per-agent voice settings [SRC]. Buzz publishes agent audio under the agent identity, flushes on about 300 ms silence, cancels agent speech on any human speech and speaks only when addressed [SRC].

**Verevon today and approach.** Model Plane already serves POST /v1/ai/realtime/session with server VAD and a ZDR fail-closed route (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:4900-4945), but the request has no tools field (apps/Model Plane/proto/model_plane/v1/inference.proto:1156-1175), the BFF has no route and the shipped voice mode is half-duplex and mounted only in the dashboard composer [SRC]. No call substrate exists anywhere. Add a BFF route that mints a session with Space-scoped instructions and a tool schema; because the browser holds the provider socket, send_task and search execute in the browser through the existing composer and read routes, and per-minute cost stays unmetered until a Model-side relay exists. Phase A is L; Phase B is XL and waits for [F-t1-029](#F-t1-029).

**Dependencies and risks.** ZDR Spaces get no duplex voice today; a personal Space cannot hold an agent; consent from [F-t1-050](#F-t1-050) before an agent hears a room. Always label agent voices.

<a id="F-t1-005"></a>
### F-t1-005 · Make zero-data-retention binding in every plane, with typed refusals and content-free telemetry

*Verevon today: Partial · Effort: S · Plane: Model Plane (session-core, model-gateway, sandbox-manager) with Application Plane and Frontend Plane · Wave: W1 · Confidence: High · moved down from Priority by verification*

**What and why.** Zero-Data-Retention (ZDR) is a Verevon promise and an architecture rule. The remaining work is hardening: commit the untracked refusal catalogue and matrix test, type two refusal sites, cross-check the Space decision flag, record the per-Space ZDR product decision and guard telemetry labels. Regulated buyers look for exactly these seams.

**Reference practice.** ZDR blocking persistence: 1 of 5 references. Grok Bot blocks its persistent computer (sandbox) under privacy mode with a typed no-storage error that the UI maps to a visible block state; that is the one confirmed feature, not a general pattern [SRC]. Consent-deferred client analytics 1 of 5 (Grok Bot, 256-event cap, disabled on opt-out) [SRC]; Teams has a diagnostic data viewer [REA]; QM sends error context unredacted when enabled, an anti-pattern [DOC].

**Verevon today and approach.** Session Core already enforces ZDR on the verified token claim: authorize_operation refuses non-read scopes on CreateThread, AppendMessage and StartRun, and memory preflight gates durable and Letta memory (apps/Model Plane/rust/services/session-core/src/auth.rs:262-285, memory_grpc.rs:105-122). Only the Space decision's own flag is decoded and discarded (grpc.rs:1735-1775), unreachable today. A typed catalogue (apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs:1-140), posture matrix and SPA module exist but are untracked in git; Session Core refusals are plain strings; per-Space ZDR is "not applied in v1"; the README's zdr-contract-sync script is missing [SRC]. Do the five hardening steps above; add a sandbox-manager flag only if leases become reachable by ZDR callers.

**Dependencies and risks.** A missing claim already fails closed. Applying per-Space ZDR needs a Control decision and is a separate M or L item; under ZDR, approvals cannot resume by design.
