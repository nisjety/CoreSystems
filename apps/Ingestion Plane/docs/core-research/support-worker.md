# support-worker Research Dive

> **2026-07-13 superseding update.** Source now posts canonical Application Plane `POST /api/v1/notification-requests` using a typed Control user and organization, service-specific HMAC delegation, local ZDR retention, deterministic idempotency derived from a non-reversible resolved-recipient hash, redirect refusal, a 10-second timeout, bounded errors, and strict submitted/suppressed response validation. Build/typecheck and 5/5 consumer-contract tests pass. The older `/v1/notifications` mismatch below is historical. Provider-side Novu ZDR is not proven. Notification mode defaults disabled and the Compose service remains behind `support-automation`: no current workflow supplies authoritative `organizationId`/`controlUserId`, so enabled mode cannot succeed. CSAT is deliberately unavailable because raw email requires a separate consent-aware external-contact contract. Disabled activities currently complete as skipped without a durable skip result/metric. An old pre-profile container may still be running and should be stopped only by an operator after confirming no intended workload. No trigger-to-delivery production claim is made.

Generated: 2026-07-11 (supersedes 2026-06-07)

Scope: `apps/Ingestion Plane/services/support-worker`
> **2026-07-12 classification:** TypeScript compiles, but the worker is not an active MVP capability. Default Compose now puts it behind the opt-in `support-automation` profile and removes phantom/default Zammad, AI Core, notification, and internal-key values. It must not be advertised as active until a producer, Model Plane auth/contract, notification write contract, retry/DLQ, and idempotency are proven.

Container: `support-worker` (no host port). Networks: `ingestion-net`, `inter-plane-bus`.
Image: `ingestion-plane-support-worker`, created 2026-07-07, container started 2026-07-09, `restarts=2`, state `running` (Docker reports no `(unhealthy)` because the service defines no healthcheck).

> Evidence grades used below: **[live-curl]** = confirmed by host-side curl right now; **[source-only]** = read from source/config/compose on disk; **[logs]** = from `docker logs`. NOTE: `docker logs support-worker` FAILS with `input/output error` (containerd content-store corruption — same issue that makes every `docker exec` healthcheck fail). So there is **no [logs] evidence in this pass**; runtime behavior is inferred from container state + source + network topology + curl to downstream targets.

## Bottom Line

`support-worker` is a **real, complete Temporal worker** (not a scaffold) that pairs a Temporal worker with a NATS→Temporal bridge for Zammad support-ticket automation. The code is clean — no mocks, stubs, TODOs, or placeholders anywhere in `src/`. The process is alive and its two live infra dependencies (Temporal, velion-nats) are reachable.

**But it is functionally dead-ended in this deployment.** Every one of its business dependencies is either absent or mis-routed, and nothing feeds it input:

1. **No input producer.** Nothing in the monorepo publishes to `velion.support.*` (grep across `apps/` = 0 hits), and there is **no Zammad container** (`zammad-railsserver` is not in the compose file or the runtime). The NATS bridge connects, creates the `VELION_SUPPORT` stream + `support-worker` durable consumer, and then waits forever for messages that never arrive.
2. **`ai-core` does not exist.** `classifyActivity` calls `http://ai-core:8001/api/v1/reason`. No container is named or aliased `ai-core` anywhere (compose has no such service; runtime lookup count = 0). The triage workflow would DNS-fail at step 1.
3. **Zammad is unreachable AND unauthenticated.** `patchZammadActivity` targets `http://zammad-railsserver:3000` (host absent) and `ZAMMAD_API_TOKEN` is empty, so it throws `ZAMMAD_API_TOKEN is required` before it even makes a request.
4. **notification-core route mismatch.** `notify-agent`/`send-csat` POST to `notification-core:3140/v1/notifications`. notification-core is up (`/health` = 200) but that route (and `/api/v1/notifications`, `/notifications`, `/v1/notify`) returns Go's default `404 page not found` even with the internal key — so even the one reachable downstream fails at the HTTP layer.

So: the worker is up and correctly wired to its **infrastructure** (Temporal + NATS), but every **business** hop points at a support stack (Zammad + ai-core) that is not deployed here, and its notify path targets a route notification-core does not serve. It is an idle, correctly-built worker with no live end-to-end path.

This service is **unrelated to the user's headline goals** (Bring shipping-time lookup, Visma MCP) — those live in `shipping-core` / `integration-corev2`, not here.

## Runtime Shape (source-only)

Entrypoint `src/index.ts`:
- Opens a Temporal `NativeConnection` (worker) and a gRPC `Connection` (client) to `TEMPORAL_ADDRESS`.
- Creates a `Worker` on task queue **`support-task-queue`**, `workflowsPath = ./workflows`, activities from `./activities`.
- Starts the NATS bridge, then blocks on `worker.run()`. Handles SIGTERM/SIGINT graceful shutdown (drain NATS, shutdown worker, close connections).

`src/nats-bridge.ts`:
- Connects to `VELION_NATS_URL` with token auth and **bounded exponential backoff** (2s→30s, infinite retries) so a cold-boot `ENOTFOUND velion-nats` does not crash-loop the worker. (Added in commit `25445bb7`.)
- `ensureStream` creates JetStream stream `VELION_SUPPORT` (subjects `velion.support.>`) if missing; `ensureConsumer` creates durable pull consumer `support-worker` (filter `velion.support.>`, explicit ack, deliver-new). Both idempotent.
- Consumes messages, JSON-parses each as a `ZammadTicketEvent`, routes by subject, `ack()` on success / `nak()` on throw.

`src/workflows/*`:
- `triageTicket` (triage.ts): `classifyActivity` → `patchZammadActivity` (write AI metadata) → if `confidence > 70` `patchZammadActivity` (set group) → `notifyAgentActivity`. Activities proxied with 30s start-to-close, 3 retries.
- `slaCountdown` (sla.ts): durable sleep until `deadline − 30min` → `sla.warning` notify → sleep until deadline → tag ticket `sla:breached` → `sla.breach` notify.
- `csatSurvey` (csat.ts): durable `sleep('2h')` → `sendCsatActivity`. **Exported and registered, but never started by anything** (see below).

`src/activities/*`:
- `classify.ts` → POST `AI_CORE_URL/api/v1/reason` (Model Plane "reason" endpoint), parses `answer` as JSON, validates key presence only.
- `patch-zammad.ts` → PATCH `ZAMMAD_API_URL/api/v1/tickets/{id}` with `Token token=` auth; hard-fails if token empty.
- `notify-agent.ts` → POST `NOTIFICATION_CORE_URL/v1/notifications` with `X-Internal-Api-Key`.
- `send-csat.ts` → POST `NOTIFICATION_CORE_URL/v1/notifications` (type `csat.survey`).

## Subject Routing (source-only)

Bridge `handleMessage` handles:
- `velion.support.ticket.created` → start `triage-{id}` workflow, and `sla-{id}` if `sla_deadline` present.
- `velion.support.article.added` → external replies reset SLA (terminate old `sla-{id}`, start `sla-{id}-{ts}`).
- `velion.support.ticket.assigned` → direct `notifyAgentActivity` (no workflow).
- `velion.support.ticket.updated` / `velion.support.sla.breach` → comment says "handled downstream"; there is **no handler**, so they fall through and are `ack()`'d and dropped.
- **No `ticket.closed` handler** → `csatSurvey` is never triggered. CSAT is dead from the bridge's perspective.

## Live Verification (this pass)

- **[live-curl]** notification-core `http://127.0.0.1:3140/health` = **200** (host is up). `POST /v1/notifications`, `/api/v1/notifications`, `/notifications`, `/v1/notify` (with the internal key) all = **404 `page not found`** (Go default). support-worker's notify target route does not exist as coded → notify/CSAT activities would throw. (notification-core's true route surface is Application Plane and out of scope to fully map, but the coded path is wrong.)
- **[source-only]** `docker ps -a` + `docker network inspect`: **no `ai-core`** container/alias (lookup count 0); **no `zammad`** container/alias; compose defines neither service.
- **[source-only]** Temporal reachable: `ingestion-temporal` carries alias `temporal` on `ingestion-net`; support-worker is on `ingestion-net` → `temporal:7233` resolves. (ingestion-temporal shows `(unhealthy)` only because its exec-based healthcheck fails under the containerd corruption; the process serves traffic.)
- **[source-only]** velion-nats reachable: alias `velion-nats`/`nats` on `inter-plane-bus`; support-worker on `inter-plane-bus`; `VELION_NATS_TOKEN` set → bridge connects and provisions stream/consumer.
- **[source-only]** notification-core reachable at network level (alias on `inter-plane-bus`), but see route 404 above.
- **[source-only]** No producer of `velion.support.*` anywhere in `apps/` (grep = 0), and no Zammad→NATS publisher exists.
- **[logs]** UNAVAILABLE — `docker logs support-worker` returns `input/output error` (log blob corrupted). Could not observe actual worker/bridge log lines.

## Config / Env (source-only)

`src/config.ts` validates env via Zod (all URLs + `INTERNAL_API_KEY` min 1). Running container env (from `docker inspect`) matches compose:
- `TEMPORAL_ADDRESS=temporal:7233`, `TEMPORAL_NAMESPACE=default`
- `VELION_NATS_URL=nats://velion-nats:4222` (+ token set)
- `ZAMMAD_API_URL=http://zammad-railsserver:3000`, **`ZAMMAD_API_TOKEN=` (empty)**
- `AI_CORE_URL=http://ai-core:8001` (**phantom host**)
- Notification URL is configured; the legacy runtime has a credential present. Its value is intentionally not reproduced and must be rotated if exposure is suspected. Changed source uses a dedicated support notification token and rejects legacy shared-key auth.

All env is valid *format*, so config parses and the process does **not** crash at startup — which is why it sits `running` for 2 days despite every business dependency being broken.

## Mocks / Stubs / Placeholders

- **None in `src/`.** Grep for `TODO|FIXME|mock|stub|fake|placeholder|not implemented|dummy|hardcod|xxx` = 0 matches. The activities make real `fetch` calls to real (if absent) hosts. This is honest code pointed at an undeployed backend, not fake code.

## Bugs / Warnings / Cleanup

- **[source-only] CRITICAL – broken business wiring:** `ai-core` and `zammad-railsserver` do not exist in this deployment; `ZAMMAD_API_TOKEN` is empty; notify route 404s. No end-to-end path can succeed even if a `ticket.created` event were injected.
- **[source-only] HIGH – no input producer:** nothing publishes `velion.support.*` and no Zammad instance exists, so the bridge is a consumer with no producer. The whole service is inert.
- **[source-only] MEDIUM – `csatSurvey` never invoked:** exported/registered but no bridge route starts it (no `ticket.closed` handling). Dead workflow path.
- **[live-curl] MEDIUM – notify path route mismatch:** `/v1/notifications` is not served by notification-core (404 with key). If a notify ever fired, it would throw.
- **[source-only] LOW – weak AI-response validation:** `classifyActivity` casts `parsed as ClassifyResult` after checking key *presence* only, not enum validity. An out-of-set `team` yields `resolveGroupName(...) === undefined` → `patchZammad group: undefined`.
- **[source-only] LOW – no tests:** zero `*.test.ts` in the service; 0% coverage.
- **[source-only] LOW – stale committed `dist/`:** `dist/nats-bridge.js` (built Jun 17 02:00) predates `src/nats-bridge.ts` (Jun 17 10:42) → committed build output does not reflect current source. Harmless in Docker (image rebuilds from `src`), but `npm start` locally would run stale code. Committed `node_modules/` also adds workspace noise.
- **[source-only] INFO – no source/image drift for this service** (unlike shipping-core): `git status` for the service dir is clean; last commit `25445bb7` added the NATS stream/retry logic; the running image (2026-07-07) was built from that committed source. What runs == current source.

## Uncommitted-WIP Assessment

The **support-worker service tree has NO uncommitted changes** (`git status --porcelain` for the dir is empty). The only related uncommitted change is `apps/Ingestion Plane/docker-compose.yml` (modified), but the `support-worker` compose block matches the running container's env exactly, so there is no compose→runtime drift for this service. The heavy uncommitted work called out in the audit brief (shipping-core, integration-corev2) does **not** touch support-worker.
