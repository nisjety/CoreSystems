# C1-ws4h-posture: hand-over (2026-10-21)

Slice WS4.H, the posture signal for `/v1/internal/change/record`, stamp-at-create design (design note `C1-ws4h-posture-design.md`, followed as written; the only additions are listed under "Deviations").

## Commits (branch quarryv2-next, local)
| hash | what |
|---|---|
| `2a7759f6` | quarry-control: migration `023_schedules_zdr.sql`, `store.Schedule.Zdr *bool`, pg INSERT/Get/List, `changeMonitorArgs.Zdr` (omitempty), `sourceCreateBody.Zdr` to the monitor schedule; 9 tests |
| `3bfc6a0b` | orchestrator: `ChangeMonitorInput.Zdr`, `CheckChangeInput.Zdr`, `changeRecordRequest.zdr`; 4 tests |
| `06959092` | edge: `ControlScheduleCreate.zdr`, one `effective_zdr(false)` for refusal and stamp, `create_source` (monitor:true) refusal and stamp; `tests/zdr_posture_stamp.rs` (7) |
| `8b3c108b` | matrix texts, golden, `.sha256`, `ZDR_MATRIX.md`, digest in `zdr-contract-sync.test.sh` |
| `efebf3e0` | RUNBOOKS section, plan R-2 row for 023, plan ledger row |

## Verified (what was run)
- `GOWORK=off go test -count=1 ./...` in quarry-control and in quarry-orchestrator: every package ok.
- Postgres (throwaway `postgres:16-alpine` through `INTEGRATE/pg-test-env.sh` under the docker lock, `QUARRY_TEST_DSN`): `TestPgScheduleZdrRoundTrip` (true, false, NULL), `TestMigration023IsIdempotent` and the 7 existing `TestPostgres_*` pass. Without a DSN the two DB tests skip; `TestMigration023Text` always runs.
- `cargo test -p quarry-edge --no-fail-fast` (qtest): 986 passed, 0 failed over 28 `test result` lines (the commit message of `06959092` calls them "28 test binaries"; they include lib, bin and doc targets). `log_field_discipline` 22 and `zdr_gap_rows` 9 are green.
- `QUARRY_UPDATE_GOLDEN=1 cargo test -p quarry-core zdr_matrix` then `cargo test -p quarry-core zdr_matrix` 6 passed; `zdr-contract-sync.test.sh` 52 passed, 0 failed.
- Mutation checks on a COPY of the workspace (Go natively; Rust through qtest with `QROOT=<copy>`, own volume `c1-mut-target`, removed afterwards; the copy is deleted). Each mutation made the named test fail and the restore made it pass:
  - record call without `Zdr` (`activities.go`): `TestCheckChangeSendsStampedZdr` fails.
  - workflow without `Zdr` in the `CheckChangeInput`: `TestChangeMonitorWFPassesZdr` fails.
  - `workflowFor` without `Zdr`: `TestWorkflowForCarriesZdr`, `TestCreateScheduleStampsZdr`, `TestListScheduleCarriesStampToReconciler` fail.
  - source monitor ignoring the posture (`cycle23.go`): `TestSourceMonitorCreateStampsZdr` fails.
  - schedule stamp hard-coded to `false` (edge): `observe_mode_stamps_the_zdr_posture_on_the_forwarded_schedule` and `a_client_body_cannot_lower_the_stamp` fail.
  - source stamp hard-coded to `false` (edge): `observe_mode_stamps_a_source_monitor` fails.
  - `record_internal` ignoring the declared zdr (`req.zdr.unwrap_or(false)` replaced by `false`): `enforced_internal_change_record_refuses_a_declared_zdr_body_only` and `observe_mode_serves_a_zdr_org_as_before_and_logs_one_content_free_event_per_action` (zdr_refusal_modes) and `record_trusts_the_stamp_not_the_current_posture` and `observe_internal_record_with_declared_zdr_logs_would_refuse_and_is_not_403` (zdr_posture_stamp) fail. This answers the ledger's "review-round assertions were not seen to fire against a broken handler" for the record route.
  - Trap: restoring a mutated file with `mv` brings back its OLD mtime, so cargo does not rebuild and the "restored" run still ran the mutant (it failed 2 tests). A `touch` after the restore fixed it (7 and 8 passed).

## Not verified
- No live stack, no deployed image, no real ZDR-org token. Nothing was deployed and no live container was touched.
- The deploy order (control, orchestrator, edge) is argued from the decoders, not exercised across a rolling deploy.
- A Temporal replay of an in-flight fire against the new `ChangeMonitorInput` was not run (the change is one optional field; the testsuite decodes legacy args).

## Traps found
- `efebf3e0` ALSO CONTAINS ANOTHER UNIT'S UNCOMMITTED RUNBOOKS HUNKS: the `QUARRY_BLOCK_ESCALATION` table row (default `legacy` to `honest`), the "Where the default stands" paragraph and a "Page cache" paragraph (6 changed lines). They appeared in the working copy between my `git diff` check and the commit (another agent was editing the append-only doc). They are WS3.I's text; its code commit may now arrive after the doc that already says `honest`. The WS3.I code flip landed right after as `89097598`, so doc and code now agree.
- A hung `grep -rln` over the whole worktree timed out the shell (120 s); use `git grep`.
- CRLF: the Go files and the Rust sources I edited are CRLF; all edits went through `lib-edit.js`. New files are LF (git warns on add, harmless).

## Deviations from the design note
- `refuse_for_claims` in `create_schedule` was replaced by `refuse(.., claims.effective_zdr(false), ..)` so the stamp and the refusal use one value (the note asks for exactly that). `refuse_for_claims` is still used by other routes.
- Matrix: besides the three rows the note lists, the `change_history`, `change_snapshot` and `profiles` basis texts were rewritten too, because "ZDR is consulted nowhere on these routes" has been false since stage A. No status changed.
- The new edge tests are in their own binary (`zdr_posture_stamp.rs`), not in `zdr_refusal_modes.rs`: that file counts every `zdr.would_refuse` its process emits, so an observe test added there could break its "none yet" check depending on order.

## Left for the owner / others
- ADR-4 stage B: flipping the flag default, `Enforced` on the five rows, inverting `zdr_gap_rows.rs`, the customer notice.
- The visible change with the flag off: a ZDR org's monitor created after the deploy has its edge NATS change event and webhook suppressed (existing fail-closed rule, now reachable because the stamp arrives). Accept or veto before deploy.
- The later-ZDR-org gap: operator remedy `UPDATE schedules SET zdr = true WHERE org_id = '<org>'`; permanent fix is the fire-time lookup X-9 (not approved).
- Service-principal schedules are stamped false; the legacy non-preset create is not stamped; `ChangeMonitorWF` still emits `run_started` and `change_detected` with the URL.
- WS15's `017_schedules_definition` must carry `zdr` if it rebuilds `schedules` (noted in the R-2 row).

## Round 4 and 5 (review fixes, 2026-10-21)

### Behaviour change that round 4 introduced (was not in the first hand-over)
- `POST /v1/schedules` (with a `preset`) and `POST /v1/sources` (`monitor: true`) now take `zdr` from the body: the stamp and the refusal both use `claims.effective_zdr(req.zdr.unwrap_or(false))`. A body's `zdr: true` raises the stamp for a user of a standard organization (the floor of a ZDR user token still cannot be lowered). The legacy non-preset create forwards no `zdr` (`skip_serializing`), pinned by `a_legacy_cron_create_never_forwards_a_declared_zdr`. Deviation from the design note, which had "no body field".
- Visible change with the flag off: such a standard-organization monitor with a declared `zdr: true` also gets the fail-closed record behaviour (NATS change event and webhook suppressed); with the flag on the create is refused. RUNBOOKS "Visible change" now says so.

### Round 5 fixes
- run_page fetch now carries the stamp (`RunPageInput.Zdr`), so a stamped-ZDR monitor's fetch persists no page artifact, event or local-index write-back before the record refusal. Tests: `TestCheckChangeSendsStampedZdrOnTheRunPageFetch`, `TestRunPageOmitsZdrWhenUnstamped`. RUNBOOKS gap (e) updated.
- Reconciler link pinned: `internal/schedules/schedules_zdr_test.go` (create and update).
- Matrix basis: service-principal clause removed from `schedules` and `change_record_internal`; golden, `.sha256`, `ZDR_MATRIX.md` and the digest pin in `zdr-contract-sync.test.sh` regenerated together (`76500d48...`).
- Service principal "gap" was wrong (403 before the handler): RUNBOOKS (b) says not a gap; history notes list the commit messages that carried the wrong sentence (2a7759f6, 06959092, 8b3c108b, efebf3e0), and 8b3c108b's false sync-script claim (red on the cross-plane pin; 7 pending rows).
- CI: new job `quarry-control-postgres` (postgres:16-alpine, QUARRY_REQUIRE_DB=1) and `testDSN` in pg_test.go, so a regression in the zdr INSERT/SELECT/Scan cannot merge green.

### Traps
- Docker was down (npipe missing, `docker version` hangs) during round 5: the pre-commit hook needs docker for gitleaks and prints "found a secret" when it cannot connect (it is a connect failure, not a finding).
- quarry-core builds and tests natively on Windows (`CARGO_TARGET_DIR=C:/c1-native-target`, a path without 8.3 short names; a `~` path fails linking with LNK1104).
- RUNBOOKS.md carries WS3.I hunks (FallbackDriver rows, Known limit) in the working tree: commit only the WS4.H hunk (filtered patch, `git apply --cached`).

## Round 7 (review findings, 2026-10-23)
Commits: `78c9e4ac` (edge: fan-out moved to change_webhook.rs + 2 tests), `8e74b8e7` (CI job quarry-go-zdr-stamp-chain), `0a2fc5b4` (RUNBOOKS, own hunks only).
- Findings 1-4 are about false claims in already-committed messages. History is not amended. At HEAD: `scripts/tests/zdr-contract-sync.test.sh` gives 65 passed, 0 failed, 7 pending; RUNBOOKS history notes now cover all four (the test-count note (4) is new); the service-principal and escalation text is correct.
- Finding 5 (CI): new required job runs the Go packages that carry the chain. Needs branch protection to bite (not visible in the repo).
- Finding 6: `emit_change_side_effects` was behind cfg(postgres-queue) in change_routes.rs; moved to change_webhook.rs (lib ungated, bin only under the feature) so `zdr_posture_stamp::a_declared_zdr_change_record_suppresses_the_change_event_and_the_webhook` can call it. Mutation (guard disabled) fails exactly that test.
- Finding 7: `zdr_posture_stamp::run_page_with_a_zdr_stamp_persists_no_artifact_event_or_index_write` (artifact files, durable events, local-index writes). Mutation (run_page ignores zdr) fails exactly that test.
- zdr_posture_stamp: 13 tests. Full edge + core runs: all ok except the untracked tests/agent_step_timeout.rs (other unit; fails alone too).
- Trap: the bin target re-declares the modules with `mod x;` in main.rs, so un-gating a function in a shared file can break the bin build when a module it uses is gated there; moving the function into the module that is gated the same way avoids editing main.rs.
- Volume c1-mut7-target removed; the copy deleted.

## Round 8 (review 2026-10-23)
Only finding: no defect, note about fixed 400 ms sleeps. Commit `b8b7495b`: the run_page positive controls now poll (wait_for, up to 15 s) instead of settle(); the absence assertions keep settle() (cannot be polled; positive controls keep them non-vacuous). zdr_posture_stamp 13 passed (qtest). agent_step_timeout.rs is another unit's and was not run.
