import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse, ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import {
  fetchQuarry,
  getQuarryContext,
  paginateByOffset,
} from "@/app/api/ingestions/_lib/quarry-ingestions";
import { cursorPage, fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type QuarryPage<T> = {
  items: T[];
  next_cursor?: string | null;
  total_estimated?: number | null;
};

type QuarryJob = {
  job_id: string;
  kind: string;
  status: string;
  created_at: string;
  started_at?: string | null;
  completed_at?: string | null;
  stats?: Record<string, unknown>;
};

type QuarryHandoff = {
  job_id: string;
  accepted_at: string;
};

type QuarryScrapeOutput = {
  run_id: string;
  status: number;
  fetched_at: string;
  fingerprint: string;
  url: { requested: string; final_url: string; canonical?: string | null };
  metadata?: Record<string, unknown>;
  driver?: Record<string, unknown>;
  formats?: Record<string, unknown>;
  source_trace?: Record<string, unknown> | null;
};

type QuarryExtractResult = {
  url: string;
  status: string;
  data?: unknown;
  markdown?: string;
  error?: string;
};

type QuarryExtractResponse = {
  results: QuarryExtractResult[];
  count: number;
  requested: number;
};

type RunListItem = {
  id: string;
  kind: string;
  status: string;
  createdAt: string;
  startedAt?: string | null;
  completedAt?: string | null;
  target: string;
  progress: {
    completed?: number;
    total?: number | null;
    pages?: number;
    urlCount?: number;
    query?: string | null;
  };
  stats: Record<string, unknown>;
};

const RUN_KINDS = ["crawl", "extract", "search", "agent", "batch"] as const;

function toRunItem(job: QuarryJob): RunListItem {
  const stats = job.stats ?? {};
  const derivedTarget =
    firstString(stats, ["url", "seed_url", "query"]) ||
    (firstNumber(stats, ["pages"]) !== null ? `${job.kind} target` : "Manual run");
  const completed = firstNumber(stats, ["completed", "pages_completed", "pages"]);
  const total = firstNumber(stats, ["total", "max_pages", "target_pages"]);
  const pages = firstNumber(stats, ["pages", "page_count"]);
  const urlCount = firstNumber(stats, ["url_count", "urls"]);

  return {
    id: job.job_id,
    kind: job.kind,
    status: job.status,
    createdAt: job.created_at,
    startedAt: job.started_at ?? null,
    completedAt: job.completed_at ?? null,
    target: firstString(stats, ["url", "seed_url", "query", "target"]) || derivedTarget,
    progress: {
      ...(completed !== null ? { completed } : {}),
      ...(total !== null ? { total } : {}),
      ...(pages !== null ? { pages } : {}),
      ...(urlCount !== null ? { urlCount } : {}),
      query: firstString(stats, ["query"]),
    },
    stats,
  };
}

function firstNumber(stats: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = stats[key];
    if (typeof value === "number" && Number.isFinite(value)) return value;
  }
  return null;
}

function firstString(stats: Record<string, unknown>, keys: string[]) {
  for (const key of keys) {
    const value = stats[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return null;
}

async function loadJobs(request: NextRequest, kind: (typeof RUN_KINDS)[number], limit: number) {
  const page = await fetchQuarry<QuarryPage<QuarryJob>>(request, `/v1/${kind}/jobs`, {
    query: { limit },
  });
  return page.items.map(toRunItem);
}

export async function GET(request: NextRequest) {
  try {
    const url = new URL(request.url);
    const limit = Math.min(50, Math.max(1, Number.parseInt(url.searchParams.get("limit") ?? "20", 10) || 20));
    const cursor = url.searchParams.get("cursor");
    const requestedKind = url.searchParams.get("kind")?.trim();
    const kinds = requestedKind && RUN_KINDS.includes(requestedKind as (typeof RUN_KINDS)[number])
      ? [requestedKind as (typeof RUN_KINDS)[number]]
      : [...RUN_KINDS];

    const runs = (
      await Promise.all(kinds.map((kind) => loadJobs(request, kind, limit)))
    )
      .flat()
      .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));

    const { page, nextCursor } = paginateByOffset({ cursor, data: runs, limit });
    const self = `/api/ingestions/runs?limit=${limit}${requestedKind ? `&kind=${encodeURIComponent(requestedKind)}` : ""}`;
    return NextResponse.json(cursorPage({ data: page, limit, nextCursor, self }));
  } catch (error) {
    return authErrorResponse(error);
  }
}

export async function POST(request: NextRequest) {
  try {
    const body = (await request.json().catch(() => null)) as
      | {
          kind?: "scrape" | "crawl" | "extract" | "batch";
          url?: string;
          urls?: string[];
          schema?: unknown;
          prompt?: string;
          maxPages?: number;
        }
      | null;

    const kind = body?.kind;
    if (!kind) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "Run kind is required." }),
        { status: 422 },
      );
    }

    if (kind === "crawl") {
      if (!body.url?.trim()) {
        return NextResponse.json(
          fail({ code: "validation_error", message: "A URL is required for crawl runs." }),
          { status: 422 },
        );
      }

      const ack = await fetchQuarry<QuarryHandoff>(request, "/v1/crawl", {
        method: "POST",
        body: {
          url: body.url.trim(),
          max_pages: Math.min(50, Math.max(1, body.maxPages ?? 12)),
        },
      });

      return NextResponse.json(
        ok({
          run: {
            id: ack.job_id,
            kind: "crawl",
            status: "queued",
            createdAt: ack.accepted_at,
            target: body.url.trim(),
          },
        }),
        { status: 201 },
      );
    }

    if (kind === "batch") {
      const urls = (body.urls ?? []).map((value) => value.trim()).filter(Boolean);
      if (urls.length === 0) {
        return NextResponse.json(
          fail({ code: "validation_error", message: "At least one URL is required for batch runs." }),
          { status: 422 },
        );
      }

      const ack = await fetchQuarry<QuarryHandoff>(request, "/v1/batch", {
        method: "POST",
        body: { urls },
      });

      return NextResponse.json(
        ok({
          run: {
            id: ack.job_id,
            kind: "batch",
            status: "queued",
            createdAt: ack.accepted_at,
            target: `${urls.length} URLs`,
          },
        }),
        { status: 201 },
      );
    }

    if (!body.url?.trim()) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "A URL is required for this run." }),
        { status: 422 },
      );
    }

    if (kind === "extract") {
      const extract = await fetchQuarry<QuarryExtractResponse>(request, "/v1/extract", {
        method: "POST",
        body: {
          urls: [body.url.trim()],
          schema: body.schema,
          prompt: body.prompt,
        },
      });
      const first = extract.results[0];
      return NextResponse.json(
        ok({
          run: {
            id: `extract:${first?.url || body.url.trim()}`,
            kind: "extract",
            status: first?.status || "completed",
            createdAt: new Date().toISOString(),
            target: body.url.trim(),
          },
          evidence: {
            kind: "extract",
            targetUrl: body.url.trim(),
            extractedAt: new Date().toISOString(),
            result: first ?? null,
            results: extract.results,
          },
        }),
      );
    }

    const scrape = await fetchQuarry<QuarryScrapeOutput>(request, "/v1/scrape", {
      method: "POST",
      body: {
        url: body.url.trim(),
      },
    });

    return NextResponse.json(
      ok({
        run: {
          id: scrape.run_id,
          kind: "scrape",
          status: scrape.status >= 200 && scrape.status < 400 ? "completed" : "failed",
          createdAt: scrape.fetched_at,
          target: scrape.url.final_url || scrape.url.requested,
        },
        evidence: {
          kind: "scrape",
          targetUrl: scrape.url.final_url || scrape.url.requested,
          extractedAt: scrape.fetched_at,
          fingerprint: scrape.fingerprint,
          statusCode: scrape.status,
          metadata: scrape.metadata ?? {},
          driver: scrape.driver ?? {},
          formats: scrape.formats ?? {},
          sourceTrace: scrape.source_trace ?? null,
        },
      }),
    );
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return authErrorResponse(error);
    }
    return NextResponse.json(
      fail({ code: "run_create_failed", message: "The ingestion run could not be started." }),
      { status: 500 },
    );
  }
}
