import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

export async function GET() {
  if (!zammadConfigured()) return notConfiguredResponse();

  const response = await fetch(`${ZAMMAD_URL}/api/v1/macros`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<unknown>(response);

  return Response.json(payload ?? [], { status: response.status });
}
