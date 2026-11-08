# Space build ledger, wave W1 (M0 + M1)

Generated 2026-10-27. Branch `space` (cut from `model-plane` e603fde78), builder commits are path-limited, nothing is pushed. Source: `.plan-work/space-build/units/*.json`, `tests/*.json`, `ledger.md`.

Slices and legs: 70 (built 68, partial 1, already-present 1). Files changed: see `git diff --stat e603fde78..space`.

## Slices

| slice | status | commit | compile | migrations | flags |
|---|---|---|---|---|---|
| M0-CI | built | c8dd68419 | ok |  |  |
| M0-T0 | partial | fb992136a | ok |  |  |
| P-CFG | built | b8f9a737d | ok (bash -n on the script; no compiled code in this slice) |  |  |
| P-CX1 | built | 9db20599e | ok: tsc clean on schema files; exported schema JSON identical to the o |  |  |
| P-GW1 | built | 1868b5dc1 | ok: cargo check (bin) clean, no warnings; test cfg not built per rules |  |  |
| P-SC1 | built | fe4e93e7c | ok |  |  |
| P-SPA1 | built | 541a6f835 | ok (pnpm exec tsc -b: no errors in touched files; only pre-existing Re |  |  |
| P-SPA2 | built | 44f5d857d | ok (pnpm exec tsc -b: no errors in touched files) |  |  |
| P-UC1 | built | d56b3c41d | ok (go build ./... and go vet ./internal/http) |  |  |
| WS01-S1 | built | f900b6b14 | ok | session-core 0071 apps/Model Plane/rust/services/session-core/migrations/0071_message_post_kind.sql |  |
| WS01-S2 | built | 97c5f6b7b | ok (go build ./... and go vet ./internal/spaces ./internal/http; cargo |  |  |
| WS01-S3 | built | 88fa0f3df | ok (cargo check -p session-core --bins; session-core has no lib target |  | SPACE_HUMAN_POSTS_CORE session-core env: compose default 0, dev override 1; API_ENV const SPACE_HUMAN_POSTS_API defined  |
| WS01-S4 | built | aca9c2c3e | ok (cargo check -p model-gateway --lib --bins) |  | SPACE_HUMAN_POSTS_API default 0 (compose ${:-0}), dev override 1; read via mp_contracts::space_human_posts::api_enabled |
| WS01-S5 | built | 9aecdb37a | ok: gateway cargo check clean; tsc -b no errors in src/shared/actions |  | SPACE_HUMAN_POSTS_API dev on / production false (SpaceFeatureFlags.human_posts_api); SPACE_THREAD_PANE_UI dev on / produ |
| WS01-S6 | built | bbc736596 | ok (pnpm exec tsc -b: no errors in touched files; only pre-existing Re |  | features.human_posts read from GET /context (gateway SPACE_HUMAN_POSTS_API, dev on / prod off); no new flag, no build-ti |
| WS01-S7a | built | b4556868e | ok |  | VITE_SPACE_THREAD_PANE_UI default on in dev, off in production (build-time stand-in for space_thread_pane_ui) |
| WS01-S7b | built | 39c9805ae | ok (pnpm exec tsc -b: no errors in touched files; only pre-existing Re |  | features.human_posts read from GET /context (no new flag) |
| WS01-S8 | built | 91972385c | ok (pnpm exec tsc -b: no errors in touched files; only pre-existing Re |  | VITE_SPACE_SEND_IDEMPOTENCY_API build-time, dev on / prod off; VITE_SPACE_CONNECTIVITY_BANNER_UI build-time, dev on / pr |
| WS02-S1 | built | c250be4c8 | ok | migration 0072 apps/Model Plane/rust/services/session-core/migrations/0072_space_event_log.sql | SPACE_REALTIME_EMIT false (code, production, release); true in dev compose |
| WS02-S2 | built | 65fb164d2 | ok (go build ./... and go vet ./internal/spaces ./internal/http) |  |  |
| WS02-S3 | built | 9bef5aff0 | ok (cargo check -p session-core --bins; -p model-gateway --lib --bins) |  |  |
| WS02-S4 | built | a7198313f | ok: cargo check (bin) clean, no warnings |  | SPACE_REALTIME_STREAM (SpaceFeatureFlags.realtime_stream) dev on, production false |
| WS02-S5 | built | 3d09e3d8f | ok (pnpm exec tsc -b: no errors in touched files; only pre-existing sc |  |  |
| WS03-S1 | built | fff13f9bb | ok | user-core 033 apps/Control Plane/user-core/migrations/033_space_effect_policy_events.up.sql |  |
| WS03-S2 | built | 0af97c1ca | ok |  |  |
| WS03-S3 | built | 5d8a951d0 | ok |  | SPACE_POLICY_ADMIN_WRITES default false |
| WS03-S4 | built | cdb45096e | ok: gateway cargo check clean, no warnings; tsc -b no errors in src/sh |  | SPACE_POLICY_ADMIN_API (SpaceFeatureFlags.policy_admin_api) dev on, production off |
| WS03-S5 | built | 82fbba719 | ok: cargo check (bin) clean, no warnings |  | SPACE_DENIALS_TYPED (SpaceFeatureFlags.denials_typed) dev on, production off |
| WS03-S6 | built | 87ce5408d | ok: pnpm exec tsc -b, no errors in touched files |  | space_policy_admin_api (gateway SPACE_POLICY_ADMIN_API; 404 space_policy_admin_disabled hides the console and the nav li |
| WS03-S7 | built | c78394d90 | ok (pnpm exec tsc -b: no errors in touched files; 34 pre-existing erro |  | space_denials_typed (gateway flag from WS03-S5; SPA has no flag of its own: it reads details when present) |
| WS04-S1 | built | d4d417f8f | ok: tsc --noEmit clean on convex-core in a scratch copy (node_modules  |  | SPACE_ORG_ROOM_RECONCILE (space_org_room_reconcile_sync) dev compose 1, startup.sh default 0 |
| WS04-S2 | built | 35f118a18 | ok: tsc --noEmit clean on convex-core in a scratch copy (node_modules  |  |  |
| WS04-S2.gw | built | 9a805bc26 | ok: gateway cargo check (bin) clean |  | SPACE_ROOM_DETAILS_UI (SpaceFeatureFlags.room_details_ui) dev default on, production false in compose override and when  |
| WS04-S2.spa | built | c24a51501 | ok: pnpm exec tsc -b shows no errors in touched files (pre-existing er |  | features.room_details read from space context (gateway SPACE_ROOM_DETAILS_UI, default off in production) |
| WS04-S3 | built | 79640b054 | ok (go build ./... and go vet ./internal/directory ./internal/http) |  | scope directory:read:self on the verevon-gateway user-core credential (gateway flag SPACE_PEOPLE_SEARCH_API not built he |
| WS04-S3.gw | built | 0502a1163 | ok: gateway cargo check (bin) clean |  | SPACE_PEOPLE_SEARCH_API (SpaceFeatureFlags.people_search_api) dev default on, production false in compose override and w |
| WS04-S3.spa | built | e6544130d | ok: pnpm exec tsc -b shows no errors in touched files |  | features.people_search read from space context (gateway SPACE_PEOPLE_SEARCH_API); no new flag |
| WS04-S4 | built | 4f5c048ad | ok (go build ./... and go vet ./internal/spaces ./internal/http) | migration user-core 034 apps/Control Plane/user-core/migrations/034_space_membership_source_and_events.up.sql (+ .down.s | scope spaces:membership:act on verevon-gateway and spaces:membership:read on application-space-lifecycle: dev compose on |
| WS04-S5 | built | b59983d2a | ok: tsc --noEmit over convex/**/*.ts clean (scratch tsconfig using the |  | SPACE_AUDIENCE_RECONCILE (space_audience_reconcile_sync) default 1 in dev compose, 0 in startup.sh default (production o |
| WS04-S6 | built | 49ff6d436 | ok: gateway cargo check clean; SPA tsc -b has no errors in touched fil |  | SPACE_MEMBERSHIP_WRITER_API (gateway SpaceFeatureFlags.membership_writer_api) dev default on, production false |
| WS04-S7 | built | 7bd8a2b21 | ok (go build ./... and go vet ./internal/spaces ./internal/http) | migration user-core 035 apps/Control Plane/user-core/migrations/035_space_archived_state.up.sql (+ .down.sql) | space_room_archive_api gateway flag: NOT built here (gateway leg); Control routes dark by credential spaces:membership:a |
| WS05-S1 | built | b2591719d | ok |  | AUDIT_READ_ROLE_ENFORCEMENT enforce (report/off refused when ENVIRONMENT=production) |
| WS05-S2 | built | 3ba2225dc | ok | integration-corev2 0014 internal/db/migrations/0014_scim_stub_payload_redaction.sql |  |
| WS05-S3 | built | bebd1f0c5 | user-core go build ./internal/http ok; auth-core not-checked: node_mod |  |  |
| WS06-S1 | built | 7ac416d55 | ok |  |  |
| WS06-S2 | built | fda13d44f | ok |  | SESSION_CORE_SPACE_ZDR observe |
| WS06-S3 | built | 9b8516631 | ok |  |  |
| WS06-S4 | built | eff8fcbfc | not-checked: no Rust, Go or TS source changed (golden JSON + sync scri |  |  |
| WS06-S5 | built | b41f0aefc | ok |  |  |
| WS06-S7 | built | ccd0f381c | ok |  | EXECUTION_SANDBOX_MAX_OUTPUT_BYTES 1048576; EXECUTION_SANDBOX_MAX_CONCURRENT 8; EXECUTION_SANDBOX_QUEUE_WAIT_MS 5000; EX |
| WS07-S1 | built | 0a797ec37 | ok |  |  |
| WS07-S10 | built | 22a1f0b66 | ok (pnpm exec tsc -b: no errors in touched files) |  |  |
| WS07-S11 | built | 71430a63a | ok (pnpm exec tsc -b: 34 pre-existing errors, none in touched files) |  | VITE_SPACE_ROOM_ARTIFACTS_UI default on in dev, off in production (build-time stand-in for features.room_artifacts until |
| WS07-S12 | built | 729ac600a | ok (pnpm exec tsc -b: 34 pre-existing errors, none in touched files) |  | VITE_SPACE_ROOM_ARTIFACTS_UI default on in dev, off in production (same build-time stand-in as S11; hides the tab via th |
| WS07-S13 | built | 670eec671 | ok |  |  |
| WS07-S2 | built | 68b61fd0a | ok (pnpm exec tsc -b: no errors in touched files; vite config loads, d |  |  |
| WS07-S3 | built | 462cafd5c | ok (gateway cargo check clean; tsc -b no errors in touched files) |  | CLIENT_VERSION_POLICY banner; CLIENT_MIN_CONTRACT unset (floor 1) |
| WS07-S4 | built | b60a7638d | ok (tsc -b clean for touched files; gateway cargo check ok) |  |  |
| WS07-S5 | built | 1c216a7dd | ok |  |  |
| WS07-S6 | built | 116c8b56e | ok (gateway cargo check clean) |  | CSP_REPORT_LOG_HOSTS off (forced off when APP_ENV=production) |
| WS07-S7 | built | 1a342b705 | ok (gateway cargo check ok; tsc -b no errors in touched files; convex  |  |  |
| WS07-S8 | built | 968829da4 | ok (pnpm exec tsc -b: no errors in touched files) |  |  |
| WS07-S9 | built | c9b684f8d | ok | session-core 0085 apps/Model Plane/rust/services/session-core/migrations/0085_messages_space_artifact_index.sql |  |
| WS07-S9.gw | built | b7d923cd8 | ok |  | SPACE_ROOM_ARTIFACTS_API dev on, production false (SpaceFeatureFlags.room_artifacts; features.room_artifacts on space co |
| WS07-S9.regen | built | 88ea93565 | ok: go build ./... in go/gen |  |  |
| WS07-S9.spa | already-present | 729ac600a | not-checked: no edits made; WS07-S12 compiled these files ok |  |  |
| WS07-S9a | built | 9d145f261 | ok |  | SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH false |
| WS09-S1 | built | be720734b | ok |  | SPACE_RUN_THREAD_GUARD default ON (kill switch false/0/off/no) |
| WS17-S1 | built | fad74082f | ok: cargo check -p session-core --bins; mp-contracts built as dependen | session-core 0073 apps/Model Plane/rust/services/session-core/migrations/0073_thread_space_kind.sql | MEMORY_SPACE_CONTAINMENT on (off only for exact 0/false/off) |
| WS17-S2 | built | ec2ab2c65 | ok: cargo check -p session-core -p model-gateway --lib --bins; pnpm ex |  |  |

## Test phase (written and run at the wave end)

| cluster | written | run | passed | failed and fixed | skipped (deferred tier) | depth rows | commit |
|---|---|---|---|---|---|---|---|
| T-app-1 | 69 | 144 | 144 | 0 | 6 | 0 | 8b056eae7 |
| T-ctl-1 | 113 | 86 | 86 | 3 | 27 | 10 | 3d745f674 |
| T-ctl-2 | 38 | 24 | 24 | 1 | 14 | 5 | 55b11b0b1 |
| T-gw-1 | 78 | 78 | 78 | 0 | 0 | 4 | 61786cfbb |
| T-gw-2 | 104 | 104 | 104 | 1 | 1 | 3 | 0ba304019 |
| T-gw-3 | 68 | 68 | 68 | 0 | 0 | 0 | 629821e0a |
| T-idn-1 | 64 | 59 | 59 | 5 | 5 | 7 | bc5811772 |
| T-plat-1 | 103 | 100 | 100 | 0 | 3 | 0 | 3510b064c |
| T-run-1 | 36 | 36 | 36 | 1 | 0 | 1 | 4f6dc0bad |
| T-sc-1 | 61 | 35 | 35 | 0 | 26 | 5 | ad88eff36 |
| T-sc-2 | 141 | 118 | 118 | 7 | 23 | 8 | 3da2fd11e |
| T-sc-3 | 49 | 34 | 34 | 0 | 15 | 5 | 9a7b3a04d |
| T-spa-room-1 | 203 | 545 | 542 | 11 | 9 | 3 | d69eab4d1 |
| T-spa-room-2 | 210 | 198 | 198 | 5 | 12 | 5 | 9934d5fda |
| T-spa-room-3 | 100 | 100 | 100 | 3 | 0 | 0 | 70bfb4111 |
| T-spa-shell-1 | 201 | 447 | 444 | 4 | 3 | 1 | 25e4b67df |
| T-spa-shell-2 | 90 | 90 | 90 | 2 | 0 | 0 | 16ce201a6 |
| **total** | 1728 | 2266 | 2260 | 43 | 144 | 57 | |

## Numbers claimed (migrations, proto tags, flags)

```
kind | number | slice | file
flag | VITE_SPACE_THREAD_PANE_UI (build-time, dev default on, prod off) | WS01-S7a | apps/Frontend Plane/verevonv3/src/features/spaces/lib/space-thread-deep-link.ts
migration | user-core 033 | WS03-S1 | apps/Control Plane/user-core/migrations/033_space_effect_policy_events.up.sql
proto-file | space_room.proto (service SpaceRoomReadService, empty) | P-SC1 | apps/Model Plane/proto/model_plane/v1/space_room.proto
flag | SPACE_ORG_ROOM_RECONCILE (Convex env, dev compose default 1, startup.sh/production default 0) | WS04-S1 | apps/Application Plane/convex-core/convex/spaceMembershipSync.ts
flag | AUDIT_READ_ROLE_ENFORCEMENT (audit-core env, default enforce; report/off refused when ENVIRONMENT=production) | WS05-S1 | apps/Control Plane/audit-core/internal/api/read_scope.go
migration | integration-corev2 0014 | WS05-S2 | apps/Ingestion Plane/integration-corev2/internal/db/migrations/0014_scim_stub_payload_redaction.sql
flag | SESSION_CORE_SPACE_ZDR (session-core env, observe default; only exact "enforce" refuses) | WS06-S2 | apps/Model Plane/deploy/docker-compose.yml
flag | SPACE_DENIALS_TYPED (gateway env, dev default on, production "false" in compose override; APP_ENV=production unset is off) | WS03-S5 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
flag | SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH (model-gateway env, default false = allowlist active; 1/true restores pass-through) | WS07-S9a | apps/Model Plane/rust/services/model-gateway/src/room_metadata.rs
migration | session-core 0071 | WS01-S1 | apps/Model Plane/rust/services/session-core/migrations/0071_message_post_kind.sql
proto-tag | sessions.proto AppendMessageRequest.post_kind=19, SessionMessage.post_kind=11, ListConversationRequest.include_human_posts=9, ThreadSummary.reply_count=23, last_author_subject_id=24 | WS01-S1 | apps/Model Plane/proto/model_plane/v1/sessions.proto
flag | SPACE_POLICY_ADMIN_WRITES (user-core env, strict true, compose ${:-false}) | WS03-S3 | apps/Control Plane/docker-compose.yml
migration | session-core 0072 | WS02-S1 | apps/Model Plane/rust/services/session-core/migrations/0072_space_event_log.sql
flag | SPACE_REALTIME_EMIT (session-core env, code default false, dev compose true, production+release compose "false"); also SPACE_EVENT_LOG_RETENTION_DAYS, SPACE_STREAM_DB_POOL, SPACE_STREAM_MAX_STREAMS | WS02-S1 | apps/Model Plane/deploy/docker-compose.yml
scope | directory:read:self (verevon-gateway credential on user-core; kill switch for space_people_search_api) | WS04-S3 | apps/Control Plane/docker-compose.yml
migration | session-core 0073 | WS17-S1 | apps/Model Plane/rust/services/session-core/migrations/0073_thread_space_kind.sql
flag | MEMORY_SPACE_CONTAINMENT (session-core env, containment ON unless exactly 0/false/off) | WS17-S1 | apps/Model Plane/rust/crates/mp-contracts/src/space_memory.rs
flag | SPACE_POLICY_ADMIN_API (gateway env, SpaceFeatureFlags.policy_admin_api, dev default on, production "false" in compose override; APP_ENV=production unset is off) | WS03-S4 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
migration | user-core 034 | WS04-S4 | apps/Control Plane/user-core/migrations/034_space_membership_source_and_events.up.sql
scope | spaces:membership:act (verevon-gateway credential) and spaces:membership:read (application-space-lifecycle) on user-core | WS04-S4 | apps/Control Plane/docker-compose.yml
flag | SPACE_AUDIENCE_RECONCILE (space_audience_reconcile_sync) | WS04-S5 | apps/Application Plane/convex-core/startup.sh
migration | user-core 035 | WS04-S7 | apps/Control Plane/user-core/migrations/035_space_archived_state.up.sql
flag | space_room_archive_api (gateway SpaceFeatureFlags, not built in this Control leg); Control routes dark by scope spaces:membership:act | WS04-S7 | apps/Control Plane/user-core/internal/http/routes_space_archive.go
flag | SPACE_MEMBERSHIP_WRITER_API (gateway env, SpaceFeatureFlags.membership_writer_api, dev default on, production "false" in compose override; APP_ENV=production unset is off) | WS04-S6 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
proto-file | space_events.proto (service SpaceEvents; StreamSpaceEventsRequest/SpaceEventFrame/SpaceEventRecord, no sessions.proto change) | WS02-S3 | apps/Model Plane/proto/model_plane/v1/space_events.proto
golden | space-events.json (+ .sha256, GOLDENS row) | WS02-S3 | apps/Model Plane/api/contracts/space-events.json
flag | SPACE_REALTIME_STREAM (gateway env, SpaceFeatureFlags.realtime_stream, dev default on, production "false" in compose override) | WS02-S4 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
flag | SPACE_HUMAN_POSTS_CORE (session-core env, compose ${:-0}, dev override "1"; member append path only, never an authorization input) | WS01-S3 | apps/Model Plane/rust/crates/mp-contracts/src/space_human_posts.rs
migration | session-core 0085 | WS07-S9 | apps/Model Plane/rust/services/session-core/migrations/0085_messages_space_artifact_index.sql
proto-rpc | space_room.proto SpaceRoomReadService.ListSpaceArtifacts (ListSpaceArtifactsRequest/Response, SpaceArtifactSummary) | WS07-S9 | apps/Model Plane/proto/model_plane/v1/space_room.proto
flag | SPACE_HUMAN_POSTS_API (model-gateway env, compose ${:-0}, dev override "1"; gates POST /v1/threads and POST /v1/threads/:id/messages; never an authorization input) | WS01-S4 | apps/Model Plane/rust/services/model-gateway/src/space_routes/posts.rs
flag | SPACE_HUMAN_POSTS_API, SPACE_THREAD_PANE_UI, SPACE_SEND_IDEMPOTENCY_API, SPACE_CONNECTIVITY_BANNER_UI (gateway env, SpaceFeatureFlags.human_posts_api/thread_pane_ui/send_idempotency_api/connectivity_banner_ui, dev default on, production "false" in compose override; APP_ENV=production unset is off; reported as context features.*, never an authorization input) | WS01-S5 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
action | spaces.create_post, spaces.reply_in_thread (human-only, ownerPlane model, closed in model-access.ts) | WS01-S5 | apps/Frontend Plane/verevonv3/src/shared/actions/domains/spaces/ws01.ts
flag | SPACE_ROOM_DETAILS_UI (gateway env, SpaceFeatureFlags.room_details_ui, dev default on, production "false" in compose override; APP_ENV=production unset is off; reported as context features.room_details, never an authorization input) | WS04-S2 | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
action | spaces.update_room_details (gateway dispatcher + handlers arm built; SPA registry fragment ws04.ts NOT yet) | WS04-S2.gw | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers_spaces/ws04.rs
flag | SPACE_ROOM_ARTIFACTS_API (gateway env, SpaceFeatureFlags.room_artifacts, dev default on, production "false" in compose; reported as context features.room_artifacts, never an authorization input) | WS07-S9.gw | apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs
```

## Wave-end compile check (2026-10-27)

- Model Plane Rust: `cargo check -p mp-contracts -p session-core -p model-gateway -p execution-core --lib --bins` green.
- Gateway: `cargo check` green.
- Go: `go build ./...` green in user-core, org-core, audit-core, integration-corev2 and Model Plane go/gen.
- SPA: `pnpm exec tsc -b` leaves 34 errors in 3 files no Space commit touched (quarry-client.ts, RemoteSupportPage.tsx, schedule-contract.test.ts); the two errors in a Space test file were fixed.
