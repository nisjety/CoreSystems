import { NextRequest, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import {
  AutocompleteCoreError,
  fetchAutocompleteSuggestions,
} from "@/lib/integrations/autocomplete-core";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: NextRequest) {
  const query = request.nextUrl.searchParams.get("q")?.trim() ?? "";
  const scope = parseScope(request.nextUrl.searchParams.get("scope"));
  const limit = parseLimit(request.nextUrl.searchParams.get("limit"));

  if (query.length < 2) {
    return NextResponse.json(ok({ configured: false, query, scope, suggestions: [] }));
  }

  try {
    const actor = await requireRequestActor();
    const data = await fetchAutocompleteSuggestions(actor, { query, scope, limit });
    return NextResponse.json(ok(data));
  } catch (error) {
    if (error instanceof RequestActorError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }

    if (error instanceof AutocompleteCoreError) {
      if (error.status === 502 || error.status === 503) {
        return NextResponse.json(ok({ configured: false, query, scope, suggestions: [] }));
      }

      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }

    return NextResponse.json(
      fail({ code: "autocomplete_failed", message: "Suggestions could not be loaded." }),
      { status: 500 },
    );
  }
}

function parseScope(value: string | null): "all" | "queries" | "hosts" | "titles" {
  if (value === "all" || value === "queries" || value === "hosts" || value === "titles") {
    return value;
  }

  return "queries";
}

function parseLimit(value: string | null) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return 8;
  }

  return Math.max(1, Math.min(Math.trunc(parsed), 20));
}
