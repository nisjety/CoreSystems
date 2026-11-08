"""Derive the W2 builder rules from the W1 rules."""
SRC = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/.plan-work/00-build-rules.md"
DST = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/.plan-work/00-build-rules-w2.md"
W2 = "C:/dev/CoresSystem/.claude/worktrees/space-main-merge"

t = open(SRC, encoding="utf-8").read()
subs = [
    ("# Build rules for every Space W1 builder (read fully before editing)",
     "# Build rules for every Space W2 builder (read fully before editing)"),
    ("wave W1 = milestones M0 + M1", "wave W2 = milestones M2.1 + M2.2 + M2.3; wave W1 = M0 + M1 is already built and merged"),
    ("- Your current working directory is the build worktree (git branch `space`, cut from the sibling branch `model-plane` at e603fde78).",
     f"- The build worktree is `{W2}` (git branch `space-w2`, cut from `main` right after `space` (wave W1) was merged into it, so it holds W1, the Model Plane chat build and the Quarry work). Your shell may start elsewhere: your FIRST command is `cd \"{W2}\"`, and every relative path in these rules is relative to it. After any `cd` into a subfolder, `cd` back to it. Use absolute paths with Read/Edit/Write."),
    ("NEVER edit `C:/dev/CoresSystem` (the live main checkout), any other worktree,",
     "NEVER edit `C:/dev/CoresSystem` (the live main checkout), any other worktree (the planning worktree practical-mccarthy-1dc567 included),"),
    ("This branch also contains the sibling Model Plane build (ZDR Z-series, durable loop, retention receipt, migrations up to 0068, user-core up to 032, org-core up to 024, etc.), so lines drift and some files differ:",
     "This branch also contains wave W1 (read `.plan-work/space-build/units/<W1 slice>.json` for what each W1 slice did), the Model Plane chat build Waves 1-2 and the Quarry work, so lines drift, numbers moved and some files differ:"),
    ("a test phase at the end of the wave writes and runs them.",
     "a test phase at the end of the wave writes and runs them, after W2 is merged into main."),
    ("`feat(space): <slice id> <short what>`", "`feat(space): <slice id> <short what>`"),
]
for a, b in subs:
    if a not in t:
        raise SystemExit("missing: " + a[:60])
    t = t.replace(a, b)
t += """
## Wave W2 additions
- Many W2 slices extend W1 code. Before you add a route, RPC, table, flag or component, Grep for it: W1 or a sibling build may already have it (then extend, do not duplicate).
- Model Plane wave 2 work landed in main too (message tree, starred/unread, WS14/WS16 of the Model Plane plan). Where a Space slice and a Model Plane feature overlap, reuse the Model Plane piece and record a `DEVIATION:`.
- Proto numbers: the Space reservations in plan 4.6 were checked against main at merge time; still check the highest number in the merged proto before you add one.
- If node_modules is missing in `apps/Frontend Plane/verevonv3`, do not install it: compile-check the SPA as `not-checked: no node_modules` and say so.
"""
open(DST, "w", encoding="utf-8", newline="\n").write(t)
print("ok", len(t))
