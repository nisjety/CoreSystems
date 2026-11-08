import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

// WS7.I: the `web_policy` claim on `quarry` and `browser-broker` tokens.
//
// Two layers:
//   * the resolver (`web-policy.ts`) against an org-core double: the request
//     shape org-core's contract golden pins, the claim it builds, the 45 s fresh
//     cache, the 15 min stale-if-error window, no failure caching, the 404 rule
//     (W2-B3: an error, never an empty policy) and the 2 KiB cap;
//   * the real PlaneTokenController over the real ConvexTokenService: which
//     tokens carry the claim, that `zdr` is byte-identical with and without it,
//     and that an unreadable policy means 503 and no token.
//
// Only Better Auth's session, the membership lookup and the interactive
// retention posture are stubbed (they are I/O this spec is not about).

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
const mockResolveInteractiveRetentionPosture = jest.fn();
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import { ConvexTokenService } from './convex-token.service';
import { resetInteractiveRetentionCacheForTests } from './interactive-retention-policy';
import { PlaneTokenController } from './plane-token.controller';
import {
  assertWebPolicyClaimSize,
  BASELINE_WEB_POLICY_CLAIM,
  resetWebPolicyCacheForTests,
  resolveWebPolicy,
  WEB_POLICY_CLAIM_MAX_BYTES,
  WEB_POLICY_FRESH_TTL_MS,
  WEB_POLICY_STALE_MAX_AGE_MS,
  webPolicyClaimBytes,
  webPolicyClaimFor,
  webPolicyClaimFromOrgCore,
  WebPolicyTooLargeError,
  WebPolicyUnavailableError,
} from './web-policy';

const ORG_CORE_TOKEN = 'web-policy-spec-org-core-token-0123456789abcdef';
const ORG = 'org-web-policy-spec';
const POLICY_ANSWER = {
  data: {
    revision: 2,
    rev: '0123456789ab',
    policy: {
      robots_max: 'respect',
      caps: {
        'browser.act.upload': 'disabled',
        'browser.act.download': 'disabled',
      },
      domains: { allow: [], deny: ['blocked.example'] },
    },
    entitlements: { 'web.paid_search': true },
  },
};
const POLICY_CLAIM = {
  v: 1,
  rev: '0123456789ab',
  caps: {
    'browser.act.download': 'disabled',
    'browser.act.upload': 'disabled',
  },
  robots_max: 'respect',
  domains: { deny: ['blocked.example'] },
};
const NO_POLICY_ANSWER = {
  data: { revision: 0, rev: '0', policy: {}, entitlements: {} },
};

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

type Answer =
  | { status: number; body?: unknown }
  | { throws: Error }
  | { json: 'invalid' };

type Claims = Record<string, unknown>;

function payloadOf(token: string): Claims {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Claims;
}

const originalEnv = { ...process.env };
let answer: Answer;
let fetchMock: jest.SpyInstance;
let now: number;

function webPolicyCalls(): string[] {
  return fetchMock.mock.calls
    .map(([input]) => new URL(String(input)).pathname)
    .filter((path) => path.endsWith('/web-policy'));
}

beforeEach(() => {
  process.env = { ...originalEnv };
  process.env.NODE_ENV = 'test';
  process.env.ORG_SERVICE_URL = 'http://org-core.web-policy-spec.test:8080';
  delete process.env.ORG_CORE_URL;
  process.env.ORG_CORE_SERVICE_TOKEN = ORG_CORE_TOKEN;
  process.env.WEB_POLICY_CLAIM_ENABLED = '1';
  delete process.env.WEB_POLICY_TOKEN_TTL_CAP_SECONDS;
  resetWebPolicyCacheForTests();
  resetInteractiveRetentionCacheForTests();
  now = Date.parse('2026-11-03T12:00:00.000Z');
  jest.spyOn(Date, 'now').mockImplementation(() => now);
  answer = { status: 200, body: POLICY_ANSWER };
  fetchMock = jest
    .spyOn(global, 'fetch')
    .mockImplementation((input: unknown, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      const path = new URL(String(input)).pathname;
      if (
        headers.get('x-service-id') !== 'auth-core' ||
        headers.get('x-service-token') !== ORG_CORE_TOKEN
      ) {
        return Promise.resolve({
          ok: false,
          status: 401,
          json: () => Promise.resolve({ error: 'unauthorized' }),
        } as never);
      }
      if (path.endsWith('/retention-posture')) {
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () => Promise.resolve({ zdrIntent: false, entitled: true }),
        } as never);
      }
      if ('throws' in answer) return Promise.reject(answer.throws);
      if ('json' in answer) {
        return Promise.resolve({
          ok: true,
          status: 200,
          json: () => Promise.reject(new SyntaxError('bad json')),
        } as never);
      }
      const { status, body } = answer;
      return Promise.resolve({
        ok: status >= 200 && status < 300,
        status,
        json: () => Promise.resolve(body),
      } as never);
    });
});

afterEach(() => {
  jest.restoreAllMocks();
  resetWebPolicyCacheForTests();
  resetInteractiveRetentionCacheForTests();
  process.env = { ...originalEnv };
});

describe('resolveWebPolicy: the org-core read', () => {
  it('sends exactly the request org-core pins, and builds the claim from its answer', async () => {
    await expect(resolveWebPolicy(ORG)).resolves.toEqual(POLICY_CLAIM);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(String(input)).toBe(
      `http://org-core.web-policy-spec.test:8080/internal/orgs/${ORG}/web-policy`,
    );
    expect(init.method).toBe('GET');
    const headers = new Headers(init.headers);
    expect(headers.get('accept')).toBe('application/json');
    expect(headers.get('x-service-id')).toBe('auth-core');
    expect(headers.get('x-service-token')).toBe(ORG_CORE_TOKEN);
  });

  it("matches org-core's contract golden (request shape and answer) when that tree is present", async () => {
    const golden = join(
      __dirname,
      '../../../org-core/internal/http/testdata/auth-core-org-core-web-policy.contract.json',
    );
    if (!existsSync(golden)) {
      if (process.env.REQUIRE_CONTRACT || process.env.CI) {
        throw new Error(`REQUIRE_CONTRACT/CI is set but ${golden} is absent`);
      }
      console.warn(`org-core tree absent, skipping: ${golden}`);
      return;
    }
    const contract = JSON.parse(readFileSync(golden, 'utf8')) as {
      organizationId: string;
      tokenPlaceholder: string;
      requests: Array<{
        method: string;
        path: string;
        headers: Record<string, string>;
      }>;
      response: { status: number; body: unknown };
      notFound: { status: number; body: unknown };
    };
    answer = contract.response;
    const claim = await resolveWebPolicy(contract.organizationId);
    const [input, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const request = contract.requests[0];
    expect(new URL(String(input)).pathname).toBe(request.path);
    expect(init.method).toBe(request.method);
    const sent = new Headers(init.headers);
    for (const [name, value] of Object.entries(request.headers)) {
      expect(sent.get(name)).toBe(
        value === contract.tokenPlaceholder ? ORG_CORE_TOKEN : value,
      );
    }
    expect(claim).toEqual(webPolicyClaimFromOrgCore(contract.response.body));
    expect(claim.v).toBe(1);

    // org-core's 404 is never an empty policy.
    resetWebPolicyCacheForTests();
    answer = contract.notFound;
    await expect(resolveWebPolicy(contract.organizationId)).rejects.toThrow(
      WebPolicyUnavailableError,
    );
  });

  it('an org with no policy is rev "0" and nothing else', async () => {
    answer = { status: 200, body: NO_POLICY_ANSWER };
    await expect(resolveWebPolicy(ORG)).resolves.toEqual(
      BASELINE_WEB_POLICY_CLAIM,
    );
  });

  it('carries only the four claim fields, sorted, without empty containers', () => {
    expect(
      webPolicyClaimFromOrgCore({
        data: {
          rev: 'abcdefabcdef',
          policy: {
            caps: {},
            proxy_max: 'none',
            domains: { allow: [], deny: [] },
            future_field: { anything: true },
          },
        },
      }),
    ).toEqual({ v: 1, rev: 'abcdefabcdef', proxy_max: 'none' });
  });

  it.each([
    ['no envelope', { rev: '0' }],
    ['a rev that is not a digest', { data: { rev: 'v2', policy: {} } }],
    ['a numeric rev', { data: { rev: 0, policy: {} } }],
    ['a policy that is a list', { data: { rev: '0', policy: [] } }],
    ['a non-string mode', { data: { rev: '0', policy: { caps: { a: 1 } } } }],
    [
      'a domain list of numbers',
      { data: { rev: '0', policy: { domains: { deny: [1] } } } },
    ],
  ])('a malformed answer (%s) is 503, never a partial policy', async (_, body) => {
    answer = { status: 200, body };
    await expect(resolveWebPolicy(ORG)).rejects.toMatchObject({
      reason: 'org_core_malformed_response',
    });
  });

  it('serves a successful read for 45 s without asking again', async () => {
    await resolveWebPolicy(ORG);
    now += WEB_POLICY_FRESH_TTL_MS - 1;
    await resolveWebPolicy(ORG);
    expect(webPolicyCalls()).toHaveLength(1);
    now += 1;
    await resolveWebPolicy(ORG);
    expect(webPolicyCalls()).toHaveLength(2);
  });

  it('concurrent mints for one org share one read', async () => {
    await Promise.all([
      resolveWebPolicy(ORG),
      resolveWebPolicy(ORG),
      resolveWebPolicy(ORG),
    ]);
    expect(webPolicyCalls()).toHaveLength(1);
  });

  it('org_core_down_with_nothing_remembered_is_503', async () => {
    answer = { throws: new TypeError('fetch failed') };
    const failure = resolveWebPolicy(ORG);
    await expect(failure).rejects.toThrow(WebPolicyUnavailableError);
    await expect(failure).rejects.toMatchObject({
      reason: 'org_core_unreachable',
      response: { statusCode: 503, error: 'web_policy_unavailable' },
    });
  });

  it.each([
    ['500', { status: 500, body: { error: { code: 'internal' } } }, 'org_core_error_status'],
    ['401', { status: 401, body: {} }, 'org_core_credential_rejected'],
    ['403', { status: 403, body: {} }, 'org_core_credential_rejected'],
    [
      '404 with org-core own body',
      {
        status: 404,
        body: { error: { code: 'not_found', message: 'organization not found', details: {} } },
      },
      'org_core_not_found',
    ],
    ['404 from a proxy', { status: 404, body: 'Not Found' }, 'org_core_not_found'],
  ] as const)('a %s answer is an error, never an empty policy', async (_, reply, reason) => {
    answer = reply;
    await expect(resolveWebPolicy(ORG)).rejects.toMatchObject({ reason });
  });

  it('an unparseable body is 503', async () => {
    answer = { json: 'invalid' };
    await expect(resolveWebPolicy(ORG)).rejects.toMatchObject({
      reason: 'org_core_malformed_response',
    });
  });

  it('without the scoped org-core credential nothing is asked and nothing is issued', async () => {
    delete process.env.ORG_CORE_SERVICE_TOKEN;
    await expect(resolveWebPolicy(ORG)).rejects.toMatchObject({
      reason: 'org_core_credential_missing',
    });
    expect(webPolicyCalls()).toHaveLength(0);
  });

  it('an org id it cannot ask about is 503, not a policy', async () => {
    await expect(resolveWebPolicy('../etc')).rejects.toMatchObject({
      reason: 'invalid_organization_id',
    });
    expect(webPolicyCalls()).toHaveLength(0);
  });

  it('stale_policy_served_for_15_minutes_then_503', async () => {
    await resolveWebPolicy(ORG);
    answer = { status: 503, body: {} };
    now += WEB_POLICY_STALE_MAX_AGE_MS - 1;
    await expect(resolveWebPolicy(ORG)).resolves.toEqual(POLICY_CLAIM);
    // A stale serve never extends the window: it is measured from the read.
    now += 1;
    await expect(resolveWebPolicy(ORG)).rejects.toThrow(
      WebPolicyUnavailableError,
    );
  });

  it('a 404 for a remembered org serves the stale policy, never an empty one', async () => {
    await resolveWebPolicy(ORG);
    now += WEB_POLICY_FRESH_TTL_MS;
    answer = { status: 404, body: { error: { code: 'not_found' } } };
    await expect(resolveWebPolicy(ORG)).resolves.toEqual(POLICY_CLAIM);
  });

  it('failure_is_never_cached', async () => {
    answer = { status: 500, body: {} };
    await expect(resolveWebPolicy(ORG)).rejects.toThrow(
      WebPolicyUnavailableError,
    );
    answer = { status: 200, body: POLICY_ANSWER };
    await expect(resolveWebPolicy(ORG)).resolves.toEqual(POLICY_CLAIM);
    expect(webPolicyCalls()).toHaveLength(2);
  });

  it('claim_over_2KiB_fails_closed, and the oversized answer is not remembered', async () => {
    const deny: string[] = [];
    for (let i = 0; i < 200; i += 1) deny.push(`host-${i}.blocked.example`);
    answer = {
      status: 200,
      body: { data: { rev: '0123456789ab', policy: { domains: { deny } } } },
    };
    const failure = resolveWebPolicy(ORG);
    await expect(failure).rejects.toThrow(WebPolicyTooLargeError);
    await expect(failure).rejects.toMatchObject({
      response: { statusCode: 503, error: 'web_policy_too_large' },
    });
    answer = { status: 503, body: {} };
    await expect(resolveWebPolicy(ORG)).rejects.toThrow(
      WebPolicyUnavailableError,
    );
  });

  it('a claim of exactly 2 KiB is carried', () => {
    const base = { v: 1 as const, rev: '0123456789ab', domains: { deny: [''] } };
    const room = WEB_POLICY_CLAIM_MAX_BYTES - webPolicyClaimBytes(base);
    const claim = { ...base, domains: { deny: ['x'.repeat(room)] } };
    expect(webPolicyClaimBytes(claim)).toBe(WEB_POLICY_CLAIM_MAX_BYTES);
    expect(() => assertWebPolicyClaimSize(claim)).not.toThrow();
    const over = { ...base, domains: { deny: ['x'.repeat(room + 1)] } };
    expect(() => assertWebPolicyClaimSize(over)).toThrow(WebPolicyTooLargeError);
  });
});

describe('webPolicyClaimFor: audience and flag', () => {
  it('is undefined for every audience but quarry and browser-broker, and asks nothing', async () => {
    for (const audience of ['data-plane', 'ingestion', 'session-core']) {
      await expect(webPolicyClaimFor(audience, ORG)).resolves.toBeUndefined();
    }
    expect(webPolicyCalls()).toHaveLength(0);
  });

  it.each(['0', '', 'false', 'off'])(
    'WEB_POLICY_CLAIM_ENABLED=%p yields {"v":1,"rev":"0"} and never asks org-core',
    async (flag) => {
      process.env.WEB_POLICY_CLAIM_ENABLED = flag;
      answer = { throws: new TypeError('fetch failed') };
      for (const audience of ['quarry', 'browser-broker']) {
        await expect(webPolicyClaimFor(audience, ORG)).resolves.toEqual({
          v: 1,
          rev: '0',
        });
      }
      expect(webPolicyCalls()).toHaveLength(0);
    },
  );

  it('defaults off when the variable is absent', async () => {
    delete process.env.WEB_POLICY_CLAIM_ENABLED;
    await expect(webPolicyClaimFor('quarry', ORG)).resolves.toEqual(
      BASELINE_WEB_POLICY_CLAIM,
    );
    expect(webPolicyCalls()).toHaveLength(0);
  });
});

describe('the claim on real tokens', () => {
  let tokens: ConvexTokenService;
  let controller: PlaneTokenController;
  let audit: { publishAuditDurable: jest.Mock };

  const registry = {
    'tenant-web': {
      credential: 'test-only-tenant-web-credential-key',
      audiences: ['quarry', 'browser-broker'],
      orgIds: [ORG],
      scopes: ['search:read', 'browser:read'],
      scopesByAudience: {
        quarry: ['search:read'],
        'browser-broker': ['browser:read'],
      },
      retentionByAudience: { quarry: 'zdr', 'browser-broker': 'zdr' },
      webPolicyByAudience: { quarry: 'tenant' },
    },
    'plain-web': {
      credential: 'test-only-plain-web-credential-key',
      audiences: ['quarry'],
      orgIds: [ORG],
      scopes: ['search:read'],
      retentionByAudience: { quarry: 'zdr' },
    },
  };

  beforeEach(() => {
    for (const name of keyEnvNames) delete process.env[name];
    delete process.env.QUARRY_AUTH_AUDIENCE;
    delete process.env.PLANE_TOKEN_TTL_QUARRY_SECONDS;
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify(registry);
    tokens = new ConvexTokenService();
    audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    };
    controller = new PlaneTokenController(tokens, audit as never);
    mockGetSession.mockReset().mockResolvedValue({
      user: { id: 'user-web', email: 'member@example.test' },
      session: { activeOrganizationId: ORG },
    });
    mockResolveCanonicalTokenContext
      .mockReset()
      .mockResolvedValue({ orgId: ORG, role: 'member' });
    mockResolveInteractiveRetentionPosture.mockReset().mockResolvedValue({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
  });

  async function userClaims(audience: string): Promise<Claims> {
    const bundle = await controller.getToken(audience, {
      headers: { cookie: 'session=web' },
    } as never);
    return payloadOf((bundle as { token: string }).token);
  }

  async function serviceBundle(
    serviceId: string,
    audience = 'quarry',
    scopes = ['search:read'],
  ): Promise<{ token: string; expiresInSeconds: number }> {
    return (await controller.issueInternalToken(
      audience,
      serviceId,
      `test-only-${serviceId}-credential-key`,
      { orgId: ORG, scopes, reason: 'web policy spec' },
    )) as { token: string; expiresInSeconds: number };
  }

  it('user_token_carries_the_claim_for_quarry_and_browser_broker_only', async () => {
    expect((await userClaims('quarry')).web_policy).toEqual(POLICY_CLAIM);
    expect((await userClaims('browser-broker')).web_policy).toEqual(
      POLICY_CLAIM,
    );
    for (const audience of ['data-plane', 'ingestion', 'session-core']) {
      expect(await userClaims(audience)).not.toHaveProperty('web_policy');
    }
    // One read served all of them (fresh cache), and none for the others.
    expect(webPolicyCalls()).toHaveLength(1);
  });

  it('claim_is_always_present_on_those_audiences, flag off included', async () => {
    process.env.WEB_POLICY_CLAIM_ENABLED = '0';
    for (const audience of ['quarry', 'browser-broker']) {
      expect((await userClaims(audience)).web_policy).toEqual({
        v: 1,
        rev: '0',
      });
    }
    // The pre-org onboarding preview token (sentinel org) carries the baseline.
    const onboarding = await controller.getOnboardingToken({
      headers: { cookie: 'session=web' },
    } as never);
    expect(payloadOf(onboarding.token).web_policy).toEqual({
      v: 1,
      rev: '0',
    });
    expect(webPolicyCalls()).toHaveLength(0);
  });

  it('service_token_carries_the_claim_only_for_tenant_rows', async () => {
    const tenant = await serviceBundle('tenant-web');
    expect(payloadOf(tenant.token).web_policy).toEqual(POLICY_CLAIM);
    // The tenant row names quarry only: its browser-broker token has none.
    const tenantBroker = await serviceBundle('tenant-web', 'browser-broker', [
      'browser:read',
    ]);
    expect(payloadOf(tenantBroker.token)).not.toHaveProperty('web_policy');
    const plain = await serviceBundle('plain-web');
    expect(payloadOf(plain.token)).not.toHaveProperty('web_policy');
    expect(webPolicyCalls()).toHaveLength(1);

    // The audit event names the revision for the token that carries it, and
    // only for that one; never the policy content.
    const details = audit.publishAuditDurable.mock.calls.map(
      ([, event]) => (event as { details: Record<string, unknown> }).details,
    );
    expect(details[0]).toMatchObject({ web_policy_rev: '0123456789ab' });
    expect(details[1]).not.toHaveProperty('web_policy_rev');
    expect(details[2]).not.toHaveProperty('web_policy_rev');
    expect(JSON.stringify(details)).not.toContain('blocked.example');
  });

  it('a tenant row with the flag off carries {"v":1,"rev":"0"} and asks nothing', async () => {
    process.env.WEB_POLICY_CLAIM_ENABLED = '0';
    const tenant = await serviceBundle('tenant-web');
    expect(payloadOf(tenant.token).web_policy).toEqual({ v: 1, rev: '0' });
    expect(webPolicyCalls()).toHaveLength(0);
    expect(audit.publishAuditDurable.mock.calls[0][1]).toMatchObject({
      details: { web_policy_rev: '0' },
    });
  });

  it('zdr_claim_is_byte_identical_before_and_after', async () => {
    const strip = (claims: Claims): Claims => {
      const { iat: _iat, nbf: _nbf, exp: _exp, web_policy: _wp, ...rest } =
        claims;
      return rest;
    };
    process.env.WEB_POLICY_CLAIM_ENABLED = '0';
    const userOff = await userClaims('quarry');
    const serviceOff = payloadOf((await serviceBundle('tenant-web')).token);
    process.env.WEB_POLICY_CLAIM_ENABLED = '1';
    const userOn = await userClaims('quarry');
    const serviceOn = payloadOf((await serviceBundle('tenant-web')).token);
    expect(JSON.stringify(strip(userOn))).toBe(JSON.stringify(strip(userOff)));
    expect(JSON.stringify(strip(serviceOn))).toBe(
      JSON.stringify(strip(serviceOff)),
    );
    expect(userOn.zdr).toBe(true);
    // The tenant row's quarry retention is zdr: the org's policy never moves it.
    expect(serviceOn.zdr).toBe(true);
    expect(userOn.web_policy).toEqual(POLICY_CLAIM);
  });

  it('org_core_down_with_nothing_remembered_is_503_and_no_token', async () => {
    answer = { throws: new TypeError('fetch failed') };
    await expect(userClaims('quarry')).rejects.toMatchObject({
      response: { statusCode: 503, error: 'web_policy_unavailable' },
    });
    await expect(serviceBundle('tenant-web')).rejects.toMatchObject({
      response: { statusCode: 503, error: 'web_policy_unavailable' },
    });
    // Nothing was signed, so nothing was audited as issued.
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    // A principal without a tenant row is unaffected by the outage.
    const plain = await serviceBundle('plain-web');
    expect(payloadOf(plain.token)).not.toHaveProperty('web_policy');
  });

  it('a 404 from org-core is 503 and no token, never an empty policy', async () => {
    answer = { status: 404, body: { error: { code: 'not_found' } } };
    await expect(userClaims('quarry')).rejects.toMatchObject({
      response: { error: 'web_policy_unavailable' },
    });
  });

  it('service_token_ttl_unchanged_without_the_cap', async () => {
    const plain = await serviceBundle('plain-web');
    const tenant = await serviceBundle('tenant-web');
    expect(tenant.expiresInSeconds).toBe(plain.expiresInSeconds);
    expect(tenant.expiresInSeconds).toBe(300);
    const claims = payloadOf(tenant.token);
    expect(Number(claims.exp) - Number(claims.iat)).toBe(300);
  });

  it('WEB_POLICY_TOKEN_TTL_CAP_SECONDS shortens only tokens that carry the claim', async () => {
    process.env.WEB_POLICY_TOKEN_TTL_CAP_SECONDS = '120';
    expect((await serviceBundle('tenant-web')).expiresInSeconds).toBe(120);
    expect((await serviceBundle('plain-web')).expiresInSeconds).toBe(300);
  });

  it('the signer refuses a web_policy claim on any other audience, and an oversized one', () => {
    expect(() =>
      tokens.issuePlaneToken('data-plane', {
        userId: 'user-web',
        orgId: ORG,
        webPolicy: BASELINE_WEB_POLICY_CLAIM,
      }),
    ).toThrow(/web_policy/);
    expect(() =>
      tokens.issuePlaneToken('quarry', {
        userId: 'user-web',
        orgId: ORG,
        webPolicy: {
          v: 1,
          rev: '0123456789ab',
          domains: { deny: ['x'.repeat(WEB_POLICY_CLAIM_MAX_BYTES)] },
        },
      }),
    ).toThrow(WebPolicyTooLargeError);
  });
});

describe('the vendored web-policy catalogue', () => {
  const copy = join(__dirname, 'fixtures', 'web-policy-catalogue.v1.json');
  const pin = join(__dirname, 'fixtures', 'web-policy-catalogue.v1.json.sha256');
  const source = join(
    __dirname,
    '../../../../Ingestion Plane/Quarry-v2/crates/quarry-core/contracts/web-policy-catalogue.v1.json',
  );

  it('hashes to its pin', () => {
    const digest = createHash('sha256').update(readFileSync(copy)).digest('hex');
    expect(readFileSync(pin, 'utf8').trim().split(/\s+/)[0]).toBe(digest);
  });

  it("is byte-identical to Quarry's catalogue when that tree is present", () => {
    if (!existsSync(source)) {
      if (process.env.REQUIRE_CONTRACT || process.env.CI) {
        throw new Error(`REQUIRE_CONTRACT/CI is set but ${source} is absent`);
      }
      console.warn(`Quarry tree absent, skipping: ${source}`);
      return;
    }
    expect(readFileSync(copy).equals(readFileSync(source))).toBe(true);
  });

  it('names every capability key the golden policy uses', () => {
    const catalogue = JSON.parse(readFileSync(copy, 'utf8')) as {
      keys: Array<{ key: string }>;
    };
    const known = new Set(catalogue.keys.map((entry) => entry.key));
    for (const key of Object.keys(POLICY_CLAIM.caps)) {
      expect(known.has(key)).toBe(true);
    }
  });
});
