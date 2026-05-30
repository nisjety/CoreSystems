import { NextResponse } from "next/server";
import { z } from "zod";
import { fail, ok } from "@/lib/api/envelope";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const eventSchema = z.object({
  title: z.string().trim().min(1).max(160),
  start: z.string().datetime(),
  end: z.string().datetime().optional(),
  type: z.string().trim().max(32).optional(),
});

const noteSchema = z.object({
  text: z.string().trim().min(1).max(600),
  date: z.string().trim().max(32).optional(),
});

export async function GET() {
  try {
    const actor = await requireRequestActor();
    const state = await fetchUserCoreJson(actor, "/api/v1/calendar/events");
    return NextResponse.json(ok(state));
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok({ configured: false, events: [], notes: [] }));
    }

    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "calendar_load_failed", message: "Calendar could not be loaded." }), { status: 500 });
  }
}

export async function POST(request: Request) {
  const body = await request.json().catch(() => null);
  const kind = body && typeof body === "object" && "kind" in body ? body.kind : "event";
  const parsed = kind === "note" ? noteSchema.safeParse(body) : eventSchema.safeParse(body);

  if (!parsed.success) {
    return NextResponse.json(fail({ code: "invalid_calendar_payload", message: "Calendar payload is invalid." }), { status: 400 });
  }

  try {
    const actor = await requireRequestActor();
    const path = kind === "note" ? "/api/v1/calendar/notes" : "/api/v1/calendar/events";
    const saved = await fetchUserCoreJson(actor, path, {
      method: "POST",
      body: JSON.stringify(parsed.data),
    });
    return NextResponse.json(ok(saved), { status: 201 });
  } catch (error) {
    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "calendar_save_failed", message: "Calendar data could not be saved." }), { status: 500 });
  }
}
