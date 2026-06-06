import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
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

type SyncJobRequest = {
  connectionId?: unknown;
  reason?: unknown;
  mode?: unknown;
  checkpoint?: unknown;
  metadata?: unknown;
};

export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json(
        fail({ code: "org_required", message: "No active organization found." }),
        { status: 409 },
      );
    }

    const body = (await request.json().catch(() => null)) as SyncJobRequest | null;
    const connectionId = typeof body?.connectionId === "string" ? body.connectionId.trim() : "";
    if (!connectionId) {
      return NextResponse.json(
        fail({ code: "connection_required", message: "connectionId is required." }),
        { status: 400 },
      );
    }

    const ownership = await loadOwnedIntegrationConnection(request, session, orgId, connectionId);
    if (ownership.response) return ownership.response;

    const headers = {
      ...buildIntegrationCoreHeaders(request, session, orgId),
      "Content-Type": "application/json",
    };
    const response = await fetch(`${getIntegrationCoreUrl()}/api/v1/sync-jobs`, {
      method: "POST",
      headers,
      cache: "no-store",
      body: JSON.stringify({
        connectionId,
        reason: typeof body?.reason === "string" ? body.reason : "user_requested",
        mode: typeof body?.mode === "string" ? body.mode : "incremental",
        checkpoint: safeMetadata(body?.checkpoint),
        metadata: safeMetadata(body?.metadata),
      }),
    });
    const payload = await response.json().catch(() => null);

    if (!response.ok) {
      return NextResponse.json(
        fail({
          code: "sync_job_failed",
          message: serviceErrorMessage(payload) ?? "Could not start source sync.",
        }),
        { status: response.status },
      );
    }

    return NextResponse.json(ok(unwrapSyncJobPayload(payload)), { status: 202 });
  } catch (error) {
    return authErrorResponse(error);
  }
}

function safeMetadata(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([key, entryValue]) => key.length <= 80 && isSafeMetadataValue(entryValue))
    .slice(0, 20);
  return Object.fromEntries(entries);
}

function isSafeMetadataValue(value: unknown): boolean {
  if (value == null) return true;
  if (["string", "number", "boolean"].includes(typeof value)) return true;
  if (Array.isArray(value)) {
    return value.length <= 12 && value.every((item) => ["string", "number", "boolean"].includes(typeof item));
  }
  return false;
}

function serviceErrorMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as {
    error?: { message?: unknown } | string;
    message?: unknown;
  };
  if (typeof record.message === "string") return record.message;
  if (typeof record.error === "string") return record.error;
  if (record.error && typeof record.error === "object" && typeof record.error.message === "string") {
    return record.error.message;
  }
  return null;
}

function unwrapSyncJobPayload(payload: unknown) {
  if (!payload || typeof payload !== "object") return { syncJob: null };
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  return {
    syncJob: data.syncJob ?? null,
  };
}
