# eval-evidence-v1

The response bodies that the retrieval-quality harness gate (`apps/Data Plane v2/scripts/eval-gate.py`,
task CR-T14) reads from this service, pinned so that neither side can drift unseen.

| file | produced by | meaning |
|---|---|---|
| `coverage.contextual-ready.golden.json` | `GET /v1/quality/context-coverage`, token of `org-corpus-contextual` | every eligible unit has a verified context (100 percent) |
| `coverage.contextual-below-threshold.golden.json` | the same | 1,100 of 1,164 ready (94.50 percent): the gate must fail |
| `coverage.baseline-off.golden.json` | the same, token of `org-corpus-baseline` | the raw baseline organization: policy `off`, every unit `skipped_policy`, status `skipped_policy` |
| `gates.in-sync.golden.json` | `GET /v1/quality/gates` (200) | both release gates pass |
| `gates.parity-unavailable.golden.json` | `GET /v1/quality/gates` (412) | `sparse_index_in_sync` is `unavailable`: unread evidence, never a pass |

## Who owns what

* **Producer:** `data-quality-go` (`internal/handler/eval_evidence_contract_test.go`). The test drives the real
  route table, the real handler and the real rules (`gates.EvaluateContextCoverage`,
  `gates.EvaluateSparseParity`) and compares what they produce with these files. The files are generated, not written by
  hand: `UPDATE_EVAL_EVIDENCE_GOLDEN=1 go test ./internal/handler -run EvalEvidence`.
* **Consumer:** `scripts/eval-gate.py`. It vendors this directory byte for byte under
  `apps/Data Plane v2/scripts/tests/fixtures/eval-evidence-v1/` and its tests read the files from there.
* **Drift check:** `scripts/check-eval-contract.sh` (also run in CI). Edit a golden and its pin together, in both
  directories, and say why in the commit.

## What the consumer relies on

* `counts` carries `eligible`, `ready`, `legacy`, `pending`, `skipped_zdr`, `skipped_policy`, `skipped_budget`,
  `failed_transient`, `failed_permanent`, `status_unknown`, `vector_incoherent`, `ready_unverified` (all integers).
* `evaluation.status` is `passed`, `failed` or `skipped_policy`; `ready_trusted` is `ready - ready_unverified -
  vector_incoherent`; `denominator` is `eligible - skipped_zdr`. The consumer recomputes the percentage from the counts and
  refuses a report whose own status disagrees with its counts.
* A gate whose evidence could not be read has `status: "unavailable"` and `passed: false`.
