### 3.11 WS11 Notifications, cross-room attention, read state

Source: `.plan-work/design/WS11.md` (live tree re-read 2026-10-18). Paths are relative to the repo root; line numbers are inherited from the design and were not re-checked here (UNVERIFIED unless stated). Tags: C committed, M modified-uncommitted, U untracked, NEW added by this workstream.

**Items resolved.** F-t1-008 (Priority, XL, W2): closed signal set `mention, reply, invite, agent_done, agent_failed, reminder` as refs-only Application items, approvals and "agent waiting" as live-derived needs-you, per-room levels (`all|mentions|off`), thread follow/mute, fixed precedence, coalescing, budgets, refs-only email courier, erasure. F-t1-009 (Priority, L): polled aggregate `GET /api/v1/spaces/attention`, badge semantics, sidebar counts that no longer need an open room, `/attention` inbox with done and snooze. F-t1-042 (Recommended, L): mark unread, first-unread divider, org-scoped status with text and expiry, agent availability chips, refs-only reminders. Not delivered: Web Push, PWA, shell stream (F-t1-035, Model Plane plan WS16), DND schedules (F-t1-059), digests (F-t1-060), reaction and edit signals (WS12). Scope rule: WS11 stores and transports ids, closed words and timestamps only, never text, titles or names.

**Owner planes.** Application (Convex projection and ingest, notification-core courier), Model (Session Core `SpaceAttention` RPC, attention outbox, model-gateway route), Control (one bulk read-decision route, no table), Frontend (BFF aggregate, SPA store, inbox).

| Slice | Goal | Size | Plane | Depends on | Flag | Eng-weeks |
|---|---|---|---|---|---|---|
| WS11-S1 | Control bulk `thread-read-decisions` (1..60 items, ordinary `model.thread.read` tokens) | S | Control | none | additive route | 0.4-0.6 |
| WS11-S2 | `SpaceAttention` RPC, messages index, model-gateway `POST /v1/spaces/attention` | L | Model | prerequisite tree (0044/0045 work) | `SPACE_ATTENTION_SUMMARY` | 2-3 |
| WS11-S3 | Convex read-state extensions, BFF aggregate, read-all, notify, unread, action contracts | L | Application, Frontend | S1, S2 | `space_attention_api` | 2-3 |
| WS11-S4 | SPA store (Web Locks leader poll), sidebar counts, divider, jump, mark unread | M | Frontend | S3 | `space_attention_ui` | 1.5-2 |
| WS11-S5 | Session Core attention outbox (`mp.v1.space.<ref>.event`), NATS stream and durable | M | Model | WS02-S1 | `SPACE_ATTENTION_EMIT` | 1-1.5 |
| WS11-S6 | Convex ingest, rules, items, follows, agent activity, erasure, cron | L | Application | S5, S3; WS08 for mentions | `SPACE_ATTENTION_INGEST` | 2.5-3.5 |
| WS11-S7 | Items API, headline hydration, `/attention` page, follow control | L | Frontend, Application | S2, S6 | `space_attention_ui` | 2-3 |
| WS11-S8 | Invite signal from gateway add-member | S | Frontend (gateway) | S6 | `SPACE_ATTENTION_INGEST` | 0.4-0.6 |
| WS11-S9 | notification-core principal `space-attention`, allowlist, migration, courier | M | Application | S6, D-WS11-8 | `SPACE_ATTENTION_OUT_OF_APP` | 1.5-2 |
| WS11-S10 | Org-scoped status, agent availability chips | M | Application, Frontend | S3, S6 | `space_status_api` | 1.5-2 |
| WS11-S11 | Reminders | M | Application, Frontend | S6, S7 | `space_reminders_api` | 1-1.5 |

**Key files.**
- Control: `apps/Control Plane/user-core/internal/http/spaces.go` (extract `issueOneThreadReadDecision`, :640 region), `server.go` (:359), `service_auth.go` (:114).
- Session Core: `apps/Model Plane/proto/model_plane/v1/space_attention.proto` (NEW), `apps/Model Plane/rust/services/session-core/src/space_attention_grpc.rs` (NEW), `src/audit_publisher.rs` (`OutboxRoute::SpaceEvent`), `src/grpc.rs` (registration and health names only, M hotspot); model-gateway `src/space_attention_routes.rs` (NEW), `http_routes.rs` (:201), `state.rs`; `apps/Model Plane/go/services/nats-provisioner/main.go`, `apps/Model Plane/deploy/nats.conf`.
- Application: `apps/Application Plane/convex-core/convex/schema.ts`, `spaceAttention.ts`, `spaceAttentionRules.ts`, `spaceAttentionIngest.ts` (NEW), `spaceReadMarkers.ts`, `http.ts`, `crons.ts` (shared with WS04), `nats-subscriber.js`; `apps/Application Plane/notification-core/internal/http/handlers.go` (:163-169, :199-218), `internal/config/config.go`.
- Frontend: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/attention.rs` (NEW), `spaces.rs` (hotspot), `src/features/spaces/lib/space-attention.ts` (NEW), `src/features/attention/components/AttentionPage.tsx` (NEW), `CoreSidebarSpacesPanel.tsx`, `sidebar-navigation.ts`, `SpacePage.tsx`, `SpaceRoomTimeline.tsx`, `src/shared/actions/action-registry.ts`.

**API, proto and data changes.**
- Routes (BFF, typed envelope, flag-off 404 `space_attention_disabled`): `GET /api/v1/spaces/attention?refs=` (rooms, `totals.badge`, `gaps`, `mode:"zdr_empty"` for ZDR orgs, 6 per minute per user); `GET /attention/items`; `POST /attention/items/{id}/state`; `POST /attention/read-all`; `GET|PUT /{space_ref}/notify`; `PUT /{space_ref}/threads/{thread_id}/follow`; `POST /{space_ref}/unread`; `GET|PUT /spaces/status`; reminders CRUD. Badge rule computed once in the BFF: `n = needs_you + mentions`, unread reported separately.
- Convex tables: `spaceRoomNotifyPrefs`, `spaceNotifyUserSettings`, `spaceThreadFollows`, `spaceAttentionItems`, `spaceAttentionEvents`, `spaceAttentionBudget`, `spaceMemberStatus`, `spaceAgentActivity`, `spaceReminders`; `spaceReadMarkers` gains `unreadFloor` and index `by_org_and_subject`.
- Migrations (design numbers; see reconciliation table at the end of 3.15): Session Core `0050_messages_space_activity_index` (`idx_messages_space_created`, created CONCURRENTLY by hand first), `0051_space_attention_outbox` (outbox CHECK widened, `space_attention_settings`, trigger on `space_event_log`); notification-core `015_register_space_attention_notifications`; none in Control or Convex.
- Events: `mp.v1.space.<space_ref>.event` on new stream `MODEL_PLANE_SPACE_EVENTS`, durable `convex-space-attention`; envelope carries ids and closed words, `zdr:false` only.

**Flags.** `SPACE_ATTENTION_SUMMARY` (Model, off prod), `space_attention_api`, `space_attention_ui`, `space_status_api`, `space_reminders_api` (BFF, off in production), `SPACE_ATTENTION_EMIT` (Session Core, refuses without WS02 emit flag), `SPACE_ATTENTION_INGEST`, `SPACE_ATTENTION_OUT_OF_APP` (Convex, off), `SPACE_ATTENTION_SUPPRESS_OBSERVED` (on). Flags are never authorization. Rollout: S1/S2 dark, dev API then UI, emit and ingest in dev, staging, S9 last and only after Novu workflows exist.

**Tests (exact).**
- `cd "apps/Control Plane/user-core" && go test ./internal/spaces/... ./internal/http/...` (bulk issuance, digest equals single route).
- `cd "apps/Model Plane/rust" && cargo test -p session-core -p model-gateway -p mp-contracts`; DB tests `cargo test -p session-core -- --ignored` on a scratch DB.
- `cd "apps/Model Plane/go/services/nats-provisioner" && go test ./...`.
- `cd "apps/Application Plane/convex-core" && npm test` (not in CI): rules, markers, ingest, subscriber, status, reminders.
- `cd "apps/Application Plane/notification-core" && go test -v ./... && go build ./cmd/server`.
- `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `pnpm exec vitest run src/features/spaces src/features/attention src/features/core src/shared/api src/shared/actions src/shared/i18n`; `pnpm lint && pnpm typecheck`; `pnpm test:e2e tests/e2e/space-attention.spec.ts`.

**Definition of done and evidence.** Rebuilt images for user-core, session-core, model-gateway, nats-provisioner, convex-backend, convex-subscriber, notification-core, gateway; running stack proven with `bash "apps/Frontend Plane/verevonv3/build-verevon-services.sh" --status`, `psql` (`\d messages`, `\d space_attention_settings`), `nats stream info MODEL_PLANE_SPACE_EVENTS`. Flow: B posts, A's sidebar shows the room badge without opening it, opening clears it; mark unread then re-enter clears the floor; mention produces one `kind=mention` row with no text field, replay keeps the count at 1, `/attention` renders the title fetched at render and hides it under `hide_preview`; run completion gives `agent_done` to the starter; pending approval shows in Needs you with no item. Negatives: muted thread, non-audience mention, ZDR org (`zdr_empty`, typed 412 on follow), forged recipient field, `space.mention` from the gateway principal refused. Capture SQL, `nats` output, Convex rows, HAR, screenshots, test summaries.

**Effort, dependencies, risks.** XL, 16-23 engineer-weeks (about 7-9 weeks with four engineers). Hard: prerequisite Model Plane tree, WS02-S1 for S5, WS08 for mentions (pipeline provable with a DB fixture), WS10 addressee predicate (fallback: run owner). Risks: Control cost of 60 decisions per poll (cap, one Web Locks poller, 10 s single-flight, Control-side bulk query as fallback); double counting (D2 rule); notification storms (coalesce, budget 30 per hour); lost events degrade to the derived unread badge, never a wrong authority decision; notification-core delivery never exercised live (S9 flagged, in-app independent).

**Build-state delta (Model Plane and Quarry branches).**
- notification-core `015` is claimed by Model Plane WS16.S4 (`register_attention_notifications`), with 016 (web push) and 017 (scheduled run outcome); WS11-S9 must take the next free number (018 or later) and the `space.*` namespace stays distinct from `modelplane.*`.
- Model Plane WS16 plans its own `RUN_ATTENTION` outbox event, `AttentionState` enum, per-user shell stream, Web Push and PWA, and states that the Space aggregate joins as a second BFF upstream `source:"space"`. WS11's `needs_you` and badge must reuse that enum and badge counter when it lands; WS11 keeps Phase 1 independent of it. UNVERIFIED which event class WS11-S5 finally uses (trigger on `space_event_log` here, sweeper there).
- Latent notification-core bug, verified in `apps/Application Plane/notification-core/internal/preferences/preferences.go:144-155`: one explicit opt-out on any channel for an event type suppresses all channels. Model Plane WS16.S4 plans the fix; WS11-S9 depends on it for per-channel `space.*` preferences.
- Session Core numbers 0050/0051 sit inside Model Plane's planned 0046-0066 range; reassign.

### 3.12 WS12 Direct messages, message lifecycle, reactions

Source: `.plan-work/design/WS12.md`. Assumes ADR 1 option A (Session Core owns the transcript), WS01 delivered (member append path, `post_kind`, `SpaceTurnList`), WS04-S4/S5 delivered (membership writer, audience registration).

**Items resolved.** F-t1-016 (Priority, L, W2): Space kinds `dm` (2 people) and `group_dm` (3 to 9) with a roster fixed at registration, `participant_hash` open-or-return, per-viewer hide that resurfaces on activity, DM sidebar group, entitlement `dm_create_entitled`, Control `agent_scope` fact, hide-not-delete semantics. F-t1-017 (Priority, L): append-only `message_revisions` folded at every read; edit own message, delete own message, agent-owner retract, moderator delete; Control decision `model.thread.revise`; per-org original-retention rule; model-visible edited marker and tombstone; derived-memory retirement; per-message toolbar; content-free reactions. Not delivered: mutable DM rosters (D-WS12-1 option B), agents in DMs (WS17), DM notifications (WS11), quote and fork (WS13), edit window, custom emoji.

**Owner planes.** Control (DM kinds, `participant_hash`, revise decision, entitlements, retention policy), Application (DM Space open-or-return, hides, reactions), Model (Session Core revisions, fold, retention execution, model-gateway routes), Frontend (BFF routes, toolbar, DM UI).

| Slice | Goal | Size | Plane | Depends on | Flag | Eng-weeks |
|---|---|---|---|---|---|---|
| WS12-S1 | Control DM kinds: migration, `participant_hash`, registration validation, fixed roster, `dm-eligibility`, `agent_scope`, vectors | M | Control | none | `SPACE_DM_REGISTRATION` + `dm_create_entitled` | 1.2-1.8 |
| WS12-S2 | Convex kind union, `openDmForGateway`, hide table, legacy-path guards | M | Application | S1 contract | `SPACE_DM_OPEN` | 1-1.5 |
| WS12-S3 | Gateway DM routes, index fields, sidebar group, start dialog, DM traits, hide | L | Frontend | S1, S2, WS01-S5/S6/S7b, WS04-S5, WS04-S3 or S9 | `SPACE_DM_API` | 2-3 |
| WS12-S4 | Session Core revisions: migration, proto, fold in reads, `ReviseMessage`, `ResolveMessageTarget`, retention, memory retirement, audit | L | Model | WS01-S1 | `SPACE_MESSAGE_REVISE_CORE` (fold always on) | 2.5-3.5 |
| WS12-S5 | Control revise decision, entitlements, `space_message_policies`, message-policy routes, Rust verifier golden | M | Control | S4 proto | none (deny by default) | 1-1.5 |
| WS12-S6 | model-gateway PATCH, DELETE, target routes, error map | M | Model | S4, S5 | `SPACE_MESSAGE_REVISE_API` | 0.8-1.2 |
| WS12-S7 | Gateway lifecycle routes, toolbar, edit, delete, tombstone, id-preserving normalizers | L | Frontend | S5, S6, WS01-S7a/S7b | `SPACE_MESSAGE_REVISE_API` | 2-3 |
| WS12-S8 | Reactions: Convex table, gateway routes, transcript merge, chips | M | Application, Frontend | S4, S7, WS01-S5 | `SPACE_REACTIONS` / `SPACE_REACTIONS_API` | 1.5-2 |

**Key files.**
- Control: `apps/Control Plane/user-core/internal/spaces/authority.go` (kinds :14-20, :109, :356; `SpaceIndexEntry` :224), `repository.go` (`Register` :57-130, `ReplaceMemberships` :669, `SpacesForSubject` :574), `dm_participants.go`, `thread_revise_decision.go`, `entitlement_catalog.go` (WS03), `internal/http/spaces.go`, `server.go`, `service_auth.go`; shared vectors `internal/spaces/testdata/participant-set-vectors.v1.json` (Go and Convex tests).
- Application: `apps/Application Plane/convex-core/convex/schema.ts` (:549-554), `spaces.ts`, `spaceRegistration.ts`, `spaceMembers.ts` (:22), `spaceAgents.ts` (:412, :529), `spaceParticipants.ts`, `spaceDmHides.ts`, `spaceReactions.ts` (NEW).
- Model: `apps/Model Plane/rust/services/session-core/src/message_revision.rs` (NEW), `grpc.rs` (`list_conversation` fold :5455, `load_thread_messages` :4048, verifier :2014), `dreaming.rs` (:1263-1342), `proto/model_plane/v1/sessions.proto`, `crates/mp-contracts/src/message_revision.rs` (NEW), model-gateway `http_routes.rs`, `sse.rs` (:5350-5433), capability-core `internal/sessionreview/transcript.go`.
- Frontend: gateway `domains/spaces/dm.rs`, `message_lifecycle.rs` (NEW), `spaces.rs`; `src/features/spaces/components/SpaceMessageToolbar.tsx`, `DmStartDialog.tsx`, `SpaceReactionBar.tsx` (NEW), `SpaceRoomTimeline.tsx`, `CoreSidebarSpacesPanel.tsx`, `spaces-client.ts`.

**API, proto and data changes.**
- Control migration `037_space_dm_kinds` (kind CHECK widened, `participant_hash`, unique index `uq_registered_spaces_dm_participants`, `dm_create_entitled`) and `038_space_message_lifecycle` (`message_revise_entitled`, `message_moderate_entitled`, `space_message_policies`, `space_message_policy_events`). Internal routes: `POST .../dm-eligibility`, `POST .../thread-revise-decision` (schema `sha256:thread-revise-v1`, permissions `thread:revise`, `thread:moderate` for manager/owner outside DMs, exactly one of `revise:purge_original` or `revise:retain_original`), `GET .../{space_ref}/message-policy`, `PUT .../message-policy`.
- Session Core migration `0048_message_revisions` (table keyed by `message_id`, `ON DELETE CASCADE`, action `edited|deleted`, actor class `author|agent_owner|moderator`, idempotency hash); the only `UPDATE messages` in the service is the purge write in `message_revision.rs` (source-scan test pins it). Proto additive: `SessionMessage` fields 8-12 (`revision_state, revised_at, revised_by_subject_id, revision_no, agent_followed_up`), RPCs `ReviseMessage`, `ResolveMessageTarget`.
- BFF: `POST /api/v1/spaces/dm` (202 new, 200 existing, typed 412 under ZDR), `POST|DELETE /api/v1/spaces/{ref}/hide`, `PATCH|DELETE .../threads/{thread_id}/messages/{message_id}`, `PUT|DELETE .../messages/{message_id}/reactions/{emoji}`; transcript gains `message_id`, `revision_state`, `reactions[]`. Convex: `spaceDmHides`, `spaceReactions`; five new action contracts, none model-executable.
- Model-visible rendering: two static strings (deleted line, edited marker) in `mp-contracts`, used by `load_thread_messages` and model-gateway history.

**Flags.** `SPACE_DM_REGISTRATION` (Control), `SPACE_DM_OPEN` (Convex), `SPACE_DM_API`, `SPACE_MESSAGE_REVISE_API`, `SPACE_REACTIONS_API` (BFF, off in production), `SPACE_MESSAGE_REVISE_CORE` (Session Core; reads fold always), Control policy bits default false. ZDR orgs are refused everywhere by design (typed 412), never silently.

**Tests (exact).**
- `cd "apps/Control Plane/user-core" && go test ./internal/spaces/... ./internal/http/... ./internal/database/...` (vectors, registration, fixed roster scan, `TestThreadReviseIssuedPermissionsMatrix`; live variant with `TEST_DATABASE_URL`).
- `cd "apps/Model Plane/rust" && cargo test -p session-core -p model-gateway -p mp-contracts`; `cargo test -p session-core -- --ignored` (fold, purge, retain, ceiling, dedupe, run-active, memory retirement); `buf lint` and `buf breaking` against `baseline.binpb`; `cd "apps/Model Plane/go/services/capability-core" && go test ./internal/sessionreview/...`.
- `cd "apps/Application Plane/convex-core" && pnpm test` (`space-dm.test.cjs`, `space-reactions.test.cjs`).
- Gateway `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `node --test apps/gateway/scripts/action-surface-contract.test.mjs`; SPA `pnpm exec vitest run src/features/spaces src/features/core src/shared/actions src/shared/api src/shared/i18n`, `pnpm typecheck && pnpm lint`; `pnpm test:e2e tests/e2e/space-dm-and-messages.spec.ts`.

**Definition of done and evidence.** Rebuilt user-core, session-core, model-gateway, convex-gateway (restart after env), gateway; confirm migrations in `schema_migrations` and `\d message_revisions`. Flow: A opens a DM with B (202 then `active`, one `registered_spaces` row with `participant_hash`, two memberships), repeat open returns the same `space_ref`, hide resurfaces on the peer's post, group of 3 has no add-member, agent bind returns 409, org without `dm_create_entitled` gets 403 with `missing[]`, ZDR org gets typed 412 with no Control call. Lifecycle: edit under `retain` keeps `messages.content`, under `purge` blanks it; delete retires `agent_memory` rows with that `message:` link; `MESSAGE_REVISED` event has ids only; audit event `message_revised` reachable; moderator delete stores `actor_class=moderator`; foreign edit 403; replay returns `deduplicated:true`; edit during an active run returns `thread_run_active`; reactions toggle, viewer refused, delete clears them.

**Effort, dependencies, risks.** 12-18 engineer-weeks (6-8 weeks, three engineers). Hard: WS01 base, WS04-S5 audience registration (DMs cannot work without it, no code calls `replaceRecipientAudience` today), WS03 catalogue preferred. Risks: re-litigating the rejected versions design (per-message, no nesting, no supersede, reviewer cites `apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md:87-93`); first `UPDATE` on `messages` (single module, scan test, `retain` mode strict append-only); hidden `ListConversation` consumers (fold in shared SQL); orphan Space after Control rejects registration (pre-check, `failed_registration` surfaced); creator GDPR erasure deletes the peer's messages (inherits WS01 open question).

**Build-state delta.**
- Session Core `0048` is now BUILT by Model Plane as `0048_tool_action_effect_ledger` (branch `model-plane`; built 0046 approval_consume_and_mode, 0047 run_loop_checkpoints, 0048). WS12 `0048_message_revisions` must be renumbered; Model Plane plans through 0066.
- Model Plane WS17.S8 plans `message_versions` (0061) with `messages.on_active_path`, a flag-independent reader filter on `list_conversation`, `load_thread_messages`, `list_threads` and Dreaming, and a contract test scanning every `messages` reader. It refuses Space threads. WS12's fold touches the same readers: both must pass that scan, and `SessionMessage` tags 7-10 are claimed there while WS12 claims 8-12 (integrator must reconcile). Conflict C11 resolution stays: Space uses `message_revisions`, chat uses `message_versions`, one reader joins both.
- user-core `037`/`038` are Space-internal reservations; Model Plane also takes 030-032 (WS07.S5, WS24.S7, WS21.S8).

### 3.13 WS13 Message search, permalinks, command layer

Source: `.plan-work/design/WS13.md`. Assumes ADR 9 option A (index in Session Core, ZDR excluded), ADR 1 option A, WS01-S6/S7a for `messageId` and the thread pane.

**Items resolved.** F-t1-018 (Priority, L, W2): keyword search over Space messages (expression GIN index, no text copy), new `SpaceHistoryService` (`SearchMessages`, `ResolveMessage`), per-query Control read decision, audience predicate inside the same SQL, Ctrl/Cmd+F in-room search with scope chips, global scope (S9). F-t1-039 (Recommended, L): entity reference grammar (`message`, `thread`, reserved `run`), `?thread_id=&m=` scroll and highlight, `/spaces/:spaceId/m/:messageId` resolver, room pins (S8), visibility-preserving fork (S10), quote with server-resolved tombstone (S11). F-t1-041 (Recommended, M): command registry, one hotkey layer, Cmd/Ctrl+K palette and switcher, shortcut sheet, platform slash commands. Not delivered: agent recall (`search_space_history`, F-t1-038, WS18, conflict C10), Norwegian stemming, operators, attachment search, forward, "To:" picker. Scope rule: the index proposes candidates; Control's decision and Session Core's audience predicate decide in the same statement.

**Owner planes.** Model (Session Core index and RPCs, model-gateway POST routes), Frontend (BFF routes, SPA search, router, palette), Application (room pins only), Control (decision reuse; batch route in S9 only).

| Slice | Goal | Size | Plane | Depends on | Flag | Eng-weeks |
|---|---|---|---|---|---|---|
| WS13-S1 | `SpaceHistoryService`: migration, proto, `space_history_grpc.rs`, SQL, cursor, semaphore, plan-shape test | L | Model | base tree; number assigned | `space_message_search_core` | 2-3 |
| WS13-S2 | model-gateway POST routes, client, error map | S/M | Model | S1 | `space_message_search_api` | 0.6-1 |
| WS13-S3 | BFF search, entries, features routes; flags in `space_features` | M | Frontend | S2; WS01-S5 | `space_message_search_api` | 1-1.5 |
| WS13-S4a | SPA entity router, scroll and highlight, `messageId`, anchors | S | Frontend | WS01-S7a | `space_entry_links_ui` | 0.5-0.8 |
| WS13-S4b | `/m/:messageId` route and resolver redirect | S | Frontend | S3, S4a | `space_entry_links_ui` | 0.5-0.8 |
| WS13-S5 | In-room search UI, chips, listbox, states, e2e | M | Frontend | S3, S4a | `space_message_search_ui` | 1.5-2 |
| WS13-S6 | Command registry, hotkey layer, palette replacing `GlobalSearchDialog`, `Alt+1..9` | M | Frontend | none | `space_command_palette_ui` | 2-3 |
| WS13-S7 | Platform slash commands in the composer | S | Frontend | S6 | `space_command_palette_ui` | 0.5-0.8 |
| WS13-S8 | Room pins (Convex `spacePins`, routes, timeline lift) | M | Application, Frontend | Control role read; WS04 roles | `space_room_pins_api` | 1.5-2 |
| WS13-S9 | Global scope: Control batch decision, multi-space request | M | Control, Model, Frontend | S1-S3, S6, D-5 | `space_message_search_global` | 1-1.5 |
| WS13-S10 | Fork with visible-prefix copy and lineage | L | Model, Frontend | sibling b `ForkThread` contract, WS01-S3/S4/S5, S1 | `space_thread_fork_core/api` | 3-4 |
| WS13-S11 | Quote with tombstone | M | Model, Frontend | WS01-S3/S4, WS12 | `space_quote_api` | 1.5-2 |

**Key files.**
- Model: `apps/Model Plane/proto/model_plane/v1/space_history.proto` (NEW), `rust/services/session-core/src/space_history_grpc.rs` (NEW), `grpc.rs` (`HEALTH_SERVICE_NAMES` :47-56, `serve()` :7324-7361 only), `main.rs`, `rust/crates/mp-contracts/build.rs` (:48-65), `rust/services/model-gateway/src/space_history_routes.rs` (NEW), `http_routes.rs` (:135-181), `state.rs`.
- Frontend gateway: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces/message_search.rs`, `pins.rs` (NEW), `spaces.rs` (`mod` at :30, routes :649-736), `config.rs`.
- SPA: `src/features/spaces/lib/space-entity-link.ts`, `components/SpaceRoomSearch.tsx`, `SpaceEntryRedirect.tsx`, `lib/space-slash-commands.ts`, `src/features/command/*` (all NEW), `CoreNavbar.tsx` (:122-132, :383), `SpaceRoomComposer.tsx` (:83, :333-367), `SpaceRoomTimeline.tsx`, `SpacePage.tsx`, `App.tsx` (:182).

**API, proto and data changes.**
- Session Core migration `0048_space_message_search_index` (first line `-- no-transaction`; `CREATE INDEX CONCURRENTLY idx_messages_space_fts ... USING GIN (to_tsvector('simple', left(content, 100000))) WHERE space_id IS NOT NULL AND role IN ('user','assistant')`; revert file under `migrations/revert/`). No text copy, no purge hook (thread delete and erasure remove rows). Whether the pinned sqlx honours `-- no-transaction` is UNVERIFIED; prove in S1 on a scratch DB.
- gRPC `SearchMessages` (steps: identity, `session:read`, flag, validate 2..200 chars, `verify_thread_read_space_decision`, revocation row, 8-permit semaphore, 2.5 s statement timeout, one SQL with audience ceiling on message and thread, owner path when no decision, `zdr_waived` run exclusion) and `ResolveMessage` (identical NotFound for hidden, missing, foreign).
- Routes: model-gateway `POST /v1/spaces/:space_id/messages/search|resolve` (decision in body, never URL); BFF `POST /api/v1/spaces/{space_ref}/search` (`Cache-Control: no-store`, fresh decision per query, `None` becomes `coverage:"own_threads_only"`, hit `space_id` asserted, else 502), `GET .../entries/{message_id}`, `GET /api/v1/spaces/features`. Cursors are opaque on `(created_at, id)`, never `messages.sequence`. Control: no new decision type for S1-S8; S9 adds `POST /api/v1/internal/spaces/thread-read-decisions` (<=25 spaces).
- S8 Convex `spacePins {spaceRef, externalOrgId, threadId, pinnedByExternalAuthId, pinnedAt}` ids only. S10/S11 migrations `0049` and `0049/0050` (design numbers; internally contested with WS15 and WS11).

**Flags.** `SPACE_MESSAGE_SEARCH_CORE`, `SPACE_MESSAGE_SEARCH_API` (env via `switch_value_is_on`, off in production), BFF `SPACE_ENTRY_LINKS_UI`, `SPACE_COMMAND_PALETTE_UI`, `SPACE_ROOM_PINS_API`, and the S9-S11 flags (off). ZDR callers may search (read path) but nothing is persisted, cached, logged or kept as client history unless `isLocalRetentionAllowed()`.

**Tests (exact).**
- `cd "apps/Model Plane/rust" && cargo test -p session-core`; DB tests `cargo test -p session-core --bin session-core space_history -- --ignored --test-threads=1` (audience ceiling, owner-only, revoked member, archived, waived exclusion, erasure, relevance cap, plan shape naming `idx_messages_space_fts` on 50,000 rows); `cargo test -p model-gateway` (keep `tests/zdr_refusal_inventory.rs` green).
- Gateway `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`.
- SPA `pnpm exec vitest run src/features/spaces src/features/command src/features/core src/shared/api src/shared/actions src/features/chat/lib/chat-route-ownership.test.ts`, `pnpm typecheck && pnpm lint`; `pnpm test:e2e tests/e2e/space-search.spec.ts`; Convex `cd "apps/Application Plane/convex-core" && pnpm test` (`space-pins.test.cjs`); S9 `go test ./internal/spaces/... ./internal/http/...` in user-core.

**Definition of done and evidence.** Rebuilt session-core, model-gateway, gateway with `SPACE_MESSAGE_SEARCH_CORE=1 SPACE_MESSAGE_SEARCH_API=1`; `\d+ messages` shows the index, `pg_index.indisvalid` true. Flow: A posts "kvartalsbudsjett", B types "kvartals" (prefix hit), opens the hit, URL becomes `?thread_id=...&m=...#chat`, pane scrolls and highlights; `/spaces/<ref>/m/<id>` redirects; `EXPLAIN` uses the index; palette, `Alt+2`, `?` sheet, `/read` posts nothing. Negatives: removed member fails closed, org without `thread_read_entitled` sees the `own_threads_only` banner, `zdr_waived` thread excluded with `excluded` reason, ZDR credential search leaves no rows and no query text in gateway or model-gateway logs, a 100,000+ character message appends cleanly. Target p95 under 300 ms at `limit=20` on 50,000 rows, else D-10.

**Effort, dependencies, risks.** Per-slice sums: S1-S7 8.6-12.9, S1-S11 15.6-22.4 engineer-weeks (the design states 9.7-14.4 and 17-24; the per-slice figures above are used here). Hard: WS01-S6/S7a for S4a; WS12 for edit and delete behaviour in the index; sibling b for fork. Risks: index treated as access boundary (SQL predicates, hit-space assertion, open re-authorizes); expression drift (plan-shape test); GIN write cost; hotkey collisions (single layer, IME guard, duplicate-binding test); hotspots `CoreNavbar.tsx`, `SpaceRoomComposer.tsx`, `grpc.rs` (swaps and one-line registrations only).

**Build-state delta.**
- Session Core `0048` is built by Model Plane (see 3.12): renumber.
- Model Plane WS17.S3 plans `message_search_index` (0055, also `-- no-transaction`): `idx_messages_content_fts` on `to_tsvector('simple', left(content, 65536)) WHERE role IN ('user','assistant')` (not partial on `space_id`), plus a `SearchMessages` RPC in `SessionCore` with a `space_id` arm, audience ceiling and a `RECALL` purpose. This duplicates WS13-S1. Two indexes on the same expression would double write cost: one index and one tsquery helper must win (the design's disjoint-partial option D-1 assumed b would index only `space_id IS NULL`). Not built yet in either branch; decide before S1. WS13 keeps its separate service for Space grants and the `zdr_waived` exclusion.
- Model Plane WS17.S6 (`ForkThread`, 0059, refuses Space threads) owns lineage columns; WS13-S10 reuses them and adds only the Space variant (D-8). Model Plane F-t3-077 is the same command surface: WS13 owns `src/features/command/`.

### 3.14 WS14 Attachments, file library, artifact sharing grants

Source: `.plan-work/design/WS14.md`. Legend used below: FV = `apps/Frontend Plane/verevonv3`, GW = `FV/apps/gateway/src`, UC = `apps/Control Plane/user-core`, DA = `apps/Data Plane v2/services/documents-api-go`, RE = `apps/Data Plane v2/services/retrieval-engine-rs`, IC = `apps/Ingestion Plane/imports-core`.

**Items resolved.** F-t1-013 (Priority, L, W2): phase 1 per-turn attachments in the room composer (paste, drop, attach; extracted text inline; images only on non-ZDR turns; timeline chips); phase 2 durable room files for text-extractable types (pdf, docx, xlsx, pptx, txt, md, csv, json) via imports-core, a Control shared-Space import decision, a Space-owned grant, a Data listing with provenance, uploader delete, Files tab, typed refusals. F-t1-057 (Recommended, L, was M): un-fence the grant write surface with a reviewed owner-asserted delegation mode, re-point ShareDialog and the model-callable `ownership.*` actions at the owning service, `edit` plumbed but refused for documents, `space` grant subject written only by the file-attach path and evaluated as Space decision AND grant. Not delivered: original-bytes store, scanning, thumbnails (epic E1), access requests and general access (epic E2), agent use of room files (F-t1-046), manager removal of others' files (D-6), `edit` UI (waits for WS19).

**Owner planes.** Control (shared import decision, grants, `space` subject, owner-service registry), Data (documents-api owner and quota, retrieval-engine listing and grant union), Ingestion (imports-core `space-upload`), Frontend (gateway `space_files.rs`, composer, Files tab).

| Slice | Goal | Size | Plane | Depends on | Flag | Eng-weeks |
|---|---|---|---|---|---|---|
| WS14-S1 | Control shared import decision, unified `import-decision` route, `code` in refusals | M | Control | none | `CONTROL_SPACE_SHARED_IMPORT` | 1-1.5 |
| WS14-S2 | Control grants: owner-asserted mode, owner-service registry, space subject, `file-grant`, `space-visible`, credential scopes, taxonomy lint in CI | L | Control | none | `CONTROL_OWNER_ASSERTED_GRANTS`, `CONTROL_SPACE_SUBJECT_GRANTS` | 3-4 |
| WS14-S3 | documents-api space upload acceptance (owner, visibility, quota, dedupe, grant call, delete and revoke) | L | Data | S1, S2 | `DOCUMENTS_SPACE_FILES` | 2-3 |
| WS14-S4 | Sharing un-fence: documents-api share routes, gateway `shares.rs` and `ownership.*` re-pointed | M | Data, Frontend | S2 | `DOCUMENTS_SHARE_WRITES` | 1.5-2 |
| WS14-S5 | retrieval-engine listing provenance, `visible_space_documents`, grants union at two sites, `space-document` | M | Data | S2 | `RETRIEVAL_SPACE_GRANTS` | 1.5-2 |
| WS14-S6 | imports-core `space-upload` route, typed refusals, limits, scan seam, job idempotency, user-scoped job reads | M | Ingestion | S1 | `IMPORTS_SPACE_UPLOAD` | 1.5-2 |
| WS14-S7 | Gateway `space_files.rs` R1-R5, body cap, ZDR split, action contracts, context `features/limits` | M | Frontend | S1, S5, S6 | `space_room_files_api` | 1.5-2 |
| WS14-S8 | SPA phase 1 per-turn attachments, chips, i18n, typed ZDR copy | M | Frontend | none | `space_turn_attachments_ui` | 1.5-2 |
| WS14-S9 | SPA Files tab, durable attach toggle, preview, delete | L | Frontend | S7 | `space_room_files_ui` | 2-3 |

**Key files.**
- Control: `UC/internal/spaces/personal_thread_decision.go` (`ValidateForSharedImport`), `shared_import_decision.go` (NEW), `repository.go` (`ResolveSharedImportDecisionEvidence`, deletion hook), `UC/internal/http/authz_facade.go` (:34-72, :160), `spaces.go`, `server.go`, `service_auth.go` (:114), `UC/internal/users/acl_repository.go`, `UC/internal/authztaxonomy/owner_service.go` (NEW), `.github/workflows/user-service.yml`.
- Data: `DA/internal/handler/documents.go` (:106, :160, :257-283), `space_import.go`, `space_upload.go`, `shares.go`, `DA/internal/spacegrant/client.go` (NEW), `DA/internal/userauthz/client.go`; `RE/src/authz/visibility.rs`, `RE/src/api/mod.rs`, `RE/src/pipeline/orchestrator.rs` (:1147; M files).
- Ingestion: `IC/app/main.py` (:216-266), `space_upload.py`, `scan.py` (NEW), `service.py`, `config.py`.
- Frontend: `GW/domains/space_files.rs` (NEW), `GW/domains/shares.rs`, `actions/dispatchers.rs` (:3358, :3393), `FV/src/features/spaces/lib/room-attachments.ts`, `SpaceFilesPanel.tsx`, `SpaceRoomComposer.tsx` (M), `SpaceCockpit.tsx`, `FV/src/shared/actions/action-registry.ts` (:2343, :2354).

**API, proto and data changes.**
- Control migration `039_resource_grants_space_subject` (CHECK admits `'space'`, `space` rows `view` only). New internal routes: `POST /api/v1/internal/spaces/import-decision` (personal versus shared dispatch; additive `code` in 403), `POST .../spaces/file-grant`, `GET .../authz/space-visible`; owner-asserted mode for `POST|DELETE /authz/grant` and `GET /authz/grants`; `EditEnforced(document)=false` so `edit` returns `role_not_supported`. The `document -> documents-api` registry is a Go map, so the Rust taxonomy mirror and its lint stay unchanged.
- Data migration `apps/Data Plane v2/infra/postgres/migrations/20261020120000_documents_space_file_dedupe.sql` (unique `(org_id, space_ref, metadata->>'sha256')` for `type='space_upload'`). Ingestion migration `005_space_upload_idempotency.sql`. No Data proto change (the Space import contract is HTTP).
- BFF routes R1-R6: `POST /api/v1/spaces/{ref}/files` (multipart, `Idempotency-Key`, 202), `GET .../files/uploads/{id}`, `GET .../files`, `GET .../files/{id}/text`, `DELETE .../files/{id}`, context `features.room_files` and `limits.room_files`. Typed refusal codes `room_file_*`, `room_files_quota_exceeded`, `space_files_not_authorized`, `zdr_durable_mutation_forbidden` (feature `room_files`).
- Room file = a Data `documents` row (`type='space_upload'`, `owner_id` = uploader, `visibility='shared'`, text only, `scan_state:'not_scanned_text_only'`); audience = owner plus the `space` grant honoured only inside a Space read with a verified decision.

**Flags.** Per-service env flags listed in the slice table, default off in production; compose `${VAR:-false}`, dev override on. Rollout order is fixed: re-point the `ownership.*` actions (S4) before flipping `CONTROL_OWNER_ASSERTED_GRANTS`, never the reverse, because the model-callable twin would otherwise allow sharing any document id. ZDR orgs are refused at gateway, Control, imports-core and documents-api.

**Tests (exact).**
- `cd "apps/Control Plane/user-core" && go test ./internal/spaces/... ./internal/http/... ./internal/users/... ./internal/authztaxonomy/... ./internal/database/...` (live variants with `TEST_DATABASE_URL`); `bash scripts/lint-resource-taxonomy.sh`.
- `cd "apps/Data Plane v2/services/documents-api-go" && go test ./...`; `cd "apps/Data Plane v2" && make vet-go build-go test-rs test-integration`.
- `cd "apps/Ingestion Plane" && make test-imports`.
- Gateway `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (includes a 10 MiB upload passing the 2 MiB default); SPA `pnpm lint && pnpm typecheck && pnpm test && pnpm build`; `pnpm exec playwright test tests/e2e/space-files.spec.ts`.

**Definition of done and evidence.** Rebuilt Control (`scripts/run-control-plane.sh up -d --build user-core`), Data, Ingestion, gateway; `psql` shows the new CHECK and both unique indexes. Step 0 premise check: connector-import under a personal Space and record `owner_id` and `visibility` (the Space import is owned privately by the imports-core principal, UNVERIFIED live). Flow: A uploads `brief.docx` and `data.csv`, R2 reaches `completed`, rows show `owner_id`=A, `visibility='shared'`, `resource_grants.subject_type='space'`; B lists and previews, a removed member gets nothing; same key replays `deduplicated:true`; duplicate file is `room_file_duplicate`; 10 MiB passes and 30 MB is `room_file_too_large`; scanned PDF is `room_file_no_extractable_text`; A deletes and B loses it and the grant; ZDR org gets typed 412 and no Control call. F-t1-057: owner shares with C, C cannot share A's document (404), `ownership.share_document` as a model actor hits the approval gate, `role:"edit"` is refused, the gateway principal gets 403 on `/authz/grant`.

**Effort, dependencies, risks.** Per-slice sums 15.5-21.5 engineer-weeks (design: 16-22; about 7-9 weeks with a Control/Data pair, an Ingestion/gateway engineer, an SPA engineer). Soft: WS01 `space_features`, WS03 `Denial`, WS06 typed ZDR vocabulary, WS07 viewer registry, WS15 typed entries. Risks: un-fencing before re-pointing (rollout order plus test that gateway and model actors cannot reach the write); owner change surprising connector imports (D-3, pinned to `space_upload` and personal connector decisions); a second grant writer (user-core gRPC `GrantDocumentAccess`, S2 audit test); the 2 MiB gateway default body limit (static reading); hot uncommitted retrieval-engine files (call-site edits only).

**Build-state delta.** Quarry plan (branch `quarryv2-next`) builds no scan engine or OCR router: `scan_upload` stays an empty seam and `room_file_no_extractable_text` stands until F-t2-037; its plan also lists Space-scoped run grants (F-t2-072) as not designed, so the WS14 owner-service registry is the only taxonomy owner for now. user-core `039` is clear of Model Plane's 030-032 but Space-internal 030-038 reservations overlap those (see reconciliation). Whether Model Plane WS08 S5 (`RunAttachment` staging into `code_interpreter`) shares the imports-core path is UNVERIFIED (touchpoint T1 assumes `context=chat` variant).

### 3.15 WS15 Composer for real work, scalable timeline, typed entries

Source: `.plan-work/design/WS15.md`. FV = `apps/Frontend Plane/verevonv3`, GW = `FV/apps/gateway/src`, SC = `apps/Model Plane/rust/services/session-core/src`, MG = `apps/Model Plane/rust/services/model-gateway/src`.

**Items resolved.** F-t1-032 (Recommended, L): per-user, per-room, per-thread drafts through `useHomeDraft` scope (tab-local, ZDR-gated), clear only once the server has the message, markdown toolbar and preview (no Tiptap), human posts rendered through `ChatMarkdown`, shared paste helpers. F-t1-034 (Recommended, L): client slice (newest turns kept instead of the first 30, anchoring policy, jump-to-latest, `aria-setsize`, `content-visibility`) then server slice (persisted thread activity summary, keyset windows, tail transcript reads, incremental refresh); real virtualisation only if the measured budget fails (S13). F-t1-052 (Recommended, L): typed in-thread `MessageEntry` with one kind (`link_previews`), card registry with neutral fallback, link previews snapshotted at send through Quarry, room system rows from Control's membership ledger (WS04) and an Application notice table. Not delivered: queue and steer (WS09-S2), uploads (WS14), send journal (WS01), pointer embeds, preview images, server-signed system messages.

**Owner planes.** Frontend (store, composer, anchoring, cards), Model (Session Core activity summary, `ListSpaceTimeline`, tail transcript, `MessageEntry` validation), Control (membership-events read route), Application (`spaceRoomNotices`), Ingestion (Quarry preview contract, owned by sibling a).

| Slice | Goal | Size | Plane | Depends on | Flag | Eng-weeks |
|---|---|---|---|---|---|---|
| WS15-S1 | Client timeline: newest turns kept with expander, anchoring policy, pill, single scroll root, a11y, `content-visibility` | S | Frontend | none | truncation fix ungated; rest `features.timeline_anchor` | 0.6-1 |
| WS15-S2 | Per-room, per-thread drafts; clear on acceptance, keep on failure | S | Frontend | none (WS01-S6 journal soft) | `features.composer_drafts` | 0.4-0.8 |
| WS15-S3 | Markdown toolbar, preview, auto-grow, counter; paste-helper extraction | M | Frontend | WS01-S6 soft | `features.composer_markdown` | 1-1.5 |
| WS15-S4 | Human posts as markdown in the timeline | S | Frontend | WS07-S5 | `features.human_markdown` | 0.2-0.4 |
| WS15-S5 | Session Core thread activity summary: migration, trigger, backfill task, recompute function | M | Model | base tree; number assigned | none (inert) | 0.8-1.2 |
| WS15-S6 | `ListSpaceTimeline`, tail transcript, proto, model-gateway route | L | Model | S5; WS01-S1; WS07-S9 skeleton | `SPACE_TIMELINE_CORE`, `SPACE_TIMELINE_API` | 2-3 |
| WS15-S7 | BFF routes R1, R2, R5, legacy fallback | M | Frontend | S6 | `space_timeline_api` | 1-1.5 |
| WS15-S8 | SPA timeline store, paged UI, lazy transcripts, refresh, pinned shelf | L | Frontend | S1, S7 | `features.timeline_windows` | 2-3 |
| WS15-S9 | Typed entry plumbing and SPA registry | M | Model, Frontend | WS01-S1, S6 | none until S10 | 1-1.5 |
| WS15-S10 | Link previews: R3/R4, action contract, composer chips, card | M | Ingestion contract, Frontend | S9, WS01-S5, WS07-S5, Quarry preview mode | `space_link_previews_api` | 1.5-2.5 |
| WS15-S11 | System rows from Control ledger | M | Control, Frontend | S8, S9, WS04-S4 | `space_system_entries_api` | 1-1.5 |
| WS15-S12 | Application notices `spaceRoomNotices` | M | Application, Frontend | S11, WS04-S2 | same | 1-1.5 |
| WS15-S13 | Conditional real windowed list (spike on Solid 2 RC first) | M | Frontend | S8 measurement | `features.timeline_virtual` | 1-2 |

**Key files.**
- SPA: `FV/src/features/spaces/components/SpaceRoomComposer.tsx` (M, :147, :376-402), `SpaceRoomTimeline.tsx` (:50-76, :137-175, :329-336), `SpacePage.tsx` (:87-113, :293-298, :1308), `lib/timeline-anchor.ts`, `lib/space-timeline-store.ts`, `lib/markdown-edit.ts`, `lib/entry-registry.ts`, `components/entries/*` (NEW), `FV/src/features/dashboard/home/home-draft.ts`, `FV/src/shared/session/home-draft-storage.ts`, `FV/src/shared/composer/paste-attachments.ts` (NEW), `space-timeline.css` (NEW; not the dirty `global.css`).
- Model: `SC/space_timeline.rs`, `SC/thread_activity.rs`, `SC/space_entry.rs` (NEW), `SC/grpc.rs` (`list_conversation` tail branch :5387-5501, `append_message_in_tx`), `SC/main.rs`, `apps/Model Plane/proto/model_plane/v1/sessions.proto`, `MG/http_routes.rs`, `MG/state.rs`.
- Gateway: `GW/domains/spaces/timeline.rs`, `link_preview.rs` (NEW), `spaces.rs` (:30, :710-718), `GW/domains/knowledge/quarry.rs`, `GW/cache.rs`.
- Control: `apps/Control Plane/user-core/internal/spaces/membership_feed.go` (NEW), `internal/http/spaces.go`, `server.go`. Application: `apps/Application Plane/convex-core/convex/schema.ts`, `spaceAgents.ts`, `spaceRoomNotices.ts` (NEW).

**API, proto and data changes.**
- Session Core migration `0049_thread_activity_summary` (design number): `threads` gains `last_message_seq`, `last_activity_at`, `turn_count`, `last_role`, `last_author_subject_id`, `recent_author_subject_ids`, `activity_max_audience_revision`; trigger `messages_thread_activity` AFTER INSERT; `idx_threads_space_activity`; backfill is a bounded task, not in the migration (500 threads per batch; window RPC refuses with `summary_backfill_pending` while rows remain and the BFF degrades to the legacy listing). Message entry persists in `messages.metadata.entry` (no migration).
- Proto additive (numbers provisional): `ListConversationRequest` 8-10 (`tail_limit, before_sequence, include_first`), response 2-3, `SessionMessage.sequence = 8` and `entry = 10`, `ThreadSummary` 20-23, `AppendMessageRequest.entry = 21`, `message MessageEntry`/`LinkPreview`, `service SpaceRoomReadService.ListSpaceTimeline` (whichever of WS07-S9 or WS15-S6 lands first creates the skeleton).
- BFF: `GET /api/v1/spaces/{ref}/timeline?limit&before&after&refresh` (fresh Control read decision every page; opaque cursor `v1|sequence`, carries no authority), transcript window params `tail`, `before_sequence`, `include_first`, `POST .../link-previews`, `preview_ids` on WS01 post routes (BFF builds `entry` from its own cache; client cannot supply snapshot text), `spaces.preview_link` action (not model-executable). Control: `GET /api/v1/internal/spaces/{space_ref}/membership-events` (reads WS04's ledger, applies the history floor). Convex: `spaceRoomNotices` written in the same transaction as the agent-binding mutations.

**Flags.** `SPACE_TIMELINE_CORE` (Session Core, off in compose, on in dev), `SPACE_TIMELINE_API` (model-gateway), `space_timeline_api`, `space_system_entries_api`, `space_link_previews_api` (BFF, off in production), `data.features.*` served by `GET .../context`. `link_previews` is forced false under ZDR; drafts write nothing when `isLocalRetentionAllowed()` is false. Rollout: S1/S2 client-only to staging, S5 everywhere with backfill watched until `idx_threads_activity_pending` is empty, then S6/S7, `timeline_windows` per org on a 2,000-post staging room.

**Tests (exact).**
- `cd "apps/Model Plane/rust" && cargo test -p session-core -p model-gateway`; DB tests `cargo test -p session-core -- --ignored` (trigger maintenance, backfill idempotence, window plan uses `idx_threads_space_activity` over 20,000 threads, ceiling and `activity_max_audience_revision`, tail transcript, entry round trip); `tests/zdr_refusal_inventory.rs` stays green.
- `cd "apps/Control Plane/user-core" && go test ./internal/spaces/... ./internal/http/...`; `cd "apps/Application Plane/convex-core" && npm test && npm run typecheck`.
- Gateway `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `node --test apps/gateway/scripts/action-surface-contract.test.mjs`.
- SPA `pnpm exec vitest run src/features/spaces src/features/dashboard/home src/shared/session src/shared/api src/shared/actions src/shared/i18n src/features/chat/components/chat-markdown.test.ts`, `pnpm typecheck && pnpm lint` (diff failing sets against HEAD; baseline fails on this machine); `pnpm test:e2e -- tests/e2e/space-timeline.spec.ts` (2,000 mocked posts, DOM under 450 nodes, p95 scripting under 8 ms per scroll frame) and `tests/e2e/space-composer.spec.ts`.

**Definition of done and evidence.** Rebuilt session-core, model-gateway, user-core, convex-core (S12), gateway; `psql` shows seven new `threads` columns and trigger `messages_thread_activity`. Scale flow: insert 1,500 threads with `generate_series`, backfill drains to zero, first paint shows 50 posts and "Load earlier", at most one timeline request and four transcript requests, keyset paging, no jump when A posts, "N new" pill only when scrolled up, a 200-turn post shows first turn, last 12 and "188 earlier turns"; `?before=garbage` gives 400, removed member 403, flag off 404 with legacy list. Composer flow: drafts survive navigation and reload per scope, failed send keeps text, sign-out clears `verevon.home-draft.v1:*`. Typed entries: `metadata->'entry'` holds the snapshot, private-IP URL gives 422 `preview_blocked` with no Quarry call, ZDR gives typed 412 and no cache key. System rows after WS04-S4 match ledger and `spaceRoomNotices`.

**Effort, dependencies, risks.** 12-18 engineer-weeks without S13 (+1-2), about 6-8 weeks with three engineers. Hard: WS01-S1 field numbers and `post_kind`, WS07-S9 service skeleton (or create it), WS04-S4 ledger for S11. Risks: trigger on `messages` (benchmark appends, concurrency test), backfill load (batching, window refuses until done), bump-on-reply order surprises (keyset never skips, D-WS15-2), summary leaking counts across audience ceilings (fail-closed predicate), preview text spoofing and SSRF (BFF cache indirection, no images, `dns_guard`), shared composer files (flag-gated hook-ups, land S2 before WS01-S6 or rebase).

**Build-state delta.**
- Model Plane WS17.S1 plans `thread_activity_cursor` (session-core 0047 in its plan, not built; branch holds 0046-0048 for other slices): `threads.last_activity_at TIMESTAMPTZ NOT NULL DEFAULT now()` updated inside `append_message_in_tx`, with a keyset on it, and says it is "also the Space last_activity_at". WS15-S5 adds a nullable `last_activity_at` through a trigger. Same column, different type and writer: the first lander creates it, the other adds only its own columns (`last_message_seq` and the author/count fields) and drops its duplicate writer.
- `SessionMessage` tags 8 and 10 and `ListConversationRequest` 8-10 overlap Model Plane's planned `include_inactive = 8` and `SessionMessage` 7-10 (WS17.S8): integrator must assign.
- Quarry branch: no metadata-only `/v1/preview` route exists in its plan (only scrape options and inline return, WS5.D, and scoped result cache `GW/cache_admission.rs`, commit c207b039). WS15-S10 must (a) use the interim `/v1/scrape` metadata read, (b) write the preview snapshot through the admission-scoped cache API rather than `GW/cache.rs store_for_secs`, and (c) treat the `/v1/preview` contract as UNVERIFIED.
- Session Core `0049` is a Model Plane planned slot (`tool_action_effect_ledger` was renumbered to 0048 on the branch): reassign.

#### Cross-workstream reconciliation for 3.11 to 3.15 (assembler: keep)

| Topic | Conflict | Resolution used here |
|---|---|---|
| Session Core migration numbers | Designs: WS12 0048, WS13 0048 and 0049/0050, WS15 0049, WS11 0050 and 0051. Model Plane branch has built 0046, 0047, 0048 and plans through 0066 | All Space numbers are placeholders. Final numbers come after Model Plane's range. Build order proposed: WS15-S5, WS11-S2 index, WS13-S1 (or merged with Model Plane WS17.S3), WS12-S4, WS11-S5, WS13-S10/S11 |
| user-core numbers | WS12 037/038, WS14 039; other Space designs 030-036; Model Plane 030-032 | Reassign through the sequencing owner |
| notification-core | WS11 015 versus Model Plane WS16.S4/S6 015/016/017 | WS11-S9 takes the next free number after 017 |
| `SessionMessage` and `ListConversationRequest` tags | WS01 7, WS12 8-12, WS15 8 and 10, WS13 provisional 20; Model Plane WS17.S8 7-10 | Integrator assigns from one table |
| Thread activity column | WS15-S5 versus Model Plane WS17.S1 | One column, first lander creates it |
| Message index | WS13-S1 versus Model Plane WS17.S3 | One index expression; decide before S1 |
| Message edits | WS12 `message_revisions` versus Model Plane `message_versions` and `on_active_path` reader scan | Both folds coexist in the same readers |
