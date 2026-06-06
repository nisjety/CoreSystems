import { NextRequest, NextResponse } from "next/server";
import { ok, fail } from "@/lib/api/envelope";
import {
  RequestActorError,
  requireRequestActor,
} from "@/lib/integrations/request-actor";
import {
  buildInformationCoreHeaders,
  getInformationCoreUrl,
} from "@/app/api/v1/information/_lib/upstream";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type InformationSliceResult<T> = {
  data: T | null;
  error: string | null;
};

async function fetchInformationSlice<T>({
  fallbackMessage,
  path,
  searchParams,
}: {
  fallbackMessage: string;
  path: string;
  searchParams?: URLSearchParams;
}): Promise<InformationSliceResult<T>> {
  try {
    const upstreamUrl = new URL(`${getInformationCoreUrl()}${path}`);
    if (searchParams) {
      upstreamUrl.search = searchParams.toString();
    }

    const response = await fetch(upstreamUrl, {
      headers: buildInformationCoreHeaders(),
      cache: "no-store",
      signal: AbortSignal.timeout(15_000),
    });

    const payload = (await response.json().catch(() => null)) as
      | { error?: { message?: string } }
      | T
      | null;
    if (!response.ok || !payload) {
      return {
        data: null,
        error:
          (payload as { error?: { message?: string } } | null)?.error?.message ??
          fallbackMessage,
      };
    }

    return {
      data: payload as T,
      error: null,
    };
  } catch {
    return {
      data: null,
      error: fallbackMessage,
    };
  }
}

export async function GET(request: NextRequest) {
  try {
    await requireRequestActor();

    const lat = request.nextUrl.searchParams.get("lat");
    const lon = request.nextUrl.searchParams.get("lon");
    const altitude = request.nextUrl.searchParams.get("altitude");
    const trafficRadius =
      request.nextUrl.searchParams.get("trafficRadius") ?? "40";
    const trafficSearch = request.nextUrl.searchParams.get("trafficSearch");
    const newsLimit = request.nextUrl.searchParams.get("newsLimit") ?? "8";
    const newsCategory = request.nextUrl.searchParams.get("newsCategory");
    const newsMaxAge = request.nextUrl.searchParams.get("newsMaxAge") ?? "24";

    const weatherParams = new URLSearchParams();
    const trafficParams = new URLSearchParams({ radius: trafficRadius });
    const newsParams = new URLSearchParams({
      limit: newsLimit,
      maxAge: newsMaxAge,
    });

    if (lat) {
      weatherParams.set("lat", lat);
      trafficParams.set("lat", lat);
    }
    if (lon) {
      weatherParams.set("lon", lon);
      trafficParams.set("lon", lon);
    }
    if (altitude) {
      weatherParams.set("altitude", altitude);
    }
    if (trafficSearch) {
      trafficParams.set("search", trafficSearch);
    }
    if (newsCategory) {
      newsParams.set("category", newsCategory);
    }

    const [weather, traffic, news] = await Promise.all([
      fetchInformationSlice({
        fallbackMessage: "Weather data is unavailable.",
        path: "/api/v1/weather",
        searchParams: weatherParams,
      }),
      fetchInformationSlice({
        fallbackMessage: "Traffic data is unavailable.",
        path: "/api/v1/traffic",
        searchParams: trafficParams,
      }),
      fetchInformationSlice({
        fallbackMessage: "News data is unavailable.",
        path: "/api/v1/news",
        searchParams: newsParams,
      }),
    ]);

    return NextResponse.json(
      ok({
        news: news.data,
        newsError: news.error,
        traffic: traffic.data,
        trafficError: traffic.error,
        weather: weather.data,
        weatherError: weather.error,
      }),
    );
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }

    return NextResponse.json(
      fail({
        code: "information_dashboard_unavailable",
        message: "Dashboard information is unavailable.",
      }),
      { status: 502 },
    );
  }
}
