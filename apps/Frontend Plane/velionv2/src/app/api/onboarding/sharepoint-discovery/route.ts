import { NextRequest, NextResponse } from "next/server";

import {
  authErrorResponse,
  getInternalApiKey,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getFinspoCoreUrl,
  getOrgCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const DISCOVERY_TIMEOUT_MS = 6_000;

/**
 * Warm finspo-core after Microsoft connect. integration-corev2 owns OAuth;
 * SharePoint discovery/sync belongs to finspo so ACL + delta state stay in the
 * ingestion plane.
 */
export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const requestBody = (await request.json().catch(() => null)) as { orgId?: string } | null;
    const requestedOrgId =
      typeof requestBody?.orgId === "string" ? requestBody.orgId.trim() : "";
    const activeOrgId = await resolveActiveOrgId(request, session);
    const orgId = requestedOrgId || activeOrgId;
    if (!orgId) {
      return NextResponse.json(
        { error: { code: "no_org", message: "No active organization found." } },
        { status: 409 },
      );
    }
    if (requestedOrgId && requestedOrgId !== activeOrgId) {
      const hasAccess = await verifyOrgAccess(request, session, requestedOrgId);
      if (!hasAccess) {
        return NextResponse.json(
          { error: { code: "forbidden", message: "Organization is not available." } },
          { status: 403 },
        );
      }
    }

    const response = await fetch(`${getFinspoCoreUrl()}/api/v1/sharepoint/sites`, {
      method: "GET",
      headers: {
        Accept: "application/json",
        "X-API-Key": getInternalApiKey(),
        "x-internal-api-key": getInternalApiKey(),
        "X-Org-ID": orgId,
        "X-User-ID": session.user.id,
      },
      cache: "no-store",
      signal: AbortSignal.timeout(DISCOVERY_TIMEOUT_MS),
    });

    const responseBody = (await response.json().catch(() => null)) as
      | { success?: boolean; data?: { count?: number; sites?: unknown[] }; error?: unknown }
      | null;

    if (!response.ok) {
      return NextResponse.json(
        {
          ready: false,
          count: 0,
          warning: response.status === 503 ? "sharepoint_not_configured" : "sharepoint_discovery_failed",
        },
        { status: 202 },
      );
    }

    return NextResponse.json(
      {
        ready: true,
        count:
          typeof responseBody?.data?.count === "number"
            ? responseBody.data.count
            : responseBody?.data?.sites?.length ?? 0,
      },
      { status: 202 },
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}

async function verifyOrgAccess(
  request: NextRequest,
  session: Awaited<ReturnType<typeof requireSession>>,
  orgId: string,
): Promise<boolean> {
  try {
    const response = await fetch(`${getOrgCoreUrl()}/orgs/${encodeURIComponent(orgId)}`, {
      method: "GET",
      headers: buildServiceHeaders(request, session, orgId),
      cache: "no-store",
      signal: AbortSignal.timeout(DISCOVERY_TIMEOUT_MS),
    });
    return response.ok;
  } catch {
    return false;
  }
}
