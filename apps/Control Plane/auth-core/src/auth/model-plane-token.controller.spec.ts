import {
  BadRequestException,
  ForbiddenException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';

const mockGetSession = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import { ModelPlaneTokenController } from './model-plane-token.controller';

describe('ModelPlaneTokenController secure ZDR issuance', () => {
  const originalRegistry = process.env.PLANE_SERVICE_PRINCIPALS_JSON;
  const availableAudit = () => ({
    publishAuditDurable: jest
      .fn()
      .mockResolvedValue({ stream: 'VELION_CONTROL_OBSERVABILITY', seq: 42 }),
  });

  beforeEach(() => {
    jest.clearAllMocks();
  });

  afterEach(() => {
    if (originalRegistry === undefined) {
      delete process.env.PLANE_SERVICE_PRINCIPALS_JSON;
    } else {
      process.env.PLANE_SERVICE_PRINCIPALS_JSON = originalRegistry;
    }
  });

  it('requires mandatory ZDR for an authenticated user token', async () => {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a', email: 'user-a@example.test' },
      session: { activeOrganizationId: 'org-a' },
    });
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId: 'org-a',
      role: 'member',
    });
    const tokenService = {
      issueModelPlaneToken: jest
        .fn()
        .mockReturnValue({ token: 'bounded-user' }),
    };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: { cookie: 'session=test' } } as never),
    ).resolves.toMatchObject({
      token: 'bounded-user',
      userId: 'user-a',
      orgId: 'org-a',
    });
    expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith({
      userId: 'user-a',
      orgId: 'org-a',
      zdr: true,
      email: 'user-a@example.test',
      scopes: undefined,
    });
  });

  it('keeps an authenticated user unprivileged when canonical role is absent', async () => {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a' },
      session: { activeOrganizationId: 'org-a' },
    });
    mockResolveCanonicalTokenContext.mockResolvedValue({ orgId: 'org-a' });
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: 'member-user' }),
    };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: {} } as never),
    ).resolves.toMatchObject({ token: 'member-user', role: null });
    expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith(
      expect.objectContaining({ scopes: undefined }),
    );
  });

  it('rejects a request without a canonical authenticated session', async () => {
    mockGetSession.mockResolvedValue(null);
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: {} } as never),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    expect(mockResolveCanonicalTokenContext).not.toHaveBeenCalled();
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
  });

  it('fails closed when the canonical membership authority is unavailable', async () => {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a' },
      session: { activeOrganizationId: 'org-a' },
    });
    mockResolveCanonicalTokenContext.mockRejectedValue(
      new Error('test authority outage'),
    );
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: {} } as never),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
  });

  it('rejects a session without a verified active organization', async () => {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a' },
      session: {},
    });
    mockResolveCanonicalTokenContext.mockResolvedValue(null);
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: {} } as never),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(mockResolveCanonicalTokenContext).toHaveBeenCalledWith('user-a', '');
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
  });

  it.each(['owner', 'admin'])(
    'adds admin scope only for a canonical %s',
    async (role) => {
      mockGetSession.mockResolvedValue({
        user: { id: 'user-a', email: null },
        session: { activeOrganizationId: 'org-a' },
      });
      mockResolveCanonicalTokenContext.mockResolvedValue({
        orgId: 'org-a',
        role,
      });
      const tokenService = {
        issueModelPlaneToken: jest
          .fn()
          .mockReturnValue({ token: 'admin-user' }),
      };
      const controller = new ModelPlaneTokenController(
        tokenService as never,
        availableAudit() as never,
      );

      await controller.getToken({
        headers: { cookie: ['session=a', 'context=b'], 'x-ignored': undefined },
      } as never);

      expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith({
        userId: 'user-a',
        orgId: 'org-a',
        zdr: true,
        email: undefined,
        scopes: ['admin'],
      });
    },
  );

  it('mints only the configured model audience, tenant, and scope', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
    };
    const auditPublisher = availableAudit();
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      auditPublisher as never,
    );

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
        },
      ),
    ).resolves.toEqual({ token: 'bounded' });
    expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith({
      userId: 'service:model-worker',
      orgId: 'org-a',
      scopes: ['runs:submit'],
      principalType: 'service',
      serviceId: 'model-worker',
      reason: 'submit scheduled evaluation',
      zdr: true,
    });
    expect(auditPublisher.publishAuditDurable).toHaveBeenCalledWith(
      'velion.audit.v1.control.model_service_token_issued',
      expect.objectContaining({
        org_id: 'org-a',
        actor_role: 'service',
        plane: 'control',
        event: 'model_service_token_issued',
        subject: 'service:model-worker',
        resource_id: 'model-gateway',
        outcome: 'ok',
        details: {
          audience: 'model-gateway',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
          zdr: true,
        },
      }),
    );
  });

  it('fails closed when the durable service-token audit event cannot publish', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    const controller = new ModelPlaneTokenController(
      {
        issueModelPlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
      } as never,
      {
        publishAuditDurable: jest
          .fn()
          .mockRejectedValue(new Error('PubAck timeout')),
      } as never,
    );

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
        },
      ),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });

  it('does not return a Model token until the durable audit PubAck arrives', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    let acknowledge:
      | ((ack: { stream: string; seq: number }) => void)
      | undefined;
    const auditPending = new Promise<{ stream: string; seq: number }>(
      (resolve) => {
        acknowledge = resolve;
      },
    );
    const controller = new ModelPlaneTokenController(
      {
        issueModelPlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
      } as never,
      {
        publishAuditDurable: jest.fn().mockReturnValue(auditPending),
      } as never,
    );

    const issuance = controller.issueInternalToken(
      'model-worker',
      'test-only-model-worker-key',
      {
        orgId: 'org-a',
        scopes: ['runs:submit'],
        reason: 'submit scheduled evaluation',
      },
    );
    let returned = false;
    void issuance.then(() => {
      returned = true;
    });
    await Promise.resolve();
    expect(returned).toBe(false);

    acknowledge?.({ stream: 'VELION_CONTROL_OBSERVABILITY', seq: 43 });
    await expect(issuance).resolves.toEqual({ token: 'bounded' });
  });

  it('normalizes a thrown audit transport failure to service unavailable', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: 'bounded' }),
    };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      {
        publishAuditDurable: jest.fn(() => {
          throw new Error('test transport outage');
        }),
      } as never,
    );

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
        },
      ),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });

  it.each([
    ['', 'missing registry'],
    ['{', 'invalid JSON'],
    ['[]', 'non-object registry'],
    [JSON.stringify({ '!': {} }), 'invalid service identifier'],
    [
      JSON.stringify({
        'model-worker': {
          credential: 'short',
          audiences: ['model-gateway'],
          orgIds: ['org-a'],
          scopes: ['runs:submit'],
        },
      }),
      'invalid principal policy',
    ],
  ])('returns 503 for a deployment-owned %s (%s)', async (registry) => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = registry;
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const auditPublisher = availableAudit();
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      auditPublisher as never,
    );

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
        },
      ),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
    expect(auditPublisher.publishAuditDurable).not.toHaveBeenCalled();
  });

  it('rejects omitted service identity and request bounds without inferring defaults', async () => {
    delete process.env.PLANE_SERVICE_PRINCIPALS_JSON;
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const auditPublisher = availableAudit();
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      auditPublisher as never,
    );

    await expect(
      controller.issueInternalToken(undefined, undefined, {}),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
    expect(auditPublisher.publishAuditDurable).not.toHaveBeenCalled();
  });

  it.each([{ orgId: 'org-b' }, { scopes: ['admin'] }, { reason: '  ' }])(
    'returns 403 when caller input exceeds registry bounds: %o',
    async (body) => {
      process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
        'model-worker': {
          credential: 'test-only-model-worker-key',
          audiences: ['model-gateway'],
          orgIds: ['org-a'],
          scopes: ['runs:submit'],
        },
      });
      const tokenService = { issueModelPlaneToken: jest.fn() };
      const auditPublisher = availableAudit();
      const controller = new ModelPlaneTokenController(
        tokenService as never,
        auditPublisher as never,
      );

      await expect(
        controller.issueInternalToken(
          'model-worker',
          'test-only-model-worker-key',
          {
            orgId: 'org-a',
            scopes: ['runs:submit'],
            reason: 'submit scheduled evaluation',
            ...body,
          },
        ),
      ).rejects.toBeInstanceOf(ForbiddenException);
      expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
      expect(auditPublisher.publishAuditDurable).not.toHaveBeenCalled();
    },
  );

  it('does not accept the former shared internal key as identity authority', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    const controller = new ModelPlaneTokenController(
      { issueModelPlaneToken: jest.fn() } as never,
      availableAudit() as never,
    );

    await expect(
      controller.issueInternalToken('model-worker', 'shared-fleet-key', {
        orgId: 'org-a',
        scopes: ['admin'],
        reason: 'escalate',
      }),
    ).rejects.toBeInstanceOf(ForbiddenException);
  });
});
