import { NextRequest, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import {
  getQuarryEdgeUrl,
  getQuarryAudience,
  mintAudienceToken,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type WebResult = {
  url: string;
  title?: string;
  snippet?: string;
};

type WebCitation = {
  url: string;
  title?: string;
};

type QuarrySearchResponse = {
  query: string;
  provider: string;
  results: WebResult[];
  count: number;
  answer?: string;
  citations?: WebCitation[];
  context?: string;
};

function isSameOriginRequest(request: Request) {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");

  if (!origin || !host) {
    return true;
  }

  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

export async function POST(request: NextRequest) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(
      fail({ code: "invalid_origin", message: "Request origin is not allowed." }),
      { status: 403 },
    );
  }

  try {
    await requireRequestActor();

    const body = (await request.json().catch(() => null)) as
      | { query?: string; limit?: number }
      | null;

    const query = typeof body?.query === "string" ? body.query.trim() : "";

    if (!query) {
      return NextResponse.json(
        fail({ code: "invalid_query", message: "A non-empty query is required." }),
        { status: 400 },
      );
    }

    const token = await mintAudienceToken(request, getQuarryAudience());

    const quarryUrl = `${getQuarryEdgeUrl()}/v1/search`;
    const quarryHeaders: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (token) {
      quarryHeaders["Authorization"] = `Bearer ${token}`;
    }

    const upstream = await fetch(quarryUrl, {
      method: "POST",
      headers: quarryHeaders,
      body: JSON.stringify({
        query,
        limit: typeof body?.limit === "number" ? body.limit : 8,
        include_answer: true,
        safe_search: true,
      }),
      signal: AbortSignal.timeout(15000),
    });

    if (!upstream.ok) {
      return NextResponse.json(
        fail({ code: "web_search_unavailable", message: "Web search could not be completed." }),
        { status: 502 },
      );
    }

    const data = (await upstream.json().catch(() => null)) as QuarrySearchResponse | null;

    if (!data) {
      return NextResponse.json(
        fail({ code: "web_search_parse_error", message: "Web search returned an unexpected response." }),
        { status: 502 },
      );
    }

    return NextResponse.json(
      ok({
        results: Array.isArray(data.results) ? data.results : [],
        answer: typeof data.answer === "string" ? data.answer : null,
        citations: Array.isArray(data.citations) ? data.citations : [],
      }),
    );
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }

    if (error instanceof Error && error.name === "TimeoutError") {
      return NextResponse.json(
        fail({ code: "web_search_timeout", message: "Web search timed out." }),
        { status: 504 },
      );
    }

    return NextResponse.json(
      fail({ code: "web_search_failed", message: "Web search could not be completed." }),
      { status: 500 },
    );
  }
}
