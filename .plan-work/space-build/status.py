import json, glob, os, collections, subprocess
R = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
q = json.load(open(R + ".plan-work/space-build/queue.json", encoding="utf-8"))
ids = [r["id"] for r in q["rows"] if r["id"] != "M0-BASE"]
st = {}
for i in ids:
    p = R + ".plan-work/space-build/units/%s.json" % i
    if os.path.exists(p):
        try:
            st[i] = json.load(open(p, encoding="utf-8")).get("status", "?")
        except Exception:
            st[i] = "unreadable"
c = collections.Counter(st.values())
print("units with result file: %d/%d" % (len(st), len(ids)), dict(c))
print("pending:", len([i for i in ids if i not in st]))
bad = [(i, s) for i, s in st.items() if s not in ("built", "already-present")]
print("not built:", bad)
out = subprocess.run(["git", "-C", R, "log", "--oneline", "e603fde78..HEAD"], capture_output=True, text=True, encoding="utf-8").stdout.strip().split("\n")
print("commits on space:", len([l for l in out if l]))
dirty = subprocess.run(["git", "-C", R, "status", "--short"], capture_output=True, text=True, encoding="utf-8").stdout.strip().split("\n")
dirty = [d for d in dirty if d and not d.startswith("??")]
print("dirty tracked files:", len(dirty))
dp = glob.glob(R + ".plan-work/space-build/depth/*.md")
print("depth files:", len(dp))
