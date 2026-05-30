import { NextResponse } from "next/server";
import { z } from "zod";
import { fail, ok } from "@/lib/api/envelope";
import { listNovuNotifications, markNovuNotificationRead } from "@/lib/integrations/novu";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const markReadSchema = z.object({
  notificationId: z.string().min(1),
});

export async function GET() {
  try {
    const actor = await requireRequestActor();
    const data = await listNovuNotifications(actor, 40);
    return NextResponse.json(ok(data));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(ok({
        configured: false,
        unreadCount: 0,
        notifications: [],
        messages: [],
      }));
    }
    return NextResponse.json(
      fail({ code: "novu_feed_failed", message: error instanceof Error ? error.message : "Novu feed could not be loaded." }),
      { status: 502 },
    );
  }
}

export async function POST(request: Request) {
  const parsed = markReadSchema.safeParse(await request.json().catch(() => null));

  if (!parsed.success) {
    return NextResponse.json(fail({ code: "invalid_notification", message: "notificationId is required." }), { status: 400 });
  }

  try {
    const actor = await requireRequestActor();
    const result = await markNovuNotificationRead(actor, parsed.data.notificationId);
    return NextResponse.json(ok(result));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(
      fail({ code: "novu_mark_read_failed", message: error instanceof Error ? error.message : "Notification could not be updated." }),
      { status: 502 },
    );
  }
}
