/**
 * SSO enforcement (WS05-S7).
 *
 * An organisation can put its SSO policy in `off`, `report` or `enforce`
 * (org-core owns the policy and the proof of the e-mail domain). This module is
 * auth-core's half: the decision made when a session is about to be created,
 * and the evidence that a user really signed in through an identity provider.
 *
 *   - A session created by an SSO callback path always proceeds. Enforcement
 *     only ever blocks the non-SSO paths (e-mail and password, social, passkey,
 *     impersonation).
 *   - The user's e-mail domain selects the policy: org-core answers which
 *     organisation holds the verified claim on that domain and what its mode is.
 *   - `report` records `sso_enforcement_would_block` and lets the sign-in
 *     through; `enforce` refuses it with `SSO_REQUIRED`, except for the
 *     organisation's break-glass owners, whose password sign-in is allowed and
 *     audited as `sso_break_glass_signin`.
 *   - `SSO_ENFORCEMENT` (off|report|enforce, default report) is the platform
 *     ceiling and kill switch: an organisation's mode is never applied above it.
 *   - org-core unreachable: SSO paths proceed; a non-SSO path fails closed with
 *     503 `SSO_POLICY_UNAVAILABLE` only while the ceiling is `enforce`. Below
 *     that nothing could be blocked anyway, so the lookup failure is logged and
 *     the sign-in proceeds.
 *
 * No e-mail address, token or profile attribute is logged or stored here.
 */
import { randomUUID } from 'node:crypto';
import { APIError } from 'better-auth/api';
import { and, eq } from 'drizzle-orm';

import { db } from '../db';
import * as schema from '../db/schema';
import { publishVerevonAuditDurable } from './audit-plugin';
import { requiredScopedServiceToken } from './control-service-credentials';

export type SsoMode = 'off' | 'report' | 'enforce';

const MODE_ORDER: Record<SsoMode, number> = { off: 0, report: 1, enforce: 2 };

const POLICY_CACHE_TTL_MS = 60_000;
const POLICY_CACHE_MAX = 5_000;
const POLICY_LOOKUP_TIMEOUT_MS = 2_000;
const SERVICE_ID = 'auth-core';

const SSO_PATHS: readonly RegExp[] = [
  /^\/sso\/callback(\/[^/]+)?\/?$/,
  /^\/sso\/saml2\/callback\/[^/]+\/?$/,
  /^\/sso\/saml2\/sp\/acs\/[^/]+\/?$/,
];

/** The platform ceiling. An unknown value refuses to boot rather than guessing. */
export function resolveSsoEnforcementCeiling(
  env: NodeJS.ProcessEnv = process.env,
): SsoMode {
  const raw = env.SSO_ENFORCEMENT?.trim().toLowerCase();
  if (!raw) return 'report';
  if (raw === 'off' || raw === 'report' || raw === 'enforce') return raw;
  throw new Error('SSO_ENFORCEMENT must be one of off, report, enforce');
}

export function lowerMode(a: SsoMode, b: SsoMode): SsoMode {
  return MODE_ORDER[a] <= MODE_ORDER[b] ? a : b;
}

/** True for the Better Auth SSO callback endpoints; every other path is non-SSO. */
export function isSsoSignInPath(path: string | undefined | null): boolean {
  if (typeof path !== 'string') return false;
  const clean = path.split('?')[0];
  return SSO_PATHS.some((pattern) => pattern.test(clean));
}

/** The provider id in an SSO callback path, or undefined for `/sso/callback`. */
export function ssoProviderIdFromPath(
  path: string | undefined | null,
): string | undefined {
  if (!isSsoSignInPath(path)) return undefined;
  const parts = (path as string).split('?')[0].split('/').filter(Boolean);
  const last = parts[parts.length - 1];
  return last && last !== 'callback' ? decodeURIComponent(last) : undefined;
}

export function emailDomain(email: string | undefined | null): string | null {
  if (typeof email !== 'string') return null;
  const at = email.lastIndexOf('@');
  if (at < 1 || at === email.length - 1) return null;
  return email
    .slice(at + 1)
    .trim()
    .toLowerCase();
}

export interface SsoPolicyForDomain {
  orgId: string;
  mode: SsoMode;
  breakGlassUserIds: readonly string[];
}

export class SsoPolicyUnavailableError extends Error {
  constructor(readonly reason: string) {
    super(reason);
  }
}

type CacheEntry = { value: SsoPolicyForDomain | null; expiresAt: number };
const policyCache = new Map<string, CacheEntry>();

export function clearSsoPolicyCache(): void {
  policyCache.clear();
}

function orgCoreBaseUrl(): string {
  return (
    process.env.ORG_SERVICE_URL ||
    process.env.ORG_CORE_URL ||
    'http://org-core:8080'
  ).replace(/\/$/, '');
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function parseMode(value: unknown): SsoMode {
  return value === 'enforce' || value === 'report' ? value : 'off';
}

/**
 * org-core's answer for an e-mail domain: the policy of the organisation that
 * holds the verified claim, or null when no organisation does (nothing to
 * enforce). Successful answers, including "none", are cached for 60 seconds;
 * failures are never cached.
 */
export async function lookupSsoPolicyForDomain(
  domain: string,
  now: number = Date.now(),
): Promise<SsoPolicyForDomain | null> {
  const cached = policyCache.get(domain);
  if (cached && cached.expiresAt > now) return cached.value;

  let token: string;
  try {
    token = requiredScopedServiceToken('ORG_CORE_SERVICE_TOKEN');
  } catch {
    throw new SsoPolicyUnavailableError('org_core_credential_missing');
  }
  let response: Response;
  try {
    response = await fetch(
      `${orgCoreBaseUrl()}/internal/orgs/by-domain/${encodeURIComponent(domain)}/sso-policy`,
      {
        method: 'GET',
        headers: {
          accept: 'application/json',
          'x-service-id': SERVICE_ID,
          'x-service-token': token,
        },
        signal: AbortSignal.timeout(POLICY_LOOKUP_TIMEOUT_MS),
      },
    );
  } catch {
    throw new SsoPolicyUnavailableError('org_core_unreachable');
  }

  let value: SsoPolicyForDomain | null;
  if (response.status === 404) {
    // No verified owner for the domain, or domain verification is switched off
    // in org-core: either way there is no policy to apply.
    value = null;
  } else if (response.ok) {
    let body: unknown;
    try {
      body = await response.json();
    } catch {
      throw new SsoPolicyUnavailableError('org_core_malformed_response');
    }
    const data = isRecord(body) ? body.data : undefined;
    if (!isRecord(data) || typeof data.org_id !== 'string' || !data.org_id) {
      throw new SsoPolicyUnavailableError('org_core_malformed_response');
    }
    value = {
      orgId: data.org_id,
      mode: parseMode(data.mode),
      breakGlassUserIds: Array.isArray(data.break_glass_user_ids)
        ? data.break_glass_user_ids.filter(
            (id): id is string => typeof id === 'string',
          )
        : [],
    };
  } else {
    throw new SsoPolicyUnavailableError(`org_core_status_${response.status}`);
  }

  if (policyCache.size >= POLICY_CACHE_MAX) policyCache.clear();
  policyCache.set(domain, { value, expiresAt: now + POLICY_CACHE_TTL_MS });
  return value;
}

export type SsoDecision =
  | { action: 'allow' }
  | { action: 'would_block'; orgId: string; domain: string }
  | { action: 'break_glass'; orgId: string; domain: string }
  | { action: 'block'; orgId: string; domain: string }
  | { action: 'unavailable' };

/**
 * The pure decision for one session about to be created. `lookup` is injected
 * so the rules are testable without org-core.
 */
export async function decideSsoSignIn(
  input: { userId: string; email: string | null; path: string | undefined },
  ceiling: SsoMode,
  lookup: (domain: string) => Promise<SsoPolicyForDomain | null> = (domain) =>
    lookupSsoPolicyForDomain(domain),
): Promise<SsoDecision> {
  if (ceiling === 'off') return { action: 'allow' };
  if (isSsoSignInPath(input.path)) return { action: 'allow' };
  const domain = emailDomain(input.email);
  if (!domain) return { action: 'allow' };

  let policy: SsoPolicyForDomain | null;
  try {
    policy = await lookup(domain);
  } catch {
    return ceiling === 'enforce' ? { action: 'unavailable' } : { action: 'allow' };
  }
  if (!policy || policy.mode === 'off') return { action: 'allow' };

  const effective = lowerMode(policy.mode, ceiling);
  const exempt = policy.breakGlassUserIds.includes(input.userId);
  if (effective === 'enforce') {
    return exempt
      ? { action: 'break_glass', orgId: policy.orgId, domain }
      : { action: 'block', orgId: policy.orgId, domain };
  }
  if (effective === 'report' && !exempt) {
    return { action: 'would_block', orgId: policy.orgId, domain };
  }
  return { action: 'allow' };
}

async function emitSsoAudit(
  event: 'sso_enforcement_would_block' | 'sso_break_glass_signin',
  orgId: string,
  userId: string,
  path: string | undefined,
): Promise<void> {
  try {
    await publishVerevonAuditDurable({
      occurred_at: new Date().toISOString(),
      event_id: `sso:${event}:${randomUUID()}`,
      org_id: orgId,
      user_id: userId,
      plane: 'control',
      event,
      subject: `user:${userId}`,
      resource_id: userId,
      outcome: 'ok',
      // Ids and a closed path class only: no e-mail address, no domain.
      details: { path_kind: pathKind(path) },
    });
  } catch {
    // The sign-in decision never depends on the audit transport.
    console.warn(`SSO audit event ${event} could not be published`);
  }
}

function pathKind(path: string | undefined): string {
  if (!path) return 'unknown';
  if (path.startsWith('/sign-in/email')) return 'email';
  if (path.startsWith('/sign-in/social') || path.startsWith('/callback/'))
    return 'social';
  if (path.startsWith('/passkey')) return 'passkey';
  return 'other';
}

function readPath(context: unknown): string | undefined {
  return isRecord(context) && typeof context.path === 'string'
    ? context.path
    : undefined;
}

/**
 * The session-create `before` hook body. Throws a Better Auth APIError to
 * refuse the sign-in (`SSO_REQUIRED`, or `SSO_POLICY_UNAVAILABLE` when the
 * ceiling is enforce and org-core cannot be read); otherwise returns.
 */
export async function enforceSsoOnSessionCreate(
  session: { userId: string },
  context: unknown,
): Promise<void> {
  const ceiling = resolveSsoEnforcementCeiling();
  if (ceiling === 'off') return;
  const path = readPath(context);
  if (isSsoSignInPath(path)) return;

  const [row] = await db
    .select({ email: schema.user.email })
    .from(schema.user)
    .where(eq(schema.user.id, session.userId))
    .limit(1);
  const decision = await decideSsoSignIn(
    { userId: session.userId, email: row?.email ?? null, path },
    ceiling,
  );
  switch (decision.action) {
    case 'allow':
      return;
    case 'would_block':
      await emitSsoAudit(
        'sso_enforcement_would_block',
        decision.orgId,
        session.userId,
        path,
      );
      return;
    case 'break_glass':
      await emitSsoAudit(
        'sso_break_glass_signin',
        decision.orgId,
        session.userId,
        path,
      );
      return;
    case 'unavailable':
      throw new APIError('SERVICE_UNAVAILABLE', {
        code: 'SSO_POLICY_UNAVAILABLE',
        message: 'Sign-in policy could not be verified. Retry shortly.',
      });
    case 'block':
      throw new APIError('FORBIDDEN', {
        code: 'SSO_REQUIRED',
        message: 'Your organization requires single sign-on.',
        domain: decision.domain,
      });
  }
}

/**
 * The session-create `after` hook body: when the session came from an SSO
 * callback, remember that this user signed in through this organisation's
 * provider. Best effort, never fails the sign-in.
 */
export async function recordSsoSignInEvidence(
  session: { userId: string },
  context: unknown,
): Promise<void> {
  try {
    const providerId = ssoProviderIdFromPath(readPath(context));
    if (!providerId) return;
    const [provider] = await db
      .select({ organizationId: schema.ssoProvider.organizationId })
      .from(schema.ssoProvider)
      .where(eq(schema.ssoProvider.providerId, providerId))
      .limit(1);
    if (!provider?.organizationId) return;
    await db.insert(schema.ssoSigninEvidence).values({
      userId: session.userId,
      organizationId: provider.organizationId,
      providerId,
    });
  } catch {
    console.warn('SSO sign-in evidence could not be recorded');
  }
}

/** Exposed for the internal controller: the owner or admin of an organisation. */
export async function isOrganizationAdmin(
  organizationId: string,
  userId: string,
): Promise<boolean> {
  const [row] = await db
    .select({ role: schema.member.role })
    .from(schema.member)
    .where(
      and(
        eq(schema.member.organizationId, organizationId),
        eq(schema.member.userId, userId),
      ),
    )
    .limit(1);
  const role = row?.role.toLowerCase();
  return role === 'owner' || role === 'admin';
}
