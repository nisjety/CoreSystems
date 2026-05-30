export type ApiEnvelope<T> = { data: T } | { error: { code: string; message: string } };

type ApiRequestOptions = {
  cache?: RequestCache;
  credentials?: RequestCredentials;
  signal?: AbortSignal;
};

export async function apiGet<T>(url: string, options: ApiRequestOptions = {}): Promise<T> {
  const response = await fetch(url, {
    cache: options.cache ?? "no-store",
    credentials: options.credentials,
    signal: options.signal,
  });

  return readApiEnvelope<T>(response);
}

export async function apiSend<T>(
  url: string,
  body: unknown,
  method = "POST",
  options: ApiRequestOptions = {},
): Promise<T> {
  const response = await fetch(url, {
    method,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    cache: options.cache,
    credentials: options.credentials,
    signal: options.signal,
  });

  return readApiEnvelope<T>(response);
}

export function isLocalIntegrationUnavailable(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);

  return message.includes("DATABASE_URL") || message.includes("signed-in user") || message.includes("authentication service");
}

async function readApiEnvelope<T>(response: Response): Promise<T> {
  const payload = (await response.json().catch(() => null)) as ApiEnvelope<T> | null;

  if (!response.ok || !payload || "error" in payload) {
    throw new Error(payload && "error" in payload ? payload.error.message : `Request failed: ${response.status}`);
  }

  return payload.data;
}
