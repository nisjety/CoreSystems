# Model Plane verification plan (2026-11-11)

Branch `mp-w345` (tree `C:/dev/mp-w345`), cut from main `031e7965d`. Tip at writing: `68e247254` (batch B18). The code phase is finished. Every new behaviour is dark behind a default-off switch. Only compile-only checks ran after each batch. No test, generator-check, migration, deploy or live proof has been run on this tree.

This file is the order of work for the next phase. It is a plan, not a log. One line per item. A unit id points to its report in `mp-w345-plan/ (programme plan folder) results/<batch>.json` and its spec in `mp-w345-plan/ (programme plan folder) units/<id>.json`. The sources of this plan are `mp-w345-plan/ (programme plan folder) followups.md`, `mp-w345-plan/ (programme plan folder) h/consistency.md`, `mp-w345-plan/ (programme plan folder) coding-complete-draft.md` and `docs/plans/model-plane-open-decisions.md` (sections 1 and 3.6).

Order of work: (1) generated artifacts, (2) compile and test baseline, (3) tests owed, (4) principal and contract parity, (5) migrations, (6) live proofs, (7) consistency fixes. Do not start a later section until the earlier one is green or its failures are classified as pre-existing.

## 0. Ground rules for the whole phase

- Never touch `C:/dev/CoresSystem`. Work only in `C:/dev/mp-w345`, and in scratch folders named below.
- Never read, grep or print `.env*` files. Use `grep -l` or `-c` if you must test for a name. Secrets stay in the live files outside the repo.
- Never run `cargo fmt --all`. Use `rustfmt --check --edition 2021 <file>` on changed files only. Keep each file's line endings (`git ls-files --eol`); new files are LF.
- Write long scripts with the Write tool, then run them. Long heredocs fail in the Bash tool. Python `subprocess.run(["bash", ...])` hits the WSL stub: use Git bash by absolute path.
- Use only the cargo targets `C:/dev/mp-target-w345`, `-gw` and `-other` (and one more, `C:/dev/mp-target-w345-base`, for the base run in 2.2). Check free space on C: after every batch of work. Delete only scratch targets you made.
- If a cargo result contradicts the tree, the warm target is stale: `cargo clean -p <members>` and run again.
- If tests that call `localhost:9091` hang, or `docker version` prints nothing, the Docker engine is wedged. Run `docker-desktop-recover.ps1 -Apply` (twice if the stale socket folder returns). Never `compose up`. The gitleaks commit hook runs in Docker and reports a false secret while the engine is down; never use `--no-verify`.
- Live proofs use the owner's Anthropic test key and model `claude-sonnet-5-5` only. The key lives outside the repos. Never use Verevon routing modes or a subscription route for a proof.
- Stop line for usage is the owner's current weekly line (95%, see the usage note in memory). When committing is allowed, make one commit per generator or per fix, in the Conventional Commit style this repo uses.

## 1. Generated artifacts first

FUGEN is unit `FU-11-generated-artifacts`. It ran in batch B18 (`68e247254`). Its report is in `mp-w345-plan/ (programme plan folder) results/B18.json`.

### 1.1 What FUGEN regenerated (re-check, do not redo)

- Agent-action admission golden (now holds `chat.run.continue` with its refine hash and `thread.share_snapshot`, `thread.revoke_snapshot`, `thread.grant_snapshot`). Pin starts `84af80d0`. Copies: `apps/Model Plane/api/contracts`, capability-core testdata, execution-core and model-gateway fixtures.
- ZDR refusal catalogue (adds `mcp_secret_ref`, `custom_rest`, `artifact_output`, `process_output_spill`, `teach_mode`, `reasoning_translation`). Pin starts `fc18191e`. Copies: Go `pkg/zdrrefusal`, auth-core fixtures, SPA contracts (digest constant set, `awaitingGolden` cleared), Quarry runtime fixtures.
- ZDR posture matrix, 37 rows (authored, not generated). Pin starts `21bb7bf7`. Vendored byte-identical into mp-contracts, model-gateway, execution-core, org-core, auth-core and the verevonv3 gateway fixtures.
- Prompt-tool manifest (`mp-contracts/tests/fixtures/prompt-tool-manifest.json`): digests moved, regenerated with its pin.
- Inference status contract `infer-statuses.json` (adds `model_hidden`, `image_model_unavailable`, `providers_benched`). Pin starts `79f2ed09`. Data Plane copies in inference-client-rs and embedding-engine-rs moved, with their case tables (also graph-index-rs by R2-OD-40-c).
- SSE event payload golden (adds `awaiting_input`, `goal_progress.last_verdict`). Pin starts `64db08de`. SPA copy and `chat-event-payloads.generated.ts` regenerated.
- Service route table golden. Pin starts `c6a3716f`.
- Auth Core org-core retention-posture contract pair (adds `effectiveZdr`, `lapsed`, `planLapsePolicy`). Pin starts `b3c4575f`. Needed with ZDR-Z-03a (see section 4).
- Authored data re-pinned: `space-surface.v1.json` (starts `e50d9cd6`, MP pin, model-gateway and gateway copies) and `org-agent-policy-schema.json` (starts `8105676c`).
- Python proto bindings for 13 changed protos: capabilities, event_triggers, execution, gateway, inference, memory, orchestration, proof_bundle, routing_policy, runs, sessions, thread_goals, thread_shares. Made with buf plugin `protocolbuffers/python:v36.0` (header stamp 7.36.0; some were 7.36.2 before).
- Go proto bindings were regenerated by the compile fixers batch by batch (FU-11 says "already regenerated"). Not re-proven.

### 1.2 What FUGEN did not regenerate (do these first, one commit per generator)

- Do tool catalogue: `apps/Model Plane/rust/services/execution-core` `do_tool_catalogue.json`. Generator: `DO_TOOL_CATALOGUE_UPDATE=1` on the catalogue test. FUGEN did not check whether it is stale; likely, because W4-CC-7 (custom REST), W4-P6 (process spill) and W5-071 (coordinator) changed Do tools.
- user-core denial-codes golden (`apps/Control Plane/user-core`). Find the generator switch by `git grep -n "UPDATE" -- "apps/Control Plane/user-core"`. Not checked for staleness; W4-SH-3, FU-15 and W3-EP-8-S7-a added scopes and denial paths.
- audit-core wire fixtures. Generator: `UPDATE_AUDIT_CORE_FIXTURES=1`. Not checked for staleness; units that add audit events (approval decision events, tool-policy rule audit, waiver issuance) may have moved them.
- Quarry `zdr-feature-matrix.json`. Generator: `QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix` in the Quarry container (see 2.4). Its pin (`f80b8c96`) already matched the file, so only confirm that the file is not stale against ZDR-Z-03e and FU-09 (quarry-edge test count 40 -> 47).
- `sandboxes_pb2.py` (stale since 2026-11-08, before the base commit) and Python modules for the four Space protos (`space_attention`, `space_events`, `space_history`, `space_room`, none exist). Decide first whether the Python bindings list must cover them (consistency item C-18).
- `orchestration_pb2.py`: FU-25 added an RPC and the file only has it if FU-25 was in the tree at about 23:3x on 2026-11-11. Regenerate and diff.
- Stray copy `apps/Frontend Plane/Ingestion Plane/Quarry-v2/.../zdr-refusals/` still holds the old catalogue. Not in the sync script. Owner decision first (consistency item C-19). Do not edit by hand.

### 1.3 How to check the pins

- Run `bash scripts/tests/zdr-contract-sync.test.sh`. FUGEN's last result was 197 passed, 0 failed. Any other result is a regression since B18.
- Run `bash scripts/tests/model-entitlement-contract-sync.test.sh` and `bash scripts/tests/space-contract-vectors-sync.test.sh`. Both passed at B18.
- Run `bash "apps/Control Plane/scripts/control-train-repin.sh"` (check mode). Expect no output and exit 0.
- Check the first eight hex characters of each pin in `apps/Model Plane/api/contracts/*.sha256` against the list in 1.1. Pins are LF-normalised sha256, written `hash *name` (Model Plane, SPA, gateway) or `hash  name` (Data Plane, generator pins, auth-core).
- For every generator in 1.1, run it again in generator mode and require an empty `git diff`. Generators: `VEREVON_UPDATE_GOLDEN=1` on `admission-contract.test.ts`; `MP_UPDATE_GOLDEN=1 cargo test -p mp-contracts zdr_refusal`; `MP_UPDATE_PROMPT_MANIFEST=1 cargo test -q -p mp-contracts --test prompt_tool_manifest`; `MP_UPDATE_GOLDEN=1 cargo test -p inference-core --lib status_contract`; model-gateway `sse_event_payload_golden` and `service_route_table_matches_the_golden`; mp-contracts `model_entitlement_catalogue`; `UPDATE_CONTRACT_GOLDEN=1 npx jest interactive-retention-policy.org-core-contract` in auth-core.
- Go bindings: with the installed buf 1.72 and protoc-gen-go 1.36.12, generate into a scratch dir and diff against `apps/Model Plane/go/gen/model_plane/v1` for the 13 protos above. Require no diff. Then `go test ./...` in `go/gen` (wire-parity tests).
- Proto hygiene: `buf lint "apps/Model Plane/proto"` and `buf breaking` against `031e7965d`. Contracts must be additive (new fields only). Next free `RunAgentRequest` field is 27 (B03a note).
- Python bindings: import each of the 13 modules; header stamp must read 7.36.0.
- After any later change to `model-access.ts`, the action registry, a proto, a posture row or the refusal catalogue, run that artifact's generator again before moving on.

### 1.4 Hand edits FUGEN had to make to sources (review them)

- inference-core `grpc.rs` test helper `grpc_code_name`: added `NOT_FOUND` arm (the generator panicked without it).
- Data Plane `inference-client-rs/tests/status_contract.rs`, `embedding-engine-rs/src/context/outcome/tests.rs`, `graph-index-rs/src/state/tests.rs`: new cases for `model_hidden`, `image_model_unavailable`, `providers_benched`, `provider_model_not_entitled`, `provider_entitlement_unavailable` (also R2-OD-40-c).
- SPA `chat-event-contract.test.ts`: `UNUSED_KEYS` gained `awaiting_input` and `goal_progress.last_verdict`. The SPA does not read `last_verdict` yet.
- auth-core: `pnpm install --frozen-lockfile --ignore-scripts` created `node_modules` (untracked). Not a tracked change.

## 2. Compile and test baseline

### 2.1 Pre-flight

- `git -C C:/dev/mp-w345 status --short` should show only the ZDR.md edit from the consistency pass (`apps/Model Plane/docs/ZDR.md`, a merged cell) and this file. Settle those two (commit them when the owner allows commits) before anything else.
- Free space on C: at least 40 GB before any cargo run. Delete only agent scratch targets (`%TEMP%/cargo-ws02s6*` style).
- Docker engine healthy (`docker version` answers within 15 s). DB tests and live proofs need it.
- Install deps once where missing: `pnpm install` in `apps/Control Plane/auth-core` (done by FUGEN) and in `apps/Frontend Plane/verevonv3` (a worktree prunes `pnpm-lock.yaml` on every pnpm call: restore it with `git checkout -- "apps/Frontend Plane/verevonv3/pnpm-lock.yaml"` after the last pnpm call). `apps/Application Plane/convex-core` has no `node_modules`; install it, because B01 and B02 convex changes were never type-checked.

### 2.2 Base export and the diff rule

- Make a clean export of the base commit without writing git metadata: `git -C C:/dev/mp-w345 archive 031e7965d <paths> | tar -x -C C:/dev/mp-w345-base`. Never `git worktree add` from here (it writes into the shared `.git`).
- Run each suite in 2.4 on the export first, with its own target dir (`C:/dev/mp-target-w345-base`), and save the sorted failing test names to `mp-w345-plan/ (programme plan folder) baseline/<suite>.txt`.
- Run the same suite on `mp-w345`. The rule: diff the sorted failing sets. Only a test that fails on the branch and passes on the base is a regression. Do not trust exit codes (a `| tail` hides them; redirect to a file and read `$?`).
- A test that fails on both is "pre-existing" only if it is in the list in 2.5 or you can show the same failure on the base export.
- A test that passes on the base and fails here must be fixed in the owning unit, not by editing the test, unless 3.3 says the test is stale.

### 2.3 Compile gates (the lead re-runs these; writers' "untouched" claims were wrong twice)

- Model Plane Rust: `cd "apps/Model Plane/rust" && CARGO_TARGET_DIR=C:/dev/mp-target-w345 cargo check --workspace --all-targets` (exit 0 required at every batch).
- Verevon v3 gateway: `CARGO_TARGET_DIR=C:/dev/mp-target-w345-gw cargo check --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (exit 0).
- SPA: `cd "apps/Frontend Plane/verevonv3" && node node_modules/typescript/bin/tsc6 -p tsconfig.app.json --noEmit > out.txt; python mp-w345-plan/ (programme plan folder) tscdiff.py out.txt`. Baseline is 33 errors in 16 keys; the diff must say "new 0". `tsc -p tsconfig.json` checks nothing; do not use it.
- Model Plane Go: per service from `apps/Model Plane/go` use `go build ./services/<name>/...` and `go vet`, for capability-core, orchestrator-core, cost-core, sandbox-manager and `pkg`. Build per service; a bare `./...` from `go/` fails. Use `GOWORK=off` only when adding a dependency.
- Control Plane Go: `go build ./... && go vet ./...` in org-core, user-core, audit-core, `flag-registry`.
- Ingestion Plane Go: same in `integration-corev2`.
- Data Plane: `make check-rs`, then `make build-go`.
- auth-core: `npx tsc --noEmit -p tsconfig.json` (covers spec files; `resolveJsonModule` was added by ZDR-Z-14).
- convex-core: type-check `convex/agents.ts` and `convex/spaceAgents.ts` (B01 and B02 never did).
- Flag registry static rules: `python mp-w345-plan/ (programme plan folder) regcheck.py` (expect 0 problems), then `go test ./...` in `apps/Control Plane/flag-registry`.
- Scripts: `python -m pytest scripts/tests` for the `test_*.py` files and `node --test` for the `.mjs` tests under `scripts/tests`.

### 2.4 Suites per plane (run in this order)

- Shared contracts first: `cargo test -p mp-contracts` (Model Plane Rust). Everything else reads its fixtures.
- Control Plane: auth-core `npx jest` (full), org-core `go test ./...`, user-core `go test ./internal/http/... ./internal/authztaxonomy/... ./internal/nats/...`, audit-core `go test ./internal/provisioner/...`, flag-registry `go test ./...`.
- Data Plane v2: `make test-rs` (inference-client-rs `status_contract`, embedding-engine-rs, graph-index-rs), then `make test-integration` for the wiki-store migration (W5-094-a).
- Model Plane Go (per service, no `-race`, cgo is off): capability-core, orchestrator-core, cost-core, sandbox-manager, `go/gen` and `go/pkg`.
- Model Plane Rust, as `--lib` plus the named integration tests, per crate: mp-contracts, mp-content-safety, inference-core, session-core, execution-core, model-gateway. Use `cargo test -p <crate> --lib` first; add `--tests` only for the named files in section 3. Session-core DB tests (`#[ignore]`) need the DB harness: run with `-- --ignored` after the engine and Postgres are up.
- Ingestion Plane: `go test ./...` in integration-corev2 (`migrate_upgrade_test.go`, `zdr_acknowledgement_test.go`, `zdr_posture_matrix_test.go` once written). Quarry-v2 cannot build on the host: run in a `rust:1` container with cache volumes `quarry-cargo`, `quarry-go`, `quarry-target`, using `bash -c` (not `-lc`). Expect over 10 minutes; run in the background.
- Application Plane: `go test ./...` in `conversation-core/conversation-core-go`; convex-core `test/*.test.cjs` (needs `node_modules`).
- Frontend Plane: `pnpm exec vitest run` for the touched folders (`src/shared/actions`, `src/shared/api`, `src/features/chat`, `src/features/settings`, `src/features/agents`, `src/features/router-policy`), then the full suite against the base failing set; gateway `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`; `node --test` on `action-surface-contract.test.mjs`.
- Repo scripts: the four scripts in section 4 plus `bash scripts/tests/zdr-contract-sync.test.sh`.

### 2.5 Known pre-existing failures and where they are recorded

- SPA `tsc6` baseline: 33 errors in 16 keys. Recorded in `mp-w345-plan/ (programme plan folder) tsc-B*.txt`, `tscdiff.py` and the handoff (`docs/plans/model-plane-code-completion-handoff-2026-11-10.md`, section 6). Memory note `verevonv3-worktree-verification` gives other numbers for an older tree; the plan-folder baseline wins for this tree.
- SPA `pnpm lint` dies before reading a file (typescript-eslint refuses TS 7.0). SPA full vitest has failures on a pristine tree (memory note: 12 files, 30 tests, 8 errors). Baseline against the export.
- Quarry-v2 `quarry-evals` bin `quarry-browser-benchmark`: E0063 `LiveFrame.agent_cursor` missing. Recorded in the handoff section 6, followups (B08x) and consistency item C-20. Fix belongs to the Quarry branch `quarry-w45`.
- Two Model Plane Rust targets failed on Windows before this work: inference-core `main.rs` (`tokio::signal::unix`) and model-gateway `tests/e2e_invoke_chain_test.rs` (missing field `metadata`). Recorded in memory note `model-plane-build-and-deploy`. Several units edited `e2e_invoke_chain_test.rs`, so re-check both on the base export. Use `--lib` when they fail.
- Gateway contract test `tests/support/offline_downstreams.rs`: asserts 22 client fields while AppState has 35 (red before W4-T7; B08x/W4-T7 note).
- Gateway `action-surface-contract.test.mjs`: whole-set equality fails on 8 dotted literals in `handlers.rs` (`web.crawl_site`, `web.job_status`, `web.pause_job`, `web.cancel_job`, `web.resume_job`, `web.map_site`, `web.batch_scrape`, `ingestions.run_schedule_action`). Recorded in followups (B18) and consistency item C-13. Decision owed (section 7).
- auth-core full jest: 15 failures in 3 suites (`deployment-contract.spec.ts` 8, `plane-service-principal.spec.ts` 6, `model-plane-token.controller.spec.ts` 1). Recorded in memory note `auth-core-local-testing`. `pnpm lint` dies with ENOBUFS; prettier on CRLF files shows only `Delete CR` noise.
- Go `-race` cannot run (cgo off). Say so in the report; do not skip silently.
- Flag registry: `VITE_APPROVAL_SCOPE_GRANTS` has a wrong `codePath` (consistency item C-12). `TestShippedCodeFlagsAreStillRead` would flag it.
- Model-gateway e2e tests that dial the dev stack on `localhost:9091` hang when Docker is wedged. That is the engine, not the code.
- `rustfmt --check` on `inference-core/src/http_health.rs` shows 2 diffs in old test code (W3-EP-14-S10-a note). Leave.

## 3. Tests owed

The hard rule in the code phase was "no tests". Writers wrote some tests (never run) and left many unwritten. The spec of each unit has its Tests line: `mp-w345-plan/ (programme plan folder) units/<id>.json`, field `spec`. Where this section names a test, the unit is in brackets.

### 3.1 Written but never run (run these first, they may fail)

- auth-core: `api-key-exchange.controller.spec.ts` [F-f08-12]; `interactive-retention-policy.org-core-contract.spec.ts` [ZDR-Z-03b]; `plane-service-principals-policy.contract.spec.ts` [R2-OD-58-a]; `retention-decision.posture-matrix.spec.ts` [ZDR-Z-14].
- org-core: `internal/http/retention_posture_handler_test.go` [ZDR-Z-03a]. user-core: `internal/authztaxonomy/taxonomy_test.go` [W4-SH-3]. audit-core: `internal/provisioner/provisioner_test.go` (consumer count 15 -> 16) [FU-27].
- integration-corev2: `internal/api/zdr_acknowledgement_test.go` [ZDR-Z-12-13]; `internal/db/migrate_upgrade_test.go` [W4-CC-3, FU-18].
- Data Plane: `inference-client-rs/tests/status_contract.rs`, `embedding-engine-rs/src/context/outcome/tests.rs`, `graph-index-rs/src/state/tests.rs` [R2-OD-40-c, FU-11].
- Quarry: `quarry-edge/tests/zdr_feature_matrix.rs` (count 40 -> 47) [FU-09].
- mp-contracts: `tests/approval_cause_golden.rs` [W2-WS23-S3-a]. execution-core: `tests/approval_gate_contract.rs` [W2-WS23-S3-a], `tests/loop_fold_zdr_matrix.rs` and `tests/zdr_mcp_golden.rs` [R2-OD-12-b] (counts 36 rows, ZDR 9, WAIVED 6).
- inference-core: `tests/fallback_test.rs`, `src/grpc/real_rules_tests.rs`, `src/grpc/zdr_waiver_route_tests.rs` [R2-OD-40-b1/b3]; `tests/model_entitlement_chain_test.rs`.
- model-gateway: `tests/zdr_posture_matrix.rs`, `tests/zdr_mcp_observation_golden.rs`, `tests/zdr_feature_table.rs` [R2-OD-12-b, R2-OD-12-a]; `tests/orchestration_http_test.rs` and `tests/e2e_invoke_chain_test.rs` [FU-25, W3-EP-6-P3-a].
- session-core: `tests/zdr_refusal_inventory.rs` (15 sites) [R2-OD-12-a, F-f02-10]; `src/run_phase_tests.rs` and `src/grpc_space_mentions_tests.rs` [W3-EP-6-S9-runmeta].
- verevonv3 gateway: `apps/gateway/tests/continue_error_vocabulary.rs` [FU-28, W2-WS14-S5-h].
- SPA: `FleetRunConsole.test.tsx` [W5-071-c]; `ChatArtifactPanel.history.test.tsx`, `ChatArtifactPanel.promote.test.tsx` [W3-EP-2-S8-ui]; `ChatMessages.plan-gate.test.tsx`; `ChatPage.browser-dock.test.ts`; `PersonalSetupCard.test.tsx`; `SettledTurnFold.test.tsx`; `settled-fold.test.ts`; `settled-fold-state.test.ts`; `ApprovalRuleProposals.test.tsx` [W2-WS13-S7-spa-proposals]; `action-registry.test.ts` (count literal 374) [FU-02]; `chat-run-continue-contract.test.ts` [FU-28, W2-WS14-S5-h]; `narration.test.ts` [W4-IMG-4]; `chat-event-contract.test.ts` [FU-11]; `zdr-refusals.test.ts` [ZDR-Z-05b].
- Script: `scripts/tests/zdr-contract-sync.test.sh` (new VENDORED rows, ZDR-Z-32).

### 3.2 Owed, not written, by service

Session-core (`apps/Model Plane/rust/services/session-core`; `#[ignore]` DB tests need the harness):
- DB tests: `verdict_met_closes_goal`, `user_reopen_bumps_revision`, idempotent verdict [W3-EP-7-S3-a].
- Judge gate: `refuse_non_judge` admits `service:model-gateway-goal-judge`, refuses `service:model-gateway` and a user; a verdict on a thread with a waived run is refused and changes no row [R2-OD-58-b].
- Browser timeout: a `timed_out` browser_human_assist approval ends the run as `browser_help_timed_out`, with and without the switch; mirror `a_resume_question_that_lapsed_ends_its_run_as_question_expired` [W3-EP-1-S5-timeout].
- Attention: goal item build unit test; keep the 4-key golden test for pending rows [W3-EP-7-S5-a].
- Named agents: thread list filter narrows only [W3-EP-6-P3-d]; run-meta validation unit plus a DB test that `metadata.agent` is written and absent for ZDR [W3-EP-6-S9-runmeta].
- GDPR: switch parser takes only `true` [R2-OD-59-a1]; DB tests that the person's own personal-thread rows are deleted and recorded as forgotten, Space-thread attribution, hold refusal [R2-OD-59-a2]; two statements bound to org and user, grants erased before proposals, summary literal updated [W2-WS13-S7-user-erasure, W2-WS13-S7-proposal-store-a]; ordering of `erase_user_space_memory_sources` against the hold check [W3-EP-8-S4-gdpr, see C-list item in section 7].
- Approvals: refusal codes and SQL WHERE clauses [W2-WS13-S7-proposal-store-a]; resolution and `policy_id` validation [W2-WS13-S7-proposal-store-b]; persistent -> true cases [W2-WS13-S7-waived-decided]; pinned MCP external_write grantable with its digest and not without [W2-WS13-S7-grant-digest].
- Input pause: mapping test near line 7789 extended, and the source-reading test `run_question_the_spend_keeps_its_predicate_and_stores_only_the_claim_hash` moved to the new predicate text, with an assertion that `renew_recovery_deadline(` is called once [W2-WS14-S5-b1]; `prepare_question` with the switch on and off and the pause-ref mismatch refusal [W2-WS14-S5-b2]; input-pause-only branches and a DB test that an input fold save is stamped [W2-WS14-S5-b3].
- Space versions: a service reader with no `for_user_id` does not see a Space row's versions (DB-gated) [F-f10-16].

Execution-core (`apps/Model Plane/rust/services/execution-core`):
- Do allowlist: `allowlist_narrows_never_widens`, `unmapped_tool_dropped`, `field_absent_unchanged` [W3-EP-6-S9-do].
- Loop fold and resume: `build_fold` writes the taint bool always, scope only when non-empty, offered tools only when Some [W2-WS14-S5-c1]; helper tables [W2-WS14-S5-c2]; `with_carried_state` writes and omits keys [W2-WS14-S5-d]; `resume_delivery_when` for each excluded run kind, and approval pause reference unchanged [W2-WS14-S5-e1]; pre-pass turns a resume ask into AwaitingInput and rejects a second [W2-WS14-S5-e2]; `enforce_question_resume_needs_the_switch_and_exactly_the_question_shape` updated for segment mode [W2-WS14-S5-f1]; two parallel continues with different tokens give one continue and one `continue_in_progress` [W2-WS14-S5-f2].
- Approvals: request builder carries persistent, retention_bound or empty per entry point [W2-WS13-S7-rule-use-a]; empty `pause_facts` on every other constructor [W2-WS13-S7-producers-b]; `shell_rule_not_proposed_for_a_waived_run` in `mod shell_rule_proposals` and callers near 18344-18495 updated [F-f10-01].
- Scheduled: `scheduled_agent_refuses_zdr_and_waived` (failed with `scheduled_zdr_refused`, no inference, no tools), thin wrapper for `refuses_retention` [F-f10-29].
- Provider read: a connection id such as `../x` or `a/b` errors with no HTTP call [W2-WS23-S3-b]. Sandbox lease: two concurrent `ensure` calls make one acquire [W2-WS24-S4-a].
- Space listing: ExecuteStep withholds Space-scoped search without the header; list refused with the request's space id (gRPC path) [F-f05-01, FU-26, P0-D2-769].
- Held test-only unit: sha256 pin assertion for the provider-read golden [W2-WS23-S2-a].

Model-gateway (`apps/Model Plane/rust/services/model-gateway`):
- Judge: `judge_not_called_for_paused_or_failed_turn`, `switch_off_makes_no_judge_call`, `judge_request_inherits_privacy_floors` [W3-EP-7-S3-b]; `from_lookup` with only `MODEL_GATEWAY_SERVICE_API_KEY` yields Configuration [R2-OD-58-c]; route 404 when off [W3-EP-7-S6-a].
- Deck guard: `update_artifact_refuses_a_deck`, `update_artifact_on_document_unchanged` [W3-EP-5-S3-deckguard].
- Org policy: paused, lockdown, read-only and unreadable-policy orgs refused with no upstream call [F-f06-21]; continuation of a run after lockdown refused and the source-pinning test near line 26318 updated [F-f06-19]; thread-owner test double `resolve_thread_owner` and 404/503 cases with no sandbox RPC [F-f06-01].
- Domain rules: wiremock org-core where `example.org` is allowed and `blocked.example` is denied [F-f10-04]; enforce drops the blocked host's hit [F-f10-05]; extract_structured final-URL gate [FU-29].
- Versions: switch off still gives 501 `history_versions_unavailable` [W2-WS17-S9-a]; `ask_user_ask_mode.rs` only literal `true` enables [W2-WS14-S6]; knowledge receipt unit tests in `knowledge_receipt.rs` [W2-WS18-S5-a]; question-form preflight table, `settle_outcome_maps_every_status` extended [W2-WS14-S5-g]; list refused in Space on the inline path [W2-WS18-S5-b].
- Cap: the `run_ceiling.rs` case at line 247 under `MODEL_GATEWAY_CAP_MIN` on and off [P0-D2-698].
- Grants and proposals clients: pure projection tests (action_key omitted) [W2-WS13-S6-gw-grants, W2-WS13-S7-gw-proposals]; cause offered only for a pinned MCP tool, narrowed rule in `approval_cause.rs` and `http_routes.rs` `mcp_cause` [W2-WS13-S7-producers-a].
- Composer-capabilities `deep_research_knowledge` field unit test [W3-EP-2-S7-fe]; relay of `goal_status` serialization [W3-EP-7-S5-b]; `auth.rs` `DELEGATED_USER_BEARERS` gains the tenth entry `x-quarry-authorization` [W5-081-c].

Inference-core:
- Failure classes: `classify` over one example of every `ProviderError` variant [R2-OD-40-a]; fallback with both switches off unchanged, switch on [R2-OD-40-b1]; `ProvidersBenched { retry_after_ms: 1_500 }` -> UNAVAILABLE, kind `providers_benched`, header `x-mp-retry-after-ms` [R2-OD-40-b2]; chain benching and no retry of a just-benched provider [R2-OD-40-b3]; hidden model refused on infer [GAP-W4-BYO-hidden-infer, W4-BYO-1]; routing-policy promotion routes [W3-EP-14-S10-a]; Codex priority-capable models near `codex_subscription.rs:1140` [W5-095-a].

mp-contracts and mp-content-safety:
- `user_question.rs` pause ref for both origins never equals `{run_id}:{step_id}`; resume window constant [W2-WS14-S5-a]. JSON-schema validator moved to `json_schema_subset.rs`, corpus test unchanged [W5-071-a]. Benign and attack rows for subdivision flags in `tests/corpus`, plus `scan.rs` unit tests [W2-WS26-S2-b]. Both the posture matrix row counts and `e_waived_message_versions_refused` in the `caught` set [R2-OD-12-b].

Model Plane Go:
- capability-core: handler test for both modes and gateway route test [W3-EP-6-S11-route]; `TestRunNowRefusesGoalBoundSchedule` [W3-EP-7-S6-d]; custom REST capability seeds start unavailable [W4-CC-7]; org-permission-rules parity tests [W5-083-a, W5-074-b]; rule-use and rule-expiry sweeper [W2-WS13-S7-rule-use-b, W2-WS13-S7-rule-expiry]; plugin loader pins [W4-PL-1, W4-PL-3].
- orchestrator-core: activity unit with `httptest` asserting an ids-only body [W3-EP-7-S6-b]; memory-gate refusal counts [P0-D2-481, test file `cmd/activities/integration_test.go`].
- cost-core: `zdr_spend` kind admitted with the allowlist only, every other `zdr=true` usage still refused [FU-30]; priority multiplier in `RecordUsage` [FU-24]; image price rows [W4-IMG-2].

Control Plane:
- auth-core: banned owner denied on both paths [F-f08-12]; `retention-waiver-mint.spec.ts` [ZDR-Z-14b]; `integration-connection-posture.client.spec.ts` [ZDR-Z-14e]; `retention-decision.signer.spec.ts` [ZDR-Z-14s]; `interactive-retention-policy.posture-matrix.spec.ts` [ZDR-Z-03b]; `inference-service-token.contract.spec.ts` pin for the new scope [W3-EP-14-S6-grant]; provider-null waiver check (section 7, C-16).
- org-core: `internal/org/retention_posture_matrix_test.go` (load the matrix, check the sha, drive `resolveOrgPosture`) [ZDR-Z-03a]; flag-registry test that every name in `org_flags.go` is registered `orgOverridable:true` [W5-079-b].
- flag-registry: entries for the zdr2 switches [ZDR-Z-32].

Ingestion Plane:
- integration-corev2: `internal/api/zdr_posture_matrix_test.go` with an injectable wording index so `text_version_below_minimum` is reachable [ZDR-Z-12-13]; seal and binding tests [W4-CC-3, W4-CC-4].
- Quarry: tests in the schedule-preflight module next to the wiremock tests near line 762 [ZDR-Z-03e].

Application Plane:
- conversation-core: `internal/clients/orgcore_client_posture_matrix_test.go`; a record without `retention_posture` fails `ZDREnabledOrgIDs` and refuses `AllowAIProposal` with the switch on [ZDR-Z-03d].
- convex-core: `test/agent-definition-versions.test.cjs` cases (publish mirrors name, model, knowledgeBindings; unknown version refused) [W3-EP-6-S7-convex, W3-EP-6-S9-pin].

Verevon v3 gateway (BFF, `apps/Frontend Plane/verevonv3/apps/gateway`):
- Posture reader: drive every `org_posture_rows` row through `org_record_posture` with the switch on; per-site Unknown; cached NotZdr -> 412 [ZDR-Z-03c]. Retention-decision client with wiremock Auth Core and the vendored matrix [ZDR-Z-19a]. Acknowledgement proxy routes 404 when off [ZDR-Z-19c]. Waived turn sends `x-retention-decision` on every mint, bearers carry the same waiver [ZDR-Z-19b]. Waived continuation asks Auth Core once [ZDR-Z-19e].
- Relays: history relay and junk stripping [W3-EP-3-S5-reload]; forged `agent_roles` stripped with the switch off and on [W3-EP-6-S10-bff]; preview body test [W3-EP-6-S8-preview]; grants wiremock tests [W2-WS13-S6-bff]; `AppState` flag for each BFF route.

Verevon v3 SPA:
- Component tests: `print.test` (jsdom: no iframe, no toolbar, code text present) [W3-EP-4-S1-export]; `DashboardSearchPanel.href.test.ts` for http, https, `javascript:`, `data:`, relative, malformed [W3-EP-4-S11-href]; `GoalBar.test.tsx` [W3-EP-7-S3-c]; Space memory panel [W3-EP-6-P3-c]; agent presets fail-closed parse [W3-EP-6-S11-ui]; deck template picker [W3-EP-5-S7-ui]; `browser-terminal-copy.test.ts` and the `it.each` at `assistant-browser-agent.test.ts:463` [GAP-W3-EP-1-S5-timeout-copy]; `pending-chat-launch.test.ts` ids survive write and read [W2-WS23-S6-a]; `question-resume.test.ts` per branch [W2-WS14-S5-i]; `ThreadApprovalGrants.test.tsx` [W2-WS13-S6-spa-grants]; `ChatMessages.always.test.tsx` [W2-WS13-S7-spa-decide]; `ConnectionZdrAcknowledgement.test.tsx` [ZDR-Z-20-to-Z-21b]; `memory-client` and composer intercept (500-character bound) [W3-EP-11-S7-a]; info-note branch [W3-EP-10-S7-a]; fetch-level `invalid_role_table` test in `RouterPolicyPage.test.tsx` [W3-EP-13-S5-a]; capability fields parse [W3-EP-2-S7-fe]; BYO group hidden for ZDR [GAP-W4-BYO-picker, W4-BYO-6].
- Every `VITE_*` flag added in this phase (section 6 of the coding-complete report): one test that off renders nothing.

### 3.3 Existing tests that will fail or need edits

- `src/shared/api/http.error-hints.test.ts:54` and `http.space-denial.test.ts:39-41`: now fail because `details.issues` is read in the production path [W3-EP-13-S5-a]. Update to expect the new shape.
- `apps/gateway/tests/continue_error_vocabulary.rs`: `bff.len()` 8 -> 9 (line ~177), `gateway.len()` 10 -> 11 (line ~282), status match gets `FORBIDDEN` -> 403, and model-gateway `CONTINUE_HTTP_REFUSALS` needs `("org_agents_paused", FORBIDDEN)` [FU-23]. FU-28 also touched this file: re-count after it.
- Tests that assert a deny deletes the encrypted continuation descriptor: now false for the `browser_human_assist` cause [FU-25]. Find with `git grep -n "decide_approval" -- "apps/Model Plane/rust/services/session-core"`.
- Registry count assertions: `action-registry.test.ts` (374), `zdr_feature_matrix.rs` (47), fold tuples `(3, 16)` and `(3, 10, 6)`, org-core `TestOnlyAuthCoreAndQuarryEdgeMayReadRetentionPosture`, provisioner consumer count 16.
- `gateway-error-codes.test.ts` may fail until the `transcripts.rs` codes have copy in `src/shared/i18n/errors.ts` [W5-090-b].
- `approval_cause.rs` lines ~589 and ~594 and `approval_cause_golden.rs` line ~82 used `tainted_egress` as an unknown code; it is now known [W2-WS23-S3-a]. Confirm the edit landed.
- Inference-core `status_contract` golden test fails until FUGEN pins match (they do after B18); confirm [W4-BYO-1].
- `auth-core/src/auth/plane-token-scopes.spec.ts` keeps a single-holder scope list. Decide whether `session:request-snapshots:write`, `session:goal-judge:read`, `session:goal-verdict:write` and `model:goal_judge` belong there [W3-EP-14-S6-grant].

### 3.4 One test per switch (all units)

- For each of the 93 switches in section 6 of the coding-complete report: a parser test (only the literal `true` turns it on) and a "off changes nothing" test. Exceptions with different rules: `CONTENT_SAFETY_NORMALIZE` (default on, falsey words opt out), `PROMOTE_ON_USE_SIGNAL` (value `fetch` or `cited`), `MODEL_GATEWAY_SPEED_TIER` (see C-01).
- Switches outside the 93: `ORG_FLAGS_ENABLED` (org-core), `WIKI_CREATE_PAGE_PROPOSALS_ENABLED` (Data Plane), `CREDENTIAL_BINDINGS_ENABLED`, `CUSTOMER_GATEWAY_CONNECTIONS_ENABLED`, `CUSTOM_REST_CONNECTORS_ENABLED` (Integration Core), `VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED` (BFF). Test the same two cases.
- About 138 units (of 295 reports) name no test at all in their report or spec. They owe at least the per-switch pair above plus the checks in section 6 for their group.

## 4. Principal and contract parity checks

### 4.1 Units that changed `apps/Control Plane/config/plane-service-principals.json`

- W3-EP-14-S6-grant (B01): `inference-core` on audience session-core gains `session:request-snapshots:write` and its retention moves `zdr` -> `persistent` (owner decision D3-263; `routing:read` tokens for that audience now carry persistent retention).
- W3-EP-7-S6-b (B03c): `orchestrator-core` gains audience `model-gateway`, scope `model:goal_judge`, retention persistent; `pkg/servicetoken` gained an optional `Config.MintPath` (model-gateway mints on `/api/model-plane/internal-token`).
- P0-D2-481 (B11): `orchestrator-core` gains `session:memory-controls:read` on session-core.
- R2-OD-58-a (B13): new principal `model-gateway-goal-judge` (audience session-core, scopes `session:goal-judge:read` and `session:goal-verdict:write`, retention persistent, `consumerEnv` `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_API_KEY`). Model-gateway's own session-core row stays `zdr` (a spec assertion pins this).

### 4.2 Checks to run for those four units (the PRINCIPAL-TRAIN acceptance list, `apps/Control Plane/config/PRINCIPAL-TRAIN.md`)

- `bash scripts/tests/principal-registry-parity.test.sh` (policy file, launcher and bootstrap agree; takes minutes on Windows; a missing tool is a failure).
- `bash "apps/Control Plane/scripts/control-train-repin.sh"` (then `--apply` only if it reports drift caused by a Quarry manifest row; none was edited here).
- `cd "apps/Control Plane/auth-core" && pnpm exec jest plane-service-principals-policy.contract inference-service-token.contract`.
- `cd "apps/Control Plane/user-core" && go test ./internal/http/...` (the user-core test of the acceptance list). Add `./internal/authztaxonomy/...`.
- `bash scripts/tests/zdr-contract-sync.test.sh` (goldens and copies pinned).
- Run the auth-core full suite and diff against the base failing set (2.5): the new `model-gateway-goal-judge` block must not add a failure.
- File one Control principal-train request that carries all four entries, with each unit's request block (slice, file, principal, audience, scopes, retention, `consumerEnv`, NATS ACL none, flag none). DEC-M4 asks for one reviewed approval.
- Deploy proof for each change: run `scripts/run-control-plane.sh config` once and discard the output (it prints secrets), recreate `auth-service`, then do a real mint and confirm the token carries the new scope: inference-core -> session-core, orchestrator-core -> model-gateway, orchestrator-core -> session-core, goal-judge -> session-core.
- Consumers' compose: production `docker-compose.production.yml` still lacks `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_ID` and `_API_KEY` (item C-10). Until fixed, the judge route answers 503 `goal_judge_unavailable`.
- Not yet in the registry (units not built): the five DEC-M4 scopes (F-f04-12, F-f09-07, F-f10-03, F-f10-08, F-f10-23a), `orchestration:question:service-origin` (O-27, unblocks W3-EP-4-S7), `approval:grant_use` for execution-core (D-WS13-5). When built, they join this list.

### 4.3 Units that changed a contract (run the parity check named on the right)

- Posture matrix, 37 rows: R2-OD-12-a, R2-OD-12-b, ZDR-Z-05a, ZDR-Z-03a/c/d, ZDR-Z-14, ZDR-Z-32 -> `zdr-contract-sync.test.sh`, `zdr-contract-schema.test.mjs`, the matrix-reading tests in section 3.1 (row counts 18/10/6/3 and tuples).
- Auth Core / org-core retention-posture contract: ZDR-Z-03a with ZDR-Z-03b together -> auth-core `interactive-retention-policy.org-core-contract` and org-core `TestAuthCoreRetentionPostureRequestIsAdmittedByRealOrgCoreMiddleware`. Org-core's golden test fails until the golden is regenerated (done by FUGEN; confirm). Also org-core `TestOnlyAuthCoreAndQuarryEdgeMayReadRetentionPosture`.
- ZDR refusal catalogue: W4-CC-5, W4-CC-7, W4-P6, W5-081-c, ZDR-Z-05a, W5-087, W5-098-c, W3-EP-6-P3-b, R2-OD-12-a -> `zdr-contract-sync.test.sh`, SPA `zdr-refusals.test.ts` (every code has Norwegian and English copy), Quarry `zdr_feature_matrix.rs`.
- Admission golden and action registry: W4-SH-6, FU-01, W4-BYO-2, W4-BYO-6, W3-EP-8-S7-b, W5-081-d, FU-02 (374 ids) -> `admission-contract.test.ts`, gateway `action-surface-contract.test.mjs` (8 known literals), capability-core and execution-core fixture tests.
- Status contract: W4-BYO-1, W4-IMG-1, R2-OD-40-b2, R2-OD-40-c -> inference-core `status_contract`, the three Data Plane tables.
- Space surface: W3-EP-8-S10-b, W4-CC-1, W4-PL-3, W5-074-c, FU-07 -> `space-contract-vectors-sync.test.sh`, model-gateway and gateway fixture tests. Reason text for `GET /api/v1/spaces/{space_ref}/memory` is stale (needs golden, fixture copies and pins).
- Org agent policy schema: W5-083-a, F-f06-19 -> sync script digest `8105676c`.
- Protos (additive only; run `buf lint` and `buf breaking`): see 1.3 for the 13 files and the units behind them. FU-25 added an orchestration RPC; ZDR-Z-19 and GAP-W5-069-a added `holds_retention_run` and `model_connection_id`; W3-EP-6-P3-a/b added memory field 7 and `CheckMemoryWrite`.
- User-core authz taxonomy and scopes: W4-SH-3, FU-15, W3-EP-8-S7-a (`scopes_space_memory_decision.go`) -> user-core `go test ./internal/authztaxonomy/... ./internal/http/...`; denial-codes golden (1.2). Handlers assert their own scope: an unmapped POST falls through to `users:write:self`.
- NATS: FU-27 changed `control-shared-nats.conf` and the audit-core provisioner (durable `user-core-org-erasure`) -> `go test ./internal/provisioner/...`; load the conf in a throwaway NATS to prove the ACL parses.
- Docs under frozen contract: W4-CC-6 extended `docs/actions-surface-operations.md` (custom_rest section). Owner review first (OD-54). Revert if refused; no unit may edit it again.

## 5. Migrations to apply, per service, in order

Rule for all: apply to a copy of the dev database first; run the down file; apply again; run the service's own migrate test. Never edit a migration after it has been applied anywhere (session-core checks sqlx checksums; a changed file crash-loops the service). New files are LF.

### 5.1 session-core (`apps/Model Plane/rust/services/session-core/migrations`, self-migrates at boot, `revert/` has the down files)

- 0093_thread_goal_verdicts [W3-EP-7-S3-a]. This number sorts below the already-applied 0094-0114. Prove on a copy of the live database that sqlx applies it; if not, renumber before any deploy.
- 0115_browser_help_timed_out_failure_code [W3-EP-1-S5-timeout]. Down is valid only while no outbox row carries the code (the revert header gives the check query).
- 0116_agent_skills_teach_origin [W5-081-b].
- 0117_org_routing_overlays [W4-BYO-3].
- 0118_event_trigger_schedule [W4-T3].
- 0119_event_trigger_fire_task [W4-T4].
- 0120_event_trigger_wake_filter [W4-T5]. Switch the wake gate off before running its down.
- 0121_memory_control_events_erased [FU-03].
- 0122_promote_use_counts [W5-093-b].
- 0123_messages_model_connection_id [GAP-W4-BYO-1-turn-connection].
- 0124_thread_retention_receipt [ZDR-Z-17] (taken after 0121-0123; NULL reads as persistent, no backfill).
- 0125_approval_grant_tool_digest [W2-WS13-S7-grant-digest].
- 0126_approval_rule_proposal_resolution [W2-WS13-S7-proposal-store-a].
- 0127_run_question_resume [W2-WS14-S5-b1].
- Check: numbers 0095-0099 and 0106 are absent on the branch; confirm they are absent on the live database too, and that no other branch (`quarry-w45`, model-plane) took 0115-0127.

### 5.2 capability-core (`apps/Model Plane/go/services/capability-core/migrations`; the entrypoint re-applies every `*.up.sql` at every boot, so each must be idempotent: apply twice)

- 0039_plugin_supply_chain [W4-PL-1, extended by W4-PL-3 with `catalog_approved`; one file].
- 0040_org_permission_rules [W5-074-b].
- 0041_custom_rest_capabilities [W4-CC-7]. Seeds `cap.custom_rest.read` and `.write`, both unavailable until execution-core's attestation names them.
- 0042_tool_policy_expiry_audit [W2-WS13-S7-rule-expiry].

### 5.3 cost-core (`apps/Model Plane/go/services/cost-core/migrations`)

- 0012_image_pricing [W4-IMG-2]. Creates `image_price_rates` with no rows; the down is `SELECT 1`.
- 0013_zdr_spend_marker [FU-30]. Apply before the gateway starts publishing `zdr_spend` rows.

### 5.4 orchestrator-core

- 0003_feedback_skill_version [W3-EP-11-S3-b]. Rebuilds the primary key and an index. Down deletes duplicate rows (keeps the latest). Test on a copy with data.

### 5.5 Control Plane

- org-core `032_org_flag_values` [W5-079-b], behind `ORG_FLAGS_ENABLED`. The gateway reads the member route `GET /orgs/:id/flags` (org:read:self) on the signed-in viewer's delegation, so it needs no new principal or grant. The internal route `GET /internal/orgs/:orgId/flags` (org:flags:read:any) is granted to no credential in code; a backend reader that needs it would need its own principal and grant.
- No SQL for FU-27 (provisioner durable and NATS conf only).

### 5.6 Data Plane v2

- `infra/postgres/migrations/20261110120000_wiki_create_page_proposals` [W5-094-a], behind `WIKI_CREATE_PAGE_PROPOSALS_ENABLED`. Run through `make test-integration` first.

### 5.7 Ingestion Plane (integration-corev2, forward-only, no down files)

- 0015_credential_bindings [W4-CC-3]. 0016_webhook_event_connection_id [FU-18]. Deploy 0016 before the event-trigger dry run is switched on in capability-core.

### 5.8 Convex (no SQL)

- Schema and function pushes: `agents.ts` (internal mutations, `publishForGateway` mirrors name, model and knowledge bindings), `spaceAgents.ts` (`setSpaceAgentBindingPinForGateway`), `schema/spaces.ts`. Type-check first. Then `docker restart convex-gateway` and wait for "Convex functions ready!".

## 6. Live proofs

Common setup for every proof:
- Build images from `mp-w345`, then recreate with `bash .plan-work/execution/tools/recreate-with-live-env.sh <svc...>` (a plain `compose.sh` from a worktree drops live env and NATS passwords). Check `docker logs` for `Authorization Violation`.
- One side-by-side stack, one test org. Use the ZDR test org for ZDR cases (plan pro). Model `claude-sonnet-5-5`, web search off unless the proof is about it.
- Each proof has three legs: ON works; OFF is byte-for-byte unchanged (same request, compare the response frame list or the DB row set); ZDR and waived behave as the line says. Save evidence to `docs/compliance/evidence/`, redact cookies from any printed request.
- A proof that fails turns the switch off again and goes into the depth register as a row. Do not fix inside the proof.

### 6.1 Deploy-order rules (apply to every group below)

- Control first: recreate `auth-service` after any `plane-service-principals.json` change; mint credentials with the launcher; confirm real mints (section 4.2).
- org-core (ZDR-Z-03a, migration 032) before any posture reader switch. ZDR-Z-03a is verified together with ZDR-Z-03b: the Auth Core golden test must pass first. Flip `AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED` and `INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED` together (D-ZDR-10), then the BFF, conversation-core and quarry-edge readers one at a time.
- session-core before model-gateway for `holds_retention_run` (GAP-W5-069-a: an older session-core never sets it, so the side-chat refusal on waived or ZDR threads never fires; add this line to the OD-64 rollout list). Same order for the named-agent filter (tag 12, W3-EP-6-P3-d) and `model_connection_id` (migration 0123).
- session-core before `EXECUTION_BROWSER_HUMAN_ASSIST` (FU-25 keeps the encrypted continuation descriptor of a denied assist approval; the old code deleted it).
- execution-core before model-gateway for `AGENTIC_SPACE_RETRIEVAL_ENABLED` (an older execution-core ignores the header and searches the whole org).
- `OWNER_LIST_SPACE_REFUSE` on before `MODEL_ACTION_ADMISSION=table` in any Space-capable deployment.
- audit-core (durable `user-core-org-erasure`), then the updated `control-shared-nats.conf`, then `USER_CORE_ORG_ERASURE_CONSUMER_ENABLED`.
- integration-corev2 with 0015 before `CREDENTIAL_BINDINGS_ENABLED`; with 0016 before the capability-core dry run.
- cost-core with 0013 and the allowlist before `MODEL_GATEWAY_ZDR_LEDGER_ROWS`.
- `INFERENCE_ROUTE_NOTE` on inference-core before `MODEL_GATEWAY_ROUTING_DECISION_ENABLED`. `SESSION_ORG_ROUTING_OVERLAY` with `INFERENCE_ORG_OVERLAY`. `SESSION_THREAD_IMPORT_ENABLED` before the foreign-reader switch. `CONTENT_WRITE_POLICY` on session-core must not be off for the memory gates; run `report` first and watch the downgrade counter before `enforce` (F-f03-18).
- Z-03a with Z-03b is one deploy (org-core and auth-core together, goldens regenerated).

### 6.2 Switches that must stay off until a named follow-up is deployed and proven

- `EXECUTION_BROWSER_HUMAN_ASSIST`: off until FU-25 (retained authority for the skip continuation) is deployed and its proof passes, and FU-28 (`skip_authority_unavailable` in the gateway, BFF and SPA lists) is deployed. Without FU-25 the skip continuation runs under placeholder authority and can route below the run's privacy floor.
- `MODEL_GATEWAY_ZDR_LEDGER_ROWS`: off until FU-30 is deployed (explicit `zdr_spend` kind, allowlist, cost-core 0013) and proven, and the owner confirms two assumptions: `cost_basis` is in the allowlist, and a metered or subscription ZDR spend row still reaches the billing outbox (DEC-18).
- `APPROVAL_SCOPE_GRANTS_ENABLED`: off until execution-core holds `approval:grant_use` (D-WS13-5). `APPROVAL_SHELL_RULE_PROPOSALS_ENABLED`: stays off (D-WS13-6).
- `ZDR_WAIVER_ENABLED`: manual owner step after the four enforcers report the waiver contract version (D-ZDR-08); wording needs counsel (OD-62).
- `MODEL_GATEWAY_GOAL_JUDGE_ENABLED`, `MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED`, `ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED`: off until the judge principal is minted and session-core accepts it, and `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_*` are in the compose files (C-10). Evidence is limited to `tool:knowledge_search` (met is downgraded to unsure).
- `COST_CORE_IMAGE_PRICING_ENABLED`, `MODEL_GATEWAY_IMAGE_TOOL`: off until price rows exist (none seeded; owner supplies prices) and DEC-18 attribution is confirmed.
- `MP_TELEMETRY_FIELD_ALLOWLIST`: off until the owner decides whether the `error` field stays on `TRACE_FIELDS`.
- `WIKI_CREATE_PAGE_PROPOSALS_ENABLED`: off until the default workspace is decided.
- `CLIENT_ENV_IDENTITY` (gateway, W5-098-a): off outside a deliberate rollout, because on it tells anyone which environment class the deployment is.

### 6.3 Smallest live proof per switch group

1. Space scope (`AGENTIC_SPACE_RETRIEVAL_ENABLED`, `EXECUTE_STEP_SPACE_SEARCH_WITHHOLD`, `OWNER_LIST_SPACE_REFUSE`, `MODEL_GATEWAY_GRAPH_SPACE_SCOPE`, `SPACE_THREAD_SCOPE_RESOLVE`, `CONNECTOR_SCOPE_RECORD_ENABLED`). ON: a Do run in a Space with a document outside the Space; knowledge search returns only Space documents, the listing is refused typed, a follow-up turn without `space_ref` resolves the Space. OFF: the same run searches the org. ZDR: a ZDR run records no connector ids. Waived: same as ZDR.
2. Approvals (`APPROVAL_AUDIT_EVENTS_ENABLED`, `TOOL_POLICY_RULE_AUDIT_ENABLED`, `APPROVAL_RULE_PROPOSALS_ENABLED`, `CAPABILITY_PERMISSION_RULES_ENABLED`, `APPROVAL_WAIVER_REFUSE`, `VITE_APPROVAL_*`). ON: one gated tool call, one decision; a content-free `approval_decided` row exists; "Always" proposes a rule that capability-core then applies. OFF: no audit row, no proposal. ZDR: no approval content rows. Waived: create/decide refused before any store (`APPROVAL_WAIVER_REFUSE`).
3. Input pause (`AGENT_ASK_USER_RESUME_ENABLED`, `EXECUTION_LOOP_RESUME_ENABLED`, `VEREVON_GATEWAY_CONTINUE_QUESTION_ENABLED`, `MODEL_GATEWAY_ASK_USER_ASK_MODE_ENABLED`). ON: a run asks a question, the answer resumes it exactly once, a second answer is refused, a lapsed question ends `question_expired`. OFF: `ask_user` ends the turn as before. ZDR/waived: no `ask_user` in Ask mode for them.
4. ZDR posture readers (org-core Z-03a, then Z-03b/c/d/e and Z-12-13, `SESSION_THREAD_RECEIPT_ENFORCE`, `ZDR_TYPED_REFUSALS_ENABLED`). ON: an org with a lapsed plan reads ZDR in every reader (policy `plan_lapse` stays ZDR) and a turn is refused `zdr_unavailable`; web search and file output are typed `zdr_feature_not_supported`; an `AppendMessage` with a different posture is refused `zdr_posture_mismatch`. OFF: the old intent-and-entitlement rule, silent drops. ZDR/waived: these are the cases.
5. ZDR waiver (`ZDR_WAIVER_ENABLED`, `ZDR_WAIVER_UI_ENABLED`, `VITE_ZDR_WAIVER_UI`, `ZDR_ACKNOWLEDGEMENT_ENABLED`, `GATEWAY_ZDR_EFFECTIVE_POSTURE_ENABLED`). ON: acknowledge one connection, run one waived turn; every bearer carries the same decision, no sandbox or browser bearer is minted, no approval rows. Also: a request with provider null cannot waive a connection whose provider differs (C-16). OFF: every waiver refused `zdr_connection_not_supported`, routes 404. Only after the keys exist (`ZDR_DECISION_KEY_ID`) and section 6.1.
6. ZDR ledger (`MODEL_GATEWAY_ZDR_LEDGER_ROWS`). ON: one ZDR turn runs the budget check before the provider call and one `zdr_spend` row lands (ids, counts, amount, `zdr=true` kept; no text, tool names, thread or message ids). A second turn over a ceiling is refused before the call. OFF: no row. Waived: counted as ZDR.
7. Memory and skills (`MEMORY_GATE_CAPABILITY_WRITES`, `MEMORY_GATE_ORCHESTRATOR_WRITES`, `SKILL_ORG_ADMIN_CHECK`, `SKILL_ORG_OWNER_CHECK`, `PROMOTE_ON_USE_DURABLE`, `EXECUTION_CORE_SKILL_PINS`, `FEEDBACK_SKILL_VERSIONS`, `SKILL_BUNDLE_MOUNT_ENABLED`). ON: a memory save with an injection string is refused 403 (session-core down gives 503); a member writing an org skill gets `org_scope_requires_admin`; the promote counter survives a restart. OFF: unchanged. ZDR/waived: consolidation refused `zdr_durable_mutation_forbidden`; no skill pins recorded.
8. Erasure (`SESSION_CORE_GDPR_THREAD_MEMORY_ERASURE`, `USER_CORE_ORG_ERASURE_CONSUMER_ENABLED`, request snapshots via `INFERENCE_REQUEST_CAPTURE` and `SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED`). ON: erase a test user; personal-thread memory is gone and recorded as forgotten; a shared snapshot blob stays while another snapshot references it; purge a test org and see its resource grants deleted. A legal hold refuses the whole event first. OFF: rows stay. ZDR/waived: no snapshot is captured.
9. Goals (`MODEL_GATEWAY_GOAL_JUDGE_ENABLED`, `MODEL_GATEWAY_GOAL_UNATTENDED_ENABLED`, `ORCHESTRATOR_GOAL_JUDGE_ACTIVITY_ENABLED`, `MODEL_GATEWAY_NAMED_AGENT_THREADS`). ON: a completed goal turn gets one verdict row (reasons and evidence NULL); retry does not double-record. OFF: no judge call. ZDR/waived threads refused before any read.
10. Event triggers (`MODEL_GATEWAY_EVENT_TRIGGERS`, `EVENT_TRIGGER_RUN_ENABLED`, `EVENT_TRIGGER_WAKE_GATE_ENABLED` plus `EVENT_TRIGGERS_ENABLED`, `EVENT_TRIGGER_FIRE_ENABLED`). ON: one webhook fire starts one scheduled run, bounded by run, hourly and org limits; wake filter with no match starts nothing. OFF: routes 404. ZDR/waived orgs: no run.
11. Inference routing (`INFERENCE_PROVIDER_BENCHING`, `INFERENCE_SAME_TIER_FALLBACK`, `INFERENCE_SPEED_TIER_SPLIT`, `MODEL_GATEWAY_SPEED_TIER`, `INFERENCE_CATALOGUE_OVERLAY`, `INFERENCE_ORG_OVERLAY`, `INFERENCE_GEMINI_EFFORT_ENABLED`, `VERTEX_GEMINI_ENABLED`, `INFERENCE_OPENAI_PROMPT_CACHE_KEY`, `MODEL_GATEWAY_CAP_MIN`, `MODEL_GATEWAY_ROUTING_DECISION_ENABLED`). ON: fail one provider with a bad key; it is benched and the chain moves within the same tier; when all are benched the caller gets `providers_benched` with retry-after; a request cap above the org cap gets the org cap. OFF: no benching, no tier field. ZDR: fallback never leaves the ZDR tier and the cache key is never sent; Vertex is skipped.
12. BYO and custom connectors (`INFERENCE_BYO_GATEWAY_ENABLED`, `MODEL_GATEWAY_BYO_GATEWAY`, `MCP_SECRET_REF_AUTH_ENABLED`, `EXECUTION_CUSTOM_REST_TOOLS`, `PROVIDER_READ_TOOL_ENABLED`, `MODEL_GATEWAY_MCP_PROBE`, the Integration Core three). ON: one customer-gateway turn is served by that gateway only and billed to the customer; one custom REST write pauses for approval. OFF: refused. ZDR/waived: always refused.
13. Execution (`EXECUTION_COORDINATOR_MODE_ENABLED`, `EXECUTION_CORE_LEASE_RELEASE_FILTER`, `EXECUTION_PROCESS_OUTPUT_SPILL`, `EXECUTION_PROCESS_TTY_ENABLED`, `EXECUTION_SHADOW_REVIEWER`, `EXECUTION_DEFERRED_TOOL_INDEX_ENABLED`, `SUBAGENT_TYPED_RESULTS_ENABLED`). ON: coordinator gets read and delegation tools only; a ZDR workspace is discarded, not uploaded; a TTY refuses ZDR. OFF: unchanged. ZDR/waived: never coordinator, never spill, never shadow review.
14. Browser (`EXECUTION_BROWSER_HUMAN_ASSIST` after FU-25/FU-28). ON: deny the takeover; Skip releases the page and the continuation runs under the retained authority (privacy floor, provider hint, tools), or refuses typed `skip_authority_unavailable`. Check the retained descriptor is removed at expiry and never consumable. OFF: unchanged. ZDR: browser runs are refused as before.
15. Chat features on the gateway (`MODEL_GATEWAY_SIDE_CHAT_ENABLED`, `MODEL_GATEWAY_MODEL_COUNCIL_ENABLED`, `MODEL_GATEWAY_REVIEW_RUN_ENABLED`, `MODEL_GATEWAY_TEACH_MODE_ENABLED`, `MODEL_GATEWAY_REASONING_TRANSLATE_ENABLED`, `MODEL_GATEWAY_WEB_BUNDLE_ARTIFACTS`, `MODEL_GATEWAY_DATA_SOURCE_CATALOGUE`, `MODEL_GATEWAY_LIFECYCLE_EVENTS`, `PLUGIN_LOADER_ENABLED`, `SESSION_SNAPSHOT_GRANTS_ENABLED`, `SESSION_THREAD_IMPORT_FOREIGN_ENABLED`, `SESSION_PROOF_WALKTHROUGH_ENABLED`). ON: each route answers and stores nothing it should not. OFF: each route is 404. ZDR/waived: side chat refuses a thread that holds a ZDR or waived run (needs the session-core order in 6.1); translate, council, teach, snapshots and walkthrough refuse as the unit says.
16. Org policy and flags (`ORG_AGENT_POLICY_MODE`, `WEB_DOMAIN_POLICY_MODE`, `ORG_FLAGS_ENABLED`, `CONTENT_SAFETY_NORMALIZE`). ON: lockdown refuses continuation of a running run with `org_agents_paused`; a blocked domain is dropped from search and refused by fetch and extract. OFF: unchanged. ZDR: policy applies the same.
17. SPA flags (every `VITE_*` in the coding-complete report section 6). One headless pass per flag, off then on: off renders nothing and sends nothing; on shows the control and calls the right route. Warm the Vite container first and restart it after host edits.
18. Data Plane (`WIKI_CREATE_PAGE_PROPOSALS_ENABLED`): migration applied, one proposal created and reviewed through the SPA flag `VITE_WIKI_PROPOSALS_REVIEW`.

## 7. Consistency fixes owed, and units written but not built

### 7.1 Code consistency items (`mp-w345-plan/ (programme plan folder) h/consistency.md`, section A; nothing is changed yet)

- C-01: make `MODEL_GATEWAY_SPEED_TIER` strict (`latency_levers.rs` line 336, docs, compose line 703).
- C-02: one strict reader for `switch_from` (`mp-contracts/src/loop_fold.rs` line 348, about 176 call sites, private copy in `session-core/src/request_snapshots.rs` line 58). Check deployed values by grepping compose and the registry only.
- C-03: decide `continue_after_approval_from` (accepts 1, on, yes) together with C-02.
- C-04: read `INFERENCE_VISION_MODEL_DENYLIST_EXTRA` once at start (`inference-core/src/provider/mod.rs` lines 278, 321-326).
- C-05: one switch for the Space list refusal on both paths (`OWNER_LIST_SPACE_REFUSE`), and fire it before `MODEL_ACTION_ADMISSION=table` (D2-774).
- C-06: add the three Integration Core switches to `apps/Ingestion Plane/docker-compose.yml` and the registry.
- C-07: register or compose `CODEX_SUBSCRIPTION_ENABLED`, `INTEGRATION_RATE_LIMIT_ENABLED`, `PYTRENDS_DEV_ENABLED`; state every IC switch in the production compose.
- C-08: delete `ensure_not_paused` (`org_agent_policy.rs` line 509).
- C-09: apply org domain rules in the gRPC `fetch` and `extract_structured` (`grpc.rs` lines 3042 and 3083, `web_extraction.rs` line 228).
- C-10: add `MODEL_GATEWAY_GOAL_JUDGE_SERVICE_ID` and `_API_KEY` to `apps/Model Plane/deploy/docker-compose.production.yml`.
- C-11: principal parity checks and one Control train request (section 4).
- C-12: fix `codePath` of `VITE_APPROVAL_SCOPE_GRANTS` to `ChatPanels.tsx`.
- C-13: fix the 8 dotted literals in `handlers.rs` or teach the extractor; do not add them to the registry.
- C-14: derive the question-form refusal lists from the pinned `/continue` lists (or add a superset test), in model-gateway, the BFF and `chat-run-continue-contract.ts`.
- C-15: gate the revoked-acknowledgement sweep on its switch, or document that it always runs (`integration-corev2/cmd/api/main.go` lines 237, 415-424).
- C-16: test that a null provider cannot waive a connection of a different provider (`retention-decision.service.ts` lines 190, 319).
- C-17: confirm FU-26 passes the request's `space_id` to `owner_reads` on the ExecuteStep path (`execution-core/src/grpc.rs` line 1691).
- C-18: the generated files in 1.2.
- C-19: decide the stray tracked copy `apps/Frontend Plane/Ingestion Plane/` (2226 files): delete, or add to the sync script.
- C-20: add `agent_cursor` to the `quarry-browser-benchmark` literal on the Quarry branch.
- C-21: merge `quarry-w45` first, then re-apply the ZDR-Z-03e quarry-edge change; rerun `QUARRY_UPDATE_GOLDEN`.
- C-22: write the rollout order lines of 6.1 into the OD-64 rollout list.
- C-23: ask the owner to confirm the two FU-30 assumptions and record the answer in the allowlist comment.
- C-24: five small leftovers: service-path thread creates read as persistent (ZDR-Z-17); F-f10-26 contract-text storage not adopted; a person's own approvals rows not erased by user erasure; FU-14 `auth_binding_id` restore until next hydrate; P0-D2-481 scope thread with `SystemActorID`.
- C-25: check whether the registry ratchet covers the Control compose; if so change `USER_CORE_ORG_ERASURE_CONSUMER_ENABLED` source from code to compose.
- Also owed from followups, not in the list above: the GDPR order bug (`erase_user_space_memory_sources` before the hold check, W3-EP-8-S4-gdpr; FU-03 covers part), `user_anonymize` not reaching profile or personal memory, the missing content-free audit row for erasure, `D-f06-B` (lockdown withdraws the inline code interpreter from Ask turns, no unit), W5-075-b side chat question must be `UserContent`, and the `CONTINUE_HTTP_REFUSALS` entry for FU-23.

### 7.2 Owner answers that need work no unit covers (consistency.md, section B: 28 items)

- O-01 retract a queued message; O-02 4 h crash-park notice; O-03 `CRON_CONTRACT_GUARDS_ENABLED` default; O-04 host cap dedupe; O-05 dev-only keys in production compose (`:?` form); O-06 read-class actions into the admission table; O-07 scheduled-run token ceiling per org; O-08 conservative price for unpriced calls; O-09 office-skills gate of 40 requests (docs); O-10 in-place schedule approval renewal; O-11 re-mint browser bearer near expiry.
- O-12 notification-core org purge; O-13 thread ownership check in the gateway and sandbox-manager; O-14 lockdown withdraws the code interpreter; O-15 `answered_message_id`; O-16 plan approval scope; O-17 Cohere rerank check; O-18 API-key turn budget (cost-core + gateway); O-19 web mode covers browser (new switch `CAPABILITY_ORG_WEB_MODE_COVERS_BROWSER`); O-20 Do-turn document and skill references.
- O-21 compliance role end to end; O-22 effective-policy panel in Space settings; O-23 domain rules on the BFF person search and crawl links; O-24 audit-core switches to literal `true` (`audit-core/cmd/server/main.go` lines 276-280, `main_test.go` lines 1927-1935); O-25 action surface (done in FU-02; two ids still answer 501 `owner_not_wired`); O-26 `executor` field on the action registry; O-27 narrow scopes through the train; O-28 chat projects (append-only first).

### 7.3 Units written but not built

- Full list and counts: the coding-complete report, section 5 (`mp-w345-plan/ (programme plan folder) coding-complete-draft.md` until it is moved into `docs/plans`). That section holds the pointer; it has no table of its own.
- 195 F units (depth-register step F) are written and wait: `mp-w345-plan/ (programme plan folder) units-draft/f/` (220 files before the merge, 207 kept, 13 built or part-built; the merged list is `units-draft/f/_merged.md`, decisions D-F* and DEC-F* with the owner's answers in `owner-answers-triage.md`). Includes the five DEC-M4 scope units.
- Two ZDR units were replaced by split units and are not built as written: `ZDR-Z-03-shared-posture-readers`, `ZDR-Z-19-to-Z-21b-bff-spa-waiver`.
- Held for this phase: `W2-WS23-S2-a` (test-only, the sha256 pin assertion for the provider-read golden; see 3.2). Dropped: `ZDR-L12-L33-embed-posture-send` (Data Plane contract request, D-ZDR-05).
- Partial or proof-only units whose gap is a follow-up unit or a test: `FU-22-control-org-purge-grants` (finished by FU-27), `F-f10-29` (F-f10-26 contract text storage not adopted), `W3-EP-1-S5-timeout`, `W3-EP-2-S7-fe`, `W3-EP-6-S7-convex` (tests only).
- Owner must decide, not built: `docs/plans/model-plane-open-decisions.md` section 2 (OD-40 to OD-66). Assumed and built dark, owner to confirm: section 1 (OD-01 to OD-26) and section 3.
- Open debts named in the reports: `browser_run.steer` and `browser_download.send_to_knowledge` answer 501 `owner_not_wired` (D-f09-5); no GET for the plugin kill list entry whose plugin row is gone; side-chat usage attribution; `Priced.note` not emitted on usage frames; council, review and side-chat calls publish no usage envelope (metering decision).
