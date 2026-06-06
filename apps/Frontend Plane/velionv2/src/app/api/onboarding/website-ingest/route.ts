import { createHash } from "node:crypto";
import { NextRequest, NextResponse } from "next/server";

import { authErrorResponse, requireSession } from "@/app/api/_lib/control-plane-auth";
import {
  buildQuarryControlHeaders,
  buildServiceHeaders,
  getOrgCoreUrl,
  getQuarryControlUrl,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { normalizePublicHttpUrl } from "@/app/api/onboarding/_lib/public-url";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const INGEST_TIMEOUT_MS = 7_000;

export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as
      | { orgId?: string; url?: string; brief?: string; maxPages?: number }
      | null;

    const orgId = typeof body?.orgId === "string" ? body.orgId.trim() : "";
    const rawUrl = typeof body?.url === "string" ? body.url.trim() : "";
    if (!orgId || !rawUrl) {
      return NextResponse.json(
        { error: { code: "bad_request", message: "orgId and url are required." } },
        { status: 400 },
      );
    }

    const hasAccess = await verifyOrgAccess(request, session, orgId);
    if (!hasAccess) {
      return NextResponse.json(
        { error: { code: "forbidden", message: "Organization is not available." } },
        { status: 403 },
      );
    }

    const validatedUrl = await normalizePublicHttpUrl(rawUrl);
    if (!validatedUrl.ok) {
      return NextResponse.json(
        { error: { code: validatedUrl.code, message: "Website URL is not crawlable." } },
        { status: 400 },
      );
    }

    const maxPages =
      typeof body?.maxPages === "number" ? Math.min(20, Math.max(1, body.maxPages)) : 8;
    const path = "/v1/jobs/";
    const requestBody = JSON.stringify({
      kind: "crawl",
      params: {
        url: validatedUrl.url,
        max_pages: maxPages,
        max_depth: 1,
        auto_commit: true,
        org_id: orgId,
        brief: typeof body?.brief === "string" ? body.brief.trim() || undefined : undefined,
      },
    });

    const response = await fetch(`${getQuarryControlUrl()}${path}`, {
      method: "POST",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "Idempotency-Key": websiteIngestKey(orgId, validatedUrl.url, maxPages),
        ...buildQuarryControlHeaders("POST", path, requestBody),
      },
      cache: "no-store",
      signal: AbortSignal.timeout(INGEST_TIMEOUT_MS),
      body: requestBody,
    });

    const responseBody = (await response.json().catch(() => null)) as unknown;
    if (!response.ok) {
      console.error("[onboarding:website-ingest] quarry-control failed", {
        status: response.status,
        responseBody,
      });
      return NextResponse.json(
        { error: { code: "upstream_error", message: "Could not start website ingest." } },
        { status: 502 },
      );
    }

    const data = unwrapData(responseBody);
    return NextResponse.json(
      {
        accepted: true,
        jobId: record(data)?.id,
      },
      { status: 202 },
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}

async function verifyOrgAccess(
  request: NextRequest,
  session: Awaited<ReturnType<typeof requireSession>>,
  orgId: string,
): Promise<boolean> {
  try {
    const response = await fetch(`${getOrgCoreUrl()}/orgs/${encodeURIComponent(orgId)}`, {
      method: "GET",
      headers: buildServiceHeaders(request, session, orgId),
      cache: "no-store",
      signal: AbortSignal.timeout(INGEST_TIMEOUT_MS),
    });
    return response.ok;
  } catch {
    return false;
  }
}

function websiteIngestKey(orgId: string, url: string, maxPages: number): string {
  const digest = createHash("sha256")
    .update(`${orgId}:${url}:${maxPages}`)
    .digest("hex")
    .slice(0, 40);
  return `velion-website-${digest}`;
}

function unwrapData(value: unknown): unknown {
  const root = record(value);
  return root && "data" in root ? root.data : value;
}

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
