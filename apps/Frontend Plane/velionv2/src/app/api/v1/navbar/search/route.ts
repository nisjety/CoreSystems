import { NextRequest, NextResponse } from "next/server";
import { fail, ok } from "@/lib/api/envelope";
import { isReadIntegrationUnavailable } from "@/lib/integrations/optional-service";
import { RequestActorError, requireRequestActor } from "@/lib/integrations/request-actor";
import { searchSignedInUserDatabase } from "@/lib/integrations/search";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: NextRequest) {
  const query = request.nextUrl.searchParams.get("q")?.trim() ?? "";
  const scope = "knowledge" as const;

  if (query.length < 2) {
    return NextResponse.json(ok({ query, results: [], scope }));
  }

  try {
    const actor = await requireRequestActor();
    const results = await searchSignedInUserDatabase(actor, query, { scope });
    return NextResponse.json(ok({ query, results, scope }));
  } catch (error) {
    if (isReadIntegrationUnavailable(error)) {
      return NextResponse.json(ok({ configured: false, query, results: [], scope }));
    }

    if (error instanceof RequestActorError) {
      return NextResponse.json(fail({ code: error.code, message: error.message }), { status: error.status });
    }
    return NextResponse.json(
      fail({ code: "knowledge_search_failed", message: error instanceof Error ? error.message : "Knowledge search could not be completed." }),
      { status: 500 },
    );
  }
}
