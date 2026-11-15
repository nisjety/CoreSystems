# Verevon Space: backend and authority baseline (code is ground truth)

Auditor role: Verevon baseline auditor (backend and authority). Read-only audit, nothing was changed.
Repo: `C:/dev/CoresSystem`, HEAD `afb83928` (2026-10-10 20:18 +0200). The working tree has about 311 uncommitted paths, but none under the Space-critical paths I read (`gateway/src/domains`, `convex-core/convex`, `user-core/internal/spaces`). I did not run anything: no builds, no tests, no services.

Provenance tags: **SRC** means read directly in source. **DOC** means taken from internal docs. **INF** means my inference, with the basis given. "Shipped" means **present in source**. It does not mean deployed. Every plane status doc says the changed sources are not deployed and that release gates R-1 to R-5 are still open (`apps/Application Plane/APPLICATION_PLANE_STATUS.md:1-12`; QM plan tracker `docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md:211-229`).

Path abbreviations used below:

- `GW` = `apps/Frontend Plane/verevonv3/apps/gateway/src/domains`
- `FE` = `apps/Frontend Plane/verevonv3/src`
- `CVX` = `apps/Application Plane/convex-core/convex`
- `UC` = `apps/Control Plane/user-core`
- `SC` = `apps/Model Plane/rust/services/session-core`
- `MG` = `apps/Model Plane/rust/services/model-gateway/src`
- `CAP` = `apps/Model Plane/go/services/capability-core`
- `NC` = `apps/Application Plane/notification-core`
- `CC` = `apps/Application Plane/conversation-core/conversation-core-go`

---

## 0. One-paragraph verdict

Verevon's Space is an **authority-first, agent-first room**, not a chat product. Its backend has four layers:

- **Application (Convex)** owns the Space aggregate: identity, kind, lifecycle, names, agent bindings, presence, read markers and the deletion saga.
- **Control (user-core)** owns membership and roles. It issues short-lived Ed25519 decisions for each effect: thread create, append and read, retrieval, import, schedule, watch, owner grants and sandbox.
- **Model Plane Session Core** stores every room "post" as a durable *chat thread* (`origin="space"`). Every post is a model turn.
- **The Rust BFF** (`GW/spaces.rs`, 9,882 lines and 104 tests) composes all of this, strips forged authority fields, and reports each upstream gap by name.

The authority model, receipts, binding policy, deletion receipts and member-revocation propagation are strong, and ahead of the reference apps. What the code does **not** have:

- There is no human-to-human messaging path. Every post goes to the model (SRC `FE/features/spaces/components/SpaceRoomComposer.tsx:402-420`).
- There is no push realtime. The room polls every 6 seconds.
- There are no DMs or group DMs, no reactions, no edit or delete, and no attachments in rooms.
- There are no Space-originated notifications.
- Personal agents are not possible, because a personal Space cannot hold an agent.
- There are no calls or meetings.
- Non-owners cannot reply into another member's thread.
- Approval grants are recorded, but the paused run is not resumed through that path.
- Nearly all shared-room features are gated on per-org `space_effect_policies` rows. Those rows are deny-by-default, and per the docs they do not exist in dev.

---

## 1. Messaging core

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Room post = Model Plane thread | partial | Every room message is sent through `streamChat` with `spaceRef`. The gateway (`inject_personal_thread_context`) obtains a Control `thread:create` decision and passes it to Model Gateway. Session Core persists the thread with `origin="space"` and `space_id`, and a CHECK constraint enforces the pairing. Each post therefore *always* produces an AI turn. | SRC `FE/features/spaces/components/SpaceRoomComposer.tsx:402-420`; `GW/spaces.rs:54-293`; `SC/src/grpc.rs:2252,2301-2330`; `GW/chat/json_handlers.rs:97-130` |
| Human-only message (no AI reply) | **absent** | No route posts a human message without a model invocation. The doc claims "if no agent is called, the normal human-only flow continues"; the code does not do that. | Searched the `GW/spaces.rs:649-737` router, `FE/.../SpaceRoomComposer.tsx` (only `streamChat`), and `GW/chat.rs:17-78` |
| Shared thread listing for all members | shipped (policy-gated) | The gateway mints a Control `thread-read-decision` (`model.thread.read`, viewer floor) and passes it to Model Gateway `/v1/threads?space_id=...`. A 403 or 404 from Control degrades the listing to owner-bound. The gateway also verifies that every returned thread belongs to the Space. | SRC `GW/spaces.rs:1086-1264,1268-1374,2734-2745`; `UC/internal/http/server.go:359`; `UC/migrations/026_space_thread_read_effect_policy.up.sql`; `SC/src/grpc.rs:5320-5345` |
| Room transcript read "as the room" | shipped (policy-gated) | This route is separate from Chat's owner-bound transcript route. Session Core drops rows stamped with a newer audience revision than the decision. | SRC `GW/spaces.rs:2616-2732`; `SC/src/grpc.rs:5340-5360` |
| Reply into a thread | partial | The gateway mints a content-digest-bound `thread:append` decision for each reply. However, Session Core `append_message` still calls `authorize_thread_owner(..., Mutate)`, so a **non-owner reply is refused**. The UI offers Reply on any thread. | SRC `GW/spaces.rs:120-184`; `SC/src/grpc.rs:4397-4416`; `FE/features/spaces/components/SpacePage.tsx:581`; DOC `docs/SPACE_PAGE_AUDIT_2026-09-06.md` §7 "Replies by a non-owner still fail" |
| Mid-stream steer (queued input) | shipped | `POST /api/v1/chat/invocations/{id}/queue` re-mints the append decision for each message. | SRC `GW/chat.rs:41-43`; `GW/chat/json_handlers.rs:165-205` |
| Stop / cancel recorded | shipped | The composer aborts its SSE read, then calls cancel, which flips the run to `cancelled`. | SRC `GW/chat.rs:35-37`; `FE/.../SpaceRoomComposer.tsx:37-50` (doc comment) |
| Author attribution per turn | shipped | `messages.author_subject_id` is written from the verified caller and is never taken from the request. Agent attribution uses `agent_name`. | SRC `SC/migrations/0035_message_author_attribution.sql`, `0030_message_agent_attribution.sql`; `SC/src/grpc.rs:2570-2590` |
| Retitle / pin a post | partial (owner-only) | `PATCH .../threads/{id}/presentation` checks lifecycle and membership, then calls the Session Core presentation write, which is owner-bound. A non-owner gets `thread_presentation_owner_only`. Pinned threads sort first. | SRC `GW/spaces.rs:2183-2340`; `apps/Model Plane/proto/model_plane/v1/sessions.proto:1034-1056` |
| `@agent` mention invokes a bound agent | shipped | The browser sends only `mentioned_agent_ref`, which is the Control subject id. The gateway re-resolves Control roster ∩ Application binding and injects `agent_name` and `agent_system_prompt`. | SRC `GW/spaces.rs:4175-4216,4399-4498` |
| `@person` mention | stub | The mention is inserted as text only (`@Name`). There is no notification and no server-side mention record. | SRC `FE/.../SpaceRoomComposer.tsx:187,308`; notification allowlist `NC/internal/http/handlers.go:197-215` |
| Markdown, code, citations render | shipped (frontend) | The room reuses `ChatMarkdown`. | SRC `FE/.../SpaceRoomComposer.tsx:3` |
| Edit or delete one message | absent | Searched Model Plane proto for `EditMessage`, `UpdateMessage` and `edit_message`: no hits. The Spaces router has no delete route. Whole-thread `DELETE /v1/threads/:id` exists for the owner through Chat only. | SRC `MG/http_routes.rs:144-160` |
| Reactions / emoji | absent | Searched `reaction` and `emoji` in Session Core src and migrations and in `GW/spaces.rs`: none. | — |
| Attachments / link previews in room | absent in room | The room composer has no upload path (searched `attach`, `upload`, `file`). Chat has `/api/v1/chat/documents`, but the room does not use it. | SRC `GW/chat.rs:64-68` |
| Presence + typing | shipped | `POST /spaces/{ref}/presence` takes `online`, `typing` or `offline`, writes the caller's own row, and returns who else is present. TTL is 30 s and typing decays after 8 s. There is no content in rows. | SRC `GW/spaces.rs:1918-2061`; `CVX/spacePresence.ts:42,50,122`; `CVX/schema.ts:1003-1011` |
| Read state | shipped (room-level only) | A per-member `lastReadAt` row is stored, and unread is derived client-side from thread `updated_at`. There is no per-thread or per-message read receipt. | SRC `GW/spaces.rs:1894-1916,2063-2181`; `CVX/spaceReadMarkers.ts:50,123`; `CVX/schema.ts:845-851` |
| DMs / group DMs | absent | Space kinds are only `personal`, `room`, `project` and `case`. Create accepts only `personal` and `room`. | SRC `CVX/schema.ts:546-556`; `UC/migrations/017_space_authority.up.sql:10-27`; `GW/spaces.rs:2837-2955` |
| Drafts, scheduled send, polls, forwarding, voice clips, saved items | absent | No Space or room route or schema for any of these (Spaces router, Convex schema, Session Core migrations). Inbox drafts exist only for support conversations. | SRC `GW/inbox.rs:45-55` (support only) |

## 2. Org & membership

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Space aggregate + lifecycle | shipped (source) | Convex `spaces` holds `spaceRef`, kind, name, lifecycle (`pending_registration`, `active`, `suspended`, `deleting`, `deleted`, `failed_registration`), revision and instructions. A transactional outbox delivers lifecycle events to Control registration. A Space is unusable until Control acknowledges it, and the gateway returns 202 rather than 201 for creation. | SRC `CVX/schema.ts:546-604,680-716`; `CVX/spaceRegistration.ts:26`; `CVX/spaces.ts:92,1325-1500`; `UC/internal/http/server.go:345` |
| Control membership and roles | shipped | `registered_spaces`, `space_authority_revisions` (aggregate, membership, privacy, audience and entitlement revisions) and `space_memberships` (subject_type `user` or `service`, roles `viewer`, `editor`, `manager` or `owner`). | SRC `UC/migrations/017_space_authority.up.sql:10-66` |
| Space listing | shipped | Control's actor-filtered index is intersected with Application labels. If Control is unavailable, only the caller's personal Space is shown. | SRC `GW/spaces.rs:2747-2835,4575-4634` |
| Organization room (#general) | partial | `ensureOrganizationRoom` is idempotent. Its roster comes from active org-core members, mapped to `editor`. A sync runs when registration is acknowledged. `syncAllOrganizationRooms` exists, but there is no `crons.ts` and no caller, so later org-roster changes are not re-synced automatically. | SRC `GW/spaces.rs:2949-3017`; `CVX/spaceMembershipSync.ts:1-64,135-147`; `CVX/spaces.ts:1441` |
| Create a named room | shipped | Any active org member can create one (`requireGatewayMember`). The creator becomes the registration owner. | SRC `GW/spaces.rs:2873-2905`; `CVX/spaces.ts:364-406` |
| Add / remove a person | partial | Gated to owner or manager (Control role, resolved under the caller's delegation). Convex `spaceMemberGrants` is declared to Control with `managed_subject_types:["user"]`, so a human roster change cannot remove agents. Grants are always `editor`. Personal Spaces and the org room are refused. | SRC `GW/spaces.rs:3399-3542,3669-3718`; `CVX/spaceMembers.ts:31-44,45,98,206`; `CVX/schema.ts:821-836` |
| Change a member's room role | absent | `role: v.literal("editor")`. No route. | SRC `CVX/schema.ts:831`; DOC audit §10 |
| Roster read | shipped | Straight proxy to the Control roster. A non-member gets 404. | SRC `GW/spaces.rs:3013-3052`; `UC/internal/http/server.go:351` |
| Personal Space (one per principal) | shipped | Uniqueness via the `by_personal_owner` index and an ensure mutation. | SRC `CVX/schema.ts:600-603`; `CVX/spaces.ts:158-296`; `GW/spaces.rs:2925-2955` |
| Project / case Spaces | absent in product | Control accepts all four kinds, but the gateway refuses any kind other than `personal` or `room`. | SRC `GW/spaces.rs:2879-2890` |
| Guests / external / shared channels | absent | Rosters come only from active org members or org members added by hand. External identity linking is only an ADR (S5.1). | SRC `CVX/spaceMembershipSync.ts:34-48`; DOC QM plan S5.1 |
| Room discovery / browse / join | absent | The listing shows only Spaces in the caller's Control index. There is no public-room browse or self-join route. | SRC `GW/spaces.rs:2747-2810` |
| Org members, invites, roles | shipped (org level) | `/api/v1/orgs/{id}/members[/invite]`, `/members/{user_id}/role`, `/roles`, and invitation accept. org-core has RBAC with a capability catalog. | SRC `GW/orgs/members.rs`, `GW/orgs/roles.rs`; `apps/Control Plane/org-core/internal/rbac/catalog.go:15-60` |
| Teams / sub-groups inside an org | absent | Searched org-core Go for "team": no hits. Ticket teams in conversation-core are support routing only. | SRC `CC/internal/http/server.go:92,135` |
| Room deletion | partial | Only personal Spaces can be deleted. Control rejects non-personal deletion even when entitlement is on. | SRC `GW/spaces.rs:960-1084`; `UC/internal/spaces/deletion_authorization.go:17-22`; `UC/internal/spaces/repository.go:228-230` |

## 3. Calls & meetings

| Capability | Status | Evidence |
|---|---|---|
| Audio / video calls, huddles, screen share between members | absent | No route, schema or service. Channel Plane is docs-only. SRC search of the `GW` router list; DOC `apps/Channel Plane/docs/meeting-intelligence.md:1-8` |
| Remote desktop assistance (support agent controls a customer PC) | partial | The gateway serves only the config (`/api/v1/remote-support/config`). Sessions run browser to hbbs/hbbr (RustDesk) and never pass through the gateway. There is no durable session or audit record. SRC `GW/remote_support.rs:1-30`; DOC `apps/Support Plane/README.md:1-12` |
| Meeting transcription, recap, minutes | planned | WhisperX ASR, keyframes and wiki minutes are planned. DOC `apps/Channel Plane/docs/meeting-intelligence.md` (Phasing) |
| AI realtime voice | partial (AI only) | `/v1/ai/realtime*` exists in Model Gateway. It is not a human call. SRC `MG/http_routes.rs:448-450` |
| Calendar / scheduling of meetings | absent | Only agent cron schedules exist (§6). |

## 4. Search & discovery

| Capability | Status | Evidence / notes |
|---|---|---|
| Space knowledge tab (documents + wiki bound to Space) | shipped (source); empty in practice | A Control `retrieval.read` decision feeds Data Plane `POST /v1/knowledge/space-sources`, which reads `documents.space_ref` and `space_retrieval_bindings`. Each section reports its own gaps. DOC: no import yet writes `space_ref`, so lists are legitimately empty. SRC `GW/spaces.rs:1664-1892`; `apps/Data Plane v2/services/retrieval-engine-rs/src/api/mod.rs:498,573-590`; DOC audit §13 |
| Grounded room turns (retrieval) | shipped (policy-gated) | A separate `retrieval_read_entitled` bit is checked. A 403 degrades the turn to ungrounded. SRC `GW/spaces.rs:225-280`; `UC/internal/http/server.go:361`; `UC/migrations/019_space_retrieval_effect_policy.up.sql` |
| Search messages / threads in a Space | absent | `GW/search.rs:39-45` is web, image and video search. No Space or thread search route. Conversation search exists only for the support inbox. SRC `CC/internal/http/server.go:74` |
| People directory | partial | Per-Space Control roster plus the org members list. No cross-org directory. SRC `GW/spaces.rs:3019`; `GW/orgs/members.rs` |
| Org-wide agent registry ("installations") | shipped | One Convex query grouped by definition. It answers presence, not authority (ADR-0002). SRC `GW/spaces.rs:3165-3259`; `CVX/spaceAgents.ts:159-286` |

## 5. Notifications & attention

| Capability | Status | Evidence / notes |
|---|---|---|
| Notification feed, unread count, mark read, delete, preferences | partial | The gateway proxies notification-core (`/notifications`, `/preferences/{event}/{channel}`). The service has a Novu delivery mode, but it is disabled by default, and the delivery worker is off. SRC `GW/notifications.rs:17-30`; DOC `NC/README.md:1-60` |
| Notifications from Space events (mention, reply, approval needed, member added to room) | absent | The notification-core allowlist lets the gateway send only `notification.created` to *itself*. capability-core may send only `modelplane.run_completed` and `run_failed`. `mention.received` is seeded in channel config, but nothing produces it. SRC `NC/internal/http/handlers.go:163-215`; `NC/migrations/005_create_channel_configs.up.sql:60-62` |
| "Notify me when this run finishes" | shipped (source) | `/api/v1/runs/{id}/watchers` goes to capability-core, which calls notification-core. This is per user, not per room. SRC `GW/run_watchers.rs:1-56`; `CAP/cmd/main.go:225-303` |
| Space watches (S4.3) | partial | capability-core has `/api/v1/watches` (create, list, cancel) with Control `watch-create` and `watch-observe` decisions, a TTL of 8 h (max 7 d), at most 8 per Space, and a process-output adapter. It is **not exposed** through Model Gateway or the Verevon gateway. Space Activity still reports `watches_not_built`. SRC `CAP/internal/watch/routes.go:15-33`; `CAP/internal/watch/handler.go:21,27`; `CAP/cmd/main.go:329-362`; `SC/migrations/0036_space_watches.sql`; `UC/internal/http/server.go:367-368`; `GW/spaces.rs:1624-1628` |
| Space Activity feed | shipped | Runs (limit 100), per-run approvals for `awaiting_approval` (at most 10), owner effect receipts and the grants that authorized them (Conversation Core `/spaces/:ref/activity`), plus named gaps for `delivery` (S4.4) and `watches` (S4.3). SRC `GW/spaces.rs:1349-1652`; `CC/internal/http/server.go:81` |
| Unread badges | partial | Room-level only, for the open room. There is no cross-room aggregate for unread or presence in the sidebar. DOC audit §16 "What remains open"; SRC `CVX/spaceReadMarkers.ts:50` |
| Mute, DND, quiet hours, digest | absent | Quiet hours are planned in S4.4. Per-event/channel preferences exist in notification-core. DOC QM plan S4.4; SRC `NC/internal/channels/channels.go:1-40` |
| Durable delivery ledger (S4.4) | planned | Named gap `delivery_ledger_not_built`. SRC `GW/spaces.rs:1618-1622` |
| Mobile / desktop push | partial (source only) | Novu adapter. Disabled by default. Provider-side ZDR is not proven. DOC `NC/README.md` |

## 6. Apps & extensibility

| Capability | Status | Evidence / notes |
|---|---|---|
| AI-first action registry for Spaces | shipped | 14 `spaces.*` action IDs (create personal Space or room, ensure org room, instructions, create or bind agent, deletion, presentation, mark read, presence, set or revoke agent, add or remove member) are dispatched by the gateway. A drift guard checks registry ↔ dispatcher parity. SRC `FE/shared/actions/action-registry.ts:1991-2134`; `GW/actions/handlers.rs:476-505`; DOC QM plan S0.2 |
| `/` skill picker in room | shipped | Skills ride as `skill_ids`, resolved server-side in Model Gateway. SRC `FE/.../SpaceRoomComposer.tsx:5,413-417`; DOC audit §17 |
| Space-scoped schedules (cron) | partial | The BFF obtains a Control `schedule-create-decision` bound to a template digest that is Go-hash compatible. capability-core stores `space_ref` and `creator_subject_id`, with a separate fire decision at each tick. Creation stays in Settings. The Work tab lists schedules read-only. SRC `GW/spaces.rs:426-596`; `GW/spaces/schedule_template.rs:1-30`; `CAP/internal/api/workplane_apis.go:886-1000`; `UC/internal/http/server.go:363,370` |
| Owner agent-action grants (`tickets.create`) | partial | An org owner or admin requests a Control `owner-grant-decision`, which is forwarded to Conversation Core. Only `tickets.create` is correlated. SRC `GW/spaces.rs:743-937`; `UC/internal/http/server.go:375`; DOC audit §14 |
| Delivery targets on bindings (Teams, Messenger, embed) | stub | The schema field exists. Only an internal mutation writes it, and no adapter exists. SRC `CVX/schema.ts:644-664`; `CVX/spaceAgents.ts:319-385` |
| External surfaces (Slack / Teams mirror) | planned | S5.1 ADR adopted; S5.3 blocked behind S4.4 and S4.3. Channel Plane is docs-only. DOC QM plan S5.1-S5.3; `apps/Channel Plane/docs/vision.md:17-125` |
| Third-party bots, webhooks, slash-command apps, app directory | absent (Space) | The Convex `webhooks` table is legacy and not Space-scoped. There is no app marketplace. SRC `CVX/schema.ts:257-276` |
| MCP servers (register / share / OAuth) | partial (not Space-scoped) | SRC `MG/http_routes.rs:330-343` |
| Mail-to-task (email an agent) | stub | Verification, rate limit and dedup work, then it always returns `503 mail_task_pipeline_not_configured`. SRC `GW/mail_task.rs:1-30,135` |
| AG-UI protocol stream | shipped | `/api/v1/ag-ui/stream` runs the same Space persona, instruction and authority injection as chat. SRC `GW/ag_ui.rs:58-61,93-121` |

## 7. Agents in the room

| Capability | Status | Evidence / notes |
|---|---|---|
| Agent identity as a room member | shipped | Control `space_memberships(subject_type='service')` answers "who may act". Convex `spaceAgentBindings` answers "what it is called". The join runs Control-first: an unnamed but authorized agent is shown, and an unauthorized binding is dropped. SRC `GW/spaces.rs:3116-3188,4052-4173`; `CVX/schema.ts:606-678` |
| Create a simple agent from the room | shipped | Two steps behind one action: Convex creates the definition plus a `pending` binding, then the service roster is declared to Control and set `active`. Gated to owner or manager, name 2-60 chars, instructions up to 4,000 chars. SRC `GW/spaces.rs:3261-3405,3890-3911`; `CVX/spaceAgents.ts:412-484,709-763` |
| Bind an existing definition | shipped | SRC `GW/spaces.rs:3913-4055`; `CVX/spaceAgents.ts:486-595` |
| Pause, resume, revoke a binding | shipped | PATCH `active`/`paused`; DELETE revokes and re-converges the Control roster. SRC `GW/spaces.rs:3544-3666`; `CVX/spaceAgents.ts:596-665` |
| Binding policy enforced at invocation | shipped (partial set) | `trigger_modes` (without `mention` → refuse), `approval_mode` (`blocked` → refuse; `require_confirmation` or absent → strip `agentic` and `plan_mode`), and `allowed_tools` (filter only). `knowledge_scope`, `default_thread_policy` and `audit_visibility` are deliberately **not stored**. SRC `GW/spaces.rs:4500-4573`; `CVX/schema.ts:620-634` |
| Authored instruction hierarchy | shipped | Platform env, then org, then Space (editor or higher can write; any member can read; up to 4,000 chars), then user persona, then agent persona. The gateway strips any client-supplied value. There is no versioning. SRC `GW/spaces.rs:3720-3888,4231-4398`; `CVX/spaces.ts:1503-1555`; DOC `apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md` |
| Personal agents (Grok-style, per user) | **absent** | A personal Space cannot hold an agent: Control refuses personal membership replacement, and `requireManagedRoom` refuses personal Spaces. Agents can only be born inside a shared room. SRC `UC/internal/spaces/repository.go:670-673`; `CVX/spaceMembers.ts:31-44`; DOC `docs/space-defenition.md` "Open questions" |
| In-room approvals | partial | Approve or deny through the orchestration proxy; owner-bound (`decided_by` is pinned to the caller). Approval decisions are org-scoped (IDOR fix). A **granted** approval is durably recorded, but its continuation is quarantined: there is no generic resume on this path. A denial cancels the run. Only two verbs are offered, because there is no approval scope. SRC `GW/orchestration.rs:64-115`; `MG/http_routes.rs:1505-1570`; `SC/src/orchestration_grpc.rs:2630-2700`; DOC audit §8 |
| Work projection (runs, schedules, processes) | shipped (read-only) | Runs use the room's read decision. Schedules are filtered by `space_ref`. Background processes come from sandbox-manager under a separate audience-pinned read decision. Each section can report its own gap. SRC `GW/spaces.rs:2316-2614` |
| Agent "PC": durable Space workspace | partial | Space-scoped `code_interpreter` gets a sandbox-manager lease (credential-free, no network). The workspace is hydrated and snapshotted at run end. Inline chat `ExecuteStep` runs lose their edits (documented gap). SRC `apps/Model Plane/rust/services/execution-core/src/sandbox_lease.rs:1-60` |
| Agent browser in a Space | absent | Browser-agent runs always send `space_id: ""`. Browser actions are governed by Quarry grants, not by Space bindings. SRC `MG/browser_run.rs:435-441` |
| Background process registry (S4.2) | shipped (read) | SRC `apps/Model Plane/go/services/sandbox-manager/internal/process/store.go`; `GW/spaces.rs:2530-2598` |
| Agent-to-agent delegation in the room (UI-2c) | absent | Subagent machinery exists generically and is approval-gated, but it carries no Space context, and there is no execution-core to gateway path. SRC `apps/Model Plane/rust/services/execution-core/src/runtime_loop/subagent_results.rs:1-30,127`; DOC audit §4 item 8 |
| Memory scoped per room | absent | Memory scopes are `run`, `thread`, `workspace`, `session` and `user`, with no `space` scope. DOC: the Memory page is blocked on a plane decision. SRC `CAP/internal/api/workplane_apis.go:70`; DOC audit §17 |
| Cost per Space / allowance | absent | cost-core has no Space awareness (searched `space_ref`/`space_id`). Only the org `budget_exceeded` hard stop is rendered. DOC audit §17 |
| Chief/Core routing view | partial (read-only) | Org registry plus per-Space roster. Broad visibility does not grant broad authority. SRC `GW/spaces.rs:3192`; DOC ADR-0002 |
| Membership revocation propagated to Model Plane | shipped (best effort) | Control publishes `aqencia.controlplane.space.membership_changed`. Session Core upserts or deletes `space_membership_revocations`. This is plain NATS (not JetStream) and tolerates loss. SRC `UC/internal/nats/shared_publisher.go:196-214`; `SC/src/space_membership_nats.rs:1-80`; `SC/migrations/0032_space_membership_revocations.sql` |
| Tool-call / run event visibility | partial | Run events SSE and replay exist for the run owner. The room renders thread status (`queued`, `running`, `awaiting_approval`) from the 6 s poll. SRC `GW/chat.rs:73-78`; `MG/http_routes.rs:207-220` |

## 8. Collaborative artifacts

| Capability | Status | Evidence / notes |
|---|---|---|
| Document sharing (view-only grants) | shipped | `resource_grants` in user-core is the single authority enforced by retrieval and documents-api. SRC `GW/shares.rs:1-44,90-130` |
| Space knowledge (documents / wiki) | shipped (read) | See §4. |
| Canvases, wikis or docs authored in the room | absent | No Space canvas. Wiki pages are reachable only through a retrieval binding. |
| Tasks / boards | partial (not Space) | Orchestration todos are per thread. Model Plane `/v1/tasks` exists. Neither is a Space board. SRC `GW/orchestration.rs:66-69`; `MG/http_routes.rs:345-347` |
| Planner documents | stub / legacy | Convex `plannerDocuments` uses a resource-local `workspaceId`. ADR-0001 classifies this as a migration candidate. SRC `CVX/schema.ts:277-304`; DOC ADR-0001 inventory |
| Support inbox / tickets / incidents | shipped (separate domain) | The full support desk (conversations, drafts with leases, notes, tags, tickets, SLA, macros, AI-proposal HITL) lives in conversation-core. It is not a Space. ADR-0001 plans to map support chat to a `case` Space. SRC `GW/inbox.rs:1-100`; `CC/internal/http/server.go:62-143` |
| Social publishing (`GW/social.rs`) | n/a | Social-media marketing (posts, campaigns, approvals), not social chat. SRC `GW/social.rs:28-57` |

## 9. Realtime & sync architecture

| Capability | Status | Evidence / notes |
|---|---|---|
| Room updates | partial (polling) | Visible-tab poll every 6 s for the thread projection, a 30 s Space-context recheck, and an extra typing beat. SRC `FE/features/spaces/components/SpacePage.tsx:78,87,276-320` |
| Own-turn streaming | shipped | SSE `/api/v1/chat/stream` plus resume. SRC `GW/chat.rs:17,31-32`; `MG/http_routes.rs:136-142` |
| Push transport to browsers (WS / SSE / Convex subscription) | absent | The v3 SPA has no Convex client (searched `convex` and `ConvexClient` in `FE`; hits only in demo and settings files). DOC audit §2 "Realtime as a push transport: Not built" |
| Transactional outboxes (lifecycle, audience, deletion, owner receipts) | shipped (source) | Leases, retry, at-least-once delivery, idempotent event IDs. SRC `CVX/schema.ts:680-820,873-903`; `CVX/spaceAudienceRegistration.ts:13-22`; `CVX/spaces.ts:1053-1062` |
| Cross-plane events | partial | Membership-change NATS (above). The Convex NATS subscriber exists in `convex-core/nats-subscriber.js`. Per the Application status doc, cross-plane fan-out is unsigned and best effort. DOC `APPLICATION_PLANE_STATUS.md` finding 4 |
| Offline cache, multi-device sync, E2EE | absent | No offline store. Session content is server-side, and E2EE is not present (INF from the transport design: all content passes through the BFF and Model Gateway). |
| Cross-room aggregates (sidebar unread / presence) | absent | DOC audit §16, §18 |

## 10. Governance, security & compliance

| Capability | Status | Evidence / notes |
|---|---|---|
| Signed, short-lived, effect-bound decisions | shipped | Ed25519 signing. Personal decisions last 2 minutes. Decisions bind subject, org, Space, audience (ref, hash, revision), authority revision, privacy policy, action ID, schema hash, payload digest and idempotency key. The BFF strips 12 forged authority fields and conflicting `space_ref`/`spaceRef` spellings. Session Core verifies signature, expiry and privacy completeness. SRC `UC/internal/spaces/decision.go:4-60,145-222`; `UC/internal/spaces/personal_thread_decision.go:21`; `GW/spaces.rs:34-47,60-98`; `SC/src/grpc.rs:1521-1790` |
| Deny-by-default effect policies per org | shipped | `space_effect_policies` has no default row; absence denies. Separate bits exist for thread create, thread read, retrieval, import, schedule fire, agent action, sandbox capability and process registry. SRC `UC/migrations/018-028_*.up.sql` (018 header: "absence must deny issuance") |
| Recipient audience snapshots | shipped | Application stores an immutable recipient set, a SHA hash and a revision, delivered to Control through the outbox. SRC `CVX/schema.ts:853-903`; `CVX/spaceAudience.ts:32-76`; `UC/internal/http/server.go:353` |
| ZDR propagation | partial | The gateway raises turn `zdr=true` when the org enables ZDR (fails closed). Control forbids durable owner actions under ZDR. Session Core *decodes* the decision's `zero_data_retention` claim but does not act on it ("retain ... for later propagation slices"). Notification ZDR keeps payloads out of the ledger, but they still go to Novu. SRC `GW/chat/shared.rs:80-116`; `UC/internal/spaces/personal_thread_decision.go:292-294`; `SC/src/grpc.rs:1749-1755`; DOC `NC/README.md` |
| Space deletion saga with per-plane receipts + legal hold | partial | Personal only. Seven-day receipt deadlines. Final `deleted` only when every owner reports `succeeded`. Migrations 022, 023, 0026 and 0027 are not applied in staging (tracker `[ ]`). SRC `GW/spaces.rs:960-1084`; `CVX/spaces.ts:484-1325`; `CVX/spaceDeletion*.ts`; `UC/internal/http/server.go:346-349` |
| Audit | partial | `/api/v1/audit` proxies audit-core (tool actions). There is no Space-scoped audit view. Space Activity shows owner receipts and authority grants. SRC `GW/audit.rs:1-30`; `GW/spaces.rs:1569-1610` |
| Privacy export / erase (user GDPR) | shipped | Space export is absent. SRC `GW/privacy.rs:39-40` |
| SSO | partial | better-auth `sso()` is enabled only when `SSO_ENABLED=true`. SCIM: searched `scim` in auth-core, user-core and org-core with no code hits. SRC `apps/Control Plane/auth-core/src/auth/auth.ts:1018-1030` |
| Approval tenant isolation | shipped | `org_id` is taken from verified JWT claims on get and decide. SRC `MG/http_routes.rs:1510-1540`; `apps/Model Plane/proto/model_plane/v1/orchestration.proto:719-760` |
| DLP / moderation / eDiscovery for rooms | absent (low confidence) | No Space DLP or eDiscovery route found in the gateway or Control routes I read. Model Plane has content provenance and moderation vocabulary (referenced by ADR-0002), but I did not trace it into rooms. |
| Multi-tenancy | shipped | Every Space call resolves `authorized_org_id` from the verified session. Convex `requireGatewayMember` is checked again per call. The Application service key never reaches the browser. SRC `GW/spaces.rs:4704-4746`; `CVX/spaces.ts:373,428` |

## 11. Client shell

Out of scope for this backend baseline; see `v-space-ui.baseline.md`. The one backend fact that matters here: every Space route sits behind the same-origin BFF `require_session` layer, and the browser holds no upstream credential. SRC `GW/spaces.rs:736`; `CLAUDE.md` "Frontend Plane owns ... same-origin gateway/BFF normalization".

## 12. UX patterns & layout (backend-driven contracts)

| Capability | Status | Evidence |
|---|---|---|
| "Never go dark" contract | shipped | Every composed Space response carries `unavailable[]` with `{section, code, reason}`. This keeps "nothing here" distinct from "could not ask". SRC `GW/spaces.rs:1205-1222,1600-1640,2470-2610,1780-1880` |
| Six-tab cockpit fed by the endpoints above (Chat, Work, Activity, Knowledge, Agents, Members) | shipped (frontend) | SRC `FE/features/spaces/components/SpaceCockpit.tsx`; DOC audit §2 |
| `space_actions` presents the org catalog under a `space_ref` | partial / misleading | It returns `human_owner_action_catalog_for(user)` and only echoes the ref. SRC `GW/spaces.rs:4890-4925`; DOC audit §3.7 |

---

## Planned / in-flight per docs

1. **S4.4 durable Space completion delivery.** An Application `DeliveryTarget`, an outbox and receipts (`pending`, `claimed`, `sent_unconfirmed`, `acknowledged`, `failed`, `unknown`), plus preferences and quiet hours. (DOC `docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md` S4.4)
2. **S4.3 watches**, remaining adapters (runs, ingestion jobs, documents, deployments, connectors) and the read path. (DOC `docs/S4_3_WATCH_PRIMITIVE_DESIGN_2026-09-14.md`; QM plan S4.3)
3. **S4.5 unified Work cockpit** with cancel, steer and reconcile controls. (QM plan S4.5)
4. **S5.1-S5.3 surface-neutral ingress and first external adapter** (Slack preferred), external identity link graph. (QM plan S5)
5. **S5.2 standing orders / web presence** with versioned policy. (QM plan S5.2)
6. **S5.4 governed internal app build and publish.** (QM plan S5.4)
7. **S3.1 / S3.4 / S3.5 / S3.6:** revisioned authored context, Space credential grants and broker, governed Space skills, a Space MCP/plugin cockpit. (QM plan S3)
8. **UI-2c agent-to-agent delegation inside the roster**, with a depth cap, initiating-human chain and collapsed rendering. (DOC `docs/space-defenition.md` "Agent-to-agent collaboration"; `docs/SPACE_AGENT_SCOPE_PLAN_2026-08-14.md`)
9. **Space-authorized presentation write** (room-wide pin and retitle) and **shared append** for non-owner replies. (DOC audit §7, §16)
10. **Member role management, project and case Spaces, rollout Personal → Case → Project/Room.** (DOC audit §2/§10; QM plan S6.3)
11. **Binding fields not yet stored:** `knowledge_scope`, `default_thread_policy`, `audit_visibility`. (DOC `docs/space-defenition.md` "Binding model")
12. **Approval scopes** (Allow once / for conversation / always) need a Model Plane approval scope first. (DOC audit §8)
13. **Cross-room aggregates** (sidebar unread and presence) need a Model Plane per-Space activity aggregate. (DOC audit §16, §18)
14. **Memory page with revision restore.** Blocked on the Model vs Data ownership decision. (DOC audit §17)
15. **Meeting intelligence:** file-upload ASR MVP, then slides, then minutes, then live channel bots. (DOC `apps/Channel Plane/docs/meeting-intelligence.md`)
16. **Channel Plane runtime:** widget-core, adapter-core and external conversation runtime. (DOC `apps/Channel Plane/docs/vision.md`)
17. **S6.1-S6.3:** apply the deletion migrations, chaos matrix, staged rollout. (QM plan S6)
18. **Release gates R-0 to R-5:** canonical delegation, capability health, approval continuation, durable delivery, immutable rollback. (QM plan tracker)
19. **Mail-to-task pipeline:** an unattended execution-core turn. (SRC `GW/mail_task.rs` module doc)
20. **Browser-agent runs carrying Space context.** (SRC comment `MG/browser_run.rs:435-440`; DOC `docs/S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md` §3.5)

## Known gaps

1. There is no human-only message. Every room post is an AI turn and incurs model cost and latency. This is the biggest functional gap against Teams and Slack.
2. Non-owner replies into another member's thread are refused by Session Core (owner-bound `append_message`). The room is effectively "one thread per author".
3. There is no push realtime. The 6 s poll is the floor, and there is no cross-room unread or presence.
4. There are no DMs, group DMs, project or case Spaces, guests, external or shared channels, or room discovery and self-join.
5. Messaging has no reactions, no edit or delete of messages, no attachments in rooms, no message search, no drafts, polls or scheduled send, and person mentions are text only.
6. No Space event produces a notification (mention, reply, approval, invite to room). The notification allowlist blocks it, and the delivery provider is disabled by default.
7. Personal agents are impossible (personal Space membership is immutable). Agents exist only inside shared rooms.
8. A granted approval does not resume the paused run on this path (continuation quarantined). There is a single approval scope.
9. Agent "PC and browser" is partial: a Space sandbox workspace exists for `code_interpreter`, browser runs are not Space-scoped, and there is no streamed desktop or live view in the room.
10. There is no Space memory, no per-Space cost or allowance, and no agent-to-agent delegation.
11. Watches exist in capability-core but are not exposed. Delivery (S4.4) is not built.
12. The org room roster re-sync is not scheduled, so new org members may not appear until a manual or registration-triggered sync (INF from no `crons.ts` and no caller of `syncAllOrganizationRooms`).
13. Room roles are fixed to `editor` for added people, and there is no role editor.
14. Shared rooms cannot be deleted; only personal Spaces have deletion.
15. In practice, every shared feature needs the operator to seed `space_effect_policies` (compliance values) per org. Without that, shared read, create, retrieval and agent actions are all denied. Per the docs, dev has no such row, so most of this is unverified live.
16. Calls, huddles, meetings, screen share and whiteboard are absent. Only RustDesk remote support config and AI realtime voice exist.
17. Session Core does not act on the Space decision's ZDR claim (decoded only). ZDR relies on the gateway's turn flag.

## Architecture constraints that limit what Verevon can adopt

- **No cross-plane DB access** (`CLAUDE.md` Architecture Rules). Any Teams or Slack feature that needs a join across planes (for example message search across Model threads plus Control rosters, or a sidebar unread aggregate) must become an owner-plane API or projection. It cannot be a shared table.
- **Ownership split for a "message":** content and threads are Model Plane (Session Core), membership is Control, room identity, presence and read state are Application (Convex), and knowledge is Data. A human-only chat message has no natural owner today. Options are a new Application conversation store (as conversation-core does for support) or a Session Core non-AI post. Either needs an explicit ADR.
- **Control is the only authority.** The browser may *select* a Space but never assert actor, org, audience or decision (`GW/spaces.rs:34-47`; ADR-0001). Reference-app patterns where clients write membership or ACLs directly cannot be copied.
- **Space membership never widens a resource ACL.** Effective access is the Space decision ∩ the recipient-audience authorization ∩ the owner-plane resource authorization (ADR-0001). Shared files, connectors, credentials or agent tools in a room need their own owner-plane grants.
- **The registry answers presence, not authority** (ADR-0002). Any org-wide agent surface must re-check the per-Space Control roster before invoking.
- **"Decisions that authorize future agent behavior come from outside the agent"** (`docs/space-defenition.md` Security model). Agents cannot bind agents, change roles, approve or act as another principal. This forbids Grok-style agents that self-escalate or share credentials.
- **ZDR must propagate** across every content-persisting boundary. Room history, notification payloads, previews and search indexes must all honour it, which limits rich notifications, link previews and server-side search under ZDR orgs.
- **Model proposes browser actions; Quarry executes or rejects them.** An agent "browser" in the room must route through Quarry grants rather than an agent-owned browser.
- **Deny-by-default per-org effect policies** with compliance fields (`privacy_policy_ref`, `lawful_basis`, `retention_class`, `residency`). Features cannot be "on by default" without operator policy.
- **Channel Plane is docs-only.** Slack, Teams or embed delivery cannot be built against it as if it existed (`CLAUDE.md`).
- **Short-lived decisions** (2-minute TTL) mean long-lived subscriptions, watches and schedules must reauthorize per fire or observe (already the pattern in cron and watch). A push transport would need re-authorization at subscribe time and on revocation.
- **Release gates R-1 to R-5** bar new *effectful* Space surfaces until candidate evidence exists (QM plan; audit §2).

## Doc-vs-code discrepancies

1. `docs/space-defenition.md` "Message routing behavior" says "If no agent is called, the normal human-only flow continues". In code, every room post calls `streamChat`, so the model answers every message (`FE/.../SpaceRoomComposer.tsx:402-420`).
2. `docs/space-defenition.md` Non-goals lists "no ... presence". Presence and typing are shipped (`CVX/spacePresence.ts`, `GW/spaces.rs:1934`). The audit's own §6.1 gap table (presence, unread, approvals, Stop, member management, rooms: "no") is superseded by its later §7-§18 sections.
3. `docs/S4_3_WATCH_PRIMITIVE_DESIGN_2026-09-14.md` says "Status: proposed", and the gateway reports `watches_not_built` (`GW/spaces.rs:1624-1628`). In fact, the capability-core watch store, handler, sweeper and create path landed on 2026-09-14 (commits `690f91ae`, `6316becb`, `d45e0269`). They are simply not exposed through Model Gateway or the Verevon gateway.
4. The comment at `GW/orchestration.rs:31-35` claims approval get and decide carry no `org_id` (an IDOR). Model Gateway now passes the verified JWT org (`MG/http_routes.rs:1510-1540`), and the proto documents the Phase 6 tenant filter (`orchestration.proto:719-760`).
5. ADR-0002 and ADR-0003 cite stale line numbers: `spaces.rs:2299-2315`, `1424-1435`, `2094-2172` and `schema.ts:527-560`. The functions are now at `GW/spaces.rs:4612` (labels), `4057` (compose), `4399` (mention injection) and `CVX/schema.ts:546` (spaces).
6. `docs/space-defenition.md` binding values `mention_only`/`group_only` vs schema literals `mention`/`group` (`CVX/schema.ts:628`).
7. `docs/space-defenition.md` routing step 3 promises "per-Space memory policy", but no Space memory scope exists (`CAP/internal/api/workplane_apis.go:70`).
8. `apps/CODEBASE_INFORMATION_SYSTEM.md:87,245` describes Application Plane as "Convex workspace, realtime sync". For Spaces, the v3 browser uses no Convex subscription; all room liveness is HTTP polling (`FE/.../SpacePage.tsx:87`).
9. ADR-0001 says privacy fields including ZDR "travel with" decisions and are re-checked before effects. Session Core decodes `zero_data_retention` from the Space decision but explicitly does not act on it yet (`SC/src/grpc.rs:1749-1755`).
10. The QM plan's execution tracker (dated 2026-08-17) shows S3.x, S4.x and S1.5-S1.7 as `[~]` with August evidence. Code has since landed S3.3 (durable workspace), S4.2 (processes), S4.3 steps 1-3, the Work, Knowledge and Activity tabs, and shared read. The tracker is stale relative to code.
11. `docs/SPACE_PAGE_AUDIT_2026-09-06.md` §4 marks items 1-6 "done", but item 1's shared *reply* is still blocked in Session Core (`SC/src/grpc.rs:4406`), while the UI still offers Reply on any thread (`FE/.../SpacePage.tsx:581`).
12. `docs/space-defenition.md` states "Pause / mute / remove here" and the "Binding states" table; code matches (`paused` replaced `muted`). No discrepancy, recorded for completeness.
13. `docs/space-defenition.md` acceptance criteria require delegation inside the roster and logging of "who invoked which agent, in which Space and thread, on whose authority". Delegation is not built. Per-invocation logging exists only as message `agent_name` and `author_subject_id` plus run records, with no dedicated invocation audit (INF; no invocation-audit table found).
