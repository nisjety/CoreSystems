#!/usr/bin/env bash
# Operator proof for the conversation history foundation (build plan WS17).
#
# S1 (cursor paging and by-id read): seeds 150 chat threads for one user straight
# into session-core's database, pages the BFF listing to its end, checks that
# every thread came back once, and opens thread 120 by id. Later WS17 slices add
# their own checks here.
#
# Run by the lead against a rebuilt stack, never in CI. The database must be
# disposable: the script inserts rows and does not clean them up.
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
PROOF_PREFIX="ws17s1-${PROOF_RUN}"
THREAD_COUNT=150
PAGE_SIZE=50
PROBE_INDEX=120

echo "== seed ${THREAD_COUNT} threads (prefix ${PROOF_PREFIX})"
psql "${PROOF_DATABASE_URL}" -v ON_ERROR_STOP=1 -q <<SQL
INSERT INTO threads (id, session_key, org_id, user_id, last_activity_at)
SELECT '${PROOF_PREFIX}-' || lpad(n::text, 3, '0'),
       '${PROOF_PREFIX}-' || lpad(n::text, 3, '0'),
       '${PROOF_ORG_ID}',
       '${PROOF_USER_ID}',
       TIMESTAMPTZ '2026-10-01 00:00:00+00' + make_interval(secs => n)
  FROM generate_series(0, ${THREAD_COUNT} - 1) AS n;
SQL

echo "== page the BFF listing to its end"
PAGES_FILE="$(mktemp)"
trap 'rm -f "${PAGES_FILE}"' EXIT
cursor=""
pages=0
while :; do
  url="${PROOF_BFF_URL}/api/v1/chat/threads?limit=${PAGE_SIZE}"
  if [ -n "${cursor}" ]; then url="${url}&cursor=${cursor}"; fi
  body="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${url}")"
  pages=$((pages + 1))
  printf '%s\n' "${body}" >> "${PAGES_FILE}"
  next="$(printf '%s' "${body}" | python3 -c 'import json,sys; c=json.load(sys.stdin)["data"].get("nextCursor"); print(c or "")')"
  if [ -z "${next}" ]; then break; fi
  cursor="${next}"
  if [ "${pages}" -gt $((THREAD_COUNT / PAGE_SIZE + 2)) ]; then
    echo "FAIL: listing did not end after ${pages} pages" >&2
    exit 1
  fi
done

python3 - "${PAGES_FILE}" "${PROOF_PREFIX}" "${THREAD_COUNT}" "${pages}" <<'PY'
import json, sys
path, prefix, expected, pages = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
ids = []
for line in open(path, encoding="utf-8"):
    if line.strip():
        ids.extend(s["threadId"] for s in json.loads(line)["data"]["sessions"])
mine = [i for i in ids if i.startswith(prefix)]
assert len(mine) == expected, f"expected {expected} seeded threads, saw {len(mine)}"
assert len(set(mine)) == expected, "a seeded thread repeated across pages"
print(f"ok: {pages} pages, {len(mine)} seeded threads, none repeated")
PY

echo "== open thread ${PROBE_INDEX} by id"
probe_id="${PROOF_PREFIX}-$(printf '%03d' "${PROBE_INDEX}")"
probe="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${PROOF_BFF_URL}/api/v1/chat/threads/${probe_id}")"
printf '%s' "${probe}" | python3 -c 'import json,sys; s=json.load(sys.stdin)["data"]["session"]; assert s["threadId"]=="'"${probe_id}"'", s; print("ok: opened", s["threadId"])'

echo "== archive thread ${PROBE_INDEX}, then restore it (S2)"
# Walks every page of one archive state and reports whether the probe is listed.
probe_listed() {
  local mode="$1"
  local cursor=""
  local url body next
  while :; do
    url="${PROOF_BFF_URL}/api/v1/chat/threads?limit=${PAGE_SIZE}&archived=${mode}"
    if [ -n "${cursor}" ]; then url="${url}&cursor=${cursor}"; fi
    body="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${url}")"
    if printf '%s' "${body}" | python3 -c 'import json,sys; ids=[s["threadId"] for s in json.load(sys.stdin)["data"]["sessions"]]; sys.exit(0 if "'"${probe_id}"'" in ids else 1)'; then
      echo yes
      return
    fi
    next="$(printf '%s' "${body}" | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"].get("nextCursor") or "")')"
    if [ -z "${next}" ]; then
      echo no
      return
    fi
    cursor="${next}"
  done
}
curl -fsS -b "${PROOF_COOKIE_JAR}" -X POST "${PROOF_BFF_URL}/api/v1/chat/threads/${probe_id}/archive" > /dev/null
[ "$(probe_listed active)" = "no" ] || { echo "FAIL: archived thread still in the active list" >&2; exit 1; }
[ "$(probe_listed archived)" = "yes" ] || { echo "FAIL: archived thread missing from the archived list" >&2; exit 1; }
curl -fsS -b "${PROOF_COOKIE_JAR}" -X POST "${PROOF_BFF_URL}/api/v1/chat/threads/${probe_id}/unarchive" > /dev/null
[ "$(probe_listed active)" = "yes" ] || { echo "FAIL: restored thread missing from the active list" >&2; exit 1; }
echo "ok: archive and restore moved ${probe_id} between the lists"

echo "== plan check: the newest-first keyset uses the owner-activity index"
psql "${PROOF_DATABASE_URL}" -q -c "EXPLAIN (ANALYZE, BUFFERS) SELECT t.id FROM threads t WHERE t.org_id = '${PROOF_ORG_ID}' AND t.user_id = '${PROOF_USER_ID}' AND t.archived_at IS NULL ORDER BY (t.pinned_at IS NOT NULL) DESC, t.last_activity_at DESC, t.id DESC LIMIT 51"

echo "== search the probe thread by a word only it holds (S3)"
# Message search needs a word no other thread holds. The token is lower-case
# alphanumeric from the run stamp, so the query reads it as one prefix word.
PROOF_TOKEN="ws17s3probe$(printf '%s' "${PROOF_RUN}" | tr -cd 'a-z0-9')"
psql "${PROOF_DATABASE_URL}" -v ON_ERROR_STOP=1 -q <<SQL
INSERT INTO messages (id, thread_id, role, content)
VALUES ('${PROOF_PREFIX}-search', '${probe_id}', 'user', 'the ${PROOF_TOKEN} marker');
SQL
search_body="$(curl -fsS -b "${PROOF_COOKIE_JAR}" "${PROOF_BFF_URL}/api/v1/chat/threads/search?q=${PROOF_TOKEN}")"
printf '%s' "${search_body}" | python3 -c '
import json, sys
hits = json.load(sys.stdin)["data"]["hits"]
probe = sys.argv[1]
mine = [h for h in hits if h["threadId"] == probe]
assert len(mine) == 1, f"expected one hit in the probe thread, saw {len(mine)}"
assert "\ue000" in mine[0]["snippet"] and "\ue001" in mine[0]["snippet"], mine[0]["snippet"]
print("ok: search found the probe thread once, highlight markers present")
' "${probe_id}"

echo "== plan check: the search predicate uses the content index"
psql "${PROOF_DATABASE_URL}" -q -c "EXPLAIN (ANALYZE, BUFFERS) SELECT m.id FROM messages m WHERE m.role IN ('user', 'assistant') AND to_tsvector('simple', left(m.content, 65536)) @@ to_tsquery('simple', '${PROOF_TOKEN}':*)"

echo "PASS: WS17 S1-S3 history proof (${PROOF_PREFIX})"
