#!/usr/bin/env bash
# verify-approvals-v2.sh: the approvals v2 end-to-end walk (build plan WS13 S8).
#
# Against a running stack it checks what S1 to S6 promise on real approvals, through the model
# gateway's HTTP surface and the session-core database: the cause a pause states (S1), the closed
# projection the surfaces read (S2), the provenance and the single audit row of a decision (S3), a
# declined call that is not asked again (S4), the step-up a high-risk grant needs (S5), and the
# thread grant that releases the next identical call (S6). Each response is kept in a dated
# evidence folder under docs/compliance/evidence.
#
# The walk starts no model turn. Make the pauses first (a provider write under ask, a tickets.create,
# an eligible read-class pause, a declined call that is re-proposed), then give their ids. A decision
# is made only with DECIDE=1, and a revocation only with REVOKE=1. Every value comes from the
# environment; no credential is read from a file, written to the evidence folder or printed.
#
#   MP_GATEWAY_URL                    model gateway base URL, no trailing slash
#   MP_USER_BEARER                    bearer of the run owner
#   SESSION_CORE_SQL                  a command that reads SQL on stdin and prints unaligned rows,
#                                     for example: docker exec -i <session-core-postgres> psql -U <user> -d <db> -tAq
#   APPROVAL_ID                       a pending provider-write pause of RUN_ID (S1, S2, S3, S6 refusal)
#   RUN_ID                            the run that owns APPROVAL_ID
#   THREAD_ID                         the thread of RUN_ID (S6 grant list)
#   DECIDE=1                          make the decisions: S3 (approve, then replay), S5, S6
#   APPROVAL_AUDIT_EVENTS_ENABLED=1   the stack runs with the audit switch on (S3 audit row)
#   STEP_UP_ENFORCE=1                 the gateway runs with APPROVAL_STEP_UP_ENFORCED=enforce (S5)
#   STEP_UP_APPROVAL_ID               a pending high-risk pause for S5 (a pause you are willing to approve)
#   MP_STEP_UP_BEARER                 optional: the owner's bearer after a fresh step-up (S5 granted path)
#   DENIED_APPROVAL_ID                a pause the person declined; re-propose its identical call first (S4)
#   SCOPE_GRANTS=1                    the stack runs with APPROVAL_SCOPE_GRANTS_ENABLED=true (S6)
#   GRANT_APPROVAL_ID                 an eligible pause for S6 (read class, consent, or risky under ask)
#   GRANT_RELEASED=1                  second run, after the identical call ran in THREAD_ID (S6 release)
#   REVOKE=1                          revoke GRANT_APPROVAL_ID's grant through the gateway (S6)
#   EVIDENCE_DIR                      optional: where the responses and result.txt go
#
# Exit status: 0 when no check failed, 1 when one did. A check that cannot run says why and is
# SKIP, never PASS.
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
EVIDENCE="${EVIDENCE_DIR:-$ROOT/docs/compliance/evidence/approvals-v2-$STAMP}"
FAILED=0

if [[ -z "${MP_GATEWAY_URL:-}" || -z "${MP_USER_BEARER:-}" || -z "${SESSION_CORE_SQL:-}" \
  || -z "${APPROVAL_ID:-}" || -z "${RUN_ID:-}" ]]; then
  echo "usage: set MP_GATEWAY_URL, MP_USER_BEARER, SESSION_CORE_SQL, APPROVAL_ID and RUN_ID (see the header)" >&2
  exit 2
fi
# Ids go into SQL text, so only plain identifiers are accepted.
for id in "$APPROVAL_ID" "$RUN_ID" "${THREAD_ID:-run}" "${STEP_UP_APPROVAL_ID:-appr_none}" \
  "${DENIED_APPROVAL_ID:-appr_none}" "${GRANT_APPROVAL_ID:-appr_none}"; do
  if [[ ! "$id" =~ ^[A-Za-z0-9_-]+$ ]]; then
    echo "refused: an id is not a plain identifier" >&2
    exit 2
  fi
done

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

# A dotted field of a JSON file, empty when absent or when the file is not JSON.
field() {
  python3 -c '
import json, sys
value = json.load(open(sys.argv[1]))
for key in sys.argv[2].split("."):
    value = value.get(key) if isinstance(value, dict) else None
print("" if value is None else value)
' "$1" "$2" 2>/dev/null || true
}

# The closed cause vocabulary, from the contract the Rust tests pin.
CAUSES="$(python3 -c '
import json, sys
print(" ".join(row["code"] for row in json.load(open(sys.argv[1]))["codes"]))
' "$ROOT/apps/Model Plane/api/contracts/approval-causes.json")"

s1_cause() {
  local status code stored
  status="$(http GET "/v1/orchestration/approvals/$APPROVAL_ID" "$MP_USER_BEARER" "" s1-approval)"
  if [[ "$status" != 200 ]]; then
    fail "S1 the pause reads back with status $status"
    return
  fi
  code="$(field "$EVIDENCE/s1-approval.json" approval.cause.code)"
  if [[ -z "$code" ]]; then
    fail "S1 the pause states no cause"
    return
  fi
  if [[ " $CAUSES " != *" $code "* ]]; then
    fail "S1 the cause '$code' is not in approval-causes.json"
    return
  fi
  pass "S1 the pause states the closed cause $code"
  stored="$(db "SELECT metadata->'cause'->>'code' FROM approvals WHERE id = '$APPROVAL_ID'")"
  if [[ "$stored" == "$code" ]]; then
    pass "S1 approvals.metadata.cause.code is the code the gateway reads"
  else
    fail "S1 the stored cause '$stored' differs from the read cause '$code'"
  fi
}

s2_projection() {
  local leaked
  if [[ ! -f "$EVIDENCE/s1-approval.json" ]]; then
    fail "S2 there is no approval response to project"
    return
  fi
  if ! leaked="$(python3 -c '
import json, sys
found = set()
def walk(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key in ("context", "action_key", "call_digest"):
                found.add(key)
            walk(child)
    elif isinstance(value, list):
        for child in value:
            walk(child)
walk(json.load(open(sys.argv[1])))
print(" ".join(sorted(found)))
' "$EVIDENCE/s1-approval.json")"; then
    fail "S2 the approval response is not JSON, so its projection cannot be read"
    return
  fi
  if [[ -z "$leaked" ]]; then
    pass "S2 the projection carries no context, action_key or call_digest"
  else
    fail "S2 the projection carries: $leaked"
  fi
}

s6_scope_not_offered() {
  local status code
  status="$(http POST "/v1/orchestration/approvals/$APPROVAL_ID/decide" "$MP_USER_BEARER" \
    '{"decision":"approve","scope":"thread"}' s6-scope-refused)"
  code="$(field "$EVIDENCE/s6-scope-refused.json" error.code)"
  if [[ "$status" == 4?? && ( "$code" == scope_not_offered || "$code" == scope_not_enabled ) ]]; then
    pass "S6 a thread scope on a provider write is refused ($code, HTTP $status)"
  else
    fail "S6 a thread scope on a provider write was not refused (HTTP $status, code '$code')"
  fi
}

s3_provenance() {
  local status before after decided_source replay_status
  if [[ "${DECIDE:-0}" != 1 ]]; then
    skip "S3 provenance and the audit row: DECIDE=1 makes the decision"
    return
  fi
  before="$(db "SELECT count(*) FROM session_audit_outbox WHERE payload->>'event' = 'approval_decided' AND payload->>'resource_id' = '$APPROVAL_ID'")"
  status="$(http POST "/v1/orchestration/approvals/$APPROVAL_ID/decide" "$MP_USER_BEARER" '{"decision":"approve"}' s3-decide)"
  if [[ "$status" != 200 ]]; then
    fail "S3 the approval decision returned HTTP $status"
    return
  fi
  decided_source="$(db "SELECT metadata->'decision'->>'source' FROM approvals WHERE id = '$APPROVAL_ID'")"
  if [[ "$decided_source" == user ]]; then
    pass "S3 the decision records its source as user"
  else
    fail "S3 the decision records source '$decided_source', expected user"
  fi
  if [[ "${APPROVAL_AUDIT_EVENTS_ENABLED:-0}" == 1 ]]; then
    after="$(db "SELECT count(*) FROM session_audit_outbox WHERE payload->>'event' = 'approval_decided' AND payload->>'resource_id' = '$APPROVAL_ID'")"
    if [[ "$after" == "$((before + 1))" ]]; then
      pass "S3 the decision enqueued one approval_decided audit row"
    else
      fail "S3 the decision enqueued $((after - before)) audit rows, expected one"
    fi
    replay_status="$(http POST "/v1/orchestration/approvals/$APPROVAL_ID/decide" "$MP_USER_BEARER" '{"decision":"approve"}' s3-replay)"
    after="$(db "SELECT count(*) FROM session_audit_outbox WHERE payload->>'event' = 'approval_decided' AND payload->>'resource_id' = '$APPROVAL_ID'")"
    if [[ "$replay_status" == 200 && "$after" == "$((before + 1))" ]]; then
      pass "S3 a replayed decision answers again and adds no audit row"
    else
      fail "S3 the replay answered HTTP $replay_status and left $after audit rows"
    fi
  else
    skip "S3 the audit row: APPROVAL_AUDIT_EVENTS_ENABLED=1 says the stack records it"
  fi
}

s4_denial_memory() {
  local status digest count
  if [[ -z "${DENIED_APPROVAL_ID:-}" ]]; then
    skip "S4 denial memory: DENIED_APPROVAL_ID is not set"
    return
  fi
  status="$(db "SELECT status FROM approvals WHERE id = '$DENIED_APPROVAL_ID'")"
  if [[ "$status" != denied ]]; then
    fail "S4 $DENIED_APPROVAL_ID is '$status', not denied"
    return
  fi
  digest="$(db "SELECT metadata->'cause'->>'call_digest' FROM approvals WHERE id = '$DENIED_APPROVAL_ID'")"
  if [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    fail "S4 the declined pause carries no call digest"
    return
  fi
  count="$(db "SELECT count(*) FROM approvals WHERE run_id = '$RUN_ID' AND metadata->'cause'->>'call_digest' = '$digest'")"
  if [[ "$count" == 1 ]]; then
    pass "S4 the declined call has one approval in its run, so it was not asked again"
  else
    fail "S4 the declined call has $count approvals in $RUN_ID, expected 1"
  fi
}

s5_step_up() {
  local status code verified
  if [[ "${STEP_UP_ENFORCE:-0}" != 1 || -z "${STEP_UP_APPROVAL_ID:-}" ]]; then
    skip "S5 step-up: STEP_UP_ENFORCE=1 and STEP_UP_APPROVAL_ID are needed"
    return
  fi
  if [[ "${DECIDE:-0}" != 1 ]]; then
    skip "S5 step-up: DECIDE=1 is needed, because a grant without the claim is refused and a granted one is a decision"
    return
  fi
  status="$(http POST "/v1/orchestration/approvals/$STEP_UP_APPROVAL_ID/decide" "$MP_USER_BEARER" '{"decision":"approve"}' s5-no-step-up)"
  code="$(field "$EVIDENCE/s5-no-step-up.json" error.code)"
  if [[ "$status" == 403 && "$code" == step_up_required ]]; then
    pass "S5 a grant without a fresh step-up is refused (HTTP 403 step_up_required)"
  else
    fail "S5 a grant without a fresh step-up answered HTTP $status '$code', expected 403 step_up_required"
    return
  fi
  if [[ -z "${MP_STEP_UP_BEARER:-}" ]]; then
    skip "S5 the granted path: MP_STEP_UP_BEARER is not set"
    return
  fi
  status="$(http POST "/v1/orchestration/approvals/$STEP_UP_APPROVAL_ID/decide" "$MP_STEP_UP_BEARER" '{"decision":"approve"}' s5-with-step-up)"
  verified="$(db "SELECT metadata->'decision'->>'step_up_verified' FROM approvals WHERE id = '$STEP_UP_APPROVAL_ID'")"
  if [[ "$status" == 200 && "$verified" == true ]]; then
    pass "S5 with a fresh step-up the grant is recorded with step_up_verified"
  else
    fail "S5 with a fresh step-up answered HTTP $status, step_up_verified '$verified'"
  fi
}

s6_grant() {
  local status grants use revoked
  if [[ "${SCOPE_GRANTS:-0}" != 1 || -z "${GRANT_APPROVAL_ID:-}" ]]; then
    skip "S6 thread grant: SCOPE_GRANTS=1 and GRANT_APPROVAL_ID are needed"
    return
  fi
  if [[ "${DECIDE:-0}" == 1 ]]; then
    status="$(http POST "/v1/orchestration/approvals/$GRANT_APPROVAL_ID/decide" "$MP_USER_BEARER" \
      '{"decision":"approve","scope":"thread"}' s6-thread-grant)"
    if [[ "$status" != 200 ]]; then
      fail "S6 the thread decision returned HTTP $status"
      return
    fi
    grants="$(db "SELECT count(*) FROM approval_scope_grants WHERE created_from_approval_id = '$GRANT_APPROVAL_ID' AND scope = 'thread' AND revoked_at IS NULL")"
    if [[ "$grants" == 1 ]]; then
      pass "S6 the thread decision left one active grant"
    else
      fail "S6 the thread decision left $grants active grants, expected one"
    fi
  else
    skip "S6 the thread decision: DECIDE=1 makes it"
  fi
  if [[ "${GRANT_RELEASED:-0}" == 1 ]]; then
    use="$(db "SELECT use_count FROM approval_scope_grants WHERE created_from_approval_id = '$GRANT_APPROVAL_ID'")"
    if [[ "${use:-0}" -ge 1 ]]; then
      pass "S6 the identical call was released by the grant (use_count $use)"
    else
      fail "S6 the grant released no call (use_count '${use:-}')"
    fi
  else
    skip "S6 the release: re-run the identical call in THREAD_ID, then run with GRANT_RELEASED=1"
  fi
  if [[ -z "${THREAD_ID:-}" ]]; then
    skip "S6 the grant list: THREAD_ID is not set"
  else
    status="$(http GET "/v1/orchestration/threads/$THREAD_ID/approval-grants" "$MP_USER_BEARER" "" s6-grants-list)"
    if [[ "$status" == 200 ]]; then
      pass "S6 the gateway lists the thread's grants"
    else
      fail "S6 the gateway serves no grant list (HTTP $status)"
    fi
  fi
  if [[ "${REVOKE:-0}" == 1 ]]; then
    grants="$(db "SELECT id FROM approval_scope_grants WHERE created_from_approval_id = '$GRANT_APPROVAL_ID' AND revoked_at IS NULL LIMIT 1")"
    if [[ ! "$grants" =~ ^[A-Za-z0-9_-]+$ ]]; then
      fail "S6 no active grant to revoke for $GRANT_APPROVAL_ID"
      return
    fi
    status="$(http DELETE "/v1/orchestration/approval-grants/$grants" "$MP_USER_BEARER" "" s6-revoke)"
    revoked="$(db "SELECT revoked_at IS NOT NULL FROM approval_scope_grants WHERE id = '$grants'")"
    if [[ "$status" == 200 && "$revoked" == t ]]; then
      pass "S6 the revoked grant no longer releases calls"
    else
      fail "S6 revoke answered HTTP $status, revoked '$revoked' (the gateway has no revoke route if 404)"
    fi
  else
    skip "S6 the revocation: REVOKE=1 makes it"
  fi
}

s1_cause
s2_projection
s6_scope_not_offered
s3_provenance
s4_denial_memory
s5_step_up
s6_grant

say "RESULT $FAILED failed check(s); evidence in $EVIDENCE"
[[ "$FAILED" -eq 0 ]]
