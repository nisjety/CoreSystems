import "server-only";

import {
  getDataPlaneDocumentsUrl,
  getEmbeddingEngineUrl,
  getGraphIndexUrl,
  getKnowledgeRetrievalUrl,
  getMinioUrl,
  getQdrantUrl,
  getQuickwitAdapterUrl,
  getQuickwitUrl,
  getWikiStoreUrl,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import type {
  LiveKnowledgeDiagnosticItem,
  LiveKnowledgeDiagnosticTone,
  LiveKnowledgeDiagnostics,
} from "@/features/knowledge-v2/lib/knowledge-live";

type ReadyzResponse = {
  checks?: Record<string, unknown>;
  index?: unknown;
  service?: unknown;
  sparse_backend?: unknown;
  status?: unknown;
};

type QdrantCollectionsResponse = {
  result?: {
    collections?: Array<{ name?: unknown }>;
  };
};

type QuickwitIndexResponse = Array<{
  index_config?: {
    index_id?: unknown;
    index_uri?: unknown;
  };
}>;

type QuickwitIndexSummary = {
  id: string;
  uri: string;
};

const REQUEST_TIMEOUT_MS = 1_800;
const PRIMARY_VECTOR_COLLECTION = "dataplane_knowledge";
const WIKI_VECTOR_COLLECTION = "wiki_block_embeddings";

export async function loadKnowledgeDiagnostics(input: {
  documentCount: number;
  graphAvailable: boolean;
  graphEdgeCount: number;
  graphNodeCount: number;
  indexedCount: number;
}): Promise<LiveKnowledgeDiagnostics> {
  const [
    documentsReady,
    retrievalReady,
    embeddingReady,
    graphReady,
    wikiReady,
    quickwitAdapterReady,
    qdrantCollections,
    quickwitIndexes,
    minioReady,
  ] = await Promise.all([
    loadReadyz(`${getDataPlaneDocumentsUrl()}/readyz`),
    loadReadyz(`${getKnowledgeRetrievalUrl()}/readyz`),
    loadReadyz(`${getEmbeddingEngineUrl()}/readyz`),
    loadReadyz(`${getGraphIndexUrl()}/readyz`),
    loadReadyz(`${getWikiStoreUrl()}/readyz`),
    loadReadyz(`${getQuickwitAdapterUrl()}/readyz`),
    loadQdrantCollections(),
    loadQuickwitIndexes(),
    loadMinioReady(),
  ]);

  const sparseBackend = stringValue(retrievalReady?.sparse_backend);
  const vectorCollections = qdrantCollections;
  const quickwitIndexIds = quickwitIndexes.map((index) => index.id);
  const quickwitMinioBacked = quickwitIndexes.some((index) => index.uri.startsWith("s3://quickwit/"));
  const retrievalChecks = recordValue(retrievalReady?.checks);

  const services: LiveKnowledgeDiagnosticItem[] = [
    buildServiceItem({
      id: "documents-api",
      label: "Documents API",
      ready: isReady(documentsReady),
      detail: isReady(documentsReady)
        ? `${formatCount(input.documentCount)} live documents are available to the knowledge workspace.`
        : "Document metadata could not be confirmed from Data Plane v2.",
      meta: serviceLabel(documentsReady),
    }),
    buildServiceItem({
      id: "retrieval-engine",
      label: "Retrieval engine",
      ready: isReady(retrievalReady),
      detail: isReady(retrievalReady)
        ? `Sparse backend ${sparseBackend || "unknown"} is serving live retrieval.`
        : "Retrieval-engine readyz did not report healthy status.",
      meta: serviceLabel(retrievalReady),
    }),
    buildServiceItem({
      id: "embedding-engine",
      label: "Embedding engine",
      ready: isReady(embeddingReady),
      detail: isReady(embeddingReady)
        ? `${formatCount(vectorCollections.length)} vector collections are provisioned in Qdrant.`
        : "Embedding-engine readyz did not confirm vector writer readiness.",
      meta: serviceLabel(embeddingReady),
    }),
    buildServiceItem({
      id: "graph-index",
      label: "Graph index",
      ready: isReady(graphReady),
      detail: isReady(graphReady)
        ? input.graphAvailable
          ? `${formatCount(input.graphNodeCount)} nodes and ${formatCount(input.graphEdgeCount)} edges are currently surfaced into Knowledge.`
          : "Graph-index is ready, but the current workspace has not surfaced graph entities yet."
        : "Graph-index readyz did not confirm healthy status.",
      meta: serviceLabel(graphReady),
    }),
    buildServiceItem({
      id: "wiki-store",
      label: "Wiki store",
      ready: isReady(wikiReady),
      detail: isReady(wikiReady)
        ? "Wiki CRUD and publish flow are live in Data Plane v2."
        : "Wiki-store readyz did not confirm healthy status.",
      meta: serviceLabel(wikiReady),
    }),
    buildServiceItem({
      id: "quickwit-adapter",
      label: "Quickwit adapter",
      ready: isReady(quickwitAdapterReady),
      detail: isReady(quickwitAdapterReady)
        ? `${formatCount(quickwitIndexIds.length)} Quickwit indexes are visible from the adapter.`
        : "Quickwit sparse search adapter did not report healthy status.",
      meta: stringValue(quickwitAdapterReady?.index) || serviceLabel(quickwitAdapterReady),
    }),
  ];

  const storage: LiveKnowledgeDiagnosticItem[] = [
    buildStorageItem({
      id: "postgres",
      label: "Postgres",
      ready: booleanValue(retrievalChecks.postgres),
      detail: booleanValue(retrievalChecks.postgres)
        ? "Canonical metadata store for documents, graph, wiki, traces, and evals is reachable."
        : "Retrieval-engine could not reach the canonical Postgres store.",
      meta: "documents, graph, wiki, traces",
    }),
    buildStorageItem({
      id: "redis-rag",
      label: "Redis RAG cache",
      ready: booleanValue(retrievalChecks.redis),
      detail: booleanValue(retrievalChecks.redis)
        ? "Retrieval cache is reachable from the live retrieval-engine."
        : "Redis cache could not be confirmed from retrieval-engine readyz.",
      meta: "retrieval-engine readyz",
    }),
    buildStorageItem({
      id: "qdrant",
      label: "Qdrant vectors",
      ready: booleanValue(retrievalChecks.qdrant) && vectorCollections.length > 0,
      detail: booleanValue(retrievalChecks.qdrant) && vectorCollections.length > 0
        ? `Collections include ${joinNames(vectorCollections, 3)}.`
        : "Vector collections could not be confirmed from Qdrant.",
      meta: `${formatCount(vectorCollections.length)} collections`,
    }),
    buildStorageItem({
      id: "minio",
      label: "MinIO object storage",
      ready: minioReady && quickwitMinioBacked,
      detail: minioReady && quickwitMinioBacked
        ? "Quickwit indexes are mounted under s3://quickwit through the MinIO-backed object store."
        : "MinIO or its Quickwit backing path could not be confirmed from the live runtime.",
      meta: quickwitMinioBacked ? "s3://quickwit/indexes" : undefined,
    }),
    buildStorageItem({
      id: "quickwit",
      label: "Quickwit sparse corpus",
      ready: isReady(quickwitAdapterReady) && quickwitIndexIds.includes("dataplane-corpus"),
      detail: isReady(quickwitAdapterReady) && quickwitIndexIds.includes("dataplane-corpus")
        ? `dataplane-corpus is available and retrieval reports ${sparseBackend || "an unknown sparse backend"}.`
        : "Quickwit sparse corpus could not be confirmed from the live runtime.",
      meta: joinNames(quickwitIndexIds, 2),
    }),
  ];

  const capabilities: LiveKnowledgeDiagnosticItem[] = [
    buildCapabilityItem({
      id: "embeddings",
      label: "Embedding system",
      status: isReady(embeddingReady) && vectorCollections.includes(PRIMARY_VECTOR_COLLECTION) ? "Live" : "Degraded",
      tone: isReady(embeddingReady) && vectorCollections.includes(PRIMARY_VECTOR_COLLECTION) ? "good" : "warn",
      detail: isReady(embeddingReady) && vectorCollections.includes(PRIMARY_VECTOR_COLLECTION)
        ? "embedding-engine is ready and the primary dataplane_knowledge vector collection exists."
        : "The embedding path is present, but the primary vector collection could not be confirmed.",
      meta: joinNames(vectorCollections, 2),
    }),
    buildCapabilityItem({
      id: "graphrag",
      label: "GraphRAG",
      status: isReady(retrievalReady) && isReady(graphReady) ? "Live" : "Degraded",
      tone: isReady(retrievalReady) && isReady(graphReady) ? "good" : "warn",
      detail: isReady(retrievalReady) && isReady(graphReady)
        ? "retrieve/graph and graph-index are both live for retrieval-grounded graph expansion."
        : "Graph retrieval is wired, but one of the live graph services is not healthy.",
      meta: input.graphAvailable
        ? `${formatCount(input.graphNodeCount)} nodes · ${formatCount(input.graphEdgeCount)} edges`
        : "no surfaced entities",
    }),
    buildCapabilityItem({
      id: "agentic-rag",
      label: "Agentic RAG",
      status: isReady(retrievalReady) ? "Supported" : "Degraded",
      tone: isReady(retrievalReady) ? "neutral" : "warn",
      detail: isReady(retrievalReady)
        ? "Retrieval-engine supports agent-scoped configs and suggested next tools, but this page is not running a dedicated agentic eval."
        : "Agent-aware retrieval is defined in Data Plane v2, but the live retrieval service is degraded.",
      meta: "agent_retrieval_configs + suggested_next_tools",
    }),
    buildCapabilityItem({
      id: "llm-wiki",
      label: "LLM wiki",
      status: isReady(wikiReady) && vectorCollections.includes(WIKI_VECTOR_COLLECTION) ? "Live" : "Degraded",
      tone: isReady(wikiReady) && vectorCollections.includes(WIKI_VECTOR_COLLECTION) ? "good" : "warn",
      detail: isReady(wikiReady) && vectorCollections.includes(WIKI_VECTOR_COLLECTION)
        ? "wiki-store is ready and wiki_block_embeddings is provisioned for wiki retrieval."
        : "Wiki storage exists, but the runtime wiki embedding path could not be fully confirmed.",
      meta: WIKI_VECTOR_COLLECTION,
    }),
    buildCapabilityItem({
      id: "context-pack",
      label: "Context pack",
      status: isReady(retrievalReady) ? "Live" : "Degraded",
      tone: isReady(retrievalReady) ? "good" : "warn",
      detail: isReady(retrievalReady)
        ? "Standalone context packing is exposed by the live retrieval-engine."
        : "The context packing surface exists in code, but the live retrieval service is degraded.",
      meta: "/v1/retrieve/pack",
    }),
    buildCapabilityItem({
      id: "context-mode",
      label: "Context mode",
      status: "Not wired",
      tone: "warn",
      detail: "No current Data Plane v2 implementation was detected for a dedicated context mode surface.",
    }),
    buildCapabilityItem({
      id: "mempalace",
      label: "MemPalace",
      status: "Not wired",
      tone: "warn",
      detail: "No current Data Plane v2 implementation was detected for mempalace or memory-palace retrieval.",
    }),
  ];

  return {
    available:
      services.some((item) => item.tone !== "bad") ||
      storage.some((item) => item.tone !== "bad") ||
      capabilities.length > 0,
    sparseBackend: sparseBackend || null,
    vectorCollections,
    quickwitIndexes: quickwitIndexIds,
    services,
    storage,
    capabilities,
  };
}

async function loadReadyz(url: string): Promise<ReadyzResponse | null> {
  return loadJson<ReadyzResponse>(url);
}

async function loadQdrantCollections(): Promise<string[]> {
  const payload = await loadJson<QdrantCollectionsResponse>(`${getQdrantUrl()}/collections`);
  const collections = Array.isArray(payload?.result?.collections) ? payload.result.collections : [];
  return collections
    .map((collection) => stringValue(collection.name))
    .filter((value): value is string => Boolean(value));
}

async function loadQuickwitIndexes(): Promise<QuickwitIndexSummary[]> {
  const payload = await loadJson<QuickwitIndexResponse>(`${getQuickwitUrl()}/api/v1/indexes`);
  const indexes = Array.isArray(payload) ? payload : [];
  return indexes.flatMap((index) => {
    const id = stringValue(index.index_config?.index_id);
    const uri = stringValue(index.index_config?.index_uri);
    if (!id || !uri) return [];
    return [{ id, uri }];
  });
}

async function loadMinioReady(): Promise<boolean> {
  try {
    const response = await fetch(`${getMinioUrl()}/minio/health/live`, {
      method: "GET",
      cache: "no-store",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    return response.ok;
  } catch {
    return false;
  }
}

async function loadJson<T>(url: string): Promise<T | null> {
  try {
    const response = await fetch(url, {
      method: "GET",
      cache: "no-store",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    return (await response.json().catch(() => null)) as T | null;
  } catch {
    return null;
  }
}

function buildServiceItem(input: {
  id: string;
  label: string;
  ready: boolean;
  detail: string;
  meta?: string;
}): LiveKnowledgeDiagnosticItem {
  return {
    id: input.id,
    label: input.label,
    status: input.ready ? "Ready" : "Unavailable",
    tone: input.ready ? "good" : "bad",
    detail: input.detail,
    meta: input.meta,
  };
}

function buildStorageItem(input: {
  id: string;
  label: string;
  ready: boolean;
  detail: string;
  meta?: string;
}): LiveKnowledgeDiagnosticItem {
  return {
    id: input.id,
    label: input.label,
    status: input.ready ? "Ready" : "Degraded",
    tone: input.ready ? "good" : "warn",
    detail: input.detail,
    meta: input.meta,
  };
}

function buildCapabilityItem(input: {
  id: string;
  label: string;
  status: string;
  tone: LiveKnowledgeDiagnosticTone;
  detail: string;
  meta?: string;
}): LiveKnowledgeDiagnosticItem {
  return input;
}

function isReady(value: ReadyzResponse | null): boolean {
  return stringValue(value?.status) === "ready";
}

function recordValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function booleanValue(value: unknown): boolean {
  return value === true;
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function serviceLabel(value: ReadyzResponse | null): string | undefined {
  const service = stringValue(value?.service);
  return service || undefined;
}

function joinNames(values: string[], limit: number): string {
  const selected = values.slice(0, limit);
  if (selected.length === 0) return "none";
  const suffix = values.length > selected.length ? ` +${values.length - selected.length}` : "";
  return `${selected.join(", ")}${suffix}`;
}

function formatCount(value: number): string {
  return new Intl.NumberFormat("en-US").format(Math.max(0, Math.trunc(value)));
}
