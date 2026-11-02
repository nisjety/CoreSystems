#!/usr/bin/env bash
# verify-hitl.sh: the approve-then-execute (WS14 S2) and ask_user (WS14 S4) walks (build plan WS14 S7).
#
# Against a running proof stack it drives the two human-in-the-loop flows through the model gateway's
# HTTP surface and checks them in the session-core database. Every response is kept in a dated evidence
# folder under docs/compliance/evidence. The script changes no switch: the stack must already run with
# the switches named below, flipped together from the evidence the lead captures. It approves a real
# plan and sends real turns, so run it only against the proof stack.
#
# S2, a plan approval starts a new Do run. PLAN_RUN_ID owns the plan PLAN_ID, which a plan-mode turn of
# PLAN_THREAD_ID proposed (make the plan in the UI first; this walk starts no plan). The walk approves
# it, expects continuation execute_now, sends one execute turn that names the plan, expects exactly one
# run of the thread bound to the plan at the granted rung, then sends the same execute turn again and
# expects plan_grant_unavailable with still one bound run. Needs MODEL_GATEWAY_PLAN_EXECUTE_ENABLED,
# MODEL_GATEWAY_PLAN_EXECUTE_ON_APPROVE and MODEL_GATEWAY_DURABLE_PLAN_GRANT set to true on
# model-gateway, and AGENT_CONTINUE_AFTER_APPROVAL_ENABLED set to true on model-gateway and
# execution-core (the coupled WS03 flip).
#
# S4, ask_user. S4_THREAD_ID is an existing thread of the owner. The walk sends an ambiguous Do turn and
# waits for a user_question frame, lists the question (requested, next_turn), answers it once, retries
# the same answer (replayed), sends a different answer (409 question_already_answered), then sends a
# follow-up turn on the thread and reads the row back (answered, next_turn). It does not prove that the
# model used the answer; read the follow-up's request log for that. Needs AGENT_ASK_USER_ENABLED set to
# true on model-gateway and execution-core. A model that asks nothing gives SKIP, never PASS: rerun with
# a prompt that is genuinely ambiguous (S4_PROMPT).
#
#   MP_GATEWAY_URL                          model gateway base URL, no trailing slash
#   MP_USER_BEARER                          bearer of the owner of the runs (never printed or saved)
#   SESSION_CORE_SQL                        a command that reads SQL on stdin and prints unaligned rows,
#                                           for example: docker exec -i <session-core-postgres> psql -U <user> -d <db> -tAq
#   PLAN_RUN_ID, PLAN_ID, PLAN_THREAD_ID    S2: the plan-mode run, the plan it proposed and its thread
#   GRANTED_RUNG                            S2: read_only, workspace_write or danger_full_access (default workspace_write)
#   PLAN_JUSTIFICATION                      S2: the approval's reason, at least 12 characters (has a default)
#   S4_THREAD_ID                            S4: an existing thread of the owner
#   S4_PROMPT, S4_FOLLOWUP                  S4: the ambiguous turn and the follow-up (both have defaults)
#   TURN_TIMEOUT                            optional: seconds one streamed turn may take (default 300)
#   EVIDENCE_DIR                            optional: where the responses and result.txt go
#
# Exit status: 0 when no check failed, 1 when one did, 2 when the inputs are missing or refused. A check
# that cannot run says why and is SKIP, never PASS.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
EVIDENCE="${EVIDENCE_DIR:-$ROOT/docs/compliance/evidence/hitl-$STAMP}"
FAILED=0

if [[ -z "${MP_GATEWAY_URL:-}" || -z "${MP_USER_BEARER:-}" || -z "${SESSION_CORE_SQL:-}" ]]; then
  echo "usage: set MP_GATEWAY_URL, MP_USER_BEARER and SESSION_CORE_SQL, then PLAN_RUN_ID with PLAN_ID and PLAN_THREAD_ID (S2) and/or S4_THREAD_ID (S4); see the header" >&2
  exit 2
fi
if [[ -z "${PLAN_RUN_ID:-}" && -z "${S4_THREAD_ID:-}" ]]; then
  echo "usage: nothing to walk: set PLAN_RUN_ID (S2) and/or S4_THREAD_ID (S4)" >&2
  exit 2
fi
if [[ -n "${PLAN_RUN_ID:-}" && ( -z "${PLAN_ID:-}" || -z "${PLAN_THREAD_ID:-}" ) ]]; then
  echo "usage: S2 needs PLAN_ID and PLAN_THREAD_ID with PLAN_RUN_ID" >&2
  exit 2
fi
# Ids go into SQL text and URL paths, so only plain identifiers are accepted.
for id in "${PLAN_RUN_ID:-run}" "${PLAN_ID:-plan}" "${PLAN_THREAD_ID:-thread}" "${S4_THREAD_ID:-thread}"; do
  if [[ ! "$id" =~ ^[A-Za-z0-9_-]+$ ]]; then
    echo "refused: an id is not a plain identifier" >&2
    exit 2
  fi
done

GRANTED_RUNG="${GRANTED_RUNG:-workspace_write}"
PLAN_JUSTIFICATION="${PLAN_JUSTIFICATION:-Approved in the HITL proof: run the plan once under the granted rung.}"
S4_PROMPT="${S4_PROMPT:-Schedule the review meeting for the team.}"
S4_FOLLOWUP="${S4_FOLLOWUP:-Go ahead with the option I chose.}"
TURN_TIMEOUT="${TURN_TIMEOUT:-300}"

mkdir -p "$EVIDENCE"
RESULT="$EVIDENCE/result.txt"
: >"$RESULT"
say() { printf '%s\n' "$*" | tee -a "$RESULT"; }
pass() { say "PASS  $*"; }
fail() { say "FAIL  $*"; FAILED=$((FAILED + 1)); }
skip() { say "SKIP  $*"; }

# One query against session-core's database, rows on stdout. A failed query is reported and prints
# nothing, so the check that asked for it fails instead of the whole walk stopping.
db() {
  local -a cmd
  read -r -a cmd <<<"$SESSION_CORE_SQL"
  if ! printf '%s;\n' "$1" | "${cmd[@]}"; then
    echo "db: the query failed: $1" >&2
  fi
}

# One request. Prints the HTTP status and keeps the body as evidence/<name>.json. The bearer goes
# into the header only.
http() {
  local method="$1" path="$2" bearer="$3" body="$4" name="$5" status
  local -a args=(-sS -o "$EVIDENCE/$name.json" -w '%{http_code}' -X "$method" -H "Authorization: Bearer $bearer")
  if [[ -n "$body" ]]; then
    args+=(-H 'Content-Type: application/json' --data "$body")
  fi
  status="$(curl "${args[@]}" "$MP_GATEWAY_URL$path")" || status=000
  printf '%s' "$status"
}

# One streamed turn. The frames are kept as evidence/<name>.sse; the HTTP status is printed. A turn
# that stops at the timeout prints 000 and its partial frames stay in the evidence.
turn() {
  local name="$1" body="$2" status
  status="$(curl -sS -N --max-time "$TURN_TIMEOUT" -o "$EVIDENCE/$name.sse" -w '%{http_code}' \
    -X POST -H "Authorization: Bearer $MP_USER_BEARER" -H 'Accept: text/event-stream' \
    -H 'Content-Type: application/json' --data "$body" "$MP_GATEWAY_URL/v1/invoke/stream")" || status=000
  printf '%s' "$status"
}

# A dotted field of a JSON file, empty when absent or when the file is not JSON.
field() {
  python3 -c '
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
for key in sys.argv[2].split("."):
    value = value.get(key) if isinstance(value, dict) else None
print("" if value is None else value)
' "$1" "$2" 2>/dev/null || true
}

# The data of the first SSE frame named $2 in the stream file $1, empty when there is none. The
# frames are read as text so a stream cut short by the timeout still yields the frames it sent.
sse_frame() {
  python3 -c '
import sys
name = sys.argv[2]
text = open(sys.argv[1], encoding="utf-8", errors="replace").read().replace("\r\n", "\n")
for block in text.split("\n\n"):
    event, data = None, []
    for line in block.split("\n"):
        if line.startswith("event:"):
            event = line[len("event:"):].strip()
        elif line.startswith("data:"):
            data.append(line[len("data:"):].lstrip())
    if event == name and data:
        print("\n".join(data))
        break
' "$1" "$2" 2>/dev/null || true
}

# The request bodies of the walk, built by one program so every value reaches the JSON as data.
body() {
  python3 -c '
import json, sys
kind, values = sys.argv[1], sys.argv[2:]
if kind == "approve":
    rung, justification, plan = values
    out = {"decision": "approve", "granted_rung": rung, "justification": justification, "plan_id": plan}
elif kind == "execute":
    thread, plan = values
    out = {"content": "Run the approved plan.", "thread_id": thread, "features": ["agentic"],
           "plan_mode": False, "execute_granted": True, "approved_plan_id": plan}
elif kind == "ask":
    thread, prompt = values
    out = {"content": prompt, "thread_id": thread, "features": ["agentic"], "plan_mode": False}
elif kind == "skip":
    out = {"skip": True}
else:
    raise SystemExit("unknown body kind")
print(json.dumps(out))
' "$@"
}

# The body that answers a user_question frame: the first option of each item, or a short text when an
# item offers no options. Nothing is pasted into JSON by hand.
answer_body() {
  python3 -c '
import json, sys
frame = json.load(open(sys.argv[1], encoding="utf-8"))
answers = []
for item in frame.get("items") or []:
    options = item.get("options") or []
    answers.append({
        "item_id": item["id"],
        "option_ids": [options[0]["id"]] if options else [],
        "text": "" if options else "proof answer",
    })
print(json.dumps({"answers": answers}))
' "$1" 2>/dev/null || true
}

count_lines() { awk 'NF { n++ } END { print n + 0 }'; }

valid_id() { [[ "${1:-}" =~ ^[A-Za-z0-9_-]+$ ]]; }

has_error_frame() { [[ -n "$(sse_frame "$1" error)" ]]; }

# The runs of the plan other than its planning run, each with the rung it carries.
bound_runs() {
  db "select id || ' ' || coalesce(metadata->>'autonomy_rung', '') from runs where metadata->>'approved_plan_id' = '$PLAN_ID' and id <> '$PLAN_RUN_ID' order by id"
}

# S2: approve the plan, run it once under the granted rung, refuse the replay.
s2_walk() {
  local status continuation execute bound replay_status
  status="$(http POST "/v1/runs/$PLAN_RUN_ID/plan-approval" "$MP_USER_BEARER" \
    "$(body approve "$GRANTED_RUNG" "$PLAN_JUSTIFICATION" "$PLAN_ID")" s2-approval)"
  if [[ "$status" != 200 ]]; then
    fail "S2 the plan approval answers status $status"
    return
  fi
  continuation="$(field "$EVIDENCE/s2-approval.json" continuation)"
  if [[ "$continuation" != execute_now ]]; then
    fail "S2 the approval continues with '${continuation:-nothing}', not execute_now: the switches in the header are not all on"
    return
  fi
  pass "S2 the approval answers continuation execute_now"

  execute="$(body execute "$PLAN_THREAD_ID" "$PLAN_ID")"
  status="$(turn s2-execute "$execute")"
  if [[ "$status" == 200 ]] && ! has_error_frame "$EVIDENCE/s2-execute.sse"; then
    pass "S2 the execute turn runs"
  else
    fail "S2 the execute turn answers status $status without a run (see s2-execute.sse)"
    return
  fi

  bound="$(bound_runs)"
  if [[ "$(printf '%s' "$bound" | count_lines)" == 1 ]] \
    && [[ "$(printf '%s' "$bound" | awk 'NF { print $2; exit }')" == "$GRANTED_RUNG" ]]; then
    pass "S2 exactly one run of the thread is bound to the plan at the granted rung"
  else
    fail "S2 expected one bound run at $GRANTED_RUNG, found: $(printf '%s' "$bound" | tr '\n' ';')"
  fi

  replay_status="$(turn s2-replay "$execute")"
  if grep -q plan_grant_unavailable "$EVIDENCE/s2-replay.sse"; then
    pass "S2 the replayed execute turn is refused plan_grant_unavailable"
  else
    fail "S2 the replayed execute turn is not refused plan_grant_unavailable (status $replay_status)"
  fi
  bound="$(bound_runs)"
  if [[ "$(printf '%s' "$bound" | count_lines)" == 1 ]]; then
    pass "S2 the replay started no second run"
  else
    fail "S2 the replay left $(printf '%s' "$bound" | count_lines) bound runs, not one"
  fi
}

# S4: ask an ambiguous question, answer it once, refuse a different second answer, follow up.
s4_walk() {
  local status qid run_id listed body state
  status="$(turn s4-ask "$(body ask "$S4_THREAD_ID" "$S4_PROMPT")")"
  if [[ "$status" != 200 ]]; then
    fail "S4 the ask turn answers status $status (see s4-ask.sse)"
    return
  fi
  sse_frame "$EVIDENCE/s4-ask.sse" user_question > "$EVIDENCE/s4-user-question.json"
  if [[ ! -s "$EVIDENCE/s4-user-question.json" ]]; then
    skip "S4 the model put no question on this turn: rerun with an ambiguous S4_PROMPT"
    return
  fi
  qid="$(field "$EVIDENCE/s4-user-question.json" question_id)"
  if ! valid_id "$qid"; then
    fail "S4 the user_question frame names no plain question id"
    return
  fi
  run_id="$(db "select run_id from run_questions where id = '$qid'" | awk 'NF { print; exit }')"
  if ! valid_id "$run_id"; then
    fail "S4 the question is not in run_questions"
    return
  fi
  pass "S4 the Do turn put a question to the owner"

  status="$(http GET "/v1/orchestration/runs/$run_id/questions" "$MP_USER_BEARER" "" s4-list)"
  listed="$(python3 -c '
import json, sys
for question in json.load(open(sys.argv[1], encoding="utf-8")).get("questions", []):
    if question.get("id") == sys.argv[2]:
        print(question.get("state"), question.get("delivery"))
' "$EVIDENCE/s4-list.json" "$qid" 2>/dev/null || true)"
  if [[ "$status" == 200 && "$listed" == "requested next_turn" ]]; then
    pass "S4 the question is listed as requested, delivered next_turn"
  else
    fail "S4 the list answers status $status with '${listed:-no such question}'"
  fi

  body="$(answer_body "$EVIDENCE/s4-user-question.json")"
  if [[ -z "$body" || "$body" == '{"answers": []}' ]]; then
    fail "S4 the question offers no item to answer"
    return
  fi
  status="$(http POST "/v1/orchestration/questions/$qid/answer" "$MP_USER_BEARER" "$body" s4-answer)"
  if [[ "$status" == 200 && "$(field "$EVIDENCE/s4-answer.json" question.state)" == answered ]]; then
    pass "S4 the answer is recorded"
  else
    fail "S4 the answer answers status $status"
  fi

  status="$(http POST "/v1/orchestration/questions/$qid/answer" "$MP_USER_BEARER" "$body" s4-answer-replay)"
  if [[ "$status" == 200 ]]; then
    pass "S4 an identical retry of the answer replays"
  else
    fail "S4 an identical retry of the answer answers status $status"
  fi

  status="$(http POST "/v1/orchestration/questions/$qid/answer" "$MP_USER_BEARER" "$(body skip)" s4-answer-second)"
  if [[ "$status" == 409 && "$(field "$EVIDENCE/s4-answer-second.json" error.code)" == question_already_answered ]]; then
    pass "S4 a different second answer is refused question_already_answered"
  else
    fail "S4 a different second answer answers status $status"
  fi

  status="$(turn s4-followup "$(body ask "$S4_THREAD_ID" "$S4_FOLLOWUP")")"
  if [[ "$status" == 200 ]] && ! has_error_frame "$EVIDENCE/s4-followup.sse"; then
    pass "S4 the follow-up turn runs on the thread"
  else
    fail "S4 the follow-up turn answers status $status"
  fi

  state="$(db "select state || ' ' || delivery from run_questions where id = '$qid'" | awk 'NF { print; exit }')"
  if [[ "$state" == "answered next_turn" ]]; then
    pass "S4 the row reads answered, next_turn after the follow-up"
  else
    fail "S4 the row reads '${state:-nothing}', not 'answered next_turn'"
  fi
}

if [[ -n "${PLAN_RUN_ID:-}" ]]; then
  s2_walk
else
  skip "S2 PLAN_RUN_ID is not set: no plan to approve"
fi
if [[ -n "${S4_THREAD_ID:-}" ]]; then
  s4_walk
else
  skip "S4 S4_THREAD_ID is not set: no thread for the ask_user walk"
fi

say "result: $FAILED failed; evidence in $EVIDENCE"
exit $((FAILED > 0 ? 1 : 0))
