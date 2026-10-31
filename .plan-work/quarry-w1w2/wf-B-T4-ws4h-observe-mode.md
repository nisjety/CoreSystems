# B-T4-ws4h-observe-mode progress
Design note: wf-B-T4-ws4h-observe-mode-design.md. STATUS: code written, NOT compiled, NOT committed (Docker was down).
qtest.sh printed "DOCKER IS NOT ANSWERING"; `docker version` hung > 20 s. The pre-commit hook also needs docker (gitleaks), so no commit was possible.
A native `cargo check` on the host fails at link time (LNK1104 on a build script under the long scratchpad path); not pursued.

## Uncommitted files that are mine (all under apps/Ingestion Plane/Quarry-v2)
- crates/quarry-edge/src/zdr_refusal.rs (new), tests/zdr_refusal_modes.rs (new)
- crates/quarry-edge/src/{config,state,lib,schedule_routes,change_routes,profile_routes}.rs (whole files are mine)
- one-line field `zdr_refuse_durable_resources: false,` in src/routes.rs (test literal), src/test_support.rs, tests/support/mod.rs
- tests/support/zdr_harness.rs: added Harness::patch and Harness::delete (additive)
- src/main.rs: ONLY two hunks are mine (`mod zdr_refusal;` and `zdr_refuse_durable_resources: cfg.zdr_refuse_durable_resources,` in the
  AppState literal). The claim_mode hunk (3 lines near line 223) is U5's: stage mine with `git apply --cached`, not `git add`.
- docs/RUNBOOKS.md: appended section "A ZDR organization and durable resources" at the END (file also carries other units' hunks: check git diff first).
## Next agent
1. `cargo check --workspace --all-targets` through qtest (every AppState literal outside src/ uses `..` spread, checked by reading; routes.rs, test_support.rs, tests/support/mod.rs and main.rs got the new field).
2. `cargo test -p quarry-edge --lib zdr_refusal --test zdr_refusal_modes --test zdr_gap_rows --test zdr_feature_matrix --test zdr_claim_floor --test zdr_log_capture --test log_field_discipline`
3. Fix whatever the compiler says, then commit in two steps: (a) helper + flag + wiring + routes (+ main.rs hunks), (b) tests + RUNBOOKS.
Expected: zdr_gap_rows unchanged (flag off); log_field_discipline green without baseline change.
## Judgement calls (see design note)
restore_probe left allowed (read-only POST); change latest/history left allowed (reads); change check refused (matches the gap test's inversion wording and the matrix row, though the plan's Files list names only snapshot and refresh); internal record refuses with 403 before the store when body zdr:true (orchestrator Go side retry behaviour on 403 not checked).
