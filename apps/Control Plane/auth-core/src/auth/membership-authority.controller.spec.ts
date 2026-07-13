import {
  BadRequestException,
  ServiceUnavailableException,
  UnauthorizedException,
} from '@nestjs/common';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { db } from '../db';
import {
  buildMembershipDecision,
  MembershipAuthorityController,
  requireMembershipAuthoritySecret,
} from './membership-authority.controller';

const TOKEN = '0123456789abcdef0123456789abcdef';

describe('MembershipAuthorityController', () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    process.env = {
      ...originalEnv,
      USER_CORE_MEMBERSHIP_SERVICE_TOKEN: TOKEN,
    };
  });

  afterEach(() => {
    process.env = { ...originalEnv };
    jest.restoreAllMocks();
  });

  it('rejects missing, short, and placeholder credentials at startup', () => {
    for (const value of ['', 'too-short', `change-me-${'x'.repeat(40)}`]) {
      expect(() => requireMembershipAuthoritySecret(value)).toThrow();
    }
    expect(requireMembershipAuthoritySecret(`a${'b'.repeat(31)}`)).toBe(
      `a${'b'.repeat(31)}`,
    );
    expect(() => requireMembershipAuthoritySecret(TOKEN, [TOKEN])).toThrow(
      /dedicated/,
    );
  });

  it('builds an exact canonical allow or deny decision', () => {
    expect(buildMembershipDecision({ role: 'ADMIN' })).toEqual({
      version: 'v1',
      member: true,
      role: 'admin',
    });
    expect(buildMembershipDecision(null)).toEqual({
      version: 'v1',
      member: false,
      role: null,
    });
    expect(() => buildMembershipDecision({ role: 'superuser' })).toThrow();
  });

  it('authenticates the dedicated caller and resolves the exact membership row', async () => {
    const limit = jest.fn().mockResolvedValue([{ role: 'member' }]);
    jest.spyOn(db, 'select').mockReturnValue({
      from: () => ({ where: () => ({ limit }) }),
    } as never);

    const decision = await new MembershipAuthorityController().decide(TOKEN, {
      userId: ' user-1 ',
      orgId: ' org-1 ',
    });

    expect(decision).toEqual({
      version: 'v1',
      member: true,
      role: 'member',
    });
    expect(limit).toHaveBeenCalledWith(2);
  });

  it('fails closed when duplicate canonical membership rows exist', async () => {
    const limit = jest
      .fn()
      .mockResolvedValue([{ role: 'member' }, { role: 'owner' }]);
    jest.spyOn(db, 'select').mockReturnValue({
      from: () => ({ where: () => ({ limit }) }),
    } as never);

    await expect(
      new MembershipAuthorityController().decide(TOKEN, {
        userId: 'user-1',
        orgId: 'org-1',
      }),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });

  it('rejects invalid credentials and inputs before querying the database', async () => {
    const select = jest.spyOn(db, 'select');
    const controller = new MembershipAuthorityController();

    await expect(
      controller.decide('wrong-token', { userId: 'user-1', orgId: 'org-1' }),
    ).rejects.toBeInstanceOf(UnauthorizedException);
    await expect(
      controller.decide(TOKEN, { userId: '', orgId: 'org-1' }),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      controller.decide(TOKEN, null as never),
    ).rejects.toBeInstanceOf(BadRequestException);
    await expect(
      controller.decide(TOKEN, {
        userId: 'user-1',
        orgId: 'org-1',
        role: 'owner',
      } as never),
    ).rejects.toBeInstanceOf(BadRequestException);
    expect(select).not.toHaveBeenCalled();
  });

  it('fails closed when the canonical database is unavailable', async () => {
    const limit = jest
      .fn()
      .mockRejectedValue(new Error('database unavailable'));
    jest.spyOn(db, 'select').mockReturnValue({
      from: () => ({ where: () => ({ limit }) }),
    } as never);

    await expect(
      new MembershipAuthorityController().decide(TOKEN, {
        userId: 'user-1',
        orgId: 'org-1',
      }),
    ).rejects.toBeInstanceOf(ServiceUnavailableException);
  });

  it('ships a duplicate preflight before enforcing unique canonical membership', () => {
    const migration = readFileSync(
      join(process.cwd(), 'migrations/016_unique_organization_membership.sql'),
      'utf8',
    );

    expect(migration).toMatch(/GROUP BY organization_id, user_id/i);
    expect(migration).toMatch(/HAVING COUNT\(\*\) > 1/i);
    expect(migration).toMatch(
      /CREATE UNIQUE INDEX[^;]+member[^;]+organization_id[^;]+user_id/is,
    );
  });
});
