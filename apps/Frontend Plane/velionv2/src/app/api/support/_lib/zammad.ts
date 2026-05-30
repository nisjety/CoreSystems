export const ZAMMAD_URL = process.env.ZAMMAD_API_URL || "http://zammad-railsserver:3000";
const ZAMMAD_TOKEN = process.env.ZAMMAD_API_TOKEN || "";

export function zammadConfigured() {
  return ZAMMAD_TOKEN.length > 0;
}

export function zammadHeaders(): Record<string, string> {
  return {
    Authorization: `Token token=${ZAMMAD_TOKEN}`,
    "Content-Type": "application/json",
  };
}

export function notConfiguredResponse(): Response {
  return Response.json(
    {
      error: "support_not_configured",
      message: "The support integration is not configured. Set ZAMMAD_API_URL and ZAMMAD_API_TOKEN to enable the inbox.",
    },
    { status: 503 },
  );
}

export async function jsonOrNull<T>(response: Response): Promise<T | null> {
  return response.json().catch(() => null) as Promise<T | null>;
}
