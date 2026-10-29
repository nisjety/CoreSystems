import { readFileSync } from 'node:fs';
import { join } from 'node:path';

describe('isolated Control lifecycle container harness contract', () => {
  const scriptsDirectory = join(process.cwd(), '..', 'scripts');

  it('uses disposable images, containers, and a unique network with cleanup', () => {
    const runner = readFileSync(
      join(scriptsDirectory, 'run-isolated-control-container-e2e.sh'),
      'utf8',
    );

    expect(runner).toContain('docker network create "$network"');
    expect(runner).toContain('trap cleanup EXIT');
    expect(runner).toContain("trap 'exit 130' INT");
    expect(runner).toContain("trap 'exit 143' TERM");
    expect(runner).not.toContain('trap cleanup EXIT INT TERM');
    expect(runner).toContain('"${driver_containers[@]}"');
    expect(runner).toContain('"${prefix}-driver-seed-and-converge"');
    expect(runner).toContain('"${prefix}-driver-membership-lifecycle"');
    expect(runner).toContain('"${prefix}-driver-partial-delete"');
    expect(runner).toContain('"${prefix}-driver-retry-delete"');
    expect(runner).toContain('docker build');
    expect(runner).toContain('--network "$network"');
    expect(runner).toContain('docker stop "$billing_container"');
    expect(runner).toContain('CONTROL_LIFECYCLE_PHASE=partial-delete');
    expect(runner).toContain('for _ in $(seq 1 90); do');
    expect(runner).toContain(
      'emit_bounded_container_logs "$postgres_container"',
    );
    expect(runner).not.toContain('docker compose');
    expect(runner).not.toContain('DATABASE_URL:-');
  });

  it('boots the real Auth image with disposable stable keys and validates JWKS readiness safely', () => {
    const runner = readFileSync(
      join(scriptsDirectory, 'run-isolated-control-container-e2e.sh'),
      'utf8',
    );

    expect(runner).toContain('auth_container="${prefix}-auth"');
    expect(runner).toContain('auth_key_dir="$(mktemp -d');
    expect(runner).toContain('openssl genpkey -algorithm RSA');
    expect(runner).toContain('openssl pkey');
    expect(runner).toContain('-pubout');
    expect(runner).toContain(
      'CONVEX_AUTH_PRIVATE_KEY_FILE=/run/control-auth-keys/private.pem',
    );
    expect(runner).toContain(
      'CONVEX_AUTH_PUBLIC_KEY_FILE=/run/control-auth-keys/public.pem',
    );
    expect(runner).toContain('NATS_USER=control-lifecycle');
    expect(runner).toContain('NATS_PASSWORD=$nats_password');
    expect(runner).not.toContain('NATS_ALLOW_TOKEN_FALLBACK=1');
    expect(runner).toContain('AUTH_GRPC_SERVICE_CREDENTIALS_FILE=');
    expect(runner).toContain('AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE=');
    expect(runner).toContain('USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE=');
    expect(runner).toContain(
      'USER_CORE_GRPC_TLS_CA_FILE=/run/control-auth-keys/user-grpc-ca.pem',
    );
    expect(runner).toContain('USER_SERVICE_GRPC_URL=user-core:50012');
    expect(runner).toContain('credentialId');
    expect(runner).toContain('auth:token:validate');
    expect(runner).toContain('auth-core-internal');
    expect(runner).toContain('nats:authenticate');
    expect(runner).toContain('user-core-grpc');
    expect(runner).not.toContain('"auth:admin"');
    expect(runner).not.toContain('auth:session:write');
    expect(runner).not.toContain('--env INTERNAL_API_KEY=');
    expect(runner).not.toContain('--env INTERNAL_SERVICE_SECRET=');
    expect(runner).toContain('wait_for_auth_jwks');
    expect(runner).toContain('/api/convex-auth/jwks');
    expect(runner).toContain("body.keys[0].kty !== 'RSA'");
    expect(runner).toContain('emit_bounded_container_logs');
    expect(runner).toContain('docker logs --tail 160');
    expect(runner).toContain('redact-container-logs.mjs');
    expect(runner).toContain('rm -rf "$auth_key_dir"');
  });

  it('runs the real Auth repair and outbox publishers across scoped HTTP boundaries', () => {
    const driver = readFileSync(
      join(
        scriptsDirectory,
        'fixtures',
        'control-lifecycle-container-driver.cjs',
      ),
      'utf8',
    );
    const seedPhase = driver.slice(
      driver.indexOf('async function seedRepairAndConverge'),
      driver.indexOf('async function runCanonicalMembershipLifecycle'),
    );

    expect(driver).toContain('invitation-acceptance-repair.js');
    expect(driver).toContain('organization-events.plugin.js');
    expect(driver).toContain('flushInvitationAcceptanceRepairs');
    expect(driver).toContain('flushOrganizationProjectionOutbox');
    expect(driver).toContain('flushOrganizationMembershipOutbox');
    expect(driver).toContain('flushOrganizationInvitationAuditOutbox');
    expect(driver).toContain('flushOrganizationMembershipAuditOutbox');
    expect(driver).toContain('flushOrganizationDeletionOutbox');
    expect(driver).toContain('setOrganizationEventPublisher');
    expect(driver).toContain('auditPublications');
    expect(driver).toContain(
      'concurrent Auth reconciliation may win the claim',
    );
    expect(driver).toContain('flushUntilDurable');
    expect(driver).toContain('projection outbox convergence');
    expect(driver).toContain('membership outbox convergence');
    expect(driver).toContain('projection_outbox.published_at IS NOT NULL');
    expect(driver).toContain('projectionDelivery[0].attempts >= 1');
    expect(driver).toContain('projectionDelivery[0].last_error');
    expect(seedPhase).not.toContain('{ published: true, attempts: 1 }');
    expect(seedPhase).toContain('ON CONFLICT (org_id) DO UPDATE');
    expect(seedPhase).toContain('billingAccountCardinality');
    expect(driver).not.toContain(
      'assert.equal(projectionsFlushed, 1 - projectionsBeforeSweep)',
    );
    expect(seedPhase).not.toContain(
      'assert.equal(await flushOrganizationProjectionOutbox(), 0)',
    );
    expect(seedPhase).not.toContain(
      'assert.equal(await flushOrganizationMembershipOutbox(), 0)',
    );
    expect(driver).toMatch(/case ["']partial-delete["']/);
    expect(driver).toMatch(/case ["']retry-delete["']/);
    expect(driver).toContain('process.exit(process.exitCode ?? 0)');
    expect(
      driver.indexOf('process.exit(process.exitCode ?? 0)'),
    ).toBeGreaterThan(driver.indexOf('authSQL.end({ timeout: 5 })'));
  });

  it('proves the canonical membership lifecycle with durable retries and ordering evidence', () => {
    const driver = readFileSync(
      join(
        scriptsDirectory,
        'fixtures',
        'control-lifecycle-container-driver.cjs',
      ),
      'utf8',
    );
    const runner = readFileSync(
      join(scriptsDirectory, 'run-isolated-control-container-e2e.sh'),
      'utf8',
    );

    expect(driver).toContain('runCanonicalMembershipLifecycle');
    expect(driver).toMatch(/case ["']membership-lifecycle["']/);
    expect(driver).toContain('InvitationAcceptanceController');
    expect(driver).toContain('canonicalAuthRequest');
    expect(driver.match(/["']\/organization\/invite-member["']/g)).toHaveLength(
      2,
    );
    expect(driver).toContain('/organization/update-member-role');
    expect(driver).toContain('/organization/remove-member');
    expect(driver).toContain('/organization/leave');
    expect(driver).toContain('USER_IS_ALREADY_INVITED_TO_THIS_ORGANIZATION');
    expect(driver).toContain('MEMBER_NOT_FOUND');
    expect(driver).toContain('duplicateInvitationEvidence');
    expect(driver).toContain('duplicateRoleEvidence');
    expect(driver).toContain('duplicateRemovalEvidence');
    expect(driver).toContain('nonAdminRoleChange');
    expect(driver).toContain('403');
    expect(driver).toContain('organization_membership_audit_outbox');
    expect(driver).toContain('organization_invitation_audit_outbox');
    expect(driver).toMatch(/desired_action: ["']remove["']/);
    expect(driver).toContain('auth_membership_projection_versions');
    expect(driver).toContain('attempts: 6');
    expect(runner).toContain('dragonfly_container');
    expect(runner).toContain('DRAGONFLY_URL');
    expect(runner).toContain('run_driver_phase membership-lifecycle');
  });

  it('runs the reviewed owner preflight only against disposable Postgres', () => {
    const runner = readFileSync(
      join(scriptsDirectory, 'run-isolated-control-lifecycle-e2e.sh'),
      'utf8',
    );
    expect(runner).toContain('owner-invariant-preflight.postgres.spec.ts');
    expect(runner).toContain('CONTROL_LIFECYCLE_FIXTURE_ID');
    expect(runner).toContain('coverage-summary.json');
    expect(runner).toContain('invitation-acceptance-repair.ts');
    expect(runner).toContain('coverage < 80');
    expect(runner).toContain('emit_bounded_container_logs "$container"');
    expect(runner).toContain('redact-container-logs.mjs');
    expect(runner).toContain('trap cleanup EXIT');
    expect(runner).toContain("trap 'exit 130' INT");
    expect(runner).toContain("trap 'exit 143' TERM");
    expect(runner).not.toContain('trap cleanup EXIT INT TERM');
  });
});
