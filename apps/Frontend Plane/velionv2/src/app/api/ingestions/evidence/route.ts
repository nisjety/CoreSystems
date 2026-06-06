import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse } from "@/app/api/_lib/control-plane-auth";
import { fetchQuarry } from "@/app/api/ingestions/_lib/quarry-ingestions";
import { fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type QuarryPage<T> = {
  items: T[];
};

type QuarryJobHistoryEvent = {
  run_id: string;
  kind: string;
  stage: string;
  status: string;
  seq: number;
  completed: number;
  total?: number | null;
  discovered: number;
  queued: number;
  retries: number;
  blocks: number;
  timestamp: string;
  payload?: Record<string, unknown>;
};

export async function GET(request: NextRequest) {
  try {
    const url = new URL(request.url);
    const runId = url.searchParams.get("runId")?.trim();
    if (!runId) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "runId is required." }),
        { status: 422 },
      );
    }

    const events = await fetchQuarry<QuarryPage<QuarryJobHistoryEvent>>(request, `/v1/runs/${runId}/events`, {
      query: { limit: 250 },
    });

    const warnings = events.items
      .filter((event) => event.status === "warn" || event.blocks > 0)
      .map((event) => ({
        id: `${event.run_id}:${event.seq}`,
        stage: event.stage,
        summary:
          (typeof event.payload?.error === "string" && event.payload.error) ||
          (typeof event.payload?.message === "string" && event.payload.message) ||
          `Warning during ${event.stage}`,
      }));

    return NextResponse.json(
      ok({
        runId,
        timeline: events.items,
        warnings,
      }),
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}
