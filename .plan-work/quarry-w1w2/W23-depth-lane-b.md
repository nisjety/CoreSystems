# W23 depth findings, lane b

## W2-U31 (lane b)

| id | where | finding | severity | suggested fix |
|---|---|---|---|---|
| W23-b-U31-1 | docs/ERROR_REASONS.md, openapi.yaml, CONTRACTS.md | The new 403 `details.reason = job_event_urls` on `follow=true` for a ZDR caller, and the redacted drain for ZDR, are not documented. | low | Add a row under the 403 section and a note on the three events routes (docs unit). |
| W23-b-U31-2 | crates/quarry-core/src/zdr_matrix.rs row `control_records` | Basis says a ZDR organization "reads them as before"; the event routes now redact for ZDR and refuse follow. The row stays Inert, but the basis text and the test citation are stale. | low | Reword the basis and cite `tests/job_follow_zdr.rs`. |
| W23-b-U31-3 | job_stream::redact_event_for_zdr | Allowlist drops non-allowlisted strings, including failure text of `run_failed`; a ZDR reader sees `type` only. | low | Add a closed `reason` vocabulary from control if ZDR readers need it. |
| W23-b-U31-4 | pipeline.rs ~1281 | Change event idempotency keys embed `final_url`; they reach control's event log and any non-redacted reader (non-ZDR callers, webhooks). | low | Hash the URL part of the key at the source. |
| W23-b-U31-5 | qtest-lane.sh | Every lane switch touches all sources, so interleaved lanes cost a full 10 min rebuild per run; a lane's run waited ~1.5 h behind other lanes. | process | Run all of a batch's lane-b tests in one call. |
