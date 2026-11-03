# Space depth register, wave W1 (M0 + M1)

Generated 2026-10-27 from the builders' and test agents' depth bullets (`.plan-work/space-build/depth/*.md`). Owner strategy: the wave was built to scope first, findings are logged here instead of being chased; at the wave stop the owner prioritises. Severity: P0 a hole that already exists in committed code or makes a slice unsafe; P1 likely bug or missing authority/ZDR coverage; P2 hardening; P3 polish.

Totals: P0 0, P1 100, P2 368, P3 215 (683 rows).
By kind: gap 187, decision 112, risk 94, follow-up 87, test-gap 81, deviation 57, deferred 30, pre-existing 14, test-fail 13, polish 8.

Slices not fully built (status partial or blocked): M0-T0.

## P0: holes and unsafe slices

None.

## P1: likely bugs and missing authority or ZDR coverage

| id | source | kind | finding |
|---|---|---|---|
| D1-001 | M0-CI | gap | space-gate cannot be green for SPA lint/typecheck/vitest or auth-core while they have failing ids until the M0-T0 baseline (docs/plans/evidence/baseline-<date>/) is captured on the base SHA and committed; without it a dirty run fails with an explicit message, a clean run passes. |
| D1-002 | M0-CI | decision | space-gate must be made a required status check in the repository branch protection for `space`; that is a GitHub setting outside the repo and was not changed. |
| D1-011 | M0-T0 | gap | the Day-0 baseline JSON under docs/plans/evidence/baseline-<date>/ is not captured; run `node scripts/tests/baseline-capture.mjs --root <clean worktree of the base SHA>` in the wave-end test phase (capturing from this tree would hide Space edits). |
| D1-012 | M0-T0 | risk | UNVERIFIED seed assumptions: `/api/v1/me` id field, invite response id (fallback: `invitation` table in auth_service via docker exec psql, columns organization_id/email/status), and the organization-room response shape for `--ensure-room`; the first live run may need small fixes. |
| D1-013 | M0-T0 | gap | B, C and E are plain org members; Space roles editor and viewer (and E's non-membership) are not seeded because no member-role Space route exists yet (add_space_member takes only member_id). WS01 specs must grant roles through the Space routes. |
| D1-022 | P-CFG | gap | scripts/tests/zdr-contract-sync.test.sh GOLDENS row for Quarry's cross-plane-token-requests.json pins 40bd3586... but the file and both .sha256 pins are 0849784e...; the train check (control-train-repin.sh, check mode) reports it stale, fix is one `--apply` run in the first train PR (not done here: hotspot file 21, outside the lock group). |
| D1-028 | P-CX1 | risk | UNVERIFIED that Convex deploy/codegen accepts table definitions imported from convex/schema/*.ts; the TS type and exported-JSON equivalence are verified, the live push (docker restart convex-gateway, wait for "Convex functions ready!") is not. |
| D1-046 | P-SPA1 | gap | node --test action-surface-contract.test.mjs dispatcher-equality fails at HEAD already (11 registry ids have no handlers.rs arm: mcp.approve_tool, tools.set_policy, chat.execute_approved_plan, chat.run.continue, chat.answer_question, approvals.decide, audit.verify_chain, audit.export_log, audit.*_siem_destination); registry ids are byte-identical to HEAD, so this comes from the sibling Model Plane build, not from the move. |
| D1-072 | WS01-S1 | gap | `message_search.rs` (`FROM messages m`, line ~60) was not filtered for `post_kind`; if it backs a model-callable or owner-memory search, human posts can surface there (UNVERIFIED which callers); add the same predicate. |
| D1-073 | WS01-S1 | gap | `loop_checkpoint.rs` and `memory_control.rs:574` read messages by id/sequence only; they do not read content into model context (UNVERIFIED), no filter added. |
| D1-074 | WS01-S1 | risk | migration 0071 must be applied before the new session-core binary runs: the dedupe lookup and INSERT now name `post_kind`, so an un-migrated database fails every append. |
| D1-075 | WS01-S1 | test-gap | the v1 thread-append digest does not bind `post_kind`; the owner path accepts it as sent. WS01-S2 (v2 digest) and WS01-S3 (member path refuses v1 and non-post) must close this before any member appends. |
| D1-090 | WS01-S3 | risk | handler pre-check (authorize_append) establishes caller org and human-ness; the in-tx gate assumes both (org_matches=true), so a future second caller of append_message_in_tx_as(Member) must repeat that check. |
| D1-091 | WS01-S3 | test-gap | no test exercises the member INSERT COALESCE binds (types text/bigint) against a real Postgres; compile-only so far. |
| D1-102 | WS01-S4 | risk | UNVERIFIED that axum 0.7.9 `Router::merge` merges the POST onto the existing GET/DELETE method routers of `/v1/threads` and `/v1/threads/:thread_id/messages` without a panic; cargo check cannot show it. Any router-construction test (e2e) will; if it panics, chain `.post()` in http_routes.rs instead. |
| D1-103 | WS01-S4 | gap | the BFF sendId allows a leading `_` or `-` (^[A-Za-z0-9_-]{16,64}$) but model-gateway and Session Core need an alphanumeric first character; WS01-S5 must mint or validate to that shape or such a send is a 400 `invalid_client_message_id`. |
| D1-109 | WS01-S5 | gap | Neither route has a per-user rate limiter at the BFF (plan: flood bounded by the 8,000-char cap and a fleet limiter; moderation is F-t1-043), so a member can post at request speed. |
| D1-110 | WS01-S5 | risk | model-gateway error bodies that carry error.code pass through unfiltered (status and body); S4 uses fixed words, but a changed upstream message would reach the browser as is. |
| D1-120 | WS01-S6 | risk | Accepted-entry reconcile for a reply compares the thread `updated_at` with the one captured at send time; if Session Core does not bump `updated_at` on a member post (WS01-S1 decision D2 says bump-on-reply stays, UNVERIFIED on this branch) the optimistic row lingers until its 30 s TTL, then disappears while the real row shows only after the transcript refetch. |
| D1-121 | WS01-S6 | gap | The optimistic reply row is not keyed to the real message id (the receipt carries `message_id` but the transcript turn mapper does not read `post_kind`/`message_id` yet), so between accept and the next transcript refetch the reply can be briefly missing or doubled; WS01-S7b/WS15 should key rows by message id. |
| D1-132 | WS01-S7a | deviation | DEVIATION: plan path `components/aux/` is unusable on Windows (`aux` is a reserved device name; tsc failed to resolve `./aux/*`), so the pane files live in `components/sidepane/`; WS01-S7b, WS12-S7, WS13-S4a, WS17-S6, WS20-S5 and P-SPA1 must use `sidepane/` (or the plan must be amended). |
| D1-142 | WS01-S7b | decision | Collapse counts rendered turns (user and assistant), so the row says "N more messages in the thread"; the plan's "N replies" wording is mapped to the transcript turns the timeline actually loaded (capped by the transcript normaliser, 30 per post), while the post footer uses `reply_count` when the listing carries it. |
| D1-143 | WS01-S7b | gap | The pane composer and the room composer each create their own send pipeline; an Ask streamed from the pane and one from the room can run at the same time and share the global own-stream store (`space-live-work`). Needs a single-flight rule per member. |
| D1-150 | WS01-S8 | deviation | DEVIATION: the plan's "one BFF line" (send_idempotency and connectivity_banner in space context features) is not built; the SPA reads VITE_SPACE_SEND_IDEMPOTENCY_API / VITE_SPACE_CONNECTIVITY_BANNER_UI build-time switches (same as thread pane) until WS01-S5 adds SpaceFeatureFlags and features. |
| D1-151 | WS01-S8 | risk | the room never sets `zdr` on streamChat, so the key is sent for a ZDR org too; the server (model-gateway) must keep refusing/not persisting for ZDR (UNVERIFIED that the managed start key row is skipped for ZDR). |
| D1-158 | WS02-S1 | risk | deferred triggers fire at commit and swallow errors, so a trigger defect shows only as space_event_errors rows and the gauge mp_session_space_event_errors (15-min cadence); no alert is wired. |
| D1-159 | WS02-S1 | gap | threads.thread.updated/archived/deleted rows use actor_type 'system' with no id (the trigger cannot see the verified caller; mp.actor_id GUC is set only on some paths), so "who archived" is not in the log. |
| D1-174 | WS02-S3 | gap | space-surface.v1.json has no row for GET /v1/spaces/:space_id/events or the SpaceEvents gRPC (ZDR class poll_only); left out to avoid a concurrent edit of that golden and its pin, add it with the WS06 registry owner. |
| D1-175 | WS02-S3 | gap | the golden space-events.json is hand-authored, not generated; a drift test against the real closing/kind/refs tables is owed so code and golden cannot diverge. |
| D1-184 | WS02-S4 | gap | Control-declined mint (403/404) always becomes a plain 403 space_stream_forbidden; the typed SpaceDenial details (WS03-S5 mapper) are not carried, so the SPA cannot tell "not entitled" from "removed" except by code. |
| D1-185 | WS02-S4 | risk | any org-core outage makes every org poll_only (fail-closed per plan), so a short org-core blip downgrades all streams until the SPA's next reconnect/recheck. |
| D1-195 | WS02-S5 | decision | Cursor is advanced only from `space_event` frames (contiguity-checked) and from `connected` when resumed=false; heartbeat and closing ids are NOT adopted, because rows up to head may still be in flight and adopting would skip them. Cost: after a very long idle a resume may get cursor_too_old and resnapshot (safe, one refetch). |
| D1-205 | WS03-S1 | deviation | DEVIATION: this branch already has migration 032_space_browser_entitlement (column browser_entitled, no Go field), so the catalogue has the plan's eight bits and no browser bit; UpsertEffectPolicy never touches browser_entitled so it is preserved, but the org-admin console and presets will not show it until a catalogue row plus an EffectPolicy field are added. |
| D1-214 | WS03-S2 | gap | personal-retrieval, personal-import, personal-thread, watch-create and schedule-create resolver paths return 503 for a typed refusal (entitlement off, role floor, kind unsupported) because their existing status is 503; DEVIATION-free (plan says statuses unchanged) but WS03-S5 must key off `code`, never off status, and a later slice may want to move these to 403. |
| D1-224 | WS03-S3 | decision | SPACE_POLICY_ADMIN_WRITES is wired through a main.go setter (SetSpacePolicyAdminWrites, same pattern as SCHEDULE_CONTRACT_ENABLED) instead of being read in NewServer as the design says; same effect (read once at startup, never in a handler), fewer edits to the server.go hotspot. |
| D1-225 | WS03-S3 | decision | scopes spaces:policy:admin:read/write and the env line were edited directly in apps/Control Plane/docker-compose.yml (the P-CFG rules say they ride the weekly train; ledger batch 1 already lists WS03-S3). plane-service-principals.json has no user-core scope rows, so nothing else changes; recreate user-core for the scopes to take effect. |
| D1-226 | WS03-S3 | decision | ValidateEntitlementChange refuses only NEWLY enabling a ZDR-inert or ZDR-unproven bit under Space ZDR; an already-enabled bit left alone passes, and turning bits off is always allowed (an org already in that state is not forced to clean up to change anything else). |
| D1-234 | WS03-S4 | gap | agent-action-admission.json (and its .sha256 pin plus the four vendored copies in Model Plane and zdr-contract-sync.test.sh) has no rows for spaces.set_effect_entitlements and spaces.apply_effect_preset; admission-contract.test.ts and the cross-plane copy checks fail until regenerated (VEREVON_UPDATE_GOLDEN=1, refresh pin, re-vendor). |
| D1-242 | WS03-S5 | gap | the plan says the retrieval decline on thread create "carries the reason in the turn's gap", but space_context has no gap channel and model-gateway's ThreadSpaceContext is a fixed struct; the decline still degrades to ungrounded with no reason surfaced (needs a Model Plane contract field, not a gateway edit). |
| D1-258 | WS03-S7 | gap | space-send-pipeline.ts catch (around the strategy call) still sets errorMessage from error.message for a thrown ApiError (non-stream failure such as thread create); it should call describeError with the ApiError's code/details so a typed denial shows presenter copy there too. |
| D1-259 | WS03-S7 | gap | SpaceRoomTimeline.tsx swallows the transcript 403 (design: WS07 owns it); it must call presentSpaceDenial; not touched here (other lane's file). |
| D1-267 | WS04-S1 | risk | DEVIATION: the flag also gates `syncOrganizationRoom`, so with `SPACE_ORG_ROOM_RECONCILE=0` (production default) the existing room-activation sync stops too, as the design kill switch says; production org rooms then stay owner-only until the flag is on. |
| D1-268 | WS04-S1 | gap | the roster is declared with `roster_mode:"org_derived"` but Control ignores it until WS04-S4, so manual Application member writes can still diverge from org-core until the next sync. |
| D1-275 | WS04-S2.gw | gap | the handlers.rs arm for spaces.update_room_details exists before the SPA registry fragment (ws04.ts) does, so action-surface-contract.test.mjs may fail until the SPA leg lands; land the SPA leg before the wave test phase. |
| D1-276 | WS04-S2.gw | risk | UNVERIFIED Convex HTTP error shape; the helper reads `errorMessage` from a non-success (or status:"error") body and substring-matches stable codes. Live check needed against the real convex-gateway that a thrown Error returns this body and not a bare 500. |
| D1-283 | WS04-S2 | gap | gateway and SPA legs of S2 (PATCH /api/v1/spaces/{space_ref}, flag space_room_details_ui, spaces.update_room_details action, SpaceRoomSettings, topic in header) are not built; they sit in the gw and spa-room lock groups and need P-SPA1 fragments (P-SPA1 not built yet). |
| D1-284 | WS04-S2 | decision | the gateway must map Convex error messages (room_details_invalid:<field>:<reason>, space_not_found, space_kind_not_editable, room_not_active) to 400/404/409/409; room_not_active has no plan code, suggest 409 room_state_conflict or 503. |
| D1-285 | WS04-S2 | risk | D-WS04-6 open (topic under ZDR); topic stored as a label for all orgs, never in prompts/events/notifications; gateway PATCH should not need an org_zdr_lookup refusal under the default. |
| D1-292 | WS04-S2.spa | gap | no Control-outage or sidebar refresh after rename; SpacePage only refetches its own context, so CoreSidebarSpacesPanel and SpacesIndexPage keep the old name until their next load. |
| D1-307 | WS04-S3 | gap | Gateway leg (domains/spaces/directory.rs, flag SPACE_PEOPLE_SEARCH_API, space_directory_rate_limiter, GET /api/v1/directory/people) and SPA leg (spaces-client searchDirectoryPeople, SpaceMemberControls debounced search box) are not built here (other lock groups); Control route is live behind the scope only. |
| D1-322 | WS04-S4 | gap | space_memberships and audience rows of an erased user are still not removed by the GDPR saga (existing gap V19); only the new ledger ids are pseudonymised, so a hard-deleted user can stay an active member row until WS05 or a follow-up fixes it. |
| D1-323 | WS04-S4 | risk | UNVERIFIED whether the production credential registry (USER_CORE_SERVICE_CREDENTIALS) lives outside the dev compose; production scope additions for spaces:membership:act and :read are not in the repo and must be added at SD-13 enablement. |
| D1-324 | WS04-S4 | decision | any Control-native write (add, role change, remove, leave) sets the row source to member_ops, including on the registrar row, so a later declarative PUT cannot re-promote or reactivate it; the registrar is no longer protected only by the ownerKey guard once it was acted on. |
| D1-333 | WS04-S5 | risk | A persistently rejected audience (Control 409) is replaced on every cron run (new revision each 10 min); a bounded retry or backoff per Space is not built. |
| D1-334 | WS04-S5 | gap | UNVERIFIED that Control's recipient-audiences registration (POST) accepts the lifecycle token principal for application-space-lifecycle with the scope the S4 compose edit grants; production credential registry (USER_CORE_SERVICE_CREDENTIALS) is outside the repo. |
| D1-335 | WS04-S5 | gap | One candidate missing from the Application users projection makes replaceRecipientAudience throw for the whole set (projection_lag); the audience then stays stale until the projection catches up rather than registering the subset. |
| D1-342 | WS04-S6 | gap | no per-user member-ops rate limiter (30/min, plan gateway section): adding `space_directory_rate_limiter` needs an AppState field plus the 5 test literals P-GW1 already touched, so it was not built; Control's own limits and idempotency still apply. |
| D1-343 | WS04-S6 | risk | production credential registry (`USER_CORE_SERVICE_CREDENTIALS`) is UNVERIFIED outside dev compose; the gateway flag defaults on in dev but without `spaces:membership:act` every ACT call is 403, so SPA controls would show and fail in an environment with the flag on and the scope missing. |
| D1-344 | WS04-S6 | test-gap | `scripts/tests/space-contract-vectors-sync.test.sh` needs the Go golden `apps/Control Plane/user-core/internal/spaces/testdata/room-role-matrix.v1.json`, which WS04-S4 never created; the vendored SPA copy `src/shared/api/contracts/space-room-role-matrix.v1.json` is generated from roles.go semantics and must be copied byte for byte into Go testdata. |
| D1-352 | WS04-S7 | gap | gateway, SPA and docs legs of S7 are not built (control_membership_url helper at six read sites, archive routes, flag space_room_archive_api, banner, composer prop, sidebar group, ADR amendment); until the gateway sends readable=1 an archived room still reads as not-found in the SPA. |
| D1-353 | WS04-S7 | decision | DEVIATION: archive and unarchive reuse the room writer scope spaces:membership:act (plan table says ACT) rather than a new scope, so the same credential kill switch covers both; no scope or compose change. |
| D1-354 | WS04-S7 | risk | thread-read-decision (including the WS02 stream audience) now issues for archived rooms, so realtime streams stay open on an archived room; confirm that is wanted (D-WS04-3 says reads stay). |
| D1-361 | WS05-S1 | test-gap | existing audit-core api tests that call /v1/audit or /v1/usage with a non-admin or role-less delegation will now see narrowed or 403 results; the wave test phase must update them, none were run. |
| D1-362 | WS05-S1 | risk | the gateway clamps `limit` to 200 and the Trust Center default fell from 500 to 200, so large orgs aggregate fewer tool_action rows (the aggregation was already a window, not a total); a server-side aggregate endpoint is the real fix (D-WS05-1 option C). |
| D1-370 | WS05-S2 | risk | integration-corev2 still holds scim_tokens table, store methods (Create/Find/MarkUsed) and Postgres/memory implementations; the dead create/find paths remain in the store interface until WS05-S8 decides the token owner. |
| D1-376 | WS05-S3 | risk | Existing SSO users already promoted to admin by the old getRole rule keep their admin role; no cleanup or audit of the member table was done. |
| D1-390 | WS06-S2 | decision | SD-12 still open; flag ships observe, so a ZDR-flagged Space still persists posts on legacy create/append until the enforce flip (counter first). |
| D1-401 | WS06-S4 | gap | Three model-gateway rows are classed n/a with basis "unreviewed" (no ZDR guard in the handler): POST /v1/threads/:id/presentation (stores a title/preview), PUT /v1/memories/controls, POST /v1/internal/mail-task/prepare-thread; a reviewer must decide whether the first two need require_non_zdr_durable_mutation. |
| D1-402 | WS06-S4 | gap | Seven BFF rows carry min_role "unverified" (members add/remove, agents available/bind/state/revoke, thread presentation); roles were read from handler hints only where a role constant exists (instructions write editor+, create agent owner/manager, grants owner/admin). |
| D1-403 | WS06-S4 | gap | No BFF Space route calls crate::zdr::org_zdr_lookup today; the 3 DurableWrite rows (agent-action-grants create, instructions PATCH, thread presentation PATCH) are classed refuse in the registry but the gateway does not enforce it yet (basis "registry"). |
| D1-417 | WS06-S7 | gap | the spawn permit in process_host is held for the spawn call only, not the background process lifetime, so live background processes are not counted against EXECUTION_SANDBOX_MAX_CONCURRENT; decide in WS06-S8/S10 whether to hold it for the process lifetime. |
| D1-418 | WS06-S7 | gap | nothing reads ExecOutcome.limit_hit yet; the shell and code_interpreter callers report a killed-for-output child as a plain nonzero exit (-1). WS06-S8 classification should map LimitHit to a typed step error. |
| D1-425 | WS07-S1 | gap | SpaceCockpit tab panels are constructed eagerly by the caller's `tabs` object (SpacePage memo), so a throw in a panel component body happens outside the per-tab RegionBoundary and reaches the outlet boundary; true per-tab isolation needs lazy tab values (functions) in SpacePage. |
| D1-439 | WS07-S11 | deviation | DEVIATION: features.room_artifacts has no delivery path yet (WS07-S9 gateway space_context is unbuilt); S11 reads build-time VITE_SPACE_ROOM_ARTIFACTS_UI (dev on, prod off) like the thread pane; swap to the BFF flag when S9 lands. |
| D1-447 | WS07-S11 | risk | my path-limited commit swept in another builder's uncommitted WS03-S7 hunks from core.ts and SpaceRoomComposer.tsx (shared files); verify WS03-S7's own commit does not double-apply or miss them. |
| D1-448 | WS07-S12 | gap | the gateway BFF `/api/v1/spaces/{ref}/artifacts` route (S9's gateway half, flag SPACE_ROOM_ARTIFACTS_API) does not exist on this branch; against a real gateway the tab shows the error state until it lands (the client speaks the plan's wire contract). |
| D1-465 | WS07-S2 | gap | client_contract in version.json is a literal 1 in vite.config.ts; WS07-S3 must replace it with the shared CLIENT_CONTRACT constant and add the parity test, or the two drift. |
| D1-481 | WS07-S4 | gap | gateway `shared_read` hunk sits uncommitted inside `domains/spaces/mod.rs` next to another builder's edits; the committer of that file must keep it (else the room never shows the room-level notice, only per-post ones). |
| D1-525 | WS07-S9 | gap | SCOPE SPLIT: only the Model Plane half is built (proto, session-core handler, migration 0085, model-gateway route and client). Still owed: gateway BFF `domains/spaces/room_artifacts.rs` + `space_features.room_artifacts` flag (SPACE_ROOM_ARTIFACTS_API, 404 `space_room_artifacts_disabled`), `retention.zdr` via org_zdr_lookup, `shared_read`, SPA `getSpaceArtifacts` (outside this lock group). |
| D1-526 | WS07-S9 | gap | Go bindings under `apps/Model Plane/go/gen` and mp-events-py `_gen` not regenerated for the new RPC (buf generate owed at L0). |
| D1-527 | WS07-S9 | test-gap | tests/support/offline_downstreams.rs and offline_downstreams_contract.rs enumerate the AppState session clients; the new `space_room_read_client` is not covered by that offline guard. |
| D1-541 | WS07-S9a | risk | Room read is detected by a non-empty `space_read_decision_ref` only; session-core verifies it, so a bogus ref fails the read, but a future owner-path caller that sends a ref would also get the reduced view (safe direction). |
| D1-547 | WS09-S1 | deviation | DEVIATION: the guard module is `session-core/src/run_phase.rs` (design path), not `src/space/run_phase.rs`, because `P-SC1` has not created `src/space/` on this branch; a later `git mv` into `space/` is a pure move. |
| D1-548 | WS09-S1 | deviation | DEVIATION: `SPACE_RUN_THREAD_GUARD` defaults ON when unset and compose passes `:-true` (plan table says ON by merge in every environment and "S1 must ship ON"; the plan file list said `:-false` with dev override true). Kill switch is env `false`/`0`/`off`/`no` plus restart. |
| D1-549 | WS09-S1 | gap | other `prepare_managed_run*` callers (`model-gateway/src/grpc.rs` invoke RPCs, `http_routes.rs:10983`, `session_flow.rs` browser start) do not map `thread_run_active`; they surface the generic session error. WS08 wake and WS20 browser need the 409 mapping. |
| D1-550 | WS09-S1 | gap | the refused send has already appended the user message (append precedes start), so the room shows an orphan post with no reply; copy says the message was posted. A durable queue is the epic (SD-15 C2). |
| D1-558 | WS17-S1 | gap | Thread-READ decisions do not self-heal space_kind (VerifiedSpaceRead is Copy and carries no kind); only create and append stamp a legacy NULL-kind personal thread, so a personal thread that is only read stays contained (fail closed, forgets) until its next append. |
| D1-559 | WS17-S1 | gap | Other background writers of agent memory were not audited for containment (skill proposals, learning_events, memory_dedup/grounding passes); dream_once is gated, others UNVERIFIED. |
| D1-560 | WS17-S1 | risk | Rule C7 legacy exclusion applies to every non-contained reader and scans source_links through threads by primary key per memory row; cost on a large agent_memory table is UNVERIFIED (LIMIT-bound queries, but the filter runs per candidate). |
| D1-569 | WS17-S2 | decision | DEVIATION: S3's origin_kind column does not exist yet, so a withheld row is derived from source_links (thread:<id> of a contained thread) with the same shape as the S1 recall exclusion (memory_scope::legacy_room_origin_space); S3 should replace both with the origin_kind='room_unverified' column. |
| D1-570 | WS17-S2 | risk | withheld flagging is on only while MEMORY_SPACE_CONTAINMENT is on; with the flag off the rows are recalled again and are listed unflagged. |
| D1-571 | WS17-S2 | gap | only scope='user' rows are listed (list_user_memory); legacy room rows with scope thread/org/agent are not shown to anyone as withheld. |
| D1-626 | tests-T-run-1 | test-fail | session-core metric_label_guard: mp_session_space_zdr_flag_total (WS06-S2) uses labels action and enforced that are not on METRIC_LABELS; closed values, so the list is stale; test exempts the two exact pairs under DEPTH-WS06S3-1 until the list is edited. |
| D1-638 | tests-T-sc-2 | test-fail | model-gateway zdr_posture every_non_get_route_is_classified: POST /v1/threads and POST /v1/threads/:thread_id/messages (WS01-S4 human posts) have no row in space-surface.v1.json; kept as KNOWN_UNREGISTERED with DEPTH-sc2-1 (the lint stays live for every other route). |
| D1-639 | tests-T-sc-2 | test-fail | gateway tests/space_routes_zdr_classes.rs: six BFF routes have no row (POST threads, POST threads/{id}/messages, PATCH spaces/{ref}, PATCH members/{id}, GET artifacts, GET events); kept as KNOWN_UNREGISTERED with DEPTH-sc2-2. Fix is the golden rows plus re-vendor, re-pin and the digest in zdr-contract-sync.test.sh. |
| D1-646 | tests-T-sc-3 | deferred | grpc_space_memory_tests.rs (15 DB tests: C1-C7 append/create/dream/index/search/list/flag-off/letta): written and compiling, never run against Postgres; first DB-tier run may expose seed or schema mismatches. |
| D1-651 | tests-T-spa-room-1 | test-fail | DEPTH-SPA-1 (WS01-S6) space-post-sender.ts attempt(): `journal.get(sendId)` straight after `journal.begin` returns undefined under Solid 2 (writes show after flush), so a human post never calls createSpacePost/replyInSpaceThread and the entry sits at `sending` with the box disabled; skipped in space-post-sender.test.ts, SpaceRoomComposer.test.tsx and SpacePage.test.tsx, other post-path tests use a flush-wrapped journal. |
| D1-652 | tests-T-spa-room-1 | test-fail | DEPTH-SPA-2 (WS01-S8) SpaceConnectivityBanner calls watchBrowserConnectivity() in its component body whose sync() writes a signal: REACTIVE_WRITE_IN_OWNED_SCOPE halts the reactive system whenever the banner mounts (dev default on); composer and page suites stub VITE_SPACE_CONNECTIVITY_BANNER_UI=false as a workaround. |
| D1-662 | tests-T-spa-room-2 | test-fail | DEPTH-SPA-2 SpaceConnectivityBanner / watchBrowserConnectivity(): the first sync() writes a signal inside the component body, so with the banner switched on it throws REACTIVE_WRITE_IN_OWNED_SCOPE (Solid 2) and halts reactivity; 7 banner cases and 1 lib case skipped. Fix: start the watcher from onMount or an effect. |
| D1-674 | tests-T-spa-shell-1 | test-fail | RegionBoundary.test.tsx (DEPTH-SHELL-1): Solid 2.0.0-rc.0 passes Errored's fallback the error as an accessor, RegionBoundary hands that function to FailureCard unread, so stale_deploy, session_expired and gateway_unreachable never classify and Retry is never hidden; fix is calling `error()` in RegionBoundary (two tests skipped until then). |

## Deviations from the plan (all severities)

| id | source | sev | finding |
|---|---|---|---|
| D1-003 | M0-CI | P2 | DEVIATION: the "buf path fix" was already present (buf.yml quotes all proto paths since e45caa71c); buf.yml was left unchanged, and its branch filter (main, develop, fix/ui-premium-restore) still skips PRs based on `space`, which space-gate's proto job covers. |
| D1-016 | M0-T0 | P2 | plan row 23 lists `local-auth-b.setup.ts`; not created, the seed script writes the storage states A..F and the new `space` Playwright project reads E2E_STORAGE_STATE_A, so no setup project is needed. |
| D1-017 | M0-T0 | P2 | `space-shell.spec.ts` moved from the `e2e` project (e2e@verevon.dev state) to the new `space` project (member A state) because every `space-*.spec.ts` is routed there; it now uses `skipUnlessSpace`. It needs E2E_SPACE_REF owned by A. |
| D1-032 | P-GW1 | P2 | DEVIATION: the plan says the 5 test AppState literals compile unchanged, but adding the `space_features` field forces one `space_features: Default::default()` line into each; `for_tests()` then keeps future fields out of them. |
| D1-033 | P-GW1 | P2 | DEVIATION: `org_zdr_enabled` could not be a thin wrapper that distinguishes failure, so the body moved into `org_zdr_lookup` (tri-state `OrgZdrPosture`) and `org_zdr_enabled` now delegates via `treat_as_zdr()`; behaviour is identical (cache, non-caching of failures, empty org scope is ZDR). |
| D1-039 | P-SC1 | P2 | DEVIATION: SpaceRoomReadService is registered in grpc.rs serve() (the real add_service site), not main.rs; main.rs only gets `mod space;`. |
| D1-040 | P-SC1 | P2 | DEVIATION: AppendAs::Member is refused (permission_denied) inside append_message_in_tx_as, a tiny fail-closed addition to an otherwise pure move; WS01-S3 replaces it with authorize_append. |
| D1-078 | WS01-S1 | P2 | DEVIATION: `run_context.rs` also had an exhaustive `ListConversationRequest {` literal the plan did not list; fixed with `..Default::default()` (default false = contained). |
| D1-079 | WS01-S1 | P2 | DEVIATION: (b)'s `client_message_id` (proto 18, messages column, dedupe) is already on this branch, so S1 adds no dedupe code of its own. |
| D1-088 | WS01-S2 | P3 | DEVIATION: golden files and tests not written (build rule: tests owed); `thread-append-v2.golden.json` is therefore absent and the Rust copy sha pin is owed. |
| D1-095 | WS01-S3 | P2 | plan names src/space/append.rs for member rules and authz.rs for the gate; done so, but member INSERT stamping stays inline in grpc.rs (one statement shared with the owner path via COALESCE binds) rather than a separate fn. |
| D1-099 | WS01-S4 | P2 | DEVIATION: chat-path key pass-through and `prepare_managed_run_authenticated` `client_message_id` param were already built by (b) (`send_receipt_id`, `prepare_managed_run_authenticated_with_receipt`); nothing added for them in S4. |
| D1-100 | WS01-S4 | P2 | DEVIATION: `derive_message_key` not added; the post key is passed through as an opaque id validated by `valid_client_message_id` (Session Core's own shape), so managed_start_key.rs is untouched. |
| D1-101 | WS01-S4 | P2 | DEVIATION: `session_post_error` and the post JSON live in `space_routes/posts.rs` (hotspot rule), not in `http_routes.rs`; `require_non_zdr_durable_mutation` was made `pub(crate)` for it. Unauthenticated maps to 401 (plan said other = 503). |
| D1-132 | WS01-S7a | P1 | DEVIATION: plan path `components/aux/` is unusable on Windows (`aux` is a reserved device name; tsc failed to resolve `./aux/*`), so the pane files live in `components/sidepane/`; WS01-S7b, WS12-S7, WS13-S4a, WS17-S6, WS20-S5 and P-SPA1 must use `sidepane/` (or the plan must be amended). |
| D1-133 | WS01-S7a | P2 | DEVIATION: the pane is an absolutely positioned child of `.verevon-space-workroom` that reserves its width as right padding, not a grid column, because global.css has three conflicting grid definitions of the workroom; below 760px it covers the room. |
| D1-150 | WS01-S8 | P1 | DEVIATION: the plan's "one BFF line" (send_idempotency and connectivity_banner in space context features) is not built; the SPA reads VITE_SPACE_SEND_IDEMPOTENCY_API / VITE_SPACE_CONNECTIVITY_BANNER_UI build-time switches (same as thread pane) until WS01-S5 adds SpaceFeatureFlags and features. |
| D1-169 | WS02-S1 | P3 | DEVIATION: file is src/space/events.rs (P-SC1's module) not src/space_events.rs; mod line is in space/mod.rs, so main.rs gains no mod line; messages trigger WHEN also limits roles to user/assistant/system. |
| D1-194 | WS02-S4 | P3 | plan names domains/spaces/stream.rs and a local SpaceStreamConfig Extension; implemented as StreamRuntime Extension (config plus limiter plus slots) because module_routes() takes no state. |
| D1-196 | WS02-S5 | P2 | Plan says "three failures within 10 s go to degraded"; built as 3 consecutive failures (reset on first heartbeat/event) plus an immediate degraded when a live stream dies, so the DoD "degraded within 35 s of stopping session-core" holds with full-jitter backoff. |
| D1-197 | WS02-S5 | P2 | Presence effect is keyed on a confirmed room context (`context()` loaded, no error), not on the threads listing; threads refetch failure (fetcher returns undefined) would otherwise have fired offline/online churn. |
| D1-205 | WS03-S1 | P1 | DEVIATION: this branch already has migration 032_space_browser_entitlement (column browser_entitled, no Go field), so the catalogue has the plan's eight bits and no browser bit; UpsertEffectPolicy never touches browser_entitled so it is preserved, but the org-admin console and presets will not show it until a catalogue row plus an EffectPolicy field are added. |
| D1-215 | WS03-S2 | P2 | DEVIATION: `Denial` carries a `Kind` field the plan's struct sketch omits (its detail table names `kind`); `DiagnoseDenial` is in `internal/spaces/denial.go`, not `repository.go`, to keep the hot file untouched. |
| D1-227 | WS03-S3 | P2 | view `effective` treats an enabled zdr_unproven bit as effective under ZDR (Control still issues it); only zdr_inert is ineffective. Added additive field `zdr_blocked` per entitlement for the console; design text says effective = enabled and not ZDR-blocked. |
| D1-235 | WS03-S4 | P2 | DEVIATION: the plan names `dispatchers_spaces/ws03.rs` but no `dispatchers_spaces` module existed on this branch; created `dispatchers_spaces.rs` as the per-workstream seam and made `ticket_string`, `ticket_bad_request`, `owner_json_response_to_envelope` in dispatchers.rs `pub(super)` (visibility only) so the child module reuses them. |
| D1-236 | WS03-S4 | P2 | DEVIATION: MODEL_ACCESS (model-access.ts) is `Record<ActionId,...>`, so the two ids needed rows to typecheck; both closed as exempt('human_identity_or_security'), the same class as org.update_zdr. |
| D1-290 | WS04-S2 | P3 | DEVIATION: schema change went into convex/schema/spaces.ts (post P-CX1), not schema.ts; RoomDetailsResult is a flat type because convex-core compiles strict:false (union narrowing fails). |
| D1-299 | WS04-S3.gw | P2 | DEVIATION: the 60/min limiter is a process-wide `static OnceLock<RateLimiter>` in directory.rs, not an `AppState.space_directory_rate_limiter` field, because a new AppState field would force edits to five foreign test state literals (browser.rs, chat/shared.rs, orchestration.rs, stream_e2e.rs, main.rs tests); WS04-S4/S8 (20 join, 30 member ops) will need the same choice. |
| D1-308 | WS04-S3 | P2 | DEVIATION: scope mapping added via registerSpaceScopeRule in scopes_directory.go (the seam hook runs for all internal paths) instead of editing service_auth.go; route via registerFeatureRoute in routes_directory.go instead of editing server.go routes. server.go gained only one Server field, main.go one wiring line. |
| D1-357 | WS04-S7 | P2 | DEVIATION: ResolveCurrentUserMembership/ResolveReadableUserMembership share resolveUserMembership with registration_state = ANY($4), so the scan test must pin callers, not a literal; membership JSON gains additive state and archived_at. |
| D1-363 | WS05-S1 | P2 | DEVIATION: plan lists a new `.github/workflows/audit-core.yml`; `audit-core-ci.yml` already exists on this branch, so no workflow was added. |
| D1-364 | WS05-S1 | P2 | DEVIATION: report mode logs would-deny but the plan's "max 3 days" cap is not enforced in code (no clock source for the flag); it is an operational rule. |
| D1-380 | WS05-S3 | P2 | DEVIATION: auth-core could not be type-checked (node_modules absent in the worktree); the edit is a pure deletion plus a helper returning the literal 'member'. |
| D1-381 | WS05-S3 | P2 | DEVIATION: no scopes_spaces.go seam exists on this branch; the scope table is still the switch in user-core service_auth.go. |
| D1-383 | WS06-S1 | P2 | DEVIATION: the tree has 14 untyped sites, not 11; also typed grpc.rs scheduled-result gate (feature_not_supported scheduled_step), memory_grpc.rs memory-controls refusal and message_search.rs recall refusal (DURABLE_MUTATION_FORBIDDEN, PermissionDenied kept) so the inventory scan stays green. |
| D1-395 | WS06-S3 | P2 | DEVIATION: METRIC_LABELS also carries check, code, decision, kind, mode, op, phase, point, source, surface, to, tool (found by scanning the tree) beyond the design's 14 + route; each is an enum or bucketed value, but the value sets were not individually audited. |
| D1-404 | WS06-S4 | P2 | DEVIATION: no CONSUMERS/VENDORED rows added (plan lists a CONSUMERS row): under REQUIRE_CONTRACT a CONSUMERS row without a vendored copy fails CI; add both with the lint tests that read the copy. |
| D1-405 | WS06-S4 | P2 | DEVIATION: .github/workflows/contract-goldens.yml untouched; its existing path filter "apps/Model Plane/api/contracts/**" already covers the new golden. |
| D1-406 | WS06-S4 | P2 | DEVIATION: GW/domains/spaces/mod.rs comment pointing at the table not written (file owned by the gateway builder; the module moved to domains/spaces/mod.rs after P-GW1); the table itself is the golden. |
| D1-407 | WS06-S4 | P2 | DEVIATION: the BFF route-class set gains `Config` (plan lists ReadMetadata, ModelRead, DurableWrite, Chat) so K4-14 configuration writes are not forced to refuse; Chat has no rows yet. |
| D1-427 | WS07-S1 | P2 | DEVIATION: chunk recovery already exists (src/app/chunk-recovery.ts, WS09-S5, 30 s window, notice) while the design puts it at shared/build/chunk-recovery.ts with a 60 s guard; WS07-S2 should reuse the existing module instead of adding a second `vite:preloadError` listener. |
| D1-439 | WS07-S11 | P1 | DEVIATION: features.room_artifacts has no delivery path yet (WS07-S9 gateway space_context is unbuilt); S11 reads build-time VITE_SPACE_ROOM_ARTIFACTS_UI (dev on, prod off) like the thread pane; swap to the BFF flag when S9 lands. |
| D1-440 | WS07-S11 | P2 | DEVIATION: roomItems lives in room-artifact-items.ts, not room-artifacts.ts, so spaces-client (which imports the mapper) does not pull chat-media-markdown into the API layer. |
| D1-461 | WS07-S2 | P2 | DEVIATION: chunk recovery already exists (src/app/chunk-recovery.ts from WS09-S5: key verevon.chunk-reload.v1, 30 s window, reload guards, StaleBuildNotice); the plan's shared/build/chunk-recovery.ts (key verevon.chunk-reload, 60 s) was not created, and the watchdog reuses the existing key. |
| D1-462 | WS07-S2 | P2 | DEVIATION: Report-Only CSP header not added to the snippet (the plan puts it in S6); a marker comment is left in nginx-security-headers.conf. |
| D1-472 | WS07-S3 | P2 | DEVIATION: `client_policy` lives in `SpaceFeatureFlags` (config.rs), not as a new `AppState` field, so none of the five AppState test literals (which now name `space_features: Default::default()`) needed an edit; the plan's "AppState.client_policy" is reachable as `state.space_features.client_policy`. |
| D1-473 | WS07-S3 | P2 | DEVIATION: `POST /api/v1/client-diagnostics` also accepts build `dev` (a local build's id, which the watchdog beacons) besides 1-12 hex; an over-512-byte body returns 400 `invalid_diagnostic` (not 413) because the plan lists only 400 for this route. |
| D1-482 | WS07-S4 | P2 | DEVIATION: `loadSpaceTranscript` maps ANY 403 to `restricted` (plan says 403 `space_read_not_authorized`); matches existing panel behaviour and tolerates WS03 denial-code changes, but a revoked-membership 403 also reads as "not shared". |
| D1-488 | WS07-S5 | P2 | DEVIATION: the scheme allowlist already existed as features/chat/lib/safe-href.ts (isSafeLinkHref, WS26-S3) and unsafe links already rendered as a span; safe-link.ts wraps it instead of duplicating, and shared/ now imports from features/chat (layering inversion; move safe-href into shared/lib later). |
| D1-489 | WS07-S5 | P3 | DEVIATION: blocked links keep the existing verevon-chat-link-blocked span with its localized title; the plan's "title={href}" is applied to safe anchors only. |
| D1-494 | WS07-S6 | P2 | DEVIATION: no `$csp_candidate` nginx variable (design table mention); the Report-Only header is a plain add_header in the shared snippet, so there is no per-location switch to disable it other than editing the snippet. |
| D1-528 | WS07-S9 | P2 | DEVIATION: keyset is (created_at, message_id, artifact_id), not (created_at, message_id), because one message can carry several artifacts and the two-column key would skip or repeat them. |
| D1-547 | WS09-S1 | P1 | DEVIATION: the guard module is `session-core/src/run_phase.rs` (design path), not `src/space/run_phase.rs`, because `P-SC1` has not created `src/space/` on this branch; a later `git mv` into `space/` is a pure move. |
| D1-548 | WS09-S1 | P1 | DEVIATION: `SPACE_RUN_THREAD_GUARD` defaults ON when unset and compose passes `:-true` (plan table says ON by merge in every environment and "S1 must ship ON"; the plan file list said `:-false` with dev override true). Kill switch is env `false`/`0`/`off`/`no` plus restart. |
| D1-565 | WS17-S1 | P2 | DEVIATION: resolver lives in src/space/memory_scope.rs (plan file list says src/memory_scope.rs; the P-SC1 seam moved new logic under space/); migration number 0073 (not the design file's 0052 placeholder). |
| D1-566 | WS17-S1 | P2 | DEVIATION: docker-compose.override.yml not edited; the base compose default is ON everywhere, so the override needs no entry. |
| D1-575 | WS17-S2 | P2 | DEVIATION: Go bindings regenerated for memory.pb.go only (native buf); sessions.pb.go and the new space_room proto from other builders were left alone. |

## Decisions the builders could not make (all severities)

| id | source | sev | finding |
|---|---|---|---|
| D1-002 | M0-CI | P1 | space-gate must be made a required status check in the repository branch protection for `space`; that is a GitHub setting outside the repo and was not changed. |
| D1-010 | M0-CI | P3 | baseline-capture.mjs gained SPACE_TARGET_MP / SPACE_TARGET_GW env overrides (cargo target dirs) so CI on Linux does not create a `C:/dev/...` directory in the checkout; the Windows defaults are unchanged. |
| D1-014 | M0-T0 | P2 | the effect-policy compliance fields (assistant_collaboration, contract, internal, standard, swedencentral, space) are TEST values taken from set-space-effect-policy.sh's own usage example, overridable by SPACE_E2E_PRIVACY_*; per-WS entitlement bits are added via SPACE_E2E_POLICY_EXTRA_FLAGS until WS03's admin route replaces the script. |
| D1-015 | M0-T0 | P2 | owner order 2026-10-27 (no Verevon-routed models in tests): the seed selects no model; space-fixtures.ts `spaceTestModel` requires an explicit E2E_SPACE_MODEL and rejects verevon, verevon-*, auto, default. The plan's "deterministic model from a seed fixture" (E3/SD-27) is therefore an open operator fact: which non-Verevon model id is deterministic in dev. |
| D1-023 | P-CFG | P2 | P-CFG is a mechanism slice with no code seam, so it ships a runbook (config/PRINCIPAL-TRAIN.md), a re-pin script and a batch ledger; the batch-to-slice assignment is derived from plan row 19 and workstream tables and is PROVISIONAL (UNVERIFIED which scope lands in batch 1 vs 2). |
| D1-030 | P-CX1 | P3 | the comment block on recipient-set ownership sat above spaceMemberGrants but describes spaceRecipientAudiences; moved with its table (comment-only change). |
| D1-031 | P-CX1 | P3 | grouping chosen by feature (identity, chat, planner, agents, ingest, spaces, spaceDeletion, spaceAudience, conversationProjection); later slices may add new files (e.g. schema/spaceAttention.ts) and one line in schema.ts. |
| D1-034 | P-GW1 | P2 | `SpaceFeatureFlags` ships with no fields (each consumer slice appends one field plus one `from_env` line); `space_flag(key)` is the dev-on/prod-off reader and is unused until the first flag lands. |
| D1-035 | P-GW1 | P2 | `AppState::for_tests()` delegates to `crate::tests::test_state(false)` in main.rs instead of duplicating the literal; if that test module is renamed the helper breaks. |
| D1-042 | P-SC1 | P3 | authz.rs holds only AppendAs; authorize_append and member_append_denial are WS01-S3 per plan. |
| D1-043 | P-SC1 | P3 | space_routes/ holds only mod.rs (merges space_sse::router); posts.rs, artifacts.rs, timeline.rs are created by their slices. |
| D1-047 | P-SPA1 | P2 | spaces-client is split as core.ts (the whole pre-split client, unchanged) plus an index.ts barrel; a per-feature split of core.ts into spaces-client/<feature>.ts is left for the slices that add calls (pure-move scope). |
| D1-048 | P-SPA1 | P2 | the existing 13 spaces.* contracts live in a single fragment base.ts (no workstream owns them); new fragments are added to domains/spaces/index.ts. |
| D1-049 | P-SPA1 | P2 | the gateway contract test reads every domains/spaces/*.ts except *.test.ts; a fragment must therefore not contain unrelated `id: '...'` literals. |
| D1-051 | P-SPA1 | P3 | SpaceTabDefinition.enabled and SpacePaneDefinition.enabled are presentation switches only, never authorisation (documented in the files). |
| D1-056 | P-SPA2 | P2 | slot entries are static arrays (SPACE_COMPOSER_TOOLBAR, SPACE_COMPOSER_CHIPS) in components/composer/SpaceComposerSlots.tsx, same pattern as SPACE_TABS; a slice adds one entry. A runtime register() API was not built. |
| D1-057 | P-SPA2 | P2 | the send pipeline is a Solid-style factory (createSpaceSendPipeline) not a React-style hook; strategies are keyed by mode id in a Record so adding a mode without a strategy is a type error. |
| D1-058 | P-SPA2 | P2 | pipeline takes a `host` (the composer's props, read at call time) and `describeError`/`resolveTier` callbacks, so lib/ never imports from components/. |
| D1-066 | P-UC1 | P2 | Feature scope rules are checked only after the core table, so a feature can never re-scope an existing path; the plan did not specify the order. |
| D1-067 | P-UC1 | P2 | Registrars are init()-appended slices (no edit of shared files); route order across features follows Go file-name init order, which gin does not depend on for distinct paths. |
| D1-076 | WS01-S1 | P2 | `reply_count` = user rows after the first under the audience ceiling (assistant answers not counted); `last_author_subject_id` = author of the latest user row with an author, same ceiling. Packed as one jsonb column because sqlx tuples cap at 16 columns. |
| D1-077 | WS01-S1 | P2 | owner `post` requires role `user` (else invalid_argument); a dedupe repeat whose `post_kind` differs from the stored row is `already_exists`. |
| D1-083 | WS01-S2 | P2 | v2 shape is `model.thread.append\0v2\0` header, v1 fields with action_schema_hash = v2 constant, then one `post_kind` field (name, NUL, u64 BE length, value) after idempotency_key and before the two revisions; Control and Rust encode it identically (golden test owed to pin it). |
| D1-092 | WS01-S3 | P2 | a member post still bumps threads.last_activity_at (list ordering); plan says "no thread-column UPDATE" for authority columns only. UNVERIFIED whether owner list ordering should move on member posts. |
| D1-093 | WS01-S3 | P2 | dedupe author check applies to every post_kind=post (owner posts too), not only members; the (thread, client_message_id) unique index is still author-agnostic, so two authors cannot share one id in a thread (409 for the second) - answers open question 2 as "key does not include author". |
| D1-111 | WS01-S5 | P2 | one send id (space-post-<client_send_id>) is the session key, the Control idempotency key and the stored client_message_id; it avoids model-gateway's leading-alphanumeric rule rejecting ids that start with _ or -, and the SPA must never reuse a send id across Spaces. |
| D1-112 | WS01-S5 | P2 | content is trimmed at the BFF (design D7) and the Control digest and the model-gateway body use the trimmed bytes; model-gateway (S4) forwards untrimmed, so any other caller must digest exactly what it sends. |
| D1-122 | WS01-S6 | P2 | Agent path (`agent_turn`) is NOT journaled; its stages stay in the composer's exchange block (WS01-S8) to avoid two progress surfaces. The journal kind exists in the vocabulary only. |
| D1-123 | WS01-S6 | P2 | One unsettled post per composer (busy blocks a second send); a failed post is re-sent with the same sendId only when the trimmed text and target are unchanged, otherwise the stale entry is dropped and a fresh id is minted (the server refuses one key with two bodies). |
| D1-125 | WS01-S6 | P2 | A human post is not blocked by a thread awaiting approval (it starts no run); only the Ask path keeps that gate. Revisit if the room wants posts to wait too. |
| D1-134 | WS01-S7a | P2 | `SpaceRestrictedNotice.adminCta` is `{label, href}` (plan names the prop but not its shape); WS07-S4 may change it. |
| D1-142 | WS01-S7b | P1 | Collapse counts rendered turns (user and assistant), so the row says "N more messages in the thread"; the plan's "N replies" wording is mapped to the transcript turns the timeline actually loaded (capped by the transcript normaliser, 30 per post), while the post footer uses `reply_count` when the listing carries it. |
| D1-144 | WS01-S7b | P2 | With `features.human_posts` on and the pane available, the per-post Reply button opens the pane (design D8) instead of pointing the room composer at the thread; the room composer's reply-target path stays for pane-off or flag-off. |
| D1-160 | WS02-S1 | P2 | seq is monotonic per Space and never reset on a generation bump (bump sets min_seq = head_seq+1); contiguity is per generation from the connect head, which S3 must honour. |
| D1-161 | WS02-S1 | P2 | run_state has an SQL (space_event_run_state, used by triggers) and a Rust mirror (events::run_state); a db test must assert agreement, else C17's enum change edits two places. |
| D1-177 | WS02-S3 | P2 | the dedicated stream pool reads DATABASE_URL with the same default as store.rs (duplicated literal), and falls back to the main pool if it cannot be configured. |
| D1-178 | WS02-S3 | P2 | a decision within 10 s of expiry gets a 1 s lease (closes lease_expired, client re-mints) rather than a refusal. |
| D1-189 | WS02-S4 | P2 | requests with the x-zdr header also get poll_only, beyond the plan (org posture only); harmless, fails closed. |
| D1-193 | WS02-S4 | P3 | limiter and slot counters are per replica, in-process slots; the 30 rpm bucket uses the shared Dragonfly backend with local fallback. |
| D1-195 | WS02-S5 | P1 | Cursor is advanced only from `space_event` frames (contiguity-checked) and from `connected` when resumed=false; heartbeat and closing ids are NOT adopted, because rows up to head may still be in flight and adopting would skip them. Cost: after a very long idle a resume may get cursor_too_old and resnapshot (safe, one refetch). |
| D1-206 | WS03-S1 | P2 | UpsertEffectPolicy(ctx, policy, actor) keeps full-row semantics for Go callers; the operator handler uses UpsertEffectPolicyPartial with the set of stated keys (absent or null keeps the stored value, new row defaults false). |
| D1-207 | WS03-S1 | P2 | unwired policyAudit outbox fails the write (no permissive default); the live test thread_read_decision_live_test.go only had its call site updated and its Repository does not wire an outbox, so that live test needs a fake enqueuer when tests are written. |
| D1-208 | WS03-S1 | P2 | creation event (operator_created) stores before_entitlements as {} and the audit "changed" list shows every bit set true as from false; privacy_changed is true. |
| D1-216 | WS03-S2 | P2 | validators for the six entitlement keys with a combined `ref == "" // !entitled` check were split (entitlement first) keeping the same error string, so a missing entitlement is typed `space_entitlement_off` even when the resource ref is also empty. |
| D1-217 | WS03-S2 | P2 | thread-read own-scheduled fallback: when the shared resolver finds no row and the personal-owner resolver returns a typed denial, that denial is reported (the personal Space is proven by that query) under the unchanged "current shared Space authority required" message. |
| D1-224 | WS03-S3 | P1 | SPACE_POLICY_ADMIN_WRITES is wired through a main.go setter (SetSpacePolicyAdminWrites, same pattern as SCHEDULE_CONTRACT_ENABLED) instead of being read in NewServer as the design says; same effect (read once at startup, never in a handler), fewer edits to the server.go hotspot. |
| D1-225 | WS03-S3 | P1 | scopes spaces:policy:admin:read/write and the env line were edited directly in apps/Control Plane/docker-compose.yml (the P-CFG rules say they ride the weekly train; ledger batch 1 already lists WS03-S3). plane-service-principals.json has no user-core scope rows, so nothing else changes; recreate user-core for the scopes to take effect. |
| D1-226 | WS03-S3 | P1 | ValidateEntitlementChange refuses only NEWLY enabling a ZDR-inert or ZDR-unproven bit under Space ZDR; an already-enabled bit left alone passes, and turning bits off is always allowed (an org already in that state is not forced to clean up to change anything else). |
| D1-228 | WS03-S3 | P2 | denied-write audit event (space_effect_policy_write_denied) is enqueued only when an owner/admin hits the switched-off write route; role or membership denials do not call the store (plan test: member 403 with store never called), so they leave no audit row. |
| D1-237 | WS03-S4 | P2 | gateway requires expected_entitlement_revision (non-negative int) because Control's PUT does; the plan's shape-check list did not name it. |
| D1-238 | WS03-S4 | P2 | unknown PUT body fields are refused (400) rather than forwarded, so a stray org_id/role/actor never reaches Control even though Control also uses DisallowUnknownFields. |
| D1-245 | WS03-S5 | P2 | all mapped membership failures use operation code `space_read_not_authorized`; the plan lists no separate membership operation code, so a distinct code would need a plan decision. |
| D1-247 | WS03-S5 | P3 | the flag defaults on in dev via space_flag (APP_ENV unset) and off in production; compose base is `:-true`, production override `"false"`; flag changes only the body shape, never status or the refusal itself. |
| D1-251 | WS03-S6 | P2 | the console calls the BFF routes through the client rather than the `spaces.set_effect_*` action ids (the ids exist for the action surface and agents-excluded flows); no UI operation lacks a contract, but a later pass may route saves through the action dispatcher. |
| D1-262 | WS03-S7 | P2 | a pass-through (flag off) Control body keeps only top-level code, so extractError yields no details; the presenter falls back to code and the table's resolvable_by, losing the entitlement name. Top-level details/resolvable_by are not read. |
| D1-263 | WS03-S7 | P2 | unknown reason tokens (newer server) show the generic line with the server's resolvable_by CTA, never server text. |
| D1-273 | WS04-S1 | P3 | `convex-stubs.d.ts` gained `cronJobs: any` because the repo typecheck shadows `convex/server` with stubs. |
| D1-274 | WS04-S1 | P3 | dev default on is set in the two compose files, not in code; handler reads only `1` or `true` as on. |
| D1-277 | WS04-S2.gw | P2 | gateway validation is shape and size only (string, at most 2000 chars, at least one field); NFC, trim, 1-120 and 0-250 and control/bidi rules run only in Application's spaceRoomRules.ts and come back as room_details_invalid:<field>:<reason>. A Rust copy of the rules was not built to avoid two drifting copies (plan says "same rules"). |
| D1-279 | WS04-S2.gw | P2 | room_not_active maps to 409 `room_not_active`, not in the plan's four codes (400/403/409 space_kind_not_editable/503); the SPA ERROR_COPY needs an entry for it and for space_not_found. |
| D1-284 | WS04-S2 | P1 | the gateway must map Convex error messages (room_details_invalid:<field>:<reason>, space_not_found, space_kind_not_editable, room_not_active) to 400/404/409/409; room_not_active has no plan code, suggest 409 room_state_conflict or 503. |
| D1-293 | WS04-S2.spa | P2 | form offered for every active kind=room including the organization room (Application only refuses non-room kinds); plan does not exclude the org room, confirm with owner whether renaming it is intended. |
| D1-295 | WS04-S2.spa | P2 | name and topic limits (120, 250) are duplicated as input maxlength hints in SpaceRoomSettings.tsx; the authoritative copy stays in Application spaceRoomRules.ts and field-specific copy keys off its reason tokens (name_required, name_too_long, topic_too_long, control_character, bidi_override). |
| D1-301 | WS04-S3.gw | P2 | gateway pre-validates q (100 chars), limit (1..=50), cursor (512 bytes), exclude_member_of (200 bytes) so an invalid query never reaches Control; Control stays the authority and re-validates. |
| D1-302 | WS04-S3.gw | P2 | Control 5xx or unreachable maps to 503 `directory_unavailable`; Control typed 4xx (403 member_not_in_organization, 400) passes through unchanged. |
| D1-309 | WS04-S3 | P2 | Ordering is (prefix-match rank, lower(name), id) per design D2.7 and the cursor carries all three keys, which extends the plan row's "(lower(name), id)" cursor. |
| D1-311 | WS04-S3 | P2 | Caller must be an active user_org_memberships member of the delegated org (local projection, same backstop every Space query uses), else 403 member_not_in_organization; it is not re-resolved from auth-core. |
| D1-312 | WS04-S3 | P2 | exclude_member_of is applied only when the caller is an active member of that Space in the org (checked in SQL), otherwise silently ignored, so it cannot probe other rosters. |
| D1-316 | WS04-S3.spa | P2 | The search is a read, so no action contract was added in src/shared/actions (reads such as listOrganizationMembers have none); add one only if the model should ever search people. |
| D1-324 | WS04-S4 | P1 | any Control-native write (add, role change, remove, leave) sets the row source to member_ops, including on the registrar row, so a later declarative PUT cannot re-promote or reactivate it; the registrar is no longer protected only by the ownerKey guard once it was acted on. |
| D1-325 | WS04-S4 | P2 | an add on an already-active application_sync row returns changed=false and does not adopt the row, so the legacy Convex sync can still deactivate it until the next role change; adoption on add may be wanted at cutover. |
| D1-326 | WS04-S4 | P2 | member_count counts active user rows (not services, not filtered by org status); the roster may show a different number. |
| D1-329 | WS04-S4 | P2 | PUT .../memberships now publishes membership_changed for brand-new user rows too (active=true), where before only reactivation published; consumers already handle active=true (clears a revocation) but Session Core was not re-checked (UNVERIFIED). |
| D1-338 | WS04-S5 | P2 | viewers are included in the audience (every active human member that is an active org member), matching Control's candidate read; 512 cap enforced by plan, Control's maxSpaceMembers is 5000. |
| D1-345 | WS04-S6 | P2 | the SPA learns the writer flag from `data.writer_api` added to the roster response (additive); the plan's `features` booleans on `GET /spaces` and `.../context` were not built, so other flags still have no SPA signal. |
| D1-353 | WS04-S7 | P1 | DEVIATION: archive and unarchive reuse the room writer scope spaces:membership:act (plan table says ACT) rather than a new scope, so the same credential kill switch covers both; no scope or compose change. |
| D1-355 | WS04-S7 | P2 | archive and unarchive advance authority_revision only (not membership or audience), so signed decisions issued before the change go stale; no NATS membership event is published. |
| D1-373 | WS05-S2 | P2 | scimError now emits status as a string per RFC 7644 and omits scimType for the 501; the 501 handler does no auth, so unauthenticated callers learn SCIM is unavailable (intended). |
| D1-379 | WS05-S3 | P2 | resolveSsoDefaultRole throws for any non-member value (not only admin) and runs only when SSO_ENABLED=true, so a bad value with SSO off does not block boot. |
| D1-384 | WS06-S1 | P2 | terminalization.rs ZDR managed-run-with-thread refusal typed as feature_not_supported("agent_run") per plan; the plan's ":1586" site is now at :1663 (zero-retention run has no durable assistant message) and is DURABLE_MUTATION_FORBIDDEN. |
| D1-385 | WS06-S1 | P2 | message_search recall (a read) uses DURABLE_MUTATION_FORBIDDEN like memory preflight (WS06-D5); no vocabulary feature fits, a new feature key would change goldens. |
| D1-390 | WS06-S2 | P1 | SD-12 still open; flag ships observe, so a ZDR-flagged Space still persists posts on legacy create/append until the enforce flip (counter first). |
| D1-410 | WS06-S4 | P3 | Model-gateway inference routes (/v1/ai/*, /v1/invoke*) are class n/a with basis "posture-decided": they send content to a processor under a ZDR-aware route and store nothing; /v1/browser/grants is refs_only. |
| D1-415 | WS06-S5 | P3 | ZDR withholds space:processes but still issues the decision (lease granted, process RPCs refused by sandbox-manager), matching the existing unentitled-Space pattern; WS03 should flip process_registry to inert-under-ZDR now that S5 is built (S6 is also required by the plan). |
| D1-420 | WS06-S7 | P2 | the permit is taken in spawn_capture before the timeout starts, so queue wait does not eat the execution timeout; worst case a step waits 5 s extra. |
| D1-428 | WS07-S1 | P2 | build id for Copy details is read with `typeof __VEREVON_BUILD_ID__` in classify-client-error.ts; WS07-S2 should point it at build-info.ts once that exists. |
| D1-432 | WS07-S10 | P2 | Room HTML consent is keyed on id + version + content hash, not on artifact id alone, because ids are only unique inside a thread (two threads can both emit "report") and a new revision can change the markup. |
| D1-433 | WS07-S10 | P2 | Extra renderers registered through `extraArtifactRenderers` (sibling b) are applied on the chat surface only; they carry no `surfaces`/`trust`, so the room never uses them until they are folded into `viewer-registry.ts` (X26). |
| D1-449 | WS07-S12 | P2 | the tab's visibility is the SPA build-time flag (VITE_SPACE_ROOM_ARTIFACTS_UI via roomArtifactsEnabled), a stand-in for `features.room_artifacts` from the BFF; swap when S9's gateway half delivers the flag. |
| D1-450 | WS07-S12 | P2 | any 403 on the aggregate maps to the restricted notice (plan: 403 or shared_read.available=false), including `active_membership_required`, which is really a membership problem, not a shared-read one. |
| D1-451 | WS07-S12 | P2 | the tab re-reads the aggregate on every re-entry and clears its transcript cache then; live (sender-stream) entries are dropped only when the aggregate carries the same id with version >= theirs, so a lagging aggregate can briefly show a live entry beside it. |
| D1-456 | WS07-S13 | P3 | Rule text is bilingual (NO/EN picked by i18n.tr); the plan gives only the EN text, so the NO wording is an unreviewed translation. |
| D1-463 | WS07-S2 | P2 | HSTS now comes from a `map` ($verevon_hsts) instead of `set` + `if`; empty value means no header, same behaviour as before, and the snippet is safe to include in locations. |
| D1-464 | WS07-S2 | P2 | build id is the first 12 chars of SOURCE_REVISION; empty, unverified or unknown becomes `dev`, which never banners. A local `docker build` without args therefore has no stale-tab banner. |
| D1-474 | WS07-S3 | P2 | `CLIENT_MIN_CONTRACT` env may only raise the compiled-in minimum (never lower); `CLIENT_VERSION_POLICY` unknown text means `banner`. Revision/build_date are read once in `ClientPolicy::from_env`, not per request. |
| D1-475 | WS07-S3 | P2 | `enforce` degrades to banner in the SPA (client-contract.ts `contractAction`) when no unsaved-work probe is registered; `registerUnsavedWork` shares the existing WS09-S5 reload-guard set in chunk-recovery.ts, so the chat and dashboard composers already count as probes and `enforce` can auto-reload once anything is registered. |
| D1-495 | WS07-S6 | P2 | `report-uri` only (as planned); the Reporting API (`report-to` plus a `Reporting-Endpoints` header) is not set, so `application/reports+json` is accepted by the route but no browser sends it yet. |
| D1-503 | WS07-S7 | P2 | turn-to-agent mapping is by exact agentName match (exactly one roster agent), per plan; WS08 per-turn agent id should replace agentForName. |
| D1-508 | WS07-S8 | P2 | DEVIATION: WS07-D9 spike not run; in-house popover used because @kobalte/core is 2.0.0-alpha.0 and unproven under Solid 2 rc; revisit if a shared HoverCard is wanted. |
| D1-511 | WS07-S8 | P2 | attribution for blocking/working is by name heuristic (sole active agent, or the thread's last loaded assistant turn names it); WS08 agent ids should replace it. |
| D1-517 | WS07-S9.gw | P2 | the flag field is `room_artifacts` (plan name), not the `_api` suffix other Space flags use; env var is SPACE_ROOM_ARTIFACTS_API. |
| D1-518 | WS07-S9.gw | P2 | a ZDR org still reaches model-gateway (Session Core returns an empty page); `retention.zdr` is only copy for the empty state, never a gate. |
| D1-519 | WS07-S9.gw | P2 | Space membership is not called separately (same as the transcript route): Control's read decision is the membership authority, a decline is a 403. |
| D1-529 | WS07-S9 | P2 | the request has no user_id field (plan proto); the decision subject is bound to the caller's verified user id, and a service identity is refused permission_denied. |
| D1-530 | WS07-S9 | P2 | created_at on SpaceArtifactSummary is an RFC3339 string (microseconds, Z); the plan did not fix the proto type. |
| D1-538 | WS07-S9.spa | P2 | getSpaceArtifacts was not rewritten: WS07-S12 already shipped it in spaces-client/room-artifacts.ts and it matches the gateway wire, so S9.spa is a no-op (status already-present). |
| D1-543 | WS07-S9a | P2 | Kill switch accepts exactly `1` or `true` (case-insensitive); the env var is read per request, not cached in AppState (cheap, and lets ops flip it with a restart only). |
| D1-554 | WS09-S1 | P2 | `ListRuns`/`ListRunsByThread` `status_filter` now compares the derived phase, so `status_filter=running` matches live runs and `queued` no longer matches them; no known caller filters on `queued` (not checked across Go callers). |
| D1-562 | WS17-S1 | P2 | thread_is_contained_by_id fails closed (DB error reads as contained) on read and retrieval paths; ReaderScope::Contained audience fields are unused until WS17-S3 (dead_code allowed). |
| D1-563 | WS17-S1 | P2 | index_agent_memory answers a wide topic in a contained thread with the empty id (no write); the typed memory_scope_contained refusal is raised by the index_memory handler first. Other direct callers of index_agent_memory see "nothing written". |
| D1-569 | WS17-S2 | P1 | DEVIATION: S3's origin_kind column does not exist yet, so a withheld row is derived from source_links (thread:<id> of a contained thread) with the same shape as the S1 recall exclusion (memory_scope::legacy_room_origin_space); S3 should replace both with the origin_kind='room_unverified' column. |
| D1-574 | WS17-S2 | P2 | "keep or delete" is the existing per-row delete button plus a badge; no separate keep action or confirm copy was added. |

## P2 and P3 by source

### M0-CI
- D1-003 P2 deviation: DEVIATION: the "buf path fix" was already present (buf.yml quotes all proto paths since e45caa71c); buf.yml was left unchanged, and its branch filter (main, develop, fix/ui-premium-restore) still skips PRs based on `space`, which space-gate's proto job covers.
- D1-004 P2 gap: org-core's Postgres suites (TEST_DATABASE_URL plus CONTROL_LIFECYCLE_TEST_DATABASE_URL with a fixture marker row, see org-service.yml) are not provisioned in control-go, so they skip there; user-core and audit-core get the generic TEST_DATABASE_URL.
- D1-005 P2 risk: UNVERIFIED that pnpm 11.3.0 with --frozen-lockfile matches auth-core's and convex-core's pnpm-lock.yaml (auth-service.yml runs plain `pnpm install` on Node 20); verevonv3-ci.yml uses 11.3.0 for its own lockfile.
- D1-006 P2 risk: UNVERIFIED that session-core-db (`cargo test -p session-core -- --ignored`, whole crate) is green today; model-plane-rust-ci.yml runs only filtered subsets (zdr_receipt, loop_checkpoint, ...) of the ignored tests.
- D1-007 P2 gap: Playwright milestone gate (E2E_REQUIRE_SPACE=1 on a second Vite server) is not wired into CI; it needs a running stack and stays a manual milestone step.
- D1-008 P3 follow-up: no data-service jobs (imports-core, Data Plane Postgres contracts, retrieval-engine-rs) were added; plan 4.5.5 lists them but no Space slice in W1 touches them. changes script has no area for them.
- D1-009 P3 follow-up: scripts/gates/*.sh are committed 100644 (Windows); the workflows call them through `bash`, so the mode does not matter.
- D1-010 P3 decision: baseline-capture.mjs gained SPACE_TARGET_MP / SPACE_TARGET_GW env overrides (cargo target dirs) so CI on Linux does not create a `C:/dev/...` directory in the checkout; the Windows defaults are unchanged.

### M0-T0
- D1-014 P2 decision: the effect-policy compliance fields (assistant_collaboration, contract, internal, standard, swedencentral, space) are TEST values taken from set-space-effect-policy.sh's own usage example, overridable by SPACE_E2E_PRIVACY_*; per-WS entitlement bits are added via SPACE_E2E_POLICY_EXTRA_FLAGS until WS03's admin route replaces the script.
- D1-015 P2 decision: owner order 2026-10-27 (no Verevon-routed models in tests): the seed selects no model; space-fixtures.ts `spaceTestModel` requires an explicit E2E_SPACE_MODEL and rejects verevon, verevon-*, auto, default. The plan's "deterministic model from a seed fixture" (E3/SD-27) is therefore an open operator fact: which non-Verevon model id is deterministic in dev.
- D1-016 P2 deviation: plan row 23 lists `local-auth-b.setup.ts`; not created, the seed script writes the storage states A..F and the new `space` Playwright project reads E2E_STORAGE_STATE_A, so no setup project is needed.
- D1-017 P2 deviation: `space-shell.spec.ts` moved from the `e2e` project (e2e@verevon.dev state) to the new `space` project (member A state) because every `space-*.spec.ts` is routed there; it now uses `skipUnlessSpace`. It needs E2E_SPACE_REF owned by A.
- D1-018 P2 risk: ZDR on org Z needs ORG_CORE_SERVICE_TOKEN and a plan that includes ZDR (402 otherwise) and the script only warns on refusal; org Z may stay ZDR-off. Policy PUT on org Z sets zero_data_retention=true independently of org-core's switch.
- D1-019 P2 test-gap: the `.sh` was only syntax-checked and run in plan mode; the Node half was loaded and its loopback guard exercised, nothing was sent to any stack.
- D1-020 P3 follow-up: baseline-capture runs Go modules found by `go.mod` under Control Plane and Model Plane/go/services only; Data Plane, Quarry and Application Plane Go services are not in the watch-list set.
- D1-021 P3 follow-up: new files are LF; the worktree is otherwise CRLF (git warned for the README).

### P-CFG
- D1-023 P2 decision: P-CFG is a mechanism slice with no code seam, so it ships a runbook (config/PRINCIPAL-TRAIN.md), a re-pin script and a batch ledger; the batch-to-slice assignment is derived from plan row 19 and workstream tables and is PROVISIONAL (UNVERIFIED which scope lands in batch 1 vs 2).
- D1-024 P2 gap: "rows added alphabetically" is a documented rule only; no script enforces it (existing rows in plane-service-principals.json and the compose credential JSON may not be sorted today, UNVERIFIED), so a lint would first need a baseline.
- D1-025 P2 gap: control-train-repin.sh covers only the Quarry token-request manifest; other goldens still re-pin by hand via their owner's generator plus the zdr-contract-sync row.
- D1-026 P3 follow-up: the PR scope-request template is in the runbook, not in .github/PULL_REQUEST_TEMPLATE; add one if the integrator wants it enforced.
- D1-027 P2 test-gap: repin script --apply path (copy plus three pin rewrites, CRLF-normalised digest) was not run here to avoid editing the Quarry-owned manifest pins; only check mode was exercised.

### P-CX1
- D1-029 P2 follow-up: Convex treats files under convex/schema/ as modules, so the next `convex codegen` may add `schema/*` entries to _generated/api.d.ts; regenerate rather than hand-edit.
- D1-030 P3 decision: the comment block on recipient-set ownership sat above spaceMemberGrants but describes spaceRecipientAudiences; moved with its table (comment-only change).
- D1-031 P3 decision: grouping chosen by feature (identity, chat, planner, agents, ingest, spaces, spaceDeletion, spaceAudience, conversationProjection); later slices may add new files (e.g. schema/spaceAttention.ts) and one line in schema.ts.

### P-GW1
- D1-032 P2 deviation: DEVIATION: the plan says the 5 test AppState literals compile unchanged, but adding the `space_features` field forces one `space_features: Default::default()` line into each; `for_tests()` then keeps future fields out of them.
- D1-033 P2 deviation: DEVIATION: `org_zdr_enabled` could not be a thin wrapper that distinguishes failure, so the body moved into `org_zdr_lookup` (tri-state `OrgZdrPosture`) and `org_zdr_enabled` now delegates via `treat_as_zdr()`; behaviour is identical (cache, non-caching of failures, empty org scope is ZDR).
- D1-034 P2 decision: `SpaceFeatureFlags` ships with no fields (each consumer slice appends one field plus one `from_env` line); `space_flag(key)` is the dev-on/prod-off reader and is unused until the first flag lands.
- D1-035 P2 decision: `AppState::for_tests()` delegates to `crate::tests::test_state(false)` in main.rs instead of duplicating the literal; if that test module is renamed the helper breaks.
- D1-036 P3 follow-up: the other 4 test literals (browser, chat/shared, orchestration, stream_e2e) could be rewritten to struct-update from `for_tests()` to shrink future churn; not done (not a pure seam).
- D1-037 P2 test-gap: `--all-targets` test build and clippy `-D warnings` were not run (rules forbid building tests now); the test-cfg `for_tests` path and the 5 literal edits are unverified by the compiler.
- D1-038 P2 risk: `spaces::module_routes()` is merged before `require_session`, so module routes inherit session gating; a module that needs a public route must not use this seam.

### P-SC1
- D1-039 P2 deviation: DEVIATION: SpaceRoomReadService is registered in grpc.rs serve() (the real add_service site), not main.rs; main.rs only gets `mod space;`.
- D1-040 P2 deviation: DEVIATION: AppendAs::Member is refused (permission_denied) inside append_message_in_tx_as, a tiny fail-closed addition to an otherwise pure move; WS01-S3 replaces it with authorize_append.
- D1-041 P2 follow-up: Go bindings (apps/Model Plane/go/gen/model_plane/v1) and mp-events-py _gen were not regenerated for space_room.proto; L0 buf generate owed.
- D1-042 P3 decision: authz.rs holds only AppendAs; authorize_append and member_append_denial are WS01-S3 per plan.
- D1-043 P3 decision: space_routes/ holds only mod.rs (merges space_sse::router); posts.rs, artifacts.rs, timeline.rs are created by their slices.
- D1-044 P2 risk: HEALTH_SERVICE_NAMES grew to 10; the standard health test array was updated to match (MessageSearch and LearningGovernance remain unlisted, pre-existing).
- D1-045 P3 test-gap: memory_control.rs include_str! test relies on the wrapper preceding append_message_in_tx_as; keep that order.

### P-SPA1
- D1-047 P2 decision: spaces-client is split as core.ts (the whole pre-split client, unchanged) plus an index.ts barrel; a per-feature split of core.ts into spaces-client/<feature>.ts is left for the slices that add calls (pure-move scope).
- D1-048 P2 decision: the existing 13 spaces.* contracts live in a single fragment base.ts (no workstream owns them); new fragments are added to domains/spaces/index.ts.
- D1-049 P2 decision: the gateway contract test reads every domains/spaces/*.ts except *.test.ts; a fragment must therefore not contain unrelated `id: '...'` literals.
- D1-050 P2 gap: the dispatchers_spaces/<ws>.rs gateway half of the action seam belongs to P-GW1/each WS slice; not touched here.
- D1-051 P3 decision: SpaceTabDefinition.enabled and SpacePaneDefinition.enabled are presentation switches only, never authorisation (documented in the files).
- D1-052 P3 follow-up: SpacePage still renders the thread pane inline; a pane registered later needs its own render branch next to it (the registry only resolves which pane the URL asks for).
- D1-053 P3 follow-up: the pane registry exports SpacePaneId = 'thread' as a closed union; each new pane widens it.
- D1-054 P3 gap: SpacePage.tsx now imports the pane registry; WS01-S7b and later slices that edit SpacePage should rebase on this commit.
- D1-055 P3 risk: SpaceCockpit evaluates visibleSpaceTabs() once per render (non-reactive); a flag that flips at runtime will not re-show a tab until remount.

### P-SPA2
- D1-056 P2 decision: slot entries are static arrays (SPACE_COMPOSER_TOOLBAR, SPACE_COMPOSER_CHIPS) in components/composer/SpaceComposerSlots.tsx, same pattern as SPACE_TABS; a slice adds one entry. A runtime register() API was not built.
- D1-057 P2 decision: the send pipeline is a Solid-style factory (createSpaceSendPipeline) not a React-style hook; strategies are keyed by mode id in a Record so adding a mode without a strategy is a type error.
- D1-058 P2 decision: pipeline takes a `host` (the composer's props, read at call time) and `describeError`/`resolveTier` callbacks, so lib/ never imports from components/.
- D1-059 P2 gap: the `ask` strategy still resolves the privacy tier inside itself; WS01-S6 `post` mode (no model) will ignore resolveTier and the model selection fields in SpaceSendInput.
- D1-060 P2 gap: SpaceSendStage ('started','accepted','settled') is emitted but nothing subscribes yet; the composer passes no onStage, WS01-S6 journal and WS15-S2 draft-clear-on-accept hook there.
- D1-061 P3 gap: the `/` and `@` picker (PickerItem) is not behind a slot; WS13-S7 adds its command kind directly in the composer as its design says.
- D1-062 P3 gap: the mode registry has no selector UI and `mode` is not exposed beyond the slot context; labels (Spør Verevon / Ask Verevon) are placeholders for WS01-S6.
- D1-063 P3 risk: SpaceComposerSlotContext.spaceRef is a getter, other fields are fixed accessors; an entry that destructures ctx loses reactivity.
- D1-064 P3 follow-up: the "Stopped by you" copy still attributes any server stopped frame to the member (WS09-S8).
- D1-065 P3 follow-up: the 2-line composer change named in plan 4.1.2 was not found as uncommitted in this tree (UNVERIFIED whether it was already committed on model-plane).

### P-UC1
- D1-066 P2 decision: Feature scope rules are checked only after the core table, so a feature can never re-scope an existing path; the plan did not specify the order.
- D1-067 P2 decision: Registrars are init()-appended slices (no edit of shared files); route order across features follows Go file-name init order, which gin does not depend on for distinct paths.
- D1-068 P2 test-gap: No route-table golden exists in the repo (plan says "route table golden unchanged"); a golden from gin Routes() should be added and captured at the end of W1 tests.
- D1-069 P2 gap: Non-spaces internal scopes (authz, memberships, mail-task, users) remain in service_auth.go; only spaces scopes moved, as the plan names scopes_spaces.go.
- D1-070 P3 follow-up: serviceScopeRequiresVerifiedDelegation still lives in service_auth.go and now delegates the spaces part to spaceScopeRequiresVerifiedDelegation; WS03-S3 registers the "spaces:policy:admin:" prefix there.
- D1-071 P3 follow-up: gofmt -l flags every file in the package because the worktree is CRLF; formatting was not checked beyond go vet.

### WS01-S1
- D1-076 P2 decision: `reply_count` = user rows after the first under the audience ceiling (assistant answers not counted); `last_author_subject_id` = author of the latest user row with an author, same ceiling. Packed as one jsonb column because sqlx tuples cap at 16 columns.
- D1-077 P2 decision: owner `post` requires role `user` (else invalid_argument); a dedupe repeat whose `post_kind` differs from the stored row is `already_exists`.
- D1-078 P2 deviation: DEVIATION: `run_context.rs` also had an exhaustive `ListConversationRequest {` literal the plan did not list; fixed with `..Default::default()` (default false = contained).
- D1-079 P2 deviation: DEVIATION: (b)'s `client_message_id` (proto 18, messages column, dedupe) is already on this branch, so S1 adds no dedupe code of its own.
- D1-080 P2 follow-up: Go bindings under `apps/Model Plane/go/gen` not regenerated (`buf generate` owed; capability-core consumes `ListConversationRequest` and gets the contained default).
- D1-081 P3 gap: list_threads `preview`/`title` laterals still read the latest/first message regardless of kind, so a member post can become a thread preview in the owner's list; decide in WS01-S3/S7b.
- D1-082 P3 test-gap: session-core grpc.rs / dreaming.rs have no test of the new predicates yet; see tests_owed.

### WS01-S2
- D1-083 P2 decision: v2 shape is `model.thread.append\0v2\0` header, v1 fields with action_schema_hash = v2 constant, then one `post_kind` field (name, NUL, u64 BE length, value) after idempotency_key and before the two revisions; Control and Rust encode it identically (golden test owed to pin it).
- D1-084 P2 gap: the member-path rules (subject must be the caller, not the owner; flag; revocation; stale revision) are S3. Today `append_message` still calls authorize_thread_owner first, so a v2 post from a non-owner is denied until S3.
- D1-085 P2 gap: Rust v2 verification accepts post_kind turn or post under schema v2 (digest binds whichever); Control never issues v2 for turn. Consider making the verifier require "post" under v2 when S3 lands.
- D1-086 P2 risk: Control handler rejects unknown post_kind with a plain `{"error": ...}` 400 (the Control-internal body style of this handler), not the typed envelope; the BFF maps it.
- D1-087 P3 follow-up: Control does not check the thread belongs to the Space (Session Core does); unchanged by this slice.
- D1-088 P3 deviation: DEVIATION: golden files and tests not written (build rule: tests owed); `thread-append-v2.golden.json` is therefore absent and the Rust copy sha pin is owed.
- D1-089 P3 follow-up: the grpc.rs line numbers in the plan drifted (verifier now ~:2992); located by symbol.

### WS01-S3
- D1-092 P2 decision: a member post still bumps threads.last_activity_at (list ordering); plan says "no thread-column UPDATE" for authority columns only. UNVERIFIED whether owner list ordering should move on member posts.
- D1-093 P2 decision: dedupe author check applies to every post_kind=post (owner posts too), not only members; the (thread, client_message_id) unique index is still author-agnostic, so two authors cannot share one id in a thread (409 for the second) - answers open question 2 as "key does not include author".
- D1-094 P2 gap: the member path does not run the source-scope lock; a conversation-only thread's member post carries empty metadata (no source_scope marker). Harmless while posts are excluded from model history.
- D1-095 P2 deviation: plan names src/space/append.rs for member rules and authz.rs for the gate; done so, but member INSERT stamping stays inline in grpc.rs (one statement shared with the owner path via COALESCE binds) rather than a separate fn.
- D1-096 P2 gap: owner-with-revoked-membership may still append as owner (existing behaviour, no revocation check on the owner path); not changed.
- D1-097 P3 follow-up: append_message_inner is now test-only (allow(dead_code) under not(test)); callers in tests could move to append_message_inner_as.
- D1-098 P3 follow-up: SPACE_HUMAN_POSTS_API const/api_enabled() are defined in mp-contracts for S4 but unused here; production/release compose pins for SPACE_HUMAN_POSTS_CORE are not set (default 0 already off).

### WS01-S4
- D1-099 P2 deviation: DEVIATION: chat-path key pass-through and `prepare_managed_run_authenticated` `client_message_id` param were already built by (b) (`send_receipt_id`, `prepare_managed_run_authenticated_with_receipt`); nothing added for them in S4.
- D1-100 P2 deviation: DEVIATION: `derive_message_key` not added; the post key is passed through as an opaque id validated by `valid_client_message_id` (Session Core's own shape), so managed_start_key.rs is untouched.
- D1-101 P2 deviation: DEVIATION: `session_post_error` and the post JSON live in `space_routes/posts.rs` (hotspot rule), not in `http_routes.rs`; `require_non_zdr_durable_mutation` was made `pub(crate)` for it. Unauthenticated maps to 401 (plan said other = 503).
- D1-104 P2 risk: a create whose append fails with a definite refusal (403/409/412/400) returns that refusal, leaving an empty thread with a decision-bearing session_key; session-core hides empty Space threads from shared listing (S1) but the orphan remains.
- D1-105 P2 gap: create path appends the first post decision-less (owner bootstrap); it relies on session-core letting a decision-less first user row through only when message_count == 0, so a replay with changed text hits already_exists, not a fresh-decision denial.
- D1-106 P2 gap: no rate limit or per-room flood limit on the two routes beyond the 32768-byte cap and the fleet limiter (F-t1-043).
- D1-107 P3 polish: `append_post_message` unpacks the context with three `.map(..).unwrap_or_default()` tuples; a small `ThreadSpaceContext::empty()` would read better.
- D1-108 P3 follow-up: `space_routes/mod.rs` header comment still names only artifacts.rs as the first feature file.

### WS01-S5
- D1-111 P2 decision: one send id (space-post-<client_send_id>) is the session key, the Control idempotency key and the stored client_message_id; it avoids model-gateway's leading-alphanumeric rule rejecting ids that start with _ or -, and the SPA must never reuse a send id across Spaces.
- D1-112 P2 decision: content is trimmed at the BFF (design D7) and the Control digest and the model-gateway body use the trimmed bytes; model-gateway (S4) forwards untrimmed, so any other caller must digest exactly what it sends.
- D1-113 P2 gap: space_scope_for_thread(thread_id) shape is UNVERIFIED (plan names it only): an owner-bound by-id read of model-gateway's thread list returning Space(ref) / NoSpace / NotVisible; a member cannot see another member's room post through it, so mp-WS18 must confirm it fits (it may need a Control membership read).
- D1-114 P2 gap: Control 404 on thread-append-decision always maps to space_not_found; thread_not_found only comes from Session Core after the mint (Control does not check thread-in-Space).
- D1-115 P2 gap: a Control 403 carries details.reason only when SPACE_DENIALS_TYPED is on and Control sent a well-formed denial; otherwise a bare space_post_not_authorized (WS03 fills the reason).
- D1-116 P2 risk: features.human_posts reflects only the BFF flag; it does not prove model-gateway's SPACE_HUMAN_POSTS_API or session-core's SPACE_HUMAN_POSTS_CORE is on (mismatch shows as 404 space_human_posts_disabled or 403 space_post_not_authorized), so the three flags must be flipped together.
- D1-117 P3 gap: on a lost reply to a create, model-gateway answers 502 post_not_stored with details.thread_id; the BFF passes it through and a retry with the same send id lands on the same thread through the shared session key.
- D1-118 P3 follow-up: the agent-action-admission golden and vendored copies need regenerating for the two new none rows (test phase, cross-plane).
- D1-119 P3 follow-up: features and limits are on GET /spaces/{ref}/context only; spaces-client types and the SPA gates are WS01-S6.

### WS01-S6
- D1-122 P2 decision: Agent path (`agent_turn`) is NOT journaled; its stages stay in the composer's exchange block (WS01-S8) to avoid two progress surfaces. The journal kind exists in the vocabulary only.
- D1-123 P2 decision: One unsettled post per composer (busy blocks a second send); a failed post is re-sent with the same sendId only when the trimmed text and target are unchanged, otherwise the stale entry is dropped and a fresh id is minted (the server refuses one key with two bodies).
- D1-124 P2 gap: Retry and Discard live in the composer only; the optimistic row in the timeline states "Ikke sendt, prøv igjen" but has no buttons. The pane composer (S7b) must pass the same journal and sender surface.
- D1-125 P2 decision: A human post is not blocked by a thread awaiting approval (it starts no run); only the Ask path keeps that gate. Revisit if the room wants posts to wait too.
- D1-126 P2 gap: Posting is gated in the UI by `canPostInSpace(role)` (editor/manager/owner); an unknown or missing role fails closed to a disabled box. Org-wide rooms with derived rosters (viewer by role mapping) rely on the server refusal when the SPA role is stale.
- D1-127 P2 risk: A typed ZDR refusal switches Send off for the composer's lifetime (`zdrRefused`); there is no proactive org ZDR signal in the SPA, so the first send finds out. A `features`-style ZDR hint on the context would avoid the failed first attempt.
- D1-128 P3 polish: Mode switch and counter styles are in `space-send.css` (not global.css); dark-mode only restated for the failed-state colour. Norwegian copy is first-pass and unreviewed.
- D1-129 P3 polish: `solid/reactivity` lint warnings in SpaceRoomComposer (`wanted`, `props.journal`) are heuristic false positives on one-time reads inside functions; left as is.
- D1-130 P2 test-gap: Existing SpaceRoomComposer, SpaceRoomTimeline, SpacePage and registry tests were not run; the registry now lists two modes, so any test asserting a single mode must be updated in the test phase.
- D1-131 P3 follow-up: The connectivity banner flag (build-time constant from WS01-S8) and `features.human_posts` (context) are read from different sources; WS01-S5 context `features.connectivity_banner`/`send_idempotency` are still unused by the SPA.

### WS01-S7a
- D1-133 P2 deviation: DEVIATION: the pane is an absolutely positioned child of `.verevon-space-workroom` that reserves its width as right padding, not a grid column, because global.css has three conflicting grid definitions of the workroom; below 760px it covers the room.
- D1-134 P2 decision: `SpaceRestrictedNotice.adminCta` is `{label, href}` (plan names the prop but not its shape); WS07-S4 may change it.
- D1-135 P2 risk: the pane flag is a build-time constant (`VITE_SPACE_THREAD_PANE_UI`), so a production kill needs an SPA rebuild until WS01-S5 delivers `features.thread_pane`.
- D1-136 P2 gap: `SpaceTurnList` has no row slots yet (`run_card`, `work_summary` for WS09-S5/S6); add them when the first consumer lands.
- D1-137 P2 risk: the timeline re-renders posts on every 6 s poll (thread objects change), so the "Open thread" opener button can be disconnected before Escape and focus return then has no target.
- D1-138 P2 gap: the pane transcript refetches whenever the thread's `updated_at` changes, with no abort of an in-flight read; fine at 6 s polling, revisit with WS02 realtime.
- D1-139 P3 gap: the sidebar thread link now carries `?thread_id`, but the sidebar `spaceHref` for the Space row is unchanged; no `/chat?thread_id=` literal was added.
- D1-140 P3 gap: UNVERIFIED in a browser: Solid router `setSearchParams` keeping `#chat` and the 760px layout were not exercised (no dev server run in this slice; verification is the end-of-wave test phase).
- D1-141 P3 gap: only the first 30 turns of a post render inline in the timeline (existing cap); the pane shows up to 500.

### WS01-S7b
- D1-144 P2 decision: With `features.human_posts` on and the pane available, the per-post Reply button opens the pane (design D8) instead of pointing the room composer at the thread; the room composer's reply-target path stays for pane-off or flag-off.
- D1-145 P2 gap: The pane composer is shown for every non-missing state including `restricted` (a member may post without shared read); the optimistic reply row is drawn only in the timeline, not inside the pane's turn list, so after a send the pane shows nothing until the next poll.
- D1-146 P2 risk: `unreadThreadIds` (SPA and Convex pure function) now falls back to the owner rule only when `last_author_subject_id` is absent; a listing that carries an empty string is treated as absent. Convex copy is exercised only by tests; no handler passes threads to it yet.
- D1-147 P2 follow-up: sidebar unread dots (`unreadThreadIdsIn`) read the page's computed set, so they follow the new rule only after the page publishes; sidebar link builder was already done in WS01-S7a, nothing added here.
- D1-148 P3 polish: `.verevon-room-post__collapsed` and the pane composer footer styles live in `space-aux-pane.css` (loaded globally because SpacePage imports the pane statically); sticky footer offsets assume the pane body's 20px bottom padding.
- D1-149 P3 test-gap: no DOM test covers the sticky composer footer on the phone-width sheet (virtual keyboard overlap).

### WS01-S8
- D1-152 P2 risk: a dropped stream resumed after the run finished relies on the gateway resume buffer (`/api/v1/chat/stream/resume/{requestId}`); a 404 there currently ends as connection_error after 3 tries, shown as retryable.
- D1-153 P2 gap: `managed_run_already_started` is accepted without observing the original stream; the run shows only through the room poll/event stream, so the sender sees no live text for that send.
- D1-154 P2 risk: retry of a transport failure that happened before `connected` reuses the same key; if the first request actually started a run the retry is answered `managed_run_already_started` (accepted), which is the intended outcome.
- D1-155 P2 gap: browser offline state does not pause or queue sends; the banner only warns (offline queue-and-send-on-online was in the design's test list, not the S8 row).
- D1-156 P3 follow-up: sendId is in-memory only (no journal); WS01-S6's space-send-journal should adopt newSpaceSendId so a post and an Ask share one id shape.
- D1-157 P3 gap: the banner is mounted in the composer, not SpacePage, to avoid touching that hot file.

### WS02-S1
- D1-160 P2 decision: seq is monotonic per Space and never reset on a generation bump (bump sets min_seq = head_seq+1); contiguity is per generation from the connect head, which S3 must honour.
- D1-161 P2 decision: run_state has an SQL (space_event_run_state, used by triggers) and a Rust mirror (events::run_state); a db test must assert agreement, else C17's enum change edits two places.
- D1-162 P2 gap: unarchive (archived_at NOT NULL to NULL) and thread persona/pin changes by unarchive emit nothing; the registry has no kind for it.
- D1-163 P2 risk: LISTEN holds one of the 20 main-pool connections (PgListener::connect_with(&pool)); the dedicated SPACE_STREAM_DB_POOL is S3, and a pooler in front of Postgres (UNVERIFIED in deployed topology) would break LISTEN.
- D1-164 P2 risk: sync_flag is last-writer-wins across replicas; replicas booted with different SPACE_REALTIME_EMIT values flip the flag and each off-to-on bumps generation.
- D1-165 P2 gap: purge_space (delete_space_threads) deletes the whole Space's log although the RPC only deletes the owner's threads; other members' threads that remain have no history rows and their later deletion is silent only until rows resume.
- D1-166 P2 gap: space_event_heads rows are never garbage collected; PurgeSummary does not count the log rows (erasure proof counts content tables only).
- D1-167 P3 gap: the gdpr FK-order test (purge_statements_are_parameterized_and_correctly_ordered) should also assert suppress precedes the first thread/message delete.
- D1-168 P3 follow-up: CI step requires >= 8 space_event tests; red until the wave-end test phase writes them. space_event_forget_subject is shipped uncalled (no user-erasure path yet).
- D1-169 P3 deviation: DEVIATION: file is src/space/events.rs (P-SC1's module) not src/space_events.rs; mod line is in space/mod.rs, so main.rs gains no mod line; messages trigger WHEN also limits roles to user/assistant/system.

### WS02-S2
- D1-170 P2 gap: Cross-audience refusal (stream token refused by ListThreads, read token refused by the stream) is only enforceable in the Session Core verifier (WS02-S3); S2 alone is inert and proves nothing about it.
- D1-171 P2 test-gap: The three plan tests named in the file list live in `shared_thread_read_decision_test.go` and are owed (see unit tests_owed); the existing near-miss audience test at ~:186 could also gain `model-plane-space-stream-x`.
- D1-172 P2 risk: UNVERIFIED whether the own-scheduled personal-Space fallback (IssueOwnScheduledThreadReadDecision) checks `thread_read_entitled`; it also calls Validate so it will now sign a stream-audience decision for a personal Space owner. Personal Spaces are in scope per SD-25, but the entitlement check on that path was not read.
- D1-173 P3 follow-up: Older Control (404 or Validate error on the unknown audience) is what an older user-core returns for the stream audience; WS02-S4 must map it to space_stream_forbidden/unavailable and deploy order must put user-core before gateway.

### WS02-S3
- D1-176 P2 risk: every stream task subscribes to the one global broadcast of wakes and filters by Space, so a wake costs O(open streams); fine at the 5000 cap, the per-Space tailer is WS02-S6.
- D1-177 P2 decision: the dedicated stream pool reads DATABASE_URL with the same default as store.rs (duplicated literal), and falls back to the main pool if it cannot be configured.
- D1-178 P2 decision: a decision within 10 s of expiry gets a 1 s lease (closes lease_expired, client re-mints) rather than a refusal.
- D1-179 P2 risk: a seq hole in the rows (or a generation change mid-stream) answers cursor_too_old(retention|generation_changed); a stalled log with a gap would make clients resnapshot repeatedly.
- D1-180 P2 gap: mid-stream upstream failure is relayed as an `error` event by model-gateway; the BFF (S4) must map it to closing upstream_unavailable.
- D1-181 P2 gap: message_sequence is proto3 int64 where 0 means absent; if message sequences can be 0 the SSE omits the key.
- D1-182 P3 follow-up: the old run_state allow(dead_code) in space/events.rs could be dropped now; its db parity test is still owed (S1).
- D1-183 P3 follow-up: UNVERIFIED that LISTEN works behind any pooler in a deployed stack (plan open question); the 30 s floor poll covers it.

### WS02-S4
- D1-186 P2 gap: no BFF-side seq contiguity or generation consistency check; it relies on Session Core and the SPA controller (plan puts the hole check in the client).
- D1-187 P2 gap: no maximum stream lifetime in the relay; it trusts upstream lease_expired closing. A stuck upstream is bounded only by client disconnect (heartbeats are not watched at the BFF).
- D1-188 P2 gap: no BFF metrics (opened/closed by reason, frames by kind, invalid_frame count); only plan metrics are Session Core's.
- D1-189 P2 decision: requests with the x-zdr header also get poll_only, beyond the plan (org posture only); harmless, fails closed.
- D1-190 P2 gap: upstream error event message is forwarded after a 256-byte cap and closed-code check; consider replacing with fixed words.
- D1-191 P3 follow-up: the Control decision's expires_at is not read; lease length is Session Core's decision, so the BFF does not cap reconnect cadence by it.
- D1-192 P3 follow-up: refs value checks are structural (non-empty string <=128); message.appended / thread.* required refs (thread_id) are not enforced as mandatory.
- D1-193 P3 decision: limiter and slot counters are per replica, in-process slots; the 30 rpm bucket uses the shared Dragonfly backend with local fallback.
- D1-194 P3 deviation: plan names domains/spaces/stream.rs and a local SpaceStreamConfig Extension; implemented as StreamRuntime Extension (config plus limiter plus slots) because module_routes() takes no state.

### WS02-S5
- D1-196 P2 deviation: Plan says "three failures within 10 s go to degraded"; built as 3 consecutive failures (reset on first heartbeat/event) plus an immediate degraded when a live stream dies, so the DoD "degraded within 35 s of stopping session-core" holds with full-jitter backoff.
- D1-197 P2 deviation: Presence effect is keyed on a confirmed room context (`context()` loaded, no error), not on the threads listing; threads refetch failure (fetcher returns undefined) would otherwise have fired offline/online churn.
- D1-198 P2 risk: Stream starts as soon as context() is non-undefined for the ref; on a ref change the previous room's context may still be held for one render, so the new room's stream can start before its membership resolves (the gateway re-verifies membership itself, so no authority gap, only a wasted mint).
- D1-199 P2 gap: Frame `v` and `space_id` are not checked client-side (BFF validator already drops foreign space ids); a future v2 frame would be processed as v1.
- D1-200 P2 gap: No frame-count or burst cap in the client; a herd is bounded only by the 1.5 s refetch floor. WS15 windows / S6 shared tailer are the later answer.
- D1-201 P3 follow-up: Two tabs each hold one stream (HTTP/1.1 six-connection limit, X35); leader-tab election is WS02-S6 (conditional).
- D1-202 P3 follow-up: Indicator copy is inline i18n.tr pairs; error-copy entries added for rate_limited/space_stream_* are not yet surfaced anywhere (controller degrades silently by design).
- D1-203 P3 follow-up: `rate_limited` is a generic key and may collide with a later workstream's banner; sorted within WS02 banner only.
- D1-204 P2 test-gap: space-events.json vendored copy has a sha256 copied from Model Plane but no CI row ties the two (add to zdr-contract-sync GOLDENS/CONSUMERS in the test phase).

### WS03-S1
- D1-206 P2 decision: UpsertEffectPolicy(ctx, policy, actor) keeps full-row semantics for Go callers; the operator handler uses UpsertEffectPolicyPartial with the set of stated keys (absent or null keeps the stored value, new row defaults false).
- D1-207 P2 decision: unwired policyAudit outbox fails the write (no permissive default); the live test thread_read_decision_live_test.go only had its call site updated and its Repository does not wire an outbox, so that live test needs a fake enqueuer when tests are written.
- D1-208 P2 decision: creation event (operator_created) stores before_entitlements as {} and the audit "changed" list shows every bit set true as from false; privacy_changed is true.
- D1-209 P2 risk: operator events have no request_digest and no idempotency_key (NULL, excluded from the unique index); a retried operator PUT with identical material is a no-op (no event), a changed one is a new event.
- D1-210 P2 risk: audit payload user_id is "service:control-space-policy" with actor_role "operator"; UNVERIFIED that audit-core's consumer accepts a non-user id in user_id (it only validates org_id and event shape as far as read).
- D1-211 P3 gap: the 033 down migration is not run by the runner (only *.up.sql); the events table is immutable via trigger, so an org erasure cannot delete its rows (plan question 6).
- D1-212 P3 follow-up: the Docker network/service name user-core in the script default is UNVERIFIED against a running compose project; header documents the override.
- D1-213 P3 follow-up: scripts/set-space-effect-policy.sh cannot turn a bit off any more (omitted keeps); there is no --no-<flag>, by design until the S3 console exists.

### WS03-S2
- D1-215 P2 deviation: DEVIATION: `Denial` carries a `Kind` field the plan's struct sketch omits (its detail table names `kind`); `DiagnoseDenial` is in `internal/spaces/denial.go`, not `repository.go`, to keep the hot file untouched.
- D1-216 P2 decision: validators for the six entitlement keys with a combined `ref == "" || !entitled` check were split (entitlement first) keeping the same error string, so a missing entitlement is typed `space_entitlement_off` even when the resource ref is also empty.
- D1-217 P2 decision: thread-read own-scheduled fallback: when the shared resolver finds no row and the personal-owner resolver returns a typed denial, that denial is reported (the personal Space is proven by that query) under the unchanged "current shared Space authority required" message.
- D1-218 P2 gap: service-to-service routes (watch-observe, run-action, scheduled-*, sandbox-capability, import-execution, schedule-fire) keep plain bodies as planned; their validators now return `*Denial` internally, harmless because Error() text is unchanged.
- D1-219 P2 gap: `space_authority_unavailable` is emitted only on routes whose existing failure status is 503; `resolveCurrentSpaceMembership` keeps 500 plain for non-membership failures.
- D1-220 P2 risk: UNVERIFIED that `DiagnoseDenial` SQL matches live schema column names in a real DB (copied from the shared-evidence joins); the live test in tests_owed is what proves it.
- D1-221 P3 test-gap: effect names used in `details.effect` (`import_write`, `schedule_create`, `watch`, `agent_action`, `owner_grant`) and entitlement keys are closed sets but not yet pinned by the golden (it pins detail key names only); a test could tie them to the catalogue.
- D1-222 P3 follow-up: Norwegian/English copy for each code is WS03-S7; the vendored golden copies and VENDORED rows are added by S5/S7.
- D1-223 P3 risk: zdr-contract-sync.test.sh already reports 25 pre-existing failures on this branch (missing rerank goldens, CRLF pin drift); the S2 golden and pin rows pass.

### WS03-S3
- D1-227 P2 deviation: view `effective` treats an enabled zdr_unproven bit as effective under ZDR (Control still issues it); only zdr_inert is ineffective. Added additive field `zdr_blocked` per entitlement for the console; design text says effective = enabled and not ZDR-blocked.
- D1-228 P2 decision: denied-write audit event (space_effect_policy_write_denied) is enqueued only when an owner/admin hits the switched-off write route; role or membership denials do not call the store (plan test: member 403 with store never called), so they leave no audit row.
- D1-229 P2 risk: RecordOrgPolicyWriteDenied is best-effort (a failed enqueue is logged, the 403 still returns); and it uses its own tx via the same enqueuer.
- D1-230 P2 gap: a no-change write (changed:false) records no event, so a replay of its idempotency key later with a different body is not an idempotency conflict.
- D1-231 P2 gap: v2 delegations are not excluded on these routes; a v2 delegation is bound to method, URI and body digest so it cannot be replayed elsewhere, but only v1 is what the gateway sends.
- D1-232 P3 follow-up: history actor name comes from a LEFT JOIN on users (id::text); after GDPR erasure the name is blank and only the user id remains (immutable events keep the id, plan question 6).
- D1-233 P3 test-gap: UNVERIFIED live behaviour (Postgres) of GetOrgPolicy/ListOrgPolicyEvents/ApplyOrgEntitlements SQL, and gin route precedence of org-policy vs :space_ref was only checked via route registration + scope table in a scratch test (not kept).

### WS03-S4
- D1-235 P2 deviation: DEVIATION: the plan names `dispatchers_spaces/ws03.rs` but no `dispatchers_spaces` module existed on this branch; created `dispatchers_spaces.rs` as the per-workstream seam and made `ticket_string`, `ticket_bad_request`, `owner_json_response_to_envelope` in dispatchers.rs `pub(super)` (visibility only) so the child module reuses them.
- D1-236 P2 deviation: DEVIATION: MODEL_ACCESS (model-access.ts) is `Record<ActionId,...>`, so the two ids needed rows to typecheck; both closed as exempt('human_identity_or_security'), the same class as org.update_zdr.
- D1-237 P2 decision: gateway requires expected_entitlement_revision (non-negative int) because Control's PUT does; the plan's shape-check list did not name it.
- D1-238 P2 decision: unknown PUT body fields are refused (400) rather than forwarded, so a stray org_id/role/actor never reaches Control even though Control also uses DisallowUnknownFields.
- D1-239 P2 risk: the action path takes ExecuteActionRequest.idempotency_key verbatim; if the SPA action client sends an empty key the action returns 400 (never auto-generated by design); WS03-S6 must pass a stable key per attempt.
- D1-240 P3 follow-up: events pass-through returns Control's cursor unchanged; the gateway validates it as up to 19 digits (UNVERIFIED that Control's cursor stays a decimal event id if it is later changed to opaque).
- D1-241 P3 follow-up: new Rust files were written LF; git will normalise to CRLF on next touch (worktree convention), no content change.

### WS03-S5
- D1-243 P2 gap: personal-space import and schedule-create callers (`personal_import_ingress_decision`, `inject_personal_schedule_create_context`) now return typed bodies with codes space_import_not_authorized / space_schedule_not_authorized, but the action dispatchers that call them were not audited to confirm they forward the body unchanged to the SPA.
- D1-244 P2 gap: membership denials are typed only on the 8 read handlers (list threads, activity, work, knowledge, etc., sites with `membership_status`); `require_space_agent_grant_role`, `resolve_space_role` and `space_context` (membership failures) still pass through, to be handled by WS04/WS17 or a follow-up.
- D1-245 P2 decision: all mapped membership failures use operation code `space_read_not_authorized`; the plan lists no separate membership operation code, so a distinct code would need a plan decision.
- D1-246 P2 risk: map_control_denial trusts Control's `error` string as the message (truncated to 200 chars); it is Control-owned fixed text, but if a Control path ever echoed user input into it the gateway would relay it.
- D1-247 P3 decision: the flag defaults on in dev via space_flag (APP_ENV unset) and off in production; compose base is `:-true`, production override `"false"`; flag changes only the body shape, never status or the refusal itself.
- D1-248 P3 follow-up: `space_retrieval_decision` wrapper is kept only for the plan's "old signatures stay" rule and is `allow(dead_code)`; remove once no caller or test needs it.
- D1-249 P3 test-gap: thread-read list handler (`list_space_threads`) still uses the Option wrapper, so its decline carries no reason (the request is deliberately left as-is on decline).

### WS03-S6
- D1-250 P2 gap: history shows only the first 20 events; `unwrap` in http.ts drops `meta.next_cursor`, so paging needs a raw-envelope request helper.
- D1-251 P2 decision: the console calls the BFF routes through the client rather than the `spaces.set_effect_*` action ids (the ids exist for the action surface and agents-excluded flows); no UI operation lacks a contract, but a later pass may route saves through the action dispatcher.
- D1-252 P2 risk: the sidebar probe issues one GET /space-policy per admin org on settings-panel mount (one extra Control call, cached by the resource); a shared cached store would avoid the duplicate when the section opens.
- D1-253 P3 gap: styling reuses existing verevon-settings classes only; preset cards, groups and history have no dedicated layout CSS.
- D1-254 P3 gap: SpacePolicyView types are hand-written mirrors of Control's JSON (org_policy.go); no golden or zod guard on the response shape.
- D1-255 P2 risk: draft key rule: a conflict resets the key; an unreachable response after a successful write retries with the same key and gets replayed:true (relies on Control digest equality, UNVERIFIED end to end).
- D1-256 P3 follow-up: Norwegian copy (console and nav label) needs native review before production (plan question 7).
- D1-257 P3 gap: top-level sidebar-navigation.ts settings group and shell-data route union have no space-policy entry (route falls back to '/settings'); only the settings panel lists it.

### WS03-S7
- D1-260 P2 gap: panels and composer pass isAdmin false (no admin signal in those components), so the org_admin CTA is always "ask an administrator"; the /settings/space-policy link only appears where a caller passes isAdmin true (S6 console exists in the same slice family).
- D1-261 P2 gap: space_role_below_floor and space_not_active copy does not name required_roles or state; role/state tokens have no localised vocabulary yet, so they are read but not shown.
- D1-262 P2 decision: a pass-through (flag off) Control body keeps only top-level code, so extractError yields no details; the presenter falls back to code and the table's resolvable_by, losing the entitlement name. Top-level details/resolvable_by are not read.
- D1-263 P2 decision: unknown reason tokens (newer server) show the generic line with the server's resolvable_by CTA, never server text.
- D1-264 P2 follow-up: no CONSUMERS row was added for the golden (a VENDORED row satisfies the sync check); add one if REQUIRE_CONTRACT should force the SPA copy.
- D1-265 P3 follow-up: ChatErrorEvent.details is set only from a failed connection; SSE error frames do not carry it (Model Plane emits no Space details), so frame parsing is unchanged.
- D1-266 P3 risk: shared hunks (SpaceRoomComposer, spaces-client/core.ts) landed in sibling commit 71430a63a; revert of S7 must include them.

### WS04-S1
- D1-269 P2 risk: `listActiveOrganizationRooms` does a full `spaces` table scan and `runRequestedOrganizationRoomSync` filters it by org; add a by-org+org-room index or per-org query if the table grows.
- D1-270 P2 risk: the cron syncs each org room sequentially with one org-core and one Control call per room; per-org batching is a follow-up.
- D1-271 P2 gap: S5 post-sync audience reconcile is not called from `syncOrganizationRoom` yet (S5 adds the call).
- D1-272 P2 test-gap: `requestOrganizationRoomSync` and the runner are untested against a live Convex; only the pure `planOrgRoomSyncRequest` is unit-testable.
- D1-273 P3 decision: `convex-stubs.d.ts` gained `cronJobs: any` because the repo typecheck shadows `convex/server` with stubs.
- D1-274 P3 decision: dev default on is set in the two compose files, not in code; handler reads only `1` or `true` as on.

### WS04-S2
- D1-286 P2 risk: updateRoomDetailsForGateway trusts the gateway's Control role check (owner or manager) as setInstructionsForGateway does; a leaked service key could rename any room of an org the caller belongs to.
- D1-287 P2 gap: the org room (isOrganizationRoom) is renameable by this mutation since it is kind room; plan does not say to block it.
- D1-288 P2 gap: invisible format characters beyond the plan's bidi set (U+200B-200F, U+2028/2029, U+FEFF) are not rejected, and U+0085/U+2028 inner whitespace is collapsed by \s.
- D1-289 P2 risk: the Rust gateway copy of the rules must match this normalizer (NFC, code-point lengths); a shared vector file is not yet created.
- D1-290 P3 deviation: DEVIATION: schema change went into convex/schema/spaces.ts (post P-CX1), not schema.ts; RoomDetailsResult is a flat type because convex-core compiles strict:false (union narrowing fails).
- D1-291 P3 gap: spacesForOrgForGateway topic is returned for every Space in the org; the gateway still intersects with Control's list so no widening.

### WS04-S2.gw
- D1-277 P2 decision: gateway validation is shape and size only (string, at most 2000 chars, at least one field); NFC, trim, 1-120 and 0-250 and control/bidi rules run only in Application's spaceRoomRules.ts and come back as room_details_invalid:<field>:<reason>. A Rust copy of the rules was not built to avoid two drifting copies (plan says "same rules").
- D1-278 P2 gap: archived-room check (plan: "archived state checked once S7 lands") not wired; WS04-S7 gateway leg should refuse PATCH on an archived room (Application already refuses non-active lifecycle with room_not_active).
- D1-279 P2 decision: room_not_active maps to 409 `room_not_active`, not in the plan's four codes (400/403/409 space_kind_not_editable/503); the SPA ERROR_COPY needs an entry for it and for space_not_found.
- D1-280 P2 gap: gateway change of name does not refresh Control's or session-core's cached room labels, and the thread list in other clients; consumers reading the name from Control index remain unchanged (display labels come from Application only).
- D1-281 P3 follow-up: GET /api/v1/spaces falls back to the personal-space projection when Control is down; that path goes through public_space and now also emits topic (null).
- D1-282 P3 follow-up: update_room_details_route returns 422/400 from axum's Json extractor for a non-JSON body with the framework's own body shape, not the typed envelope (same as the sibling member routes).

### WS04-S2.spa
- D1-293 P2 decision: form offered for every active kind=room including the organization room (Application only refuses non-room kinds); plan does not exclude the org room, confirm with owner whether renaming it is intended.
- D1-294 P2 gap: the action is not wired into any UI command palette, only the form calls updateSpaceRoomDetails directly (not through action-client); other Space forms do the same.
- D1-295 P2 decision: name and topic limits (120, 250) are duplicated as input maxlength hints in SpaceRoomSettings.tsx; the authoritative copy stays in Application spaceRoomRules.ts and field-specific copy keys off its reason tokens (name_required, name_too_long, topic_too_long, control_character, bidi_override).
- D1-296 P2 gap: archived-room hiding uses lifecycle==='active' only; WS04-S7 should add the archived state check and its banner.
- D1-297 P3 polish: topic is shown only as one line under the title; no ellipsis or expand control for 250-char topics, no topic in the room list or sidebar.
- D1-298 P3 test-gap: SpacePage.test.tsx fixtures lack space.topic and features.room_details; untested until the test phase.

### WS04-S3
- D1-308 P2 deviation: DEVIATION: scope mapping added via registerSpaceScopeRule in scopes_directory.go (the seam hook runs for all internal paths) instead of editing service_auth.go; route via registerFeatureRoute in routes_directory.go instead of editing server.go routes. server.go gained only one Server field, main.go one wiring line.
- D1-309 P2 decision: Ordering is (prefix-match rank, lower(name), id) per design D2.7 and the cursor carries all three keys, which extends the plan row's "(lower(name), id)" cursor.
- D1-310 P2 risk: ILIKE '%q%' on users.name/email is a seq scan within the org's active members; fine for hundreds of members, add a pg_trgm index if an org grows past a few thousand.
- D1-311 P2 decision: Caller must be an active user_org_memberships member of the delegated org (local projection, same backstop every Space query uses), else 403 member_not_in_organization; it is not re-resolved from auth-core.
- D1-312 P2 decision: exclude_member_of is applied only when the caller is an active member of that Space in the org (checked in SQL), otherwise silently ignored, so it cannot probe other rosters.
- D1-313 P3 gap: No audit event for directory reads (read-only listing of org members, same exposure as list_members); revisit if the owner wants search logging.
- D1-314 P3 follow-up: Email is returned for every match (PD-WS04 default); a per-org hide-email policy is not built.
- D1-315 P3 test-gap: No live-DB test yet for escaping and keyset paging; owed in people_live_test.go.

### WS04-S3.gw
- D1-299 P2 deviation: DEVIATION: the 60/min limiter is a process-wide `static OnceLock<RateLimiter>` in directory.rs, not an `AppState.space_directory_rate_limiter` field, because a new AppState field would force edits to five foreign test state literals (browser.rs, chat/shared.rs, orchestration.rs, stream_e2e.rs, main.rs tests); WS04-S4/S8 (20 join, 30 member ops) will need the same choice.
- D1-300 P3 follow-up: the static limiter is built from the first request's cache; in tests it is shared across cases (keys are per org and user, so only same-user tests interact).
- D1-301 P2 decision: gateway pre-validates q (100 chars), limit (1..=50), cursor (512 bytes), exclude_member_of (200 bytes) so an invalid query never reaches Control; Control stays the authority and re-validates.
- D1-302 P2 decision: Control 5xx or unreachable maps to 503 `directory_unavailable`; Control typed 4xx (403 member_not_in_organization, 400) passes through unchanged.
- D1-303 P3 gap: the rate limiter fails open to an in-process bucket when Dragonfly is down (existing RateLimiter behaviour), so the per-user limit is per-replica in that case.
- D1-304 P3 follow-up: `features.people_search` is in the room context only; GET /api/v1/spaces list does not carry it (the SPA picker lives in the room view).
- D1-305 P2 risk: UNVERIFIED that user_core_delegation_headers signs the query string of a GET URL the way user-core's verifier expects; other gateway GETs with queries to user-core exist but no live run was done.
- D1-306 P3 gap: live deploy not run; the Control credential scope directory:read:self is in compose from the Control leg.

### WS04-S3.spa
- D1-316 P2 decision: The search is a read, so no action contract was added in src/shared/actions (reads such as listOrganizationMembers have none); add one only if the model should ever search people.
- D1-317 P2 gap: rate_limited and active_membership_required copy in errors.ts speaks of room refresh and posting, which reads oddly for a people search; give the picker its own copy.
- D1-318 P2 gap: After an add, the search list is only filtered client-side by the re-read roster; the server exclusion applies on the next search, so a long-open picker may show a just-added person until the roster refetch lands.
- D1-319 P3 gap: No minimum query length: an empty box browses the directory from the top (Control ranks it); consider a hint or a minimum if directories get large.
- D1-320 P3 follow-up: SpaceMemberControls still carries the legacy org-list path for flag-off; remove it once SPACE_PEOPLE_SEARCH_API is on everywhere.
- D1-321 P3 gap: The input has no explicit aria-live result count; screen readers get only the loading status line.

### WS04-S4
- D1-325 P2 decision: an add on an already-active application_sync row returns changed=false and does not adopt the row, so the legacy Convex sync can still deactivate it until the next role change; adoption on add may be wanted at cutover.
- D1-326 P2 decision: member_count counts active user rows (not services, not filtered by org status); the roster may show a different number.
- D1-327 P2 risk: a replay of an add/remove/role change returns changed=false, replayed=true and the current state, not the original revisions; a client comparing revisions across a lost response sees newer values.
- D1-328 P2 gap: the idempotency fingerprint includes actor_id, so after GDPR pseudonymisation of that actor a replay with the same key returns idempotency_key_reused.
- D1-329 P2 decision: PUT .../memberships now publishes membership_changed for brand-new user rows too (active=true), where before only reactivation published; consumers already handle active=true (clears a revocation) but Session Core was not re-checked (UNVERIFIED).
- D1-330 P2 gap: ledger rows for the declarative replace use actor_id application-space-lifecycle for every caller; the real principal id is not threaded into ReplaceMemberships.
- D1-331 P3 follow-up: published_at, publish_attempts and next_publish_at are created but unused until WS04-S10; events are fire-and-forget after commit as before.
- D1-332 P3 gap: role matrix golden file testdata/room-role-matrix.v1.json and the SPA mirror are not written (test phase and WS04-S6).

### WS04-S5
- D1-336 P2 gap: reconcileAllRoomAudiences runs every active shared Space serially in one action; with many rooms it can exceed the action time limit; per-org batching/fan-out via scheduler is follow-up.
- D1-337 P2 gap: reconcileRoomAudienceForGateway checks the Space belongs to the caller's org but not the caller's membership (same as syncRoomMembersForGateway); the gateway's Control check is the gate.
- D1-338 P2 decision: viewers are included in the audience (every active human member that is an active org member), matching Control's candidate read; 512 cap enforced by plan, Control's maxSpaceMembers is 5000.
- D1-339 P2 follow-up: module-level env capture in spaceAudienceRegistration.ts means the new env vars take effect only after a functions push; the reconcile reads its own env at handler time.
- D1-340 P3 follow-up: personal Space deletion/space deletion paths do not stop the cron; skipped by lifecycle != active check only.
- D1-341 P3 follow-up: _generated/api.d.ts was edited by hand (two modules); regenerate with convex codegen on the next stack restart.

### WS04-S6
- D1-345 P2 decision: the SPA learns the writer flag from `data.writer_api` added to the roster response (additive); the plan's `features` booleans on `GET /spaces` and `.../context` were not built, so other flags still have no SPA signal.
- D1-346 P2 gap: with the writer flag off, `spaces.add_member` with a `role` silently adds with the legacy default (role ignored), and `spaces.leave_room` answers 404 `feature_disabled`; the legacy path has no role concept.
- D1-347 P2 gap: `agent-action-admission.json` has no rows for `spaces.change_member_role` and `spaces.leave_room` (WS03-S4 did the same for its ids); model-access.ts rows exist, so a test that cross-checks the JSON against the map may flag both.
- D1-348 P2 risk: `activeOwners` in the SPA counts roster user rows with role owner; Control also requires them to be active org members, so the SPA may offer demoting the last real owner and Control then answers `room_sole_owner` (typed copy exists).
- D1-349 P2 risk: the PATCH sends `expected_revision` = the roster row `revision`; if Control's row revision and the roster `revision` ever diverge in meaning every role change would answer `room_member_changed` (UNVERIFIED against a live roster).
- D1-350 P3 polish: the Members tab does not poll `recipient_ready` for an added member; the pending notice is a static sentence from `audience.state`, and the 2 s poll up to 30 s from the plan is not built (belongs with the joining user's view in S8).
- D1-351 P3 polish: `errors.ts` WS04 keys are not strictly sorted (`invalid_request`, `feature_disabled` first); `SpaceMemberRoleSelect` uses the shared `verevon-space-agent__control` class, no new CSS.

### WS04-S7
- D1-355 P2 decision: archive and unarchive advance authority_revision only (not membership or audience), so signed decisions issued before the change go stale; no NATS membership event is published.
- D1-356 P2 gap: SD-14 (archive as Control read-class state, amending ADR-0001) is assumed at the recommended default; UNVERIFIED that the owner approved it.
- D1-357 P2 deviation: DEVIATION: ResolveCurrentUserMembership/ResolveReadableUserMembership share resolveUserMembership with registration_state = ANY($4), so the scan test must pin callers, not a literal; membership JSON gains additive state and archived_at.
- D1-358 P2 risk: fenceActiveSpaces widened to archived rooms, so an effect-policy change bumps their authority_revision too (needed so unarchive does not resurrect stale decisions); org_policy.go:283 active-room count and directory/people.go stay active-only.
- D1-359 P2 gap: ResolveAgentActionDecisionEvidence, DiagnoseDenial (space_not_active for archived on other write routes) and scheduled/import/sandbox routes keep the generic 403 space_not_active, not 409 room_archived; only thread create and append are typed per the plan.
- D1-360 P3 gap: Register keeps archived_at/archived_by when a later lifecycle event moves an archived room to suspended; unarchive then answers room_state_conflict until it is active again.

### WS05-S1
- D1-363 P2 deviation: DEVIATION: plan lists a new `.github/workflows/audit-core.yml`; `audit-core-ci.yml` already exists on this branch, so no workflow was added.
- D1-364 P2 deviation: DEVIATION: report mode logs would-deny but the plan's "max 3 days" cap is not enforced in code (no clock source for the flag); it is an operational rule.
- D1-365 P2 gap: SPA `RecentSecurityEvents` uses `hasWorkspaceAdminAccess`, which also admits a platform admin/superadmin who is only a plain org member; the gateway and audit-core still scope that user to own rows, so the panel shows own rows without the "own events" note.
- D1-366 P2 gap: redacted projection omits `org_id`; the Trust Center per-connection attribution (`eventSource`, which reads `details.source`/resource) gets nothing for members, only the workspace rollup works (whitelist is tool, data_category, zdr, phase).
- D1-367 P2 gap: the dead "Open audit log" button in the settings panel still has no target (S4).
- D1-368 P3 follow-up: usage routes are admin-only in audit-core; no SPA consumer was found, but a gateway `/api/v1/usage` route was not checked here.
- D1-369 P3 follow-up: remove AUDIT_READ_ROLE_ENFORCEMENT after two releases per plan; `error_with_details` in the gateway envelope still carries its dead_code allow although audit.rs now uses it.

### WS05-S2
- D1-371 P2 follow-up: docs/compliance/evidence/tools/scan-egress.mjs still mentions SCIM_BEARER_TOKEN; update when the evidence tool is next touched.
- D1-372 P2 gap: other readers of provider Status (gateway/SPA integration catalog) were not audited for the new `unavailable` value for the scim provider (UNVERIFIED they render it sensibly).
- D1-373 P2 decision: scimError now emits status as a string per RFC 7644 and omits scimType for the 501; the 501 handler does no auth, so unauthenticated callers learn SCIM is unavailable (intended).
- D1-374 P2 gap: the migration redacts only payload; integration_webhook_events.organization_id and event_type (which carries the path) stay; also scim.token.* audit metadata holds token prefixes (not secrets).
- D1-375 P3 follow-up: README endpoint list and docs/COMPLETION_PHASES.md Phase 8 now describe SCIM as moved to org-core; the plan's WS05-S8 must update them again when real SCIM ships.

### WS05-S3
- D1-377 P2 gap: Other auth.ts logs still print user emails (password reset, verification, ADMIN_DEFAULT_EMAILS auto-assign at the user.create hook); plan only scoped the SSO provisioning log.
- D1-378 P2 risk: ADMIN_DEFAULT_EMAILS hook still auto-assigns admin by email string; unverified whether it applies to SSO-provisioned users and whether the email is IdP-proven.
- D1-379 P2 decision: resolveSsoDefaultRole throws for any non-member value (not only admin) and runs only when SSO_ENABLED=true, so a bad value with SSO off does not block boot.
- D1-380 P2 deviation: DEVIATION: auth-core could not be type-checked (node_modules absent in the worktree); the edit is a pure deletion plus a helper returning the literal 'member'.
- D1-381 P2 deviation: DEVIATION: no scopes_spaces.go seam exists on this branch; the scope table is still the switch in user-core service_auth.go.
- D1-382 P3 test-gap: UNVERIFIED that the service principals calling retrieval-decision (gateway) hold the spaces:issue scope in every compose file; the compose principal list was not audited per environment, a missing scope would now yield 403 instead of the default scope.

### WS06-S1
- D1-383 P2 deviation: DEVIATION: the tree has 14 untyped sites, not 11; also typed grpc.rs scheduled-result gate (feature_not_supported scheduled_step), memory_grpc.rs memory-controls refusal and message_search.rs recall refusal (DURABLE_MUTATION_FORBIDDEN, PermissionDenied kept) so the inventory scan stays green.
- D1-384 P2 decision: terminalization.rs ZDR managed-run-with-thread refusal typed as feature_not_supported("agent_run") per plan; the plan's ":1586" site is now at :1663 (zero-retention run has no durable assistant message) and is DURABLE_MUTATION_FORBIDDEN.
- D1-385 P2 decision: message_search recall (a read) uses DURABLE_MUTATION_FORBIDDEN like memory preflight (WS06-D5); no vocabulary feature fits, a new feature key would change goldens.
- D1-386 P2 gap: wire text for these refusals changes to the catalogue English fallback; any client or log matching the old sentences breaks (only tests found in repo).
- D1-387 P3 test-gap: model-gateway session_flow.rs:2487 and e2e_invoke_chain_test.rs:9874 still build untyped statuses for ZDR; session-core db test at grpc.rs:~18880 now asserts the catalogue message.
- D1-388 P3 gap: session_core_rules_double.rs is cfg(test), so its edits were not compile-checked.
- D1-389 P2 follow-up: other planes' Go/TS callers that inspect session-core status text for these refusals were not audited (grep of the Model Plane Go tree found none).

### WS06-S2
- D1-391 P2 gap: zdr-posture-matrix.json `policy.space_zdr` stays `not_applied_in_v1` plus vendored copies and pins; must change with the enforce flip (strict-loader acceptance UNVERIFIED).
- D1-392 P2 risk: mode is read from env on every verify call (cheap but not cached); a startup-parsed value on AppState would be consistent with the gateway rule against std::env in handlers.
- D1-393 P2 gap: model-gateway/SPA do not show the ZDR copy for the new typed refusal on Space writes; wording needs counsel (SD-12).
- D1-394 P3 follow-up: the third-party-processing flag is still decoded and unused (kept as in the original).

### WS06-S3
- D1-395 P2 deviation: DEVIATION: METRIC_LABELS also carries check, code, decision, kind, mode, op, phase, point, source, surface, to, tool (found by scanning the tree) beyond the design's 14 + route; each is an enum or bucketed value, but the value sets were not individually audited.
- D1-396 P2 risk: UNVERIFIED that axum 0.7 exposes MatchedPath to the Router::layer middleware at http_routes.rs (layer wraps each route so it should); if the owed route test shows `unmatched` everywhere, move the middleware to route_layer on the sub-routers.
- D1-397 P2 follow-up: dashboards or alerts keyed on the old `path` label of gateway_requests_total / gateway_request_duration_seconds must switch to `route`; none were found in apps/Model Plane, other repos unchecked.
- D1-398 P2 gap: the guard checks keys only, not values; a key on the list fed an unbounded value (e.g. `tool` without the mcp__ bucketing) is not caught.
- D1-399 P3 gap: scanner is textual (no macro expansion), so labels passed via a pre-built slice/`with_labels` style are not seen; macros inside string literals or raw strings may confuse paren matching.
- D1-400 P3 follow-up: the Go sandbox-manager attribute guard (labels_test.go) was not examined for existing attribute keys.

### WS06-S4
- D1-404 P2 deviation: DEVIATION: no CONSUMERS/VENDORED rows added (plan lists a CONSUMERS row): under REQUIRE_CONTRACT a CONSUMERS row without a vendored copy fails CI; add both with the lint tests that read the copy.
- D1-405 P2 deviation: DEVIATION: .github/workflows/contract-goldens.yml untouched; its existing path filter "apps/Model Plane/api/contracts/**" already covers the new golden.
- D1-406 P2 deviation: DEVIATION: GW/domains/spaces/mod.rs comment pointing at the table not written (file owned by the gateway builder; the module moved to domains/spaces/mod.rs after P-GW1); the table itself is the golden.
- D1-407 P2 deviation: DEVIATION: the BFF route-class set gains `Config` (plan lists ReadMetadata, ModelRead, DurableWrite, Chat) so K4-14 configuration writes are not forced to refuse; Chat has no rows yet.
- D1-408 P2 risk: zdr_posture.rs read_sources() reads only top-level src/*.rs, so the owed every_non_get_route_is_classified must also walk src/space_routes/*.rs (empty today) and space_sse.rs; routes in browser_run.rs and browser_grants.rs are non-GET and already in the registry.
- D1-409 P2 gap: The golden is authored, not generated (like the posture matrix); there is no strict loader for it yet, only the pin; a schema test (ids unique, zdr_class in vocabulary, DurableWrite implies refuse) is owed.
- D1-410 P3 decision: Model-gateway inference routes (/v1/ai/*, /v1/invoke*) are class n/a with basis "posture-decided": they send content to a processor under a ZDR-aware route and store nothing; /v1/browser/grants is refs_only.
- D1-411 P3 follow-up: event subject, index, cache key family, notification type and preview rows are not populated; WS01/WS04/WS09/WS10/WS11 add them in their own PRs (kinds list is reserved in the file).

### WS06-S5
- D1-412 P2 gap: the old egress test in space_capability_decision_test.go (~line 61) is left pinning the open behaviour and will fail until the test phase flips it (tests are end-of-wave by rule).
- D1-413 P2 risk: the `if intent.Egress != EgressDisabledByDefault` space:egress branch is now unreachable; kept deliberately so a future tier added to allowedEgressClaims cannot be signed without the permission, but a tier table (tier -> permissions) would be cleaner when WS06-S11 lands.
- D1-414 P2 follow-up: sandbox-manager's capability_verifier.go and execution-core (sandbox.rs, capability_client.rs) still spell "disabled_by_default" as free literals; no shared constant or cross-plane parity test exists.
- D1-415 P3 decision: ZDR withholds space:processes but still issues the decision (lease granted, process RPCs refused by sandbox-manager), matching the existing unentitled-Space pattern; WS03 should flip process_registry to inert-under-ZDR now that S5 is built (S6 is also required by the plan).
- D1-416 P3 gap: no audit/telemetry record when a refused egress claim is rejected; the refusal is a plain error string, not a typed Control denial.

### WS06-S7
- D1-419 P2 risk: after an output-cap kill the pipes of orphaned grandchildren are not drained or awaited; only the direct child (bwrap) is killed and reaped.
- D1-420 P2 decision: the permit is taken in spawn_capture before the timeout starts, so queue wait does not eat the execution timeout; worst case a step waits 5 s extra.
- D1-421 P2 gap: mem_limit, cpus, pids_limit values are plan starting values, uncalibrated; docker compose config was not run (engine avoided), YAML hand-checked only.
- D1-422 P2 gap: docker-compose.release.yml and docker-compose.override.yml execution-core blocks were not given limits (the base file is merged into them; unverified for release overlay).
- D1-423 P3 gap: Semaphore settings are read once per process (OnceLock); a changed env needs a restart.
- D1-424 P3 follow-up: sandbox-backend seam (code_interpreter) should adopt acquire_spawn_permit as SpawnAdmission.

### WS07-S1
- D1-426 P2 risk: UNVERIFIED that Solid 2.0.0-rc.0 `Errored` catches a rejected `lazy()` import with no outer `Loading` boundary (App has none); the owed spike test proves or disproves it.
- D1-427 P2 deviation: DEVIATION: chunk recovery already exists (src/app/chunk-recovery.ts, WS09-S5, 30 s window, notice) while the design puts it at shared/build/chunk-recovery.ts with a 60 s guard; WS07-S2 should reuse the existing module instead of adding a second `vite:preloadError` listener.
- D1-428 P2 decision: build id for Copy details is read with `typeof __VEREVON_BUILD_ID__` in classify-client-error.ts; WS07-S2 should point it at build-info.ts once that exists.
- D1-429 P3 follow-up: root boundary renders outside I18nProvider so copy is Norwegian only; reading localStorage locale would fix that.
- D1-430 P3 follow-up: `console.error` of the caught error in RegionBoundary is local only; no telemetry call is wired (diagnostics route is WS07-S3).
- D1-431 P3 follow-up: the "Sign in" button uses a full page navigation to /login; no return-to path is kept.

### WS07-S10
- D1-432 P2 decision: Room HTML consent is keyed on id + version + content hash, not on artifact id alone, because ids are only unique inside a thread (two threads can both emit "report") and a new revision can change the markup.
- D1-433 P2 decision: Extra renderers registered through `extraArtifactRenderers` (sibling b) are applied on the chat surface only; they carry no `surfaces`/`trust`, so the room never uses them until they are folded into `viewer-registry.ts` (X26).
- D1-434 P2 gap: `registerViewer` is in place but `load` is typed loosely (`Promise<(props)=>unknown>`) and no caller uses it yet; WS14 should tighten it when it registers DOCX/XLSX viewers.
- D1-435 P3 gap: Refusal card and gate copy are inline Norwegian/English strings, not in the i18n catalogue.
- D1-436 P3 gap: Header actions (diff toggle, copy, export) are not hidden for a kind the room refuses; the body is gated, and the room mapper (WS07-S11) never delivers pdf/image/binary kinds.
- D1-437 P2 risk: Residual self-navigation of the sandboxed srcdoc frame after the click is accepted by plan (WS07-D1); not mitigated here.
- D1-438 P3 follow-up: ArtifactsPanel empty-state copy still names "Verevon" for the room surface; S11 may want a room-specific subtitle.

### WS07-S11
- D1-440 P2 deviation: DEVIATION: roomItems lives in room-artifact-items.ts, not room-artifacts.ts, so spaces-client (which imports the mapper) does not pull chat-media-markdown into the API layer.
- D1-441 P2 risk: the sheet reorders items so the pressed artifact is last (ArtifactsPanel selects its last item and has no initial-selection prop); list order in the sheet differs from timeline order. A prop on ArtifactsPanel (chat group) would be cleaner.
- D1-442 P2 gap: the sheet has no focus trap (focus goes to the dialog on open and back to the opener on close; Tab can leave it) and the background is not inert.
- D1-443 P2 gap: transcript turns carry no created_at, so ArtifactViewer shows no revision time in the room; needs a created_at on the model-gateway turn.
- D1-444 P2 risk: live artifacts from onArtifact are keyed on the thread id of the stream's connected frame and shown as chips only after the thread list lists that thread; for a brand-new thread the chip appears with the refetch, not during streaming.
- D1-445 P3 gap: only the room timeline shows chips; the side-pane thread panel and ZDR/restricted copy for artifacts belong to S12 and are not built.
- D1-446 P2 test-gap: pendingLive dedupes on id+version only; if a revision's content changed under the same version (server rewrite) the persisted copy wins silently.

### WS07-S12
- D1-449 P2 decision: the tab's visibility is the SPA build-time flag (VITE_SPACE_ROOM_ARTIFACTS_UI via roomArtifactsEnabled), a stand-in for `features.room_artifacts` from the BFF; swap when S9's gateway half delivers the flag.
- D1-450 P2 decision: any 403 on the aggregate maps to the restricted notice (plan: 403 or shared_read.available=false), including `active_membership_required`, which is really a membership problem, not a shared-read one.
- D1-451 P2 decision: the tab re-reads the aggregate on every re-entry and clears its transcript cache then; live (sender-stream) entries are dropped only when the aggregate carries the same id with version >= theirs, so a lagging aggregate can briefly show a live entry beside it.
- D1-452 P2 risk: UNVERIFIED that Solid 2's createContext provider wrapping a child component reaches panels constructed inside the child's createMemo (the owner chain should carry it); the SpaceCockpit test must prove a lazy panel sees the active tab.
- D1-453 P2 gap: no `spaces.*` read action entries exist in the action registry for the room reads (Work, Knowledge are the same); the Artifacts list is a read, so none was added.
- D1-454 P3 follow-up: the sheet over a tab entry has no thread title (the aggregate carries none); it shows 'Artifacts in this conversation'.
- D1-455 P3 follow-up: ZDR and restricted states hide live entries even though the sender's own stream may still hold one; chosen to avoid implying persistence.

### WS07-S13
- D1-456 P3 decision: Rule text is bilingual (NO/EN picked by i18n.tr); the plan gives only the EN text, so the NO wording is an unreviewed translation.
- D1-457 P3 gap: The button only edits the local draft; it does not save. The editor must press Save instructions (intentional, matches opt-in model WS07-D3 A).
- D1-458 P3 gap: "Already present" is a whitespace-normalised substring match; an edited or paraphrased rule is not detected and can be appended again.
- D1-459 P3 gap: No CSS added; the button and hint reuse verevon-space-secondary-action and verevon-space-inline-status.
- D1-460 P2 test-gap: SpacePage.test.tsx cases are listed in tests_owed; none were written in this phase.

### WS07-S2
- D1-461 P2 deviation: DEVIATION: chunk recovery already exists (src/app/chunk-recovery.ts from WS09-S5: key verevon.chunk-reload.v1, 30 s window, reload guards, StaleBuildNotice); the plan's shared/build/chunk-recovery.ts (key verevon.chunk-reload, 60 s) was not created, and the watchdog reuses the existing key.
- D1-462 P2 deviation: DEVIATION: Report-Only CSP header not added to the snippet (the plan puts it in S6); a marker comment is left in nginx-security-headers.conf.
- D1-463 P2 decision: HSTS now comes from a `map` ($verevon_hsts) instead of `set` + `if`; empty value means no header, same behaviour as before, and the snippet is safe to include in locations.
- D1-464 P2 decision: build id is the first 12 chars of SOURCE_REVISION; empty, unverified or unknown becomes `dev`, which never banners. A local `docker build` without args therefore has no stale-tab banner.
- D1-466 P2 gap: classify-client-error.ts still has its own clientBuildId() reading __VEREVON_BUILD_ID__; should import BUILD_ID from shared/build/build-info.
- D1-467 P2 test-gap: package.json test script not extended with scripts/nginx-headers.test.mjs (file not written yet, adding it would break pnpm test); add both together at the test phase.
- D1-468 P2 risk: UNVERIFIED nginx behaviour: `location = /index.html` re-entry from the try_files fallback and `/` index redirect, and `add_header` with an empty $verevon_hsts being skipped; verify with nginx -t and curl on the rebuilt image (CI steps added).
- D1-469 P3 follow-up: the dev server (vite) serves index.html for /version.json, so the check is a no-op in dev by design (dev build id); an e2e of the banner needs two image builds.
- D1-470 P3 follow-up: CI BUILD_DATE uses head_commit.timestamp with a repo-updated_at fallback; pull_request events have no head_commit so the fallback is not a build time.
- D1-471 P3 follow-up: banner shows regardless of unsaved work because it never reloads; the unsaved-work registry integration is S3 scope.

### WS07-S3
- D1-472 P2 deviation: DEVIATION: `client_policy` lives in `SpaceFeatureFlags` (config.rs), not as a new `AppState` field, so none of the five AppState test literals (which now name `space_features: Default::default()`) needed an edit; the plan's "AppState.client_policy" is reachable as `state.space_features.client_policy`.
- D1-473 P2 deviation: DEVIATION: `POST /api/v1/client-diagnostics` also accepts build `dev` (a local build's id, which the watchdog beacons) besides 1-12 hex; an over-512-byte body returns 400 `invalid_diagnostic` (not 413) because the plan lists only 400 for this route.
- D1-474 P2 decision: `CLIENT_MIN_CONTRACT` env may only raise the compiled-in minimum (never lower); `CLIENT_VERSION_POLICY` unknown text means `banner`. Revision/build_date are read once in `ClientPolicy::from_env`, not per request.
- D1-475 P2 decision: `enforce` degrades to banner in the SPA (client-contract.ts `contractAction`) when no unsaved-work probe is registered; `registerUnsavedWork` shares the existing WS09-S5 reload-guard set in chunk-recovery.ts, so the chat and dashboard composers already count as probes and `enforce` can auto-reload once anything is registered.
- D1-476 P2 gap: parity test `apps/gateway/scripts/client-contract-parity.test.mjs` and its entry in package.json `test` are not written (tests phase); `vite.config.ts` `clientContract` is a third mirror the parity test must read.
- D1-477 P2 gap: csp-report route and `verevon_csp_reports_total` are WS07-S6; the router in client_config.rs has the seam.
- D1-478 P3 risk: client-diagnostics has no auth and relies on the global IP limiter only; the beacon metric label set is closed (3 kinds x 4 build_match values), so cardinality is bounded.
- D1-479 P3 follow-up: the contract check is wired to `checkClientVersion` triggers, so it never runs in a `dev` build and shares that function's 30 s throttle.
- D1-480 P3 risk: UNVERIFIED nginx proxies `/api/v1/client-config` and `/api/v1/client-diagnostics` to the gateway (existing `/api/` location assumed; not run).

### WS07-S4
- D1-482 P2 deviation: DEVIATION: `loadSpaceTranscript` maps ANY 403 to `restricted` (plan says 403 `space_read_not_authorized`); matches existing panel behaviour and tolerates WS03 denial-code changes, but a revoked-membership 403 also reads as "not shared".
- D1-483 P2 gap: `SpaceThreadPanel` (WS01-S7a) still has its own try/catch mapping; should use `loadSpaceTranscript`.
- D1-484 P2 gap: room notice has no `adminCta`; WS03 `presentSpaceDenial` is to supply it.
- D1-485 P2 risk: transcript resource key is now `undefined` when shared read is unavailable, so a later `available:true` poll re-keys and fetches all posts at once.
- D1-486 P3 gap: own posts show only preview while restricted (plan flow B says so); Control's decision covers own threads too, UNVERIFIED whether own-thread reads should bypass it.
- D1-487 P3 follow-up: after WS07-S9 the artifacts route also carries `shared_read`; reuse `SpaceSharedRead` type.

### WS07-S5
- D1-488 P2 deviation: DEVIATION: the scheme allowlist already existed as features/chat/lib/safe-href.ts (isSafeLinkHref, WS26-S3) and unsafe links already rendered as a span; safe-link.ts wraps it instead of duplicating, and shared/ now imports from features/chat (layering inversion; move safe-href into shared/lib later).
- D1-489 P3 deviation: DEVIATION: blocked links keep the existing verevon-chat-link-blocked span with its localized title; the plan's "title={href}" is applied to safe anchors only.
- D1-490 P2 gap: other model-authored link renderers (ChatMessages, ChatPanels, ChatLiveRunPanel, citation chips) do not use looksSpoofed or show the host; only the markdown inline link and bare-URL anchors got title={href}.
- D1-491 P2 risk: looksSpoofed is a heuristic (domain-like visible text with differing host, www stripped); unicode/punycode lookalike hosts are not detected.
- D1-492 P2 risk: the sandbox wrapper now always nests the artifact's own html/head/body inside our body, so a full HTML document artifact is parsed with its inner head tags dropped by the HTML parser; artifacts that rely on a head-level meta viewport or title lose them (CSP stays first; styles in the body still apply).
- D1-493 P3 gap: mailto host extraction ignores multiple recipients and comma lists beyond the first address.

### WS07-S6
- D1-494 P2 deviation: DEVIATION: no `$csp_candidate` nginx variable (design table mention); the Report-Only header is a plain add_header in the shared snippet, so there is no per-location switch to disable it other than editing the snippet.
- D1-495 P2 decision: `report-uri` only (as planned); the Reporting API (`report-to` plus a `Reporting-Endpoints` header) is not set, so `application/reports+json` is accepted by the route but no browser sends it yet.
- D1-496 P2 risk: any body-read error on csp-report (too large or a broken stream) returns 413 `report_too_large`; the plan lists only 400 and 413 and `axum::body::to_bytes` does not distinguish them cheaply.
- D1-497 P2 risk: a wrong Content-Type returns 400 `invalid_report` (not 415); browsers always send one of the three accepted types.
- D1-498 P2 gap: report volume is bounded only by the gateway IP rate limiter; the counter label set is closed (<= 21 directives x 3 dispositions), so cardinality is safe, but a flood still costs CPU.
- D1-499 P2 risk: UNVERIFIED that Firefox/Safari accept `frame-ancestors` and `report-uri` in the Report-Only header without console warnings (frame-ancestors is ignored in report-only mode by spec; harmless).
- D1-500 P3 follow-up: `CSP_REPORT_LOG_HOSTS` logs origin via tracing::info; no dedupe, so a noisy page can fill dev logs.
- D1-501 P3 test-gap: nginx -t and a curl of the Report-Only header on a built image are not run (Docker not used); the nginx-headers node test (S2) does not exist yet.

### WS07-S7
- D1-502 P2 gap: SpaceThreadPanel (side pane) does not pass agents to SpaceTurnList, so its assistant avatars are always neutral.
- D1-503 P2 decision: turn-to-agent mapping is by exact agentName match (exactly one roster agent), per plan; WS08 per-turn agent id should replace agentForName.
- D1-504 P3 gap: gateway mod.rs commit includes the leftover uncommitted WS07-S4 shared_read hunk in the same file.
- D1-505 P3 follow-up: Convex createSpaceAgent silently drops an invalid colour rather than rejecting; legacy invalid stored values are filtered only at the gateway read path.
- D1-506 P3 gap: contrast of the ten AGENT_AVATAR_COLORS against white/near-black is picked by higher ratio, not asserted against a 4.5 threshold (some mid colours may fall below 4.5 both ways); test will show.
- D1-507 P3 follow-up: Convex typecheck for spaceAgents.ts not run.

### WS07-S8
- D1-508 P2 decision: DEVIATION: WS07-D9 spike not run; in-house popover used because @kobalte/core is 2.0.0-alpha.0 and unproven under Solid 2 rc; revisit if a shared HoverCard is wanted.
- D1-509 P2 gap: transcript turns carry no timestamp, so the "last reply" time shown is the thread's updated_at, not the turn's own time.
- D1-510 P2 gap: "last reply" only considers loaded turns of the post being hovered, not the agent's latest turn across the room.
- D1-511 P2 decision: attribution for blocking/working is by name heuristic (sole active agent, or the thread's last loaded assistant turn names it); WS08 agent ids should replace it.
- D1-512 P2 gap: "Waiting for {owner}" uses the thread's owner_subject_id as the approver; UNVERIFIED that Control's approver is always the thread owner.
- D1-513 P3 gap: side pane (SpaceThreadPanel) passes no threads, so its names stay plain with no hover card.
- D1-514 P3 gap: agent card only gets a hover card when identity is published and named; hover card has no touch-specific behaviour beyond focus (tabindex 0).
- D1-515 P3 gap: popover position is computed once at open (no reposition on scroll/resize); card closes on blur but not on scroll.
- D1-516 P3 test-gap: no keyboard test for Escape when focus is outside the trigger (document-level listener only while open).

### WS07-S9
- D1-528 P2 deviation: DEVIATION: keyset is (created_at, message_id, artifact_id), not (created_at, message_id), because one message can carry several artifacts and the two-column key would skip or repeat them.
- D1-529 P2 decision: the request has no user_id field (plan proto); the decision subject is bound to the caller's verified user id, and a service identity is refused permission_denied.
- D1-530 P2 decision: created_at on SpaceArtifactSummary is an RFC3339 string (microseconds, Z); the plan did not fix the proto type.
- D1-531 P2 risk: the decision token travels in the query string per the plan (like list_threads); headers (as in the SSE route) would keep it out of access logs. This handler logs no URL but upstream proxies might.
- D1-532 P2 risk: the query scans every artifact-bearing assistant turn of the Space on each page (latest-version pick before the keyset); fine at pilot size, needs a bound or a materialised latest table if a room grows large.
- D1-533 P2 risk: UNVERIFIED that `jsonb_exists` partial-index predicate is proven by the planner from `m.space_id = $2` (EXPLAIN check owed in the DB test); migration is CREATE INDEX CONCURRENTLY with `-- no-transaction` like 0055.
- D1-534 P3 gap: rows deleted/edited/redacted by future WS12 moderation are not filtered (no such column exists on this branch).
- D1-535 P3 follow-up: version_count is the count of artifact-bearing messages for that (thread, artifact), not max(version); equal in the persisted shape unless a revision is lost.

### WS07-S9.gw
- D1-517 P2 decision: the flag field is `room_artifacts` (plan name), not the `_api` suffix other Space flags use; env var is SPACE_ROOM_ARTIFACTS_API.
- D1-518 P2 decision: a ZDR org still reaches model-gateway (Session Core returns an empty page); `retention.zdr` is only copy for the empty state, never a gate.
- D1-519 P2 decision: Space membership is not called separately (same as the transcript route): Control's read decision is the membership authority, a decline is a 403.
- D1-520 P2 risk: the read decision token is in the upstream query string (plan), so access logs of model-gateway/proxies could carry it; headers would avoid that.
- D1-521 P2 gap: a malformed single item fails the whole page (502) rather than dropping that item; chosen so a contract break is visible, not hidden.
- D1-522 P2 gap: the route has no per-user rate limiter (plan names none); each call mints a Control decision and calls org-core for posture (cached 60 s).
- D1-523 P3 gap: lifecycle, decision, session token and MG calls are sequential; could be overlapped if latency matters.
- D1-524 P3 follow-up: Go/Python proto regen for ListSpaceArtifacts and SPA getSpaceArtifacts remain owed outside this lock group.

### WS07-S9.regen
- D1-536 P2 follow-up: Python/TS proto outputs (mp-events-py _gen, ts) were not regenerated for sessions/space_events/space_room additions; run the full buf.gen.yaml in a dedicated step.

### WS07-S9.spa
- D1-537 P2 gap: the Artifacts tab is still hidden by the build-time stand-in VITE_SPACE_ROOM_ARTIFACTS_UI, not by the space context `features.room_artifacts` that the gateway (WS07-S9.gw) now sends; with the gateway flag off the tab shows an error state (404 space_room_artifacts_disabled) instead of hiding.
- D1-538 P2 decision: getSpaceArtifacts was not rewritten: WS07-S12 already shipped it in spaces-client/room-artifacts.ts and it matches the gateway wire, so S9.spa is a no-op (status already-present).
- D1-539 P3 gap: the client drops items outside document/code/html and over-long titles silently; the gateway keeps them, so a page can show fewer rows than the server counted (cursor is unaffected).
- D1-540 P3 follow-up: the SPA treats the cursor as opaque; the Session Core keyset adds artifact_id (see WS07-S9 depth), nothing to change client-side.

### WS07-S9a
- D1-542 P2 gap: Only `list_thread_messages` is reduced; other model-gateway reads that flatten message metadata (if any exist for Space readers) were not audited.
- D1-543 P2 decision: Kill switch accepts exactly `1` or `true` (case-insensitive); the env var is read per request, not cached in AppState (cheap, and lets ops flip it with a restart only).
- D1-544 P3 follow-up: WS09 must add `"work"` to `ROOM_VISIBLE_METADATA_KEYS`; non-artifact keys are passed whole (no per-key reduction) so WS09 should reduce `work` before adding it.
- D1-545 P2 test-gap: Unit tests in room_metadata.rs owed (see unit file); no integration test of the HTTP handler.
- D1-546 P3 polish: room_metadata.rs is LF while neighbouring http_routes.rs/lib.rs are CRLF (git autocrlf warning); consistent with other new Space files.

### WS09-S1
- D1-551 P2 risk: UNVERIFIED live: derived `running` assumes the outbox row stays `open`/`processing` for the whole turn; heartbeats renew `deadline_at` only for the direct-inference producer, so an agentic run longer than 15 min reads `queued` again and the guard releases.
- D1-552 P2 risk: a parked run (`run_loop_checkpoints.kind='park'`) keeps an open outbox row with a future deadline, so it blocks its thread until the park horizon; decide with sibling (b) whether a parked run should hold the thread (plan risk row, UNVERIFIED).
- D1-553 P2 gap: `awaiting_approval` is never derived here (WS10 adds it to the same macro); the guard SQL already lists it in the status set.
- D1-554 P2 decision: `ListRuns`/`ListRunsByThread` `status_filter` now compares the derived phase, so `status_filter=running` matches live runs and `queued` no longer matches them; no known caller filters on `queued` (not checked across Go callers).
- D1-555 P2 test-gap: `run_service_grpc.rs` tests around `RUN_SELECT` (`ends_with`, the five `ap.` predicates count) were not run; the concat! keeps the text but the derived CASE adds `rp_o.` aliases, no `ap.`.
- D1-556 P3 follow-up: `RUN_SELECT` lines 672 and 1094 (`r.status AS run_status` for event and approval reads) still read the raw column; only GetRun/ListRuns/list_threads are honest.
- D1-557 P3 follow-up: no Prometheus counter for refusals; add `mp_thread_run_guard_total{outcome}` with id-free labels when WS09-S4 adds the attach metrics.

### WS17-S1
- D1-561 P2 risk: load_context_memory_rows, search_memory and persist/fetch retrieval each add one extra threads lookup per call for the containment test (thread_is_contained_by_id); could be folded into the existing thread query.
- D1-562 P2 decision: thread_is_contained_by_id fails closed (DB error reads as contained) on read and retrieval paths; ReaderScope::Contained audience fields are unused until WS17-S3 (dead_code allowed).
- D1-563 P2 decision: index_agent_memory answers a wide topic in a contained thread with the empty id (no write); the typed memory_scope_contained refusal is raised by the index_memory handler first. Other direct callers of index_agent_memory see "nothing written".
- D1-564 P2 gap: Contained search still returns thread-scope rows written by earlier non-contained flows only when session_id = this thread; legacy rows on a thread that later became contained are visible to that thread by design (C7 exception).
- D1-565 P2 deviation: DEVIATION: resolver lives in src/space/memory_scope.rs (plan file list says src/memory_scope.rs; the P-SC1 seam moved new logic under space/); migration number 0073 (not the design file's 0052 placeholder).
- D1-566 P2 deviation: DEVIATION: docker-compose.override.yml not edited; the base compose default is ON everywhere, so the override needs no entry.
- D1-567 P3 test-gap: Existing tests that build PendingMessage were updated (contained: false); no new tests written per wave rules.
- D1-568 P3 follow-up: Counter mp_session_memory_contained_total{op} is emitted; the one-week dev counter review and a count of NULL-kind personal threads (S1 evidence) are still to run live.

### WS17-S2
- D1-572 P2 risk: the list query adds a per-row correlated subquery over source_links and threads (limit 200 rows); cost on a large agent_memory table UNVERIFIED.
- D1-573 P2 gap: Letta-listed entries carry no withheld flag; a Letta copy of a withheld row is hidden only when its content equals the durable row.
- D1-574 P2 decision: "keep or delete" is the existing per-row delete button plus a badge; no separate keep action or confirm copy was added.
- D1-575 P2 deviation: DEVIATION: Go bindings regenerated for memory.pb.go only (native buf); sessions.pb.go and the new space_room proto from other builders were left alone.
- D1-576 P3 gap: withheld_reason and origin_space_ref ride model-gateway /v1/memories JSON (omitted when empty); gateway BFF passes it through unchanged, UNVERIFIED live; no i18n key registry used (inline tr()).

### tests-T-app-1
- D1-577 P2 deferred: P-CX1 codegen spike: docker restart convex-gateway must show 'Convex functions ready!' on the split schema; needs the live stack (later tier).
- D1-578 P2 deferred: WS04-S1 F1 live flow: add/remove a user in org X, Control roster follows within ~10 s, flag 0 returns disabled; needs the live stack.
- D1-579 P2 deferred: WS04-S5 live flow: owner creates room, adds member, audience revision advances and Control controlState becomes acknowledged; needs the live stack.
- D1-580 P2 deferred: WS04-S2 gateway tests (PATCH viewer refused before Convex, typed errors, zdr_org_room_admin_never_calls_the_model_plane): owed to the gw lock group.
- D1-581 P2 deferred: WS04-S2 SPA tests (SpaceRoomSettings.test.tsx, action-surface-contract spaces.update_room_details): owed to the spa-room lock group.
- D1-582 P3 test-gap: syncOrganizationRoom / requestOrganizationRoomSync handlers (org-core fetch, PUT body roster_mode org_derived, debounce row writes) are not driven by a mocked ctx yet; only the pure planners and the cron are covered.
- D1-583 P3 test-gap: convex-core has no committed tsc coverage of test/*.cjs; tests rely on tsx at run time.

### tests-T-ctl-1
- D1-584 P2 pre-existing: scripts/tests/zdr-contract-sync.test.sh: red today with 26 failures (90 pass), the P-CFG owed "green after the first train PR" cannot be shown yet. The stale Quarry cross-plane-token-requests.json row (pinned 40bd3586, actual 0849784e) is the one control-train-repin.sh --apply fixes; the other 25 are not P-CFG (missing Data Plane rerank-v1/usage-ledger-v2 vendored fixtures, model-plane agent-action-admission.json pin 8eb89148 vs a644aa1d, .sha256 vendored-copy rows), log for the train owner.
- D1-585 P2 deferred: scripts/tests/principal-registry-parity.test.sh: takes more than 8 minutes under Git Bash on Windows (killed at 500 s with every line ok so far); run again from CI or a Linux shell after the first train PR. See notes in the result JSON if the background run finished.
- D1-586 P3 test-fail: spaces/own_scheduled_thread_read_wire_test.go + session-core grpc.rs wire token: stale since WS17-S1 added the space_kind claim; refreshed the literal on both sides (fixed test-side, Rust verifier accepts it).
- D1-587 P3 test-fail: spaces/space_capability_decision_test.go: stale since WS06-S5 closed the egress vocabulary (allow_domains is now refused before signing); rewritten as TestIssueSpaceCapabilityDecisionNeverGrantsEgressPermissionToday.
- D1-588 P2 test-fail: session-core grpc.rs conversation_page_db_tests: four call sites did not compile (conversation_page_rows and conversation_cursor_sequence gained include_human_posts); the whole session-core test binary was red until fixed (passed true; those DB tests stay ignored without a harness).
- D1-589 P3 pre-existing: cargo test -p session-core --lib is not valid (bin-only crate); the working command is cargo test -p session-core --bins <filter>. The test rule text should say so.
- D1-590 P2 deferred: 27 DB-gated Go tests written, skipped without TEST_DATABASE_URL: TestThreadAppendMemberDecisionMatrix, 6 TestEffectPolicy* (audit trail, partial PUT, rollback, unwired, append-only), TestEnqueueAuditTxJoinsTheCallersTransactionLive, TestUpsertSpaceEffectPolicyTreatsAbsentAndNullEntitlementsAsNotStatedLive, 2 TestDiagnoseDenial*Live, TestThreadReadOwnScheduledFallbackReportsThePersonalSpaceDenialLive, 9 org_policy_live_test.go, 6 people_live_test.go.
- D1-591 P2 test-gap: http upsertSpaceEffectPolicy: the absent/null-key handling lives inline in the handler and can only be proven with a DB; extracting the "provided" map builder would let it be unit tested.
- D1-592 P3 test-gap: zdr_unproven class has no Control-side behaviour to assert beyond "Control still issues" (sandbox_capability, process_registry): the phase-1 refusal is in ValidateEntitlementChange only, covered there.
- D1-593 P3 test-gap: WS03-S2 owed zdr-contract-sync.test.sh and .github/workflows/contract-goldens.yml were not exercised (shell/CI only; no Go assertion exists for them).
- D1-594 P3 deferred: control-train-repin-test.sh mode 100755 is staged in the index but not committed: another agent has scripts/tests/space-contract-vectors-sync.test.sh staged, so a path-limited commit could not carry the mode bit. Harmless; it rides the next whole-index commit.

### tests-T-ctl-2
- D1-595 P2 deferred: spaces/membership_ops_live_test.go (7 tests): room writer matrix, byte-equal not-found, sole owner, two-owner race, expected_revision, idempotency, ledger seq, source-scoped sync; compile-checked only, need TEST_DATABASE_URL.
- D1-596 P2 deferred: spaces/archive_live_test.go (5 tests): archived room read-only vs write evidence, personal evidence active-only, Register replay precedence, refusals, set-to-state idempotency; need TEST_DATABASE_URL.
- D1-597 P2 deferred: http/space_room_state_live_test.go: typed 409 room_archived on thread-decision and thread-append-decision, non-member answer byte-equal to unknown room, ?readable=1 archived state; needs TEST_DATABASE_URL.
- D1-598 P2 deferred: users/gdpr_space_membership_ledger_postgres_test.go: erasure saga pseudonymises ledger actor_id and subject_id (fixture table added to the saga fixture schema); needs TEST_DATABASE_URL.
- D1-599 P3 test-gap: WS04-S7 "archived room fails schedule/import/sandbox" is proven at resolver level (personal resolvers forced archived by SQL), not through the HTTP decision routes; a personal Space cannot be archived by the writer.

### tests-T-gw-1
- D1-600 P2 pre-existing: gateway `cargo clippy --all-targets -D warnings` fails with 14 errors on old code (mail_task.rs doc lists, spaces/mod.rs:630 type_complexity, chat/history.rs:649, chat/streams.rs:383 constant assert); none in W1 or test code, the P-GW1 clippy-green DoD is unreachable until those are cleaned (new clippy 1.97 lints).
- D1-601 P2 pre-existing: apps/gateway/scripts/action-surface-contract.test.mjs "every registered action has exactly one live gateway dispatcher contract" red: 11 registry ids (mcp.approve_tool, tools.set_policy, chat.execute_approved_plan, chat.run.continue, chat.answer_question, approvals.decide, ...) have no handler arm; no Space ids involved.
- D1-602 P2 deferred: agent-action-admission golden regenerated (+92 rows incl. 4 WS01/WS03 ids and all other W1 ids) and 4 Model Plane vendored copies plus the .sha256 refreshed; the Model Plane consumers (capability-core Go test, execution-core and model-gateway Rust tests, zdr-contract-sync) were not run here and must be run in the Model Plane tier.
- D1-603 P3 test-gap: the WS02-S4 "31st subscribe" and "13th concurrent" tests use the in-process limiter (cache disabled); the distributed Dragonfly bucket path is untested.
- D1-604 P3 test-gap: delegation signature is asserted present and v1 with org/user headers, not recomputed against user-core's verifier (that is a Control Plane test).

### tests-T-gw-2
- D1-605 P2 deferred: scripts/tests/space-contract-vectors-sync.test.sh: Go testdata/room-role-matrix.v1.json and roles_test.go do not exist, so the Go-vs-SPA role matrix equality check is a skip until the Control S4 test creates the Go golden.
- D1-606 P2 pre-existing: apps/gateway/scripts/action-surface-contract.test.mjs "every registered action has exactly one live gateway dispatcher contract": registry/handler id sets differ (reported red by T-gw-1 too, predates these slices).
- D1-607 P3 test-gap: spaces/directory.rs: the handler's active_membership_required branch is unreachable through the router (middleware answers organization_membership_required first), so it is tested by calling the handler directly.
- D1-608 P3 test-gap: spaces/mod.rs: the /membership read route (current_membership) is a plain pass-through that does not use typed_response, so no typed mapping applies to it (only to membership refusals on the read routes).
- D1-609 P3 test-gap: SPA SpacePage.tsx wiring of writer_api, SpaceMemberRoleSelect and SpaceLeaveRoomButton is not covered here (component tests cover the pieces); a SpacePage level test belongs to the later tier.

### tests-T-gw-3
- D1-610 P3 test-gap: ClientContractBanner Reload click: asserted through a stubbed window.location.reload only; a real browser reload is not exercised (Playwright tier).
- D1-611 P3 test-gap: room_artifacts retention.zdr: tested for org-core 200 true/false and 500 (null); a 200 with a malformed body is not separately pinned.
- D1-612 P3 test-gap: csp-report 413: the cap is asserted at the 8 KiB boundary through the router; the nginx-side body limit is not covered.

### tests-T-idn-1
- D1-613 P2 deferred: audit-role-gate.spec.ts (4 e2e cases): written and listed by Playwright but skipped without E2E_ADMIN_STORAGE_STATE and E2E_MEMBER_STORAGE_STATE; the later tier needs a stack plus an admin and a member session of one org.
- D1-614 P2 deferred: db/scim_redaction_postgres_test.go: runs migration 0014 twice on seeded scim/stripe/slack rows, needs INTEGRATION_TEST_DATABASE_URL (disposable DB); the text contract test for 0014 runs and passes.
- D1-615 P3 test-gap: audit-core ListAudit/ListUsage SQL is never exercised (stub store); the user_id narrowing is asserted on the filter passed to the store, not on rows returned by Postgres.
- D1-616 P3 test-gap: WS05-S1 gateway scope is tested against wiremock doubles; no test proves the real audit-core accepts the gateway's delegated role header end to end (the e2e spec above covers that later).
- D1-617 P3 test-gap: auth-core has no real-auth.ts import path in Jest (better-auth and jose are ESM-only, not transformed); sso-provisioning.spec.ts stubs them, so it proves the options auth.ts builds, not Better Auth's runtime use of defaultRole.
- D1-618 P3 test-gap: TestServiceScopeTableCoversNewPaths only proves each Space route resolves to some spaces:* scope, not that it is the intended one; the per-path table in TestServiceScopeForRequest stays the source for that.
- D1-619 P3 pre-existing: integration-corev2 internal/codexsubscription/appserver_test.go still lists SCIM_ORG_BEARER_TOKENS as a broker-secret name; harmless (env filter fixture), left unchanged.

### tests-T-plat-1
- D1-620 P2 test-gap: new node --test files (scripts/tests/baseline-tooling, space-changed-areas, space-e2e-seed-session; tests/e2e/space-fixtures.test.ts) are not run by any package.json script or CI job yet; add them to the gate-self-test job.
- D1-621 P2 deferred: scripts/space-e2e-seed.sh --apply live proof (six users, orgs X/Y/Z, six storage states that sign in): needs the dev stack, run by someone other than the author.
- D1-622 P2 deferred: space-gate.yml on a real PR (red on a seeded failing area test, green on an untouched area): needs GitHub.
- D1-623 P2 deferred: convex-core.yml on a real PR (pnpm 11.3.0 install --frozen-lockfile, convex-test/typecheck/lint compare): needs GitHub.
- D1-624 P2 deferred: docs/plans/evidence/baseline-<date>/ capture against a clean worktree of the base SHA: runs every suite, not run here; until it exists space-baseline-compare.sh fails any id with failing tests and no stored baseline.
- D1-625 P3 test-gap: scripts/space-e2e-seed.sh shell plan-mode and the gateway half of the seed are not unit-tested (only the Node session script is).

### tests-T-run-1
- D1-627 P2 pre-existing: execution-core a_hung_child_is_killed_at_the_timeout: test fn passes in seconds but the process takes about 300 s to exit on Windows (orphaned sleep holds a blocking pipe read; runtime shutdown waits); skip it when iterating.
- D1-628 P2 test-gap: WS06-S7 "memory flat" is asserted only as returned text under the 64 KiB cap plus the kill finishing early; no RSS measurement.
- D1-629 P2 test-gap: WS06-S7 deploy check is a text contract; `docker compose config` for base and production overlay is deferred (COMPOSE_CONFIG=1) to the docker tier.
- D1-630 P3 test-gap: WS06-S3 route test uses a synthetic Router with the same Router::layer ordering, not the full http_routes::router (needs AppState).
- D1-631 P3 test-gap: spawn_gate and its permit count are env-fixed OnceLock; gate tests hold the real global gate under paused time (serialised) instead of an injected gate.
- D1-632 P3 test-gap: process_host WouldBlock test covers the refusal before scratch creation; a successful gated spawn needs bwrap (Linux), not run here.

### tests-T-sc-1
- D1-633 P2 pre-existing: model-gateway tests/zdr_refusal_inventory.rs: fails on untyped ZDR refusal at browser_run.rs:802 (imported Quarry browser work, not W1); the space_routes rows are not flagged.
- D1-634 P2 deferred: session-core space::events db_tests (11, space_event_*): written, compile, never run; scripts/tests/space-event-log-postgres-test.sh needs docker and expects at least 8.
- D1-635 P2 deferred: session-core grpc.rs member/post DB tests (13) and dreaming DB test: need Postgres and env-set decision keys; fixtures (threads/messages NOT VALID checks, threads columns) are unverified until run.
- D1-636 P3 test-gap: authz flag-off authorize_append DB test and SpaceRoomReadService served-empty (unimplemented) call are DB/grpc-server tier; only the health-name list is asserted.
- D1-637 P3 test-gap: chat_path_forwards_client_message_id is covered only by session_flow send_receipt_id unit test, not an e2e invoke assertion.

### tests-T-sc-2
- D1-640 P2 pre-existing: scripts/tests/zdr-contract-sync.test.sh: 30 failures in this worktree (missing Quarry and retrieval-engine fixtures, CRLF pin files, unvendored posture matrix consumers); the new space-surface rows pass, REQUIRE_CONTRACT=1 cannot be green here.
- D1-641 P2 deferred: 23 DB-tier tests (space_stream_grpc db_tests 7, space/read_service db_tests 7 incl. migration 0085 and EXPLAIN, run_phase_pg_* 9) are written and compile but never ran; run with --ignored --test-threads=1 on a scratch DB, the Space-thread start path in run_phase_pg_* is the likeliest to need a fix.
- D1-642 P2 test-gap: space-events.json has no generator under MP_UPDATE_GOLDEN (hand-authored golden); replaced by drift tests of kinds, closing codes, closed words, ref keys and timing against the real tables.
- D1-643 P2 test-gap: stream live tail relies on the 10 s heartbeat drain in the DB test, the LISTEN wake path (SpaceWakes publish is private to events.rs) has no direct test.
- D1-644 P3 test-gap: record_scheduled_run_result_inner ZDR refusal (DB) and the lazy-pool-free paths of complete_direct_run are covered by the source inventory scan and the builder's updated DB test only.
- D1-645 P3 test-gap: model-gateway no-decision-in-logs for the artifacts route is asserted on the response body only; session_flow.rs:2487 and e2e:9874 were not switched to typed statuses (optional).

### tests-T-sc-3
- D1-647 P2 test-gap: grpc.rs read_letta (context assembly) and retrieval_org_wide / retrieval_persist containment sites (grpc.rs ~6545, 9372, 9564): no test; they need a Letta/retrieval fake.
- D1-648 P2 test-gap: mp_session_memory_contained_total{op} counter labels are not asserted (the counter recorder in grpc_space_w1_tests.rs is private).
- D1-649 P3 test-gap: gateway list_memories handler mapping is inline, so only the serialised shape is tested, not the handler through a mocked session-core (e2e list_memory mock returns unimplemented).
- D1-650 P3 test-gap: containment_enabled() env read is untested (parallel env races); only the pure containment_enabled_from is covered.

### tests-T-spa-room-1
- D1-653 P2 test-fail: SpacePage.test.tsx mock of spaces-client was stale (no loadSpaceTranscript, getSpaceRosterView, createSpacePost, replyInSpaceThread, SPACE_POST_MAX_CHARS_DEFAULT): 6 tests failed on W1 changes; fixed test-side.
- D1-654 P2 test-fail: chat-route-ownership.test.ts flagged the doc comment in space-thread-deep-link.ts; the guard now skips whole-line comments (production file untouched).
- D1-655 P2 test-fail: CoreSidebarSpacesPanel.test.tsx still expected the plain room href; thread rows now carry ?thread_id=...#chat, literal updated.
- D1-656 P2 test-gap: SpaceRoomTimeline.test.tsx mocked only getSpaceThreadTranscript, so every pre-existing case silently ran the error path (preview fallback); loadSpaceTranscript mock added.
- D1-657 P2 pre-existing: apps/gateway/scripts/action-surface-contract.test.mjs dispatcher-equality still fails (registry ids without a handlers.rs arm: mcp.approve_tool, tools.set_policy, chat.run.continue, approvals.decide, audit.*), unrelated to the fragment move.
- D1-658 P3 pre-existing: tsc -b error TS2345 at action-registry.test.ts lines 442 and 468 (getActionDescriptor called with a loop string id); not from this slice.
- D1-659 P3 test-gap: SpaceCockpit evaluates visibleSpaceTabs() once per render; a tab flag flipped at runtime is not re-shown until remount, so no runtime-toggle test was written.
- D1-660 P3 deferred: tests/e2e/space-human-posts.spec.ts (3) and space-thread-pane.spec.ts (3) written and listed by Playwright, not run (need seeded fixtures and the stack); none uses a Verevon, budget or genius model.
- D1-661 P3 deferred: space-thread-pane focus-return and keyboard-resize assertions rely on the stored width key and .verevon-space-auxpane class; unverified against a real browser.

### tests-T-spa-room-2
- D1-663 P2 test-gap: apps/Application Plane/convex-core has no convex/_generated/server in this tree, so test/*.test.cjs cannot run in-package; space-read-markers cases were verified against a scratch extraction only.
- D1-664 P2 deferred: tests/e2e/space-realtime.spec.ts (4 tests: 1.5 s p95 and no 6 s timer, Last-Event-ID resume, session-core stop to degraded within 35 s, ZDR stays on poll); needs seeded fixtures, E2E_SPACE_STOP/START_SESSION_CORE_CMD and E2E_SPACE_REF_ZDR.
- D1-665 P2 deferred: tests/e2e/space-human-posts.spec.ts reply count and unread badge on A's own thread after B replies from the pane; needs the live fixtures.
- D1-666 P3 test-gap: SpaceRoomSettings: a room_details_invalid ApiError whose details carry any string reason (even an unknown one) is read as a typed Space denial and shows the generic denial sentence instead of the code's own copy; only details with no reason key reach the catalogue text.
- D1-667 P3 test-gap: people-search 429 shows the shared rate_limited copy, which talks about the room refreshing more slowly, not about the search.
- D1-668 P3 pre-existing: src/shared/api/quarry-client.test.ts fails to resolve @quarry/client; action-surface-contract whole-set equality test fails on registry/handler drift predating W1.
- D1-669 P3 test-gap: parseSpaceStreamCursor accepts a seq of whitespace ('g. ') as 0 because Number(' ') is 0; harmless for server-minted ids.

### tests-T-spa-room-3
- D1-670 P3 test-gap: SpaceThreadPanel: still reads getSpaceThreadTranscript (not migrated to loadSpaceTranscript, per WS07-S4 notes); no restricted/missing/error states there are tested.
- D1-671 P3 test-gap: ArtifactsPanel room surface: only the document text and "Run preview" gate are asserted; the iframe sandbox attributes after Run preview are not covered (needs a browser tier).
- D1-672 P3 test-gap: real stream to live chip path (streamChat events into createRoomLiveArtifacts through SpacePage) is covered in two halves (pipeline onArtifact, timeline liveArtifacts), not end to end.
- D1-673 P3 pre-existing: running the whole src/features/spaces tree at default vitest workers times out forks while other agents run in parallel; scoped runs with --maxWorkers=3 are green.

### tests-T-spa-shell-1
- D1-675 P3 pre-existing: scripts/tests/zdr-contract-sync.test.sh: 26 failures in rows unrelated to WS03-S7 (missing rerank-v1 and dp-contracts goldens, drifted pins for cross-plane-token-requests and agent-action-admission, .sha256 copies differing); the 4 new space-denial rows pass, and --self-test aborts on the missing rerank golden.
- D1-676 P3 deferred: tests/e2e/space-policy-admin.spec.ts: written, not run (needs a live stack, E2E_ADMIN_STORAGE_STATE, and E2E_SPACE_POLICY_ORG_OK=1 for the write case on a dedicated org).
- D1-677 P3 deferred: CI nginx -t and curl header smoke in the docker-image job (WS07-S2) is not run here.
- D1-678 P2 test-gap: nginx.conf: header tests parse the config text, they do not run nginx, so an include path typo would only be caught by the CI nginx -t step.
- D1-679 P2 test-gap: boot-watchdog: tested against jsdom with fake timers; a real CSP-restricted load of the classic script is untested.
- D1-680 P3 test-gap: build_id mismatch reload relies on a hard-coded copy of the chunk-recovery key in boot-watchdog.js; the test pins the literal but not equality with CHUNK_RELOAD_KEY in src/app/chunk-recovery.ts.

### tests-T-spa-shell-2
- D1-681 P3 test-gap: SpaceIdentityHoverCard positioning (flip above, viewport clamp): jsdom has no layout, so placement is not asserted; covered by a later browser tier.
- D1-682 P3 test-gap: hover card blocking line (agent awaiting_approval): exercised only as a model/unit case, because the server does not yet derive awaiting_approval into the thread projection (WS09/WS10).
- D1-683 P3 test-gap: Convex createSpaceAgentForGateway: driven through a mocked ctx (handler._handler); no real Convex deployment test.


## Gateway surface review (2026-11-03): the 46 bff rows, the approval guard, the live-view posture

Reviewed against the handlers by reading each one and checking its guard calls, following same-file
delegation (the checker is `.plan-work` scratch, not committed). 36 rows match their handler with no finding;
the other 10 are noted in the table. The three decision types the classification pass invented
were checked against each handler's Control path: the first mapping of each to an existing value was
wrong, and the classification's own value is restored.

| # | Route | Route class | zdr_class | basis | decision_type | Handler | Review |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `DELETE /api/v1/spaces/{space_ref}/approval-grants/{grant_id}` | Config | n/a | `path-rule` | `membership` | `domains/spaces/approvals.rs::revoke_approval_grant_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 2 | `GET /api/v1/spaces/{space_ref}/approval-grants` | ReadMetadata | n/a | `path-rule` | `membership` | `domains/spaces/approvals.rs::list_approval_grants_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 3 | `GET /api/v1/spaces/{space_ref}/approval-policy` | ReadMetadata | n/a | `path-rule` | `membership` | `domains/spaces/approvals.rs::get_approval_policy_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 4 | `POST /api/v1/spaces/{space_ref}/approvals/{approval_id}/decide` | DurableWrite | refuse | `guard:durable_write_refusal` | `membership` | `domains/spaces/approvals.rs::decide_space_approval` | fixed: guard added to the shared decide body (durable_write_refusal); basis now guard; decision_type not named in the bff code, so membership is UNVERIFIED |
| 5 | `GET /api/v1/spaces/attention` | ModelRead | n/a | `posture-decided` | `thread-read-decision` | `domains/spaces/attention.rs::get_attention` | checked: ZDR decided per call, answers zdr_empty with no upstream call; nothing stored, so n/a stands |
| 6 | `GET /api/v1/spaces/attention/items` | ModelRead | refs_only | `posture-decided` | `thread-read-decision` | `domains/spaces/attention.rs::list_items` | checked: ZDR answers zdr_refs_only; refs_only stands |
| 7 | `GET /api/v1/spaces/{space_ref}/notify` | ReadMetadata | n/a | `path-rule` | `membership` | `domains/spaces/attention.rs::get_notify` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 8 | `GET /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` | ReadMetadata | n/a | `path-rule` | `membership` | `domains/spaces/attention.rs::get_follow` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 9 | `POST /api/v1/spaces/attention/items/{item_id}/state` | Config | refs_only | `path-rule` | `none` | `domains/spaces/attention.rs::set_item_state` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 10 | `POST /api/v1/spaces/attention/read-all` | Config | refs_only | `path-rule` | `membership` | `domains/spaces/attention.rs::read_all` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 11 | `POST /api/v1/spaces/{space_ref}/unread` | Config | refs_only | `path-rule` | `membership` | `domains/spaces/attention.rs::post_unread` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 12 | `PUT /api/v1/spaces/{space_ref}/notify` | Config | n/a | `path-rule` | `membership` | `domains/spaces/attention.rs::put_notify` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 13 | `PUT /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` | DurableWrite | refuse | `guard:durable_write_refusal` | `membership` | `domains/spaces/attention.rs::put_follow` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 14 | `DELETE /api/v1/spaces/{space_ref}/hide` | Config | n/a | `path-rule` | `membership` | `domains/spaces/dm.rs::unhide_space` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 15 | `POST /api/v1/spaces/dm` | DurableWrite | refuse | `guard:durable_write_refusal` | `membership` | `domains/spaces/dm.rs::open_dm` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 16 | `POST /api/v1/spaces/{space_ref}/hide` | Config | refs_only | `path-rule` | `membership` | `domains/spaces/dm.rs::hide_space` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 17 | `DELETE /api/v1/spaces/{space_ref}/files/{document_id}` | Config | n/a | `path-rule` | `retrieval-decision` | `domains/spaces/files.rs::delete_file` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 18 | `GET /api/v1/spaces/{space_ref}/files` | ModelRead | n/a | `path-rule` | `retrieval-decision` | `domains/spaces/files.rs::list_files` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 19 | `GET /api/v1/spaces/{space_ref}/files/uploads/{upload_id}` | ReadMetadata | n/a | `path-rule` | `none` | `domains/spaces/files.rs::get_upload` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 20 | `GET /api/v1/spaces/{space_ref}/files/{document_id}/text` | ModelRead | n/a | `path-rule` | `retrieval-decision` | `domains/spaces/files.rs::read_file_text` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 21 | `POST /api/v1/spaces/{space_ref}/files` | DurableWrite | refuse | `guard:durable_write_refusal` | `import-decision` | `domains/spaces/files.rs::upload_files` | corrected: decision_type is import-decision (Control path in files.rs); my earlier retrieval-decision was wrong |
| 22 | `DELETE /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}` | DurableWrite | refuse | `guard:durable_write_refusal` | `thread-revise-decision` | `domains/spaces/message_lifecycle.rs::delete_route` | corrected: decision_type is thread-revise-decision (Control path in message_lifecycle.rs); my earlier thread-decision was wrong |
| 23 | `DELETE /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}` | Config | n/a | `path-rule` | `membership` | `domains/spaces/message_lifecycle.rs::remove_reaction_route` | checked: set_reaction refuses only the add path; removal is not refused by design, n/a stands |
| 24 | `PATCH /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}` | DurableWrite | refuse | `guard:durable_write_refusal` | `thread-revise-decision` | `domains/spaces/message_lifecycle.rs::edit_route` | corrected: decision_type is thread-revise-decision (Control path in message_lifecycle.rs); my earlier thread-decision was wrong |
| 25 | `PUT /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}/reactions/{emoji}` | DurableWrite | refuse | `guard:durable_write_refusal` | `membership` | `domains/spaces/message_lifecycle.rs::add_reaction_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 26 | `GET /api/v1/spaces/features` | ReadMetadata | n/a | `path-rule` | `none` | `domains/spaces/message_search.rs::space_features` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 27 | `GET /api/v1/spaces/{space_ref}/entries/{message_id}` | ModelRead | n/a | `path-rule` | `thread-read-decision` | `domains/spaces/message_search.rs::resolve_space_entry` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 28 | `POST /api/v1/spaces/{space_ref}/search` | ModelRead | n/a | `path-rule` | `thread-read-decision` | `domains/spaces/message_search.rs::search_space_messages` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 29 | `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy` | Config | n/a | `path-rule` | `membership` | `domains/spaces/agent_policy.rs::set_space_agent_wake_policy_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 30 | `PATCH /api/v1/spaces/{space_ref}/settings` | Config | n/a | `path-rule` | `membership` | `domains/spaces/room_admin.rs::update_room_settings_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 31 | `POST /api/v1/spaces/{space_ref}/join` | Config | n/a | `path-rule` | `membership` | `domains/spaces/room_admin.rs::join_room_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 32 | `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}` | Config | n/a | `path-rule` | `membership` | `domains/spaces/pins.rs::unpin_thread` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 33 | `PUT /api/v1/spaces/{space_ref}/pins/{thread_id}` | Config | refs_only | `path-rule` | `membership` | `domains/spaces/pins.rs::pin_thread` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 34 | `DELETE /api/v1/spaces/reminders/{reminder_id}` | Config | n/a | `path-rule` | `none` | `domains/spaces/reminders.rs::cancel_reminder` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 35 | `GET /api/v1/spaces/reminders` | ReadMetadata | n/a | `path-rule` | `none` | `domains/spaces/reminders.rs::list_reminders` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 36 | `POST /api/v1/spaces/{space_ref}/reminders` | DurableWrite | refuse | `guard:durable_write_refusal` | `membership` | `domains/spaces/reminders.rs::create_reminder` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 37 | `GET /api/v1/spaces/{space_ref}/runs/{run_id}/live` | ModelRead | refuse | `posture-decided` | `thread-read-decision` | `domains/spaces/run_live.rs::space_run_live` | checked: ZDR org refused before any Control call (org-level, not per token); see open item RV-03 |
| 38 | `POST /api/v1/spaces/{space_ref}/runs/{run_id}/stop` | DurableWrite | refuse | `guard:durable_write_refusal` | `run-control-decision` | `domains/spaces/run_live.rs::space_run_stop` | corrected: decision_type is run-control-decision (Control path in run_live.rs); my earlier token_posture was wrong |
| 39 | `GET /api/v1/spaces/sidebar` | ReadMetadata | local_only | `posture-decided` | `none` | `domains/spaces/sidebar.rs::get_sidebar` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 40 | `PUT /api/v1/spaces/sidebar` | DurableWrite | refuse | `guard:durable_write_refusal` | `none` | `domains/spaces/sidebar.rs::put_sidebar` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 41 | `GET /api/v1/spaces/status` | ReadMetadata | refs_only | `posture-decided` | `none` | `domains/spaces/status.rs::get_status` | checked: ZDR withholds text (text_allowed); ids and expiry only, refs_only stands |
| 42 | `PUT /api/v1/spaces/status` | DurableWrite | refuse | `guard:durable_write_refusal` | `none` | `domains/spaces/status.rs::put_status` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 43 | `GET /api/v1/spaces/{space_ref}/timeline` | ModelRead | n/a | `path-rule` | `thread-read-decision` | `domains/spaces/timeline.rs::space_timeline` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 44 | `GET /api/v1/spaces/{space_ref}/tool-policy` | ReadMetadata | n/a | `path-rule` | `membership` | `domains/spaces/tool_policy.rs::get_tool_policy_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 45 | `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy` | Config | n/a | `path-rule` | `membership` | `domains/spaces/tool_policy.rs::put_agent_tool_policy_route` | checked: guard matches the handler body (or no ZDR call for n/a) |
| 46 | `PUT /api/v1/spaces/{space_ref}/tool-policy` | Config | n/a | `path-rule` | `membership` | `domains/spaces/tool_policy.rs::put_space_tool_policy_route` | checked: guard matches the handler body (or no ZDR call for n/a) |

### Open items from this pass

- RV-01 (decided by the owner, enforced now): approval decide routes refuse a zero-retention caller before any Session Core call, through require_non_zdr_durable_mutation (model-gateway, DURABLE_MUTATIONS) and durable_write_refusal (frontend gateway, shared decide body). The ZDR catalogue already has copy for zdr_durable_mutation_forbidden (shared/api/zdr-refusals.ts), so errors.ts needed no new entry. Both are tested: the frontend by a handler test; the model-gateway side by the DURABLE_MUTATIONS lint only.
- RV-02 (decided by the owner): model-gateway run_live allows a waiver for a run bound to the waived connection, decided with zdr_posture::decide_for on the run's route; a waiver on a run that names no connection, or another, is refused, and a plain ZDR credential stays ZDR. The three cases are unit-tested in zdr_posture. OPEN: Session Core's AuthorizeRoomRunAttach response names no provider or connection, and Session Core refuses every zdr_waived run (room_run_attach.rs: retention_class != persistent). Until both change, the handler decides on the default route and a waived viewer is refused end to end. The change is a proto field, a Session Core read of the run's provider and waiver connection, and relaxing that refusal for zdr_waived runs, with DB tests. It is cross-plane and was not made here.
- RV-03: the frontend run_live refuses every zero-retention org member before any Control call, by org posture, not per token. Under the owner's waiver rule that is stricter than model-gateway; left as it is pending the owner's answer.
- RV-04 (accepted by the owner): members. Control's CanAdd is the single authority (room_role_forbidden for a viewer); the gateway applies no owner/manager floor since WS04-S12, and its tests assert the relayed refusal.
- RV-05 (UNVERIFIED): decision_type of the two approval decide rows. Neither handler names the Control decision it mints, so the values are not confirmed.
- RV-06: the golden's diff in git shows deletions because the new rows are sorted into the array; every HEAD row is unchanged, in order (checked by comparing the rows by id).

## Live-view waiver through the attach verdict (owner-approved 2026-11-03)

- Proto: `AuthorizeRoomRunAttachResponse` gains `provider` (field 8) and `connection` (field 9). They are set only for a
  zero-retention-waived run bound to the caller's own waiver (same connection and acknowledgement). A persistent run leaves
  both empty. Additive, so `buf breaking` against the baseline sees no break.
- Go bindings regenerated for `runs.proto` with the local `protoc-gen-go` and `protoc-gen-go-grpc`. The checked-in `go/gen`
  was stale: it lacked `CancelRunAsRoomManager`, which the proto already declared, so the regeneration also brings those
  messages in. Rust bindings come from `mp-contracts/build.rs` at build time. Python (`mp-events-py/_gen`) and the Verevon TS
  output were not regenerated; run the full `buf generate` in the test phase.
- Session Core (`room_run_attach.rs`): the attach query now reads `waiver_connection_id` and `waiver_ack_id`
  (`fetch_attach_row`). `live_route` admits a `zdr_waived` run only when the caller's waiver names the same connection and
  acknowledgement the run rests on; the verdict then names that waiver's provider and connection. The run row stores the
  connection and the ack but not the provider, so the provider is taken from the matching waiver: the matching ack is what
  makes it the same waiver. A persistent run names no route. A zero-retention run, a waived run with no receipt, and a
  waived run for another connection or ack are refused.
- Gateway (`model-gateway` `space_routes/runs.rs`): after the verdict, the posture is decided on `Route::named(provider,
  connection)` when the verdict names a route, and on the default route otherwise. A bound waiver is persistent for its
  connection; anything else is refused.
- Tests: `live_route` unit tests (five cases) are runnable. Two DB tests are `#[ignore]`d and were not run:
  `a_bound_waived_live_run_reads_its_receipt_and_is_admitted_on_its_connection` and
  `a_persistent_run_reads_with_no_route`. Both run in the test phase after the merge.
- Merge note: Model Plane is also editing `session-core` on `model-plane` (the `ApprovalCause` field moves to 10 and 11).
  Those numbers are on a different message, so they do not collide with fields 8 and 9 here, but check them at the merge.
- Still open: the frontend `run_live` (`domains/spaces/run_live.rs`) refuses every zero-retention org member by org
  posture, not per token. It was left as it is pending the owner's answer.
