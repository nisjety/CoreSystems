import {
  BadRequestException,
  ForbiddenException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { createHash } from 'node:crypto';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const mockGetSession = jest.fn();
jest.mock('./auth', () => ({
  auth: { api: { getSession: mockGetSession } },
}));
const mockResolveCanonicalTokenContext = jest.fn();
jest.mock('./plane-token-membership', () => ({
  resolveCanonicalTokenContext: mockResolveCanonicalTokenContext,
}));

import { ModelPlaneTokenController } from './model-plane-token.controller';
import { InteractiveRetentionPolicyConfigurationError } from './interactive-retention-policy';

describe('ModelPlaneTokenController secure ZDR issuance', () => {
  const originalRegistry = process.env.PLANE_SERVICE_PRINCIPALS_JSON;
  const originalRegistryFile = process.env.PLANE_SERVICE_PRINCIPALS_FILE;
  const issuedAt = '2026-07-15T00:00:00.000Z';
  const mintedToken = [
    'header',
    Buffer.from(
      JSON.stringify({ iat: Math.floor(Date.parse(issuedAt) / 1000) }),
    ).toString('base64url'),
    'signature',
  ].join('.');
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
    if (originalRegistryFile === undefined) {
      delete process.env.PLANE_SERVICE_PRINCIPALS_FILE;
    } else {
      process.env.PLANE_SERVICE_PRINCIPALS_FILE = originalRegistryFile;
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

  it('fails closed when the issuer-owned interactive retention policy is invalid', async () => {
    mockGetSession.mockResolvedValue({
      user: { id: 'user-a' },
      session: { activeOrganizationId: 'org-a' },
    });
    mockResolveCanonicalTokenContext.mockResolvedValue({
      orgId: 'org-a',
      role: 'member',
    });
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockImplementation(() => {
        throw new InteractiveRetentionPolicyConfigurationError(
          'invalid policy',
        );
      }),
    };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.getToken({ headers: {} } as never),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
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
        email: undefined,
        scopes: ['admin'],
      });
    },
  );

  it('mints only the file-configured model audience, tenant, and scope', async () => {
    const directory = mkdtempSync(join(tmpdir(), 'model-plane-principals-'));
    const registryFile = join(directory, 'registry.json');
    writeFileSync(
      registryFile,
      JSON.stringify({
        'model-worker': {
          credential: 'test-only-model-worker-key',
          audiences: ['model-gateway'],
          orgIds: ['org-a'],
          scopes: ['runs:submit'],
        },
      }),
      { mode: 0o600 },
    );
    delete process.env.PLANE_SERVICE_PRINCIPALS_JSON;
    process.env.PLANE_SERVICE_PRINCIPALS_FILE = registryFile;
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
    };
    const auditPublisher = availableAudit();
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      auditPublisher as never,
    );

    try {
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
      ).resolves.toEqual({ token: mintedToken });
      expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith({
        userId: 'service:model-worker',
        orgId: 'org-a',
        scopes: ['runs:submit'],
        principalType: 'service',
        serviceId: 'model-worker',
        reason: 'submit scheduled evaluation',
        retentionPosture: {
          zdr: true,
          authority: 'service-principal-policy',
        },
      });
      expect(auditPublisher.publishAuditDurable).toHaveBeenCalledWith(
        'velion.audit.v2.control.auth-core.model_service_token_issued',
        expect.objectContaining({
          occurred_at: issuedAt,
          event_id: `model-token:${createHash('sha256')
            .update(mintedToken)
            .digest('hex')}`,
          org_id: 'org-a',
          actor_role: 'service',
          plane: 'control',
          producer: 'auth-core',
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
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });

  it('uses exact deployment policy for non-ZDR Model service issuance', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
        retentionByAudience: {
          'model-gateway': 'persistent',
        },
      },
    });
    const tokenService = {
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
    };
    const auditPublisher = availableAudit();
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      auditPublisher as never,
    );
    const callerBody = {
      orgId: 'org-a',
      scopes: ['runs:submit'],
      reason: 'submit scheduled evaluation',
    };

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        callerBody,
      ),
    ).resolves.toEqual({ token: mintedToken });
    expect(tokenService.issueModelPlaneToken).toHaveBeenCalledWith(
      expect.objectContaining({
        retentionPosture: {
          zdr: false,
          authority: 'service-principal-policy',
        },
      }),
    );
    expect(auditPublisher.publishAuditDurable).toHaveBeenCalledWith(
      'velion.audit.v2.control.auth-core.model_service_token_issued',
      expect.objectContaining({
        details: {
          audience: 'model-gateway',
          scopes: ['runs:submit'],
          reason: 'submit scheduled evaluation',
          zdr: false,
        },
      }),
    );
  });

  it('rejects Model retention posture supplied in either the request body or headers', async () => {
    process.env.PLANE_SERVICE_PRINCIPALS_JSON = JSON.stringify({
      'model-worker': {
        credential: 'test-only-model-worker-key',
        audiences: ['model-gateway'],
        orgIds: ['org-a'],
        scopes: ['runs:submit'],
      },
    });
    const tokenService = { issueModelPlaneToken: jest.fn() };
    const controller = new ModelPlaneTokenController(
      tokenService as never,
      availableAudit() as never,
    );

    await expect(
      controller.issueInternalToken(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'attempt body persistence downgrade',
          zdr: false,
        },
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    const issueWithHeader = controller.issueInternalToken.bind(controller) as (
      serviceId: string,
      credential: string,
      body: object,
      zdrHeader?: string,
    ) => Promise<unknown>;
    await expect(
      issueWithHeader(
        'model-worker',
        'test-only-model-worker-key',
        {
          orgId: 'org-a',
          scopes: ['runs:submit'],
          reason: 'attempt header persistence downgrade',
        },
        'false',
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(tokenService.issueModelPlaneToken).not.toHaveBeenCalled();
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
        issueModelPlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
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
        issueModelPlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
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
    await expect(issuance).resolves.toEqual({ token: mintedToken });
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
      issueModelPlaneToken: jest.fn().mockReturnValue({ token: mintedToken }),
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
