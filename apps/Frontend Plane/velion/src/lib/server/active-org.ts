import { cache } from 'react';
import { getServerSession } from '@/components/auth/lib/auth-server';

const USER_SERVICE_URL = process.env.USER_SERVICE_URL ?? 'http://user-core:3012';
const ORG_SERVICE_URL = process.env.ORG_SERVICE_URL ?? 'http://org-core:8080';
const INTERNAL_API_KEY =
  (process.env.INTERNAL_API_KEY ??
  process.env.INTERNAL_SERVICE_SECRET) as string;
if (!INTERNAL_API_KEY) throw new Error('INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET must be set');

type SessionContextResponse = {
  userId?: string;
  orgId?: string;
  role?: string;
  onboardingStatus?: string;
};

type OrganizationSummary = {
  id: string;
  name?: string;
  slug?: string;
};

export type ActiveOrgContext = {
  userId: string;
  userEmail: string;
  userName: string;
  orgId: string | null;
  role: string | null;
};

function buildInternalHeaders(actor: {
  userId: string;
  userEmail: string;
  userName: string;
}) {
  return {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': INTERNAL_API_KEY,
    'X-User-Id': actor.userId,
    'X-User-Email': actor.userEmail,
    'X-User-Name': actor.userName,
  };
}

async function readJson<T>(response: Response): Promise<T | null> {
  if (!response.ok) {
    return null;
  }

  const text = await response.text();
  if (!text.trim()) {
    return null;
  }

  return JSON.parse(text) as T;
}

async function getSessionContext(actor: {
  userId: string;
  userEmail: string;
  userName: string;
}) {
  const response = await fetch(`${USER_SERVICE_URL}/api/v1/me/session-context`, {
    method: 'GET',
    headers: buildInternalHeaders(actor),
    signal: AbortSignal.timeout(3_000),
    cache: 'no-store',
  });

  return readJson<SessionContextResponse>(response);
}

async function getOrganizationsForUser(actor: {
  userId: string;
  userEmail: string;
  userName: string;
}) {
  const response = await fetch(`${ORG_SERVICE_URL}/orgs/me`, {
    method: 'GET',
    headers: buildInternalHeaders(actor),
    signal: AbortSignal.timeout(3_000),
    cache: 'no-store',
  });

  const payload = await readJson<OrganizationSummary[] | { organizations?: OrganizationSummary[] }>(response);
  if (Array.isArray(payload)) {
    return payload;
  }

  return Array.isArray(payload?.organizations) ? payload.organizations : [];
}

/**
 * Resolves the active org context for the current request.
 *
 * Wrapped in React.cache() so multiple RSC/route invocations within the same
 * request share the result without duplicate network calls.
 *
 * Fires session-context and org-list in PARALLEL to eliminate the waterfall
 * that previously added ~3-6s to every server-rendered page.
 */
export const resolveActiveOrgContext = cache(async (): Promise<ActiveOrgContext | null> => {
  const session = await getServerSession();
  if (!session?.user?.id || !session.user.email) {
    return null;
  }

  const actor = {
    userId: session.user.id,
    userEmail: session.user.email,
    userName: session.user.name || session.user.email.split('@')[0] || 'User',
  };

  // Fire both requests in parallel — org list is needed as fallback anyway
  const [sessionContext, organizations] = await Promise.all([
    getSessionContext(actor).catch(() => null),
    getOrganizationsForUser(actor).catch(() => []),
  ]);

  const orgId = sessionContext?.orgId ?? organizations[0]?.id ?? null;

  return {
    ...actor,
    orgId,
    role: sessionContext?.role ?? null,
  };
})
