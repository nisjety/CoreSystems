import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { buildIntegrationCoreHeaders } from "@/app/api/v1/integrations/_lib/service-auth";
import { fail } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ jobId: string }> },
) {
  try {
    const [session, { jobId }] = await Promise.all([requireSession(request), params]);
    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json(
        fail({ code: "org_required", message: "No active organization found." }),
        { status: 409 },
      );
    }

    const response = await fetch(
      `${getIntegrationCoreUrl()}/api/v1/sync-jobs/${encodeURIComponent(jobId)}/events`,
      {
        method: "GET",
        headers: {
          ...buildIntegrationCoreHeaders(request, session, orgId),
          Accept: "text/event-stream",
        },
        cache: "no-store",
      },
    );

    if (!response.ok || !response.body) {
      return NextResponse.json(
        fail({ code: "sync_events_failed", message: "Could not open sync event stream." }),
        { status: response.status || 502 },
      );
    }

    return new Response(response.body, {
      status: 200,
      headers: {
        "Content-Type": "text/event-stream; charset=utf-8",
        "Cache-Control": "no-cache, no-transform",
        Connection: "keep-alive",
      },
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
