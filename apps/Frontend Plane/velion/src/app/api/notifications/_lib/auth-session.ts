import { NextRequest } from 'next/server';

const DEFAULT_AUTH_SERVICE_URL = 'http://auth-service:3011';

interface SessionUser {
  id: string;
  email?: string;
  name?: string;
}

interface AuthSessionResponse {
  user?: SessionUser;
}

function getAuthServiceUrl(): string {
  return process.env.AUTH_SERVICE_URL ?? DEFAULT_AUTH_SERVICE_URL;
}

function getInternalApiKey(): string {
  const key = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET;
  if (!key) {
    throw new Error('INTERNAL_API_KEY (or INTERNAL_SERVICE_SECRET) must be configured');
  }
  return key;
}

export async function getSessionUser(request: NextRequest): Promise<SessionUser | null> {
  const forwardHeaders = new Headers();
  const allowedHeaders = ['cookie', 'user-agent', 'accept-language'];

  request.headers.forEach((value, key) => {
    if (allowedHeaders.includes(key.toLowerCase())) {
      forwardHeaders.set(key, value);
    }
  });

  forwardHeaders.set('x-internal-api-key', getInternalApiKey());
  forwardHeaders.set('Content-Type', 'application/json');

  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), 10_000);

  let response: Response;
  try {
    // G30 v3: use Better Auth's native /api/auth/get-session (GET).
    response = await fetch(`${getAuthServiceUrl()}/api/auth/get-session`, {
      method: 'GET',
      headers: forwardHeaders,
      credentials: 'include',
      cache: 'no-store',
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timeoutId);
  }

  if (!response.ok) {
    return null;
  }

  const session = (await response.json()) as AuthSessionResponse;
  return session.user ?? null;
}
