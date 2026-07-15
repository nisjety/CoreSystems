import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

describe('Control Plane deployment contract', () => {
  it('never writes a credential-bearing NATS URL to Auth logs', () => {
    const main = readFileSync(resolve(process.cwd(), 'src', 'main.ts'), 'utf8');

    expect(main).not.toContain('NATS_URL: process.env.NATS_URL');
    expect(main).toContain("transport: 'nats'");
  });

  it('starts Auth with scoped gRPC principals and no legacy shared key', () => {
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );
    const authService = compose.match(
      /\n {2}auth-core:\n[\s\S]*?\n {2}user-core:\n/,
    )?.[0];

    expect(authService).toBeDefined();
    expect(authService).toContain('AUTH_GRPC_SERVICE_CREDENTIALS:');
    expect(authService).toContain('credentialId');
    expect(authService).toContain('GATEWAY_AUTH_GRPC_SERVICE_TOKEN');
    expect(authService).toContain('RETRIEVAL_AUTH_GRPC_SERVICE_TOKEN');
    expect(authService).not.toContain('auth:session:write');
    expect(authService).not.toContain('INTERNAL_API_KEY:');
    expect(authService).not.toContain('INTERNAL_SERVICE_SECRET:');
  });

  it('removes development Auth env/key mounts from the production render contract', () => {
    const production = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const authService = production.match(
      /\n {2}auth-core:\n[\s\S]*?\n {2}user-core:\n/,
    )?.[0];

    expect(authService).toBeDefined();
    expect(authService).toContain('env_file: !reset []');
    expect(authService).toContain('AUTH_GRPC_SERVICE_CREDENTIALS_FILE:');
    expect(authService).toContain('CONVEX_AUTH_PRIVATE_KEY_FILE:');
    expect(authService).toContain('CONVEX_AUTH_PUBLIC_KEY_FILE:');
    expect(authService).toContain('BEARER_TOKEN_ENABLED: "true"');
    expect(authService).toContain('/api/convex-auth/jwks');
    expect(authService).toContain("key.kty !== 'RSA'");
    expect(authService).toContain("key.alg !== 'RS256'");
    expect(authService).toContain("key.use !== 'sig'");
    expect(authService).toContain('volumes: !override');
    expect(authService).not.toContain('./auth-core/keys:/app/keys');
    expect(production).toContain('auth_grpc_service_credentials:');
    expect(production).toContain('auth_convex_private_key:');
    expect(production).toContain('auth_convex_public_key:');
  });

  it('pins User production verification to the same required Auth key and issuer', () => {
    const production = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const userService = production.match(
      /\n {2}user-core:\n[\s\S]*?\n {2}org-core:\n/,
    )?.[0];

    expect(userService).toBeDefined();
    expect(userService).toContain('volumes: !reset []');
    expect(userService).toContain(
      'AUTH_CORE_JWT_PUBLIC_KEY_FILE: /run/secrets/auth_convex_public_key',
    );
    expect(userService).toContain(
      'AUTH_CORE_ISSUER: ${AUTH_CORE_ISSUER:?AUTH_CORE_ISSUER is required in production}',
    );
    expect(userService).toContain('source: auth_convex_public_key');
    expect(userService).not.toContain('./auth-core/keys/convex-auth.pub');
    expect(userService).not.toContain('http://localhost:3011');
  });

  it('removes every service-local development env file from the release render', () => {
    const production = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const serviceNames = [
      'auth-core',
      'user-core',
      'org-core',
      'billing-core',
      'session-core',
      'audit-core',
    ];

    for (const [index, service] of serviceNames.entries()) {
      const next = serviceNames[index + 1];
      const expression = next
        ? new RegExp(`\\n {2}${service}:\\n[\\s\\S]*?\\n {2}${next}:\\n`)
        : /\n {2}audit-core:\n[\s\S]*?\n {2}lago-db:\n/;
      const block = production.match(expression)?.[0];
      expect(block).toBeDefined();
      expect(block).toContain('env_file: !reset []');
      expect(block).toContain('ALLOW_INSECURE_DEV_DEFAULTS: "0"');
    }
  });

  it('wires Auth and User gRPC through matching secret-manager tuples', () => {
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );
    const production = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const authService = compose.match(
      /\n {2}auth-core:\n[\s\S]*?\n {2}user-core:\n/,
    )?.[0];
    const userService = compose.match(
      /\n {2}user-core:\n[\s\S]*?\n {2}org-core:\n/,
    )?.[0];

    expect(authService).toContain('USER_CORE_GRPC_CLIENT_CREDENTIAL:');
    expect(userService).toContain('USER_CORE_GRPC_SERVICE_CREDENTIALS:');
    expect(userService).not.toContain('INTERNAL_API_KEY:');
    expect(userService).not.toContain('INTERNAL_SERVICE_SECRET:');
    expect(production).toContain(
      'USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE: /run/secrets/user_core_grpc_client_credential',
    );
    expect(production).toContain(
      'USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE: /run/secrets/user_core_grpc_service_credentials',
    );
    expect(production).toContain(
      'USER_CORE_GRPC_TLS_CA_FILE: /run/secrets/user_core_grpc_tls_ca',
    );
    expect(production).toContain(
      'USER_CORE_GRPC_TLS_CERT_FILE: /run/secrets/user_core_grpc_tls_certificate',
    );
    expect(production).toContain(
      'USER_CORE_GRPC_TLS_KEY_FILE: /run/secrets/user_core_grpc_tls_private_key',
    );
    expect(production).toContain('USER_SERVICE_GRPC_URL: user-core:50012');
  });

  it('removes legacy fleet keys from active Auth internal controllers and Session release config', () => {
    const production = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );
    const sessionService = compose.match(
      /\n {2}session-core:\n[\s\S]*?\n {2}audit-nats-provisioner:\n/,
    )?.[0];
    const sources = [
      resolve(process.cwd(), 'src', 'internal', 'internal-oauth.controller.ts'),
      resolve(
        process.cwd(),
        'src',
        'internal',
        'internal-agent-signup.controller.ts',
      ),
      resolve(process.cwd(), 'src', 'auth', 'nats-auth.controller.ts'),
      resolve(process.cwd(), 'src', 'nats', 'direct-nats.service.ts'),
    ].map((path) => readFileSync(path, 'utf8'));

    expect(sessionService).toBeDefined();
    expect(sessionService).not.toContain('INTERNAL_API_KEY:');
    expect(sessionService).not.toContain('INTERNAL_SERVICE_SECRET:');
    expect(production).not.toContain('INTERNAL_API_KEY:');
    expect(production).not.toContain('INTERNAL_SERVICE_SECRET:');
    for (const source of sources) {
      expect(source).not.toContain('process.env.INTERNAL_API_KEY');
      expect(source).not.toContain('process.env.INTERNAL_SERVICE_SECRET');
    }
  });

  it('routes Auth organization deletion to Billing Core HTTP port 3014', () => {
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );

    expect(compose).toMatch(/BILLING_CORE_URL:\s*http:\/\/billing-core:3014\b/);
    expect(compose).not.toMatch(
      /BILLING_CORE_URL:\s*http:\/\/billing-core:3017\b/,
    );
  });

  it('advertises the browser-facing Velion origin for auth callbacks and invitation links', () => {
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );

    expect(compose).toMatch(
      /BETTER_AUTH_URL:\s*\$\{VELION_PUBLIC_ORIGIN:-http:\/\/localhost:5173\}/,
    );
    expect(compose).toMatch(
      /FRONTEND_URL:\s*\$\{VELION_PUBLIC_ORIGIN:-http:\/\/localhost:5173\}/,
    );
  });

  it('applies Auth SQL migrations atomically, once, and in bootstrap order', () => {
    const entrypoint = readFileSync(
      resolve(process.cwd(), 'docker-entrypoint.sh'),
      'utf8',
    );

    expect(entrypoint).toContain('ON_ERROR_STOP=1');
    expect(entrypoint).toContain('auth_schema_migrations');
    expect(entrypoint).toContain('pg_advisory_xact_lock');
    expect(entrypoint).toContain('sha256sum');
    expect(entrypoint).toContain('AS checksum_mismatch \\gset');
    expect(entrypoint).toContain('\\if :checksum_mismatch');
    expect(entrypoint).toContain('\\quit 3');
    expect(entrypoint).not.toMatch(/THEN\s+1\s*\/\s*0/i);
    expect(entrypoint.indexOf('init_better_auth.sql')).toBeLessThan(
      entrypoint.indexOf('[0-9][0-9][0-9]_*.sql'),
    );
    expect(entrypoint).not.toContain('drizzle-kit push || true');
    expect(entrypoint).toContain('POSTGRES_READY_MAX_ATTEMPTS');
    expect(entrypoint).toContain('Postgres readiness timed out');
    expect(entrypoint).not.toContain('via $PG_ADMIN_URL');
  });

  it('fails release startup on ownerless organizations or historical accepted-invite gaps', () => {
    const entrypoint = readFileSync(
      resolve(process.cwd(), 'docker-entrypoint.sh'),
      'utf8',
    );
    const dockerfile = readFileSync(
      resolve(process.cwd(), 'Dockerfile'),
      'utf8',
    );
    const preflightPath = resolve(
      process.cwd(),
      'scripts',
      'validate-lifecycle-preflight.sh',
    );

    expect(existsSync(preflightPath)).toBe(true);
    const preflight = readFileSync(preflightPath, 'utf8');
    expect(preflight).toContain('owner_invariant_preflight_report');
    expect(preflight).toContain('accepted_invitation_membership_gap_report');
    expect(preflight).toContain('OWNER_INVARIANT_PREFLIGHT_FAILED');
    expect(preflight).toContain('INVITATION_MEMBERSHIP_PREFLIGHT_FAILED');
    expect(dockerfile).toContain('validate-lifecycle-preflight.sh');
    expect(entrypoint).toContain('/app/validate-lifecycle-preflight.sh');
    expect(
      entrypoint.indexOf('/app/validate-lifecycle-preflight.sh'),
    ).toBeLessThan(entrypoint.indexOf('exec "$@"'));
  });

  it('adds the Better Auth invitation creation timestamp without rewriting bootstrap history', () => {
    const schema = readFileSync(
      resolve(process.cwd(), 'src', 'db', 'schema.ts'),
      'utf8',
    );
    const migrationPath = resolve(
      process.cwd(),
      'migrations',
      '021_invitation_created_at.sql',
    );

    const invitationSchema = schema.match(
      /export const invitation = pgTable\('invitation', \{[\s\S]*?\n\}\);/,
    )?.[0];
    expect(invitationSchema).toContain(
      "createdAt: timestamp('created_at').defaultNow().notNull()",
    );
    expect(existsSync(migrationPath)).toBe(true);
    const migration = readFileSync(migrationPath, 'utf8');
    expect(migration).toMatch(
      /ALTER TABLE invitation\s+ADD COLUMN IF NOT EXISTS created_at TIMESTAMP/,
    );
    expect(migration).toMatch(
      /UPDATE invitation[\s\S]*SET created_at = NOW\(\)[\s\S]*WHERE created_at IS NULL/,
    );
    expect(migration).toContain('ALTER COLUMN created_at SET DEFAULT NOW()');
    expect(migration).toContain('ALTER COLUMN created_at SET NOT NULL');
  });

  it('builds with the audited pnpm workspace policy and CLI patch', () => {
    const dockerfile = readFileSync(
      resolve(process.cwd(), 'Dockerfile'),
      'utf8',
    );

    expect(dockerfile).toContain('corepack prepare pnpm@11.3.0 --activate');
    expect(dockerfile).toContain(
      'COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./',
    );
    expect(dockerfile).toContain('COPY patches ./patches');
    expect(dockerfile.indexOf('COPY patches ./patches')).toBeLessThan(
      dockerfile.indexOf('pnpm install --frozen-lockfile'),
    );
    expect(dockerfile).toContain(
      '--mount=type=cache,target=/root/.local/share/pnpm/store',
    );
  });

  it('re-enqueues organization updates and acknowledges only the claimed revision', () => {
    const migration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '015_organization_projection_outbox.sql',
      ),
      'utf8',
    );
    const deletionMigration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '024_revisioned_cross_plane_projection.sql',
      ),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );
    const publisher = readFileSync(
      resolve(process.cwd(), 'src', 'internal', 'auth-event.publisher.ts'),
      'utf8',
    );

    expect(migration).toMatch(
      /organization_projection_outbox[\s\S]*revision BIGINT NOT NULL DEFAULT 1/,
    );
    expect(migration).toMatch(
      /revision\s*=\s*organization_projection_outbox\.revision\s*\+\s*1/,
    );
    expect(migration).toMatch(/published_at\s*=\s*NULL/);
    expect(worker).toContain('revision: OutboxRevision;');
    expect(worker).toContain('normalizeOutboxRevision(row.revision)');
    expect(worker).toContain('/internal/orgs/');
    expect(worker).toContain('/reconcile');
    expect(worker).toContain('ownerUserId: projection.owner_user_id');
    expect(worker).toMatch(
      /WHERE organization_id = \$\{row\.organization_id\}[\s\S]*AND revision = \$\{row\.revision\}/,
    );
    expect(worker).toMatch(
      /UPDATE organization_projection_outbox[\s\S]*RETURNING organization_id/,
    );
    expect(worker).toContain('if (!acknowledged) continue;');
    expect(worker).toMatch(
      /UPDATE organization_membership_outbox[\s\S]*RETURNING organization_id/,
    );
    expect(worker).toContain('publishOrganizationProjection');
    expect(worker).toContain('publishOrganizationMembershipProjection');
    expect(worker).toContain('publishOrganizationDeletionProjection');
    expect(publisher).toContain('aqencia.controlplane.org.changed');
    expect(publisher).toContain('aqencia.controlplane.org.member_changed');
    expect(worker).toContain('revision,');
    expect(deletionMigration).toContain('event_synced_at');
  });

  it('commits each membership mutation, actor, audit, and idempotent result atomically', () => {
    const auditMigration = readFileSync(
      resolve(process.cwd(), 'migrations', '019_membership_audit_outbox.sql'),
      'utf8',
    );
    const mutationMigration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '027_membership_mutation_intents.sql',
      ),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    for (const required of [
      'organization_membership_audit_outbox',
      'published_at',
      'attempts',
      'actor_classification',
      "'member_added'",
      "'role_changed'",
      "'member_removed'",
    ]) {
      expect(auditMigration).toContain(required);
    }
    expect(auditMigration).toMatch(
      /PRIMARY KEY \(organization_id, user_id, revision\)/,
    );
    expect(worker).toContain('candidate.published_at IS NULL');
    expect(worker).toContain("candidate.actor_classification <> 'pending'");
    expect(worker).toContain('WITH exact_transition AS');
    expect(worker).toContain('AND audit.member_id = ${member.id}');
    expect(worker).not.toContain('projection.member_id');
    expect(worker).toContain('prior.published_at IS NULL');
    expect(auditMigration).toContain('invitation_id TEXT');
    expect(worker).toContain('candidate.invitation_id');
    expect(worker).not.toMatch(
      /JOIN "user" invited_user[\s\S]*invitation_audit\.invitee_email/,
    );
    expect(worker).toContain('membershipAuditIdempotencyKey(audit)');
    expect(worker).toMatch(
      /UPDATE organization_membership_audit_outbox[\s\S]*published_at = NOW\(\)[\s\S]*AND revision = \$\{revision\}[\s\S]*RETURNING organization_id/,
    );
    expect(worker).not.toContain('@better-auth/core/context');
    expect(worker).not.toContain('verifiedActorUserId');
    expect(worker).toContain('membershipAuditBearerSessionToken');
    expect(worker).toContain('context.getSignedCookie');
    expect(worker).toContain('internalAdapter.findSession(sessionToken)');
    expect(worker).toContain('captureMembershipAuditOperationResult');
    expect(worker).toContain("captureResult.status === 'not_found'");
    expect(worker).toContain("captureResult.status === 'bad_request'");
    expect(worker).toContain('applyMembershipMutation');
    expect(worker).toContain('return context.json(response)');
    expect(worker).not.toContain('expirePendingMembershipMutationIntents');
    expect(mutationMigration).toContain(
      'organization_membership_mutation_operation',
    );
    expect(mutationMigration).toContain('apply_membership_mutation');
    expect(mutationMigration).toContain(
      "set_config('app.membership_operation_id'",
    );
    expect(mutationMigration).toContain(
      'zz_membership_mutation_operation_trigger',
    );
    expect(mutationMigration).toMatch(
      /UPDATE organization_membership_audit_outbox audit[\s\S]*actor_classification = 'verified_user'/,
    );
    expect(mutationMigration).toContain(
      "RAISE EXCEPTION 'verified membership mutation operation is required'",
    );
  });

  it('retains a GDPR membership removal only until its exact projection delivery is acknowledged', () => {
    const migration = readFileSync(
      resolve(process.cwd(), 'migrations', '023_gdpr_outbox_erasure.sql'),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    expect(migration).toContain('gdpr_erasure_requested_at');
    expect(migration).toContain('purge_completed_gdpr_membership_outbox');
    expect(worker).toMatch(
      /flushOrganizationMembershipOutbox[\s\S]*purgeCompletedGdprMembershipOutbox\(\)/,
    );
    expect(worker).toMatch(
      /synced_at = NOW\(\)[\s\S]*purgeCompletedGdprMembershipOutbox\(\)/,
    );
  });

  it('bounds missing membership actor recovery and never fabricates a user actor', () => {
    const migration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '022_membership_audit_actor_recovery.sql',
      ),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );
    const scheduler = readFileSync(
      resolve(
        process.cwd(),
        'src',
        'services',
        'orphan-organization-cleanup.service.ts',
      ),
      'utf8',
    );

    expect(migration).toContain('actor_resolution_not_before');
    expect(migration).toContain('actor_resolution_attempts');
    expect(migration).toContain('actor_resolution_dead_lettered_at');
    expect(migration).toContain("THEN 'unresolved'");
    expect(migration).toContain('actor_user_id IS NULL');
    expect(migration).toContain('recover_pending_membership_audit_actors');
    expect(worker).toContain('recoverPendingMembershipAuditActors');
    expect(worker).toContain('recover_pending_membership_audit_actors(200, 3)');
    expect(worker).toContain("| 'unresolved'");
    expect(worker).toContain(
      "actor_resolution_dead_lettered_at < NOW() - INTERVAL '5 minutes'",
    );
    expect(
      scheduler.indexOf('recoverPendingMembershipAuditActors()'),
    ).toBeLessThan(
      scheduler.indexOf('flushOrganizationMembershipAuditOutbox()'),
    );
  });

  it('erases Auth outbox PII through a forward-only user lifecycle trigger', () => {
    const migration = readFileSync(
      resolve(process.cwd(), 'migrations', '023_gdpr_outbox_erasure.sql'),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    expect(migration).toContain('subject_erased_at');
    expect(migration).toContain('actor_erased_at');
    expect(migration).toContain('inviter_erased_at');
    expect(migration).toContain('invitee_erased_at');
    expect(migration).toContain("'erased_actor'");
    expect(migration).toContain('erase_auth_outbox_pii_on_user_change');
    expect(migration).toContain('SECURITY DEFINER');
    expect(migration).toContain('quote_ident(TG_TABLE_SCHEMA)');
    expect(migration).toContain('pg_trigger_depth() > 1');
    expect(migration).toContain(
      "target_user_id || ':' || LOWER(BTRIM(target_email))",
    );
    expect(migration).toMatch(
      /DELETE FROM auth_identity_event_outbox\s+WHERE user_id = target_user_id/,
    );
    expect(migration).toMatch(
      /AFTER DELETE OR UPDATE OF name, email, banned, ban_reason ON "user"/,
    );
    expect(worker).toContain("| 'erased_actor';");
  });

  it('fences every PII-bearing outbox claim against concurrent GDPR erasure', () => {
    const migrationPath = resolve(
      process.cwd(),
      'migrations',
      '025_gdpr_outbox_publish_fence.sql',
    );
    expect(existsSync(migrationPath)).toBe(true);
    const migration = readFileSync(migrationPath, 'utf8');
    const organizationWorker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );
    const identityWorker = readFileSync(
      resolve(process.cwd(), 'src', 'internal', 'auth-identity-outbox.ts'),
      'utf8',
    );

    for (const table of [
      'organization_projection_outbox',
      'organization_membership_outbox',
      'organization_membership_audit_outbox',
      'organization_invitation_audit_outbox',
      'auth_identity_event_outbox',
    ]) {
      expect(migration).toMatch(
        new RegExp(`ALTER TABLE ${table}[\\s\\S]*claim_token TEXT`),
      );
    }
    expect(migration).toContain('pg_advisory_xact_lock');
    expect(migration).toContain("'auth-gdpr:email:'");
    expect(migration).toContain("'auth-gdpr:user:'");
    expect(migration).toContain('BEFORE DELETE OR UPDATE OF');
    expect(migration).toContain('clear_auth_outbox_claims_after_erasure');
    for (const worker of [organizationWorker, identityWorker]) {
      expect(worker).toContain('claim_token');
      expect(worker).toContain('lockAuthGdprIdentities');
      expect(worker).toContain('FOR UPDATE');
    }
  });

  it('requires verified email for invitation acceptance unless explicitly disabled', () => {
    const authConfig = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'auth.ts'),
      'utf8',
    );
    expect(authConfig).toContain(
      "process.env.ORG_REQUIRE_EMAIL_VERIFICATION !== 'false'",
    );
  });

  it('enables real database transactions for multi-step Better Auth mutations', () => {
    const authConfig = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'auth.ts'),
      'utf8',
    );
    expect(authConfig).toMatch(
      /drizzleAdapter\(db,\s*\{[\s\S]*?provider:\s*'pg',[\s\S]*?transaction:\s*true/,
    );
    const rateLimitBlock = authConfig.match(
      /rateLimit:\s*\{[\s\S]*?\n\s*\},\n\n\s*\/\/ Advanced IP address detection/,
    )?.[0];
    expect(rateLimitBlock).toMatch(
      /'\/organization\/accept-invitation':\s*\{[\s\S]*?RATE_LIMIT_INVITATION_ACCEPT_WINDOW[\s\S]*?RATE_LIMIT_INVITATION_ACCEPT_MAX/,
    );
    const socialProvidersStart = authConfig.indexOf('socialProviders: {');
    const authOptionsEnd = authConfig.indexOf(
      '\n};\n\nexport const auth = betterAuth(authOptions);',
      socialProvidersStart,
    );
    expect(socialProvidersStart).toBeGreaterThan(-1);
    expect(authOptionsEnd).toBeGreaterThan(socialProvidersStart);
    const socialProvidersBlock = authConfig.slice(
      socialProvidersStart,
      authOptionsEnd,
    );
    expect(socialProvidersBlock).not.toContain(
      '/organization/accept-invitation',
    );
    expect(authConfig).toContain('verifyInvitationAcceptanceInternalMarker');
    expect(authConfig).toContain("'/organization/accept-invitation'");
    expect(authConfig).toContain("'x-velion-invitation-acceptance'");
  });

  it('never includes NATS credential material in bootstrap diagnostics', () => {
    const bootstrap = readFileSync(
      resolve(process.cwd(), 'src', 'main.ts'),
      'utf8',
    );

    expect(bootstrap).not.toMatch(/natsToken\.substring/i);
    expect(bootstrap).not.toMatch(/NATS_TOKEN:\s*natsToken/i);
  });

  it('tracks billing and Org deletion reconciliation independently', () => {
    const migration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '015_organization_projection_outbox.sql',
      ),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    expect(migration).toContain('billing_synced_at TIMESTAMPTZ');
    expect(migration).toContain('org_synced_at TIMESTAMPTZ');
    expect(worker).toContain('billing_synced_at');
    expect(worker).toContain('org_synced_at');
  });

  it('uses distinct audience-bound service principals for Org and Billing reconciliation', () => {
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    expect(worker).toContain('ORG_CORE_SERVICE_TOKEN');
    expect(worker).toContain('BILLING_CORE_SERVICE_TOKEN');
    expect(worker).toContain("'x-service-id': 'auth-core'");
    expect(worker).toContain("'x-service-token': config.token");
    expect(worker).not.toContain("'x-internal-api-key'");
    expect(worker).not.toContain('process.env.INTERNAL_API_KEY');
    expect(worker).not.toContain('process.env.INTERNAL_SERVICE_SECRET');

    const initializer = readFileSync(
      resolve(process.cwd(), 'src', 'internal', 'auth-service.initializer.ts'),
      'utf8',
    );
    expect(initializer).toContain(
      'validateOrganizationReconciliationCredentials()',
    );
    expect(
      initializer.indexOf('validateOrganizationReconciliationCredentials()'),
    ).toBeLessThan(initializer.indexOf('try {'));
  });

  it('does not register the legacy response-interception membership publisher', () => {
    const appModule = readFileSync(
      resolve(process.cwd(), 'src', 'app.module.ts'),
      'utf8',
    );
    const bootstrap = readFileSync(
      resolve(process.cwd(), 'src', 'main.ts'),
      'utf8',
    );
    expect(appModule).not.toContain('OrganizationEventMiddleware');
    expect(bootstrap).not.toContain('OrganizationEventMiddleware');
    expect(bootstrap).not.toContain('orgMiddleware.use');
  });

  it('uses the canonical projection outbox as the only organization-create producer', () => {
    const router = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'orpc-router.ts'),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
      'utf8',
    );

    expect(router).not.toContain('publishOrganizationCreated');
    expect(worker).toMatch(
      /publishOrganizationProjection[\s\S]*UPDATE organization_projection_outbox[\s\S]*published_at = NOW\(\)/,
    );
    expect(worker).toContain(
      '`organization:${projection.organization_id}:${revision}:upsert`',
    );
  });

  it('commits identity lifecycle events to a durable outbox before asynchronous delivery', () => {
    const migration = readFileSync(
      resolve(
        process.cwd(),
        'migrations',
        '020_auth_identity_event_outbox.sql',
      ),
      'utf8',
    );
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'internal', 'auth-identity-outbox.ts'),
      'utf8',
    );

    for (const required of [
      'auth_identity_event_outbox',
      "'user_registered'",
      "'provider_linked'",
      'published_at',
      'dead_lettered_at',
      'attempts',
    ]) {
      expect(migration).toContain(required);
    }
    expect(migration).toContain("'user:' || NEW.id || ':registered'");
    expect(migration).toContain("'account:' || NEW.id || ':provider_linked'");
    expect(migration).toMatch(
      /UPDATE auth_identity_event_outbox[\s\S]*jsonb_set[\s\S]*NEW\.provider_id[\s\S]*event_type = 'user_registered'/,
    );
    expect(worker).toContain('FOR UPDATE SKIP LOCKED');
    expect(worker).toContain('flushAuthIdentityEventOutbox');
    expect(worker).toContain('row.event_id');
    expect(worker).toMatch(
      /event_type <> 'provider_linked'[\s\S]*dependency\.event_type = 'user_registered'[\s\S]*dependency\.published_at IS NOT NULL/,
    );
    expect(worker).toMatch(
      /publishUserRegistered[\s\S]*UPDATE auth_identity_event_outbox[\s\S]*published_at = NOW\(\)/,
    );
    expect(worker).toMatch(
      /publishUserProviderLinked[\s\S]*UPDATE auth_identity_event_outbox[\s\S]*published_at = NOW\(\)/,
    );
  });

  it('wires a dedicated Auth-canonical membership reader for Application reconciliation', () => {
    const compose = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.yml'),
      'utf8',
    );
    const controller = readFileSync(
      resolve(
        process.cwd(),
        'src',
        'auth',
        'membership-authority.controller.ts',
      ),
      'utf8',
    );

    expect(compose).toContain(
      'APPLICATION_RECONCILER_AUTH_TOKEN: ${APPLICATION_RECONCILER_AUTH_TOKEN:?APPLICATION_RECONCILER_AUTH_TOKEN is required}',
    );
    expect(controller).toContain(
      "callerServiceID !== 'application-reconciler'",
    );
    expect(controller).not.toContain("@Headers('x-internal-api-key')");
  });

  it('removes every Control Plane host-published port in release mode', () => {
    const overlay = readFileSync(
      resolve(process.cwd(), '..', 'docker-compose.production.yml'),
      'utf8',
    );
    const blocks = overlay.split(/^ {2}(?=[a-z0-9-]+:\s*$)/m);
    const hostPublishedServices = [
      'controlplane-postgres',
      'controlplane-dragonfly',
      'controlplane-nats',
      'control-shared-nats',
      'auth-core',
      'org-core',
      'billing-core',
      'session-core',
      'audit-core',
      'lago-db',
      'lago-dragonfly',
      'lago-api',
      'lago-front',
      'controlplane-prometheus',
      'controlplane-grafana',
    ];

    for (const service of hostPublishedServices) {
      const block = blocks.find((candidate) =>
        candidate.startsWith(`${service}:`),
      );
      expect(block).toBeDefined();
      expect(block).toContain('ports: !reset []');
    }
    expect(overlay).not.toContain('ALLOW_INSECURE_DEV_DEFAULTS: "1"');
  });
});
