"use client";

/**
 * Client-side network layer for the interactive onboarding steps. Every call
 * targets a same-origin Next.js BFF route (src/app/api/**) which proxies to the
 * real plane service with the internal key + org scope. Functions here own the
 * wire shapes and graceful degradation; React Query owns caching/polling.
 */

import { consumeSse, parseEventJson, type SseEvent } from "@/lib/net/sse";
import type {
  BrandingSignals,
  ConnectorPick,
  CrawlEvidence,
  OnboardingPlanId,
  OnboardingBrandTheme,
  OrganizationPayload,
  PlanRecommendation,
  SafeConnectorMetadata,
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
  source?: "seed" | "live";
  elementCount?: number;
}

export interface CrawlProgress {
  status: "starting" | "running" | "completed" | "failed" | "cancelled";
  pages: number;
  elements: number;
  target?: number;
  jobId?: string;
  latestUrl?: string;
  latestTitle?: string;
}

export interface CrawlPreviewHandlers {
  onStarted?: (started: { jobId?: string; url?: string; target?: number }) => void;
  onSnippet?: (snippet: CrawlSnippet) => void;
  onProgress?: (progress: CrawlProgress) => void;
  onBranding?: (payload: unknown) => void;
  onWarning?: (warning: { code?: string; message?: string }) => void;
  onDone?: (done: { count?: number; pages?: number; elements?: number; status?: string }) => void;
}

/**
 * POST /api/onboarding/crawl-preview (SSE). The BFF composes quarry-edge's
 * single-page scrape stream (instant snippets/branding for the seed page) with
 * quarry-control crawl job event polling. Resolves when the
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
        case "started": {
          const started = parseEventJson<{ jobId?: string; url?: string; target?: number }>(event);
          if (started) handlers.onStarted?.(started);
          break;
        }
        case "snippet": {
          const snippet = parseEventJson<CrawlSnippet>(event);
          if (snippet) handlers.onSnippet?.(snippet);
          break;
        }
        case "progress": {
          const progress = parseEventJson<CrawlProgress>(event);
          if (progress) handlers.onProgress?.(progress);
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
          const done = parseEventJson<{ count?: number; pages?: number; elements?: number; status?: string }>(event);
          handlers.onDone?.(done ?? {});
          break;
        }
      }
    },
  });
}

/* ------------------------------------------------------------- connections */

export interface ConnectSessionResult {
  /** Provider authorization URL returned by integration-corev2. */
  connectUrl: string;
  /** Provider config key used by older callers; direct OAuth does not require this client-side. */
  providerConfigKey?: string;
  authMode?: "direct-oauth";
  sessionToken?: string;
  expiresAt?: string;
}

export interface ConnectorDiscoveryResult {
  metadata: SafeConnectorMetadata;
  graphSeedStatus: "pending" | "ready" | "failed";
  graphSeedDocumentId?: string;
}

export interface CleanupOnboardingSourceResult {
  cleanupStatus: "skipped" | "completed" | "pending" | "failed";
}

export interface DisconnectConnectionsResult {
  disconnected: Array<{ id: string; providerKey: string }>;
  failed?: Array<{ id: string; providerKey: string; status: number; message: string }>;
  skippedProviderKeys?: string[];
}

/**
 * Extract a human-readable message from a BFF error body, which may be either
 * `{ error: "string" }` (explicit route returns) or `{ error: { code, message } }`
 * (the control-plane auth error envelope). Without this, `String(object)` leaks
 * `"[object Object]"` into the UI.
 */
function errorMessageFrom(body: unknown, fallback: string): string {
  if (body && typeof body === "object" && "error" in body) {
    const error = (body as { error?: unknown }).error;
    if (typeof error === "string" && error.trim()) return error;
    if (error && typeof error === "object" && "message" in error) {
      const message = (error as { message?: unknown }).message;
      if (typeof message === "string" && message.trim()) return message;
    }
  }
  return fallback;
}

/**
 * Thrown when connector setup can't run because the integration service is
 * unreachable/unconfigured (network error or gateway 502/503/504) — as opposed
 * to a genuine per-connector failure. The step renders a calm "unavailable in
 * this environment" state for this rather than a red error.
 */
export class ConnectUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ConnectUnavailableError";
  }
}

const GATEWAY_DOWN_STATUSES = new Set([502, 503, 504]);

function unwrapConnectSessionResult(body: unknown): ConnectSessionResult | null {
  if (!body || typeof body !== "object") return null;
  const record = body as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? (record.data as Record<string, unknown>)
    : record;
  const connectUrl = typeof data.connectUrl === "string" ? data.connectUrl.trim() : "";
  if (!connectUrl) return null;

  return {
    connectUrl,
    authMode: data.authMode === "direct-oauth" ? "direct-oauth" : undefined,
    sessionToken: typeof data.sessionToken === "string" ? data.sessionToken : undefined,
    expiresAt: typeof data.expiresAt === "string" ? data.expiresAt : undefined,
    providerConfigKey: typeof data.providerConfigKey === "string" ? data.providerConfigKey : undefined,
  };
}

/** POST /api/v1/integrations/providers/{provider}/connect-session → integration-core connect-session. */
export async function createConnectSession(input: {
  provider: string;
  sources: string[];
  shop?: string;
  providerContext?: Record<string, string>;
}): Promise<ConnectSessionResult> {
  let response: Response;
  try {
    response = await fetch(`/api/v1/integrations/providers/${encodeURIComponent(input.provider)}/connect-session`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      credentials: "include",
      body: JSON.stringify({
        selectedSources: input.sources,
        shop: input.shop,
        providerContext: input.providerContext,
        bundles: ["onboarding"],
      }),
    });
  } catch {
    throw new ConnectUnavailableError("Could not reach the integration service.");
  }
  const body = await response.json().catch(() => null);
  const session = unwrapConnectSessionResult(body);
  if (!response.ok || !session) {
    const message = errorMessageFrom(body, "Could not start the connection.");
    if (GATEWAY_DOWN_STATUSES.has(response.status)) {
      throw new ConnectUnavailableError(message);
    }
    throw new Error(message);
  }
  return session;
}

export interface IntegrationSummaryConnection {
  id: string;
  providerKey: string;
  providerLabel: string;
  status: string;
  syncStatus: string;
  latestSyncJob?: { id: string; status: string; updatedAt?: string };
}

export interface IntegrationSummaryResult {
  connections: IntegrationSummaryConnection[];
}

export async function fetchIntegrationSummary(signal?: AbortSignal): Promise<IntegrationSummaryResult> {
  const response = await fetch("/api/v1/integrations?discovery=1&graph=1", {
    method: "GET",
    credentials: "include",
    cache: "no-store",
    signal,
  });
  if (!response.ok) return { connections: [] };
  const body = (await response.json().catch(() => null)) as
    | { data?: IntegrationSummaryResult }
    | IntegrationSummaryResult
    | null;
  if (!body) return { connections: [] };
  if ("data" in body) return body.data ?? { connections: [] };
  if ("connections" in body) return body;
  return { connections: [] };
}

export async function startIntegrationSync(
  connectionId: string,
  options?: {
    checkpoint?: Record<string, unknown>;
    metadata?: Record<string, unknown>;
  },
): Promise<{ syncJob?: { id?: string; status?: string } }> {
  const response = await fetch("/api/v1/integrations/sync-jobs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    cache: "no-store",
    body: JSON.stringify({
      connectionId,
      reason: "onboarding_connected",
      mode: "incremental",
      checkpoint: options?.checkpoint,
      metadata: options?.metadata,
    }),
  });
  if (!response.ok) return {};
  const body = (await response.json().catch(() => null)) as
    | { data?: { syncJob?: { id?: string; status?: string } } }
    | { syncJob?: { id?: string; status?: string } }
    | null;
  if (!body) return {};
  const envelopeData = (body as { data?: { syncJob?: { id?: string; status?: string } } }).data;
  if (envelopeData) return envelopeData;
  const syncJob = (body as { syncJob?: { id?: string; status?: string } }).syncJob;
  return syncJob ? { syncJob } : {};
}

/** POST /api/connections/disconnect → integration-core connection delete. */
export async function disconnectConnections(
  connectors: string[],
  signal?: AbortSignal,
): Promise<DisconnectConnectionsResult> {
  if (connectors.length === 0) {
    return { disconnected: [], failed: [], skippedProviderKeys: [] };
  }
  const response = await fetch("/api/connections/disconnect", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    cache: "no-store",
    keepalive: true,
    signal,
    body: JSON.stringify({ connectors }),
  });
  const body = (await response.json().catch(() => null)) as
    | (DisconnectConnectionsResult & { error?: unknown })
    | { error?: unknown }
    | null;
  if (!response.ok || !body || !("disconnected" in body)) {
    throw new Error(errorMessageFrom(body, "Could not disconnect the selected sources."));
  }
  return body;
}

/** Best-effort finspo-core warmup after Microsoft OAuth succeeds. */
export async function warmSharePointDiscovery(orgId?: string): Promise<void> {
  await fetch("/api/onboarding/sharepoint-discovery", {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ orgId }),
  }).catch(() => undefined);
}

/** POST /api/onboarding/source-discovery → safe, high-level integration metadata. */
export async function discoverConnectorSource(input: {
  provider: string;
  connectorId: string;
  label: string;
  sources: string[];
  orgId?: string;
}): Promise<ConnectorDiscoveryResult> {
  const response = await fetch("/api/onboarding/source-discovery", {
    method: "POST",
    credentials: "include",
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  const body = (await response.json().catch(() => null)) as ConnectorDiscoveryResult | { error?: unknown } | null;
  if (!response.ok || !body || !("metadata" in body)) {
    return {
      metadata: {
        status: "failed",
        workspaceName: input.label,
        sensitivity: "safe_metadata_only",
        discoveredAt: new Date().toISOString(),
      },
      graphSeedStatus: "failed",
    };
  }
  return body;
}

/** POST /api/onboarding/source-cleanup → best-effort Data Plane onboarding seed cleanup. */
export async function cleanupOnboardingSource(input: {
  connectorId?: string;
  documentId?: string;
  source?: string;
}): Promise<CleanupOnboardingSourceResult> {
  const response = await fetch("/api/onboarding/source-cleanup", {
    method: "POST",
    credentials: "include",
    cache: "no-store",
    keepalive: true,
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  const body = (await response.json().catch(() => null)) as CleanupOnboardingSourceResult | null;
  if (!response.ok || !body?.cleanupStatus) return { cleanupStatus: "failed" };
  return body;
}

/** Persist the explicit onboarding theme choice as a workspace appearance hint. */
export async function saveOnboardingBrandTheme(input: {
  mode: OnboardingBrandTheme["mode"];
  primaryColor: string;
}): Promise<{ persisted: boolean }> {
  const response = await fetch("/api/onboarding/brand-theme", {
    method: "PUT",
    credentials: "include",
    cache: "no-store",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
  });
  if (!response.ok) return { persisted: false };
  const body = (await response.json().catch(() => null)) as
    | { persisted?: boolean; data?: { persisted?: boolean } }
    | null;
  return { persisted: (body?.data?.persisted ?? body?.persisted) !== false };
}

/* ----------------------------------------------------------- recommendation */

export interface OnboardingContext {
  organization?: Pick<OrganizationPayload, "name" | "size" | "employeeCount">;
  website?: Pick<WebsitePayload, "url" | "agentBrief">;
  websites?: Array<Pick<WebsitePayload, "url" | "agentBrief"> & { crawlEvidence?: CrawlEvidence }>;
  brand?: {
    logoUrl?: string;
    primaryColor?: string;
    palette?: string[];
    themeMode?: OnboardingBrandTheme["mode"];
    branding?: Pick<BrandingSignals, "siteName" | "themeColor" | "favicon" | "logoCandidate">;
  };
  connectors: Array<Pick<ConnectorPick, "id" | "label">>;
  integrationEvidence?: Array<{
    provider: string;
    label: string;
    status: SafeConnectorMetadata["status"];
    metadata?: SafeConnectorMetadata;
  }>;
  sourceCount: number;
  locale: "nb" | "en";
  dataPlane?: OnboardingDataPlaneContext;
}

export interface OnboardingDataPlaneContext {
  nodeCount: number;
  edgeCount: number;
  groups: string[];
  sampleNodes: Array<{ label: string; group: string }>;
  sampleEdges: Array<{ from: string; to: string }>;
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
