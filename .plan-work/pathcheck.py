import re, os, subprocess, json, collections

ROOT = "C:/dev/CoresSystem/"
PLAN = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/docs/plans/space-hybrid-build-plan-2026-10-14.md"
OUT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/.plan-work/review/pathcheck.json"

text = open(PLAN, encoding="utf-8").read().split("\n")
pat = re.compile(r"`((?:apps|\.github|scripts|docs|\.githooks)/[^`\n]+?)`")
loc = re.compile(r"^(.*?)(?::(\d+)(?:-(\d+))?(?:,[\d,\-]+)?)?$")

tracked = set(l.rstrip("\n") for l in open("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/.plan-work/live-tree-tracked-files.txt", encoding="utf-8", errors="replace"))

branches = {}
for b in ("model-plane", "quarryv2", "quarryv2-next"):
    try:
        out = subprocess.run(["git", "--no-optional-locks", "-C", ROOT, "ls-tree", "-r", "--name-only", b], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120).stdout
        branches[b] = set(out.split("\n"))
    except Exception as e:
        branches[b] = set()

cache_len = {}
def nlines(p):
    if p not in cache_len:
        try:
            with open(ROOT + p, "rb") as f:
                cache_len[p] = f.read().count(b"\n") + 1
        except Exception:
            cache_len[p] = None
    return cache_len[p]

res = collections.defaultdict(list)
seen = set()
for i, line in enumerate(text, 1):
    for m in pat.finditer(line):
        raw = m.group(1).strip()
        # skip globs / placeholders / multiple-path tokens
        if any(c in raw for c in ("*", "<", ">", "{", "}", " -> ", " and ", " or ", "…", "...")):
            continue
        mm = loc.match(raw)
        path, l1, l2 = mm.group(1), mm.group(2), mm.group(3)
        path = path.rstrip("/").strip()
        key = (path, l1, l2, i)
        if key in seen:
            continue
        seen.add(key)
        exists = os.path.exists(ROOT + path)
        isfile = os.path.isfile(ROOT + path)
        ctx = line
        proposed = bool(re.search(r"\[N\]|\bnew\b|\bNEW\b|\bcreate|\bcreated\b|adds?\b|\badd\b|to be created|proposed|P-[A-Z]+\d|rename|moves? to|git mv|after the move|NEW file|\(new\)|seam|golden|scripts/space|space-gate|convex-core\.yml|model-plane-go\.yml|audit-core\.yml", ctx))
        status = None
        if exists:
            status = "ok"
            if isfile and l1:
                n = nlines(path)
                hi = int(l2) if l2 else int(l1)
                if n is not None and hi > n:
                    status = "line-out-of-range(file has %d)" % n
        else:
            where = [b for b, s in branches.items() if path in s]
            if where:
                status = "only-on-branch:" + ",".join(where)
            elif proposed:
                status = "proposed-new"
            else:
                status = "MISSING-UNEXPLAINED"
        res[status.split("(")[0] if status.startswith("line-out") else status].append({"line": i, "path": path, "l1": l1, "l2": l2, "status": status})

summary = {k: len(v) for k, v in res.items()}
print(json.dumps(summary, indent=1))
json.dump(res, open(OUT, "w", encoding="utf-8"), indent=1)
bad = res.get("MISSING-UNEXPLAINED", []) + res.get("line-out-of-range", [])
print("unexplained/out-of-range:", len(bad))
by_path = collections.Counter(b["path"] for b in bad)
for p, c in by_path.most_common(60):
    print(c, p, [b["line"] for b in bad if b["path"] == p][:4])
