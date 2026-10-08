import { ForbiddenException, UnauthorizedException } from '@nestjs/common';

// Composition spec for the delegated-identity token exchange. The sibling
// specs mock either the controller's token service (plane-token.controller
// .spec.ts) or run the token service alone (convex-token.service.spec.ts), so
// neither can see a defect that only exists when the REAL subject-token
// verification and the REAL endpoint run together against the way
// execution-core actually calls them: ONE exchange PER TOOL CALL, all of them
// presenting the SAME run-scoped `aud=session-core` bearer.
//
// Nothing below is mocked except the two module imports the controller pulls
// in for its unrelated interactive path, and the NATS audit publisher.
jest.mock('./auth', () => ({ auth: { api: { getSession: jest.fn() } } }));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: jest.fn(),
}));
jest.mock('./interactive-retention-policy', () => ({
  resolveInteractiveRetentionPosture: jest.fn(),
}));

import { ConvexTokenService } from './convex-token.service';
import { PlaneTokenController } from './plane-token.controller';

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

function payloadOf(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

describe('delegated-identity token exchange, real token service + real endpoint', () => {
  const originalEnv = { ...process.env };
  let tokens: ConvexTokenService;
  let controller: PlaneTokenController;

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-execution': {
        credential: 'test-only-execution-key',
        audiences: ['ingestion'],
        orgIds: ['org-a'],
        scopes: ['integration:read'],
        // Registry default is persistent (zdr:false); a ZDR subject must win.
        retentionByAudience: { ingestion: 'persistent' },
        tokenExchange: { ingestion: ['session-core'] },
      },
    });
    tokens = new ConvexTokenService();
    controller = new PlaneTokenController(tokens, {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    } as never);
  });

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  function runBearer(userId = 'user-a', zdr = false): string {
    return tokens.issuePlaneToken('session-core', {
      userId,
      orgId: 'org-a',
      retentionPosture: {
        zdr,
        authority: 'interactive-org-retention-policy',
      },
    }).token;
  }

  function exchange(
    subjectToken: string,
    credential = 'test-only-execution-key',
  ) {
    return controller.issueTokenExchange(
      'ingestion',
      'model-execution',
      credential,
      {
        orgId: 'org-a',
        scopes: ['integration:read'],
        reason: 'execution-core integration:read',
        subjectToken,
        subjectTokenType: 'urn:ietf:params:oauth:token-type:jwt',
      },
    );
  }

  it('lets one run bearer be exchanged once per tool call, each time for the same acting user', async () => {
    // execution-core holds ONE bearer per run and mints a fresh delegated
    // credential for every read it makes (list, key lookup, read, ...). A
    // single-use marker on the SUBJECT token would let only the first of these
    // through and hard-fail every later read of the run with a 401.
    const bearer = runBearer();

    const first = await exchange(bearer);
    const second = await exchange(bearer);
    const third = await exchange(bearer);

    for (const bundle of [first, second, third]) {
      const claims = payloadOf(bundle.token);
      expect(claims.acting_user_id).toBe('user-a');
      expect(claims.org_id).toBe('org-a');
      expect(claims.principal_type).toBe('service');
      expect(claims.aud).toBe('ingestion');
    }
  });

  it('gives every exchanged token its own unguessable jti so the resource server can enforce single use', async () => {
    const bearer = runBearer();

    const ids = new Set<string>();
    for (let i = 0; i < 5; i += 1) {
      const jti = payloadOf((await exchange(bearer)).token).jti;
      expect(typeof jti).toBe('string');
      expect((jti as string).length).toBeGreaterThanOrEqual(32);
      ids.add(jti as string);
    }
    expect(ids.size).toBe(5);
  });

  it('never stamps a jti on an ordinary /internal-token style token', () => {
    const bundle = tokens.issuePlaneToken('ingestion', {
      userId: 'service:model-execution',
      orgId: 'org-a',
      principalType: 'service',
      serviceId: 'model-execution',
      reason: 'plain org-scoped token',
      retentionPosture: { zdr: false, authority: 'service-principal-policy' },
    });
    expect(payloadOf(bundle.token).jti).toBeUndefined();
  });

  it('does not burn the run bearer when the exchange itself is refused', async () => {
    const bearer = runBearer();

    await expect(exchange(bearer, 'wrong-credential')).rejects.toBeInstanceOf(
      ForbiddenException,
    );
    // The refused attempt must not have consumed anything: the legitimate
    // caller still gets its credential with the same bearer.
    await expect(exchange(bearer)).resolves.toMatchObject({
      audience: 'ingestion',
    });
  });

  it('inherits the subject zdr posture rather than the registry default', async () => {
    const zdrBearer = runBearer('user-zdr', true);
    const claims = payloadOf((await exchange(zdrBearer)).token);
    expect(claims.zdr).toBe(true);
    expect(claims.acting_user_id).toBe('user-zdr');

    const persistentBearer = runBearer('user-p', false);
    expect(payloadOf((await exchange(persistentBearer)).token).zdr).toBe(false);
  });

  it('mints a token that lives no longer than 90 seconds', async () => {
    const claims = payloadOf((await exchange(runBearer())).token);
    expect((claims.exp as number) - (claims.iat as number)).toBeLessThanOrEqual(
      90,
    );
  });

  it('refuses a service-audience or wrong-audience token as the subject', async () => {
    const wrongAudience = tokens.issuePlaneToken('execution-core', {
      userId: 'user-a',
      orgId: 'org-a',
    }).token;
    await expect(exchange(wrongAudience)).rejects.toBeInstanceOf(
      UnauthorizedException,
    );
  });
});
