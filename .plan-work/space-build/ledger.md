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
