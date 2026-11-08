import json, glob, collections, re, os

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
SCRATCH = "C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-practical-mccarthy-1dc567/98a6fa7c-4507-4864-9c20-d81fc3ffb9de/scratchpad/"
q = json.load(open(ROOT + ".plan-work/space-build/queue.json", encoding="utf-8"))
lane = {r["id"]: r["lane"] for r in q["rows"]}
G = {"L0": "plat", "L1": "sc", "L2": "sc", "L3": "run", "L4": "ctl", "L5": "idn", "L6": "app", "L7": "gw", "L8": "spa-room", "L9": "spa-shell"}
LEGGROUP = {".gw": "gw", ".spa": "spa-room", ".regen": "sc"}

by_group = collections.defaultdict(list)  # group -> [(slice, commit, tests, files)]
for p in sorted(glob.glob(ROOT + ".plan-work/space-build/units/*.json")):
    d = json.load(open(p, encoding="utf-8"))
    sid = d.get("slice") or os.path.basename(p)[:-5]
    g = None
    for suf, gg in LEGGROUP.items():
        if sid.endswith(suf):
            g = gg
    if g is None:
        g = G.get(lane.get(sid, "L0"), "plat")
    tests = d.get("tests_owed", [])
    if not tests:
        continue
    by_group[g].append((sid, d.get("commit", ""), tests, d.get("files", [])))

MAXT = 30
units = []
for g, items in sorted(by_group.items()):
    cur, n, k = [], 0, 1
    for it in items:
        if cur and n + len(it[2]) > MAXT:
            units.append((g, k, cur)); cur, n, k = [], 0, k + 1
        cur.append(it); n += len(it[2])
    if cur:
        units.append((g, k, cur))

out = []
for g, k, items in units:
    uid = "T-%s-%d" % (g, k)
    text = []
    for sid, commit, tests, files in items:
        text.append("### %s (commit %s)\nfiles: %s\ntests owed:\n%s" % (sid, commit or "?", ", ".join(files[:25]), "\n".join("- " + t for t in tests)))
    out.append({"id": uid, "group": g, "slices": [i[0] for i in items], "count": sum(len(i[2]) for i in items), "spec": "\n\n".join(text)})
print(len(out), "test units; tests:", sum(u["count"] for u in out))
for u in out:
    print(u["id"], u["count"], u["slices"])

tpl = open(ROOT + ".plan-work/space-build/w1-tests-template.js", encoding="utf-8").read()
open(SCRATCH + "w1-tests.js", "w", encoding="utf-8", newline="\n").write(tpl.replace("__TESTUNITS__", json.dumps(out, indent=1)))
