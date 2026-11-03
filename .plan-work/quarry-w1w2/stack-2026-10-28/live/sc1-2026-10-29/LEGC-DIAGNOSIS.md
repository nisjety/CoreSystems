# SC1 leg C diagnosis: the AI run "never executes" (2026-10-29)

This diagnosis is read-only. It reconciles two independent traces, and I re-verified the decisive claims myself (SELECTs on `model-plane-postgres-1/session_core`, docker logs, and code on branch `model-plane` in worktree `epic-shaw-4d613b`, head `b42acf718`). I did not read any secrets, and I did not restart or write anything.

## Symptom

- C.2: `POST /api/v1/browser/sessions/{id}/ai-runs` returned 200 with run `01M63P6N33NST4QRRCBGACKMGJ` at 11:19:56Z. model-gateway logged `browser run grant resolved derived_run_grant:true`.
- C.3: `GET /api/v1/runs/{run}/events/replay` returned 200 with `events:[]` for 75 s (23 polls). quarry-edge saw no step. execution-core logged nothing that names the run id.
- C.5: the stop at 11:21:13Z returned 200.

## Cause (verified)

**The run never sat idle. It failed about 0.5 s after it started, before the poller began. The cause is the Model Plane capability gate.**

1. **The run is terminal.** In `session_core.runs`, the row is `status=failed`, `agent_id=browser-agent`, created 11:19:56.259Z, `ended_at` 11:19:56.757Z. Its events, in order:
   - `RUN_STARTED` .259
   - `CHECKPOINT_SAVED` .668
   - `STEP_COMPLETED {status:permission_denied, error:"permission denied by policy"}` .685
   - `STEP_COMPLETED {step_id:model-gateway-browser-agent-final, terminal:true, failure_code:browser_failed}` .757
   - `RUN_FAILED {failure_code:browser_failed}` .757

   No browser event exists for this run.
2. **The error string comes from execution-core's capability-gate deny.** `StepOutcome::permission_denied()` sets the text "permission denied by policy" (`execution-core/src/runtime_loop/mod.rs:209-214`). The capability gate's deny arm `Ok(_evaluation) => return StepOutcome::permission_denied();` (`mod.rs:846-847`) **logs nothing**. Every other deny arm logs a WARN (for example, allow without evidence at `:816-821` and policy unavailable at `:849`), and none of those WARNs appears. The permission RunGate is not the source: `TOOL_CLASS_GATE=off` on execution-core, and `permission_mode=auto`.
3. **capability-core denies because `cap.browser.open` has never been attested.** A SELECT on `capabilities` returns this row: `cap.browser.open | availability_state=unavailable | availability_reason_code=health_not_attested | health_checked_at NULL | risk_level=high | enabled=t`. For comparison, `cap.tool.http` is `available` and was attested at 11:32Z.
   - execution-core maps `browser_agent`, `browser.act` and `browser.observe` to `cap.browser.open` (`capability_policy.rs:1510`).
   - `EvaluatePolicy` denies through `capabilityPolicyBlockReason` (`capability-core/internal/server/server.go:552-557` and `:886-911`).
   - `DeriveAvailabilityAt` returns `health_not_attested` when `health_checked_at` is NULL (`internal/models/availability.go:~201`).
4. **No service on any branch attests `cap.browser.open`, and that is by design.** `capability-core/internal/api/availability.go:91-95` keeps `cap.browser.open` off `genericGlobalHealthCapabilityIDs`, with the comment that such capabilities "must be attested by their own owners". The route refuses a generic attester with a 403 at `:269-272`.
   - execution-core's `health_attest.rs` attests only `cap.command.*`, `cap.memory.*`, `cap.agent.spawn`, `cap.process.background`, `cap.retrieval.query`, `cap.tool.http` and `cap.workspace.*`. The live log at 11:33Z confirms that set.
   - execution-core's own test lists `cap.browser.open` in `UNATTESTED_BASELINE` (`runtime_loop/agent.rs:6373-6380`).
   - `git log --all -S cap.browser.open` (since 2026-10-01) shows only seed, class and plan commits. None of the 53 `model-plane` commits after the live image `0b5d2b7b6` adds a browser attester (they cover workspace capabilities only). **No commit exists that fixes this.**
5. **Each layer reports the failure silently.**
   - execution-core: no log, as noted in step 2.
   - model-gateway: classifies `permission_denied` as `Failed("browser_agent_permission_denied")` (`browser_run.rs:1397`), then `session_flow.rs:1604` collapses every browser failure to `browser_failed` and terminalizes without a log. The derived run grant `2a74a328…` was revoked at 11:19:56.72Z.
   - session-core: the only trace is a WARN at 11:19:56.694Z, "unrecognised CompleteStep status; classifying by error presence", `status=permission_denied`.
   - The replay endpoint: it passes only the 7 browser and approval event types (`model-gateway/src/http_routes.rs:1656-1667`) and drops `RUN_FAILED`. A failed run therefore returns `200 events:[]` forever.
6. **SC1 posture is not a factor.** Out of 2012 runs on this stack, this is the only `browser-agent` run. No browser AI run has ever executed here. Capability availability does not depend on strict grants, broker TTL or edge mode.

### Where the traces disagreed (resolved)

| Point | Trace 1 | Trace 2 | Verified |
|---|---|---|---|
| `TOOL_CLASS_GATE` on execution-core | "set", RunGate might judge | off | **off** |
| `TOOL_POLICY_ENFORCED` on capability-core | set, a policy could relax ASK | false | **false**. Relaxation does not apply anyway: `DecideWithPolicies` relaxes only `mcp_tool` capabilities (`policy/engine.go:265-305`), and `browser_agent` is a built-in. **No org or user policy can turn this ASK into allow.** |
| Why `browser_failed` | open question | n/a | model-gateway `session_flow.rs:1604` maps every `BrowserAgentTerminal::Failed(_)` to `browser_failed` |

The planner flags agree with the L2 doc. `QUARRY_BROWSER_AGENT_LLM` and `QUARRY_BROWSER_AGENT_PLANNER_MODE` are unset on execution-core; these are **execution-core** environment keys, not Quarry ones. `QUARRY_BROWSER_AGENT_ENABLED=1` and `QUARRY_EDGE_URL` is set. The run never reached the planner.

### The second blocker behind the first (verified in code)

`cap.browser.open` is `risk_level=high` and is on the protected high-risk list (`models/capability.go:91`). Lowering it needs `capability:risk:override` (`api/capabilities.go:225`). Once the capability is attested:

1. `DeriveAvailability` returns `approval_required`.
2. `riskResult` returns **ASK** (`policy/engine.go:393-400`).
3. execution-core returns `awaiting_approval(CapabilityHighRisk)` (`mod.rs:~824-843`).
4. model-gateway only logs INFO "awaiting approval" (`browser_run.rs:~1337`).

The browser-run path has **no durable approval continuation**: `reject_unsupported_browser_approval` (`browser_run.rs:67-81`) says so explicitly. Attestation alone would therefore turn "fails in 0.5 s" into "parks forever, non-terminal, empty replay".

## Fix options (ranked)

There is **no live-stack config fix**. No environment flag bypasses availability, a tool policy cannot relax a built-in capability, and lowering the risk level is a governance change that needs `capability:risk:override`. A manual DB write is not allowed here, would expire after the 5-minute `AvailabilityAttestationTTL`, and would still hit the ASK. Every real fix is **Model Plane code on branch `model-plane`**, which is Model Plane ownership.

1. **Owner attester for `cap.browser.open` plus a decision on the high-risk ASK for managed browser runs.** Owner: Model Plane. Risk: medium-high, because it makes a high-risk external-write capability runnable.
   - a. **Attester.** browser-broker (Model Plane owns browser grants) or a dedicated execution-core lane attests `cap.browser.open` from a real probe: quarry-edge `/ready`, the broker store and the agent client configured. capability-core needs a dedicated owner lane and principal in `availability.go`, like the ticket `ownerActionLane`, plus the matching `plane-service-principals.json` entry. Do not just add the capability to the generic list: that breaks the stated design, and the cross-language test in `health_attest.rs` reads that list.
   - b. **ASK resolution.** Choose one:
     - (i) Treat the broker-validated, user-initiated, domain-scoped **derived run grant** as the approval for `browser_agent` on the managed `/v1/browser/runs` path. This is the smallest change. It needs explicit sign-off because it skips HITL for a high-risk capability.
     - (ii) Build the durable approval continuation for browser runs (larger; WS21 territory). The cap.browser.observe/act split in the WS21 plan would let `observe` run at medium risk.
   - This needs an owner decision before anyone builds it.
2. **Observability fixes. Owner: Model Plane. Risk: low. Worth doing regardless of option 1.**
   - Log the capability deny with `run_id`, `decision_id`, `reason` and `policy_source` at `mod.rs:846`.
   - Keep the classified failure code at `session_flow.rs:1604`, or log it.
   - Teach session-core the `permission_denied` CompleteStep status.
   - Let the replay endpoint pass a terminal marker (`RUN_FAILED`, `RUN_COMPLETED`, `RUN_CANCELLED`) so a polling client can tell a failed run from an idle one.
3. **Harness fix. Owner: us (Quarry plan-work). Risk: none.** In `sc1-legs.js`, C.3 should also read the run's terminal state and stop polling when the run is failed. Until option 1 lands, record leg C as **BLOCKED (Model Plane: cap.browser.open unattested)**, not as a Quarry or gateway FAIL. C.1, C.2 and C.4 remain valid PASSes for the gateway, broker and grant path.
4. **Not recommended:** a flag-gated widening of execution-core's generic attester to `cap.browser.open` for a dev proof. It contradicts the design comment, and it still parks at the ASK unless 1b also lands.

## Re-running leg C

Prerequisite: option 1 (a and b) is merged to `model-plane`. Then the Model Plane images are rebuilt and deployed. The owner or the Model Plane session does this, not this session.

1. Rebuild and redeploy capability-core, execution-core, model-gateway and browser-broker (whichever 1a and 1b touch) per `model-plane-build-and-deploy`. Then run `recreate-with-live-env.sh` if the build came from a worktree.
2. Preflight (read-only):
   - `SELECT id, availability_state, availability_reason_code, health_checked_at FROM capabilities WHERE id='cap.browser.open';` should return `available` (or `approval_required` only if 1b(ii) was chosen), with `health_checked_at` less than 5 minutes old.
   - `docker logs model-plane-<attester> | grep 'cap.browser.open'` should show periodic attestations.
   - `docker exec model-plane-execution-core-1 printenv` key check only: `QUARRY_BROWSER_AGENT_ENABLED` is set, `QUARRY_EDGE_URL` is set, and `QUARRY_BROWSER_AGENT_LLM` and `QUARRY_BROWSER_AGENT_PLANNER_MODE` are unset (LLM-free leg).
3. Signed in on live dev, run `await L2.legC()` from `sc1-legs.js`, with the C.3 terminal-state check from option 3 added.
4. Right after C.2, check the run:
   - `SELECT status, ended_at FROM runs WHERE id='<run>';` should be `running`, then `completed`.
   - `SELECT event_type, ts FROM events WHERE run_id='<run>' ORDER BY ts;` should show no `STEP_COMPLETED permission_denied`, and no step left `awaiting_approval` unless 1b(ii) was chosen and approved.
5. Pass criteria are unchanged: C.3 replay shows both `BROWSER_ACTION_DISPATCHED` and `BROWSER_OBSERVATION_RECEIVED`, quarry-edge logs the steps (grep by `plan_<run>`: execution-core's loop logs key on the plan id, not the run id), C.4 is 403, and C.5 is 2xx.
6. Run H.1 to H.3 over the C happy window as in `SC1-RUN.md`.
