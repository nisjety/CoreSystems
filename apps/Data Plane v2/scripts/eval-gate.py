"""Quality gate for retrieval. Exits non-zero when metrics fall below a committed
baseline by more than the measured noise floor, when the evidence the metrics rest on
is missing or wrong, and when a candidate is not good enough to become the default.

Run it in CI, or by hand before/after any change to fusion weights, rerank
config, chunking, embedding models, or the contextual-retrieval prompt or model:

    python3 scripts/eval-gate.py <results-dir>            # check vs baseline
    python3 scripts/eval-gate.py <results-dir> --update    # re-baseline (merge)
    python3 scripts/eval-gate.py <results-dir> --replace   # re-baseline (drop absent cells)
    python3 scripts/eval-gate.py <results-dir> --require-coverage --require-parity
    python3 scripts/eval-gate.py <results-dir> --promote ctx-v2 --against ctx-v1
    python3 scripts/eval-gate.py <results-dir> --records-out <dir> --run-id <id>

`<results-dir>` holds the `st-rr-on-<cell>.json` files written by
`eval-attribution-study.sh` / `eval-lexical-cell.sh` (or any run of
`eval-run-retrieval.py` with OUT_FILE named that way). The baseline lives in
`scripts/eval-baseline.json`, committed, so a regression is a diff a reviewer
can see.

`--update` MERGES: cells present in the results dir are rewritten, cells absent
from it are left alone. It used to replace the file wholesale, which meant
re-baselining one cell silently deleted every other cell's baseline — and a
deleted baseline does not fail the gate, it downgrades to
"no baseline entry (new org?) — not gated". Losing coverage looked identical to
passing. `--replace` still does the wholesale rewrite, for when a cell is
genuinely retired.

`--update` also refuses three things it used to allow, because each one made the baseline
quietly worse: a cell with fewer than MIN_QUERIES_TO_BASELINE queries (a baseline that small
gates nothing; `--allow-small` records it anyway), a cell in which queries returned nothing
(the baseline would bake in a dead arm), and a change that LOWERS an existing cell past its
tolerance (`--accept-lower`: a decrease is a decision, and the commit message says why).

## What else the gate judges (CR-T14)

The numbers are only worth gating when they were measured on the thing they claim to measure.
Evidence files sit in the same directory, and are fetched by `scripts/eval-service.py`:

  * `coverage-<baseline|contextual>.json` — the body of GET /v1/quality/context-coverage. The
    contextual organisation must have a verified context on at least 99 percent of the units
    that are expected to have one (recomputed here from the counts, never trusted from the
    report's own verdict, and the two are compared so the rule cannot drift unseen); the
    baseline organisation must be RAW (policy off, no unit holding a context), or the A/B
    compares contextual retrieval with itself; and both must hold the same corpus.
  * `gates-<baseline|contextual>.json` — the body of GET /v1/quality/gates. The keyword index
    must hold what Postgres holds (`sparse_index_in_sync`): a lexical arm that reaches a quarter
    of the chunks makes every lexical number meaningless. "unavailable" is never a pass.
  * the two organisations of one query class must have been asked the same queries.

A piece of evidence that is PRESENT is always judged. One that is absent fails only when it is
required (`--require-coverage`, `--require-parity`, or EVAL_REQUIRE_COVERAGE=1 /
EVAL_REQUIRE_PARITY=1; the nightly job requires both).

## The promotion gate

A prompt version, a model or a rerank input becomes the default only through
`--promote <candidate> --against <default>`, which reads, per variant, the files that
`scripts/eval-variant-cell.sh` writes: `cell-<v>-rr-off.json`, `cell-<v>-rr-on-1..3.json`,
`cell-<v>-lexical.json`, `coverage-<v>.json` and `state-<v>.json`. The candidate is promoted only
when ALL hold, and anything less (including evidence that is merely absent) is HOLD, exit 1:

  1. non-inferiority with the reranker OFF, on the same queries: nDCG@10 and MRR no worse than the
     default's by more than 0.01 (the reranker-off noise is about 0.008, the reranker-on noise
     about +/-0.04, which is why attribution runs with it off);
  2. the candidate's contextual state covers at least 99 percent and has no incoherent vector;
  3. the state is the DECLARED one: the units carry the candidate's prompt version and model
     (otherwise the measurement is of the default under the candidate's name);
  4. the exact-identifier class (the 23 mined queries) unchanged: no query lost, none empty;
  5. confirmation with the reranker ON, three complete runs each: the mean no worse than the
     default's by more than the rerank-on tolerance.

The verdict is written to `promotion-<candidate>-vs-<default>.json`, deterministically.

## Persisted results

`--records-out DIR --run-id ID` writes, for every cell judged (also when the gate FAILED: a failing
run is the one worth keeping) and for the promotion, `record-<cell>.json`: the request that
`scripts/eval-service.py record` POSTs to data-quality-go's POST /v1/evals/results, which stores it
in `quality_eval_runs`, so that regressions are a query. The wire contract is `harness-record-v1`,
pinned by fixtures/harness-record-v1/ on this side and by the service's testdata on the other.

## Why the tolerances are what they are

A gate tighter than the instrument's noise is a flaky test, and a flaky quality
gate gets disabled within a week — which is worse than no gate. Measured
2026-08-26 on this corpus:

  * one query = 1.15 recall points at n=87
  * re-running an identical cell moves recall by ~1 query
  * WITH the reranker on, cell-to-cell nDCG variance is +/-0.04, because ~19% of
    queries lose the cross-encoder to provider 429s and *which* queries lose it
    differs per run (see docs/retrieval-fusion-evidence-2026-08-26.md)

So the tolerances below are deliberately loose. This gate catches a stage going
dark (the reranker silently 429ing, an arm returning nothing, a metric
saturating) — the class of failure that actually happened four times in this
codebase. It does NOT resolve a 2-point ranking change; nothing can, until
rerank coverage is deterministic. Tighten TOLERANCE once it is.

Exit status: 0 the gate passed (or PROMOTE), 1 it failed (or HOLD), 2 the command line or the
environment is wrong.
"""
from __future__ import annotations

import argparse
import glob
import json
import math
import os
import re
import statistics
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
BASELINE = os.path.join(HERE, "eval-baseline.json")
CORPUS_MANIFEST = os.path.join(HERE, "eval-corpus-manifest.json")

# Per-metric slack, from the measured noise above. recall gets ~2 queries.
TOLERANCE = {"recall_at_10": 0.030, "ndcg_at_10": 0.045, "mrr": 0.045}

# A comparison that lands exactly on a boundary must not fail on float noise
# (0.97 - 1.0 is -0.030000000000000027, not -0.03).
EPS = 1e-9

# "Did the eval actually run?" floor. Cells legitimately differ in size — 87
# hand-authored natural-language questions vs 23 mined lexical queries — so a
# single global count is wrong in both directions at once: it false-fails the
# small cell as an incomplete run, and (had the floor been lowered to suit it)
# would stop catching a truncated run in the large one. Gate each cell against
# its OWN baselined count instead, with a small absolute backstop so a cell
# cannot be baselined down to nothing. The backstop never exceeds the cell's own
# count, so an honest small cell is not failed for being small; --update refuses
# to baseline a cell below MIN_QUERIES_TO_BASELINE instead.
MIN_QUERY_FRACTION = 0.9
MIN_QUERIES_ABSOLUTE = 20
MIN_QUERIES_TO_BASELINE = 10

# A query that retrieved NOTHING is the signature of an arm or the confidence
# gate eating results. Kept tight on purpose, and it is the sharpest signal the
# lexical cell has: those queries are mined to occur in exactly one corpus
# document, so a zero result there means lexical retrieval is broken, not that
# the question was hard.
MAX_ZERO_RESULT_FRACTION = 0.02

# The share of the units expected to have a context that must have a VERIFIED one (the design's
# 99 percent; the same default as data-quality-go's CONTEXT_COVERAGE_THRESHOLD_PERCENT).
COVERAGE_THRESHOLD_PERCENT = 99.0

# The two evaluation organisations, which are the two the corpus-seeder principal may act for
# (apps/Control Plane/config/plane-service-principals.json; a test pins the equality).
ORG_IDS = {"baseline": "org-corpus-baseline", "contextual": "org-corpus-contextual"}

# Every cell the nightly measures: its organisation, its query class (the name the service's
# `by_class` uses) and how the reranker was set. A cell that is not here is still gated if it has
# a baseline, but it is not recorded: its class and organisation are unknown.
CELL_SPECS = {
    "baseline": {"org_label": "baseline", "class": "natural_language", "rerank": "on"},
    "contextual": {"org_label": "contextual", "class": "natural_language", "rerank": "on"},
    "lexical-baseline": {"org_label": "baseline", "class": "lexical_identifier", "rerank": "default"},
    "lexical-contextual": {"org_label": "contextual", "class": "lexical_identifier", "rerank": "default"},
    "norwegian-baseline": {"org_label": "baseline", "class": "norwegian_morphology", "rerank": "on"},
    "norwegian-contextual": {"org_label": "contextual", "class": "norwegian_morphology", "rerank": "on"},
    "mixed-baseline": {"org_label": "baseline", "class": "mixed_language", "rerank": "on"},
    "mixed-contextual": {"org_label": "contextual", "class": "mixed_language", "rerank": "on"},
}

# The promotion rules (see the docstring). The two 0.01 are the design's non-inferiority margins and
# are about one rerank-off noise width above the measured 0.008; the 0.045 is the rerank-on noise
# allowance the regression gate already uses.
PROMOTION = {
    "max_ndcg_drop": 0.01,
    "max_mrr_drop": 0.01,
    "rerank_on_runs": 3,
    "rerank_on_slack": 0.045,
    "lexical_ndcg_drop": 0.01,
}

BASELINE_KEYS = ("queries", "zero_result", "recall_at_10", "ndcg_at_10", "mrr")

VARIANT_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,48}$")


class UsageError(Exception):
    """The command line or the environment is wrong (exit 2), as opposed to a failed gate (exit 1)."""


# ── scoring ──────────────────────────────────────────────────────────────────

def golden_metrics(retrieved, relevant):
    rel = set(relevant)
    if not rel:
        return 0.0, 0.0, 0.0
    found, dcg, mrr = set(), 0.0, 0.0
    for i in range(min(len(retrieved), 10)):
        d, c = retrieved[i].get("document_id"), retrieved[i].get("knowledge_id")
        dh, ch = d in rel, c in rel
        if not dh and not ch:
            continue
        if mrr == 0:
            mrr = 1.0 / (i + 1)
        fresh = False
        if dh and d not in found:
            found.add(d)
            fresh = True
        if ch and c not in found:
            found.add(c)
            fresh = True
        if fresh:
            dcg += 1.0 / math.log2(i + 2)
    idcg = sum(1.0 / math.log2(i + 2) for i in range(min(len(relevant), 10)))
    ndcg = dcg / idcg if idcg else 0.0
    if ndcg > 1.0 + 1e-9:
        raise SystemExit(f"nDCG {ndcg} > 1 — the metric accounting regressed")
    return len(found) / len(relevant), ndcg, mrr


def score_rows(rows):
    if not rows:
        raise SystemExit("a result file holds no rows — the eval for that cell wrote nothing")
    per = [golden_metrics(r["retrieved"], r["relevant_ids"]) for r in rows]
    zero = sum(1 for r in rows if not r["retrieved"])
    latencies = sorted(float(r.get("latency_ms", 0.0)) for r in rows)
    p95 = latencies[max(0, math.ceil(0.95 * len(latencies)) - 1)]
    return {
        "queries": len(rows),
        "zero_result": zero,
        "recall_at_10": statistics.fmean(x[0] for x in per),
        "ndcg_at_10": statistics.fmean(x[1] for x in per),
        "mrr": statistics.fmean(x[2] for x in per),
        "mean_latency_ms": statistics.fmean(latencies),
        "p95_latency_ms": p95,
    }


def _load_json(path):
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def read_rows(path):
    return _load_json(path)


def score(path):
    return score_rows(read_rows(path))


def cell_detail(path):
    rows = read_rows(path)
    return {"score": score_rows(rows), "queries": sorted({r["query"] for r in rows})}


def collect_detail(results_dir):
    out = {}
    for path in sorted(glob.glob(os.path.join(results_dir, "st-rr-on-*.json"))):
        cell = os.path.basename(path)[len("st-rr-on-"):-len(".json")]
        out[cell] = cell_detail(path)
    return out


def collect(results_dir):
    return {cell: d["score"] for cell, d in collect_detail(results_dir).items()}


def baseline_entry(metrics):
    return {key: metrics[key] for key in BASELINE_KEYS}


# ── the regression check ─────────────────────────────────────────────────────

def _check_cells(baseline, current):
    rows, failures, warnings = [], [], []

    # A baselined cell that produced NO result file this run is not "nothing to
    # compare" — it is the darkest form of the failure this gate exists to
    # catch: not degraded, not zero-result, just entirely absent. The loop
    # below iterates `current`, so a cell missing from `current` was never
    # being visited at all — caught live 2026-08-28: a transient 401/500 storm
    # killed the two natural-language `rr-on` cells mid-run (auth token expiry
    # partway through an ~87-query, several-minute cell is the suspected
    # cause), `eval-gate.py` still printed GATE PASSED, because the two
    # lexical cells that DID complete were the only ones ever checked. The
    # gate was silently skipping its two largest, longest-established cells.
    for cell in sorted(set(baseline) - set(current)):
        failures.append((cell, f"{cell}: baselined at {baseline[cell]['queries']} queries, but "
                               f"produced NO result file this run — the eval for this cell did "
                               f"not complete, not just regress"))

    for cell, cur in sorted(current.items()):
        base = baseline.get(cell)
        if base is None:
            warnings.append(f"{cell}: no baseline entry (new org?) — not gated")
            continue

        floor = max(min(MIN_QUERIES_ABSOLUTE, base["queries"]), int(base["queries"] * MIN_QUERY_FRACTION))
        if cur["queries"] < floor:
            failures.append((cell, f"{cell}: only {cur['queries']} queries scored, against {base['queries']} "
                                   f"at baseline (floor {floor}) — the eval itself did not complete"))
        zero_frac = cur["zero_result"] / max(cur["queries"], 1)
        if zero_frac > MAX_ZERO_RESULT_FRACTION:
            failures.append((cell, f"{cell}: {cur['zero_result']}/{cur['queries']} queries returned NOTHING "
                                   f"({zero_frac:.1%} > {MAX_ZERO_RESULT_FRACTION:.0%}) — an arm or the "
                                   f"confidence gate is eating results"))

        for metric, slack in TOLERANCE.items():
            b, c = base[metric], cur[metric]
            delta = c - b
            ok = delta >= -slack - EPS
            rows.append((cell, metric, b, c, delta, slack, "ok" if ok else "REGRESSED"))
            if not ok:
                failures.append((cell, f"{cell}: {metric} {b:.4f} -> {c:.4f} ({delta:+.4f}), "
                                       f"beyond the {slack:.3f} noise allowance"))
    return rows, failures, warnings


def check_cells(baseline, current):
    """(table rows, failure messages, warnings) of the regression check. `current` maps a cell to the
    five baseline keys at least."""
    rows, failures, warnings = _check_cells(baseline, current)
    return rows, [message for _, message in failures], warnings


def print_table(rows):
    print(f"{'cell':<22}{'metric':<14}{'baseline':>10}{'current':>10}{'delta':>9}{'slack':>8}  verdict")
    print("-" * 84)
    for cell, metric, b, c, delta, slack, verdict in rows:
        print(f"{cell:<22}{metric:<14}{b:>10.4f}{c:>10.4f}{delta:>+9.4f}{slack:>8.3f}  {verdict}")


# ── query parity: an A/B is an A/B only when both sides were asked the same questions ─────

def check_query_parity(detail):
    """[(cells, message)] for every query class whose two organisations were not asked the same queries."""
    by_class = {}
    for cell, d in detail.items():
        spec = CELL_SPECS.get(cell)
        if spec:
            by_class.setdefault(spec["class"], {})[spec["org_label"]] = (cell, d)
    failures = []
    for cls, sides in sorted(by_class.items()):
        if "baseline" not in sides or "contextual" not in sides:
            continue
        (bcell, b), (ccell, c) = sides["baseline"], sides["contextual"]
        qb, qc = set(b["queries"]), set(c["queries"])
        if qb != qc:
            failures.append(([bcell, ccell],
                             f"{cls}: the A/B is not comparable: {bcell} scored {len(qb)} queries and {ccell} {len(qc)} "
                             f"({len(qb - qc)} only in {bcell}, {len(qc - qb)} only in {ccell})"))
    return failures


# ── the coverage evidence ────────────────────────────────────────────────────

COUNT_KEYS = ("eligible", "ready", "legacy", "pending", "skipped_zdr", "skipped_policy", "skipped_budget",
              "failed_transient", "failed_permanent", "status_unknown", "vector_incoherent", "ready_unverified")
POLICY_VALUES = ("inherit", "on", "off")
COVERAGE_STATUSES = ("passed", "failed", "skipped_policy")


def _is_count(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def validate_coverage_report(report):
    """The problems that make a coverage report unreadable ([] when it is the contract's shape)."""
    if not isinstance(report, dict):
        return ["the report is not a JSON object"]
    problems = []
    if not isinstance(report.get("org_id"), str) or not report.get("org_id"):
        problems.append("org_id is missing")
    counts = report.get("counts")
    if not isinstance(counts, dict):
        problems.append("counts is missing")
    else:
        for key in COUNT_KEYS:
            if not _is_count(counts.get(key)):
                problems.append(f"counts.{key} is not a non-negative integer")
    policy = report.get("policy")
    if not isinstance(policy, dict) or policy.get("contextual_retrieval") not in POLICY_VALUES:
        problems.append("policy.contextual_retrieval is not inherit, on or off")
    evaluation = report.get("evaluation")
    if not isinstance(evaluation, dict):
        problems.append("evaluation is missing")
    else:
        if evaluation.get("status") not in COVERAGE_STATUSES:
            problems.append("evaluation.status is not passed, failed or skipped_policy")
        threshold = evaluation.get("threshold_percent")
        if isinstance(threshold, bool) or not isinstance(threshold, (int, float)):
            problems.append("evaluation.threshold_percent is not a number")
    return problems


def coverage_figures(counts):
    """(verified units, units expected to have a context): the numerator and the denominator of the rule,
    ready minus the ones whose vector is not proven to match, over eligible minus the zero-retention ones."""
    trusted = max(counts["ready"] - counts["ready_unverified"] - counts["vector_incoherent"], 0)
    expected = max(counts["eligible"] - counts["skipped_zdr"], 0)
    return trusted, expected


def meets(trusted, expected, threshold):
    # ready*100 >= expected*threshold: 990 of 1000 at 99 percent is on the line, not a rounding error below it.
    return trusted * 100 >= expected * threshold - EPS


def judge_coverage(label, report, threshold=COVERAGE_THRESHOLD_PERCENT):
    """(failures, one summary line) for one organisation's coverage report."""
    problems = validate_coverage_report(report)
    if problems:
        return ([f"{label}: the coverage report is malformed ({'; '.join(problems)}): never a pass"],
                f"coverage   {label:<11} malformed")
    failures = []
    counts, policy, evaluation = report["counts"], report["policy"], report["evaluation"]
    if report["org_id"] != ORG_IDS[label]:
        failures.append(f"{label}: the coverage report is for {report['org_id']}, not {ORG_IDS[label]}")
    if counts["eligible"] == 0:
        failures.append(f"{label}: the organisation holds no eligible units: an empty corpus is not coverage")
        return failures, f"coverage   {label:<11} no eligible units"

    trusted, expected = coverage_figures(counts)
    pct = 100.0 * trusted / expected if expected else 0.0
    own = evaluation["threshold_percent"]
    if policy["contextual_retrieval"] == "off":
        own_status = "skipped_policy"
    else:
        own_status = "passed" if expected and meets(trusted, expected, own) else "failed"
    if own_status != evaluation["status"]:
        failures.append(f"{label}: the coverage report's own status ({evaluation['status']}) disagrees with its counts "
                        f"({own_status} at its {own} percent threshold): the rule has drifted between the service and this gate")

    if label == "baseline":
        if policy["contextual_retrieval"] != "off":
            failures.append(f"baseline: the organisation is not raw: its contextual_retrieval policy is "
                            f"{policy['contextual_retrieval']}, not off, so the A/B would compare contextual retrieval with itself")
        if counts["ready"] > 0 or counts["legacy"] > 0:
            failures.append(f"baseline: the organisation holds contexts (ready {counts['ready']}, legacy {counts['legacy']}): "
                            f"it is not raw, so the A/B would compare contextual retrieval with itself")
        return failures, (f"coverage   baseline    policy {policy['contextual_retrieval']}, {counts['skipped_policy']} of "
                          f"{counts['eligible']} units skipped_policy, {counts['ready']} ready")

    if policy["contextual_retrieval"] == "off":
        failures.append("contextual: the organisation's contextual_retrieval policy is off, so no context is generated for it")
    elif not expected:
        failures.append("contextual: no unit is expected to have a context (every eligible unit is zero-retention)")
    elif not meets(trusted, expected, threshold):
        failures.append(f"contextual: coverage is {pct:.2f}% ({trusted} of {expected} expected units have a verified context), "
                        f"below the {threshold:g}% threshold")
    if counts["vector_incoherent"] > 0:
        failures.append(f"contextual: {counts['vector_incoherent']} unit(s) are vector_incoherent: the stored vector was not "
                        f"built from the stored context, so the contextual cell would measure something else")
    return failures, (f"coverage   contextual  {trusted}/{expected} = {pct:.2f}% (threshold {threshold:g}%), "
                      f"{counts['vector_incoherent']} incoherent, {counts['failed_permanent']} failed_permanent")


def check_coverage(evidence, require):
    """[(label, message)] and the summary lines. `evidence` maps a label to its parsed report (or None)."""
    failures, lines, valid = [], [], {}
    for label in ("baseline", "contextual"):
        report = evidence.get(label)
        if report is None:
            if require:
                failures.append((label, f"{label}: the coverage evidence is missing (coverage-{label}.json): run "
                                        f"python3 scripts/eval-service.py coverage <results-dir>"))
            continue
        problems, line = judge_coverage(label, report)
        failures.extend((label, message) for message in problems)
        lines.append(line)
        if not validate_coverage_report(report) and report["counts"]["eligible"] > 0:
            valid[label] = report
    if len(valid) == 2:
        eligible = {label: report["counts"]["eligible"] for label, report in valid.items()}
        if eligible["baseline"] != eligible["contextual"]:
            failures.append(("contextual", f"the two organisations hold different corpora (baseline {eligible['baseline']} eligible units, "
                                           f"contextual {eligible['contextual']}): the A/B is not comparable"))
    return failures, lines


# ── the keyword-index parity evidence ────────────────────────────────────────

PARITY_GATE = "sparse_index_in_sync"


def judge_gates(label, report):
    """[message] for one organisation's release-gate report: the keyword index must hold what Postgres holds."""
    if not isinstance(report, dict) or not isinstance(report.get("gates"), list):
        return [f"{label}: the release-gate report is malformed: never a pass"]
    found = [g for g in report["gates"] if isinstance(g, dict) and g.get("gate") == PARITY_GATE]
    if not found:
        return [f"{label}: the release-gate report does not carry {PARITY_GATE}: never a pass"]
    gate = found[0]
    status, passed = gate.get("status"), gate.get("passed")
    if status == "unavailable":
        return [f"{label}: {PARITY_GATE} is unavailable ({gate.get('message', 'no reason given')}): unread evidence is never a pass"]
    if passed is True and status == "passed":
        return []
    if passed is True:
        return [f"{label}: {PARITY_GATE} says passed but its status is {status!r}: the report contradicts itself"]
    return [f"{label}: {PARITY_GATE} failed: {gate.get('message', 'no message')}"]


def check_parity(evidence, require):
    failures = []
    for label in ("baseline", "contextual"):
        report = evidence.get(label)
        if report is None:
            if require:
                failures.append((label, f"{label}: the parity evidence is missing (gates-{label}.json): run "
                                        f"python3 scripts/eval-service.py gates <results-dir>"))
            continue
        failures.extend((label, message) for message in judge_gates(label, report))
    return failures


def load_evidence(results_dir, prefix):
    """{label: parsed JSON or None}; an unreadable file is a malformed report (a dict that fails validation)."""
    out = {}
    for label in ORG_IDS:
        path = os.path.join(results_dir, f"{prefix}-{label}.json")
        if not os.path.exists(path):
            out[label] = None
            continue
        try:
            out[label] = _load_json(path)
        except (OSError, ValueError):
            out[label] = {"malformed": True}
    return out


# ── the persisted record (harness-record-v1) ─────────────────────────────────

RECORD_SCHEMA = "harness-record-v1"
STRATEGY_RE = re.compile(r"^harness:[a-z0-9][a-z0-9._-]{0,62}$")
CELL_RE = re.compile(r"^[a-z0-9][a-z0-9._-]{0,62}$")
RUN_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{3,63}$")
CLASS_RE = re.compile(r"^[a-z][a-z0-9_]{0,47}$")
CORPUS_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._@:/-]{0,199}$")
DIGEST_RE = re.compile(r"^[0-9a-f]{64}$")
CONFIG_KEY_RE = re.compile(r"^[a-z][a-z0-9_]{0,31}$")
VERDICTS = ("pass", "fail", "unbaselined", "promote", "hold")
KINDS = ("cell", "promotion")
RERANKS = ("on", "off", "default")
ORG_LABELS = ("baseline", "contextual")
SCORECARD_KEYS = {"strategy", "queries_run", "golden_queries", "mean_recall_at_10", "mean_ndcg_at_10", "mean_mrr",
                  "mean_latency_ms", "p95_latency_ms", "by_class", "harness"}
HARNESS_KEYS = {"schema", "kind", "cell", "run_id", "org_label", "query_class", "rerank", "zero_result", "corpus",
                "gate", "config"}
MAX_REASONS, MAX_REASON_LEN = 20, 500


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def validate_record(body):
    """The problems that make the service refuse a record ([] when it would accept it). This mirrors
    data-quality-go's eval.ValidateRecord rule for rule; the pinned goldens of harness-record-v1 (valid ones
    and the `invalid.*` ones) are the proof that both sides agree."""
    if not isinstance(body, dict):
        return ["the record is not a JSON object"]
    problems = []
    extra = sorted(set(body) - {"strategy", "corpus", "scorecard"})
    if extra:
        problems.append(f"unknown field(s) {extra} in the record")
    strategy, corpus, card = body.get("strategy"), body.get("corpus"), body.get("scorecard")
    if not isinstance(strategy, str) or not STRATEGY_RE.match(strategy):
        problems.append("strategy must be harness:<cell> (lower case letters, digits, . _ -)")
    if not isinstance(corpus, str) or not CORPUS_RE.match(corpus):
        problems.append("corpus is missing or has characters outside A-Z a-z 0-9 . _ @ : / -")
    if not isinstance(card, dict):
        problems.append("scorecard is missing")
        return problems
    extra = sorted(set(card) - SCORECARD_KEYS - {"details"})
    if extra:
        problems.append(f"unknown field(s) {extra} in the scorecard")
    if card.get("details"):
        problems.append("details must be absent: a record is a summary, the per-query rows stay in the run's artifacts")
    if card.get("strategy") != strategy:
        problems.append("scorecard.strategy must equal strategy")
    queries, golden = card.get("queries_run"), card.get("golden_queries")
    if not (isinstance(queries, int) and not isinstance(queries, bool) and queries >= 1):
        problems.append("queries_run must be an integer of at least 1")
        queries = None
    if not (isinstance(golden, int) and not isinstance(golden, bool) and 1 <= golden <= (queries or golden)):
        problems.append("golden_queries must be an integer from 1 to queries_run")
        golden = None
    for key in ("mean_recall_at_10", "mean_ndcg_at_10", "mean_mrr"):
        if not _number(card.get(key)) or not 0.0 <= card[key] <= 1.0:
            problems.append(f"{key} must be a number from 0 to 1")
    for key in ("mean_latency_ms", "p95_latency_ms"):
        if not _number(card.get(key)) or card[key] < 0:
            problems.append(f"{key} must be a number of at least 0")
    by_class = card.get("by_class")
    if by_class is not None:
        if not isinstance(by_class, dict) or not by_class:
            problems.append("by_class must be a non-empty object when present")
        else:
            total = 0
            for name, summary in by_class.items():
                if not CLASS_RE.match(str(name)) or not isinstance(summary, dict):
                    problems.append(f"by_class has an invalid class {name!r}")
                    continue
                n = summary.get("queries_run")
                if not (isinstance(n, int) and not isinstance(n, bool) and n >= 1):
                    problems.append(f"by_class.{name}.queries_run must be an integer of at least 1")
                else:
                    total += n
                for key in ("mean_recall_at_10", "mean_ndcg_at_10", "mean_mrr"):
                    if not _number(summary.get(key)) or not 0.0 <= summary[key] <= 1.0:
                        problems.append(f"by_class.{name}.{key} must be a number from 0 to 1")
                extra = sorted(set(summary) - {"queries_run", "mean_recall_at_10", "mean_ndcg_at_10", "mean_mrr"})
                if extra:
                    problems.append(f"unknown field(s) {extra} in by_class.{name}")
            if golden is not None and total != golden:
                problems.append(f"by_class queries add up to {total}, not golden_queries {golden}")
    harness = card.get("harness")
    if not isinstance(harness, dict):
        problems.append("harness is missing")
        return problems
    extra = sorted(set(harness) - HARNESS_KEYS)
    if extra:
        problems.append(f"unknown field(s) {extra} in harness")
    if harness.get("schema") != RECORD_SCHEMA:
        problems.append(f"harness.schema must be {RECORD_SCHEMA}")
    if harness.get("kind") not in KINDS:
        problems.append("harness.kind must be cell or promotion")
    cell = harness.get("cell")
    if not isinstance(cell, str) or not CELL_RE.match(cell):
        problems.append("harness.cell is missing or invalid")
    elif isinstance(strategy, str) and strategy != f"harness:{cell}":
        problems.append("strategy must be harness:<harness.cell>")
    if not isinstance(harness.get("run_id"), str) or not RUN_ID_RE.match(harness["run_id"]):
        problems.append("harness.run_id must be 4 to 64 characters of A-Z a-z 0-9 . _ -")
    if harness.get("org_label") not in ORG_LABELS:
        problems.append("harness.org_label must be baseline or contextual")
    if not isinstance(harness.get("query_class"), str) or not CLASS_RE.match(harness["query_class"]):
        problems.append("harness.query_class is missing or invalid")
    if harness.get("rerank") not in RERANKS:
        problems.append("harness.rerank must be on, off or default")
    zero = harness.get("zero_result")
    if not (isinstance(zero, int) and not isinstance(zero, bool) and zero >= 0 and (queries is None or zero <= queries)):
        problems.append("harness.zero_result must be an integer from 0 to queries_run")
    info = harness.get("corpus")
    if not isinstance(info, dict) or set(info) != {"id", "manifest_digest", "documents"}:
        problems.append("harness.corpus must carry exactly id, manifest_digest and documents")
    else:
        if not isinstance(info["id"], str) or not CORPUS_RE.match(info["id"]):
            problems.append("harness.corpus.id is invalid")
        if not isinstance(info["manifest_digest"], str) or not DIGEST_RE.match(info["manifest_digest"]):
            problems.append("harness.corpus.manifest_digest must be 64 hex characters")
        if not (isinstance(info["documents"], int) and not isinstance(info["documents"], bool) and info["documents"] >= 1):
            problems.append("harness.corpus.documents must be an integer of at least 1")
    verdict = harness.get("gate")
    if not isinstance(verdict, dict) or set(verdict) != {"verdict", "reasons"}:
        problems.append("harness.gate must carry exactly verdict and reasons")
    else:
        if verdict["verdict"] not in VERDICTS:
            problems.append(f"harness.gate.verdict must be one of {', '.join(VERDICTS)}")
        reasons = verdict["reasons"]
        if not isinstance(reasons, list) or len(reasons) > MAX_REASONS or any(
                not isinstance(r, str) or not r or len(r) > MAX_REASON_LEN for r in reasons):
            problems.append(f"harness.gate.reasons must be at most {MAX_REASONS} texts of 1 to {MAX_REASON_LEN} characters")
    config = harness.get("config")
    if config is not None:
        if not isinstance(config, dict) or len(config) > 16 or any(
                not CONFIG_KEY_RE.match(str(k)) or not isinstance(v, str) or len(v) > 200 for k, v in config.items()):
            problems.append("harness.config must be at most 16 short text values under lower case keys")
    return problems


def load_corpus_info(path):
    try:
        manifest = _load_json(path)
    except (OSError, ValueError) as exc:
        raise UsageError(f"cannot read the corpus manifest {path}: {exc}")
    digest = manifest.get("manifest_digest")
    corpus_id = manifest.get("corpus_id")
    documents = manifest.get("documents")
    if not (isinstance(corpus_id, str) and isinstance(digest, str) and DIGEST_RE.match(digest) and isinstance(documents, list) and documents):
        raise UsageError(f"{path} is not a corpus manifest (corpus_id, manifest_digest and documents are needed)")
    return {"id": corpus_id, "manifest_digest": digest, "documents": len(documents)}


def _round(value):
    return round(float(value), 6)


def build_cell_record(cell, metrics, queries, verdict, reasons, *, run_id, corpus):
    spec = CELL_SPECS[cell]
    summary = {key: _round(metrics[key]) for key in ("recall_at_10", "ndcg_at_10", "mrr")}
    body = {
        "strategy": f"harness:{cell}",
        "corpus": f"{corpus['id']}@{corpus['manifest_digest'][:12]}",
        "scorecard": {
            "strategy": f"harness:{cell}",
            "queries_run": metrics["queries"],
            "golden_queries": metrics["queries"],
            "mean_recall_at_10": summary["recall_at_10"],
            "mean_ndcg_at_10": summary["ndcg_at_10"],
            "mean_mrr": summary["mrr"],
            "mean_latency_ms": _round(metrics["mean_latency_ms"]),
            "p95_latency_ms": _round(metrics["p95_latency_ms"]),
            "by_class": {spec["class"]: {"queries_run": metrics["queries"], "mean_recall_at_10": summary["recall_at_10"],
                                         "mean_ndcg_at_10": summary["ndcg_at_10"], "mean_mrr": summary["mrr"]}},
            "harness": {
                "schema": RECORD_SCHEMA, "kind": "cell", "cell": cell, "run_id": run_id,
                "org_label": spec["org_label"], "query_class": spec["class"], "rerank": spec["rerank"],
                "zero_result": metrics["zero_result"],
                "corpus": {"id": corpus["id"], "manifest_digest": corpus["manifest_digest"], "documents": corpus["documents"]},
                "gate": {"verdict": verdict, "reasons": _clip(reasons)},
            },
        },
    }
    return {"org_label": spec["org_label"], "org_id": ORG_IDS[spec["org_label"]],
            "idempotency_key": f"harness-{run_id}-{cell}", "body": body}


def _clip(reasons):
    clipped = [r if len(r) <= MAX_REASON_LEN else r[:MAX_REASON_LEN - 1] + "…" for r in reasons[:MAX_REASONS]]
    return clipped


IDEMPOTENCY_KEY_MAX = 120  # the handler's own bound on an Idempotency-Key (requestIdempotencyKey: 8 to 120; the table's CHECK is 8 to 128)


def write_record(directory, name, record):
    problems = validate_record(record["body"])
    if problems:
        raise UsageError(f"the record for {name} would be refused by the service: {'; '.join(problems)}")
    if not 8 <= len(record["idempotency_key"]) <= IDEMPOTENCY_KEY_MAX:
        raise UsageError(f"the idempotency key {record['idempotency_key']!r} of {name} is not 8 to {IDEMPOTENCY_KEY_MAX} characters: "
                         f"use a shorter --run-id")
    os.makedirs(directory, exist_ok=True)
    path = os.path.join(directory, f"record-{name}.json")
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(record, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return path


# ── the promotion gate ───────────────────────────────────────────────────────

def _json_or_problem(path, problems):
    try:
        return _load_json(path)
    except OSError:
        return None
    except ValueError:
        problems.append(f"{os.path.basename(path)} is not valid JSON")
        return None


def load_variant(results_dir, name):
    """Everything the promotion gate reads about one variant, and the names of the files that are absent."""
    bundle = {"name": name, "missing": [], "problems": [], "rr_off": None, "rr_on": [], "lexical": None,
              "coverage": None, "state": None}

    def cell(filename):
        path = os.path.join(results_dir, filename)
        if not os.path.exists(path):
            bundle["missing"].append(filename)
            return None
        try:
            return cell_detail(path)
        except (OSError, ValueError, KeyError, TypeError, SystemExit) as exc:
            bundle["problems"].append(f"{filename} cannot be scored ({exc})")
            return None

    bundle["rr_off"] = cell(f"cell-{name}-rr-off.json")
    for index in range(1, PROMOTION["rerank_on_runs"] + 1):
        run = cell(f"cell-{name}-rr-on-{index}.json")
        if run is not None:
            run["index"] = index
            bundle["rr_on"].append(run)
    bundle["lexical"] = cell(f"cell-{name}-lexical.json")
    for key, filename in (("coverage", f"coverage-{name}.json"), ("state", f"state-{name}.json")):
        path = os.path.join(results_dir, filename)
        if not os.path.exists(path):
            bundle["missing"].append(filename)
            continue
        bundle[key] = _json_or_problem(path, bundle["problems"])
    return bundle


def judge_state(name, state):
    """[reason] when the state of a variant is not the one it declared (or cannot be read)."""
    if not isinstance(state, dict) or not isinstance(state.get("declared"), dict) or not isinstance(state.get("observed"), dict):
        return [f"state of {name} is malformed: never a pass"]
    declared, observed = state["declared"], state["observed"]
    policy = observed.get("policy") if isinstance(observed.get("policy"), dict) else {}
    provenance = observed.get("provenance")
    if not isinstance(provenance, list):
        return [f"state of {name} carries no provenance: never a pass"]
    reasons = []
    if policy.get("contextual_retrieval") == "off":
        reasons.append(f"state of {name}: the organisation's contextual_retrieval policy is off, so no context was generated for it")
    ready = [p for p in provenance if isinstance(p, dict) and p.get("context_status") == "ready" and _is_count(p.get("units"))]
    total = sum(p["units"] for p in ready)
    if not total:
        reasons.append(f"state of {name}: no ready unit carries provenance")
    else:
        for field in ("prompt_version", "model"):
            want = declared.get(field)
            if not want:
                continue
            tally = {}
            for p in ready:
                tally[p.get(field)] = tally.get(p.get(field), 0) + p["units"]
            share = 100.0 * tally.get(want, 0) / total
            if share < COVERAGE_THRESHOLD_PERCENT - EPS:
                top = sorted(tally.items(), key=lambda kv: (-kv[1], str(kv[0])))[0][0]
                reasons.append(f"state of {name} is not the declared one: {share:.1f}% of its ready units carry {field} "
                               f"{want} (the most common is {top}); the measurement would be of another state under this name")
    flag = declared.get("rerank_input_include_context")
    if flag is not None:
        retrieval = observed.get("retrieval") if isinstance(observed.get("retrieval"), dict) else {}
        want = "1" if flag else "0"
        if retrieval.get("rerank_input_include_context") != want:
            reasons.append(f"state of {name}: rerank_input_include_context is declared {flag} but retrieval-engine runs with "
                           f"{retrieval.get('rerank_input_include_context')!r}")
    return reasons


def _mean(runs, metric):
    return statistics.fmean(run["score"][metric] for run in runs)


def promotion_verdict(candidate, default, rules=None):
    """Judge `candidate` against `default` (both from load_variant). Pure and deterministic."""
    rules = rules or PROMOTION
    needed = rules["rerank_on_runs"]
    checks, reasons = [], []

    def record(name, ok, detail, reason=None):
        checks.append({"name": name, "ok": bool(ok), "detail": detail})
        if not ok:
            reasons.append(reason or detail)

    both = (candidate, default)
    incomplete = []
    for variant in both:
        for filename in variant["missing"]:
            if "-rr-on-" not in filename:
                incomplete.append(f"evidence incomplete: {filename} is missing")
        incomplete.extend(f"evidence incomplete: {problem}" for problem in variant["problems"])
        if len(variant["rr_on"]) < needed:
            incomplete.append(f"evidence incomplete: {variant['name']} has {len(variant['rr_on'])} rerank-on run(s); "
                              f"{needed} rerank-on runs are needed")
    checks.append({"name": "evidence_complete", "ok": not incomplete,
                   "detail": "every file of both variants is present" if not incomplete else "; ".join(incomplete)})
    reasons.extend(incomplete)

    # 1. non-inferiority with the reranker off, on the same queries
    c_off, d_off = candidate["rr_off"], default["rr_off"]
    if c_off and d_off:
        if c_off["queries"] != d_off["queries"]:
            record("non_inferiority_rerank_off", False,
                   "the rerank-off cells were not run on the same queries",
                   f"non-inferiority cannot be judged: the rerank-off cells of {candidate['name']} and {default['name']} "
                   f"were not run on the same queries ({len(c_off['queries'])} against {len(d_off['queries'])})")
        else:
            d_ndcg = c_off["score"]["ndcg_at_10"] - d_off["score"]["ndcg_at_10"]
            d_mrr = c_off["score"]["mrr"] - d_off["score"]["mrr"]
            ok = d_ndcg >= -rules["max_ndcg_drop"] - EPS and d_mrr >= -rules["max_mrr_drop"] - EPS
            record("non_inferiority_rerank_off", ok,
                   f"nDCG@10 {d_ndcg:+.4f} and MRR {d_mrr:+.4f} against the default (allowed {-rules['max_ndcg_drop']:+.2f} each)",
                   f"non-inferiority failed with the reranker off: nDCG@10 {d_ndcg:+.4f} and MRR {d_mrr:+.4f} against "
                   f"{default['name']} (each must be at least {-rules['max_ndcg_drop']:+.2f})")

    # 2. coverage of each variant's own contextual state
    for variant in both:
        report = variant["coverage"]
        if report is None:
            continue
        problems, line = judge_coverage("contextual", report)
        summary = f"{variant['name']}: {line.split('contextual', 1)[1].strip()}"
        record("coverage", not problems, summary, f"coverage of {variant['name']}: {'; '.join(problems)}")

    # 3. the state is the declared one
    for variant in both:
        if variant["state"] is None:
            continue
        problems = judge_state(variant["name"], variant["state"])
        record("state", not problems, f"{variant['name']}: the state is the declared one" if not problems else "; ".join(problems),
               "; ".join(problems) if problems else None)

    # 4. the exact-identifier class unchanged
    c_lex, d_lex = candidate["lexical"], default["lexical"]
    if c_lex and d_lex:
        c, d = c_lex["score"], d_lex["score"]
        problems = []
        if c_lex["queries"] != d_lex["queries"]:
            problems.append("the two lexical cells were not run on the same queries")
        if c["recall_at_10"] < d["recall_at_10"] - EPS:
            problems.append(f"recall fell from {d['recall_at_10']:.4f} to {c['recall_at_10']:.4f}: an identifier query was lost")
        if c["zero_result"] > 0:
            problems.append(f"{c['zero_result']} identifier queries returned nothing")
        if c["ndcg_at_10"] < d["ndcg_at_10"] - rules["lexical_ndcg_drop"] - EPS:
            problems.append(f"nDCG fell from {d['ndcg_at_10']:.4f} to {c['ndcg_at_10']:.4f}")
        record("exact_identifier_class", not problems,
               "no identifier query lost" if not problems else "; ".join(problems),
               f"the exact-identifier class changed: {'; '.join(problems)}")

    # 5. the rerank-on confirmation: three complete runs each, and the means no worse than the tolerance
    run_problems = []
    for variant in both:
        if not variant["rr_off"]:
            continue
        off_n = len(variant["rr_off"]["queries"])
        floor = int(off_n * MIN_QUERY_FRACTION)
        for run in variant["rr_on"]:
            if run["score"]["queries"] < floor:
                run_problems.append(f"rerank-on run {run['index']} of {variant['name']} did not complete "
                                    f"({run['score']['queries']} of {off_n} queries)")
    if len(candidate["rr_on"]) == needed and len(default["rr_on"]) == needed:
        c_n, c_m = _mean(candidate["rr_on"], "ndcg_at_10"), _mean(candidate["rr_on"], "mrr")
        d_n, d_m = _mean(default["rr_on"], "ndcg_at_10"), _mean(default["rr_on"], "mrr")
        slack = rules["rerank_on_slack"]
        means_ok = c_n >= d_n - slack - EPS and c_m >= d_m - slack - EPS
        detail = (f"mean of {needed} runs: nDCG@10 {c_n:.4f} against {d_n:.4f} ({c_n - d_n:+.4f}), MRR {c_m:.4f} against "
                  f"{d_m:.4f} ({c_m - d_m:+.4f}), allowed {-slack:+.3f} each")
        checks.append({"name": "rerank_on_confirmation", "ok": bool(means_ok and not run_problems), "detail": detail})
        reasons.extend(run_problems)
        if not means_ok:
            reasons.append(f"rerank-on confirmation failed: {detail}")

    numbers = {}
    if c_off and d_off:
        numbers["rerank_off"] = {
            "candidate": {k: _round(c_off["score"][k]) for k in ("queries", "recall_at_10", "ndcg_at_10", "mrr")},
            "default": {k: _round(d_off["score"][k]) for k in ("queries", "recall_at_10", "ndcg_at_10", "mrr")},
        }
    if len(candidate["rr_on"]) == needed and len(default["rr_on"]) == needed:
        numbers["rerank_on_means"] = {
            "candidate": {"ndcg_at_10": _round(_mean(candidate["rr_on"], "ndcg_at_10")), "mrr": _round(_mean(candidate["rr_on"], "mrr"))},
            "default": {"ndcg_at_10": _round(_mean(default["rr_on"], "ndcg_at_10")), "mrr": _round(_mean(default["rr_on"], "mrr"))},
        }
    return {
        "schema": "eval-promotion-v1",
        "candidate": candidate["name"],
        "default": default["name"],
        "verdict": "hold" if reasons or not checks or not all(check["ok"] for check in checks) else "promote",
        "reasons": reasons,
        "checks": checks,
        "numbers": numbers,
    }


def build_promotion_record(verdict, candidate, *, run_id, corpus):
    cell = f"promotion-{verdict['candidate']}"
    off = candidate["rr_off"]["score"]
    metrics = {key: off[key] for key in ("queries", "zero_result", "recall_at_10", "ndcg_at_10", "mrr", "mean_latency_ms", "p95_latency_ms")}
    summary = {key: _round(metrics[key]) for key in ("recall_at_10", "ndcg_at_10", "mrr")}
    body = {
        "strategy": f"harness:{cell}",
        "corpus": f"{corpus['id']}@{corpus['manifest_digest'][:12]}",
        "scorecard": {
            "strategy": f"harness:{cell}",
            "queries_run": metrics["queries"], "golden_queries": metrics["queries"],
            "mean_recall_at_10": summary["recall_at_10"], "mean_ndcg_at_10": summary["ndcg_at_10"], "mean_mrr": summary["mrr"],
            "mean_latency_ms": _round(metrics["mean_latency_ms"]), "p95_latency_ms": _round(metrics["p95_latency_ms"]),
            "by_class": {"natural_language": {"queries_run": metrics["queries"], "mean_recall_at_10": summary["recall_at_10"],
                                              "mean_ndcg_at_10": summary["ndcg_at_10"], "mean_mrr": summary["mrr"]}},
            "harness": {
                "schema": RECORD_SCHEMA, "kind": "promotion", "cell": cell, "run_id": run_id,
                "org_label": "contextual", "query_class": "natural_language", "rerank": "off",
                "zero_result": metrics["zero_result"],
                "corpus": {"id": corpus["id"], "manifest_digest": corpus["manifest_digest"], "documents": corpus["documents"]},
                "gate": {"verdict": verdict["verdict"], "reasons": _clip(verdict["reasons"])},
                "config": {"candidate": verdict["candidate"], "default": verdict["default"]},
            },
        },
    }
    return {"org_label": "contextual", "org_id": ORG_IDS["contextual"], "idempotency_key": f"harness-{run_id}-{cell}", "body": body}


# ── command line ─────────────────────────────────────────────────────────────

class _Parser(argparse.ArgumentParser):
    def error(self, message):
        raise UsageError(message)


def _switch(name, flag):
    value = os.environ.get(name, "")
    if value in ("", "0"):
        return flag
    if value == "1":
        return True
    raise UsageError(f"{name} must be 0 or 1 (or unset), got {value!r}")


def _parser():
    parser = _Parser(prog="eval-gate.py", description="Retrieval quality gate (see the module docstring).")
    parser.add_argument("results_dir", nargs="?", default=HERE)
    parser.add_argument("--baseline", default=BASELINE, help="the committed baseline (default scripts/eval-baseline.json)")
    parser.add_argument("--update", action="store_true", help="re-baseline: merge the cells of the results directory")
    parser.add_argument("--replace", action="store_true", help="re-baseline: replace the file, dropping absent cells")
    parser.add_argument("--accept-lower", action="store_true", help="allow --update to LOWER a cell past its tolerance")
    parser.add_argument("--allow-small", action="store_true", help="allow --update to baseline a cell below the minimum size")
    parser.add_argument("--require-coverage", action="store_true", help="a missing coverage report is a failure")
    parser.add_argument("--require-parity", action="store_true", help="a missing release-gate report is a failure")
    parser.add_argument("--promote", metavar="CANDIDATE", help="judge a variant for promotion")
    parser.add_argument("--against", metavar="DEFAULT", help="the variant it must not be worse than")
    parser.add_argument("--records-out", metavar="DIR", help="write the records the service stores")
    parser.add_argument("--run-id", help="the id of this run (records)")
    parser.add_argument("--corpus-manifest", default=CORPUS_MANIFEST, help="the corpus manifest (records)")
    return parser


def _update(args, detail):
    existing = {}
    if os.path.exists(args.baseline):
        existing = _load_json(args.baseline)
    current = {cell: d["score"] for cell, d in detail.items()}
    refusals = []
    for cell, metrics in sorted(current.items()):
        if metrics["queries"] < MIN_QUERIES_TO_BASELINE and not args.allow_small:
            refusals.append(f"{cell}: {metrics['queries']} queries is too few queries to baseline "
                            f"(at least {MIN_QUERIES_TO_BASELINE}; --allow-small records it anyway)")
        if metrics["zero_result"] > 0:
            refusals.append(f"{cell}: {metrics['zero_result']} queries returned nothing; a baseline must not bake in a dead arm")
        was = existing.get(cell)
        if was is not None and not args.accept_lower:
            for metric, slack in TOLERANCE.items():
                if metrics[metric] < was[metric] - slack - EPS:
                    refusals.append(f"{cell}: {metric} {was[metric]:.4f} -> {metrics[metric]:.4f} would LOWER the baseline beyond its "
                                    f"{slack:.3f} allowance; pass --accept-lower to record a deliberate decrease, and say why in the commit")
    if refusals:
        print("BASELINE NOT UPDATED:")
        for message in refusals:
            print(f"  - {message}")
        return 1

    kept = sorted(set(existing) - set(current))
    merged = {cell: baseline_entry(m) for cell, m in current.items()} if args.replace else {
        **existing, **{cell: baseline_entry(m) for cell, m in current.items()}}
    with open(args.baseline, "w", encoding="utf-8") as handle:
        json.dump(merged, handle, indent=1, sort_keys=True)
    print(f"baseline updated from {args.results_dir}:")
    for cell, m in sorted(current.items()):
        mark = "new " if existing.get(cell) is None else "     "
        print(f"  {mark}{cell:<22} recall={m['recall_at_10']:.4f} nDCG={m['ndcg_at_10']:.4f} MRR={m['mrr']:.4f} n={m['queries']}")
    if kept:
        # Say this out loud. Silence here is what made the old wholesale
        # rewrite dangerous: a cell vanishing from the baseline is invisible
        # at gate time, because an unbaselined cell only warns.
        verb = "DROPPED (--replace)" if args.replace else "left untouched"
        print(f"\ncells not in {args.results_dir}, {verb}:")
        for cell in kept:
            print(f"    {cell}")
    print(f"\nwrote {args.baseline} — commit it, so a later regression is a visible diff.")
    return 0


def _promote(args):
    candidate_name, default_name = args.promote, args.against
    if not default_name:
        raise UsageError("--promote needs --against <the variant it must not be worse than>")
    for name in (candidate_name, default_name):
        if not VARIANT_RE.match(name):
            raise UsageError(f"{name!r} is not a variant name (lower case letters, digits, . _ -)")
    if candidate_name == default_name:
        # A variant judged against itself is non-inferior to itself on every count: a typo would read as a PROMOTE.
        raise UsageError(f"--promote and --against name the same variant ({candidate_name!r}): a variant cannot be promoted over itself")
    corpus = None
    if args.records_out:
        if not args.run_id or not RUN_ID_RE.match(args.run_id):
            raise UsageError("--records-out needs --run-id (4 to 64 characters of A-Z a-z 0-9 . _ -)")
        corpus = load_corpus_info(args.corpus_manifest)
    candidate = load_variant(args.results_dir, candidate_name)
    default = load_variant(args.results_dir, default_name)
    verdict = promotion_verdict(candidate, default)
    path = os.path.join(args.results_dir, f"promotion-{candidate_name}-vs-{default_name}.json")
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        json.dump(verdict, handle, indent=2, sort_keys=True)
        handle.write("\n")
    print(f"{'check':<28}{'ok':<5}detail")
    print("-" * 100)
    for check in verdict["checks"]:
        print(f"{check['name']:<28}{'yes' if check['ok'] else 'NO':<5}{check['detail']}")
    print()
    print(f"PROMOTION VERDICT: {verdict['verdict'].upper()}  ({candidate_name} against {default_name}; written to {path})")
    for reason in verdict["reasons"]:
        print(f"  - {reason}")
    if args.records_out and candidate["rr_off"]:
        written = write_record(args.records_out, f"promotion-{candidate_name}",
                               build_promotion_record(verdict, candidate, run_id=args.run_id, corpus=corpus))
        print(f"record written to {written}")
    return 0 if verdict["verdict"] == "promote" else 1


def main(argv=None):
    try:
        return _main(argv)
    except UsageError as exc:
        print(f"eval-gate.py: {exc}", file=sys.stderr)
        return 2


def _main(argv):
    parser = _parser()
    args = parser.parse_args(sys.argv[1:] if argv is None else argv)
    args.update = args.update or args.replace
    args.require_coverage = _switch("EVAL_REQUIRE_COVERAGE", args.require_coverage)
    args.require_parity = _switch("EVAL_REQUIRE_PARITY", args.require_parity)
    if args.records_out and not args.run_id:
        raise UsageError("--records-out needs --run-id")
    if args.promote:
        return _promote(args)
    if args.against:
        raise UsageError("--against only goes with --promote")

    detail = collect_detail(args.results_dir)
    if not detail:
        raise SystemExit(
            f"no st-rr-on-*.json in {args.results_dir} — run eval-attribution-study.sh first")

    if args.update:
        return _update(args, detail)

    if not os.path.exists(args.baseline):
        raise SystemExit(
            f"no baseline at {args.baseline}. Establish one with:\n"
            f"  python3 scripts/eval-gate.py {args.results_dir} --update")
    baseline = _load_json(args.baseline)

    current = {cell: d["score"] for cell, d in detail.items()}
    rows, cell_failures, warnings = _check_cells(baseline, current)
    print_table(rows)

    # Evidence is judged whenever it is present, and demanded only when required.
    evidence_failures = []   # [(labels, message)]
    for cells, message in check_query_parity(detail):
        evidence_failures.append((cells, message))
    coverage_failures, coverage_lines = check_coverage(load_evidence(args.results_dir, "coverage"), args.require_coverage)
    for label, message in coverage_failures:
        evidence_failures.append((label, message))
    for label, message in check_parity(load_evidence(args.results_dir, "gates"), args.require_parity):
        evidence_failures.append((label, message))
    if coverage_lines:
        print()
        for line in coverage_lines:
            print(line)
    if not args.require_coverage and not any(os.path.exists(os.path.join(args.results_dir, f"coverage-{l}.json")) for l in ORG_IDS):
        warnings.append("no coverage evidence in the results directory: the contextual cells are not checked for coverage "
                        "(--require-coverage makes that a failure)")

    failures = [message for _, message in cell_failures] + [message for _, message in evidence_failures]

    print()
    for w in warnings:
        print(f"WARNING  {w}")
    if args.records_out:
        corpus = load_corpus_info(args.corpus_manifest)
        for cell in sorted(current):
            if cell not in CELL_SPECS:
                print(f"WARNING  {cell}: not a known cell, no record written")
                continue
            spec = CELL_SPECS[cell]
            reasons = [m for c, m in cell_failures if c == cell]
            for who, m in evidence_failures:
                if who == spec["org_label"] or (isinstance(who, list) and cell in who):
                    reasons.append(m)
            if cell not in baseline:
                verdict = "unbaselined" if not reasons else "fail"
            else:
                verdict = "fail" if reasons else "pass"
            record = build_cell_record(cell, current[cell], detail[cell]["queries"], verdict, reasons,
                                       run_id=args.run_id, corpus=corpus)
            write_record(args.records_out, cell, record)
        print(f"records written to {args.records_out}")
    if failures:
        print(f"\nGATE FAILED ({len(failures)}):")
        for f in failures:
            print(f"  - {f}")
        print("\nIf this change is a deliberate, understood trade-off, re-baseline with"
              "\n  python3 scripts/eval-gate.py <results-dir> --update"
              "\nand say why in the commit message.")
        return 1
    print("GATE PASSED — no metric regressed beyond the measured noise floor.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
