# zammad-foundation

_Audit date: 2026-07-11. Evidence grades: [live-curl] host curl to a published port; [source-only] read from disk; [inspect] `docker ps` / `docker inspect`. Docker exec/build/logs unavailable this pass (containerd content-store corruption)._

## Current State

`zammad-foundation` is **not a runtime service**. It is a **deployment/bootstrap foundation package** for a self-hosted [Zammad](https://zammad.org) support-ticketing instance. It ships an operations guide, an idempotent provisioning CLI, example configs, and admin/test checklists. The actual runtime it provisions lives in two sibling files in the plane root: `docker-compose.zammad.yml` and `.env.zammad.example`.

Status this pass:

- [inspect] **Not deployed / not running.** No `zammad-*` container exists (`docker ps -a | grep -i zammad` returns nothing; 92 other containers are up). The Zammad stack is an opt-in, separately-launched compose project (`name: application-plane-zammad`), not part of the always-on Application Plane runtime (`docker-compose.yml` contains no Zammad service).
- [live-curl] The documented host port **8088 is squatted by `model-plane-letta-bridge-1`**, not Zammad. `curl localhost:8088/api/v1/getting_started` returns a Go-style `404 page not found` (plaintext, `X-Content-Type-Options: nosniff`), which is the Model Plane bridge, not a Zammad response. Port collision — see Findings.
- [source-only] Fully committed; **zero uncommitted WIP.** `git status --porcelain` for the package, `docker-compose.zammad.yml`, and `.env.zammad.example` is empty. Last touched only by fleet-wide build/perf commits (`fb161cc7`), not by service-specific edits.
- [source-only] Bootstrap TypeScript **typechecks clean** (`tsc --noEmit`, exit 0; Node v25.8.2, TypeScript 5.9.3, tsx 4.20.6 all present in `bootstrap/node_modules`).

## Entry Points

- Operations guide: `apps/Application Plane/zammad-foundation/README.md` (10-section stock-lean Zammad architecture + MVP setup)
- Provisioning CLI: `apps/Application Plane/zammad-foundation/bootstrap/src/bootstrap.ts` (run via `bootstrap:dry-run` / `bootstrap:apply` / `bootstrap:migrate` npm scripts)
- Group/field/webhook/trigger config: `apps/Application Plane/zammad-foundation/config/bootstrap.example.json`
- Future webhook blueprint: `apps/Application Plane/zammad-foundation/config/webhook-plan.example.json`
- Checklists: `apps/Application Plane/zammad-foundation/checklists/{admin,test}-checklist.md`
- Runtime stack (siblings, not inside the package dir):
  - `apps/Application Plane/docker-compose.zammad.yml`
  - `apps/Application Plane/.env.zammad.example`

## Runtime Stack (docker-compose.zammad.yml)

Stock-lean Zammad 7.0.0-0042 (`ghcr.io/zammad/zammad`) on its own `zammad-net`, attaching app services to external `verevon-net`:

- App roles (shared image): `zammad-init`, `zammad-railsserver`, `zammad-scheduler`, `zammad-websocket`, `zammad-nginx` (host `8088`→`8080`), `zammad-backup`.
- Dependencies: `zammad-postgresql` (postgres 17.9), `zammad-dragonfly` (Dragonfly v1.37 as Redis), `zammad-memcached`, `zammad-elasticsearch` (elasticsearch 9.3.2, `-Xms1g -Xmx1g`).
- `zammad-railsserver` / `-scheduler` / `-websocket` / `-nginx` are on `verevon-net`, so other planes reach the Zammad REST API internally at `http://zammad-railsserver:3000`.

[source-only] **No shared-DB crossing.** The stack uses its own dedicated `zammad-postgresql` inside `zammad-net`; it does not touch `application-postgres`. Cross-plane DB isolation is respected.

## Consumers (who actually uses it)

Correcting the "legacy / check if even used" framing: **within the Application Plane there are zero Zammad code references** (the Inbox is owned by `conversation-core-go`, not Zammad). But the foundation is **not dead** — it is an un-deployed-but-wired-for dependency of consumers in *other* planes, all targeting `zammad-railsserver:3000`:

- [source-only] **verevonv3 gateway (canonical frontend, Rust)** — `apps/gateway/src/domains/agents.rs` proxies Agent-Console "runtime resources" (Agents / Groups / Macros) to `ZAMMAD_API_URL` (default `http://zammad-railsserver:3000`) using `Token token=` auth. `config.rs` defines `zammad_api_url` / `zammad_api_token`. Degrades gracefully: when `ZAMMAD_API_TOKEN` is empty it returns "Set ZAMMAD_API_URL and ZAMMAD_API_TOKEN to enable live support actions" instead of erroring. Currently in that degraded state (stack not running, no token).
- [source-only] **Ingestion Plane `support-worker` (Temporal)** — `src/activities/patch-zammad.ts` exposes `patchZammadActivity`, which PATCHes `/api/v1/tickets/:id` via the Zammad REST API and throws if `ZAMMAD_API_TOKEN` is unset.
- [source-only] **`verevon` (older Next.js web app, not the v3 SPA)** — full support UI: `src/lib/clients/zammad-client.ts`, `src/lib/hooks/useZammad.ts`, and ~10 `src/app/api/support/*` routes (tickets, articles, agents, groups, macros, reports, summarize, sentiment, quick-replies).
- [source-only] **`verevonv2` (deprecated)** — `src/app/api/support/_lib/zammad.ts`.

So `zammad-foundation` is the provisioning substrate for a Zammad instance those consumers expect; it is "foundation/legacy" relative to `conversation-core-go` (the live v3 Inbox), but it is a real, referenced deployment aid, not abandoned code.

## Bootstrap CLI behavior [source-only]

`bootstrap.ts` is genuine, functioning REST-sync code (not a stub): fetch-based `GET`/`POST`/`PUT` against the Zammad API with `Token token=` auth, idempotent create-or-update by name for groups, ticket object attributes (8 custom fields incl. `ai_*`), webhooks, and triggers.

- Dry-run by default; `--apply` writes; `--apply --execute-migrations` runs `object_manager_attributes_execute_migrations`.
- On dry-run, unresolved webhook IDs are `-1` and dependent triggers are skipped with a warning — correct guard, not a fake.
- Reads `ZAMMAD_BASE_URL` / `ZAMMAD_TOKEN` (required, fails fast) and optional `ZAMMAD_WEBHOOK_ENDPOINT` / `ZAMMAD_WEBHOOK_TOKEN` overrides.

## Stub / Mock / Placeholder / Unused Audit

No genuine fakes, mock runtimes, or fake-data paths. All grep hits (`dry-run`, `placeholder`, `change-me`) are legitimate:

- `dry-run` messaging and the `-1` webhook-ID placeholder — **honest guard** in an idempotent provisioning tool.
- README/`.env.zammad.example` "future webhook placeholders" — **documentation placeholders** for a deferred integration.
- `change-me` values in `.env.zammad.example` and `bootstrap/.env.example` — **example env defaults**, expected for a template.

## Findings

1. **[live-curl / inspect] Host-port collision (medium).** `.env.zammad.example` sets `ZAMMAD_EXPOSE_PORT=8088`, but 8088 is already bound by `model-plane-letta-bridge-1` on this host. Running the Zammad stack with the example defaults would fail to bind `zammad-nginx`. Override `ZAMMAD_EXPOSE_PORT` before deploying, or reconcile the Model Plane bridge port.
2. **[source-only] Webhook receiver does not exist + example is `active:true` (medium footgun).** `config/bootstrap.example.json` defines webhook `verevon-support-events` → `http://integration-core:3026/api/v1/webhooks/zammad` with `active:true` and three `active:true` triggers (ticket create/update, article create). There is **no `integration-core` service** in the repo (only `integration-corev2`, which has no `webhooks/zammad` route). Running `bootstrap:apply` verbatim would register live triggers firing into a non-existent endpoint. This contradicts the README (§3: "Initially disabled until receiver exists") and `webhook-plan.example.json` (`enabled:false`). Recommend flipping the example webhook/triggers to `active:false` until the receiver is built, and aligning the two config files.
3. **[source-only] Webhook `ssl_verify` defaults to false.** `sslVerify:false` in the example config and `?? false` in `buildWebhookPayload`. Fine for internal `verevon-net`; revisit before any non-internal endpoint.
4. **[inspect] Heavy footprint.** Elasticsearch 9.3.2 (1 GB heap) plus its own Postgres 17.9, Dragonfly, and Memcached; requires host `vm.max_map_count=262144`. This is why it is a deliberate opt-in stack rather than always-on — consistent with the README.

## Notes

Treat `zammad-foundation` as a **support-stack provisioning foundation and deployment aid**, not an always-on peer of `conversation-core-go` or `notification-core`. It is not running, has no live health endpoint of its own this pass, and its intended port is currently occupied by an unrelated Model Plane container. Its consumers (verevonv3 gateway Agent Console, Ingestion `support-worker`, the `verevon` web app) are correctly written to degrade when the Zammad instance and token are absent, which is the present state.
