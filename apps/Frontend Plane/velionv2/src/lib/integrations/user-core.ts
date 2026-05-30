import type { RequestActor } from "@/lib/integrations/request-actor";

export class UserCoreError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

function getUserCoreUrl() {
  return (process.env.USER_CORE_URL ?? process.env.USER_SERVICE_URL ?? "http://localhost:3012").replace(/\/+$/, "");
}

function getInternalApiKey() {
  return process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET;
}

function buildHeaders(actor: RequestActor, contentType = true) {
  const internalApiKey = getInternalApiKey();

  if (!internalApiKey) {
    throw new UserCoreError(
      503,
      "user_core_key_not_configured",
      "INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET is required for user-core writes.",
    );
  }

  const headers = new Headers({
    "X-Internal-Api-Key": internalApiKey,
    "X-User-Id": actor.userId,
  });

  if (contentType) {
    headers.set("Content-Type", "application/json");
  }
  if (actor.email) {
    headers.set("X-User-Email", actor.email);
  }
  if (actor.name) {
    headers.set("X-User-Name", actor.name);
  }
  if (actor.avatar) {
    headers.set("X-User-Avatar", actor.avatar);
  }
  if (actor.cookieHeader) {
    headers.set("Cookie", actor.cookieHeader);
  }

  return headers;
}

export async function fetchUserCoreJson<T>(
  actor: RequestActor,
  path: string,
  init?: Omit<RequestInit, "headers"> & { headers?: HeadersInit },
): Promise<T> {
  const response = await fetch(`${getUserCoreUrl()}${path}`, {
    ...init,
    headers: buildHeaders(actor, init?.body !== undefined),
    cache: "no-store",
    signal: AbortSignal.timeout(5000),
  }).catch((error: unknown) => {
    throw new UserCoreError(
      502,
      "user_core_unreachable",
      error instanceof Error ? error.message : "user-core request failed",
    );
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const message =
      body && typeof body === "object" && "error" in body
        ? String(body.error)
        : `user-core returned ${response.status}`;
    throw new UserCoreError(response.status, "user_core_error", message);
  }

  return response.json() as Promise<T>;
}
