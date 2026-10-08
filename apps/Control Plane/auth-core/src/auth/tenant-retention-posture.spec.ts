import {
  BadRequestException,
  ForbiddenException,
  Logger,
  ServiceUnavailableException,
} from '@nestjs/common';

// Nothing below is faked except the two modules the plane controller pulls in
// for its unrelated interactive path, org-core's HTTP answers, and the NATS
// audit publisher. The principal policy, the retention resolver, both token
// controllers and the RS256 signing are the real ones, so what these cases
// prove is what a worker minting a token actually gets.
jest.mock('./auth', () => ({ auth: { api: { getSession: jest.fn() } } }));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: jest.fn(),
}));

import {
  ConvexTokenService,
  TENANT_PRINCIPAL_TOKEN_TTL_SECONDS,
} from './convex-token.service';
import {
  RETENTION_STALE_NON_ZDR_MAX_AGE_MS,
  RetentionPostureUnavailableError,
  resetInteractiveRetentionCacheForTests,
  resolveTenantServiceRetentionPosture,
} from './interactive-retention-policy';
import { ModelPlaneTokenController } from './model-plane-token.controller';
import { PlaneTokenController } from './plane-token.controller';

const ORG_CORE_TOKEN = 'tenant-spec-org-core-service-token-0123456789abcdef';

type OrgPosture = { zdrIntent: boolean; entitled: boolean };

type OrgCore = {
  up: boolean;
  postures: Record<string, OrgPosture>;
  requests: Array<{ method: string; path: string }>;
};

function jsonResponse(status: number, body: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: () => Promise.resolve(body),
  } as unknown as Response;
}

/**
 * org-core as the resolver meets it: service authentication on every route (no
 * credential, no answer), the retention-posture route, and org-core's own
 * "organization not found" 404. The request shape is the one pinned by
 * src/auth/fixtures/auth-core-org-core-retention-posture.contract.json.
 */
function installOrgCore(state: OrgCore): void {
  jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const url = new URL(String(input));
      const headers = new Headers(init?.headers);
      state.requests.push({
        method: (init?.method ?? 'GET').toUpperCase(),
        path: url.pathname,
      });
      if (!state.up) return Promise.reject(new TypeError('fetch failed'));
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== ORG_CORE_TOKEN
      ) {
        return Promise.resolve(jsonResponse(401, { error: 'unauthorized' }));
      }
      const match = /^\/internal\/orgs\/([^/]+)\/retention-posture$/.exec(
        url.pathname,
      );
      if (!match) {
        return Promise.resolve(jsonResponse(404, { error: 'page not found' }));
      }
      const posture = state.postures[decodeURIComponent(match[1])];
      return Promise.resolve(
        posture
          ? jsonResponse(200, posture)
          : jsonResponse(404, { error: 'organization not found' }),
      );
    });
}

function payloadOf(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

const originalEnv = { ...process.env };
let orgCore: OrgCore;

function configureEnvironment(): void {
  process.env.NODE_ENV = 'test';
  for (const name of keyEnvNames) delete process.env[name];
  process.env.ORG_SERVICE_URL = 'http://org-core.tenant-spec.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_TOKEN;
}

function silenceLogs(): void {
  jest.spyOn(Logger.prototype, 'error').mockImplementation();
  jest.spyOn(Logger.prototype, 'warn').mockImplementation();
  jest.spyOn(Logger.prototype, 'log').mockImplementation();
}

beforeEach(() => {
  configureEnvironment();
  silenceLogs();
  resetInteractiveRetentionCacheForTests();
  orgCore = {
    up: true,
    postures: {
      'org-zdr': { zdrIntent: true, entitled: true },
      'org-plain': { zdrIntent: false, entitled: true },
      'org-lapsed-plan': { zdrIntent: true, entitled: false },
    },
    requests: [],
  };
  installOrgCore(orgCore);
});

afterEach(() => {
  jest.useRealTimers();
  jest.restoreAllMocks();
  process.env = { ...originalEnv };
});

const tenantPrincipal = (orgId: string) => ({
  retention: 'tenant' as const,
  orgId,
  serviceId: 'tenant-worker',
});

describe('resolveTenantServiceRetentionPosture', () => {
  it('resolves an org with stored ZDR intent and plan entitlement to zdr:true, stamped as the org policy under the service authority', async () => {
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-zdr')),
    ).resolves.toEqual({
      zdr: true,
      authority: 'service-principal-policy',
      source: 'tenant-policy',
    });
    expect(orgCore.requests).toEqual([
      {
        method: 'GET',
        path: '/internal/orgs/org-zdr/retention-posture',
      },
    ]);
  });

  it.each([
    ['org-plain', 'no ZDR intent'],
    ['org-lapsed-plan', 'ZDR intent but a plan that no longer includes it'],
    ['org-unknown-to-org-core', 'no org-core record at all'],
  ])('resolves %s (%s) to zdr:false', async (orgId) => {
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal(orgId)),
    ).resolves.toEqual({
      zdr: false,
      authority: 'service-principal-policy',
      source: 'tenant-policy',
    });
  });

  it('answers 503 retention_posture_unavailable, never a guessed posture, when org-core is unreachable and nothing is remembered', async () => {
    orgCore.up = false;

    const attempt = resolveTenantServiceRetentionPosture(
      tenantPrincipal('org-zdr'),
    );

    await expect(attempt).rejects.toBeInstanceOf(
      RetentionPostureUnavailableError,
    );
    await expect(attempt).rejects.toBeInstanceOf(ServiceUnavailableException);
    await expect(attempt).rejects.toMatchObject({
      reason: 'org_core_unreachable',
      response: { statusCode: 503, error: 'retention_posture_unavailable' },
    });
  });

  it('answers 503 when org-core rejects Auth Core credential: an authentication failure is not a posture either', async () => {
    process.env.ORG_CORE_SERVICE_TOKEN =
      'rotated-away-org-core-token-0123456789abcdef';

    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-zdr')),
    ).rejects.toMatchObject({ reason: 'org_core_credential_rejected' });
  });

  it('answers 503 when org-core answers with a body that is not a posture', async () => {
    jest.restoreAllMocks();
    silenceLogs();
    jest
      .spyOn(global, 'fetch')
      .mockResolvedValue(jsonResponse(200, { zdrIntent: 'yes' }));

    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-zdr')),
    ).rejects.toMatchObject({ reason: 'org_core_malformed_response' });
  });

  it('answers 503 for an org id it cannot even ask org-core about', async () => {
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('../org-zdr')),
    ).rejects.toMatchObject({ reason: 'invalid_organization_id' });
    expect(orgCore.requests).toEqual([]);
  });

  it('keeps a remembered ZDR org zero-retention through an org-core outage (a known ZDR org is never downgraded by an outage)', async () => {
    jest.useFakeTimers({ now: new Date('2026-10-03T10:00:00.000Z') });
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-zdr')),
    ).resolves.toMatchObject({ zdr: true });

    orgCore.up = false;
    jest.advanceTimersByTime(60 * 60 * 1000);

    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-zdr')),
    ).resolves.toMatchObject({ zdr: true, source: 'tenant-policy' });
  });

  it('stops serving a remembered non-ZDR posture once it is older than the stale window, then answers 503', async () => {
    // A stale `false` is the one that can leak (the org may have turned ZDR on
    // since), so it is bounded far tighter than a stale `true`.
    jest.useFakeTimers({ now: new Date('2026-10-03T10:00:00.000Z') });
    await resolveTenantServiceRetentionPosture(tenantPrincipal('org-plain'));

    orgCore.up = false;
    jest.advanceTimersByTime(RETENTION_STALE_NON_ZDR_MAX_AGE_MS - 1_000);
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-plain')),
    ).resolves.toMatchObject({ zdr: false });

    jest.advanceTimersByTime(2_000);
    await expect(
      resolveTenantServiceRetentionPosture(tenantPrincipal('org-plain')),
    ).rejects.toBeInstanceOf(RetentionPostureUnavailableError);
  });

  it.each(['zdr', 'persistent'] as const)(
    'refuses to resolve for a principal whose registry retention is %s: only a tenant principal takes the org posture',
    async (retention) => {
      await expect(
        resolveTenantServiceRetentionPosture({
          retention,
          orgId: 'org-zdr',
          serviceId: 'not-a-tenant-worker',
        } as never),
      ).rejects.toThrow(/tenant/i);
      expect(orgCore.requests).toEqual([]);
    },
  );
});

describe('token lifetime for tenant principals (the staleness bound)', () => {
  const claims = (
    retentionPosture: { zdr: boolean; source?: 'tenant-policy' },
    overrides: Record<string, unknown> = {},
  ) => ({
    userId: 'service:tenant-worker',
    orgId: 'org-zdr',
    scopes: ['inference:invoke'],
    principalType: 'service' as const,
    serviceId: 'tenant-worker',
    reason: 'contextualise indexed chunks',
    retentionPosture: {
      authority: 'service-principal-policy' as const,
      ...retentionPosture,
    },
    ...overrides,
  });

  it('pins the cap at 60 seconds', () => {
    expect(TENANT_PRINCIPAL_TOKEN_TTL_SECONDS).toBe(60);
  });

  it('caps a plane token minted from the org posture at 60 seconds, below the 300 second audience default', () => {
    const service = new ConvexTokenService();

    const tenant = service.issuePlaneToken(
      'inference-core',
      claims({ zdr: true, source: 'tenant-policy' }),
    );
    const registry = service.issuePlaneToken(
      'inference-core',
      claims({ zdr: true }),
    );

    expect(tenant.expiresInSeconds).toBe(60);
    const tenantPayload = payloadOf(tenant.token);
    expect((tenantPayload.exp as number) - (tenantPayload.iat as number)).toBe(
      60,
    );
    expect(registry.expiresInSeconds).toBe(300);
  });

  it('caps it even when the audience TTL is configured far longer', () => {
    process.env.PLANE_TOKEN_TTL_INFERENCE_CORE_SECONDS = '3600';
    const service = new ConvexTokenService();

    expect(
      service.issuePlaneToken(
        'inference-core',
        claims({ zdr: false, source: 'tenant-policy' }),
      ).expiresInSeconds,
    ).toBe(60);
    expect(
      service.issuePlaneToken('inference-core', claims({ zdr: false }))
        .expiresInSeconds,
    ).toBe(3600);
  });

  it('lets a shorter caller override win but never lengthens a tenant token', () => {
    const service = new ConvexTokenService();

    expect(
      service.issuePlaneToken(
        'inference-core',
        claims(
          { zdr: true, source: 'tenant-policy' },
          { ttlSecondsOverride: 45 },
        ),
      ).expiresInSeconds,
    ).toBe(45);
    expect(
      service.issuePlaneToken(
        'inference-core',
        claims(
          { zdr: true, source: 'tenant-policy' },
          { ttlSecondsOverride: 200 },
        ),
      ).expiresInSeconds,
    ).toBe(60);
  });

  it('caps a Model Plane token the same way, below its 900 second default', () => {
    const service = new ConvexTokenService();

    const tenant = service.issueModelPlaneToken(
      claims({ zdr: true, source: 'tenant-policy' }),
    );
    const registry = service.issueModelPlaneToken(claims({ zdr: true }));

    expect(tenant.expiresInSeconds).toBe(60);
    const payload = payloadOf(tenant.token);
    expect((payload.exp as number) - (payload.iat as number)).toBe(60);
    expect(registry.expiresInSeconds).toBe(900);
  });

  it('carries the resolved posture into the zdr claim, true or false', () => {
    const service = new ConvexTokenService();

    expect(
      payloadOf(
        service.issuePlaneToken(
          'inference-core',
          claims({ zdr: true, source: 'tenant-policy' }),
        ).token,
      ).zdr,
    ).toBe(true);
    expect(
      payloadOf(
        service.issuePlaneToken(
          'inference-core',
          claims({ zdr: false, source: 'tenant-policy' }),
        ).token,
      ).zdr,
    ).toBe(false);
  });

  it('ignores the tenant marker on a user token: it neither relaxes the posture nor shortens the lifetime', () => {
    const service = new ConvexTokenService();

    const bundle = service.issuePlaneToken('inference-core', {
      userId: 'user-a',
      orgId: 'org-zdr',
      retentionPosture: {
        zdr: true,
        authority: 'service-principal-policy',
        source: 'tenant-policy',
      },
    });

    expect(payloadOf(bundle.token).zdr).toBe(false);
    expect(bundle.expiresInSeconds).toBe(300);
  });
});

type Endpoint = {
  name: string;
  audience: string;
  scopes: string[];
  /** The token service method this endpoint signs with. */
  signer: 'issuePlaneToken' | 'issueModelPlaneToken';
  build: (
    tokens: ConvexTokenService,
    audit: { publishAuditDurable: jest.Mock },
  ) => (
    serviceId: string,
    credential: string,
    body: Record<string, unknown>,
    zdrHeader?: string,
  ) => Promise<{ token: string; expiresInSeconds: number }>;
};

const endpoints: Endpoint[] = [
  {
    name: 'plane token (/api/:audience/internal-token)',
    audience: 'inference-core',
    scopes: ['inference:invoke'],
    signer: 'issuePlaneToken',
    build: (tokens, audit) => {
      const controller = new PlaneTokenController(tokens, audit as never);
      return (serviceId, credential, body, zdrHeader) =>
        controller.issueInternalToken(
          'inference-core',
          serviceId,
          credential,
          body as never,
          zdrHeader,
        ) as never;
    },
  },
  {
    name: 'Model Plane token (/api/model-plane/internal-token)',
    audience: 'model-gateway',
    scopes: ['models:invoke'],
    signer: 'issueModelPlaneToken',
    build: (tokens, audit) => {
      const controller = new ModelPlaneTokenController(tokens, audit as never);
      return (serviceId, credential, body, zdrHeader) =>
        controller.issueInternalToken(
          serviceId,
          credential,
          body as never,
          zdrHeader,
        ) as never;
    },
  },
];

function registryFor(endpoint: Endpoint): string {
  const postureOf = (retention: string) => ({
    [endpoint.audience]: retention,
  });
  const principal = (credential: string, retention: string) => ({
    credential,
    audiences: [endpoint.audience],
    orgIds: [],
    allowAnyOrg: true,
    scopes: endpoint.scopes,
    retentionByAudience: postureOf(retention),
  });
  return JSON.stringify({
    'tenant-worker': principal('test-only-tenant-worker-key', 'tenant'),
    'zdr-worker': principal('test-only-zdr-worker-key', 'zdr'),
    'persistent-worker': principal(
      'test-only-persistent-worker-key',
      'persistent',
    ),
  });
}

describe.each(endpoints)(
  'minting for a tenant principal through the $name',
  (endpoint) => {
    let tokens: ConvexTokenService;
    let audit: { publishAuditDurable: jest.Mock };
    let mint: ReturnType<Endpoint['build']>;
    let signer: jest.SpyInstance;

    beforeEach(() => {
      process.env.PLANE_SERVICE_PRINCIPALS_JSON = registryFor(endpoint);
      tokens = new ConvexTokenService();
      signer = jest.spyOn(tokens, endpoint.signer);
      audit = {
        publishAuditDurable: jest.fn().mockResolvedValue({
          stream: 'VEREVON_CONTROL_OBSERVABILITY',
          seq: 1,
        }),
      };
      mint = endpoint.build(tokens, audit);
    });

    const body = (orgId: string) => ({
      orgId,
      scopes: endpoint.scopes,
      reason: 'contextualise indexed chunks',
    });
    const asTenant = (orgId: string) =>
      mint('tenant-worker', 'test-only-tenant-worker-key', body(orgId));
    /** The first durable audit event published, as the NATS payload. */
    const firstAuditEvent = () =>
      (audit.publishAuditDurable.mock.calls as Array<[string, object]>)[0][1];
    const auditDetails = () =>
      (firstAuditEvent() as { details: unknown }).details;

    it('stamps zdr:true for an org with ZDR on, reads org-core exactly once, and lives at most 60 seconds', async () => {
      const bundle = await asTenant('org-zdr');

      const claims = payloadOf(bundle.token);
      expect(claims).toMatchObject({
        zdr: true,
        org_id: 'org-zdr',
        principal_type: 'service',
        sub: 'service:tenant-worker',
      });
      expect(
        (claims.exp as number) - (claims.iat as number),
      ).toBeLessThanOrEqual(60);
      expect(orgCore.requests).toEqual([
        { method: 'GET', path: '/internal/orgs/org-zdr/retention-posture' },
      ]);
    });

    it('stamps zdr:false for an org without ZDR, and only because org-core said so', async () => {
      const bundle = await asTenant('org-plain');

      expect(payloadOf(bundle.token).zdr).toBe(false);
      expect(bundle.expiresInSeconds).toBeLessThanOrEqual(60);
      expect(orgCore.requests).toHaveLength(1);
    });

    it('stamps zdr:false for an org whose ZDR intent outlived its plan entitlement', async () => {
      expect(payloadOf((await asTenant('org-lapsed-plan')).token).zdr).toBe(
        false,
      );
    });

    it('gives two orgs on one principal two postures: the claim follows the org, not the principal', async () => {
      const zdrOrg = payloadOf((await asTenant('org-zdr')).token);
      const plainOrg = payloadOf((await asTenant('org-plain')).token);

      expect([zdrOrg.org_id, zdrOrg.zdr]).toEqual(['org-zdr', true]);
      expect([plainOrg.org_id, plainOrg.zdr]).toEqual(['org-plain', false]);
    });

    it('records the posture and where it came from in the durable audit event', async () => {
      await asTenant('org-zdr');

      expect(auditDetails()).toEqual({
        audience: endpoint.audience,
        scopes: endpoint.scopes,
        reason: 'contextualise indexed chunks',
        zdr: true,
        zdr_source: 'tenant-policy',
        org_binding: 'any',
      });
    });

    it('records the RESOLVED posture in the audit event, not the registry default: zdr:false for an org without ZDR, the same value the token carries', async () => {
      // A tenant row's registry-side value is the safe `true`, so an audit event
      // built from it would say `true` for exactly the orgs whose token says
      // `false`. org-zdr cannot tell the two apart; these orgs can.
      for (const orgId of ['org-plain', 'org-lapsed-plan']) {
        audit.publishAuditDurable.mockClear();

        const bundle = await asTenant(orgId);

        expect(payloadOf(bundle.token).zdr).toBe(false);
        expect(auditDetails()).toMatchObject({
          zdr: false,
          zdr_source: 'tenant-policy',
        });
      }
    });

    it('keeps orgs apart when their mints overlap: every concurrent token carries its own org posture, and org-core is asked once per org', async () => {
      // Nothing is awaited between the three starts, so all three org-core reads
      // are in flight together. A lookup slot shared between orgs would hand one
      // org the answer meant for another: a ZDR org a non-ZDR token.
      const [zdrOrg, plainOrg, lapsedOrg] = await Promise.all([
        asTenant('org-zdr'),
        asTenant('org-plain'),
        asTenant('org-lapsed-plan'),
      ]);

      expect(
        [zdrOrg, plainOrg, lapsedOrg].map((bundle) => {
          const claims = payloadOf(bundle.token);
          return [claims.org_id, claims.zdr];
        }),
      ).toEqual([
        ['org-zdr', true],
        ['org-plain', false],
        ['org-lapsed-plan', false],
      ]);
      expect(orgCore.requests.map((request) => request.path).sort()).toEqual([
        '/internal/orgs/org-lapsed-plan/retention-posture',
        '/internal/orgs/org-plain/retention-posture',
        '/internal/orgs/org-zdr/retention-posture',
      ]);
    });

    it('answers 503 retention_posture_unavailable and issues no token and no audit event while org-core cannot be read', async () => {
      orgCore.up = false;

      const attempt = asTenant('org-zdr');

      await expect(attempt).rejects.toBeInstanceOf(ServiceUnavailableException);
      await expect(attempt).rejects.toMatchObject({
        response: { statusCode: 503, error: 'retention_posture_unavailable' },
      });
      expect(signer).not.toHaveBeenCalled();
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    });

    it('mints again with whatever org-core then reports, as soon as it is back', async () => {
      orgCore.up = false;
      await expect(asTenant('org-plain')).rejects.toBeInstanceOf(
        ServiceUnavailableException,
      );

      orgCore.up = true;

      expect(payloadOf((await asTenant('org-plain')).token).zdr).toBe(false);
    });

    it('keeps minting zdr:true for an org last known to be ZDR while org-core is down', async () => {
      await asTenant('org-zdr');
      orgCore.up = false;

      expect(payloadOf((await asTenant('org-zdr')).token).zdr).toBe(true);
    });

    it('asks org-core nothing for a caller that is not authorized: a wrong credential, an unknown principal or a scope outside its allowlist', async () => {
      await expect(
        mint('tenant-worker', 'wrong-key', body('org-zdr')),
      ).rejects.toBeInstanceOf(ForbiddenException);
      await expect(
        mint('tenant-worker', 'test-only-tenant-worker-key', {
          ...body('org-zdr'),
          scopes: ['admin'],
        }),
      ).rejects.toBeInstanceOf(ForbiddenException);
      await expect(
        mint('unknown-worker', 'test-only-tenant-worker-key', body('org-zdr')),
      ).rejects.toBeInstanceOf(ForbiddenException);

      expect(orgCore.requests).toEqual([]);
      expect(signer).not.toHaveBeenCalled();
    });

    it('still refuses a caller-selected posture, in the body or the x-zdr header, and asks org-core nothing', async () => {
      await expect(
        mint('tenant-worker', 'test-only-tenant-worker-key', {
          ...body('org-zdr'),
          zdr: false,
        }),
      ).rejects.toBeInstanceOf(BadRequestException);
      await expect(
        mint(
          'tenant-worker',
          'test-only-tenant-worker-key',
          body('org-zdr'),
          'false',
        ),
      ).rejects.toBeInstanceOf(BadRequestException);

      expect(orgCore.requests).toEqual([]);
      expect(signer).not.toHaveBeenCalled();
    });

    it('lets org-core decide nothing for a zero-retention principal: zdr:true for every org, org-core never asked, lifetime unchanged', async () => {
      for (const orgId of ['org-zdr', 'org-plain']) {
        const bundle = await mint(
          'zdr-worker',
          'test-only-zdr-worker-key',
          body(orgId),
        );
        expect(payloadOf(bundle.token).zdr).toBe(true);
        expect(bundle.expiresInSeconds).toBeGreaterThan(60);
      }

      expect(orgCore.requests).toEqual([]);
      expect(firstAuditEvent()).toMatchObject({
        details: { zdr: true, zdr_source: 'registry' },
      });
    });

    it('lets org-core decide nothing for a persistent principal either: zdr:false even for a ZDR org, org-core never asked, lifetime unchanged', async () => {
      for (const orgId of ['org-zdr', 'org-plain']) {
        const bundle = await mint(
          'persistent-worker',
          'test-only-persistent-worker-key',
          body(orgId),
        );
        expect(payloadOf(bundle.token).zdr).toBe(false);
        expect(bundle.expiresInSeconds).toBeGreaterThan(60);
      }

      expect(orgCore.requests).toEqual([]);
      expect(firstAuditEvent()).toMatchObject({
        details: { zdr: false, zdr_source: 'registry' },
      });
    });
  },
);
