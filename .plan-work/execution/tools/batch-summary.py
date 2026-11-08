"""Print a short per-slice summary of a run-wave task output: outcome, files, lead to-dos, other files, P0/P1 findings."""
import json
import sys

res = json.load(open(sys.argv[1], encoding="utf-8"))["result"]
if isinstance(res, str):
    res = json.loads(res)
for r in res["results"]:
    v = r.get("verifier") or {}
    print(f"== {r['id']} {r['outcome']} status={r.get('status')} verdict={v.get('verdict')} tester={(r.get('tester') or {}).get('all_passed')}")
    print("  files:", [f.split("epic-shaw-4d613b/")[-1] for f in r.get("files_changed", [])])
    for x in v.get("required_fixes") or []:
        print("  FIX:", x[:500])
    for x in r.get("lead_todo", []):
        print("  TODO:", x[:600])
    for x in r.get("needs_other_files", []):
        print("  OTHER:", x[:300])
    for f in r.get("depth_findings", []):
        if f["severity"] in ("P0", "P1"):
            print(f"  {f['severity']} {f['kind']}: {f['text'][:400]}")
    print("  findings total:", len(r.get("depth_findings", [])))
