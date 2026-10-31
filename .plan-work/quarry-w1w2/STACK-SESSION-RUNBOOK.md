# Stack-evidence session runbook (STEP 2): SC1 to SC7, then list B

Written 2026-10-28 by unit D2-report. Owner order of 2026-10-28: list B is built and tested now with every flag off, observe or shadow; flipping happens in this session, not before. **Nothing in this file has been run.** It merges `L2-signed-in-steps.md` (the signed-in legs, unchanged and still the source for SC1) with every list B item of `DECISIONS-ROUND-2026-10-27.md` section 2 and the scripts the D2 units committed.

Branch `quarryv2-next` at `795fef111` or later. Paths: `Q` = `apps/Ingestion Plane/Quarry-v2`, `IP` = `apps/Ingestion Plane`.

## 0. Rules for this session

- **The user signs in themselves.** No agent reads, asks for, types, prints or pastes a password, token, cookie or connection string. Where a script needs a token (`QUARRY_TOKEN`, `E2E_TOKEN`) or a DSN file (`QUARRY_PROFILES_DSN_FILE`), the **user** gets it in their own signed-in browser or terminal (for example `GET /api/quarry/token` from the signed-in SPA tab, or a cookie jar the user exports) and sets the variable in **their own** terminal. The agent may then start the script in that terminal only if the user asks, and must never echo the variable. Scripts here never print a token.
- **Never the live stack.** Every step runs on the evidence stack (rebuilt images, side-by-side or gate stack, section 1). A broker restart (R.3 of L2) or any change to a live container needs the owner's go-ahead in chat, per stack.
- **Flags change only on the evidence stack**, through its env file or an evidence-only override file kept outside the repo. Committed compose defaults stay as they are. A production flip is a separate, owner-approved, one-commit change after the evidence; each item below names it.
- **A FAIL is recorded, never edited away.** A failed check means: do not flip anything for that item, add the observation to the item's depth file, and fix it in a new unit.
- **Docker health first.** On 2026-10-28 the engine wedged (500) during the Rust test run. Before the session: `docker-desktop-recover.ps1 -Apply` (twice if the stale socket dir comes back), then re-run the Rust lane (section 1, step 2) so the session starts from a known count. One container build or run at a time.
- **Who must be present.** Marked per step: **[USER]** = the user must be at the keyboard (sign-in, a token or DSN, a second account, or a go-ahead). **[AGENT]** = an agent can run it alone on the evidence stack.

## 1. Preparation (before SC1)

1. **[AGENT]** Docker recovered and answering (`docker ps` without a 500).
2. **[AGENT]** Fresh Rust lane on the branch head through qtest (the D2-test run did not finish): `cargo test --no-fail-fast` for quarry-core, quarry-transform, quarry-security, quarry-runtime, quarry-edge, quarry-browser. Compare with 2026-10-27 (3727 passed, 0 failed). Do not start the stack on a red lane.
3. **[AGENT]** Rebuild images from the branch (freeze the commit and record it in the results header):
   - quarry-edge (every list A and B edge change: billing key, reload refusal, takeover counter, X-12 guard, D16 stamp, D2-161 robots, X-19 masking, scrape actions blocked count, empty `MODEL_PLANE_URL` read as unset).
   - quarry-orchestrator (`pages_blocked_robots` in crawl and batch terminal events, `14dedfdff`).
   - quarry-control (no list B change; rebuild so the stack runs one revision).
   - verevonv3 gateway and SPA (the crawl composer row `3ba650802`; the gateway docs-only change `7df37a4b3`).
   - browser broker and model-gateway only if the stack does not already run images that contain the W1 broker fixes (L2 section 7 notes `grantlog.go`).
   - Use the Control Plane recipe (`scripts/run-control-plane.sh`) and the Model Plane recipe (`compose.sh`, then `recreate-with-live-env.sh` after a worktree build); a bare `docker compose build` exits 0 and does nothing for Control Plane.
4. **[AGENT]** Stack choice, as in `L2-signed-in-steps.md` section 1: the **gate stack** (`IP/docker-compose.yml` + `docker-compose.production.yml` + `docker-compose.grant-contract.yml`, gateway `BROWSER_GRANT_ENFORCEMENT=strict`, broker TTL 45 s) for SC1 legs A to C; the **side-by-side stack** (default 900 s TTL) for the restart check and for the list B items. The crawl fixture overlay `IP/docker-compose.crawl-fixture.yml` is added for X-10.
5. **[AGENT]** `docker compose config` on the stack's compose set with its env files (this was never run in the worktree: no env files there). Confirm the defaults the units committed: `QUARRY_EDGE__SCRAPE_ACTIONS` off, `QUARRY_EDGE__SCRAPE_CAPTURE` off, `QUARRY_EDGE__CRAWL_ROBOTS` observe, `QUARRY_EDGE__ROBOTS_UNREACHABLE` disallow, `QUARRY_EDGE__BUDGET_ENFORCEMENT` shadow, the four Model Plane flags 0 and `MODEL_PLANE_URL` empty.
6. **[AGENT]** Preflight, read only: `bash scripts/tests/browser-grant-prod-contract.sh --preflight-only` (repo root). Keep the transcript for the results header.

## 2. Phase 1: SC1 to SC7 (core proof; list B waits for it)

| Step | Who | What to run | PASS means |
|---|---|---|---|
| SC1 | **[USER]** signs in on the gate stack's SPA | `L2-signed-in-steps.md` sections 2 to 5 (legs A, B, C, console helper `L2`), the host-side scan of section 7, and the wrapper `scripts/tests/browser-grant-prod-contract.sh` for the legs the page cannot reach (A1 to A3, A5a to A5c, A6) | every check PASS in the L2 table; zero 401/403 and zero 5xx on the happy path; B.1 shows `strict` (400 `browser_host_not_grantable`); host-name lines 0 on broker, model-gateway and gateway |
| Restart | **[USER]** page stays open; **owner go-ahead** for the broker restart | `L2-signed-in-steps.md` section 6, R.1 to R.7, on the side-by-side stack. The psql row counts: if `psql` prompts for a password, stop; the owner supplies the count or the step uses broker-log evidence only | non-ZDR grant survives (200), ZDR grant lost (403 inactive), row counts as stated |
| SC2a, SC2b | [AGENT] (a user token from the user for the labelled-token legs: **[USER]**) | plan table at "SC2a" (`Evaluate` 403 `raw_script_denied` for every principal; takeover with `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` on the evidence stack) | as the plan row states |
| SC3 | [AGENT] | sweeper, cap+1 429 and the RSS runs (plan "SC3" rows; `QUARRY_BROWSER_MAX_SESSIONS=48` in the test env only) | as the plan row states |
| SC4 | [AGENT] | two-session viewport probe, typed `TIMEOUT`, sandbox launch (zero `--no-sandbox`, the wrapper's `check_no_sandbox`) | as the plan row states |
| SC5 | [AGENT] | cross-org cache probes incl. the `PageCache` `prev_fingerprint` probe, ZDR canary log scan, the unrun evidence scripts (cache purge and forensics, NATS erasure, `Q/scripts/evidence/ws4g-live-zdr-token-probe.sh`, `ws4-image-probes.sh`) | as the plan row states |
| SC6 | [AGENT] | PDF and DOCX through the stack with `DOC_ROUTING=on`, 415 for images and archives | as the plan row states |
| SC7 | [AGENT] | crawl clause (finite default 1000, `clamped_by:["default"]`); the budget clause is X-12 below | as the plan row states |

Record every SC row PASS, FAIL or NOT-RUN with its evidence type. **List B starts only when SC1 is PASS** (ADR-6 needs all of SC1 to SC7).

## 3. Phase 2: list B, in this order

Each item: the image it needs, what to run, what PASS means, the flag to flip on the evidence stack, how to flip it back, and the production change that follows (owner's call, not this session's unless the owner says so).

### 3.1 X-12 agent-run budget, then `on` (decision 3.3)

- **Image:** quarry-edge from `1b701f99b` or later (the edge guard).
- **Before the flip, shadow:** [AGENT] a scripted day in `shadow` (the default). Read `quarry_agent_budget_exhausted_total{mode="shadow"}` (the would-exhaust count: in `shadow` the step gate counts once per run segment, the first would-be refusal only, `take_shadow_report`, and a resumed run is a new segment that may count again; separately, every agent run start while the org allowance is exhausted counts once with `dimension="org_allowance"`; it does not count the legacy 429, X-12 depth item 4. After the flip the same counter with `mode="on"` counts every refused call, so a run refused three times adds 3: read a rate on it as refused calls, not as runs that ran out) and the takeover counters `quarry_agent_takeover_exempt_steps_total` and `quarry_agent_takeover_exempt_step_duration_seconds` (`3cbce1f0f`). There is no real agent traffic, so a scripted day is enough (decision 3.3).
- **Flip on the evidence stack:** `QUARRY_EDGE_BUDGET_ENFORCEMENT=on` (compose reads it into `QUARRY_EDGE__BUDGET_ENFORCEMENT`) and, for a short run, `QUARRY_EDGE__AGENT_DEFAULT_MAX_STEPS=5` in the evidence-only override (it is not in compose). Restart quarry-edge. Keep `GRANT_BUDGET_CLAIM` at `ignore`.
- **Run:** **[USER]** sets `QUARRY_TOKEN` (an org member's Quarry token) in their own terminal, then `node Q/scripts/x12-budget-stack-proof.mjs` (optional `EDGE_URL`, `GRANT_ID` if the stack requires grants, `RUNTIME_S`, `PERSON_STEPS`, `PAGE_URL`). The first step of every run the script drives is a navigate to `PAGE_URL` (default `http://fixture-a.crawl.test/`, so the crawl-fixture overlay must be up, or point `PAGE_URL` at a page the edge's browser can open): on the chromium driver a wait before any navigate is 502 `no_current_page`. That navigate counts in each bound, and the `cost` bound is sized from the flat table (navigate 0.001 + three waits at 0.0001 = `max_cost_usd 0.0013`). Use the script from `d0a07aade` or later; the version the 2026-10-28 session ran had neither (X12.5), and `ff12ef379` could FAIL a correct edge when a browser second ticked over after the gate admitted the last wait. Offline check: `--self-test` prints SELF-TEST PASS.
- **By hand:** [AGENT] resume of an idle-expired run is refused at once; `psql` on `quarry_agent_run_checkpoints`: `state->'budget'->>'exhausted' = 'steps'` for the steps run (the latch is `BudgetCheckpoint.exhausted` under the checkpoint's `budget` key, `step_receipts.rs`; one value, no rows printed); the SSE stream (`GET /v1/agent/runs/{id}/events/stream`, opened before the first step) shows exactly one `agent.budget_exhausted` however many calls are refused (once per segment; a resumed run is a new segment and may announce again). The SSE check needs a quarry-edge image from `a3f825098` or later (`agent.budget_exhausted` on the SSE stream); an older edge drops the event and leaves only a gap in the `id:` values.
- **Takeover check (decision 3.4 step 4):** **[USER]** a capped agent-class run is taken over by the person, the person steps past the bound (all 200), the ledger `meta.budget.steps` does not move, and after the hand-back the next agent step is 402. The script's `takeover` bound covers this; record it as the 3.4 evidence too.
- **PASS:** every bound of the script PASS (ceilings, steps, runtime, cost, people, guard, takeover) and the three manual checks as stated.
- **Flip back:** `QUARRY_EDGE_BUDGET_ENFORCEMENT=shadow` (or `off`), remove `AGENT_DEFAULT_MAX_STEPS`, restart.
- **Afterwards (owner):** `on` in the Ingestion env (decision 3.3 step 4); `off` is the documented rollback. The Model Plane 402 mapping follows in the Model Plane session; it does not block.

### 3.2 D16 Q2 profile owner: observe and the legacy count (decision 3.5)

- **Image:** quarry-edge and quarry-browser/quarry-runtime from `3f9017d62` or later (the run-start stamp; migration 0015 applies at the edge boot).
- **Note the time** the rebuilt edge with the stamp started (ISO 8601, UTC). It is the `--since` value.
- **Count:** **[USER]** creates a file holding the libpq connection string of the Quarry Postgres (a user that can SELECT `quarry_profiles`) and sets `QUARRY_PROFILES_DSN_FILE` to it; the agent never opens that file. Then `bash Q/scripts/evidence/d16-legacy-profile-count.sh --since <time>` (read only, counts and tenant ids). The offline `--check` already passed.
- **Observe:** the stack keeps `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=observe` (the default). Read `quarry_profile_owner_decisions_total` and `quarry_profile_owner_stamps_total` (watch `skipped_service`: model-driven runs whose profiles will be hidden under enforce).
- **PASS (this session):** the count runs and its numbers are recorded; the stamp counter shows `stamped` for a person's persistent run.
- **Enforce on the evidence stack (optional in this session, only to run the two-user evidence):** `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=enforce` in the evidence override, restart. **[USER]** with two signed-in users A and B in one org: A starts a run with `persist_profile: true`; `GET /v1/profiles` as A lists it with `owner_is_caller: true`; as B it is absent and `GET /v1/profiles/<id>` is 404 (under observe it is served and a `profile.owner.would_deny` line appears). Flip back: `observe`, restart.
- **Afterwards (owner):** a one-week observe on the stack that will carry it, then the choice between a manual org-confirmed rescue and a backfill from run records (user actors only), then `enforce`. Not this session.

### 3.3 X-10 robots.txt: observe legs, soak, then enforce (decisions 3.1 and 3.2)

- **Image:** quarry-edge from `2ba69b8dc` (D2-161 cached-page check) and the ROBOTS-RFC parser fixes (`c7bf539d4`, `ade9c9d98`, `eddc51302`); quarry-orchestrator from `14dedfdff`; SPA from `3ba650802`.
- **Before the soak (owner):** two parser questions from the ROBOTS-RFC unit are still open and the soak measures this parser: a CR-only line-ending file is read as one line and fails open, and a group with two `Crawl-delay` values keeps the last one. Decide before the soak starts (section 5).
- **Run (observe):** [AGENT] with the crawl fixture overlay: `bash Q/scripts/e2e/crawl-scope-robots.sh --expect legacy`, then `--expect observe` (`QUARRY_EDGE__CRAWL_SCOPE_MODE=observe`, robots `observe`). PASS: robots.txt fetched on fixture-a, `/private/secret` still fetched (observe blocks nothing), decisions counted in `quarry_robots_decisions_total{mode,outcome}`.
- **Run (enforce, evidence stack only):** `QUARRY_EDGE_CRAWL_ROBOTS=enforce`, restart, `--expect enforce`. PASS: no `/private/secret` and no `/ext` fetched, page fetches on fixture-a at least 2 s apart (Crawl-delay 2), `/v1/map` on the seed is 200 and lists no `/private/` URL. The `unreachable` (503) and `redirect` (metadata) legs report NOT-RUN until fixture hosts exist (`UNREACHABLE_SEED_URL`, `FIXTURE_UNREACHABLE_CONTAINER`, `REDIRECT_SEED_URL`, `FIXTURE_REDIRECT_CONTAINER`); adding them is a small follow-up, not this session's blocker. Flip back to `observe` after the leg.
- **Composer row:** **[USER]** in the signed-in SPA, a crawl of the fixture under enforce shows "N pages blocked by robots.txt" (nb and en).
- **Soak:** deploy in `observe` and soak until the volume target is met (enough crawls across enough distinct sites, topped up by an internal crawl of a known site list). Go criterion: `would_unreachable` and `would_deny` as a share of all robots decisions. Distinct origins are not countable from the counters (no origin label); either add the small origin-level signal first or accept the per-decision ratio (owner). The soak is days, not part of this session.
- **Afterwards (owner):** if the criterion holds, `QUARRY_EDGE__CRAWL_ROBOTS: enforce` and `QUARRY_EDGE__ROBOTS_UNREACHABLE: disallow` in `docker-compose.production.yml` in one revertible commit; scope mode stays observe; watch `page_blocked` for a week. Rollback: `observe` (or `off`) and restart.

### 3.4 X-19 receipts evidence (decision 3.6, condition (d))

- **Image:** quarry-edge and quarry-runtime from `b40ee90b5` or later.
- **Run:** on a rebuilt edge with the Postgres store and `zdr:false`. **[USER]** provides the user token for the agent run. Drive a login form on a fixture page: click the password field, then one `press` per character, then `Enter`. Then [AGENT] `psql -c "SELECT action->>'key' FROM quarry_step_receipts WHERE run_id='<run>' ORDER BY step"`. PASS: every character key reads `[redacted:password]`, `Enter` reads `Enter`; the `ActionStarted` events on the stream show the same.
- **Second leg:** `QUARRY_EDGE__RECEIPT_TEXT_MODE=all` in the evidence override, restart, type into a search box. PASS: the event and the stored row both show `[redacted:secret]`. Flip back: unset (default `sensitive`), restart.
- **Also from the WS12.S3 runbook section:** a hand-inserted legacy row is masked on read, and the procedure `redacted_input` check.
- **What this does not settle:** condition (c) (retention or erasure of receipts) is a design only (`D2-x19-retention-design.md`). Receipts stay off for real users until (a) to (d) all hold; (a) and (b) are built, (d) is this step, (c) is open.

### 3.5 ADR-6 proof on fixture hosts (decision 3.8)

- **Only after SC1 to SC7.** Evidence stack only, never production.
- **Image:** quarry-edge and quarry-runtime from `9c548a823` or later (blocked-request count for actions-only runs).
- **Flip on the evidence stack:** `QUARRY_EDGE_SCRAPE_ACTIONS=user` and `QUARRY_EDGE_SCRAPE_CAPTURE=on`; keep `QUARRY_SCRAPE_HEAVY_PERMITS` at 1. Restart the edge.
- **Fixture:** serve `page.html` (text in `D2-B-adr6-proof-progress.md`) from a throwaway nginx or the fixture host, reachable by the edge at `E2E_FIXTURE_URL`. Confirm the stack's egress does NOT allow `offhost.example.net`.
- **Run:** **[USER]** sets `E2E_TOKEN` (a user token; optional `E2E_SERVICE_TOKEN` without `scrape:interact`) in their own terminal. Then `QUARRY_EVIDENCE_STACK=1 E2E_EDGE=<edge base> E2E_FIXTURE_URL=<fixture> cargo test -p quarry-edge --test scrape_actions_live -- --ignored --nocapture --test-threads=1` (through qtest or a host toolchain that reaches the edge). PASS: one `ok` per check and `all N checks passed` (N = 11, plus one each for the optional probe and service token).
- **Also:** `QUARRY_EVIDENCE_STACK=1 cargo test -p quarry-edge --test scrape_actions_real_token --test scrape_actions_real_token_user_mode -- --ignored` (no stack needed; 5 and 2 passed offline already). `make test-scrape-capture` in `IP` on the same stack (it takes `E2E_TOKEN` or `E2E_COOKIE_JAR`, **[USER]**). `docker logs <edge> 2>&1 | grep -c SECRET_SENTINEL_123` prints 0. Cache and CAS untouched: count `GET /v1/artifacts` before and after; a plain scrape of the fixture URL afterwards is not served from an actions run's entry.
- **Flip back:** `QUARRY_EDGE_SCRAPE_ACTIONS=off` (or unset), restart.
- **Afterwards:** nothing opens. `user` outside dev needs a billing meter and a per-org entitlement (option C, a separate decision); `on` needs a verified Model caller and an approval gate.

## 4. List A items that still need the stack

These were built in list A without a stack and leave one stack check each. [AGENT] unless marked.

| Item | Check | PASS means |
|---|---|---|
| DATAPLANE-OPTION-B (decision 3.12 step 4) | recreate quarry-edge with the new compose defaults and watch it during the session | no Model Plane token mints and no Model Plane calls from quarry-edge; `/v1/answer` answers 501 (the gateway relays it as `model_error`, a known follow-up) |
| OPENAPI-INTERNAL (D2-312) | from outside the stack's public ingress, request `/v1/internal/run_page` and the page-images route | refused at the ingress (not routed); the page-images route is never public |
| BILLING-IDEMPOTENCY | a crawl submitted twice with the same Idempotency-Key | one usage event keyed by Control's job id; the replay (Control 200) bills nothing |
| MP-GATEWAY-AUTH | an agent reload through the gateway | refused by Quarry (it is already covered by wire tests; the stack run is confirmation) |
| TAKEOVER enforcement (SC2a) | `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1` on the evidence stack: an agent step during a person's takeover is 409 `USER_TOOK_OVER`; flip back to unset | as stated; production flip is the owner's, after WS11 maps the refusal |

## 5. Owner questions to settle before or during the session

1. ROBOTS-RFC follow-ups before the X-10 soak: CR-only line endings (fail-open today) and the two-Crawl-delay reading.
2. X-10: accept the per-decision ratio as the go criterion, or add the origin-level signal first.
3. The commit trailer: several list A and B commits carry `Co-Authored-By: Claude Haiku 5.5`; the workflow asked for the Opus line. Rewriting local commits is forbidden in the workflow; the owner decides whether it matters.
4. D16: what to do with model-driven runs that persist a profile (`skipped_service`) after the observe week.

## 6. Results header (one per stack)

Date, operator, stack name and compose files, branch commit, image ids and `SOURCE_REVISION` of quarry-edge, quarry-orchestrator, quarry-control, gateway, broker; broker TTL; `BROWSER_GRANT_ENFORCEMENT`; every flag value this session changed and the time it was changed back; preflight transcript attached. Then the L2 table of `L2-signed-in-steps.md` section 8, the SC rows, and one row per list B item: PASS, FAIL or NOT-RUN, with the transcript file name.
