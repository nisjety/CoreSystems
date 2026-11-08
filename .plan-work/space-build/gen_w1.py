import json, re, sys

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
SCRATCH = "C:/Users/IMAFER~1/AppData/Local/Temp/claude/C--dev-CoresSystem--claude-worktrees-practical-mccarthy-1dc567/98a6fa7c-4507-4864-9c20-d81fc3ffb9de/scratchpad/"
PLAN = ROOT + "docs/plans/space-hybrid-build-plan-2026-10-14.md"

q = json.load(open(ROOT + ".plan-work/space-build/queue.json", encoding="utf-8"))
rows = [r for r in q["rows"] if r["id"] != "M0-BASE"]
level = q["level"]
plan = open(PLAN, encoding="utf-8").read().split("\n")

# workstream subsection ranges
heads = [(i + 1, l) for i, l in enumerate(plan) if re.match(r"^#{3,4} ", l)]
ws_range = {}
for idx, (ln, l) in enumerate(heads):
    m = re.match(r"^#### (WS\d\d) ", l)
    if m:
        end = heads[idx + 1][0] - 1 if idx + 1 < len(heads) else len(plan)
        ws_range[m.group(1)] = (ln, end)

GROUP = {"L0": "plat", "L1": "sc", "L2": "sc", "L3": "run", "L4": "ctl", "L5": "idn", "L6": "app", "L7": "gw", "L8": "spa-room", "L9": "spa-shell", "L10": "dat"}
SIZEW = {"S": 1, "S/M": 2, "M": 3, "L": 5, "XL": 8}

units = []
for r in rows:
    sid = r["id"]
    ws = sid[:4] if sid.startswith("WS") else None
    rng = ws_range.get(ws) if ws else None
    deps = [d for d in r["deps"] if d != "M0-BASE"]
    units.append({
        "id": sid, "what": r["what"], "lane": r["lane"], "group": GROUP.get(r["lane"], "plat"),
        "size": r["size"], "level": level[sid], "deps": deps, "ws": ws,
        "planStart": rng[0] if rng else None, "planEnd": rng[1] if rng else None,
    })
units.sort(key=lambda u: (u["level"], -SIZEW.get(u["size"], 3), u["id"]))

TEMPLATE = open(ROOT + ".plan-work/space-build/w1-template.js", encoding="utf-8").read()
js = TEMPLATE.replace("__UNITS__", json.dumps(units, indent=1))
open(SCRATCH + "w1-build.js", "w", encoding="utf-8", newline="\n").write(js)
print("units", len(units), "groups", sorted(set(u["group"] for u in units)))
print({g: sum(1 for u in units if u["group"] == g) for g in sorted(set(u["group"] for u in units))})
print("ws ranges", {k: v for k, v in ws_range.items() if k in set(u["ws"] for u in units)})
