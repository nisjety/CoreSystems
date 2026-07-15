import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import {
  flushInvitationAcceptanceRepairs,
  repairAcceptedInvitationForActor,
  type InvitationAcceptanceRepairRepository,
} from './invitation-acceptance-repair';

describe('durable invitation acceptance repair', () => {
  const completed = {
    invitationId: 'inv_1',
    organizationId: 'org_1',
    memberId: 'member_existing',
    memberRole: 'owner',
  } as const;

  function repository(
    overrides: Partial<InvitationAcceptanceRepairRepository> = {},
  ): InvitationAcceptanceRepairRepository {
    return {
      claimPending: jest.fn().mockResolvedValue([]),
      repair: jest.fn().mockResolvedValue({ kind: 'superseded' }),
      recordFailure: jest.fn().mockResolvedValue(undefined),
      ...overrides,
    };
  }

  it('repairs only the exact verified invitation actor and preserves the existing role', async () => {
    const repair = jest.fn().mockResolvedValue({
      kind: 'completed',
      response: completed,
    });
    const repo = repository({
      repair,
    });

    await expect(
      repairAcceptedInvitationForActor(
        'inv_1',
        { userId: 'user_1', email: ' User@Example.com ' },
        repo,
      ),
    ).resolves.toEqual(completed);
    expect(repair).toHaveBeenCalledWith('inv_1', {
      userId: 'user_1',
      normalizedEmail: 'user@example.com',
    });
  });

  it('does not infer an actor when durable reconciliation runs', async () => {
    const repair = jest.fn().mockResolvedValue({
      kind: 'completed',
      response: completed,
    });
    const repo = repository({
      claimPending: jest
        .fn()
        .mockResolvedValue([{ invitationId: 'inv_1', attempts: 1 }]),
      repair,
    });

    await expect(flushInvitationAcceptanceRepairs(repo)).resolves.toEqual({
      claimed: 1,
      completed: 1,
      superseded: 0,
      notRepairable: 0,
      retried: 0,
      deadLettered: 0,
    });
    expect(repair).toHaveBeenCalledWith('inv_1');
  });

  it('reports superseded and non-repairable claims separately', async () => {
    const repo = repository({
      claimPending: jest.fn().mockResolvedValue([
        { invitationId: 'inv_superseded', attempts: 1 },
        { invitationId: 'inv_not_repairable', attempts: 1 },
      ]),
      repair: jest
        .fn()
        .mockResolvedValueOnce({ kind: 'superseded' })
        .mockResolvedValueOnce({ kind: 'not_repairable' }),
    });

    await expect(flushInvitationAcceptanceRepairs(repo)).resolves.toEqual({
      claimed: 2,
      completed: 0,
      superseded: 1,
      notRepairable: 1,
      retried: 0,
      deadLettered: 0,
    });
  });

  it.each([
    ['', { userId: 'user_1', email: 'user@example.com' }],
    ['inv_1', { userId: '', email: 'user@example.com' }],
    ['inv_1', { userId: 'user_1', email: '   ' }],
  ])(
    'rejects incomplete actor-bound repair input',
    async (invitationId, actor) => {
      const repair = jest.fn();
      await expect(
        repairAcceptedInvitationForActor(
          invitationId,
          actor,
          repository({ repair }),
        ),
      ).resolves.toBeNull();
      expect(repair).not.toHaveBeenCalled();
    },
  );

  it('releases transient failures without blocking independent claims', async () => {
    const failure = new Error('database unavailable with sensitive detail');
    const recordFailure = jest.fn().mockResolvedValue(undefined);
    const repo = repository({
      claimPending: jest.fn().mockResolvedValue([
        { invitationId: 'inv_failure', attempts: 2 },
        { invitationId: 'inv_success', attempts: 1 },
      ]),
      repair: jest.fn().mockRejectedValueOnce(failure).mockResolvedValueOnce({
        kind: 'completed',
        response: completed,
      }),
      recordFailure,
    });

    await expect(flushInvitationAcceptanceRepairs(repo)).resolves.toEqual({
      claimed: 2,
      completed: 1,
      superseded: 0,
      notRepairable: 0,
      retried: 1,
      deadLettered: 0,
    });
    expect(recordFailure).toHaveBeenCalledWith(
      { invitationId: 'inv_failure', attempts: 2 },
      'repair_failed',
    );
  });

  it('reports exhausted repairs as dead letters without exposing identifiers', async () => {
    const repo = repository({
      claimPending: jest
        .fn()
        .mockResolvedValue([
          { invitationId: 'sensitive-invitation', attempts: 5 },
        ]),
      repair: jest
        .fn()
        .mockRejectedValue(new Error('sensitive database detail')),
    });

    await expect(flushInvitationAcceptanceRepairs(repo)).resolves.toEqual({
      claimed: 1,
      completed: 0,
      superseded: 0,
      notRepairable: 0,
      retried: 0,
      deadLettered: 1,
    });
  });

  it('installs a delayed, supersedable, idempotent migration contract', () => {
    const migrationPath = join(
      process.cwd(),
      'migrations/017_invitation_acceptance_repair.sql',
    );
    expect(existsSync(migrationPath)).toBe(true);
    const migration = readFileSync(migrationPath, 'utf8');

    expect(migration).toContain('invitation_acceptance_repair');
    expect(migration).toMatch(/not_before[\s\S]*INTERVAL '30 seconds'/);
    expect(migration).toMatch(
      /OLD\.status = 'pending'[\s\S]*NEW\.status = 'accepted'/,
    );
    expect(migration).toMatch(
      /OLD\.status = 'accepted'[\s\S]*NEW\.status IN \('pending', 'canceled', 'rejected'\)/,
    );
    expect(migration).toContain("state = 'superseded'");
    expect(migration).not.toMatch(
      /INSERT INTO invitation_acceptance_repair[\s\S]*SELECT[\s\S]*FROM invitation i/,
    );
    expect(migration).toContain('AFTER INSERT ON member');
    expect(migration).toContain('AFTER DELETE ON member');
    expect(migration).toContain('ON CONFLICT (invitation_id)');
  });

  it('uses locking claims and conflict-safe membership insertion', () => {
    const source = readFileSync(
      join(process.cwd(), 'src/auth/invitation-acceptance-repair.ts'),
      'utf8',
    );
    expect(source).toContain('FOR UPDATE SKIP LOCKED');
    expect(source).toContain(
      'ON CONFLICT (organization_id, user_id) DO NOTHING',
    );
    expect(source).not.toMatch(/ON CONFLICT[\s\S]{0,160}DO UPDATE SET\s+role/);
    expect(source).not.toContain('active_organization_id');
    expect(source).toContain('actor_classification = ${actorClassification}');
    expect(source).toContain("audit.action = 'member_added'");
    expect(source).not.toMatch(
      /if \(insertedMembers\.length === 1\)[\s\S]{0,800}actorClassification/,
    );
  });
});
