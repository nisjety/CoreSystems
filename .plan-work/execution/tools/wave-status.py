"""Status of Model Plane waves 1-5 from the queue, the depth register, the designs folder and git."""
import collections
import json
import re
import subprocess

ROOT = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b"
Q = ROOT + r"\.plan-work\execution\queue.json"
REG = ROOT + r"\docs\plans\model-plane-depth-register.md"


def ws(sid):
    m = re.match(r"WS(\d+)", sid)
    return int(m.group(1)) if m else 0


def wave(sid):
    n = ws(sid)
    return 1 if 1 <= n <= 12 else 2 if 13 <= n <= 27 else 0


queue = json.load(open(Q, encoding="utf-8"))
by_wave = collections.defaultdict(collections.Counter)
todo = collections.defaultdict(list)
for item in queue:
    w = wave(item["id"])
    by_wave[w][item["status"]] += 1
    if item["status"] == "todo":
        todo[w].append(item["id"])
for w in (1, 2):
    c = by_wave[w]
    print(f"Wave {w}: total {sum(c.values())} | " + ", ".join(f"{k} {v}" for k, v in sorted(c.items())))
    print(f"  todo ids: {' '.join(sorted(todo[w]))}")

# Depth register: rows are | ID | slice | kind | Px | text | status |
rows = []
for line in open(REG, encoding="utf-8"):
    m = re.match(r"\| (D[0-9]-\d+) \| ([^|]+) \| ([^|]+) \| (P[0-3]) \| (.*) \| ([^|]*) \|\s*$", line)
    if m:
        rid, sl, kind, sev, text, status = (g.strip() for g in m.groups())
        rows.append((rid, sl, kind, sev, text, status))
print(f"register rows parsed: {len(rows)}")


def row_wave(rid, sl):
    if rid.startswith("D1-"):
        return 1
    if rid.startswith("D2-"):
        return 2
    return wave(sl) or 0


open_rows = [r for r in rows if r[5].startswith("open")]
for w in (0, 1, 2):
    sev = collections.Counter(r[3] for r in open_rows if row_wave(r[0], r[1]) == w)
    kinds = collections.Counter(r[2] for r in open_rows if row_wave(r[0], r[1]) == w and r[3] in ("P0", "P1"))
    print(f"open rows wave {w}: " + ", ".join(f"{k} {v}" for k, v in sorted(sev.items())) + f" | P0/P1 kinds: {dict(kinds)}")
print("OPEN P0:")
for r in open_rows:
    if r[3] == "P0":
        print(f"  {r[0]} {r[1]}: {r[4][:230]}")
decisions = [r for r in open_rows if r[2] == "decision" and r[3] in ("P0", "P1")]
print(f"open decision rows P0/P1: {len(decisions)}")
for r in decisions:
    if row_wave(r[0], r[1]) == 2 and r[0] >= "D2-442":
        print(f"  {r[0]} {r[1]}: {r[4][:200]}")


def git(*args):
    return subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True, encoding="utf-8").stdout


print("commits since Wave 2 build start (9c60f9f61^..HEAD):", len(git("log", "--oneline", "9c60f9f61^..HEAD").splitlines()))
print("HEAD:", git("log", "-1", "--format=%h %s").strip())
