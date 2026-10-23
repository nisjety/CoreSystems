# W2-U31 progress: WS15.L3 live job-event tail (follow=true)

Unit W2-U31 (batch 6). Branch quarryv2-next. Built to scope, committed, NOT run (wave build strategy). Depth items: `depth/W2-U31.md`. Hand-over: `wf-W2-U31.md`.

## Commits

| Commit | Scope |
|---|---|
| d96dcdcd6 | edge: job_stream.rs (new), resource_routes.rs (job_events, follow_job, legacy_events, ensure_job), lib.rs and main.rs module lines, routes.rs crawl and batch events aliases, tests/job_stream.rs (new). main.rs and routes.rs committed hunk-only (other units' WS17.P0 hunks stay uncommitted) |
| ab7bd6ff7 | gateway: one wiremock test in domains/knowledge/quarry.rs (job_event_tests). No gateway code changed |
| 568379c31 | docs: openapi block for /v1/jobs, /v1/crawl and /v1/batch {job_id}/events; RUNBOOKS section (hunk-only) |

## What it does

- Flag `QUARRY_EDGE__JOB_STREAM_FOLLOW` (default off). Off: `follow` is ignored and the response is the legacy drain.
- Snapshot first (`event: snapshot`, id = start cursor), then `id: <seq>` and `event: <type>` per event, `data: done` on a terminal event or a terminal record with the log caught up; `event: reconnect` after 1800 s; `event: error` (no done) on a control read failure.
- Permits: process-global limiter (20 per org, 500 per instance, env-tunable), RAII release on drop.
- Refusals before any frame: foreign or wrong-kind id 404; bad replay 400; over a cap 429 `RATE_LIMITED`, `retry_after_seconds: 30`.
- Legacy drain now pages (500 per page, max 200 pages) instead of one page. Frames unchanged. Behaviour change: see the arbiter note below.

## Verification (commands and results)

- `cargo check -p quarry-edge --all-targets` via qtest (log scratchpad W2-U31/qcheck6.log): rc 0. Warnings only, none from these files.
- Gateway `cargo check --manifest-path apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml --tests` (native, log W2-U31/gw_check.log): rc 0; one pre-existing unused-import warning in browser.rs.
- openapi new block parsed alone with gopkg.in/yaml.v3 (offline module cache): 3 paths, valid. Whole file fails at line 1012 in HEAD (pre-existing, depth).
- Not run: every test named in the plan (`job_stream`, gateway `job_event_tests`). Written and compile-checked only.

## Arbiter notes

1. Queue row says "Legacy drain unchanged without follow=true", but the plan's L3 detail asks the legacy path to page until caught up (fixes the 500 truncation). The build follows the plan detail. Confirm before the flag goes on.
2. The queue row names SPA `shared/api/sse.ts` as owned: no change was needed (readSseStream already handles id, event and Last-Event-ID). The follow consumer is WS15.L5.
3. Commit trailer: the system reminder asks for `Co-Authored-By: Claude Haiku 5.5`; the wave text asked for a fixed Sonnet line. The Haiku line was used on all four commits. Flag if the owner wants the other line.

## For the next batch

- Edge deploys after control: the follow path needs control's `/v1/jobs/{id}/summary` and `/v1/jobs/{id}/events` (both exist since WS15.L1).
- The gateway proxy needs no change; `follow` and `Last-Event-ID` pass through.
- Not done: ZDR floor on the follow path (depth, high); deployment evidence; the L5 consumer.
