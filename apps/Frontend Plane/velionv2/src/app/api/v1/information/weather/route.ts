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

    const upstreamUrl = new URL(`${getInformationCoreUrl()}/api/v1/weather`);
    const lat = request.nextUrl.searchParams.get("lat");
    const lon = request.nextUrl.searchParams.get("lon");
    const altitude = request.nextUrl.searchParams.get("altitude");

    if (lat) upstreamUrl.searchParams.set("lat", lat);
    if (lon) upstreamUrl.searchParams.set("lon", lon);
    if (altitude) upstreamUrl.searchParams.set("altitude", altitude);

    const upstream = await fetch(upstreamUrl, {
      headers: buildInformationCoreHeaders(),
      cache: "no-store",
      signal: AbortSignal.timeout(15000),
    });

    const payload = await upstream.json().catch(() => null);
    if (!upstream.ok || !payload) {
      return NextResponse.json(
        mapInformationCoreError(
          (payload as { error?: { message?: string } } | null)?.error?.message ?? "Weather data is unavailable.",
          "weather_unavailable",
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
      fail({ code: "weather_unavailable", message: "Weather data is unavailable." }),
      { status: 502 },
    );
  }
}
