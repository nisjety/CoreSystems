import { handleVitalsExport } from "../../../../lib/web-vitals/handler";

export const runtime = "nodejs";
export const maxDuration = 15;
export function GET(request: Request) {
  return handleVitalsExport(request);
}
export function POST() {
  return new Response(null, {
    status: 405,
    headers: { Allow: "GET", "Cache-Control": "no-store" },
  });
}
