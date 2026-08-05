import { NextResponse } from "next/server";
import { ok } from "@/lib/api/envelope";

export const runtime = "nodejs";

export function GET() {
  return NextResponse.json(
    ok({
      service: "verevonv2",
      status: "ok",
      time: new Date().toISOString(),
      checks: {
        appRouter: true,
        typedContracts: true,
        authDatabaseConfigured: Boolean(process.env.DATABASE_URL),
      },
    }),
  );
}
