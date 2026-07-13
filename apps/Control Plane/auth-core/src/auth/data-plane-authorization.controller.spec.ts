import { HttpException } from '@nestjs/common';
import { generateKeyPairSync } from 'node:crypto';

import { db } from '../db';
import { ConvexTokenService } from './convex-token.service';
import {
  buildDataPlaneDecision,
  DataPlaneAuthorizationController,
  type DataPlaneDecision,
  type MembershipRecord,
} from './data-plane-authorization.controller';

const POLICY_AUDIENCE = 'control-policy';
const POLICY_SCOPE = 'data:authorization:decide';
const POLICY_ISSUER = 'https://auth.test.invalid/api/convex-auth';

const keyPair = generateKeyPairSync('rsa', { modulusLength: 2048 });
const privateKeyPem = keyPair.privateKey
  .export({ type: 'pkcs8', format: 'pem' })
  .toString();
const publicKeyPem = keyPair.publicKey
  .export({ type: 'spki', format: 'pem' })
  .toString();

type DecisionRequest = {
  userId?: string;
  orgId?: string;
  action?: string;
};

type DecisionEndpoint = {
  decide(
    authorization: string | undefined,
    body: DecisionRequest,
  ): Promise<DataPlaneDecision>;
};

type PolicyTokenClaims = {
  serviceId?: string;
  orgId?: string;
  scopes?: readonly string[];
  reason?: string;
  principalType?: 'user' | 'service';
};

function newController(service = new ConvexTokenService()): DecisionEndpoint {
  return Reflect.construct(DataPlaneAuthorizationController, [
    service,
  ]) as DecisionEndpoint;
}

function issuePolicyToken(
  service: ConvexTokenService,
  claims: PolicyTokenClaims = {},
): string {
  const serviceId = claims.serviceId ?? 'retrieval-engine';
  const principalType = claims.principalType ?? 'service';
  const issuePlaneToken = service.issuePlaneToken.bind(service) as (
    audience: string,
    tokenClaims: {
      userId: string;
      orgId: string;
      scopes: readonly string[];
      principalType: 'user' | 'service';
      serviceId?: string;
      reason?: string;
    },
  ) => { token: string };
  return issuePlaneToken(POLICY_AUDIENCE, {
    userId:
      principalType === 'service' ? `service:${serviceId}` : 'ordinary-user',
    orgId: claims.orgId ?? 'org-a',
    scopes: claims.scopes ?? [POLICY_SCOPE],
    principalType,
    ...(principalType === 'service' ? { serviceId } : {}),
    reason:
      claims.reason === undefined
        ? 'authorize a retrieval request'
        : claims.reason,
  }).token;
}

function mockMembershipLookup(rows: MembershipRecord[] | Error): void {
  const limit =
    rows instanceof Error
      ? jest.fn().mockRejectedValue(rows)
      : jest.fn().mockResolvedValue(rows);
  jest.spyOn(db, 'select').mockReturnValue({
    from: () => ({
      where: () => ({ limit }),
    }),
  } as never);
}

async function expectHttpStatus(
  operation: Promise<unknown>,
  status: number,
): Promise<void> {
  try {
    await operation;
    throw new Error(`expected HTTP ${status}`);
  } catch (error) {
    expect(error).toBeInstanceOf(HttpException);
    expect((error as HttpException).getStatus()).toBe(status);
  }
}

describe('DataPlaneAuthorizationController caller authentication', () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    process.env = {
      ...originalEnv,
      NODE_ENV: 'test',
      CONVEX_AUTH_PRIVATE_KEY_PEM: privateKeyPem,
      CONVEX_AUTH_PUBLIC_KEY_PEM: publicKeyPem,
      PLANE_TOKEN_ISSUER: POLICY_ISSUER,
      DATA_PLANE_POLICY_AUTH_AUDIENCE: POLICY_AUDIENCE,
      DATA_PLANE_POLICY_CALLER_SERVICE_ID: 'retrieval-engine',
    };
    delete process.env.CONVEX_AUTH_PRIVATE_KEY_FILE;
    delete process.env.CONVEX_AUTH_PUBLIC_KEY_FILE;
    delete process.env.DATA_PLANE_POLICY_API_KEY;
  });

  afterEach(() => {
    process.env = { ...originalEnv };
    jest.useRealTimers();
    jest.restoreAllMocks();
  });

  const request: DecisionRequest = {
    userId: 'user-a',
    orgId: 'org-a',
    action: 'data.read',
  };

  it('cryptographically verifies the canonical scoped retrieval principal', () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service);

    expect(
      service.verifyPlaneServiceToken('control-policy', token),
    ).toMatchObject({
      subject: 'service:retrieval-engine',
      serviceId: 'service:retrieval-engine',
      orgId: 'org-a',
      scopes: [POLICY_SCOPE],
      reason: 'authorize a retrieval request',
    });
  });

  it.each([
    ['missing authorization', undefined],
    ['malformed bearer', 'Bearer not-a-jwt'],
    [
      'unsigned self-forged JWT',
      `Bearer ${Buffer.from(JSON.stringify({ alg: 'none', typ: 'JWT' })).toString('base64url')}.${Buffer.from(
        JSON.stringify({
          iss: POLICY_ISSUER,
          aud: POLICY_AUDIENCE,
          sub: 'service:retrieval-engine',
          org_id: 'org-a',
          principal_type: 'service',
          service_id: 'service:retrieval-engine',
          scopes: [POLICY_SCOPE],
        }),
      ).toString('base64url')}.`,
    ],
  ])('returns 401 for %s', async (_name, authorization) => {
    await expectHttpStatus(newController().decide(authorization, request), 401);
  });

  it('returns 401 for a token whose signature was forged', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service);
    const [header, payload] = token.split('.');

    await expectHttpStatus(
      newController(service).decide(
        `Bearer ${header}.${payload}.forged-signature`,
        request,
      ),
      401,
    );
  });

  it('returns 401 for a validly signed token from the wrong issuer', async () => {
    process.env.PLANE_TOKEN_ISSUER = 'https://wrong-issuer.test.invalid';
    const token = issuePolicyToken(new ConvexTokenService());
    process.env.PLANE_TOKEN_ISSUER = POLICY_ISSUER;

    await expectHttpStatus(
      newController().decide(`Bearer ${token}`, request),
      401,
    );
  });

  it('returns 401 for a validly signed token with the wrong audience', async () => {
    const service = new ConvexTokenService();
    const token = service.issuePlaneToken('data-plane', {
      userId: 'service:retrieval-engine',
      orgId: 'org-a',
      scopes: [POLICY_SCOPE],
      principalType: 'service',
      serviceId: 'retrieval-engine',
      reason: 'authorize a retrieval request',
    }).token;

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      401,
    );
  });

  it('returns 401 when a signed caller omits the audited reason', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service, { reason: '' });

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      401,
    );
  });

  it('returns 401 for an expired caller token', async () => {
    jest.useFakeTimers({ now: new Date('2026-07-10T10:00:00.000Z') });
    process.env.PLANE_TOKEN_TTL_CONTROL_POLICY_SECONDS = '60';
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service);
    jest.setSystemTime(new Date('2026-07-10T10:01:01.000Z'));

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      401,
    );
  });

  it('returns 403 when the verified service lacks the decision scope', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service, { scopes: ['data:read'] });

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      403,
    );
  });

  it('returns 403 for a verified but unexpected service caller', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service, { serviceId: 'graph-worker' });

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      403,
    );
  });

  it('returns 403 when the request tenant conflicts with the signed tenant', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service, { orgId: 'org-b' });

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      403,
    );
  });

  it('returns 401 for an interactive user token', async () => {
    const service = new ConvexTokenService();
    const token = issuePolicyToken(service, { principalType: 'user' });

    await expectHttpStatus(
      newController(service).decide(`Bearer ${token}`, request),
      401,
    );
  });

  it('validates the request body only after caller authentication', async () => {
    const service = new ConvexTokenService();
    const authorization = `Bearer ${issuePolicyToken(service)}`;

    await expectHttpStatus(
      newController(service).decide(authorization, {
        orgId: 'org-a',
        action: 'data.read',
      }),
      400,
    );
    await expectHttpStatus(
      newController(service).decide(authorization, {
        userId: 'user-a',
        orgId: 'org-a',
        action: 'unsupported',
      }),
      400,
    );
  });

  it('preserves the versioned decision for an authenticated scoped caller', async () => {
    mockMembershipLookup([
      { role: 'member', createdAt: new Date('2026-07-10T10:00:00.000Z') },
    ]);
    const service = new ConvexTokenService();

    await expect(
      newController(service).decide(
        `Bearer ${issuePolicyToken(service)}`,
        request,
      ),
    ).resolves.toEqual({
      version: 'v1',
      allowed: true,
      role: 'member',
      permissions: ['data:read'],
      membershipRevision: '2026-07-10T10:00:00.000Z',
      reason: 'member',
    });
  });

  it('fails closed when canonical membership authority is unavailable', async () => {
    mockMembershipLookup(new Error('test-only database outage'));
    const service = new ConvexTokenService();

    await expectHttpStatus(
      newController(service).decide(
        `Bearer ${issuePolicyToken(service)}`,
        request,
      ),
      503,
    );
  });

  it('fails closed when token verification itself is unavailable', async () => {
    const service = {
      verifyPlaneServiceToken: () => {
        throw new Error('test-only verifier outage');
      },
    } as unknown as ConvexTokenService;

    await expectHttpStatus(
      newController(service).decide('Bearer a.b.c', request),
      503,
    );
  });
});

describe('DataPlaneAuthorizationController decision contract', () => {
  it('returns an explicit denial when membership is absent', () => {
    expect(buildDataPlaneDecision(null, 'data.read')).toEqual({
      version: 'v1',
      allowed: false,
      role: null,
      permissions: [],
      membershipRevision: null,
      reason: 'not_member',
    });
  });

  it('returns a versioned allow decision from canonical Better Auth membership', () => {
    const membership: MembershipRecord = {
      role: 'admin',
      createdAt: new Date('2026-07-10T10:00:00.000Z'),
    };
    expect(buildDataPlaneDecision(membership, 'data.read')).toEqual({
      version: 'v1',
      allowed: true,
      role: 'admin',
      permissions: ['data:read', 'org:data:read_all'],
      membershipRevision: '2026-07-10T10:00:00.000Z',
      reason: 'member',
    });
  });

  it('denies data.admin to an ordinary member', () => {
    const membership: MembershipRecord = {
      role: 'member',
      createdAt: new Date('2026-07-10T10:00:00.000Z'),
    };
    expect(buildDataPlaneDecision(membership, 'data.admin')).toMatchObject({
      allowed: false,
      role: 'member',
      reason: 'insufficient_role',
    });
  });
});
