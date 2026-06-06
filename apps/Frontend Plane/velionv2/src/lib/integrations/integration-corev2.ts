import "server-only";

import type { NextRequest } from "next/server";

import {
  requireSession,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getDataPlaneAudience,
  getDataPlaneDocumentsUrl,
  getGraphIndexUrl,
  getIntegrationCoreUrl,
  mintAudienceToken,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { buildIntegrationCoreHeaders } from "@/app/api/v1/integrations/_lib/service-auth";

export type IntegrationCapability = {
  key: string;
  label?: string;
  description?: string;
  sensitive?: boolean;
};

export type IntegrationProviderSummary = {
  key: string;
  label: string;
  category: string;
  authType: string;
  connectorType: string;
  configured: boolean;
  status: "ready" | "connected" | "missing_config" | "manual_or_admin_config" | "unavailable";
  missingConfig: string[];
  directOAuthReady: boolean;
  capabilities: IntegrationCapability[];
};

export type IntegrationConnectionSummary = {
  id: string;
  providerKey: string;
  providerLabel: string;
  connectorType: string;
  displayName: string;
  status: string;
  capabilities: string[];
  scopeCount: number;
  syncStatus: string;
  latestSyncJob?: {
    id: string;
    status: string;
    mode?: string;
    reason?: string;
    updatedAt?: string;
  };
  discovery?: SafeDiscoverySnapshot;
  updatedAt?: string;
  deletedAt?: string | null;
};

export type SafeDiscoverySnapshot = {
  connectionId: string;
  providerKey: string;
  workspaceName?: string;
  workspaceId?: string;
  accountName?: string;
  entityCounts: Record<string, number>;
  sampleEntities: Array<{ kind: string; label: string }>;
  availability: Record<string, boolean>;
  sensitivity: "safe_metadata_only";
  fetchedAt?: string;
  providerWarnings?: string[];
};

export type IntegrationSummary = {
  configured: boolean;
  orgId: string | null;
  providers: IntegrationProviderSummary[];
  connections: IntegrationConnectionSummary[];
  metrics: {
    totalProviders: number;
    readyProviders: number;
    connected: number;
    syncing: number;
    failed: number;
  };
  graph?: {
    available: boolean;
    nodeCount: number;
    edgeCount: number;
    groups: string[];
  };
  generatedAt: string;
};

export type DataPlaneDocumentSummary = {
  id: string;
  title: string;
  source: string;
  type: string;
  status: string;
  content?: string;
  createdAt?: string;
  createdBy?: string;
  updatedAt?: string;
};

type LoadIntegrationSummaryOptions = {
  includeDiscovery?: boolean;
  includeGraph?: boolean;
};

type IntegrationCoreProvider = {
  key?: unknown;
  label?: unknown;
  connectorType?: unknown;
  authType?: unknown;
  directOAuthReady?: unknown;
  configured?: unknown;
  status?: unknown;
  missingConfig?: unknown;
  capabilities?: unknown;
};

type IntegrationCoreConnection = {
  id?: unknown;
  providerKey?: unknown;
  provider_key?: unknown;
  connectorType?: unknown;
  connector_type?: unknown;
  displayName?: unknown;
  display_name?: unknown;
  status?: unknown;
  capabilities?: unknown;
  scopes?: unknown;
  lastSyncStatus?: unknown;
  last_sync_status?: unknown;
  updatedAt?: unknown;
  updated_at?: unknown;
  deletedAt?: unknown;
  deleted_at?: unknown;
};

type IntegrationCoreSyncJob = {
  id?: unknown;
  connectionId?: unknown;
  connection_id?: unknown;
  status?: unknown;
  reason?: unknown;
  mode?: unknown;
  updatedAt?: unknown;
  updated_at?: unknown;
};

type GraphIndexResponse = {
  nodes?: Array<{ entity_type?: unknown }>;
  node_count?: unknown;
  edge_count?: unknown;
};

type DataPlaneDocument = {
  document_id?: unknown;
  documentId?: unknown;
  title?: unknown;
  source?: unknown;
  type?: unknown;
  status?: unknown;
  content?: unknown;
  created_at?: unknown;
  createdAt?: unknown;
  created_by?: unknown;
  createdBy?: unknown;
  updated_at?: unknown;
  updatedAt?: unknown;
};

const REQUEST_TIMEOUT_MS = 5_000;
const DISCOVERY_TIMEOUT_MS = 2_000;
const MAX_DISCOVERY_CONNECTIONS = 8;

export async function loadIntegrationSummary(
  request: NextRequest,
  options: LoadIntegrationSummaryOptions = {},
): Promise<IntegrationSummary> {
  const session = await requireSession(request);
  const orgId = await resolveActiveOrgId(request, session);
  if (!orgId) {
    return emptySummary(null);
  }

  const headers = buildIntegrationCoreHeaders(request, session, orgId);
  const providerUrl = `${getIntegrationCoreUrl()}/api/v1/providers`;
  const connectionsUrl = new URL(`${getIntegrationCoreUrl()}/api/v1/connections`);
  connectionsUrl.searchParams.set("organizationId", orgId);
  const syncJobsUrl = new URL(`${getIntegrationCoreUrl()}/api/v1/sync-jobs`);
  syncJobsUrl.searchParams.set("organizationId", orgId);

  const [providersPayload, connectionsPayload, syncJobsPayload] = await Promise.all([
    fetchIntegrationCore(providerUrl, headers),
    fetchIntegrationCore(connectionsUrl.toString(), headers),
    fetchIntegrationCore(syncJobsUrl.toString(), headers),
  ]);

  const providers = providerSummaries(arrayFromPayload(providersPayload, "providers"));
  const jobsByConnection = latestSyncJobsByConnection(arrayFromPayload(syncJobsPayload, "syncJobs"));
  const connections = connectionSummaries(
    arrayFromPayload(connectionsPayload, "connections"),
    providerLabelMap(providers),
    jobsByConnection,
  );

  const connectionsWithDiscovery = options.includeDiscovery
    ? await attachDiscovery(request, session, orgId, connections)
    : connections;

  return {
    configured: true,
    orgId,
    providers: providersWithConnectedStatus(providers, connectionsWithDiscovery),
    connections: connectionsWithDiscovery,
    metrics: buildMetrics(providers, connectionsWithDiscovery),
    graph: options.includeGraph ? await loadGraphSummary(request, session, orgId) : undefined,
    generatedAt: new Date().toISOString(),
  };
}

export function integrationSourcesFromSummary(summary: IntegrationSummary) {
  return summary.connections
    .filter((connection) => !connection.deletedAt)
    .map((connection) => {
      const counts = connection.discovery?.entityCounts ?? {};
      const sampleEntities = connection.discovery?.sampleEntities ?? [];
      const countLabel = Object.entries(counts)
        .slice(0, 3)
        .map(([key, value]) => `${formatCountKey(key)}: ${value}`)
        .join(" · ");
      const availability = Object.entries(connection.discovery?.availability ?? {})
        .filter(([, available]) => available)
        .map(([key]) => formatCountKey(key))
        .slice(0, 4);

      return {
        id: connection.id,
        title: connection.discovery?.workspaceName || connection.displayName || connection.providerLabel,
        provider: connection.providerLabel,
        providerKey: connection.providerKey,
        status: connection.syncStatus || connection.status,
        detail: countLabel || availability.join(" · ") || "Connected source metadata",
        counts,
        samples: sampleEntities.slice(0, 4),
        capabilities: connection.capabilities,
        syncJob: connection.latestSyncJob ?? null,
        readOnly: true,
        manageHref: "/knowledge",
      };
    });
}

export async function loadDataPlaneDocumentSummaries(
  request: NextRequest,
): Promise<DataPlaneDocumentSummary[]> {
  const token = bearerTokenFromRequest(request) ?? await mintAudienceToken(request, getDataPlaneAudience());
  if (!token) return [];

  try {
    const url = new URL(`${getDataPlaneDocumentsUrl()}/v1/documents`);
    url.searchParams.set("limit", "100");
    url.searchParams.set("offset", "0");
    const response = await fetch(url, {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
      cache: "no-store",
      signal: AbortSignal.timeout(2_500),
    });
    if (!response.ok) return [];
    const payload = await response.json().catch(() => null) as unknown;
    return documentsFromPayload(payload);
  } catch {
    return [];
  }
}

export function knowledgeSourcesFromSummary(
  summary: IntegrationSummary,
  documents: DataPlaneDocumentSummary[],
) {
  const dataPlaneSources = dataPlaneSourcesFromDocuments(documents);
  const seenProviderKeys = new Set(dataPlaneSources.map((source) => source.providerKey));
  const integrationSources = integrationSourcesFromSummary(summary)
    .filter((source) => !seenProviderKeys.has(source.providerKey))
    .map((source) => ({ ...source, backing: "integration_metadata" as const }));
  return [...dataPlaneSources, ...integrationSources];
}

function emptySummary(orgId: string | null): IntegrationSummary {
  return {
    configured: false,
    orgId,
    providers: [],
    connections: [],
    metrics: {
      totalProviders: 0,
      readyProviders: 0,
      connected: 0,
      syncing: 0,
      failed: 0,
    },
    generatedAt: new Date().toISOString(),
  };
}

function bearerTokenFromRequest(request: NextRequest): string | null {
  const authorization = request.headers.get("authorization")?.trim();
  if (!authorization?.toLowerCase().startsWith("bearer ")) return null;
  const token = authorization.slice("bearer ".length).trim();
  return token || null;
}

async function fetchIntegrationCore(url: string, headers: Record<string, string>) {
  try {
    const response = await fetch(url, {
      method: "GET",
      headers,
      cache: "no-store",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    return response.json().catch(() => null) as Promise<unknown>;
  } catch {
    return null;
  }
}

function arrayFromPayload(payload: unknown, key: string): unknown[] {
  if (!payload || typeof payload !== "object") return [];
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  return Array.isArray(data[key]) ? data[key] : [];
}

function providerSummaries(rawProviders: unknown[]): IntegrationProviderSummary[] {
  return rawProviders.flatMap((raw) => {
    if (!raw || typeof raw !== "object") return [];
    const provider = raw as IntegrationCoreProvider;
    const key = stringValue(provider.key);
    if (!key) return [];
    const configured = boolValue(provider.configured);
    const status = providerStatus(stringValue(provider.status), configured);
    return [{
      key,
      label: stringValue(provider.label) || key,
      category: providerCategory(key, stringValue(provider.connectorType)),
      authType: stringValue(provider.authType) || "unknown",
      connectorType: stringValue(provider.connectorType) || key,
      configured,
      status,
      missingConfig: stringArray(provider.missingConfig),
      directOAuthReady: boolValue(provider.directOAuthReady),
      capabilities: capabilitySummaries(provider.capabilities),
    }];
  });
}

function capabilitySummaries(value: unknown): IntegrationCapability[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const record = item as Record<string, unknown>;
    const key = stringValue(record.key);
    if (!key) return [];
    return [{
      key,
      label: stringValue(record.label) || key,
      description: stringValue(record.description),
      sensitive: boolValue(record.sensitive),
    }];
  });
}

function connectionSummaries(
  rawConnections: unknown[],
  labels: Map<string, string>,
  jobsByConnection: Map<string, IntegrationConnectionSummary["latestSyncJob"]>,
): IntegrationConnectionSummary[] {
  return rawConnections.flatMap((raw) => {
    if (!raw || typeof raw !== "object") return [];
    const connection = raw as IntegrationCoreConnection;
    const id = stringValue(connection.id);
    const providerKey = normalizeProviderKey(stringValue(connection.providerKey) || stringValue(connection.provider_key));
    if (!id || !providerKey) return [];
    const latestSyncJob = jobsByConnection.get(id);
    const status = stringValue(connection.status) || "unknown";
    return [{
      id,
      providerKey,
      providerLabel: labels.get(providerKey) || providerLabel(providerKey),
      connectorType: stringValue(connection.connectorType) || stringValue(connection.connector_type) || providerKey,
      displayName: stringValue(connection.displayName) || stringValue(connection.display_name) || labels.get(providerKey) || providerLabel(providerKey),
      status,
      capabilities: stringArray(connection.capabilities),
      scopeCount: stringArray(connection.scopes).length,
      syncStatus: stringValue(connection.lastSyncStatus) || stringValue(connection.last_sync_status) || latestSyncJob?.status || status,
      latestSyncJob,
      updatedAt: stringValue(connection.updatedAt) || stringValue(connection.updated_at),
      deletedAt: stringValue(connection.deletedAt) || stringValue(connection.deleted_at) || null,
    }];
  });
}

function latestSyncJobsByConnection(rawJobs: unknown[]) {
  const jobs = new Map<string, IntegrationConnectionSummary["latestSyncJob"]>();
  for (const raw of rawJobs) {
    if (!raw || typeof raw !== "object") continue;
    const job = raw as IntegrationCoreSyncJob;
    const connectionId = stringValue(job.connectionId) || stringValue(job.connection_id);
    const id = stringValue(job.id);
    if (!connectionId || !id) continue;
    const next = {
      id,
      status: stringValue(job.status) || "unknown",
      mode: stringValue(job.mode),
      reason: stringValue(job.reason),
      updatedAt: stringValue(job.updatedAt) || stringValue(job.updated_at),
    };
    const current = jobs.get(connectionId);
    if (!current || String(next.updatedAt ?? "") > String(current.updatedAt ?? "")) {
      jobs.set(connectionId, next);
    }
  }
  return jobs;
}

async function attachDiscovery(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  connections: IntegrationConnectionSummary[],
): Promise<IntegrationConnectionSummary[]> {
  const headers = buildIntegrationCoreHeaders(request, session, orgId);
  const activeConnections = connections.filter((connection) => !connection.deletedAt).slice(0, MAX_DISCOVERY_CONNECTIONS);
  const snapshots = await Promise.allSettled(
    activeConnections.map(async (connection) => {
      const response = await fetch(
        `${getIntegrationCoreUrl()}/api/v1/connections/${encodeURIComponent(connection.id)}/discovery`,
        {
          method: "GET",
          headers,
          cache: "no-store",
          signal: AbortSignal.timeout(DISCOVERY_TIMEOUT_MS),
        },
      );
      if (!response.ok) return null;
      const payload = await response.json().catch(() => null) as unknown;
      return safeDiscoveryFromPayload(payload);
    }),
  );
  const byConnection = new Map<string, SafeDiscoverySnapshot>();
  for (const result of snapshots) {
    if (result.status === "fulfilled" && result.value) {
      byConnection.set(result.value.connectionId, result.value);
    }
  }
  return connections.map((connection) => ({
    ...connection,
    discovery: byConnection.get(connection.id),
  }));
}

function safeDiscoveryFromPayload(payload: unknown): SafeDiscoverySnapshot | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  const discovery = data.discovery && typeof data.discovery === "object"
    ? data.discovery as Record<string, unknown>
    : data;
  const connectionId = stringValue(discovery.connectionId);
  const providerKey = stringValue(discovery.providerKey);
  if (!connectionId || !providerKey) return null;
  return {
    connectionId,
    providerKey: normalizeProviderKey(providerKey),
    workspaceName: stringValue(discovery.workspaceName),
    workspaceId: stringValue(discovery.workspaceId),
    accountName: stringValue(discovery.accountName),
    entityCounts: numberRecord(discovery.entityCounts),
    sampleEntities: sampleEntities(discovery.sampleEntities),
    availability: boolRecord(discovery.availability),
    sensitivity: "safe_metadata_only",
    fetchedAt: stringValue(discovery.fetchedAt),
    providerWarnings: stringArray(discovery.providerWarnings),
  };
}

function documentsFromPayload(payload: unknown): DataPlaneDocumentSummary[] {
  if (!payload || typeof payload !== "object") return [];
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  const documents = Array.isArray(data.documents) ? data.documents : [];
  return documents.flatMap((raw) => {
    if (!raw || typeof raw !== "object") return [];
    const document = raw as DataPlaneDocument;
    const id = stringValue(document.document_id) ?? stringValue(document.documentId);
    const source = stringValue(document.source);
    const title = stringValue(document.title);
    if (!id || !source || !title) return [];
    return [{
      id,
      title,
      source,
      type: stringValue(document.type) ?? "document",
      status: stringValue(document.status) ?? "unknown",
      content: stringValue(document.content),
      createdAt: stringValue(document.created_at) ?? stringValue(document.createdAt),
      createdBy: stringValue(document.created_by) ?? stringValue(document.createdBy),
      updatedAt: stringValue(document.updated_at) ?? stringValue(document.updatedAt),
    }];
  });
}

function dataPlaneSourcesFromDocuments(documents: DataPlaneDocumentSummary[]) {
  const groups = new Map<string, DataPlaneDocumentSummary[]>();
  for (const document of documents) {
    const key = normalizeProviderKey(document.source) || document.source;
    groups.set(key, [...(groups.get(key) ?? []), document]);
  }

  return Array.from(groups.entries()).map(([source, docs]) => {
    const statusCounts = docs.reduce<Record<string, number>>((acc, document) => {
      acc[document.status] = (acc[document.status] ?? 0) + 1;
      return acc;
    }, {});
    const indexedCount = statusCounts.indexed ?? statusCounts.active ?? statusCounts.completed ?? 0;
    return {
      id: `dataplane:${source}`,
      title: sourceLabel(source),
      provider: "Data Plane",
      providerKey: source,
      status: indexedCount > 0 ? "indexed" : docs[0]?.status ?? "pending",
      detail: `${docs.length} documents · ${indexedCount} indexed`,
      counts: {
        documents: docs.length,
        indexed: indexedCount,
        ...statusCounts,
      },
      samples: docs.slice(0, 4).map((document) => ({ kind: document.type, label: document.title })),
      capabilities: [],
      syncJob: null,
      readOnly: true,
      manageHref: "/knowledge",
      backing: "data_plane" as const,
    };
  });
}

function sourceLabel(source: string) {
  const normalized = normalizeProviderKey(source);
  if (["microsoft", "google", "github", "notion", "slack", "shopify", "stripe", "okta", "scim"].includes(normalized)) {
    return providerLabel(normalized);
  }
  if (source.startsWith("onboarding:")) {
    const [, provider] = source.split(":");
    const onboardingProvider = normalizeProviderKey(provider);
    return onboardingProvider ? providerLabel(onboardingProvider) : "Onboarding source";
  }
  return source.replace(/^onboarding:/, "").replace(/[_:-]+/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}

async function loadGraphSummary(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
): Promise<IntegrationSummary["graph"]> {
  try {
    const response = await fetch(
      `${getGraphIndexUrl()}/v1/graphs/${encodeURIComponent(orgId)}?limit_nodes=120&limit_edges=240`,
      {
        method: "GET",
        headers: buildServiceHeaders(request, session, orgId),
        cache: "no-store",
        signal: AbortSignal.timeout(1_500),
      },
    );
    if (!response.ok) {
      return { available: false, nodeCount: 0, edgeCount: 0, groups: [] };
    }
    const body = await response.json().catch(() => null) as GraphIndexResponse | null;
    if (!body) {
      return { available: false, nodeCount: 0, edgeCount: 0, groups: [] };
    }
    const groups = Array.from(
      new Set(
        (body.nodes ?? [])
          .map((node) => stringValue(node.entity_type)?.toLowerCase())
          .filter((group): group is string => Boolean(group && group !== "org")),
      ),
    ).slice(0, 8);
    return {
      available: true,
      nodeCount: numberValue(body.node_count),
      edgeCount: numberValue(body.edge_count),
      groups,
    };
  } catch {
    return { available: false, nodeCount: 0, edgeCount: 0, groups: [] };
  }
}

function providersWithConnectedStatus(
  providers: IntegrationProviderSummary[],
  connections: IntegrationConnectionSummary[],
): IntegrationProviderSummary[] {
  const connected = new Set(
    connections
      .filter((connection) => !connection.deletedAt && connection.status !== "deleted")
      .map((connection) => connection.providerKey),
  );
  return providers.map((provider) => ({
    ...provider,
    status: connected.has(provider.key) ? "connected" : provider.status,
  }));
}

function buildMetrics(
  providers: IntegrationProviderSummary[],
  connections: IntegrationConnectionSummary[],
): IntegrationSummary["metrics"] {
  const activeConnections = connections.filter((connection) => !connection.deletedAt);
  return {
    totalProviders: providers.length,
    readyProviders: providers.filter((provider) => provider.configured).length,
    connected: activeConnections.length,
    syncing: activeConnections.filter((connection) =>
      ["queued", "running", "waiting_provider", "handoff_data_plane", "syncing"].includes(connection.syncStatus),
    ).length,
    failed: activeConnections.filter((connection) =>
      ["failed", "error", "needs_refresh"].includes(connection.syncStatus),
    ).length,
  };
}

function providerLabelMap(providers: IntegrationProviderSummary[]) {
  return new Map(providers.map((provider) => [provider.key, provider.label]));
}

function providerStatus(
  rawStatus: string | undefined,
  configured: boolean,
): IntegrationProviderSummary["status"] {
  if (!configured) return "missing_config";
  switch (rawStatus) {
    case "ready":
    case "connected":
    case "missing_config":
    case "manual_or_admin_config":
      return rawStatus;
    default:
      return configured ? "ready" : "unavailable";
  }
}

function providerCategory(key: string, connectorType?: string) {
  const normalized = normalizeProviderKey(key || connectorType);
  switch (normalized) {
    case "microsoft":
    case "google":
    case "notion":
      return "knowledge";
    case "slack":
      return "chat";
    case "github":
      return "code";
    case "shopify":
    case "stripe":
      return "commerce";
    case "okta":
    case "scim":
      return "identity";
    default:
      return "workspace";
  }
}

function providerLabel(providerKey: string) {
  switch (normalizeProviderKey(providerKey)) {
    case "microsoft":
      return "Microsoft 365";
    case "google":
      return "Google Workspace";
    case "github":
      return "GitHub";
    case "notion":
      return "Notion";
    case "slack":
      return "Slack";
    case "shopify":
      return "Shopify";
    case "stripe":
      return "Stripe";
    case "okta":
      return "Okta";
    case "scim":
      return "SCIM";
    default:
      return providerKey;
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

function sampleEntities(value: unknown): SafeDiscoverySnapshot["sampleEntities"] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const record = item as Record<string, unknown>;
    const label = stringValue(record.label);
    if (!label || label.length > 100 || label.includes("@")) return [];
    return [{ kind: stringValue(record.kind) || "entity", label }];
  }).slice(0, 6);
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function boolValue(value: unknown): boolean {
  return value === true;
}

function numberValue(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
}

function numberRecord(value: unknown): Record<string, number> {
  if (!value || typeof value !== "object") return {};
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter((entry): entry is [string, number] => typeof entry[1] === "number" && Number.isFinite(entry[1])),
  );
}

function boolRecord(value: unknown): Record<string, boolean> {
  if (!value || typeof value !== "object") return {};
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter((entry): entry is [string, boolean] => typeof entry[1] === "boolean"),
  );
}

function formatCountKey(key: string) {
  return key.replace(/_/g, " ").replace(/\b\w/g, (char) => char.toUpperCase());
}
