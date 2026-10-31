#!/usr/bin/env bash
# Operator proof for tail-first paging of a thread's transcript (build plan WS09 S8).
#
# Seeds one synthetic thread of 400 turns straight into session-core's database,
# then reads it through the BFF messages route in pages of 100, from the newest
# turns back to the oldest. It checks that every turn comes back exactly once in
# conversation order, that has_more is true on every page but the oldest, that an
# unpaged read is unchanged (every turn, no has_more key), that malformed and
# foreign cursors are refused with 400, and it prints the plan of the tail query.
#
# Run by the lead against a rebuilt stack (session-core, model-gateway and the
# BFF), never in CI. The database must be disposable: the script inserts rows and
# does not clean them up.
#
#   PROOF_DATABASE_URL  session-core Postgres (disposable)
#   PROOF_BFF_URL       BFF base URL, for example http://localhost:8080
#   PROOF_COOKIE_JAR    curl cookie jar that holds a session for the seeded user
#   PROOF_ORG_ID        org that owns the seeded threads
#   PROOF_USER_ID       user that owns the seeded threads
#
# Nothing here prints a credential; the cookie jar is only read by curl.
set -euo pipefail

: "${PROOF_DATABASE_URL:?set PROOF_DATABASE_URL to a disposable session-core database}"
: "${PROOF_BFF_URL:?set PROOF_BFF_URL to the BFF base URL}"
: "${PROOF_COOKIE_JAR:?set PROOF_COOKIE_JAR to a curl cookie jar for the seeded user}"
: "${PROOF_ORG_ID:?set PROOF_ORG_ID}"
: "${PROOF_USER_ID:?set PROOF_USER_ID}"

PROOF_RUN="$(date -u +%Y%m%dt%H%M%Sz)"
PROOF_PREFIX="ws09s8-${PROOF_RUN}"
TURN_COUNT=400
PAGE_SIZE=100
PROBE_THREAD="${PROOF_PREFIX}-thread"
FOREIGN_THREAD="${PROOF_PREFIX}-other"
FOREIGN_TURN="${PROOF_PREFIX}-foreign"

echo "== seed ${TURN_COUNT} turns in ${PROBE_THREAD} and one turn in ${FOREIGN_THREAD}"
psql "${PROOF_DATABASE_URL}" -v ON_ERROR_STOP=1 -q <<SQL
INSERT INTO threads (id, session_key, org_id, user_id)
VALUES ('${PROBE_THREAD}', '${PROBE_THREAD}', '${PROOF_ORG_ID}', '${PROOF_USER_ID}'),
       ('${FOREIGN_THREAD}', '${FOREIGN_THREAD}', '${PROOF_ORG_ID}', '${PROOF_USER_ID}');
INSERT INTO messages (id, thread_id, role, content)
SELECT '${PROOF_PREFIX}-m' || lpad(n::text, 4, '0'),
       '${PROBE_THREAD}',
       CASE WHEN n % 2 = 0 THEN 'user' ELSE 'assistant' END,
       'turn ' || n
  FROM generate_series(0, ${TURN_COUNT} - 1) AS n
 ORDER BY n;
INSERT INTO messages (id, thread_id, role, content)
VALUES ('${FOREIGN_TURN}', '${FOREIGN_THREAD}', 'user', 'turn elsewhere');
SQL

echo "== page the transcript from the newest turns back to the oldest, ${PAGE_SIZE} at a time"
PAGES_FILE="$(mktemp)"
trap 'rm -f "${PAGES_FILE}"' EXIT
before=""
pages=0
while :; do
  url="${PROOF_BFF_URL}/api/v1/chat/threads/${PROBE_THREAD}/messages?limit=${PAGE_SIZE}"
  if [ -n "${before}" ]; then url="${url}&before=${before}"; fi
  body="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${url}")"
  pages=$((pages + 1))
  printf '%s\n' "${body}" >> "${PAGES_FILE}"
  more="$(printf '%s' "${body}" | python3 -c 'import json,sys; print("yes" if json.load(sys.stdin).get("has_more") is True else "no")')"
  if [ "${more}" != "yes" ]; then break; fi
  before="$(printf '%s' "${body}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["messages"][0]["message_id"])')"
  if [ "${pages}" -gt $((TURN_COUNT / PAGE_SIZE + 2)) ]; then
    echo "FAIL: the tail pages did not end after ${pages} pages" >&2
    exit 1
  fi
done

python3 - "${PAGES_FILE}" "${PROOF_PREFIX}" "${TURN_COUNT}" "${PAGE_SIZE}" "${pages}" <<'PY'
import json, sys
path, prefix, expected, page_size, pages = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
bodies = [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
assert len(bodies) == pages == expected // page_size, f"expected {expected // page_size} pages, read {len(bodies)}"
# The reads ran newest first; reversed they are the conversation in order.
turns = []
for body in reversed(bodies):
    assert len(body["messages"]) == page_size, "a page did not carry its full limit"
    turns.extend(body["messages"])
ids = [t["message_id"] for t in turns]
assert len(ids) == expected, f"expected {expected} turns, saw {len(ids)}"
assert len(set(ids)) == expected, "a turn repeated across pages"
assert ids == [f"{prefix}-m{n:04d}" for n in range(expected)], "the turns are not in conversation order"
# has_more is true on every page read before the oldest, and false on the oldest.
flags = [body["has_more"] for body in bodies]
assert flags == [True] * (len(bodies) - 1) + [False], f"has_more was {flags}"
print(f"ok: {len(bodies)} tail pages of {page_size}, {len(ids)} turns, none repeated, in order")
PY

echo "== an unpaged read is unchanged: every turn, oldest first, no has_more key"
unpaged="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${PROOF_BFF_URL}/api/v1/chat/threads/${PROBE_THREAD}/messages")"
printf '%s' "${unpaged}" | python3 -c '
import json, sys
body = json.load(sys.stdin)
assert "has_more" not in body, "an unpaged read must not carry has_more"
assert len(body["messages"]) == '"${TURN_COUNT}"', len(body["messages"])
assert body["messages"][0]["content"] == "turn 0", body["messages"][0]
print("ok: unpaged read returns all", len(body["messages"]), "turns, oldest first")
'

echo "== malformed and foreign paging parameters are refused with 400"
for query in "limit=0" "limit=201" "limit=fifty" "before=${PROOF_PREFIX}-m0100" "limit=10&before=a%26limit%3D1" "limit=10&before=${FOREIGN_TURN}"; do
  code="$(curl -s -o /dev/null -w '%{http_code}' -b "${PROOF_COOKIE_JAR}" "${PROOF_BFF_URL}/api/v1/chat/threads/${PROBE_THREAD}/messages?${query}")"
  if [ "${code}" != "400" ]; then
    echo "FAIL: ${query} answered ${code}, expected 400" >&2
    exit 1
  fi
done
echo "ok: six refused parameter sets answered 400"

echo "== plan check: the tail page reads the (thread_id, sequence) index backwards"
psql "${PROOF_DATABASE_URL}" -q -c "EXPLAIN (ANALYZE, BUFFERS) SELECT m.id::text, m.role, m.content, m.agent_name, m.metadata, m.author_subject_id FROM messages m JOIN threads t ON t.id = m.thread_id WHERE m.thread_id = '${PROBE_THREAD}' AND t.org_id = '${PROOF_ORG_ID}' AND m.sequence < (SELECT max(sequence) FROM messages WHERE thread_id = '${PROBE_THREAD}') ORDER BY m.sequence DESC LIMIT $((PAGE_SIZE + 1))"
