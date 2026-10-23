# W1 work breakdown (unit w1-synthesis, 2026-10-21)

Branch `quarryv2-next`, HEAD `473d0a78`. Companion to `W1-closeout-baseline.md` (the per-slice table, the percentages and the SC1 to SC7 table). Nothing here was built; no repository file was edited.

Method: each item a to l of the parent's list was checked against the plan (slice detail paragraphs, 1.2, 4.3, 6.2, the build ledger) and against the code. Statements say "read", "grep" or "ran" when I did it myself; "census" when they come from the census and I did not re-check them; "ESTIMATE" for effort numbers, which are mine and not measured.

Definition used for "W1 at 100 %": every DoD and deployment item of the 39 W1 slices (baseline table) is verified at the plan's evidence level, except the items that are strictly outside this branch (GitHub runs, counsel, customer notice, PRE-1, live-system operator actions, other planes). Those are listed in baseline section 5 and are not on any lane.

## 1. Corrections to the parent's list (summary; detail under each item)

1. Item b, "session-grant renewal (a Quarry endpoint that swaps a live run's grant id)": not part of any W1 slice's DoD. The plan reserves `Renew` for WS8.C (`RenewGrant`, plan lines 1538 and 1582); it appears only in the ledger's "Not done" text of WS1.B. A design note already exists: `.plan-work/quarry-w1w2/wf-R4-grant-renewal-design.md` (options A fail clearly, B TTL policy, C Quarry swap endpoint plus a broker `Renew`, D renew by resume). Its recommendation is A now plus the `ttl_seconds` half of B, C only inside WS8, D as a stop-gap. I checked that neither A nor B was built: `browser_run.rs:625` still sends `ttl_seconds: 0` and `git grep grant_too_short` finds nothing. So the W1-sized item is the small A+B hardening (about 0.75 d, optional, owner decision), not a swap endpoint.
2. Item b, "the Quarry-side grant checks and their tests": already built and passing in non-prod form. `user_cannot_use_another_users_grant`, `zdr_grant_cannot_back_a_non_zdr_run`, `validated_grant_overrides_client_domains_in_every_mode` exist twice (agent_routes/tests.rs:2082, 2105, 2114 and tests/start_run_grants.rs:96, 182, 249). Only the prod-mode controls (`prod_*`, WS1.A2's `browser_grants_prod.rs`) are missing.
3. Item b, "L1 pins of the derive field numbers": already done (`browser-grant-contract.test.mjs:98`, commit `be85fa94`; I ran the file: 19 of 19 pass).
4. Item b, the execution-core test: still absent (grep: no `cross_domain_goto` anywhere under `apps/Model Plane/rust`). It belongs to the execution-core file set, so it moves to lane E.
5. Item b, plan names `proxy_with_delegations_sets_x_browser_authorization_only_when_present` and `required_browser_token`: the code does it differently (`domains/browser.rs:4129 proxy_model_json_with_delegated`, header set at 1631, 4303, 4355; tests at 6556, 6808, 6829). Fix the plan text, not the code.
6. Item c, "the browser-broker-ci integration job": already exists (b40242f7; jobs broker, integration, bootstrap-db, contract). Never run on GitHub.
7. Item c, "the compose depends_on": cannot be built. The database is `ingestion-postgres` in the Ingestion compose project; the Model Plane compose says so at `apps/Model Plane/deploy/docker-compose.yml:1637-1641` and relies on the entrypoint failing and `unless-stopped` plus `/readyz`.
8. Item c, "grant-level restart-survival against the dev broker": the broker-level proof exists (e2e scenario a, `37712250`, isolated, real docker restart). What is open is the `/step`-level leg (needs WS1.A2) and a side-by-side restart; the live broker has had 0 grants before and after its restart.
9. Item d, "delete stealth.rs": already deleted by WS3.E (2e62ad5a; grep: no `stealth.rs`, no `stealth_level`, no `STEALTH_SCRIPT`). The remaining WS3.I work is the escalation mechanism, the preset rename with alias, and the default flip.
10. Item e, "the R-26 boot check": already built and wired (`quarry-edge/src/main.rs:205-215`, `slots.rs:269 session_cap_warnings`, commit e50046a9). Only its boot line is to be captured. Also: the reaper default is `log` (`slots.rs:223`), so the "reaper at SESSION_IDLE_S=30" evidence needs `QUARRY_BROWSER_REAPER=1`.
11. Item f, WS3.J: the Model Plane half is wider than "challenge, planner rule, 409/403 mapping": see item f. The overlap with branch `model-plane` is measured in section 6.
12. Item g, WS4.B: the real purge and forensics on live gateways are operator actions (OWNER). Only the side-by-side run is buildable evidence. WS4.C needs a NATS JetStream with a provisioned durable, not just "a real NATS".
13. Item h, WS4.H: stage A (flag, helper, observe and enforce paths, tests) is already built. "Orchestrator sending zdr" is a design decision, not a one-line change; the stamp-at-create option fits the approved defaults, the fire-time lookup (X-9) is not in the relayed approval list.
14. Item i, WS4.I: "absent" is wrong for the source: the catalogue `zdr-refusals.json` and the SPA `zdr-refusals.ts` are committed on branch `model-plane` (5ae7fe13), only absent on `quarryv2-next`.
15. Item j, WS5.C flate-bomb on a rebuilt image: already verified (RUNBOOKS.md:406 row 4, image 1f4edbca). Not open.
16. Item k, "WS6.A: confirm nothing is left": things are left (CONTRACTS.md text, Go test run record, 7-page crawl evidence, recorded-history replay).
17. Item a: five agent-route call sites, not the plan's three stale line numbers; plus pieces the parent's list omitted (auth-core P2 describe, token manifest digest propagation, `principal-registry-parity`, a rebuilt auth-core, the WS3.A proof on the same stack).
18. Whole parent list: omitted build items in already-"built" slices (listed in section 3: WS2 leftovers, WS3.D's 13 call sites, WS5.H's ScrapeRequest completion, WS4.A clippy, WS6.A docs, WS1.A1 doc and log items).
19. Item i: the uncommitted `go.work.sum` hunk (+1 line, `go.uber.org/automaxprocs v1.6.0/go.mod`) is not WS5.G's.

## 2. Items a to l, corrected

Effort numbers are ESTIMATES in engineer-days. "Opus review" says which parts need an Opus-grade reviewer and why; everything else can be reviewed at the normal level.

### a. WS1.A2: Quarry-to-broker credential and the SC1 harness (plan 6 d, nothing built)

Corrected scope (plan lines 637 to 654, checked against code):
- Registry: `apps/Control Plane/config/plane-service-principals.json`: `quarry-edge` audiences gain `browser-broker`, `scopesByAudience["browser-broker"]=["browser:read"]`, `retentionByAudience["browser-broker"]="zdr"`; top-level scopes gain `browser:read`. No principal may hold `browser:write` (the L1 test already asserts it and passes).
- Manifest: new row `browser-grant-validate` in `quarry-runtime/contracts/cross-plane-token-requests.json` (audience `browser-broker`, scopes `["browser:read"]`, `granted:true`), and the digests that follow: its `.sha256`, the auth-core vendored copy `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+ `.sha256`), and the pin in `scripts/tests/zdr-contract-sync.test.sh`.
- Quarry service tokens: `ServiceTokenAudience::BrowserBroker` (endpoint `/api/browser-broker/internal-token`) in `service_tokens.rs`, plus the three tests that hard-code today's two audiences and six rows (`EXPECTED_REQUESTS` becomes seven, the `other => panic!` arm, `token_request_contract.rs` `expected_path`). `AuthCoreServiceTokenProvider::token_for_org(org_id, request, force_refresh)` already exists (service_tokens.rs:325-334, 485), so G1 is wiring, not a new minter.
- G1: `GrantValidator::validate_for_org(org_id, grant_id)` with a default body; `GrpcGrantValidator` gets a token source and calls `token_for_org` per call, retrying once with `force_refresh` on `Unauthenticated`. Why org-scoped matters: the broker resolves the grant with `GetScoped(grant, principal.OrganizationID, ownerFilter)` (server.go:191), so a token for the wrong org yields NotFound, and a service token is not bound to the grant's owner (known gap pinned by `known_gap_a_service_principal_is_not_bound_to_the_grants_owner`, start_run_grants.rs:331).
- G6: `FailedPrecondition` (the broker's answer for a revoked or expired grant, errors.go:31 and 43) must become `active:false` and a 403 `SECURITY_BLOCKED`, not `DriverFailed` (browser_broker.rs:211-221).
- Boot gates (main.rs): refuse a gRPC validator without service tokens; refuse an HTTP-only validator in production (`HttpGrantValidator` posts to a route no Model Plane service serves, plan line 8). Existing generic gate is at main.rs:290-294; `browser_grant_http_validator` (1645-1665) returns a no-op validator when no URL is set.
- Call sites to move to `validate_for_org` (current code, not the plan's stale lines 2258, 3095, 3280): `agent_routes/admission.rs:38` (inside `validate_run_grant`, itself called at admission.rs:371 and tabs.rs:145), `admission.rs:396` via `validate_sensitive_approval`, `lifecycle.rs:218` (start_run), `sensitive_approval.rs:156`.
- Tests: `tests/browser_grants_prod.rs` with the six `prod_*` names; token-request row test; auth-core P2 describe; L1 A2 assertions (registry rows, manifest row byte-identical copy, compose network, production overlay).
- Deploy files: `apps/Ingestion Plane/docker-compose.grant-contract.yml` (new; env and volumes only, never removes `read_only`, `cap_drop`, the tmpfs set or `QUARRY_BROWSER_NO_SANDBOX: "0"`), validator URL line in the base compose (also closes WS1.A1 leftover 4), `.github/workflows/browser-grant-contract.yml` (new).
- Harness: `tests/e2e/browser-grants-prod-contract.spec.ts` (Playwright API legs A to C; legs B and C written with `test.fail()` and the recorded expected-failure texts, flipped by item b), `scripts/tests/browser-grant-prod-contract.sh` (wrapper, `--boot`, preflight for the auth-core registry, five-hop log scan for `\b40[13]\b`, `PermissionDenied`, `Unauthenticated`, `browser grant is not authorized`, `browser grant_id is required`).

Owned files (concrete): `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/{service_tokens.rs, token_request_contract.rs, grant_validator.rs, grpc/browser_broker.rs}`, `.../quarry-runtime/contracts/cross-plane-token-requests.json` and `.sha256`, `.../quarry-edge/src/main.rs` (validator wiring at 777-796 and the HTTP helper at 1645-1665, the generic boot gate at 290-294), `.../quarry-edge/src/test_support.rs` (append), `.../quarry-edge/tests/browser_grants_prod.rs` (new), the four `agent_routes` call-site lines above (first window only, see lanes), `apps/Control Plane/config/plane-service-principals.json`, `apps/Control Plane/auth-core/src/auth/{plane-service-principals-policy.contract.spec.ts, fixtures/cross-plane-token-requests.json, fixtures/cross-plane-token-requests.json.sha256}`, `apps/Ingestion Plane/docker-compose.yml`, `apps/Ingestion Plane/docker-compose.production.yml`, `apps/Ingestion Plane/docker-compose.grant-contract.yml` (new), `scripts/tests/browser-grant-contract.test.mjs`, `scripts/tests/browser-grant-prod-contract.sh` (new), `apps/Frontend Plane/verevonv3/tests/e2e/browser-grants-prod-contract.spec.ts` (new), `.github/workflows/browser-grant-contract.yml` (new).

Hot files shared: `quarry-edge/src/main.rs` (C wires a tombstone sink later; edit A first), `agent_routes/{admission,lifecycle,sensitive_approval,tabs}.rs` (lane B's files), `test_support.rs` (lane B appends `insert_live_run`), `scripts/tests/zdr-contract-sync.test.sh` (lane C adds rows for WS4.I and WS4.J), `docs/RUNBOOKS.md` (append only).

Tests and evidence: `cargo test -p quarry-runtime grpc::browser_broker service_tokens token_request_contract`, `cargo test -p quarry-edge --test browser_grants_prod` and `--test start_run_grants` (container, via qtest); `cd "apps/Control Plane/auth-core" && npx jest plane-service-principals-policy.contract` (15 pre-existing failures in 3 suites: diff the failing sets against HEAD, never trust the exit code); `bash scripts/tests/principal-registry-parity.test.sh`; `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`; `node --test scripts/tests/browser-grant-contract.test.mjs`. Then the L2 run on the exit-gate stack (section 7).

Opus-grade review: yes, all of the G1/G6 code and the registry and overlay changes. Reasons: it is the authentication and authorisation boundary between two planes (token audience, scope, org binding, fail-closed mapping of a revoked grant, retry semantics that must not turn an auth failure into a silent allow), and the contract overlay is a production-hardening guarantee (an overlay that quietly drops `read_only` or `cap_drop` invalidates the L2 evidence). The harness itself needs normal review.

Risks: (1) The registry row must be served by an auth-core that has it. The live auth-core cannot be changed here. The 2026-10-16 side-by-side stack minted user tokens from the live auth-core (`host.docker.internal:3011`); a quarry-edge service token with the new row needs an auth-core that carries the new registry and whose JWKS the broker trusts. [UNVERIFIED] how to do that without touching the live stack: either a side-by-side auth-core with its own database and seeded dev accounts, or an owner decision to swap the live auth-core. This is the largest unknown of the whole W1 gate (open question 5 below). (2) L2 needs outbound HTTPS to `example.com` (plan 6.2). (3) Manifest and digest churn: touch only the appended row, re-pin once. (4) `known_gap_...` test must stay green or flip deliberately. (5) The generic gate already requires a validator in production, so a placeholder URL boots only because gRPC connect is eager (verification note: "DriverFailed: grpc connect failed").

Effort ESTIMATE: 6 d build (registry and manifest 1, Quarry G1 G6 and gates 2.5, prod tests 1, overlay, harness and workflow 1.5), plus the L2 run counted in item l.

### b. WS1.B leftovers (plan 10 d; most is built)

Corrected scope:
- Add `cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant` to `apps/Model Plane/rust/services/execution-core/src/browser_agent/browser_risk.rs` tests (file unchanged on `model-plane` since the merge base: verified). Moves to lane E's file set.
- Set `BROWSER_BROKER_MAX_RUN_DOMAINS` in `apps/Model Plane/deploy/docker-compose.yml` (broker block has TTL, session domains, active-per-owner but not this one: read 1625-1629) and decide where the gateway gets `BROWSER_GRANT_ENFORCEMENT` for the L2 stack (grep found no compose in the Model Plane, Ingestion or Frontend compose files that sets it; the gateway defaults to `strict` under production, `config.rs:469-485`).
- Plan errata for the two names that do not exist (correction 5). If the owner wants the L1 (1b) assertion kept, assert that the gateway sets `x-browser-authorization` only through `proxy_model_json_with_delegated` and contains no `format!("session:` literal; `browser.rs` already has no `session:` grant string (L1 passes).
- L2 legs B and C: remove the `test.fail()` markers written in item a once WS1.A2 is on the stack; the three prod-mode negative controls are item a's `prod_*` tests.
- The production `strict` flip is evidence-gated (item l), not a build item.
- Renewal: not in scope (correction 1). Facts from the R4 note, re-checked in code: the session grant TTL is `BROWSER_BROKER_GRANT_TTL_SECONDS` 900 by default (clamp 30 to 3600, compose line 1627); a run grant expires at `min(now+ttl, parent.ExpiresAt)`; Quarry revalidates the grant on every action (`admission.rs:38` via `validate_run_grant`); execution-core defaults `max_runtime_s` to 120 s, so the failing case is a run started late in a session or with a long explicit runtime. Optional small hardening (R4 options A and the TTL half of B): model-gateway refuses a start with 409 `browser_grant_too_short` when the derived grant would not outlive `max_runtime_s`, and derives with `ttl_seconds = clamp(max_runtime_s + 60, 60, broker max)` instead of 0; gateway maps the new code to the existing reopen-the-session path; the typed expiry failure in execution-core is sibling (b)'s file (lane E, optional). Files: `model-gateway/src/browser_run.rs` (HOT: changed on both branches, `ttl_seconds: 0` at 625 and its test at 2298), gateway `domains/browser.rs`. Whether W1 includes this, accepts the cliff, or pulls `RenewGrant` forward from WS8.C is an owner decision (open question 1). Effort ESTIMATE 0.75 d if taken.

Owned files: `apps/Model Plane/deploy/docker-compose.yml` (broker block only), `scripts/tests/browser-grant-contract.test.mjs` (shared with a and c), the L2 spec (shared with a), `docs/plans/...` errata (single writer, see section 5). Execution-core test: lane E.

Hot files: `apps/Model Plane/deploy/docker-compose.yml` (model-plane branch changed it, +143 lines vs the merge base: the broker block edit will merge textually only if the hunks are disjoint [UNVERIFIED]).

Opus-grade review: the negative controls (do the three prod-mode controls fail closed: no grant, forged id, revoked grant not 5xx) and the cross-domain risk test (security semantics of the planner's risk flag). The rest is mechanical.

Effort ESTIMATE: 1.75 d (compose 0.25, errata 0.25, L1 decision 0.25, execution-core test 0.25, flipping legs 0.75 inside the item l run).

### c. WS1.C leftovers (plan 6 d; built and integration-tested)

Corrected scope: L1 assertion 7 (`migrations/0001_browser_grants.up.sql` has the `browser_grants_no_zdr_id_check` CHECK and no `zdr` column; add a migration assertion to the L1 file); ledger restart-semantics table (non-ZDR grants survive a restart, ZDR grants are lost with the next `/step` 403, broker unavailable is 5xx or 503 with no fail-open, Postgres unavailable fails `/readyz`); evidence: restart the broker on the side-by-side stack with a live non-ZDR grant, `/step` 200 after, row in `psql`; ZDR grant after restart `/step` 403 `inactive`, row count unchanged (needs item a); rerun L2 legs A to C against the Postgres-backed broker (item l). Drop the `depends_on` item (correction 7). X-2 (production rollout) is not in the relayed approval list; the code already implements its recommended default; record that rollout waits.

Owned files: `scripts/tests/browser-grant-contract.test.mjs` (append), `apps/Model Plane/docs/decisions/ledger.md` (shared with a: the WS1.A1 entry), nothing else in code.

Hot files: `ledger.md` (model-plane changed it, +239 lines: an append at the end will conflict textually at integration; prepare the entry as its own commit so it can be redone), the L1 file.

Opus-grade review: the evidence claim about ZDR grants (memory only, never Postgres) and the restart semantics statement; the code was already reviewed (reviewed = yes).

Effort ESTIMATE: 1.0 d (L1 0.25, ledger table 0.25, restart recipe on the stack 0.5), `/step` leg inside item l.

### d. WS3.I: honest identity, preset rename, default flip (plan 3 d, nothing built)

Corrected scope: ADR-5 approved. Plan lines 903 to 909 (read). Build: `quarry-runtime/src/escalation.rs` (new; `EscalationTrigger`, `escalation_allowed(mode, trigger)`, `QUARRY_BLOCK_ESCALATION=legacy|honest`, default `legacy` in the first commit); wire it into `fetch.rs` (status-driven proxy rotation at ~252 only when allowed; transport-error rotation unchanged), `fallback_driver.rs` (~96), `egress_broker.rs` (label `block_rotation` at 149 to `health_rotation`), `driver_plan.rs` (40-45: `prior_block_signals` ignored in `honest`, field stays accepted and documented deprecated in openapi.yaml); `pipeline.rs:608` still uses `is_block_status` (the plan says it is kept for slow-down); preset: `services/quarry-control/internal/resources/presets.go:84` rename to a neutral id (for example `careful`) with `stealth` as a one-release deprecated alias, `Block` set to `{On: TrigChallenge, Action: BlkAbort}`, `presets_test.go` updated (lines 51, 61, 70, 93, 107 use `stealth`); docs (`PROGRESS.md`, `ROADMAP.md`, `ARCHITECTURE.md:91`, `GAP.md:245,466`). Not needed: deleting `stealth.rs` (done). The request-side reserved key `stealth` in `quarry-core/src/scrape.rs:241` and `routes.rs:2030` stays (it is a refusal list, not stealth code).
The default flip to `honest` is a separate last commit after a before/after measurement on the stack (plan risk: "scrapes that a TLS or browser tier used to rescue may regress"). ADR-5 is approved at its default, so the flip is allowed; the measurement is evidence, not a decision.

Owned files: `quarry-runtime/src/{escalation.rs (new), fetch.rs, fallback_driver.rs, egress_broker.rs, driver_plan.rs, fingerprint_rotation.rs}`, `quarry-control/internal/resources/{presets.go, presets_test.go}`, docs above, `docs/openapi.yaml` (the deprecated-field line only).

Hot files: `quarry-runtime/src/lib.rs` (`pub mod escalation;`, one line), `pipeline.rs` (read only unless the slow-down use needs a change), RUNBOOKS (append).

Tests and evidence: plan names `honest_mode_never_rotates_on_status`, `honest_mode_still_rotates_on_transport_error`, `legacy_mode_unchanged`, fetch.rs wiremock origin answering 403 with a two-proxy pool (honest: one attempt, legacy: two), `prior_block_signals_ignored_in_honest_mode`, Go `go test ./internal/resources/...`. Evidence: rebuilt edge with `QUARRY_BLOCK_ESCALATION=honest` and a two-entry `QUARRY_PROXY_POOL`, a 403 origin gives one attempt and `grep -c rotating` 0, `GET /v1/presets` lists the new id and the alias. [UNVERIFIED] a stable public 403 origin: use a local fixture server on the side-by-side network behind the SSRF allowance, or document that a public origin was used.

Opus-grade review: the mode matrix (which trigger may change identity) because it is the ADR-5 policy in code and a regression there silently reintroduces identity rotation on block statuses; the preset alias because the id is public API (SDKs). The rest is normal.

Risks: scrape success rate regression in `honest`; public preset id; hot-file edits in `quarry-runtime`.

Effort ESTIMATE: 3 d plan; evidence 0.5 d on top.

### e. WS3.F deployment evidence and the R-26 boot check

Corrected scope: no code for the R-26 check (done, correction 10). Evidence only, on the final rebuilt edge (plan WS3.F detail, census):
1. 13 runs with `QUARRY_BROWSER_MAX_SESSIONS=12`: set `QUARRY_EDGE__MAX_CONCURRENT_RUNS` (default 32) at least 13 so the driver cap, not the run cap, trips: the 13th returns 429 `RATE_LIMITED` with `details.reason browser_session_cap`, `scope driver`. The R-26 WARN will fire at boot with these values: capture the line as the boot-check evidence.
2. `docker exec ... pkill chromium`, then a run succeeds; logs show "browser process lost" and "browser slot launched generation=2".
3. Abandoned run reaped with `QUARRY_BROWSER_SESSION_IDLE_S=30` and `QUARRY_BROWSER_REAPER=1` (the default is `log`, which closes nothing: slots.rs:223); `reason=idle`, `sessions_open` drops.
4. `/ready` 503 with the Chromium binary hidden inside the container, then recovery when restored.
5. One profile directory per slot (`ls profiles | wc -l`), and the missing test for profile-directory accumulation (small build: a test beside `sweep_stale_launch_dirs`, chromiumoxide.rs ~4550).
Also capture the memory-limit situation: neither compose file sets a memory limit on quarry-edge (grep: limits exist only for other services); the X-15 number is an owner decision.

Owned files: `quarry-browser/src/{chromiumoxide.rs, slots.rs}` only for the small test (HOT: chromiumoxide.rs is lane B's), evidence notes under `.plan-work`, RUNBOOKS append.

Opus-grade review: none for evidence; the c8c31a26 open race (two releases of one key and an acquire) is a concurrency question that deserves one Opus read if lane B touches slots.rs.

Effort ESTIMATE: 1.5 d on the evidence stack, 0.5 d for the test.

### f. WS3.J Model Plane half (plan 6 d, 4 d in sibling planes) and overlap with `model-plane`

Corrected scope (plan lines 913 to 915, code read):
- `BrowserObservation.challenge: Option<BrowserChallenge {kind, strength, block_reason, recovery, confidence_per_mille}>` in `execution-core/src/browser_agent.rs` (struct at 171-186); every construction site adds `challenge: None`: `browser_agent.rs:1180, 1208` (tests), `browser_agent/browser_risk.rs:539, 747` (tests), `llm_planner.rs:1006` (test), `quarry_agent.rs:460` (production), `tool_bridge/mod.rs:1083`, `tests/browser_agent_e2e.rs:177` (the grep for `BrowserObservation {` under `apps/Model Plane/rust` finds these construction sites plus the struct itself).
- `quarry_agent.rs`: `WireObservation` (line 219) parses `challenge` with `#[serde(default)]`; `observation_from_wire` (407) maps it. Mapping `CHALLENGE_DETECTED` 403 to `ObservationStatus::Blocked` instead of `AgentClientError::Status` is sibling (b)'s decision (B6, R-22): the current 403 path is `AgentClientError::Status` (quarry_agent.rs:680-705).
- `llm_planner.rs` (`observation_text`, rule text near 236-242): a `challenge:` line and the recovery rule (no captcha solving, no identity change, at most one wait of up to 5 s and one reload, then report blocked and ask the user to take over or use another source; login, consent, paywall, rate limit and bare 403 are not bot blocks).
- `model-gateway/src/http_routes.rs`: the same rule text in the browser planner prompt constants (`BROWSER_SUGGEST_SYSTEM_PROMPT` at 2005 is the nearest; the plan says const 2013-2021 [stale line numbers]).
- Tests: `observation_from_wire_maps_challenge`, `planner_text_includes_challenge_and_recovery_rule`; `tests/e2e/browser-workspace-blocked.spec.ts` (new, Playwright, pattern `browser-workspace-live-takeover.spec.ts`).
- The parent's "409/403 mapping" is wider in the plan: sibling (b) also owns the typed-error mirrors for `USER_TOOK_OVER` (WS2.C), `TIMEOUT` effect, `TARGET_REPAIR_REQUIRED`, `RATE_LIMITED` driver reasons, `UNSUPPORTED_MEDIA` (WS5.A), `BUDGET_EXHAUSTED` 402 (WS6.B), and the removal of the `Evaluate` variant (`quarry_agent.rs:161`, WS2.A). None of those is in WS3.J's DoD; they are the "Model Plane consumers" items listed as OTHER-PLANE in the baseline. Whether to do them in this branch's own commits is open question 6.

Overlap with branch `model-plane` (measured with `git diff --stat <merge-base> model-plane -- <file>`, merge base `afb83928`; `quarryv2-next` is 294 commits ahead and 102 behind):
- No change on `model-plane` for: `execution-core/src/{browser_agent.rs, quarry_agent.rs, llm_planner.rs, tool_bridge/mod.rs}`, `execution-core/src/browser_agent/browser_risk.rs`, `execution-core/tests/browser_agent_e2e.rs`. A hunk-disjoint series on these files cannot conflict with `model-plane` as of today.
- The command asked for in the order, `git diff --stat model-plane...quarryv2-next -- 'apps/Model Plane/rust/services/execution-core'`, prints nothing: `quarryv2-next` has no execution-core change since the merge base. The reverse (`quarryv2-next...model-plane`) lists 23 files, 34914 insertions: `runtime_loop/agent.rs` (16215 lines changed), `runtime_loop/{closers,effect,fold,mod,park,resume,session_core_rules_double}.rs`, `agent_route.rs`, `auth.rs`, `grpc.rs`, `knowledge_tools.rs`, `browser_events.rs`, `scrub.rs`, `workspace_promote.rs` and new tests. WS3.J must not touch these; `browser.act`'s closed list lives in `runtime_loop/agent.rs` (plan: 3479-3508) and stays out.
- Changed on BOTH branches (will conflict at integration, from `comm` of the two name lists): `apps/Model Plane/rust/services/model-gateway/src/{http_routes.rs, browser_run.rs, lib.rs, state.rs, session_flow.rs, zdr_precheck.rs}`, `apps/Model Plane/deploy/docker-compose.yml`, `apps/Model Plane/proto/baseline.binpb`. WS3.J touches `http_routes.rs` (rule text const).
- Not in the committed overlap list but conflicting in the working tree: the uncommitted WS5.G hunks in `model-gateway/src/quarry.rs` and `grpc.rs`. `model-plane` changed `quarry.rs` (+534/-12) and `grpc.rs` (+78/-16) against the merge base, with hunks in base-line ranges 408-526 and 949-1101 of `quarry.rs` that interleave with WS5.G's working-tree hunks (462-528, 972-1068). Expect textual conflicts at merge.
- `apps/Model Plane/docs/decisions/ledger.md` (+239 on `model-plane`) overlaps the WS1.A1 ledger entry.

Owned files: `execution-core/src/{browser_agent.rs, quarry_agent.rs, llm_planner.rs, tool_bridge/mod.rs, browser_agent/browser_risk.rs}`, `execution-core/tests/browser_agent_e2e.rs`, `model-gateway/src/http_routes.rs` (rule text only), `apps/Frontend Plane/verevonv3/tests/e2e/browser-workspace-blocked.spec.ts` (new).

Tests: `cd "apps/Model Plane/rust" && cargo test -p execution-core --lib quarry_agent llm_planner browser_agent` through qtest with `QROOT="C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b/apps/Model Plane/rust" QTARGET=mp-target`; on Windows use `--lib` (two cargo targets fail natively). Evidence: rebuilt quarry-edge, model-gateway/execution-core, gateway, SPA on a second dev server; drive an agent run into a recorded challenge fixture (the WS3.H scrape fixtures from the R9 run are the starting point); SPA shows the notice; the planner's next action is one wait or reload then `done` with a blocked report; third attempt is `CHALLENGE_DETECTED`.

Opus-grade review: yes. It is a cross-plane wire contract (additive field with serde default, status mapping that decides whether a refusal is a typed `Blocked` observation or a hard error), a planner-prompt policy statement (no evasion), and a series of hunks on a branch with an active session. Order: LAST in W1; commit as a separate hunk-disjoint series.

Risks: the sibling (b) session can edit the same files at any time (they are unchanged on `model-plane` today); `http_routes.rs` conflicts at integration.

Effort ESTIMATE: 4 d plan for the Model Plane half (core 2.5, tests 0.75, e2e spec 0.5, evidence 1 = 4.75); +2 d if the typed-error mirrors are taken (open question 6).

### g. WS4.B and WS4.C evidence on a side-by-side stack

Corrected scope: no new code (scripts and handler exist). Side-by-side stack with its own Redis (no `GATEWAY_CACHE_REDIS_URL` of the live stack) and, for WS4.C, a scratch NATS with JetStream.
- WS4.B: seed legacy-shape keys (`verevon:gw:search-web:*`, `verevon:gw:scrape-preview:*`, hand-seeded or from an old-image run) next to org-scoped `:o:` keys; step 0 table (env x cache x revision) in `docs/runbooks/gateway-search-cache-tenant-incident.md`; `gateway-cache-forensics.sh` before; deploy the rebuilt gateway; `gateway-cache-purge.sh` dry run then `--apply`; forensics after: legacy 0, org-scoped unchanged; written answer to open question 3 (from deployment history, not from the cache). The script self-test already passes (commit message, not re-run). The same operation on the real gateways is an operator action (OWNER).
- WS4.C: publish `verevon.gdpr.erasure.requested` through NATS to a gateway started with `VEREVON_GATEWAY_GDPR_NATS_URL/USER/PASSWORD`; the consumer binds a PROVISIONED durable on stream `AQENCIA_CONTROLPLANE` with `get_consumer_from_stream` and refuses a different filter subject (gdpr_nats.rs:24-52, 143-150), so the scratch NATS needs the stream and durable created (the repo has `cmd/nats-consumer-migrate`; I did not read it). Org A keys gone, org B intact. State plainly that the broker is a scratch NATS, not the shared `control-shared-nats`.

Owned files: `apps/Frontend Plane/verevonv3/{scripts/gateway-cache-forensics.sh, scripts/gateway-cache-purge.sh, apps/gateway/src/{cache.rs, gdpr_nats.rs}}` (edit only if the run finds a defect), `docs/runbooks/gateway-search-cache-tenant-incident.md`, `docs/compliance/evidence/retention-and-erasure-evidence.md` (W2 row only if the text changes).

Hot files: none beyond RUNBOOKS (append).

Opus-grade review: the evidence claims about purge safety (never FLUSH, `:o:` keys survive, glob-character org id purges nothing). Code is already unit-tested.

Effort ESTIMATE: 1.5 d (0.75 each) on the evidence stack.

### h. WS4.H stage B, WS4.I, WS4.J (to the counsel gate)

WS4.H corrected scope:
- Built already: flag `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (config.rs:368, default false), `zdr_refusal.rs`, observe and enforce modes for schedules, change check, snapshot, refresh, profiles and the internal change record, `details.zdr_refusal` on crawl, batch, persistent profile and resume, the Go activity `CheckChange` treating a 403 `zdr_durable_mutation_forbidden` as a clean terminal outcome (commits 03a15eb7 to c80a2ff8; 8 tests in `zdr_refusal_modes.rs`).
- Remaining build: (1) the orchestrator sends no `zdr` on `/v1/internal/change/record` (`changeRecordRequest` has `org_id, url, fresh_fingerprint, run_id`). Two ways out (ledger V4 row): stamp the posture at schedule create (a `zdr` column in quarry-control plus the edge's create forward; next free migration number is NOT 015 to 018 if those are reserved by R-2 for WS4.J and WS15, and 019 and 020 are used; 023 is unallocated in R-2 [UNVERIFIED, allocate in the ledger]) or look the posture up at fire time through the narrow `org:retention:read:any` GET (X-9, plus a Control Plane registry row for the orchestrator). X-9 is not in the relayed approval list, so the stamp-at-create option is the one that fits the approved defaults; its known gap is an org that turns ZDR on later (documented, pinned by a test). (2) A mutation check of the review-round assertions (the ledger says they were not seen to fire against a broken handler). (3) Flag-on test through the orchestrator path.
- Not buildable now (OWNER): inverting the `zdr_gap_rows.rs` assertions and flipping the matrix rows to `Enforced` is only true once the default is on; the default flip needs ADR-4 sign-off (relayed as "observe then enforce behind a dark flag") and the customer notice.
- Evidence: a ZDR-org `POST /v1/schedules` returns 403 with `error.details.zdr_refusal.code` with the flag on, `GET /v1/schedules` for that org stays empty, a standard org still creates one. A real ZDR-org token needs a posture flip on a test org; the 2026-10-16 note says that "would change live org metadata". Use a side-by-side test issuer (RS256 JWKS served to the edge, `zdr:true` claim) and say so in the record.

WS4.I corrected scope (1.5 d plan): vendor `zdr-refusals.json` byte-identically from `git show model-plane:"apps/Model Plane/api/contracts/zdr-refusals.json"` into `quarry-runtime/tests/fixtures/zdr-refusals/` with its sha256; make `quarry-edge/tests/zdr_feature_matrix.rs` check the closed lists against it; add the VENDORED and CONSUMERS rows to `scripts/tests/zdr-contract-sync.test.sh`. The file has none of the four new feature keys (grep: 0 hits for `durable_crawl`, `browser_profile`, `session_resume`, `page_screenshot`), so X-16 ("existing codes only") means these rows use existing codes; the new keys are a request to the Model Plane owner. The gateway and SPA mapping needs `zdr-refusals.ts`, which exists only on `model-plane`: OTHER-PLANE.

WS4.J corrected scope (5 d plan, build to the counsel gate): migration `015_zdr_run_tombstones.sql`, store, `zdr_tombstone.go` handler, own janitor, org-erasure inclusion, edge `zdr_tombstone.rs` sink (fail-open), allowlist schema `zdr-tombstone.schema.json` with digest and vendored copy, flag `QUARRY_EDGE__ZDR_TOMBSTONE` default off. The "QB-T17 state.rs commit" ordering is moot: `git status` shows no uncommitted hunk in `state.rs` or `tests/support/mod.rs`. Counsel review gates enabling, not building; the evidence (two tombstone rows, `pg_dump | grep -c <canary>` 0, retention-0 sweep) can run with the flag on in a side-by-side stack, and the record must say counsel has not reviewed it.

Owned files: `quarry-edge/src/{zdr_refusal.rs, schedule_routes.rs, change_routes.rs, profile_routes.rs, config.rs (append), zdr_tombstone.rs (new)}`, `quarry-edge/tests/{zdr_refusal_modes.rs, zdr_gap_rows.rs, zdr_feature_matrix.rs, zdr_tombstone.rs (new)}`, `quarry-core/src/zdr_matrix.rs` and the golden and `docs/ZDR_MATRIX.md`, `quarry-control/internal/{store/pg/migrations/015_zdr_run_tombstones.sql, store/store.go, store/resources_v2*.go, store/pg/resources_v2.go, store/gdpr_purge.go, resources/zdr_tombstone.go, janitor/tombstones.go}` and `cmd/control/main.go`, `quarry-orchestrator/internal/{activities,workflows}` for the posture signal, `quarry-runtime/contracts/zdr-tombstone.schema.json`, `scripts/tests/zdr-contract-sync.test.sh` rows.

Hot files: `quarry-edge/src/{routes.rs, state.rs, lib.rs}` (WS4.J emits at scrape end and adds `AppState.tombstones`; edit after lane A's `main.rs` hunks), `scripts/tests/zdr-contract-sync.test.sh` (lane A pins the token-request digest in the same file), `docs/ERROR_REASONS.md` and RUNBOOKS (append).

Opus-grade review: yes for WS4.H's posture signal and its flag-on behaviour (ZDR enforcement: a wrong default leaves durable state for a ZDR org or breaks a customer), and yes for all of WS4.J (a new egress path under ZDR: allowlist, `://` rejection, immutability trigger, fail-open emission, retention janitor that must not be armed without a provenance guard, org erasure, schema-pinned contract). WS4.I: normal review, careful with digest pins.

Effort ESTIMATE: WS4.H 2.5 d (posture signal 1.5, mutation and flag-on tests 0.5, evidence 0.5); WS4.I 0.75 d; WS4.J 4 d to the gate plus 0.5 d evidence.

### i. WS5.G: the uncommitted hunks

Read the hunks (git diff, `-U1`). They are WS5.G's, with these exceptions noted:
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs` (+159): `scrape_preview` sets `formats: ["markdown"]` when the caller sent none, before the variant digest; `classify_scrape_failure` maps 415 to `unsupported_media` (Norwegian text inline); tests `preview_makes_one_quarry_call_when_markdown_is_inline`, `preview_keeps_the_formats_the_caller_sent`, `a_415_is_classified_as_unsupported_media`, `preview_of_an_unsupported_media_type_is_a_typed_415`. The WS3.J-era `classify_scrape_failure` edits are already committed; this 415 hunk is WS5.G.
- `.../gateway/src/domains/search.rs` (+46/-): `maxPages` removed from both payloads, excerpt from inline markdown.
- `.../verevonv3/src/shared/api/knowledge-client.ts`: `renderHints` and `cachePolicy` removed from `ScrapeRequest`, `formats`, `only_main_content`, `include_tags`, `exclude_tags` added, `rawHtml` renamed `raw_html` in `ScrapeResult`. Open finding: `src/features/dashboard/home/knowledge-preview.ts:150-152` still reads `rawHtml` and `data.rawHtml`, not `raw_html` (grep). Decide which key the gateway emits and fix the reader or the type.
- `.../src/shared/i18n/errors.ts`: one `unsupported_media` copy line (nb and en).
- `apps/Model Plane/rust/services/model-gateway/src/grpc.rs` (+29): `UNSUPPORTED_MEDIA` maps to `failed_precondition` with the code in the message, test added.
- `apps/Model Plane/rust/services/model-gateway/src/quarry.rs` (+408/-45): inline markdown skips the artifact call, ZDR inline never escalates to the browser, an edge without inline support falls back, a typed 415 does not escalate.
- NOT WS5.G's: `apps/Model Plane/go/go.work.sum` (+1 line `go.uber.org/automaxprocs v1.6.0/go.mod`), a side effect of a `go` command. Do not stage it with WS5.G; leave it or commit it alone if a build needs it (open question 7).
- The three gateway and SPA files have CRLF-versus-LF warnings from git (`LF will be replaced by CRLF`): commit through `qcommit.sh` and check `git diff --stat` shows only the hunks above, not whole-file rewrites.

Finish: run gateway `cargo test` natively (the whole binary, plus `--all-targets` per CLAUDE.md), `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (clippy was recorded red in 17 unrelated places earlier: compare against HEAD, do not fix unrelated code); model-gateway `cargo test -p model-gateway --lib quarry grpc` via qtest with `QROOT` of the Model Plane workspace; SPA in a scratch copy: `pnpm install`, `pnpm typecheck`, `pnpm lint`, `pnpm test` (pre-existing failures: diff against HEAD; worktree `pnpm` prunes `pnpm-lock.yaml`: restore it). Commit as WS5.G in two or three commits (Model Plane files, gateway files, SPA files) so each is reviewable and revertible; the commit message says it carries the hunks of the in-flight session.

Evidence: one Quarry call per agent read on the model-gateway path (count requests at the edge), ZDR `fetch_url` text, search-bar excerpt, PNG preview message through the gateway, on the final stack.

Owned files: the seven listed above (minus `go.work.sum`), `knowledge-preview.ts` if the key decision needs it.

Hot files: model-gateway `quarry.rs` and `grpc.rs` (conflict with `model-plane`, section 6), gateway `domains/knowledge/quarry.rs` (WS6.A's `start_crawl` is in the same file, already committed), `errors.ts` (shared with lane E only if new keys appear).

Opus-grade review: the cache-key effect of forcing `formats` before the variant digest (gateway cache key and tenant boundary, WS4.A) and the 415-to-`failed_precondition` mapping (retry semantics across planes). Rest normal.

Effort ESTIMATE: 1.5 d to finish and commit, 0.5 d evidence.

### j. WS5.C, WS5.D, WS5.E, WS5.F evidence leftovers

- WS5.C: already verified (correction 15). Left: GitHub Linux CI (CI) and the Windows in-process fallback (untested, not W1).
- WS5.D: `Sunset` header on an image (needs the sunset date configured in the evidence env), `Deprecation` already seen; Redis page-cache count for case (3) and the resolvable formats ref for (4) not recorded; docs-contract gate is BASELINE.
- WS5.E: `only_main_content:false` keeps nav text on an image.
- WS5.F: second-org 404 and ZDR-reader 403 on an image; S3 backend not run (the plan accepts the filesystem backend).
All are probes on the final stack; no code. Plus the residual build items from section 3 (WS5.A unwrap audit, WS5.B `/v1/extract` decision, `zdr_claim_floor` PDF test, WS5.H openapi completion).

Owned files: none beyond evidence notes; the section 3 items own `docs/openapi.yaml` (ScrapeRequest region), `quarry-edge/src/extract_routes.rs`, `quarry-edge/tests/zdr_claim_floor.rs`.

Effort ESTIMATE: 1 d evidence plus 1.5 d for the section 3 build items.

### k. WS6.A and WS2.E

- WS6.A: add the crawl-bound text to `apps/Ingestion Plane/Quarry-v2/docs/CONTRACTS.md` (defaults 1000 pages and depth 5, hard 5000 and 10, `limits`, `0` means default; grep found none); run and record `cd services/quarry-orchestrator && go test ./...` (and `pkg/quarrycontracts`) natively; evidence: orchestrator and edge on the stack with `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7`, a public site with more than 7 internal links through the gateway: `limits.max_pages.effective == 7`, `clamped_by == ["default"]`, events `pages_visited == 7`, control `params.max_pages == 7`; pre-fix capture on an old image; replace or supplement the `OnGetVersion` mock replay with a recorded history if the SDK allows ([UNVERIFIED]). The 7-page crawl needs outbound HTTPS to a real public site (example.com has too few links: pick a documented site, record it).
- WS2.E: in a scratch copy of the SPA: `pnpm install`, `pnpm build` (no `FleetRunConsole` chunk), `pnpm typecheck` and `pnpm test`; add the route test only if an App route harness exists (the plan makes it conditional; `src/app` holds App.tsx, providers, routes, shell: unresolved); SPA container evidence: restart the container (Vite needs a restart over the bind mount), open `/agents/fleets/abc`, not-found page. Known baselines: vitest 3566 passed and 2 files that fail to load, `tsc -b` 33 baseline errors, eslint 0 errors (census, B-T1 row), so compare, do not expect green.

Owned files: `docs/CONTRACTS.md` (crawl section), orchestrator Go tests (read-only unless a gap appears), `apps/Frontend Plane/verevonv3/src/app/App.tsx` (read-only unless a test is added).

Opus-grade review: none.

Effort ESTIMATE: WS6.A 0.75 d build + 0.5 d evidence; WS2.E 0.25 d + 0.25 d.

### l. The W1 exit gate

Corrected scope (plan 4.3 and 6.2, SC1): after all lanes have committed:
1. PRE-1 (the baseline commit) is outside the branch; PRE-2 is done (438b6e83).
2. Freeze `HEAD`, rebuild every image the gate uses from that revision with `SOURCE_REVISION` and `BUILD_DATE`: quarry-edge, browser-broker, model-gateway, auth-core (new registry), the Verevon gateway (`swap_gateway_image.py` recipe), quarry-control, quarry-orchestrator, and the SPA container. Tag them like the 2026-10-16 run (`qv2v/<service>:<rev>`) and never retag live names. All docker through `with-lock.sh`; no cargo build in parallel with a running stack (Docker Desktop has crashed under parallel containers).
3. Evidence stack E1 (dev flags, own Postgres, Dragonfly or Redis, NATS, Temporal, test issuer): the probes of items e, g, h, j and the per-slice evidence of section 3 (takeover with `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=1`, caps, sweeper, challenge `enforce`, `DOC_ROUTING=on`, `QUARRY_BROWSER_REAPER=1`, budget and crawl envs). Record the flag values in each row.
4. Bring E1 down; bring up the production-overlay stack: `docker-compose.yml` plus `docker-compose.production.yml` plus `docker-compose.grant-contract.yml`, `ENVIRONMENT=production`, `BROWSER_GRANT_ENFORCEMENT=strict` on the gateway, dev bypass flags `0`. Wrapper preflight (auth-core registry serves the new row), then legs A (green), B and C (green after item b), the log scan, `docker exec quarry-edge sh -c 'ps -eo args | grep -c -- --no-sandbox'` printing 0, a `navigate` step 200 and a non-granted host refused, `docker ps` showing quarry-edge healthy in production mode, image ids and `SOURCE_REVISION`. Test identity: the seeded dev accounts `local@verevon.dev` (`tests/e2e/local-auth.setup.ts`) or `e2e@verevon.dev` (`auth.setup.ts`).
5. Fill the SC1 to SC7 table (baseline section 6) with PASS, FAIL or NOT-RUN and one evidence line each.

Hot files: none (no code). The plan ledger is written by one agent (section 5).

Opus-grade review: yes, one final review of the evidence claims and the rule "no claim without the command output", because this is the document that says W1 is closed.

Risks: the auth-core unknown (item a); `example.com` availability; Docker wedge (`docker-desktop-recover.ps1` recipe is in project memory; do not retry in a loop); seccomp profile on Docker Desktop; the evidence is on side-by-side images, never the live stack, which the plan's status line already says (the live stack still runs the 2026-10-01 edge image).

Effort ESTIMATE: 2 d for the gate on top of the lanes (rebuild 0.5, E1 set-up 0.5, production overlay run and write-up 1), excluding the L2 build work counted in item a.

## 3. Items the parent's list left out (add to the lanes)

| Id | Slice | What | Lane | ESTIMATE |
|---|---|---|---|---|
| m1 | WS2.A | `tests/agent_evaluate_denied.rs` and the `insert_live_run` helper (`test_support.rs`); WebSocket and `/interact` evaluate tests; zero receipts, zero budget, zero driver evaluate calls; evaluate re-check on the HEAD image | B | 1.0 |
| m2 | WS2.C | tabs through `admit_action` (or an explicit documented exception to the plan's "every action surface": admission.rs:430, tabs.rs:125, 254, 320); `taken_over_at` in the 409 details; service-principal give-back test; migration idempotency test | B | 2.25 |
| m3 | WS2.D | `initiatedBy system` on the lifecycle `expired` item (lifecycle.rs:624); drain path `Arc::try_unwrap` (lifecycle.rs:708) | B | 1.0 |
| m4 | WS2.B | clippy and `cargo doc` (image has no clippy: add it to `quarry-rust-env` or run elsewhere), size target for lifecycle.rs (747) and types.rs (676) or a recorded exception | B | 0.5 |
| n1 | WS3.D | 13 unwrapped CDP call sites (census scan: `update_tab_metadata`, `install_network_guard`, `hydrate_cookies`, `hydrate_storage`, `install_popup_bridge`, `capture_live_frame`, `list_tabs`, `root_frame_id` via `download`), typed Timeout for the screencast frame, table exceptions named, post-504 step check | B | 2.0 |
| n2 | WS3.A, B, C, E, H | evidence rows of baseline section 3 (navigate proof is item l; Linux host; seccomp decision; mutation run; ledger X-15 line; webdriver strings; challenge pages) | B | 1.5 |
| n3 | WS2.C, D, WS3.F | stack evidence rows (psql columns, timeline and receipts after give back, idle expiry, SSE, resume, slot release, boot line) | B | 2.0 |
| o1 | WS4.A | gateway clippy triage against HEAD, `/metrics` read, four deploy probes (needs a search backend and a posture-flippable org) | C | 1.25 |
| o2 | WS4.D, E, G | image probes (ZDR search with a dead provider, stream, agent start, change/latest span), live ZDR-token check, owner for `/v1/search/suggest` | C | 1.25 |
| p1 | WS5.A, B, H | unwrap audit; `/v1/extract` routing or recorded deferral; `zdr_claim_floor` PDF test; openapi `ScrapeRequest` completion and `x-quarry-lenient-until`; crawl PDF+PNG probe and 20 MB timing | D | 1.5 |
| q1 | WS1.A1 | Model Plane ledger entry (D1, D2, D5, D6, R-6); decide the per-call broker log line (add a no-domain line in `server.go`/`cmd/main.go`, or amend the plan text); ZDR log assertion; base compose validator line | A | 0.75 |

## 4. Lanes (at most 5, disjoint file sets) and totals

The lane file sets below are disjoint at file level except for the named hot files, which are handled by order and by the "small exact-anchor edit" rule. "Docker work" always queues behind the global lock.

### Lane A: GRANTS AND AUTH (critical path, Opus review)
Items: a (WS1.A2), b (without the execution-core test), c, q1.
Files: everything listed under items a, b, c and q1: `quarry-runtime/src/{service_tokens.rs, token_request_contract.rs, grant_validator.rs, grpc/browser_broker.rs}`, `contracts/cross-plane-token-requests.json`, `quarry-edge/src/main.rs` (validator and boot-gate hunks), `quarry-edge/tests/browser_grants_prod.rs`, Control Plane registry and auth-core specs and fixtures, the Ingestion and Model Plane compose files named above, the broker Go tree and `ledger.md`, `scripts/tests/{browser-grant-contract.test.mjs, browser-grant-prod-contract.sh}`, the L2 spec, `.github/workflows/browser-grant-contract.yml`.
Order inside the lane: (1) trait `validate_for_org` with a default body and the five agent-route call sites (admission.rs:38, lifecycle.rs:218, sensitive_approval.rs:156; tabs.rs and admission.rs:371, 396 follow through `validate_run_grant`) committed FIRST so lane B can start on `agent_routes`; (2) registry and manifest rows with digests; (3) G1, G6, boot gates; (4) `prod_*` tests; (5) overlay, wrapper, workflow; (6) b and c leftovers and q1; (7) the L2 run is part of item l.
ESTIMATE: 6 + 1.75 + 1.0 + 0.75 = 9.5 d build (+0.75 d if the optional grant-too-short hardening of item b is taken).

### Lane B: RUN CONTROL AND DRIVER (Quarry Rust, Opus review for concurrency)
Items: m1, m2, m3, m4, n1, n2, n3, e (the small test and the evidence).
Files: `quarry-edge/src/agent_routes/*` (after lane A's call-site commit), `quarry-edge/src/test_support.rs` (append `insert_live_run`), `quarry-edge/tests/{agent_run_control.rs, agent_evaluate_denied.rs (new), run_sweeper_integration.rs, postgres_step_receipts.rs}`, `quarry-browser/src/{chromiumoxide.rs, deadlines.rs, slots.rs}`, `docs/openapi.yaml` (takeover and evaluate regions).
ESTIMATE: 1.0 + 2.25 + 1.0 + 0.5 + 2.0 + 0.5 (e test) = 7.25 d build; evidence n2, n3 and e's 1.5 are 5.0 d on the evidence stack.

### Lane C: ZDR AND TENANT (Opus review)
Items: g, h (WS4.H, WS4.I, WS4.J), o1, o2.
Files: listed under items g and h, plus gateway `src/{cache.rs, gdpr_nats.rs, cache_admission.rs, observability.rs}` for fixes only.
ESTIMATE build: WS4.H 2.5 (of which 1.5 build) + WS4.I 0.75 + WS4.J 4 = about 6.25 d; evidence g 1.5 + o1 1.25 + o2 1.25 + WS4.H 0.5 + WS4.J 0.5 = 5.0 d.

### Lane D: SCRAPE, FETCH PATH AND CONSUMERS
Items: d (WS3.I), i (WS5.G), j, k (WS6.A, WS2.E), p1.
Files: listed under items d, i, j, k and p1: `quarry-runtime/src/{escalation.rs, fetch.rs, fallback_driver.rs, egress_broker.rs, driver_plan.rs, content_route.rs}`, `quarry-control/internal/resources/presets*.go`, model-gateway `src/{quarry.rs, grpc.rs}`, gateway `src/domains/{knowledge/*, search.rs}`, SPA `{knowledge-client.ts, errors.ts, knowledge-preview.ts, App.tsx}`, `quarry-edge/src/extract_routes.rs`, `quarry-edge/tests/zdr_claim_floor.rs`, `docs/{CONTRACTS.md, openapi.yaml (ScrapeRequest region)}`.
ESTIMATE build: 3.0 + 1.5 + 1.5 + 0.75 + 0.25 = 7.0 d; evidence 0.5 (WS3.I) + 0.5 (WS5.G) + 1.0 (j) + 0.5 (WS6.A) + 0.25 (WS2.E) = 2.75 d.

### Lane E: MODEL PLANE HALF OF WS3.J (LAST, Opus review)
Items: f, plus the `cross_domain_goto` test from b.
Files: `execution-core/src/{browser_agent.rs, quarry_agent.rs, llm_planner.rs, tool_bridge/mod.rs, browser_agent/browser_risk.rs}`, `execution-core/tests/browser_agent_e2e.rs`, `model-gateway/src/http_routes.rs` (rule text), `tests/e2e/browser-workspace-blocked.spec.ts`.
ESTIMATE: 3.75 d build (core 2.5, tests 0.75, e2e spec 0.5) + 0.25 for the cross-domain test + 1.0 evidence = 5.0 d; +2 d if open question 6 says yes.

Totals (ESTIMATE): build 9.5 + 7.25 + 6.25 + 7.0 + 4.0 = 34.0 d; evidence on the stacks 5.0 + 5.0 + 2.75 + 1.0 + the gate 2.0 and lane A's L2 work (about 3.0, item l step 4) = about 18.75 d. Remaining total about 53 d, in line with the 52 d quoted in the baseline (rounding; both are estimates). Wall clock with five lanes: lane A about 9.5 d build, then the L2 and the gate: about 14 working days before queueing on the Docker lock.

## 5. Hot files shared between lanes and the rules

| File | Lanes | Rule |
|---|---|---|
| `quarry-edge/src/main.rs` | A (validator, boot gates), C (WS4.J wiring), B (none expected) | A first; C after A's commit; small exact-anchor edits |
| `quarry-edge/src/agent_routes/{admission,lifecycle,sensitive_approval,tabs}.rs` | A (5 call-site lines, first), B (all later work) | A commits its call sites in its first window; B starts on these files afterwards |
| `quarry-edge/src/test_support.rs` | A (`test_state_with_grants`), B (`insert_live_run`) | append-only, different functions |
| `quarry-edge/src/{routes.rs, state.rs, lib.rs}` | C only (WS4.J), D (read) | C owns; anyone else uses a small exact-anchor edit |
| `quarry-runtime/src/lib.rs` | A (exports if any), D (`pub mod escalation;`) | one line each |
| `quarry-browser/src/{chromiumoxide.rs, lib.rs}` | B only | |
| `scripts/tests/zdr-contract-sync.test.sh` | A (token-request digest), C (WS4.I and WS4.J rows) | append rows, re-run `--self-test` after each |
| `docs/RUNBOOKS.md`, `docs/ERROR_REASONS.md` | all | append-only, one section per slice |
| `docs/openapi.yaml`, `docs/CONTRACTS.md` | B (takeover, evaluate), C (zdr_refusal), D (ScrapeRequest, crawl bounds) | each lane edits only its region |
| `apps/STALE_DOC_DELETION_REGISTER.md` | all | append-only |
| `docs/plans/quarry-browser-build-plan-2026-10-14.md` (ledger rows, errata) | none during lanes | single writer after the gate (the ledger-keeper), because every lane would otherwise edit one 4000-line file |
| `apps/Model Plane/deploy/docker-compose.yml`, `apps/Model Plane/docs/decisions/ledger.md` | A | both also changed on `model-plane`: textual conflicts at integration, keep the hunks tiny and separate commits |
| `model-gateway/src/{quarry.rs, grpc.rs}` (D) vs `http_routes.rs` (E) | D, E | different files, same crate: builds queue on the lock anyway |

Never stage or commit: `.plan-work/`, and `go.work.sum` unless open question 7 says so.

## 6. Execution order and dependencies

Phase 0 (day 0, no build): the owner questions in section 8 are not blockers; every lane starts on its default.
Phase 1, build, in parallel: lanes A, B (agent_routes work after A's first commit, driver items immediately), C, D. Lane E starts when A to D are at about 70 % of their build, because WS3.J goes LAST in W1 (plan 4.3 position 11) and the sibling session may move; commit it as its own hunk-disjoint series.
Phase 2, freeze: all lanes commit; `git status` clean except `.plan-work/`; rebuild images once from that revision (item l step 2).
Phase 3, evidence stack E1: lanes' evidence probes, one agent drives the stack at a time.
Phase 4, exit gate: production overlay plus contract overlay; legs A to C; SC table.
Phase 5, closeout: one writer updates the plan ledger and `STALE_DOC_DELETION_REGISTER.md`, regenerates the baseline table.

Dependencies that matter:
- A2 needs nothing else (A0, A1, WS2.B, WS3.A are on the branch).
- b's legs B and C, c's `/step` leg, WS3.A's `navigate` proof: need A2 and the gate stack.
- Evidence rows: need the Phase 2 images.
- WS3.J (lane E) last; the exit gate after everything.
- WS4.H stage B default flip, WS4.J enable, X-2 rollout, and every CI row: outside the branch.

## 7. Evidence rules that apply to every lane

- Rust in the Quarry workspace only through `qtest.sh`; the Model Plane workspace with `QROOT` and `QTARGET=mp-target`; Go and the gateway natively; Node tests natively. Live-Chromium tests with `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium`.
- `DOCKER IS NOT ANSWERING` or an engine 500: stop and report; do not loop.
- The live stack is read-only: its Redis, its auth-core, its broker, its gateway container are never restarted or changed. The live broker was redeployed on 2026-10-20 from `e7c321d5` on its own database (plan status line); the live edge image is 2f2c3e83 from 2026-10-01.
- Every PR-style evidence record lists: image ids and `SOURCE_REVISION`, `docker ps` health in the intended mode, the transcript with its assertion, a pre-fix capture where a defect is fixed, a log scan for the hop, rollback notes, the stale doc and its register row (plan 6.2).

## 8. Open questions for the user (none blocks the lanes; each has a default)

1. Grant renewal: take the R4 note's recommendation (A fail clearly plus the TTL half of B, about 0.75 d, lane A), or accept that an AI run longer than its grant (session grant 900 s default, max 3600) gets 403 in W1, or pull `RenewGrant` forward from WS8.C. Default: take A plus the TTL half of B only if the owner wants it; otherwise record the risk.
2. Seccomp profile shape (allow-by-default, no mount, unshare, pivot_root, setns denies): accept for W1 or tighten. Default: accept and record.
3. WS4.H posture signal: stamp at create (needs a quarry-control migration and a number from the R-2 ledger) versus a fire-time Control Plane lookup (X-9, not in the relayed approvals). Default: stamp at create, document the later-ZDR-org gap.
4. WS3.I default: ship `legacy` first and flip to `honest` after a measurement (default), or flip in the same commit.
5. L2 on a side-by-side stack needs an auth-core that serves the new `quarry-edge` row. Default: a side-by-side auth-core with its own database and seeded dev accounts. Alternative that touches the live stack: swap the live auth-core (owner decision, not taken here).
6. Do the Model Plane typed-error mirrors and the `Evaluate` variant removal (sibling (b) work) go into this branch's lane E (about 2 d more), or stay with the Model Plane session. Default: stay out.
7. The `go.work.sum` hunk: leave uncommitted (default) or commit it alone.
8. `/v1/extract` (WS5.B): route it through the content sniff in W1 or record an explicit deferral. Default: record the deferral.

## 9. Parent-list items versus lanes

a: lane A. b: lane A (+ test in lane E). c: lane A. d: lane D. e: lane B. f: lane E. g: lane C. h: lane C. i: lane D. j: lane D. k: lane D (WS6.A, WS2.E). l: after all lanes. Additions m, n: lane B; o: lane C; p: lane D; q: lane A.
