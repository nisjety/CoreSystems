import { ServiceUnavailableException } from '@nestjs/common';

const mockGetSession = jest.fn();
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import { PlaneTokenController } from './plane-token.controller';

describe('PlaneTokenController service issuance audit', () => {
  const originalRegistry = process.env.PLANE_SERVICE_PRINCIPALS_JSON;

  beforeEach(() => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      worker: {
        credential: 'test-only-worker-key',
        audiences: ['data-plane'],
        orgIds: ['org-a'],
        scopes: ['documents:write'],
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
      issuePlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
    };
    const audit = {
      publishAuditDurable: jest
        .fn()
        .mockResolvedValue({ stream: 'VELION_CONTROL_OBSERVABILITY', seq: 41 }),
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
    ).resolves.toEqual({ token: 'bounded' });
    expect(audit.publishAuditDurable).toHaveBeenCalledWith(
      'velion.audit.v1.control.plane_service_token_issued',
      expect.objectContaining({
        org_id: 'org-a',
        subject: 'service:worker',
        resource_id: 'data-plane',
        details: {
          audience: 'data-plane',
          scopes: ['documents:write'],
          reason: 'persist verified import',
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
        issuePlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
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

    acknowledge?.({ stream: 'VELION_CONTROL_OBSERVABILITY', seq: 42 });
    await expect(issuance).resolves.toEqual({ token: 'bounded' });
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
          issuePlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
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
});

describe('PlaneTokenController interactive capability scope issuance', () => {
  beforeEach(() => {
    mockGetSession.mockReset();
    mockResolveCanonicalTokenContext.mockReset();
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a', email: 'user@example.test' },
      session: { activeOrganizationId: 'org-a' },
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
      });
    },
  );
});
