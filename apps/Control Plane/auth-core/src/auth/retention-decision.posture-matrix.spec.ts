import { HttpException } from '@nestjs/common';
import { Test } from '@nestjs/testing';
import {
  createHash,
  createPublicKey,
  generateKeyPairSync,
  type KeyObject,
} from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

// The d_plan_lapsed_* rows need org-core's effective posture read, so the
// switch is on before the policy module is used and re-read below.
process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED = 'true';
process.env.ORG_CORE_SERVICE_TOKEN =
  'unit-spec-service-token-0123456789abcdef0123';

const mockGetSession = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import {
  resetEffectivePostureReadForTests,
  resetInteractiveRetentionCacheForTests,
} from './interactive-retention-policy';
import {
  IntegrationConnectionPostureClient,
  type ConnectionPostureResult,
} from './integration-connection-posture.client';
import { RetentionDecisionController } from './retention-decision.controller';
import {
  RetentionDecisionService,
  ZDR_WAIVER_ENABLED_TOKEN,
  loadRetentionDecisionRefusals,
  retentionDecisionRefusalStatus,
  zdrWaiverEnabledProvider,
  type RetentionDecisionOutcome,
} from './retention-decision.service';
import {
  FileKeyRetentionDecisionSigner,
  RETENTION_DECISION_SIGNER,
  RetentionDecisionInvalidError,
  UnavailableRetentionDecisionSigner,
  type RetentionDecisionSigner,
} from './retention-decision.signer';

const FIXTURES = join(__dirname, 'fixtures');
const MODEL_PLANE_CONTRACTS = join(
  __dirname,
  '../../../../Model Plane/api/contracts',
);

type MatrixInputs = {
  org_zdr_intent: boolean | null;
  org_entitled: boolean | null;
  explicit_zdr: boolean;
  space_zdr: boolean;
  waiver_enabled: boolean;
  connection: 'none' | 'own' | 'other_member' | 'other_org';
  capability: 'supported' | 'not_supported' | 'cannot_verify' | null;
  waiver_route: boolean | null;
  ack: string;
  outage: 'org_core' | 'auth_core' | 'integration_core' | null;
};
type MatrixRow = {
  id: string;
  inputs: MatrixInputs;
  expect: {
    decision: 'zdr' | 'persistent' | 'waived' | 'refuse';
    code: string;
    decision_call: boolean;
    claims: { zdr: boolean; org_zdr: boolean; waiver: boolean } | null;
  };
};
type Matrix = {
  vocabulary: { retention_classes: Record<string, string> };
  decision_rows: MatrixRow[];
};

const matrix = JSON.parse(
  readFileSync(join(FIXTURES, 'zdr-posture-matrix.json'), 'utf8'),
) as Matrix;

const CONNECTION_ID = 'connection-1';
const WAIVER_PROVIDER = 'openai-codex-subscription';

function sha256Of(path: string): string {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

describe('the vendored ZDR contracts', () => {
  it.each(['zdr-posture-matrix.json', 'zdr-refusals.json'])(
    '%s hashes to its pin',
    (name) => {
      const pin = readFileSync(join(FIXTURES, `${name}.sha256`), 'utf8');
      expect(pin.trim().split(/\s+/)[0]).toBe(sha256Of(join(FIXTURES, name)));
    },
  );

  it('keeps the refusals catalogue byte-identical to the Model Plane source when that tree is present', () => {
    const source = join(MODEL_PLANE_CONTRACTS, 'zdr-refusals.json');
    if (!existsSync(source)) return;
    expect(readFileSync(join(FIXTURES, 'zdr-refusals.json'))).toEqual(
      readFileSync(source),
    );
  });

  it('refuses to load a catalogue that lacks one of the codes it answers', () => {
    expect(() => loadRetentionDecisionRefusals({ codes: [] })).toThrow(
      /no usable entry/,
    );
    expect(() => loadRetentionDecisionRefusals(undefined)).toThrow(
      /no usable entry/,
    );
  });

  it('answers 503 for zdr_decision_unavailable and 412 for the other codes', () => {
    expect(retentionDecisionRefusalStatus('zdr_decision_unavailable')).toBe(
      503,
    );
    expect(
      retentionDecisionRefusalStatus('zdr_connection_not_supported'),
    ).toBe(412);
  });
});

describe('ZDR_WAIVER_ENABLED provider', () => {
  const factory = (
    zdrWaiverEnabledProvider as unknown as { useFactory: () => boolean }
  ).useFactory;
  const original = process.env.ZDR_WAIVER_ENABLED;
  afterEach(() => {
    if (original === undefined) delete process.env.ZDR_WAIVER_ENABLED;
    else process.env.ZDR_WAIVER_ENABLED = original;
  });

  it('is on only for the strict literal true', () => {
    delete process.env.ZDR_WAIVER_ENABLED;
    expect(factory()).toBe(false);
    for (const value of ['', '1', 'TRUE', 'True', ' true', 'yes', 'false']) {
      process.env.ZDR_WAIVER_ENABLED = value;
      expect(factory()).toBe(false);
    }
    process.env.ZDR_WAIVER_ENABLED = 'true';
    expect(factory()).toBe(true);
  });
});

describe('RetentionDecisionService against the ZDR posture matrix', () => {
  let signer: FileKeyRetentionDecisionSigner;
  let publicKey: KeyObject;
  let fetchSpy: jest.SpyInstance;

  beforeAll(() => {
    process.env.AUTH_CORE_ZDR_EFFECTIVE_POSTURE_ENABLED = 'true';
    resetEffectivePostureReadForTests();
    const pair = generateKeyPairSync('rsa', { modulusLength: 2048 });
    publicKey = createPublicKey(pair.privateKey);
    signer = new FileKeyRetentionDecisionSigner({
      keyId: 'matrix-spec-key',
      privateKey: pair.privateKey,
      publicKey,
      previous: new Map(),
    });
  });

  beforeEach(() => {
    resetInteractiveRetentionCacheForTests();
  });

  afterEach(() => {
    fetchSpy?.mockRestore();
  });

  /** An org-core double: the effective posture the matrix row describes. */
  function orgCoreServes(inputs: MatrixInputs): void {
    fetchSpy = jest
      .spyOn(global, 'fetch')
      .mockImplementation(async () => {
        if (inputs.outage === 'org_core') {
          throw new TypeError('fetch failed');
        }
        const intent = inputs.org_zdr_intent === true;
        const entitled = inputs.org_entitled === true;
        return {
          ok: true,
          status: 200,
          json: () =>
            Promise.resolve({
              zdrIntent: intent,
              entitled,
              // Lapse rule: a lapsed org stays ZDR (o_plan_lapsed_keeps_zdr).
              effectiveZdr: intent,
              lapsed: intent && !entitled,
            }),
        } as never;
      });
  }

  /** The integration-corev2 answer the matrix row describes. */
  function connectionAnswer(inputs: MatrixInputs): ConnectionPostureResult {
    if (inputs.outage === 'integration_core') return { kind: 'unavailable' };
    if (inputs.connection === 'other_member' || inputs.connection === 'other_org') {
      return { kind: 'not_owner' };
    }
    const valid = inputs.ack === 'valid';
    const supported = inputs.capability === 'supported';
    return {
      kind: 'posture',
      providerKey: inputs.waiver_route ? WAIVER_PROVIDER : 'unknown-provider',
      capability: inputs.capability ?? 'cannot_verify',
      waiverRoute: inputs.waiver_route === true,
      ackState: valid ? 'acknowledged' : supported ? 'not_required' : 'required',
      ackReason: valid ? '' : inputs.ack === 'none' ? 'no_acknowledgement' : inputs.ack,
      ackId: valid ? 'ack-1' : '',
      digest: 'digest-1',
      textVersion: 'v1',
    };
  }

  async function serviceWith(
    waiverEnabled: boolean,
    answer: ConnectionPostureResult | Error,
    activeSigner: RetentionDecisionSigner = signer,
  ): Promise<{ service: RetentionDecisionService; read: jest.Mock }> {
    const read = jest.fn(async () => {
      if (answer instanceof Error) throw answer;
      return answer;
    });
    const moduleRef = await Test.createTestingModule({
      providers: [
        RetentionDecisionService,
        { provide: RETENTION_DECISION_SIGNER, useValue: activeSigner },
        {
          provide: IntegrationConnectionPostureClient,
          useValue: { readConnectionPosture: read },
        },
        { provide: ZDR_WAIVER_ENABLED_TOKEN, useValue: waiverEnabled },
      ],
    }).compile();
    return { service: moduleRef.get(RetentionDecisionService), read };
  }

  const input = (row: MatrixRow) => ({
    userId: 'user-a',
    orgId: 'org-a',
    sessionId: 'session-a',
    connectionId: row.inputs.connection === 'none' ? null : CONNECTION_ID,
    provider: null,
    explicitZdr: row.inputs.explicit_zdr,
  });

  // An Auth Core outage is the BFF's case: it cannot call the endpoint, so a
  // row that needs the call has nothing for this function to answer. The rows
  // that make no call are still evaluated (the outage changes nothing there).
  const evaluated = matrix.decision_rows.filter(
    (row) => !(row.inputs.outage === 'auth_core' && row.expect.decision_call),
  );

  it('evaluates every decision row except the ones that need a dead Auth Core', () => {
    expect(matrix.decision_rows.length).toBeGreaterThan(0);
    const skipped = matrix.decision_rows.filter((row) => !evaluated.includes(row));
    expect(skipped.length).toBeGreaterThan(0);
    for (const row of skipped) {
      expect(row.inputs.outage).toBe('auth_core');
      expect(row.expect.decision).toBe('refuse');
      expect(row.expect.code).toBe('zdr_decision_unavailable');
    }
  });

  it.each(evaluated.map((row) => [row.id, row] as const))(
    '%s',
    async (_id, row) => {
      orgCoreServes(row.inputs);
      const { service, read } = await serviceWith(
        row.inputs.waiver_enabled,
        connectionAnswer(row.inputs),
      );

      const outcome: RetentionDecisionOutcome = await service.decide(input(row));

      if (row.expect.decision === 'refuse') {
        expect(outcome.kind).toBe('refused');
        if (outcome.kind === 'refused') {
          expect(outcome.code).toBe(row.expect.code);
        }
        return;
      }

      expect(outcome.kind).toBe('decided');
      if (outcome.kind !== 'decided') return;
      expect(outcome.decision).toBe(row.expect.decision);
      expect(outcome.reason).toBe(row.expect.code);
      expect(outcome.retentionClass).toBe(
        matrix.vocabulary.retention_classes[row.expect.decision],
      );
      expect(outcome.effectiveZdr).toBe(row.expect.decision === 'zdr');

      // The claims a token minted under this decision would carry.
      const claims = row.expect.claims as NonNullable<typeof row.expect.claims>;
      expect({
        zdr: outcome.orgZdr && outcome.decision === 'zdr',
        org_zdr: outcome.orgZdr,
        waiver: outcome.decision === 'waived',
      }).toEqual(claims);

      // A token exists exactly when the decision is waived, and it verifies.
      if (outcome.decision === 'waived') {
        const signed = outcome.signed;
        expect(signed).toBeDefined();
        const verified = signer.verify((signed as { token: string }).token);
        expect(verified.userId).toBe('user-a');
        expect(verified.orgId).toBe('org-a');
        expect(verified.sessionId).toBe('session-a');
        expect(verified.connection).toBe(CONNECTION_ID);
        expect(verified.provider).toBe(WAIVER_PROVIDER);
        expect(verified.ackId).toBe('ack-1');
        expect(verified.digest).toBe('digest-1');
        const token = (signed as { token: string }).token;
        const [header, payload, signature] = token.split('.');
        const forged = Buffer.from(
          JSON.stringify({
            ...JSON.parse(Buffer.from(payload, 'base64url').toString('utf8')),
            sub: 'user-b',
          }),
        ).toString('base64url');
        expect(() => signer.verify(`${header}.${forged}.${signature}`)).toThrow(
          RetentionDecisionInvalidError,
        );
      } else {
        expect(outcome.signed).toBeUndefined();
      }

      // The connection is read only when it is named and the org is ZDR or the
      // turn asked for ZDR itself.
      const orgZdr = row.inputs.org_zdr_intent === true;
      const shouldRead =
        row.inputs.connection !== 'none' && (orgZdr || row.inputs.explicit_zdr);
      expect(read).toHaveBeenCalledTimes(shouldRead ? 1 : 0);
    },
  );

  it('reads nothing when the org posture is unknown', async () => {
    const row = matrix.decision_rows.find(
      (candidate) => candidate.id === 'd_org_core_outage_with_connection',
    ) as MatrixRow;
    orgCoreServes(row.inputs);
    const { service, read } = await serviceWith(true, connectionAnswer(row.inputs));
    const outcome = await service.decide(input(row));
    expect(outcome).toMatchObject({
      kind: 'refused',
      code: 'zdr_decision_unavailable',
    });
    expect(read).not.toHaveBeenCalled();
  });

  describe('predicates the matrix rows do not pin on their own', () => {
    const zdrOrg: MatrixInputs = {
      org_zdr_intent: true,
      org_entitled: true,
      explicit_zdr: false,
      space_zdr: false,
      waiver_enabled: true,
      connection: 'own',
      capability: 'not_supported',
      waiver_route: true,
      ack: 'valid',
      outage: null,
    };
    const posture = (
      overrides: Partial<Extract<ConnectionPostureResult, { kind: 'posture' }>>,
    ): ConnectionPostureResult => ({
      ...(connectionAnswer(zdrOrg) as Extract<
        ConnectionPostureResult,
        { kind: 'posture' }
      >),
      ...overrides,
    });
    const ask = {
      userId: 'user-a',
      orgId: 'org-a',
      sessionId: 'session-a',
      connectionId: CONNECTION_ID,
      provider: null,
      explicitZdr: false,
    };

    it('refuses an explicit request on a cannot_verify connection even with a valid acknowledgement', async () => {
      orgCoreServes(zdrOrg);
      const { service } = await serviceWith(
        true,
        posture({ capability: 'cannot_verify' }),
      );
      await expect(service.decide({ ...ask, explicitZdr: true })).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_explicit_request_not_supported_by_connection',
      });
      // Without the explicit request, the same connection is waived.
      await expect(service.decide(ask)).resolves.toMatchObject({
        kind: 'decided',
        decision: 'waived',
      });
    });

    it('refuses an unassessed connection as unavailable, never as not supported', async () => {
      orgCoreServes(zdrOrg);
      const { service } = await serviceWith(
        true,
        posture({ ackState: 'unassessed', ackId: '' }),
      );
      await expect(service.decide(ask)).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_decision_unavailable',
      });
    });

    it('refuses a request provider other than the connection provider', async () => {
      orgCoreServes(zdrOrg);
      const { service } = await serviceWith(true, posture({}));
      await expect(
        service.decide({ ...ask, provider: 'some-other-provider' }),
      ).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_waiver_route_mismatch',
      });
      await expect(
        service.decide({ ...ask, provider: WAIVER_PROVIDER }),
      ).resolves.toMatchObject({ kind: 'decided', decision: 'waived' });
    });

    it('refuses as unavailable when the signer has no key, and never signs with the switch off', async () => {
      orgCoreServes(zdrOrg);
      const noKey = await serviceWith(
        true,
        posture({}),
        new UnavailableRetentionDecisionSigner(),
      );
      await expect(noKey.service.decide(ask)).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_decision_unavailable',
      });
      const off = await serviceWith(false, posture({}));
      await expect(off.service.decide(ask)).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_connection_not_supported',
      });
    });

    it('treats an E2 client that throws as unavailable', async () => {
      orgCoreServes(zdrOrg);
      const { service } = await serviceWith(true, new Error('boom'));
      await expect(service.decide(ask)).resolves.toMatchObject({
        kind: 'refused',
        code: 'zdr_decision_unavailable',
      });
    });
  });

  describe('RetentionDecisionController', () => {
    const POLICY_SUBJECT = 'service:verevon-gateway-retention-decision';
    const validBody = {
      connection_id: CONNECTION_ID,
      provider: WAIVER_PROVIDER,
      explicit_zdr: false,
    };
    const zdrOrg: MatrixInputs = {
      org_zdr_intent: true,
      org_entitled: true,
      explicit_zdr: false,
      space_zdr: false,
      waiver_enabled: true,
      connection: 'own',
      capability: 'not_supported',
      waiver_route: true,
      ack: 'valid',
      outage: null,
    };

    function build(options: {
      policyOrg?: string;
      waiverEnabled?: boolean;
      auditFails?: boolean;
    }) {
      const tokens = {
        verifyPlaneServiceToken: jest.fn().mockReturnValue({
          subject: POLICY_SUBJECT,
          serviceId: POLICY_SUBJECT,
          orgId: options.policyOrg ?? 'org-a',
          scopes: ['retention:zdr-decide'],
          reason: 'retention decision',
        }),
      };
      const nats = {
        publishAuditDurable: options.auditFails
          ? jest.fn().mockRejectedValue(new Error('no ack'))
          : jest.fn().mockResolvedValue({ stream: 'S', seq: 1 }),
      };
      const read = jest.fn(async () => connectionAnswer(zdrOrg));
      const service = new RetentionDecisionService(
        signer,
        { readConnectionPosture: read } as never,
        options.waiverEnabled ?? true,
      );
      const controller = new RetentionDecisionController(
        tokens as never,
        service,
        nats as never,
      );
      return { controller, tokens, nats, read, service };
    }

    const response = () => ({
      setHeader: jest.fn(),
      status: jest.fn(),
    });
    const request = { headers: { cookie: 'session=test' } } as never;
    const bearer = 'Bearer aaa.bbb.ccc';

    async function statusOf(promise: Promise<unknown>): Promise<number> {
      try {
        await promise;
      } catch (error) {
        return (error as HttpException).getStatus();
      }
      return 200;
    }

    beforeEach(() => {
      mockGetSession.mockReset();
      mockResolveCanonicalTokenContext.mockReset();
      mockGetSession.mockResolvedValue({
        user: { id: 'user-a' },
        session: { id: 'session-a', activeOrganizationId: 'org-a' },
      });
      mockResolveCanonicalTokenContext.mockResolvedValue({
        orgId: 'org-a',
        role: 'member',
      });
      orgCoreServes(zdrOrg);
    });

    it('answers 403 for a policy token minted for another org, and reads nothing', async () => {
      const { controller, read } = build({ policyOrg: 'org-b' });
      fetchSpy.mockClear();
      const res = response();
      expect(
        await statusOf(controller.decide(bearer, request, res as never, validBody)),
      ).toBe(403);
      expect(read).not.toHaveBeenCalled();
      expect(fetchSpy).not.toHaveBeenCalled();
      expect(res.setHeader).toHaveBeenCalledWith('Cache-Control', 'no-store');
    });

    it('answers 401 without a session or without a bearer', async () => {
      const { controller } = build({});
      mockGetSession.mockResolvedValue(null);
      expect(
        await statusOf(
          controller.decide(bearer, request, response() as never, validBody),
        ),
      ).toBe(401);
      mockGetSession.mockResolvedValue({
        user: { id: 'user-a' },
        session: { id: 'session-a', activeOrganizationId: 'org-a' },
      });
      expect(
        await statusOf(
          controller.decide(undefined, request, response() as never, validBody),
        ),
      ).toBe(401);
    });

    it.each([
      ['an extra field', { ...validBody, zdr: false }],
      ['a missing field', { connection_id: CONNECTION_ID, provider: null }],
      ['a non-boolean explicit_zdr', { ...validBody, explicit_zdr: 'no' }],
      ['an id over 128 characters', { ...validBody, connection_id: 'c'.repeat(129) }],
      ['an empty id', { ...validBody, provider: '' }],
      ['an array', [validBody]],
      ['null', null],
    ])('answers 400 invalid_request for %s', async (_name, body) => {
      const { controller, read } = build({});
      expect(
        await statusOf(
          controller.decide(bearer, request, response() as never, body),
        ),
      ).toBe(400);
      expect(read).not.toHaveBeenCalled();
    });

    it('answers a waived decision with a verifying token and one content-free audit event', async () => {
      const { controller, nats } = build({});
      const res = response();
      const answer = (await controller.decide(
        bearer,
        request,
        res as never,
        validBody,
      )) as { data: Record<string, unknown> };
      expect(answer.data).toMatchObject({
        decision: 'waived',
        reason: 'connection_not_zdr_capable',
        org_zdr: true,
        effective_zdr: false,
        retention_class: 'zdr_waived',
      });
      const token = answer.data.decision_token as string;
      expect(signer.verify(token).connection).toBe(CONNECTION_ID);
      expect(typeof answer.data.expires_at).toBe('string');
      expect(res.setHeader).toHaveBeenCalledWith('Cache-Control', 'no-store');

      expect(nats.publishAuditDurable).toHaveBeenCalledTimes(1);
      const [subject, payload] = nats.publishAuditDurable.mock.calls[0] as [
        string,
        Record<string, unknown>,
      ];
      expect(subject).toBe(
        'verevon.audit.v2.control.auth-core.retention_zdr_decided',
      );
      expect(payload).toMatchObject({
        org_id: 'org-a',
        subject: 'user-a',
        resource_id: CONNECTION_ID,
        outcome: 'ok',
        event: 'retention_zdr_decided',
      });
      const serialised = JSON.stringify(payload);
      expect(serialised).not.toContain(token);
      expect(serialised).not.toContain('digest-1');
    });

    it('refuses a waived decision whose audit event cannot be published', async () => {
      const { controller } = build({ auditFails: true });
      const res = response();
      const answer = (await controller.decide(
        bearer,
        request,
        res as never,
        validBody,
      )) as { data?: unknown; error?: Record<string, unknown> };
      expect(answer.data).toBeUndefined();
      expect(answer.error).toMatchObject({
        code: 'zdr_decision_unavailable',
        retryable: true,
        action: 'retry',
      });
      expect(res.status).toHaveBeenCalledWith(503);
    });

    it('keeps a non-waived answer when the audit event cannot be published', async () => {
      const { controller } = build({ auditFails: true });
      const answer = (await controller.decide(
        bearer,
        request,
        response() as never,
        { connection_id: null, provider: null, explicit_zdr: false },
      )) as { data: Record<string, unknown> };
      expect(answer.data).toMatchObject({ decision: 'zdr', reason: 'org_zdr' });
      expect(answer.data.decision_token).toBeUndefined();
    });

    it('answers the typed 412 refusal with the switch off, and never signs', async () => {
      const { controller } = build({ waiverEnabled: false });
      const res = response();
      const answer = (await controller.decide(
        bearer,
        request,
        res as never,
        validBody,
      )) as { error: Record<string, unknown> };
      expect(res.status).toHaveBeenCalledWith(412);
      expect(Object.keys(answer.error).sort()).toEqual([
        'action',
        'code',
        'message',
        'retryable',
      ]);
      expect(answer.error.code).toBe('zdr_connection_not_supported');
      expect(answer.error.retryable).toBe(false);
    });
  });
});
