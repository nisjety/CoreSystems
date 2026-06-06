import { NextResponse, type NextRequest } from "next/server";
import { authErrorResponse } from "@/app/api/_lib/control-plane-auth";
import { fetchQuarry } from "@/app/api/ingestions/_lib/quarry-ingestions";
import { ok } from "@/lib/api/envelope";
import {
  knowledgeSourcesFromSummary,
  loadDataPlaneDocumentSummaries,
  loadIntegrationSummary,
} from "@/lib/integrations/integration-corev2";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type QuarryPage<T> = { items: T[] };

type QuarrySource = {
  source_id: string;
  name: string;
  url: string;
  kind: string;
  status: string;
  created_at: string;
  updated_at: string;
  config?: Record<string, unknown>;
};

export async function GET(request: NextRequest) {
  try {
    const [summary, documents, quarrySources] = await Promise.all([
      loadIntegrationSummary(request, { includeDiscovery: true, includeGraph: true }),
      loadDataPlaneDocumentSummaries(request),
      fetchQuarry<QuarryPage<QuarrySource>>(request, "/v1/sources", {
        query: { limit: 100 },
        timeoutMs: 2_500,
      }).catch(() => ({ items: [] })),
    ]);

    return NextResponse.json(
      ok({
        generatedAt: summary.generatedAt,
        metrics: summary.metrics,
        graph: summary.graph,
        integrations: knowledgeSourcesFromSummary(summary, documents),
        quarrySources: quarrySources.items.map((source) => ({
          id: source.source_id,
          name: source.name,
          url: source.url,
          kind: source.kind,
          status: source.status,
          createdAt: source.created_at,
          updatedAt: source.updated_at,
          config: source.config ?? {},
        })),
      }),
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}
