# W2-U49 status: user-level receipt erasure (X-19 condition c, Control event to Quarry)

Branch w23/lane-b. Date 2026-11-03. Planes: Ingestion (quarry-runtime, quarry-edge, quarry-control). Code only, not compiled, no test run.

## Built (to scope)
- Already existed: the org erasure path (quarry-control `HandleErasureWithEdge`, edge `DELETE /v1/internal/orgs/:org/profiles`, `step_receipts` org family from W2-U41a). User events were acked as no-ops. Built the user-level gap below.
- Control Plane: no change. user-core's saga (`gdpr_erasure_saga.go` `erasureFanoutPayload`, `gdpr_erasure_store.go` `BeginErasureOperation`) already publishes `verevon.gdpr.erasure.requested` with `subject_type` `user`/`user_anonymize`, `subject_id` = user id, one event per org the user belonged to (`user_erasure_fanout` snapshot). It is a broadcast subject with no per-consumer target list, and quarry-control's durable consumer already receives these events. So `gdpr_erasure_store.go` was not touched (no registration needed).
- quarry-runtime `step_receipts.rs`: `ActorReceiptPurge` (counts: receipts, timeline_events); `StepReceiptStore::purge_for_actor(org, actor)` (default refuses with `Unsupported`, 501); in-memory implementation; `purge_actor_receipts_pg(pool, org, actor)` (feature `postgres-queue`): one transaction, `DELETE FROM quarry_browser_timeline_events` then `quarry_step_receipts` `WHERE org_id = $1 AND actor_id = $2`. Idempotent, counts only, errors carry the class only. Module doc updated (erasure is the one delete exception).
- quarry-edge `internal_routes.rs`: `purge_user_receipts` for `DELETE /v1/internal/orgs/:org/users/:user/receipts`: shared internal token (`verify_internal_token`), both path ids checked like the org id, Postgres delete over `state.queue_pool` when present (same `DATABASE_URL` as the durable receipt store), else `state.receipts.purge_for_actor`. Response: the org route's `PurgeOrgReport` shape, one `step_receipts` family, `rows_deleted` = receipts + timeline events.
- quarry-edge `routes.rs` (outside the row's file list, one route line, required to serve the handler): registered next to the profiles route.
- quarry-control `erasure.go`: `UserReceiptPurger`, `HandleErasureWithUsers` (`HandleErasureWithEdge` now delegates with users nil, so its behaviour is unchanged), `HTTPEdgePurger.PurgeEdgeUserReceipts` (+ private `deleteForCounts`), `failingEdgePurger.PurgeEdgeUserReceipts`, `UserReceiptPurgerFromEnv`, `flagOn`.
- quarry-control `subscriber.go`: `StartSubscriber` builds the user hop from the env (signature unchanged, so `cmd/control/main.go` is untouched); `handleDelivery` uses `HandleErasureWithUsers`; the purge log message is now "gdpr: erasure data purged" (it covers user events too).

## Tests written (not run)
- step_receipts.rs `user_erasure_tests`: purge_for_actor_removes_only_that_user_in_that_org_and_is_idempotent (other user, other org and a NULL-actor legacy row stay; timeline events too), purge_for_actor_refuses_empty_ids, a_store_without_erasure_refuses_rather_than_reporting_zeros.
- step_receipts.rs `user_erasure_pg_tests` (postgres-queue, DATABASE_URL-gated via `postgres_test_support::isolated_pool`): pg_purge_actor_receipts_deletes_one_user_in_one_org_and_is_idempotent.
- internal_routes.rs: path_ids_refuse_empty_padded_and_overlong_values, the_user_report_is_one_counts_only_family.
- erasure_test.go: user and user_anonymize delete that user's receipts (never the org purge), flag off keeps user events a no-op, missing ids are a no-op, hop failure is retryable not poison, redelivery repeats the idempotent call, organization event unchanged, `UserReceiptPurgerFromEnv` off by default / on uses the edge hop / on without edge fails every call, HTTP route path + escaping + bearer + counts, 501 is an error.
- Only `gofmt -d` (clean on LF copies) and `rustfmt --check` (parses; new code not rustfmt-formatted, the files already had diffs) were run. No cargo, no go build or test.

## Commits (local, w23/lane-b)
- 8597d18f8 feat(quarry-runtime): user-level step receipt erasure by org and actor (W2-U49, X-19 c)
- 301c8321b feat(quarry-edge): DELETE /v1/internal/orgs/:org/users/:user/receipts (W2-U49, X-19 c)
- 2e2f8d1c2 feat(quarry-control): user erasure events delete the user's step receipts via quarry-edge (W2-U49, X-19 c)
- this status file and the lane-b depth rows (docs commit)

## Flag and default
- `QUARRY_CONTROL_GDPR_USER_RECEIPTS` (quarry-control): off by default (1/true/on/yes turns it on). Off = user events stay acked no-ops, exactly as before. On with `QUARRY_EDGE_URL` unset = every user erasure fails and is redelivered (gap stays visible, logged at boot).
- The edge route itself is always registered, behind the internal token; nothing calls it unless the flag is on.

## Assumed decisions
- Default from the queue: a user-level erasure deletes the user's stored step receipts; subject = the initiating actor (`actor_id`, migration 0008) on the receipt.
- Browser timeline events (also keyed by org_id + actor_id) are deleted with the receipts, matching the org `step_receipts` family. Run checkpoints are not (not in scope; they hold run state, not typed text; depth row).
- `user_anonymize` deletes the receipts too: typed text cannot be anonymised in place.
- Scope per event = the event's `org_id`; user-core publishes one event per org the user belonged to, which answers the design note's open question 5.3 without a cross-tenant `WHERE actor_id = $1` statement.
- Rows with a NULL `actor_id` (written before 0008) are not attributable and stay; only org erasure and retention (W2-U49a) reach them.
- The Postgres delete is a free function called by the edge on its pool, not a `PostgresStepReceiptStore` override: `postgres_step_receipts.rs` belongs to W2-U49a in the same lane.

## MP asks
- None. No Model Plane change.

## Not verified / follow-ups
- Nothing compiled or run (owner order: code only). Compile risk points: `#[cfg(feature = "postgres-queue")]` block in `purge_user_receipts_in`, the in-memory `retain` closures, `got != edge` interface comparison in the Go test.
- Not wired in compose/env: `QUARRY_CONTROL_GDPR_USER_RECEIPTS` is not passed to the quarry-control container yet (compose not in scope).
- With the flag off, user events are acked and gone: turning the flag on later does not reach users erased before (no replay).
- RUNBOOKS / OpenAPI / CONTROL_PLANE_OWNERSHIP fan-out table not updated with the new route and the Quarry user-event consumer.
