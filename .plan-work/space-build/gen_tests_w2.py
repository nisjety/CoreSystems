"""Generate the W2 test-phase workflow (w2-tests.js) and the W2 test rules from the W1 versions."""
import json, re, collections, os

HERE = os.path.dirname(os.path.abspath(__file__)).replace("\\", "/")
PW = os.path.dirname(HERE)
W2 = "C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/w2"

# Units and their lock groups come from the W2 build script.
js = open(HERE + "/w2-build.js", encoding="utf-8").read()
units = json.loads(re.search(r"const UNITS = (\[.*?\n\])\n", js, re.S).group(1))
group = {u["id"]: u["group"] for u in units}

by_group = collections.defaultdict(list)
for sid in group:
    p = f"{HERE}/units/{sid}.json"
    if not os.path.exists(p):
        continue
    d = json.load(open(p, encoding="utf-8"))
    tests = d.get("tests_owed", [])
    if not tests:
        continue
    by_group[group[sid]].append((sid, d.get("commit", ""), tests, d.get("files", [])))

MAXT = 30
tunits = []
for g, items in sorted(by_group.items()):
    cur, n, k = [], 0, 1
    for it in items:
        if cur and n + len(it[2]) > MAXT:
            tunits.append((g, k, cur)); cur, n, k = [], 0, k + 1
        cur.append(it); n += len(it[2])
    if cur:
        tunits.append((g, k, cur))

out = []
for g, k, items in tunits:
    text = []
    for sid, commit, tests, files in items:
        text.append("### %s (commit %s)\nfiles: %s\ntests owed:\n%s" % (sid, commit or "?", ", ".join(files[:25]), "\n".join("- " + t for t in tests)))
    out.append({"id": "T2-%s-%d" % (g, k), "group": g, "slices": [i[0] for i in items],
                "count": sum(len(i[2]) for i in items), "spec": "\n\n".join(text)})
print(len(out), "test units; tests owed:", sum(u["count"] for u in out))
for u in out:
    print(u["id"], u["count"], u["slices"])

# Rules: W1 rules with the W2 location and branch.
r = open(PW + "/00-test-rules.md", encoding="utf-8").read()
subs = [
    ("# Rules for every Space W1 test-phase agent", "# Rules for every Space W2 test-phase agent"),
    ("Wave W1 of the Space plan is built (all slices committed on branch `space` in this worktree).",
     f"Wave W2 of the Space plan is built (all slices committed on branch `space-w2` in the worktree `{W2}`, which main has been fast-forwarded to). Your FIRST shell command is `cd \"{W2}\"`; every relative path here is relative to it, and after any `cd` into a subfolder you `cd` back. `w2/` is a git worktree nested inside the planning worktree practical-mccarthy-1dc567: run every git command inside `w2/` and check that `git branch --show-current` prints `space-w2` before you commit. Never edit the planning worktree outside `w2/`."),
    ("- Work only inside this worktree (cwd).", "- Work only inside the `w2` worktree."),
    ("or a stale existing test/literal that W1 changed", "or a stale existing test/literal that W1 or W2 changed"),
    ("(c) pre-existing failure unrelated to W1", "(c) pre-existing failure unrelated to W2"),
]
for a, b in subs:
    if a not in r:
        raise SystemExit("rules: missing " + a[:50])
    r = r.replace(a, b)
r += "\n## Wave W2 additions\n- The Model Plane Go services build inside `apps/Model Plane/go/go.work`: do not set GOWORK=off for build or test there.\n- SPA typecheck baseline is ONE error (schedule-contract.test.ts TS2352); keep it there.\n- Solid 2: aria-pressed/aria-expanded take 'true'/'false' strings, there is no onMount (use onSettled), inputs use maxlength.\n"
open(PW + "/00-test-rules-w2.md", "w", encoding="utf-8", newline="\n").write(r)

tpl = open(HERE + "/w1-tests-template.js", encoding="utf-8").read()
tsubs = [
    ("name: 'space-w1-tests'", "name: 'space-w2-tests'"),
    ("Wave-end test phase for W1", "Wave-end test phase for W2"),
    ("Read .plan-work/00-test-rules.md first (current working directory = the build worktree) and follow it exactly.",
     f"Read {W2}/.plan-work/00-test-rules-w2.md first and follow it exactly; your first shell command is cd \"{W2}\"."),
    ("postponed tests of these W1 slices", "postponed tests of these W2 slices"),
    ("log('W1 test phase finished: '", "log('W2 test phase finished: '"),
]
for a, b in tsubs:
    if a not in tpl:
        raise SystemExit("template: missing " + a[:50])
    tpl = tpl.replace(a, b)
open(HERE + "/w2-tests.js", "w", encoding="utf-8", newline="\n").write(tpl.replace("__TESTUNITS__", json.dumps(out, indent=1)))
print("wrote w2-tests.js and 00-test-rules-w2.md")
