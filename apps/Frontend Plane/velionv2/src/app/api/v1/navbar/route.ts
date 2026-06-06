import { NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { loadNavbarPayload, resolveNavbarPayloadError } from "@/features/shell-v2/lib/navbar-server";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET() {
  try {
    const payload = await loadNavbarPayload();
    return NextResponse.json(ok(payload));
  } catch (error) {
    const resolved = resolveNavbarPayloadError(error);
    return NextResponse.json(
      fail({ code: resolved.code, message: resolved.message }),
      { status: resolved.status },
    );
  }
}
