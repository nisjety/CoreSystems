const RECONCILIATION_TIMEOUT_MS = 5_000;

export async function fetchReconciliation(
  input: string | URL | Request,
  init?: RequestInit,
): Promise<Response> {
  const controller = new AbortController();
  const timeout = setTimeout(
    () => controller.abort(new DOMException('request timed out', 'AbortError')),
    RECONCILIATION_TIMEOUT_MS,
  );

  try {
    return await fetch(input, {
      ...init,
      redirect: 'manual',
      signal: controller.signal,
    });
  } finally {
    clearTimeout(timeout);
  }
}
