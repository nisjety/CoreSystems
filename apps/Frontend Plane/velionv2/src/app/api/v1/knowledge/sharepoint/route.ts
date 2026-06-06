import { NextResponse, type NextRequest } from "next/server";
import { z } from "zod";

import { ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fail, ok } from "@/lib/api/envelope";
import { createSharePointKnowledgeSource } from "@/lib/knowledge/knowledge-workspace";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const sharePointSourceSchema = z.object({
  driveId: z.string().trim().min(1),
  driveName: z.string().trim().optional(),
  driveType: z.string().trim().optional(),
  siteId: z.string().trim().min(1),
  siteWebUrl: z.string().trim().optional(),
  tenantId: z.string().trim().optional(),
});

export async function POST(request: NextRequest) {
  try {
    const body = await request.json().catch(() => null);
    const parsed = sharePointSourceSchema.safeParse(body);
    if (!parsed.success) {
      return NextResponse.json(
        fail({ code: "invalid_sharepoint_source", message: "siteId and driveId are required." }),
        { status: 400 },
      );
    }

    return NextResponse.json(ok(await createSharePointKnowledgeSource(request, parsed.data)), { status: 201 });
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    if (error instanceof Error) {
      return NextResponse.json(
        fail({ code: "sharepoint_source_failed", message: error.message }),
        { status: 400 },
      );
    }
    return NextResponse.json(
      fail({ code: "sharepoint_source_failed", message: "SharePoint source could not be registered." }),
      { status: 500 },
    );
  }
}
