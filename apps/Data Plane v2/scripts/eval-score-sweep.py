"""Score the 87-query w_bm25 sweep with the corrected goldenMetrics.

Reports a bootstrap 95% CI per cell, because the previous 28-query sweep
produced an ordering (0.714 / 0.750 / 0.786) that was one or two queries wide
and therefore not a result. With 87 queries one query is 1.15 points, but that
is still not the same as a significant difference — the interval says whether
any of these weights is actually distinguishable from the others.
"""
import glob, json, math, os, re, statistics

SP = os.path.dirname(os.path.abspath(__file__))
BOOT = 2000
SEED = 20260826  # fixed so the interval is reproducible across runs


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
    assert ndcg <= 1.0 + 1e-9, f"nDCG {ndcg} > 1 — accounting regressed"
    return len(found) / len(relevant), ndcg, mrr


def lcg(seed):
    """Deterministic PRNG — no Math.random equivalent needed, and reproducible."""
    x = seed
    while True:
        x = (1103515245 * x + 12345) & 0x7FFFFFFF
        yield x


def boot_ci(values, rng):
    n = len(values)
    means = []
    for _ in range(BOOT):
        means.append(statistics.fmean(values[next(rng) % n] for _ in range(n)))
    means.sort()
    return means[int(BOOT * 0.025)], means[int(BOOT * 0.975)]


def load(path):
    rows = json.load(open(path, encoding="utf-8"))
    per = [golden_metrics(r["retrieved"], r["relevant_ids"]) for r in rows]
    lat = sorted(r["latency_ms"] for r in rows)
    return per, lat[len(lat) // 2], len(rows)


cells = {}
for path in sorted(glob.glob(os.path.join(SP, "s87-*-w*.json"))):
    m = re.search(r"s87-(baseline|contextual)-w([0-9.]+)\.json$", path)
    if m:
        cells[(m.group(2), m.group(1))] = path

if not cells:
    raise SystemExit("no sweep output found")

weights = sorted({w for w, _ in cells}, key=float)
rng = lcg(SEED)

print(f"87-query sweep at the DEPLOYED blend (all six arms live)")
print(f"bootstrap 95% CI, {BOOT} resamples, fixed seed\n")
head = (f"{'w_bm25':>7} {'org':<11} {'recall@10':>10} {'95% CI':>16} "
        f"{'nDCG@10':>8} {'MRR':>7} {'p50':>7} {'n':>4}")
print(head)
print("-" * len(head))

table = {}
for w in weights:
    for org in ("baseline", "contextual"):
        path = cells.get((w, org))
        if not path:
            continue
        per, p50, n = load(path)
        rec = [x[0] for x in per]
        nd = [x[1] for x in per]
        mr = [x[2] for x in per]
        lo, hi = boot_ci(rec, rng)
        table[(w, org)] = (statistics.fmean(rec), statistics.fmean(nd),
                           statistics.fmean(mr), p50, (lo, hi), n)
        print(f"{w:>7} {org:<11} {statistics.fmean(rec):>10.4f} "
              f"{f'[{lo:.3f}, {hi:.3f}]':>16} {statistics.fmean(nd):>8.4f} "
              f"{statistics.fmean(mr):>7.4f} {p50:>6.0f}ms {n:>4}")
    print()

for org in ("baseline", "contextual"):
    rows = [(w, table[(w, org)]) for w in weights if (w, org) in table]
    if not rows:
        continue
    best_r = max(rows, key=lambda r: r[1][0])
    best_n = max(rows, key=lambda r: r[1][1])
    dep = dict(rows).get("0.2")
    print(f"{org}: best recall at w={best_r[0]} ({best_r[1][0]:.4f}), "
          f"best nDCG at w={best_n[0]} ({best_n[1][1]:.4f})")
    if dep:
        lo, hi = best_r[1][4]
        overlaps = dep[0] >= lo
        print(f"  deployed w=0.2 recall {dep[0]:.4f} - "
              f"{'inside' if overlaps else 'OUTSIDE'} the best cell's 95% CI "
              f"[{lo:.3f}, {hi:.3f}] -> "
              f"{'not distinguishable' if overlaps else 'a real difference'}")


print()
print("contextual minus baseline, at each weight (recall is saturated; the")
print("signal is in ranking quality):")
print(f"{'w_bm25':>7} {'d recall':>9} {'d nDCG':>8} {'d MRR':>8}")
for w in weights:
    b, c = table.get((w, "baseline")), table.get((w, "contextual"))
    if b and c:
        print(f"{w:>7} {c[0]-b[0]:>+9.4f} {c[1]-b[1]:>+8.4f} {c[2]-b[2]:>+8.4f}")

print()
print("nDCG as w_bm25 rises (same org, so this isolates the lexical arm):")
for org in ("baseline", "contextual"):
    seq = [f"{table[(w, org)][1]:.4f}" for w in weights if (w, org) in table]
    first = table[(weights[0], org)][1]
    last = table[(weights[-1], org)][1]
    print(f"  {org:<11} {' -> '.join(seq)}   net {last-first:+.4f}")
