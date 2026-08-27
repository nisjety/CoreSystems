"""Score the 14-cell attribution study.

Reads st-<cell>-<org>.json produced by study14.sh and reports:
  - the cross-encoder's measured contribution (rr-on vs rr-off), including how
    often the confidence gate emptied a result (the gate only runs when the
    reranker actually scored the list, so it is part of what "rerank on" means);
  - each arm's leave-one-out contribution (rr-on minus loo-<arm>; positive =
    the arm helps);
  - the dense+bm25 floor, i.e. what all four auxiliary arms are jointly worth.

Per-query sign counts are reported alongside means because at n=87 a mean can
be dragged by two or three queries; an effect that is real tends to show up as
a lopsided win/loss count, not just a shifted mean.
"""
import json, math, os, statistics, sys

# Directory holding the st-<cell>-<org>.json outputs — pass it as argv[1]
# (the same directory given to eval-attribution-study.sh); defaults to this
# script's own directory so a colocated run needs no arguments.
SP = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))
CELLS = ["rr-on", "rr-off", "loo-graph", "loo-wiki", "loo-visual", "loo-keyword", "floor"]
ORGS = ["baseline", "contextual"]


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
    assert ndcg <= 1.0 + 1e-9
    return len(found) / len(relevant), ndcg, mrr


def load(cell, org):
    path = os.path.join(SP, f"st-{cell}-{org}.json")
    if not os.path.exists(path):
        return None
    rows = json.load(open(path, encoding="utf-8"))
    per = {r["query"]: golden_metrics(r["retrieved"], r["relevant_ids"]) for r in rows}
    zero = sum(1 for r in rows if not r["retrieved"])
    lat = sorted(r["latency_ms"] for r in rows)
    return per, zero, lat[len(lat) // 2], len(rows)


def mean(per, i):
    return statistics.fmean(v[i] for v in per.values())


def signs(a, b, i):
    """(improved, regressed) counts of b vs a on metric i, per shared query."""
    up = dn = 0
    for q in a.keys() & b.keys():
        d = b[q][i] - a[q][i]
        if d > 1e-9:
            up += 1
        elif d < -1e-9:
            dn += 1
    return up, dn


data = {}
for cell in CELLS:
    for org in ORGS:
        got = load(cell, org)
        if got:
            data[(cell, org)] = got

print("cells loaded:", sorted({c for c, _ in data}), "\n")
head = f"{'cell':<13}{'org':<12}{'recall@10':>10}{'nDCG@10':>9}{'MRR':>8}{'zero':>6}{'p50':>7}"
print(head)
print("-" * len(head))
for cell in CELLS:
    for org in ORGS:
        if (cell, org) not in data:
            continue
        per, zero, p50, n = data[(cell, org)]
        print(f"{cell:<13}{org:<12}{mean(per,0):>10.4f}{mean(per,1):>9.4f}"
              f"{mean(per,2):>8.4f}{zero:>6}{p50:>6.0f}ms")
    print()

print("=== cross-encoder contribution (rr-off -> rr-on) ===")
for org in ORGS:
    if ("rr-on", org) not in data or ("rr-off", org) not in data:
        continue
    on, off = data[("rr-on", org)][0], data[("rr-off", org)][0]
    up, dn = signs(off, on, 2)
    print(f"  {org:<11} d_recall {mean(on,0)-mean(off,0):+.4f}  "
          f"d_nDCG {mean(on,1)-mean(off,1):+.4f}  d_MRR {mean(on,2)-mean(off,2):+.4f}  "
          f"MRR per-query up {up} / down {dn}")

print("\n=== leave-one-out: what each arm contributes (rr-on reference) ===")
print("(positive delta = removing the arm HURT, i.e. the arm helps)")
for arm in ("graph", "wiki", "visual", "keyword"):
    for org in ORGS:
        ref = data.get(("rr-on", org))
        loo = data.get((f"loo-{arm}", org))
        if not ref or not loo:
            continue
        up, dn = signs(loo[0], ref[0], 2)
        print(f"  {arm:<8} {org:<11} "
              f"recall {mean(ref[0],0)-mean(loo[0],0):+.4f}  "
              f"nDCG {mean(ref[0],1)-mean(loo[0],1):+.4f}  "
              f"MRR {mean(ref[0],2)-mean(loo[0],2):+.4f}  "
              f"(arm-on wins {up} / loses {dn})")
    print()

print("=== all four auxiliary arms jointly (floor -> rr-on) ===")
for org in ORGS:
    ref = data.get(("rr-on", org))
    flo = data.get(("floor", org))
    if not ref or not flo:
        continue
    up, dn = signs(flo[0], ref[0], 2)
    print(f"  {org:<11} recall {mean(ref[0],0)-mean(flo[0],0):+.4f}  "
          f"nDCG {mean(ref[0],1)-mean(flo[0],1):+.4f}  "
          f"MRR {mean(ref[0],2)-mean(flo[0],2):+.4f}  "
          f"(full-blend wins {up} / loses {dn})")
