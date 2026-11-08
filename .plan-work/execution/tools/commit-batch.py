"""Commit the slices of one wave-mode batch, one commit per slice, and book the batch in the queue, the ledger and the depth register.

usage: python commit-batch.py TASK_OUTPUT_FILE ARGS_FILE [--dry] [--only ID,ID]
  TASK_OUTPUT_FILE  the workflow's task output (JSON with a `result` field: {batchId, wave, results[]})
  ARGS_FILE         the args JSON the batch was started with (for each slice's title, summary and files)
Slices whose outcome is not ready / ready-partial are left uncommitted and listed. Never run docker; the pre-commit gitleaks hook does that itself.
"""
import datetime
import json
import os
import re
import subprocess
import sys

REPO = r"C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b"
EX = REPO + r"\.plan-work\execution"
REGISTER = REPO + r"\docs\plans\model-plane-depth-register.md"
LEDGER = REPO + r"\apps\Model Plane\docs\decisions\ledger.md"
ANCHOR = "## 2026-10-21 - The reasoning-effort ladder lives in inference-core (WS11-S1)"
TRAILER = "\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n"


NOISE = re.compile(r"uncommitted changes|stage only|stage the|commit the \d+ files|the lead commits|nothing (is )?staged|commit (these|the) files|files_changed", re.I)
CAP = 8


def tokens(s):
    return set(re.findall(r"[a-z0-9_]{4,}", s.lower()))


def reduce_findings(items):
    """Drop noise and near-duplicates (implementer and verifier often say the same), keep P0 and P1 first, cap per slice."""
    order = {"P0": 0, "P1": 1, "P2": 2, "P3": 3}
    items = [f for f in items if f.get("text") and not NOISE.search(f["text"])]
    items.sort(key=lambda f: order.get(f.get("severity", "P2"), 2))
    kept, toks = [], []
    for f in items:
        tk = tokens(f["text"])
        if any(tk and o and len(tk & o) / len(tk | o) > 0.5 for o in toks):
            continue
        kept.append(f)
        toks.append(tk)
    extra = 0
    if len(kept) > CAP:
        extra = len(kept) - CAP
        kept = kept[:CAP]
    return kept, extra


def write_bytes(path, data):
    import time
    for _ in range(6):
        try:
            with open(path, "wb") as f:
                f.write(data)
            return
        except OSError:
            time.sleep(1.5)
    raise OSError("could not write " + path)

task_out, args_file = sys.argv[1], sys.argv[2]
dry = "--dry" in sys.argv
book_only = "--book-only" in sys.argv
force = "--force" in sys.argv  # also commit needs-attention slices whose blockers the lead has since fixed
only = set(sys.argv[sys.argv.index("--only") + 1].split(",")) if "--only" in sys.argv else set()

res = json.load(open(task_out, encoding="utf-8"))["result"]
if isinstance(res, str):
    res = json.loads(res)
args = json.load(open(args_file, encoding="utf-8"))
by_arg = {s["id"]: s for s in args["slices"]}
batch, wave = res["batchId"], res.get("wave") or args.get("wave")
force_tier = args.get("forceTier")
today = datetime.date.today().isoformat()


def git(*a, inp=None):
    return subprocess.run(["git", *a], cwd=REPO, capture_output=True, text=True, input=inp, encoding="utf-8", errors="replace")


def clean_path(p):
    p = re.sub(r"\s+\(.*$", "", p.strip()).replace("\\", "/")
    root = REPO.replace("\\", "/").rstrip("/") + "/"
    return p[len(root):] if p.lower().startswith(root.lower()) else p


def kind_of(files):
    low = [f.lower() for f in files]
    if all(f.endswith((".md", ".txt", ".json", ".sha256", ".yaml", ".yml", ".tsv")) or "/docs/" in f or "/contracts/" in f for f in low):
        return "docs"
    if all("/tests/" in f or "_test." in f or ".test." in f or ".spec." in f or f.endswith(("test.sh", "-test.sh")) for f in low):
        return "test"
    return "feat"


def scope_of(files):
    for f in files:
        m = re.search(r"apps/Model Plane/(?:rust|go)/(?:services|crates)/([^/]+)", f)
        if m:
            return m.group(1)
    for f in files:
        if "Frontend Plane" in f:
            return "verevonv3"
        m = re.search(r"apps/Control Plane/([^/.]+)/", f)
        if m:
            return m.group(1)
        m = re.search(r"apps/(Ingestion Plane|Application Plane|Data Plane v2)/", f)
        if m:
            return m.group(1).split()[0].lower()
    return "model-plane"


os.makedirs(os.path.join(EX, "results"), exist_ok=True)
json.dump(res, open(os.path.join(EX, "results", f"{batch}.json"), "w", encoding="utf-8"), indent=1, ensure_ascii=False)
q = json.load(open(EX + r"\queue.json", encoding="utf-8"))
qby = {r["id"]: r for r in q}
rows, strict, shared, summary = [], [], [], []
counts = {"committed": 0, "left": 0}


def next_id(prefix, text):
    nums = [int(n) for n in re.findall(rf"\b{prefix}-(\d+)\b", text)]
    return (max(nums) if nums else 0) + 1


reg = open(REGISTER, encoding="utf-8").read()
nid = next_id(f"D{wave}", reg)

for r in res["results"]:
    sid = r["id"]
    if only and sid not in only:
        continue
    if r.get("outcome") not in ("ready", "ready-partial") and not (force and r.get("outcome") == "needs-attention"):
        summary.append(f"LEFT   {sid}: {r.get('outcome')} {r.get('reason', '')}")
        counts["left"] += 1
        continue
    files = []
    for f in r.get("files_changed", []):
        f = clean_path(f)
        if f and os.path.exists(os.path.join(REPO, f)) and f not in files:
            files.append(f)
    if not files:
        summary.append(f"EMPTY  {sid}: no files to commit")
        counts["left"] += 1
        continue
    a = by_arg.get(sid, {})
    title_src = a.get("title") or qby.get(sid, {}).get("title") or sid
    typ, scope = kind_of(files), scope_of(files)
    # Lower the first letter only for an ordinary word, never an acronym (SPA, ZDR).
    first = (title_src[0].lower() + title_src[1:] if title_src[1:2].islower() else title_src) if title_src else sid
    title = f"{typ}({scope}): {first} ({sid})"
    body = [a.get("summary") or qby.get(sid, {}).get("summary") or ""]
    if r.get("status") == "partial":
        body.append("PARTIAL: " + (r.get("notes") or "")[:600])
    tests = [f"- {t['command'][:140]}: {t['outcome'][:200]}" for t in r.get("tests_run", [])[:8]]
    body.append("Tests (the slice's own, run by the implementing agent):\n" + ("\n".join(tests) if tests else "- none named by the plan row"))
    if r.get("deviations"):
        body.append("Deviations: " + " | ".join(d[:250] for d in r["deviations"][:6]))
    findings = list(r.get("depth_findings", []))
    for d in r.get("verifier", {}).get("dod_gaps", []) if r.get("verifier") else []:
        findings.append({"kind": "live-proof", "severity": "P1", "text": d})
    for d in r.get("needs_other_files", []):
        findings.append({"kind": "follow-up", "severity": "P2", "text": "needed an edit outside the slice's files: " + d})
    for t in r.get("lead_todo", []):
        if t.startswith("shared-edit"):
            shared.append((sid, t))
        else:
            findings.append({"kind": "live-proof", "severity": "P1", "text": t})
    body.append(f"Built in wave mode ({batch}, {'haiku high' if force_tier else 'sonnet'}): no hostile review, no fix round beyond blocking defects; {len(findings)} depth finding(s) after de-duplication are in docs/plans/model-plane-depth-register.md.")
    msg = title + "\n\n" + "\n\n".join(b for b in body if b).strip() + "\n" + TRAILER
    if r.get("unreviewed_risk"):
        strict.append((sid, ", ".join(r["unreviewed_risk"])))
    findings, extra = reduce_findings(findings)
    for f in findings:
        rows.append((sid, f.get("kind", "follow-up"), f.get("severity", "P2"), " ".join(str(f.get("text", "")).split())))
    if extra:
        rows.append((sid, "follow-up", "P3", f"{extra} more lower-priority notes from the implementer and the verifier are in .plan-work/execution/results/{batch}.json"))
    if book_only:
        rec0 = qby.get(sid) or {}
        if f"({batch})" not in (rec0.get("remaining") or ""):
            rows = [x for x in rows if x[0] != sid]
            strict = [x for x in strict if x[0] != sid]
            shared = [x for x in shared if x[0] != sid]
            summary.append(f"SKIP   {sid}: not committed in {batch}")
            continue
        summary.append(f"BOOK   {sid}")
        counts["committed"] += 1
        continue
    if dry:
        summary.append(f"DRY    {sid}: {len(files)} files -> {title}")
        continue
    add = git("add", "--", *files)
    if add.returncode:
        summary.append(f"ADDFAIL {sid}: {add.stderr[-200:]}")
        counts["left"] += 1
        continue
    # Isolated builders may share files: an earlier slice's commit can already hold all of this slice's changes.
    shared_only = git("diff", "--cached", "--quiet").returncode == 0
    if not shared_only:
        c = git("commit", "-q", "-F", "-", inp=msg)
        if c.returncode:
            summary.append(f"COMMITFAIL {sid}: {(c.stderr or c.stdout)[-300:]}")
            counts["left"] += 1
            git("reset", "-q", "--", *files)
            continue
    sha = git("rev-parse", "--short", "HEAD").stdout.strip()
    summary.append(f"OK     {sid}: {sha} {title[:90]}" + (" (changes already in that commit)" if shared_only else ""))
    counts["committed"] += 1
    rec = qby.get(sid)
    if rec is not None:
        if r.get("status") == "done" and r.get("outcome") == "ready":
            rec["status"], rec["remaining"] = "done", f"committed {sha} ({batch})"
        else:
            rec["status"], rec["remaining"] = "partial", f"committed {sha} ({batch}); {(r.get('notes') or '')[:200]}"

if not dry:
    if not book_only:
        json.dump(q, open(EX + r"\queue.json", "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    # depth register: a table per batch at the end of the wave's section, the strict slices at the end of the file
    if rows or strict:
        reg = open(REGISTER, encoding="utf-8").read()
        crlf = "\r\n" in reg
        reg = reg.replace("\r\n", "\n")
        if rows:
            tbl = [f"\n### Wave-mode batch {batch} ({today})\n", "| ID | Slice | Kind | P | Finding | Status |", "|---|---|---|---|---|---|"]
            for sid, kind, sev, text in rows:
                tbl.append(f"| D{wave}-{nid:03d} | {sid} | {kind} | {sev} | {text.replace('|', '/')} | open |")
                nid += 1
            block = "\n".join(tbl) + "\n"
            marker = "## Wave 2 (WS13 to WS27)" if wave == 1 else "## Cross-wave"
            reg = reg.replace(marker, block + "\n" + marker, 1) if marker in reg else reg + block
        for sid, risk in strict:
            reg = reg.rstrip("\n") + f"\n| {sid} | {risk} | {batch} | no |\n"
        open(REGISTER, "w", encoding="utf-8", newline="").write(reg.replace("\n", "\r\n") if crlf else reg)
    # ledger: the slices' own decision paragraphs
    led = [(sid, t) for sid, t in shared if re.search(r"ledger\.md", t.split(":", 1)[0])]
    other = [(sid, t) for sid, t in shared if (sid, t) not in led]
    if led:
        raw = open(LEDGER, "rb").read().decode("utf-8")
        crlf = "\r\n" in raw
        t = raw.replace("\r\n", "\n")
        paras = "\n\n".join(f"**{sid}.** " + re.sub(r"^shared-edit [^:]*?ledger\.md\s*(\([^)]*\))?:\s*", "", tx).strip() for sid, tx in led)
        entry = f"## {today} - Wave-mode batch {batch}: decisions recorded by its slices\n\n**Status: built in wave mode (unit tests only; depth in the register).**\n\n{paras}\n\n"
        if t.count(ANCHOR) == 1:
            t = t.replace(ANCHOR, entry + ANCHOR)
            open(LEDGER, "wb").write((t.replace("\n", "\r\n") if crlf else t).encode("utf-8"))
    if other:
        with open(EX + rf"\shared-edits-{batch}.md", "w", encoding="utf-8") as f:
            for sid, tx in other:
                f.write(f"## {sid}\n{tx}\n\n")
    # release the batch's file locks
    subprocess.run(["python", EX + r"\tools\inflight.py", "release", batch], capture_output=True)
    # the bookkeeping edits (register, ledger) go in their own docs commit
    docs_paths = [REGISTER, LEDGER]
    git("add", "--", *docs_paths)
    if git("diff", "--cached", "--quiet", "--", *docs_paths).returncode:
        dc = git("commit", "-q", "-m", f"docs(model-plane): the depth register and the ledger take the findings and decisions of wave-mode batch {batch}\n" + TRAILER, "--", *docs_paths)
        summary.append("DOCS   committed" if dc.returncode == 0 else "DOCS   FAILED " + (dc.stderr or dc.stdout)[-200:].replace("\n", " "))

print("\n".join(summary))
print(f"{batch}: committed {counts['committed']}, left {counts['left']}, findings {len(rows)}, unreviewed strict {len(strict)}, shared edits {len(shared)} ({len([1 for s in shared if 'ledger.md' in s[1].split(':',1)[0]])} to the ledger)")
