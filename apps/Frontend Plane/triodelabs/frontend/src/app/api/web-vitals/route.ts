import { handleVitalsPost } from "../../../lib/web-vitals/handler";

export const runtime = "nodejs";
export const maxDuration = 15;
export function POST(request: Request) {
  return handleVitalsPost(request);
}
export function GET() {
  return new Response(null, {
    status: 405,
    headers: { Allow: "POST", "Cache-Control": "no-store" },
  });
}
