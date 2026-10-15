# CI baseline, October 2026

Purpose: record, per workflow, whether it runs on the integration branch and what its first recorded run did, so that a pre-existing red is not mistaken for a regression.

Integration branch: `fix/ui-premium-restore` (decision D1 of the build plan, `docs/plans/model-plane-chat-build-plan-2026-10-14.md`, WS12 S1). The GitHub default branch is `dev`; changing it is a repository-owner action (D1 part c) and is requested, not done, by this change.

Before this change only `sse-taxonomy` ran on pushes in this repository (3 runs, all red: the taxonomy at HEAD was stale until the live ZDR wave landed). `verevonv3-ci`, `buf`, `quarry-v2-ci`, `verevonv3-security` filtered on `main`/`develop`, and `contract-goldens` and `model-plane-rust-ci` filtered their `push` trigger on `main`.

| Workflow | Triggers after this change | First run on the integration branch | Pre-existing reds and owner |
|---|---|---|---|
| `verevonv3-ci` | push and pull_request on `main`, `fix/ui-premium-restore` | not determined: no run yet | per project notes: typecheck needs `@quarry/client`; about 12 vitest files failed at an earlier date; not re-verified (owner: Frontend Plane) |
| `buf` | same, plus the ratchet script path | to be recorded | none expected: baseline and lint ratchet were produced from the committed protos |
| `sse-taxonomy` | unchanged | to be recorded | should be green once the ZDR wave is committed (`gen-sse-event-taxonomy.py --check` exits 0 on the landed tree) |
| `model-plane-rust-ci` | push now also on the integration branch | to be recorded | per-crate `--lib` is green locally; two cargo targets fail on the Windows host only |
| `contract-goldens` | push now also on the integration branch | to be recorded | `zdr-contract-sync.test.sh` fails locally on other planes' vendored copies not yet landed (Data Plane, Quarry) and on CRLF checkouts |
| `quarry-v2-ci`, `verevonv3-security` | filters extended to the integration branch | to be recorded | not determined |

How to fill the "first run" column: push the branch, then `gh run list --branch fix/ui-premium-restore --limit 30` and copy each workflow's conclusion and URL here. Each new gate starts with `continue-on-error: true` for its first three runs, then blocks.
