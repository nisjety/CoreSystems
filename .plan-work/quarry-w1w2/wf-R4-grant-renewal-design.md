# Session-grant renewal for AI browser runs: options (design note, nothing implemented)

Unit R4-ws1b-left, 2026-10-18. Question: Quarry has no endpoint to swap a live run's grant id, so a grant that
expires ends the run. What should WS1.B do about it?

## What the code does today (read, not assumed)

1. **Two grants, one clock.** `POST /api/v1/browser/sessions` issues a *session* grant at open: broker TTL
   `BROWSER_BROKER_GRANT_TTL_SECONDS` (default 900 s, clamped 30..3600; `internal/server/config.go`). An AI run
   derives a *run* grant from it (`DeriveRunGrant`, model-gateway `browser_run.rs`), expiry
   `min(now + ttl, parent.ExpiresAt)` (`grant.go` Derive). model-gateway sends `ttl_seconds: 0`, so the run grant
   gets the broker default, capped by whatever is left of the session grant. Fewer than 30 s left
   (`MinDerivedLifetime`) is refused at derive (409 `browser_grant_not_derivable`, which the gateway shows as
   `browser_session_grant_inactive`).
   Consequence: **an AI run started late in a session gets a short grant**. Opened 14 min ago, a run gets about
   1 minute, whatever `max_runtime_s` says.
2. **Nothing renews anything.** The broker has no `Renew` (the plan reserves it for WS8; `grep Renew` in
   `internal/grant/grant.go` finds nothing). The gateway never touches a session grant after open except to
   revoke it on close. Revoking the parent cascades to its run grants.
3. **Quarry checks the grant on every action.** `agent_routes/admission.rs:368` (step 4 of `admit_action`) validates
   `entry.grant_id`, set once at start (`lifecycle.rs:163`), and `tabs.rs:94` does the same for tab calls. An
   expired or revoked grant is `SecurityBlocked` 403 "browser grant is inactive or expired". There is no route
   that replaces `entry.grant_id` on a live run.
4. **Run length.** execution-core defaults `max_runtime_s` to 120 s when the caller omits it
   (`tool_bridge/mod.rs:881`); the gateway forwards the SPA's value or nothing. So the common run is shorter than
   a fresh grant, and the failure case is "late in the session" or "long explicit runtime", not "every run".
5. **A renewal path that exists today, by accident:** starting a run with `resume_run_id` takes
   `body.grant_id` over the checkpoint's (`lifecycle.rs:163-166`), validates it like any grant, and requires the
   grant's hosts to equal the checkpointed run's (`lifecycle.rs:~196`). Non-ZDR only (a ZDR run has no durable
   checkpoint). It restarts the live run from its checkpoint, so it is a resume, not a swap.

## Options

### A. Fail clearly (no new authority)
Keep one grant per run and make the end honest and early.
- model-gateway already has `DeriveRunGrantResponse.expires_at`. Compare it with the run's `max_runtime_s`
  (default 120 s) before any thread or run is created. If the grant would not outlive the run budget, refuse
  with 409 `browser_grant_too_short` and the expiry, instead of starting a run that dies at an arbitrary step.
  The gateway maps it to the same SPA path as `browser_session_grant_inactive` (reopen the session) or a sibling
  code. About 40 lines plus tests, all inside model-gateway `browser_run.rs` and gateway `browser.rs`.
- A run that still outlives its grant (a long approval pause) ends with the Quarry 403. execution-core should
  report it as a typed failure (`browser_grant_expired`), not a generic step error. That part is sibling (b)'s
  file (`browser_agent.rs`/`quarry_agent.rs`).
- Cost: low. Risk: none new. Does not let a long run continue.

### B. TTL policy (still no new authority)
Make the clocks fit the work.
- Derive with `ttl_seconds = clamp(max_runtime_s + 60, 60, broker max)` instead of 0, so the run grant is sized
  to the run, not to the broker default. The cap by the parent's expiry stays.
- Raise `BROWSER_BROKER_GRANT_TTL_SECONDS` for the session grant (hard clamp is 3600) so a working session is not
  cut at 15 minutes. This widens the window of a leaked session grant; the grant is already owner- and
  org-pinned and revoked on session close.
- Refuse a start when less than `max_runtime_s + 30` s remain on the session grant (same check as A).
- Cost: small (one field, one env). Does not renew; it only moves the cliff. A session that is open for longer
  than the TTL still has to be reopened.

### C. Quarry swap endpoint
`POST /v1/agent/runs/{id}/grant {grant_id}`, owner only.
- Checks: new grant active, `kind == run`, same `parent_grant_id` as the grant it replaces, same owner, ZDR not
  lower, hosts equal to the run's `constraints.allowed_domains`. Swaps `entry.grant_id`, records a receipt, updates the
  checkpoint. A caller cannot move a run onto another session's grant, because the parent must match.
- It is only useful together with two more pieces, because a run grant never outlives its parent:
  (1) a broker **session renew** (the reserved `Renew`; owner-pinned, with an absolute maximum lifetime so a
  stolen session cannot live forever), (2) a **re-derive** of the run grant under a new key from the renewed
  parent (`browser-<run>:g2`), counted against the per-parent cap of 8, then (3) the swap. Someone has to drive
  it: execution-core on seeing `expires_at` approach, or model-gateway on a timer.
- Cost: large and multi-plane (broker, Quarry, execution-core or model-gateway), and it adds a new authority on a
  live run. This is the real "renewal". It should be designed with WS8 (human-browse and attach grants also need
  `Renew`), not before.

### D. Renew by resume (no Quarry change)
When the grant has expired and the session is still open, the caller derives a fresh run grant and starts the
run again with `resume_run_id` and the new `grant_id` (point 5 above).
- Works only for non-ZDR runs with a checkpoint, and restarts the live page. Not a swap: in-flight work is lost.
- Needs a driver (execution-core), and a decision about what "the same run" means for approvals and the
  event stream.
- Cost: medium, in sibling (b)'s files. Reasonable as a stop-gap for long non-ZDR runs once A reports the failure.

## Recommendation

1. **Do A now** (model-gateway pre-flight plus the typed expiry code), **plus the `ttl_seconds` half of B**. Both
   are small, need no new authority, and remove the surprising case (a run that starts with a few seconds of
   grant left). A and B do not conflict with any later option.
2. **Do not build C yet.** A swap endpoint without a parent renew does not extend anything, and parent renew is
   WS8's `Renew` with its own design (absolute maximum, owner pinning). Build C inside WS8, or when a measured
   share of runs hits the cliff after A is deployed.
3. **D is the stop-gap** if long non-ZDR runs matter before WS8: it needs only execution-core work.
4. Open question for the owner: is a 15-minute session grant the intended ceiling for a browsing session? If a
   session is meant to live for hours, the fix is a session `Renew` (C's first piece), not a longer TTL.

## Not decided or measured here
- How often a run starts with a short grant (no measurements; needs the L2 run and the broker's `derived` metric).
- Whether execution-core already reports a Quarry 403 as a distinct failure (not read; sibling (b)'s file).
- Interplay with the per-parent run-grant cap (`DefaultMaxActiveRunsPerParent`, hard-coded 8, not an env setting): C's re-derive adds children under the
  same parent; revoke the old run grant once the swap lands.
