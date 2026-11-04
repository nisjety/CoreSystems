# SC1 signed-in legs: run order for the lead (2026-10-29)

Prepared 2026-10-29 from `.plan-work/quarry-w1w2/L2-signed-in-steps.md` (sections 0 to 8). The gateway, Model Gateway and Quarry code was read again for this file. Nothing here has been run, and no stack was touched while preparing it. Not committed.

- Page script: `sc1-legs.js` in this folder. Pass the whole file as ONE `javascript_tool` call in the signed-in SPA tab. It returns `L2 ready, t0=...`. Each leg is then one more call.
- Who signs in: the user, in the built-in browser pane, with a seeded dev account (`local@verevon.dev` or `e2e@verevon.dev`). Nobody reads, asks for or pastes a password, token or cookie. The script never reads cookies or storage. It never returns a body, a token, a grant id or `allowed_domains`.
- Stack: the stack you choose for SC1, with broker TTL 45 s (the contract value; the wrapper pins `GRANT_TTL_SECONDS=45`). The SPA origin must serve `/api` to THAT stack's gateway. Section 1 of the doc reserves the live stack for no step. If SC1 runs on live dev, that is the owner's call; write it in the run header.
- The restart check (doc section 6, rows R.*) is NOT in this package. It needs a TTL of at least 300 s and the owner's go-ahead to restart the broker.

## Code read on 2026-10-29 that differs from the 2026-10-23 doc (already handled in `sc1-legs.js`)

1. **A.4 refusal shape.** The gateway now maps Quarry's 403 "outside the run's allowed domains" to `403 {code:"browser_navigation_outside_scope", message:"This session is limited to the domains of its run."}` (`GW/domains/browser.rs:5093-5107`, test `forward_quarry_failure_maps_navigation_scope` `:5772`). The doc's check on Quarry's message would FAIL against a current gateway image. The script accepts either form, and the detail says which one appeared (`form=mapped` or `form=quarry-message`).
2. **C.3 replay shape.** Model Gateway passes the replay through as a top-level object with no `data` envelope: `{run_id, events:[{event_id, event_type, at, payload}], next_event_id, truncated}` (`MG/http_routes.rs:1255-1272, 1280-1376`). The event types are proto names, `EVENT_TYPE_BROWSER_ACTION_DISPATCHED` and `EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED` (`:1348-1356`), so the doc's `'BrowserObservationReceived'` would never match. The script accepts both spellings and pages with `after_event_id`. The refusal scan covers the event type and payload only, not ids or timestamps. Otherwise a timestamp such as `.403+00:00` would match `\b403\b`.
3. **WS8.D human_browse.** `BROWSER_HUMAN_BROWSE` (`GW/config.rs:216, 542-546`) is off by default. When it is on, a session gets a `human_browse` grant with no hosts and no first-host constraints (`GW/domains/browser.rs:597-609, 4634-4645`). A.4 and B.1 then cannot pass. The page can see this mode as the session capability `human_browsing` (`:3267-3299`), and A.1 records it as an INFO row (mode evidence).
4. **No broker log line for DeriveRunGrant.** `DeriveRunGrant` writes no `grantlog` line. `browser grant issued` is written only by AcquireGrant (`browser-broker/internal/server/server.go:154`) and human_browse (`human_browse.go:73`). Derive increments a telemetry counter only (`server.go:231-232`). So the broker log cannot give the doc's "one DeriveRunGrant" count. The page-side evidence is C.2 returning 200 with a run id plus the C.3 events. H.3 records the run-grant count as "not in the log".
5. **The 120 s runtime cap.** A first-host session's Quarry run carries `max_runtime_s 120` and `max_steps 12` (`GW/domains/browser.rs:4634-4645`). A.6 waits T + 5 = 50 s, which fits. With a TTL above about 110 s, the runtime cap may end the run before the grant expires. For A.7, a session older than 120 s gives a trivial 404.
6. **Line numbers have moved** in `GW/domains/browser.rs`: routes `461-536`, `create_session` `563`, `run_action` `899`, `close_session` `1430`, `start_ai_run` `1694`, `session_grant_refusal` `4584`, `session_grant_inactive` `4854`. The comments in `sc1-legs.js` give the 2026-10-29 lines.

## 0. Host preflight (read-only; record as "mode evidence")

```bash
# container names only, to set the variables below
docker ps --format '{{.Names}}' | grep -E 'gateway|browser-broker|model-gateway|execution-core|quarry-edge|auth-service'
# the wrapper's read-only preflight (env of quarry-edge, broker TTL=45, strict, read-only root, cap_drop,
# no published port, zero --no-sandbox, auth-core registry row); it returns before any tunnel is started
QUARRY_CONTAINER=quarry-edge BROKER_CONTAINER=model-plane-browser-broker-1 \
MODEL_GATEWAY_CONTAINER=model-plane-model-gateway-1 EXECUTION_CORE_CONTAINER=model-plane-execution-core-1 \
GATEWAY_CONTAINER=verevon-gateway-rs AUTH_CONTAINER=auth-service \
bash scripts/tests/browser-grant-prod-contract.sh --preflight-only
```
Note the TTL `T` from the preflight's `BROWSER_BROKER_GRANT_TTL_SECONDS` line. If the preflight fails, write down which line failed. A failed preflight does not stop the page legs, but the run header must say the stack is not the gate posture.

Page preconditions (doc section 1): signed in, the tab stays in the foreground during the waits, `example.com` is reachable from the stack, and few sessions are open. `browser_start` allows 30 starts per minute per organization (`GW/rate_limit.rs:48,185`). All legs together start 6 sessions and 3 AI runs.

## 1. Run order (one `javascript_tool` call per line)

| # | Call | Time | PASS means |
|---|---|---|---|
| 1 | contents of `sc1-legs.js` | instant | Returns `L2 ready, t0=<ISO>`. Copy `t0`: it is `SINCE` for section 3. Injecting the file again on the same page keeps the results. |
| 2 | `await L2.legA0()` | <1 s | `A.0 signed in` PASS. If it FAILs, stop. The user signs in, then go back to step 1. |
| 3 | `await L2.legA_start()` | ~20-60 s | A.1 to A.5 all PASS. A.1: 200 with a live, non-ZDR session id, an observation and no grant id. A.2: 200 with the same `runId` and no grant id. A.3: 200. A.4: evil.example and example.org each 403 (either form), then example.com 200 again. A.5: close 200 `closed:true`, then a step on the closed session is 4xx (expect 404). The INFO row `A.1 mode evidence` must say "absent" (start-host grant). |
| 4 | `await L2.legA6(45)` | ~60-80 s | Start 200, step 200, then wait 50 s. The step after expiry is 403 `browser grant is inactive or expired` (not a 5xx). The AI run on the expired session is 409 `browser_session_grant_inactive`. Use T from the preflight. If the tool call times out, the leg keeps running in the page: poll `L2.legStatus()`, then call `L2.legResult('A6')`. |
| 5 | `await L2.legA_close()` | few s | `A.close nothing left open` PASS, and `A verdict (page side)` PASS: every A check passed and no leg-A response was 5xx. The host scan (section 3) is the other half of the leg A verdict. |
| 6 | `await L2.legB()` | ~20-40 s | B.1: 400 `browser_host_not_grantable`. The INFO row `B.1 outcome` must say `strict`; only that outcome proves strict. B.2: start 200 with an observation, tabs 200, close 200 (INFO: start ms). B.3: ZDR start 200 with `zdr:true`, step 200, close 200. |
| 7 | `await L2.legC()` | ~40-110 s | C.1: session 200. C.2: AI run 200 with a run id (INFO: ms). C.3: replay 200, both event types seen, no refusal text in the events. C.4: on a fresh session, naming `evil.example` gives 403 `browser grant is not authorized`. C.5 rows are INFO: a 2xx or a typed 4xx is fine, a 5xx is recorded. `L2.legC(60)` shortens the poll. If the stack has an LLM planner, this leg spends model tokens; write down which planner ran. |
| 8 | optional A.7 | | Needs a second account in the same pane. In the first account, run `await L2.openForA7()` and keep the returned id. Sign in as the second account, inject the file again, and run `await L2.legA7('<id>', <seconds since opened>)`. PASS means both the read and the step are 4xx, and the age is under 120 s. Back in the first account, run `await L2.closeById('<id>')`. Without a second account, run `await L2.legA7()`, which records NOT-RUN. |
| 9 | `L2.summary()` | instant | Returns plain data: `t0`, `marks` (the log-scan windows), leg states, totals, `openSessions`/`openRuns` (must be empty), `responses5xx`, `responses401` (both must be empty), the `results` rows and the `requests` rows (label, status, code, message, ms). Copy it into the table in section 4. |

Every leg returns `[{name, ok, detail}]`: `ok:true` is PASS, `ok:false` is FAIL, and `ok:null` is INFO or NOT-RUN (the detail starts with the kind). Every leg also adds `<leg> no 401 in this leg` and `<leg> no 5xx in this leg`; cleanup calls are recorded but are not counted in the 5xx check. A leg closes what it opened, even when a step throws. If something is still left open, `L2.legA_close()` closes every session and stops every run that remains. Only one leg runs at a time (there is a guard).

Leg verdicts (doc sections 3 to 5):
- **Leg A passes** when every A row is PASS on the gate stack, H.1 is 0 for the A happy window, and no A response is 5xx.
- **Leg B passes** when every B row is PASS and the broker counts match (one issued and one revoked per session).
- **Leg C passes** when C.1 to C.4 are PASS and H.1 is clean for the C happy window.

## 2. Rows the page cannot reach (they stay with the wrapper `scripts/tests/browser-grant-prod-contract.sh` and the `prod_*` tests)

| Spec leg | Why the page cannot reach it | Where it is proven |
|---|---|---|
| A1: three user tokens mint | Auth Core is cross-origin | wrapper. A.1 only shows it indirectly: the gateway minted all three. |
| A2: grant issued | The grant id is never visible to the page | wrapper. A.1 in strict is the indirect proof. The broker `browser grant issued` line is H.3. |
| A3: Quarry start with hostile client hosts | The BFF sends one host and no client list | wrapper. `prod_start_run_with_active_grant_overrides_client_domains` |
| A5a: no grant gives 403 `browser grant_id is required` | The BFF always sends a grant in strict | wrapper. `prod_start_run_without_grant_is_403`. B.1 shows strict. |
| A5b: forged grant id | It needs a direct Quarry call | wrapper. `prod_*` |
| A5c: revoked grant on a LIVE run gives 403, not 5xx | The page cannot revoke a grant without closing the run | wrapper. `prod_step_with_revoked_grant_is_403_not_5xx`. A.6 is the same-origin stand-in. |
| A6: another user's grant | Only through optional A.7 | wrapper (second seeded account) |
| A7: no host names in the hop logs | Host side | section 3 H.2 here, and the wrapper's `scan_hops` |
| H.4: zero `--no-sandbox` plus a `navigate` 200 | Host side | wrapper `check_no_sandbox` (`browser-grant-prod-contract.sh:257`) |
| A later `/step` with the derived run grant gives 403 after the session closes | It is a direct Quarry call | wrapper |
| R.1 to R.7 restart survival | It needs TTL ≥ 300 s and the owner's go-ahead to restart the broker | doc section 6, separate run |

## 3. Host-side log scan (counts only; no log line is printed)

Run it after leg A, leg B and leg C, and once at the end. `SINCE` is `L2.t0`. Take the happy-window bounds from `L2.summary().marks`: `A.happy.from`/`A.happy.to` cover A.1 to A.3, `B.happy.*` covers B.2 to B.3, and `C.happy.*` covers C.1 to C.3. The negative steps (A.4, A.6, B.1, C.4) are supposed to write refusal lines, so H.1 counts only the happy windows. The patterns are the wrapper's (`browser-grant-prod-contract.sh:80-82`). Set the container variables to the names found in section 0.

```bash
GW=verevon-gateway-rs; MG=model-plane-model-gateway-1; EC=model-plane-execution-core-1
QE=quarry-edge; BR=model-plane-browser-broker-1
SINCE='<L2.t0>'
FAIL='\b40[13]\b|PermissionDenied|Unauthenticated|browser grant is not authorized|browser grant_id is required'
HOSTS='example\.com|example\.org|evil\.example'

# H.1 refusal lines per hop, per happy window (expect 0 each)
for w in A B C; do
  FROM='<marks.'$w'.happy.from>'; TO='<marks.'$w'.happy.to>'   # paste the two ISO times per window
  for c in $GW $MG $EC $QE $BR; do
    printf '%s %s refusal-lines=%s\n' "$w" "$c" "$(docker logs --since "$FROM" --until "$TO" "$c" 2>&1 | grep -Ec -- "$FAIL")"
  done
done

# H.2 host names in the three hops kept domain-free, whole run (expect 0 each)
for c in $BR $MG $GW; do
  printf '%s host-name-lines=%s\n' "$c" "$(docker logs --since "$SINCE" "$c" 2>&1 | grep -Ec -- "$HOSTS")"
done

# H.3 grant lifecycle counts at the broker, whole run (the lines are JSON slog: "kind":"session")
printf 'issued-session=%s\n' "$(docker logs --since "$SINCE" $BR 2>&1 | grep 'browser grant issued' | grep -Ec 'kind[=":]+session')"
printf 'issued-session-zdr=%s\n' "$(docker logs --since "$SINCE" $BR 2>&1 | grep 'browser grant issued' | grep -Ec '"zdr":true')"
printf 'issued-other-kinds=%s\n' "$(docker logs --since "$SINCE" $BR 2>&1 | grep 'browser grant issued' | grep -Evc 'kind[=":]+session')"
printf 'revoked=%s\n' "$(docker logs --since "$SINCE" $BR 2>&1 | grep -c 'browser grant revoked')"
printf 'derive-mentions-broker=%s derive-mentions-mg=%s\n' \
  "$(docker logs --since "$SINCE" $BR 2>&1 | grep -c 'DeriveRunGrant')" \
  "$(docker logs --since "$SINCE" $MG 2>&1 | grep -ic 'derive')"
```

Expected [CODE] for H.3, when legs A, B and C each run once without A.7: `issued-session` = 6 (A.1, A.6, B.2, B.3, C.1, C.4; B.1 is refused before a grant exists), of which `issued-session-zdr` = 1 (B.3). `issued-other-kinds` = 0. `revoked` is written only when `RevokeScoped` succeeds (`server.go:158-176`). It should be 5 or 6 session revokes: A.6's grant had expired before its close, so that revoke may be refused. A revoke of the derived run grant at run end can add 1 (Model Gateway revokes it, `MG/browser_run.rs:995-1006`). Record the observed numbers and explain any difference. Do not fit the expectation to them. Derive has no grantlog line (difference 4 above), so the `derive-mentions` counts are informational. If the broker image predates `d7a04e07a` (`browser grant issued/revoked`), the issued and revoked counts are 0: say so in H.3.

## 4. Results table (one copy per stack and per run; doc section 8)

Run header: date, operator, stack name and compose files, gateway image id and `SOURCE_REVISION`, quarry-edge image id and revision, broker TTL, `BROWSER_GRANT_ENFORCEMENT`, `BROWSER_HUMAN_BROWSE` (A.1 mode row), quarry-edge `ENVIRONMENT`, planner flags unset (yes/no), preflight transcript attached (yes/no), SPA origin used, `L2.t0`.

| Id | Step | Expected | Status seen | errorCode / message seen | PASS/FAIL | Notes (ms, ids) |
|---|---|---|---|---|---|---|
| A.0 | session probe | 200, user present | | | | |
| A.1 | start session | 200, session id, live, zdr false, observation, no grant id | | | | mode row; start ms |
| A.2 | get session | 200, same run id, no grant id | | | | |
| A.3 | step example.com | 200 | | | | |
| A.4a | step evil.example | 403 `browser_navigation_outside_scope` (or Quarry's outside-domains message) | | | | form= |
| A.4b | step example.org | 403, same | | | | form= |
| A.4c | step example.com after refusals | 200 | | | | |
| A.5 | close, then step | 200 closed:true; then 4xx (expect 404) | | | | |
| A.6 | expiry (T s) | before 200; after 403 inactive; AI run 409 browser_session_grant_inactive | | | | waited= |
| A.7 | other user (optional) | 403 or 404, session age < 120 s | | | | or NOT-RUN |
| A.v | leg A page verdict | A.close + A verdict PASS | | | | |
| B.1 | non-grantable host | 400 browser_host_not_grantable (strict) | | | | strict: yes/no |
| B.2 | start, tabs, close | 200, 200, 200 | | | | start ms |
| B.3 | zdr session | 200, zdr true, step 200, close 200 | | | | |
| C.1 | session | 200 | | | | |
| C.2 | AI run start | 200 run id | | | | ms, run id, planner |
| C.3 | replay | both event types, no refusal text | | | | types seen x count |
| C.4 | widening request | 403 browser grant is not authorized | | | | |
| C.5 | stop, close | 2xx or typed 4xx | | | | |
| L.* | per leg: no 401, no 5xx | PASS each | | | | |
| H.1 | refusal lines per hop, happy windows A/B/C | 0 each | | | | |
| H.2 | host-name lines (broker, model-gateway, gateway) | 0 each | | | | |
| H.3 | broker issued / revoked counts | issued-session 6 (zdr 1), revoked 5-7 explained | | | | image has grantlog: yes/no |
| H.4 | zero `--no-sandbox`, a `navigate` 200 on the same stack | 0; 200 | | | | wrapper line |
| R.* | restart survival | not in this run | | | | doc section 6 |

Verdict rule (plan SC1 and `W1-closeout-baseline.md` section 6): SC1 is PASS only if every clause is PASS at its evidence type. When a run contradicts a [CODE] expectation, record a FAIL with the observed status and message; never edit the expectation in the table to match. What the page cannot show is listed in section 2 and stays with the wrapper and the `prod_*` tests.
