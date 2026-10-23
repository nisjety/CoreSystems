# R4-ws1b-left hand-over (WS1.B leftovers), 2026-10-18

Branch quarryv2-next, local commits only. Companion notes: wf-R4-grant-renewal-design.md (task 6),
wf-R4-ws1b-left-compose.md (task 7), wf-R4-ws1b-left-progress.md (state log).

## What was done
1. **L1 contract test** (`scripts/tests/browser-grant-contract.test.mjs`, the existing L1 harness, same style):
   DeriveRunGrantRequest 1-6 and DeriveRunGrantResponse 1-7 pinned by name/type/repeated plus the rpc signature;
   generated Go (browser.pb.go tags, browser_grpc.pb.go method) compared with the proto; DeriveRunGrant has one
   non-test caller (model-gateway browser_run.rs); the gateway never calls the broker, never builds a `session:<id>`
   string, still mints the browser-broker token and forwards `x-browser-authorization`. 12 tests pass; planting
   `AcquiredGrant` in Quarry's production code fails the new test (reverted). Commit 5fee6c16.
2. **derive failure creates no thread/run**: already present at HEAD as `derive_failure_creates_no_thread_or_run`
   (33a362f6): it drives the real `browser_run_start` with a Session Core stand-in that counts connections. Re-run
   result is in the final report.
3. **GrpcGrantValidator::acquire + AcquiredGrant deleted** (no caller anywhere; commit 5fee6c16). The L1
   single-minting allow-list no longer names Quarry. `GrpcGrantValidator::revoke` is also caller-less; left (it is not
   what the plan names).
4. **Idempotency-Key on POST /v1/browser/runs** (the plan names no header: it only says "client idempotency key";
   `Idempotency-Key` is the plan's own name for the batch key in WS14/WS15, so the same header is used).
   - model-gateway: `browser_run.rs` + `session_flow.rs::prepare_idempotent_browser_run`.
   - gateway: commit f7818066.
   Semantics, as built:
   - Optional header, 1-128 of `A-Za-z0-9-_.:`, one header. Else 400 `idempotency_key_invalid`. With a body `thread_id`
     the key is refused (400 `idempotency_key_requires_a_new_thread`): a keyed start owns its thread.
   - `run_key = "browser-idem-" + hex(SHA-256(len-prefixed [org, user, session grant id, key]))[..32]`. The same
     string is the broker `session_key` (so a retry derives the SAME run grant), the thread `session_key`
     (CreateThread is idempotent on it) and the managed run `start_key` (idempotent in Session Core).
     `plan_id` (when the body names none) is `plan-idem-<same digest>`.
   - A keyed start appends the prompt only AFTER the managed run is known to be new (the unkeyed order would append
     a second copy of the prompt on a retry).
   - Replay (Session Core says `already_started`): the handler renews the run's lease with the managed heartbeat. Live:
     200 `{run_id, thread_id, plan_id, idempotent_replay: true}`, nothing dispatched, nothing appended, grant not
     touched. Terminal: 409 `idempotency_key_run_ended`. Liveness unknown: 503 `idempotency_replay_unverified`.
   - Same key, different goal: Session Core refuses the managed start (immutable input) -> 409 `idempotency_key_reused`;
     the grant (shared with the live run) is not revoked. Different hosts: the broker's own 409
     `browser_grant_not_derivable`.
   - Failure handling for keyed starts differs from unkeyed on purpose: an unkeyed failed start revokes its grant (the
     grant is its alone); a keyed one may share the grant with a live first attempt, so a failure before the run is
     known new keeps the grant (same-key retry reuses it; bounded by the per-parent cap of 8 and the parent's life).
     When THIS request created the run and cannot dispatch it (liveness heartbeat failed), it cancels the run and
     revokes the grant, so a replay cannot answer 200 for a run that never started. Same for a failed prompt append:
     the new run is cancelled.
   - Known limits: (a) a keyed retry after the key's own run ended or its grant was revoked is 409
     `browser_grant_not_derivable` (the broker refuses to resurrect a revoked child), which the gateway shows as
     `browser_session_grant_inactive` ("reopen the session"); a client with a new key on a live session succeeds.
     (b) A key whose first attempt failed at Session Core with the response lost leaves a grant until the session ends
     or it expires. (c) The goal is compared only through Session Core's immutable-input check (goal text and thread);
     `max_steps`, `stop_criteria`, `start_url` and the other body fields are NOT compared across retries. (d) A concurrent
     retry that arrives between the first request's managed start and its heartbeat sees a live run and answers 200
     before the first has dispatched; the first still dispatches (the only failure there is the cancel-and-revoke path
     above, which makes that replayed 200 wrong). Not closed here.
5. **Gateway mapping for WS3.F codes** (commit 62f1b66a). `forward_quarry_failure` now maps:
   `browser_session_cap` 429 -> `browser_capacity_reached` (+ Retry-After, retryAfterSeconds);
   `browser_page_cap` 429 -> `browser_tab_limit_reached` (+ Retry-After, retryAfterSeconds);
   `session_reaped` 502 DRIVER_FAILED -> 410 `browser_session_reaped`; `browser_process_lost` -> 410
   `browser_process_lost` (no retry hint). `/ready` `browser_unhealthy`: the gateway has no reader of Quarry's /ready
   (grep), so nothing to map.
6. Renewal design note: wf-R4-grant-renewal-design.md. 7. Compose lines: wf-R4-ws1b-left-compose.md.

## For the SPA owner (src/shared/i18n/errors.ts is held by WS5.G; not edited)
Codes that need no/en copy (the SPA may also want a 410 handler: nothing checked on that side):
- `browser_tab_limit_reached` (429, Retry-After) "This browser has too many tabs open. Close a tab or try again shortly."
- `browser_session_reaped` (410) "This browser session was closed after being idle. Start a new session."
- `browser_process_lost` (410) "The browser behind this session stopped unexpectedly. Start a new session."
- `browser_idempotency_key_reused` (409), `browser_idempotency_key_run_ended` (409, details.runId),
  `browser_idempotency_unverified` (503, Retry-After 5), `invalid_idempotency_key` (400): only reachable once the SPA
  sends an `Idempotency-Key`; it does not today.
- Still missing from the earlier round: `browser_session_grant_inactive` (WS1.B review).
Already present: `browser_capacity_reached`.

## Not done / traps
- Quarry-side `GrantValidator` checks and L2 legs: need X-1 / WS1.A2 (unchanged).
- `cargo check -p quarry-edge --features grpc`: see the final report (blocked by another unit's chromiumoxide.rs at
  the time of the first try).
- Gateway `cargo clippy --all-targets -- -D warnings` is red at HEAD in 17 unrelated places (not from this unit).
- The docker lock was contended for hours (another unit's 3x chromium loops, integration tests). My first model-gateway
  run gave up after 45 min of waiting; mg-run.sh in scratchpad/R4-ws1b-left waits 100 min.
- `docs/BROWSER_WORKSPACE_PLAN.md` documents POST /v1/browser/runs; not updated for the header (outside owned paths).
- Hard trap found: apply.js/stage-head.js (scratchpad/R4-ws1b-left) stage "HEAD blob + my transform" so a file that
  also carries another unit's uncommitted hunks (gateway browser.rs: R2's ~283 lines) can be committed without them.
- Heredocs in this Bash tool eat backslashes in JS source (a regex lost its `\n`/`\b`): write JS with the Write tool.

## Round 3 close-out
- e4af12cb feat(model-gateway): Idempotency-Key on POST /v1/browser/runs (browser_run.rs + session_flow.rs). model-gateway --lib: 1727 passed, 1 failed (known zdr_precheck CRLF artifact), 4 ignored. Broker go test ./...: all ok. Gateway domains::browser: 97 pass. cargo check -p quarry-edge --features grpc: Finished (2 unused-import warnings from other units).

## Round 3 (review fixes)
- e36c879f: L1 test strips #[cfg(test)] items by brace matching (self-test + real-file checks), idempotency-key literal agreement test, browser-broker-ci.yml path filters widened.
- 860c83ce: keyed start detached from the client future; replay liveness is a read-only GetRun (no lease renewal); tests for disconnect, keyed start failure branch, wire-name literal.
- Residual (documented in commit and handler docs): process death between managed start and dispatch leaves a live undispatched run until lease expiry; needs a durable dispatch marker in Session Core.
