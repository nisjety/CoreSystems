/**
 * Better Auth Integration Plugin
 *
 * Integrates Better Auth events with the user-service for syncing
 * registrations, logins, profile updates, OAuth, and sign-outs.
 */

/* eslint-disable @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unsafe-member-access */
import { APIError, createAuthMiddleware } from 'better-auth/api';
import type { BetterAuthPlugin } from 'better-auth';
import { eq, sql } from 'drizzle-orm';
import { db } from '../db';
import * as schema from '../db/schema';
import { redisSecondaryStorage } from '../db/redis';
import { AuthIntegrationService } from '../internal/auth-integration.service';

let authIntegrationService: AuthIntegrationService | null = null;

export function setAuthIntegrationService(service: AuthIntegrationService) {
  console.log('🔧 [Plugin] Setting AuthIntegrationService:', !!service);
  authIntegrationService = service;
  console.log('✅ [Plugin] AuthIntegrationService set successfully');
}

/**
 * A brand-new Better Auth session never carries `activeOrganizationId` —
 * Better Auth only sets it once something explicitly calls
 * `/organization/set-active`, which today only happens from the manual
 * org-switcher UI. A user who belongs to exactly one organization has
 * nothing to actually choose, so every fresh sign-in otherwise lands them
 * in a broken "no active org" state (every org-scoped request fails) until
 * they manually switch once. This closes that gap without touching
 * multi-org accounts, where the ambiguity is real and switching stays a
 * deliberate user action.
 *
 * Writes both backing stores directly (Redis via the same
 * `secondaryStorage` Better Auth itself reads sessions from, plus Postgres
 * for any reader that goes straight to the DB) rather than calling
 * `auth.api.setActiveOrganization` — that endpoint authenticates the caller
 * from cookies on the INCOMING request, which for a just-created session
 * during its own sign-in hook don't exist yet.
 */
async function autoActivateSoleOrganization(
  userId: string,
  sessionToken: string,
): Promise<string | undefined> {
  try {
    const memberships = await db
      .select({ organizationId: schema.member.organizationId })
      .from(schema.member)
      .where(eq(schema.member.userId, userId));
    if (memberships.length !== 1) return undefined;
    const organizationId = memberships[0].organizationId;

    const raw = await redisSecondaryStorage.get(sessionToken);
    if (raw) {
      const cached = JSON.parse(raw) as {
        session?: { activeOrganizationId?: string | null; expiresAt?: string };
      };
      if (cached.session && !cached.session.activeOrganizationId) {
        const expiresAtMs = cached.session.expiresAt
          ? new Date(cached.session.expiresAt).getTime()
          : Date.now();
        const ttlSeconds = Math.max(
          1,
          Math.floor((expiresAtMs - Date.now()) / 1000),
        );
        cached.session.activeOrganizationId = organizationId;
        await redisSecondaryStorage.set(
          sessionToken,
          JSON.stringify(cached),
          ttlSeconds,
        );
      }
    }

    await db
      .update(schema.session)
      .set({ activeOrganizationId: organizationId })
      .where(eq(schema.session.token, sessionToken));

    console.log(
      `✅ [Plugin] Auto-activated sole organization ${organizationId} for user ${userId}`,
    );
    return organizationId;
  } catch (error) {
    // Best-effort UX enhancement — never let this break sign-in itself.
    console.error(
      '⚠️ [Plugin] Auto-activate sole organization failed:',
      error,
    );
    return undefined;
  }
}

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

            // Try to get user from different context properties
            const user =
              ctx.context.user ||
              ctx.context.newUser ||
              (ctx.context as any).returned?.user ||
              (ctx.context as any).returned?.data?.user;

            console.log('🔍 [Plugin Debug] User extraction:', {
              contextUser: !!ctx.context.user,
              contextNewUser: !!ctx.context.newUser,
              returnedUser: !!(ctx.context as any).returned?.user,
              returnedDataUser: !!(ctx.context as any).returned?.data?.user,
              returnedKeys: (ctx.context as any).returned
                ? Object.keys((ctx.context as any).returned as object)
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
            const activatedOrgId = await autoActivateSoleOrganization(
              user.id,
              newSession.session.token,
            );
            if (activatedOrgId) {
              (newSession.session as any).activeOrganizationId = activatedOrgId;
            }
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
              // /organization/set-active call. autoActivateSoleOrganization
              // above makes that call implicitly for single-org accounts and
              // patches this in-memory session object, so this reads the
              // freshly-activated id on that path, a carried-over id on
              // re-authentication, or undefined for a genuinely org-less /
              // multi-org session still awaiting a manual switch.
              activeOrganizationId:
                (newSession.session as any).activeOrganizationId ?? undefined,
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
            const user = ctx.context.user;
            const account = ctx.context.account;
            const newSession = ctx.context.newSession;
            if (!user || !account) return;
            if (newSession?.session) {
              const activatedOrgId = await autoActivateSoleOrganization(
                user.id,
                newSession.session.token,
              );
              if (activatedOrgId) {
                (newSession.session as any).activeOrganizationId = activatedOrgId;
              }
            }
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

            if (isNewUser) {
              // Brand-new user registered via OAuth
              const providerAccountId = this.resolveProviderAccountId(
                account,
                user,
              );
              const scopesGranted = this.resolveScopesGranted(account);
              const tenantId = this.resolveTenantID(account, user);
              const emailFromProvider = this.resolveProviderEmail(
                account,
                user,
              );
              const profileHints = this.resolveProfileHints(account, user);
              const metadata = {
                accountId: providerAccountId,
                providerId: account.providerId,
              };
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
              const providerAccountId =
                (account as { accountId?: string }).accountId || account.id;
              const tenantId = this.resolveTenantID(account, user);
              const profileHints = this.resolveProfileHints(account, user);
              const scopesGranted = this.resolveScopesGranted(account);

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
                // callback either; autoActivateSoleOrganization above covers
                // single-org accounts and patches this in-memory object, so
                // this reads the freshly-activated id, a carried-over id on
                // re-authentication, or undefined pending a manual switch.
                activeOrganizationId:
                  (newSession.session as any).activeOrganizationId ?? undefined,
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
            const activeOrganizationId =
              (ctx.context.session as any)?.activeOrganizationId ?? undefined;
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
