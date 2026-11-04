/**
 * WS05-S7: org-core's internal SSO administration surface on auth-core. The
 * database is a queue-driven stub (each awaited query takes the next queued
 * result) and the session adapter is a spy, so nothing leaves the process.
 */
const mockQueue: unknown[] = [];
const mockWhereArgs: unknown[] = [];
const mockLimitArgs: unknown[] = [];
const mockInsertValues = jest.fn();
const mockDeleteSessions = jest.fn();

function nextResult(): unknown {
  if (mockQueue.length === 0) return [];
  const next = mockQueue.shift();
  if (next instanceof Error) throw next;
  return next;
}

jest.mock('better-auth/api', () => ({
  APIError: class APIError extends Error {},
}));
jest.mock('./audit-plugin', () => ({ publishVerevonAuditDurable: jest.fn() }));
jest.mock('./control-service-credentials', () => ({
  requiredScopedServiceToken: jest.fn(),
}));
jest.mock('./auth', () => ({
  auth: {
    $context: Promise.resolve({
      internalAdapter: {
        deleteSessions: (...args: unknown[]) => mockDeleteSessions(...args),
      },
    }),
  },
}));
jest.mock('../db', () => {
  const link: Record<string, unknown> = {};
  const chain = () => link;
  for (const method of ['from', 'innerJoin', 'orderBy']) link[method] = chain;
  link.where = (condition: unknown) => {
    mockWhereArgs.push(condition);
    return link;
  };
  link.limit = (n: unknown) => {
    mockLimitArgs.push(n);
    return link;
  };
  link.returning = chain;
  // Awaiting the chain takes the next queued result.
  link.then = (
    resolve: (value: unknown) => void,
    reject: (reason: unknown) => void,
  ) => {
    try {
      resolve(nextResult());
    } catch (error) {
      reject(error);
    }
  };
  return {
    db: {
      select: chain,
      delete: chain,
      insert: () => ({
        values: (values: unknown) => {
          mockInsertValues(values);
          return Promise.resolve(nextResult());
        },
      }),
    },
  };
});

import {
  BadRequestException,
  ConflictException,
  Logger,
  NotFoundException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';

import { SsoProviderAdminController } from './sso-provider-admin.controller';

const TOKEN = 'identity-admin-token-0123456789abcdef01';
const SECRET = 'oidc-client-secret-ssssssssssssssss';

function controller(): SsoProviderAdminController {
  process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = TOKEN;
  return new SsoProviderAdminController();
}

function queue(...results: unknown[]): void {
  mockQueue.push(...results);
}

/** Every bound parameter value inside a drizzle SQL condition. */
function paramsOf(node: unknown, seen = new Set<unknown>()): unknown[] {
  if (!node || typeof node !== 'object' || seen.has(node)) return [];
  seen.add(node);
  const record = node as Record<string, unknown>;
  const out: unknown[] = [];
  if (Array.isArray(record.queryChunks)) {
    for (const chunk of record.queryChunks) {
      // Template interpolations of plain values sit in the chunk list as they are.
      if (typeof chunk === 'number' || typeof chunk === 'string')
        out.push(chunk);
      else out.push(...paramsOf(chunk, seen));
    }
  }
  if ('value' in record && 'encoder' in record) out.push(record.value);
  return out;
}

const registerBody = (overrides: Record<string, unknown> = {}) => ({
  organizationId: 'org-1',
  actorUserId: 'admin-1',
  providerId: 'Acme-OIDC',
  domain: 'Example.COM',
  issuer: 'https://idp.example.com/realms/acme',
  clientId: 'client-1',
  clientSecret: SECRET,
  ...overrides,
});

beforeEach(() => {
  mockQueue.length = 0;
  mockWhereArgs.length = 0;
  mockLimitArgs.length = 0;
  mockInsertValues.mockReset();
  mockDeleteSessions.mockReset().mockResolvedValue(undefined);
  jest.restoreAllMocks();
  jest.spyOn(Logger.prototype, 'log').mockImplementation(() => undefined);
  jest.spyOn(Logger.prototype, 'error').mockImplementation(() => undefined);
});

describe('SsoProviderAdminController authentication', () => {
  const calls: Array<
    [
      string,
      (c: SsoProviderAdminController, token?: string) => Promise<unknown>,
    ]
  > = [
    ['list', (c, t) => c.list(t, 'org-1')],
    ['register', (c, t) => c.register(t, registerBody())],
    ['remove', (c, t) => c.remove(t, 'acme-oidc', 'org-1', 'admin-1')],
    ['evidence', (c, t) => c.evidence(t, 'org-1', 'user-1', '3600')],
    [
      'member-facts',
      (c, t) => c.memberFacts(t, { organizationId: 'org-1', userIds: ['u1'] }),
    ],
    [
      'revoke-sessions',
      (c, t) =>
        c.revokeSessions(t, {
          organizationId: 'org-1',
          actorUserId: 'admin-1',
          domains: ['example.com'],
        }),
    ],
  ];

  it.each(calls)(
    '%s refuses a missing, wrong or padded-wrong token with 401 before touching the database',
    async (_name, call) => {
      const c = controller();
      for (const token of [
        undefined,
        '',
        'wrong',
        `${TOKEN}x`,
        TOKEN.slice(1),
      ]) {
        await expect(call(c, token)).rejects.toBeInstanceOf(
          UnauthorizedException,
        );
      }
      expect(mockWhereArgs).toHaveLength(0);
      expect(mockInsertValues).not.toHaveBeenCalled();
    },
  );

  it.each(calls)('%s accepts the right token, trimmed', async (name, call) => {
    const c = controller();
    queue([{ role: 'owner' }], [], [], [], []);
    // Reaching the handler body means authentication passed; a body-level 400/401/404 is fine.
    const outcome = await call(c, `  ${TOKEN}  `).then(
      () => 'ok',
      (error: unknown) =>
        error instanceof UnauthorizedException &&
        /credential/.test(String(error.message))
          ? 'auth-failed'
          : 'ok',
    );
    expect({ name, outcome }).toEqual({ name, outcome: 'ok' });
  });

  it.each(calls)(
    '%s fails closed with 503 when the credential is not configured',
    async (_name, call) => {
      process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = 'too-short';
      const c = new SsoProviderAdminController();
      await expect(call(c, 'too-short')).rejects.toBeInstanceOf(
        ServiceUnavailableException,
      );
      process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = '';
      await expect(
        call(new SsoProviderAdminController(), ''),
      ).rejects.toBeInstanceOf(ServiceUnavailableException);
    },
  );
});

describe('SsoProviderAdminController list', () => {
  it('requires an organization id and never returns client credentials', async () => {
    const c = controller();
    await expect(c.list(TOKEN, undefined)).rejects.toBeInstanceOf(
      BadRequestException,
    );
    await expect(c.list(TOKEN, '   ')).rejects.toBeInstanceOf(
      BadRequestException,
    );

    queue([
      {
        providerId: 'acme-oidc',
        domain: 'example.com',
        issuer: 'https://idp.example.com',
        oidcConfig: JSON.stringify({ clientId: 'c', clientSecret: SECRET }),
        samlConfig: null,
      },
      {
        providerId: 'acme-saml',
        domain: 'saml.example',
        issuer: 'https://saml.example',
        oidcConfig: null,
        samlConfig: '{}',
      },
      {
        providerId: 'bare',
        domain: 'bare.example',
        issuer: 'https://bare.example',
        oidcConfig: null,
        samlConfig: null,
      },
    ]);
    const result = await c.list(TOKEN, 'org-1');
    expect(result).toEqual({
      providers: [
        {
          providerId: 'acme-oidc',
          domain: 'example.com',
          issuer: 'https://idp.example.com',
          type: 'oidc',
        },
        {
          providerId: 'acme-saml',
          domain: 'saml.example',
          issuer: 'https://saml.example',
          type: 'saml',
        },
        {
          providerId: 'bare',
          domain: 'bare.example',
          issuer: 'https://bare.example',
          type: 'unknown',
        },
      ],
    });
    expect(JSON.stringify(result)).not.toContain(SECRET);
    // Scoped to the named organisation.
    expect(paramsOf(mockWhereArgs[0])).toContain('org-1');
  });

  it('maps a database failure to 503 without leaking it', async () => {
    queue(new Error('connection to db.internal refused'));
    const failure = await controller()
      .list(TOKEN, 'org-1')
      .catch((e: unknown) => e);
    expect(failure).toBeInstanceOf(ServiceUnavailableException);
    expect(String((failure as Error).message)).not.toContain('db.internal');
  });
});

describe('SsoProviderAdminController register', () => {
  it('needs an object body and every required field', async () => {
    const c = controller();
    for (const body of [null, 'x', [], undefined]) {
      await expect(c.register(TOKEN, body)).rejects.toBeInstanceOf(
        BadRequestException,
      );
    }
    for (const field of [
      'organizationId',
      'actorUserId',
      'clientId',
      'clientSecret',
    ]) {
      await expect(
        c.register(TOKEN, registerBody({ [field]: '  ' })),
      ).rejects.toBeInstanceOf(BadRequestException);
    }
    expect(mockInsertValues).not.toHaveBeenCalled();
  });

  it('refuses a bad provider id or domain', async () => {
    const c = controller();
    for (const overrides of [
      { providerId: 'ab' },
      { providerId: '-acme' },
      { providerId: 'acme_oidc' },
      { providerId: 'acme oidc' },
      { domain: 'localhost' },
      { domain: 'exa mple.com' },
      { domain: 'example' },
      { domain: '-bad.example.com' },
      { domain: '10.0.0.1' },
    ]) {
      await expect(
        c.register(TOKEN, registerBody(overrides)),
      ).rejects.toBeInstanceOf(BadRequestException);
    }
    expect(mockInsertValues).not.toHaveBeenCalled();
  });

  it.each([
    'http://idp.example.com',
    'idp.example.com',
    'https://user:pw@idp.example.com',
    'https://idp.example.com/?tenant=1',
    'https://idp.example.com/#frag',
    'ftp://idp.example.com',
    'javascript:alert(1)',
    '',
  ])(
    'refuses the issuer %p with the typed invalid_issuer body',
    async (issuer) => {
      const failure = await controller()
        .register(TOKEN, registerBody({ issuer }))
        .catch((e: unknown) => e);
      expect(failure).toBeInstanceOf(BadRequestException);
      expect((failure as BadRequestException).getResponse()).toMatchObject({
        code: 'invalid_issuer',
      });
      expect(mockInsertValues).not.toHaveBeenCalled();
    },
  );

  it('refuses an actor who is not an owner or admin of the organisation', async () => {
    queue([{ role: 'member' }]);
    await expect(
      controller().register(TOKEN, registerBody()),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    queue([]);
    await expect(
      controller().register(TOKEN, registerBody()),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    expect(mockInsertValues).not.toHaveBeenCalled();
  });

  it('answers provider_exists when the id or the domain is taken', async () => {
    queue([{ role: 'admin' }], [{ id: 'existing' }]);
    const failure = await controller()
      .register(TOKEN, registerBody())
      .catch((e: unknown) => e);
    expect(failure).toBeInstanceOf(ConflictException);
    expect((failure as ConflictException).getResponse()).toMatchObject({
      code: 'provider_exists',
    });
    expect(mockInsertValues).not.toHaveBeenCalled();
  });

  it('stores the OIDC config, returns no secret and never logs it', async () => {
    queue([{ role: 'owner' }], [], undefined);
    const result = await controller().register(TOKEN, registerBody());
    expect(result).toEqual({
      provider: {
        providerId: 'acme-oidc',
        domain: 'example.com',
        issuer: 'https://idp.example.com/realms/acme',
        type: 'oidc',
      },
    });
    expect(JSON.stringify(result)).not.toContain(SECRET);

    expect(mockInsertValues).toHaveBeenCalledTimes(1);
    const row = mockInsertValues.mock.calls[0][0];
    expect(row).toMatchObject({
      providerId: 'acme-oidc',
      domain: 'example.com',
      issuer: 'https://idp.example.com/realms/acme',
      userId: 'admin-1',
      organizationId: 'org-1',
    });
    const config = JSON.parse(row.oidcConfig);
    expect(config).toMatchObject({
      issuer: 'https://idp.example.com/realms/acme',
      pkce: true,
      clientId: 'client-1',
      clientSecret: SECRET,
      discoveryEndpoint:
        'https://idp.example.com/realms/acme/.well-known/openid-configuration',
      scopes: ['openid', 'email', 'profile'],
    });
    const logged = JSON.stringify([
      (Logger.prototype.log as jest.Mock).mock.calls,
      (Logger.prototype.error as jest.Mock).mock.calls,
    ]);
    expect(logged).not.toContain(SECRET);
    expect(logged).not.toContain('client-1');
  });

  it('maps a database failure to 503', async () => {
    queue([{ role: 'owner' }], new Error('insert exploded'));
    await expect(
      controller().register(TOKEN, registerBody()),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });
});

describe('SsoProviderAdminController remove', () => {
  it('needs a valid provider id, an organization and an actor', async () => {
    const c = controller();
    await expect(
      c.remove(TOKEN, 'ab', 'org-1', 'admin-1'),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.remove(TOKEN, 'acme-oidc', undefined, 'admin-1'),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.remove(TOKEN, 'acme-oidc', 'org-1', undefined),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.remove(TOKEN, 'a/b/c', 'org-1', 'admin-1'),
    ).rejects.toBeInstanceOf(BadRequestException);
  });

  it('refuses a non-admin actor', async () => {
    queue([{ role: 'member' }]);
    await expect(
      controller().remove(TOKEN, 'acme-oidc', 'org-1', 'member-1'),
    ).rejects.toBeInstanceOf(UnauthorizedException);
  });

  it('deletes only inside the named organisation and answers not-found when nothing matched', async () => {
    queue([{ role: 'admin' }], []);
    const failure = await controller()
      .remove(TOKEN, 'ACME-oidc', 'org-1', 'admin-1')
      .catch((e: unknown) => e);
    expect(failure).toBeInstanceOf(NotFoundException);
    expect((failure as NotFoundException).getResponse()).toMatchObject({
      code: 'provider_not_found',
    });
    // The delete condition carries both the (lower-cased) provider id and the organisation.
    const params = paramsOf(mockWhereArgs[mockWhereArgs.length - 1]);
    expect(params).toEqual(expect.arrayContaining(['acme-oidc', 'org-1']));

    queue([{ role: 'owner' }], [{ id: 'row-1' }]);
    await expect(
      controller().remove(TOKEN, 'acme-oidc', 'org-1', 'owner-1'),
    ).resolves.toEqual({ status: 'removed' });
  });
});

describe('SsoProviderAdminController evidence', () => {
  const windowOf = async (within: string | undefined) => {
    queue([{ id: 1 }]);
    mockWhereArgs.length = 0;
    await controller().evidence(TOKEN, 'org-1', 'user-1', within);
    return paramsOf(mockWhereArgs[0]).filter(
      (value) => typeof value === 'number',
    );
  };

  it('needs an organization and a user', async () => {
    await expect(
      controller().evidence(TOKEN, undefined, 'user-1', '3600'),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      controller().evidence(TOKEN, 'org-1', undefined, '3600'),
    ).rejects.toBeInstanceOf(BadRequestException);
  });

  it('reports recent only when a row exists', async () => {
    queue([{ id: 7 }]);
    await expect(
      controller().evidence(TOKEN, 'org-1', 'user-1', '3600'),
    ).resolves.toEqual({ recent: true });
    queue([]);
    await expect(
      controller().evidence(TOKEN, 'org-1', 'user-1', '3600'),
    ).resolves.toEqual({ recent: false });
  });

  it('clamps the evidence window to 60 seconds .. 7 days and defaults to 24 hours', async () => {
    expect(await windowOf('3600')).toEqual([3600]);
    expect(await windowOf(undefined)).toEqual([86_400]);
    expect(await windowOf('not-a-number')).toEqual([86_400]);
    expect(await windowOf('0')).toEqual([86_400]);
    expect(await windowOf('5')).toEqual([60]);
    expect(await windowOf('99999999')).toEqual([7 * 24 * 3600]);
  });
});

describe('SsoProviderAdminController member-facts', () => {
  it('validates the id list', async () => {
    const c = controller();
    await expect(c.memberFacts(TOKEN, null)).rejects.toBeInstanceOf(
      BadRequestException,
    );
    await expect(
      c.memberFacts(TOKEN, { organizationId: '', userIds: ['u1'] }),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.memberFacts(TOKEN, { organizationId: 'org-1', userIds: 'u1' }),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.memberFacts(TOKEN, { organizationId: 'org-1', userIds: [' '] }),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.memberFacts(TOKEN, {
        organizationId: 'org-1',
        userIds: Array.from({ length: 11 }, (_, i) => `u${i}`),
      }),
    ).rejects.toBeInstanceOf(BadRequestException);
  });

  it('answers an empty list without a query', async () => {
    await expect(
      controller().memberFacts(TOKEN, { organizationId: 'org-1', userIds: [] }),
    ).resolves.toEqual({ members: [] });
    expect(mockWhereArgs).toHaveLength(0);
  });

  it('returns role and a strict two-factor boolean per named user', async () => {
    queue([
      { userId: 'u1', role: 'owner', twoFactorEnabled: true },
      { userId: 'u2', role: 'owner', twoFactorEnabled: false },
      { userId: 'u3', role: 'admin', twoFactorEnabled: null },
    ]);
    await expect(
      controller().memberFacts(TOKEN, {
        organizationId: 'org-1',
        userIds: ['u1', 'u2', 'u3'],
      }),
    ).resolves.toEqual({
      members: [
        { userId: 'u1', role: 'owner', twoFactorEnabled: true },
        { userId: 'u2', role: 'owner', twoFactorEnabled: false },
        { userId: 'u3', role: 'admin', twoFactorEnabled: false },
      ],
    });
    expect(paramsOf(mockWhereArgs[0])).toEqual(
      expect.arrayContaining(['org-1']),
    );
  });
});

describe('SsoProviderAdminController revoke-sessions', () => {
  const body = (overrides: Record<string, unknown> = {}) => ({
    organizationId: 'org-1',
    actorUserId: 'admin-1',
    domains: ['Example.com'],
    ...overrides,
  });

  it('validates the request and the actor', async () => {
    const c = controller();
    await expect(c.revokeSessions(TOKEN, null)).rejects.toBeInstanceOf(
      BadRequestException,
    );
    await expect(
      c.revokeSessions(TOKEN, body({ domains: [] })),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.revokeSessions(TOKEN, body({ domains: 'example.com' })),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.revokeSessions(TOKEN, body({ actorUserId: '' })),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      c.revokeSessions(
        TOKEN,
        body({
          domains: Array.from({ length: 11 }, (_, i) => `d${i}.example`),
        }),
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
    queue([{ role: 'member' }]);
    await expect(c.revokeSessions(TOKEN, body())).rejects.toBeInstanceOf(
      UnauthorizedException,
    );
    expect(mockDeleteSessions).not.toHaveBeenCalled();
  });

  it('ends the sessions of covered users only and spares the exempt ones', async () => {
    queue(
      [{ role: 'owner' }],
      [
        { userId: 'a', email: 'a@Example.com' },
        { userId: 'b', email: 'b@other.example' },
        { userId: 'c', email: 'c@example.com' },
        { userId: 'd', email: 'd@EXAMPLE.COM' },
        { userId: 'e', email: 'not-an-email' },
      ],
    );
    const result = await controller().revokeSessions(
      TOKEN,
      body({ exemptUserIds: ['c'], limit: 100 }),
    );
    expect(result).toEqual({ revoked: 2, nextCursor: '' });
    expect(mockDeleteSessions.mock.calls.map((call) => call[0])).toEqual([
      'a',
      'd',
    ]);
  });

  it('pages in batches with a resumable cursor and clamps the batch size', async () => {
    const page = Array.from({ length: 3 }, (_, i) => ({
      userId: `u${i}`,
      email: `u${i}@example.com`,
    }));
    queue([{ role: 'admin' }], page);
    const first = await controller().revokeSessions(TOKEN, body({ limit: 3 }));
    expect(first).toEqual({ revoked: 3, nextCursor: 'u2' });
    expect(mockLimitArgs[mockLimitArgs.length - 1]).toBe(3);

    // The next call resumes after the cursor.
    queue([{ role: 'admin' }], [{ userId: 'u3', email: 'u3@example.com' }]);
    const second = await controller().revokeSessions(
      TOKEN,
      body({ limit: 3, afterUserId: 'u2' }),
    );
    expect(second).toEqual({ revoked: 1, nextCursor: '' });
    expect(paramsOf(mockWhereArgs[mockWhereArgs.length - 1])).toEqual(
      expect.arrayContaining(['org-1', 'u2']),
    );

    for (const [asked, applied] of [
      [100000, 200],
      [0, 100],
      [undefined, 100],
      [-5, 1],
      [7, 7],
    ] as const) {
      queue([{ role: 'admin' }], []);
      await controller().revokeSessions(TOKEN, body({ limit: asked }));
      expect({
        asked,
        applied: mockLimitArgs[mockLimitArgs.length - 1],
      }).toEqual({ asked, applied });
    }
  });

  it('maps a session-store failure to 503 and never claims success', async () => {
    queue([{ role: 'owner' }], [{ userId: 'a', email: 'a@example.com' }]);
    mockDeleteSessions.mockRejectedValue(new Error('redis down'));
    await expect(
      controller().revokeSessions(TOKEN, body()),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });
});
