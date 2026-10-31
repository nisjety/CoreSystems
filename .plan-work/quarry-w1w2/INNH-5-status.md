# INNH-5: Change-webhook subscription API (create, list, delete) in control, plus the gateway/UI contract

Status: done in tests, dark. Both switches default off; nothing is deployed, no live flag was touched.
Lane: B (control/orchestrator), branch innh/lane-b. Plan home: E3.5, WS15. Bundled with the tenancy work of INNH-13(b).
Checked against the code first (2026-10-31); the "What existed" section below is what the tree held, the rest is what was built.

## What existed

EMISSION (edge `change_webhook.rs`), RECEIVER (control `change_webhook.go`), the generic webhook store, and the DISPATCHER (HMAC, SSRF-guarded SafeClient, retry 1s/5s/30s/5m/1h, dlq at attempt 5) were built and tested. The gap was the layer between them: `webhooks` had no `org_id`, `fanoutWebhooks` listed every webhook of every org, the old `/v1/webhooks` CRUD was global and returned secrets in cleartext, org erasure excluded the tables, edge had no route, and CONTRACTS section 4 described a wrapper and an "org secret" that never existed.

## What was built

Control (Go, `services/quarry-control`)
- Migration `025_webhook_subscriptions.sql`: `org_id`, `secret_hint`, `description`, `created_by` on `webhooks`; `org_id`, `event_type`, `dedupe_key` on `webhook_deliveries`; indexes for the org reads, the retention sweep and the per-subscription dedupe. Idempotent; a legacy row keeps `org_id ''`.
- TENANCY FIRST. `fanoutWebhooks(db, ev, orgID)` reads only `ListActiveByOrg(orgID)`; an empty org delivers to nobody; an empty `events` list matches nothing (it used to match everything); `*` matches that org's events only. Org of an event: the signed `org_id` for the change webhook, the run's job (`GetByRunID`) for the event batch; unknown run = nobody. The dispatcher re-checks (`wh.OrgID == "" || != delivery.OrgID` = `failed`), which also kills deliveries queued before 025. A blocked (SSRF) target is now dead-lettered at once instead of retried five times.
- One change reaches control by two paths (the edge webhook and the run event batch; the orchestrator emits a third). Deliveries carry a `dedupe_key` (org, url, new baseline id), unique per subscription, so a subscriber gets it once when the baseline id is known.
- `store.WebhooksStore` / delivery store: `CreateCapped` (count, duplicate check and insert under an advisory lock), `GetByOrg`, `ListByOrg`, `CountByOrg`, `DeleteByOrg`, `UpdateByOrg`, `ListActiveByOrg`; deliveries `GetByOrg`, `ListByWebhook`, `DeleteTerminalBefore`. Memory and Postgres implementations answer one conformance suite (`storetest.WebhooksConformance`).
- `/v1/webhook-subscriptions` (create, list, get, patch, delete, rotate-secret, deliveries, delivery retry) behind `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS` (not mounted when off). Secret = `whsec_` + 64 hex, minted in control, returned by create and rotate only; `store.Webhook.Secret` is `json:"-"`; the views have no secret field. URL: https only, no userinfo/fragment, 2048 max, every resolved address through `egress.Guard.ResolveAndVet`. Events: closed list `[change_detected]`. Cap 10 per org (429), duplicate url+events (409). `last_error` shown as a closed token.
- Legacy `GET/POST /v1/webhooks`, `GET /v1/webhooks/{id}` and `/v1/webhook-deliveries...` answer 410 (not flag-gated; nothing called them); `POST /v1/webhooks/change` unchanged.
- GDPR: `PurgeOrg` (memory and Postgres) deletes the org's deliveries and webhooks and reports them in `PurgeResult`. Delivery retention: hourly sweep of terminal rows older than 14 days (`QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS`), started only while the control switch is on.

Edge (Rust, `quarry-edge`, `quarry-core`)
- `webhook_subscription_routes.rs`: the eight routes as a passthrough, behind `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` (501 off, read per request). Org from the token; unknown body fields are a 400; every answer is rebuilt from a typed view (`quarry_core::resources::WebhookSubscription*`), so a stray secret in a control read cannot leave the edge; control's validation `field`/`reason`/`limit` are passed through and nothing else of its body; a plain 404 from control (switch off there) becomes a 501 naming the control switch; ids and list queries are checked before control is called.
- `control_client::send_raw` (any method, empty bodies, 204, returns status and body) for these routes.
- ZDR: create, patch and delivery retry are refused for a ZDR token (403, `details.reason: webhook_off_process`) before control is called; reads, delete and rotate-secret stay open (wind-down). Service principals: 403 by the route scope and again in the gate. Matrix row `webhook_subscriptions` (Refused, Enforced) added, golden JSON + sha256 + `ZDR_MATRIX.md` regenerated.

Docs: `openapi.yaml` (five paths, eight operations, schemas, tag `webhooks`), `CONTRACTS.md` section 4 rewritten to the real delivery contract plus a "Webhook subscriptions" section, `CHANGE_TRACKING.md`, `RUNBOOKS.md` (enable, rotate, retry, erase, two owner decisions), `ERROR_REASONS.md`, route pin 112 to 117 in `openapi-route-allowlist.json`, a docs-contract test for the two switches.

## Commits (branch innh/lane-b)

- d65b5b907 feat(quarry-control): org-scoped webhook subscriptions, tenant-safe fan-out, legacy CRUD retired (INNH-5)
- b39ff7d02 feat(quarry-edge): webhook subscription routes behind QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS, ZDR refusal on create (INNH-5)
- docs commit (openapi, CONTRACTS, RUNBOOKS, ERROR_REASONS, CHANGE_TRACKING, docs-contract test, status, progress, depth): the next commit on the branch

## Tests

Go (`GOWORK=off go test -count=1 ./...` in services/quarry-control): whole module green. 31 new top-level tests plus the 10 existing dispatcher/change-webhook tests adapted: subscriptions (secret once and never on read, org isolation on every route, URL table of 17 cases, events allow-list, cap and duplicate, patch/rotate/delete, paging, deliveries and retry), `DeliveryErrorToken`, flag default off, legacy routes 410 while `POST /v1/webhooks/change` still answers, fan-out isolation (two orgs with `*`, legacy org '' row, inactive, `run_completed`-only and empty-events subscriptions, unknown owner = nobody, run event uses the run's job org, one change by two paths = one delivery), dispatcher (legacy row and foreign-org delivery refused, signature recomputed from the documented recipe, backoff 5s/30s/5m/1h then dlq at attempt 5, SSRF loopback target dead-lettered at once with the production SafeClient), retention sweep, `WebhooksConformance` on the memory store.
Postgres: the same `WebhooksConformance` plus cascade-on-delete, PurgeOrg and migration 025 re-applied over legacy rows ran against a scratch `postgres:16-alpine` container (started and removed by this lane, port 55439): whole `store/pg` package green.
Rust (Docker, lane b): `quarry-edge` lib `webhook_subscription_routes` 11 passed; `tests/webhook_subscriptions.rs` 10 passed (flag off 501 without a control call, create forwards only checked fields with the token org and returns the secret once, bad/unknown body 400 before control, typed reads drop a stray secret, patch/delete/rotate/retry, foreign and malformed ids, control reasons pass through, control-switch-off 501, ZDR token 403 on create/patch/retry with wind-down open, service token 403); `zdr_feature_matrix` 14 and `zdr_gap_rows` 9 passed with the new row; `quarry-core` 281 passed plus the other binaries; golden regenerated with `QUARRY_UPDATE_GOLDEN=1` then rerun without it.
Node: `node --test scripts/tests/*.test.mjs` 50 passed, 0 failed (openapi route coverage with the pin 112 to 117, no-bare-null, docs-contract incl. the new switch test).

## Deviations from the plan in this file

- The old `/v1/webhook-deliveries` list/get/retry was retired as well as `/v1/webhooks` (it listed every org's payloads); the tenant replacements live under the subscription.
- A per-subscription delivery log and retry were built (the plan listed them as optional); `POST .../test` and auto-disable were not (depth file).
- The advisory-lock cap was used instead of a count-then-insert; the duplicate check is in the same transaction (a unique index on `(org_id, url, events)` was dropped because `array_to_string` is not immutable).
- ZDR create refusal is unconditional (not the observe-then-enforce mode of `zdr_refusal::refuse`): the feature is new, so there is nothing to observe.

## Not verified

- No end-to-end run: no side-by-side stack, no real receiver, no Temporal. The dispatcher is tested against an httptest receiver and the production SafeClient against a loopback target; the edge against a control double; control's Postgres store against a scratch Postgres 16 container (removed afterwards), including migration 025 over legacy rows.
- The cross-path de-duplication assumes the orchestrator's `baseline_id` equals the edge's `new_baseline.baseline_id` (depth file).
- `EGRESS_ALLOWLIST_HOSTS` / `EGRESS_ALLOWLIST_CIDRS` on live control were not read (no env files read); the runbook says they must be empty.
- Migration 025 on the live database was not run; control applies it at boot.

## Owner decisions needed

1. Secret at rest: accept plaintext in `webhooks.secret` for the first customers, or require envelope encryption before the switches go on.
2. ZDR: an org that turns ZDR on later keeps its subscriptions (they receive nothing). Deactivate them on that event?
3. Turn the switches on (control, then edge) after the control image is rebuilt; only the owner may.

## Contract for the Innhenting gateway and UI

Edge paths, bearer user token; the gateway forwards them as it does the other edge routes. Envelope `{ data, meta: { request_id } }`; errors `{ error: { code, message, details } }`.

- `POST /v1/webhook-subscriptions` body `{ url, events: ["change_detected"], description? }` -> 201 `data` = subscription + `secret` (show once, copy button, never stored by the UI).
- `GET /v1/webhook-subscriptions?limit&cursor` -> `data: { items: [subscription], next_cursor? }`.
- `GET /v1/webhook-subscriptions/{id}` -> subscription. `PATCH` body `{ active?, events?, description? }` -> subscription. `DELETE` -> 204.
- `POST .../{id}/rotate-secret` -> `data: { id, secret, secret_hint }` (show once).
- `GET .../{id}/deliveries?status&limit&cursor` -> `data: { items: [{ id, event_id, event_type, status, attempt, next_attempt_at?, last_error?, created_at }], next_cursor? }`; `POST .../{id}/deliveries/{did}/retry` -> 202 delivery.
- subscription = `{ id, url, events, active, description?, secret_hint, created_at, last_delivery?: { status, at, event_id } }`.
- Errors to render: 400 `details.field` in `url|events|description|body` with `details.reason` (list in `docs/ERROR_REASONS.md`); 403 `webhook_off_process` (hide the create button for ZDR orgs); 409 `subscription_exists`; 429 `subscription_limit` (10); 501 = feature not enabled (show "not available").
- What the customer's endpoint receives (for the docs page of the UI): `docs/CONTRACTS.md` section 4: header `X-Quarry-Signature: t=<unix>,v1=<hex HMAC-SHA256 of "<t>.<raw body>">`, body = the control event with `type: change_detected` and `payload.subject: quarry.change.detected`, 5-minute replay tolerance, retries 5s/30s/5m/1h then dlq.
