# SC1 signed-in legs on live dev, 2026-10-29

- **Stack:** live dev in the SC1 gate posture (see PREP.md, VERIFY.md and preflight.txt). The owner chose live dev ("dette er kun dev") and said "Forbered SC1".
  - quarry-edge: production plus the grant-contract overlay.
  - broker TTL: 45 s.
  - gateway: `strict`.
  - auth-core: registry row present.
  - The preflight passed at 11:11:28Z.
- **How it ran:** in the built-in browser pane on http://localhost:5173. The pane was already signed in (the owner's dev account). The legs were run with `sc1-legs.js` through javascript_tool. No password, token or cookie was read, and no response body was printed.
- **Timing:** t0 was 2026-10-29T11:17:38.773Z. Log-scan windows (UTC):

  | Window | From | To |
  |---|---|---|
  | A happy | 11:18:18.390 | 11:18:21.026 |
  | B happy | 11:19:47.447 | 11:19:50.276 |
  | C happy | 11:19:54.760 | 11:21:12.197 |

- **Totals:** 43 PASS, 2 FAIL, 10 INFO. There were zero 401 responses and zero 5xx responses, and nothing was left open (sessions 0, runs 0).

| Row | Result | Evidence |
|---|---|---|
| A.0 signed in | PASS | 200 |
| A.1 start on example.com | PASS | 200 in 1993 ms; live, not ZDR; first observation present; no grant id in the body; start-host grant (human_browsing absent) |
| A.2 read back | PASS | 200, same runId, no grant id |
| A.3 step to the granted host | PASS | 200 in 585 ms |
| A.4 evil.example and example.org | PASS | 403 `browser_navigation_outside_scope` (mapped form), 41 and 356 ms; example.com is 200 again afterwards |
| A.5 close, then a step | PASS | close 200 `closed:true`; step 404 "agent run is not live" |
| A.6 expiry (TTL 45 s, waited 51 s) | PASS | before the wait 200; after it 403 `SECURITY_BLOCKED` "browser grant is inactive or expired" (not 5xx); AI run 409 `browser_session_grant_inactive` |
| A verdict (page side) | PASS | legs A0, A and A6 done; 0 failed checks; 0 5xx |
| B.1 non-grantable host 1.1.1.1 | PASS | 400 `browser_host_not_grantable`, outcome = strict |
| B.2 start, tabs, close | PASS | 200 (871 ms), 200, 200 |
| B.3 ZDR session | PASS | start 200 with `zdr:true`; step 200; close 200 |
| C.1 session | PASS | 200 |
| C.2 AI run start | PASS | 200 in 829 ms, run `01M63P6N33NST4QRRCBGACKMGJ`. model-gateway logged "browser run grant resolved" with `derived_run_grant:true` |
| C.3 run events | **FAIL** | the replay answered 200 over 23 polls in 75 s, but the run had 0 events (no BrowserActionDispatched and no BrowserObservationReceived). quarry-edge saw no step from the run. execution-core logged nothing for this run id. |
| C.4 widening (example.com + evil.example) | PASS | 403 "browser grant is not authorized" in 93 ms |
| C.5 stop and close | INFO | stop 200, both closes 200 |

## Open items

- **C.3: the AI run never executes.** model-gateway derives the run grant and answers 200 with a run id. Nothing in execution-core, the Model Plane runtime loop, picks the run up within 75 s. Not diagnosed yet.
- **Not run in this pass:**
  - the host-side log scan (H.1 to H.3)
  - the wrapper legs the page cannot reach (A1 to A3, A5a to A5c, A6)
  - A.7, which needs a second account
  - the restart check, which needs a TTL of at least 300 s
