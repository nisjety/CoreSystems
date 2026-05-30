import 'server-only';

import { NextRequest } from 'next/server';

import { getAuthServiceUrl, getInternalApiKey } from './config';

const ALLOWED_FORWARD_HEADERS = new Set([
  'accept-language',
  'cookie',
  'user-agent',
]);
const AUTH_SERVICE_TIMEOUT_MS = 10_000;

export interface SessionUser {
  id: string;
  email?: string;
  name?: string;
}

interface SessionPayload {
  user?: SessionUser | null;
}

function buildForwardHeaders(request: NextRequest): Headers {
  const headers = new Headers();

  request.headers.forEach((value, key) => {
    if (ALLOWED_FORWARD_HEADERS.has(key.toLowerCase())) {
      headers.set(key, value);
    }
  });

  headers.set('content-type', 'application/json');
  headers.set('x-internal-api-key', getInternalApiKey());

  return headers;
}

function isSessionPayload(value: unknown): value is SessionPayload {
  if (!value || typeof value !== 'object') {
    return false;
  }

  if (!('user' in value)) {
    return true;
  }

  const user = (value as SessionPayload).user;

  if (user == null) {
    return true;
  }

  if (typeof user !== 'object' || typeof user.id !== 'string') {
    return false;
  }

  if ('email' in user && user.email !== undefined && typeof user.email !== 'string') {
    return false;
  }

  if ('name' in user && user.name !== undefined && typeof user.name !== 'string') {
    return false;
  }

  return true;
}

export async function getSessionUser(
  request: NextRequest,
): Promise<SessionUser | null> {
  try {
    const response = await fetch(
      `${getAuthServiceUrl()}/api/v2/auth/getSession`,
      {
        method: 'POST',
        headers: buildForwardHeaders(request),
        body: JSON.stringify({}),
        cache: 'no-store',
        signal: AbortSignal.timeout(AUTH_SERVICE_TIMEOUT_MS),
      },
    );

    if (!response.ok) {
      return null;
    }

    const payload = (await response.json()) as unknown;

    if (!isSessionPayload(payload)) {
      return null;
    }

    return payload.user ?? null;
  } catch (error) {
    console.error('[avelis] session lookup failed', error);
    return null;
  }
}