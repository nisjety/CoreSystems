import { NextResponse, type NextRequest } from "next/server";

import { ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fail, ok } from "@/lib/api/envelope";
import { syncKnowledgeWorkspace } from "@/lib/knowledge/knowledge-workspace";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(request: NextRequest) {
  try {
    return NextResponse.json(ok(await syncKnowledgeWorkspace(request)), { status: 202 });
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    return NextResponse.json(
      fail({
        code: "knowledge_sync_failed",
        message: error instanceof Error ? error.message : "Knowledge sync could not be started.",
      }),
      { status: 400 },
    );
  }
}
