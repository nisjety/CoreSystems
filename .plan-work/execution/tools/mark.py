"""Record batch results in queue.json and progress.md.
usage: python mark.py B01 "done:WS01-S6,WS02-S5,WS12-S3" "partial:WS09-S1=baseline runs on a second dev server pending,WS11-S1.code=..." [commits "id=sha,..."]
"""
import json
import sys
import datetime

D = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution"
batch = sys.argv[1]
q = json.load(open(D + r"\queue.json", encoding="utf-8"))
byid = {r["id"]: r for r in q}
lines = []
for spec in sys.argv[2:]:
    kind, _, rest = spec.partition(":")
    for item in [x for x in rest.split(",") if x]:
        sid, _, note = item.partition("=")
        r = byid[sid]
        if kind == "done":
            r["status"] = "done"
            r["remaining"] = note or ""
        elif kind == "partial":
            r["status"] = "partial"
            r["remaining"] = note
        elif kind == "blocked":
            r["status"] = "blocked-external"
            r["remaining"] = note
        lines.append(f"- {sid}: {kind}" + (f" ({note})" if note else ""))
json.dump(q, open(D + r"\queue.json", "w", encoding="utf-8"), indent=1, ensure_ascii=False)
with open(D + r"\progress.md", "a", encoding="utf-8") as f:
    f.write(f"\n## {batch} ({datetime.date.today()})\n" + "\n".join(lines) + "\n")
counts = {}
for r in q:
    counts[r["status"]] = counts.get(r["status"], 0) + 1
print(counts)
