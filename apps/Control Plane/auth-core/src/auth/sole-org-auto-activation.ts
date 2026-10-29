/**
 * Sole-organization auto-activation, applied when a session is created.
 *
 * A brand-new Better Auth session never carries `activeOrganizationId` —
 * the organization plugin only sets it once something explicitly calls
 * `/organization/set-active`, which today only happens from the manual
 * org-switcher UI. A user who belongs to exactly one organization has
 * nothing to actually choose, so every fresh sign-in otherwise lands them
 * in a broken "no active org" state (every org-scoped request fails) until
 * they manually switch once. Multi-org accounts are deliberately left
 * untouched: there the ambiguity is real and switching stays a user action.
 *
 * This must run as a `databaseHooks.session.create.before` hook, NOT as a
 * route-matched after-hook, for three structural reasons (all observed in
 * production while diagnosing the "fresh Microsoft login lands in a dead
 * personal workspace" bug):
 *
 * 1. Path-independence. Sessions are created by `/sign-in/email`,
 *    `/sign-in/social`, `/callback/:id`, `/sso/callback/:providerId`,
 *    passkeys, admin impersonation, etc. The previous route-matched
 *    implementation only covered email sign-in, and on the OAuth callback
 *    its `ctx.context.user` / `ctx.context.account` guards were never
 *    satisfied, so Microsoft logins never got an active org. A database
 *    hook runs for every session Better Auth creates, whatever route
 *    created it.
 * 2. Sessions live in secondaryStorage (Dragonfly/Redis) and the Postgres
 *    `session` table stays empty. `createWithHooks` hands this hook's
 *    returned data to the secondaryStorage write, so the org id is in the
 *    stored blob from the start — no post-hoc patching of a cached blob
 *    and no dead Postgres UPDATE.
 * 3. The session-data cookie cache is minted from the session object
 *    `createSession` returns, which is also this hook's returned data, so
 *    a stale "no active org" cookie can never be issued for a sole-org
 *    user.
 */
import { eq } from 'drizzle-orm';
import { db } from '../db';
import * as schema from '../db/schema';

/**
 * Returns the user's organization id when they belong to exactly one
 * organization; null for zero or several.
 */
export async function resolveSoleOrganizationId(
  userId: string,
): Promise<string | null> {
  const memberships = await db
    .select({ organizationId: schema.member.organizationId })
    .from(schema.member)
    .where(eq(schema.member.userId, userId))
    // "exactly one" only needs to distinguish 0 / 1 / more-than-1.
    .limit(2);
  if (memberships.length !== 1) return null;
  return memberships[0].organizationId;
}

/**
 * Session-create decorator: returns a copy of the session with the sole
 * organization activated for single-org users, and the session unchanged
 * for everyone else. Best-effort by contract — auto-activation must never
 * break sign-in itself, so lookup failures fail open.
 */
export async function withSoleOrganizationActivated<
  T extends { userId: string } & Record<string, unknown>,
>(session: T): Promise<T> {
  try {
    // Respect an explicitly pre-seeded org (e.g. a flow that creates the
    // session with an organization on purpose).
    const current = session.activeOrganizationId;
    if (typeof current === 'string' && current.length > 0) return session;

    const organizationId = await resolveSoleOrganizationId(session.userId);
    if (!organizationId) return session;

    console.log(
      `✅ [Auth] Activating sole organization ${organizationId} for user ${session.userId} at session creation`,
    );
    return { ...session, activeOrganizationId: organizationId };
  } catch (error) {
    console.error(
      '⚠️ [Auth] Sole-org auto-activation failed (sign-in continues without an active org):',
      error,
    );
    return session;
  }
}
