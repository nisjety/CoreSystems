import { NextRequest } from "next/server";
import { z } from "zod";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { getSupportTicket, patchSupportTicket } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const updateTicketSchema = z.object({
  state_id: z.number().optional(),
  priority_id: z.number().optional(),
  owner_id: z.number().optional(),
  group_id: z.number().optional(),
  tags: z.array(z.string()).optional(),
});

export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const { id } = await params;

  try {
    const actor = await requireRequestActor();
    const ticket = await getSupportTicket(actor, id);
    return Response.json(ticket);
  } catch (error) {
    return supportRouteError(error, "ticket_fetch_failed", "Support ticket could not be loaded.");
  }
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = updateTicketSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  try {
    const actor = await requireRequestActor();
    const ticket = await patchSupportTicket(actor, id, parsed.data);
    return Response.json(ticket);
  } catch (error) {
    return supportRouteError(error, "ticket_update_failed", "Support ticket could not be updated.");
  }
}
