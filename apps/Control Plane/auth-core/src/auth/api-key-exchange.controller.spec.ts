import { HttpException, Logger } from '@nestjs/common';

const mockVerifyApiKey = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
const mockResolveInteractiveRetentionPosture = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { verifyApiKey: mockVerifyApiKey } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
// Only the org posture lookup is replaced; the 503 error class stays real so
// the controller's `instanceof` check is exercised.
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import { ApiKeyExchangeController } from './api-key-exchange.controller';
import { ConvexTokenService } from './convex-token.service';
import { RetentionPostureUnavailableError } from './interactive-retention-policy';

const RAW_KEY = 'user_test-only-raw-api-key-0123456789abcdef';
const GATEWAY_CREDENTIAL = 'test-only-verevon-gateway-key';

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

function claimsOf(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

function verifiedKey(override: Record<string, unknown> = {}) {
  return {
    valid: true,
    error: null,
    key: {
      id: 'key-1',
      configId: 'user-keys',
      referenceId: 'user-a',
      enabled: true,
      expiresAt: null,
      permissions: { agent: ['runs'] },
      metadata: { org_id: 'org-a' },
      ...override,
    },
  };
}

async function refusalOf(promise: Promise<unknown>): Promise<{
  status: number;
  body: { error: { code: string; message: string; details: { reason: string } } };
}> {
  const error = await promise.then(
    () => {
      throw new Error('expected a refusal');
    },
    (caught: unknown) => caught,
  );
  expect(error).toBeInstanceOf(HttpException);
  const http = error as HttpException;
  return {
    status: http.getStatus(),
    body: http.getResponse() as {
      error: { code: string; message: string; details: { reason: string } };
    },
  };
}

describe('ApiKeyExchangeController', () => {
  const originalEnv = { ...process.env };
  let tokens: ConvexTokenService;
  let audit: { publishAuditDurable: jest.Mock };
  let controller: ApiKeyExchangeController;

  const body = (override: Record<string, unknown> = {}) => ({
    apiKey: RAW_KEY,
    orgId: 'org-a',
    scopes: ['agent:runs'],
    ...override,
  });

  const exchange = (
    audience = 'model-gateway',
    requestBody: Record<string, unknown> = body(),
    serviceId = 'verevon-gateway',
    credential = GATEWAY_CREDENTIAL,
  ) => controller.exchange(audience, serviceId, credential, requestBody);

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    process.env.API_KEY_ENABLED = 'true';
    process.env.API_KEY_AGENT_EXCHANGE_ENABLED = 'true';
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'verevon-gateway': {
        credential: GATEWAY_CREDENTIAL,
        audiences: ['model-gateway'],
        tenancy: 'directory',
        orgIds: [],
        scopes: ['mail-task:prepare-thread'],
        apiKeyExchange: ['model-gateway', 'session-core', 'inference-core'],
      },
      'other-worker': {
        credential: 'test-only-other-worker-key',
        audiences: ['model-gateway'],
        tenancy: 'directory',
        orgIds: [],
        scopes: ['mail-task:prepare-thread'],
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
    tokens = new ConvexTokenService();
    audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    };
    controller = new ApiKeyExchangeController(tokens, audit as never);
  });

  afterEach(() => {
    process.env = { ...originalEnv };
    jest.restoreAllMocks();
  });

  it.each<[string, Record<string, string | undefined>]>([
    ['the exchange switch unset', { API_KEY_AGENT_EXCHANGE_ENABLED: undefined }],
    ['the exchange switch not the literal true', { API_KEY_AGENT_EXCHANGE_ENABLED: 'TRUE' }],
    ['the api-key plugin switch off', { API_KEY_ENABLED: 'false' }],
  ])('answers 404 like an unknown audience with %s', async (_case, env) => {
    for (const [name, value] of Object.entries(env)) {
      if (value === undefined) delete process.env[name];
      else process.env[name] = value;
    }
    const off = await refusalOf(exchange());
    process.env.API_KEY_ENABLED = 'true';
    process.env.API_KEY_AGENT_EXCHANGE_ENABLED = 'true';
    const unknown = await refusalOf(exchange('data-plane'));

    expect(off).toEqual(unknown);
    expect(off).toEqual({
      status: 404,
      body: {
        error: {
          code: 'not_found',
          message: 'Unknown API-key exchange audience',
          details: { reason: 'audience_not_found' },
        },
      },
    });
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('refuses a caller without an apiKeyExchange entry before reading the key, and does not audit it', async () => {
    const refused = await refusalOf(
      exchange('model-gateway', body(), 'other-worker', 'test-only-other-worker-key'),
    );
    expect(refused.status).toBe(403);
    expect(refused.body.error.details).toEqual({
      reason: 'caller_not_authorized',
    });
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('refuses an opted-in caller for an audience its list does not name, and a wrong credential', async () => {
    expect((await refusalOf(exchange('quarry', body({ scopes: ['browser:agent'] })))).status).toBe(403);
    expect(
      (await refusalOf(exchange('model-gateway', body(), 'verevon-gateway', 'test-only-wrong-key'))).status,
    ).toBe(403);
    expect(mockVerifyApiKey).not.toHaveBeenCalled();
  });

  it('answers 503 when the registry cannot be read', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = '{not json';
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(503);
    expect(refused.body.error.details).toEqual({
      reason: 'registry_unavailable',
    });
  });

  it.each<[string, Record<string, unknown>]>([
    ['a missing key', { apiKey: undefined }],
    ['a missing org', { orgId: '' }],
    ['no scopes', { scopes: [] }],
    ['a non-string scope', { scopes: [1] }],
  ])('refuses %s as a malformed request', async (_case, override) => {
    const refused = await refusalOf(exchange('model-gateway', body(override)));
    expect(refused.status).toBe(400);
    expect(refused.body.error.details).toEqual({ reason: 'malformed_request' });
  });

  it('refuses a caller-selected retention posture', async () => {
    const refused = await refusalOf(
      exchange('model-gateway', body({ zdr: false })),
    );
    expect(refused.status).toBe(400);
    expect(refused.body.error.details).toEqual({
      reason: 'retention_not_selectable',
    });
  });

  it('mints a 120 s model-gateway api_key token for the key, attributing the owner only, and audits it', async () => {
    const result = await exchange();

    expect(mockVerifyApiKey).toHaveBeenCalledWith({ body: { key: RAW_KEY } });
    expect(mockResolveCanonicalTokenContext).toHaveBeenCalledWith(
      'user-a',
      'org-a',
    );
    expect(Object.keys(result)).toEqual(['data']);
    expect(result.data.audience).toBe('model-gateway');
    const claims = claimsOf(result.data.token);
    expect(claims).toMatchObject({
      aud: 'model-gateway',
      sub: 'apikey:key-1',
      principal_type: 'api_key',
      key_id: 'key-1',
      acting_user_id: 'user-a',
      org_id: 'org-a',
      scopes: ['agent:runs'],
      zdr: false,
    });
    expect(claims.user_id).toBeUndefined();
    expect(claims.service_id).toBeUndefined();
    expect(claims.email).toBeUndefined();
    expect(claims.step_up).toBeUndefined();
    expect(typeof claims.jti).toBe('string');
    expect((claims.exp as number) - (claims.iat as number)).toBeLessThanOrEqual(
      120,
    );

    expect(audit.publishAuditDurable).toHaveBeenCalledTimes(1);
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      'verevon.audit.v2.control.auth-core.api_key_token_exchanged',
      expect.objectContaining({
        org_id: 'org-a',
        event: 'api_key_token_exchanged',
        subject: 'apikey:key-1',
        resource_id: 'model-gateway',
        outcome: 'ok',
        details: {
          audience: 'model-gateway',
          key_id: 'key-1',
          acting_user_id: 'user-a',
          scopes: ['agent:runs'],
          zdr: false,
          key_zdr: false,
          calling_service: 'service:verevon-gateway',
          org_binding: 'directory',
        },
      }),
    );
  });

  it('mints a fresh jti on every exchange', async () => {
    const first = claimsOf((await exchange()).data.token);
    const second = claimsOf((await exchange()).data.token);
    expect(first.jti).not.toEqual(second.jti);
  });

  it('mints a session-core plane token whose zdr the key flag raises', async () => {
    mockVerifyApiKey.mockResolvedValue(
      verifiedKey({ metadata: { org_id: 'org-a', zdr: true } }),
    );
    const claims = claimsOf((await exchange('session-core')).data.token);
    expect(claims).toMatchObject({
      aud: 'session-core',
      principal_type: 'api_key',
      zdr: true,
    });
    expect((claims.exp as number) - (claims.iat as number)).toBeLessThanOrEqual(
      120,
    );
  });

  it('keeps a ZDR org zero-retention whatever the key says', async () => {
    mockResolveInteractiveRetentionPosture.mockResolvedValue({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
    mockVerifyApiKey.mockResolvedValue(
      verifiedKey({ metadata: { org_id: 'org-a', zdr: false } }),
    );
    expect(claimsOf((await exchange()).data.token).zdr).toBe(true);
  });

  it('issues no token and answers 503 when the org posture cannot be verified', async () => {
    mockResolveInteractiveRetentionPosture.mockRejectedValue(
      new RetentionPostureUnavailableError('org_core_unreachable'),
    );
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(503);
    expect(refused.body.error.code).toBe('retention_posture_unavailable');
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('issues no token and answers 503 when the durable audit cannot be written', async () => {
    audit.publishAuditDurable.mockRejectedValue(new Error('no PubAck'));
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(503);
    expect(refused.body.error.details).toEqual({ reason: 'audit_unavailable' });
    expect(JSON.stringify(refused.body)).not.toMatch(/eyJ/);
  });

  it('answers 503 when the key cannot be verified at all', async () => {
    mockVerifyApiKey.mockRejectedValue(new Error('adapter down'));
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(503);
    expect(refused.body.error.details).toEqual({
      reason: 'key_verification_unavailable',
    });
  });

  it.each<[string, number, string]>([
    ['KEY_DISABLED', 401, 'key_disabled'],
    ['KEY_EXPIRED', 401, 'key_expired'],
    ['RATE_LIMITED', 429, 'key_rate_limited'],
    ['INVALID_API_KEY', 401, 'key_invalid'],
  ])('refuses a key the plugin answers %s and audits the refusal', async (code, status, reason) => {
    mockVerifyApiKey.mockResolvedValue({ valid: false, error: { code }, key: null });
    const refused = await refusalOf(exchange());
    expect(refused).toEqual({
      status,
      body: {
        error: {
          code: 'api_key_exchange_denied',
          message: 'The API key cannot be exchanged for this audience',
          details: { reason },
        },
      },
    });
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      'verevon.audit.v2.control.auth-core.api_key_token_denied',
      expect.objectContaining({
        org_id: 'org-a',
        outcome: 'denied',
        details: {
          audience: 'model-gateway',
          reason,
          scopes: ['agent:runs'],
          calling_service: 'service:verevon-gateway',
        },
      }),
    );
    expect(mockResolveCanonicalTokenContext).not.toHaveBeenCalled();
  });

  it('refuses an org key and names the key, not an organization, in the audit', async () => {
    mockVerifyApiKey.mockResolvedValue(
      verifiedKey({ configId: 'org-keys', referenceId: 'org-a' }),
    );
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(403);
    expect(refused.body.error.details).toEqual({ reason: 'org_key_refused' });
    const [, payload] = audit.publishAuditDurable.mock.calls[0] as [
      string,
      { details: Record<string, unknown> },
    ];
    expect(payload.details.key_id).toBe('key-1');
    expect(payload.details.acting_user_id).toBeUndefined();
  });

  it.each<
    [
      string,
      { orgId: string; role: string } | null,
      ReturnType<typeof verifiedKey>,
      string,
    ]
  >([
    ['an owner who left the org', null, verifiedKey(), 'owner_not_member'],
    [
      'a key bound to another org',
      { orgId: 'org-a', role: 'member' },
      verifiedKey({ metadata: { org_id: 'org-b' } }),
      'org_mismatch',
    ],
    [
      'a key without the scope',
      { orgId: 'org-a', role: 'member' },
      verifiedKey({ permissions: { browser: ['agent'] } }),
      'scope_not_granted',
    ],
  ])('refuses %s with 403 and no token', async (_case, membership, verified, reason) => {
    mockResolveCanonicalTokenContext.mockResolvedValue(membership);
    mockVerifyApiKey.mockResolvedValue(verified);
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(403);
    expect(refused.body.error.details).toEqual({ reason });
  });

  it('still refuses when the refusal cannot be audited', async () => {
    audit.publishAuditDurable.mockRejectedValue(new Error('no PubAck'));
    mockResolveCanonicalTokenContext.mockResolvedValue(null);
    const refused = await refusalOf(exchange());
    expect(refused.status).toBe(403);
    expect(refused.body.error.details).toEqual({ reason: 'owner_not_member' });
  });

  it('never writes the key to a log, an audit event or a refusal', async () => {
    const logged: unknown[][] = [];
    for (const method of ['log', 'error', 'warn', 'debug', 'verbose'] as const) {
      jest
        .spyOn(Logger.prototype, method)
        .mockImplementation((...args: unknown[]) => {
          logged.push(args);
        });
    }
    const consoleCalls: unknown[][] = [];
    for (const method of ['log', 'error', 'warn', 'info', 'debug'] as const) {
      jest.spyOn(console, method).mockImplementation((...args: unknown[]) => {
        consoleCalls.push(args);
      });
    }

    await exchange();
    audit.publishAuditDurable.mockRejectedValue(new Error('no PubAck'));
    const failedAudit = await refusalOf(exchange());
    mockVerifyApiKey.mockResolvedValue({
      valid: false,
      error: { code: 'KEY_DISABLED' },
      key: null,
    });
    const denied = await refusalOf(exchange());
    mockVerifyApiKey.mockRejectedValue(new Error(RAW_KEY));
    const unavailable = await refusalOf(exchange());

    const everything = JSON.stringify([
      logged,
      consoleCalls,
      audit.publishAuditDurable.mock.calls,
      failedAudit,
      denied,
      unavailable,
    ]);
    expect(everything).not.toContain(RAW_KEY);
    expect(everything).not.toContain(RAW_KEY.slice(0, 12));
  });
});
