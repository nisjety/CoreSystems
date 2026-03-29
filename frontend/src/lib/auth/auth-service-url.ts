import 'server-only';

function normalizeBaseUrl(value: string | undefined) {
  const trimmed = value?.trim();

  if (!trimmed) {
    return null;
  }

  return trimmed.replace(/\/+$/, '');
}

export function getAuthServiceBaseUrls() {
  const configuredUrls = [
    process.env.AUTH_SERVICE_URL,
    process.env.BACKEND_URL,
    process.env.API_AUTH_URL,
  ]
    .map(normalizeBaseUrl)
    .filter((value): value is string => Boolean(value));

  const devFallbackUrls =
    process.env.NODE_ENV !== 'production'
      ? [
          'http://127.0.0.1:3011',
          'http://localhost:3011',
          'http://host.docker.internal:3011',
        ]
      : [];

  return [...new Set([...configuredUrls, ...devFallbackUrls])];
}

export async function fetchFromAuthService(
  path: string,
  init: RequestInit & {
    retryOn5xx?: boolean;
    timeoutMs?: number;
  } = {}
) {
  const { retryOn5xx = false, timeoutMs = 2500, ...requestInit } = init;
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  const baseUrls = getAuthServiceBaseUrls();

  let lastResponse: { response: Response; targetUrl: string } | null = null;
  let lastError: unknown = null;

  for (const baseUrl of baseUrls) {
    const targetUrl = `${baseUrl}${normalizedPath}`;

    try {
      const response = await fetch(targetUrl, {
        ...requestInit,
        signal: requestInit.signal ?? AbortSignal.timeout(timeoutMs),
      });

      if (retryOn5xx && response.status >= 500) {
        lastResponse = { response, targetUrl };
        continue;
      }

      return { response, targetUrl };
    } catch (error) {
      lastError = error;
    }
  }

  if (lastResponse) {
    return lastResponse;
  }

  throw new Error(
    `Unable to reach auth service using: ${baseUrls.join(', ') || 'no configured URLs'}`,
    lastError ? { cause: lastError } : undefined,
  );
}