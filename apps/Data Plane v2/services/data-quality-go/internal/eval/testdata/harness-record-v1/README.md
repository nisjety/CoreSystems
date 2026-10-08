# harness-record-v1

The request body of `POST /v1/evals/results` (data-quality-go, task CR-T14): one summary of a retrieval-quality
run, which the service stores in `quality_eval_runs` so that a regression is a query and not an archaeology.

| file | meaning |
|---|---|
| `record.cell.golden.json` | the record of one gated cell (here the contextual cell, 10 queries, gate verdict `pass`) |
| `record.promotion.golden.json` | the record of one promotion verdict (candidate `ctx-v2` against `ctx-v1`, verdict `promote`) |
| `invalid.*.json` | records the service must refuse, each for one reason: the strategy is not `harness:<cell>`; a metric outside 0 to 1; `details` present; the by-class counts do not add up; an unknown field; a verdict that does not exist |

## Who owns what

* **Producer:** `apps/Data Plane v2/scripts/eval-gate.py` (`--records-out`). Its tests
  (`scripts/tests/test_eval_gate_promotion.py`) build the two valid records from fixed inputs and compare them with these
  files; `UPDATE_HARNESS_RECORD_GOLDEN=1` regenerates them. The `invalid.*` files are written by hand, by mutating a valid record.
* **Consumer:** `data-quality-go` (`internal/eval/record_test.go`, `eval.ValidateRecord`). It vendors this directory
  byte for byte under `services/data-quality-go/internal/eval/testdata/harness-record-v1/` with the same `SHA256SUMS`.
  Both sides must accept the valid records and refuse every `invalid.*` file: that is how the two implementations of
  the rules are kept equal.
* **Drift check:** `scripts/check-eval-contract.sh` (also run in CI). Edit a golden and its pin together, in both
  directories, and say why in the commit.

## The rules in one place

`strategy` is `harness:<cell>` and equals `scorecard.strategy` and `harness:<harness.cell>`; the metrics are numbers
from 0 to 1; `golden_queries` is from 1 to `queries_run` and equals the sum of `by_class[*].queries_run`; `details` is
absent (the per-query rows stay in the run's artifacts); `harness.schema` is `harness-record-v1`; `harness.kind` is
`cell` or `promotion`; `harness.gate.verdict` is one of `pass`, `fail`, `unbaselined`, `promote`, `hold`, with at most 20
reasons of at most 500 characters; unknown fields are refused at every level.
