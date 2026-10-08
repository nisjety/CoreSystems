#!/bin/sh
# Run the full retrieval-quality eval unattended and gate on the result.
#
# ## Why this exists, and why it runs where it runs
#
# The eval needs three things a GitHub-hosted runner cannot supply: an
# ALREADY-INGESTED corpus (the frozen corpus of scripts/eval-corpus-manifest.json,
# about 1,200 chunks, contextualized and embedded. Since CR-T14 the ingest is
# committed (scripts/eval-ingest-corpus.py), but it is a paid, owner-approved
# act (decision OD8) that this job never performs: one inference call per chunk
# makes "just re-ingest in CI" an expensive thing to do unattended), Control Plane's `auth-service`
# reachable to mint bearer tokens (its own bootstrap needs 11 gitignored
# per-service `.env`/`.env.docker` files that have no auto-generation script —
# `run-control-plane.sh` refuses to run without them), and live Cohere/Azure
# credentials that cost real money per call.
#
# All three already exist, continuously, on whatever machine runs the
# Data Plane v2 + Control Plane dev/eval stack — which is why this is designed
# to run on a SELF-HOSTED Actions runner registered on that machine (see
# .github/workflows/dataplane-v2-eval-nightly.yml), not to boot anything fresh.
# The corollary: this script reads the corpus-seeder credential from Control
# Plane's OWN generated-secrets file on that machine at run time. It is never a
# GitHub Actions secret, never written to a log, and never touches git.
#
# ## What is and is not durable
#
# The hand-authored judgments (87 natural-language, 28 Norwegian-morphology, 20
# mixed-language queries) are committed as QUERIES AND DOCUMENT TITLES in
# scripts/eval-build-golden-set.py — they cannot be regenerated (a human wrote
# them; the golden set's own docs say model-generated questions bias lexical
# measurements). The id-keyed files the retrieval driver reads are NOT
# committed state any more: document ids are assigned by documents-api, so a
# re-seeded organisation holds the same corpus under new ids, and a file
# committed once is stale from then on. This script builds them afresh at the
# start of every run (step 2), as it re-mines the 23 lexical judgments (SQL
# over the live corpus), so there is no window in which a judgment names a
# document that is no longer there.
#
# What this script does NOT do, on purpose: ingest the corpus. If the stack is
# torn down and rebuilt, this job fails loudly at the golden-set build ("ingest
# the corpus first") and the remedy is the owner-approved
# `eval-ingest-corpus.py apply`, which the runbook describes.
#
# ## What it records
#
# With EVAL_RECORD_RESULTS=1 every gated cell, and the gate's verdict on it, is
# stored in quality_eval_runs through POST /v1/evals/results (CR-T14), so that
# a regression is a query. Default 0: the route exists once the data-quality
# image of CR-T14 is deployed, and a nightly that posts to a route that is not
# there would fail for the wrong reason. EVAL_REQUIRE_COVERAGE and
# EVAL_REQUIRE_PARITY (default 1 here) make missing coverage or keyword-index
# parity evidence a failure of the gate; all three are checked first thing.
#
# ## A note on method, since this is exactly the trap round 8 named twice
#
# Every step below that must be allowed to fail the job runs to a FILE first,
# with its exit status checked explicitly, and only THEN gets tailed/cat for
# display. `cmd | tail` under `set -e` reports `tail`'s exit status, not
# `cmd`'s — a failing step behind a pipe like that would look green.
#
# Usage: ./scripts/eval-nightly.sh [work-dir]
#   work-dir defaults to a fresh temp directory; GitHub Actions sets
#   RUNNER_TEMP, which is preferred when present so nothing leaks between runs.
set -eu

SELF_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SELF_DIR/.." && pwd)

if [ $# -ge 1 ]; then
  WORK="$1"
elif [ -n "${RUNNER_TEMP:-}" ]; then
  WORK="$RUNNER_TEMP/dpv2-eval-$$"
else
  WORK="/tmp/dpv2-eval-$$"
fi
mkdir -p "$WORK"
echo "work dir: $WORK"

SUMMARY="${GITHUB_STEP_SUMMARY:-$WORK/summary.md}"
: > "$SUMMARY"
note() { printf '%s\n' "$1" | tee -a "$SUMMARY"; }
run_step() {
  # $1 = human label, $2 = log file, $@[3:] = command. Runs to a file, checks
  # the REAL exit status, tails a short preview into the summary either way,
  # then propagates the failure (set -e) if the command failed.
  label="$1"; log="$2"; shift 2
  note ""
  note "## $label"
  status=0
  "$@" > "$log" 2>&1 || status=$?
  note '```'
  tail -20 "$log" >> "$SUMMARY"
  note '```'
  return "$status"
}

note "# Data Plane v2 nightly retrieval eval — $(date -u +%Y-%m-%dT%H:%M:%SZ)"

# ── 0. The switches: each has a default, and a typo must not silently turn evidence off ──
EVAL_RECORD_RESULTS="${EVAL_RECORD_RESULTS:-0}"
EVAL_REQUIRE_COVERAGE="${EVAL_REQUIRE_COVERAGE:-1}"
EVAL_REQUIRE_PARITY="${EVAL_REQUIRE_PARITY:-1}"
for sw in EVAL_RECORD_RESULTS EVAL_REQUIRE_COVERAGE EVAL_REQUIRE_PARITY; do
  eval "sw_value=\$$sw"
  case "$sw_value" in
    0|1) ;;
    *) note "FAILED: $sw must be 0 or 1, got '$sw_value'"; exit 2 ;;
  esac
done
export EVAL_REQUIRE_COVERAGE EVAL_REQUIRE_PARITY
RUN_ID="${GITHUB_RUN_ID:+gh$GITHUB_RUN_ID-}$(date -u +%Y%m%dT%H%M%SZ)"
note "run id $RUN_ID; record results: $EVAL_RECORD_RESULTS; require coverage evidence: $EVAL_REQUIRE_COVERAGE; require keyword-index parity evidence: $EVAL_REQUIRE_PARITY"

# ── 1. Fail fast and specifically, rather than 40 minutes into a run ─────────
note ""
note "## Preflight"
if ! docker inspect data-plane-v2-retrieval-engine-1 --format '{{.State.Health.Status}}' 2>/dev/null | grep -q healthy; then
  note "FAILED: retrieval-engine is not healthy — is the DP2 stack up on this host?"
  exit 1
fi
if ! docker inspect auth-service --format '{{.State.Health.Status}}' 2>/dev/null | grep -q healthy; then
  note "FAILED: Control Plane auth-service is not healthy on this host"
  exit 1
fi
if ! docker inspect data-plane-v2-data-quality-1 --format '{{.State.Health.Status}}' 2>/dev/null | grep -q healthy; then
  note "FAILED: data-quality is not healthy on this host: the coverage evidence, the release gates and the recorded results come from it"
  exit 1
fi
# No DP2 container carries this credential as an env var — it exists only in
# Control Plane's own generated-secrets file on the host, for operator
# scripts. The relative-path default assumes the runner is registered
# directly against the same monorepo checkout this stack runs from (the
# natural setup for a single-machine self-hosted runner). If the Actions
# runner instead checks out into its own separate `_work` tree, that guess is
# wrong for THIS job's checkout — override with CONTROL_PLANE_ENV_FILE, set
# once in the runner's own environment (e.g. its systemd service file), not
# as a workflow input.
CONTROL_ENV="${CONTROL_PLANE_ENV_FILE:-$REPO_ROOT/../Control Plane/.env.generated-secrets}"
if [ ! -f "$CONTROL_ENV" ]; then
  note "FAILED: $CONTROL_ENV not found."
  note "If this runner's checkout isn't the same tree as the running stack,"
  note "set CONTROL_PLANE_ENV_FILE in the runner's own environment to the"
  note "absolute path of Control Plane's .env.generated-secrets on this host."
  exit 1
fi
DATA_PLANE_CORPUS_SEEDER_API_KEY=$(grep '^DATA_PLANE_CORPUS_SEEDER_API_KEY=' "$CONTROL_ENV" | head -1 | cut -d= -f2-)
[ -n "$DATA_PLANE_CORPUS_SEEDER_API_KEY" ] || { note "FAILED: DATA_PLANE_CORPUS_SEEDER_API_KEY empty in $CONTROL_ENV"; exit 1; }
export DATA_PLANE_CORPUS_SEEDER_API_KEY
note "stack healthy, credential loaded (not logged)"

cd "$REPO_ROOT"

# The corpus this run measures is the frozen one: a snapshot that does not match its manifest is not a corpus.
run_step "The frozen corpus matches its manifest" "$WORK/corpus-verify.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-ingest-corpus.py verify

# ── 2. Stage the work dir: the golden sets as the organizations are, and the eval driver ──
#
# `run_eval_hybrid.py` is not a typo for eval-run-retrieval.py — every eval
# script (eval-attribution-study.sh, eval-lexical-cell.sh) mounts the work
# dir into a throwaway `python:3.12-slim` container and runs this exact
# filename from inside it. Caught by actually running this script end to end
# before calling it done, not by reading the callers: without this copy,
# eval-lexical-cell.sh fails fast and loud (`missing .../run_eval_hybrid.py`),
# but eval-attribution-study.sh has no such check — every one of its 14 cells
# fails silently inside the container and the script still prints
# "STUDY COMPLETE". `eval-gate.py` catches the resulting empty result set
# downstream (`raise SystemExit` on zero `st-rr-on-*.json` files), so nothing
# was ever falsely reported as passing — but it would have burned through
# every cell before saying so instead of failing at the actual cause.
cp scripts/eval-run-retrieval.py "$WORK/run_eval_hybrid.py"
# The golden sets are built NOW, for the organizations as they are (CR-T14): the judgments are keyed by document
# title and resolved to ids through the corpus key the ingest stored, so a re-seeded organization needs no committed
# file to be regenerated. It fails, and says what to do, when the corpus is not in the organizations.
run_step "Building the golden sets for the organizations as they are" "$WORK/golden.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-build-golden-set.py --out "$WORK"

# ── 3. Re-mine the lexical class fresh — cheap (SQL only), self-healing ─────
#
# Calls the underlying scripts directly rather than through `make` — `make`
# is not on PATH in the Git-for-Windows shell this runs under (confirmed:
# `command -v make` exits 1 on this exact host), and the Makefile targets
# these mirror (`eval-retrieval`, `eval-retrieval-lexical`, `eval-gate`) do
# nothing but call these same scripts with `$EVAL_SCRATCH` as the one
# argument. Calling them directly removes an implicit new CI-environment
# prerequisite rather than silently depending on one nobody asked for.
run_step "Mining lexical queries" "$WORK/mine.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-mine-lexical-queries.py "$WORK"
run_step "Seeding lexical judgments" "$WORK/seed.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-seed-lexical-judgments.py "$WORK" --apply

# ── 4. Run both query classes, natural-language and lexical ─────────────────
run_step "Retrieval — natural language" "$WORK/retrieval-nl.log" \
  sh scripts/eval-attribution-study.sh "$WORK"
run_step "Score attribution" "$WORK/score-attribution.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-score-attribution.py "$WORK"
run_step "Retrieval — lexical" "$WORK/retrieval-lex.log" \
  sh scripts/eval-lexical-cell.sh "$WORK"

# ── 4b. The evidence the gate judges the numbers by (CR-T14) ─────────────────
#
# Coverage: does the contextual organization have a verified context on 99 percent of its units, and is the baseline
# organization really raw? Parity: does the keyword index hold what Postgres holds? Both are fetched from the service,
# and a report that cannot be read is removed, never left stale (see eval-service.py).
#
# A fetch that fails must not stop the run before the gate: with the evidence required, the gate then says exactly
# which file is missing, and the records of the numbers that WERE measured are still written.
run_step "Evidence: context coverage" "$WORK/evidence-coverage.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-service.py coverage "$WORK" || \
  note "(the coverage evidence could not be fetched: the gate judges the run without it, or fails it, per EVAL_REQUIRE_COVERAGE)"
run_step "Evidence: release gates (keyword-index parity)" "$WORK/evidence-gates.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-service.py gates "$WORK" || \
  note "(the parity evidence could not be fetched: the gate judges the run without it, or fails it, per EVAL_REQUIRE_PARITY)"

# ── 5. Gate against the committed baseline. This is the pass/fail signal. ───
#
# The records are written whether the gate passes or not: a failing run is the one worth keeping. The gate reads
# EVAL_REQUIRE_COVERAGE and EVAL_REQUIRE_PARITY from the environment (exported above).
GATE_STATUS=0
run_step "Gate" "$WORK/gate.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-gate.py "$WORK" --records-out "$WORK/records" --run-id "$RUN_ID" || GATE_STATUS=$?
note ""
if [ "$GATE_STATUS" -eq 0 ]; then
  note "**GATE PASSED**"
else
  note "**GATE FAILED** — see the gate output above for which metric regressed and by how much."
fi

# ── 5b. Keep the result (CR-T14) ──────────────────────────────────────────────
RECORD_STATUS=0
if [ "$EVAL_RECORD_RESULTS" = 1 ]; then
  run_step "Recording the results in quality_eval_runs" "$WORK/record.log" \
    env PYTHONIOENCODING=utf-8 python3 scripts/eval-service.py record "$WORK/records" || RECORD_STATUS=$?
  if [ "$RECORD_STATUS" -ne 0 ]; then
    note "**RECORDING FAILED** — is the data-quality image with POST /v1/evals/results deployed? The gate verdict above stands."
  fi
else
  note ""
  note "(EVAL_RECORD_RESULTS=0: this run is not recorded in quality_eval_runs)"
fi

# ── 6. Refresh the in-service by-class scorecard too (informational) ────────
#
# Not gated: no committed baseline exists for the data-quality-go scorecard
# yet (only the offline scripts/eval-baseline.json is baselined). This step
# exists so `GET /v1/evals/retrieval/{id}` reflects tonight's numbers for
# anyone checking the service directly, and the by_class split is worth
# surfacing in the job summary — but the gate above is the only thing that
# fails this job. Failure here is deliberately swallowed (`|| true` inside
# run_step's own status handling is not used — this call's status is just
# never propagated to GATE_STATUS).
if [ -f "$REPO_ROOT/scripts/eval-refresh-inservice-scorecard.sh" ]; then
  run_step "In-service scorecard (informational, not gated)" "$WORK/scorecard.log" \
    sh "$REPO_ROOT/scripts/eval-refresh-inservice-scorecard.sh" || \
    note "(scorecard refresh failed — non-fatal, does not affect gate result)"
else
  note ""
  note "## In-service scorecard"
  note "(eval-refresh-inservice-scorecard.sh not present — skipped)"
fi

if [ "$GATE_STATUS" -ne 0 ]; then
  exit "$GATE_STATUS"
fi
exit "$RECORD_STATUS"
