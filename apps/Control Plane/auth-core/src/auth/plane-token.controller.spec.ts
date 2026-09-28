import {
  BadRequestException,
  ForbiddenException,
  NotFoundException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash } from 'node:crypto';

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
  resolveInteractiveRetentionPosture: mockResolveInteractiveRetentionPosture,
}));

import { PlaneTokenController } from './plane-token.controller';

describe('PlaneTokenController service issuance audit', () => {
  const originalRegistry = process.env.PLANE_SERVICE_PRINCIPALS_JSON;
  const issuedAt = '2026-07-15T00:00:00.000Z';
  const mintedToken = [
    'header',
    Buffer.from(
      JSON.stringify({ iat: Math.floor(Date.parse(issuedAt) / 1000) }),
    ).toString('base64url'),
    'signature',
  ].join('.');

  beforeEach(() => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      worker: {
        credential: 'test-only-worker-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['documents:write'],
        retentionByAudience: {
          'data-plane': 'persistent',
        },
      },
    });
  });

  afterEach(() => {
    if (originalRegistry === undefined) {
      delete process.env.PLANE_SERVICE_PRINCIPALS_JSON;
    } else {
      process.env.PLANE_SERVICE_PRINCIPALS_JSON = originalRegistry;
    }
  });

  it('publishes a durable bounded issuance event before returning the token', async () => {
    const tokens = {
      isKnownPlaneAudience: jest.fn().mockReturnValue(true),
      issuePlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
    };
    const audit = {
      publishAuditDurable: jest.fn().mockResolvedValue({
        stream: 'VEREVON_CONTROL_OBSERVABILITY',
        seq: 41,
      }),
    };
    const controller = new PlaneTokenController(
      tokens as never,
      audit as never,
    );

    await expect(
      controller.issueInternalToken(
        'data-plane',
        'worker',
        'test-only-worker-key',
        {
          orgId: 'org-a',
          scopes: ['documents:write'],
          reason: 'persist verified import',
        },
      ),
    ).resolves.toEqual({ token: mintedToken });
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      'verevon.audit.v2.control.auth-core.plane_service_token_issued',
      expect.objectContaining({
        occurred_at: issuedAt,
        event_id: `plane-token:${createHash('sha256')
          .update(mintedToken)
          .digest('hex')}`,
        org_id: 'org-a',
        producer: 'auth-core',
        subject: 'service:worker',
        resource_id: 'data-plane',
        details: {
          audience: 'data-plane',
          scopes: ['documents:write'],
          reason: 'persist verified import',
          zdr: false,
        },
      }),
    );
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith(
      'data-plane',
      expect.objectContaining({
        retentionPosture: {
          zdr: false,
          authority: 'service-principal-policy',
        },
      }),
    );
  });

  it('does not return the token until the durable audit PubAck arrives', async () => {
    let acknowledge:
      | ((ack: { stream: string; seq: number }) => void)
      | undefined;
    const auditPending = new Promise<{ stream: string; seq: number }>(
      (resolve) => {
        acknowledge = resolve;
      },
    );
    const controller = new PlaneTokenController(
      {
        isKnownPlaneAudience: jest.fn().mockReturnValue(true),
        issuePlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
      } as never,
      {
        publishAuditDurable: jest.fn().mockReturnValue(auditPending),
      } as never,
    );

    const issuance = controller.issueInternalToken(
      'data-plane',
      'worker',
      'test-only-worker-key',
      {
        orgId: 'org-a',
        scopes: ['documents:write'],
        reason: 'persist verified import',
      },
    );
    let returned = false;
    void issuance.then(() => {
      returned = true;
    });
    await Promise.resolve();
    expect(returned).toBe(false);

    acknowledge?.({ stream: 'VEREVON_CONTROL_OBSERVABILITY', seq: 42 });
    await expect(issuance).resolves.toEqual({ token: mintedToken });
  });

  it.each([
    {
      publishAuditDurable: jest
        .fn()
        .mockRejectedValue(new Error('PubAck timeout')),
    },
    {
      publishAuditDurable: jest.fn().mockImplementation(() => {
        throw new Error('transport unavailable');
      }),
    },
  ])(
    'fails closed when the durable audit PubAck is unavailable',
    async (audit) => {
      const controller = new PlaneTokenController(
        {
          isKnownPlaneAudience: jest.fn().mockReturnValue(true),
          issuePlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
        } as never,
        audit as never,
      );

      await expect(
        controller.issueInternalToken(
          'data-plane',
          'worker',
          'test-only-worker-key',
          { orgId: 'org-a', scopes: ['documents:write'], reason: 'test' },
        ),
      ).rejects.toBeInstanceOf(ServiceUnavailableException);
    },
  );

  it('rejects unknown audiences before consulting the principal registry', async () => {
    const controller = new PlaneTokenController(
      { isKnownPlaneAudience: jest.fn().mockReturnValue(false) } as never,
      {} as never,
    );

    await expect(
      controller.issueInternalToken(
        'unknown-plane',
        'worker',
        'test-only-worker-key',
        { orgId: 'org-a', scopes: ['documents:write'], reason: 'test' },
      ),
    ).rejects.toBeInstanceOf(NotFoundException);
  });

  it('distinguishes unavailable policy and rejects caller-selected retention', async () => {
    const tokens = {
      isKnownPlaneAudience: jest.fn().mockReturnValue(true),
      issuePlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
    };
    const controller = new PlaneTokenController(
      tokens as never,
      {
        publishAuditDurable: jest.fn().mockResolvedValue({
          stream: 'VEREVON_CONTROL_OBSERVABILITY',
          seq: 44,
        }),
      } as never,
    );
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = '';
    await expect(
      controller.issueInternalToken(
        'data-plane',
        'worker',
        'test-only-worker-key',
        { orgId: 'org-a', scopes: ['documents:write'], reason: 'test' },
      ),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);

    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      worker: {
        credential: 'test-only-worker-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['documents:write'],
      },
    });
    const callerDowngrade = {
      orgId: 'org-a',
      scopes: ['documents:write'],
      reason: 'attempt persistence downgrade',
      zdr: false,
    };
    await expect(
      controller.issueInternalToken(
        'data-plane',
        'worker',
        'test-only-worker-key',
        callerDowngrade,
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    const issueWithHeader = controller.issueInternalToken.bind(controller) as (
      audience: string,
      serviceId: string,
      credential: string,
      body: object,
      zdrHeader?: string,
    ) => Promise<unknown>;
    await expect(
      issueWithHeader(
        'data-plane',
        'worker',
        'test-only-worker-key',
        {
          orgId: 'org-a',
          scopes: ['documents:write'],
          reason: 'attempt header persistence downgrade',
        },
        'false',
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
  });
});

describe('PlaneTokenController interactive capability scope issuance', () => {
  beforeEach(() => {
    mockGetSession.mockReset();
    mockResolveCanonicalTokenContext.mockReset();
    mockResolveInteractiveRetentionPosture.mockReset();
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a', email: 'user@example.test' },
      session: { activeOrganizationId: 'org-a' },
    });
    // Default: normal retention. Individual tests override to exercise the
    // zdr:true or org-core-outage paths.
    mockResolveInteractiveRetentionPosture.mockResolvedValue({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
  });

  it.each([
    ['member', ['capability:read']],
    ['viewer', ['capability:read']],
    ['admin', ['capability:read', 'capability:write']],
    ['owner', ['capability:read', 'capability:write']],
  ] as const)(
    'mints least-privilege capability scopes for %s',
    async (role, expectedScopes) => {
      mockResolveCanonicalTokenContext.mockResolvedValue({
        orgId: 'org-a',
        role,
      });
      const tokens = {
        isInteractivePlaneAudience: jest.fn().mockReturnValue(true),
        issuePlaneToken: jest.fn().mockReturnValue({ token: 'interactive' }),
      };
      const controller = new PlaneTokenController(tokens as never, {} as never);

      await expect(
        controller.getToken('capability-core', {
          headers: { cookie: 'session=test' },
        } as never),
      ).resolves.toEqual(
        expect.objectContaining({
          token: 'interactive',
          orgId: 'org-a',
          role,
        }),
      );
      expect(tokens.issuePlaneToken).toHaveBeenCalledWith('capability-core', {
        userId: 'user-a',
        orgId: 'org-a',
        email: 'user@example.test',
        scopes: expectedScopes,
        retentionPosture: {
          zdr: false,
          authority: 'interactive-org-retention-policy',
        },
      });
      expect(mockResolveInteractiveRetentionPosture).toHaveBeenCalledWith(
        'org-a',
      );
    },
  );

  it('stamps zdr:true into the minted token when org-core resolves a qualifying org', async () => {
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId: 'org-a',
      role: 'member',
    });
    mockResolveInteractiveRetentionPosture.mockResolvedValue({
      zdr: true,
      authority: 'interactive-org-retention-policy',
    });
    const tokens = {
      isInteractivePlaneAudience: jest.fn().mockReturnValue(true),
      issuePlaneToken: jest.fn().mockReturnValue({ token: 'zdr-interactive' }),
    };
    const controller = new PlaneTokenController(tokens as never, {} as never);

    await expect(
      controller.getToken('inference-core', { headers: {} } as never),
    ).resolves.toMatchObject({ token: 'zdr-interactive' });
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith(
      'inference-core',
      expect.objectContaining({
        retentionPosture: {
          zdr: true,
          authority: 'interactive-org-retention-policy',
        },
      }),
    );
  });

  it('still succeeds and mints a normal-retention token when the org-core retention lookup is unavailable (fails closed, never blocks login)', async () => {
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId: 'org-a',
      role: 'member',
    });
    // resolveInteractiveRetentionPosture never throws — an org-core outage
    // or timeout resolves to the fail-closed posture, not a rejection.
    mockResolveInteractiveRetentionPosture.mockResolvedValue({
      zdr: false,
      authority: 'interactive-org-retention-policy',
    });
    const tokens = {
      isInteractivePlaneAudience: jest.fn().mockReturnValue(true),
      issuePlaneToken: jest.fn().mockReturnValue({ token: 'degraded' }),
    };
    const controller = new PlaneTokenController(tokens as never, {} as never);

    await expect(
      controller.getToken('inference-core', { headers: {} } as never),
    ).resolves.toMatchObject({ token: 'degraded' });
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith(
      'inference-core',
      expect.objectContaining({
        retentionPosture: {
          zdr: false,
          authority: 'interactive-org-retention-policy',
        },
      }),
    );
  });
});

describe('PlaneTokenController delegated-identity token exchange (issueTokenExchange)', () => {
  const originalRegistry = process.env.PLANE_SERVICE_PRINCIPALS_JSON;
  const issuedAt = '2026-09-28T00:00:00.000Z';
  const mintedToken = [
    'header',
    Buffer.from(
      JSON.stringify({ iat: Math.floor(Date.parse(issuedAt) / 1000) }),
    ).toString('base64url'),
    'signature',
  ].join('.');
  const validSubject = {
    orgId: 'org-a',
    userId: 'user-a',
    zdr: false,
    audience: 'session-core',
  };

  beforeEach(() => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-execution': {
        credential: 'test-only-execution-key',
        audiences: ['ingestion'],
        orgIds: ['org-a'],
        scopes: ['integration:read'],
        retentionByAudience: { ingestion: 'persistent' },
        tokenExchange: { ingestion: ['session-core'] },
      },
    });
  });

  afterEach(() => {
    if (originalRegistry === undefined) {
      delete process.env.PLANE_SERVICE_PRINCIPALS_JSON;
    } else {
      process.env.PLANE_SERVICE_PRINCIPALS_JSON = originalRegistry;
    }
  });

  function buildController(overrides: {
    verifyPlaneSubjectToken?: jest.Mock;
    issuePlaneToken?: jest.Mock;
    publishAuditDurable?: jest.Mock;
  } = {}) {
    const tokens = {
      isKnownPlaneAudience: jest.fn().mockReturnValue(true),
      verifyPlaneSubjectToken:
        overrides.verifyPlaneSubjectToken ??
        jest.fn().mockReturnValue(validSubject),
      issuePlaneToken:
        overrides.issuePlaneToken ??
        jest.fn().mockReturnValue({ token: mintedToken }),
    };
    const audit = {
      publishAuditDurable:
        overrides.publishAuditDurable ??
        jest.fn().mockResolvedValue({
          stream: 'VEREVON_CONTROL_OBSERVABILITY',
          seq: 1,
        }),
    };
    return { controller: new PlaneTokenController(tokens as never, audit as never), tokens, audit };
  }

  const validBody = {
    orgId: 'org-a',
    scopes: ['integration:read'],
    reason: 'execution-core integration:read',
    subjectToken: 'subject.token.value',
    subjectTokenType: 'urn:ietf:params:oauth:token-type:jwt',
  };

  it('mints a delegated token carrying the subject token acting user and publishes an audit event', async () => {
    const { controller, tokens, audit } = buildController();

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).resolves.toEqual({ token: mintedToken });

    expect(tokens.verifyPlaneSubjectToken).toHaveBeenCalledWith(
      'subject.token.value',
      ['session-core'],
    );
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith(
      'ingestion',
      expect.objectContaining({
        userId: 'service:model-execution',
        orgId: 'org-a',
        actingUserId: 'user-a',
        principalType: 'service',
        ttlSecondsOverride: 90,
      }),
    );
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      'verevon.audit.v2.control.auth-core.plane_service_token_exchanged',
      expect.objectContaining({
        subject: 'service:model-execution',
        resource_id: 'ingestion',
        details: expect.objectContaining({ acting_user_id: 'user-a' }),
      }),
    );
  });

  it('token_exchange_rejects_a_subject_token_for_a_different_org_than_the_request_body', async () => {
    const { controller, tokens } = buildController();

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        { ...validBody, orgId: 'org-different' },
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
  });

  it('token_exchange_preserves_the_subject_tokens_own_zdr_even_when_the_registry_default_differs', async () => {
    // Registry's own retentionByAudience for `ingestion` is 'persistent'
    // (zdr:false) — the subject token below claims zdr:true, which MUST win.
    const { controller, tokens } = buildController({
      verifyPlaneSubjectToken: jest
        .fn()
        .mockReturnValue({ ...validSubject, zdr: true }),
    });

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).resolves.toEqual({ token: mintedToken });
    expect(tokens.issuePlaneToken).toHaveBeenCalledWith(
      'ingestion',
      expect.objectContaining({
        retentionPosture: { zdr: true, authority: 'service-principal-policy' },
      }),
    );
  });

  it('rejects an unverifiable subject token before ever consulting the registry', async () => {
    const { controller, tokens } = buildController({
      verifyPlaneSubjectToken: jest.fn().mockImplementation(() => {
        throw new Error('Plane subject token claims are invalid');
      }),
    });

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
  });

  it('rejects a request missing subjectToken/subjectTokenType before verifying anything', async () => {
    const { controller, tokens } = buildController();

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        { ...validBody, subjectToken: '' },
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(tokens.verifyPlaneSubjectToken).not.toHaveBeenCalled();
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
  });

  it('rejects a principal with no tokenExchange registry entry for this target audience', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-execution': {
        credential: 'test-only-execution-key',
        audiences: ['ingestion'],
        orgIds: ['org-a'],
        scopes: ['integration:read'],
        // no tokenExchange field at all — absent by default (T-4/§1.2).
      },
    });
    const { controller, tokens } = buildController();

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).rejects.toBeInstanceOf(ForbiddenException);
    expect(tokens.issuePlaneToken).not.toHaveBeenCalled();
  });

  it('rejects unknown audiences before consulting the subject token or registry', async () => {
    const tokens = {
      isKnownPlaneAudience: jest.fn().mockReturnValue(false),
      verifyPlaneSubjectToken: jest.fn(),
    };
    const controller = new PlaneTokenController(tokens as never, {} as never);

    await expect(
      controller.issueTokenExchange(
        'unknown-plane',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).rejects.toBeInstanceOf(NotFoundException);
    expect(tokens.verifyPlaneSubjectToken).not.toHaveBeenCalled();
  });

  it('fails closed when the durable audit PubAck is unavailable', async () => {
    const { controller } = buildController({
      publishAuditDurable: jest
        .fn()
        .mockRejectedValue(new Error('PubAck timeout')),
    } as never);

    await expect(
      controller.issueTokenExchange(
        'ingestion',
        'model-execution',
        'test-only-execution-key',
        validBody,
      ),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });
});
