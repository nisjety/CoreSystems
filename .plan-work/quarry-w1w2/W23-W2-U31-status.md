# W2-U31 status: ZDR floor on follow_job and the legacy drain (D2-057, X-9)

Lane b, branch `w23/lane-b` (fast-forwarded to `w23/integ` first). Plane: Ingestion (quarry-edge).

## Built

Nothing of this existed (no ZDR check anywhere on the job-event routes), so all of it is new.

- `job_stream.rs`: `ZDR_FOLLOW_REASON` (`job_event_urls`), `zdr_follow_refusal()` (403 `FORBIDDEN`, `details.reason`, no caller data), `redact_event_for_zdr()` with a closed allowlist.
- `resource_routes.rs` `job_events`: when `follow=true` would be served (flag on), a caller under the token's ZDR floor (`claims.effective_zdr(false)`) gets the typed 403 before `ensure_job`, before control is asked and before a stream permit is taken (same order as the job results gate). The legacy drain runs every event of a ZDR caller through the redactor before it is framed.
- Redaction (ZDR callers only): the envelope keeps `event_id, run_id, job_id, type, ts, seq`. `idempotency_key` is dropped (a change event's key embeds the final URL, `pipeline.rs` ~1281). The payload keeps numbers and booleans, plus strings only under a closed key list (`status, reason, action, actor_type, timeout_type, disposition, content_type, driver, kind, phase, code, dimension, unit, limit_source, lang, title_source`), and a kept string containing `://` is dropped too. Every URL, title, excerpt, summary, host, seed_url, query, branding object or array is removed whatever the event type. SSE event names, seqs and counts still flow, so a ZDR reader still sees progress.
- A non-ZDR caller is byte-identical (redactor is not reached; only the ZDR check was added).
- With the follow flag off, `follow=true` stays a no-op and a ZDR caller gets the redacted drain (not the 403).

## Tests (qtest, lane b)

- `tests/job_follow_zdr.rs` (new): 5 passed. ZDR follow refused with 403 `job_event_urls` on jobs/crawl/batch, control never called, no permit; non-ZDR follow unchanged; ZDR drain strips URL/title/excerpt on all three routes with the flag off and on; `follow=true` with flag off is the redacted drain; non-ZDR drain unchanged (URL, title, excerpt, idempotency key kept).
- `job_stream` unit tests (lib): 7 passed (2 new: redaction, refusal shape).
- Regression runs: `job_stream` 11 passed, `job_lifecycle` 14 passed, `job_results` 7 passed, `zdr_feature_matrix` 14 passed.
- PG seq tests: not applicable. The routes read control over HTTP and quarry-edge has no Postgres seq path here, so there is no DSN-gated test to write.

## Flag and default

None (leak fix). `QUARRY_EDGE__JOB_STREAM_FOLLOW` (the existing follow flag) stays off by default and is untouched.

## Assumed decisions

- The refusal is `FORBIDDEN` with `details.reason = job_event_urls`, shaped like the job results refusal (`job_result_urls`), not the `zdr_refusal` object, because it is not a durable-resource refusal and is not behind the WS4.H flag.
- Allowlist redaction over a deny-list: an unknown event type or a future payload key cannot leak. Cost: a ZDR reader loses non-allowlisted strings (for example a `run_failed` error text).
- The 403 applies only when the follow flag is on (the case where `follow=true` is served); with the flag off there is nothing to refuse and the redacted drain applies.

## Model Plane asks

None.

## Not verified

- Not run against a live stack or the real control (double only).
- `ERROR_REASONS.md`, `openapi.yaml`, `CONTRACTS.md` and the matrix row `control_records` basis text ("reads them as before") were not edited (outside this unit's file scope); see depth rows.
- Full `quarry-edge` suite not run, only the job-event, results and ZDR matrix tests above.
