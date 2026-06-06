import { NextRequest } from "next/server";
import { z } from "zod";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { executeSupportMacro } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const executeSchema = z.object({
  ticketId: z.union([z.number(), z.string().min(1)]),
});

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = executeSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  try {
    const actor = await requireRequestActor();
    const result = await executeSupportMacro(actor, id, String(parsed.data.ticketId));
    return Response.json(result);
  } catch (error) {
    return supportRouteError(error, "macro_execute_failed", "Support macro could not be executed.");
  }
}
