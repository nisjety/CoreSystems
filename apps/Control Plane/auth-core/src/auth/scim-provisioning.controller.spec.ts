/**
 * WS05-S8: org-core's internal SCIM membership surface on auth-core. The database is a
 * queue-driven stub (each awaited statement takes the next queued result; an Error in the
 * queue is thrown instead) and the auth adapter is a spy, so nothing leaves the process.
 */
const mockQueue: unknown[] = [];
const mockStatements: Array<{ text: string; values: unknown[] }> = [];
const mockCreateUser = jest.fn();

function mockNextResult(): unknown {
  if (mockQueue.length === 0) return [];
  const next = mockQueue.shift();
  if (next instanceof Error) throw next;
  return next;
}

jest.mock('./auth', () => ({
  auth: {
    $context: Promise.resolve({
      internalAdapter: {
        createUser: (...args: unknown[]) => mockCreateUser(...args),
      },
    }),
  },
}));

jest.mock('../db', () => {
  const statement = (strings: TemplateStringsArray, ...values: unknown[]) => {
    mockStatements.push({ text: strings.join('?'), values });
    try {
      return Promise.resolve(mockNextResult());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  return {
    db: {},
    sqlClient: Object.assign(statement, {
      begin: (work: (tx: typeof statement) => Promise<unknown>) =>
        work(statement),
    }),
  };
});

import {
  BadRequestException,
  ConflictException,
  HttpException,
  Logger,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';

import { ScimProvisioningController } from './scim-provisioning.controller';

const TOKEN = 'scim-provisioning-token-0123456789abcdef';
const ORG = 'org-scim-1';
const OPERATION = '6f0c5f2e-7a4b-4c1d-9e3f-2a1b0c9d8e7f';
const ACTOR = 'u-owner';

function controller(): ScimProvisioningController {
  process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = TOKEN;
  return new ScimProvisioningController();
}

function queue(...results: unknown[]): void {
  mockQueue.push(...results);
}

/** A postgres.js command result: an empty row list that reports its row count. */
function commandResult(count: number): unknown {
  return Object.assign([], { count });
}

function provisionBody(
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    operationId: OPERATION,
    organizationId: ORG,
    actorUserId: ACTOR,
    email: 'Ada@Corp.Example.com',
    ...overrides,
  };
}

function deprovisionBody(
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    operationId: OPERATION,
    organizationId: ORG,
    actorUserId: ACTOR,
    userId: 'user-1',
    ...overrides,
  };
}

async function rejectionOf(work: Promise<unknown>): Promise<unknown> {
  try {
    await work;
  } catch (error) {
    return error;
  }
  throw new Error('expected the call to reject');
}

function responseOf(error: unknown): unknown {
  return error instanceof HttpException ? error.getResponse() : undefined;
}

beforeEach(() => {
  mockQueue.length = 0;
  mockStatements.length = 0;
  mockCreateUser.mockReset();
  mockCreateUser.mockResolvedValue({ id: 'user-new' });
  jest.spyOn(Logger.prototype, 'log').mockImplementation(() => undefined);
  jest.spyOn(Logger.prototype, 'error').mockImplementation(() => undefined);
});

afterEach(() => {
  jest.restoreAllMocks();
});

describe('ScimProvisioningController (WS05-S8)', () => {
  it('refuses every call without the dedicated credential', async () => {
    const c = controller();
    await expect(
      c.provision('wrong-token', provisionBody()),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    await expect(
      c.provision(undefined, provisionBody()),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    await expect(
      c.deprovision('wrong-token', deprovisionBody()),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    expect(mockStatements).toHaveLength(0);
    expect(mockCreateUser).not.toHaveBeenCalled();
  });

  it('is unavailable until the credential is configured', async () => {
    process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN = 'short';
    const short = new ScimProvisioningController();
    await expect(
      short.provision(TOKEN, provisionBody()),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
    delete process.env.ORG_CORE_IDENTITY_SERVICE_TOKEN;
    const unset = new ScimProvisioningController();
    await expect(
      unset.deprovision(TOKEN, deprovisionBody()),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
    expect(mockStatements).toHaveLength(0);
  });

  it('validates the bodies before touching the database', async () => {
    const c = controller();
    await expect(c.provision(TOKEN, 'not-an-object')).rejects.toBeInstanceOf(
      BadRequestException,
    );
    await expect(
      c.provision(TOKEN, provisionBody({ operationId: 'not-a-uuid' })),
    ).rejects.toBeInstanceOf(BadRequestException);

    const badEmail = await rejectionOf(
      c.provision(TOKEN, provisionBody({ email: 'not an email' })),
    );
    expect(responseOf(badEmail)).toMatchObject({ code: 'invalid_email' });

    // SCIM can never grant owner: the role is refused, not clamped.
    const ownerRole = await rejectionOf(
      c.provision(TOKEN, provisionBody({ role: 'owner' })),
    );
    expect(responseOf(ownerRole)).toMatchObject({ code: 'invalid_role' });

    const missingUser = await rejectionOf(
      c.deprovision(TOKEN, deprovisionBody({ userId: '' })),
    );
    expect(missingUser).toBeInstanceOf(BadRequestException);
    const badOperation = await rejectionOf(
      c.deprovision(TOKEN, deprovisionBody({ operationId: 'nope' })),
    );
    expect(badOperation).toBeInstanceOf(BadRequestException);

    expect(mockStatements).toHaveLength(0);
    expect(mockCreateUser).not.toHaveBeenCalled();
  });

  it('creates the user without a password and adds the member as org-core names it', async () => {
    queue(
      [{ id: ORG }], // the organisation exists
      [], // no user with that e-mail yet
      [{ id: 'member-1', role: 'member' }], // the membership row is inserted
      commandResult(1), // exactly one audit transition is bound to the actor
    );
    const result = await controller().provision(TOKEN, provisionBody());

    expect(result).toEqual({
      userId: 'user-new',
      memberId: 'member-1',
      role: 'member',
      memberCreated: true,
      userCreated: true,
    });
    expect(mockCreateUser).toHaveBeenCalledWith({
      email: 'ada@corp.example.com',
      name: 'ada',
      emailVerified: false,
    });
    expect(mockCreateUser.mock.calls[0][0]).not.toHaveProperty('password');

    const insert = mockStatements.find((s) =>
      s.text.includes('INSERT INTO member'),
    );
    expect(insert?.values).toEqual(
      expect.arrayContaining([ORG, 'user-new', 'member']),
    );
    const audit = mockStatements.find((s) =>
      s.text.includes('organization_membership_audit_outbox'),
    );
    expect(audit?.values).toEqual(
      expect.arrayContaining([ACTOR, ORG, 'user-new', 'member-1', 'member']),
    );
  });

  it('answers an existing membership without binding a second audit row', async () => {
    queue(
      [{ id: ORG }],
      [{ id: 'user-1' }],
      [], // the insert hits the unique membership and returns nothing
      [{ id: 'member-1', role: 'admin' }], // the canonical row is read back
    );
    const result = await controller().provision(
      TOKEN,
      provisionBody({ role: 'admin' }),
    );

    expect(result).toEqual({
      userId: 'user-1',
      memberId: 'member-1',
      role: 'admin',
      memberCreated: false,
      userCreated: false,
    });
    expect(mockCreateUser).not.toHaveBeenCalled();
    expect(
      mockStatements.some((s) =>
        s.text.includes('organization_membership_audit_outbox'),
      ),
    ).toBe(false);
  });

  it('rolls the membership back and answers 503 when the audit transition is not bound', async () => {
    queue(
      [{ id: ORG }],
      [{ id: 'user-1' }],
      [{ id: 'member-1', role: 'member' }],
      commandResult(0),
    );
    const error = await rejectionOf(
      controller().provision(TOKEN, provisionBody()),
    );

    expect(error).toBeInstanceOf(ServiceUnavailableException);
    expect((error as HttpException).message).toBe(
      'SCIM provisioning unavailable',
    );
  });

  it('answers an unknown organisation with a typed bad request', async () => {
    queue([]);
    const error = await rejectionOf(
      controller().provision(TOKEN, provisionBody()),
    );

    expect(error).toBeInstanceOf(BadRequestException);
    expect(responseOf(error)).toMatchObject({
      code: 'organization_not_found',
    });
    expect(mockCreateUser).not.toHaveBeenCalled();
  });

  it('answers a database outage with 503 and no connection detail', async () => {
    queue(new Error('connection refused by 10.0.0.5:5432'));
    const error = await rejectionOf(
      controller().provision(TOKEN, provisionBody()),
    );

    expect(error).toBeInstanceOf(ServiceUnavailableException);
    expect((error as HttpException).message).not.toContain('10.0.0.5');
  });

  it('deprovisions through the membership mutation as a scim_deprovision', async () => {
    queue(
      [], // no earlier completed operation
      [{ id: 'member-1', role: 'member', revision: '4' }],
      [{ result: { revision: 5 } }],
    );
    const result = await controller().deprovision(TOKEN, deprovisionBody());

    expect(result).toEqual({
      removed: true,
      replay: false,
      role: 'member',
      revision: 5,
    });
    const apply = mockStatements.find((s) =>
      s.text.includes('apply_membership_mutation'),
    );
    expect(apply?.text).toContain("'scim_deprovision'");
    expect(apply?.values).toEqual(
      expect.arrayContaining([
        OPERATION,
        ORG,
        'user-1',
        'member-1',
        4,
        'member',
        'member',
        ACTOR,
      ]),
    );
  });

  it('answers a retried operation from the stored result without applying it again', async () => {
    queue([{ previous_role: 'admin', audit_revision: '9' }]);
    const result = await controller().deprovision(TOKEN, deprovisionBody());

    expect(result).toEqual({
      removed: true,
      replay: true,
      role: 'admin',
      revision: 9,
    });
    expect(
      mockStatements.some((s) =>
        s.text.includes('apply_membership_mutation'),
      ),
    ).toBe(false);
  });

  it('reports a user who is already not a member as not removed', async () => {
    queue([], []);
    const result = await controller().deprovision(TOKEN, deprovisionBody());

    expect(result).toEqual({ removed: false, replay: false, role: '', revision: 0 });
  });

  it('refuses to deprovision the only owner with 409 last_owner', async () => {
    queue(
      [],
      [{ id: 'member-1', role: 'owner', revision: '2' }],
      new Error('cannot remove or demote the only owner'),
    );
    const error = await rejectionOf(
      controller().deprovision(TOKEN, deprovisionBody()),
    );

    expect(error).toBeInstanceOf(ConflictException);
    expect(responseOf(error)).toMatchObject({ code: 'last_owner' });
  });

  it.each([
    'membership mutation conflict',
    'membership mutation projection revision conflicted',
  ])(
    'maps the concurrent change "%s" to 409 membership_conflict',
    async (message) => {
      queue(
        [],
        [{ id: 'member-1', role: 'member', revision: '2' }],
        new Error(message),
      );
      const error = await rejectionOf(
        controller().deprovision(TOKEN, deprovisionBody()),
      );

      expect(error).toBeInstanceOf(ConflictException);
      expect(responseOf(error)).toMatchObject({ code: 'membership_conflict' });
    },
  );
});
