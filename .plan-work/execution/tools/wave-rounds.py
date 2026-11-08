"""Simulate building Wave 2's todo slices in rounds: each round takes a disjoint packing of the pickable todo slices
(queue order first, then fewest files), marks them done, and repeats. Prints each round's slices and the blocked tail."""
import json
import re
import sys

Q = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution\queue.json"
q = json.load(open(Q, encoding="utf-8"))
by = {s["id"]: s for s in q}
status = {s["id"]: s["status"] for s in q}
MET = {"done", "partial", "blocked-external"}
cap = int(sys.argv[1]) if len(sys.argv) > 1 else 6
mode = sys.argv[2] if len(sys.argv) > 2 else "order"


def wave(i):
    m = re.match(r"WS(\d+)", i)
    return 0 if not m else (1 if int(m.group(1)) <= 12 else 2)


def dep_ok(s):
    for d in s.get("depends_on") or []:
        st = status.get(d) or status.get(d.split(".")[0])
        if st not in MET:
            return False
    return True


todo = [s for s in q if wave(s["id"]) == 2 and s["status"] == "todo"]
rnd = 0
while True:
    cand = [s for s in todo if status[s["id"]] == "todo" and dep_ok(s)]
    if not cand:
        break
    rnd += 1
    key = (lambda s: s.get("order", 0)) if mode == "order" else (lambda s: (len(s.get("files") or []), s.get("order", 0)))
    taken, picked = set(), []
    for s in sorted(cand, key=key):
        fs = set(s.get("files") or []) | set(s.get("new_files") or [])
        if fs & taken:
            continue
        taken |= fs
        picked.append(s)
        if len(picked) >= cap:
            break
    for s in picked:
        status[s["id"]] = "done"
    print(f"round {rnd}: {len(picked)} -> " + " ".join(f"{s['id']}({s.get('importance','?')[0]})" for s in picked))
left = [s["id"] for s in todo if status[s["id"]] == "todo"]
print(f"rounds: {rnd}; todo never pickable: {len(left)} {left}")
