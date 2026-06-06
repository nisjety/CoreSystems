import { type NextRequest, NextResponse } from "next/server";

import { ok, fail } from "@/lib/api/envelope";
import {
  requireSession,
  authErrorResponse,
} from "@/app/api/_lib/control-plane-auth";
import { resolveActiveOrgId } from "@/app/api/onboarding/_lib/onboarding-proxy";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

// BFF — GET /api/v1/search/context
//
// Returns the identifiers the browser needs to subscribe to its own search
// history in Convex: { userId, orgId }. Resolved with the SAME helpers the
// persist route uses (requireSession + resolveActiveOrgId), so reads are scoped
// identically to writes — the recent list always matches what was stored.
export async function GET(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const orgId = await resolveActiveOrgId(request, session);
    return NextResponse.json(
      ok({ userId: session.user.id, orgId: orgId ?? null }),
      { status: 200 },
    );
  } catch (error) {
    if (error && typeof error === "object" && "status" in error) {
      return authErrorResponse(error);
    }
    return NextResponse.json(
      fail({ code: "context_failed", message: "Could not resolve search context." }),
      { status: 500 },
    );
  }
}
