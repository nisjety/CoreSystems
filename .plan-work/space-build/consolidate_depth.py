import re, glob, os, collections, json, datetime

ROOT = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/"
OUT = ROOT + "docs/plans/space-depth-register-w1.md"
rows = []
rx = re.compile(r"^\s*[-*]\s+\**(P[0-3])\**[\s:|,-]+\**([A-Za-z][A-Za-z\- ]{1,24}?)\**\s*:\s*(.+)$")
loose = re.compile(r"^\s*[-*]\s+\**(P[0-3])\**[\s:|,-]+(.+)$")
for p in sorted(glob.glob(ROOT + ".plan-work/space-build/depth/*.md")):
    src = os.path.basename(p)[:-3]
    for ln in open(p, encoding="utf-8", errors="replace").read().split("\n"):
        m = rx.match(ln)
        if m:
            rows.append((m.group(1), m.group(2).strip().lower(), src, m.group(3).strip()))
            continue
        m = loose.match(ln)
        if m:
            rows.append((m.group(1), "note", src, m.group(2).strip()))

c = collections.Counter(r[0] for r in rows)
kinds = collections.Counter(r[1] for r in rows)
# assign stable ids
rows = [(("D%s-%03d" % ("1", i + 1)),) + r for i, r in enumerate(rows)]

units = {}
for p in glob.glob(ROOT + ".plan-work/space-build/units/*.json"):
    try:
        d = json.load(open(p, encoding="utf-8"))
        units[os.path.basename(p)[:-5]] = d
    except Exception:
        pass
partial = sorted(k for k, v in units.items() if v.get("status") not in ("built", "already-present"))

L = []
L.append("# Space depth register, wave W1 (M0 + M1)")
L.append("")
L.append("Generated %s from the builders' and test agents' depth bullets (`.plan-work/space-build/depth/*.md`). Owner strategy: the wave was built to scope first, findings are logged here instead of being chased; at the wave stop the owner prioritises. Severity: P0 a hole that already exists in committed code or makes a slice unsafe; P1 likely bug or missing authority/ZDR coverage; P2 hardening; P3 polish." % datetime.date.today().isoformat())
L.append("")
L.append("Totals: " + ", ".join("%s %d" % (k, c.get(k, 0)) for k in ("P0", "P1", "P2", "P3")) + " (%d rows)." % len(rows))
L.append("By kind: " + ", ".join("%s %d" % (k, v) for k, v in kinds.most_common()) + ".")
L.append("")
L.append("Slices not fully built (status partial or blocked): " + (", ".join(partial) if partial else "none") + ".")
L.append("")
for sev, title in (("P0", "P0: holes and unsafe slices"), ("P1", "P1: likely bugs and missing authority or ZDR coverage")):
    L.append("## " + title)
    L.append("")
    sel = [r for r in rows if r[1] == sev]
    if not sel:
        L.append("None.")
    else:
        L.append("| id | source | kind | finding |")
        L.append("|---|---|---|---|")
        for r in sel:
            L.append("| %s | %s | %s | %s |" % (r[0], r[3], r[2], r[4].replace("|", "/")))
    L.append("")
for kind, title in (("deviation", "Deviations from the plan (all severities)"), ("decision", "Decisions the builders could not make (all severities)")):
    L.append("## " + title)
    L.append("")
    sel = [r for r in rows if r[2] == kind]
    if not sel:
        L.append("None.")
    else:
        L.append("| id | source | sev | finding |")
        L.append("|---|---|---|---|")
        for r in sel:
            L.append("| %s | %s | %s | %s |" % (r[0], r[3], r[1], r[4].replace("|", "/")))
    L.append("")
L.append("## P2 and P3 by source")
L.append("")
by = collections.defaultdict(list)
for r in rows:
    if r[1] in ("P2", "P3"):
        by[r[3]].append(r)
for src in sorted(by):
    L.append("### " + src)
    for r in by[src]:
        L.append("- %s %s %s: %s" % (r[0], r[1], r[2], r[4]))
    L.append("")
open(OUT, "w", encoding="utf-8", newline="\n").write("\n".join(L))
print("rows", len(rows), dict(c), "partial", partial)
