import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse, ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fetchQuarry, paginateByOffset } from "@/app/api/ingestions/_lib/quarry-ingestions";
import { cursorPage, fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type QuarryPage<T> = {
  items: T[];
};

type QuarrySchedule = {
  schedule_id: string;
  name: string;
  kind: string;
  status: string;
  cron?: string | null;
  schedule_at?: string | null;
  created_at: string;
  last_run_at?: string | null;
  next_run_at?: string | null;
  config?: Record<string, unknown>;
};

type CreateScheduleBody = {
  name?: string;
  kind?: "crawl" | "extract" | "search" | "batch" | "agent";
  cron?: string;
  scheduleAt?: string;
  overlapPolicy?: "skip" | "allow" | "cancel";
  targetUrl?: string;
  maxPages?: number;
  urls?: string[];
  schema?: unknown;
};

function normalizeSchedule(schedule: QuarrySchedule) {
  const config = schedule.config ?? {};
  return {
    id: schedule.schedule_id,
    name: schedule.name,
    kind: schedule.kind,
    status: schedule.status,
    cron: schedule.cron ?? null,
    scheduleAt: schedule.schedule_at ?? null,
    createdAt: schedule.created_at,
    lastRunAt: schedule.last_run_at ?? null,
    nextRunAt: schedule.next_run_at ?? null,
    target:
      (typeof config.url === "string" && config.url) ||
      (Array.isArray(config.urls) ? `${config.urls.length} URLs` : null) ||
      (typeof config.query === "string" && config.query) ||
      "Scheduled ingestion",
    config,
  };
}

export async function GET(request: NextRequest) {
  try {
    const url = new URL(request.url);
    const limit = Math.min(50, Math.max(1, Number.parseInt(url.searchParams.get("limit") ?? "20", 10) || 20));
    const cursor = url.searchParams.get("cursor");
    const page = await fetchQuarry<QuarryPage<QuarrySchedule>>(request, "/v1/schedules", {
      query: { limit: 100 },
    });
    const schedules = page.items
      .map(normalizeSchedule)
      .sort((a, b) => {
        const nextA = a.nextRunAt ? Date.parse(a.nextRunAt) : 0;
        const nextB = b.nextRunAt ? Date.parse(b.nextRunAt) : 0;
        return nextB - nextA;
      });
    const { page: paged, nextCursor } = paginateByOffset({ cursor, data: schedules, limit });
    return NextResponse.json(
      cursorPage({
        data: paged,
        limit,
        nextCursor,
        self: `/api/ingestions/schedules?limit=${limit}`,
      }),
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}

export async function POST(request: NextRequest) {
  try {
    const body = (await request.json().catch(() => null)) as CreateScheduleBody | null;
    if (!body?.name?.trim() || !body.kind) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "Schedule name and kind are required." }),
        { status: 422 },
      );
    }
    if (!body.cron?.trim() && !body.scheduleAt?.trim()) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "Provide either a cron expression or a scheduled timestamp." }),
        { status: 422 },
      );
    }

    const config =
      body.kind === "batch"
        ? { urls: (body.urls ?? []).map((value) => value.trim()).filter(Boolean) }
        : {
            url: body.targetUrl?.trim(),
            max_pages: body.maxPages,
            schema: body.schema,
          };

    const schedule = await fetchQuarry<QuarrySchedule>(request, "/v1/schedules", {
      method: "POST",
      body: {
        name: body.name.trim(),
        kind: body.kind,
        cron: body.cron?.trim() || undefined,
        schedule_at: body.scheduleAt?.trim() || undefined,
        overlap_policy: body.overlapPolicy ?? "skip",
        config,
      },
    });

    return NextResponse.json(ok(normalizeSchedule(schedule)), { status: 201 });
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return authErrorResponse(error);
    }
    return NextResponse.json(
      fail({ code: "schedule_create_failed", message: "Schedule could not be created." }),
      { status: 500 },
    );
  }
}
