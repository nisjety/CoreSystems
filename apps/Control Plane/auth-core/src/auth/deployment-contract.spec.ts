import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

describe('Control Plane deployment contract', () => {
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
    const worker = readFileSync(
      resolve(process.cwd(), 'src', 'auth', 'organization-events.plugin.ts'),
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
    expect(worker).toContain('ownerUserId: row.owner_user_id');
    expect(worker).toMatch(
      /WHERE organization_id = \$\{row\.organization_id\}[\s\S]*AND revision = \$\{row\.revision\}/,
    );
    expect(worker).toMatch(
      /UPDATE organization_projection_outbox[\s\S]*RETURNING organization_id/,
    );
    expect(worker).toContain('if (acknowledged.length !== 1) continue;');
    expect(worker).toMatch(
      /UPDATE organization_membership_outbox[\s\S]*RETURNING organization_id/,
    );
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
});
