"use client";

/**
 * Client-side network layer for the interactive onboarding steps. Every call
 * targets a same-origin Next.js BFF route (src/app/api/**) which proxies to the
 * real plane service with the internal key + org scope. Functions here own the
 * wire shapes and graceful degradation; React Query owns caching/polling.
 */

import { consumeSse, parseEventJson, type SseEvent } from "@/lib/net/sse";
import type {
  ConnectorPick,
  OnboardingPlanId,
  OrganizationPayload,
  PlanRecommendation,
  WebsitePayload,
} from "./onboarding-machine";

/* ------------------------------------------------------------------ graph */

export interface PreviewNode {
  id: string;
  label: string;
  group: string;
}

export interface PreviewEdge {
  a: string;
  b: string;
}

export interface PreviewResponse {
  nodes: PreviewNode[];
  edges: PreviewEdge[];
  counts: { nodes: number; edges: number; groups: number };
  warning?: string;
}

const EMPTY_PREVIEW: PreviewResponse = {
  nodes: [],
  edges: [],
  counts: { nodes: 0, edges: 0, groups: 0 },
};

/** GET /api/onboarding/graph-preview → graph-index-rs `/v1/graphs/{org_id}`. */
export async function fetchGraphPreview(signal?: AbortSignal): Promise<PreviewResponse> {
  try {
    const response = await fetch("/api/onboarding/graph-preview", {
      method: "GET",
      credentials: "include",
      cache: "no-store",
      signal,
    });
    if (!response.ok) return EMPTY_PREVIEW;
    return (await response.json()) as PreviewResponse;
  } catch {
    // Keep the last rendered graph; never throw into the polling loop.
    return EMPTY_PREVIEW;
  }
}

/* ----------------------------------------------------------- crawl preview */

export type CrawlSnippetKind = "text" | "image" | "file" | "link";

export interface CrawlSnippet {
  id: string;
  kind: CrawlSnippetKind;
  title: string;
  excerpt?: string;
  thumbUrl?: string;
  url: string;
  contentType?: string;
}

export interface CrawlPreviewHandlers {
  onSnippet?: (snippet: CrawlSnippet) => void;
  onBranding?: (payload: unknown) => void;
  onWarning?: (warning: { code?: string; message?: string }) => void;
  onDone?: (done: { count?: number }) => void;
}

/**
 * POST /api/onboarding/crawl-preview (SSE). The BFF composes quarry-edge's
 * single-page scrape stream (instant snippets/branding for the seed page) with
 * the fire-and-forget crawl handoff + run-events relay. Resolves when the
 * stream ends; callers add their own safety-advance timer.
 */
export async function streamCrawlPreview(
  input: { url: string; brief?: string; maxPages?: number },
  handlers: CrawlPreviewHandlers,
  signal?: AbortSignal,
): Promise<void> {
  await consumeSse("/api/onboarding/crawl-preview", {
    method: "POST",
    body: JSON.stringify({
      url: input.url,
      brief: input.brief || undefined,
      maxPages: input.maxPages ?? 8,
    }),
    signal,
    onEvent: (event: SseEvent) => {
      switch (event.event) {
        case "snippet": {
          const snippet = parseEventJson<CrawlSnippet>(event);
          if (snippet) handlers.onSnippet?.(snippet);
          break;
        }
        case "branding": {
          const payload = parseEventJson<unknown>(event);
          if (payload) handlers.onBranding?.(payload);
          break;
        }
        case "warning": {
          const warning = parseEventJson<{ code?: string; message?: string }>(event);
          if (warning) handlers.onWarning?.(warning);
          break;
        }
        case "done": {
          const done = parseEventJson<{ count?: number }>(event);
          handlers.onDone?.(done ?? {});
          break;
        }
      }
    },
  });
}

/* ------------------------------------------------------------- connections */

export interface ConnectSessionResult {
  /** Nango Connect link to embed. */
  connectUrl: string;
  sessionToken?: string;
  expiresAt?: string;
}

/** POST /api/connections/create → integration-core connect-session. */
export async function createConnectSession(input: {
  provider: string;
  sources: string[];
}): Promise<ConnectSessionResult> {
  const response = await fetch("/api/connections/create", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify(input),
  });
  const body = (await response.json().catch(() => null)) as
    | (ConnectSessionResult & { error?: string })
    | { error?: string }
    | null;
  if (!response.ok || !body || !("connectUrl" in body) || !body.connectUrl) {
    const message = (body && "error" in body && body.error) || "Could not start the connection.";
    throw new Error(message);
  }
  return body;
}

/* ----------------------------------------------------------- recommendation */

export interface OnboardingContext {
  organization?: Pick<OrganizationPayload, "name" | "size" | "employeeCount">;
  website?: Pick<WebsitePayload, "url" | "agentBrief">;
  connectors: Array<Pick<ConnectorPick, "id" | "label">>;
  sourceCount: number;
  locale: "nb" | "en";
}

/**
 * POST /api/onboarding/recommend-plan → Model Plane `POST /v1/recommend/plan`.
 * The caller renders the instant local-engine result first and swaps in this
 * authoritative recommendation when it resolves; on error the local result
 * stands (the route itself also falls back, so this throwing is rare).
 */
export async function recommendPlan(
  context: OnboardingContext,
  signal?: AbortSignal,
): Promise<PlanRecommendation & { planId: OnboardingPlanId }> {
  const response = await fetch("/api/onboarding/recommend-plan", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify({ context }),
    signal,
  });
  if (!response.ok) throw new Error(`recommend-plan ${response.status}`);
  const body = (await response.json()) as { recommendation?: PlanRecommendation };
  if (!body.recommendation) throw new Error("recommend-plan: empty recommendation");
  return body.recommendation as PlanRecommendation & { planId: OnboardingPlanId };
}
