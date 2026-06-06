import { NextResponse, type NextRequest } from "next/server";

import { authErrorResponse } from "@/app/api/_lib/control-plane-auth";
import { ok } from "@/lib/api/envelope";
import { loadKnowledgeWorkspace } from "@/lib/knowledge/knowledge-workspace";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: NextRequest) {
  try {
    return NextResponse.json(ok(await loadKnowledgeWorkspace(request)));
  } catch (error) {
    return authErrorResponse(error);
  }
}
