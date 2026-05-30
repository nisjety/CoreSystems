import { NextResponse } from "next/server";
import { z } from "zod";
import { fail, ok } from "@/lib/api/envelope";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { fetchUserCoreJson, UserCoreError } from "@/lib/integrations/user-core";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const supportSchema = z.object({
  subject: z.string().trim().min(2).max(160),
  message: z.string().trim().min(4).max(2000),
  context: z.string().trim().max(200).optional(),
});

export async function POST(request: Request) {
  const parsed = supportSchema.safeParse(await request.json().catch(() => null));

  if (!parsed.success) {
    return NextResponse.json(fail({ code: "invalid_support_request", message: "Subject and message are required." }), { status: 400 });
  }

  try {
    const actor = await requireRequestActor();
    const saved = await fetchUserCoreJson(actor, "/api/v1/support/requests", {
      method: "POST",
      body: JSON.stringify(parsed.data),
    });
    return NextResponse.json(ok(saved), { status: 201 });
  } catch (error) {
    if (error instanceof RequestActorError || error instanceof UserCoreError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(fail({ code: "support_save_failed", message: "Support request could not be saved." }), { status: 500 });
  }
}
