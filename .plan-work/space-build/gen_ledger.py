import json, glob, os, collections, datetime

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
OUT = ROOT + "docs/plans/space-build-ledger-w1.md"
units = {}
for p in sorted(glob.glob(ROOT + ".plan-work/space-build/units/*.json")):
    try:
        d = json.load(open(p, encoding="utf-8"))
    except Exception:
        continue
    units[os.path.basename(p)[:-5]] = d

tests = {}
for p in sorted(glob.glob(ROOT + ".plan-work/space-build/tests/*.json")):
    try:
        d = json.load(open(p, encoding="utf-8"))
    except Exception:
        continue
    tests[os.path.basename(p)[:-5]] = d

L = ["# Space build ledger, wave W1 (M0 + M1)", ""]
L.append("Generated %s. Branch `space` (cut from `model-plane` e603fde78), builder commits are path-limited, nothing is pushed. Source: `.plan-work/space-build/units/*.json`, `tests/*.json`, `ledger.md`." % datetime.date.today().isoformat())
L.append("")
c = collections.Counter(u.get("status") for u in units.values())
L.append("Slices and legs: %d (%s). Files changed: see `git diff --stat e603fde78..space`." % (len(units), ", ".join("%s %d" % (k, v) for k, v in c.most_common())))
L.append("")
L.append("## Slices")
L.append("")
L.append("| slice | status | commit | compile | migrations | flags |")
L.append("|---|---|---|---|---|---|")
for k in sorted(units):
    u = units[k]
    comp = (u.get("compile") or "")[:70].replace("|", "/")
    L.append("| %s | %s | %s | %s | %s | %s |" % (k, u.get("status"), u.get("commit", ""), comp, "; ".join(u.get("migrations", []))[:120].replace("|", "/"), "; ".join(u.get("flags", []))[:120].replace("|", "/")))
L.append("")
L.append("## Test phase (written and run at the wave end)")
L.append("")
L.append("| cluster | written | run | passed | failed and fixed | skipped (deferred tier) | depth rows | commit |")
L.append("|---|---|---|---|---|---|---|---|")
tot = collections.Counter()
for k, t in tests.items():
    for f in ("written", "run", "passed", "failed_fixed", "skipped_deferred", "depth_rows"):
        tot[f] += int(t.get(f, 0) or 0)
    L.append("| %s | %s | %s | %s | %s | %s | %s | %s |" % (k, t.get("written"), t.get("run"), t.get("passed"), t.get("failed_fixed"), t.get("skipped_deferred"), t.get("depth_rows"), t.get("commit", "")))
L.append("| **total** | %d | %d | %d | %d | %d | %d | |" % tuple(tot[f] for f in ("written", "run", "passed", "failed_fixed", "skipped_deferred", "depth_rows")))
L.append("")
L.append("## Numbers claimed (migrations, proto tags, flags)")
L.append("")
L.append("```")
L.append(open(ROOT + ".plan-work/space-build/ledger.md", encoding="utf-8").read().strip())
L.append("```")
L.append("")
L.append("## Wave-end compile check (2026-10-27)")
L.append("")
L.append("- Model Plane Rust: `cargo check -p mp-contracts -p session-core -p model-gateway -p execution-core --lib --bins` green.")
L.append("- Gateway: `cargo check` green.")
L.append("- Go: `go build ./...` green in user-core, org-core, audit-core, integration-corev2 and Model Plane go/gen.")
L.append("- SPA: `pnpm exec tsc -b` leaves 34 errors in 3 files no Space commit touched (quarry-client.ts, RemoteSupportPage.tsx, schedule-contract.test.ts); the two errors in a Space test file were fixed.")
L.append("")
open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(L))
print("ledger written", len(L), "lines")
