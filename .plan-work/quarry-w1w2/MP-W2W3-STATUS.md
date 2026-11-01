# MP W2/W3 status (Model Plane rows of Quarry W2 and W3)

Branch `mp-w2w3` (worktree `mp-w2w3-350537`), cut from `quarryv2-next` at `cc8c43389`. Brief: `side-session-notes/local_6a0cd694-.../mp-w2w3-brief.md`. Specs: `.plan-work/quarry-w1w2/mp-w2w3/spec-A.md` (WS7.L, WS8, WS9, WS10, WS12.S4) and `spec-B.md` (WS11, WS16, INNH-10, W3 rows).

Rules: W2 rows are tested in process (unit, integration with a scratch Postgres where needed), with no live stack. W3 rows are built and compile-checked only, and the user tests them after the merge. Every new behaviour sits behind a flag that defaults off. Commits are local only.

Status key: built / tested / commits / flags / assumed decisions / not verified.

## W2 rows

| Row | Built | Tested | Commits | Flags (default) | Assumed decisions | Not verified |
|---|---|---|---|---|---|---|
| U03 WS8.A ledger row | yes | n/a (docs) | see git log | none | ADR-3 + X-13 taken as ratified (owner order 2026-11-01) | spike measurements (live lane) |
| U61 broker browser_grants + migration 0004 | yes (test fix) | broker `go test ./...` and `-tags integration` on scratch Postgres 16: all green after fixing stale `TestPg_MigrationIsRerunnable`; model-gateway `browser_` lib tests: see below | see git log | n/a | none | none |

U61 live check (read only, 2026-11-01): the live broker image ships `0001`, `0003` and `0004` in `/app/migrations`. The broker's database `browser_broker` on `ingestion-postgres` has the 0004 columns (`policy_mode`, `denied_domains`, `resource_id`), the widened kind, domain, parent, policy and resource checks, and the indexes `browser_grants_attach_idempotency_idx` and `browser_grants_owner_human_active_idx`. D2-140 is closed. Side finding: `session_core` on `model-plane-postgres-1` also holds an old pre-0004 `browser_grants` table, which looks like a leftover from before the broker got its own database. It is recorded in the depth file and not touched.

(Rows added as each batch lands.)

## W3 rows

(Rows added as each batch lands.)

## Assumed owner decisions (defaults taken, owner said "stop asking / just do it")

| Id | Decision | Default taken | Source |
|---|---|---|---|
| ADR-3 / X-13 | shared human + agent browser, human_browse and attach grants, bounds | ratified as designed | brief, plan X-13 |
