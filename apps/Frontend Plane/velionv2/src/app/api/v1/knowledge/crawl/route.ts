import { NextResponse, type NextRequest } from "next/server";
import { z } from "zod";

import { ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fail, ok } from "@/lib/api/envelope";
import { startKnowledgeWebsiteCrawl } from "@/lib/knowledge/knowledge-workspace";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const crawlRequestSchema = z.object({
  url: z.string().trim().url(),
  maxPages: z.number().int().min(1).max(50).optional(),
});

export async function POST(request: NextRequest) {
  try {
    const body = await request.json().catch(() => null);
    const parsed = crawlRequestSchema.safeParse(body);
    if (!parsed.success) {
      return NextResponse.json(
        fail({ code: "invalid_crawl_request", message: "A valid website URL is required." }),
        { status: 400 },
      );
    }

    return NextResponse.json(ok(await startKnowledgeWebsiteCrawl(request, parsed.data)), { status: 202 });
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    return NextResponse.json(
      fail({
        code: "knowledge_crawl_failed",
        message: error instanceof Error ? error.message : "The website crawl could not be started.",
      }),
      { status: 400 },
    );
  }
}
