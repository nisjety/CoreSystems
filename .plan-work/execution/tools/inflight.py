"""Manage the in-flight file locks that keep parallel batches (and merges) off each other's files.
usage: python inflight.py list | release OWNER | reserve OWNER FILE [FILE ...] | clear
wave-args.py --register adds a lock for a batch; release it once the batch is committed."""
import json
import sys

F = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution\inflight.json"
try:
    d = json.load(open(F, encoding="utf-8"))
except Exception:
    d = {"locks": []}
cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
if cmd == "release":
    d["locks"] = [l for l in d["locks"] if l["owner"] != sys.argv[2]]
elif cmd == "reserve":
    d["locks"] = [l for l in d["locks"] if l["owner"] != sys.argv[2]]
    d["locks"].append({"owner": sys.argv[2], "files": sys.argv[3:], "slices": []})
elif cmd == "clear":
    d["locks"] = []
json.dump(d, open(F, "w", encoding="utf-8"), indent=1)
for l in d["locks"]:
    print(l["owner"], len(l["files"]), "files", l.get("slices", []))
if not d["locks"]:
    print("no locks")
