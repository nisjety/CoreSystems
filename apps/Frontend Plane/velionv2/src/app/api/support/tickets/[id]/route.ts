import { NextRequest } from "next/server";
import { z } from "zod";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

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
  if (!zammadConfigured()) return notConfiguredResponse();
  const { id } = await params;

  const response = await fetch(`${ZAMMAD_URL}/api/v1/tickets/${encodeURIComponent(id)}?expand=true`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<unknown>(response);

  return Response.json(payload ?? { error: "ticket_fetch_failed" }, { status: response.status });
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse();
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = updateTicketSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  const response = await fetch(`${ZAMMAD_URL}/api/v1/tickets/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: zammadHeaders(),
    cache: "no-store",
    body: JSON.stringify(parsed.data),
  });
  const payload = await jsonOrNull<unknown>(response);

  return Response.json(payload ?? { error: "ticket_update_failed" }, { status: response.status });
}
