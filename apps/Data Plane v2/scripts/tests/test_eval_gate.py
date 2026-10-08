"""Tests of scripts/eval-gate.py: the retrieval-quality gate (CR-T14).

Every rule is proved in BOTH directions, because a gate that cannot fail reads as coverage:
the committed baseline passes; a regression past the tolerance, a coverage below 99 percent,
too few scored queries, an arm returning nothing, a missing cell and an A/B that is not an
A/B each exit 1.

The coverage and gate evidence the gate reads are the REAL response bodies of data-quality-go,
vendored under fixtures/eval-evidence-v1/ (produced by the real handler and the real rules; see
that directory's README and scripts/check-eval-contract.sh), never bodies written by hand.

Run:  python -m unittest discover -s "apps/Data Plane v2/scripts/tests" -p "test_eval_*.py"
"""
from __future__ import annotations

import contextlib
import copy
import io
import json
import math
import os
import shutil
import tempfile
import unittest
from pathlib import Path

from _scripts import FIXTURES, SCRIPTS, load

gate = load("eval-gate.py")
EVIDENCE = FIXTURES / "eval-evidence-v1"


def fixture(name: str) -> dict:
    return json.loads((EVIDENCE / name).read_text(encoding="utf-8"))


def make_rows(n: int, ranks: list, prefix: str = "q") -> list[dict]:
    """n result rows the way eval-run-retrieval.py writes them. ranks[i % len(ranks)] is the
    1-based rank at which query i's one relevant document appears (None: it is not retrieved)."""
    rows = []
    for i in range(n):
        rank = ranks[i % len(ranks)]
        retrieved = []
        for position in range(1, 11):
            if rank == position:
                retrieved.append({"document_id": f"d{i}", "knowledge_id": f"k{i}"})
            else:
                retrieved.append({"document_id": f"x{i}-{position}", "knowledge_id": f"y{i}-{position}"})
        rows.append({"query": f"{prefix}{i}", "relevant_ids": [f"d{i}"], "latency_ms": 100.0 + i, "retrieved": retrieved})
    return rows


def expected(ranks: list) -> tuple[float, float, float]:
    """Independent expectation for one relevant document per query: recall, nDCG@10, MRR."""
    recall = sum(1 for r in ranks if r) / len(ranks)
    ndcg = sum(1.0 / math.log2(r + 1) for r in ranks if r) / len(ranks)
    mrr = sum(1.0 / r for r in ranks if r) / len(ranks)
    return recall, ndcg, mrr


def synthesize_ranks(entry: dict) -> list:
    """Ranks (1-based, None for a miss) of one relevant document per query whose recall, nDCG@10 and MRR are as near as can
    be to the baseline entry's: hits at rank 1, 2, 3 or 5 in the counts that fit best. What lets a test feed the gate results
    that REPRODUCE the committed baseline, through the command line, without a corpus."""
    n = entry["queries"]
    hits = round(entry["recall_at_10"] * n)
    best, best_error = None, 9.0
    for c1 in range(hits + 1):
        for c2 in range(hits - c1 + 1):
            for c3 in range(hits - c1 - c2 + 1):
                c5 = hits - c1 - c2 - c3
                mrr = (c1 + c2 / 2 + c3 / 3 + c5 / 5) / n
                ndcg = (c1 + c2 / math.log2(3) + c3 / 2 + c5 / math.log2(6)) / n
                error = max(abs(mrr - entry["mrr"]), abs(ndcg - entry["ndcg_at_10"]))
                if error < best_error:
                    best, best_error = (c1, c2, c3, c5), error
    c1, c2, c3, c5 = best
    return [1] * c1 + [2] * c2 + [3] * c3 + [5] * c5 + [None] * (n - hits)


class Workdir:
    """A results directory plus a baseline file, both in a temp dir that is removed after."""

    def __init__(self, test: unittest.TestCase):
        self.dir = Path(tempfile.mkdtemp(prefix="evalgate-"))
        test.addCleanup(shutil.rmtree, self.dir, True)
        self.baseline = self.dir / "baseline.json"
        self.results = self.dir / "results"
        self.results.mkdir()

    def cell(self, name: str, rows: list[dict]) -> None:
        (self.results / f"st-rr-on-{name}.json").write_text(json.dumps(rows), encoding="utf-8")

    def evidence(self, filename: str, body: dict) -> None:
        (self.results / filename).write_text(json.dumps(body), encoding="utf-8")

    def set_baseline(self, entries: dict) -> None:
        self.baseline.write_text(json.dumps(entries), encoding="utf-8")

    def run(self, *args: str, env: dict | None = None) -> tuple[int, str]:
        out = io.StringIO()
        saved = {k: os.environ.get(k) for k in (env or {})}
        os.environ.update(env or {})
        try:
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
                try:
                    code = gate.main([str(self.results), "--baseline", str(self.baseline), *args])
                except SystemExit as exit_:
                    code = exit_.code if isinstance(exit_.code, int) else 1
                    if isinstance(exit_.code, str):
                        out.write(exit_.code)
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v
        return code, out.getvalue()


def entry(ranks: list, n: int) -> dict:
    recall, ndcg, mrr = expected(ranks)
    return {"queries": n, "zero_result": 0, "recall_at_10": recall, "ndcg_at_10": ndcg, "mrr": mrr}


class MetricsAreComputedCorrectly(unittest.TestCase):
    def test_score_matches_an_independent_computation(self):
        w = Workdir(self)
        ranks = [1, 1, 2, 1, 3, None, 1, 2, 1, 1]
        w.cell("baseline", make_rows(10, ranks))
        scored = gate.score(str(w.results / "st-rr-on-baseline.json"))
        recall, ndcg, mrr = expected(ranks)
        self.assertEqual(scored["queries"], 10)
        self.assertAlmostEqual(scored["recall_at_10"], recall, places=9)
        self.assertAlmostEqual(scored["ndcg_at_10"], ndcg, places=9)
        self.assertAlmostEqual(scored["mrr"], mrr, places=9)

    def test_a_row_with_no_retrieved_documents_counts_as_zero_result(self):
        w = Workdir(self)
        rows = make_rows(10, [1])
        rows[3]["retrieved"] = []
        w.cell("baseline", rows)
        self.assertEqual(gate.score(str(w.results / "st-rr-on-baseline.json"))["zero_result"], 1)


class TheCommittedBaseline(unittest.TestCase):
    """exit 0 on the committed baseline, and the file is the shape the gate needs."""

    def setUp(self):
        self.committed = json.loads((SCRIPTS / "eval-baseline.json").read_text(encoding="utf-8"))

    def test_every_entry_has_the_five_keys_in_range(self):
        self.assertTrue(self.committed, "the committed baseline is empty")
        for cell, entry_ in self.committed.items():
            with self.subTest(cell=cell):
                self.assertEqual(set(entry_), {"queries", "zero_result", "recall_at_10", "ndcg_at_10", "mrr"})
                self.assertGreaterEqual(entry_["queries"], gate.MIN_QUERIES_TO_BASELINE)
                for metric in ("recall_at_10", "ndcg_at_10", "mrr"):
                    self.assertTrue(0.0 <= entry_[metric] <= 1.0)

    def test_the_gate_passes_when_the_current_run_equals_the_committed_baseline(self):
        rows, failures, warnings = gate.check_cells(self.committed, copy.deepcopy(self.committed))
        self.assertEqual(failures, [])
        self.assertEqual(warnings, [])
        self.assertEqual(len(rows), len(self.committed) * len(gate.TOLERANCE))

    def test_the_command_line_passes_on_results_that_reproduce_the_committed_baseline(self):
        # The gate, run the way CI runs it, against the committed file itself: every cell's result reproduces its
        # committed numbers (within a hundredth), so the verdict must be GATE PASSED and exit 0.
        w = Workdir(self)
        for cell, entry_ in self.committed.items():
            ranks = synthesize_ranks(entry_)
            scored = gate.score_rows(make_rows(entry_["queries"], ranks))
            for metric in ("recall_at_10", "ndcg_at_10", "mrr"):
                self.assertAlmostEqual(scored[metric], entry_[metric], delta=0.01, msg=f"{cell} {metric}")
            w.cell(cell, make_rows(entry_["queries"], ranks, prefix="q" if "lexical" not in cell else "lex"))
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = gate.main([str(w.results)])  # the committed baseline: no --baseline
        self.assertEqual(code, 0, out.getvalue())
        self.assertIn("GATE PASSED", out.getvalue())

    def test_the_same_command_line_fails_when_one_committed_cell_collapses(self):
        w = Workdir(self)
        for cell, entry_ in self.committed.items():
            ranks = synthesize_ranks(entry_)
            if cell == "contextual":
                ranks = [None] * (len(ranks) // 3) + ranks[len(ranks) // 3:]  # a third of the queries lose their document
            w.cell(cell, make_rows(entry_["queries"], ranks, prefix="q" if "lexical" not in cell else "lex"))
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            code = gate.main([str(w.results)])
        self.assertEqual(code, 1, out.getvalue())
        self.assertIn("contextual", out.getvalue())
        self.assertIn("REGRESSED", out.getvalue())

    def test_the_gate_fails_when_one_committed_cell_drops_past_its_tolerance(self):
        for cell in self.committed:
            for metric, slack in gate.TOLERANCE.items():
                with self.subTest(cell=cell, metric=metric):
                    current = copy.deepcopy(self.committed)
                    current[cell][metric] = max(self.committed[cell][metric] - slack - 0.001, -1.0)
                    _, failures, _ = gate.check_cells(self.committed, current)
                    self.assertEqual(len(failures), 1, failures)
                    self.assertIn(cell, failures[0])
                    self.assertIn(metric, failures[0])


class RegressionDirection(unittest.TestCase):
    def test_a_change_inside_the_noise_floor_passes(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        ranks = [1] * 99 + [2]  # one query slips to rank 2: nDCG -0.0037, MRR -0.005
        w.cell("baseline", make_rows(100, ranks))
        code, out = w.run()
        self.assertEqual(code, 0, out)
        self.assertIn("GATE PASSED", out)

    def test_a_regression_past_the_tolerance_exits_1(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        w.cell("baseline", make_rows(100, [3]))  # every query at rank 3: nDCG 0.5, MRR 0.333
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("REGRESSED", out)
        self.assertIn("GATE FAILED", out)

    def test_an_improvement_passes(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([2], 100)})
        w.cell("baseline", make_rows(100, [1]))
        code, out = w.run()
        self.assertEqual(code, 0, out)

    def test_each_metric_is_gated_on_its_own_tolerance_boundary(self):
        # recall: losing 3 of 100 queries is -0.03, exactly the slack, so it passes; 4 of 100 fails.
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        w.cell("baseline", make_rows(100, [1] * 97 + [None] * 3))
        self.assertEqual(w.run()[0], 0)
        w.cell("baseline", make_rows(100, [1] * 96 + [None] * 4))
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("recall_at_10", out)


class TheEvalMustReallyRun(unittest.TestCase):
    def test_too_few_scored_queries_exits_1(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        w.cell("baseline", make_rows(50, [1]))  # half the queries: the floor is 90
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("only 50 queries scored", out)

    def test_exactly_the_floor_passes_and_one_below_fails(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        w.cell("baseline", make_rows(90, [1]))
        self.assertEqual(w.run()[0], 0)
        w.cell("baseline", make_rows(89, [1]))
        self.assertEqual(w.run()[0], 1)

    def test_a_small_cell_is_floored_by_its_own_count_not_by_the_global_minimum(self):
        # 12 queries baselined: the global floor of 20 must not make an honest small cell fail.
        w = Workdir(self)
        w.set_baseline({"norwegian-contextual": entry([1], 12)})
        w.cell("norwegian-contextual", make_rows(12, [1]))
        self.assertEqual(w.run()[0], 0)
        w.cell("norwegian-contextual", make_rows(10, [1]))
        self.assertEqual(w.run()[0], 1)

    def test_queries_that_return_nothing_exit_1(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        rows = make_rows(100, [1])
        for row in rows[:5]:
            row["retrieved"] = []
        w.cell("baseline", rows)
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("returned NOTHING", out)

    def test_a_baselined_cell_with_no_result_file_exits_1(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100), "contextual": entry([1], 100)})
        w.cell("baseline", make_rows(100, [1]))
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("contextual: baselined at 100 queries, but produced NO result file", out)

    def test_an_unbaselined_cell_warns_and_does_not_fail(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        w.cell("baseline", make_rows(100, [1]))
        w.cell("mixed-baseline", make_rows(20, [1]))
        code, out = w.run()
        self.assertEqual(code, 0, out)
        self.assertIn("WARNING  mixed-baseline: no baseline entry", out)

    def test_no_result_files_at_all_is_a_hard_stop(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100)})
        code, out = w.run()
        self.assertNotEqual(code, 0)
        self.assertIn("no st-rr-on-*.json", out)

    def test_a_missing_baseline_file_is_a_hard_stop(self):
        w = Workdir(self)
        w.cell("baseline", make_rows(100, [1]))
        code, out = w.run()
        self.assertNotEqual(code, 0)
        self.assertIn("no baseline at", out)


class Rebaselining(unittest.TestCase):
    def test_update_merges_and_leaves_other_cells_alone(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([2], 100), "contextual": entry([2], 100)})
        w.cell("contextual", make_rows(100, [1]))
        code, out = w.run("--update")
        self.assertEqual(code, 0, out)
        written = json.loads(w.baseline.read_text(encoding="utf-8"))
        self.assertEqual(set(written), {"baseline", "contextual"})
        self.assertEqual(written["baseline"], entry([2], 100))
        self.assertAlmostEqual(written["contextual"]["mrr"], 1.0, places=9)
        self.assertEqual(set(written["contextual"]), {"queries", "zero_result", "recall_at_10", "ndcg_at_10", "mrr"})

    def test_replace_drops_cells_that_are_absent(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 100), "contextual": entry([1], 100)})
        w.cell("contextual", make_rows(100, [1]))
        code, _ = w.run("--replace")
        self.assertEqual(code, 0)
        self.assertEqual(set(json.loads(w.baseline.read_text(encoding="utf-8"))), {"contextual"})

    def test_update_refuses_to_lower_a_baseline_unless_the_decision_is_explicit(self):
        # The contextual baseline is nDCG 0.7732 (retrieval-fusion-evidence-2026-08-26.md) and the owner's
        # ruling is that no change undercuts it. A re-baseline that lowers a cell past its tolerance is a
        # decision, so it needs --accept-lower and is said out loud.
        w = Workdir(self)
        w.set_baseline({"contextual": entry([1], 100)})
        w.cell("contextual", make_rows(100, [3]))
        code, out = w.run("--update")
        self.assertEqual(code, 1, out)
        self.assertIn("would LOWER", out)
        self.assertEqual(json.loads(w.baseline.read_text(encoding="utf-8"))["contextual"], entry([1], 100))
        code, out = w.run("--update", "--accept-lower")
        self.assertEqual(code, 0, out)
        self.assertLess(json.loads(w.baseline.read_text(encoding="utf-8"))["contextual"]["ndcg_at_10"], 0.9)

    def test_update_allows_a_change_inside_the_tolerance_without_the_flag(self):
        w = Workdir(self)
        w.set_baseline({"contextual": entry([1], 100)})
        w.cell("contextual", make_rows(100, [1] * 99 + [2]))
        code, out = w.run("--update")
        self.assertEqual(code, 0, out)

    def test_a_cell_too_small_to_gate_is_refused_unless_asked(self):
        w = Workdir(self)
        w.set_baseline({})
        w.cell("mixed-baseline", make_rows(gate.MIN_QUERIES_TO_BASELINE - 1, [1]))
        code, out = w.run("--update")
        self.assertEqual(code, 1, out)
        self.assertIn("too few queries to baseline", out)
        self.assertEqual(json.loads(w.baseline.read_text(encoding="utf-8")), {})
        code, out = w.run("--update", "--allow-small")
        self.assertEqual(code, 0, out)

    def test_a_cell_with_queries_that_returned_nothing_is_not_baselined(self):
        w = Workdir(self)
        w.set_baseline({})
        rows = make_rows(50, [1])
        rows[0]["retrieved"] = []
        w.cell("baseline", rows)
        code, out = w.run("--update")
        self.assertEqual(code, 1, out)
        self.assertIn("returned nothing", out)


class QueryParityOfTheTwoOrganizations(unittest.TestCase):
    """The A/B is an A/B only when both organizations were asked the same questions."""

    def test_the_same_queries_pass(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 50), "contextual": entry([1], 50)})
        w.cell("baseline", make_rows(50, [1]))
        w.cell("contextual", make_rows(50, [1]))
        self.assertEqual(w.run()[0], 0)

    def test_different_queries_exit_1_even_when_every_metric_is_fine(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 50), "contextual": entry([1], 50)})
        w.cell("baseline", make_rows(50, [1]))
        w.cell("contextual", make_rows(50, [1], prefix="other"))
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("not comparable", out)

    def test_a_partial_run_of_one_side_exits_1(self):
        w = Workdir(self)
        w.set_baseline({"baseline": entry([1], 50), "contextual": entry([1], 50)})
        w.cell("baseline", make_rows(50, [1]))
        w.cell("contextual", make_rows(46, [1]))  # 46 >= the floor of 45, so only the parity rule catches it
        code, out = w.run()
        self.assertEqual(code, 1, out)
        self.assertIn("not comparable", out)


class ContextCoverageGate(unittest.TestCase):
    """coverage below 99 percent exits 1; the evidence is the real body of GET /v1/quality/context-coverage."""

    def setUp(self):
        self.w = Workdir(self)
        self.w.set_baseline({"baseline": entry([1], 50), "contextual": entry([1], 50)})
        self.w.cell("baseline", make_rows(50, [1]))
        self.w.cell("contextual", make_rows(50, [1]))

    def ready(self) -> dict:
        return fixture("coverage.contextual-ready.golden.json")

    def raw(self) -> dict:
        return fixture("coverage.baseline-off.golden.json")

    def give(self, contextual: dict | None = None, baseline: dict | None = None) -> None:
        if contextual is not None:
            self.w.evidence("coverage-contextual.json", contextual)
        if baseline is not None:
            self.w.evidence("coverage-baseline.json", baseline)

    def test_full_coverage_and_a_raw_baseline_pass(self):
        self.give(self.ready(), self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 0, out)
        self.assertIn("coverage", out.lower())

    def test_the_real_below_threshold_body_exits_1(self):
        self.give(fixture("coverage.contextual-below-threshold.golden.json"), self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("94.50", out)
        self.assertIn("99", out)

    def test_the_boundary_is_inclusive_99_passes_and_98_9_fails(self):
        for ready, ok in ((990, True), (989, False)):
            with self.subTest(ready=ready):
                report = self.ready()
                counts = report["counts"]
                counts.update(eligible=1000, ready=ready, pending=1000 - ready)
                report["evaluation"].update(
                    status="passed" if ok else "failed", ready_trusted=ready, denominator=1000,
                    coverage_percent=ready / 10, threshold_percent=99)
                baseline = self.raw()
                baseline["counts"].update(eligible=1000, skipped_policy=1000)
                self.give(report, baseline)
                code, out = self.w.run("--require-coverage")
                self.assertEqual(code, 0 if ok else 1, out)

    def test_a_status_that_disagrees_with_the_counts_is_refused(self):
        report = fixture("coverage.contextual-below-threshold.golden.json")
        report["evaluation"]["status"] = "passed"  # a report that claims to pass at 94.5 percent
        self.give(report, self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("disagrees", out)

    def test_missing_evidence_exits_1_when_required_and_only_warns_otherwise(self):
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("coverage evidence is missing", out)
        code, out = self.w.run()
        self.assertEqual(code, 0, out)

    def test_a_malformed_report_is_never_a_pass(self):
        for name, mutate in (
            ("no counts", lambda r: r.pop("counts")),
            ("no evaluation", lambda r: r.pop("evaluation")),
            ("a string count", lambda r: r["counts"].update(ready="1164")),
            ("a negative count", lambda r: r["counts"].update(pending=-1)),
            ("unknown policy", lambda r: r["policy"].update(contextual_retrieval="maybe")),
        ):
            with self.subTest(name):
                report = self.ready()
                mutate(report)
                self.give(report, self.raw())
                code, out = self.w.run("--require-coverage")
                self.assertEqual(code, 1, out)
                self.assertIn("malformed", out)

    def test_an_empty_corpus_is_not_coverage(self):
        report = self.ready()
        report["counts"].update(eligible=0, ready=0)
        report["evaluation"].update(status="passed", ready_trusted=0, denominator=0, coverage_percent=None)
        self.give(report, self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("no eligible units", out)

    def test_a_contextual_organization_whose_policy_is_off_is_refused(self):
        report = fixture("coverage.baseline-off.golden.json")
        report["org_id"] = "org-corpus-contextual"
        self.give(report, self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("policy is off", out)

    def test_units_whose_vector_does_not_match_their_context_exit_1_even_above_the_threshold(self):
        report = self.ready()
        report["counts"].update(eligible=1164, ready=1164, vector_incoherent=3)
        report["evaluation"].update(ready_trusted=1161, coverage_percent=1161 / 1164 * 100)
        self.give(report, self.raw())
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("vector_incoherent", out)

    def test_the_baseline_organization_must_be_raw(self):
        raw_ok = self.raw()
        self.give(self.ready(), raw_ok)
        self.assertEqual(self.w.run("--require-coverage")[0], 0)

        inherit = self.raw()
        inherit["policy"] = {"contextual_retrieval": "inherit", "source": "default"}
        self.give(self.ready(), inherit)
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("not raw", out)

        with_contexts = self.raw()
        with_contexts["counts"].update(ready=100, skipped_policy=1064)
        self.give(self.ready(), with_contexts)
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("holds contexts", out)

    def test_both_organizations_must_hold_the_same_corpus(self):
        different = self.raw()
        different["counts"].update(eligible=1100, skipped_policy=1100)
        self.give(self.ready(), different)
        code, out = self.w.run("--require-coverage")
        self.assertEqual(code, 1, out)
        self.assertIn("different corpora", out)

    def test_the_environment_switch_is_the_same_as_the_flag_and_is_checked(self):
        code, out = self.w.run(env={"EVAL_REQUIRE_COVERAGE": "1"})
        self.assertEqual(code, 1, out)
        code, out = self.w.run(env={"EVAL_REQUIRE_COVERAGE": "0"})
        self.assertEqual(code, 0, out)
        code, out = self.w.run(env={"EVAL_REQUIRE_COVERAGE": "yes please"})
        self.assertEqual(code, 2, out)
        self.assertIn("EVAL_REQUIRE_COVERAGE", out)


class IndexParityGate(unittest.TestCase):
    """sparse_index_in_sync: the keyword index holds what Postgres holds. The body is the real GET /v1/quality/gates."""

    def setUp(self):
        self.w = Workdir(self)
        self.w.set_baseline({"baseline": entry([1], 50), "contextual": entry([1], 50)})
        self.w.cell("baseline", make_rows(50, [1]))
        self.w.cell("contextual", make_rows(50, [1]))
        self.w.evidence("coverage-contextual.json", fixture("coverage.contextual-ready.golden.json"))
        self.w.evidence("coverage-baseline.json", fixture("coverage.baseline-off.golden.json"))

    def test_in_sync_passes(self):
        self.w.evidence("gates-contextual.json", fixture("gates.in-sync.golden.json"))
        self.w.evidence("gates-baseline.json", fixture("gates.in-sync.golden.json"))
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 0, out)

    def test_unavailable_evidence_is_never_a_pass(self):
        self.w.evidence("gates-contextual.json", fixture("gates.parity-unavailable.golden.json"))
        self.w.evidence("gates-baseline.json", fixture("gates.in-sync.golden.json"))
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 1, out)
        self.assertIn("sparse_index_in_sync", out)
        self.assertIn("unavailable", out)

    def test_a_gate_that_failed_exits_1(self):
        report = fixture("gates.in-sync.golden.json")
        report["gates"][1].update(passed=False, status="failed", message="3 missing")
        report["all_passed"] = False
        self.w.evidence("gates-contextual.json", report)
        self.w.evidence("gates-baseline.json", fixture("gates.in-sync.golden.json"))
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 1, out)
        self.assertIn("3 missing", out)

    def test_a_report_without_the_gate_is_never_a_pass(self):
        report = fixture("gates.in-sync.golden.json")
        report["gates"] = [g for g in report["gates"] if g["gate"] != "sparse_index_in_sync"]
        self.w.evidence("gates-contextual.json", report)
        self.w.evidence("gates-baseline.json", fixture("gates.in-sync.golden.json"))
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 1, out)
        self.assertIn("does not carry sparse_index_in_sync", out)

    def test_a_gate_that_says_passed_with_status_unavailable_is_still_refused(self):
        report = fixture("gates.in-sync.golden.json")
        report["gates"][1]["status"] = "unavailable"  # contradictory: passed true, status unavailable
        self.w.evidence("gates-contextual.json", report)
        self.w.evidence("gates-baseline.json", fixture("gates.in-sync.golden.json"))
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 1, out)

    def test_missing_evidence_exits_1_when_required(self):
        code, out = self.w.run("--require-coverage", "--require-parity")
        self.assertEqual(code, 1, out)
        self.assertIn("parity evidence is missing", out)
        self.assertEqual(self.w.run("--require-coverage")[0], 0)


class TheGateReadsTheCommittedCellSpecs(unittest.TestCase):
    def test_every_class_has_a_baseline_and_a_contextual_cell(self):
        classes: dict[str, set] = {}
        for cell, spec in gate.CELL_SPECS.items():
            classes.setdefault(spec["class"], set()).add(spec["org_label"])
        for name, labels in classes.items():
            self.assertEqual(labels, {"baseline", "contextual"}, name)

    def test_the_organizations_are_the_ones_the_corpus_seeder_may_act_for(self):
        registry = json.loads((SCRIPTS.parents[1] / "Control Plane" / "config" / "plane-service-principals.json").read_text(encoding="utf-8"))
        allowed = set(registry["corpus-seeder"]["orgIds"])
        self.assertEqual(set(gate.ORG_IDS.values()), allowed)


if __name__ == "__main__":
    unittest.main()
