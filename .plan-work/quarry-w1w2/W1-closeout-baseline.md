# W1 closeout baseline (unit w1-synthesis, 2026-10-21)

Branch `quarryv2-next`, HEAD `473d0a78`. Written by the w1-synthesis unit. No repository file was edited and nothing was committed.
Inputs: the mechanical census (per-slice facts), the plan `docs/plans/quarry-browser-build-plan-2026-10-14.md` (1.2, 4.3, 4.4, 6.2, the slice detail paragraphs, the build ledger), `git log`, and the code. Where I re-checked a census row against the code it says so; everything else is "census".

Commands I ran myself in this unit (all native, no Docker):
- `node --test scripts/tests/browser-grant-contract.test.mjs`: 19 tests, 19 pass, 0 fail.
- `GOWORK=off go test -count=1 ./...` in `apps/Model Plane/go/services/browser-broker`: packages cmd, internal/authz, internal/grant, internal/server ok (telemetry has no test files).
- `git diff --stat`, `git rev-list`, `git ls-tree`, `git diff --name-only <merge-base> <branch>` comparisons between `quarryv2-next` and `model-plane` (section 5).
- greps and reads of the files named in each row.
Nothing else (no cargo, no docker, no pnpm) was run by this unit. Counts quoted from commit messages or notes are marked "census" or "ledger".

## 1. The plan's own W1 total (checked)

Plan at-a-glance 3 and 4.4: W1 = 148.25 engineer-days (132.25 core + 16 gated). I summed the slice days of the 39 W1 slices from the slice tables (3.1):

| Workstream | Slices counted (days) | Sum |
|---|---|---|
| WS1 | A0 1.5, A1 10, A2 6, B 10, C 6 | 33.5 |
| WS2 | A 1.5, B 2.5, C 10, D 6.5, E 0.5 | 21.0 |
| WS3 | A 2, B 2, C 3, D 3, E 1, F 10, H 5, I 3, J 6 | 35.0 |
| WS4 | A 3, B 1, C 0.75, D 1.5, E 3, F 2.5, G 6, H 2.5, I 1.5, J 5, K 0.5 | 27.25 |
| WS5 | A 2, B 5, C 4, D 5, E 4, F 2, G 3, H 3 | 28.0 |
| WS6 | A 3.5 | 3.5 |
| Total | 39 slices | 148.25 |

The gated days are WS3.I 3 + WS4.H 2.5 + WS4.I 1.5 + WS4.J 5 + WS5.C 4 = 16. The plan's own sum holds. Two headline numbers in the workstream titles do not add to this because they include optional slices that are not in 148.25: WS2 "about 22" includes WS2.F (1 d, optional), WS3 "about 40" includes WS3.G (5 d, optional). WS2.F and WS3.G have no commits and are not counted below. WS5.I (unscheduled epic) is excluded as well.

## 2. How "done" is counted here

Evidence levels (from the census): none < unit < integration (real Postgres, Redis, router, real child processes) < rebuilt-image (the slice's code in a rebuilt image that ran in an isolated side-by-side stack, per plan 6.2).
Left kinds: BUILD (code, test or doc still to write in this repo), STACK (needs a running rebuilt stack), OWNER (owner or counsel decision, customer notice, operator action on live systems), CI (GitHub Actions run or repository setting), BASELINE (the user's uncommitted live-tree baseline, PRE-1), OTHER-PLANE (Model Plane `model-plane` branch or sibling session).

Status classes (my rule, stated so it can be argued with):
- **CLOSED**: every DoD and deployment item is verified, or is strictly outside the branch (CI, OWNER, BASELINE, OTHER-PLANE). Nothing BUILD or STACK is left.
- **BUILT, evidence open**: code committed and tested; only STACK evidence, review, or outside-branch items remain.
- **BUILT, small build left**: code committed; a named test, doc or small code item is still missing.
- **PARTLY**: the slice's own code is incomplete.
- **NOT STARTED**: no commit and no code.

## 3. Baseline table, one row per W1 slice

"Reviewed" is the census value (yes = an independent review round is recorded; partly = reviewed in a bundle or only some commits). Commit lists are in the census and the plan ledger; only the headline commits are repeated here.

| Slice | Days | Built | Reviewed | Evidence | What is left, and why (kind) | Class |
|---|---|---|---|---|---|---|
| WS1.A0 | 1.5 | yes (37f9d278) | partly | rebuilt-image | The literal `compose up quarry-edge` on the production overlay was not run (it would replace the live container); the rendered flags were run instead: /ready 200, durable true, no durability bail, negative controls refuse. STACK (minor). | CLOSED (with that caveat) |
| WS1.A1 | 10 | yes (5b92585e, 19cba5c1) | partly | rebuilt-image | (1) Model Plane ledger entry for D1, D2, D5, D6 and R-6 not written (BUILD, doc). (2) The broker writes no per-call log line, the plan's evidence text asks for "one AcquireGrant" in the log: add a no-domain line or amend the text (BUILD small / plan amendment). (3) The ZDR unit test asserts the response only, not the log (BUILD small). (4) Base Ingestion compose lacks the validator URL line, only the overlay sets it (BUILD small). (5) browser-broker-ci.yml and L1 never ran on GitHub (CI). | BUILT, small build left |
| WS1.A2 | 6 | no | no | none | Whole slice. X-1 is approved at its default, so nothing blocks it. Prerequisites A0, A1, WS2.B, WS3.A are all on the branch. I confirmed in code: no `BrowserBroker` audience in `service_tokens.rs`, no `browser` text in `cross-plane-token-requests.json`, `grpc_status_to_error` (browser_broker.rs:211-221) maps `FailedPrecondition` to `DriverFailed` (5xx), the gRPC validator gets a bearer only under `cross_plane_auth_dev_bypass` (main.rs:784-790), no `validate_for_org`. BUILD, then STACK. | NOT STARTED |
| WS1.B | 10 | partly (18 commits) | yes | integration | Quarry grant checks and their three named tests exist and pass in non-prod form (agent_routes/tests.rs:2082, 2105, 2114; tests/start_run_grants.rs:96, 182, 249). Missing: execution-core `cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant` (absent, grep); compose does not set `BROWSER_GRANT_ENFORCEMENT` or `BROWSER_BROKER_MAX_RUN_DOMAINS` (grep, BUILD small); the plan's `proxy_with_delegations_...` test and `required_browser_token` helper do not exist because the code sets the header in `domains/browser.rs` (`proxy_model_json_with_delegated`, tests at 6556, 6808, 6829): plan-text divergence, close by errata (BUILD doc); the three prod-mode negative controls and L2 legs A to C need WS1.A2 (STACK); production `strict` flip only after L2 (STACK). Session-grant renewal is NOT in this slice's DoD: `Renew` is reserved for WS8.C (see work-breakdown, corrections). | PARTLY |
| WS1.C | 6 | yes (26f1c679 ... 08d07ad2, b40242f7) | yes | rebuilt-image | L1 assertion 7 (no `zdr` column, `z_` CHECK stays) is not in the L1 test (BUILD small, the migration itself has both). /step-level restart checks and L2 legs need WS1.A2 (STACK); broker-level restart survival was recorded in e2e scenario a at 37712250 (isolated). Ledger restart-semantics table absent (BUILD doc). CI job `integration` exists since b40242f7, never run on GitHub (CI). Production rollout waits on X-2 which is not in the relayed approval list although the code implements its default (OWNER). `depends_on` cannot be built: the database is in the Ingestion compose project (compose comment at Model Plane docker-compose.yml:1637-1641); `/readyz` plus restart policy is the mechanism: amend the plan. | BUILT, small build left |
| WS2.A | 1.5 | partly (256bae09) | partly | rebuilt-image | `tests/agent_evaluate_denied.rs` and `insert_live_run` absent; no WebSocket or `/interact` evaluate test; evaluate 403 seen only on image 3269c377 (lacks later admission commits), not on a HEAD image, and no receipt, budget or log checks (BUILD, STACK). Python SDK text untracked (git-ignored generated models, BASELINE-like). Model Plane `Evaluate` variant removal (OTHER-PLANE). Seven-day production log search (OWNER, needs production log access). | PARTLY |
| WS2.B | 2.5 | yes (8cf73c12, 24ee3957) | partly | unit | `cargo clippy -D warnings` and `cargo doc` never run: image `quarry-rust-env:1` has no clippy (BUILD, tooling). At HEAD `lifecycle.rs` is 747 and `types.rs` 676 lines against the "about 650" target (later slices grew them). UI flow and `make test-endpoints` not run (STACK). | BUILT, evidence open |
| WS2.C | 10 | partly (9 commits) | yes | rebuilt-image | Tab open/select/close go through `admit_tab_operation` (closed check and takeover gate only), not `admit_action` (admission.rs:430; tabs.rs:125, 254, 320); no service-principal give-back test; `taken_over_at` missing from the 409 details; no migration idempotency test (BUILD). Stack evidence: gateway image, psql columns, timeline and receipts after give back, real UI take-over (STACK). Model Plane consumers of `USER_TOOK_OVER` (OTHER-PLANE). | PARTLY |
| WS2.D | 6.5 | partly (8 commits) | yes | rebuilt-image | Lifecycle `expired` item carries no `initiatedBy system` (lifecycle.rs:624) and the drain path still uses `Arc::try_unwrap` (lifecycle.rs:708) (BUILD). Stack evidence for gateway 429 mapping, idle expiry, SSE keep-alive, resume, slot release and the test-env boot line (STACK). 24 h dry run and RSS default on the target host (OWNER, time). Optional pagehide DELETE (BUILD, optional). | PARTLY |
| WS2.E | 0.5 | partly (24ee3957) | no | none | Code is in place (App.tsx:18-20, 211-212). `pnpm build` (no chunk), a route test and SPA container evidence were never run (BUILD check, STACK). | PARTLY |
| WS3.A | 2 | yes (61533fdd) | partly | rebuilt-image | `navigate` 200 with zero `--no-sandbox` on the SAME stack as the production overlay needs WS1.A2 (STACK). Only Docker Desktop on Windows was tried (STACK, Linux host). Shipped seccomp profile is allow-by-default with a 26-name deny list and does not deny mount, unshare, pivot_root, setns (review decision, OWNER). | BUILT, evidence open |
| WS3.B | 2 | yes (9e4c9b37 ...) | yes | integration | GitHub Actions run of the lane and the required-check setting (CI). No mutation run showing a relaunch fails the lane; no per-test timeout or Chrome cache (BUILD small). | BUILT, evidence open |
| WS3.C | 3 | yes (6806a0e0) | partly | rebuilt-image | Ledger acknowledgement line for X-15 (BUILD doc, the decision itself is approved). Reverse-order run, profile-directory count, 390 px frame width not recorded (STACK). | BUILT, evidence open |
| WS3.D | 3 | yes (d724763d, b86a32c8, 7420d3a6) | yes | rebuilt-image | 13 `page.execute` or evaluate call sites in 8 functions without a deadline wrapper (census scan of chromiumoxide.rs; I confirmed 35 `with_deadline` uses against 32 `.execute(` hits, I did not re-list the 13); `screencast frame timed out` is a `DriverFailed`; durations outside the table (BROWSER_EXIT_WAIT, builder timeouts). Post-504 step check not recorded. BUILD. A calibration week is time-based. | BUILT, small build left |
| WS3.E | 1 | yes (2e62ad5a) | no | unit | `strings quarry-edge-rs | grep -c navigator.webdriver` and the named scrape and agent-run checks on a rebuilt edge (STACK). No code change. | BUILT, evidence open |
| WS3.F | 10 | yes (14 commits) | yes | rebuilt-image | Five image checks not run: 13th session 429 at MAX_SESSIONS=12, `pkill chromium` relaunch, idle reap, /ready 503 with a hidden binary and recovery, one profile directory per slot (STACK). No test for profile-directory accumulation (BUILD small). No memory limit on quarry-edge in either compose file (grep: only other services have limits), X-15 placeholder (OWNER). The R-26 boot check IS wired (main.rs:205-215, e50046a9), only its boot line is not captured. | BUILT, small build left |
| WS3.H | 5 | yes (14 commits) | yes | rebuilt-image | Live Turnstile, hCaptcha and login pages and the agent-path image check (STACK; external pages UNVERIFIED). `QUARRY_CHALLENGE_ENFORCE` defaults to log, so the DoD refusal needs `enforce` on the evidence stack. Python SDK not regenerated; two CI steps not run (CI). | BUILT, evidence open |
| WS3.I | 3 | no | no | none | Whole mechanism: `escalation.rs`, transport-only proxy rotation, `health_rotation` label (egress_broker.rs:149 still `block_rotation`), `prior_block_signals` ignored in honest mode (driver_plan.rs:40-45), preset rename with alias (presets.go:84 still `stealth`), docs. ADR-5 approved, so not blocked. `stealth.rs` is already deleted by WS3.E (grep: absent). BUILD, then STACK. | NOT STARTED |
| WS3.J | 6 (4 in sibling planes) | partly (21 commits) | partly | unit | Quarry, gateway and SPA halves done. Model Plane half absent (BrowserObservation.challenge, `observation_from_wire`, planner rule, model-gateway rule text) (OTHER-PLANE, branch `model-plane` is active; see work-breakdown). `tests/e2e/browser-workspace-blocked.spec.ts` absent; no browser run. The uncommitted gateway hunks in `knowledge/quarry.rs` and `errors.ts` are WS5.G's. | PARTLY |
| WS4.A | 3 | yes (c207b039, e4056b14) | no | unit | Gateway clippy was recorded red in 17 unrelated places at an earlier round (`wf-R4-ws1b-left.md:76`, not re-run here): triage (BUILD). `/metrics` read not recorded. The four deploy probes (per-org key, org B posture flip, org-core outage) need a search backend and a ZDR-flippable test org (STACK). | BUILT, evidence open |
| WS4.B | 1 | yes (56dd0ddf) | no | integration | The scripts exist and self-test. The step 0 table, forensics before and after, the purge with `--apply`, and the written answer to open question 3 were never produced: the local stack has no cache env. STACK on a side-by-side stack with its own Redis. The same operation against real gateways is an operator action (OWNER). | BUILT, evidence open |
| WS4.C | 0.75 | yes (997800c9) | no | integration | `verevon.gdpr.erasure.requested` through a real NATS to a running gateway not run (STACK). The consumer needs a provisioned durable on stream `AQENCIA_CONTROLPLANE` and `VEREVON_GATEWAY_GDPR_NATS_*` (gdpr_nats.rs:24-52, 88-90). | BUILT, evidence open |
| WS4.D | 1.5 | yes (69c24711, 344fc053) | no | rebuilt-image | ZDR scrape marker 0 on an image (verified). Image probe of a ZDR search with a failing provider not run (STACK). | BUILT, evidence open |
| WS4.E | 3 | yes (9 commits) | yes | rebuilt-image | Image probes for stream, search and agent start; `GET /v1/change/latest?url=` span check (STACK). Canaries exist for four route families only, the other rows read `log-unverified`, which the plan accepts. | BUILT, evidence open |
| WS4.F | 2.5 | yes (10 commits) | yes | unit | Green and red CI runs on GitHub only (CI). | CLOSED |
| WS4.G | 6 | yes (11 commits) | no | integration | Live-stack run with a real ZDR-org token (STACK). `/v1/search/suggest` has no owner (BUILD, doc/assignment). Gap rows stay Gap until WS4.H stage B. Repo-level check on GitHub (CI). | BUILT, evidence open |
| WS4.H | 2.5 | partly (11 commits) | yes | integration | Stage A (flag, helper, observe and enforce paths, 8 tests in zdr_refusal_modes.rs) is built. The orchestrator sends no `zdr` on `/v1/internal/change/record`: no column or posture lookup exists (ledger, V4 row) (BUILD, needs a design choice). Gap tests not inverted and rows not flipped to Enforced (waits on the default flip). Flag default on in compose needs ADR-4 sign-off and customer notice (OWNER). No flag-on live evidence (STACK). Review assertions not mutation-tested (BUILD small). | PARTLY |
| WS4.I | 1.5 | no | no | none | Source catalogue is NOT absent on every branch: `apps/Model Plane/api/contracts/zdr-refusals.json` (+ `.sha256`), `mp-contracts/src/zdr_refusal.rs` and the SPA `src/shared/api/zdr-refusals.ts` are committed on branch `model-plane` (5ae7fe13), absent on `quarryv2-next`. Vendoring a byte-identical copy is possible (BUILD); the four new feature keys are a Model Plane PR (OTHER-PLANE); the gateway and SPA mapping needs `zdr-refusals.ts` (OTHER-PLANE). | NOT STARTED |
| WS4.J | 5 | no | no | none | Whole slice. Counsel review (OWNER/counsel) gates enabling, not building: flag `QUARRY_EDGE__ZDR_TOMBSTONE` defaults off. The "QB-T17 `state.rs` commit" is moot here: `git status` shows no uncommitted hunk in `state.rs` or `tests/support/mod.rs`. | NOT STARTED |
| WS4.K | 0.5 | yes (e4056b14) | no | rebuilt-image | The plan's `prev_fingerprint` probe form on a stack (the two-key probe on an image and the Redis integration test cover the scenario); the Redis gate on GitHub (CI). | CLOSED |
| WS5.A | 2 | yes (2718959e, f7bd1702, edbf13ce) | no | rebuilt-image | No-new-unwrap audit not recorded (BUILD small); `/v1/scrape/stream` `content_kind` probe (STACK); docs-contract gate files untracked in this tree (BASELINE). | BUILT, evidence open |
| WS5.B | 5 | partly (4b28542a, edbf13ce) | no | rebuilt-image | `/v1/extract` neither routed nor explicitly deferred in the commit (BUILD decision); planned `zdr_claim_floor` PDF test absent (BUILD small); staging re-ingest check, crawl with PDF and PNG link, 20 MB timing not run (STACK); layout differs from the plan (`content_route.rs`, no `document.rs`). | PARTLY |
| WS5.C | 4 | yes (9 commits) | yes | rebuilt-image | Flate-bomb typed 415 IS verified on a rebuilt image: RUNBOOKS.md:406 row 4, image 1f4edbca, 415 UNSUPPORTED_MEDIA reason `too_large` in 0.25 s, peak 18.8 MiB of 1 GiB, OOMKilled false (I read the row). Both bomb tests on GitHub Linux CI (CI). Windows in-process fallback untested. | CLOSED |
| WS5.D | 5 | yes (7 commits) | yes | rebuilt-image | `Sunset` header on an image; docs-contract gate (BASELINE); the flip to strict is a week of logs later (OWNER, time). | BUILT, evidence open |
| WS5.E | 4 | yes (5 commits) | yes | rebuilt-image | `only_main_content:false` probe on an image. | BUILT, evidence open |
| WS5.F | 2 | yes (4 commits) | no | rebuilt-image | Second-org 404 and ZDR-reader 403 on an image; S3 backend not run. | BUILT, evidence open |
| WS5.G | 3 | partly (not committed) | no | unit | Hunks are in the working tree (7 modified files, see work-breakdown item i). Finish, test, commit; SPA gates; four stack checks. `go.work.sum` (+1 line, automaxprocs) is NOT WS5.G's. | PARTLY |
| WS5.H | 3 | yes (12 commits) | no | unit | CI runs and the throwaway-failure demonstration (CI). openapi `ScrapeRequest` completion and the `x-quarry-lenient-until` note absent (`grep` empty) (BUILD small). | BUILT, small build left |
| WS6.A | 3.5 | partly (5995eeca) | partly | rebuilt-image | `docs/CONTRACTS.md` has no crawl-bound text (grep: the crawl lines are the denial schema and job statistics, not max_pages); no Go test run recorded for the orchestrator; gateway-to-orchestrator 7-page crawl and pre-fix capture not recorded; replay uses an `OnGetVersion` mock, not a recorded history (BUILD small, STACK). | PARTLY |

## 4. Honest percentages

39 W1 slices, 148.25 days. Classes are the ones defined in section 2, applied to the table in section 3.

| Class | Slices | Count | Share of 39 |
|---|---|---|---|
| CLOSED (nothing BUILD or STACK left) | WS1.A0 (with its caveat), WS4.F, WS4.K, WS5.C | 4 | 10.3 % |
| BUILT, evidence open | WS2.B, WS3.A, WS3.B, WS3.C, WS3.E, WS3.H, WS4.A, WS4.B, WS4.C, WS4.D, WS4.E, WS4.G, WS5.A, WS5.D, WS5.E, WS5.F | 16 | 41.0 % |
| BUILT, small build left | WS1.A1, WS1.C, WS3.D, WS3.F, WS5.H | 5 | 12.8 % |
| PARTLY built | WS1.B, WS2.A, WS2.C, WS2.D, WS2.E, WS3.J, WS4.H, WS5.B, WS5.G, WS6.A | 10 | 25.6 % |
| NOT STARTED | WS1.A2, WS3.I, WS4.I, WS4.J | 4 | 10.3 % |
| Total | | 39 | 100 % |

Summary: slices fully done 4 of 39 (10.3 %); slices whose own code is fully committed ("built = yes", the first three rows) 25 of 39 (64.1 %); slices partly done 10 (25.6 %); slices not started 4 (10.3 %).

Days (from the section 1 table):

| Measure | Days | Share of 148.25 |
|---|---|---|
| Days of slices built = yes | 84.25 (WS1 A0 1.5, A1 10, C 6; WS2.B 2.5; WS3 A 2, B 2, C 3, D 3, E 1, F 10, H 5; WS4 A 3, B 1, C 0.75, D 1.5, E 3, F 2.5, G 6, K 0.5; WS5 A 2, C 4, D 5, E 4, F 2, H 3) | 56.8 % |
| Days of slices partly built | 48.5 (WS1.B 10, WS2.A 1.5, C 10, D 6.5, E 0.5, WS3.J 6, WS4.H 2.5, WS5.B 5, G 3, WS6.A 3.5) | 32.7 % |
| Days of slices not started | 15.5 (WS1.A2 6, WS3.I 3, WS4.I 1.5, WS4.J 5) | 10.5 % |
| Check | 84.25 + 48.5 + 15.5 = 148.25 | |
| Days built, honest bounds | lower bound 84.25 (56.8 %); upper bound 132.75 (89.5 %) if every partly built slice counted whole. I do not give a point estimate inside the band: a fraction per partly built slice would be my guess, not a measurement. | |
| Fully verified days (CLOSED slices only) | 8.5 (WS1.A0 1.5 + WS4.F 2.5 + WS4.K 0.5 + WS5.C 4) | 5.7 % |

Evidence level counts (census): rebuilt-image 21 slices, integration 6, unit 7, none 5 (21 + 6 + 7 + 5 = 39). Review counts (census): reviewed yes 14, partly 8, no 17 (= 39).

Remaining work, my ESTIMATE (not measured): about 53 engineer-days, about 34 of build and about 19 of evidence on stacks plus the exit gate. Basis: the per-lane figures in `W1-work-breakdown.md` section 4. Wall clock with 5 parallel lanes is bounded by the grants lane (about 9.5 d of build) plus its L2 work (about 3 d) and the exit gate (about 2 d), before Docker-lock queueing: roughly 14 working days.

## 5. What cannot be closed on this branch (will remain at "100 % of what the branch can do")

- GitHub Actions runs: browser-broker-ci.yml (jobs broker, integration, bootstrap-db, contract), browser-grant-contract.yml (to be written), quarry-browser-live as a required check (a repository setting), the two WS3.H CI steps, the WS4.F red and green lint runs, the WS5.C Linux bomb tests, the WS5.H throwaway-route demonstration, the WS4.K Redis gate, the repo-level `zdr-contract-sync` run. Nothing is pushed.
- Counsel review before WS4.J may be enabled; ADR-4 sign-off and customer notice before the `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` default flips (WS4.H stage B); X-2 sign-off for production rollout of the Postgres grant store; the X-15 memory limit numbers; the 24 h sweeper dry run review (WS2.D); the seccomp shape decision (WS3.A); the strict-mode date (WS5.D).
- PRE-1 (the user's uncommitted live-tree baseline) and everything tagged [U] in the plan: the docs-contract gate files (WS5.A, WS5.B, WS5.D), `rerank_behavior.rs`, the 28 fixtures. Never touch `C:/dev/CoresSystem`.
- Production log access: the WS2.A seven-day search; the live Redis purge and forensics of WS4.B on real gateways.
- Other planes: Model Plane `Evaluate` variant removal and `USER_TOOK_OVER` consumers (WS2.A, WS2.C), the ZDR catalogue feature keys (WS4.I), the typed-error mirrors, the WS3.J Model Plane half until it is done in this branch's own commits.
- The Python SDK text (WS2.A) and Python SDK regeneration (WS3.H): generated models are git-ignored.

## 6. Success criteria SC1 to SC7 at the W1 exit: current state

Rule: a criterion is PASS only if every clause is PASS at the evidence type the plan names (CI, L2, LOG, PROBE). Any FAIL makes it FAIL. Otherwise NOT-RUN. SC2 has the W1 part SC2a only (SC2b is W2). SC7 has the crawl clause only at W1.

| SC | State | Clause by clause, with the evidence line |
|---|---|---|
| SC1 Prod-mode browser path | **FAIL by code reading, and NOT-RUN as an L2** | NOT-RUN: no L2 harness exists (`tests/e2e/browser-grants-prod-contract.spec.ts`, `scripts/tests/browser-grant-prod-contract.sh`, `docker-compose.grant-contract.yml`, `.github/workflows/browser-grant-contract.yml` all absent). FAIL (code, not a run): production quarry-edge cannot authenticate to the broker (main.rs:784-790 attaches a bearer only under dev bypass) and a revoked or expired grant surfaces as `DriverFailed` (browser_broker.rs:211-221), so the clauses "zero 401/403" and "revoked is 403 SECURITY_BLOCKED, not 5xx" cannot hold today. PASS: no principal holds `browser:write` (L1 test, 19 of 19 run by me); gateway default `strict` in production (`config.rs:469-485`, tests in the gateway binary per census). PASS: production overlay boots quarry-edge (WS1.A0, rebuilt-image evidence, flag-equivalent run). NOT-RUN: `ps` shows zero `--no-sandbox` plus `navigate` 200 on the same stack (zero `--no-sandbox` seen on an isolated image, 30 s sample; no `navigate` 200 anywhere). |
| SC2a Raw script and takeover inside Quarry | NOT-RUN (partly evidenced) | PASS: `evaluate` on `/step` is 403 `raw_script_denied` on image 3269c377 (REST step only; that image lacks later admission commits). NOT-RUN: the same on a HEAD image, over WebSocket and `/interact`. PASS: with `TAKEOVER_ENFORCEMENT=1` a dev-bypass identity labelled agent gets 409 `USER_TOOK_OVER` and succeeds after give back (images 1f4edbca and ddbd4b2c, select, close, step), caveat: dev-bypass identity, not a token minted at `/api/quarry/token`. PASS in-process: service-principal refusal (`agent_run_control.rs:408`). The flag default stays `0`; the evidence names the value it ran with. |
| SC3 No leaked or runaway runs | NOT-RUN | PASS: cap+1 gets 429 `global_run_cap` with `Retry-After` and `details.reason` (images 89c67974, 1f4edbca). PASS: RSS measured 506, 1092, 1741, 3042 MiB at 1, 8, 16, 32 sessions (RUNBOOKS.md:261; synthetic page, rust:1 container, not the stack env file). NOT-RUN on a stack: idle expiry frees the context and keeps a checkpoint; `close_run` with a viewer; the 4 h backstop; the shipped default decision. In-process only: `run_sweeper_integration.rs`. |
| SC4 Driver stability | NOT-RUN | PASS: three leases share one browser, navigate on lease A 200 after C starts (image 89c67974); 504 `TIMEOUT` with `op navigate`, `effect unknown` (same image); `active_viewport` grep empty; no stealth code (grep empty). PASS on an isolated image: zero `--no-sandbox` for 30 s. NOT-RUN: the five WS3.F image checks (13th-session 429, pkill relaunch, idle reap, /ready 503, profile dirs); sandbox on under the final production overlay. |
| SC5 ZDR and tenant boundary | NOT-RUN | PASS: org B with org A's fingerprint is a miss, two keys under `quarry:page:{org}` (WS4.K image run, Redis db 6). PASS: ZDR scrape marker count 0 on an image at RUST_LOG=debug, standard identity 1. PASS: ZDR PDF scrape returns inline markdown, artifacts 0 before and after (WS5.B image run). PASS in CI-level tests: matrix generated from the Rust table, one test per gap row. NOT-RUN: cross-org search-cache probe and org-core outage probe (no search backend), the cache purge recorded (WS4.B), per-route-family log canaries on an image beyond scrape, the NATS erasure. WS4.H enforce waits on ADR-4. |
| SC6 One strict scrape contract | NOT-RUN (nearly) | PASS on an image (edbf13ce, 2026-10-16 and 89c67974 RUNBOOKS evidence): PDF and DOCX return text with `DOC_ROUTING=on`; PNG and ZIP are 415 `UNSUPPORTED_MEDIA`; `maxPages` is a 200 with a warning and `Deprecation`, strict mode 400; ZDR inline with no artifact; restart keeps the scrape record. PASS: flate bomb typed 415 on 1f4edbca. NOT-RUN: `GET /v1/scrape/{id}` second-org 404 and ZDR-reader 403 on an image; `Sunset` header; the gateway PNG message (needs WS5.G committed and a rebuilt gateway). |
| SC7 crawl-bound clause (WS6.A) | NOT-RUN | PASS on an image (3269c377): edge defaults (1000 pages, depth 5, `clamped_by` default) and hard clamps (5000, 10). NOT-RUN: the 7-page crawl with `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7` through the gateway and orchestrator (`pages_visited == 7`, `params.max_pages == 7`); `CONTRACTS.md` text; Go test run record. |

No criterion is PASS today. SC1 is the only one with a known FAIL. The shortest route to the first honest PASS lines is the work in `W1-work-breakdown.md`: lane A for SC1, lane B for SC2a, SC3 and SC4, lane C for SC5, lane D for SC6 and SC7.

## 7. Corrections to numbers the parent might repeat

- The 148.25 W1 total is right (section 1), but the per-workstream headlines in the plan (WS2 22, WS3 40) include optional slices that the 148.25 excludes.
- "Slices fully done" is 4, not "most of W1": 25 of 39 slices have their code committed, but only 4 have nothing left to build or run.
- WS5.C is closed on this branch except GitHub CI; it was listed as open work in the parent's item j.
