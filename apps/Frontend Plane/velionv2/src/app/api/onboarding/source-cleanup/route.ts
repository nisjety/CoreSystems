import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  getInternalApiKey,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getDataPlaneDocumentsUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const dynamic = "force-dynamic";

const CLEANUP_TIMEOUT_MS = 4_000;

type CleanupBody = {
  connectorId?: unknown;
  documentId?: unknown;
  source?: unknown;
};

/**
 * Best-effort cleanup for onboarding-only graph seeds. The browser can only
 * request cleanup for the caller's active org; document deletion is scoped by
 * Data Plane using X-Org-ID.
 */
export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as CleanupBody | null;
    const documentId = stringValue(body?.documentId);
    const source = stringValue(body?.source);
    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json({ cleanupStatus: "failed", error: "No active organization found." }, { status: 409 });
    }

    if (!documentId) {
      return NextResponse.json({
        cleanupStatus: source ? "pending" : "skipped",
      });
    }

    const response = await fetch(
      `${getDataPlaneDocumentsUrl()}/v1/documents/${encodeURIComponent(documentId)}`,
      {
        method: "DELETE",
        headers: {
          "X-Internal-Api-Key": getInternalApiKey(),
          "X-Org-ID": orgId,
          "X-User-Id": session.user.id,
        },
        cache: "no-store",
        signal: AbortSignal.timeout(CLEANUP_TIMEOUT_MS),
      },
    );

    if (response.ok || response.status === 404) {
      return NextResponse.json({ cleanupStatus: "completed" });
    }
    return NextResponse.json({ cleanupStatus: "pending" }, { status: 202 });
  } catch (error) {
    return authErrorResponse(error);
  }
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}
