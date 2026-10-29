# CHECK: independent check of the 2026-10-29 live-dev re-run (R-SC4, R-X12)

- Checked 2026-10-28T23:30Z (UTC). Read-only: no stack or file change other than this file.
- Read: every file in this folder (29 files) and "## Re-run 2026-10-29" in `../flags-log.md`.
- Live checks run now:
  - `live-flags.env` size: 0 bytes (mtime 23:27:48Z).
  - `bash live.sh flags | diff - ../P0-live-flags-end.txt`: empty (exit 0).
  - `docker ps -a --filter name=qlive-rerun`: none. `--filter name=qlive`: none.
  - `quarry-edge`: Up, `healthy`, started 23:27:41Z, RestartCount 0.
  - `GET http://127.0.0.1:8082/ready`: HTTP 200, `status:"ready"`, `sessions_open 0`.
  - Container env: `QUARRY_EDGE__BUDGET_ENFORCEMENT=shadow`, `QUARRY_EDGE__METRICS_ENABLED=0` (the compose default), no `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS`, no `QUARRY_BROWSER_DEADLINE_SCALE`.

## Results

| row | result | evidence file |
|---|---|---|
| R-SC4 T1 (shipped navigate deadline) | PASS (confirmed) | R-SC4-t1-t3-shipped.log (6 hangs: 504 at 25.019 to 25.056 s), R-SC4-t5-final-container.log (2 hangs: 25.051 and 25.063 s), R-SC4-t1-hang-body.raw. The body is `TIMEOUT`, `deadline_ms:25000`, `effect:"unknown"`, `op:"navigate"`. Recount: 20 TIMEOUT bodies across the SC4 logs, and 0 contain `http://`, `crawl.test` or `.html`. |
| R-SC4 T3a (next step navigate) | PASS (confirmed) | R-SC4-t1-t3-shipped.log (0.227, 0.351, 0.200 s), R-SC4-t5-final-container.log (0.280 s). All 200, title "Fixture A", `ax_snapshot TIMEOUT in next body: 0`. |
| R-SC4 T3b (next step wait) | PASS (confirmed) | R-SC4-t1-t3-shipped.log (0.224, 0.182, 0.164 s), R-SC4-t5-final-container.log (0.190 s). All 200, title "Fixture A", 0 ax_snapshot TIMEOUTs. |
| R-SC4 T2 (scale 0.5) | PASS (confirmed) | R-SC4-t2-scale05.log (4 hangs: 504 at 12.528 to 12.538 s), R-SC4-extra-scale05.log E1 (12.542 and 12.602 s, back to back), R-SC4-t2-hang-body.raw (`deadline_ms:12500`). Next steps: navigate 0.233/0.327 s, wait 0.160/0.161 s, E1 navigate 0.252 s. |
| R-SC4 T4 (held img, run already has a page) | PASS (confirmed) | R-SC4-t4-held.log tries 1 and 2 (504 at 25.016 and 25.013 s), R-SC4-t5-final-container.log t5h try 1 (25.052 s). The next wait is 200 in 0.153 to 0.202 s with title "R-SC4 held image page" and 10 AX nodes. browser-session currentUrl is the held page. |
| R-SC4 T4b (held/slow page as the first step) | FAIL (confirmed) | R-SC4-t4-held.log try 3, R-SC4-t5-final-container.log t5h try 2, R-SC4-extra-scale05.log E2, R-SC4-first-step-wait-body.raw. Each shows the expected 504 TIMEOUT, then the wait returns 502 `DRIVER_FAILED` `no_current_page`, `retryable:false`, in 0.113 to 0.119 s, with currentUrl "". A navigate after it returns 200. |
| R-SC4 T5a (`/ready` generation) | PASS (confirmed) | All 5 SC4 .log files: `ready={"generation":1,"sessions_open":0}` after every close. Generation is 0 only at the start of each fresh container (t2 start, t5 start). |
| R-SC4 T5b (no relaunch/crash lines) | PASS (confirmed, scope note) | R-SC4-edge-scan.log: 0 matches in the scale-0.5 container (143 lines) and in the final container (67 lines). This is a count summary written by the agent; the raw logs are not kept and both containers have since been replaced. The first container is not covered, but its rows were repeated on the final container. |
| R-SC4 T5c (slow host in the edge log) | PASS (confirmed) | R-SC4-edge-scan.log: 0 lines contain `slow.crawl.test`, `slow.html` or `held.html`. ERROR lines carry status, latency and the run path only: 7x 504 at about 12.5 s, 4x 504 at about 25 s, 2x 502 at 109/111 ms. |
| R-SC4 closes | PASS (confirmed) | `close: HTTP=200` appears 19 times across the SC4 logs. |
| R-X12 X12.0 self-test | PASS (confirmed) | R-X12-selftest.log: `SELF-TEST PASS`, `exit=0`. |
| R-X12 X12.4 flip | PASS (confirmed) | R-X12-flags-on.txt (`BUDGET_ENFORCEMENT=on`, `AGENT_DEFAULT_MAX_STEPS=5`), R-X12-sse-steps.log (`enforcement:"on"`, `max_steps:5`). The boot-log lines quoted in the summary (mode on, grant claim ignore) are not saved in any evidence file. |
| R-X12 X12.5 script x2 | PASS (confirmed) | R-X12-script-run1.log, R-X12-script-run2.log: each `7 PASS, 0 FAIL, 0 NOT-RUN`, `exit=0`, with enforcement on. The verdicts are the script's own. |
| R-X12 X12.5a ceilings | PASS (confirmed) | script-run1/2.log: `clamped to 300 steps, 3600 s, 20 USD by "ceiling"`. |
| R-X12 X12.5b steps | PASS (confirmed) | script-run1/2.log: `5 steps, then 402 steps; 5 receipts`. |
| R-X12 X12.5c runtime | PASS (confirmed) | script-run1/2.log: `402 runtime after 5 s`. |
| R-X12 X12.5d cost | PASS (confirmed) | script-run1/2.log: `4 steps within 0.0013 USD ... then 402 cost ... before it ran`. |
| R-X12 X12.5f people | PARTIAL (as reported) | script-run1/2.log: `20 person steps ... all 200`. The identity was the dev stub, not a real [USER] token. |
| R-X12 X12.5g guard | PASS (confirmed) | script-run1/2.log: `the model's 4th step is 429, the person's step is 200`. |
| R-X12 X12.5h takeover | PARTIAL (as reported) | script-run1/2.log, R-X12-metrics-end.log: `quarry_agent_takeover_exempt_steps_total 10`, histogram count 10, sum 0.547 s, all observations <= 0.1 s. The identity was the dev stub. |
| R-X12 X12.8 SSE | PASS (confirmed) | R-X12-sse-steps.log: steps 1 to 5 return 200, steps 6 to 8 return 402 `BUDGET_EXHAUSTED steps 5/5`, then DELETE 200. R-X12-sse.raw: `id:` values 2 to 13 are contiguous, with exactly 1 `agent.budget_exhausted` (id 12). Its payload has only dimension, estimated, limit, limit_source, spent and unit (no URL). |
| R-X12 X12.7 checkpoint | PASS (confirmed) | R-X12-checkpoints.txt shows 1 row for the SSE run: `steps | closed | 5`, and `rows_for_run 1, exhausted_steps 1`. The window totals (cost 2, runtime 2, steps 5, none 8) are all closed. The file does not show the `since 23:25:39Z` filter that the .md describes. |
| R-X12 X12.9 counter | PASS (info, confirmed) | R-X12-metrics-before.log: 97 lines, 0 budget lines. R-X12-metrics-pre-sse.log: steps 4, cost 4, runtime 2. R-X12-metrics-post-sse.log: steps 7, cost 4, runtime 2. All series are `mode="on"`. |
| R-X12 X12.10 revert | PASS (confirmed, live re-check) | R-X12-flags-diff.txt is 0 bytes and R-X12-flags-end.txt is present. Live now: live-flags.env is 0 bytes, the diff is empty, the edge is healthy, `/ready` returns 200 with sessions_open 0, and the container env is at baseline (see header). |
| Flag ledger (flags-log.md, Re-run section) | PASS (confirmed) | 4 changes and 4 reverts. `QUARRY_BROWSER_DEADLINE_SCALE` was set at 23:18:02Z and reverted at 23:20:33Z. `METRICS_ENABLED`, `BUDGET_ENFORCEMENT` and `AGENT_DEFAULT_MAX_STEPS` were set at 23:25:35Z and reverted at 23:27:37Z. |
| Helper containers | PASS (live) | `docker ps -a` shows no `qlive-rerun-*` or `qlive*` container. |
| Secret scan of the evidence | PASS (note) | No JWT, sk-/ghp_/xox/AKIA key, private key, credentialed URL or long hex/base64 blob was found. The only credential-shaped strings are on line 6 of R-SC4.md and R-X12.md: a `Bearer` value that is the 12-character all-lowercase dev-stub literal used under `QUARRY_EDGE_AUTH_DEV_BYPASS=1`. It is not a real secret, but it is a bearer literal in a committed-path file. |

## Downgrades

None. Every PASS row is backed by its evidence file with matching status, timing and count. These scope notes do not change a result:

- T5b/T5c cover 2 of the 3 edge containers. The first container's log was lost when the T2 flag change recreated it. The leg repeated T1, T3a, T3b and T4 on the final container to cover this, and the evidence is a count summary, not raw logs.
- X12.4: the boot-log lines in the leg summary are not saved, but the row is carried by the flags-on file and the SSE start reply.
- X12.7: the window-wide query's `updated since` filter is not shown in the file. The per-run query (the row's actual check) is fully shown.
- X12.10 / flag diff: `live.sh flags` lists neither `QUARRY_EDGE__METRICS_ENABLED` nor `QUARRY_BROWSER_DEADLINE_SCALE`, so the "diff is empty" check cannot see whether those two flags were reverted. I checked the container env directly: METRICS_ENABLED=0 (the default) and DEADLINE_SCALE is absent.

## Remaining open issues

1. **T4b FAIL (not fixed):** a navigate TIMEOUT on a run's first step leaves no page. `open_tab_page` closes the tab, and the `goto` recovery only runs `if !created` (`quarry-browser/src/chromiumoxide.rs:3920`, `:1597-1600`). The next observation returns 502 `DRIVER_FAILED` `no_current_page` with `retryable:false`, even though a navigate on the same run works. Fix options are in R-SC4.md Depth 1.
2. **X12.5f people and X12.5h takeover are PARTIAL** until the [USER] run with a real token (decision 3.4 step 4).
3. **The SSE stream carries the navigate URL** in `action.started` (`payload.action.url`) and `observation.ready` (`payload.url`) (R-X12-sse.raw). This is outside the X12.8 row, but it is a ZDR question for the owner if the stream is persisted or relayed.
4. **Gap in the flag-diff tool:** `live.sh flags` does not include `METRICS_ENABLED` or `QUARRY_BROWSER_DEADLINE_SCALE`. Both were changed in this re-run, so future revert checks should add them or inspect the container env.
5. **NATS `authorization violation` at edge boot:** 1 line in the scale-0.5 container (R-SC4-edge-scan.log, 23:18:07Z) and 2 lines in the current container. This is pre-existing (it is also noted in P0-live.md, L-X10-robots.md and REVIEW.md) and was not reported in R-SC4.md's T5 row.
6. **Dev-stub bearer literal** in R-SC4.md and R-X12.md line 6. It is harmless as a dev stub, but it could be redacted to `Bearer <dev-stub>` for hygiene.
7. Not run in this re-run: `pdf` at its new 25 s deadline, X12.6 (resume of an idle-expired run), and the shadow day. chromiumoxide `WS Invalid message` WARN noise (7 per container) is still present.

## R2-SC4 (T4b fix)

- Checked 2026-10-29 (UTC, after the leg's 01:15:06Z end). Read-only: no stack or code change; this section is the only file write.
- Read: R2-SC4.md, all 10 R2-SC4-* files (shipped, scale05, final-container, edge-scan, ready-and-end logs; flags-end.txt; 4 .raw bodies), the R2-SC4 lines (70-71) in `../flags-log.md`, and the cited code lines in the worktree.
- Live checks run now: live-flags.env is 0 bytes (stat); `live.sh flags | diff - P0-live-flags-end.txt` is empty (exit 0); `docker ps -a` has 0 `qlive-rerun-*` / `qlive*` containers; quarry-edge is running, healthy, started 01:12:16Z, RestartCount 0; the container env has no `QUARRY_BROWSER_DEADLINE_SCALE`; `GET 127.0.0.1:8082/ready` returns HTTP 200, status ready, browser slot generation 1, sessions_open 0, probe_failures 0. Read-only SELECT on ingestion-postgres (db quarry_edge) for the 22 run ids parsed from the three .log files: `closed|22`.

| row | result | evidence checked |
|---|---|---|
| T4b-1 (first step: slow page) x3 + 1 final | FAIL (confirmed) | shipped.log tries 1-3 + final-container.log: navigate 504 in 25.103 / 25.105 / 25.115 / 25.174 s, `TIMEOUT`, `deadline_ms:25000`, `op:"navigate"`, url-in-body 0. Next wait 403 in 0.214 / 0.173 / 0.205 / 0.220 s, `BAD_REQUEST`, "outside the run's allowed domains" (t4b1-wait-body.raw). currentUrl `about:blank` in all 4. Navigate after 200 in 0.252 / 0.296 / 0.394 / 0.302 s, "Fixture A"; wait after 200 in 0.16-0.21 s. Receipts show step 1 wait `completed` in all 4. Code path confirmed: chromiumoxide.rs:1604-1611 (ByCaller commits about:blank), step.rs:150-165 (receipt appended) and :203-208 (checkpoint saved) before :210-216 (403), lib.rs:300-307 (http/https only), types.rs:613-620 (`status_err` always `BadRequest`) vs step.rs:370-375 (`SecurityBlocked`), lifecycle.rs:563-573 (resume refuses a non-empty, non-allowed checkpoint URL). |
| T4b-2 (first step: held page) x3 + 1 final | PASS (confirmed) | 504 in 25.188 / 25.148 / 25.101 / 25.180 s, `deadline_ms:25000`, no URL. Wait 200 in 0.176 / 0.193 / 0.184 / 0.171 s, url held.html, title "R-SC4 held image page", 10 AX nodes (t4b2-wait-body.raw), `no_current_page` 0. currentUrl held.html. Navigate after 200 in 0.235 / 0.272 / 0.228 / 0.337 s. |
| T1 x4 / T3a x2 / T3b x2 | PASS (confirmed) | Hang 504 in 25.035 / 25.026 / 25.036 / 25.047 s, `TIMEOUT`, `deadline_ms:25000`, url-in-body 0, receipt `failed`. T3a next navigate 200 in 0.222 / 0.261 s; T3b next wait 200 in 0.195 / 0.189 s (receipt step 2 `wait`), url fixture-a, "Fixture A", 37 AX nodes, `ax_snapshot` 0. |
| T4 x1 | PASS (confirmed) | 504 in 25.025 s; wait 200 in 0.226 s, held.html, 10 AX nodes; wait after 200 in 0.193 s. |
| T2 scale 0.5 (4 runs, 2 first-step) | PASS on the deadline (confirmed); first-step slow sub-case = T4b-1 FAIL | scale05.log: 504 in 12.557 / 12.542 / 12.627 / 12.622 s, `deadline_ms:12500` (t2-first-step-hang-body.raw), no URL. Later-step next navigate 200 0.290 s, next wait 200 0.226 s. First-step held: wait 200 0.232 s with held title, navigate 200 0.276 s. First-step slow: wait 403 `BAD_REQUEST` 0.212 s, currentUrl about:blank, navigate 200 0.298 s. flags-log.md lines 70-71: set 01:10:33Z, reverted 01:12:13Z. |
| T6 (5 new runs, first step fixture-a) | PASS (confirmed) | 200 "Fixture A" step 0 in 0.460 / 0.414 / 0.296 / 0.354 / 0.419 s: min 0.296, median 0.414. Recomputed from the logs: the 12 step-0 fixture-a navigates in R2-SC4 (5 T6 + 5 shipped warm-ups + 2 scale05 warm-ups) min 0.279, median 0.359; the 16 in the R-SC4 logs min 0.256, median 0.336, max 0.513. |
| T5 (generation, relaunch, log content) | PASS (confirmed, scope note) | Every .log: `generation:0` at container start, `1` after every close and at the end; ready-and-end.log shows slot ready, probe_failures 0 for all 3 containers. edge-scan.log (288 / 124 / 88 lines): 0 relaunch/crash/lost/disconnect/dead/recycled/unhealthy/panic; 0 lines with slow.crawl.test, held.html, slow.html, about:blank, fixture-a.crawl.test. ERROR lines: 3 NATS `authorization violation` (1 per container, at boot) + 17 `504` lines (11 + 4 + 2) carrying status, latency and run path only, matching the 17 hang steps one-for-one by run id. WS Invalid message WARN 21 / 6 / 4. |
| closes / end state | PASS (confirmed, live re-check) | `close: HTTP=200` 22 times (16 + 4 + 2); 22 distinct run ids; live SELECT `closed|22`. Live checks above all pass. |
| Flag ledger | PASS (confirmed) | 1 change, 1 revert (`QUARRY_BROWSER_DEADLINE_SCALE` 0.5, 01:10:33Z -> 01:12:13Z). R2-SC4-flags-end.txt equals the baseline (empty diff now). |
| Secret scan of the evidence | PASS | 0 matches for JWT, `Bearer <value>`, sk-/sk-ant/ghp_/AKIA keys, `password/secret/token/api_key=` values or PEM headers in the 11 R2-SC4 files and flags-log.md. The only 40+ character strings are `/v1/agent/runs/run_...` paths (17) and two env var names. R2-SC4.md line 6 uses the redacted `Bearer <dev-stub>` form. |

### Downgrades

None. Every PASS row matches its evidence in status, timing and count, and the one FAIL row (T4b-1) is confirmed with its cited code. Notes that do not change a result:

- T5b/T5c are count summaries written by the leg agent; the raw `docker logs` of the 3 containers are not kept in the folder (the scan's 17 ERROR lines do match the 17 hang steps by run id and latency).
- The `live.sh flags` diff cannot see `QUARRY_BROWSER_DEADLINE_SCALE` (open issue 4 above); the container env was checked directly and the variable is absent.
- t4b1-wait-body.raw has no `details` key, while the .log line shows `"details":null` (the log reformats the body). Cosmetic.
- T2's row is mixed: PASS for the deadline, the first-step slow sub-case carries the T4b-1 FAIL.
- The `curlimages/curl:8.10.1` image pulled for the helper probes is still present locally (as the leg summary says).

### Remaining open issues (R2-SC4)

1. **T4b-1 FAIL (not fixed):** after a first-step navigate TIMEOUT on a page that never commits, the tab is on `about:blank` and the next observe step is refused 403 (`BAD_REQUEST`, "outside the run's allowed domains"); a navigate still works. Side effects: the refused wait's receipt is `completed`; the code is `BAD_REQUEST` where the shared path uses `SECURITY_BLOCKED`; untested, a checkpoint holding `about:blank` would make a resume 403. Fix options in R2-SC4.md Depth 1. Open issue 1 above (502 `no_current_page`) is superseded for the held-page case (T4b-2 now PASS) and replaced by this one for the never-committing case.

## R3-SC4 (T4b-1 edge fix)

- Checked 2026-10-29 (UTC, after the leg's 02:48 end). Read-only: no stack or code change; this section is the only file write.
- Read: R3-SC4.md, all 21 R3-SC4-* files (s1, s1b, shipped, rs, final-container, edge-scan, ready-and-end logs; flags-end.txt; helper.sh.txt; rs-receipts.txt; 7 .raw and 4 .json bodies), the R3-SC4 lines (72-74) in `../flags-log.md`, and the cited code lines for Depth 1 and 2 in the worktree (`types.rs:101-114` counts successful steps only, `step.rs:155` reserves a key per admitted step, `lifecycle.rs:716` `.with_resume_step(entry.summary.step)`, `lifecycle.rs:571-575` recovery navigate to the checkpoint URL, `chromiumoxide.rs:1269-1273` `no_current_page`).
- Live checks run now: live-flags.env is 0 bytes (stat; mtime 02:50:55Z, after the revert, still empty); `live.sh flags | diff - P0-live-flags-end.txt` is empty (exit 0), and R3-SC4-flags-end.txt equals P0-live-flags-end.txt; `docker ps -a --filter name=qlive` has 0 rows; quarry-edge is running, healthy, started 02:46:41Z (the revert recreate), RestartCount 0; the container env has `RUN_SWEEPER_DRY_RUN=1` and no `RUN_SWEEP_INTERVAL_S`, `AGENT_RUN_IDLE_TTL_S` or `QUARRY_BROWSER_DEADLINE_SCALE`; `GET 127.0.0.1:8082/ready` returns HTTP 200, status ready, browser slot generation 1, sessions_open 0, probe_failures 0. Read-only SELECT on ingestion-postgres (db quarry_edge) for the 18 run ids parsed from the five .log files: `closed|18`; the RS run's row is `3|closed|http://fixture-a.crawl.test/`.

| row | result | evidence checked |
|---|---|---|
| T4b-1 (first step: slow page) x4 + 1 final | PASS (confirmed; was FAIL in R2-SC4) | shipped.log tries 1-4 + final-container.log: navigate 504 in 25.103 / 25.162 / 25.116 / 25.132 / 25.163 s, `TIMEOUT`, `{deadline_ms:25000, effect:"unknown", op:"navigate"}`, url-in-body 0 (t4b1-hang-body.raw). Next wait 200 in 0.169 / 0.189 / 0.186 / 0.191 / 0.202 s, url `about:blank`, step 1, 3 AX nodes, `action_outcome verified wait_completed`, `policy_denials []` (t4b1-wait-body.json); `no_current_page` / `SECURITY_BLOCKED` / `BAD_REQUEST` 0 in all 5. currentUrl `about:blank`, live. Navigate fixture-a 200 in 0.260 / 0.235 / 0.309 / 0.350 / 0.331 s, "Fixture A", step 2; wait after 200 in 0.158-0.199 s. Receipts in all 5: `0 navigate failed TIMEOUT, 1 wait completed, 2 navigate completed, 3 wait completed`. |
| T4b-2 (first step: held page) x2 | PASS (confirmed) | 504 in 25.165 / 25.123 s, `deadline_ms:25000`, no URL. Wait 200 in 0.193 / 0.193 s, url held.html, title "R-SC4 held image page", 10 AX nodes. Navigate 200 in 0.299 / 0.336 s. Receipts `failed TIMEOUT, completed, completed, completed`. |
| S1a (about:blank as target) x2 | PASS (confirmed) | s1.log: 403 in 0.0048 / 0.0050 s, `SECURITY_BLOCKED`, "browser navigation is outside the run's allowed domains" (s1-blank-target-body.raw). Receipt count 0 -> 0 and 1 -> 1; currentUrl `""` / fixture-a unchanged. |
| S1b (off-list target; off-policy landing) x2 + x2 | PASS (confirmed, scope note) | Direct fixture-b target: 403 in 0.0046 / 0.0033 s, `SECURITY_BLOCKED`, no receipt (s1-offlist-target-body.raw). Landing: `/toblank.html` 200 in 0.211 / 0.123 s, then wait 403 in 1.574 / 1.565 s, `SECURITY_BLOCKED`, "browser action reached a domain outside the run's allowed domains" (s1b-landing-body.raw); receipt `step 2 wait failed SECURITY_BLOCKED` in both (s1b-receipts.json, cost 100 flat_table); checkpoint after the refusal `2|active|http://slow.crawl.test/toblank.html`; navigate fixture-a after it 200 in 0.176 / 0.153 s. |
| RS (resume after a blank-tab checkpoint) x1 | PASS (confirmed) | rs.log, run_01M62RQ7Z9YNQ065Z1EK3DQ19A: navigate slow.html 504 in 25.158 s; wait 200 in 0.212 s url about:blank; checkpoint `1|active|[]`. sessions_open 0 after 37 s; edge-scan shows 1 `run expired` line in the RS container. Step on it 404 `NOT_FOUND`, `run_not_live`, `resumable:true`, same `resume_run_id` (rs-expired-step-body.raw); checkpoint still `1|active|[]`. Resume 200 in 0.019 s, `error:null`, 0 "durable checkpoint URL is outside" (rs-resume-body.json). Navigate fixture-a 200 in 0.414 s, step 2; wait 200 in 0.171 s. Checkpoint `3|closed|[http://fixture-a.crawl.test/]` (and live SELECT now). |
| T1 (later-step hang) x2 | PASS (confirmed) | 504 in 25.037 / 25.030 s, `TIMEOUT`, `deadline_ms:25000`, 0 URLs. Next navigate 200 in 0.222 / 0.207 s, "Fixture A", 37 AX nodes, `ax_snapshot` 0; wait after 200 in 0.225 / 0.235 s. Receipts `completed, failed TIMEOUT, completed, completed`. |
| T6 (first step fixture-a) x3 + 1 final | PASS (confirmed) | 200 "Fixture A" step 0 in 0.292 / 0.330 / 0.373 / 0.399 s. Recomputed from the logs: the 9 step-0 fixture-a navigates (T6 x4, S1 try 2, S1b x2, T1 x2 warm-ups) sorted 0.292, 0.304, 0.330, 0.331, 0.344, 0.347, 0.365, 0.373, 0.399: min 0.292, median 0.344. |
| T5 (generation, relaunch, log content) | PASS (confirmed, scope note) | Each container: generation 0 at start (s1.log, rs.log, final-container.log), 1 after every close and at the end; ready-and-end.log: slot ready, generation 1, probe_failures 0, sessions_open 0, RestartCount 0 for all 3. edge-scan.log (319 / 83 / 76 lines): 0 relaunch/crash/lost/disconnect/dead/recycled/unhealthy/panic; 0 lines with slow.crawl.test, held/slow/toblank//redir, about:blank, fixture-a/fixture-b. ERROR lines: 3 NATS `authorization violation` (1 per container, at boot) + 12 `504` lines (10 + 1 + 1) + 1 `502` (110 ms, the Depth 1 wait in the RS container), status, latency and run path only. The 12 504 lines match the 12 TIMEOUT steps by run id (S1 x2 redirects, T4b-1 x5, T4b-2 x2, T1 x2, RS x1). |
| closes / end state | PASS (confirmed, live re-check) | `close: HTTP=200` 18 times (2 + 2 + 11 + 1 + 2); 18 distinct run ids; live SELECT `closed|18`. Live checks above all pass. |
| Flag ledger | PASS (confirmed) | flags-log.md lines 72-74: `RUN_SWEEPER_DRY_RUN` 1 -> 0, `RUN_SWEEP_INTERVAL_S` 15 -> 5, `AGENT_RUN_IDLE_TTL_S` 300 -> 30 set 02:44:10Z; all three reverted 02:46:37Z. Container env and the flags diff confirm the revert now. |
| Secret scan of the evidence | PASS | 0 matches for JWT, `Bearer <value>`, sk-/sk-ant/AKIA keys, `password/secret/token/api_key=` values, nats URLs with credentials, 40+ hex or 48+ base64 runs in R3-SC4.md, the 21 R3-SC4-* files and the R3-SC4 flags-log lines. R3-SC4.md uses the redacted `Bearer <dev-stub>` form. |

### Downgrades

None. Every PASS row matches its evidence in status, timing and count. Notes that do not change a result:

- S1b: R3-SC4.md says the checkpoint is `2|active|.../toblank.html` "before and after the refusal", but s1b.log records it only after the refusal (the earlier SELECT is `1|active|http://fixture-a.crawl.test/`, taken before the toblank navigate). The after value is enough for the claim.
- RS: the `idle_s=33` and 02:45:44Z details in R3-SC4.md are not in the saved files (edge-scan.log keeps counts only: 1 `run expired`, 0 `would_expire`); rs.log's "sessions_open 0 after 37 s" from 02:45:11Z is consistent.
- The leg summary's T5 line counts 12 ERROR 504 lines and leaves out the one ERROR 502 line (the Depth 1 wait); R3-SC4.md lists it.
- T5b/T5c are count summaries written by the leg agent; the raw `docker logs` of the 3 containers are not kept in the folder.
- live-flags.env was touched at 02:50:55Z, after the leg's end checks; it is 0 bytes and the edge has not been recreated since 02:46:41Z, so the running config is the baseline.

### Remaining open issues (R3-SC4)

1. **Wait right after a resume from an empty-URL checkpoint is 502 `DRIVER_FAILED` `no_current_page`** (rs-wait-after-resume-body.raw, `retryable:false`); a navigate then works. Confirmed in code: recovery navigation only runs for a non-empty checkpoint URL (`lifecycle.rs:571-597`), chromium has no tab until `goto` (`chromiumoxide.rs:1269-1273`).
2. **Duplicate receipt step key after a resume** (rs-receipts.txt: step 1 `wait completed` at 02:45:09Z and step 1 `wait failed DRIVER_FAILED` at 02:45:50Z). Confirmed in code: resume seeds from `summary.step` (`lifecycle.rs:716`), which counts successful steps only (`types.rs:101-114`), while keys are reserved for every admitted step (`step.rs:155`). Every T4b-1-shaped run that idles out and resumes hits it.
3. **Off-list redirect costs the full 25 s and answers `TIMEOUT`, not `SECURITY_BLOCKED`** (s1.log x2, s1-redirect-body.raw; the block is visible as `domain_grant_blocked` for fixture-b/ext in s1-redirect-next-egress.json).
4. Not run: open-web run, browser socket and attach-delegate step paths, and a resume from an old checkpoint that still holds `about:blank`.

## R4-SC4 (depth fixes)

- Checked on 2026-10-29 at about 10:20Z, after the leg ended at 10:13:50Z. The check was read-only: no stack, flag or code change, and this section is the only file write.
- **Read:** R4-SC4.md, all 25 R4-SC4-* files (the d1, d3, reg, rs, final, edge-scan, probe and ready-and-end logs; flags-end.txt; helper.sh.txt; rs-expired-lines.txt; rs-receipts.txt; 4 .raw bodies and 9 .json bodies), and the R4-SC4 lines (75-77) in `../flags-log.md`.
- **Live checks run now:**
  - live-flags.env is 0 bytes (stat). Its mtime is 10:15:20Z because `live.sh` line 15 runs `touch` on it whenever it is called; the file is still empty.
  - `live.sh flags | diff - P0-live-flags-end.txt` is empty (exit 0), and so is the diff against R4-SC4-flags-end.txt.
  - `docker ps -a --filter name=qlive` returns 0 rows.
  - quarry-edge is running and healthy. It started at 10:10:25Z (the revert recreate) with RestartCount 0. Its env has `RUN_SWEEPER_DRY_RUN=1` and no `RUN_SWEEP_INTERVAL_S`, `AGENT_RUN_IDLE_TTL_S` or `QUARRY_BROWSER_DEADLINE_SCALE`.
  - `GET 127.0.0.1:8082/ready` returns 200: status ready, browser healthy, generation 1, sessions_open 0, probe_failures 0.
  - A read-only SELECT (`BEGIN READ ONLY`, database quarry_edge) over the 26 run ids gave `closed|26`. The same SELECT found no run whose receipts repeat a step key.

| row | result | evidence checked |
|---|---|---|
| D3a (x4: 2 first-step, 2 later-step) | PASS (confirmed) | d3.log tries 1-3 and final.log: 403 in 0.148 / 0.134 s as a first step and 0.037 / 0.053 s as a later step. Every body has `SECURITY_BLOCKED`, `retryable:false` and `details {op:navigate, reason:domain_grant_blocked}` (d3a-first-body.raw, d3a-later-body.raw), and every leak check is 0. The receipt is `failed SECURITY_BLOCKED` at step 0 (first step) or step 1 (later step); d3a-first-receipts.json agrees. The next wait is 200 in 0.188 / 0.182 s on about:blank, or 0.166 / 0.167 s on fixture-a. The next navigate to fixture-a is 200 in 0.264 / 0.188 / 0.183 / 0.282 s. `tabs` is n=1 after the refusal and at the end of tries 1-3. |
| D3b (x2) | PASS (confirmed) | img.html returns 200 in 0.246 / 0.126 s with title "R4-SC4 offlist image", 10 AX nodes and `policy_denials []`. The same observation has `fixture-b.crawl.test/pixel.png block domain_grant_blocked` on tab-1 (d3b-navigate-observation.json). The wait after it is 200 in 0.164 / 0.179 s, and every receipt is completed. |
| D3c (x4) | PASS (confirmed, scope note) | All 4 D3a runs' next observation includes `slow/redir allow` and `fixture-b/ext block domain_grant_blocked` with tab-1 (d3c-next-observation-egress.json). `GET /egress-receipts` shows the fixture-b row in all 3 shipped tries (d3c-egress-receipts-endpoint.json; final.log does not check the endpoint). The receipt holds the full URL with its path and `host:null`, so it is not host-only. |
| D1a (x4) | PASS (confirmed) | d1.log tries 1-3 and final.log: the first wait is 200 in 0.432 / 0.411 / 0.317 / 0.275 s, url about:blank, step 0, 3 AX nodes, `wait_completed`, `no_current_page` 0. Tabs go 0 → 1 (about:blank) → 1 (fixture-a) in tries 1-3; the final try has no "before" read but shows 1 → 1. Checkpoint `1|active|[]|1`. Navigate is 200 "Fixture A" in 0.296 / 0.288 / 0.216 / 0.190 s. |
| D1b (x1) | PASS (confirmed) | `wait_for body` is 200 in 0.199 s with `selector_observed` and url about:blank (d1b-first-wait-for-body.json). Tabs are n=1 after it and after the navigate (200 in 0.237 s). |
| D1c (x2, +rs3) | PASS (confirmed) | rs.log rs1/rs2: navigate 504 in 25.211 / 25.121 s, then a wait 200 on about:blank. rs-expired-lines.txt has `run expired idle` lines with idle_s 31 / 30 (and rs3 31). A step on the expired run is 404 `run_not_live`, `resumable:true`. Resume is 200 in 0.024 / 0.021 s with tabs n=0. The wait right after the resume is 200 in 0.333 / 0.316 s on about:blank with `DRIVER_FAILED` 0 (rs1/rs2 wait-after-resume bodies). Navigate is 200 in 0.220 / 0.215 s. rs3: resume 200 in 0.236 s, then a wait 200 on "Fixture A". |
| D2 (x3) | PASS (confirmed, live re-check) | rs-receipts.txt and the API lists agree. rs1 has keys 0-4, rs2 has 0-5 with the failed `/redir` at key 2 at 10:08:39.386, and rs3 has 0-4 with the failed key 1. The keys are distinct and strictly increasing by started_at. rs2's checkpoint before the idle was `1|active|[]|3`, and the closed checkpoints have next_step 5 / 6 / 5. The live SELECT found 0 runs with a repeated step key. |
| T1 (x2) | PASS (confirmed) | reg.log: 504 in 25.027 / 25.027 s, `TIMEOUT`, `deadline_ms:25000`, url-in-body 0. The next navigate is 200 in 0.199 / 0.209 s with `ax_snapshot` 0, and the wait after it is 200. |
| T4b-1 (x3, +rs1/rs2) | PASS (confirmed) | reg.log x2 and final.log: 504 in 25.187 / 25.218 / 25.151 s (t4b1-hang-body.raw, no URL). The wait is 200 in 0.180 / 0.184 / 0.208 s on about:blank, and the navigate is 200. rs1/rs2 return 504 in 25.211 / 25.121 s. The 25.15-25.22 range in the summary covers only the x3. |
| T6 (x4) | PASS (confirmed) | 0.368 / 0.323 / 0.310 / 0.450 s. I recomputed the 11 step-0 fixture-a navigates (T6 x4, D3a warm x2, D3b warm, S1a warm, T1 warm x2, rs3): 0.274, 0.280, 0.295, 0.310, 0.310, 0.323, 0.353, 0.366, 0.368, 0.409, 0.450. That gives min 0.274 and median 0.323, as reported. |
| S1a (x2 +rs2) | PASS (confirmed) | 403 in 0.0057 / 0.0046 / 0.0039 s with `details:null` (s1a-first-body.raw). Receipt counts stay 0 -> 0 and 1 -> 1. |
| T5 | PASS (confirmed, scope note) | The generation values in the logs are only 0 (the first `/ready` of the d1, rs and final containers) and 1 (d1 5, d3 8, reg 10, rs 4, final 5), with nothing above 1. ready-and-end.log shows generation 1, probe_failures 0 and rc=0 for all 3 containers. edge-scan.log (372 / 160 / 104 lines) has 0 relaunch/crash lines, 0 test host/URL lines and 0 SECURITY_BLOCKED/no_current_page lines. Its 7 ERROR 504 paths are runs HW4A, HX0A, HXVX, HYRA, J1AD, J3B1 and J88W, which are exactly the 7 TIMEOUT steps (T1 x2, T4b-1 x3, rs1, rs2). There is no 502, and 1 NATS boot line per container. The raw docker logs are not kept; the scan is the leg's own count summary. |
| closes | PASS (confirmed, live re-check) | `close: HTTP=200` appears 4 + 6 + 8 + 3 + 4 = 25 times for 25 distinct run ids, plus the tabs-shape probe run (ready-and-end.log). The live read-only SELECT gives `closed|26`. |
| Flag ledger / end state | PASS (confirmed) | flags-log lines 75-77 record that `RUN_SWEEPER_DRY_RUN` 1 -> 0, `RUN_SWEEP_INTERVAL_S` -> 5 and `AGENT_RUN_IDLE_TTL_S` -> 30 were set at 10:06:51Z, and all three were reverted at 10:10:21Z. The edge start time (10:10:25Z), the container env and the empty flags diff confirm the revert. |
| Secret scan of the evidence | PASS | No matches for JWTs (`eyJ`), `Bearer <value>`, sk-/sk-ant/AKIA/gh*_/xox keys, private-key headers, `password/secret/token/api_key=` values, or nats/postgres URLs with credentials, across R4-SC4.md and the R4-SC4-* files. |

### Downgrades

None. Every PASS row matches its evidence in status, timing and count. These notes do not change any result:

- **D3a:** R4-SC4.md says "One tab throughout (`GET /tabs` n=1 before and after)", but d3.log records tabs only after the refusal and at the end; there is no "before" read in the D3a tries. Those reads are n=1 and are enough for the claim.
- **D3c:** the endpoint check covers the 3 shipped tries; the final D3a try checked only the next observation.
- **T4b-1:** the summary's 25.15-25.22 s range covers the x3 only; rs2's first-step 504 was 25.121 s.
- **Tabs shape:** the helper's jq reads `id`, but the API field is `tabId` (see the probe run in ready-and-end.log). Every tabs `id:null` value in the logs comes from that, not from the edge.

### Depth (R4-SC4), agreed with the leg

1. **Egress receipts carry the full URL with its path, and `host` is null.** Examples are `fixture-b.crawl.test/ext` and `/pixel.png`, in both the observation and `GET /egress-receipts`. The endpoint reports `zdr:false` for these runs. The owner should decide whether this is acceptable under ZDR persistence or relay, like open issue 3.
2. **After a first-step D3 refusal, the two read surfaces disagree:** browser-session shows `currentUrl ""` while `/tabs` shows about:blank. This lasts until the next observation.
3. **A refused navigate is charged 1000 micro-USD**, the flat navigate cost (d3a-first-receipts.json).
4. **`outcome.latency_ms` is 0 on completed receipts** even though the steps took 0.16-0.28 s.
5. **The checkpoint `step` column is still the count of completed steps.** The key high-water mark is in `state->>'next_step'`.
6. **Not run:** `screenshot`, `pdf` and `get_content` on a session with no page; the socket and attach-delegate step paths; an open-web run; and `HUMAN_RECEIPT_MODE=none`.

R3-SC4 open issues 1 (502 after resume), 2 (duplicate step key) and 3 (25 s TIMEOUT on an off-list redirect) are closed on live dev by this leg.
