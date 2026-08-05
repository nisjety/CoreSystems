import "server-only";

import type { NextRequest } from "next/server";

import { fetchQuarry } from "@/app/api/ingestions/_lib/quarry-ingestions";
import {
  getInternalApiKey,
  requireSession,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getFinspoCoreUrl,
  getGraphIndexUrl,
  getImportsCoreUrl,
  getIntegrationCoreUrl,
  getKnowledgeRetrievalUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { buildIntegrationCoreHeaders } from "@/app/api/v1/integrations/_lib/service-auth";
import type {
  LiveKnowledgeCollection,
  LiveKnowledgeFile,
  LiveKnowledgeFolder,
  LiveKnowledgeGraph,
  LiveKnowledgeGraphLink,
  LiveKnowledgeGraphNode,
  LiveKnowledgeIntegration,
  LiveKnowledgeMetric,
  LiveKnowledgePayload,
  LiveKnowledgeSource,
  LiveKnowledgeSourceStatus,
  LiveKnowledgeSourceType,
  LiveKnowledgeWebSource,
} from "@/features/knowledge-v2/lib/knowledge-live";
import {
  loadDataPlaneDocumentSummaries,
  loadIntegrationSummary,
  type DataPlaneDocumentSummary,
  type IntegrationConnectionSummary,
} from "@/lib/integrations/integration-corev2";
import { loadKnowledgeDiagnostics } from "@/lib/knowledge/knowledge-diagnostics";

type GraphEntity = {
  entity_id?: unknown;
  entity_type?: unknown;
  entity_text?: unknown;
  source_refs?: unknown;
};

type GraphRelationship = {
  entity_a_id?: unknown;
  entity_b_id?: unknown;
  relation_type?: unknown;
  confidence?: unknown;
  source_refs?: unknown;
};

type GraphSnapshotResponse = {
  nodes?: GraphEntity[];
  edges?: GraphRelationship[];
  node_count?: unknown;
  edge_count?: unknown;
  truncated?: unknown;
};

type RetrievalChunk = {
  knowledge_id?: unknown;
  document_id?: unknown;
  text?: unknown;
  chunk_index?: unknown;
  content_hash?: unknown;
};

type RetrievalChunksResponse = {
  chunks?: RetrievalChunk[];
  count?: unknown;
};

type FreshnessRow = {
  document_id?: unknown;
  freshness_score?: unknown;
  age_days?: unknown;
};

type FreshnessResponse = {
  freshness?: FreshnessRow[];
};

type FinspoSource = {
  id?: unknown;
  site_id?: unknown;
  drive_id?: unknown;
  drive_name?: unknown;
  drive_type?: unknown;
  enabled?: unknown;
  updated_at?: unknown;
};

type FinspoSiteAggregate = {
  source_id?: unknown;
  site_id?: unknown;
  drive_id?: unknown;
  drive_name?: unknown;
  file_count?: unknown;
  total_bytes?: unknown;
};

type FinspoLargestItem = {
  source_id?: unknown;
  name?: unknown;
  path?: unknown;
  size_bytes?: unknown;
};

type FinspoInactiveItem = {
  source_id?: unknown;
  name?: unknown;
  days_since_touch?: unknown;
};

type FinspoDuplicateGroup = {
  count?: unknown;
  total_bytes?: unknown;
};

type FinspoRecommendation = {
  estimated_bytes?: unknown;
};

type FinspoEnvelope<T> = {
  success?: boolean;
  data?: T;
  error?: unknown;
};

type FinspoSourceList = {
  count?: unknown;
  sources?: FinspoSource[];
};

type FinspoLargestList = {
  count?: unknown;
  items?: FinspoLargestItem[];
};

type FinspoInactiveList = {
  count?: unknown;
  items?: FinspoInactiveItem[];
};

type FinspoAggregateList = {
  count?: unknown;
  aggregates?: FinspoSiteAggregate[];
};

type FinspoDuplicateList = {
  count?: unknown;
  groups?: FinspoDuplicateGroup[];
};

type FinspoRecommendationList = {
  count?: unknown;
  estimated_bytes_total?: unknown;
  drafts?: FinspoRecommendation[];
};

type ChunkPreview = {
  count: number;
  previews: Array<{
    id: string;
    title: string;
    score: string;
    text: string;
  }>;
};

type ChunkLookup = {
  documentId: string;
  chunkIndex: number;
  text: string;
};

type SharePointSourceInput = {
  tenantId?: string;
  siteId: string;
  siteWebUrl?: string;
  driveId: string;
  driveName?: string;
  driveType?: string;
};

type SyncWorkspaceResult = {
  finspoFailures: string[];
  finspoStarted: number;
  integrationFailures: string[];
  integrationStarted: number;
};

type CreatedSharePointSource = {
  id: string;
  syncStarted: boolean;
};

type UploadImportResult = {
  failedItems: number;
  id: string;
  processedItems: number;
  sourceType: string;
  status: string;
  totalItems: number;
};

type QuarryPage<T> = {
  items?: T[];
};

type QuarrySource = {
  source_id?: unknown;
  name?: unknown;
  url?: unknown;
  kind?: unknown;
  status?: unknown;
  created_at?: unknown;
  updated_at?: unknown;
};

type QuarryHandoff = {
  job_id?: unknown;
  accepted_at?: unknown;
};

type WebsiteCrawlInput = {
  maxPages?: number;
  url: string;
};

type StartedWebsiteCrawl = {
  createdAt: string;
  id: string;
  status: string;
  target: string;
};

const KNOWLEDGE_SOURCE_LIMIT = 8;
const GRAPH_NODE_LIMIT = 40;
const GRAPH_EDGE_LIMIT = 80;
const GRAPH_SOURCE_REF_LIMIT = 80;
const CHUNK_PREVIEW_LIMIT = 3;
const RING_RADII = [110, 155, 195];
const EMPTY_GRAPH: LiveKnowledgeGraph = {
  available: false,
  edgeCount: 0,
  groups: [],
  links: [],
  nodeCount: 0,
  nodes: [],
  truncated: false,
};

export async function loadKnowledgeWorkspace(request: NextRequest): Promise<LiveKnowledgePayload> {
  const session = await requireSession(request);
  const orgId = await resolveActiveOrgId(request, session);
  const [summary, documents] = await Promise.all([
    loadIntegrationSummary(request, { includeDiscovery: true, includeGraph: true }),
    loadDataPlaneDocumentSummaries(request),
  ]);
  const indexedCount = countIndexedDocuments(documents);

  if (!orgId) {
    const diagnostics = await loadKnowledgeDiagnostics({
      documentCount: documents.length,
      graphAvailable: false,
      graphEdgeCount: 0,
      graphNodeCount: 0,
      indexedCount,
    });
    return {
      generatedAt: summary.generatedAt,
      orgId: summary.orgId,
      collections: buildKnowledgeCollections(documents, []),
      dataPlane: {
        available: false,
        documentCount: documents.length,
        indexedCount,
      },
      graph: EMPTY_GRAPH,
      metrics: summary.metrics,
      metricCards: buildMetricCards({
        dataPlaneCount: documents.length,
        indexedCount,
        duplicateGroups: 0,
        graph: EMPTY_GRAPH,
        recommendationCount: 0,
        reclaimableBytes: 0,
        syncMetrics: summary.metrics,
      }),
      folders: buildFolderCards([], documents),
      integrations: buildIntegrationCards(summary.connections, documents, 0),
      files: buildFiles(documents),
      sources: [],
      webSources: [],
      diagnostics,
      finspo: {
        available: false,
        duplicateGroups: 0,
        inactiveCount: 0,
        largestCount: 0,
        recommendationCount: 0,
        reclaimableBytes: 0,
        sourceCount: 0,
      },
    };
  }

  const [graphSnapshot, finspoData, quarrySources] = await Promise.all([
    loadGraphSnapshot(request, session, orgId),
    loadFinspoWorkspace(request, session, orgId),
    loadQuarrySources(request),
  ]);

  const graphRefLookups = await loadGraphReferenceChunks(
    request,
    session,
    orgId,
    collectGraphSourceRefs(graphSnapshot),
  );
  const graph = buildGraph(graphSnapshot, graphRefLookups);
  const diagnosticsPromise = loadKnowledgeDiagnostics({
    documentCount: documents.length,
    graphAvailable: graph.available,
    graphEdgeCount: graph.edgeCount,
    graphNodeCount: graph.nodeCount,
    indexedCount,
  });

  const sortedDocuments = sortDocumentsByRecency(documents);
  const selectedDocuments = sortedDocuments.slice(0, KNOWLEDGE_SOURCE_LIMIT);
  const [documentChunks, freshness, diagnostics] = await Promise.all([
    loadDocumentChunkPreviews(request, session, orgId, selectedDocuments),
    loadFreshnessRows(request, session, orgId, selectedDocuments.map((document) => document.id)),
    diagnosticsPromise,
  ]);

  const relatedLabelsByDocument = buildRelatedLabelsByDocument(graph);
  const sources = buildKnowledgeSources(selectedDocuments, documentChunks, freshness, relatedLabelsByDocument);
  const webSources = buildWebSources(quarrySources);

  return {
    generatedAt: summary.generatedAt,
    orgId,
    collections: buildKnowledgeCollections(documents, webSources),
    dataPlane: {
      available: documents.length > 0,
      documentCount: documents.length,
      indexedCount,
    },
    graph,
    metrics: summary.metrics,
    metricCards: buildMetricCards({
      dataPlaneCount: documents.length,
      indexedCount,
      duplicateGroups: finspoData.duplicateGroups.length,
      graph,
      recommendationCount: finspoData.recommendations.length,
      reclaimableBytes: finspoData.reclaimableBytes,
      syncMetrics: summary.metrics,
    }),
    folders: buildFolderCards(finspoData.aggregates, documents),
    integrations: buildIntegrationCards(summary.connections, documents, finspoData.sources.length),
    files: buildFiles(documents),
    sources,
    webSources,
    diagnostics,
    finspo: {
      available:
        finspoData.sources.length > 0 ||
        finspoData.aggregates.length > 0 ||
        finspoData.largest.length > 0 ||
        finspoData.inactive.length > 0 ||
        finspoData.duplicateGroups.length > 0 ||
        finspoData.recommendations.length > 0,
      sourceCount: finspoData.sources.length,
      largestCount: finspoData.largest.length,
      inactiveCount: finspoData.inactive.length,
      duplicateGroups: finspoData.duplicateGroups.length,
      recommendationCount: finspoData.recommendations.length,
      reclaimableBytes: finspoData.reclaimableBytes,
    },
  };
}

export async function startKnowledgeWebsiteCrawl(
  request: NextRequest,
  input: WebsiteCrawlInput,
): Promise<StartedWebsiteCrawl> {
  const target = safeTrim(input.url);
  if (!target) {
    throw new Error("A website URL is required.");
  }

  const ack = await fetchQuarry<QuarryHandoff>(request, "/v1/crawl", {
    method: "POST",
    body: {
      url: target,
      max_pages: clamp(Math.round(input.maxPages ?? 12), 1, 50),
    },
    timeoutMs: 8_000,
  });

  const id = stringValue(ack.job_id);
  if (!id) {
    throw new Error("The website crawl could not be started.");
  }

  return {
    id,
    status: "queued",
    target,
    createdAt: stringValue(ack.accepted_at) || new Date().toISOString(),
  };
}

export async function syncKnowledgeWorkspace(request: NextRequest): Promise<SyncWorkspaceResult> {
  const session = await requireSession(request);
  const orgId = await resolveActiveOrgId(request, session);
  if (!orgId) {
    throw new Error("No active organization found.");
  }

  const [summary, finspoSources] = await Promise.all([
    loadIntegrationSummary(request),
    loadFinspoSources(request, session, orgId),
  ]);

  const integrationHeaders = buildIntegrationCoreHeaders(request, session, orgId);
  const finspoHeaders = buildServiceHeaders(request, session, orgId);
  const activeConnections = summary.connections.filter((connection) => !connection.deletedAt);

  const [integrationResults, finspoResults] = await Promise.all([
    Promise.allSettled(
      activeConnections.map(async (connection) => {
        const response = await fetch(
          `${getIntegrationCoreUrl()}/api/v1/connections/${encodeURIComponent(connection.id)}/sync`,
          {
            method: "POST",
            headers: integrationHeaders,
            cache: "no-store",
            signal: AbortSignal.timeout(6_000),
          },
        );
        if (!response.ok) {
          throw new Error(connection.displayName || connection.providerLabel);
        }
        return connection.displayName || connection.providerLabel;
      }),
    ),
    Promise.allSettled(
      finspoSources.map(async (source) => {
        const response = await fetch(
          `${getFinspoCoreUrl()}/api/v1/sources/${encodeURIComponent(source.id)}/sync`,
          {
            method: "POST",
            headers: finspoHeaders,
            cache: "no-store",
            signal: AbortSignal.timeout(6_000),
          },
        );
        if (!response.ok) {
          throw new Error(source.driveName || source.siteId || source.id);
        }
        return source.driveName || source.siteId || source.id;
      }),
    ),
  ]);

  return {
    integrationStarted: integrationResults.filter((result) => result.status === "fulfilled").length,
    integrationFailures: integrationResults.flatMap((result) =>
      result.status === "rejected" ? [result.reason instanceof Error ? result.reason.message : "Integration sync failed"] : []
    ),
    finspoStarted: finspoResults.filter((result) => result.status === "fulfilled").length,
    finspoFailures: finspoResults.flatMap((result) =>
      result.status === "rejected" ? [result.reason instanceof Error ? result.reason.message : "SharePoint sync failed"] : []
    ),
  };
}

export async function createSharePointKnowledgeSource(
  request: NextRequest,
  input: SharePointSourceInput,
): Promise<CreatedSharePointSource> {
  const session = await requireSession(request);
  const orgId = await resolveActiveOrgId(request, session);
  if (!orgId) {
    throw new Error("No active organization found.");
  }

  const sourceResponse = await fetch(`${getFinspoCoreUrl()}/api/v1/sources`, {
    method: "POST",
    headers: buildJsonServiceHeaders(request, session, orgId),
    cache: "no-store",
    signal: AbortSignal.timeout(6_000),
    body: JSON.stringify({
      tenant_id: safeTrim(input.tenantId),
      site_id: safeTrim(input.siteId),
      site_web_url: safeTrim(input.siteWebUrl),
      drive_id: safeTrim(input.driveId),
      drive_name: safeTrim(input.driveName),
      drive_type: safeTrim(input.driveType),
      enabled: true,
    }),
  });

  const sourcePayload = (await sourceResponse.json().catch(() => null)) as FinspoEnvelope<FinspoSource> | null;
  const source = unwrapFinspo(sourcePayload);
  const sourceId = stringValue(source?.id);
  if (!sourceResponse.ok || !sourceId) {
    throw new Error("SharePoint source could not be registered.");
  }

  const syncResponse = await fetch(`${getFinspoCoreUrl()}/api/v1/sources/${encodeURIComponent(sourceId)}/sync`, {
    method: "POST",
    headers: buildServiceHeaders(request, session, orgId),
    cache: "no-store",
    signal: AbortSignal.timeout(6_000),
  });

  return {
    id: sourceId,
    syncStarted: syncResponse.ok,
  };
}

export async function forwardImportUpload(request: NextRequest): Promise<UploadImportResult> {
  const session = await requireSession(request);
  const orgId = await resolveActiveOrgId(request, session);
  if (!orgId) {
    throw new Error("No active organization found.");
  }

  const incoming = await request.formData();
  const files = incoming
    .getAll("files")
    .filter((entry): entry is File => typeof File !== "undefined" && entry instanceof File);
  if (files.length === 0) {
    throw new Error("Select at least one file to import.");
  }

  const body = new FormData();
  for (const file of files) {
    body.append("files", file, file.name);
  }

  const response = await fetch(`${getImportsCoreUrl()}/api/v1/import/jobs/upload`, {
    method: "POST",
    headers: {
      "X-Internal-Api-Key": getInternalApiKey(),
      "X-Org-Id": orgId,
      "X-User-Id": session.user.id,
      "X-Service-Name": "verevonv2",
    },
    cache: "no-store",
    signal: AbortSignal.timeout(30_000),
    body,
  });

  const payload = (await response.json().catch(() => null)) as
    | {
      id?: unknown;
      status?: unknown;
      source_type?: unknown;
      total_items?: unknown;
      processed_items?: unknown;
      failed_items?: unknown;
      detail?: unknown;
    }
    | null;
  if (!response.ok) {
    throw new Error(stringValue(payload?.detail) || "Upload import could not be started.");
  }

  return {
    id: stringValue(payload?.id) || "",
    status: stringValue(payload?.status) || "queued",
    sourceType: stringValue(payload?.source_type) || "upload",
    totalItems: numberValue(payload?.total_items),
    processedItems: numberValue(payload?.processed_items),
    failedItems: numberValue(payload?.failed_items),
  };
}

async function loadGraphSnapshot(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<GraphSnapshotResponse | null> {
  try {
    const response = await fetch(
      `${getGraphIndexUrl()}/v1/graphs/${encodeURIComponent(orgId)}?limit_nodes=120&limit_edges=240`,
      {
        method: "GET",
        headers: buildServiceHeaders(request, session, orgId),
        cache: "no-store",
        signal: AbortSignal.timeout(2_500),
      },
    );
    if (!response.ok) return null;
    return (await response.json().catch(() => null)) as GraphSnapshotResponse | null;
  } catch {
    return null;
  }
}

async function loadGraphReferenceChunks(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  knowledgeIds: string[],
): Promise<Map<string, ChunkLookup>> {
  if (knowledgeIds.length === 0) return new Map();
  try {
    const response = await fetch(`${getKnowledgeRetrievalUrl()}/v1/retrieve/chunks`, {
      method: "POST",
      headers: buildJsonServiceHeaders(request, session, orgId),
      cache: "no-store",
      signal: AbortSignal.timeout(4_000),
      body: JSON.stringify({
        org_id: orgId,
        knowledge_ids: knowledgeIds.slice(0, GRAPH_SOURCE_REF_LIMIT),
      }),
    });
    if (!response.ok) return new Map();
    const payload = (await response.json().catch(() => null)) as RetrievalChunksResponse | null;
    const chunks = Array.isArray(payload?.chunks) ? payload.chunks : [];
    return new Map(
      chunks.flatMap((chunk) => {
        const knowledgeId = stringValue(chunk.knowledge_id);
        const documentId = stringValue(chunk.document_id);
        if (!knowledgeId || !documentId) return [];
        return [[knowledgeId, {
          documentId,
          chunkIndex: numberValue(chunk.chunk_index),
          text: stringValue(chunk.text) || "",
        }]];
      }),
    );
  } catch {
    return new Map();
  }
}

async function loadDocumentChunkPreviews(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  documents: DataPlaneDocumentSummary[],
): Promise<Map<string, ChunkPreview>> {
  const results = await Promise.allSettled(
    documents.map(async (document) => {
      const response = await fetch(`${getKnowledgeRetrievalUrl()}/v1/retrieve/chunks`, {
        method: "POST",
        headers: buildJsonServiceHeaders(request, session, orgId),
        cache: "no-store",
        signal: AbortSignal.timeout(4_000),
        body: JSON.stringify({
          org_id: orgId,
          document_id: document.id,
          limit: CHUNK_PREVIEW_LIMIT,
          offset: 0,
        }),
      });
      if (!response.ok) {
        return [document.id, previewFromContent(document.content)] as const;
      }
      const payload = (await response.json().catch(() => null)) as RetrievalChunksResponse | null;
      return [document.id, chunkPreviewFromPayload(payload, document.content)] as const;
    }),
  );

  return new Map(
    results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []),
  );
}

async function loadFreshnessRows(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  documentIds: string[],
): Promise<Map<string, { ageDays: number; score: number }>> {
  if (documentIds.length === 0) return new Map();
  try {
    const response = await fetch(`${getKnowledgeRetrievalUrl()}/v1/knowledge/freshness`, {
      method: "POST",
      headers: buildJsonServiceHeaders(request, session, orgId),
      cache: "no-store",
      signal: AbortSignal.timeout(3_000),
      body: JSON.stringify({
        org_id: orgId,
        document_ids: documentIds,
      }),
    });
    if (!response.ok) return new Map();
    const payload = (await response.json().catch(() => null)) as FreshnessResponse | null;
    const rows = Array.isArray(payload?.freshness) ? payload.freshness : [];
    return new Map(
      rows.flatMap((row) => {
        const documentId = stringValue(row.document_id);
        if (!documentId) return [];
        return [[documentId, {
          ageDays: numberValue(row.age_days),
          score: numberValue(row.freshness_score),
        }]];
      }),
    );
  } catch {
    return new Map();
  }
}

async function loadFinspoWorkspace(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
) {
  const [sources, aggregates, largest, inactive, duplicateGroups, recommendations] = await Promise.all([
    loadFinspoSources(request, session, orgId),
    loadFinspoAggregates(request, session, orgId),
    loadFinspoLargest(request, session, orgId),
    loadFinspoInactive(request, session, orgId),
    loadFinspoDuplicates(request, session, orgId),
    loadFinspoRecommendations(request, session, orgId),
  ]);

  return {
    sources,
    aggregates,
    largest,
    inactive,
    duplicateGroups,
    recommendations,
    reclaimableBytes: recommendations.reduce((sum, recommendation) => sum + numberValue(recommendation.estimated_bytes), 0),
  };
}

async function loadQuarrySources(request: NextRequest): Promise<QuarrySource[]> {
  try {
    const page = await fetchQuarry<QuarryPage<QuarrySource>>(request, "/v1/sources", {
      query: { limit: 24 },
      timeoutMs: 2_500,
    });
    return Array.isArray(page.items) ? page.items : [];
  } catch {
    return [];
  }
}

async function loadFinspoSources(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<Array<{ driveName: string; id: string; siteId: string }>> {
  const payload = await loadFinspoData<FinspoSourceList>(request, session, orgId, "/api/v1/sources");
  const sources = Array.isArray(payload?.sources) ? payload.sources : [];
  return sources.flatMap((source) => {
    const id = stringValue(source.id);
    if (!id) return [];
    return [{
      id,
      siteId: stringValue(source.site_id) || "",
      driveName: stringValue(source.drive_name) || stringValue(source.drive_id) || "",
    }];
  });
}

async function loadFinspoAggregates(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<FinspoSiteAggregate[]> {
  const payload = await loadFinspoData<FinspoAggregateList>(request, session, orgId, "/api/v1/analytics/by-site");
  return Array.isArray(payload?.aggregates) ? payload.aggregates : [];
}

async function loadFinspoLargest(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<FinspoLargestItem[]> {
  const payload = await loadFinspoData<FinspoLargestList>(request, session, orgId, "/api/v1/analytics/largest?limit=8");
  return Array.isArray(payload?.items) ? payload.items : [];
}

async function loadFinspoInactive(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<FinspoInactiveItem[]> {
  const payload = await loadFinspoData<FinspoInactiveList>(
    request,
    session,
    orgId,
    "/api/v1/analytics/inactive?limit=8&older_than=4320h",
  );
  return Array.isArray(payload?.items) ? payload.items : [];
}

async function loadFinspoDuplicates(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<FinspoDuplicateGroup[]> {
  const payload = await loadFinspoData<FinspoDuplicateList>(
    request,
    session,
    orgId,
    "/api/v1/analytics/duplicates?max_groups=8&min_count=2",
  );
  return Array.isArray(payload?.groups) ? payload.groups : [];
}

async function loadFinspoRecommendations(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<FinspoRecommendation[]> {
  const payload = await loadFinspoData<FinspoRecommendationList>(
    request,
    session,
    orgId,
    "/api/v1/recommendations?max_groups=8&inactive_limit=8&older_than=4320h",
  );
  return Array.isArray(payload?.drafts) ? payload.drafts : [];
}

async function loadFinspoData<T>(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  path: string,
): Promise<T | null> {
  try {
    const response = await fetch(`${getFinspoCoreUrl()}${path}`, {
      method: "GET",
      headers: buildServiceHeaders(request, session, orgId),
      cache: "no-store",
      signal: AbortSignal.timeout(4_000),
    });
    if (!response.ok) return null;
    const payload = (await response.json().catch(() => null)) as FinspoEnvelope<T> | null;
    return unwrapFinspo(payload);
  } catch {
    return null;
  }
}

function buildMetricCards(input: {
  dataPlaneCount: number;
  duplicateGroups: number;
  graph: LiveKnowledgeGraph;
  indexedCount: number;
  recommendationCount: number;
  reclaimableBytes: number;
  syncMetrics: LiveKnowledgePayload["metrics"];
}): LiveKnowledgeMetric[] {
  const pendingCount = Math.max(0, input.dataPlaneCount - input.indexedCount);
  return [
    {
      label: "Indexed documents",
      value: formatCount(input.indexedCount),
      delta: pendingCount > 0 ? `${formatCount(pendingCount)} pending` : "All indexed",
      tone: pendingCount > 0 ? "warn" : "good",
    },
    {
      label: "Connected integrations",
      value: formatCount(input.syncMetrics.connected),
      delta: input.syncMetrics.syncing > 0 ? `${formatCount(input.syncMetrics.syncing)} syncing` : "Healthy",
      tone: input.syncMetrics.failed > 0 ? "warn" : "good",
    },
    {
      label: "Graph entities",
      value: formatCount(input.graph.nodeCount),
      delta: input.graph.available ? `${formatCount(input.graph.edgeCount)} edges` : "Graph offline",
      tone: input.graph.available ? "good" : "warn",
    },
    {
      label: "Reclaim opportunities",
      value: input.reclaimableBytes > 0 ? formatBytes(input.reclaimableBytes) : formatCount(input.duplicateGroups),
      delta: input.recommendationCount > 0 ? `${formatCount(input.recommendationCount)} recommendations` : "No cleanup queued",
      tone: input.recommendationCount > 0 ? "warn" : "good",
    },
  ];
}

function buildKnowledgeCollections(
  documents: DataPlaneDocumentSummary[],
  webSources: LiveKnowledgeWebSource[],
): LiveKnowledgeCollection[] {
  const counts = new Map<string, { count: number; label: string }>();
  for (const document of documents) {
    const providerKey = normalizeProviderKey(document.source) || "docs";
    const current = counts.get(providerKey);
    counts.set(providerKey, {
      label: sourceLabel(providerKey),
      count: (current?.count ?? 0) + 1,
    });
  }

  if (webSources.length > 0) {
    counts.set("web", {
      label: "Web sources",
      count: webSources.length,
    });
  }

  return [
    {
      id: "all",
      label: "General Knowledge",
      count: documents.length + webSources.length,
    },
    ...Array.from(counts.entries())
      .sort((left, right) => right[1].count - left[1].count || left[1].label.localeCompare(right[1].label))
      .slice(0, 6)
      .map(([providerKey, value]) => ({
        id: providerKey === "web" ? "web" : `provider:${providerKey}`,
        label: value.label,
        count: value.count,
      })),
  ];
}

function buildFolderCards(
  aggregates: FinspoSiteAggregate[],
  documents: DataPlaneDocumentSummary[],
): LiveKnowledgeFolder[] {
  const tones: LiveKnowledgeFolder["tone"][] = ["warm", "green", "blue", "gray"];
  const folders: LiveKnowledgeFolder[] = [];

  for (const aggregate of aggregates.slice(0, 2)) {
    folders.push({
      id: stringValue(aggregate.source_id) || crypto.randomUUID(),
      title: stringValue(aggregate.drive_name) || stringValue(aggregate.site_id) || "SharePoint drive",
      subtitle: "SharePoint / OneDrive",
      providerKey: "microsoft",
      primaryValue: formatCount(numberValue(aggregate.file_count)),
      primaryLabel: "Files",
      secondaryValue: formatBytes(numberValue(aggregate.total_bytes)),
      secondaryLabel: "Stored",
      connections: ["Microsoft 365", "Finspo"],
      tone: tones[folders.length % tones.length],
    });
  }

  const documentGroups = groupDocumentsBySource(documents);
  for (const [source, group] of documentGroups.slice(0, Math.max(0, 4 - folders.length))) {
    folders.push({
      id: `source:${source}`,
      title: sourceLabel(source),
      subtitle: `${formatCount(group.length)} retrieval documents`,
      providerKey: normalizeProviderKey(source) || source,
      primaryValue: formatCount(group.length),
      primaryLabel: "Docs",
      secondaryValue: formatCount(countIndexedDocuments(group)),
      secondaryLabel: "Indexed",
      connections: [sourceLabel(source), "Data Plane"],
      tone: tones[folders.length % tones.length],
    });
  }

  return folders;
}

function buildIntegrationCards(
  connections: IntegrationConnectionSummary[],
  documents: DataPlaneDocumentSummary[],
  finspoSourceCount: number,
): LiveKnowledgeIntegration[] {
  const documentCountsByProvider = new Map<string, number>();
  for (const [source, group] of groupDocumentsBySource(documents)) {
    documentCountsByProvider.set(normalizeProviderKey(source), group.length);
  }

  const cards = connections
    .filter((connection) => !connection.deletedAt)
    .map((connection) => {
      const providerKey = normalizeProviderKey(connection.providerKey);
      const documentCount = documentCountsByProvider.get(providerKey) ?? 0;
      const status = integrationCardStatus(connection.syncStatus || connection.status);
      const detail = providerKey === "microsoft" && finspoSourceCount > 0
        ? `${formatCount(finspoSourceCount)} drives · ${formatCount(documentCount)} docs`
        : `${formatCount(documentCount)} docs`;
      return {
        id: connection.id,
        name: connection.displayName || connection.providerLabel,
        providerKey,
        status,
        documents: detail,
        freshness: connection.latestSyncJob?.updatedAt ? relativeTime(connection.latestSyncJob.updatedAt) : "Live",
        detail: connection.providerLabel,
      };
    });

  if (!cards.some((card) => card.providerKey === "microsoft") && finspoSourceCount > 0) {
    cards.push({
      id: "finspo:microsoft",
      name: "Microsoft 365",
      providerKey: "microsoft",
      status: "Connected",
      documents: `${formatCount(finspoSourceCount)} drives`,
      freshness: "Finspo",
      detail: "SharePoint / OneDrive",
    });
  }

  return cards.slice(0, 8);
}

function buildFiles(documents: DataPlaneDocumentSummary[]): LiveKnowledgeFile[] {
  return sortDocumentsByRecency(documents).slice(0, 10).map((document) => ({
    id: document.id,
    name: document.title,
    addedBy: document.createdBy || "System",
    source: sourceLabel(document.source),
    providerKey: normalizeProviderKey(document.source) || document.source,
    updated: relativeTime(document.updatedAt || document.createdAt),
    type: documentSourceType(document),
  }));
}

function buildKnowledgeSources(
  documents: DataPlaneDocumentSummary[],
  chunkPreviews: Map<string, ChunkPreview>,
  freshness: Map<string, { ageDays: number; score: number }>,
  relatedLabelsByDocument: Map<string, string[]>,
): LiveKnowledgeSource[] {
  return documents.map((document) => {
    const preview = chunkPreviews.get(document.id) ?? previewFromContent(document.content);
    const freshnessRow = freshness.get(document.id);
    const freshnessScore = freshnessRow?.score ?? 0.8;
    const contentSize = Buffer.byteLength(document.content || "", "utf8");
    return {
      id: document.id,
      title: document.title,
      description: `${sourceLabel(document.source)} · ${formatDocumentStatus(document.status)}`,
      type: documentSourceType(document),
      provider: sourceLabel(document.source),
      providerKey: normalizeProviderKey(document.source) || document.source,
      category: document.type || "document",
      owner: document.createdBy || "System",
      updated: relativeTime(document.updatedAt || document.createdAt),
      size: formatBytes(contentSize),
      status: knowledgeSourceStatus(document.status),
      chunks: preview.count,
      hitRate: `${Math.round(freshnessScore * 100)}%`,
      coverage: preview.count > 0 ? "100%" : "0%",
      similarity: freshnessScore.toFixed(2),
      tags: compact([
        sourceLabel(document.source),
        document.type,
        formatDocumentStatus(document.status),
      ]).slice(0, 3),
      related: relatedLabelsByDocument.get(document.id)?.slice(0, 3) ?? [],
      chunksPreview: preview.previews,
    };
  });
}

function buildWebSources(quarrySources: QuarrySource[]): LiveKnowledgeWebSource[] {
  return quarrySources
    .flatMap((source) => {
      const id = stringValue(source.source_id);
      const url = stringValue(source.url);
      if (!id || !url) return [];
      return [{
        id,
        kind: stringValue(source.kind) || "crawl",
        name: stringValue(source.name) || hostLabel(url),
        providerKey: "web",
        status: stringValue(source.status) || "unknown",
        updated: relativeTime(stringValue(source.updated_at) || stringValue(source.created_at)),
        url,
      }];
    })
    .sort((left, right) => left.name.localeCompare(right.name));
}

function buildGraph(
  snapshot: GraphSnapshotResponse | null,
  chunkLookup: Map<string, ChunkLookup>,
): LiveKnowledgeGraph {
  if (!snapshot) return EMPTY_GRAPH;

  const rawNodes = Array.isArray(snapshot.nodes) ? snapshot.nodes : [];
  const rawEdges = Array.isArray(snapshot.edges) ? snapshot.edges : [];
  const filteredNodes = rawNodes
    .map((node) => ({
      id: stringValue(node.entity_id) || "",
      label: stringValue(node.entity_text) || stringValue(node.entity_id) || "",
      group: (stringValue(node.entity_type) || "entity").toLowerCase(),
      sourceRefs: stringArray(node.source_refs),
    }))
    .filter((node) => node.id && node.label && node.group !== "org")
    .slice(0, GRAPH_NODE_LIMIT);
  const nodeIds = new Set(filteredNodes.map((node) => node.id));

  const degrees = new Map<string, number>();
  const links: LiveKnowledgeGraphLink[] = rawEdges
    .map((edge) => ({
      from: stringValue(edge.entity_a_id) || "",
      to: stringValue(edge.entity_b_id) || "",
      label: stringValue(edge.relation_type) || "related",
      strength: Math.max(1, Math.round((numberValue(edge.confidence) || 0.4) * 3)),
      sourceRefs: stringArray(edge.source_refs),
    }))
    .filter((edge) => edge.from && edge.to && nodeIds.has(edge.from) && nodeIds.has(edge.to))
    .slice(0, GRAPH_EDGE_LIMIT);

  for (const link of links) {
    degrees.set(link.from, (degrees.get(link.from) ?? 0) + 1);
    degrees.set(link.to, (degrees.get(link.to) ?? 0) + 1);
  }

  const groups = Array.from(new Set(filteredNodes.map((node) => node.group))).slice(0, 8);
  const groupIndex = new Map(groups.map((group, index) => [group, index]));
  const total = Math.max(filteredNodes.length, 1);

  const nodes: LiveKnowledgeGraphNode[] = filteredNodes.map((node, index) => {
    const groupOrder = groupIndex.get(node.group) ?? 0;
    const angle = (Math.PI * 2 * index) / total;
    const ring = RING_RADII[groupOrder % RING_RADII.length];
    const radius = clamp(16 + (degrees.get(node.id) ?? 0) * 1.5 + Math.min(node.sourceRefs.length, 4), 16, 32);
    const sourceIds = Array.from(
      new Set(
        node.sourceRefs
          .map((ref) => chunkLookup.get(ref)?.documentId)
          .filter((value): value is string => Boolean(value)),
      ),
    );
    return {
      id: node.id,
      label: node.label,
      group: node.group,
      tone: graphTone(node.group),
      x: Math.round(320 + Math.cos(angle) * ring),
      y: Math.round(210 + Math.sin(angle) * (ring * 0.78)),
      radius,
      sourceRefs: node.sourceRefs,
      sourceIds,
    };
  });

  return {
    available: nodes.length > 0 || links.length > 0,
    nodeCount: numberValue(snapshot.node_count) || nodes.length,
    edgeCount: numberValue(snapshot.edge_count) || links.length,
    groups,
    nodes,
    links,
    truncated: snapshot.truncated === true,
  };
}

function buildRelatedLabelsByDocument(graph: LiveKnowledgeGraph) {
  const map = new Map<string, Set<string>>();
  for (const node of graph.nodes) {
    for (const sourceId of node.sourceIds) {
      if (!map.has(sourceId)) {
        map.set(sourceId, new Set());
      }
      map.get(sourceId)?.add(node.label);
    }
  }
  return new Map(
    Array.from(map.entries()).map(([sourceId, labels]) => [sourceId, Array.from(labels)]),
  );
}

function groupDocumentsBySource(documents: DataPlaneDocumentSummary[]) {
  const groups = new Map<string, DataPlaneDocumentSummary[]>();
  for (const document of documents) {
    const key = normalizeProviderKey(document.source) || document.source;
    groups.set(key, [...(groups.get(key) ?? []), document]);
  }
  return Array.from(groups.entries()).sort((left, right) => right[1].length - left[1].length);
}

function sortDocumentsByRecency(documents: DataPlaneDocumentSummary[]) {
  return [...documents].sort((left, right) =>
    sortDateValue(right.updatedAt || right.createdAt) - sortDateValue(left.updatedAt || left.createdAt)
  );
}

function collectGraphSourceRefs(snapshot: GraphSnapshotResponse | null) {
  if (!snapshot) return [];
  const refs = new Set<string>();
  for (const node of Array.isArray(snapshot.nodes) ? snapshot.nodes : []) {
    for (const ref of stringArray(node.source_refs)) {
      refs.add(ref);
      if (refs.size >= GRAPH_SOURCE_REF_LIMIT) return Array.from(refs);
    }
  }
  for (const edge of Array.isArray(snapshot.edges) ? snapshot.edges : []) {
    for (const ref of stringArray(edge.source_refs)) {
      refs.add(ref);
      if (refs.size >= GRAPH_SOURCE_REF_LIMIT) return Array.from(refs);
    }
  }
  return Array.from(refs);
}

function countIndexedDocuments(documents: DataPlaneDocumentSummary[]) {
  return documents.filter((document) => ["indexed", "active", "completed", "chunked"].includes(document.status.toLowerCase())).length;
}

function previewFromContent(content?: string): ChunkPreview {
  const parts = (content || "")
    .split(/\n{2,}/)
    .map((part) => normalizeWhitespace(part))
    .filter(Boolean)
    .slice(0, CHUNK_PREVIEW_LIMIT);
  if (parts.length === 0) {
    return {
      count: 0,
      previews: [{
        id: "chunk-0",
        title: "Awaiting chunk preview",
        score: "#0",
        text: "The retrieval engine has not returned chunks for this document yet.",
      }],
    };
  }
  return {
    count: parts.length,
    previews: parts.map((part, index) => ({
      id: `chunk-${index + 1}`,
      title: `Excerpt ${index + 1}`,
      score: `#${index + 1}`,
      text: part,
    })),
  };
}

function chunkPreviewFromPayload(
  payload: RetrievalChunksResponse | null,
  content?: string,
): ChunkPreview {
  const chunks = Array.isArray(payload?.chunks) ? payload.chunks : [];
  if (chunks.length === 0) return previewFromContent(content);
  return {
    count: Math.max(numberValue(payload?.count), chunks.length),
    previews: chunks.slice(0, CHUNK_PREVIEW_LIMIT).map((chunk) => {
      const index = numberValue(chunk.chunk_index);
      return {
        id: stringValue(chunk.knowledge_id) || `chunk-${index + 1}`,
        title: `Chunk ${index + 1}`,
        score: `#${index + 1}`,
        text: normalizeWhitespace(stringValue(chunk.text) || ""),
      };
    }),
  };
}

function integrationCardStatus(syncStatus: string) {
  const normalized = syncStatus.toLowerCase();
  if (["queued", "running", "waiting_provider", "handoff_data_plane", "syncing"].includes(normalized)) {
    return "Syncing" as const;
  }
  if (["failed", "error", "needs_refresh", "cancelled"].includes(normalized)) {
    return "Review" as const;
  }
  return "Connected" as const;
}

function knowledgeSourceStatus(status: string): LiveKnowledgeSourceStatus {
  const normalized = status.toLowerCase();
  if (["indexed", "active", "completed", "chunked"].includes(normalized)) return "Indexed";
  if (["processing", "running", "queued", "pending"].includes(normalized)) return "Re-indexing";
  return "Pending review";
}

function documentSourceType(document: DataPlaneDocumentSummary): LiveKnowledgeSourceType {
  const normalizedType = document.type.toLowerCase();
  const normalizedSource = document.source.toLowerCase();
  if (normalizedType.includes("pdf")) return "PDF";
  if (normalizedSource.includes("notion") || normalizedType.includes("notion")) return "Notion";
  if (normalizedSource.startsWith("http") || normalizedSource.includes("website") || normalizedSource.includes("crawler")) return "URL";
  return "Docs";
}

function graphTone(group: string): LiveKnowledgeGraphNode["tone"] {
  if (["policy", "claim", "rule", "compliance"].some((token) => group.includes(token))) return "policy";
  if (["product", "feature", "plan", "sku"].some((token) => group.includes(token))) return "product";
  if (["risk", "fraud", "incident"].some((token) => group.includes(token))) return "risk";
  if (["workspace", "site", "drive", "channel", "folder"].some((token) => group.includes(token))) return "support";
  return "core";
}

function formatDocumentStatus(status: string) {
  const normalized = status.replace(/[_-]+/g, " ").trim();
  return normalized ? normalized.replace(/\b\w/g, (character) => character.toUpperCase()) : "Unknown";
}

function sourceLabel(source: string) {
  const normalized = normalizeProviderKey(source);
  switch (normalized) {
    case "microsoft":
      return "Microsoft 365";
    case "google":
      return "Google Workspace";
    case "notion":
      return "Notion";
    case "github":
      return "GitHub";
    case "slack":
      return "Slack";
    default:
      return source.replace(/^onboarding:/, "").replace(/[_:-]+/g, " ").replace(/\b\w/g, (character) => character.toUpperCase());
  }
}

function normalizeProviderKey(value?: string) {
  const normalized = value?.trim().toLowerCase() ?? "";
  switch (normalized) {
    case "m365":
    case "microsoft365":
    case "microsoft-365":
    case "microsoft-graph":
    case "onedrive":
    case "outlook":
    case "sharepoint":
    case "teams":
      return "microsoft";
    case "gdrive":
    case "gmail":
    case "google-drive":
    case "google-workspace":
      return "google";
    default:
      return normalized;
  }
}

function unwrapFinspo<T>(payload: FinspoEnvelope<T> | null | undefined) {
  if (!payload || typeof payload !== "object") return null;
  return payload.data ?? null;
}

function buildJsonServiceHeaders(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId?: string,
) {
  return {
    ...buildServiceHeaders(request, session, orgId),
    Accept: "application/json",
    "Content-Type": "application/json",
  };
}

function stringValue(value: unknown) {
  return typeof value === "string" && value.trim().length > 0 ? value.trim() : "";
}

function safeTrim(value?: string) {
  return value?.trim() || undefined;
}

function numberValue(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function stringArray(value: unknown) {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string" && item.trim().length > 0)
    : [];
}

function sortDateValue(value?: string) {
  const parsed = value ? Date.parse(value) : Number.NaN;
  return Number.isFinite(parsed) ? parsed : 0;
}

function formatCount(value: number) {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(value);
}

function formatBytes(bytes: number) {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const digits = value >= 10 || unitIndex === 0 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

function relativeTime(value?: string) {
  if (!value) return "Live";
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) return "Live";
  const seconds = Math.max(0, Math.round((Date.now() - parsed) / 1000));
  if (seconds < 90) return "Just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 36) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

function compact(values: Array<string | undefined>) {
  return values.filter((value): value is string => Boolean(value && value.trim()));
}

function hostLabel(value: string) {
  try {
    return new URL(value).host || value;
  } catch {
    return value;
  }
}

function normalizeWhitespace(value: string) {
  return value.replace(/\s+/g, " ").trim();
}
