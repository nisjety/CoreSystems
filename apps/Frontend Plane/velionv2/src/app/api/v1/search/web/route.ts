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

type QuarryScrapeResponse = {
  data: {
    markdown?: string;
    metadata?: {
      title?: string;
      description?: string;
      url?: string;
    };
  };
};

// Matches bare domains like "google.com", "sub.example.co.uk/path" and full URLs.
// Bare domain rule: one or more labels of [a-z0-9-], a dot, TLD of 2+ chars, optional path.
// No spaces allowed (queries with spaces are always keyword searches).
const URL_RE = /^([a-z0-9-]+\.)+[a-z]{2,}(\/\S*)?$/i;

function isUrlInput(query: string): boolean {
  if (query.startsWith("http://") || query.startsWith("https://")) {
    try {
      new URL(query);
      return true;
    } catch {
      return false;
    }
  }
  // Bare domain — must have no spaces and match the pattern
  return !query.includes(" ") && URL_RE.test(query);
}

function normalizeUrl(query: string): string {
  if (query.startsWith("http://") || query.startsWith("https://")) {
    return query;
  }
  return `https://${query}`;
}

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
      | { query?: string; limit?: number; includeAnswer?: boolean }
      | null;

    const query = typeof body?.query === "string" ? body.query.trim() : "";

    if (!query) {
      return NextResponse.json(
        fail({ code: "invalid_query", message: "A non-empty query is required." }),
        { status: 400 },
      );
    }
    const limit =
      typeof body?.limit === "number" ? Math.max(1, Math.min(body.limit, 50)) : 8;

    const token = await mintAudienceToken(request, getQuarryAudience());

    const quarryHeaders: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (token) {
      quarryHeaders["Authorization"] = `Bearer ${token}`;
    }

    // --- URL / bare-domain path: scrape the page ---
    if (isUrlInput(query)) {
      const targetUrl = normalizeUrl(query);

      const upstream = await fetch(`${getQuarryEdgeUrl()}/v1/scrape`, {
        method: "POST",
        headers: quarryHeaders,
        body: JSON.stringify({ url: targetUrl, maxPages: 1, formats: ["markdown"] }),
        signal: AbortSignal.timeout(20000),
      });

      if (!upstream.ok) {
        const status = upstream.status;
        return NextResponse.json(
          fail({
            code: "fetch_failed",
            message: `Could not fetch "${targetUrl}" (upstream ${status}).`,
          }),
          { status: 502 },
        );
      }

      const data = (await upstream.json().catch(() => null)) as QuarryScrapeResponse | null;

      if (!data?.data) {
        return NextResponse.json(
          fail({ code: "fetch_parse_error", message: "Page fetch returned an unexpected response." }),
          { status: 502 },
        );
      }

      const markdown = typeof data.data.markdown === "string" ? data.data.markdown : "";
      const excerpt = markdown.replace(/\s+/g, " ").trim().slice(0, 600).trimEnd();

      return NextResponse.json(
        ok({
          mode: "fetch" as const,
          url: targetUrl,
          title: data.data.metadata?.title ?? targetUrl,
          description: data.data.metadata?.description ?? null,
          excerpt: excerpt || null,
        }),
      );
    }

    // --- Keyword search path ---
    const upstream = await fetch(`${getQuarryEdgeUrl()}/v1/search`, {
      method: "POST",
      headers: quarryHeaders,
      body: JSON.stringify({
        query,
        limit,
        // Default on (dashboard summary). The /search page opts out
        // (includeAnswer:false) and streams the answer client-side instead.
        include_answer: body?.includeAnswer === false ? false : true,
        safe_search: true,
      }),
      // Give Quarry enough time to widen from free providers into Brave and
      // still synthesize an answer when the dashboard asks for one.
      signal: AbortSignal.timeout(25_000),
    });

    if (!upstream.ok) {
      const status = upstream.status;

      if (status === 501) {
        return NextResponse.json(
          fail({
            code: "search_provider_unconfigured",
            message: "Web search is unavailable — no search provider is configured.",
          }),
          { status: 501 },
        );
      }

      return NextResponse.json(
        fail({
          code: "web_search_unavailable",
          message: `Web search could not be completed (upstream ${status}).`,
        }),
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

    const results = Array.isArray(data.results) ? data.results : [];
    const answer = typeof data.answer === "string" && data.answer.trim() ? data.answer : null;
    const citations = Array.isArray(data.citations) ? data.citations : [];

    return NextResponse.json(
      ok({
        mode: "search" as const,
        results,
        answer,
        citations,
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
