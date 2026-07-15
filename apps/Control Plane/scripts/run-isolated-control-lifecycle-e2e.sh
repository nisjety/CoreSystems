#!/usr/bin/env bash

# Runs destructive lifecycle assertions only inside a uniquely named,
# volume-free Postgres container. It never reads the Control Plane DATABASE_URL
# and never calls Lago, a payment provider, or a live tenant endpoint.

set -euo pipefail

root_dir="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
container="control-mvp-lifecycle-${PPID}-$$"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/control-lifecycle.XXXXXX")"
postgres_password="$(openssl rand -hex 24)"
fixture_id="$(openssl rand -hex 24)"

emit_bounded_container_logs() {
  local target="$1"
  docker logs --tail 160 "$target" 2>&1 |
    node "$root_dir/scripts/redact-container-logs.mjs" || true
}

cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
  rm -rf "$work_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for command in docker go node pnpm openssl; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command is unavailable: $command" >&2
    exit 1
  fi
done

docker run --rm -d \
  --name "$container" \
  --tmpfs /var/lib/postgresql/data:rw,noexec,nosuid,size=512m \
  -e POSTGRES_USER=control_lifecycle \
  -e POSTGRES_PASSWORD="$postgres_password" \
  -e POSTGRES_DB=postgres \
  -p 127.0.0.1::5432 \
  postgres:16-alpine >/dev/null

# Fresh arm64 Postgres initialization can exceed 30 seconds while the wider
# Docker stack is rebuilding. Poll for up to 90 seconds; healthy fixtures still
# proceed immediately.
for _ in $(seq 1 90); do
  if docker exec "$container" pg_isready \
    -U control_lifecycle -d postgres >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! docker exec "$container" pg_isready \
  -U control_lifecycle -d postgres >/dev/null 2>&1; then
  echo "disposable Postgres did not become ready" >&2
  emit_bounded_container_logs "$container"
  exit 1
fi

host_port="$(docker inspect -f '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "$container")"
for database in auth_invitation auth_outbox org_lifecycle billing_lifecycle; do
  docker exec "$container" createdb -U control_lifecycle "$database"
  docker exec "$container" psql -v ON_ERROR_STOP=1 \
    -U control_lifecycle -d "$database" \
    -c "CREATE TABLE control_lifecycle_fixture (fixture_id TEXT PRIMARY KEY, database_name TEXT NOT NULL); INSERT INTO control_lifecycle_fixture (fixture_id, database_name) VALUES ('$fixture_id', '$database');" \
    >/dev/null
done
export CONTROL_LIFECYCLE_FIXTURE_ID="$fixture_id"
export PSQL_DOCKER_CONTAINER="$container"
export PSQL_DOCKER_USER=control_lifecycle

dsn() {
  printf 'postgres://control_lifecycle:%s@127.0.0.1:%s/%s?sslmode=disable' \
    "$postgres_password" "$host_port" "$1"
}

assert_function_coverage() {
  local profile="$1"
  shift
  local report
  report="$(go tool cover -func="$profile")"

  local function_name
  for function_name in "$@"; do
    if ! printf '%s\n' "$report" | awk -v function_name="$function_name" '
      $2 == function_name {
        matches++
        coverage = $3
        sub(/%$/, "", coverage)
      }
      END {
        if (matches != 1) {
          printf "coverage gate: %s matched %d functions; want exactly 1\n", function_name, matches > "/dev/stderr"
          exit 1
        }
        if ((coverage + 0) < 80) {
          printf "coverage gate: %s is %.1f%%; want >=80.0%%\n", function_name, coverage > "/dev/stderr"
          exit 1
        }
      }
    '; then
      return 1
    fi
  done
}

assert_file_function_coverage() {
  local profile="$1"
  local file_suffix="$2"
  local function_name="$3"
  local report
  report="$(go tool cover -func="$profile")"

  printf '%s\n' "$report" | awk \
    -v file_suffix="$file_suffix" \
    -v function_name="$function_name" '
      index($1, file_suffix ":") > 0 && $2 == function_name {
        matches++
        coverage = $3
        sub(/%$/, "", coverage)
      }
      END {
        if (matches != 1) {
          printf "coverage gate: %s:%s matched %d functions; want exactly 1\n", file_suffix, function_name, matches > "/dev/stderr"
          exit 1
        }
        if ((coverage + 0) < 80) {
          printf "coverage gate: %s:%s is %.1f%%; want >=80.0%%\n", file_suffix, function_name, coverage > "/dev/stderr"
          exit 1
        }
      }
    '
}

assert_jest_line_coverage() {
  local summary="$1"
  shift
  node - "$summary" "$@" <<'NODE'
const fs = require('node:fs');

const [summaryPath, ...expectedFiles] = process.argv.slice(2);
const report = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
for (const expectedFile of expectedFiles) {
  const match = Object.entries(report).find(([file]) =>
    file.endsWith(`/src/auth/${expectedFile}`),
  );
  if (!match) {
    throw new Error(`coverage entry missing for ${expectedFile}`);
  }
  const coverage = Number(match[1].lines.pct);
  process.stdout.write(`${expectedFile} line coverage: ${coverage.toFixed(2)}%\n`);
  if (!Number.isFinite(coverage) || coverage < 80) {
    throw new Error(`${expectedFile} coverage < 80`);
  }
}
NODE
}

echo "[1/7] Better Auth invitation repair and reviewed historical owner preflight"
(
  cd "$root_dir/auth-core"
  TEST_DATABASE_URL="$(dsn auth_invitation)" \
    pnpm exec jest --runInBand \
      --testTimeout=30000 \
      --coverage \
      --coverageDirectory="$work_dir/auth-coverage" \
      --coverageReporters=text \
      --coverageReporters=json-summary \
      --collectCoverageFrom=auth/invitation-acceptance.controller.ts \
      --collectCoverageFrom=auth/invitation-acceptance-repair.ts \
      --collectCoverageFrom=auth/invitation-acceptance-rate-limit.ts \
      src/auth/invitation-acceptance.controller.spec.ts \
      src/auth/invitation-acceptance-repair.spec.ts \
      src/auth/invitation-acceptance-repair.postgres.spec.ts \
      src/auth/invitation-acceptance-rate-limit.spec.ts \
      src/auth/gdpr-outbox-erasure.postgres.spec.ts \
      src/auth/membership-audit-outbox.postgres.spec.ts \
      src/auth/owner-invariant-preflight.postgres.spec.ts
  assert_jest_line_coverage \
    "$work_dir/auth-coverage/coverage-summary.json" \
    invitation-acceptance.controller.ts \
    invitation-acceptance-repair.ts \
    invitation-acceptance-rate-limit.ts
)

echo "[2/7] Auth deletion outbox partial-failure resume and exact endpoints"
(
  cd "$root_dir/auth-core"
  lifecycle_dsn="$(dsn auth_outbox)"
  DATABASE_URL="$lifecycle_dsn" \
  CONTROL_LIFECYCLE_TEST_DATABASE_URL="$lifecycle_dsn" \
    pnpm exec jest --runInBand \
      src/auth/organization-deletion-lifecycle.postgres.spec.ts
)

echo "[3/7] Org projection/plan ordering, durable GDPR audit, erasure retry, and tombstone"
(
  cd "$root_dir/org-core"
  CONTROL_LIFECYCLE_TEST_DATABASE_URL="$(dsn org_lifecycle)" \
    go test ./internal/org \
      -run 'Test(ControlLifecycle(DeletionRejectsInvalidInputBeforeDatabaseAccess|DeletionTombstoneDecision|ProjectionOrderingAndDeletionRetry|PlanRevisionOutboxRetriesWithoutLosingOrder|GDPRAuditOutboxIsAtomicRetryableAndBounded)|PlanChangeBoundariesFailClosedBeforeStorage|GDPRAudit.*|FlushGDPRAudit.*)' \
      -count=1 -coverprofile="$work_dir/org.cover"
  go tool cover -func="$work_dir/org.cover" | \
    grep -E 'ReconcileOrganization(Projection|Member|Deletion)|UpdatePlanWithOutbox|FlushPlanChangeOutbox|GDPRAudit|total:'
  assert_function_coverage "$work_dir/org.cover" \
    UpdatePlanWithOutbox FlushPlanChangeOutbox
  assert_file_function_coverage "$work_dir/org.cover" \
    repository.go ReconcileOrganizationDeletion
  assert_file_function_coverage "$work_dir/org.cover" \
    repository.go evaluateDeletionTombstone
  for function_name in \
    newGDPRAuditEvent executeGDPRAuditOperation enqueueGDPRAuditEvent \
    ClaimGDPRAuditOutbox MarkGDPRAuditPublished \
    MarkGDPRAuditPublishFailed FlushGDPRAuditOutbox; do
    assert_file_function_coverage "$work_dir/org.cover" \
      gdpr_audit_outbox.go "$function_name"
  done

  go test ./internal/nats \
    -run 'Test(PublishAuditMessage|ClientPublishAudit)' \
    -count=1 -coverprofile="$work_dir/org-nats.cover"
  assert_file_function_coverage "$work_dir/org-nats.cover" \
    client.go PublishAudit
  assert_file_function_coverage "$work_dir/org-nats.cover" \
    client.go publishAuditMessage
)

echo "[4/7] Org scoped HTTP middleware, routes, handlers, and Postgres"
(
  cd "$root_dir/org-core"
  CONTROL_LIFECYCLE_TEST_DATABASE_URL="$(dsn org_lifecycle)" \
    go test ./internal/http -count=1
)

echo "[5/7] Full Billing package: revisions, atomic usage, stale writers, and tombstones"
(
  cd "$root_dir/billing-core"
  CONTROL_LIFECYCLE_TEST_DATABASE_URL="$(dsn billing_lifecycle)" \
    go test ./internal/billing -count=1 \
      -coverprofile="$work_dir/billing.cover"
  go tool cover -func="$work_dir/billing.cover" | \
    grep -E 'TombstoneOrganization|UpsertAccount|Apply(Organization)?Plan(Change|Revision)|SaveAccountStateCAS|RecordUsage|DeactivateOrganization|total:'
  assert_function_coverage "$work_dir/billing.cover" \
    ApplyOrganizationPlanRevision TombstoneOrganization DeactivateOrganization
  assert_file_function_coverage "$work_dir/billing.cover" \
    repository.go SaveAccountStateCAS
  assert_file_function_coverage "$work_dir/billing.cover" \
    repository.go RecordUsage
  assert_file_function_coverage "$work_dir/billing.cover" \
    service.go RecordUsage
)

echo "[6/7] Billing durable plan consumer retry and dead-letter boundary"
(
  cd "$root_dir/billing-core"
  go test ./internal/nats \
    -run TestPlanChangeConsumerIsDurableRetriesBeforeAckAndDeadLettersMalformed \
    -count=1
)

echo "[7/7] Billing scoped HTTP middleware, routes, handlers, and Postgres"
(
  cd "$root_dir/billing-core"
  CONTROL_LIFECYCLE_TEST_DATABASE_URL="$(dsn billing_lifecycle)" \
    go test ./internal/http -count=1
)

echo "isolated Control lifecycle E2E passed"
