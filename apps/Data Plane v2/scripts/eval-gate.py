"""Regression gate for retrieval quality. Exits non-zero when metrics fall
below a committed baseline by more than the measured noise floor.

Run it in CI, or by hand before/after any change to fusion weights, rerank
config, chunking, or embedding models:

    python3 scripts/eval-gate.py <results-dir>            # check vs baseline
    python3 scripts/eval-gate.py <results-dir> --update    # re-baseline

`<results-dir>` holds the `st-rr-on-<org>.json` files written by
`eval-attribution-study.sh` (or any run of `eval-run-retrieval.py` with
OUT_FILE named that way). The baseline lives in
`scripts/eval-baseline.json`, committed, so a regression is a diff a reviewer
can see.

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
"""
import glob, json, math, os, statistics, sys

BASELINE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "eval-baseline.json")

# Per-metric slack, from the measured noise above. recall gets ~2 queries.
TOLERANCE = {"recall_at_10": 0.030, "ndcg_at_10": 0.045, "mrr": 0.045}
# Floors that do not depend on the baseline: these catch a dark stage outright.
MIN_QUERIES = 80
MAX_ZERO_RESULT_FRACTION = 0.02


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


def score(path):
    rows = json.load(open(path, encoding="utf-8"))
    per = [golden_metrics(r["retrieved"], r["relevant_ids"]) for r in rows]
    zero = sum(1 for r in rows if not r["retrieved"])
    return {
        "queries": len(rows),
        "zero_result": zero,
        "recall_at_10": statistics.fmean(x[0] for x in per),
        "ndcg_at_10": statistics.fmean(x[1] for x in per),
        "mrr": statistics.fmean(x[2] for x in per),
    }


def collect(results_dir):
    out = {}
    for path in sorted(glob.glob(os.path.join(results_dir, "st-rr-on-*.json"))):
        org = os.path.basename(path)[len("st-rr-on-"):-len(".json")]
        out[org] = score(path)
    return out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    update = "--update" in sys.argv
    results_dir = args[0] if args else os.path.dirname(os.path.abspath(__file__))

    current = collect(results_dir)
    if not current:
        raise SystemExit(
            f"no st-rr-on-*.json in {results_dir} — run eval-attribution-study.sh first")

    if update:
        json.dump(current, open(BASELINE, "w", encoding="utf-8"), indent=1, sort_keys=True)
        print(f"baseline updated from {results_dir}:")
        for org, m in sorted(current.items()):
            print(f"  {org:<12} recall={m['recall_at_10']:.4f} "
                  f"nDCG={m['ndcg_at_10']:.4f} MRR={m['mrr']:.4f} n={m['queries']}")
        print(f"\nwrote {BASELINE} — commit it, so a later regression is a visible diff.")
        return 0

    if not os.path.exists(BASELINE):
        raise SystemExit(
            f"no baseline at {BASELINE}. Establish one with:\n"
            f"  python3 scripts/eval-gate.py {results_dir} --update")
    baseline = json.load(open(BASELINE, encoding="utf-8"))

    failures, warnings = [], []
    print(f"{'org':<12}{'metric':<14}{'baseline':>10}{'current':>10}{'delta':>9}{'slack':>8}  verdict")
    print("-" * 74)
    for org, cur in sorted(current.items()):
        base = baseline.get(org)
        if base is None:
            warnings.append(f"{org}: no baseline entry (new org?) — not gated")
            continue

        if cur["queries"] < MIN_QUERIES:
            failures.append(
                f"{org}: only {cur['queries']} queries scored (min {MIN_QUERIES}) — "
                f"the eval itself did not complete")
        zero_frac = cur["zero_result"] / max(cur["queries"], 1)
        if zero_frac > MAX_ZERO_RESULT_FRACTION:
            failures.append(
                f"{org}: {cur['zero_result']}/{cur['queries']} queries returned NOTHING "
                f"({zero_frac:.1%} > {MAX_ZERO_RESULT_FRACTION:.0%}) — an arm or the "
                f"confidence gate is eating results")

        for metric, slack in TOLERANCE.items():
            b, c = base[metric], cur[metric]
            delta = c - b
            ok = delta >= -slack
            print(f"{org:<12}{metric:<14}{b:>10.4f}{c:>10.4f}{delta:>+9.4f}"
                  f"{slack:>8.3f}  {'ok' if ok else 'REGRESSED'}")
            if not ok:
                failures.append(
                    f"{org}: {metric} {b:.4f} -> {c:.4f} ({delta:+.4f}), "
                    f"beyond the {slack:.3f} noise allowance")

    print()
    for w in warnings:
        print(f"WARNING  {w}")
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
