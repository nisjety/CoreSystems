#!/usr/bin/env bash
# verify-personalisation.sh: the EP-8 personalisation walk (scoped memory, standing profile, cached
# prefix, local date), written in EP-8 S5 and run by the lead (design section 6).
#
# Against a running proof stack it sends real turns through the model gateway's HTTP surface and reads
# session-core's database. Every response is kept in a dated evidence folder under docs/compliance/evidence.
# The script changes no switch: the stack must already run with the switches each step names. It writes
# three memories and sends real turns, so run it only against the proof stack, with nothing else writing
# memory while it runs (step A counts rows by time).
#
# A, a Space turn writes no personal memory. SPACE_THREAD_ID is a Space thread of the owner; the walk
# sends one turn stating a personal fact and expects no new scope user or org row in agent_memory.
# Needs EP-8 S2 in the build.
#
# B, the profile. The walk saves three memories on CHAT_THREAD_ID (a personal chat thread), rebuilds the
# profile with force (an edited profile of the owner is replaced) and expects each saved line in it. Needs SESSION_USER_PROFILE_ENABLED=true on session-core and
# the owner's memory on.
#
# C, the profile in the cached prefix. Two Ask turns on CHAT_THREAD_ID pinned to PROVIDER_HINT (an
# OpenAI-API route); the usage frame's cache_read_tokens is printed for both and must be above zero on
# the second. Needs MODEL_GATEWAY_USER_PROFILE_PREFIX=true and MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER=true
# on model-gateway. Whether the cached prefix reaches through the profile is read from the provider's
# request log (the cache view of WS02-S2); this walk proves the hit, not its length.
#
# D, the local date. One Ask turn carrying LOCAL_OFFSET_MINUTES (default 780, Auckland in summer) asks
# for today's date; the answer must name the local date. Needs MODEL_GATEWAY_USER_LOCALE_CONTEXT=true.
#
#   MP_GATEWAY_URL          model gateway base URL, no trailing slash
#   MP_USER_BEARER          bearer of the owner (never printed or saved)
#   SESSION_CORE_SQL        a command that reads SQL on stdin and prints unaligned rows,
#                           for example: docker exec -i <session-core-postgres> psql -U <user> -d <db> -tAq
#   CHAT_THREAD_ID          B, C, D: a personal chat thread of the owner
#   SPACE_THREAD_ID         A (optional): a Space thread of the owner; A is SKIP without it
#   PROVIDER_HINT, MODEL    C: the pinned route (defaults openai and gpt-4o-mini)
#   LOCAL_OFFSET_MINUTES    D: minutes east of UTC (default 780)
#   TURN_TIMEOUT            optional: seconds one streamed turn may take (default 300)
#   EVIDENCE_DIR            optional: where the responses and result.txt go
#
# Exit status: 0 when no check failed, 1 when one did, 2 when the inputs are missing or refused. A check
# that cannot run says why and is SKIP, never PASS.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
EVIDENCE="${EVIDENCE_DIR:-$ROOT/docs/compliance/evidence/personalisation-$STAMP}"
FAILED=0

if [[ -z "${MP_GATEWAY_URL:-}" || -z "${MP_USER_BEARER:-}" || -z "${SESSION_CORE_SQL:-}" || -z "${CHAT_THREAD_ID:-}" ]]; then
  echo "usage: set MP_GATEWAY_URL, MP_USER_BEARER, SESSION_CORE_SQL and CHAT_THREAD_ID (SPACE_THREAD_ID for A); see the header" >&2
  exit 2
fi
# Ids go into SQL text and request bodies, so only plain identifiers are accepted.
for id in "$CHAT_THREAD_ID" "${SPACE_THREAD_ID:-thread}"; do
  if [[ ! "$id" =~ ^[A-Za-z0-9_-]+$ ]]; then
    echo "refused: an id is not a plain identifier" >&2
    exit 2
  fi
done

PROVIDER_HINT="${PROVIDER_HINT:-openai}"
MODEL="${MODEL:-gpt-4o-mini}"
LOCAL_OFFSET_MINUTES="${LOCAL_OFFSET_MINUTES:-780}"
TURN_TIMEOUT="${TURN_TIMEOUT:-300}"
if [[ ! "$LOCAL_OFFSET_MINUTES" =~ ^-?[0-9]+$ ]] || (( LOCAL_OFFSET_MINUTES < -840 || LOCAL_OFFSET_MINUTES > 840 )); then
  echo "refused: LOCAL_OFFSET_MINUTES must be whole minutes in [-840, 840]" >&2
  exit 2
fi

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
  local method="$1" path="$2" body="$3" name="$4" status
  local -a args=(-sS -o "$EVIDENCE/$name.json" -w '%{http_code}' -X "$method" -H "Authorization: Bearer $MP_USER_BEARER")
  if [[ -n "$body" ]]; then
    args+=(-H 'Content-Type: application/json' --data "$body")
  fi
  status="$(curl "${args[@]}" "$MP_GATEWAY_URL$path")" || status=000
  printf '%s' "$status"
}

# One streamed turn. The frames are kept as evidence/<name>.sse; the HTTP status is printed.
turn() {
  local name="$1" body="$2" status
  status="$(curl -sS -N --max-time "$TURN_TIMEOUT" -o "$EVIDENCE/$name.sse" -w '%{http_code}' \
    -X POST -H "Authorization: Bearer $MP_USER_BEARER" -H 'Accept: text/event-stream' \
    -H 'Content-Type: application/json' --data "$body" "$MP_GATEWAY_URL/v1/invoke/stream")" || status=000
  printf '%s' "$status"
}

# What a stream file says: "answer" joins the chunk deltas, "cache_read" is the usage frame's
# cache_read_tokens (0 when absent). Read as text, so a stream cut short still yields what it sent.
from_stream() {
  python3 -c '
import json, sys
path, what = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8", errors="replace").read().replace("\r\n", "\n")
answer, cache_read = [], 0
for block in text.split("\n\n"):
    event, data = None, []
    for line in block.split("\n"):
        if line.startswith("event:"):
            event = line[len("event:"):].strip()
        elif line.startswith("data:"):
            data.append(line[len("data:"):].lstrip())
    try:
        payload = json.loads("\n".join(data)) if data else None
    except ValueError:
        continue
    if not isinstance(payload, dict):
        continue
    if event == "chunk":
        answer.append(payload.get("delta") or "")
    elif event == "usage":
        cache_read = int(payload.get("cache_read_tokens") or 0)
print("".join(answer) if what == "answer" else cache_read)
' "$1" "$2" 2>/dev/null || true
}

# The request bodies, built by one program so every value reaches the JSON as data.
body() {
  python3 -c '
import json, sys
kind, values = sys.argv[1], sys.argv[2:]
if kind == "turn":
    thread, content = values
    out = {"content": content, "thread_id": thread}
elif kind == "pinned":
    thread, content, hint, model = values
    out = {"content": content, "thread_id": thread, "provider_hint": hint, "model": model}
elif kind == "local":
    thread, content, offset = values
    out = {"content": content, "thread_id": thread, "client_utc_offset_minutes": int(offset)}
elif kind == "memory":
    thread, content = values
    out = {"thread_id": thread, "content": content}
elif kind == "rebuild":
    out = {"force": True}
else:
    raise SystemExit("unknown body kind")
print(json.dumps(out))
' "$@"
}

# A: a Space turn stating a personal fact leaves no personal or org row behind.
a_walk() {
  local since status rows
  if [[ -z "${SPACE_THREAD_ID:-}" ]]; then
    skip "A no SPACE_THREAD_ID: the Space containment walk did not run"
    return
  fi
  since="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  status="$(turn a-space "$(body turn "$SPACE_THREAD_ID" "For the record: my name is Proof Person and I prefer tea.")")"
  if [[ "$status" != 200 ]]; then
    fail "A the Space turn answers status $status (see a-space.sse)"
    return
  fi
  rows="$(db "select count(*) from agent_memory where scope in ('user', 'org') and created_at >= '$since'" | awk 'NF { print; exit }')"
  if [[ "$rows" == 0 ]]; then
    pass "A the Space turn wrote no personal or org memory"
  else
    fail "A the Space turn left ${rows:-an unknown number of} new user or org rows"
  fi
}

# B: three saved memories reach the rebuilt profile.
b_walk() {
  local status line missing=0
  local -a lines=(
    "I prefer tables when comparing options."
    "I work with tenders."
    "I prefer answers in Norwegian."
  )
  for i in "${!lines[@]}"; do
    status="$(http POST /v1/memories "$(body memory "$CHAT_THREAD_ID" "${lines[$i]}")" "b-save-$i")"
    if [[ "$status" != 200 && "$status" != 201 ]]; then
      fail "B saving memory $i answers status $status (see b-save-$i.json)"
      return
    fi
  done
  status="$(http POST /v1/memories/profile/rebuild "$(body rebuild)" b-rebuild)"
  if [[ "$status" != 200 ]]; then
    fail "B the rebuild answers status $status: is SESSION_USER_PROFILE_ENABLED on? (see b-rebuild.json)"
    return
  fi
  status="$(http GET /v1/memories/profile "" b-profile)"
  if [[ "$status" != 200 ]]; then
    fail "B reading the profile answers status $status"
    return
  fi
  for line in "${lines[@]}"; do
    if ! grep -qF "$line" "$EVIDENCE/b-profile.json"; then
      missing=$((missing + 1))
    fi
  done
  if [[ "$missing" == 0 ]]; then
    pass "B the profile holds the three saved memories"
  else
    fail "B $missing of the three saved memories are missing from the profile (see b-profile.json)"
  fi
}

# C: the second of two pinned Ask turns reads from the provider's cache.
c_walk() {
  local first second
  if [[ "$(turn c-first "$(body pinned "$CHAT_THREAD_ID" "Give me one sentence on why tenders need a timeline." "$PROVIDER_HINT" "$MODEL")")" != 200 ]]; then
    fail "C the first pinned turn did not stream (see c-first.sse)"
    return
  fi
  if [[ "$(turn c-second "$(body pinned "$CHAT_THREAD_ID" "And one sentence on why they need a budget." "$PROVIDER_HINT" "$MODEL")")" != 200 ]]; then
    fail "C the second pinned turn did not stream (see c-second.sse)"
    return
  fi
  first="$(from_stream "$EVIDENCE/c-first.sse" cache_read)"
  second="$(from_stream "$EVIDENCE/c-second.sse" cache_read)"
  say "INFO  C cache_read_tokens: first ${first:-0}, second ${second:-0}"
  if [[ "${second:-0}" =~ ^[0-9]+$ ]] && (( ${second:-0} > 0 )); then
    pass "C the second turn read ${second} input tokens from the cache"
  else
    fail "C the second turn read nothing from the cache: check the route and MODEL_GATEWAY_CACHE_STABLE_PROMPT_ORDER"
  fi
}

# D: a turn carrying an offset is told its local date.
d_walk() {
  local expected answer
  expected="$(python3 -c '
import datetime, sys
now = datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(minutes=int(sys.argv[1]))
print(now.strftime("%Y-%m-%d"))
' "$LOCAL_OFFSET_MINUTES")"
  if [[ "$(turn d-local "$(body local "$CHAT_THREAD_ID" "What is today's date where I am? Answer with the date only, as YYYY-MM-DD." "$LOCAL_OFFSET_MINUTES")")" != 200 ]]; then
    fail "D the local-date turn did not stream (see d-local.sse)"
    return
  fi
  answer="$(from_stream "$EVIDENCE/d-local.sse" answer)"
  if grep -qF "$expected" <<<"$answer"; then
    pass "D the answer names the local date $expected"
  else
    fail "D the answer does not name the local date $expected (see d-local.sse)"
  fi
}

a_walk
b_walk
c_walk
d_walk

say "evidence: $EVIDENCE"
if (( FAILED > 0 )); then
  exit 1
fi
exit 0
