## WS22-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core service environment, add the line 'EXECUTION_CORE_SPACE_WORKSPACE_WRITE_THROUGH: ${EXECUTION_CORE_SPACE_WORKSPACE_WRITE_THROUGH:-}' (empty means off). WS22-S3 edits the same file in this batch, so coordinate the two hunks.

## WS16-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment, directly after the line SESSION_CORE_ATTENTION_SWEEP_SECONDS (about line 947), add: '      # WS16-S5: the per-user attention stream. Off by default; only the literal true turns it on.' then '      SESSION_CORE_ATTENTION_STREAM_ENABLED: ${SESSION_CORE_ATTENTION_STREAM_ENABLED:-false}' then '      SESSION_CORE_ATTENTION_TICK_SECONDS: ${SESSION_CORE_ATTENTION_TICK_SECONDS:-5}'. The default 5 matches the code (DEFAULT_TICK_SECONDS).

## WS16-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the capability-core environment, directly after the line CRON_CONTRACT_GUARDS_ENABLED (about line 1744), add: '      # WS16-S4: RUN_ATTENTION becomes a typed notification. Off by default; turn NOTIFICATION_NATIVE_TYPES_ENABLED on first.' then '      CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED: ${CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED:-false}' then '      ATTENTION_NOTIFY_COALESCE_SECONDS: ${ATTENTION_NOTIFY_COALESCE_SECONDS:-60}' then '      CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED: ${CAPABILITY_CORE_SCHEDULED_NOTIFY_ENABLED:-false}' (WS20-S7's switch in the same block, not in this slice's flip list). The 60 s default matches DefaultAttentionCoalesce. Keep every value at its default; no flip in this step.

## WS16-S7
shared-edit docs/master-ownership-matrix.md (the tracked file; match by text, not line): in the session-core row replace 'Threads, runs, checkpoints, memory index, compaction, context assembly |' with 'Threads, runs, checkpoints, plans, approvals, attention state, memory index, compaction, context assembly |'; in the orchestrator-core row (about line 130) replace 'tasks/cron, approvals, recovery |' with 'tasks/cron, recovery |'. This is the DEC-11 MASTER edit: approvals are session-core rows (CAPM line 55 already says so).

## WS16-S7
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: insert after the row beginning '| Run metadata + lifecycle' (about line 54) this row: '| Attention state: **session-core**; delivery: **notification-core** | **session-core** (attention.rs: derivation, unseen marker, RUN_ATTENTION outbox rows); **notification-core** (Application Plane: notifications, native rows, sealed Web Push subscriptions) | Postgres | derived on read; sweeper writes outbox rows; capability-core run-watch consumer | NATS run-events | source-verified, switches off |'

## WS16-S7
shared-edit apps/Model Plane/MODEL_PLANE_STATUS.md: insert before the heading '## Critical blocked behavior' (about line 250) this section: '## WS16 attention state (2026-10-31): source-verified, every switch off' then 'The attention state, the rail and badge, the typed RUN_ATTENTION events, the notification path, the per-user stream and Web Push are built behind switches that all default off: SESSION_CORE_ATTENTION_ENABLED, SESSION_CORE_ATTENTION_EVENTS_ENABLED, SESSION_CORE_ATTENTION_STREAM_ENABLED, CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED, NOTIFICATION_NATIVE_TYPES_ENABLED and NOTIFICATION_WEB_PUSH_ENABLED. No compose file turns one on, and no slice has live evidence. The attention capability stays source_only until candidate and rollback evidence exists.'

## WS16-S7
shared-edit apps/Application Plane/.env.example (optional documentation, not a flip): after the NOTIFICATION_DELIVERY_WORKER_LEASE line add NOTIFICATION_NATIVE_TYPES_ENABLED=false and NOTIFICATION_WEB_PUSH_ENABLED=false as placeholders. No VAPID key and no subscription key belongs in this file; run-application-plane.sh generates them.

## WS22-S3
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: in the gateway service environment, after line 157 (BROWSER_DO_ENABLED: ${BROWSER_DO_ENABLED:-false}), add a comment (WS22-S3: mint the sandbox-manager credential for a personal non-ZDR turn; off by default; only the literal true) and SANDBOX_THREAD_WORKSPACE_ENABLED: ${SANDBOX_THREAD_WORKSPACE_ENABLED:-false}.

## WS22-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service environment (block 191-869), after the SANDBOX_MANAGER_AUTH_AUDIENCE line (638), add SANDBOX_THREAD_WORKSPACE_ENABLED: ${SANDBOX_THREAD_WORKSPACE_ENABLED:-false}.

## WS22-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the execution-core environment (block 1246-1619; environment at 1265), add EXECUTION_CORE_THREAD_WORKSPACE: ${EXECUTION_CORE_THREAD_WORKSPACE:-false}. This is WS22-S2's pass-through, which is still missing; keep it false.

