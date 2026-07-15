import {
  membershipAuditBearerSessionToken,
  readVerifiedMembershipAuditHookEvidence,
  verifiedAuthoritativeMembershipAuditActorUserId,
  verifiedAuthoritativeMembershipAuditCredentialActorUserId,
  verifiedMembershipAuditActorUserId,
  verifiedMembershipAuditHookEvidence,
  verifiedMembershipAuditSessionActorUserId,
  verifiedMembershipAuditSessionToken,
} from './membership-audit-hook-evidence';

describe('verifiedMembershipAuditHookEvidence', () => {
  const verifiedSession = Object.freeze({
    session: Object.freeze({ userId: 'actor-1' }),
    user: Object.freeze({ id: 'actor-1' }),
  });
  const member = Object.freeze({
    id: 'member-1',
    organizationId: 'org-1',
    userId: 'target-1',
    role: 'admin',
  });
  const roleOperation = Object.freeze({
    action: 'role_changed',
    actorUserId: 'actor-1',
    expectedRevision: 2,
    mutationExpected: true,
    member,
  });
  const removalOperation = Object.freeze({
    ...roleOperation,
    action: 'member_removed',
    expectedRevision: 3,
  });

  it('binds a successful canonical role mutation to the verified session actor', () => {
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        membershipAuditOperation: roleOperation,
        context: { session: verifiedSession, returned: member },
      }),
    ).toEqual({
      action: 'role_changed',
      actorUserId: 'actor-1',
      expectedRevision: 2,
      member,
    });
  });

  it('binds a successful canonical removal response to the verified session actor', () => {
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/remove-member',
        membershipAuditOperation: removalOperation,
        context: {
          session: verifiedSession,
          returned: { member },
        },
      }),
    ).toEqual({
      action: 'member_removed',
      actorUserId: 'actor-1',
      expectedRevision: 3,
      member,
    });
  });

  it('reads the canonical member from Better Auth Response evidence', async () => {
    await expect(
      readVerifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        membershipAuditOperation: roleOperation,
        context: {
          session: verifiedSession,
          returned: Response.json(member),
        },
      }),
    ).resolves.toEqual({
      action: 'role_changed',
      actorUserId: 'actor-1',
      expectedRevision: 2,
      member,
    });
  });

  it('recovers a missing hook actor only from a verified Better Auth session lookup', async () => {
    const resolveVerifiedSession = jest.fn().mockResolvedValue(verifiedSession);

    await expect(
      readVerifiedMembershipAuditHookEvidence(
        {
          path: '/organization/update-member-role',
          membershipAuditOperation: roleOperation,
          body: { actorUserId: 'forged-body-actor' },
          headers: new Headers({ 'x-user-id': 'forged-header-actor' }),
          context: { returned: member },
        },
        resolveVerifiedSession,
      ),
    ).resolves.toEqual({
      action: 'role_changed',
      actorUserId: 'actor-1',
      expectedRevision: 2,
      member,
    });
    expect(resolveVerifiedSession).toHaveBeenCalledTimes(1);
  });

  it.each([
    {
      name: 'mismatched session claims',
      resolver: () =>
        Promise.resolve({
          session: { userId: 'actor-1' },
          user: { id: 'different-actor' },
        }),
    },
    { name: 'missing session', resolver: () => Promise.resolve(null) },
    {
      name: 'session lookup failure',
      resolver: () => Promise.reject(new Error('session store unavailable')),
    },
  ])('fails closed for $name during actor recovery', async ({ resolver }) => {
    await expect(
      readVerifiedMembershipAuditHookEvidence(
        {
          path: '/organization/remove-member',
          membershipAuditOperation: removalOperation,
          body: { actorUserId: 'forged-actor' },
          headers: new Headers({ 'x-user-id': 'forged-actor' }),
          context: { returned: { member } },
        },
        resolver,
      ),
    ).resolves.toBeNull();
  });

  it('does not resolve a session for an unsuccessful canonical response', async () => {
    const resolveVerifiedSession = jest.fn().mockResolvedValue(verifiedSession);

    await expect(
      readVerifiedMembershipAuditHookEvidence(
        {
          path: '/organization/remove-member',
          membershipAuditOperation: removalOperation,
          context: {
            returned: Response.json({ member }, { status: 403 }),
          },
        },
        resolveVerifiedSession,
      ),
    ).resolves.toBeNull();
    expect(resolveVerifiedSession).not.toHaveBeenCalled();
  });

  it('fails closed for unsuccessful or malformed Better Auth responses', async () => {
    await expect(
      readVerifiedMembershipAuditHookEvidence({
        path: '/organization/remove-member',
        membershipAuditOperation: removalOperation,
        context: {
          session: verifiedSession,
          returned: Response.json({ member }, { status: 403 }),
        },
      }),
    ).resolves.toBeNull();
    await expect(
      readVerifiedMembershipAuditHookEvidence({
        path: '/organization/remove-member',
        membershipAuditOperation: removalOperation,
        context: {
          session: verifiedSession,
          returned: new Response('not-json'),
        },
      }),
    ).resolves.toBeNull();
  });

  it('fails closed when session claims disagree', () => {
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        membershipAuditOperation: roleOperation,
        context: {
          session: {
            session: { userId: 'actor-1' },
            user: { id: 'forged-actor' },
          },
          returned: member,
        },
      }),
    ).toBeNull();
  });

  it('never treats request body or headers as actor evidence', () => {
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        body: { actorUserId: 'forged-actor' },
        headers: new Headers({ 'x-user-id': 'forged-actor' }),
        context: { returned: member },
      }),
    ).toBeNull();
  });

  it('requires a server-captured exact revision and matching returned member', () => {
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        membershipAuditOperation: {
          ...roleOperation,
          expectedRevision: 0,
        },
        context: { session: verifiedSession, returned: member },
      }),
    ).toBeNull();
    expect(
      verifiedMembershipAuditHookEvidence({
        path: '/organization/update-member-role',
        membershipAuditOperation: {
          ...roleOperation,
          member: { ...member, userId: 'different-target' },
        },
        context: { session: verifiedSession, returned: member },
      }),
    ).toBeNull();
  });

  it('uses matching session and user claims for database-hook actor evidence', () => {
    expect(
      verifiedMembershipAuditActorUserId({
        context: { session: verifiedSession },
      }),
    ).toBe('actor-1');
    expect(
      verifiedMembershipAuditActorUserId({
        context: {
          session: {
            session: { userId: 'actor-1' },
            user: { id: 'different-actor' },
          },
        },
      }),
    ).toBeNull();
    expect(
      verifiedMembershipAuditActorUserId({
        context: { session: { user: { id: 'actor-1' } } },
      }),
    ).toBeNull();
  });

  it('binds a directly resolved authoritative session only when both claims agree', () => {
    expect(verifiedMembershipAuditSessionActorUserId(verifiedSession)).toBe(
      'actor-1',
    );
    expect(
      verifiedMembershipAuditSessionActorUserId({
        session: { userId: 'actor-1' },
        user: { id: 'different-actor' },
      }),
    ).toBeNull();
    expect(
      verifiedMembershipAuditSessionActorUserId({
        user: { id: 'body-or-header-cannot-fill-this-claim' },
      }),
    ).toBeNull();
  });

  it('accepts only an unexpired server-reread session matching the verified request token and actor', () => {
    const requestSession = {
      session: { userId: 'actor-1', token: 'opaque-session-token' },
      user: { id: 'actor-1' },
    };
    const authoritativeSession = {
      session: {
        userId: 'actor-1',
        token: 'opaque-session-token',
        expiresAt: new Date('2030-01-01T00:00:00.000Z'),
      },
      user: { id: 'actor-1' },
    };

    expect(verifiedMembershipAuditSessionToken(requestSession)).toBe(
      'opaque-session-token',
    );
    expect(
      verifiedAuthoritativeMembershipAuditActorUserId(
        requestSession,
        authoritativeSession,
        new Date('2029-01-01T00:00:00.000Z').getTime(),
      ),
    ).toBe('actor-1');

    for (const invalidAuthoritativeSession of [
      {
        ...authoritativeSession,
        session: {
          ...authoritativeSession.session,
          token: 'different-token',
        },
      },
      {
        ...authoritativeSession,
        user: { id: 'different-actor' },
      },
      {
        ...authoritativeSession,
        session: {
          ...authoritativeSession.session,
          expiresAt: new Date('2028-01-01T00:00:00.000Z'),
        },
      },
    ]) {
      expect(
        verifiedAuthoritativeMembershipAuditActorUserId(
          requestSession,
          invalidAuthoritativeSession,
          new Date('2029-01-01T00:00:00.000Z').getTime(),
        ),
      ).toBeNull();
    }
  });

  it('accepts only a bounded Bearer credential and binds it to the server-reread session', () => {
    expect(
      membershipAuditBearerSessionToken('Bearer opaque-session-token'),
    ).toBe('opaque-session-token');
    for (const malformed of [
      null,
      '',
      'Basic opaque-session-token',
      'Bearer ',
      'Bearer short',
      'Bearer token with spaces',
      'Bearer one,Bearer two',
      `Bearer ${'x'.repeat(2049)}`,
    ]) {
      expect(membershipAuditBearerSessionToken(malformed)).toBeNull();
    }

    const authoritativeSession = {
      session: {
        userId: 'actor-1',
        token: 'opaque-session-token',
        expiresAt: new Date('2030-01-01T00:00:00.000Z'),
      },
      user: { id: 'actor-1' },
    };
    expect(
      verifiedAuthoritativeMembershipAuditCredentialActorUserId(
        'opaque-session-token',
        authoritativeSession,
        new Date('2029-01-01T00:00:00.000Z').getTime(),
      ),
    ).toBe('actor-1');
    expect(
      verifiedAuthoritativeMembershipAuditCredentialActorUserId(
        'different-token',
        authoritativeSession,
        new Date('2029-01-01T00:00:00.000Z').getTime(),
      ),
    ).toBeNull();
  });

  it.each([
    { path: '/organization/update-member-role', context: {} },
    {
      path: '/organization/update-member-role',
      context: { session: verifiedSession, returned: { role: 'admin' } },
    },
    {
      path: '/organization/invite-member',
      context: { session: verifiedSession, returned: member },
    },
  ])('rejects incomplete or unrelated hook evidence', (context) => {
    expect(verifiedMembershipAuditHookEvidence(context)).toBeNull();
  });
});
