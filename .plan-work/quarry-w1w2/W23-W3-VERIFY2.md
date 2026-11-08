# W23 W3 verify 2: CW-2 / CW-3 (2026-11-08)

Worktree `innh-lane-integ`, branch `w23/integ`, HEAD 7d64ea5a5. Targeted only: no full lanes, no writer files edited.
Build: `CARGO_TARGET_DIR=C:\qv2t`, `-j 2`, one binary (`quarry-runtime` lib tests). The compile waited on a shared lock and took 21m35s.

## Counts

| Check | Result |
|---|---|
| `cargo test -p quarry-runtime --lib -- a_redacting_run_refuses otp_read_denial capture screenshot` | 29 passed, 0 failed (1362 filtered) |
| Earlier filter (`redacting_run_refuses screenshot redact`), same binary, before the working-tree edits grew | 78 passed, 0 failed |
| New CW-2 test `observation::tests::a_redacting_run_refuses_the_screenshot_and_stores_nothing` | pass |
| `observation_redaction::otp_read_denial_tests` | 7 of 7 pass |
| Flag defaults | all off (see below) |

The test binary was built from the working tree (commit plus uncommitted edits in progress), not from the commit alone.

## CW-2 (committed 7d64ea5a5, plus uncommitted work)

Committed part: `AgentAction::Screenshot` and `AgentAction::Pdf` in `observation.rs` return `pii_redaction_failed` / `capture_not_redactable` when `pii_redaction_settings().applies(false, org)`. Nothing is CAS-stored, and the test asserts `list()` is empty. The helpers in `pipeline.rs` were widened to `pub(crate)`, which is minimal.

Uncommitted (writer still working, not part of this verdict): `attach.rs`, `step.rs`, `observation.rs`, `observation_redaction.rs` and `otp_source.rs` hold +386/-11. This is the OTP read-denial work: `CredentialDeliveredReads`, `otp_reads().check_before` / `settle`, and `OTP_READ_DENIAL` on `policy_denials`. Its 7 unit tests pass in the working tree, but nothing is committed yet. VF-1 for this part has to be re-run after the writer commits.

Findings:
1. `applies(false, ..)` ignores a per-run `redact_pii` request on the agent path. Only the org default (`QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS`) triggers the refusal. A run that asks for redaction on an org outside the default still gets a stored screenshot or PDF. Low while the stage is off by default, but the ORCH CW-2 wording ("no PNG/CAS render while redacting") covers it. Needs a decision: thread the request flag, or document it.
2. The new test imports `TEST_PII_REDACTION` as unused (`unused_imports` warning in the lib test build). Cosmetic. I did not edit the file.
3. The Pdf arm has no test. Only Screenshot is covered.

## CW-3: not started

`lab/evals/src/provider_matrix.rs:316` still reads `format!("{}/content?token={}", ..., token)`, so the Browserless token is in the request URL. `.github/workflows/quarry-v2-eval-tier-c.yml` lines 42-43 still describe that behaviour. No CW-3 commit or working-tree change exists on `w23/integ`; `lane-c` has nothing for it either (HEAD 6664a3dba, clean). Nothing to test, so no verdict.

ORCH names the wrong file: it says `lab/evals/src/release_matrix.rs`, but the Browserless token lives in `provider_matrix.rs`. `release_matrix.rs` has no token handling. Fix the ORCH row.

## Flag defaults grep

- `QUARRY_EDGE__PII_REDACTION`: `PiiRedactionMode` `#[default] Off`. Parse: empty, `off`, `false` or `0` is Off. Unknown values are an error.
- `QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS`: `PiiOrgDefault` `#[default] None`.
- Neither variable appears in any compose or yml under `apps/Ingestion Plane`. The only redaction setting in compose is `QUARRY_EDGE__REDACTION_MODE ... :-enforce`, which is the existing OTP/secret masking.
- The CW-2 OTP work adds no env var; a grep of the touched files for `env::var` found none (grep was partial).

## Blockers

1. CW-3 is not built. HIGH, the token stays in the URL and is logged.
2. CW-2 OTP read denial is uncommitted. It cannot be called landed until the writer commits and the tests are re-run.
3. Per-run `redact_pii` is not honoured on the agent screenshot/PDF path (finding 1). Needs a decision.
