"""Parse plan 4.2 into the wave W2 queue (M2.1, M2.2, M2.3; conditional rows excluded)."""
import re, json, collections

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
    rows.append({"id": sid, "what": cells[1], "lane": cells[2], "ms": cells[3], "size": cells[4],
                 "deps_raw": cells[5], "soft_raw": cells[6], "blocks_raw": cells[7]})

def deps(raw):
    return [m.group(1) for m in re.finditer(r"((?:M0|P|WS\d\d)-[A-Za-z0-9]+)", raw)]

for r in rows:
    r["deps"] = [d for d in deps(r["deps_raw"]) if d != r["id"]]
    r["cond"] = "[cond]" in r["id"]

ms_of = {r["id"]: r["ms"] for r in rows}
w2 = [r for r in rows if r["ms"].startswith("M2.") and not r["cond"]]
ids = {r["id"] for r in w2}
print("W2 rows", len(w2), collections.Counter(r["ms"] for r in w2))
for r in w2:
    later = [d for d in r["deps"] if d not in ids and not ms_of.get(d, "M1").startswith(("M0", "M1"))]
    if later:
        print("DEPENDS ON A LATER OR UNKNOWN SLICE", r["id"], [(d, ms_of.get(d)) for d in later])

level = {}
def lv(i, stack=()):
    if i in level:
        return level[i]
    if i in stack:
        print("CYCLE", stack, i)
        return 0
    r = next(x for x in w2 if x["id"] == i)
    d = [lv(x, stack + (i,)) for x in r["deps"] if x in ids]
    level[i] = (max(d) + 1) if d else 0
    return level[i]
for r in w2:
    lv(r["id"])
by = collections.defaultdict(list)
for r in w2:
    by[level[r["id"]]].append(r["id"])
for k in sorted(by):
    print("LEVEL", k, len(by[k]), ", ".join(by[k]))
json.dump({"rows": w2, "level": level}, open(ROOT + ".plan-work/space-build/queue-w2.json", "w", encoding="utf-8", newline="\n"), indent=1)
print(dict(collections.Counter(r["lane"] for r in w2)))
