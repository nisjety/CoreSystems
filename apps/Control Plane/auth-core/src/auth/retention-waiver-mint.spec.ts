import { HttpException, Logger } from '@nestjs/common';
import { createPublicKey, generateKeyPairSync } from 'node:crypto';

// ZDR-Z-14b: the per-connection waiver mint, through the REAL user token
// controllers and the REAL ConvexTokenService with a test decision keypair.
// Only the Better Auth session, the membership read, the step-up read, the org
// posture lookup and the durable audit publisher are doubles.
const mockGetSession = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));
const mockReadFreshStepUp = jest.fn();
jest.mock('./step-up.controller', () => ({
  readFreshStepUp: mockReadFreshStepUp,
}));
const mockResolveInteractiveRetentionPosture = jest.fn();
jest.mock('./interactive-retention-policy', () => ({
  ...jest.requireActual<typeof import('./interactive-retention-policy')>(
    './interactive-retention-policy',
  ),
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import {
  ConvexTokenService,
  WAIVER_TOKEN_MAX_TTL_SECONDS,
} from './convex-token.service';
import { ModelPlaneTokenController } from './model-plane-token.controller';
import { PlaneTokenController } from './plane-token.controller';
import {
  FileKeyRetentionDecisionSigner,
  UnavailableRetentionDecisionSigner,
  type RetentionDecisionClaims,
} from './retention-decision.signer';
import {
  RETENTION_DECISION_HEADER,
  WAIVER_TOKEN_AUDIT_EVENT,
} from './retention-waiver-mint';

const keyEnvNames = [
  'CONVEX_AUTH_PRIVATE_KEY_FILE',
  'CONVEX_AUTH_PUBLIC_KEY_FILE',
  'CONVEX_AUTH_PRIVATE_KEY_PEM',
  'CONVEX_AUTH_PUBLIC_KEY_PEM',
] as const;

const USER = 'user-a';
const ORG = 'org-a';
const SESSION = 'session-a';

const decisionPair = generateKeyPairSync('rsa', { modulusLength: 2048 });
const signer = new FileKeyRetentionDecisionSigner({
  keyId: 'decision-test-key',
  privateKey: decisionPair.privateKey,
  publicKey: createPublicKey(decisionPair.privateKey),
  previous: new Map(),
});
const otherPair = generateKeyPairSync('rsa', { modulusLength: 2048 });
const otherSigner = new FileKeyRetentionDecisionSigner({
  keyId: 'decision-test-key',
  privateKey: otherPair.privateKey,
  publicKey: createPublicKey(otherPair.privateKey),
  previous: new Map(),
});

const CLAIMS: RetentionDecisionClaims = {
  userId: USER,
  orgId: ORG,
  sessionId: SESSION,
  connection: 'connection-1',
  provider: 'openai-codex-subscription',
  ackId: 'ack-1',
  digest: 'sha256:abc',
};

const ZDR_ORG = {
  zdr: true,
  authority: 'interactive-org-retention-policy',
} as const;

const WAIVER_PLANE_AUDIENCES = [
  'session-core',
  'inference-core',
  'execution-core',
  'data-plane',
  'ingestion',
  'capability-core',
  'cost-core',
] as const;
const EXCLUDED = ['browser-broker', 'sandbox-manager'] as const;

function claimsOf(token: string): Record<string, unknown> {
  return JSON.parse(
    Buffer.from(token.split('.')[1], 'base64url').toString('utf8'),
  ) as Record<string, unknown>;
}

/** The shape model-gateway and the other enforcers read. */
type EnforcerWaiver = {
  version: number;
  provider: string;
  connection: string;
  ack_id: string;
};

async function refusal(
  promise: Promise<unknown>,
): Promise<{ status: number; body: { error: Record<string, unknown> } }> {
  const error: unknown = await promise.then(
    () => {
      throw new Error('expected a refusal');
    },
    (caught: unknown) => caught,
  );
  expect(error).toBeInstanceOf(HttpException);
  const http = error as HttpException;
  return {
    status: http.getStatus(),
    body: http.getResponse() as { error: Record<string, unknown> },
  };
}

describe('per-connection waiver mint (ZDR-Z-14b)', () => {
  const originalEnv = { ...process.env };
  let tokens: ConvexTokenService;
  let audit: { publishAuditDurable: jest.Mock };

  function controllers(
    options: { signerImpl?: unknown; enabled?: boolean } = {},
  ) {
    const s = 'signerImpl' in options ? options.signerImpl : signer;
    const enabled = options.enabled ?? true;
    return {
      model: new ModelPlaneTokenController(
        tokens,
        audit as never,
        s as never,
        enabled,
      ),
      plane: new PlaneTokenController(
        tokens,
        audit as never,
        s as never,
        enabled,
      ),
    };
  }

  function request(decision?: string, extra: Record<string, string> = {}) {
    return {
      headers: {
        cookie: 'session=test',
        ...(decision === undefined
          ? {}
          : { [RETENTION_DECISION_HEADER]: decision }),
        ...extra,
      },
    } as never;
  }

  beforeEach(() => {
    process.env.NODE_ENV = 'test';
    for (const name of keyEnvNames) delete process.env[name];
    delete process.env.WEB_POLICY_CLAIM_ENABLED;
    jest.spyOn(Logger.prototype, 'log').mockImplementation(() => undefined);
    jest.spyOn(Logger.prototype, 'warn').mockImplementation(() => undefined);
    jest.spyOn(Logger.prototype, 'error').mockImplementation(() => undefined);
    tokens = new ConvexTokenService();
    audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 1 }),
    };
    mockGetSession.mockReset().mockResolvedValue({
      user: { id: USER, email: 'user-a@example.test' },
      session: { id: SESSION, activeOrganizationId: ORG },
    });
    mockResolveCanonicalTokenContext
      .mockReset()
      .mockResolvedValue({ orgId: ORG, role: 'member' });
    mockReadFreshStepUp.mockReset().mockResolvedValue(undefined);
    mockResolveInteractiveRetentionPosture
      .mockReset()
      .mockResolvedValue(ZDR_ORG);
  });

  afterEach(() => {
    jest.useRealTimers();
    jest.restoreAllMocks();
    process.env = { ...originalEnv };
  });

  it('a valid decision gives zdr false, org_zdr true and a byte-equal zdr_waiver on every waiver audience', async () => {
    const decision = signer.sign(CLAIMS);
    const { model, plane } = controllers();
    const minted = [await model.getToken(request(decision.token))];
    for (const audience of WAIVER_PLANE_AUDIENCES) {
      minted.push(await plane.getToken(audience, request(decision.token)));
    }

    const waivers = new Set<string>();
    for (const bundle of minted) {
      const claims = claimsOf(bundle.token);
      expect(claims.zdr).toBe(false);
      expect(claims.org_zdr).toBe(true);
      expect(Number(claims.exp) - Number(claims.iat)).toBeLessThanOrEqual(
        WAIVER_TOKEN_MAX_TTL_SECONDS,
      );
      const waiver = claims.zdr_waiver as EnforcerWaiver &
        Record<string, unknown>;
      expect(waiver.version).toBe(1);
      expect(waiver.provider).toBe('openai-codex-subscription');
      expect(waiver.connection).toBe('connection-1');
      expect(waiver.ack_id).toBe('ack-1');
      expect(waiver.digest).toBe('sha256:abc');
      expect(waiver.decision_jti).toBe(decision.jti);
      waivers.add(JSON.stringify(waiver));
    }
    expect(waivers.size).toBe(1);

    // One durable, content-free audit event per waiver mint.
    expect(audit.publishAuditDurable).toHaveBeenCalledTimes(minted.length);
    for (const [subject, payload] of audit.publishAuditDurable.mock.calls as [
      string,
      Record<string, unknown>,
    ][]) {
      expect(subject).toBe(
        `verevon.audit.v2.control.auth-core.${WAIVER_TOKEN_AUDIT_EVENT}`,
      );
      expect(payload).toMatchObject({
        subject: USER,
        org_id: ORG,
        event: WAIVER_TOKEN_AUDIT_EVENT,
      });
      expect(Object.keys(payload.details as object).sort()).toEqual(
        ['ack_id', 'connection', 'decision_jti', 'ttl_seconds'].sort(),
      );
      expect(JSON.stringify(payload)).not.toContain(decision.token);
      expect(JSON.stringify(payload)).not.toContain('sha256:abc');
    }
  });

  it('the waiver lifetime cap holds even when the audience TTL is longer', async () => {
    process.env.PLANE_TOKEN_TTL_DATA_PLANE_SECONDS = '3600';
    tokens = new ConvexTokenService();
    const bundle = await controllers().plane.getToken(
      'data-plane',
      request(signer.sign(CLAIMS).token),
    );
    const claims = claimsOf(bundle.token);
    expect(Number(claims.exp) - Number(claims.iat)).toBeLessThanOrEqual(
      WAIVER_TOKEN_MAX_TTL_SECONDS,
    );
  });

  it.each(EXCLUDED)(
    '%s: 412 zdr_waiver_route_mismatch and no token',
    async (audience) => {
      const { plane } = controllers();
      const refused = await refusal(
        plane.getToken(audience, request(signer.sign(CLAIMS).token)),
      );
      expect(refused.status).toBe(412);
      expect(refused.body.error.code).toBe('zdr_waiver_route_mismatch');
      expect(Object.keys(refused.body.error).sort()).toEqual([
        'action',
        'code',
        'message',
        'retryable',
      ]);
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
      expect(mockResolveInteractiveRetentionPosture).not.toHaveBeenCalled();
    },
  );

  describe('412 zdr_posture_mismatch and no token', () => {
    const cases: [string, () => string][] = [
      [
        'an expired decision',
        () => {
          jest.useFakeTimers({
            now: Date.now() - 120_000,
            doNotFake: ['nextTick', 'setImmediate'],
          });
          const token = signer.sign(CLAIMS).token;
          jest.useRealTimers();
          return token;
        },
      ],
      [
        'a tampered decision',
        () => {
          const [h, p, s] = signer.sign(CLAIMS).token.split('.');
          const payload = JSON.parse(
            Buffer.from(p, 'base64url').toString('utf8'),
          );
          payload.connection = 'connection-2';
          return `${h}.${Buffer.from(JSON.stringify(payload)).toString('base64url')}.${s}`;
        },
      ],
      [
        'a decision signed by another key',
        () => otherSigner.sign(CLAIMS).token,
      ],
      [
        'another session',
        () => signer.sign({ ...CLAIMS, sessionId: 'session-b' }).token,
      ],
      [
        'another user',
        () => signer.sign({ ...CLAIMS, userId: 'user-b' }).token,
      ],
      ['another org', () => signer.sign({ ...CLAIMS, orgId: 'org-b' }).token],
      ['garbage', () => 'not-a-decision'],
      ['an oversized header', () => 'a'.repeat(4097)],
    ];

    it.each(cases)('%s (model-plane route)', async (_case, make) => {
      const refused = await refusal(
        controllers().model.getToken(request(make())),
      );
      expect(refused.status).toBe(412);
      expect(refused.body.error.code).toBe('zdr_posture_mismatch');
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    });

    it.each(cases)('%s (plane route)', async (_case, make) => {
      const refused = await refusal(
        controllers().plane.getToken('session-core', request(make())),
      );
      expect(refused.status).toBe(412);
      expect(refused.body.error.code).toBe('zdr_posture_mismatch');
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    });

    it('an org that is no longer ZDR', async () => {
      mockResolveInteractiveRetentionPosture.mockResolvedValue({
        ...ZDR_ORG,
        zdr: false,
      });
      const refused = await refusal(
        controllers().model.getToken(request(signer.sign(CLAIMS).token)),
      );
      expect(refused.status).toBe(412);
      expect(refused.body.error.code).toBe('zdr_posture_mismatch');
    });
  });

  describe('503 zdr_decision_unavailable and no token', () => {
    it('the switch off', async () => {
      const { model, plane } = controllers({ enabled: false });
      for (const pending of [
        model.getToken(request(signer.sign(CLAIMS).token)),
        plane.getToken('data-plane', request(signer.sign(CLAIMS).token)),
      ]) {
        const refused = await refusal(pending);
        expect(refused.status).toBe(503);
        expect(refused.body.error.code).toBe('zdr_decision_unavailable');
      }
      expect(audit.publishAuditDurable).not.toHaveBeenCalled();
    });

    it('no signer provided, or the unavailable signer', async () => {
      for (const signerImpl of [
        undefined,
        new UnavailableRetentionDecisionSigner(),
      ]) {
        const refused = await refusal(
          controllers({ signerImpl }).model.getToken(
            request(signer.sign(CLAIMS).token),
          ),
        );
        expect(refused.status).toBe(503);
        expect(refused.body.error.code).toBe('zdr_decision_unavailable');
      }
    });

    it('the durable audit cannot be published', async () => {
      audit.publishAuditDurable.mockRejectedValue(new Error('no PubAck'));
      const refused = await refusal(
        controllers().model.getToken(request(signer.sign(CLAIMS).token)),
      );
      expect(refused.status).toBe(503);
      expect(refused.body.error.code).toBe('zdr_decision_unavailable');
      expect(JSON.stringify(refused.body)).not.toContain('eyJ');
    });
  });

  it('without the header nothing changes: a ZDR org gets zdr true and no waiver claims', async () => {
    const { model, plane } = controllers();
    for (const bundle of [
      await model.getToken(request()),
      await plane.getToken('data-plane', request()),
    ]) {
      const claims = claimsOf(bundle.token);
      expect(claims.zdr).toBe(true);
      expect(claims).not.toHaveProperty('zdr_waiver');
      expect(claims).not.toHaveProperty('org_zdr');
    }
    expect(audit.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('a request header zdr false never lowers a ZDR org', async () => {
    const { model, plane } = controllers();
    const extra = { 'x-zdr': 'false', 'x-zdr-waiver': 'true' };
    for (const bundle of [
      await model.getToken(request(undefined, extra)),
      await plane.getToken('session-core', request(undefined, extra)),
    ]) {
      expect(claimsOf(bundle.token).zdr).toBe(true);
      expect(claimsOf(bundle.token)).not.toHaveProperty('zdr_waiver');
    }
  });

  it('the issuer refuses a waiver on a service token or an excluded audience', () => {
    const verified = signer.verify(signer.sign(CLAIMS).token);
    expect(() =>
      tokens.issuePlaneToken('session-core', {
        userId: 'service:worker',
        orgId: ORG,
        principalType: 'service',
        serviceId: 'worker',
        retentionPosture: { authority: 'service-principal-policy', zdr: true },
        retentionWaiver: verified,
      } as never),
    ).toThrow();
    expect(() =>
      tokens.issuePlaneToken('browser-broker', {
        userId: USER,
        orgId: ORG,
        retentionPosture: ZDR_ORG,
        retentionWaiver: verified,
      }),
    ).toThrow();
  });

  it('the issuer refuses a structurally equal waiver that the signer did not verify', () => {
    const verified = signer.verify(signer.sign(CLAIMS).token);
    const forged = { ...verified, connection: 'connection-9' };
    let token: string | undefined;
    try {
      token = tokens.issuePlaneToken('session-core', {
        userId: USER,
        orgId: ORG,
        retentionPosture: ZDR_ORG,
        retentionWaiver: forged as never,
      }).token;
    } catch {
      token = undefined;
    }
    // A request-shaped object must never become a signed zdr_waiver claim.
    expect(
      token === undefined ? undefined : claimsOf(token).zdr_waiver,
    ).toBeUndefined();
  });
});
