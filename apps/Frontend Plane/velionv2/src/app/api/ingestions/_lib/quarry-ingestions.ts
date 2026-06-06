import "server-only";

import type { NextRequest } from "next/server";
import { ControlPlaneAuthError, requireSession } from "@/app/api/_lib/control-plane-auth";
import {
  getQuarryAudience,
  getQuarryEdgeUrl,
  mintAudienceToken,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

type QuarryEnvelope<T> = {
  data?: T;
  error?: {
    code?: string;
    message?: string;
  };
};

export type QuarryContext = {
  bearerToken: string;
  orgId: string | null;
};

function trimRightSlash(value: string) {
  return value.replace(/\/+$/, "");
}

function withQuery(path: string, query?: Record<string, string | number | undefined | null>) {
  const url = new URL(`${trimRightSlash(getQuarryEdgeUrl())}${path}`);
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value === undefined || value === null || value === "") continue;
    url.searchParams.set(key, String(value));
  }
  return url.toString();
}

export async function getQuarryContext(request: NextRequest): Promise<QuarryContext> {
  const session = await requireSession(request);
  const forwarded = request.headers.get("authorization")?.trim();
  const minted = forwarded ? null : await mintAudienceToken(request, getQuarryAudience());
  const bearerToken = forwarded?.startsWith("Bearer ") ? forwarded.slice("Bearer ".length) : minted;

  if (!bearerToken) {
    throw new ControlPlaneAuthError(
      503,
      "quarry_auth_unavailable",
      "Could not mint a Quarry audience token for this request.",
    );
  }

  const orgId = await resolveActiveOrgId(request, session);
  return { bearerToken, orgId };
}

export async function fetchQuarry<T>(
  request: NextRequest,
  path: string,
  init?: {
    method?: "GET" | "POST" | "DELETE";
    body?: unknown;
    query?: Record<string, string | number | undefined | null>;
    timeoutMs?: number;
  },
): Promise<T> {
  const context = await getQuarryContext(request);
  let response: Response;
  try {
    response = await fetch(withQuery(path, init?.query), {
      method: init?.method ?? "GET",
      headers: {
        Authorization: `Bearer ${context.bearerToken}`,
        ...(init?.body !== undefined ? { "Content-Type": "application/json" } : {}),
      },
      body: init?.body !== undefined ? JSON.stringify(init.body) : undefined,
      cache: "no-store",
      signal: init?.timeoutMs ? AbortSignal.timeout(init.timeoutMs) : undefined,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ControlPlaneAuthError(
        504,
        "quarry_request_timeout",
        "Quarry did not respond before the request timed out.",
      );
    }
    throw error;
  }

  const payload = (await response.json().catch(() => null)) as QuarryEnvelope<T> | null;
  if (!response.ok || !payload?.data) {
    throw new ControlPlaneAuthError(
      response.status || 502,
      payload?.error?.code || "quarry_request_failed",
      payload?.error?.message || `Quarry request failed: ${response.status}`,
    );
  }

  return payload.data;
}

export type OffsetCursorInput<T> = {
  cursor: string | null;
  data: T[];
  limit: number;
};

export function paginateByOffset<T>({ cursor, data, limit }: OffsetCursorInput<T>) {
  const offset = Number.parseInt(cursor ?? "0", 10);
  const safeOffset = Number.isFinite(offset) && offset > 0 ? offset : 0;
  const page = data.slice(safeOffset, safeOffset + limit);
  const nextCursor = safeOffset + limit < data.length ? String(safeOffset + limit) : null;
  return { page, nextCursor };
}
