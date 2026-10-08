"""Tests of the ctx-v2 promotion gate and of the persisted records (scripts/eval-gate.py, CR-T14).

The promotion gate decides whether a prompt version, a model or a rerank input may become the default:

  * non-inferiority with the reranker OFF (noise about 0.008): nDCG and MRR no worse than the current
    default by more than 0.01, on the same queries;
  * coverage of the candidate's own contextual state at least 99 percent, and a state that really is the
    one that was declared (the units carry the candidate's prompt version and model);
  * the exact-identifier class unchanged: no query lost, none returning nothing;
  * confirmation with the reranker ON, three runs each (noise about +/-0.04): the mean no worse than the
    default's by more than 0.045, and all three runs complete.

Anything less than all of it is HOLD (exit 1), including evidence that is simply absent.
"""
from __future__ import annotations

import contextlib
import copy
import io
import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path

from _scripts import FIXTURES, load
from test_eval_gate import entry, fixture, make_rows

gate = load("eval-gate.py")

N = 40
LEX_N = 23


def coverage_report(org="org-corpus-contextual", eligible=1164, ready=1164) -> dict:
    report = fixture("coverage.contextual-ready.golden.json")
    report["org_id"] = org
    report["counts"].update(eligible=eligible, ready=ready, pending=eligible - ready)
    denominator = eligible
    pct = 100.0 * ready / denominator
    report["evaluation"].update(
        status="passed" if ready * 100 >= denominator * 99 else "failed",
        ready_trusted=ready, denominator=denominator, coverage_percent=pct)
    return report


def state_for(variant: str, *, prompt_version: str, model: str = "gpt-4o-mini", observed_version: str | None = None,
              units: int = 1164, rerank_input: str | None = None) -> dict:
    declared = {"prompt_version": prompt_version, "model": model, "rerank_input_include_context": None}
    observed = {
        "policy": {"contextual_retrieval": "inherit", "context_prompt_version": prompt_version},
        "provenance": [{"prompt_version": observed_version or prompt_version, "model": model,
                        "context_status": "ready", "units": units}],
        "retrieval": {"rerank_input_include_context": rerank_input or "0"},
    }
    return {"schema": "eval-state-v1", "variant": variant, "declared": declared, "observed": observed}


class Bench:
    """A results directory holding the evidence of variants, written the way the variant runner writes it."""

    def __init__(self, test: unittest.TestCase):
        self.dir = Path(tempfile.mkdtemp(prefix="evalpromo-"))
        test.addCleanup(shutil.rmtree, self.dir, True)

    def variant(self, name: str, *, prompt_version: str, off=(1,), on=((1,), (1,), (1,)), lex=(1,), coverage=None,
                state=None, n: int = N, on_n: int | None = None) -> None:
        def put(filename: str, body) -> None:
            (self.dir / filename).write_text(json.dumps(body), encoding="utf-8")

        put(f"cell-{name}-rr-off.json", make_rows(n, list(off)))
        for index, ranks in enumerate(on, start=1):
            put(f"cell-{name}-rr-on-{index}.json", make_rows(on_n or n, list(ranks)))
        put(f"cell-{name}-lexical.json", make_rows(LEX_N, list(lex), prefix="lex"))
        put(f"coverage-{name}.json", coverage or coverage_report())
        put(f"state-{name}.json", state or state_for(name, prompt_version=prompt_version))

    def run(self, *args: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            try:
                code = gate.main([str(self.dir), *args])
            except SystemExit as exit_:
                code = exit_.code if isinstance(exit_.code, int) else 1
                if isinstance(exit_.code, str):
                    out.write(exit_.code)
        return code, out.getvalue()

    def verdict(self, candidate="ctx-v2", default="ctx-v1") -> dict:
        return json.loads((self.dir / f"promotion-{candidate}-vs-{default}.json").read_text(encoding="utf-8"))

    def promote(self, candidate="ctx-v2", default="ctx-v1") -> tuple[int, str]:
        return self.run("--promote", candidate, "--against", default)


def healthy(test: unittest.TestCase) -> Bench:
    bench = Bench(test)
    bench.variant("ctx-v1", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3)
    bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3)
    return bench


class Promotion(unittest.TestCase):
    def test_an_equal_candidate_with_complete_evidence_is_promoted(self):
        bench = healthy(self)
        code, out = bench.promote()
        self.assertEqual(code, 0, out)
        self.assertIn("PROMOTE", out)
        verdict = bench.verdict()
        self.assertEqual(verdict["verdict"], "promote")
        self.assertEqual(verdict["reasons"], [])
        self.assertTrue(all(check["ok"] for check in verdict["checks"]), verdict["checks"])
        names = {check["name"] for check in verdict["checks"]}
        self.assertTrue({"non_inferiority_rerank_off", "coverage", "state", "exact_identifier_class", "rerank_on_confirmation"} <= names, names)

    def test_a_better_candidate_is_promoted(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 40, on=[[1] * 40] * 3)
        self.assertEqual(bench.promote()[0], 0)

    def test_a_worse_candidate_with_the_reranker_off_is_held(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[3] * 40)  # nDCG 0.5, MRR 0.333 against 0.91 and 0.875
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertIn("HOLD", out)
        self.assertEqual(bench.verdict()["verdict"], "hold")
        self.assertTrue(any("non-inferiority" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_the_two_boundaries_are_inclusive_at_minus_0_01_and_not_beyond(self):
        bench = healthy(self)
        candidate = gate.load_variant(str(bench.dir), "ctx-v2")
        default = gate.load_variant(str(bench.dir), "ctx-v1")
        for metric in ("ndcg_at_10", "mrr"):
            for drop, ok in ((0.01, True), (0.0101, False)):
                with self.subTest(metric=metric, drop=drop):
                    mutated = copy.deepcopy(candidate)
                    mutated["rr_off"]["score"][metric] = default["rr_off"]["score"][metric] - drop
                    verdict = gate.promotion_verdict(mutated, default)
                    self.assertEqual(verdict["verdict"], "promote" if ok else "hold", verdict["reasons"])

    def test_the_rerank_on_confirmation_tolerates_noise_and_rejects_a_collapse(self):
        bench = healthy(self)
        candidate = gate.load_variant(str(bench.dir), "ctx-v2")
        default = gate.load_variant(str(bench.dir), "ctx-v1")
        base_ndcg = sum(run["score"]["ndcg_at_10"] for run in default["rr_on"]) / 3
        for drop, ok in ((0.045, True), (0.046, False)):
            with self.subTest(drop=drop):
                mutated = copy.deepcopy(candidate)
                for run in mutated["rr_on"]:
                    run["score"]["ndcg_at_10"] = base_ndcg - drop
                verdict = gate.promotion_verdict(mutated, default)
                self.assertEqual(verdict["verdict"], "promote" if ok else "hold", verdict["reasons"])

    def test_a_collapse_with_the_reranker_on_is_held_by_the_cli(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[3] * 40] * 3)
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("rerank-on" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_fewer_than_three_rerank_on_runs_is_held_as_incomplete(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 2)
        (bench.dir / "cell-ctx-v2-rr-on-3.json").unlink()  # healthy() wrote three; the third run never finished
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("3 rerank-on runs" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_a_rerank_on_run_that_did_not_finish_is_held(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3)
        # rewrite run 2 with half the queries: the reranker's 429 storm killed it
        (bench.dir / "cell-ctx-v2-rr-on-2.json").write_text(json.dumps(make_rows(20, [1])), encoding="utf-8")
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("did not complete" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_coverage_below_99_percent_holds_even_when_every_metric_is_equal(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      coverage=coverage_report(ready=1100))
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("coverage" in reason and "94.50" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_coverage_of_exactly_99_percent_is_enough(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      coverage=coverage_report(eligible=1000, ready=990), state=state_for("ctx-v2", prompt_version="ctx-v2", units=990))
        code, out = bench.promote()
        self.assertEqual(code, 0, out)

    def test_a_candidate_whose_state_is_not_the_declared_one_is_held(self):
        # The org still holds ctx-v1 contexts: measuring it would measure the DEFAULT and call it the candidate.
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      state=state_for("ctx-v2", prompt_version="ctx-v2", observed_version="ctx-v1"))
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("state" in reason and "ctx-v1" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_a_model_candidate_is_checked_against_the_models_the_units_carry(self):
        bench = healthy(self)
        bench.variant("model-mini", prompt_version="ctx-v1",
                      off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      state={**state_for("model-mini", prompt_version="ctx-v1", model="gpt-5-mini"),
                             "observed": state_for("model-mini", prompt_version="ctx-v1", model="gpt-4o-mini")["observed"]})
        code, out = bench.promote("model-mini", "ctx-v1")
        self.assertEqual(code, 1, out)
        self.assertTrue(any("gpt-4o-mini" in reason for reason in bench.verdict("model-mini")["reasons"]))

    def test_a_rerank_input_candidate_is_checked_against_the_flag_the_retrieval_engine_runs_with(self):
        bench = healthy(self)
        declared = state_for("rerank-ctx", prompt_version="ctx-v1")
        declared["declared"]["rerank_input_include_context"] = True
        declared["observed"]["retrieval"]["rerank_input_include_context"] = "0"
        bench.variant("rerank-ctx", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, state=declared)
        code, out = bench.promote("rerank-ctx", "ctx-v1")
        self.assertEqual(code, 1, out)
        self.assertTrue(any("rerank_input_include_context" in reason for reason in bench.verdict("rerank-ctx")["reasons"]))
        declared["observed"]["retrieval"]["rerank_input_include_context"] = "1"
        bench.variant("rerank-ctx", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, state=declared)
        self.assertEqual(bench.promote("rerank-ctx", "ctx-v1")[0], 0)

    def test_a_policy_that_is_off_cannot_produce_a_candidate(self):
        bench = healthy(self)
        off = state_for("ctx-v2", prompt_version="ctx-v2")
        off["observed"]["policy"]["contextual_retrieval"] = "off"
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, state=off)
        code, out = bench.promote()
        self.assertEqual(code, 1, out)

    def test_losing_one_exact_identifier_query_holds(self):
        bench = healthy(self)
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      lex=[1] * 22 + [None])
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("exact-identifier" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_a_lost_identifier_query_holds_even_when_the_class_as_a_whole_scores_better(self):
        # No query is allowed to be lost: a better average over the others does not buy it back.
        bench = healthy(self)
        bench.variant("ctx-v1", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, lex=[2])
        bench.variant("ctx-v2", prompt_version="ctx-v2", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, lex=[1] * 22 + [None])
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("an identifier query was lost" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_the_exact_identifier_class_may_improve(self):
        bench = healthy(self)
        bench.variant("ctx-v1", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3, lex=[1] * 22 + [None])
        self.assertEqual(bench.promote()[0], 0)

    def test_queries_that_are_not_the_same_hold(self):
        bench = healthy(self)
        (bench.dir / "cell-ctx-v2-rr-off.json").write_text(json.dumps(make_rows(N, [1], prefix="other")), encoding="utf-8")
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        self.assertTrue(any("same queries" in reason for reason in bench.verdict()["reasons"]), bench.verdict()["reasons"])

    def test_absent_evidence_holds_and_says_what_is_absent(self):
        bench = healthy(self)
        (bench.dir / "cell-ctx-v2-rr-off.json").unlink()
        (bench.dir / "state-ctx-v2.json").unlink()
        (bench.dir / "coverage-ctx-v2.json").unlink()
        code, out = bench.promote()
        self.assertEqual(code, 1, out)
        reasons = " | ".join(bench.verdict()["reasons"])
        for needle in ("cell-ctx-v2-rr-off.json", "state-ctx-v2.json", "coverage-ctx-v2.json"):
            self.assertIn(needle, reasons)

    def test_a_default_that_is_itself_unhealthy_cannot_be_compared_against(self):
        bench = healthy(self)
        bench.variant("ctx-v1", prompt_version="ctx-v1", off=[1] * 30 + [2] * 10, on=[[1] * 30 + [2] * 10] * 3,
                      coverage=coverage_report(ready=1000))
        code, out = bench.promote()
        self.assertEqual(code, 1, out)

    def test_the_verdict_is_deterministic(self):
        bench = healthy(self)
        bench.promote()
        first = (bench.dir / "promotion-ctx-v2-vs-ctx-v1.json").read_bytes()
        bench.promote()
        self.assertEqual(first, (bench.dir / "promotion-ctx-v2-vs-ctx-v1.json").read_bytes())

    def test_promote_without_against_is_a_usage_error(self):
        bench = healthy(self)
        code, out = bench.run("--promote", "ctx-v2")
        self.assertEqual(code, 2, out)
        self.assertIn("--against", out)

    def test_a_variant_cannot_be_promoted_over_itself(self):
        # Judged against itself a variant is non-inferior on every count, so a typo in --against would read as a PROMOTE.
        bench = healthy(self)
        code, out = bench.run("--promote", "ctx-v1", "--against", "ctx-v1")
        self.assertEqual(code, 2, out)
        self.assertIn("same variant", out)
        self.assertFalse((bench.dir / "promotion-ctx-v1-vs-ctx-v1.json").exists())

    def test_a_variant_name_cannot_escape_the_results_directory(self):
        bench = healthy(self)
        code, out = bench.run("--promote", "../ctx-v2", "--against", "ctx-v1")
        self.assertEqual(code, 2, out)


RECORDS = FIXTURES / "harness-record-v1"
MINI_MANIFEST = FIXTURES / "corpus-manifest-mini.json"
RUN_ID = "20261007T030000Z-test"
UPDATE = bool(os.environ.get("UPDATE_HARNESS_RECORD_GOLDEN"))


def run_records(test: unittest.TestCase, baseline_ranks, current_ranks, *extra: str) -> tuple[int, str, Path]:
    work = Path(tempfile.mkdtemp(prefix="evalrec-"))
    test.addCleanup(shutil.rmtree, work, True)
    results, out_dir = work / "results", work / "records"
    results.mkdir()
    (work / "baseline.json").write_text(json.dumps({"contextual": entry(list(baseline_ranks), 10)}), encoding="utf-8")
    (results / "st-rr-on-contextual.json").write_text(json.dumps(make_rows(10, current_ranks)), encoding="utf-8")
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer), contextlib.redirect_stderr(buffer):
        code = gate.main([str(results), "--baseline", str(work / "baseline.json"), "--records-out", str(out_dir),
                          "--run-id", RUN_ID, "--corpus-manifest", str(MINI_MANIFEST), *extra])
    return code, buffer.getvalue(), out_dir


class Records(unittest.TestCase):
    def golden(self, name: str, produced: dict) -> None:
        path = RECORDS / name
        if UPDATE:
            RECORDS.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(produced, indent=2) + "\n", encoding="utf-8", newline="\n")
        self.assertTrue(path.exists(), f"{name} is missing: run with UPDATE_HARNESS_RECORD_GOLDEN=1, vendor the directory to "
                        "services/data-quality-go/internal/eval/testdata/harness-record-v1/ and refresh both SHA256SUMS")
        self.assertEqual(produced, json.loads(path.read_text(encoding="utf-8")),
                         f"{name} differs from what the producer builds now (UPDATE_HARNESS_RECORD_GOLDEN=1 for a deliberate change)")

    def test_a_cell_record_is_the_pinned_one_and_is_valid(self):
        code, out, records = run_records(self, [1, 1, 2, 1, 3, None, 1, 2, 1, 1], [1, 1, 2, 1, 3, None, 1, 2, 1, 1])
        self.assertEqual(code, 0, out)
        produced = json.loads((records / "record-contextual.json").read_text(encoding="utf-8"))
        self.assertEqual(gate.validate_record(produced["body"]), [])
        self.assertEqual(produced["idempotency_key"], f"harness-{RUN_ID}-contextual")
        self.assertEqual(produced["org_id"], "org-corpus-contextual")
        self.golden("record.cell.golden.json", produced["body"])

    def test_a_failing_run_is_recorded_too_with_its_verdict(self):
        code, out, records = run_records(self, [1] * 10, [3] * 10)
        self.assertEqual(code, 1, out)
        body = json.loads((records / "record-contextual.json").read_text(encoding="utf-8"))["body"]
        self.assertEqual(body["scorecard"]["harness"]["gate"]["verdict"], "fail")
        self.assertTrue(body["scorecard"]["harness"]["gate"]["reasons"])
        self.assertEqual(gate.validate_record(body), [])

    def test_an_unbaselined_cell_is_recorded_as_such(self):
        work = Path(tempfile.mkdtemp(prefix="evalrec-"))
        self.addCleanup(shutil.rmtree, work, True)
        (work / "results").mkdir()
        (work / "baseline.json").write_text("{}", encoding="utf-8")
        (work / "results" / "st-rr-on-mixed-contextual.json").write_text(json.dumps(make_rows(20, [1])), encoding="utf-8")
        with contextlib.redirect_stdout(io.StringIO()):
            code = gate.main([str(work / "results"), "--baseline", str(work / "baseline.json"), "--records-out", str(work / "rec"),
                              "--run-id", RUN_ID, "--corpus-manifest", str(MINI_MANIFEST)])
        self.assertEqual(code, 0)
        body = json.loads((work / "rec" / "record-mixed-contextual.json").read_text(encoding="utf-8"))["body"]
        self.assertEqual(body["scorecard"]["harness"]["gate"]["verdict"], "unbaselined")
        self.assertEqual(body["scorecard"]["harness"]["query_class"], "mixed_language")

    def test_records_need_a_run_id_and_a_corpus_manifest(self):
        work = Path(tempfile.mkdtemp(prefix="evalrec-"))
        self.addCleanup(shutil.rmtree, work, True)
        (work / "results").mkdir()
        (work / "baseline.json").write_text("{}", encoding="utf-8")
        (work / "results" / "st-rr-on-baseline.json").write_text(json.dumps(make_rows(20, [1])), encoding="utf-8")
        buffer = io.StringIO()
        with contextlib.redirect_stdout(buffer), contextlib.redirect_stderr(buffer):
            code = gate.main([str(work / "results"), "--baseline", str(work / "baseline.json"), "--records-out", str(work / "rec")])
        self.assertEqual(code, 2, buffer.getvalue())
        self.assertIn("--run-id", buffer.getvalue())

    def test_a_key_the_service_would_refuse_is_never_written(self):
        body = json.loads((RECORDS / "record.cell.golden.json").read_text(encoding="utf-8"))
        work = Path(tempfile.mkdtemp(prefix="evalrec-"))
        self.addCleanup(shutil.rmtree, work, True)
        # 121 characters is past the handler's own bound (requestIdempotencyKey: 8 to 120), which is tighter than the table's 128.
        for key in ("k" * 121, "short"):
            with self.subTest(key=key[:12]):
                with self.assertRaises(gate.UsageError):
                    gate.write_record(str(work), "x", {"org_label": "contextual", "org_id": "org-corpus-contextual",
                                                       "idempotency_key": key, "body": body})
        self.assertEqual(list(work.iterdir()), [])
        gate.write_record(str(work), "x", {"org_label": "contextual", "org_id": "org-corpus-contextual",
                                           "idempotency_key": "k" * 120, "body": body})
        self.assertEqual([p.name for p in work.iterdir()], ["record-x.json"])

    def test_a_promotion_record_is_the_pinned_one_and_is_valid(self):
        bench = healthy(self)
        code, out = bench.run("--promote", "ctx-v2", "--against", "ctx-v1", "--records-out", str(bench.dir / "rec"),
                              "--run-id", RUN_ID, "--corpus-manifest", str(MINI_MANIFEST))
        self.assertEqual(code, 0, out)
        produced = json.loads((bench.dir / "rec" / "record-promotion-ctx-v2.json").read_text(encoding="utf-8"))
        self.assertEqual(gate.validate_record(produced["body"]), [])
        self.assertEqual(produced["org_id"], "org-corpus-contextual")
        self.golden("record.promotion.golden.json", produced["body"])

    def test_every_pinned_invalid_example_is_refused_with_its_reason(self):
        expectations = {
            "invalid.strategy-not-harness.json": "strategy",
            "invalid.metric-out-of-range.json": "mean_ndcg_at_10",
            "invalid.details-present.json": "details",
            "invalid.class-sum-mismatch.json": "by_class",
            "invalid.unknown-field.json": "unknown",
            "invalid.verdict.json": "verdict",
        }
        for filename, needle in expectations.items():
            with self.subTest(filename):
                body = json.loads((RECORDS / filename).read_text(encoding="utf-8"))
                problems = gate.validate_record(body)
                self.assertTrue(problems, f"{filename} was accepted")
                self.assertTrue(any(needle in problem for problem in problems), problems)

    def test_the_pinned_directory_holds_exactly_what_the_tests_expect(self):
        names = sorted(p.name for p in RECORDS.iterdir())
        self.assertEqual(names, sorted([
            "README.md", "SHA256SUMS", "record.cell.golden.json", "record.promotion.golden.json",
            "invalid.strategy-not-harness.json", "invalid.metric-out-of-range.json", "invalid.details-present.json",
            "invalid.class-sum-mismatch.json", "invalid.unknown-field.json", "invalid.verdict.json"]))


if __name__ == "__main__":
    unittest.main()
