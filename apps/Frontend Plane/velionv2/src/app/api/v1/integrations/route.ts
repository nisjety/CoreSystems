import { NextResponse, type NextRequest } from "next/server";

import { authErrorResponse } from "@/app/api/_lib/control-plane-auth";
import { ok } from "@/lib/api/envelope";
import { loadIntegrationSummary } from "@/lib/integrations/integration-corev2";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: NextRequest) {
  try {
    const includeDiscovery = request.nextUrl.searchParams.get("discovery") === "1";
    const includeGraph = request.nextUrl.searchParams.get("graph") === "1";
    const summary = await loadIntegrationSummary(request, { includeDiscovery, includeGraph });
    return NextResponse.json(ok(summary));
  } catch (error) {
    return authErrorResponse(error);
  }
}
