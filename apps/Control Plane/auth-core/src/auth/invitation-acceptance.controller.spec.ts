type GetSession = (input: { headers: Headers }) => Promise<{
  user: { id: string; email: string };
} | null>;
type AuthHandler = (request: Request) => Promise<Response>;

const mockGetSession = jest.fn<
  ReturnType<GetSession>,
  Parameters<GetSession>
>();
const mockAuthHandler = jest.fn<
  ReturnType<AuthHandler>,
  Parameters<AuthHandler>
>();
const mockRepairAcceptedInvitationForActor = jest.fn();
const mockDbLimit = jest.fn();
const mockDbWhere = jest.fn(() => ({ limit: mockDbLimit }));
const mockDbInnerJoin = jest.fn(() => ({ where: mockDbWhere }));
const mockDbFrom = jest.fn(() => ({ innerJoin: mockDbInnerJoin }));
const mockDbSelect = jest.fn(() => ({ from: mockDbFrom }));
jest.mock('./auth', () => ({
  auth: {
    handler: mockAuthHandler,
    api: {
      getSession: mockGetSession,
    },
  },
}));
jest.mock('../db', () => ({ db: { select: mockDbSelect } }));
jest.mock('./invitation-acceptance-repair', () => ({
  repairAcceptedInvitationForActor: mockRepairAcceptedInvitationForActor,
}));

import {
  acceptedInvitationRetryResponse,
  InvitationAcceptanceController,
  type AcceptedInvitationRecord,
} from './invitation-acceptance.controller';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('accepted invitation retry contract', () => {
  const record: AcceptedInvitationRecord = {
    invitationId: 'inv_123',
    invitationEmail: 'Invitee@Example.com',
    invitationOrganizationId: 'org_invited',
    invitationStatus: 'accepted',
    invitationRole: 'member',
    memberId: 'member_123',
    memberUserId: 'user_123',
    memberOrganizationId: 'org_invited',
    memberRole: 'member',
  };

  beforeEach(() => {
    jest.clearAllMocks();
    process.env.BETTER_AUTH_SECRET = 'unit-test-auth-secret';
    mockGetSession.mockResolvedValue({
      user: { id: 'user_123', email: 'invitee@example.com' },
    });
    mockRepairAcceptedInvitationForActor.mockResolvedValue(null);
  });

  it('returns the existing accepted membership only for the invited actor', () => {
    expect(
      acceptedInvitationRetryResponse([record], {
        userId: 'user_123',
        email: 'invitee@example.com',
      }),
    ).toEqual({
      invitation: {
        id: 'inv_123',
        organizationId: 'org_invited',
        status: 'accepted',
      },
      member: {
        id: 'member_123',
        organizationId: 'org_invited',
        role: 'member',
      },
    });
  });

  it.each([
    { userId: 'other_user', email: 'invitee@example.com' },
    { userId: 'user_123', email: 'other@example.com' },
  ])('fails closed for an actor mismatch: %o', (actor) => {
    expect(acceptedInvitationRetryResponse([record], actor)).toBeNull();
  });

  it('fails closed for ambiguous or cross-organization records', () => {
    expect(
      acceptedInvitationRetryResponse([record, { ...record }], {
        userId: 'user_123',
        email: 'invitee@example.com',
      }),
    ).toBeNull();
    expect(
      acceptedInvitationRetryResponse(
        [{ ...record, memberOrganizationId: 'org_other' }],
        { userId: 'user_123', email: 'invitee@example.com' },
      ),
    ).toBeNull();
  });

  it('uses Better Auth for the first acceptance attempt', async () => {
    const accepted = {
      invitation: {
        id: 'inv_123',
        organizationId: 'org_invited',
        status: 'accepted',
      },
      member: {
        id: 'member_123',
        organizationId: 'org_invited',
        role: 'member',
      },
    };
    mockAuthHandler.mockImplementation(async (request: Request) => {
      expect(request.method).toBe('POST');
      expect(new URL(request.url).pathname).toBe(
        '/api/auth/organization/accept-invitation',
      );
      expect(request.headers.get('origin')).toBe('https://verevon.example');
      expect(request.headers.get('cookie')).toContain('session=opaque');
      const actorAddress = request.headers.get('x-forwarded-for');
      expect(actorAddress).toMatch(/^fd[0-9a-f]{2}(?::[0-9a-f]{4}){7}$/);
      expect(request.headers.get('cf-connecting-ip')).toBe(actorAddress);
      expect(request.headers.get('x-real-ip')).toBe(actorAddress);
      expect(request.headers.get('true-client-ip')).toBe(actorAddress);
      expect(request.headers.get('x-verevon-invitation-acceptance')).toMatch(
        /^v1\.\d{10}\.[A-Za-z0-9_-]{43}$/,
      );
      await expect(request.json()).resolves.toEqual({
        invitationId: 'inv_123',
      });
      return jsonResponse(accepted);
    });

    await expect(
      new InvitationAcceptanceController().accept(' inv_123 ', {
        headers: {
          cookie: ['session=opaque', 'session_aux=opaque'],
          origin: 'https://verevon.example',
          'cf-connecting-ip': '203.0.113.1',
          'x-forwarded-for': '203.0.113.2',
          'x-real-ip': '203.0.113.3',
          'true-client-ip': '203.0.113.4',
          'x-optional': undefined,
        },
      } as never),
    ).resolves.toEqual(accepted);
    expect(mockAuthHandler).toHaveBeenCalledTimes(1);
    expect(mockDbSelect).not.toHaveBeenCalled();
  });

  it('returns the accepted membership when a committed response is retried', async () => {
    mockAuthHandler.mockResolvedValue(
      jsonResponse(
        { code: 'INVITATION_NOT_FOUND', message: 'Invitation not found' },
        400,
      ),
    );
    mockDbLimit.mockResolvedValue([record]);

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).resolves.toEqual(
      acceptedInvitationRetryResponse([record], {
        userId: 'user_123',
        email: 'invitee@example.com',
      }),
    );
  });

  it('repairs an accepted partial state only after the canonical handler settles', async () => {
    const settled: string[] = [];
    mockAuthHandler.mockImplementation(async () => {
      await Promise.resolve();
      settled.push('handler');
      return jsonResponse(
        { code: 'INVITATION_NOT_FOUND', message: 'Invitation not found' },
        400,
      );
    });
    mockRepairAcceptedInvitationForActor.mockImplementation(async () => {
      await Promise.resolve();
      settled.push('repair');
      return {
        invitationId: 'inv_123',
        organizationId: 'org_invited',
        memberId: 'member_repaired',
        memberRole: 'member',
      };
    });

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).resolves.toEqual({
      invitation: {
        id: 'inv_123',
        organizationId: 'org_invited',
        status: 'accepted',
      },
      member: {
        id: 'member_repaired',
        organizationId: 'org_invited',
        role: 'member',
      },
    });
    expect(settled).toEqual(['handler', 'repair']);
    expect(mockDbSelect).not.toHaveBeenCalled();
  });

  it('does not attempt partial-state repair for rate-limited requests', async () => {
    mockAuthHandler.mockResolvedValue(
      jsonResponse({ code: 'TOO_MANY_REQUESTS', message: 'slow down' }, 429),
    );

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).rejects.toMatchObject({ status: 429 });
    expect(mockRepairAcceptedInvitationForActor).not.toHaveBeenCalled();
  });

  it.each([
    [400, 'INVITATION_NOT_FOUND'],
    [403, 'YOU_ARE_NOT_THE_RECIPIENT_OF_THE_INVITATION'],
  ])(
    'normalizes invitation lookup failure %i/%s when no accepted membership exists',
    async (status, code) => {
      mockAuthHandler.mockResolvedValue(
        jsonResponse(
          { code, message: 'differentiated upstream detail' },
          status,
        ),
      );
      mockDbLimit.mockResolvedValue([]);

      await expect(
        new InvitationAcceptanceController().accept('inv_123', {
          headers: { cookie: 'session=opaque' },
        } as never),
      ).rejects.toMatchObject({
        status: 400,
        response: {
          code: 'INVITATION_NOT_FOUND',
          message: 'Invitation is invalid, expired, or unavailable.',
        },
      });
    },
  );

  it('preserves a bounded rate-limit response from the canonical Auth router', async () => {
    mockAuthHandler.mockResolvedValue(
      jsonResponse({ code: 'TOO_MANY_REQUESTS', message: 'slow down' }, 429),
    );
    mockDbLimit.mockResolvedValue([]);

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).rejects.toMatchObject({
      status: 429,
      response: {
        code: 'RATE_LIMITED',
        message: 'Too many invitation attempts.',
      },
    });
    expect(mockDbSelect).not.toHaveBeenCalled();
  });

  it('rejects malformed IDs and missing sessions before mutation', async () => {
    const controller = new InvitationAcceptanceController();
    await expect(
      controller.accept('../invite', { headers: {} } as never),
    ).rejects.toMatchObject({ status: 400 });
    expect(mockGetSession).not.toHaveBeenCalled();

    mockGetSession.mockResolvedValue(null);
    await expect(
      controller.accept('inv_123', { headers: {} } as never),
    ).rejects.toMatchObject({ status: 401 });
    expect(mockAuthHandler).not.toHaveBeenCalled();
  });

  it('fails closed when retry state cannot be read', async () => {
    mockAuthHandler.mockResolvedValue(
      jsonResponse(
        { code: 'INVITATION_NOT_FOUND', message: 'Invitation not found' },
        400,
      ),
    );
    mockDbLimit.mockRejectedValue(new Error('database unavailable'));

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).rejects.toMatchObject({ status: 503 });
  });

  it('maps an operational router failure to a bounded 503 response', async () => {
    mockAuthHandler.mockRejectedValue(new Error('database unavailable'));
    mockDbLimit.mockResolvedValue([]);

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).rejects.toMatchObject({
      status: 503,
      response: {
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      },
    });
  });

  it('maps session preflight failures to a bounded 503 response', async () => {
    mockGetSession.mockRejectedValue(new Error('cache unavailable'));

    await expect(
      new InvitationAcceptanceController().accept('inv_123', {
        headers: { cookie: 'session=opaque' },
      } as never),
    ).rejects.toMatchObject({
      status: 503,
      response: {
        code: 'INVITATION_AUTHORITY_UNAVAILABLE',
        message: 'Invitation authority is unavailable.',
      },
    });
    expect(mockAuthHandler).not.toHaveBeenCalled();
  });
});
