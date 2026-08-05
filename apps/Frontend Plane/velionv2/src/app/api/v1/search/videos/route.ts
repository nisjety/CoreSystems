import { NextRequest, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

// SearXNG is the org's metasearch aggregator; it has a first-class `videos`
// category whose JSON results carry an embeddable `iframe_src` — exactly what
// the inline player needs. verevonv2 shares the `inter-plane-bus` network with
// SearXNG, so the in-cluster URL resolves without extra config; override with
// SEARXNG_URL if the topology changes.
function getSearxngUrl(): string {
  return (process.env.SEARXNG_URL || "http://searxng:8080").replace(/\/+$/, "");
}

// Raw SearXNG video result (only the fields we use; everything else ignored).
type SearxngVideoResult = {
  url?: string;
  title?: string;
  thumbnail?: string | null;
  iframe_src?: string | null;
  length?: string | null;
  author?: string | null;
  publishedDate?: string | null;
};

// Sanitized shape sent to the client. `url` = source page (card click);
// `embedUrl` = inline-playable iframe src (video click); `thumbnailUrl` = poster.
type VideoHit = {
  url: string;
  title: string | null;
  thumbnailUrl: string | null;
  embedUrl: string | null;
  author: string | null;
  length: string | null;
};

function isSameOriginRequest(request: Request): boolean {
  const origin = request.headers.get("origin");
  const host = request.headers.get("host");
  if (!origin || !host) return true;
  try {
    return new URL(origin).host === host;
  } catch {
    return false;
  }
}

// Only surface http(s) URLs (drop data:/javascript: the proxy might return).
function isSafeHttpUrl(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0) return false;
  try {
    const u = new URL(value);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

function sanitizeVideos(results: SearxngVideoResult[]): VideoHit[] {
  const out: VideoHit[] = [];
  const seen = new Set<string>();
  for (const r of results) {
    const url = isSafeHttpUrl(r.url) ? r.url : null;
    if (!url || seen.has(url)) continue;
    seen.add(url);
    out.push({
      url,
      title: typeof r.title === "string" && r.title.trim() ? r.title.trim() : null,
      thumbnailUrl: isSafeHttpUrl(r.thumbnail) ? r.thumbnail : null,
      // iframe_src is the embeddable player; only keep https(s) embeds.
      embedUrl: isSafeHttpUrl(r.iframe_src) ? r.iframe_src : null,
      author: typeof r.author === "string" && r.author.trim() ? r.author.trim() : null,
      length: typeof r.length === "string" && r.length.trim() ? r.length.trim() : null,
    });
  }
  return out;
}

export async function POST(request: NextRequest) {
  if (!isSameOriginRequest(request)) {
    return NextResponse.json(
      fail({ code: "invalid_origin", message: "Request origin is not allowed." }),
      { status: 403 },
    );
  }

  try {
    // Same auth gate as the other search verticals (org-scoped session).
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
    const limit = typeof body?.limit === "number" ? Math.min(Math.max(body.limit, 1), 50) : 24;

    const params = new URLSearchParams({
      q: query,
      categories: "videos",
      format: "json",
      safesearch: "1",
    });
    const upstream = await fetch(`${getSearxngUrl()}/search?${params.toString()}`, {
      method: "GET",
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(15000),
    }).catch(() => null);

    if (!upstream || !upstream.ok) {
      // SearXNG returns 403 on /search?format=json when the JSON format is not
      // enabled in its settings — surface an honest, actionable message.
      return NextResponse.json(
        fail({
          code: "video_search_unavailable",
          message:
            "Video search is unavailable — SearXNG must have the JSON format enabled (search.formats: [json]).",
        }),
        { status: 502 },
      );
    }

    const data = (await upstream.json().catch(() => null)) as
      | { results?: SearxngVideoResult[] }
      | null;
    if (!data) {
      return NextResponse.json(
        fail({ code: "video_search_parse_error", message: "Video search returned an unexpected response." }),
        { status: 502 },
      );
    }

    const videos = sanitizeVideos(Array.isArray(data.results) ? data.results : []).slice(0, limit);
    return NextResponse.json(ok({ videos }));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }
    if (error instanceof Error && error.name === "TimeoutError") {
      return NextResponse.json(
        fail({ code: "video_search_timeout", message: "Video search timed out." }),
        { status: 504 },
      );
    }
    return NextResponse.json(
      fail({ code: "video_search_failed", message: "Video search could not be completed." }),
      { status: 500 },
    );
  }
}
