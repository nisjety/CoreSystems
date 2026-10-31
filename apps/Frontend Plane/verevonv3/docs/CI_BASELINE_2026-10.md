# CI baseline, October 2026

Purpose: record, per workflow, whether it runs on the integration branch and what its first recorded run did, so that a pre-existing red is not mistaken for a regression.

Integration branch: `fix/ui-premium-restore` (decision D1 of the build plan, `docs/plans/model-plane-chat-build-plan-2026-10-14.md`, WS12 S1). The GitHub default branch is `dev`; changing it is a repository-owner action (D1 part c) and is requested, not done, by this change.

As of 2026-10-21, `sse-taxonomy` is the only workflow that has ever run in this repository. Its three recorded runs are all red: `fix/ui-premium-restore` (run 35846527491, 2026-09-23), `codex/replace-arbeit-with-manrope` (run 36708647737, 2026-09-30) and `feat/product-scene-films-02-03` (run 38134106293, 2026-10-11). Before that commit, `verevonv3-ci`, `quarry-v2-ci` and `verevonv3-security` filtered both triggers on `main` only, `buf` on `main` and `develop`, and `contract-goldens` and `model-plane-rust-ci` filtered their `push` trigger on `main`.

## Evidence (read-only, 2026-10-21)

- `gh repo view --json defaultBranchRef` returns `dev`.
- Origin has five branches: `codex/replace-arbeit-with-manrope`, `codex/verevon-web-motion`, `dev`, `feat/product-scene-films-02-03`, `fix/ui-premium-restore`. It has no `main` and no `develop`, so those filter entries match nothing until the branches exist.
- `gh run list --limit 40` returns the three `SSE taxonomy freshness` runs above and no others. `gh run list --branch fix/ui-premium-restore` returns only run 35846527491.

## Trigger policy

The branch filters on `buf`, `verevonv3-ci` and `quarry-v2-ci` are the list from Appendix B row K1-23 of the plan: `main`, `develop` and `fix/ui-premium-restore` on both triggers. DEC-39 D1 asks for the `pull_request` branch filter to be dropped instead. This change does not drop it for these three files, because the Appendix B row is the binding amendment for them. `model-plane-rust-ci`, `contract-goldens`, `sse-taxonomy` and `repo-gates` already have no `pull_request` branch filter, so the repository runs two policies until the owner chooses one. Until then, a PR whose base is a feature branch gets no run from the three filtered workflows.

`verevonv3-security` still lists `main` and `fix/ui-premium-restore` only. Its `develop` entry is pending because that workflow file is outside the slice that owns this change.

## First run per workflow

| Workflow | Triggers now | First run on the integration branch | Pre-existing reds and owner |
|---|---|---|---|
| `verevonv3-ci` | push and pull_request on `main`, `develop`, `fix/ui-premium-restore` | none as of 2026-10-21; record after the first push | Owner: Frontend Plane. The `spa-verify` job generates `@quarry/client` before typecheck, so the local gap in the project notes is not expected in CI. About 12 vitest files failed at an earlier date; not re-verified |
| `buf` | push and pull_request on `main`, `develop`, `fix/ui-premium-restore`; lint ratchet and breaking-change proof in one job | none as of 2026-10-21; record after the first push | Owner: Model Plane (proto). None expected: the baseline image and the lint ratchet were produced from the committed protos |
| `sse-taxonomy` | push and pull_request on any branch, path-scoped (unchanged) | run 35846527491, failure, 2026-09-23 (`fix/ui-premium-restore`) | Owner: Model Plane. Red on the commit it ran on. The plan records the taxonomy as stale at HEAD and `gen-sse-event-taxonomy.py --check` as exit 0 on the landed live tree, so WS01-S0 is expected to turn it green |
| `model-plane-rust-ci` | pull_request on any base, path-scoped; push on `main`, `fix/ui-premium-restore` | none as of 2026-10-21; record after the first push | Owner: Model Plane. Per-crate `--lib` tests were green locally per the project notes; two non-lib cargo targets fail on the Windows host only |
| `contract-goldens` | pull_request on any base, path-scoped; push on `main`, `fix/ui-premium-restore` | none as of 2026-10-21; record after the first push | Owner: Model Plane (sync script). `zdr-contract-sync.test.sh` fails locally on other planes' vendored copies not yet landed (Data Plane, Quarry) and on CRLF checkouts |
| `quarry-v2-ci` | push and pull_request on `main`, `develop`, `fix/ui-premium-restore` | none as of 2026-10-21; record after the first push | Owner: Ingestion Plane. `govulncheck` and the coverage jobs are report-only (`continue-on-error`); the workflow header records Go standard-library CVEs fixed only in go1.26.6 (2026-09-08/09 notes, not re-verified) |
| `verevonv3-security` | push and pull_request on `main`, `fix/ui-premium-restore`; `develop` pending (file outside this slice) | none as of 2026-10-21; record after the first push | Owner: Frontend Plane. Not determined: the workflow has no run in the Actions history |
| `repo-gates` | pull_request on any base; push on `main`, `fix/ui-premium-restore`; one job, `buf-lint-ratchet-selftest` | none as of 2026-10-21; record after the first push | Owner: none yet. DEC-39 D2 asks for a matrix row, CODEOWNERS and one named maintainer for `scripts/gates/` and this file; requested, not done |

How to fill the first-run column: push the branch, then `gh run list --branch fix/ui-premium-restore --limit 30` and copy each workflow's conclusion and URL here. Each new gate starts with `continue-on-error: true` for its first three runs, then blocks.
