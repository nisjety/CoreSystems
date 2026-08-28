#!/bin/sh
# Run the full retrieval-quality eval unattended and gate on the result.
#
# ## Why this exists, and why it runs where it runs
#
# The eval needs three things a GitHub-hosted runner cannot supply: an
# ALREADY-INGESTED corpus (95 documents / 1,164 chunks, contextualized and
# embedded — no ingestion script exists anywhere in this repo to rebuild that
# from a clean checkout, and one inference call per chunk makes "just re-ingest
# in CI" an expensive thing to invent casually), Control Plane's `auth-service`
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
# The 87 hand-authored natural-language judgments (scripts/golden-v2*.json) are
# committed — they cannot be regenerated (a human wrote them; the golden set's
# own docs say model-generated questions bias lexical measurements), so this
# script would have nothing to score against on a fresh checkout without them.
# The 23 mined lexical judgments are NOT committed on purpose — mining is pure
# SQL over the live corpus and self-heals if corpus content ever changes, so
# this script re-mines them every run rather than trusting a stale snapshot.
#
# What this script does NOT solve, and is not pretending to: rebuilding the
# corpus itself from nothing. If the stack this runs against is ever torn down
# and rebuilt from a clean checkout, this job will fail loudly at the
# stack-health check below — which is the correct behavior (a clear failure,
# not a silent false pass) until that ingestion pipeline actually gets built.
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

# ── 2. Stage the work dir: the durable golden set, and the eval driver ──────
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
cp scripts/golden-v2.json scripts/golden-v2-ctx.json "$WORK/"

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

# ── 5. Gate against the committed baseline. This is the pass/fail signal. ───
GATE_STATUS=0
run_step "Gate" "$WORK/gate.log" \
  env PYTHONIOENCODING=utf-8 python3 scripts/eval-gate.py "$WORK" || GATE_STATUS=$?
note ""
if [ "$GATE_STATUS" -eq 0 ]; then
  note "**GATE PASSED**"
else
  note "**GATE FAILED** — see the gate output above for which metric regressed and by how much."
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

exit "$GATE_STATUS"
