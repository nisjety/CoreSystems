"""Print the compact Workflow args JSON for batch N of the execution queue.
usage: python batch-args.py N [implementer-overrides like WS05-S1=opus,...]"""
import json
import sys

D = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution"
n = int(sys.argv[1])
overrides = {}
if len(sys.argv) > 2:
    for kv in sys.argv[2].split(","):
        k, v = kv.split("=")
        overrides[k] = v

queue = {r["id"]: r for r in json.load(open(D + r"\queue.json", encoding="utf-8"))}
batches = json.load(open(D + r"\batches.json", encoding="utf-8"))
allb = batches["batches"] + batches.get("later_batches_preview", [])
batch = next(b for b in allb if b["batch"] == n)

STRICT = {"authority", "audit", "zdr", "money", "data-loss"}
slices = []
for s in batch["slices"]:
    r = queue[s["id"]]
    risk = [x for x in (r.get("risk") or []) if x != "none"]
    review = "opus" if (r["importance"] == "opus" and set(risk) & STRICT) else "none"
    notes = []
    if r.get("dependency_notes"):
        dn = r["dependency_notes"]
        notes.append("Dependency notes: " + (" ".join(dn) if isinstance(dn, list) else str(dn)))
    if r.get("lead_shared_files"):
        notes.append("Do NOT edit these shared files (the lead applies your exact lines after the batch; put them in lead_todo as 'shared-edit <file>: <exact lines>'): " + ", ".join(r["lead_shared_files"]))
    if r.get("needs_external"):
        notes.append("External need (do not fake): " + r["needs_external"])
    rec = {
        "id": r["id"],
        "title": r["title"],
        "tag": r["tag"],
        "summary": r.get("summary", ""),
        "remaining": r.get("remaining", ""),
        "files": r.get("files", []),
        "new_files": r.get("new_files", []),
        "test_commands": r.get("test_commands", []),
        "flag": r.get("flag", ""),
        "risk": risk or ["none"],
        "importance": r["importance"],
        "implementer": overrides.get(r["id"], "haiku" if r["importance"] == "haiku" else "sonnet"),
        "review": review,
        "depends_on": r.get("depends_on", []),
        "decisions_defaults": r.get("decision_defaults_applied", {}),
        "notes": " ".join(notes),
    }
    slices.append(rec)
print(json.dumps({"batchId": "B%02d" % n, "goal": batch["goal"], "slices": slices}, separators=(",", ":")))
