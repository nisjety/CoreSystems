import "server-only";

import type { NextRequest } from "next/server";

import {
  getCorrelationId,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";

function getIntegrationInternalApiKey() {
  const key =
    process.env.INTEGRATION_INTERNAL_API_KEY ||
    process.env.INTERNAL_API_KEY ||
    process.env.INTERNAL_SERVICE_SECRET;
  if (!key?.trim()) {
    throw new Error("INTEGRATION_INTERNAL_API_KEY or INTERNAL_API_KEY must be set");
  }
  return key.trim();
}

export function buildIntegrationCoreHeaders(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId?: string,
): Record<string, string> {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    "X-Internal-Api-Key": getIntegrationInternalApiKey(),
    "X-User-Id": session.user.id,
    "X-Correlation-Id": getCorrelationId(request),
  };

  if (orgId) headers["X-Org-ID"] = orgId;
  if (session.user.email) headers["X-User-Email"] = session.user.email;
  if (session.user.name) headers["X-User-Name"] = session.user.name;
  if (session.user.image || session.user.avatar) {
    headers["X-User-Avatar"] = session.user.image || session.user.avatar || "";
  }

  const authorization = request.headers.get("authorization");
  if (authorization) headers.Authorization = authorization;

  return headers;
}
