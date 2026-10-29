import {
  oauthCallbackPrincipal,
  providerIdFromCallbackPath,
  type OAuthCallbackAccount,
  type OAuthCallbackUser,
} from './oauth-callback-principal';

const user: OAuthCallbackUser = {
  id: 'user-1',
  email: 'ima.dacosta@coresystem.com',
  name: 'Ima',
  emailVerified: true,
  createdAt: new Date(),
};

const microsoftAccount: OAuthCallbackAccount = {
  id: 'account-ms',
  accountId: 'ms-subject-1',
  providerId: 'microsoft',
  updatedAt: new Date('2026-10-29T10:00:00Z'),
};

describe('providerIdFromCallbackPath', () => {
  it('reads the provider id from /callback/:providerId', () => {
    expect(providerIdFromCallbackPath('/callback/microsoft')).toBe('microsoft');
  });

  it.each([
    '/callback/',
    '/callback/microsoft/extra',
    '/sign-in/social',
    undefined,
  ])('names no provider for %s', (path) => {
    expect(providerIdFromCallbackPath(path)).toBeUndefined();
  });
});

describe('parameterized Better Auth callbacks', () => {
  it('uses the resolved route parameter instead of the route template', () => {
    expect(providerIdFromCallbackPath('/callback/:id', 'microsoft')).toBe('microsoft');
    expect(providerIdFromCallbackPath('/callback/:id')).toBeUndefined();
    expect(providerIdFromCallbackPath('/callback/:id', '../microsoft')).toBeUndefined();
  });
});

describe('oauthCallbackPrincipal', () => {
  it('reads the user from the session Better Auth created, and the provider account from the user', async () => {
    // The shape Better Auth gives the after-hook on /callback/:id: the session
    // it created, and no `user` or `account` on the context.
    const findAccounts = jest.fn().mockResolvedValue([microsoftAccount]);

    const principal = await oauthCallbackPrincipal(
      { newSession: { user } },
      'microsoft',
      findAccounts,
    );

    expect(principal).toEqual({ user, account: microsoftAccount });
    expect(findAccounts).toHaveBeenCalledWith('user-1');
  });

  it('uses the user and account the context names, without a lookup', async () => {
    const findAccounts = jest.fn();

    const principal = await oauthCallbackPrincipal(
      { user, account: microsoftAccount },
      'microsoft',
      findAccounts,
    );

    expect(principal).toEqual({ user, account: microsoftAccount });
    expect(findAccounts).not.toHaveBeenCalled();
  });

  it('signs nobody in when there is no session, as after an error redirect', async () => {
    const findAccounts = jest.fn();

    await expect(
      oauthCallbackPrincipal({ newSession: null }, 'microsoft', findAccounts),
    ).resolves.toBeNull();
    await expect(
      oauthCallbackPrincipal({}, 'microsoft', findAccounts),
    ).resolves.toBeNull();
    expect(findAccounts).not.toHaveBeenCalled();
  });

  it('signs nobody in when the user has no account for the provider the path names', async () => {
    const googleOnly: OAuthCallbackAccount = {
      id: 'account-google',
      providerId: 'google',
    };

    await expect(
      oauthCallbackPrincipal(
        { newSession: { user } },
        'microsoft',
        jest.fn().mockResolvedValue([googleOnly]),
      ),
    ).resolves.toBeNull();
  });

  it('takes the account row updated last when the user has several for the provider', async () => {
    const older: OAuthCallbackAccount = {
      id: 'account-old',
      accountId: 'ms-subject-1',
      providerId: 'microsoft',
      updatedAt: new Date('2026-09-01T00:00:00Z'),
    };
    const newer: OAuthCallbackAccount = {
      ...microsoftAccount,
      id: 'account-new',
      updatedAt: new Date('2026-10-29T09:00:00Z'),
    };

    const principal = await oauthCallbackPrincipal(
      { newSession: { user } },
      'microsoft',
      jest.fn().mockResolvedValue([older, newer]),
    );

    expect(principal?.account.id).toBe('account-new');
  });
});
