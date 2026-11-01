# W23 depth findings, lane b

## W2-U31 (lane b)

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-1 | docs/ERROR_REASONS.md, openapi.yaml, CONTRACTS.md | The new 403 `details.reason = job_event_urls` on `follow=true` for a ZDR caller, and the redacted drain for ZDR, are not documented. | low | Add a row under the 403 section and a note on the three events routes (docs unit). |
| W23-b-U31-2 | crates/quarry-core/src/zdr_matrix.rs row `control_records` | Basis says a ZDR organization "reads them as before"; the event routes now redact for ZDR and refuse follow. The row stays Inert, but the basis text and the test citation are stale. | low | Reword the basis and cite `tests/job_follow_zdr.rs`. |
| W23-b-U31-3 | job_stream::redact_event_for_zdr | Allowlist drops non-allowlisted strings, including failure text of `run_failed`; a ZDR reader sees `type` only. | low | Add a closed `reason` vocabulary from control if ZDR readers need it. |
| W23-b-U31-4 | pipeline.rs ~1281 | Change event idempotency keys embed `final_url`; they reach control's event log and any non-redacted reader (non-ZDR callers, webhooks). | low | Hash the URL part of the key at the source. |
| W23-b-U31-5 | qtest-lane.sh | Every lane switch touches all sources, so interleaved lanes cost a full 10 min rebuild per run; a lane's run waited ~1.5 h behind other lanes. | process | Run all of a batch's lane-b tests in one call. |

## W2-U31 | security review

Blocker fixed in this lane (see the fix commit): `GET /v1/runs/:id/events` served the same job events unredacted to a ZDR caller (the redacted drain even hands out the `run_id`), and control's handler resolves the run by id without checking `org_id`. The edge now redacts the payload under the ZDR floor and returns 404 when any event's `org_id` is not the caller's (`tests/run_events_scope.rs`).

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-S1 | services/quarry-control/internal/resources/resources.go `GET /v1/runs/{id}/events` | Control resolves the run with `GetByRunID` and never compares the owner's org with `?org_id`, so any internal caller holding another org's run id reads its events. The edge now refuses mismatches (the W2-U31 security review fix), but control is the authority and should 404 itself. | medium | `if owner.OrgID != orgFromQuery(r) { 404 }`, same guard as `jobEvents`; a Go test with two orgs. |
| W23-b-U31-S2 | job_stream::ZDR_KEPT_STRING_KEYS | A kept string survives if it has no `://`. Today's producers put closed words in `reason`, `code`, `kind`, `status`, but nothing enforces that; a future producer writing a host or a page-derived string into a kept key (`reason: "dns error for host.example"`) passes. `content_type` and `lang` come from the remote server. | low | Also require a token shape for kept strings (for example `^[A-Za-z0-9_./+;= -]{1,64}$` and no dots for `reason` and `code`), and pin the producer vocabularies in a test. |
| W23-b-U31-S3 | auth::Claims::zdr_floor used by the event routes | The floor is the caller's token, so a service principal (floor never applies) of a ZDR org reads the org's job and run events in full. Consistent with `errors_for` and the matrix, but a service reader that re-serves events to a ZDR user carries the leak. | low | Document in the ZDR matrix basis; consider the org posture (X-9 event) rather than the token for read redaction. |
| W23-b-U31-S4 | resource_routes::forward_list / forward_one error path | A non-2xx control answer is returned to the caller as `control-plane GET {path} returned {status}: {body}`, echoing control's body (internal messages, ids) to the API caller. | low | Map control errors to the typed envelope without the raw body; log the body server-side only. |
| W23-b-U31-S5 | resource_routes::job_events (kind = None) | `/v1/jobs/:id/events` drain skips `ensure_job` and relies on control's `GetByOrg` in `jobEvents` for tenancy. Correct today; a regression in control would make it a cross-org read with no edge check. | low | Check each drained event's job id or call `ensure_job` for every kind. |

## W2-U25b depth rows
| D2-098b | W2-U25b | Agent events are now withheld from NATS entirely; the WS10.S4 redacted projection that would give Model Plane/Control a safe cross-plane copy is still not built. | crates/quarry-runtime/src/nats_event_bus.rs | other-plane | medium | WS10.S4 |
| D2-098c | W2-U25b | No test drives the real JetStream publish path (no broker in the test container); a bus seam (trait over the js publish) would let the gate be tested end to end. | crates/quarry-runtime/src/nats_event_bus.rs | test | low | - |
