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
