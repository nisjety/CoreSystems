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

    const upstream = new URL(`${getInformationCoreUrl()}/api/v1/traffic`);
    const radius = request.nextUrl.searchParams.get("radius");
    const search = request.nextUrl.searchParams.get("search");
    const lat = request.nextUrl.searchParams.get("lat");
    const lon = request.nextUrl.searchParams.get("lon");

    if (lat) upstream.searchParams.set("lat", lat);
    if (lon) upstream.searchParams.set("lon", lon);
    if (radius) upstream.searchParams.set("radius", radius);
    if (search) upstream.searchParams.set("search", search);

    const response = await fetch(upstream, {
      headers: buildInformationCoreHeaders(),
      cache: "no-store",
      signal: AbortSignal.timeout(15000),
    });

    const payload = await response.json().catch(() => null);
    if (!response.ok || !payload) {
      return NextResponse.json(
        mapInformationCoreError(
          (payload as { error?: { message?: string } } | null)?.error?.message ?? "Traffic data is unavailable.",
          "traffic_unavailable",
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
      fail({ code: "traffic_unavailable", message: "Traffic data is unavailable." }),
      { status: 502 },
    );
  }
}

