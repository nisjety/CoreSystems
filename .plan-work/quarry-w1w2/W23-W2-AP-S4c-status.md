# W2-AP-S4c status: quarry schedule notice types in notification-core (WS15.S4c, D17) (lane b, branch w23/lane-b)

## Built / partly

| Scope item | State | Where |
|---|---|---|
| Register the quarry schedule notice types (channel-config rows) | Built | `apps/Application Plane/notification-core/migrations/016_register_quarry_schedule_notifications.up.sql` (`_default`, in-app on, email off) and `.down.sql` |
| Allowlist entry for quarry-control | Built, extended | `internal/http/handlers.go`, `isNotificationTypeAuthorized`: `change_detected`, `quarry.schedule_run_failed`, `quarry.schedule_run_skipped` |
| Request switch case for quarry-control | Built (gap found) | `handlers.go`, `CreateNotificationRequest` switch. Without it every quarry-control request fell to `default` and got 403, change_detected included |
| Notice principal = the schedule creator | Built | `quarryNoticeRefusal`: the recipient must equal the delegated `x-user-id`, the same rule as verevon-gateway |
| Title-only under ZDR | Built | `quarryScheduleNoticeTitleOnly`: a ZDR schedule notice carries a non-blank title and nothing else. The stored request keeps no payload, as the existing ZDR path already does |
| Closed standard payload (no URL) | Built | `quarryScheduleNoticeKeys`: title, body, schedule_id, schedule_kind, reason, run_id; string values only |
| Tests | Built, not run | `internal/http/handlers_test.go` (new) and `internal/http/quarry_control_authz_test.go` (updated) |

What already existed: the quarry-control allowlist for `change_detected` (1f1062a97) and the space-attention switch pattern. Partly built: none.

## Commits (lane b, local, not pushed)

- `3bc90ca30` feat(notifications): register quarry schedule notice types (migration 016)
- `0f23a9867` feat(notifications): accept quarry-control schedule notices for the delegated user
- `2db9481c9` test(notifications): cover quarry-control notice rules and the migration
- The commit that adds this file: docs(plan-work): W2-AP-S4c status and lane b depth rows

## Code only, not compiled

- Nothing was compiled or tested (owner order). The Go files were checked with `gofmt` only (parse and format, on LF copies): clean. The integration step must run `go vet ./...` and `go test ./internal/...` in notification-core.
- The migration test reads `../../migrations/...` from the package directory, which is the correct relative path for `go test` in `internal/http`.
- No PG-gated test was written. The migration test is static and runs on the host. A DSN-gated round trip (apply through 016, check the two rows, run the down file) is the complement and is not in this unit.

## Flag and default

- No new flag. The types stay inert until quarry-control emits them.
- Delivery stays off without `NOTIFICATION_QUARRY_CONTROL_SERVICE_TOKEN`. `internal/config/config.go` deletes an empty key, so every quarry-control call is refused with 401 (unknown delegation principal).
- The switch case also changes `change_detected`: it was refused with 403 and is now accepted in standard retention. That is a bug fix, since 1f1062a97 meant it to deliver, so no flag. Once a stack holds the token, change notices start to arrive.
- The other rules only refuse. They are leak fixes, so no flag applies.

## Assumed decisions

1. Two types, not three. D17 (`docs/plans/quarry-browser-build-plan-2026-10-14.md`, X-9 and the WS15.S4 detail) names `quarry.schedule_run_failed` and `quarry.schedule_run_skipped`. The queue's "(skipped_zdr, failed, paused)" is read as the situations these two notices cover: `skipped_zdr` is a skip reason on the skipped notice, and the schedule pause that comes with it belongs to that notice, because S4 emits no separate paused event. There is no `quarry.schedule_paused` type. Adding one later is one allowlist line and one migration row.
2. Migration number 016, as instructed. R-32 (plan, section 2.3) reserves 016 for E3.2 `016_register_approval_requested`, and the Space plan lists 016 for `web_push_subscriptions` and 017 for `scheduled_run_outcome`. The runner tracks migrations by file name, so the clash is a naming problem, not a runtime one. Re-check the number on the PR day.
3. Notice principal: the recipient must equal `x-user-id` on the delegation. The emitter must sign for the schedule's `created_by`. This reuses the gateway rule and needs no schedule lookup in notification-core.
4. Every quarry-control notice needs an idempotency key. Change notices already send the event id, so a replayed batch adds nothing.
5. Retention must be explicit, `standard` or `zdr`, with no default. This matches space-attention.
6. `change_detected` is standard only. A ZDR change notice is refused with 403.
7. Standard schedule payload: a closed key set, with `title` required and `body`, `schedule_id`, `schedule_kind`, `reason` and `run_id` optional. Values must be strings. No URL key is allowed.
8. ZDR schedule payload: `title` only, non-blank.
9. In-app on, email off. Labels and descriptions are in English, as in migrations 013 to 015.
10. `handlers_test.go` did not exist. It is created because the scope names it. `quarry_control_authz_test.go` is not in the scope list, but it is updated: its closed-set assertion (quarry-control may send exactly `change_detected`) would fail on the new types.

## Ask for the quarry-control emitter (not in this unit)

- Emit only `quarry.schedule_run_failed`, `quarry.schedule_run_skipped` and `change_detected`.
- Sign with `x-user-id` = the schedule's `created_by`, and set the recipient to the same user with kind `user`. A mismatch is refused with 403.
- Send `retention_mode` explicitly. Under ZDR send `{title}` only. Under standard send only the closed keys above, never a URL.
- Send `idempotency_key` as a stable key per run and outcome (for example run id plus kind), so a replayed batch adds nothing.
- Put the emitter behind its own Quarry-side flag, default off (for example `QUARRY_CONTROL__SCHEDULE_NOTICES=off|on`), in the emitter unit.

## MP asks

None. No Model Plane file was touched, and `MP-ASKS.md` was not appended.

## Not verified

- Compile, `go vet` and `go test`: not run (owner order). The integration compile guard owns them.
- Live delivery: not run. There is no stack and no signed call, so the `change_detected` fix is not proven end to end.
- ZDR with the async delivery worker: see finding 2. It fails with 500 when `NOTIFICATION_DELIVERY_WORKER_ENABLED=1`.

## Findings (also appended to `.plan-work/quarry-w1w2/W23-depth-lane-b.md`)

1. Bug, fixed in `0f23a9867`: quarry-control was refused for every type, because the request switch had no case and the default returned 403. change_detected could never have been delivered.
2. Open, medium: when `NOTIFICATION_DELIVERY_WORKER_ENABLED=1`, `internal/notification/service.go` refuses every ZDR request with a plain error, and the handler maps that to 500. A ZDR schedule notice would fail with 500, which the emitter cannot tell apart from an outage. Owner decision in the Application Plane; not changed here.
3. Naming, low: migration 016 clashes with the R-32 and Space-plan reservations (assumed decision 2).
4. Open, low: `change_detected` has no channel-config row, unlike the new types, so the admin list omits it. Delivery is not affected.
5. Style, fixed: the 1f1062a97 allowlist block was mis-indented and not gofmt-clean.
