import {
  captureMembershipAuditOperation,
  captureMembershipAuditOperationResult,
  isMembershipMutationPath,
} from './membership-audit-operation';

describe('captureMembershipAuditOperation', () => {
  const member = Object.freeze({
    id: 'member-1',
    organizationId: 'org-1',
    userId: 'target-1',
    role: 'member',
    revision: '1',
  });

  it('binds a role mutation to the exact observed durable revision', async () => {
    const findMember = jest.fn().mockResolvedValue([member]);

    await expect(
      captureMembershipAuditOperation(
        {
          path: '/organization/update-member-role',
          body: {
            organizationId: 'org-1',
            memberId: 'member-1',
            role: 'admin',
          },
        },
        'actor-1',
        findMember,
      ),
    ).resolves.toEqual({
      action: 'role_changed',
      kind: 'role_change',
      actorUserId: 'actor-1',
      observedRevision: 1,
      mutationExpected: true,
      previousRole: 'member',
      member: {
        id: 'member-1',
        organizationId: 'org-1',
        userId: 'target-1',
        role: 'admin',
      },
    });
    expect(findMember).toHaveBeenCalledWith('org-1', 'member-1');
  });

  it('binds an email-addressed removal to its exact member and next revision', async () => {
    const findMember = jest.fn().mockResolvedValue([member]);

    await expect(
      captureMembershipAuditOperation(
        {
          path: '/organization/remove-member',
          body: {
            organizationId: 'org-1',
            memberIdOrEmail: 'target@example.com',
          },
        },
        'actor-1',
        findMember,
      ),
    ).resolves.toEqual({
      action: 'member_removed',
      kind: 'admin_remove',
      actorUserId: 'actor-1',
      observedRevision: 1,
      mutationExpected: true,
      previousRole: 'member',
      member: {
        id: 'member-1',
        organizationId: 'org-1',
        userId: 'target-1',
        role: 'member',
      },
    });
  });

  it('captures a same-role operation against its observed revision', async () => {
    const findMember = jest.fn().mockResolvedValue([member]);
    await expect(
      captureMembershipAuditOperation(
        {
          path: '/organization/update-member-role',
          body: {
            organizationId: 'org-1',
            memberId: 'member-1',
            role: 'member',
          },
        },
        'actor-1',
        findMember,
      ),
    ).resolves.toEqual({
      action: 'role_changed',
      kind: 'role_change',
      actorUserId: 'actor-1',
      observedRevision: 1,
      mutationExpected: false,
      previousRole: 'member',
      member: {
        id: 'member-1',
        organizationId: 'org-1',
        userId: 'target-1',
        role: 'member',
      },
    });
  });

  it('captures self-leave as an exact removal by the verified actor identity', async () => {
    const findMember = jest.fn().mockResolvedValue([member]);

    await expect(
      captureMembershipAuditOperation(
        {
          path: '/organization/leave',
          body: { organizationId: 'org-1' },
        },
        'target-1',
        findMember,
      ),
    ).resolves.toEqual({
      action: 'member_removed',
      kind: 'self_leave',
      actorUserId: 'target-1',
      observedRevision: 1,
      mutationExpected: true,
      previousRole: 'member',
      member: {
        id: 'member-1',
        organizationId: 'org-1',
        userId: 'target-1',
        role: 'member',
      },
    });
    expect(findMember).toHaveBeenCalledWith('org-1', 'target-1');
  });

  it.each([
    '/organization/update-member-role',
    '/organization/remove-member',
    '/organization/leave',
  ])('guards the %s canonical mutation path', (path) => {
    expect(isMembershipMutationPath(path)).toBe(true);
  });

  it('does not classify unrelated Better Auth routes as membership mutations', () => {
    expect(isMembershipMutationPath('/organization/list-members')).toBe(false);
  });

  it.each([
    { name: 'missing actor', actor: '', rows: [member] },
    { name: 'missing member', actor: 'actor-1', rows: [] },
    { name: 'ambiguous member', actor: 'actor-1', rows: [member, member] },
    {
      name: 'unsafe revision',
      actor: 'actor-1',
      rows: [{ ...member, revision: Number.MAX_SAFE_INTEGER }],
    },
  ])('fails closed for $name', async ({ actor, rows }) => {
    await expect(
      captureMembershipAuditOperation(
        {
          path: '/organization/remove-member',
          body: {
            organizationId: 'org-1',
            memberIdOrEmail: 'member-1',
          },
        },
        actor,
        () => Promise.resolve(rows),
      ),
    ).resolves.toBeNull();
  });

  it('distinguishes a canonical missing member so Better Auth can return MEMBER_NOT_FOUND', async () => {
    await expect(
      captureMembershipAuditOperationResult(
        {
          path: '/organization/remove-member',
          body: {
            organizationId: 'org-1',
            memberIdOrEmail: 'already-removed-member',
          },
        },
        'actor-1',
        () => Promise.resolve([]),
      ),
    ).resolves.toEqual({ status: 'not_found' });
  });

  it.each([
    ['comma-separated roles', 'admin, member'],
    ['role arrays', ['admin', 'member']],
    ['unsupported roles', 'sales'],
    ['empty roles', '   '],
  ])(
    'rejects %s before reading or mutating membership state',
    async (_name, role) => {
      const findMember = jest.fn().mockResolvedValue([member]);

      await expect(
        captureMembershipAuditOperationResult(
          {
            path: '/organization/update-member-role',
            body: {
              organizationId: 'org-1',
              memberId: 'member-1',
              role,
            },
          },
          'actor-1',
          findMember,
        ),
      ).resolves.toEqual({ status: 'bad_request' });
      expect(findMember).not.toHaveBeenCalled();
    },
  );

  it('canonicalizes one supported role before predicting its transition', async () => {
    const findMember = jest.fn().mockResolvedValue([member]);

    await expect(
      captureMembershipAuditOperationResult(
        {
          path: '/organization/update-member-role',
          body: {
            organizationId: 'org-1',
            memberId: 'member-1',
            role: '  ADMIN  ',
          },
        },
        'actor-1',
        findMember,
      ),
    ).resolves.toMatchObject({
      status: 'captured',
      operation: {
        previousRole: 'member',
        member: { role: 'admin' },
      },
    });
  });
});
