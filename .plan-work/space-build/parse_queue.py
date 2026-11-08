import re, json, collections, sys

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
P = ROOT + "docs/plans/space-hybrid-build-plan-2026-10-14.md"
lines = open(P, encoding="utf-8").read().split("\n")

start = next(i for i, l in enumerate(lines) if l.startswith("### 4.2 "))
end = next(i for i, l in enumerate(lines) if l.startswith("### 4.3 "))
rows = []
for l in lines[start:end]:
    if not l.startswith("| ") or l.startswith("| Slice ") or l.startswith("|---"):
        continue
    cells = [c.strip() for c in l.strip().strip("|").split("|")]
    if len(cells) < 8:
        continue
    sid = cells[0].strip("`")
    if not re.match(r"^(M0-|P-|WS\d\d-)", sid):
        continue
    rows.append({
        "id": sid, "what": cells[1], "lane": cells[2], "ms": cells[3], "size": cells[4],
        "deps_raw": cells[5], "soft_raw": cells[6], "blocks_raw": cells[7],
    })

def deps(raw):
    out = []
    for m in re.finditer(r"((?:M0|P|WS\d\d)-[A-Za-z0-9]+)", raw):
        out.append(m.group(1))
    return out

for r in rows:
    r["deps"] = [d for d in deps(r["deps_raw"]) if d != r["id"]]
    r["cond"] = "[cond]" in r["id"] or "cond" in r["what"].lower()[:6]

w1 = [r for r in rows if r["ms"] in ("M0", "M1")]
ids = {r["id"] for r in w1}
print("total rows", len(rows), "W1 rows", len(w1))
# slices whose milestone says M1 but depend on non-W1 slices
for r in w1:
    ext = [d for d in r["deps"] if d not in ids]
    if ext:
        print("EXTERNAL DEPS", r["id"], ext)

# topological levels inside W1
level = {}
def lv(i, stack=()):
    if i in level:
        return level[i]
    if i in stack:
        print("CYCLE", stack, i); return 0
    r = next(x for x in w1 if x["id"] == i)
    d = [lv(x, stack + (i,)) for x in r["deps"] if x in ids]
    level[i] = (max(d) + 1) if d else 0
    return level[i]
for r in w1:
    lv(r["id"])
by_level = collections.defaultdict(list)
for r in w1:
    by_level[level[r["id"]]].append(r)
for k in sorted(by_level):
    print("LEVEL", k, ", ".join("%s(%s,%s)" % (r["id"], r["lane"], r["size"]) for r in by_level[k]))

json.dump({"rows": w1, "level": level}, open(ROOT + ".plan-work/space-build/queue.json", "w", encoding="utf-8"), indent=1)
lanes = collections.Counter(r["lane"] for r in w1)
print(dict(lanes))
