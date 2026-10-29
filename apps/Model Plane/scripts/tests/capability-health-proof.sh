#!/usr/bin/env bash
set -euo pipefail

# Reproducible R-2 proof for Capability Core health authority. This is a
# source/integration harness only: it uses ephemeral test keys and disposable
# Postgres, never reads or rotates deployment credentials, and never promotes
# a Model capability.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GO_DIR="$ROOT_DIR/go"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

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

run_proof capability_authorization_unit \
  bash -c "cd \"$GO_DIR\" && go test -count=1 ./services/capability-core/internal/api ./services/capability-core/internal/authz -run 'Test(GlobalHealthAttestationRequiresDedicatedServiceScope|GenericGlobalHealthAttestorCannotAttestOwnerActionTicket|GenericGlobalHealthEndpointCannotPersistOwnerActionTicket|TenantHealthEndpointCannotPersistGlobalOwnerActionTicket|TenantHealthEndpointCannotPersistGlobalRuntimeCapability)'"

run_proof stale_and_unhealthy_policy \
  bash -c "cd \"$GO_DIR\" && go test -count=1 ./services/capability-core/internal/models ./services/capability-core/internal/server -run 'TestDeriveAvailability(MarksRiskyHealthyCapabilityApprovalRequired|RejectsMissingStaleOrFutureHealthProof|DistinguishesDisabledAndUnhealthy)|TestEvaluatePolicy(.*Stale|.*stale|.*Unhealthy|.*unhealthy)'"

run_proof signed_postgres_negative \
  bash -c "cd \"$GO_DIR\" && DOCKER_HOST=unix:///var/run/docker.sock go test -tags integration -count=1 ./services/capability-core/internal/api -run 'Test(SignedTenantHealthCannotWriteGlobalCapabilityRowAgainstPostgres|GenericAndTenantHealthCannotWriteGlobalOwnerActionCapabilityAgainstPostgres)$'"

printf 'R-2 capability health proof: authorization, stale/unhealthy fail-closed policy, and real-Postgres no-write evidence passed\n'
printf 'Capability promotion remains source_only until live candidate evidence and operator review.\n'
