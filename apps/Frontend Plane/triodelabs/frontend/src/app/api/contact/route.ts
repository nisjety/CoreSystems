import { getContactConfigStatus } from "../../../lib/contact/config";
import { handleContactRequest } from "../../../lib/contact/handler";

export const runtime = "nodejs";
export const maxDuration = 30;

export async function POST(request: Request): Promise<Response> {
  return handleContactRequest(request);
}

// Health check for the form setup. Reports presence only, never secret values.
export function GET(): Response {
  return Response.json(getContactConfigStatus(), {
    headers: { "Cache-Control": "no-store" },
  });
}
