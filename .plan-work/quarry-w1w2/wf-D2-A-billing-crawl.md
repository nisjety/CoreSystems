# D2-A-billing-crawl hand-over (decision 3.11, crawl and batch)

Done
- `handoff.rs`: `post_job` marks the ack `replayed` when an Idempotency-Key was sent and Control answered 200 (Control answers 201 for a created job). New `billable_job_id(&ack)`: the job id for a created job, `None` for a replay or any failure.
- `routes.rs`: the crawl seed and batch meters are keyed by that job id (`quarry:<job_id>:<metric>`) in both the legacy and the corrected (`on`) branch; the legacy `else if !queue_full` is now "a job was created and is not a replay". `HandoffAck.replayed` is `#[serde(skip)]`, not on the wire.
- `tests/billing_handoff.rs` (5 tests, off and on, crawl and batch): first create bills once under the job id; replay bills nothing extra; failed or refused handoff (500, 502, 429) bills nothing and the later success bills once; a reused X-Request-Id does not collapse or collide; no key means a job and an event each. Unit test `a_200_for_a_sent_key_is_a_replay_and_has_no_billable_job` in handoff.rs.
- `docs/BILLING.md`: event-id text, metric table and Idempotency section; D2-033 row in DEPTH-FINDINGS-W2.md corrected.
- billing-core, sync routes and Redis: not touched (by decision).

Left / traps: see depth/D2-D2-A-billing-crawl.md (under-bill window on a replay after a crash, look-back for finance, billing owner not yet told, no stack run, no two-org request test).
Trap: `tests/` stubs that answer 200 to a create without a key are not replays (the flag needs a sent key).
