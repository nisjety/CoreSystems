import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  getInternalApiKey,
  getCorrelationId,
  requireSession,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getGraphIndexUrl,
  getModelPlaneAudience,
  getModelPlaneRecommendUrl,
  mintAudienceToken,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { ONBOARDING_COPY } from "@/features/onboarding-v2/lib/onboarding-i18n";
import { buildLocalRecommendation } from "@/features/onboarding-v2/lib/onboarding-recommendation";
import type { OnboardingContext } from "@/features/onboarding-v2/lib/onboarding-api";
import type { OnboardingPlanId, PlanRecommendation } from "@/features/onboarding-v2/lib/onboarding-machine";

export const dynamic = "force-dynamic";

const VALID_PLANS = new Set<OnboardingPlanId>(["trial", "hobby", "standard", "pro", "enterprise"]);

interface GraphEntity {
  entity_id: string;
  entity_type?: string;
  entity_text?: string;
}

interface GraphRelationship {
  entity_a_id: string;
  entity_b_id: string;
}

interface GraphIndexResponse {
  nodes?: GraphEntity[];
  edges?: GraphRelationship[];
  node_count?: number;
  edge_count?: number;
}

type DataPlaneContext = NonNullable<OnboardingContext["dataPlane"]>;

/**
 * POST /api/onboarding/recommend-plan
 * Body: { context }. Asks the Model Plane recommender (when configured) and
 * falls back to the deterministic local engine — so the client always receives
 * a valid recommendation and is never blocked on Model Plane availability.
 */
export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as { context?: OnboardingContext } | null;
    const context = body?.context;
    if (!context) {
      return NextResponse.json({ error: "Missing onboarding context." }, { status: 400 });
    }

    const locale = context.locale === "en" ? "en" : "nb";
    const local = buildLocalRecommendation({
      organization: context.organization
        ? {
            name: context.organization.name ?? "",
            size: context.organization.size,
            employeeCount: context.organization.employeeCount,
          }
        : undefined,
      website: context.website ? { url: context.website.url ?? "", agentBrief: context.website.agentBrief ?? "" } : undefined,
      additionalWebsites: (context.websites ?? [])
        .filter((site) => site.url && site.url !== context.website?.url)
        .map((site) => ({ url: site.url, agentBrief: site.agentBrief })),
      connectors: (context.connectors ?? []).map((c) => ({ id: c.id, label: c.label })),
      reasonPrefix: ONBOARDING_COPY[locale].paywall.reasonPrefix,
      locale,
    });

    const enrichedContext = await enrichRecommendationContext(request, session, context);
    const localWithEvidence = withDataPlaneEvidence(local, enrichedContext);
    const remote = await fetchModelPlaneRecommendation(request, session.user.id, enrichedContext);
    return NextResponse.json({ recommendation: withDataPlaneEvidence(remote ?? localWithEvidence, enrichedContext) });
  } catch (error) {
    return authErrorResponse(error);
  }
}

async function enrichRecommendationContext(
  request: NextRequest,
  session: ControlPlaneSession,
  context: OnboardingContext,
): Promise<OnboardingContext> {
  const dataPlane = await fetchDataPlaneContext(request, session);
  if (!dataPlane) return context;
  return { ...context, dataPlane };
}

async function fetchDataPlaneContext(
  request: NextRequest,
  session: ControlPlaneSession,
): Promise<DataPlaneContext | null> {
  const orgId = await resolveActiveOrgId(request, session);
  if (!orgId) return null;

  try {
    const response = await fetch(
      `${getGraphIndexUrl()}/v1/graphs/${encodeURIComponent(orgId)}?limit_nodes=120&limit_edges=240`,
      {
        method: "GET",
        headers: buildServiceHeaders(request, session, orgId),
        cache: "no-store",
        signal: AbortSignal.timeout(1_200),
      },
    );
    if (!response.ok) return null;
    const body = (await response.json().catch(() => null)) as GraphIndexResponse | null;
    if (!body) return null;

    const nodes = (body.nodes ?? [])
      .filter((entity) => entity.entity_id)
      .map((entity) => ({
        id: entity.entity_id,
        label: safeGraphText(entity.entity_text || entity.entity_id, 80),
        group: safeGraphText((entity.entity_type || "entity").toLowerCase(), 32),
      }));
    const nodeMap = new Map(nodes.map((node) => [node.id, node]));
    const groups = Array.from(new Set(nodes.map((node) => node.group).filter((group) => group !== "org"))).slice(0, 8);
    const sampleNodes = nodes
      .filter((node) => node.group !== "org")
      .slice(0, 14)
      .map((node) => ({ label: node.label, group: node.group }));
    const sampleEdges = (body.edges ?? [])
      .map((edge) => {
        const from = nodeMap.get(edge.entity_a_id);
        const to = nodeMap.get(edge.entity_b_id);
        return from && to ? { from: from.label, to: to.label } : null;
      })
      .filter((edge): edge is { from: string; to: string } => Boolean(edge))
      .slice(0, 12);

    return {
      nodeCount: body.node_count ?? nodes.length,
      edgeCount: body.edge_count ?? body.edges?.length ?? sampleEdges.length,
      groups,
      sampleNodes,
      sampleEdges,
    };
  } catch {
    return null;
  }
}

async function fetchModelPlaneRecommendation(
  request: NextRequest,
  userId: string,
  context: OnboardingContext,
): Promise<PlanRecommendation | null> {
  const url = getModelPlaneRecommendUrl();
  if (!url) return null;
  try {
    // The model-gateway authenticates with an auth-core JWT (audience
    // "model-plane"); mint one and send it as Bearer. The internal key is kept
    // for correlation/observability.
    const token = await mintAudienceToken(request, getModelPlaneAudience());
    const response = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Api-Key": getInternalApiKey(),
        "X-User-Id": userId,
        "X-Correlation-Id": getCorrelationId(request),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      cache: "no-store",
      body: JSON.stringify({ context, locale: context.locale }),
      // Bound the wait so a slow model never stalls the paywall.
      signal: AbortSignal.timeout(3_000),
    });
    if (!response.ok) return null;
    const raw = (await response.json().catch(() => null)) as
      | { recommendation?: Partial<PlanRecommendation>; planId?: OnboardingPlanId }
      | null;
    const candidate: Partial<PlanRecommendation> = (raw?.recommendation ?? raw ?? {}) as Partial<PlanRecommendation>;
    const planId = candidate.planId;
    if (!planId || !VALID_PLANS.has(planId)) return null;
    const reason = usefulModelText(candidate.reason);
    const summary = usefulModelText(candidate.summary);
    if (!reason || !summary) return null;
    return {
      planId,
      reason,
      summary,
      proofPoints: usefulModelList(candidate.proofPoints),
      scopeSignals: usefulModelList(candidate.scopeSignals),
      opportunities: usefulModelList(candidate.opportunities),
      expectedOutcomes: usefulExpectedOutcomes(candidate.expectedOutcomes),
      proofOfConcept: usefulProofOfConcept(candidate.proofOfConcept),
      generatedAt: candidate?.generatedAt || new Date().toISOString(),
      source: "model",
      modelVersion: typeof candidate.modelVersion === "string" ? candidate.modelVersion : undefined,
      confidence: typeof candidate.confidence === "number" ? candidate.confidence : undefined,
    };
  } catch {
    return null;
  }
}

function withDataPlaneEvidence(
  recommendation: PlanRecommendation,
  context: OnboardingContext,
): PlanRecommendation {
  const dataPlane = context.dataPlane;
  if (!dataPlane || dataPlane.nodeCount <= 0) return recommendation;
  const locale = context.locale === "en" ? "en" : "nb";
  const graphProof =
    locale === "nb"
      ? [
          `Kunnskapsgrafen har ${dataPlane.nodeCount} noder og ${dataPlane.edgeCount} relasjoner fra onboarding-kildene.`,
          dataPlane.groups.length > 0 ? `Vi ser klynger rundt ${humanList(dataPlane.groups, locale)}.` : null,
        ]
      : [
          `The knowledge graph has ${dataPlane.nodeCount} nodes and ${dataPlane.edgeCount} relationships from onboarding sources.`,
          dataPlane.groups.length > 0 ? `We can see clusters around ${humanList(dataPlane.groups, locale)}.` : null,
        ];
  const graphScope =
    locale === "nb"
      ? [
          dataPlane.sampleNodes.length > 0
            ? `Første grafutdrag inneholder ${humanList(dataPlane.sampleNodes.slice(0, 4).map((node) => node.label), locale)}.`
            : null,
        ]
      : [
          dataPlane.sampleNodes.length > 0
            ? `The first graph extract includes ${humanList(dataPlane.sampleNodes.slice(0, 4).map((node) => node.label), locale)}.`
            : null,
        ];
  const graphOpportunities =
    locale === "nb"
      ? [
          "Bruk grafen til å finne kunnskapshull før agenten får trafikk.",
          "Start med svar der nettstedet og interne kilder overlapper.",
        ]
      : [
          "Use the graph to find knowledge gaps before the agent receives traffic.",
          "Start with answers where the website and internal sources overlap.",
        ];

  return {
    ...recommendation,
    proofPoints: mergeLimited(graphProof, recommendation.proofPoints, 4),
    scopeSignals: mergeLimited(graphScope, recommendation.scopeSignals, 4),
    opportunities: mergeLimited(recommendation.opportunities, graphOpportunities, 4),
    expectedOutcomes: mergeExpectedOutcomes(
      buildGraphExpectedOutcomes(dataPlane, locale),
      recommendation.expectedOutcomes,
    ),
  };
}

function usefulModelText(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const text = value.trim();
  if (text.length < 36) return null;
  const lower = text.toLowerCase();
  const genericFragments = [
    "you can change plan",
    "select the plan",
    "choose the plan",
    "velg planen",
    "du kan endre plan",
    "ikke bare én statisk faq",
    "not just one static faq",
    "beskrev behovet som",
    "described the need as",
  ];
  if (text.length < 120 && genericFragments.some((fragment) => lower.includes(fragment))) {
    return null;
  }
  return text;
}

function usefulModelList(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const items = value
    .map((item) => safeModelText(item, 160, 8))
    .filter((item): item is string => Boolean(item))
    .slice(0, 4);
  return items.length > 0 ? items : undefined;
}

function usefulExpectedOutcomes(value: unknown): PlanRecommendation["expectedOutcomes"] | undefined {
  if (!Array.isArray(value)) return undefined;
  const outcomes = value
    .map((item) => {
      if (!item || typeof item !== "object") return null;
      const record = item as Record<string, unknown>;
      const label = safeModelText(record.label, 48, 2);
      const metricValue = safeModelText(record.value, 48, 1);
      if (!label || !metricValue) return null;
      const detail = safeModelText(record.detail, 140, 8) ?? undefined;
      return { label, value: metricValue, detail };
    })
    .filter((item): item is { label: string; value: string; detail: string | undefined } => Boolean(item))
    .slice(0, 3);
  return outcomes.length > 0 ? outcomes : undefined;
}

function usefulProofOfConcept(value: unknown): PlanRecommendation["proofOfConcept"] | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  const proof = {
    companyIdentity: usefulModelList(record.companyIdentity),
    learnedSignals: usefulModelList(record.learnedSignals),
    likelyIntents: usefulModelList(record.likelyIntents),
    nextActions: usefulModelList(record.nextActions),
    operationalImpact: usefulExpectedOutcomes(record.operationalImpact),
    recommendationFit: usefulModelList(record.recommendationFit),
  };
  return Object.values(proof).some((section) => Array.isArray(section) && section.length > 0)
    ? proof
    : undefined;
}

function safeModelText(value: unknown, maxLength: number, minLength: number): string | null {
  if (typeof value !== "string") return null;
  const text = value.replace(/\s+/g, " ").trim();
  if (text.length < minLength) return null;
  const lower = text.toLowerCase();
  const genericFragments = ["select the plan", "choose the plan", "velg planen", "beskrev behovet som"];
  if (genericFragments.some((fragment) => lower.includes(fragment))) return null;
  return text.length > maxLength ? `${text.slice(0, maxLength - 3).trim()}...` : text;
}

function safeGraphText(value: string, maxLength: number): string {
  const text = value.replace(/\s+/g, " ").trim();
  if (!text) return "entity";
  return text.length > maxLength ? `${text.slice(0, maxLength - 3).trim()}...` : text;
}

function buildGraphExpectedOutcomes(
  dataPlane: DataPlaneContext,
  locale: "nb" | "en",
): NonNullable<PlanRecommendation["expectedOutcomes"]> {
  if (locale === "nb") {
    return [
      {
        label: "Grafdekning",
        value: `${dataPlane.nodeCount} noder`,
        detail: `${dataPlane.edgeCount} relasjoner gir Velion et startkart over hva agenten kan svare fra.`,
      },
    ];
  }
  return [
    {
      label: "Graph coverage",
      value: `${dataPlane.nodeCount} nodes`,
      detail: `${dataPlane.edgeCount} relationships give Velion a starting map of what the agent can answer from.`,
    },
  ];
}

function mergeExpectedOutcomes(
  first?: PlanRecommendation["expectedOutcomes"],
  second?: PlanRecommendation["expectedOutcomes"],
): PlanRecommendation["expectedOutcomes"] | undefined {
  const merged: NonNullable<PlanRecommendation["expectedOutcomes"]> = [];
  for (const outcome of [...(first ?? []), ...(second ?? [])]) {
    if (!outcome.label || !outcome.value) continue;
    if (merged.some((item) => item.label.toLowerCase() === outcome.label.toLowerCase())) continue;
    merged.push(outcome);
    if (merged.length >= 3) break;
  }
  return merged.length > 0 ? merged : undefined;
}

function mergeLimited(
  first?: Array<string | null | undefined>,
  second?: Array<string | null | undefined>,
  limit = 4,
): string[] | undefined {
  const merged: string[] = [];
  for (const item of [...(first ?? []), ...(second ?? [])]) {
    const text = item?.replace(/\s+/g, " ").trim();
    if (!text || merged.includes(text)) continue;
    merged.push(text);
    if (merged.length >= limit) break;
  }
  return merged.length > 0 ? merged : undefined;
}

function humanList(items: string[], locale: "nb" | "en"): string {
  const unique = Array.from(new Set(items.filter(Boolean)));
  if (unique.length === 0) return "";
  if (unique.length === 1) return unique[0];
  const joiner = locale === "nb" ? "og" : "and";
  if (unique.length === 2) return `${unique[0]} ${joiner} ${unique[1]}`;
  return `${unique.slice(0, -1).join(", ")} ${joiner} ${unique[unique.length - 1]}`;
}
