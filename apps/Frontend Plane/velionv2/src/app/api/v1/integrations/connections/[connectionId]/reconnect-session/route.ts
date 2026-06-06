import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  readJsonOrNull,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { loadOwnedIntegrationConnection } from "@/app/api/v1/integrations/_lib/connection-ownership";
import { buildIntegrationCoreHeaders } from "@/app/api/v1/integrations/_lib/service-auth";
import { fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ connectionId: string }> },
) {
  try {
    const [session, { connectionId }] = await Promise.all([requireSession(request), params]);
    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json(
        fail({ code: "org_required", message: "No active organization found." }),
        { status: 409 },
      );
    }

    const headers = buildIntegrationCoreHeaders(request, session, orgId);
    const ownership = await loadOwnedIntegrationConnection(request, session, orgId, connectionId);
    if (ownership.response) return ownership.response;

    const response = await fetch(
      `${getIntegrationCoreUrl()}/api/v1/providers/${encodeURIComponent(ownership.connection.providerKey)}/reconnect-session`,
      {
        method: "POST",
        headers: {
          ...headers,
          "Content-Type": "application/json",
        },
        cache: "no-store",
        body: JSON.stringify({
          organizationId: orgId,
          workspaceId: orgId,
          userId: session.user.id,
          userEmail: session.user.email ?? undefined,
          providerContext: { reconnectConnectionId: connectionId },
        }),
      },
    );
    const payload = await readJsonOrNull(response);
    const sessionPayload = unwrapConnectSessionPayload(payload);

    if (!response.ok || !sessionPayload?.connectUrl) {
      return NextResponse.json(
        fail({
          code: "reconnect_session_failed",
          message: serviceErrorMessage(payload) ?? "Could not start provider reconnect.",
        }),
        { status: response.ok ? 502 : response.status },
      );
    }

    return NextResponse.json(ok(sessionPayload));
  } catch (error) {
    return authErrorResponse(error);
  }
}

function unwrapConnectSessionPayload(payload: unknown) {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  const connectUrl = typeof data.connectUrl === "string" ? data.connectUrl : null;
  if (!connectUrl) return null;
  return {
    connectUrl,
    authMode: typeof data.authMode === "string" ? data.authMode : undefined,
    sessionToken: typeof data.sessionToken === "string" ? data.sessionToken : undefined,
    expiresAt: typeof data.expiresAt === "string" ? data.expiresAt : undefined,
    providerConfigKey: typeof data.providerConfigKey === "string" ? data.providerConfigKey : undefined,
  };
}

function serviceErrorMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as { error?: unknown; message?: unknown };
  if (typeof record.message === "string") return record.message;
  if (typeof record.error === "string") return record.error;
  if (record.error && typeof record.error === "object" && "message" in record.error) {
    const message = (record.error as { message?: unknown }).message;
    return typeof message === "string" ? message : null;
  }
  return null;
}
