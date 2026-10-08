#!/usr/bin/env bash
# =============================================================================
# eval-harness-contract-test.sh  (CR-T14)
#
# The shell side of the retrieval-quality harness, run against a stand-in for Docker:
#
#   scripts/eval-attribution-study.sh   the chunked cells, the token per slice, the merge, the query classes
#   scripts/eval-variant-cell.sh        one variant of the contextual organization: its cells, its state, its coverage,
#                                       and the refusal to measure an organization that changes under it
#   scripts/eval-nightly.sh             the boot-time check of its three switches
#   the whole chain                     the cells it writes are what eval-gate.py scores and --promote judges
#
# WHAT THE STAND-IN IS, AND IS NOT. `docker` is a Python script (written below) that answers the five calls these scripts
# make: container health, a token mint, a retrieval-driver run, an HTTP call into a service container, and a psql
# SELECT. It writes the result files the real driver would, with ranks it is told to. It proves the plumbing: the
# slicing, the merge, one token per slice, the file names the gate looks for, what happens when a slice writes nothing.
# It proves NOTHING about retrieval, about the services, or about the real auth: the HTTP routes are proved by
# the Go tests, the contracts by the pinned goldens, and the paid path by the live proof of the runbook.
# =============================================================================
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRIPTS="$ROOT/scripts"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail() { printf 'FAIL: %s\n' "$1" >&2; exit 1; }
ok() { printf '  ok: %s\n' "$1"; }

# ── the stand-in for docker ──────────────────────────────────────────────────
mkdir -p "$TMP/bin"
cat >"$TMP/shim.py" <<'PYEOF'
import json, os, sys

args = sys.argv[1:]
log = os.environ["FAKE_LOG"]
fixtures = os.environ["FAKE_FIXTURES"]


def note(*parts):
    with open(log, "a", encoding="utf-8") as handle:
        handle.write(" ".join(str(p) for p in parts) + "\n")


def env_pairs(items):
    out = {}
    i = 0
    while i < len(items):
        if items[i] == "-e":
            key, _, value = items[i + 1].partition("=")
            out[key] = value
            i += 2
        else:
            i += 1
    return out


def fixture(name):
    return json.load(open(os.path.join(fixtures, name), encoding="utf-8"))


if not args:
    sys.exit(2)

if args[0] == "inspect":
    print("healthy")
    sys.exit(0)

if args[0] == "exec" and "auth-service" in args:
    env = env_pairs(args)
    if not env.get("CRED"):
        sys.exit(1)
    note("mint", env["ORG"])
    sys.stdout.write("tok-" + env["ORG"])
    sys.exit(0)

if args[0] == "run":
    env = env_pairs(args)
    volume = args[args.index("-v") + 1]
    host = volume.rsplit(":/work", 1)[0]
    label = args[-1]
    note("run", f"label={label}", f"org={env['EVAL_ORG']}", f"offset={env['EVAL_OFFSET']}", f"rerank={env.get('EVAL_RERANK', 'default')}",
         f"topk={env.get('EVAL_TOP_K', '30')}", f"nomix={env.get('EVAL_NO_MIX', '0')}", f"golden={env['EVAL_GOLDEN_FILE'].rsplit('/', 1)[-1]}")
    for needle in filter(None, os.environ.get("FAKE_NO_OUTPUT", "").split(",")):
        if needle in label:
            sys.exit(0)
    golden = json.load(open(os.path.join(host, env["EVAL_GOLDEN_FILE"].rsplit("/", 1)[-1]), encoding="utf-8"))
    offset, limit = int(env["EVAL_OFFSET"]), int(env["EVAL_LIMIT"])
    rank = 1
    for needle, value in json.loads(os.environ.get("FAKE_PROFILE", "[]")):
        if needle in label:
            rank = None if value == "miss" else int(value)
            break
    rows = []
    for g in golden[offset:offset + limit]:
        retrieved = []
        for position in range(1, 11):
            if rank == position:
                retrieved.append({"document_id": g["relevant_ids"][0], "knowledge_id": "k-" + g["relevant_ids"][0]})
            else:
                retrieved.append({"document_id": f"x{position}-{abs(hash(g['query'])) % 1000}", "knowledge_id": f"y{position}"})
        rows.append({"query": g["query"], "relevant_ids": g["relevant_ids"], "latency_ms": 100.0, "retrieved": retrieved})
    out_name = env["OUT_FILE"].rsplit("/", 1)[-1]
    json.dump(rows, open(os.path.join(host, out_name), "w", encoding="utf-8"))
    print(f"  latency p50=100ms p95=120ms zero-result=0")
    sys.exit(0)

if args[0] == "exec" and "-i" in args:
    # an HTTP call into a service container: docker exec -i -e TOK=... <container> sh -c <script> sh METHOD URL [-H ...]
    env = env_pairs(args)
    tail = args[args.index("sh", args.index("-c")) + 1:]
    method, url, rest = tail[0], tail[1], tail[2:]
    body = sys.stdin.buffer.read()
    org = env["TOK"].split("tok-", 1)[1]
    path = "/" + url.split("//", 1)[1].split("/", 1)[1]
    note("http", method, path, f"org={org}")
    label = "baseline" if org.endswith("baseline") else "contextual"
    if os.environ.get("FAKE_HTTP_DOWN"):
        sys.stdout.write("\n503")
        sys.exit(0)
    if path == "/v1/quality/context-coverage":
        counter = os.path.join(os.environ["FAKE_STATE_DIR"], "coverage-calls-" + label)
        calls = int(open(counter).read()) if os.path.exists(counter) else 0
        open(counter, "w").write(str(calls + 1))
        report = fixture("coverage.baseline-off.golden.json" if label == "baseline" else "coverage.contextual-ready.golden.json")
        if os.environ.get("FAKE_CHANGE_COVERAGE") and calls >= 1 and label == "contextual":
            report["counts"]["ready"] -= 3
            report["counts"]["pending"] += 3
        sys.stdout.write(json.dumps(report) + "\n200")
    elif path == "/v1/quality/gates":
        sys.stdout.write(json.dumps(fixture("gates.in-sync.golden.json")) + "\n200")
    elif path.endswith("/processing-policy"):
        sys.stdout.write(json.dumps({"org_id": org, "policy": {"contextual_retrieval": "off" if label == "baseline" else "inherit",
                                                                "context_prompt_version": None, "source": "policy"}}) + "\n200")
    elif path == "/v1/evals/results":
        headers = [rest[i + 1] for i, v in enumerate(rest) if v == "-H"]
        note("record", f"org={org}", "key=" + [h.split(": ", 1)[1] for h in headers if h.startswith("Idempotency-Key")][0])
        sys.stdout.write('{"eval_id": "e1"}\n201')
    else:
        sys.stdout.write("{}\n404")
    sys.exit(0)

if args[0] == "exec" and "psql" in args:
    sql = args[-1]
    if "GROUP BY" in sql and "context_prompt_version" in sql:
        print(json.dumps({"prompt_version": os.environ.get("FAKE_PROV_VERSION", "ctx-v2"), "model": "gpt-4o-mini",
                          "context_status": "ready", "units": 1164}))
        sys.exit(0)
    sys.exit(0)

if args[0] == "exec" and "retrieval-engine" in " ".join(args):
    sys.stdout.write(os.environ.get("FAKE_RERANK_FLAG", "0"))
    sys.exit(0)

note("UNEXPECTED", *args[:6])
sys.exit(91)
PYEOF
# The scripts run `docker run` with MSYS2_ARG_CONV_EXCL='*' (Git for Windows would otherwise rewrite the /work of the
# volume argument), which also switches off the conversion of THIS wrapper's own path argument: so hand python a path
# it understands whatever the shell is (cygpath -m on Windows, the path itself elsewhere).
SHIM_PATH=$(cygpath -m "$TMP/shim.py" 2>/dev/null || echo "$TMP/shim.py")
cat >"$TMP/bin/docker" <<EOF
#!/bin/sh
exec python3 "$SHIM_PATH" "\$@"
EOF
chmod +x "$TMP/bin/docker"

# Paths the stand-in reads from its environment must be ones python understands even where Git for Windows has path
# conversion switched off (the scripts run `docker run` with MSYS_NO_PATHCONV=1, which also stops the conversion of
# environment variables): cygpath -m on Windows, the path itself elsewhere.
winpath() { cygpath -m "$1" 2>/dev/null || echo "$1"; }
export FAKE_LOG="$(winpath "$TMP/docker.log")" FAKE_FIXTURES="$(winpath "$SCRIPTS/tests/fixtures/eval-evidence-v1")" FAKE_STATE_DIR="$(winpath "$TMP/state")"
export PATH="$TMP/bin:$PATH"
# The shell scripts find the stand-in on PATH. The Python scripts start `docker` themselves, and on Windows a program
# cannot be a shell script, so they take the executable from EVAL_DOCKER (their documented override).
export EVAL_DOCKER="python3 $SHIM_PATH"
export DATA_PLANE_CORPUS_SEEDER_API_KEY="credential-that-must-never-be-logged"
mkdir -p "$TMP/state"
: >"$FAKE_LOG"

# ── helpers ───────────────────────────────────────────────────────────────────
fresh_work() { # fresh_work <name>: a work dir with the driver placeholder and small goldens
  local dir="$TMP/$1"
  rm -rf "$dir" "$TMP/state"; mkdir -p "$dir" "$TMP/state"; : >"$FAKE_LOG"
  : >"$dir/run_eval_hybrid.py"
  python3 - "$dir" <<'PYEOF'
import json, sys
d = sys.argv[1]
def golden(name, n, prefix):
    json.dump([{"query": f"{prefix} query {i}", "relevant_ids": [f"{prefix}-doc-{i}"]} for i in range(n)], open(f"{d}/{name}", "w"))
for stem, n, prefix in (("golden-v2", 30, "nl"), ("golden-v2-no", 12, "no"), ("golden-v2-mixed", 10, "mx")):
    golden(stem + ".json", n, prefix)
    golden(stem + "-ctx.json", n, prefix)
golden("golden-lexical.json", 23, "lex"); golden("golden-lexical-ctx.json", 23, "lex")
PYEOF
  echo "$dir"
}
rows_in() { python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1], encoding="utf-8"))))' "$1"; }
count_log() { grep -c -- "$1" "$FAKE_LOG" || true; }

for script in eval-lib.sh eval-attribution-study.sh eval-variant-cell.sh eval-nightly.sh; do
  sh -n "$SCRIPTS/$script" || fail "$script does not parse as POSIX sh"
done
bash -n "$SCRIPTS/check-eval-contract.sh" || fail "check-eval-contract.sh does not parse"
ok "every harness shell script parses"

# ── 1. the study ─────────────────────────────────────────────────────────────
W=$(fresh_work study)
sh "$SCRIPTS/eval-attribution-study.sh" "$W" >"$TMP/study.out" 2>&1 || { cat "$TMP/study.out" >&2; fail "the study failed"; }
grep -q "STUDY COMPLETE" "$TMP/study.out" || fail "the study did not complete"
for cell in baseline contextual norwegian-baseline norwegian-contextual mixed-baseline mixed-contextual; do
  [ -f "$W/st-rr-on-$cell.json" ] || { head -40 "$TMP/study.out" >&2; head -20 "$FAKE_LOG" >&2; fail "the study wrote no st-rr-on-$cell.json"; }
done
[ "$(rows_in "$W/st-rr-on-baseline.json")" = 30 ] || fail "the 30 queries of two slices were not merged into one cell"
[ "$(rows_in "$W/st-rr-on-norwegian-contextual.json")" = 12 ] || fail "the norwegian cell is not 12 rows"
[ "$(rows_in "$W/st-rr-on-mixed-baseline.json")" = 10 ] || fail "the mixed cell is not 10 rows"
for cell in rr-off loo-graph loo-wiki loo-visual loo-keyword floor; do
  [ -f "$W/st-$cell-baseline.json" ] && [ -f "$W/st-$cell-contextual.json" ] || fail "the attribution cell $cell is missing"
  [ ! -f "$W/st-$cell-norwegian-baseline.json" ] || fail "an attribution variant ran the language classes"
done
ls "$W"/*chunk* >/dev/null 2>&1 && fail "slice files were left behind"
ok "the study writes every cell under the names the gate collects, merged and without leftovers"

mints=$(grep -c '^mint org-corpus-baseline' "$FAKE_LOG")
runs=$(grep -c '^run label=rr-on baseline' "$FAKE_LOG")
[ "$runs" = 2 ] || fail "the 30-query natural cell should be two slices, got $runs"
# a fresh token per slice: as many mints as slices, over every baseline cell
slices=$(grep -c '^run .*org=org-corpus-baseline' "$FAKE_LOG")
[ "$mints" = "$slices" ] || fail "tokens minted ($mints) differ from slices run ($slices): a slice reused an old token"
ok "one fresh token per slice ($slices slices, $mints mints)"
grep -q "credential-that-must-never-be-logged" "$FAKE_LOG" "$TMP/study.out" && fail "the credential reached a log"
ok "the credential is in no log"

# the cells it wrote are what the gate scores
python3 "$SCRIPTS/eval-gate.py" "$W" --baseline "$TMP/study-baseline.json" --update >"$TMP/gate-update.out" 2>&1 || { cat "$TMP/gate-update.out" >&2; fail "the gate could not baseline the study's cells"; }
python3 "$SCRIPTS/eval-gate.py" "$W" --baseline "$TMP/study-baseline.json" >"$TMP/gate.out" 2>&1 || { cat "$TMP/gate.out" >&2; fail "the gate fails the study's own cells against their own baseline"; }
python3 -c 'import json,sys; b=json.load(open(sys.argv[1])); assert {"norwegian-baseline","mixed-contextual"} <= set(b), sorted(b)' "$TMP/study-baseline.json" || fail "the language classes are not baselined"
ok "the gate scores the study's cells, language classes included"

W=$(fresh_work study-noclass)
rm "$W/golden-v2-no.json" "$W/golden-v2-no-ctx.json"
sh "$SCRIPTS/eval-attribution-study.sh" "$W" >"$TMP/study2.out" 2>&1 || fail "a missing optional class stopped the study"
grep -q "skipped, golden-v2-no.json is not in" "$TMP/study2.out" || fail "the skipped class was not said out loud"
[ ! -f "$W/st-rr-on-norwegian-baseline.json" ] && [ -f "$W/st-rr-on-mixed-baseline.json" ] || fail "a skipped class left a file, or took the others down"
ok "a class without a golden file is skipped out loud and the rest runs"

W=$(fresh_work study-fail)
FAKE_NO_OUTPUT="rr-on norwegian-contextual" sh "$SCRIPTS/eval-attribution-study.sh" "$W" >"$TMP/study3.out" 2>&1 || true
[ ! -f "$W/st-rr-on-norwegian-contextual.json" ] || fail "a cell whose every slice wrote nothing left a file (an empty one would be averaged over zero queries)"
grep -q "no chunks produced any output" "$TMP/study3.out" || fail "the missing cell was not named"
[ -f "$W/st-rr-on-norwegian-baseline.json" ] || fail "one dead cell stopped the others"
ok "a cell that wrote nothing leaves NO file, is named, and does not stop the rest"

STUDY_CLASSES="natural french" sh "$SCRIPTS/eval-attribution-study.sh" "$W" >/dev/null 2>&1 && fail "an unknown class was accepted"
[ "$(STUDY_CLASSES="natural french" sh "$SCRIPTS/eval-attribution-study.sh" "$W" >/dev/null 2>&1; echo $?)" = 2 ] || fail "an unknown class must be a usage error (2)"
[ "$(STUDY_CLASSES="mixed" sh "$SCRIPTS/eval-attribution-study.sh" "$W" >/dev/null 2>&1; echo $?)" = 2 ] || fail "a study without the natural class must be refused"
ok "the study's one switch is checked at boot"

[ "$(env -u DATA_PLANE_CORPUS_SEEDER_API_KEY sh "$SCRIPTS/eval-attribution-study.sh" "$W" >/dev/null 2>&1; echo $?)" = 1 ] || fail "no credential must stop the study"
ok "no credential, no study"

# ── 2. the variant cell ──────────────────────────────────────────────────────
variant_dir() { # variant_dir <name>: a work dir for variants (contextual goldens only)
  local dir; dir=$(fresh_work "$1"); echo "$dir"
}
V=$(variant_dir variants)
sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1 --prompt ctx-v1 >"$TMP/v1.out" 2>&1 || { cat "$TMP/v1.out" >&2; fail "the ctx-v1 run failed"; }
for f in state-ctx-v1.json coverage-ctx-v1.json cell-ctx-v1-rr-off.json cell-ctx-v1-rr-on-1.json cell-ctx-v1-rr-on-2.json cell-ctx-v1-rr-on-3.json cell-ctx-v1-lexical.json; do
  [ -f "$V/$f" ] || fail "the variant run wrote no $f"
done
[ "$(rows_in "$V/cell-ctx-v1-lexical.json")" = 23 ] || fail "the lexical class is not its 23 queries"
for i in 1 2 3; do
  grep -q "^run label=ctx-v1 rr-on-$i .*rerank=on topk=$((29 + i))" "$FAKE_LOG" || fail "rerank-on run $i did not carry its own top_k (the retrieval cache key)"
done
grep -q "^run label=ctx-v1 rr-off .*rerank=off" "$FAKE_LOG" || fail "the attribution cell did not run with the reranker off"
grep -q "^run label=ctx-v1 lexical .*nomix=1" "$FAKE_LOG" || fail "the lexical class did not run the default path"
ok "a variant run writes its state, coverage, rerank-off cell, three rerank-on cells (own top_k each) and the lexical cell"

FAKE_PROV_VERSION=ctx-v2 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v2 --prompt ctx-v2 >"$TMP/v2.out" 2>&1 || { cat "$TMP/v2.out" >&2; fail "the ctx-v2 run failed"; }
python3 "$SCRIPTS/eval-gate.py" "$V" --promote ctx-v2 --against ctx-v1 >"$TMP/promote.out" 2>&1 && fail "ctx-v1's own state (units of ctx-v2 under a ctx-v1 declaration) was accepted"
grep -q "state of ctx-v1 is not the declared one" "$TMP/promote.out" || { cat "$TMP/promote.out" >&2; fail "the gate did not name the state mismatch"; }
ok "the gate refuses a default whose units are not the version it declares (the shim's units are ctx-v2)"

V=$(variant_dir variants-ok)
FAKE_PROV_VERSION=ctx-v1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1 --prompt ctx-v1 >/dev/null 2>&1 || fail "ctx-v1 run failed"
FAKE_PROV_VERSION=ctx-v2 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v2 --prompt ctx-v2 >/dev/null 2>&1 || fail "ctx-v2 run failed"
python3 "$SCRIPTS/eval-gate.py" "$V" --promote ctx-v2 --against ctx-v1 >"$TMP/promote2.out" 2>&1 || { cat "$TMP/promote2.out" >&2; fail "an equal candidate with the right state was not promoted"; }
grep -q "PROMOTION VERDICT: PROMOTE" "$TMP/promote2.out" || fail "no PROMOTE verdict"
ok "two variants measured by the shell scripts are promoted by the gate when equal and in their declared states"

V=$(variant_dir variants-worse)
FAKE_PROV_VERSION=ctx-v1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1 --prompt ctx-v1 >/dev/null 2>&1 || fail "ctx-v1 run failed"
FAKE_PROFILE='[["ctx-v2 rr-off", 3]]' FAKE_PROV_VERSION=ctx-v2 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v2 --prompt ctx-v2 >/dev/null 2>&1 || fail "ctx-v2 run failed"
python3 "$SCRIPTS/eval-gate.py" "$V" --promote ctx-v2 --against ctx-v1 >"$TMP/promote3.out" 2>&1 && fail "a worse candidate was promoted"
grep -q "non-inferiority failed" "$TMP/promote3.out" || fail "the hold did not say why"
ok "a worse candidate is held"

V=$(variant_dir variants-changing)
if FAKE_CHANGE_COVERAGE=1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v2 --prompt ctx-v2 >"$TMP/changing.out" 2>&1; then fail "an organization that changed under the run was measured"; fi
grep -q "CHANGED while the cells ran" "$TMP/changing.out" || fail "the change was not named"
ls "$V"/cell-ctx-v2-* "$V"/state-ctx-v2.json "$V"/coverage-ctx-v2.json >/dev/null 2>&1 && fail "the files of a measurement of no single state were left"
ok "an organization that changes while the cells run fails the run and removes its files"

V=$(variant_dir variants-rerun)
FAKE_PROV_VERSION=ctx-v1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1 --prompt ctx-v1 >/dev/null 2>&1 || fail "ctx-v1 run failed"
FAKE_PROV_VERSION=ctx-v1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1-rc --prompt ctx-v1 >/dev/null 2>&1 || fail "ctx-v1-rc run failed"
[ -f "$V/cell-ctx-v1-rr-on-2.json" ] || fail "the first run wrote no rr-on-2 cell"
if FAKE_NO_OUTPUT="ctx-v1 rr-on-2" FAKE_PROV_VERSION=ctx-v1 sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v1 --prompt ctx-v1 >"$TMP/rerun.out" 2>&1; then
  fail "a re-run in which a slice wrote nothing was called complete"
fi
[ ! -f "$V/cell-ctx-v1-rr-on-2.json" ] || fail "a cell file of the earlier run survived a re-run in which that cell wrote nothing (the gate would judge a mix of two runs)"
grep -q "cell-ctx-v1-rr-on-2.json was not written" "$TMP/rerun.out" || { cat "$TMP/rerun.out" >&2; fail "the missing cell was not named"; }
[ -f "$V/cell-ctx-v1-rc-rr-off.json" ] && [ -f "$V/state-ctx-v1-rc.json" ] || fail "re-running ctx-v1 removed the files of ctx-v1-rc, a variant whose name starts with it"
ok "a re-run in which a cell writes nothing leaves no file of the earlier run and fails; another variant's files are left alone"

V=$(variant_dir variants-usage)
[ "$(sh "$SCRIPTS/eval-variant-cell.sh" "$V" breadcrumb >/dev/null 2>&1; echo $?)" = 2 ] || fail "breadcrumb must be refused (not built)"
[ "$(sh "$SCRIPTS/eval-variant-cell.sh" "$V" ctx-v2 >/dev/null 2>&1; echo $?)" = 2 ] || fail "a variant that declares nothing must be refused"
[ "$(sh "$SCRIPTS/eval-variant-cell.sh" "$V" "../x" --prompt ctx-v2 >/dev/null 2>&1; echo $?)" = 2 ] || fail "a variant name that escapes the work dir must be refused"
ok "breadcrumb, an undeclared variant and a bad name are refused (exit 2)"

# ── 3. the evidence and the records, end to end ──────────────────────────────
W=$(fresh_work records)
sh "$SCRIPTS/eval-attribution-study.sh" "$W" >/dev/null 2>&1 || fail "the study failed"
python3 "$SCRIPTS/eval-service.py" coverage "$W" >/dev/null 2>&1 || fail "coverage fetch failed"
python3 "$SCRIPTS/eval-service.py" gates "$W" >/dev/null 2>&1 || fail "gates fetch failed"
python3 "$SCRIPTS/eval-gate.py" "$W" --baseline "$TMP/records-baseline.json" --update >/dev/null 2>&1 || fail "could not baseline"
python3 "$SCRIPTS/eval-gate.py" "$W" --baseline "$TMP/records-baseline.json" --require-coverage --require-parity \
  --records-out "$W/records" --run-id "20261007T030000Z-shim" >"$TMP/records-gate.out" 2>&1 || { cat "$TMP/records-gate.out" >&2; fail "the gate with its evidence failed"; }
[ "$(ls "$W/records" | wc -l | tr -d ' ')" = 6 ] || fail "six cells, six records, got $(ls "$W/records" | wc -l)"
python3 "$SCRIPTS/eval-service.py" record "$W/records" >"$TMP/record.out" 2>&1 || { cat "$TMP/record.out" >&2; fail "recording failed"; }
[ "$(count_log '^record ')" = 6 ] || fail "six records were not sent"
grep -q '^record org=org-corpus-baseline key=harness-20261007T030000Z-shim-norwegian-baseline' "$FAKE_LOG" || fail "a record went to the wrong organization or under another key"
ok "the chain study, evidence, gate, records, record works through the real scripts"

FAKE_HTTP_DOWN=1 python3 "$SCRIPTS/eval-service.py" coverage "$W" >/dev/null 2>&1 && fail "an unavailable service produced coverage evidence"
[ ! -f "$W/coverage-contextual.json" ] || fail "a stale coverage file survived a failed fetch"
python3 "$SCRIPTS/eval-gate.py" "$W" --baseline "$TMP/records-baseline.json" --require-coverage >"$TMP/nocov.out" 2>&1 && fail "the gate passed without the coverage evidence it requires"
ok "an unavailable service leaves no evidence, and the gate that requires it fails"

# ── 4. the nightly's switches ─────────────────────────────────────────────────
for bad in "EVAL_RECORD_RESULTS=2" "EVAL_REQUIRE_COVERAGE=yes" "EVAL_REQUIRE_PARITY=true"; do
  set +e
  env "$bad" sh "$SCRIPTS/eval-nightly.sh" "$TMP/nightly-work" >"$TMP/nightly.out" 2>&1
  code=$?
  set -e
  [ "$code" = 2 ] || { cat "$TMP/nightly.out" >&2; fail "$bad: the nightly must stop at boot with 2, got $code"; }
  grep -q "${bad%%=*} must be 0 or 1" "$TMP/nightly.out" || fail "$bad: the refusal does not name the switch"
done
ok "the nightly refuses a bad switch at boot, naming it, before it touches anything"

printf 'PASS: eval harness contract\n'
