# Review: Quarry stack session on live dev, 2026-10-28

Reviewer's scope: the runbook (`../../STACK-SESSION-RUNBOOK.md`), `README.md`, `flags-log.md` and every evidence file in this folder. The isolated-stack files one level up (`../P0-preflight.md`, `../L-X10-robots.md`, `../L-SC7-billing.md`, `../flags-log.md`) were used to compare. The plan rows behind "as the plan row states" come from `docs/plans/quarry-browser-build-plan-2026-10-14.md` (SC table, lines 246 to 253 and 3865 to 3872).

The rules used:
- A PASS needs an observed status or count, not a code reading.
- If the dev stub (`Bearer dev-evidence`, which becomes `org_placeholder` / `user_placeholder`) stands in for a step the runbook marks **[USER]**, the result is PARTIAL. The exception is a step where the runbook says the stub is enough.
- A FAIL stays a FAIL.

The reviewer ran only `live.sh flags`, `docker ps` and read-only `docker inspect` / `docker exec ... printenv` checks: names only, plus the values of `RUST_LOG` and `QUARRY_EDGE__METRICS_ENABLED`. No `.env` file was opened. The size of `live-flags.env` was taken with `stat` and its content was not read.

## Flag log check

- **Every change has a revert.** P0-live: 1 change, 1 revert. L-X10-robots: 3 keys, all reverted at 20:01:38 to 20:01:40Z. L-SC7-billing: `RUST_LOG`, reverted. L-SC2a-authority: 2 keys, reverted. L-SC3-SC4-runs: 9 keys, reverted at 20:30:36, 20:34:29 and 20:35:49Z. L-SC5-SC6-docs-zdr: 3 keys, reverted. L-X12-D16: 6 keys, reverted at 20:55:46 and 20:58:56Z. No key is left changed.
- **`live-flags.env` is empty**: 0 bytes, last written 20:59:07Z (by `stat`, not read).
- **`live.sh flags` at 21:05:44Z is the same as `P0-live-flags-end.txt`** (`diff` is empty). The same 22 values are in the README "Baseline switches" list. None of the keys listed as "Not set" in the README appears.
- **Extra state checks.** `quarry-edge` started at 20:59:00Z, which is the final recreate in the log. In the edge, `QUARRY_EDGE__METRICS_ENABLED=0` and `RUST_LOG=info`. None of the other session keys is set (`TAKEOVER_ENFORCEMENT`, `RECEIPT_TEXT_MODE`, `DOC_ROUTING`, `AGENT_DEFAULT_MAX_STEPS`, the sweep/TTL/cap keys, `DEADLINE_SCALE`). `docker ps -a` shows no `qlive-*` helper. quarry-edge, quarry-control and quarry-orchestrator are healthy. crawl-fixture-a and crawl-fixture-b are still up, as the README intends.
- **Correction.** The `flags-log.md` line at 20:51:53Z says `QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S (unset, 900)`. The code default is **300** (`crates/quarry-edge/src/run_limits.rs:167`), and the 20:24:27Z line already gives the right value. The revert is still correct (unset).
- **Live-state changes that are not flags and are not in the flag log:**
  - `docker stop` / start of `quarry-control` (L-SC7-billing BI.3, 20:08:02 to 20:08:13Z; the container shows StartedAt 20:08:12Z).
  - Four helper containers, all removed.
  - Rows left on live for `org_placeholder`: crawl jobs, agent runs, 45+ checkpoint rows, and at least 1 (probably 2) `quarry_profiles` rows. The count was not taken again after the second persistent run.

  The owner chose live dev ("dette er kun dev"), so these are noted, not held against the session.

## Per leg: rows downgraded or questioned

### P0-live
- **d2 to d5 "PASS (absent)" become INFO.** A counter family that is absent proves nothing, because nothing exercised it. This is not a PASS.
- **f3 PARTIAL becomes NOT-RUN (needs the user).** The 401 without a cookie was observed, but the relay of the edge 501 as `model_error` was never exercised.
- **g2 PARTIAL is right for the serve path, but it does not block OPENAPI-INTERNAL.** That item asks only that the page-images and `run_page` routes are not routed at the public ingress. The gateway's 404 was observed for both, so OPENAPI-INTERNAL is PASS. The real serve path stays open: the producer is down because of the Data Plane NATS auth.
- **g4 "PASS (static)" is kept as static.** It is a count from the spec file, not stack evidence. The runbook asks for no stack check for OPENAPI-NULL.
- **g5 is questioned.** It proves the `/v1/map` robots filter. It does not touch the two ROBOTS-RFC parser questions: CR-only line endings and two `Crawl-delay` values. Those are NOT-RUN.
- **f5 / DATAPLANE-OPTION-B "watch it during the session" is questioned.** The log grep covers boot to 19:53 only. Each later edge recreate dropped its log (X-10 Depth 10), so the rest of the session is not watched. SC2a M1 adds one later window with no broker or Model Plane call.
- **Missing NOT-RUN rows** for the runbook preparation on live: step 2 (fresh Rust lane on this tree), step 5 (`docker compose config`) and step 6 (the browser-grant preflight script).
- **Process notes:**
  - g3 sent a `DELETE` to the org-erasure route on live. It returned `rows_deleted 0`, and the leg records the deviation.
  - L-SC3-SC4 Depth 7: a dev runtime-token value was printed into a tool transcript.

### L-X10-robots
- **X10.5 "PASS" is an observation, not a runbook criterion.** It is questioned for the soak: in enforce, a robots refusal on the link list is not counted in `quarry_robots_decisions_total` (`denied` stayed 0). Only the run events show it.
- **X10.8 (D2-161) is PASS for the edge fix, but it is reached only by a direct `run_page` call with `prev_fingerprint`.** The orchestrator never sends that field (Depth 2), so the runbook wording "crawl again" cannot show the cache path.
- **X10.7 is kept as PASS.** That the edge refused the 302 hop is inferred: the metadata address cannot be observed. The observed facts are 0 pages, code `robots_unreachable` and the counter moving from 1 to 2.
- Every other row follows from the script SUMMARY lines and the fixture logs. These match qv2e exactly.

### L-SC7-billing
- **SC7.3 "NOT-RUN" is relabelled "absent, W2 scope".** `constraints` and `clamped[]` belong to the W2 part of SC7 and are not in the W1 crawl clause. The check was run and found the fields absent.
- **BI.1 PASS is kept.** The Control 201 then 200 is written down in the md only. No log excerpt is saved.
- **BI.2 PARTIAL is agreed, and BILLING-IDEMPOTENCY as a whole is PARTIAL.** The count of publishes is observed: one `usage metered` line per created job, none for the replay, none for the failed handoff. But:
  - The event id was not seen on the wire.
  - On live, nothing reaches billing-core at all (Depth 2), so "the replay bills nothing" is not shown by billing.

### L-SC2a-authority
- **E8 keeps PARTIAL, for a different reason.** The plan's SC2a row (plan lines 247 and 3866) accepts the CI claims-injecting test for the service principal, so E7 NOT-RUN does not block. The Evaluate denial for the person principal is an [AGENT] leg, and E1 to E6 PASS stand.
- **T1, T2, T4 and T5 are PASS as observations, but the TAKEOVER item and SC2a are PARTIAL.** The runbook and the plan require "a user token labelled agent" ([USER]). The stub stood in for it.
- **R1 to R3 have the same caveat, so MP-GATEWAY-AUTH stays PARTIAL.** Only the label branch of `derive_initiator` (`run_live.rs:84`) ran. The service-principal branch did not.
- **T7 PARTIAL is a depth note, not a criterion.** The runbook does not ask for a would_reject counter.

### L-SC3-SC4-runs
- **V1 is kept as PASS, but one claim is questioned.** The SC4 criterion is met by observed facts: 16 of 16 steps 200, each title matching its own URL, `/ready` generation 1 to 1, and 0 relaunch, crash or disconnect lines. The md's "PNG decoded: A 800x600, B 1280x900" is not in the evidence. `L-SC3-SC4-runs-viewport.log` shows `png A dims: 1885676090x577334625   png B dims: 1885676090x577334625`, which is a broken decode and the same for both. The rendered frame size is unproven.
- **R1 and R2 "PASS (recorded)" become RECORDED.** The plan has no threshold. RSS at 32 contexts with `QUARRY_BROWSER_MAX_SESSIONS=48` (plan line 3868) was not run, so SC3 is PARTIAL. R1 quotes three `docker stats` reads (375.6, 390.3 and 433.3 MiB). Only 375.6 (`-cap.log`) and 353.9 (`-rss-8.log`) are in files.
- **S7 is PASS for the mechanism.** The 4 h value was shortened to 150 s, which is not the shipped value.
- **T1 FAIL stands.** The md gives the 30.01 s timing and the 4 of 4 count. `L-SC3-SC4-runs-timeout-default.log` holds only the response bodies. It also holds the `ax_snapshot` body from the scale-0.5 run (deadline 5000), so the file name is misleading.
- **T3 FAIL stands, with one nuance.** It was observed 4 of 4 under `DEADLINE_SCALE=0.5`. At the shipped scale, one try after the untyped 502 answered 200.
- **Missing NOT-RUN rows:** C3 (person sub-cap) and the SC4 clause "a challenge page is a block outcome with at most one recovery".

### L-SC5-SC6-docs-zdr
- **SC6.7 is PASS for a same-org read only.** The SC6 clause "`GET /v1/scrape/{id}` is org-bound" needs a second org, which is NOT-RUN (needs the user).
- **X19.1, X19.2 and X19.4 match the runbook exactly, but the X-19 row is PARTIAL.** The values are `[redacted:password]` x10 then `Enter`, and `[redacted:secret]` in mode `all`, in the receipts and the events. Runbook 3.4 says "[USER] provides the user token", and the steps used the stub with an `initiated_by:"agent"` label. X19.1's "PASS (stub: PARTIAL)" should read PARTIAL in the summary. X19.7 (legacy row, `redacted_input`) is NOT-RUN.
- **SC5 is PARTIAL.** One org on the edge only was observed: inline ZDR markdown, 0 artifacts, 0 marker/host/title lines at `RUST_LOG=debug`, with a positive control. Not run:
  - the cross-org probes, including the `PageCache` `prev_fingerprint` probe
  - the canary scan on the gateway and the orchestrator (plan line 252)
  - `ws4g-live-zdr-token-probe.sh`
  - `ws4-image-probes.sh` (it refuses the live edge by design)
  - the cache-purge-and-forensics and NATS-erasure evidence scripts, which runbook SC5 names and the leg does not mention at all

### L-X12-D16
- **X12.5f (people) and X12.5h (takeover) PASS become PARTIAL.** The script's `QUARRY_TOKEN` is [USER] in runbook 3.1, and the stub was used. The leg says so itself for 5f but labels it PASS.
- **X12.6 is kept as PASS, but questioned.**
  - The resume start answered 200 and took a new lease and a browser slot. The refusal came at the first step.
  - The 402 body says `resumable:false`, yet the resume start was accepted.
  - Whether "refused at once" means at the start call or at the first step is for the owner.
- **X12.7 "FAIL as written, PASS with the real path" is recorded as PASS (corrected query) plus a runbook defect.** The runbook query is wrong; the latch is stored. `L-X12-D16-checkpoints-count-final.txt` shows aggregates only (`steps` 4). The per-run value for the SSE run (`steps|closed|5`) is in the md only.
- **X12.9 "PASS (info)" becomes INFO.**
- **D16.2 is kept as PASS.** The DSN-file wrapper (D16.1) did not run. The same unchanged SQL ran through `docker exec ingestion-postgres psql`, the numbers are recorded, and that meets "the count runs and its numbers are recorded". All counts are 0: there is no legacy profile on live.
- **D16.3 is kept as PASS.** Runbook 3.2 does not mark the stamp step [USER], so the stub person is acceptable. `stamped 1` and then 2 were observed.

## Results table (runbook section 6 shape)

| SC / list A / list B item | result | evidence file |
|---|---|---|
| Prep: services healthy, edge health/ready/version | PASS | P0-live.md (a, b) |
| Prep: flag defaults (runbook 1 step 5) | PASS | P0-live.md (c), P0-live-flags-end.txt |
| Prep: Rust lane, `docker compose config`, browser-grant preflight | NOT-RUN | none |
| SC1 prod-mode browser path (L2 legs A to C, log scan) | NOT-RUN (needs the user) | none |
| Restart check (L2 section 6, R.1 to R.7) | NOT-RUN (needs the user and owner go-ahead) | none |
| SC2a raw script denied (person, three surfaces) | PASS | L-SC2a-authority.md (E1 to E6), sc2a-ws-evaluate2.log |
| SC2a takeover with a user token labelled agent | PARTIAL (dev stub, not the [USER] token) | L-SC2a-authority.md (T1 to T8) |
| SC2a overall | PARTIAL | L-SC2a-authority.md |
| SC2b Model Plane run pauses (W2) | NOT-RUN (needs a Model Plane run) | none |
| SC3 caps, sweeper, idle expiry, checkpoint, close with viewer, backstop | PASS (backstop shortened to 150 s) | L-SC3-SC4-runs.md, -cap.log, -phaseA.log, -phaseB.log |
| SC3 RSS at 8, 16, 32 contexts | PARTIAL (8 and 16 recorded, 32 NOT-RUN) | L-SC3-SC4-runs-rss-8.log, -rss-16.log |
| SC3 overall | PARTIAL | L-SC3-SC4-runs.md |
| SC4 two-session viewport probe, generation stays 1 | PASS (frame-size claim unproven) | L-SC3-SC4-runs-viewport.log |
| SC4 typed TIMEOUT at the shipped deadline | FAIL | L-SC3-SC4-runs-timeout-default.log, L-SC3-SC4-runs.md (T1) |
| SC4 typed TIMEOUT at scale 0.5 | PASS | L-SC3-SC4-runs-phaseA.log |
| SC4 next step after TIMEOUT works | FAIL | L-SC3-SC4-runs-phaseA.log, L-SC3-SC4-runs.md (T3) |
| SC4 sandbox on (zero `--no-sandbox`) | PASS | L-SC3-SC4-runs.md (X1) |
| SC4 overall | FAIL | L-SC3-SC4-runs.md |
| SC5 one-org ZDR scrape/stream/agent: inline, 0 artifacts, 0 log content | PASS | L-SC5-SC6-docs-zdr-zdr-run.txt, -zdr.sh |
| SC5 cross-org probes, PageCache `prev_fingerprint`, ZDR token probe, gateway/orchestrator canary, purge/forensics, NATS erasure, ws4-image-probes | NOT-RUN (needs the user; image probes refuse live) | none |
| SC5 overall | PARTIAL | L-SC5-SC6-docs-zdr.md |
| SC6 PDF and DOCX inline text, PNG and ZIP 415, `DOC_ROUTING=on` | PASS | L-SC5-SC6-docs-zdr-sc6.log |
| SC6 `GET /v1/scrape/{id}` org-bound (second org) | NOT-RUN (needs the user) | none |
| SC7 W1 crawl clause (default 1000, `clamped_by:["default"]`) | PASS | L-SC7-billing-a.raw, L-SC7-billing-b.raw |
| List A DATAPLANE-OPTION-B | PASS (watch covers 19:49 to 19:53 plus SC2a M1) | P0-live.md (f1, f2, f5), P0-live-edge.log |
| List A DATAPLANE-OPTION-B gateway relay as `model_error` | NOT-RUN (needs the user) | P0-live.md (f3) |
| List A OPENAPI-INTERNAL (not routed at the ingress) | PASS | P0-live.md (g1, g2) |
| List A internal-token guard under the dev bypass (extra) | FAIL (dev only) | P0-live.md (g3) |
| List A OPENAPI-NULL | PASS (static, no stack check needed) | P0-live.md (g4) |
| List A ROBOTS-RFC quick row (map filter) | PASS; parser questions NOT-RUN | P0-live.md (g5) |
| List A BILLING-IDEMPOTENCY | PARTIAL | L-SC7-billing.md (BI.1 to BI.5), -idem1.raw, -idem2.raw, -handofffail*.raw, -recover.raw |
| List A MP-GATEWAY-AUTH | PARTIAL (label branch on stub; service principal NOT-RUN) | L-SC2a-authority.md (R1 to R6), sc2a-ws-reload-agent.log |
| List A TAKEOVER enforcement | PARTIAL (dev stub) | L-SC2a-authority.md (T1 to T8) |
| List B X-12 shadow day and takeover counters | PASS | L-X12-D16-shadow-steps.log, -metrics-shadow.log |
| List B X-12 script with `on` | FAIL (cost bound; and stub token) | L-X12-D16-script-on-default-action.log, -script-on-navigate.log |
| List B X-12 cost bound by hand | PASS | L-X12-D16-cost-hand.log |
| List B X-12 resume of an idle-expired run refused | PASS (questioned: start 200, refusal at first step) | L-X12-D16-resume.log |
| List B X-12 checkpoint latch `steps` | PASS with the corrected query (runbook query wrong) | L-X12-D16-checkpoints-count.txt, -checkpoints-count-final.txt |
| List B X-12 SSE shows one `agent.budget_exhausted` | FAIL | L-X12-D16-sse.raw, -sse-steps.log |
| List B X-12 takeover bound (decision 3.4 step 4) | PARTIAL (dev stub, [USER] token not used) | L-X12-D16-script-on-navigate.log |
| List B X-12 overall | FAIL | L-X12-D16.md |
| List B D16 legacy count | PASS (all 0; SQL via docker exec, wrapper not run) | L-X12-D16-d16-count.log |
| List B D16 stamp counter `stamped` | PASS | L-X12-D16-metrics-profile-after.log, -metrics-profile-final.log |
| List B D16 `skipped_service` | NOT-RUN (needs a service principal) | none |
| List B D16 two-user enforce evidence | NOT-RUN (needs the user) | none |
| List B X-10 legacy, observe, enforce, unreachable, redirect legs | PASS | L-X10-robots-legacy.log, -observe.log, -enforce.log, -unreachable.log, -redirect.log, x10/ |
| List B X-10 D2-161 cached page under enforce | PASS (direct `run_page` only) | L-X10-robots-d2161.log |
| List B X-10 composer row (nb and en) | NOT-RUN (needs the user) | none |
| List B X-10 soak | NOT-RUN (days; owner) | none |
| List B X-19 receipts, both legs | PARTIAL (values match; stub instead of the [USER] token) | L-SC5-SC6-docs-zdr-x19-agent-receipts.txt, -x19-agent-events.tsv, -x19-all-receipts-keys.txt, -x19-all-events-keys.txt |
| List B X-19 legacy row and `redacted_input` | NOT-RUN | none |
| List B ADR-6 proof on fixture hosts | NOT-RUN (needs the user; only after SC1 to SC7) | none |
| Flags back to baseline at session end | PASS | flags-log.md, L-X12-D16-flags-end.txt; reviewer's `live.sh flags` at 21:05:44Z |

## Depth (FAIL and PARTIAL causes)

1. **SC4 T1 FAIL: the typed TIMEOUT loses the race at the shipped navigate deadline.**
   - The library has a fixed 30 s per-command timer: `chromiumoxide-0.9.1/src/handler/commandfuture.rs:51`, with `REQUEST_TIMEOUT` at `handler/mod.rs:34`.
   - Our navigate deadline is also 30 s: `quarry-browser/src/deadlines.rs:179`.
   - The comment at `quarry-browser/src/chromiumoxide.rs:888-891` claims that a 40 s `request_timeout` lets the typed error win. It does not apply to per-command timers.
   - The untyped error carries `details.url` (`chromiumoxide.rs:3815`). That puts a URL in an error body for a ZDR caller.
2. **SC4 T3 FAIL: the first step after a navigate TIMEOUT fails.** It is an `ax_snapshot` 504 after 15 to 21 s. The leg names no file:line. The unit test `agent_step_timeout.rs` uses a driver double, so it cannot see this.
3. **X-12 FAIL: the SSE stream drops `agent.budget_exhausted`.**
   - `is_user_facing_event` (`quarry-edge/src/agent_routes/live.rs:8-21`) and `event_name` (`live.rs:42-57`) do not include it.
   - The event is emitted by `agent_routes/budget.rs:125-150` and uses up a seq (the gap at seq 12 in the capture).
4. **X-12 FAIL: the script cannot prove the cost bound on chromium.**
   - The default `wait` action needs a page (`quarry-browser/src/chromiumoxide.rs:1239-1262`), and the script never navigates.
   - A navigate is priced at 0.001 plus bytes (`quarry-runtime/src/action_cost.rs:25`), which is above the script's 0.00025 USD bound.
5. **X-12 runbook defect.** The latch is stored at `state->'budget'->>'exhausted'` (`quarry-runtime/src/step_receipts.rs:121`), not at `state->>'budget_exhausted'` (runbook 3.1, line 56).
6. **Internal-token guard FAIL (dev only).**
   - `check_internal_token` returns OK under the dev bypass (`quarry-edge/src/change_routes.rs:455`).
   - That opens `change/record`, the schedules preflight and `DELETE /v1/internal/orgs/:org/profiles` (`internal_routes.rs:57`).
   - Production refuses to start with the bypass on (`main.rs:171-200`).
7. **BILLING-IDEMPOTENCY PARTIAL: usage events never reach billing-core on live.**
   - The edge publishes to `ingestion-nats`, which has no `usage.>` stream.
   - billing-core subscribes on `controlplane-nats` (`apps/Control Plane/billing-core/internal/nats/subscriber.go:41`).
   - `NatsUsageMeter` drops the publish ack (`quarry-runtime/src/usage.rs`, `Ok(_ack)` branch), so the loss is silent.
   - Also, the event id was not seen on the wire: there was no subscriber credential.
8. **SC2a, TAKEOVER, MP-GATEWAY-AUTH, X-19 and the X-12 script/takeover PARTIAL: the dev stub stood in for the [USER] token.**
   - Only the label branch of `derive_initiator` ran (`run_live.rs:84`).
   - The service-principal branch did not run on the stack.
   - Observe mode has a warn line (`admission.rs:342`) but no would_reject counter.
9. **SC3 PARTIAL.** RSS at 32 contexts with `QUARRY_BROWSER_MAX_SESSIONS=48` was not run, and neither was the person sub-cap (C3). The leg names no file:line.
10. **SC5 PARTIAL.** It needs the user (two orgs, a ZDR token), the gateway and orchestrator canary scan, and the purge, forensics and NATS-erasure scripts. `ws4-image-probes.sh` refuses the live edge by design. The leg names no file:line.
11. **P0 d0 PARTIAL: metrics are off on live by default** (`docker-compose.yml:278`). Dev needs no token (`metrics.rs:100`), and the runbook's token command fails on live.
12. **P0 g2 PARTIAL (serve path): the page-image producer is down.** The Data Plane NATS refuses the edge with "authorization violation" at boot. The cause is likely an env-credential mismatch. The leg names no file:line.

Notes on PASS rows, for the next unit and not counted as failures:
- The orchestrator never sends `prev_fingerprint` (`services/quarry-orchestrator/internal/activities/activities.go:229-239`), so the D2-161 guard (`crates/quarry-edge/src/routes.rs:1722-1731`) protects a path that crawls do not use.
- In enforce, the robots refusal of a link is not counted in `quarry_robots_decisions_total`.
- The legacy job field `effective.robots:"respect"` shows while `CRAWL_ROBOTS=off`.
- A resumed exhausted run holds a browser slot until its first step.
- The legacy 429 cap counts takeover steps, and the new ledger does not.
- The runbook X-19 expected values do not apply to an unlabelled person step: the human branch stores `[redacted]`, including for `Enter`.
