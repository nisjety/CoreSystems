import { NextRequest, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import {
  buildInformationCoreHeaders,
  getInformationCoreUrl,
  mapInformationCoreError,
} from "@/app/api/v1/information/_lib/upstream";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: NextRequest) {
  try {
    await requireRequestActor();

    const upstream = new URL(`${getInformationCoreUrl()}/api/v1/news`);
    const limit = request.nextUrl.searchParams.get("limit");
    const category = request.nextUrl.searchParams.get("category");
    const maxAge = request.nextUrl.searchParams.get("maxAge");

    if (limit) upstream.searchParams.set("limit", limit);
    if (category) upstream.searchParams.set("category", category);
    if (maxAge) upstream.searchParams.set("maxAge", maxAge);

    const response = await fetch(upstream, {
      headers: buildInformationCoreHeaders(),
      cache: "no-store",
      signal: AbortSignal.timeout(15000),
    });

    const payload = await response.json().catch(() => null);
    if (!response.ok || !payload) {
      return NextResponse.json(
        mapInformationCoreError(
          (payload as { error?: { message?: string } } | null)?.error?.message ?? "News data is unavailable.",
          "news_unavailable",
        ),
        { status: 502 },
      );
    }

    return NextResponse.json(ok(payload));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }

    return NextResponse.json(
      fail({ code: "news_unavailable", message: "News data is unavailable." }),
      { status: 502 },
    );
  }
}

