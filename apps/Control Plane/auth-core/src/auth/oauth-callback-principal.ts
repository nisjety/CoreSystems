/**
 * Who an OAuth callback signed in, read from what Better Auth sets on the
 * after-hook context.
 *
 * The /callback/:providerId route creates the session through
 * handleOAuthUserInfo and then calls setSessionCookie, which stores that
 * session as `context.newSession` (`{ session, user }`). Nothing on that path
 * sets `context.user` or `context.account`, so a handler that reads them
 * returns before syncing anything. The user comes from `newSession.user`, and
 * the provider account is the user's account row for the provider named in
 * the path.
 */

export type OAuthCallbackUser = {
  id: string;
  email: string;
  name?: string | null;
  image?: string | null;
  emailVerified?: boolean | null;
  createdAt?: Date | string;
};

export type OAuthCallbackAccount = {
  id: string;
  accountId?: string | null;
  providerId?: string | null;
  updatedAt?: Date | string;
};

/** The parts of the after-hook context the callback reads. */
export type OAuthCallbackContext = {
  user?: OAuthCallbackUser | null;
  account?: OAuthCallbackAccount | null;
  newSession?: { user?: OAuthCallbackUser | null } | null;
};

export type OAuthCallbackPrincipal = {
  user: OAuthCallbackUser;
  account: OAuthCallbackAccount;
};

/** The provider id a callback path names: `/callback/microsoft` is `microsoft`. */
export function providerIdFromCallbackPath(
  path: string | undefined,
  routeProvider?: unknown,
): string | undefined {
  if (typeof routeProvider === 'string' && /^[a-z0-9_-]+$/i.test(routeProvider)) return routeProvider;
  const match = /^\/callback\/([^/?#]+)$/.exec(path ?? '');
  return match?.[1] && /^[a-z0-9_-]+$/i.test(match[1]) ? match[1] : undefined;
}

/**
 * The user and provider account of an OAuth callback, or null when the
 * callback signed nobody in (an error redirect creates no session).
 *
 * `findAccounts` returns every account row of a user. When the context names
 * an account, that one is used; otherwise the user's latest account for the
 * provider is, which is the row the callback has just created or updated.
 */
export async function oauthCallbackPrincipal(
  context: OAuthCallbackContext,
  providerId: string | undefined,
  findAccounts: (userId: string) => Promise<OAuthCallbackAccount[]>,
): Promise<OAuthCallbackPrincipal | null> {
  const user = context.newSession?.user ?? context.user ?? undefined;
  if (!user || !providerId) return null;

  if (context.account?.providerId === providerId) return { user, account: context.account };

  const forProvider = (await findAccounts(user.id)).filter(
    (account) => account.providerId === providerId,
  );
  if (forProvider.length === 0) return null;
  return { user, account: latest(forProvider) };
}

function latest(accounts: OAuthCallbackAccount[]): OAuthCallbackAccount {
  return accounts.reduce((best, account) =>
    updatedTime(account) > updatedTime(best) ? account : best,
  );
}

function updatedTime(account: OAuthCallbackAccount): number {
  if (!account.updatedAt) return 0;
  const time = new Date(account.updatedAt).getTime();
  return Number.isNaN(time) ? 0 : time;
}
