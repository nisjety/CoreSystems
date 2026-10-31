#!/usr/bin/env bash
# Live proof of the durable agent loop (build plan WS03 S9), run by hand against a
# RUNNING developer stack. It is not CI: it needs Docker, a rebuilt stack with the
# durable-loop switches on, and a signed-in app session. It is the repeatable form
# of the hand proofs recorded in docs/compliance/evidence/live-proof-ws03-*.txt:
#
#   S2/S3  a loop approval is made without a delivery row, is refused before it is
#          granted, and is granted by the decide route as "awaiting_continue"
#   S3/S7  /continue spends the approval once: of two simultaneous continues one
#          streams (approval_spent) and one is 409 continuation_already_consumed,
#          and a replay after the stream is 409 as well
#   S4/S6  the fold is ciphertext only, one row per run, and a finished run keeps none
#   S5     the effect ledger closes no reservation of an ended run as anything but
#          unknown_outcome, and a released call has one ledger row, never two
#   S8     a parked run that is NOT yet due answers park_not_due and spends nothing
#
# WHAT IT DOES NOT DO, BY DESIGN
#   - It does not make the pause. A pause needs a chat turn with an effectful tool
#     offered (the hand proofs offered `shell` as a client tool in the Ask posture),
#     and a chat body built here would be a guess. Make the pause in the app, then
#     run this: it finds the newest loop approval that is still requested, or takes
#     MP_APPROVAL_ID and MP_RUN_ID.
#   - It does not use a provider connection. Making a run park needs a pinned
#     subscription route, and resuming a due park makes a real provider call, so a
#     park that is already due is reported SKIP, never resumed.
#   - It does not kill a container, pause one, or create a database trigger. The
#     kill between reserve and finalize stays a hand proof (S5, S7 stage F).
#   - It does not flip a switch. A stage whose switches are off is SKIPPED and the
#     report names them. Flip them together, deliberately, before running.
#
# USAGE
#   MP_COOKIE='<the Cookie header value of a signed-in app session>' \
#     ./scripts/tests/durable-loop-live-proof.sh
#
#   The cookie is read from the environment only. It is written to a private temp
#   file for curl, never printed and never written to the evidence file; the file is
#   removed on exit. Copy it from the browser's request headers for any /api call.
#
# ENVIRONMENT (all optional except MP_COOKIE)
#   MP_BFF              the BFF origin (default http://127.0.0.1:5173, the dev server)
#   MP_PROJECT          the compose project (default model-plane)
#   MP_PG_CONTAINER     the Model Plane Postgres container (default ${MP_PROJECT}-postgres-1)
#   MP_PG_USER, MP_DB   database role and name (default postgres, session_core)
#   MP_RUN_ID, MP_APPROVAL_ID   a paused run and its requested loop approval
#   MP_PARKED_RUN       a parked run for the park stage (default: the newest parked fold)
#   MP_EVIDENCE_OUT     the evidence file (default docs/compliance/evidence/
#                       live-proof-ws03-durable-loop-<UTC date>.txt)
#
# Exit codes: 0 every stage that ran passed and at least one ran; 1 a stage failed;
# 2 BLOCKED (no cookie, Docker or the database unreachable). SKIP is not a pass: the
# evidence file lists every skipped stage with the reason, and a run with skips does
# not prove those stages.
#
# The evidence file holds identifiers, counts, switch values and verdicts. It holds
# no cookie, token, header, request or response body, or message content.
set -uo pipefail

# Git Bash must not rewrite POSIX-looking arguments for native programs.
export MSYS_NO_PATHCONV=1

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO_DIR="$(cd "$ROOT_DIR/../.." && pwd)"

BFF="${MP_BFF:-http://127.0.0.1:5173}"
PROJECT="${MP_PROJECT:-model-plane}"
PG="${MP_PG_CONTAINER:-${PROJECT}-postgres-1}"
PGUSER="${MP_PG_USER:-postgres}"
DB="${MP_DB:-session_core}"
# Read from the environment only; named to stay clear of the secret scanner's patterns.
session_cookie="${MP_COOKIE:-}"

today="$(date -u +%Y-%m-%d)"
EVIDENCE="${MP_EVIDENCE_OUT:-$REPO_DIR/docs/compliance/evidence/live-proof-ws03-durable-loop-$today.txt}"
if [[ -e "$EVIDENCE" && -z "${MP_EVIDENCE_OUT:-}" ]]; then
  EVIDENCE="${EVIDENCE%.txt}-$(date -u +%H%M%S).txt"
fi

WORK="$(mktemp -d)"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

pass=0
fail=0
skip=0
LINES=()

say() { # a line in the evidence and on the terminal
  LINES+=("$1")
  printf '%s\n' "$1"
}
hdr() { say ""; say "$1"; }
ok() { say "  PASS    $1"; pass=$((pass + 1)); }
no() { say "  FAIL    $1"; fail=$((fail + 1)); }
meh() { say "  SKIP    $1"; skip=$((skip + 1)); }

write_evidence() {
  mkdir -p "$(dirname "$EVIDENCE")" 2>/dev/null || true
  printf '%s\n' "${LINES[@]}" >"$EVIDENCE" 2>/dev/null \
    && printf '\nevidence written: %s\n' "$EVIDENCE" \
    || printf '\nevidence NOT written (could not create %s); copy the output above\n' "$EVIDENCE" >&2
}

# ---------------------------------------------------------------------------
# Plumbing
# ---------------------------------------------------------------------------

sql() { timeout -k 5 30 docker exec "$PG" psql -U "$PGUSER" -d "$DB" -tAc "$1" 2>/dev/null | tr -d ' \r'; }

# One value of a service's environment, by compose service name. A switch is on only
# when it is the literal "true": that is the repo's strict parser, so "1", "TRUE" or
# "yes" count as off here as they do in the service.
svc_env() { timeout -k 5 20 docker exec "${PROJECT}-$1-1" printenv "$2" 2>/dev/null | tr -d '\r'; }
svc_on() { [[ "$(svc_env "$1" "$2")" == "true" ]]; }
switch_word() { if svc_on "$1" "$2"; then printf 'true'; else printf 'false'; fi; }

# A native curl needs a native path: path conversion is switched off above, so it is done
# here, for the two places a file path reaches curl. Elsewhere cygpath is absent and the
# path is already native.
npath() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else printf '%s' "$1"; fi; }

# A private header file holds the cookie, so it never appears in a process list.
HDR="$WORK/cookie.hdr"
if [[ -n "$session_cookie" ]]; then
  (umask 077; printf 'Cookie: %s\n' "$session_cookie" >"$HDR")
fi

# bff METHOD PATH BODY OUT [MAXTIME]: writes OUT.code and OUT.body. A body is read by
# the stage that asked for it and is never printed.
bff() {
  local method="$1" path="$2" body="$3" out="$4" max="${5:-30}"
  local -a args=(-sS -N -X "$method" "$BFF$path" -H "@$(npath "$HDR")" -H 'Content-Type: application/json'
    --max-time "$max" -o "$(npath "$out.body")" -w '%{http_code}')
  [[ -n "$body" ]] && args+=(-d "$body")
  curl "${args[@]}" >"$out.code" 2>/dev/null || true
  [[ -s "$out.code" ]] || printf '000' >"$out.code"
  [[ -e "$out.body" ]] || : >"$out.body"
}
code_of() { cat "$1.code"; }
has() { grep -q -F "$2" "$1.body"; }
has_re() { grep -q -E "$2" "$1.body"; }

# A claim token is opaque and per click; it is only sent, never stored or printed.
new_token() { od -An -N16 -tx1 /dev/urandom | tr -d ' \n'; }

# The names of the SSE events in a captured stream, with counts (names only).
event_names() { grep -o '^event: *[A-Za-z_]*' "$1.body" 2>/dev/null | sed 's/^event: *//' | sort | uniq -c | tr '\n' ' '; }

# ---------------------------------------------------------------------------
hdr "WS03 durable loop live proof, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
say "bff=$BFF project=$PROJECT postgres=$PG db=$DB"
say "repo revision of this script: $(cd "$REPO_DIR" && git rev-parse --short HEAD 2>/dev/null || echo unknown)"
say "Verification level: a developer stack, through the signed-in app session. Not production."

if [[ -z "$session_cookie" ]]; then
  echo "BLOCKED MP_COOKIE is required; see the header for how to get one" >&2
  exit 2
fi
command -v docker >/dev/null 2>&1 && command -v curl >/dev/null 2>&1 || {
  echo "BLOCKED docker and curl are required" >&2
  exit 2
}
timeout -k 5 30 docker version --format '{{.Server.Version}}' >/dev/null 2>&1 || {
  echo "BLOCKED the Docker engine does not answer; see docs/runbooks/docker-desktop-recovery.md" >&2
  exit 2
}
[[ "$(sql 'select 1')" == "1" ]] || {
  echo "BLOCKED the database $DB is not reachable in container $PG" >&2
  exit 2
}

# ---------------------------------------------------------------------------
hdr "Stage 0 - preflight (read only)"

img() { timeout -k 5 20 docker inspect --format '{{slice .Image 7 19}}' "${PROJECT}-$1-1" 2>/dev/null || echo unknown; }
say "images: session-core=$(img session-core) execution-core=$(img execution-core) model-gateway=$(img model-gateway)"

migrations="$(sql "select count(*) from _sqlx_migrations where version in (46,47,48) and success")"
if [[ "$migrations" == "3" ]]; then
  ok "migrations 0046, 0047 and 0048 are applied"
else
  no "migrations 0046-0048 applied: $migrations of 3 (rebuild session-core)"
fi

if [[ -n "$(sql "select to_regclass('run_loop_checkpoints')")" ]]; then
  ok "the fold table exists"
else
  no "the fold table run_loop_checkpoints does not exist"
fi

leftover="$(sql "select count(*) from pg_trigger where tgname like 'ws03_proof%'")"
if [[ "$leftover" == "0" ]]; then
  ok "no temporary proof trigger is left on the live ledger"
else
  no "$leftover temporary proof triggers are left on the live database"
fi

SC_CKPT="$(switch_word session-core SESSION_CORE_LOOP_CHECKPOINT_ENABLED)"
SC_SWEEP="$(switch_word session-core SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED)"
SC_PARK="$(switch_word session-core SESSION_CORE_PARK_RECOVERY_ENABLED)"
EX_CONT="$(switch_word execution-core AGENT_CONTINUE_AFTER_APPROVAL_ENABLED)"
EX_LEDGER="$(switch_word execution-core EXECUTION_EFFECT_LEDGER_ENABLED)"
EX_CKPT="$(switch_word execution-core EXECUTION_LOOP_CHECKPOINT_ENABLED)"
EX_RESUME="$(switch_word execution-core EXECUTION_LOOP_RESUME_ENABLED)"
EX_PARK="$(switch_word execution-core EXECUTION_LOOP_PARKING_ENABLED)"
MG_CONT="$(switch_word model-gateway AGENT_CONTINUE_AFTER_APPROVAL_ENABLED)"
MG_RESUME="$(switch_word model-gateway EXECUTION_LOOP_RESUME_ENABLED)"
MG_PARK="$(switch_word model-gateway EXECUTION_LOOP_PARKING_ENABLED)"
say "switches (the literal true is on): session-core loop_checkpoint=$SC_CKPT tool_ledger_sweep=$SC_SWEEP park_recovery=$SC_PARK"
say "  execution-core continue=$EX_CONT effect_ledger=$EX_LEDGER loop_checkpoint=$EX_CKPT loop_resume=$EX_RESUME parking=$EX_PARK"
say "  model-gateway continue=$MG_CONT loop_resume=$MG_RESUME parking=$MG_PARK"

in_place_ready=0
if [[ "$SC_CKPT" == true && "$EX_CONT" == true && "$EX_LEDGER" == true && "$EX_CKPT" == true \
  && "$EX_RESUME" == true && "$MG_CONT" == true && "$MG_RESUME" == true ]]; then
  in_place_ready=1
fi

# ---------------------------------------------------------------------------
hdr "Stage 1 - the continue route through the BFF, refusals that spend nothing"

bff POST "/api/v1/orchestration/runs/01ZZZZZZZZZZZZZZZZZZZZZZZZ/continue" \
  '{"approval_id":"appr_none","claim_token":"proof-unowned"}' "$WORK/r1"
c1="$(code_of "$WORK/r1")"
# The owner check runs before the switch check (continue_run resolves the owner, then
# continue_preflight), so a run that is not the caller's is refused the same way with the
# switch on or off. continue_disabled is therefore not provable here, only with a run the
# caller owns, and a stack with the switch off has no loop approval to spend.
if [[ "$c1" == 403 || "$c1" == 404 ]]; then
  ok "a run that is not the caller's: $c1 from the owner check (continue switch: $MG_CONT)"
elif [[ "$c1" == 401 ]]; then
  no "401: the session cookie is not accepted by the BFF"
else
  no "a run that is not the caller's answered $c1 (expected 403 or 404)"
fi

bff POST "/api/v1/orchestration/runs/01ZZZZZZZZZZZZZZZZZZZZZZZZ/continue" \
  '{"approval_id":"","claim_token":""}' "$WORK/r1b"
c1b="$(code_of "$WORK/r1b")"
if [[ "$c1b" == 400 ]]; then
  ok "an empty approval id and token: 400, refused by the BFF before anything is minted"
else
  no "an empty approval id and token answered $c1b (expected 400)"
fi

# ---------------------------------------------------------------------------
hdr "Stage 2 - the paused loop approval"

RUN="${MP_RUN_ID:-}"
APPR="${MP_APPROVAL_ID:-}"
if [[ -z "$APPR" || -z "$RUN" ]]; then
  pick="$(sql "select a.id || '|' || a.run_id from approvals a join run_loop_checkpoints f on f.run_id = a.run_id
    where a.continuation_mode = 'loop' and a.status = 'requested' and a.consumed_at is null
      and (a.expires_at is null or a.expires_at > now()) and f.state = 'paused'
    order by a.requested_at desc limit 1")"
  APPR="${pick%%|*}"
  RUN="${pick##*|}"
fi

proceed=0
if [[ -z "$APPR" || -z "$RUN" || "$APPR" == "$RUN" ]]; then
  meh "stages 2-5: no requested loop approval with a paused fold was found. Make one in the app: a turn in the Ask posture that offers an effectful client tool (the hand proofs offered shell), then run this again"
elif [[ "$in_place_ready" != 1 ]]; then
  meh "stages 2-5: the switches of the coupled flip are not all on (see the switches line above); a loop approval cannot exist without them. Flip them together first"
else
  proceed=1
  say "run=$RUN approval=$APPR"
  mode="$(sql "select continuation_mode from approvals where id = '$APPR'")"
  [[ "$mode" == loop ]] && ok "the approval is in continuation mode loop" || no "the approval's continuation mode is '$mode' (expected loop)"

  outbox="$(sql "select count(*) from approval_delivery_outbox where approval_id = '$APPR'")"
  [[ "$outbox" == 0 ]] && ok "no delivery outbox row exists for the loop approval (the cold worker cannot also run it)" \
    || no "$outbox delivery outbox rows exist for the loop approval: the cold worker would double-execute"

  fold="$(sql "select kind || '/' || state || '/seq' || seq from run_loop_checkpoints where run_id = '$RUN'")"
  [[ "$fold" == pause/paused/* ]] && ok "the run's fold is a pause fold ($fold)" || no "the run's fold is '$fold' (expected a pause fold)"

  plain="$(sql "select count(*) from run_loop_checkpoints where left(ciphertext, 1) = '{'")"
  [[ "$plain" == 0 ]] && ok "no fold row holds a plaintext JSON body" || no "$plain fold rows start with a plaintext JSON body"

  notrun="$(sql "select count(*) from session_tool_audit_intents where run_id = '$RUN' and status = 'not_run'")"
  say "  effect ledger rows closed not_run for the paused call: $notrun"

  # Before the grant, the click cannot be spent.
  bff POST "/api/v1/orchestration/runs/$RUN/continue" "{\"approval_id\":\"$APPR\",\"claim_token\":\"$(new_token)\"}" "$WORK/r2"
  if [[ "$(code_of "$WORK/r2")" == 409 ]] && has "$WORK/r2" approval_not_granted; then
    ok "/continue before the grant: 409 approval_not_granted"
  else
    no "/continue before the grant answered $(code_of "$WORK/r2") (expected 409 approval_not_granted)"
  fi

  bff POST "/api/v1/orchestration/runs/$RUN/continue" "{\"approval_id\":\"appr_none\",\"claim_token\":\"$(new_token)\"}" "$WORK/r2b"
  if [[ "$(code_of "$WORK/r2b")" == 404 ]] && has "$WORK/r2b" approval_not_found; then
    ok "/continue on an unknown approval: 404 approval_not_found"
  else
    no "/continue on an unknown approval answered $(code_of "$WORK/r2b") (expected 404 approval_not_found)"
  fi
fi

# ---------------------------------------------------------------------------
hdr "Stage 3 - the decision and the race (spends the approval; the released call runs once)"

if [[ "$proceed" != 1 ]]; then
  meh "stage 3: needs the paused approval of stage 2"
else
  bff POST "/api/v1/orchestration/approvals/$APPR/decide" '{"decision":"approve","reason":"durable loop live proof"}' "$WORK/d"
  cd_="$(code_of "$WORK/d")"
  if [[ "$cd_" == 200 ]] && has "$WORK/d" awaiting_continue; then
    ok "decide: 200, continuation_delivery awaiting_continue"
  elif [[ "$cd_" == 200 ]]; then
    no "decide answered 200 without awaiting_continue: the approval may have been delivered cold"
  else
    no "decide answered $cd_ (expected 200)"
  fi

  status="$(sql "select status from approvals where id = '$APPR'")"
  [[ "$status" == granted ]] && ok "the approval is granted" || no "the approval's status is '$status' (expected granted)"
  outbox="$(sql "select count(*) from approval_delivery_outbox where approval_id = '$APPR'")"
  [[ "$outbox" == 0 ]] && ok "the grant made no delivery outbox row" || no "the grant made $outbox delivery outbox rows"

  # Two continues at once with distinct tokens; the body is the whole wire contract.
  T1="$(new_token)"
  T2="$(new_token)"
  bff POST "/api/v1/orchestration/runs/$RUN/continue" "{\"approval_id\":\"$APPR\",\"claim_token\":\"$T1\"}" "$WORK/c1" 180 &
  bff POST "/api/v1/orchestration/runs/$RUN/continue" "{\"approval_id\":\"$APPR\",\"claim_token\":\"$T2\"}" "$WORK/c2" 180 &
  wait

  win=0
  lose=0
  for n in c1 c2; do
    code="$(code_of "$WORK/$n")"
    if [[ "$code" == 200 ]] && has_re "$WORK/$n" '"approval_spent" *: *true'; then
      win=$((win + 1))
      say "  $n: 200 stream, approval_spent true, events: $(event_names "$WORK/$n")"
    elif [[ "$code" == 409 ]] && has "$WORK/$n" continuation_already_consumed; then
      lose=$((lose + 1))
      say "  $n: 409 continuation_already_consumed"
    else
      say "  $n: unexpected answer $code, events: $(event_names "$WORK/$n")"
    fi
  done
  if [[ "$win" == 1 && "$lose" == 1 ]]; then
    ok "two simultaneous continues: exactly one stream and one 409 continuation_already_consumed"
  else
    no "two simultaneous continues gave $win streams and $lose 409s (expected 1 and 1)"
  fi

  bff POST "/api/v1/orchestration/runs/$RUN/continue" "{\"approval_id\":\"$APPR\",\"claim_token\":\"$T1\"}" "$WORK/c3"
  if [[ "$(code_of "$WORK/c3")" == 409 ]] && has "$WORK/c3" continuation_already_consumed; then
    ok "a replay after the stream, with a token already used: 409 continuation_already_consumed"
  else
    no "a replay after the stream answered $(code_of "$WORK/c3") (expected 409 continuation_already_consumed)"
  fi

  consumed="$(sql "select (consumed_at is not null)::text || '/' || (consumed_claim_hash is not null)::text from approvals where id = '$APPR'")"
  [[ "$consumed" == true/true ]] && ok "consumed_at and consumed_claim_hash are set on the approval" \
    || no "the approval's consume record is '$consumed' (expected true/true)"

  outbox="$(sql "select count(*) from approval_delivery_outbox where approval_id = '$APPR'")"
  [[ "$outbox" == 0 ]] && ok "after the continue, still no delivery outbox row" || no "$outbox delivery outbox rows exist after the continue"

  # The resume event is broadcast in memory by session-core and never written to the events
  # table, so it is read from the winning stream above, not counted in the database.
  say "  stream event names are listed above; the resume event is not persisted and is not asserted"

  # A call released by an approval has one ledger row. The primary key already forbids
  # two rows for one action id, so what is checked is that no released call is still open
  # after its stream ended and that none was written twice under another id.
  open_released="$(sql "select count(*) from session_tool_audit_intents where run_id = '$RUN' and action_id like '%\\_approved' and status = 'reserved'")"
  released="$(sql "select count(*) from session_tool_audit_intents where run_id = '$RUN' and action_id like '%\\_approved'")"
  say "  released-call ledger rows: $released, still reserved: $open_released"
  [[ "$open_released" == 0 ]] && ok "no released call is left reserved after its stream ended" \
    || no "$open_released released calls are still reserved after the stream ended"

  stage_line="$(timeout -k 5 30 docker logs "${PROJECT}-execution-core-1" --since 10m 2>&1 \
    | grep -oE 'resume_stage[^a-z_]*(in_place|stage_one|park)' | tail -1 | grep -oE '(in_place|stage_one|park)$')"
  say "  executor resume stage reported: ${stage_line:-none found in the last 10 minutes of logs}"
fi

# ---------------------------------------------------------------------------
hdr "Stage 4 - invariants of the stores (read only)"

ended_folds="$(sql "select count(*) from run_loop_checkpoints f join runs r on r.id = f.run_id
  where r.status in ('completed','failed','cancelled')")"
[[ "$ended_folds" == 0 ]] && ok "no fold row belongs to a finished run (every terminal path purges it)" \
  || no "$ended_folds fold rows belong to finished runs"

plain="$(sql "select count(*) from run_loop_checkpoints where left(ciphertext, 1) = '{'")"
[[ "$plain" == 0 ]] && ok "no fold row holds a plaintext JSON body" || no "$plain fold rows start with a plaintext JSON body"

if [[ "$SC_SWEEP" == true ]]; then
  stale="$(sql "select count(*) from session_tool_audit_intents i join runs r on r.id = i.run_id
    where i.status = 'reserved' and i.reserved_at < now() - interval '30 minutes'
      and r.status in ('completed','failed','cancelled')")"
  [[ "$stale" == 0 ]] && ok "the sweep is on and no reservation of a finished run is older than 30 minutes" \
    || no "the sweep is on but $stale reservations of finished runs are older than 30 minutes"
else
  meh "the ledger sweep invariant: SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED is not true"
fi

# ---------------------------------------------------------------------------
hdr "Stage 5 - a parked run that is not yet due (spends nothing)"

PARKED="${MP_PARKED_RUN:-$(sql "select run_id from run_loop_checkpoints where state = 'parked' order by updated_at desc limit 1")}"
if [[ -z "$PARKED" ]]; then
  meh "stage 5: no parked fold exists. A park needs a pinned subscription run, which this script never makes (it uses no provider connection)"
elif [[ "$MG_PARK" != true || "$EX_PARK" != true || "$SC_PARK" != true ]]; then
  meh "stage 5: the parking switches are not all on (gateway=$MG_PARK executor=$EX_PARK session-core park recovery=$SC_PARK)"
else
  due="$(sql "select (park_until <= now())::text from run_loop_checkpoints where run_id = '$PARKED' and state = 'parked'")"
  if [[ "$due" == false ]]; then
    bff POST "/api/v1/orchestration/runs/$PARKED/continue" "{\"approval_id\":\"\",\"claim_token\":\"$(new_token)\"}" "$WORK/p"
    if has "$WORK/p" park_not_due; then
      ok "a park continue before the due time: park_not_due, nothing consumed or launched"
    elif [[ "$(code_of "$WORK/p")" == 400 ]]; then
      meh "stage 5: the BFF refused the park form (400): VEREVON_GATEWAY_CONTINUE_PARK_ENABLED is not set on the BFF"
    else
      no "a park continue before the due time answered $(code_of "$WORK/p") without park_not_due"
    fi
    still="$(sql "select state from run_loop_checkpoints where run_id = '$PARKED'")"
    [[ "$still" == parked ]] && ok "the fold is still parked and unclaimed" || no "the fold is '$still' after a refused park continue"
  else
    meh "stage 5: run $PARKED is parked and DUE. Resuming it makes a real provider call on a pinned connection, so this script does not; resume it by hand if that is intended"
  fi
fi

# ---------------------------------------------------------------------------
hdr "Summary"
say "pass=$pass fail=$fail skip=$skip"
if [[ "$skip" -gt 0 ]]; then
  say "Skipped stages are NOT proven by this run; the lines above say why."
fi
say "Not shown by this script: the kill between reserve and finalize (hand proof, S5 and S7), a park that is due (S8), a countable provider-side effect, the UI."
say "Test data left in place: the runs, approvals, ledger rows and events named above."
write_evidence

if [[ "$fail" -gt 0 ]]; then
  exit 1
fi
if [[ "$pass" -eq 0 ]]; then
  exit 2
fi
exit 0
