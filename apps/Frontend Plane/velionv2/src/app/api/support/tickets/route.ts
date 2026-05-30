import { NextRequest } from "next/server";
import { z } from "zod";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type ZammadTicket = {
  id: number;
  [key: string]: unknown;
};

const createTicketSchema = z.object({
  title: z.string().min(1),
  body: z.string().min(1),
  customer_email: z.string().email(),
  group: z.string().optional(),
  priority: z.string().optional(),
});

export async function GET(request: NextRequest) {
  if (!zammadConfigured()) return notConfiguredResponse();

  const { searchParams } = new URL(request.url);
  const page = searchParams.get("page") || "1";
  const limit = searchParams.get("limit") || "50";
  const state = searchParams.get("state");
  const group = searchParams.get("group") || searchParams.get("queue");

  const query = new URLSearchParams({ expand: "true", per_page: limit, page });
  if (state) query.set("state", state);
  if (group && !["mine", "all", "unassigned", "created-by-you", "spam", "dashboard", "mentions"].includes(group)) {
    query.set("group", group);
  }

  const response = await fetch(`${ZAMMAD_URL}/api/v1/tickets?${query}`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<ZammadTicket[] | { tickets?: ZammadTicket[]; total?: number }>(response);

  if (!response.ok) {
    return Response.json(payload ?? { error: "tickets_fetch_failed" }, { status: response.status });
  }

  const tickets = Array.isArray(payload) ? payload : payload?.tickets ?? [];
  const total = Array.isArray(payload)
    ? Number(response.headers.get("x-total-count") ?? tickets.length)
    : payload?.total ?? tickets.length;

  return Response.json({ tickets, total });
}

export async function POST(request: NextRequest) {
  if (!zammadConfigured()) return notConfiguredResponse();

  const raw = await request.json().catch(() => null);
  const parsed = createTicketSchema.safeParse(raw);
  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  const { title, body, customer_email, group, priority } = parsed.data;
  const response = await fetch(`${ZAMMAD_URL}/api/v1/tickets`, {
    method: "POST",
    headers: zammadHeaders(),
    cache: "no-store",
    body: JSON.stringify({
      title,
      group: group || "Users",
      priority: priority || "2 normal",
      customer: customer_email,
      article: {
        subject: title,
        body,
        type: "note",
        internal: false,
        content_type: "text/html",
      },
    }),
  });

  const payload = await jsonOrNull<unknown>(response);
  return Response.json(payload ?? { error: "ticket_create_failed" }, { status: response.status });
}
