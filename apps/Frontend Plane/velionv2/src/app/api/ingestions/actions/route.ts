import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse, ControlPlaneAuthError } from "@/app/api/_lib/control-plane-auth";
import { fetchQuarry } from "@/app/api/ingestions/_lib/quarry-ingestions";
import { fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type ActionBody = {
  action?: "pause_schedule" | "unpause_schedule" | "trigger_schedule" | "backfill_schedule" | "delete_schedule";
  scheduleId?: string;
  startAt?: string;
  endAt?: string;
};

export async function POST(request: NextRequest) {
  try {
    const body = (await request.json().catch(() => null)) as ActionBody | null;
    if (!body?.action || !body.scheduleId?.trim()) {
      return NextResponse.json(
        fail({ code: "validation_error", message: "Action and scheduleId are required." }),
        { status: 422 },
      );
    }

    const scheduleId = body.scheduleId.trim();
    switch (body.action) {
      case "pause_schedule": {
        const schedule = await fetchQuarry(request, `/v1/schedules/${scheduleId}/pause`, { method: "POST" });
        return NextResponse.json(ok(schedule));
      }
      case "unpause_schedule": {
        const schedule = await fetchQuarry(request, `/v1/schedules/${scheduleId}/unpause`, { method: "POST" });
        return NextResponse.json(ok(schedule));
      }
      case "trigger_schedule": {
        const schedule = await fetchQuarry(request, `/v1/schedules/${scheduleId}/trigger`, { method: "POST" });
        return NextResponse.json(ok(schedule));
      }
      case "backfill_schedule": {
        if (!body.startAt?.trim() || !body.endAt?.trim()) {
          return NextResponse.json(
            fail({ code: "validation_error", message: "Backfill requires startAt and endAt." }),
            { status: 422 },
          );
        }
        const schedule = await fetchQuarry(request, `/v1/schedules/${scheduleId}/backfill`, {
          method: "POST",
          body: {
            start_at: body.startAt,
            end_at: body.endAt,
            overlap_policy: "allow",
          },
        });
        return NextResponse.json(ok(schedule));
      }
      case "delete_schedule": {
        await fetchQuarry(request, `/v1/schedules/${scheduleId}`, { method: "DELETE" });
        return NextResponse.json(ok({ deleted: true, scheduleId }));
      }
      default:
        return NextResponse.json(
          fail({ code: "validation_error", message: "Unsupported ingestion action." }),
          { status: 422 },
        );
    }
  } catch (error) {
    if (error instanceof ControlPlaneAuthError) {
      return authErrorResponse(error);
    }
    return NextResponse.json(
      fail({ code: "ingestion_action_failed", message: "The ingestion action failed." }),
      { status: 500 },
    );
  }
}
