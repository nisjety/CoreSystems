"""Print the compact Workflow args JSON for the next build batch of one plan wave.

usage: python wave-args.py WAVE SIZE [BATCH_ID] [--dry] [--skip ID,ID] [--only ID,ID]
  WAVE       1 (WS01-WS12) or 2 (WS13-WS27)
  SIZE       the most slices in one batch
  BATCH_ID   label, for example W1-03 (default W<wave>-next)
  --dry      print the choice and why others were skipped, not the args
  --skip     slices to leave out (comma separated)
  --only     take exactly these slices (still checked for conflicts)
  --register lock the picked files in inflight.json under BATCH_ID (release with inflight.py release BATCH_ID)
  --out FILE  write the full args to FILE and print only a slim args JSON that points at it (saves the lead's context)
  --target D give the batch its own cargo target directory D (a Rust batch then builds in parallel with the others)
  --tier T   force every agent of the batch to one tier, haiku-high for Wave 2 (writes forceTier into the args)

Picks slices of the wave that are todo or partial, in queue `order`, whose dependencies are done (or partial/blocked-external: their code half is
built), whose files do not overlap the files already picked for the batch or any file with uncommitted changes in the worktree.
"""
import json
import re
import subprocess
import sys

D = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b\.plan-work\execution"
REPO = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b"

_valued = {"--skip", "--only", "--tier", "--target", "--out"}
argv, flags, _skip_next = [], [], False
for a in sys.argv[1:]:
    if _skip_next:
        _skip_next = False
        continue
    if a.startswith("--"):
        flags.append(a)
        _skip_next = a in _valued
    else:
        argv.append(a)
wave = int(argv[0])
size = int(argv[1])
batch_id = argv[2] if len(argv) > 2 else f"W{wave}-next"
dry = "--dry" in flags
register = "--register" in flags


def opt(name):
    if name in sys.argv:
        i = sys.argv.index(name)
        return set(sys.argv[i + 1].split(","))
    return set()


skip = opt("--skip")
only = opt("--only")
out_file = None
if "--out" in sys.argv:
    out_file = sys.argv[sys.argv.index("--out") + 1]
target = None
if "--target" in sys.argv:
    target = sys.argv[sys.argv.index("--target") + 1]
tier = None
if "--tier" in sys.argv:
    tier = sys.argv[sys.argv.index("--tier") + 1]

queue = json.load(open(D + r"\queue.json", encoding="utf-8"))
byid = {r["id"]: r for r in queue}


def ws(i):
    m = re.match(r"WS(\d+)", i)
    return int(m.group(1)) if m else 0


def wave_of(i):
    if i.startswith("EP-"):  # Wave 3 slices are named after their epic design (docs/plans/wave3/EP-*.md)
        return 3
    n = ws(i)
    return 1 if 1 <= n <= 12 else 2 if 13 <= n <= 27 else 0


def norm(p):
    p = p.replace("\\", "/").strip()
    for tag in (" (new)", "(new)"):
        p = p.replace(tag, "")
    return p.strip().lower()


def overlaps(a, b):
    """True when two path sets share a file, or one is a folder holding the other."""
    for x in a:
        for y in b:
            if x == y or x.startswith(y.rstrip("/") + "/") or y.startswith(x.rstrip("/") + "/"):
                return True
    return False


dirty = set()
INFLIGHT = D + r"\inflight.json"
try:
    inflight = json.load(open(INFLIGHT, encoding="utf-8"))
except Exception:
    inflight = {"locks": []}
for lock in inflight["locks"]:
    for f in lock["files"]:
        dirty.add(norm(f))
out = subprocess.run(["git", "status", "--porcelain"], cwd=REPO, capture_output=True, text=True).stdout
for line in out.splitlines():
    path = line[3:].strip().strip('"')
    if " -> " in path:
        path = path.split(" -> ")[1].strip('"')
    if not path.startswith(".plan-work"):
        dirty.add(norm(path))

# A partial slice is built again only when code is left; the others wait for the depth phase (proofs, measurements, pushes).
BUILD_PARTIAL = {"WS04-S1"}
INCLUDE_OPTIONAL = "--include-optional" in sys.argv  # owner order 2026-10-26: build the optional-deferred slices too
INCLUDE_BLOCKED = "--include-blocked" in sys.argv  # 2026-11-03 coordinator order: code first; a blocked row builds the design's recommended default behind a default-off switch
cands = [r for r in queue if wave_of(r["id"]) == wave and (r["status"] == "todo" or (INCLUDE_OPTIONAL and r["status"] == "optional-deferred") or (INCLUDE_BLOCKED and r["status"] == "blocked-decision") or (r["status"] == "partial" and r["id"] in BUILD_PARTIAL))]
cands.sort(key=lambda r: r.get("order", 10**6))

OK_DEP = {"done", "partial", "blocked-external", "optional-deferred"} | ({"blocked-decision"} if INCLUDE_BLOCKED else set())
picked, why, taken = [], {}, []
for r in cands:
    sid = r["id"]
    if only and sid not in only:
        continue
    if sid in skip:
        why[sid] = "skipped by the lead"
        continue
    bad = [d for d in r.get("depends_on", []) if d in byid and byid[d]["status"] not in OK_DEP]
    xwave = [d for d in r.get("depends_on", []) if d in byid and wave_of(d) != wave and byid[d]["status"] not in ("done", "partial")]  # a partial slice's code is built; its proofs wait for the depth pass
    if "--code-half-ok" in sys.argv:
        # 2026-11-05: code-first build. A blocked-external dependency whose `<id>.code` half is built is met (its external half is a
        # live proof only), and so is a dependency already picked earlier in this batch (isolated builders build both from the design).
        met = lambda d: (byid.get(d + ".code", {}).get("status") in ("done", "partial")) or d in {p["id"] for p in picked}
        bad = [d for d in bad if not met(d)]
        xwave = [d for d in xwave if not met(d)]
    if bad or xwave:
        why[sid] = "waits for " + ", ".join(bad + xwave)
        continue
    files = {norm(f) for f in (r.get("files") or []) + (r.get("new_files") or [])}
    if overlaps(files, dirty):
        why[sid] = "files have uncommitted changes"
        continue
    if overlaps(files, set(taken)) and "--allow-overlap" not in sys.argv:  # isolated builders (run-lean isolate) merge by 3-way patch
        why[sid] = "files overlap a slice already in this batch"
        continue
    picked.append(r)
    taken.extend(files)
    if len(picked) >= size:
        break

if dry:
    print("picked:", [r["id"] for r in picked])
    for k, v in list(why.items())[:60]:
        print("  skipped", k, "-", v)
    sys.exit(0)

STRICT = {"authority", "audit", "zdr", "money", "data-loss"}
slices = []
for r in picked:
    risk = [x for x in (r.get("risk") or []) if x != "none"]
    notes = []
    if r.get("dependency_notes"):
        dn = r["dependency_notes"]
        notes.append("Dependency notes: " + (" ".join(dn) if isinstance(dn, list) else str(dn)))
    if r.get("lead_shared_files"):
        notes.append(
            "Do NOT edit these shared files (the lead applies your exact lines after the batch; put them in lead_todo as 'shared-edit <file>: <exact lines>'): "
            + ", ".join(r["lead_shared_files"])
        )
    if r.get("needs_external"):
        notes.append("External need (do not fake): " + r["needs_external"])
    slices.append(
        {
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
            "implementer": "haiku" if r["importance"] == "haiku" else "sonnet",
            "strict": bool(set(risk) & STRICT),
            "depends_on": r.get("depends_on", []),
            "decisions_defaults": r.get("decision_defaults_applied", {}),
            "notes": " ".join(notes),
        }
    )
if register:
    inflight["locks"] = [l for l in inflight["locks"] if l["owner"] != batch_id]
    inflight["locks"].append({"owner": batch_id, "files": sorted(taken), "slices": [r["id"] for r in picked]})
    json.dump(inflight, open(INFLIGHT, "w", encoding="utf-8"), indent=1)
goal = f"Wave {wave} build: " + "; ".join(r["title"] for r in picked)
args = {"batchId": batch_id, "wave": wave, "goal": goal, "slices": slices}
if target:
    args["targetDir"] = target
if tier:
    model, _, effort = tier.partition("-")
    args["forceTier"] = {"model": model, "effort": effort or "high"}
    for s in slices:
        s["implementer"] = model
if out_file:
    json.dump(args, open(out_file, "w", encoding="utf-8"), ensure_ascii=False)
    slim = {k: v for k, v in args.items() if k != "slices"}
    slim["argsFile"] = out_file.replace("\\", "/")
    slim["slices"] = [{k: s[k] for k in ("id", "title", "implementer", "strict", "risk", "importance")} for s in slices]
    print(json.dumps(slim, separators=(",", ":"), ensure_ascii=False))
else:
    print(json.dumps(args, separators=(",", ":"), ensure_ascii=False))
