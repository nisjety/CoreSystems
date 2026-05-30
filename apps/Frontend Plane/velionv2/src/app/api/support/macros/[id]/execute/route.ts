import { NextRequest } from "next/server";
import { z } from "zod";
import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

const executeSchema = z.object({
  ticketId: z.number(),
});

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  if (!zammadConfigured()) return notConfiguredResponse();
  const [{ id }, raw] = await Promise.all([
    params,
    request.json().catch(() => null),
  ]);
  const parsed = executeSchema.safeParse(raw);

  if (!parsed.success) {
    return Response.json({ error: parsed.error.flatten() }, { status: 422 });
  }

  const response = await fetch(`${ZAMMAD_URL}/api/v1/macros/${encodeURIComponent(id)}/execute`, {
    method: "POST",
    headers: zammadHeaders(),
    cache: "no-store",
    body: JSON.stringify({ ticket_id: parsed.data.ticketId }),
  });
  const payload = await jsonOrNull<unknown>(response);

  return Response.json(payload ?? { ok: response.ok }, { status: response.status });
}
