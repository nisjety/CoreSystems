import { jsonOrNull, notConfiguredResponse, ZAMMAD_URL, zammadConfigured, zammadHeaders } from "@/app/api/support/_lib/zammad";

type ZammadUser = {
  id: number;
  firstname: string;
  lastname: string;
  email: string;
};

export async function GET() {
  if (!zammadConfigured()) return notConfiguredResponse();

  const response = await fetch(`${ZAMMAD_URL}/api/v1/users?role=Agent`, {
    headers: zammadHeaders(),
    cache: "no-store",
  });
  const payload = await jsonOrNull<ZammadUser[]>(response);

  if (!response.ok) {
    return Response.json(payload ?? { error: "agents_fetch_failed" }, { status: response.status });
  }

  return Response.json(
    (payload ?? []).map(({ id, firstname, lastname, email }) => ({ id, firstname, lastname, email })),
  );
}
