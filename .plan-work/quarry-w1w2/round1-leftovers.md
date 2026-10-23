# Round 1 leftovers (generated from the workflow result, 2026-10-18)

Source: workflow wf_4680c7dd (71 agents). HEAD at the end: eb92d4ee. Each unit lists its own "not done"; the completeness critic lists what is undone overall.

## Completeness critic

### undone

- Plan build ledger is stale. docs/plans/quarry-browser-build-plan-2026-10-14.md lines 14-37 got only one new row (WS1.B, 65ce36b2). Nothing records WS2.B-E, WS3.B-H, WS4.B-G, WS5.C-H, the review fixes, or the F7 evidence run at 89c67974. The 'Files that exist only as uncommitted work' table (lines 39-57) also still describes the old baseline.
- docs/PROGRESS.md:216 still says cache_policy has 8 cases that skip without Redis. HEAD has 9 #[tokio::test] cases (grep), and since 0076d850 REQUIRE_CONTRACT=1 turns a missing Redis into a failure. F1 flagged this row to update after F4 committed, and nobody did.
- CI never runs the page-cache and ZDR-cache gates. .github/workflows/quarry-v2-ci.yml has no Redis (grep -ic redis = 0) and does not set REQUIRE_CONTRACT. So cache_policy (9 cases) and the Redis case in zdr_claim_floor skip in CI. F1 proposed a CI change and nobody applied it.
- WS3.F leftovers: (a) docs/openapi.yaml /ready is still text/plain 'ready' (lines 32-45), but WS3.F made it JSON with a browser object. (b) The R-26 boot check (session cap > MAX_CONCURRENT_RUNS + heavy lane) is not wired; main.rs has no session_cap or max_sessions check. (c) The gateway has no mapping for browser_session_cap, browser_page_cap, session_reaped or browser_process_lost (no hits under the verevonv3 gateway src). (d) RUNBOOKS.md:288 still says 'Known gap: a tab attach still holds the driver's browser lock', which WS3.F fixed. (e) main.rs builds two ChromiumoxideDrivers, but /ready reports only agent_driver.
- docs/ERROR_REASONS.md:43 still lists health_probe as a value of the 504 op field. Line 39, corrected in 3f590d6c, says no response body carries it.
- Open src findings that exist only as #[ignore] tests (CI never runs them):
  - F5-1: control timeline items serialise initiated_by, not initiatedBy.
  - F5-2: a resumed agent run is swept with the interactive class.
  - F5-3: a step or tab call queued behind an expiry gets run_not_live without resumable.
  - F5-4 (HIGH): tab select and close take no initiator, so an agent's call on a person's token is classed Human during a take-over. This needs agent_routes/tabs.rs plus gateway browser.rs forwarding.
  - Tab calls also have no in-flight guard.
- WS4.G tests every_row_has_a_log_canary_or_is_marked_log_unverified and every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list are not written (grep finds them only in the plan). The plan line 3779 still names the old test a_service_principal_with_body_zdr_gets_the_same_refusals, which 264832dd split.
- WS1.B leftovers:
  - session-grant renewal (Quarry has no endpoint to swap a live run's grant id)
  - client idempotency key on POST /v1/browser/runs
  - route-level Quarry start_run grant tests with a fake validator
  - delete the legacy GrpcGrantValidator::acquire
  - SPA i18n entry for browser_session_grant_inactive (errors.ts is held by WS5.G)
  - L2 run
  - execution-core test cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant
- WS4.E/F leftovers:
  - Quarry crates/quarry-edge telemetry.rs probably has the registry-level EnvFilter weakness that d8a9708d fixed in the gateway, and no unit took it.
  - The log lint stops scanning quarry-edge/src/main.rs at line 64 (about 46 sites, url fields at 531/592/615 unscanned).
  - Baselined: 15 R2 and 26 R1 in quarry-runtime, 22 R2 and 1 R4 in the gateway, R4 spawns in change_routes.rs and run_sweeper.rs.
  - page_renderer.rs:191 is reachable under ZDR (render_zdr) and is annotated rather than converted.
- WS3.H leftovers:
  - RUNBOOKS has no QUARRY_CHALLENGE_* entry.
  - No openapi parity test for ChallengeKind, ChallengeStrength, BlockReason or ChallengeRecovery.
  - sdks/generate.sh was not run for ChallengeSignal.md (hand-written).
  - reject_if_exhausted still allows Evaluate, MouseWheel, RespondDialog and Back on an exhausted host.
  - QUARRY_CHALLENGE_ENFORCE defaults to enforce, while the plan says log during corpus validation; no decision is recorded.
- F7 evidence findings were recorded but not fixed:
  - FilesystemStore does not override list, so GET /v1/artifacts is always empty on the fs backend.
  - Full-page markdown of a Wikipedia article is 30.9 MB from a 1.05 MB page (table padding).
  - The flate-bomb 415 branch was never observed on the image (it gave 504).
- Smaller code leftovers:
  - invalidate_live_frame_cache/page_closed key-only races in quarry-browser.
  - BrowserDriverAdapter::do_fetch has no drop guard (quarry-runtime).
  - model-gateway zdr_precheck test fails on a CRLF working copy.
  - go.sum stray lines were reverted. Not verified that HEAD go.sum builds under -mod=readonly.
- Owner decisions still pending, not code:
  - delete the 63 legacy *-Aquatiq-Dev SDK pages (register row 469e7c7d)
  - the trailer conflict: 40 of the 98 commits in bed81d16..HEAD carry 'Claude Haiku 5.5', not the requested Sonnet 5.5
  - whether to squash red-alone commit df7feedf before sharing

### unblocked_next

- Plan order, W1 position 10: WS1.C (Postgres grant store, migrations/0001_browser_grants, PgStore Derive with the per-parent run cap of 8). It needs only WS1.B semantics, which are now committed. Its production rollout is still tied to X-2 (default (a): memory-only ZDR grants, no zdr column).
- Position 10 closeout items that need no decision: the WS4.G closed-catalogue and log-canary tests, the WS3.H RUNBOOKS and openapi-parity follow-ups, and the WS3.F openapi /ready shape plus the gateway reason mapping.
- Position 11: the Quarry-owned half of WS3.J (PageRunner/scrape path calls challenge::classify). The Model Plane, gateway and SPA halves are sibling-owned (see blocked).
- Position 11 (optional): WS3.G at K>1 local slots only (LocalProcess transport). ADR-10 gates only remote substrate and the isolation claim.
- W2 head, position 12: WS10.A request-id codemod. It has no inbound dependency, but the plan wants a one-day freeze, and WS5.G's uncommitted hunks in the frontend gateway knowledge/quarry.rs and search.rs should land first.
- W2 position 17: WS6.C usage metering. Its WS2.D prerequisite is committed. It still needs Lago billing lines per metric (ledger, lines 35-37), so only the Quarry emission half is unblocked.
- WS6.E is unblocked on code (it needs WS1.B), but position 20 puts it after WS1.C, so do WS1.C first.
- Test-depth fixes that are ready now:
  - the CI Redis service + REQUIRE_CONTRACT step
  - un-ignoring F5-1/F5-2/F5-3 with their src fixes in agent_routes (types.rs, lifecycle.rs:248/386, admission.rs:350/431)
  - F5-4 in tabs.rs, plus the gateway forwarding in browser.rs

### blocked

- WS1.A2 (registry row, Quarry service token for ValidateGrant) is blocked by decision X-1 (Control Plane sign-off to register quarry-edge as a browser-broker principal). Because of it, the WS1.B Quarry grant checks have no production effect and the WS1.B L2 legs B/C cannot go green.
- WS5.G is in flight in a sibling session. Its uncommitted hunks sit in model-gateway grpc.rs and quarry.rs, in the frontend gateway knowledge/quarry.rs and search.rs, and in knowledge-client.ts and errors.ts. Those paths are not ours to commit, and they block the SPA i18n entry for browser_session_grant_inactive.
- WS3.J (Model Plane execution-core browser_agent.rs, quarry_agent.rs, llm_planner.rs; gateway and SPA consumers) is owned by sibling session (b).
- WS3.I default flip is blocked by ADR-5. WS4.H is blocked by ADR-4. WS4.I is blocked by X-16 (Model Plane Z-series refusal catalogue commit). WS4.J is blocked by ADR-4, counsel review and the QB-T17 commit. WS3.G beyond local K>1 is blocked by ADR-10. WS2.F is optional and tied to the WS11 parse_typed_error window.
- PRE-1 (baseline commit of the live tree's uncommitted work) is blocked by decision X-3. The W1 exit gate needs it, and so does every W2 hot-file slice.
- W1 exit evidence (strict BROWSER_GRANT_ENFORCEMENT on the production overlay, L2 run, SC1) is blocked by X-1/WS1.A2. WS6.D is blocked by the billing-core credential and ADR-1. The full scope of WS6.C is blocked by the missing Lago billing lines.
- WS1.B's change_record_internal gap test needs a postgres-queue harness with a real Postgres-backed PostgresBaselineStore before the baseline write can be observed. This is infrastructure, not a decision.

### unconfirmed_claims

- Known false or inaccurate commit-message claims, not amended (amend is forbidden):
  - 5f90ae3a: says the cleaning-cost section is 'in the next commit'. It is in 37b7ebb0.
  - c2b3d2e9: says '8 added, 0 removed'. git show gives 11/3, and the commit carries P1-ws3f lines.
  - 37b7ebb0: does not mention the WS5.E section it carries.
  - 2046644e: says 'every agent action during a takeover gets 409'. F5-4 disproves this.
  - ff1be0f6 and bb951ab5: their citation and sync claims were corrected in 264832dd and 41b3a9cf.
  - 4e82894b: says 'replay of recorded HTML'. The fixtures are hand-written.
  - 97186586: names a non-existent sdks/.gitignore.
  - 469e7c7d: describes its git grep check wrongly.
  - a9a71f4e: says the basis line changed in the .sha256 file. It was the digest.
  - 6da7ffd8: says 'a retried start cannot mint a second grant'. This was corrected in 33a362f6.
  - 67cff1f3: baseline arithmetic (corrected in 89c67974 to 44/18).
  - d8a9708d: says bin '709 passed'. That came from the shared tree; the committed tree gives 704 per the reviewer.
  - d26c61a5: says '694 passed'. The reviewer reproduced 688+1 failed in a scratch layout.
  - 0997b7c8: says '14 tests'. The file has 15.
- I did not re-run any test, as a read-only critic. Every pass count in the unit reports is unconfirmed by me, including:
  - INTEGRATE quarry-edge 848, runtime 713, browser live 122/4 ignored, Postgres 21, gateway 712, model-gateway 1716/1 failed
  - F5 13/16
  - P1 121 x6
  - F3 zdr suites
  - F4 cache_policy 9/9 with Redis
- F7 evidence (8 checks PASS against an image built from 89c67974) cannot be confirmed. The containers, volume and image were removed. Only the RUNBOOKS text (8adf2234) remains.
- The rustfmt-clean claims in 2046644e, f853e1ad, 3219ce1e and 6b9c636a used host rustfmt 1.9.0. The reviewers could not reproduce them, because quarry-rust-env:1 has no rustfmt, and CI has no fmt step.
- Things argued from code but never run:
  - F4 dda2618c: the actions-branch mutation.
  - P2 ecd7097e: the owner-assertion mutation.
  - P3 c1d2e91f: the new negative rows failing on the old code. cargo check --workspace and the quarry-edge lib tests were also not re-run for c1d2e91f.
  - F2: the tracing-log bridged events being capped.
- Commit trailers do not match the task text: in bed81d16..HEAD, 40 commits carry 'Claude Haiku 5.5' and 58 carry 'Claude Sonnet 5.5'.

## Per-unit not_done

### F1-docs (done, commits b807f572 440579d6 f7a94512 cee56eb5 aec85098 12ab61d3 38c65716 3f590d6c b964fe92 f292d5bb 97186586 469e7c7d)

- 63 tracked pages named *-Aquatiq-Dev.md (added by snapshot commit 7c75b3c3, referenced by nothing, not written by the generator) are NOT deleted: a deletion needs the owner's decision; the register row 469e7c7d records it as delete (pending decision).
- The python SDK was not regenerated into the tree: its output is git-ignored and CI does not commit it.
- The SPA was not type-checked as a whole: its dependencies are not installed in this worktree. Only the names it imports from @quarry/client were checked against the built SDK.
- No CI run (verevonv3-ci or quarry-v2-ci): nothing was pushed.
- The 429 rows for org_backlog_full and org_render_cap in ERROR_REASONS were checked only as reason strings in code (handoff.rs, lease_pool.rs), not against a live refusal.
- The WS3.F sections that another unit appended to ERROR_REASONS.md and RUNBOOKS.md were committed by that unit (37b7ebb0); this unit checked only the health_probe wording against them, not the rest.
- docs/SOVEREIGN_SCRAPING_RELIABILITY.md had an uncommitted diff of another unit when this unit started; it was not inspected or committed.
- The ignored local sdks/typescript/src and dist from the first, working-tree-spec run were left in place (git-ignored; generate.sh rewrites them).
- Trailer decision: the attribution reminder asks for Claude Haiku 5.5 on commits and the workflow text asked for Sonnet 5.5; the commits carry Claude Haiku 5.5 (this agent's model).

### F2-gateway-zdr-log (done, commits 2042c391 63b64619)

- 22 R2 sites and 1 R4 site stay in the baseline instead of being rewritten to error_class: cache.rs 6, gdpr_nats.rs 6, mail_task.rs 3, crawl_preview/handler.rs 3 R2 plus 1 R4, and browser.rs, mcp.rs, remote.rs and rate_limit.rs 1 each. They are infrastructure or onboarding errors, and the filter already drops them for a ZDR caller.
- The tests in the working tree need P2's uncommitted config.rs change (AppState.browser_grant_mode). The `browser_grant_mode:` test-state lines in main.rs, chat/shared.rs, orchestration.rs and stream_e2e.rs are not part of my commits and stay with P2-ws1b. Do not rely on HEAD alone compiling those test fixtures once config.rs is committed.
- Background tasks that do not inherit the request span are not covered by the filter. Only the lint's R4 check covers request-path spawns, and the gateway has no OTLP exporter.

### F3-matrix-gap-tests (done, commits 80078994 ff1be0f6 bb951ab5)

- The plan's WS4.G test list also names every_row_has_a_log_canary_or_is_marked_log_unverified and every_code_and_feature_the_matrix_names_is_in_the_closed_catalogue_list. Neither exists. Both belong in tests/zdr_feature_matrix.rs, which is outside this unit's owned paths, so I did not write them.
- change_record_internal, change_history and change_snapshot pin what this build answers: 501 with no baseline store and no postgres-queue. The stored baseline and the published snapshot cannot be observed without a postgres-queue harness, and those tests' comments say so.
- Schedule trigger, backfill, pause and unpause, and the per-profile routes (/v1/profiles/{id}, restore_probe), are not exercised. The ZDR-user persistent-profile refusal is covered by zdr_claim_floor.rs. Resume has no test: agent_resume cites none, and I did not add one.
- node --test scripts/tests/zdr-contract-schema.test.mjs, which the plan names in its commands, was not run.
- No separate hand-over .md file was written, to keep to the rule against report files. The progress file carries the state and is the notes_file.

### F4-scrape-leftovers (done, commits baa7a009 5f90ae3a d08e41ae c0aca0d3 d873cca4)

- Firecrawl include/exclude semantics were not compared: no documentation read and no live run. clean_html_selectors.rs says so and lists the open questions (exclusion before or after inclusion, an empty include result, whether structural selectors see the page before or after exclusion).
- No release-profile measurement. The cost figures are one debug (test-profile) run on a shared Windows host; the caveat is in the RUNBOOKS section.
- Commit placement: the 'Scrape cleaning cost' section is in 37b7ebb0 (WS3.F's commit), not in a commit of its own. The message of 5f90ae3a says the section is 'in the next commit', which is wrong. History was not rewritten (amend and rebase are forbidden here). WS3.F's commit message does not mention the extra section.
- The DOCX bomb is stopped by the 32 MiB document.xml cap, so the RLIMIT_AS kill path for DOCX (docx_parse_failed with parser_stopped) is not exercised; the PDF flate-bomb test covers the address-space kill. The bomb was not sent through the full HTTP path on a rebuilt image.
- The ZDR 'never touches Redis' check covers the page-cache keys, the response cache_state and the origin request count. It is not a full Redis command trace.
- The durable-record test restarts a new router and store inside one test process, not a real quarry-edge-rs restart; no rebuilt-container evidence.
- The cache tests use a LoopbackDriver test double, so they do not exercise the production static driver's address guard. That guard has its own tests in quarry-runtime (dns_guard.rs).
- Attribution: the Co-Authored-By line says Claude Haiku 5.5, as the system attribution reminder for this session requires. The unit text asked for Sonnet 5.5.
- Hand-over: the progress file (notes_file) holds the state and traps. No separate report .md was written, per the rule against report files.
- Other units' uncommitted files were not touched or staged: quarry-edge/tests/support/zdr_harness.rs, agent_run_control.rs, run_sweeper_integration.rs, zdr_gap_rows.rs, support/agent_harness.rs.

### F5-agent-run-tests (done, commits 2046644e f853e1ad)

- Sweeper periodic loop (spawn_run_sweeper, the QUARRY_EDGE__RUN_SWEEPER=0 off switch, the interval) is not exercised; the tests call sweep_once with an injected clock.
- The boot refusal of a misspelt QUARRY_EDGE__TAKEOVER_ENFORCEMENT (main.rs) is not run; the parser is tested in agent_run_control.rs.
- The Postgres round trip for the new receipt columns is not in these files; fd5d2a4c covers it (DATABASE_URL-gated).
- Cap numbers and RSS figures are not measured here (WS2.D deployment evidence).
- F5-1, F5-2 and F5-3 stay ignored until the src code agrees (src is out of scope for this unit). Un-ignore the three tests when fixed.
- The tab path of the closed-while-queued race (admission.rs:431-434, admit_tab_operation) is not tested; it has the same gap as F5-3.
- Commit trailer: the commits end with Co-Authored-By: Claude Haiku 5.5 (the session's attribution reminder). The workflow text asked for Sonnet 5.5; confirm which the orchestrator wants.

### P1-ws3f (done, commits 77749d34 b86a32c8 5102b059 37b7ebb0)

- R-26 boot check (session cap must exceed MAX_CONCURRENT_RUNS plus the heavy lane) not wired: it needs main.rs, which the plan says this slice does not touch
- docs/openapi.yaml /ready response shape not updated (outside owned paths)
- Gateway mapping of browser_session_cap, browser_page_cap, session_reaped and browser_process_lost not done (another unit / sibling slice)
- The plan's deployment evidence was not run against a rebuilt edge: 13 runs at MAX_SESSIONS=12, pkill chromium, the reaper with SESSION_IDLE_S=30, a hidden binary for the /ready 503
- K > 1 slots (WS3.G) out of scope
- Commit 37b7ebb0 also carries another unit's complete 'Scrape cleaning cost' section in docs/RUNBOOKS.md (appended to the shared file after mine; qcommit staged the whole file). It could not be split out afterwards without amend

### P2-ws1b (partial, commits 0997b7c8 6da7ffd8 d26c61a5 27801ea1)

- Quarry-side grant checks and their planned tests (user_cannot_use_another_users_grant, zdr_grant_cannot_back_a_non_zdr_run, validated_grant_overrides_client_domains_in_every_mode): need Quarry's own service token for ValidateGrant (WS1.A2 / X-1); start_run also sits in lifecycle.rs, which holds another unit's uncommitted changes
- execution-core browser_risk test cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant: not added (outside owned paths)
- L1 contract test does not yet pin the derive field numbers, and the L1 gateway assertions are not added; no L2 run and nothing deployed (broker, model-gateway, quarry-edge and gateway images not rebuilt)
- A model-gateway test proving a derive failure creates no thread or run was not written; the order is fixed in code (the grant is resolved before prepare_managed_run_authenticated)
- BROWSER_BROKER_MAX_RUN_DOMAINS and BROWSER_GRANT_ENFORCEMENT are not set in compose files; the code defaults apply
- Postgres Derive belongs to WS1.C

### P3-ws3h (done, commits 4e82894b ed45220b)

- No live-lane evidence: I did not run an agent against a Turnstile page, an hCaptcha page or a login page. The tests replay recorded HTML through ContentDriver only.
- sdks/generate.sh was not run because it needs the openapi-generator container. BrowserObservation.md and ChallengeSignal.md are hand-matched to the expected output, and the Python SDK is not regenerated.
- The scrape path (PageRunner) does not call challenge::classify yet. That belongs to WS3.J, along with the Model Plane, gateway and SPA consumers.
- I did not stage or commit projection.rs, types.rs, zdr_matrix.rs, zdr-feature-matrix.json* or the other SDK docs. They hold another unit's uncommitted changes and are still in the tree.

### F6-error-classes (done, commits df7feedf 1db86f58 6b9c636a)

- 23 R2 sites remain in quarry-runtime and quarry-browser, judged boot, background or off the request path and left raw: nats_event_bus, events.rs (spawned publish), usage.rs, publisher.rs, postgres_queue, cached_profile_store, crawl_ranker, page_renderer, the egress proxy accept loop, pipeline's local-index add_document spawn. I did not check each one for ZDR reachability beyond reading the code around it.
- main.rs is not scanned by the lint, because production_part() cuts the file at line 64 where a #[cfg(test)] mod test_support; sits. About 45 boot-time error = %e sites there are unscanned and unchanged. Fixing the lint was outside this unit.
- Only the agent_run row became Log::Canary. agent_run_surface, agent_persistent_profile and agent_resume stay Log::Unverified. No agent step-path code logs a URL, so the canary guards span inheritance and the filter rather than one real log site.
- The CDP error_class vocabulary is only timeout or cdp. It does not separate connection loss from protocol errors.
- Pre-existing and untouched: the dead_code warning for zdr_log::class_reqwest in the edge bin copy, the unused FleetStatus import, and the rustfmt diff in tests/support/zdr_harness.rs.
- The two .md notes files and .plan-work/ are not part of any commit. model-gateway grpc.rs and quarry.rs and the frontend gateway diffs belong to other sessions and were never staged.

### F7-evidence (done, commits 8adf2234)

- The typed 415 pdf_parse_failed branch of the flate bomb was not seen on the image; it gave 504 at the 15 s deadline. The 415 branch is still covered only by doc_parse_sandbox.rs.
- Sunset header not exercised, because it needs the strict-after/sunset date config.
- S3 artifact backend not run (no S3 endpoint); durability was shown on the fs backend only.
- Findings left unfixed and recorded in the notes. FilesystemStore does not override list, so GET /v1/artifacts is always empty on fs. Full-page markdown (only_main_content false) of a Wikipedia article is 30.9 MB from a 1.05 MB page because table lines are padded to over 1M characters. The SSRF guard has no env switch for private hosts. After the navigate timeout in check 6 the edge closed the run, and I did not check whether that is by design.
- SSRF workaround for the bomb server: a private docker bridge with subnet 11.250.0.0/24 that the guard classes as public (no code change). It is not a production path.

### INTEGRATE (done, commits eb92d4ee)

- Not committed: grpc.rs and quarry.rs in model-gateway, plus the Verevon hunks in knowledge/quarry.rs, search.rs, knowledge-client.ts and errors.ts. They belong to WS5.G, which is not finished, and the instructions say never to commit the two model-gateway files. .plan-work/ was also left alone.
- The one model-gateway lib test failure above is a CRLF artifact, not a defect on this branch. I did not change it because neither file has changed since 2d40e11f. A possible fix is for the test to normalise "\r\n" before scanning.
- The qtest image has no google/protobuf include files, so the model-gateway runs needed `apt-get install libprotobuf-dev` in the container. The tests also need the Frontend Plane tree mounted at /Frontend Plane, which I did in the custom runner. This is recorded as a trap in the status file.
- Not run: the quarry-v2-ci.yml workflow edits have never run in CI. The `--ignored` live-Chromium lane (4 plus 5 ignored tests) was not run. Of the Model Plane, only model-gateway --lib was run, with no other Rust crates or Go services, and no cargo check of its workspace.
- The pre-existing warnings were left as they are: the unused FleetStatus import and the three bin-only dead-code warnings.

