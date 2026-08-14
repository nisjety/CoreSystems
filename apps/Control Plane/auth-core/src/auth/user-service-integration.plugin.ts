/**
 * Better Auth Integration Plugin
 *
 * Integrates Better Auth events with the user-service for syncing
 * registrations, logins, profile updates, OAuth, and sign-outs.
 */

/* eslint-disable @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access */
import { APIError, createAuthMiddleware } from 'better-auth/api';
import type { BetterAuthPlugin } from 'better-auth';
import { sql } from 'drizzle-orm';
import { db } from '../db';
import * as schema from '../db/schema';
import { AuthIntegrationService } from '../internal/auth-integration.service';

let authIntegrationService: AuthIntegrationService | null = null;

export function setAuthIntegrationService(service: AuthIntegrationService) {
  console.log('🔧 [Plugin] Setting AuthIntegrationService:', !!service);
  authIntegrationService = service;
  console.log('✅ [Plugin] AuthIntegrationService set successfully');
}

// Sole-organization auto-activation used to live here as a route-matched
// post-hoc patcher (Redis blob patch + Postgres UPDATE). It never fired on
// Microsoft logins: `/sign-in/social` isn't matched by the email sign-in
// matcher, and on `/callback/:id` the after-hook context carries no
// `user`/`account`, so the handler bailed before reaching it. It now runs
// as a `databaseHooks.session.create.before` hook instead — see
// sole-org-auto-activation.ts — which covers every session-creation path
// and lands the org id in the session before any store or cookie sees it.

export function userServiceIntegrationPlugin(): BetterAuthPlugin {
  return {
    id: 'user-service-integration',
    hooks: {
      after: [
        // Debug: Log all paths to understand Better Auth routing
        {
          matcher: ({ path }) => {
            console.log('🔍 [Plugin Debug] Any path called:', path);
            return false; // Don't handle, just log
          },
          handler: createAuthMiddleware(async () => {
            // This won't execute since matcher returns false
          }),
        },

        // Sign-up (email/password) - Match various signup patterns
        {
          matcher: ({ path }) => {
            console.log('🔍 [Plugin Debug] Checking signup path:', path);
            const currentPath = path ?? '';
            const isMatch =
              currentPath.includes('/sign-up') ||
              currentPath === '/sign-up/email' ||
              currentPath === '/sign-up';
            console.log('🔍 [Plugin Debug] Signup path match result:', isMatch);
            return isMatch;
          },
          handler: createAuthMiddleware(async (ctx) => {
            console.log('🚀 [Plugin] Sign-up handler triggered!');
            console.log(
              '🔍 [Plugin Debug] Full context keys:',
              Object.keys(ctx.context),
            );
            // Note: Cannot stringify full context due to circular references in DB objects

            if (!authIntegrationService) {
              console.log('❌ [Plugin] AuthIntegrationService not available');
              return;
            }

            // Try to get user from different context properties. `returned`
            // is not part of the typed hook context, so narrow it once
            // instead of casting to `any` at every read.
            const contextWithReturned = ctx.context as {
              returned?: {
                user?: RegisteredUserLike;
                data?: { user?: RegisteredUserLike };
              };
            };
            const user = (ctx.context.user ||
              ctx.context.newUser ||
              contextWithReturned.returned?.user ||
              contextWithReturned.returned?.data?.user) as unknown as
              | RegisteredUserLike
              | undefined;

            console.log('🔍 [Plugin Debug] User extraction:', {
              contextUser: !!ctx.context.user,
              contextNewUser: !!ctx.context.newUser,
              returnedUser: !!contextWithReturned.returned?.user,
              returnedDataUser: !!contextWithReturned.returned?.data?.user,
              returnedKeys: contextWithReturned.returned
                ? Object.keys(contextWithReturned.returned)
                : null,
            });

            if (!user) {
              console.log('❌ [Plugin] No user found in any context location');
              return;
            }
            console.log(
              '✅ [Plugin] Processing user registration:',
              user.email,
            );
            await authIntegrationService.handleUserRegistration({
              id: user.id,
              email: user.email,
              name: user.name || undefined,
              emailVerified: user.emailVerified || false,
              provider: 'email',
            });
          }),
        },

        // Sign-in (email/password)
        {
          matcher: ({ path }) =>
            path === '/sign-in/email' || path === '/sign-in',
          handler: createAuthMiddleware(async (ctx) => {
            const user = ctx.context.user;
            const newSession = ctx.context.newSession;
            if (!user || !newSession?.session) return;
            if (!authIntegrationService) return;
            const headers = ctx.request?.headers;
            const userAgent = headers?.get('user-agent') ?? undefined;
            const ipHeader =
              headers?.get('x-forwarded-for') ||
              headers?.get('x-real-ip') ||
              undefined;
            await authIntegrationService.handleUserLogin({
              userId: user.id,
              email: user.email,
              // include name for downstream enrichment (optional)
              name: (user as { name?: string | null }).name ?? undefined,
              sessionId: newSession.session.id,
              deviceInfo: userAgent,
              ipAddress: ipHeader,
              userAgent,
              provider: 'email',
              // Better Auth does NOT set activeOrganizationId at raw sign-in
              // time; the org plugin only sets it after an explicit
              // /organization/set-active call. The session.create.before
              // database hook (sole-org-auto-activation.ts) fills it for
              // single-org accounts before the session is stored, so this
              // reads the freshly-activated id on that path, a carried-over
              // id on re-authentication, or undefined for a genuinely
              // org-less / multi-org session still awaiting a manual switch.
              activeOrganizationId: sessionActiveOrganizationId(
                newSession.session,
              ),
            });
          }),
        },

        // Profile update
        {
          matcher: ({ path }) => (path ?? '').includes('/update-user'),
          handler: createAuthMiddleware(async (ctx) => {
            if (!authIntegrationService) return;
            const user = ctx.context.user;
            if (!user) return;
            const changes = (ctx.body as Record<string, unknown>) ?? {};
            await authIntegrationService.handleUserProfileUpdate({
              userId: user.id,
              email: user.email,
              changes,
            });
          }),
        },

        // OAuth callback (e.g., /callback/microsoft, /callback/google, /callback/vipps)
        // Three cases:
        //   1. isNewUser  → handleUserRegistration  (first-ever signup via OAuth)
        //   2. !isNewUser → handleAccountLinked     (linking new provider to existing user
        //                                            OR returning OAuth login — both upsert
        //                                            the provider_account row idempotently)
        //   + Always call handleUserLogin when a session exists so last_login &
        //     session events are tracked for every OAuth authentication
        {
          matcher: ({ path }) => (path ?? '').startsWith('/callback/'),
          handler: createAuthMiddleware(async (ctx) => {
            // `user`/`account` are untyped on the after-hook context; narrow
            // them once to the structural shapes this handler reads.
            const user = ctx.context.user as OAuthCallbackUser | undefined;
            const account = ctx.context.account as
              | OAuthCallbackAccount
              | undefined;
            const newSession = ctx.context.newSession;
            if (!user || !account) return;
            if (!authIntegrationService) return;

            const headers = ctx.request?.headers;
            const userAgent = headers?.get('user-agent') ?? undefined;
            const ipHeader =
              headers?.get('x-forwarded-for') ??
              headers?.get('x-real-ip') ??
              undefined;

            const isNewUser = isRecentlyCreated(
              (user as { createdAt?: Date | string }).createdAt,
            );
            console.log(
              `🔗 [Plugin] OAuth callback: ${user.email}, provider: ${account.providerId}, isNewUser: ${isNewUser}`,
            );

            // The resolve* helpers take loose provider payloads; the typed
            // Better Auth account object is narrowed once here.
            const accountRecord = account as unknown as Record<string, unknown>;

            if (isNewUser) {
              // Brand-new user registered via OAuth
              const providerAccountId = resolveProviderAccountId(account);
              const scopesGranted = resolveScopesGranted(accountRecord);
              const tenantId = resolveTenantID(accountRecord);
              const emailFromProvider = resolveProviderEmail(
                user,
                accountRecord,
              );
              const profileHints = resolveProfileHints(user, accountRecord);
              await authIntegrationService.handleUserRegistration({
                id: user.id,
                email: user.email,
                name: user.name || undefined,
                emailVerified: user.emailVerified || false,
                provider: account.providerId || 'oauth',
                metadata: {
                  accountId: account.id,
                  providerId: account.providerId,
                },
                microsoftTenantId: tenantId,
                emailFromProvider,
                scopesGranted,
                tokenRef: providerAccountId,
                profileHints,
              });
            } else {
              // Existing user - account linking
              const providerAccountId = resolveProviderAccountId(account);
              const profileHints = resolveProfileHints(user, accountRecord);
              const scopesGranted = resolveScopesGranted(accountRecord);

              await authIntegrationService.handleAccountLinked({
                id: user.id,
                email: user.email,
                name: user.name || undefined,
                provider: account.providerId || 'oauth',
                providerAccountId,
                scopesGranted,
                tokenRef: providerAccountId,
                profileHints,
              });
            }

            if (newSession?.session) {
              await authIntegrationService.handleUserLogin({
                userId: user.id,
                email: user.email,
                name: (user as { name?: string | null }).name ?? undefined,
                sessionId: newSession.session.id,
                deviceInfo: userAgent,
                ipAddress: ipHeader,
                userAgent,
                provider: account.providerId || 'oauth',
                // Better Auth does not set activeOrganizationId during the OAuth
                // callback either; the session.create.before database hook
                // (sole-org-auto-activation.ts) covers single-org accounts, so
                // this reads the freshly-activated id, a carried-over id on
                // re-authentication, or undefined pending a manual switch.
                activeOrganizationId: sessionActiveOrganizationId(
                  newSession.session,
                ),
              });
            }
          }),
        },
      ],

      before: [
        // Sign-up (email/password)
        {
          matcher: ({ path }) => path === '/sign-up/email',
          handler: createAuthMiddleware(async (ctx) => {
            const email = normalizeEmail(
              (ctx.body as { email?: unknown })?.email,
            );
            if (!email) return;

            const [existingUser] = await db
              .select({ id: schema.user.id })
              .from(schema.user)
              .where(sql`lower(${schema.user.email}) = ${email}`)
              .limit(1);

            if (!existingUser) return;

            throw new APIError('CONFLICT', {
              code: 'USER_ALREADY_EXISTS',
              message:
                'An account with this email already exists. Sign in with the existing password or reset it.',
            });
          }),
        },

        // Sign-out
        {
          matcher: ({ path }) => path === '/sign-out',
          handler: createAuthMiddleware(async (ctx) => {
            if (!authIntegrationService) return;
            const headers = ctx.request?.headers;
            const fromHeader = headers?.get('session-id') ?? undefined;
            const fromBody = (ctx.body as { sessionId?: string } | undefined)
              ?.sessionId;
            const cookieHeader = headers?.get('cookie') ?? undefined;
            const sessionId =
              fromHeader || fromBody || extractSessionFromCookies(cookieHeader);
            if (!sessionId) return;
            const user = ctx.context.user;
            if (!user) return;
            // ctx.context.session is the session being terminated; it carries
            // activeOrganizationId when the user had an active org in this session.
            const terminatedSession = ctx.context.session as object | null;
            const activeOrganizationId = terminatedSession
              ? sessionActiveOrganizationId(terminatedSession)
              : undefined;
            await authIntegrationService.handleUserLogout({
              userId: user.id,
              email: user.email,
              sessionId,
              reason: 'manual',
              activeOrganizationId,
            });
          }),
        },
      ],
    },
  };
}

function normalizeEmail(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const email = value.trim().toLowerCase();
  return email.length > 0 ? email : null;
}

/**
 * Returns true if createdAt is within the last 30 seconds.
 * Used to distinguish new OAuth sign-ups from account linking on the /callback/* path.
 */
function isRecentlyCreated(createdAt: Date | string | undefined): boolean {
  if (!createdAt) return true; // assume new user if timestamp is missing
  const created = createdAt instanceof Date ? createdAt : new Date(createdAt);
  return Date.now() - created.getTime() < 30_000; // 30-second window
}

// Utilities
function extractSessionFromCookies(
  cookieHeader: string | null | undefined,
): string | undefined {
  if (!cookieHeader) return undefined;
  const SESSION_COOKIE_NAME = process.env.SESSION_COOKIE_NAME || 'sid';
  const cookies = cookieHeader.split(';').reduce(
    (acc, cookie) => {
      const [key, value] = cookie.trim().split('=');
      acc[key] = value;
      return acc;
    },
    {} as Record<string, string>,
  );
  return (
    cookies[SESSION_COOKIE_NAME] ||
    cookies['idknuten.session_token'] ||
    cookies['better-auth.session_token'] ||
    cookies['session_token'] ||
    cookies['session']
  );
}

/**
 * Minimal structural shape of the user object surfaced by the sign-up hook
 * context; the concrete object varies by Better Auth flow/version.
 */
type RegisteredUserLike = {
  id: string;
  email: string;
  name?: string | null;
  emailVerified?: boolean | null;
};

/**
 * Structural shapes of the user/account objects the OAuth callback hook
 * reads. They arrive untyped (`any`) on the after-hook context, so the
 * handler narrows to exactly the fields it consumes.
 */
type OAuthCallbackUser = {
  id: string;
  email: string;
  name?: string | null;
  image?: string | null;
  emailVerified?: boolean | null;
  createdAt?: Date | string;
};

type OAuthCallbackAccount = {
  id: string;
  accountId?: string | null;
  providerId?: string | null;
};

/**
 * `activeOrganizationId` is an organization-plugin field, absent from the
 * core Session type; read it structurally instead of via `any`. Preserves
 * the historical `?? undefined` semantics (null becomes undefined).
 */
function sessionActiveOrganizationId(session: object): string | undefined {
  const value = (session as { activeOrganizationId?: string | null })
    .activeOrganizationId;
  return value ?? undefined;
}

/**
 * Prefer the provider-side account id, fall back to the row id — the same
 * fallback the account-linking path has always used.
 */
function resolveProviderAccountId(account: {
  accountId?: string | null;
  id: string;
}): string {
  return account.accountId || account.id;
}

function resolveScopesGranted(account: Record<string, unknown>): string[] {
  const directScopes = account.scopes;
  if (Array.isArray(directScopes)) {
    return directScopes
      .filter((item): item is string => typeof item === 'string')
      .map((item) => item.trim())
      .filter((item) => item.length > 0);
  }

  const scopeValue = account.scope;
  if (typeof scopeValue === 'string' && scopeValue.trim() !== '') {
    return scopeValue
      .split(/\s+/)
      .map((item) => item.trim())
      .filter((item) => item.length > 0);
  }

  return [];
}

function resolveTenantID(account: Record<string, unknown>): string | undefined {
  const candidates: unknown[] = [
    account.tenantId,
    account.tenant_id,
    account.microsoftTenantId,
  ];

  for (const value of candidates) {
    if (typeof value === 'string' && value.trim() !== '') {
      return value.trim();
    }
  }

  const claims = account.idTokenClaims;
  if (claims && typeof claims === 'object') {
    const tid = (claims as { tid?: unknown }).tid;
    if (typeof tid === 'string' && tid.trim() !== '') {
      return tid.trim();
    }
  }

  return undefined;
}

function resolveProviderEmail(
  user: { email: string },
  account: Record<string, unknown>,
): string {
  const email = account.email;
  if (typeof email === 'string' && email.trim() !== '') {
    return email.trim();
  }
  return user.email;
}

function resolveProfileHints(
  user: {
    name?: string | null;
    image?: string | null;
    locale?: string | null;
    timezone?: string | null;
  },
  account: Record<string, unknown>,
): {
  displayName?: string;
  avatar?: string;
  locale?: string;
  timezone?: string;
} {
  const displayName =
    (typeof account.displayName === 'string' &&
    account.displayName.trim() !== ''
      ? account.displayName
      : typeof user.name === 'string'
        ? user.name
        : undefined) || undefined;

  const avatar =
    (typeof account.image === 'string' && account.image.trim() !== ''
      ? account.image
      : typeof user.image === 'string'
        ? user.image
        : undefined) || undefined;

  const locale =
    (typeof account.locale === 'string' && account.locale.trim() !== ''
      ? account.locale
      : typeof account.preferredLanguage === 'string' &&
          account.preferredLanguage.trim() !== ''
        ? account.preferredLanguage
        : typeof user.locale === 'string'
          ? user.locale
          : undefined) || undefined;

  const timezone =
    (typeof account.timeZone === 'string' && account.timeZone.trim() !== ''
      ? account.timeZone
      : typeof account.timezone === 'string' && account.timezone.trim() !== ''
        ? account.timezone
        : typeof user.timezone === 'string'
          ? user.timezone
          : undefined) || undefined;

  return {
    displayName:
      typeof displayName === 'string' && displayName.trim() !== ''
        ? displayName.trim()
        : undefined,
    avatar:
      typeof avatar === 'string' && avatar.trim() !== ''
        ? avatar.trim()
        : undefined,
    locale:
      typeof locale === 'string' && locale.trim() !== ''
        ? locale.trim()
        : undefined,
    timezone:
      typeof timezone === 'string' && timezone.trim() !== ''
        ? timezone.trim()
        : undefined,
  };
}
