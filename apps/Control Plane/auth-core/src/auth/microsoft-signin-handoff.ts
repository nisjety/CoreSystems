/**
 * Microsoft sign-in → integration-corev2 hand-off.
 *
 * A Verevon user who signs in with Microsoft has already consented to a Graph
 * token. Until this hook, that token stayed in Better Auth's `account` table
 * while the Microsoft *integration* connection (Support inbox, Teams,
 * SharePoint/OneDrive) lived a separate life in integration-corev2 with its
 * own consent and its own refresh token — which silently expired after two
 * idle days and left every lane failed until someone clicked "Koble til på
 * nytt". After every session creation this pushes the fresh sign-in token to
 * integration-core, which adopts it into the org's Microsoft connection
 * (widening, never narrowing, its capabilities) and records the account row
 * id so it can ask auth-core for a re-mint later. The connection therefore
 * refreshes itself on every login and holds between logins.
 *
 * Runs as a `databaseHooks.session.create.after` hook for the same reason
 * sole-org activation does: it fires for every session Better Auth creates,
 * whatever route created it (see sole-org-auto-activation.ts). It is strictly
 * best-effort — nothing here may break sign-in.
 */
import { createDecipheriv } from 'node:crypto';
import { and, desc, eq } from 'drizzle-orm';
import { db } from '../db';
import * as schema from '../db/schema';

export const MICROSOFT_PROVIDER_ID = 'microsoft';

export type MicrosoftAccountTokens = Readonly<{
  /** Better Auth `account.id` — integration-core keeps it as its token ref. */
  tokenRef: string;
  /** Microsoft object id of the signed-in account (`account.account_id`). */
  providerAccountId: string;
  accessToken: string;
  refreshTokenPresent: boolean;
  expiresAt: Date | null;
  /** Granted scopes as Better Auth stores them (comma-separated). */
  scope: string | null;
}>;

export type SignInHandoffPayload = Readonly<{
  organizationId: string;
  userId: string;
  userEmail?: string;
  providerAccountId: string;
  accessToken: string;
  expiresAt: string | null;
  scopes: readonly string[];
  tokenRef: string;
}>;

export type SignInHandoffOutcome =
  | 'sent'
  | 'skipped:no-organization'
  | 'skipped:no-microsoft-account'
  | 'skipped:token-expired'
  | 'skipped:not-configured'
  | 'failed';

type SessionLike = Readonly<{
  userId: string;
  activeOrganizationId?: string | null;
}> &
  Record<string, unknown>;

type HandoffDeps = Readonly<{
  fetch?: typeof fetch;
  env?: Readonly<Record<string, string | undefined>>;
  now?: () => Date;
  loadAccount?: (userId: string) => Promise<MicrosoftAccountTokens | null>;
  loadUserEmail?: (userId: string) => Promise<string | undefined>;
  log?: Pick<Console, 'warn' | 'log'>;
}>;

const DEFAULT_INTEGRATION_CORE_URL = 'http://integration-api:3026';
const HANDOFF_TIMEOUT_MS = 5_000;

/**
 * Mirrors the AES-256-GCM `iv.tag.ciphertext` format `encryptToken` writes in
 * auth.ts and `InternalOAuthService.decryptMaybe` reads. Returns the input
 * unchanged when no key is configured or the value is not in that format,
 * matching the dev-path behaviour of both.
 */
export function decryptProviderToken(
  value: string,
  keyB64: string | undefined = process.env.TOKEN_ENCRYPTION_KEY,
): string {
  if (!keyB64) return value;
  const segments = value.split('.');
  if (segments.length !== 3) return value;
  const key = Buffer.from(keyB64, 'base64');
  if (key.length !== 32) return value;
  try {
    const decipher = createDecipheriv(
      'aes-256-gcm',
      key,
      Buffer.from(segments[0], 'base64'),
    );
    decipher.setAuthTag(Buffer.from(segments[1], 'base64'));
    return Buffer.concat([
      decipher.update(Buffer.from(segments[2], 'base64')),
      decipher.final(),
    ]).toString('utf8');
  } catch {
    return value;
  }
}

/** Better Auth stores scopes comma-separated; providers hand them back
 * space-separated. Accept both; drop empties and duplicates. */
export function splitScopes(scope: string | null | undefined): string[] {
  if (!scope) return [];
  return [
    ...new Set(
      scope
        .split(/[\s,]+/)
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0),
    ),
  ];
}

export async function loadMicrosoftAccount(
  userId: string,
): Promise<MicrosoftAccountTokens | null> {
  const rows = await db
    .select({
      id: schema.account.id,
      accountId: schema.account.accountId,
      accessToken: schema.account.accessToken,
      refreshToken: schema.account.refreshToken,
      accessTokenExpiresAt: schema.account.accessTokenExpiresAt,
      scope: schema.account.scope,
    })
    .from(schema.account)
    .where(
      and(
        eq(schema.account.userId, userId),
        eq(schema.account.providerId, MICROSOFT_PROVIDER_ID),
      ),
    )
    .orderBy(desc(schema.account.updatedAt))
    .limit(1);
  const row = rows[0];
  if (!row || typeof row.accessToken !== 'string' || !row.accessToken) {
    return null;
  }
  return {
    tokenRef: row.id,
    providerAccountId: row.accountId,
    accessToken: decryptProviderToken(row.accessToken),
    refreshTokenPresent:
      typeof row.refreshToken === 'string' && row.refreshToken.length > 0,
    expiresAt: coerceDate(row.accessTokenExpiresAt),
    scope: row.scope ?? null,
  };
}

async function loadUserEmail(userId: string): Promise<string | undefined> {
  const rows = await db
    .select({ email: schema.user.email })
    .from(schema.user)
    .where(eq(schema.user.id, userId))
    .limit(1);
  return rows[0]?.email ?? undefined;
}

/**
 * Builds the hand-off body, or null with a reason when the session cannot be
 * mapped onto an org-scoped connection. Only a non-expired token is worth
 * pushing: a password login does not refresh a linked Microsoft account, and
 * pushing its stale token would overwrite a fresher one integration-core may
 * already hold.
 */
export function buildSignInHandoffPayload(
  session: SessionLike,
  account: MicrosoftAccountTokens | null,
  userEmail: string | undefined,
  now: Date = new Date(),
): { payload: SignInHandoffPayload } | { skip: SignInHandoffOutcome } {
  const organizationId =
    typeof session.activeOrganizationId === 'string'
      ? session.activeOrganizationId.trim()
      : '';
  if (!organizationId) return { skip: 'skipped:no-organization' };
  if (!account) return { skip: 'skipped:no-microsoft-account' };
  if (account.expiresAt && account.expiresAt.getTime() <= now.getTime()) {
    return { skip: 'skipped:token-expired' };
  }
  return {
    payload: {
      organizationId,
      userId: session.userId,
      userEmail,
      providerAccountId: account.providerAccountId,
      accessToken: account.accessToken,
      expiresAt: account.expiresAt ? account.expiresAt.toISOString() : null,
      scopes: splitScopes(account.scope),
      tokenRef: account.tokenRef,
    },
  };
}

/**
 * Push the signed-in user's Microsoft token to integration-core. Never throws.
 */
export async function pushMicrosoftSignInHandoff(
  session: SessionLike,
  deps: HandoffDeps = {},
): Promise<SignInHandoffOutcome> {
  const env = deps.env ?? process.env;
  const log = deps.log ?? console;
  const internalApiKey = env.INTERNAL_API_KEY?.trim();
  if (!internalApiKey) return 'skipped:not-configured';

  try {
    // A connection is org-scoped: without an active organization there is
    // nothing to adopt into, so do not touch the database at all.
    const preflight = buildSignInHandoffPayload(session, null, undefined);
    if ('skip' in preflight && preflight.skip === 'skipped:no-organization') {
      return preflight.skip;
    }
    const account = await (deps.loadAccount ?? loadMicrosoftAccount)(
      session.userId,
    );
    const email = account
      ? await (deps.loadUserEmail ?? loadUserEmail)(session.userId)
      : undefined;
    const built = buildSignInHandoffPayload(
      session,
      account,
      email,
      (deps.now ?? (() => new Date()))(),
    );
    if ('skip' in built) return built.skip;

    const baseUrl = (
      env.INTEGRATION_CORE_URL?.trim() || DEFAULT_INTEGRATION_CORE_URL
    ).replace(/\/+$/, '');
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), HANDOFF_TIMEOUT_MS);
    try {
      const response = await (deps.fetch ?? fetch)(
        `${baseUrl}/internal/providers/${MICROSOFT_PROVIDER_ID}/sign-in-handoff`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Accept: 'application/json',
            'X-Internal-API-Key': internalApiKey,
          },
          body: JSON.stringify(built.payload),
          signal: controller.signal,
        },
      );
      if (!response.ok) {
        log.warn(
          `⚠️ [Auth] Microsoft sign-in hand-off rejected by integration-core (HTTP ${response.status}) for user ${session.userId}`,
        );
        return 'failed';
      }
      log.log(
        `🔗 [Auth] Microsoft sign-in token handed to integration-core for user ${session.userId} (org ${built.payload.organizationId})`,
      );
      return 'sent';
    } finally {
      clearTimeout(timer);
    }
  } catch (error) {
    // Token values never reach the log; only the failure class does.
    log.warn(
      `⚠️ [Auth] Microsoft sign-in hand-off failed for user ${session.userId}: ${error instanceof Error ? error.message : String(error)}`,
    );
    return 'failed';
  }
}

function coerceDate(value: unknown): Date | null {
  if (value instanceof Date)
    return Number.isNaN(value.getTime()) ? null : value;
  if (typeof value === 'string' && value.trim()) {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed;
  }
  return null;
}
