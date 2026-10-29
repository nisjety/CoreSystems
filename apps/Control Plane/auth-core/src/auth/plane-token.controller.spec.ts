import {
  BadRequestException,
  NotFoundException,
  ServiceUnavailableException,
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
