# W3-B2-03 progress (E3.5-8, ZDR gate on monitors), 2026-10-23

## Outcome

No code change and no commit. The E3.5-8 gate is already on the branch, built by WS4.H stage A and its follow-ups. Nothing in the slice's observe/flag scope is missing, so the unit was closed without an edit.

## What is already in the tree (verified by reading, HEAD and working copy)

| Monitor path | Gate | Where |
|---|---|---|
| `POST /v1/change/check` | `refuse_for_claims` (row `change_history`), observe by default | `crates/quarry-edge/src/change_routes.rs:82` |
| `POST /v1/change/snapshot`, `POST /v1/change/refresh` | `refuse_for_claims` (row `change_snapshot`) | `change_routes.rs:206, 260, 316, 370` |
| `POST /v1/internal/change/record` (orchestrator, declared `zdr`) | `refuse` on the body's `zdr` before any store access; emit suppression for declared ZDR | `change_routes.rs:515`, `emit_change_side_effects` at `:649` |
| `POST /v1/schedules` (preset, change monitor) | `refuse` on create, stamp forwarded | `schedule_routes.rs:262` |
| `POST /v1/schedules/{id}/unpause`, `trigger`, `backfill` | `refuse_for_claims` | `schedule_routes.rs:393, 420, 448` |
| `POST /v1/sources` with `monitor: true` | `refuse` on create, stamp forwarded | `resource_routes.rs:553` |
| `latest`, `history`, `pause`, `delete` | read or wind-down, left open by design | matrix basis |

Observe mode (flag `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` off, the default) emits `zdr.would_refuse{row, action, org_id}` and serves the request as before. Flag on returns 403 `zdr_durable_mutation_forbidden` and emits `zdr.refused`. The helper is `crates/quarry-edge/src/zdr_refusal.rs`.

Tests already present (not run by this unit): `crates/quarry-edge/tests/zdr_refusal_modes.rs` (observe and enforce, durable writes, internal record, wind-down, standard org unchanged), `zdr_posture_stamp.rs` (stamp at create, client may raise but never lower, record trusts the stamp, service principal refused), `zdr_gap_rows.rs`, `zdr_claim_floor.rs`.

Commits that built it (from `git log -- change_routes.rs` and the closeout): `03a15eb76` (WS4.H stage A helper, observe), `2a7759f6`, `3bfc6a0b`, `06959092`, `8b3c108b` (C1 posture stamp), `e32a7cf8` (V4 details), `78c9e4acc` (suppression test). The orchestrator side is in `RUNBOOKS.md`, section "ZDR posture of a scheduled change monitor" (committed at HEAD line 539), so no new RUNBOOKS section was written.

W2 U01 (`e18328f50`) touched `change_routes.rs` only for the request-id codemod (`crate::request_id::current()`). Its edit did not conflict with the gate.

## What is left (owned by other units, not built here)

- Org that turns ZDR on after a monitor is created: the monitor keeps firing and storing baselines, because the edge trusts the create-time stamp. The fix is a fire-time posture preflight (`monitor-preflight-v1`, WS15.S4), which needs the X-9 org-core posture read and an edge principal (R-13). X-9 is not approved, so this is blocked (estimate: "X-9 not approved, observe half only"). Not built.
- Enforce (flag on by default) waits for ADR-4 sign-off and customer notice. Not a code change.
- Previous-content baseline and diff artifact for monitors are E3.5-5 (B5), not this unit.

## Files touched by this unit

None. No `git add`, no commit, no edit of `change_routes.rs`, `RUNBOOKS.md` or `zdr_matrix.rs`. The working-tree `RUNBOOKS.md` carries another agent's uncommitted diff and was left alone.

## Verification run

- Read `change_routes.rs`, `zdr_refusal.rs`, `schedule_routes.rs` (handler list), `resource_routes.rs` (create_source gate), `zdr_matrix.rs` (rows `change_history`, `change_snapshot`, `change_record_internal`, `schedules`), `git log` and `git show --stat e18328f50`, `git show HEAD:...RUNBOOKS.md | grep`.
- No cargo or docker run: no code edited, so no compile check was needed. No tests run.

## Next

Arbiter: mark E3.5-8 built in `W3-estimate.md` (B2 row, 1.0 d) and correct the plan lines 279 and 3208 that still say monitors are not refused under ZDR. The depth bullets are in `depth/W3-B2-03.md`.
