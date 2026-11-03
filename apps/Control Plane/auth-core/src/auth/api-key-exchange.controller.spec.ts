import { HttpException, Logger } from '@nestjs/common';

// INNH-10 CP-1 / CP-4. The REAL token service and the REAL controller route
// run together; only Better Auth's verifier, the membership read, the org
// retention posture and the NATS audit publisher are doubles.
const mockVerifyApiKey = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
const mockResolveInteractiveRetentionPosture = jest.fn();
jest.mock('./auth', () => ({
  auth: {
    api: {
      getSession: jest.fn(),
      verifyApiKey: (...args: unknown[]) => mockVerifyApiKey(...args),
    },
  },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import {
  API_KEY_TOKEN_DENIED_EVENT,
  API_KEY_TOKEN_EXCHANGED_EVENT,
} from './api-key-exchange.controller';
import {
  API_KEY_AGENT_TOKEN_TTL_SECONDS,
  ConvexTokenService,
} from './convex-token.service';
import { RetentionPostureUnavailableError } from './interactive-retention-policy';
import { PlaneTokenController } from './plane-token.controller';

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

const GATEWAY = 'verevon-gateway';
const GATEWAY_CREDENTIAL = 'test-only-gateway-credential-01';
const BROKER_ONLY = 'broker-only-caller';
const BROKER_ONLY_CREDENTIAL = 'test-only-broker-credential-01';
const NO_EXCHANGE = 'plain-service';
const NO_EXCHANGE_CREDENTIAL = 'test-only-plain-credential-001';

// A test value, never a real key: it must never appear in a log or an audit.
const API_KEY = 'user_test-only-agent-key-value-0000000000000000';
const KEY_ID = 'key-agent-1';
const OWNER = 'user-owner';

function payloadOf(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

function agentMetadata(overrides: Record<string, unknown> = {}) {
  return {
    metadata_version: 1,
    org_id: 'org-a',
    key_hosts: ['docs.example.com', 'example.com'],
    budget_ceiling: { micro_usd: 1_000_000, max_steps: 50, max_runtime_s: 600 },
    zdr: false,
    browser_agent_granted_by: 'user-admin',
    ...overrides,
  };
}

function verifiedKey(overrides: Record<string, unknown> = {}) {
  return {
    valid: true,
    error: null,
    key: {
      id: KEY_ID,
      configId: 'user-keys',
      referenceId: OWNER,
      enabled: true,
      expiresAt: new Date(Date.now() + 86_400_000).toISOString(),
      permissions: { api: ['browser:agent'] },
      metadata: agentMetadata(),
      ...overrides,
    },
  };
}

async function rejection(promise: Promise<unknown>): Promise<HttpException> {
  const error: unknown = await promise.then(
    () => {
      throw new Error('expected a rejection');
    },
    (caught: unknown) => caught,
  );
  expect(error).toBeInstanceOf(HttpException);
  return error as HttpException;
}

function codeOf(error: HttpException): unknown {
  const response = error.getResponse();
  return typeof response === 'object' && response !== null
    ? (response as Record<string, unknown>).code
    : undefined;
}

describe('POST /api/:audience/api-key-exchange (INNH-10 CP-1)', () => {
  const originalEnv = { ...process.env };
  let audit: { publishAuditDurable: jest.Mock };
  let tokens: ConvexTokenService;
  let controller: PlaneTokenController;
  let logged: string[];

  function build(): void {
    tokens = new ConvexTokenService();
    controller = new PlaneTokenController(tokens, audit as never);
  }

  function exchange(
    audience = 'quarry',
    body: Record<string, unknown> = {
      apiKey: API_KEY,
      orgId: 'org-a',
      scopes: ['browser:agent'],
    },
    serviceId = GATEWAY,
    credential = GATEWAY_CREDENTIAL,
  ) {
    return controller.exchangeApiKey(audience, serviceId, credential, body);
  }

  function auditEvents(): string[] {
    return audit.publishAuditDurable.mock.calls.map(
      ([, payload]) => (payload as { event: string }).event,
    );
  }

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    delete process.env.WEB_POLICY_CLAIM_ENABLED;
    delete process.env.WEB_POLICY_TOKEN_TTL_CAP_SECONDS;
    delete process.env.PLANE_TOKEN_TTL_QUARRY_SECONDS;
    delete process.env.PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS;
    delete process.env.PLANE_SERVICE_PRINCIPALS_FILE;
    process.env.API_KEY_ENABLED = 'true';
    process.env.API_KEY_AGENT_EXCHANGE_ENABLED = 'true';
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      [GATEWAY]: {
        credential: GATEWAY_CREDENTIAL,
        audiences: ['quarry'],
        orgIds: ['org-a'],
        scopes: ['quarry:read'],
        apiKeyExchange: ['quarry', 'browser-broker'],
      },
      [BROKER_ONLY]: {
        credential: BROKER_ONLY_CREDENTIAL,
        audiences: ['quarry'],
        orgIds: ['org-a'],
        scopes: ['quarry:read'],
        apiKeyExchange: ['browser-broker'],
      },
      [NO_EXCHANGE]: {
        credential: NO_EXCHANGE_CREDENTIAL,
        audiences: ['quarry', 'browser-broker'],
        orgIds: ['org-a'],
        scopes: ['quarry:read'],
      },
    });
    mockVerifyApiKey.mockReset().mockResolvedValue(verifiedKey());
    mockResolveCanonicalTokenContext
      .mockReset()
      .mockResolvedValue({ orgId: 'org-a', role: 'member' });
    mockResolveInteractiveRetentionPosture.mockReset().mockResolvedValue({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
    audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    };
    logged = [];
    for (const level of ['log', 'warn', 'error', 'debug', 'verbose'] as const) {
      jest
        .spyOn(Logger.prototype, level)
        .mockImplementation((...args: unknown[]) => {
          logged.push(JSON.stringify(args));
        });
    }
    build();
  });

  afterEach(() => {
    jest.restoreAllMocks();
    process.env = { ...originalEnv };
  });

  it('mints the section 3 api_key token for quarry and audits it durably', async () => {
    const bundle = await exchange();
    const claims = payloadOf(bundle.token);

    expect(claims).toMatchObject({
      aud: 'quarry',
      sub: `apikey:${KEY_ID}`,
      principal_type: 'api_key',
      key_id: KEY_ID,
      acting_user_id: OWNER,
      org_id: 'org-a',
      scopes: ['browser:agent'],
      zdr: false,
      web_policy: { v: 1, rev: '0' },
    });
    expect(claims).not.toHaveProperty('user_id');
    expect(claims).not.toHaveProperty('service_id');
    expect(claims).not.toHaveProperty('key_hosts');
    expect(typeof claims.jti).toBe('string');
    expect((claims.jti as string).length).toBeGreaterThan(0);
    expect(Number(claims.exp) - Number(claims.iat)).toBeLessThanOrEqual(
      API_KEY_AGENT_TOKEN_TTL_SECONDS,
    );

    expect(mockVerifyApiKey).toHaveBeenCalledWith({
      body: { key: API_KEY, configId: 'user-keys' },
    });
    expect(mockResolveCanonicalTokenContext).toHaveBeenCalledWith(
      OWNER,
      'org-a',
    );
    expect(audit.publishAuditDurable).toHaveBeenCalledTimes(1);
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      `verevon.audit.v2.control.auth-core.${API_KEY_TOKEN_EXCHANGED_EVENT}`,
      expect.objectContaining({
        org_id: 'org-a',
        plane: 'control',
        producer: 'auth-core',
        event: API_KEY_TOKEN_EXCHANGED_EVENT,
        subject: `apikey:${KEY_ID}`,
        outcome: 'ok',
        details: expect.objectContaining({
          key_id: KEY_ID,
          acting_user_id: OWNER,
          audience: 'quarry',
          scopes: ['browser:agent'],
          zdr: false,
          calling_service: GATEWAY,
        }),
      }),
    );
  });

  it('carries the key host ceiling on the browser-broker audience only', async () => {
    const claims = payloadOf((await exchange('browser-broker')).token);
    expect(claims.aud).toBe('browser-broker');
    expect(claims.principal_type).toBe('api_key');
    expect(claims.key_hosts).toEqual(['docs.example.com', 'example.com']);
  });

  it('each exchange gets a fresh jti', async () => {
    const first = payloadOf((await exchange()).token);
    const second = payloadOf((await exchange()).token);
    expect(first.jti).not.toEqual(second.jti);
  });

  describe('disabled flag', () => {
    it.each([
      ['exchange flag absent', { API_KEY_AGENT_EXCHANGE_ENABLED: undefined }],
      ['exchange flag false', { API_KEY_AGENT_EXCHANGE_ENABLED: 'false' }],
      ['api-key plugin off', { API_KEY_ENABLED: 'false' }],
    ])('%s: 404, the key is never verified, nothing is audited', async (_name, env) => {
      for (const [name, value] of Object.entries(env)) {
        if (value === undefined) delete process.env[name];
        else process.env[name] = value;
      }
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(404);
      expect(codeOf(error)).toBe('api_key_agent_exchange_disabled');
      expect(mockVerifyApiKey).not.toHaveBeenCalled();
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    });
  });

  it('refuses every audience other than quarry and browser-broker', async () => {
    for (const audience of ['data-plane', 'ingestion', 'model-gateway', 'x']) {
      const error = await rejection(exchange(audience));
      expect(error.getStatus()).toBe(404);
    }
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
  });

  it('refuses a caller-selected zdr or lifetime and a malformed body', async () => {
    const bodies: Record<string, unknown>[] = [
      { apiKey: API_KEY, orgId: 'org-a', scopes: ['browser:agent'], zdr: false },
      {
        apiKey: API_KEY,
        orgId: 'org-a',
        scopes: ['browser:agent'],
        ttlSeconds: 3600,
      },
      { orgId: 'org-a', scopes: ['browser:agent'] },
      { apiKey: API_KEY, scopes: ['browser:agent'] },
      { apiKey: API_KEY, orgId: 'org-a', scopes: [] },
      { apiKey: API_KEY, orgId: 'org-a' },
      { apiKey: API_KEY, orgId: '../org', scopes: ['browser:agent'] },
    ];
    for (const body of bodies) {
      const error = await rejection(exchange('quarry', body));
      expect(error.getStatus()).toBe(400);
    }
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
  });

  it('refuses a caller without apiKeyExchange for the audience before touching the key', async () => {
    const cases: Array<[string, string, string]> = [
      ['quarry', BROKER_ONLY, BROKER_ONLY_CREDENTIAL],
      ['quarry', NO_EXCHANGE, NO_EXCHANGE_CREDENTIAL],
      ['browser-broker', NO_EXCHANGE, NO_EXCHANGE_CREDENTIAL],
      ['quarry', GATEWAY, 'wrong-credential-value-000000'],
      ['quarry', 'unknown-service', GATEWAY_CREDENTIAL],
    ];
    for (const [audience, serviceId, credential] of cases) {
      const error = await rejection(
        exchange(
          audience,
          { apiKey: API_KEY, orgId: 'org-a', scopes: ['browser:agent'] },
          serviceId,
          credential,
        ),
      );
      expect(error.getStatus()).toBe(403);
    }
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('broker-only caller may exchange for browser-broker', async () => {
    const bundle = await exchange(
      'browser-broker',
      { apiKey: API_KEY, orgId: 'org-a', scopes: ['browser:agent'] },
      BROKER_ONLY,
      BROKER_ONLY_CREDENTIAL,
    );
    expect(payloadOf(bundle.token).aud).toBe('browser-broker');
  });

  it('org mismatch: 403 api_key_org_mismatch and a denial audit', async () => {
    mockVerifyApiKey.mockResolvedValue(
      verifiedKey({ metadata: agentMetadata({ org_id: 'org-b' }) }),
    );
    const error = await rejection(exchange());
    expect(error.getStatus()).toBe(403);
    expect(codeOf(error)).toBe('api_key_org_mismatch');
    expect(auditEvents()).toEqual([API_KEY_TOKEN_DENIED_EVENT]);
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      `verevon.audit.v2.control.auth-core.${API_KEY_TOKEN_DENIED_EVENT}`,
      expect.objectContaining({
        org_id: 'org-a',
        outcome: 'denied',
        details: expect.objectContaining({
          key_id: KEY_ID,
          reason: 'api_key_org_mismatch',
          calling_service: GATEWAY,
        }),
      }),
    );
  });

  it('a key without agent metadata is refused', async () => {
    for (const metadata of [null, {}, agentMetadata({ key_hosts: [] })]) {
      mockVerifyApiKey.mockResolvedValue(verifiedKey({ metadata }));
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(403);
      expect(codeOf(error)).toBe('api_key_metadata_invalid');
    }
  });

  describe('missing permission', () => {
    it('a key without browser:agent is refused', async () => {
      mockVerifyApiKey.mockResolvedValue(
        verifiedKey({ permissions: { api: ['documents:read'] } }),
      );
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(403);
      expect(codeOf(error)).toBe('api_key_scope_missing');
    });

    it('a scope beyond browser:agent is refused even if the key holds it', async () => {
      mockVerifyApiKey.mockResolvedValue(
        verifiedKey({ permissions: { api: ['browser:agent', 'browser:write'] } }),
      );
      const error = await rejection(
        exchange('quarry', {
          apiKey: API_KEY,
          orgId: 'org-a',
          scopes: ['browser:agent', 'browser:write'],
        }),
      );
      expect(error.getStatus()).toBe(403);
      expect(codeOf(error)).toBe('api_key_scope_missing');
    });
  });

  describe('org-key refused (OD-4)', () => {
    it('verification is scoped to user-keys, so an org key does not verify', async () => {
      mockVerifyApiKey.mockResolvedValue({
        valid: false,
        error: { code: 'INVALID_API_KEY', message: 'Invalid API key.' },
        key: null,
      });
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(401);
      expect(codeOf(error)).toBe('api_key_invalid');
      expect(mockVerifyApiKey.mock.calls[0][0].body.configId).toBe('user-keys');
    });

    it('a key that still comes back as an org key is refused', async () => {
      mockVerifyApiKey.mockResolvedValue(
        verifiedKey({ configId: 'org-keys', referenceId: 'org-a' }),
      );
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(403);
      expect(codeOf(error)).toBe('api_key_kind_refused');
    });
  });

  describe('expired or disabled key', () => {
    it.each([
      ['KEY_EXPIRED', 401, 'api_key_expired'],
      ['KEY_DISABLED', 401, 'api_key_disabled'],
      ['RATE_LIMITED', 429, 'api_key_rate_limited'],
      ['USAGE_EXCEEDED', 429, 'api_key_rate_limited'],
      ['KEY_NOT_FOUND', 401, 'api_key_invalid'],
    ])('Better Auth %s -> %d %s', async (code, status, reason) => {
      mockVerifyApiKey.mockResolvedValue({
        valid: false,
        error: { code, message: code },
        key: null,
      });
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(status);
      expect(codeOf(error)).toBe(reason);
      expect(auditEvents()).toEqual([API_KEY_TOKEN_DENIED_EVENT]);
    });

    it('re-checks expiry and enabled on the returned key', async () => {
      mockVerifyApiKey.mockResolvedValue(
        verifiedKey({ expiresAt: new Date(Date.now() - 1000).toISOString() }),
      );
      expect(codeOf(await rejection(exchange()))).toBe('api_key_expired');
      mockVerifyApiKey.mockResolvedValue(verifiedKey({ enabled: false }));
      expect(codeOf(await rejection(exchange()))).toBe('api_key_disabled');
    });
  });

  it('an owner who left the org is refused', async () => {
    mockResolveCanonicalTokenContext.mockResolvedValue(null);
    const error = await rejection(exchange());
    expect(error.getStatus()).toBe(403);
    expect(codeOf(error)).toBe('api_key_owner_not_member');
  });

  it('membership authority down: 503 and no token', async () => {
    mockResolveCanonicalTokenContext.mockRejectedValue(new Error('db down'));
    const error = await rejection(exchange());
    expect(error.getStatus()).toBe(503);
    expect(auditEvents()).toEqual([]);
  });

  it('retention posture unavailable: 503 and no token', async () => {
    mockResolveInteractiveRetentionPosture.mockRejectedValue(
      new RetentionPostureUnavailableError('org_core_unreachable'),
    );
    const error = await rejection(exchange());
    expect(error.getStatus()).toBe(503);
    expect(auditEvents()).toEqual([]);
  });

  describe('TTL cap', () => {
    it('never outlives 120 s even when the audience TTL is longer', async () => {
      process.env.PLANE_TOKEN_TTL_QUARRY_SECONDS = '3600';
      process.env.PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS = '3600';
      build();
      for (const audience of ['quarry', 'browser-broker']) {
        const bundle = await exchange(audience);
        const claims = payloadOf(bundle.token);
        expect(bundle.expiresInSeconds).toBe(API_KEY_AGENT_TOKEN_TTL_SECONDS);
        expect(Number(claims.exp) - Number(claims.iat)).toBe(
          API_KEY_AGENT_TOKEN_TTL_SECONDS,
        );
      }
    });

    it('a shorter override narrows, a longer one is ignored', () => {
      const base = {
        keyId: KEY_ID,
        actingUserId: OWNER,
        orgId: 'org-a',
        scopes: ['browser:agent'],
        retentionPosture: {
          zdr: false,
          authority: 'interactive-org-retention-policy' as const,
        },
        keyZdr: false,
        tokenId: 'jti-1',
      };
      expect(
        tokens.issueApiKeyAgentToken('quarry', { ...base, ttlSecondsOverride: 30 })
          .expiresInSeconds,
      ).toBe(30);
      expect(
        tokens.issueApiKeyAgentToken('quarry', {
          ...base,
          ttlSecondsOverride: 100_000,
        }).expiresInSeconds,
      ).toBe(API_KEY_AGENT_TOKEN_TTL_SECONDS);
    });
  });

  describe('audit failure', () => {
    it('no PubAck: 503 and the token is not returned', async () => {
      audit.publishAuditDurable.mockRejectedValue(
        new Error('Durable audit transport unavailable'),
      );
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(503);
      expect(JSON.stringify(error.getResponse())).not.toContain('eyJ');
    });

    it('a denial stays a denial when its audit cannot land', async () => {
      audit.publishAuditDurable.mockRejectedValue(new Error('down'));
      mockResolveCanonicalTokenContext.mockResolvedValue(null);
      const error = await rejection(exchange());
      expect(error.getStatus()).toBe(403);
    });
  });

  describe('zdr raise-only', () => {
    it.each([
      [false, false, false],
      [false, true, true],
      [true, false, true],
      [true, true, true],
    ])('org %s, key %s -> zdr %s', async (orgZdr, keyZdr, expected) => {
      mockResolveInteractiveRetentionPosture.mockResolvedValue({
        zdr: orgZdr,
        authority: 'interactive-org-retention-policy',
      });
      mockVerifyApiKey.mockResolvedValue(
        verifiedKey({ metadata: agentMetadata({ zdr: keyZdr }) }),
      );
      const claims = payloadOf((await exchange()).token);
      expect(claims.zdr).toBe(expected);
      const [, payload] = audit.publishAuditDurable.mock.calls[0] as [
        string,
        { details: { zdr: boolean } },
      ];
      expect(payload.details.zdr).toBe(expected);
    });
  });

  it('never logs or audits the key, also when verification throws', async () => {
    await exchange();
    mockVerifyApiKey.mockResolvedValue(
      verifiedKey({ metadata: agentMetadata({ org_id: 'org-b' }) }),
    );
    await rejection(exchange());
    mockVerifyApiKey.mockRejectedValue(new Error(`lookup failed for ${API_KEY}`));
    const error = await rejection(exchange());
    expect(error.getStatus()).toBe(503);

    const audited = JSON.stringify(audit.publishAuditDurable.mock.calls);
    expect(audited).not.toContain(API_KEY);
    expect(logged.join('\n')).not.toContain(API_KEY);
    expect(JSON.stringify(error.getResponse())).not.toContain(API_KEY);
  });
});

describe('api_key principal type is minted only by issueApiKeyAgentToken (INNH-10 CP-4)', () => {
  const originalEnv = { ...process.env };
  let tokens: ConvexTokenService;

  const base = {
    keyId: KEY_ID,
    actingUserId: OWNER,
    orgId: 'org-a',
    scopes: ['browser:agent'],
    retentionPosture: {
      zdr: false,
      authority: 'interactive-org-retention-policy' as const,
    },
    keyZdr: false,
    tokenId: 'jti-1',
  };

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    tokens = new ConvexTokenService();
  });

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it('issuePlaneToken refuses the api_key principal type and an apikey: subject', () => {
    expect(() =>
      tokens.issuePlaneToken('quarry', {
        userId: `apikey:${KEY_ID}`,
        orgId: 'org-a',
        principalType: 'api_key' as never,
      }),
    ).toThrow(/cannot be minted by issuePlaneToken/);
    expect(() =>
      tokens.issuePlaneToken('quarry', {
        userId: `apikey:${KEY_ID}`,
        orgId: 'org-a',
      }),
    ).toThrow(/api_key subject/);
  });

  it('refuses every audience but quarry and browser-broker', () => {
    expect(() =>
      tokens.issueApiKeyAgentToken('data-plane' as never, base),
    ).toThrow(/cannot be minted for data-plane/);
  });

  it('key_hosts: required on browser-broker, refused on quarry', () => {
    expect(() => tokens.issueApiKeyAgentToken('browser-broker', base)).toThrow(
      /host ceiling/,
    );
    expect(() =>
      tokens.issueApiKeyAgentToken('quarry', {
        ...base,
        keyHosts: ['example.com'],
      }),
    ).toThrow(/browser-broker tokens only/);
  });

  it('fails closed without the org posture, a jti, a scope, an owner or a bounded key id', () => {
    const broken: Array<Record<string, unknown>> = [
      { retentionPosture: { zdr: false, authority: 'service-principal-policy' } },
      { retentionPosture: undefined },
      { tokenId: '' },
      { scopes: [] },
      { actingUserId: '' },
      { actingUserId: 'apikey:other' },
      { keyId: '' },
      { keyId: 'x'.repeat(129) },
      { keyId: 'has space' },
      { orgId: '' },
    ];
    for (const override of broken) {
      expect(() =>
        tokens.issueApiKeyAgentToken('quarry', {
          ...base,
          ...override,
        } as never),
      ).toThrow();
    }
  });
});
