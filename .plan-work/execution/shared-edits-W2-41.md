## WS18-S4
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/rust/services/session-core/src/dreaming.rs: in load_pending_messages, after the line `AND m.copied_from_message_id IS NULL \` (about line 1318), add `AND NOT (t.space_id IS NOT NULL OR t.origin IN ('space', 'agent_run')) \` (K3-5; alias t is threads). Optionally repeat the predicate in load_preceding_context (about line 1386). Add the ignored DB test space_thread_messages_never_reach_user_memory beside the existing Dreaming DB tests and run it through the harness.

## WS18-S4
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/docker-compose.yml: after the VITE_PLAN_EXECUTE_ENABLED line (about line 349), add `      VITE_CHAT_PROJECTS_ENABLED: ${VITE_CHAT_PROJECTS_ENABLED:-false}`. The Dockerfile has no VITE_* build argument, so the staging image path needs the value at build time.

## WS18-S4
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/.env.example: add VITE_CHAT_PROJECTS_ENABLED=false beside VITE_PLAN_EXECUTE_ENABLED (about line 64).

## WS18-S4
shared-edit C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Frontend Plane/verevonv3/src/vite-env.d.ts: add `readonly VITE_CHAT_PROJECTS_ENABLED?: string;` beside VITE_PLAN_EXECUTE_ENABLED (optional; type only).

