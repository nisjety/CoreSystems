import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type ZammadGroup = {
  id: number;
  name: string;
};

export async function GET() {
  if (!zammadConfigured()) return notConfiguredResponse();

  const response = await fetch(`${ZAMMAD_URL}/api/v1/groups`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<ZammadGroup[]>(response);

  if (!response.ok) {
    return Response.json(payload ?? { error: "groups_fetch_failed" }, { status: response.status });
  }

  return Response.json((payload ?? []).map(({ id, name }) => ({ id, name })));
}
