"""Close one lean (code-first) Wave 3 batch and stage the next one.

usage: python lean-close.py W3-NN TASK_OUTPUT_FILE [NEXT_ID] [--no-next]
  1. commit-batch.py: one commit per slice plus the docs commit (register, ledger);
  2. books the guard's skipped shared edits and still-broken items as register rows;
  3. re-checks the posture matrix (zdr-contract-schema.mjs) and refuses to commit if it fails;
  4. commits everything else the guard left (shared edits, regenerated code, goldens) as the lead-edits commit;
  5. releases the batch's file locks;
  6. unless --no-next: picks the next 8 slices (wave-args --include-blocked --register), writes the launch file
     (w3-launch.py, standard note) and the slim args (lean-launch.py), and prints the slim args path.
Prints a short summary only; the full results stay in results/W3-NN.json.
"""
import datetime
import json
import re
import subprocess
import sys

REPO = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b"
EX = REPO + r"\.plan-work\execution"
REGISTER = REPO + r"\docs\plans\model-plane-depth-register.md"
TRAILER = "\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n"
EXCLUDE = [":!.plan-work", ":!apps/Control Plane/audit-core/audit-verify.exe", ":!apps/Frontend Plane/Ingestion Plane"]
NOTE = ("NOTE: code first (owner order 2026-11-03): write the code and the unit tests the row names, do not run them; one guard agent "
        "compiles after the batch. session-core migrations: numbers must apply IN ORDER, so always take the next number above the highest file "
        "in the migrations folder (0111 exists; 0106 was reserved for EP-12-S14 but is below the highest, so EP-12-S14 takes 0112 or the next above the highest), never a lower reserved round-B number; capability-core the next free at or "
        "above 0036. Never add zdr-posture-matrix rows for a feature refusal (rows are keyed on bearers and route only); use zdr_features.rs, the "
        "feature table and docs/ZDR.md. pytest and python-pptx are not installed. action-registry.test.ts P-SPA1 is main's pre-existing failure. "
        "Error bodies keep {error:{code,message,details}} with details an object. auth-core has ONE ApiKeyExchangeController serving every "
        "audience (merged from main 2026-11-04): extend it, never add a second exchange route.")

batch, task_out = sys.argv[1], sys.argv[2]
rest = [a for a in sys.argv[3:] if not a.startswith("--")]
next_id = rest[0] if rest else None
no_next = "--no-next" in sys.argv


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
ENV = {**__import__("os").environ, "PYTHONIOENCODING": "utf-8"}


def run(*a, cwd=REPO, inp=None):
    return subprocess.run(list(a), cwd=cwd, capture_output=True, text=True, input=inp, encoding="utf-8", errors="replace", env=ENV)


def git(*a, inp=None):
    return run("git", *a, inp=inp)


res = json.load(open(task_out, encoding="utf-8"))["result"]
if isinstance(res, str):
    res = json.loads(res)

# 1. slice commits
# --book-only: the slice commits already landed (an earlier run failed after them); book the register and ledger only.
cb = run("python", EX + r"\tools\commit-batch.py", task_out, rf"{EX}\batch-args\{batch}.json", *(["--book-only"] if "--book-only" in sys.argv else []), cwd=EX)
print("\n".join(l for l in cb.stdout.splitlines() if l[:6] in ("OK    ", "LEFT  ", "EMPTY ", "COMMIT", "ADDFAI", "DOCS  ") or l.startswith(batch)))
if cb.returncode or "COMMITFAIL" in cb.stdout or "DOCS   FAILED" in cb.stdout:
    # A failed slice or docs commit (for example the gitleaks hook without Docker): undo the register and ledger
    # booking so the next run of this script books the batch once.
    git("checkout", "HEAD", "--", "docs/plans/model-plane-depth-register.md", "apps/Model Plane/docs/decisions/ledger.md")
    print("commit-batch did not commit everything; register and ledger restored. Fix the cause, then rerun this command.")
    print((cb.stdout[-800:] + cb.stderr[-400:]).strip())
    sys.exit(1)

# 2. guard items into the register
guard = res.get("guard") or {}
items = [("follow-up", "P2", "guard skipped: " + s) for s in guard.get("shared_edits_skipped", []) if "ledger.md" not in s and ".env.example" not in s]
items += [("test-gap", "P1", "still broken after the guard: " + s) for s in guard.get("still_broken", []) if "prompt_tool_manifest" not in s]
if items:
    raw = open(REGISTER, encoding="utf-8", newline="").read()
    nl = "\r\n" if "\r\n" in raw else "\n"
    t = raw.replace("\r\n", "\n")
    nid = max(int(n) for n in re.findall(r"\bD3-(\d+)\b", t)) + 1
    lines = t.split("\n")
    last = max(i for i, l in enumerate(lines) if l.startswith("| D3-"))
    rows = [f"| D3-{nid + k:03d} | {batch} guard | {kind} | {sev} | {' '.join(text.split()).replace('|', '/')[:900]} | open |" for k, (kind, sev, text) in enumerate(items)]
    lines[last + 1:last + 1] = rows
    open(REGISTER, "w", encoding="utf-8", newline="").write("\n".join(lines).replace("\n", nl))
    print(f"register: {len(rows)} guard rows from D3-{nid:03d}")

# 3. posture matrix schema
schema = run("node", "scripts/tests/zdr-contract-schema.mjs")
bad = [l for l in schema.stdout.splitlines() if l.startswith("not ok")]
if bad:
    print("POSTURE MATRIX INVALID, not committing the lead edits:")
    print("\n".join(b[:300] for b in bad))
    sys.exit(2)

# 4. lead-edits commit
git("add", "-A", "--", ".", *EXCLUDE)
staged = [l for l in git("diff", "--cached", "--name-only").stdout.splitlines() if l]
if staged:
    checks = "; ".join(f"{c['command'][:70]}: {'clean' if c['clean'] else 'NOT clean'}" for c in guard.get("checks", []))[:1500]
    fixes = "\n".join(f"- {f.get('slice') or 'lead'}: {f['file'].split('/')[-1]}: {f['what'][:160]}" for f in guard.get("fixes", [])[:12])
    msg = (f"chore(model-plane): {batch} lead edits (guard shared edits, regeneration, goldens)\n\n"
           f"{len(staged)} files the guard agent changed after the builders: shared edits the slices handed back, regenerated "
           f"bindings and goldens, and compile fixes.\n{fixes}\n\nCompile guard only, no test run (owner order 2026-11-03): {checks}\n" + TRAILER)
    c = git("commit", "-q", "-F", "-", inp=msg)
    print("lead commit:", "ok " + git("rev-parse", "--short", "HEAD").stdout.strip() if c.returncode == 0 else "FAILED " + (c.stdout + c.stderr)[-600:])
    if c.returncode:
        sys.exit(3)
else:
    print("lead commit: nothing left to commit")

# 5. release locks, and remove the isolated builders' worktrees (their patches are merged by now)
run("python", EX + r"\tools\inflight.py", "release", batch, cwd=EX)
listed = git("worktree", "list", "--porcelain").stdout
known = {l[9:].strip().replace("\\", "/").lower() for l in listed.splitlines() if l.startswith("worktree ")}
removed = 0
for r in res.get("results", []):
    wt = (r.get("worktree") or "").strip().replace("\\", "/")
    if not wt or wt.lower().rstrip("/") == REPO.replace("\\", "/").lower() or wt.lower().rstrip("/") not in known:
        continue
    out = git("worktree", "remove", "--force", wt)
    removed += out.returncode == 0
    if out.returncode:
        print("worktree remove failed:", wt, out.stderr[-200:])
if removed:
    git("worktree", "prune")
    print(f"removed {removed} builder worktrees")
# The isolation creates a branch worktree-<name> per builder; builders never commit, so a branch with no commit
# outside model-plane is empty and goes.
for b in git("branch", "--list", "worktree-wf_*", "--format=%(refname:short)").stdout.split():
    if git("rev-list", "--count", f"model-plane..{b}").stdout.strip() == "0":
        git("branch", "-D", b)

# 6. next batch
if no_next or not next_id:
    sys.exit(0)
wa = run("python", EX + r"\tools\wave-args.py", "3", "8", next_id, "--include-blocked", "--allow-overlap", "--register", "--target", "C:/dev/mp-target-w2",
         "--out", rf"{EX}\batch-args\{next_id}.json", cwd=EX)
try:
    picked = [s["id"] for s in json.loads(wa.stdout.strip().splitlines()[-1])["slices"]]
except Exception:
    print("wave-args gave no batch:", (wa.stdout + wa.stderr)[-400:])
    sys.exit(0)
if not picked:
    print("no startable slice left")
    sys.exit(0)
for step in (["w3-launch.py", next_id, NOTE], ["lean-launch.py", next_id, "--isolate"]):
    out = run("python", EX + "\\tools\\" + step[0], *step[1:], cwd=EX)
    if out.returncode:
        print(step[0], "FAILED:", (out.stdout + out.stderr)[-600:])
        sys.exit(4)
print("next:", next_id, picked)
print("args:", rf"{EX}\batch-args\{next_id}.lean.json")
