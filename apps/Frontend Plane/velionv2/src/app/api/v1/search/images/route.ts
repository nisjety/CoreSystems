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

// Shape returned by quarry-edge POST /v1/search/images.
type QuarryImage = {
  img_src: string;
  thumbnail_src?: string | null;
  source_url?: string | null;
  title?: string | null;
};

type QuarryImageResponse = {
  query: string;
  provider: string;
  images: QuarryImage[];
};

// Sanitized shape returned to the client. `url` is the source page (for
// click-through); `imageUrl` is full-res; `thumbnailUrl` is the lazy-loaded
// thumbnail (falls back to the full image when no thumbnail is present).
type ImageHit = {
  url: string;
  thumbnailUrl: string;
  imageUrl: string;
  title: string | null;
};

function isSameOriginRequest(request: Request): boolean {
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

// Only forward http(s) image/source URLs to the client; drop anything else
// (e.g. data: URIs the proxy may surface) so the gallery never renders an
// untrusted scheme.
function isSafeHttpUrl(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0) return false;
  try {
    const u = new URL(value);
    return u.protocol === "http:" || u.protocol === "https:";
  } catch {
    return false;
  }
}

function sanitizeImages(images: QuarryImage[]): ImageHit[] {
  const out: ImageHit[] = [];
  for (const img of images) {
    const imageUrl = isSafeHttpUrl(img.img_src) ? img.img_src : null;
    const thumbnailRaw =
      isSafeHttpUrl(img.thumbnail_src) ? img.thumbnail_src : null;
    // Need at least one renderable image URL.
    const renderable = thumbnailRaw ?? imageUrl;
    if (!renderable) continue;

    const sourceUrl = isSafeHttpUrl(img.source_url) ? img.source_url : null;

    out.push({
      // Link to the source page when available; otherwise fall back to the
      // image itself so the thumbnail is always clickable.
      url: sourceUrl ?? (imageUrl ?? renderable),
      thumbnailUrl: thumbnailRaw ?? renderable,
      imageUrl: imageUrl ?? renderable,
      title: typeof img.title === "string" && img.title.trim() ? img.title : null,
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

    const quarryHeaders: Record<string, string> = {
      "Content-Type": "application/json",
    };

    if (token) {
      quarryHeaders["Authorization"] = `Bearer ${token}`;
    }

    const upstream = await fetch(`${getQuarryEdgeUrl()}/v1/search/images`, {
      method: "POST",
      headers: quarryHeaders,
      body: JSON.stringify({
        query,
        limit: typeof body?.limit === "number" ? body.limit : 24,
      }),
      signal: AbortSignal.timeout(15000),
    });

    if (!upstream.ok) {
      const status = upstream.status;

      if (status === 501) {
        return NextResponse.json(
          fail({
            code: "image_search_unconfigured",
            message:
              "Image search is unavailable — no SearXNG provider is configured.",
          }),
          { status: 501 },
        );
      }

      return NextResponse.json(
        fail({
          code: "image_search_unavailable",
          message: `Image search could not be completed (upstream ${status}).`,
        }),
        { status: 502 },
      );
    }

    const data = (await upstream.json().catch(() => null)) as QuarryImageResponse | null;

    if (!data) {
      return NextResponse.json(
        fail({
          code: "image_search_parse_error",
          message: "Image search returned an unexpected response.",
        }),
        { status: 502 },
      );
    }

    const images = sanitizeImages(Array.isArray(data.images) ? data.images : []);

    return NextResponse.json(ok({ images }));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(
        fail({ code: error.code, message: error.message }),
        { status: error.status },
      );
    }

    if (error instanceof Error && error.name === "TimeoutError") {
      return NextResponse.json(
        fail({ code: "image_search_timeout", message: "Image search timed out." }),
        { status: 504 },
      );
    }

    return NextResponse.json(
      fail({ code: "image_search_failed", message: "Image search could not be completed." }),
      { status: 500 },
    );
  }
}
