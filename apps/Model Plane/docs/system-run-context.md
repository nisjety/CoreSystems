# System-Initiated Run Context

> Status: design (2026-07-15). Specifies the mechanism that lets a **headless**
> Model-Plane component (the task executor, the cron sweeper, an autonomous
> loop) start a real agent run — something that today only a user session can
> do. Implementing this unblocks **task execution**, **cron end-to-end run**,
> and the **todos driver** in one stroke.

## 1. The problem

Every agent run in the Model Plane is **user/session-scoped**. `model-gateway`
verifies a JWKS-signed delegated bearer and derives `org_id` + `user_id` from
its claims; it then forwards **per-audience delegated bearers**
(`VerifiedInferenceBearer`, `VerifiedSessionBearer`, data-plane) to the
downstream services, each of which independently verifies its own audience.

A headless component has **no user session**, so it cannot mint those bearers.
That is why:

- the **task executor** (`internal/taskexec`) can only *claim + dispatch* a
  task — it has nothing to run it with;
- **cron** creates tasks (`cron_fires`) but they never execute;
- a **todos driver** has no run to attach todos to.

All three are the same missing primitive: **a run started by the system, not a
user.**

## 2. The actor model

Introduce a **system actor**: a synthetic, per-org identity that owns
system-initiated runs.

- Subject: `system:task-runner` (never a real user id).
- Org: the target org from the dispatched work (`tasks.org_id`).
- It is audited as a service actor, distinct from any human user, so an
  operator can always tell "the system ran this" from "a user ran this."

## 3. Auth: the `task-runner` service principal

Register a service principal in Control Plane `auth-core`
(`PLANE_SERVICE_PRINCIPALS_JSON`), exactly like the existing `session-core` /
`model-gateway` principals:

```json
"task-runner": {
  "credential": "<64-hex generated>",
  "audiences": ["model-gateway", "session-core", "inference-core", "data-plane"],
  "orgIds": [],
  "allowAnyOrg": true,
  "scopes": ["run:execute", "tools:invoke", "memory:read", "memory:write"]
}
```

The runner exchanges `(x-service-id: task-runner, x-service-api-key: <cred>)` at
`auth-core` `POST /api/{audience}/internal-token` — the **same mint** session-core
already uses for Letta — passing `{ orgId, scopes, subject: "system:task-runner",
reason: "system task run" }`. It gets back a per-audience bearer whose claims
carry `org_id = <org>`, `user_id = system:task-runner`, and `is_service = true`.

**auth-core change**: `internal-token` must accept an optional `subject` for a
service principal and stamp it as the token's `user_id`/`sub` (today service
tokens have no user subject). This is the one auth-core change required.

## 4. Runtime: accepting a system run

`model-gateway` (and the downstream verifiers) must accept a run whose bearer is
the `task-runner` principal:

- Auth (`auth.rs`): a token with `is_service = true` **and** scope
  `run:execute` **and** a non-empty `org_id` is a valid **system run** — treat
  `sub` (`system:task-runner`) as the `user_id` for the run. Regular user runs
  are unchanged; only this specific service+scope shape is newly accepted.
- The invoke/RunAgent path then proceeds exactly as a user run: it re-mints the
  per-audience bearers (§1) from the same principal and forwards them. No
  downstream service needs a bespoke path — each already verifies its audience;
  they only need to accept `is_service` subjects (a one-line allowance in the
  shared `identity()` guard, gated on the `run:execute` scope).

HITL posture: system runs default to the `deployed_agent`/`ask` posture is
**wrong** here (no operator is watching a cron run). Instead a system run uses a
**policy from the task/cron record**: `auto` unless the task template opts into
`ask` (in which case the run pauses and an approval is surfaced to the org's
operators via the existing `RUN_PAUSED_FOR_APPROVAL` path).

## 5. The dispatch consumer

The `taskexec` executor already publishes `mp.v1.capability.task.dispatched`
(subject `reconcile.Subject(KindTask, ActionDispatched)`). Add a **run consumer**
— the smallest new component — that:

1. Subscribes to `mp.v1.capability.task.dispatched`.
2. Loads the task (`tasks` row: org, kind, title, description, config).
3. Mints the system bearers (§3) for the task's org.
4. Calls `model-gateway` `RunAgent` (agentic loop) with:
   - the system context (§4),
   - `content` = the task's title/description (the prompt),
   - `mode` from the task template (`auto` default).
5. Streams the run to a terminal state, then **completes the task**:
   `tasks.status = completed|failed`, `completed_at = now()`.
6. **Cron closure**: `cron_fires.task_id` already links the fire to the task; a
   trigger/update flips `cron_fires.status` to `completed|failed` when its task
   terminalizes. Cron is now end-to-end: sweeper fires → task created →
   consumer runs it → task + fire completed.

**Where it lives**: `model-gateway` is the natural home (it owns the agent
runtime + already has NATS + the orchestration client), so the consumer runs
the loop in-process without an extra network hop. Alternatively a small
`run-worker` binary that calls `model-gateway` over gRPC — same contract, more
isolation. Recommendation: start in `model-gateway` behind a
`SYSTEM_RUN_CONSUMER_ENABLED` flag.

## 6. How this unblocks todos

Once a **run exists** for system work, todos have an owner. A system (or user)
run drives todos two ways, both now possible:

- **In-run TodoWrite**: the agentic loop exposes a `todo_write` tool; when the
  model plans multi-step work it writes todos against the run's plan
  (`plan_{run_id}`, already created at `start_run`). Because `session-core`
  **owns** the orchestration store and its `append_todo`/`update_todo_status`
  already exist, the writer can be an **internal session-core call on the run's
  own transaction** — so todos do **not** need the currently-missing
  create-todo gRPC method (which would require Go `buf` regen). The tool call
  routes to a session-core handler that appends to `todos` + emits `TODO_*`
  events onto the existing run-event stream.
- **Plan steps**: `append_plan_step` (already in the store) records the plan's
  steps as the run executes, so the Agent Run Console shows real plan progress.

This is why the run context unblocks todos "in one stroke": the blocker was
never the store (writes exist) — it was the absence of a run to attach them to
and a caller inside session-core's reach. The system run provides both.

## 7. Build order (each independently shippable)

1. **auth-core**: `task-runner` principal + optional `subject` on
   `internal-token`. (Config + ~10 LOC.)
2. **shared auth guard**: accept `is_service + run:execute + org_id` as a system
   run; map `sub` → run `user_id`. (`identity()` allowance.)
3. **run consumer** in `model-gateway` (flagged off) — subscribe, mint, RunAgent,
   complete task + cron_fire.
4. **todos**: `todo_write` tool → session-core internal `append_todo` + `TODO_*`
   events; Agent Run Console already renders them.
5. Flip `TASK_EXECUTOR_ENABLED=true` + `SYSTEM_RUN_CONSUMER_ENABLED=true`
   together; cron + tasks now execute end-to-end.

## 8. Security notes

- The `task-runner` principal is the **only** identity that can start a run
  without a user; its credential is a first-class secret (rotate like the
  others) and its scopes are the floor needed to run tools.
- Every system run is audited under `system:task-runner` + its org, never a
  human — so a cron/task run is always distinguishable in the audit log.
- `allowAnyOrg` is required (the runner serves every tenant) but the org is
  **always** taken from the durable task/cron row, never from the event payload
  alone, so a forged event cannot cross tenants (the consumer re-reads the task
  by id under its own org filter).
- System runs honor the same ZDR, secret-scrub, and HITL gates as user runs —
  they are ordinary runs with a synthetic actor, not a privileged bypass.
