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

export async function DELETE(
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

    const ownership = await loadOwnedIntegrationConnection(request, session, orgId, connectionId);
    if (ownership.response) return ownership.response;

    const response = await fetch(
      `${getIntegrationCoreUrl()}/api/v1/connections/${encodeURIComponent(connectionId)}`,
      {
        method: "DELETE",
        headers: buildIntegrationCoreHeaders(request, session, orgId),
        cache: "no-store",
      },
    );
    const payload = await readJsonOrNull(response);

    if (!response.ok && response.status !== 404 && response.status !== 410) {
      return NextResponse.json(
        fail({
          code: "disconnect_failed",
          message: serviceErrorMessage(payload) ?? "Could not disconnect source.",
        }),
        { status: response.status },
      );
    }

    return NextResponse.json(ok({ disconnected: response.ok, connectionId }));
  } catch (error) {
    return authErrorResponse(error);
  }
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
