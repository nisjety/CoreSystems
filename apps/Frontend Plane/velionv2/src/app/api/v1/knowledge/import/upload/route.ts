import { NextResponse, type NextRequest } from "next/server";

import { ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fail, ok } from "@/lib/api/envelope";
import { forwardImportUpload } from "@/lib/knowledge/knowledge-workspace";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function POST(request: NextRequest) {
  try {
    return NextResponse.json(ok(await forwardImportUpload(request)), { status: 202 });
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    return NextResponse.json(
      fail({
        code: "import_upload_failed",
        message: error instanceof Error ? error.message : "Upload import could not be started.",
      }),
      { status: 400 },
    );
  }
}
