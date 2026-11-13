// F-f08-12: isUserBanned reads only user.banned and user.banExpires. The
// database is replaced by a select-chain double that answers the rows given;
// the drizzle schema stays real.
const mockRows: { value: unknown[] | Error } = { value: [] };
const mockSelect = jest.fn();
jest.mock('../db', () => ({
  db: {
    select: (...args: unknown[]) => {
      mockSelect(...args);
      const chain = {
        from: () => chain,
        where: () => chain,
        limit: () =>
          mockRows.value instanceof Error
            ? Promise.reject(mockRows.value)
            : Promise.resolve(mockRows.value),
      };
      return chain;
    },
  },
}));

import { isUserBanned } from './plane-token-membership';

describe('isUserBanned (F-f08-12)', () => {
  const now = new Date('2026-11-12T12:00:00Z');

  beforeEach(() => {
    mockRows.value = [];
    mockSelect.mockReset();
  });

  it('a ban without an expiry is banned', async () => {
    mockRows.value = [{ banned: true, banExpires: null }];
    await expect(isUserBanned('user-a', now)).resolves.toBe(true);
  });

  it('a ban that expires later is banned', async () => {
    mockRows.value = [
      { banned: true, banExpires: new Date('2026-11-13T00:00:00Z') },
    ];
    await expect(isUserBanned('user-a', now)).resolves.toBe(true);
  });

  it('an expired ban (banExpires in the past) is not banned', async () => {
    mockRows.value = [
      { banned: true, banExpires: new Date('2026-11-11T00:00:00Z') },
    ];
    await expect(isUserBanned('user-a', now)).resolves.toBe(false);
  });

  it('a ban expiring exactly now is no longer banned', async () => {
    mockRows.value = [{ banned: true, banExpires: new Date(now.getTime()) }];
    await expect(isUserBanned('user-a', now)).resolves.toBe(false);
  });

  it('banned false or null is not banned', async () => {
    mockRows.value = [{ banned: false, banExpires: null }];
    await expect(isUserBanned('user-a', now)).resolves.toBe(false);
    mockRows.value = [{ banned: null, banExpires: null }];
    await expect(isUserBanned('user-a', now)).resolves.toBe(false);
  });

  it('an unknown user id is not banned', async () => {
    mockRows.value = [];
    await expect(isUserBanned('nobody', now)).resolves.toBe(false);
  });

  it('a blank user id is not banned and does not query', async () => {
    await expect(isUserBanned('  ', now)).resolves.toBe(false);
    expect(mockSelect).not.toHaveBeenCalled();
  });

  it('a database error propagates (the caller answers 503)', async () => {
    mockRows.value = new Error('db down');
    await expect(isUserBanned('user-a', now)).rejects.toThrow('db down');
  });

  it('selects only the ban flag and its expiry, never email or name', async () => {
    mockRows.value = [{ banned: false, banExpires: null }];
    await isUserBanned('user-a', now);
    expect(mockSelect).toHaveBeenCalledTimes(1);
    const [projection] = mockSelect.mock.calls[0] as [Record<string, unknown>];
    expect(Object.keys(projection).sort()).toEqual(['banExpires', 'banned']);
  });
});
