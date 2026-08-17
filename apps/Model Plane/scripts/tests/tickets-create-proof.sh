#!/usr/bin/env bash
set -euo pipefail

# Reproducible source and disposable-Postgres proof for the governed
# tickets.create owner operation. This is a dev/test harness only. It never
# reads, creates, rotates, or injects service credentials and never changes
# the Model capability allowlist.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MODEL_GO_DIR="$ROOT_DIR/go"
MODEL_RUST_DIR="$ROOT_DIR"
CONVERSATION_DIR="$ROOT_DIR/../Application Plane/conversation-core/conversation-core-go"
CONTROL_DIR="$ROOT_DIR/../Control Plane/user-core"
V3_GATEWAY_DIR="$ROOT_DIR/../Frontend Plane/verevonv3/apps/gateway"
TMP_DIR="$(mktemp -d)"
CONVERSATION_CONTAINER="model-plane-ticket-conversation-pg-${RANDOM}-$$"
CONTROL_CONTAINER="model-plane-ticket-control-pg-${RANDOM}-$$"
TEST_PG_PASSWORD="$(openssl rand -hex 16)"

cleanup() {
  docker rm -f "$CONVERSATION_CONTAINER" >/dev/null 2>&1 || true
  docker rm -f "$CONTROL_CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

run_proof() {
  local name="$1"
  shift
  printf 'PROOF %s\n' "$name"
  if "$@" >"$TMP_DIR/${name}.log" 2>&1; then
    printf 'PASS  %s\n' "$name"
  else
    printf 'BLOCKED %s\n' "$name"
    sed -n '1,220p' "$TMP_DIR/${name}.log"
    exit 2
  fi
}

run_postgres_proof() {
  local name="$1"
  shift
  local attempt log_file status
  printf 'PROOF %s\n' "$name"
  for attempt in $(seq 1 5); do
    log_file="$TMP_DIR/${name}.${attempt}.log"
    set +e
    "$@" >"$log_file" 2>&1
    status=$?
    set -e
    if [[ "$status" -eq 0 ]]; then
      printf 'PASS  %s\n' "$name"
      return 0
    fi
    # Docker Desktop may expose the published port before the host-side
    # forwarder is ready. Retry only transport-startup failures; migration,
    # assertion, and source failures remain hard failures.
    if ! grep -Eiq 'connection reset|unexpected eof|failed to receive message|connection refused|failed to connect' "$log_file"; then
      printf 'BLOCKED %s\n' "$name"
      sed -n '1,220p' "$log_file"
      return "$status"
    fi
    if [[ "$attempt" -eq 5 ]]; then
      printf 'BLOCKED %s\n' "$name"
      sed -n '1,220p' "$log_file"
      return 2
    fi
    sleep 2
  done
}

start_postgres() {
  local container="$1"
  local database="$2"
  docker create --pull=never --name "$container" -P \
    -e POSTGRES_PASSWORD="$TEST_PG_PASSWORD" \
    -e POSTGRES_DB="$database" \
    postgres:16-alpine >/dev/null
  docker start "$container" >/dev/null

  # Docker Desktop may publish the random host port after the container is
  # running, especially while the surrounding Rust/Go proof compiles. Keep a
  # bounded two-minute wait and suppress the expected pre-publication warning
  # so a startup race is not mistaken for an owner-contract failure.
  local port=""
  for _ in $(seq 1 120); do
    port="$(docker port "$container" 5432/tcp 2>/dev/null | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
    if [[ -n "$port" ]] && docker exec "$container" pg_isready -U postgres -d "$database" >/dev/null 2>&1; then
      printf '%s\n' "$port"
      return 0
    fi
    sleep 1
  done
  return 1
}

run_proof execution_core_ticket_contract \
  cargo test --manifest-path "$MODEL_RUST_DIR/rust/Cargo.toml" -p execution-core ticket --no-fail-fast

run_proof control_ticket_authority_contract \
  bash -c "cd \"$CONTROL_DIR\" && go test -count=1 ./internal/spaces ./internal/http -run 'Test(OwnerEffectReservation|IssueRunActionDecision|VerifyRunActionDecision|OwnerGrantDecision|ModelActionView)'"

run_proof conversation_ticket_owner_contract \
  bash -c "cd \"$CONVERSATION_DIR\" && go test -count=1 ./internal/http ./internal/conversation -run 'Test(AgentTicketOperation|ReconcileAgentTicketOperation|TicketOperation|AgentTicketActionAuthorization|CreateTicketOperation|GetTicketOperation|TicketOperationRequestDigest|ControlRunActionAuthorityValidator|ControlOwnerEffectReservationCoordinator|TicketCreateContract)'"

# The human V3 boundary is a separate contract from the private execution
# lane. Keep this proof in the same receipt gate: the gateway may normalize and
# forward the request, but must strip browser/control-only fields and never
# manufacture an owner operation receipt.
run_proof v3_gateway_ticket_boundary \
  cargo test --manifest-path "$V3_GATEWAY_DIR/Cargo.toml" \
    ticket_create_body_strips_sensitive_and_control_only_fields_before_conversation_upstream \
    --no-fail-fast

CONTROL_PORT="$(start_postgres "$CONTROL_CONTAINER" user_core)"
CONTROL_DSN="postgres://postgres:${TEST_PG_PASSWORD}@127.0.0.1:${CONTROL_PORT}/user_core?sslmode=disable"
run_postgres_proof control_owner_reservation_postgres_fence \
  bash -c "cd \"$CONTROL_DIR\" && TEST_DATABASE_URL=\"$CONTROL_DSN\" go test -count=1 ./internal/spaces -run 'TestOwnerEffectReservation(AuthorityRevisionCancelsOnlyUncommittedRows|RepositoryRoundTripAgainstRealPostgres)$'"

CONVERSATION_PORT="$(start_postgres "$CONVERSATION_CONTAINER" conversation_core)"
CONVERSATION_DSN="postgres://postgres:${TEST_PG_PASSWORD}@127.0.0.1:${CONVERSATION_PORT}/conversation_core?sslmode=disable"
run_postgres_proof conversation_owner_postgres_interleavings \
  bash -c "cd \"$CONVERSATION_DIR\" && TEST_DATABASE_URL=\"$CONVERSATION_DSN\" TEST_CONTROL_DATABASE_URL=\"$CONTROL_DSN\" go test -count=1 ./internal/http -run 'TestLive(AgentTicketOperationIntentUnknownLifecycle|AgentTicketOperationRevocationAtOwnerCommitBoundary|AgentTicketOperationRevocationAtOwnerCommitBoundaryAgainstControlPostgres|OwnerGrantRevocationRevisionHardening|OwnerGrantRevokeAndEffectRace|OwnerGrantCreateRevokeIdempotentAndReplayed)$'"

printf 'tickets.create proof: source contracts, owner interleavings, and durable unknown/receipt paths passed\n'
printf 'Model capability remains unavailable until deployed signer, transport, provider receipt, and candidate evidence are recorded.\n'
