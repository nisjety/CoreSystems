import { NextRequest } from "next/server";
import { z } from "zod";

import { supportRouteError } from "@/app/api/support/_lib/errors";
import { createSupportTicket, listSupportTickets } from "@/lib/integrations/conversation-core";
import { requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const createTicketSchema = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
  customer_email: z.string().email(),
  group: z.string().optional(),
  priority: z.string().optional(),
});

export async function GET(request: NextRequest) {
  try {
    const actor = await requireRequestActor();
    const tickets = await listSupportTickets(actor, new URL(request.url).searchParams);
    return Response.json({ tickets, total: tickets.length });
  } catch (error) {
    return supportRouteError(error, "tickets_fetch_failed", "Support tickets could not be loaded.");
  }
}

export async function POST(request: NextRequest) {
  const raw = await request.json().catch(() => null);
  const parsed = createTicketSchema.safeParse(raw);
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  try {
    const actor = await requireRequestActor();
    const ticket = await createSupportTicket(actor, parsed.data);
    return Response.json(ticket, { status: 201 });
  } catch (error) {
    return supportRouteError(error, "ticket_create_failed", "Support ticket could not be created.");
  }
}
