import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  buildControlPlaneHeaders,
  getInternalApiKey,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  getDataPlaneDocumentsUrl,
  getFinspoCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { buildSafeConnectorMetadata } from "@/features/onboarding-v2/lib/onboarding-evidence";
import type { ConnectorPick, SafeConnectorMetadata } from "@/features/onboarding-v2/lib/onboarding-machine";

export const dynamic = "force-dynamic";

const ALLOWED_PROVIDERS = new Set(["slack", "microsoft", "notion", "google", "google-drive", "github"]);
const DISCOVERY_TIMEOUT_MS = 4_500;
const INGEST_TIMEOUT_MS = 5_000;

type DiscoveryBody = {
  provider?: string;
  connectorId?: string;
  label?: string;
  sources?: string[];
};
type SafeMetadataInput = NonNullable<Parameters<typeof buildSafeConnectorMetadata>[1]>;
type ConnectionRecord = {
  id?: unknown;
  providerKey?: unknown;
  provider_key?: unknown;
  providerLabel?: unknown;
  provider_label?: unknown;
  workspaceId?: unknown;
  workspace_id?: unknown;
};

export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as DiscoveryBody | null;
    const provider = normalizeProvider(body?.provider ?? body?.connectorId);
    if (!provider || !ALLOWED_PROVIDERS.has(provider)) {
      return NextResponse.json({ error: { code: "unsupported_provider", message: "Unsupported provider." } }, { status: 400 });
    }

    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json({ error: { code: "no_org", message: "No active organization found." } }, { status: 409 });
    }

    const connector: ConnectorPick = {
      id: body?.connectorId || provider,
      label: body?.label || providerLabel(provider),
    };
    const connection = await fetchLatestConnection({
      request,
      session,
      orgId,
      provider,
    });
    const metadata = await safeMetadataForProvider({
      provider,
      connector,
      orgId,
      userId: session.user.id,
      sources: Array.isArray(body?.sources) ? body?.sources : [],
      connection,
    });
    const graphSeed = await seedDataPlaneSourceIntro({
      metadata,
      connector,
      orgId,
      userId: session.user.id,
      provider,
    });

    return NextResponse.json({
      metadata: graphSeed.documentId ? { ...metadata, seedDocumentId: graphSeed.documentId } : metadata,
      graphSeedStatus: graphSeed.status,
      graphSeedDocumentId: graphSeed.documentId,
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}

async function safeMetadataForProvider(input: {
  provider: string;
  connector: Pick<ConnectorPick, "id" | "label">;
  orgId: string;
  userId: string;
  sources: string[];
  connection?: ConnectionRecord | null;
}): Promise<SafeConnectorMetadata> {
  const base = providerDefaults(input.provider, input.sources);
  const workspaceName = safeWorkspaceName(input.connection) ?? base.workspaceName;
  if (input.provider === "microsoft") {
    const sharePoint = await fetchSharePointCount(input.orgId, input.userId);
    return buildSafeConnectorMetadata(input.connector, {
      ...base,
      workspaceName,
      entityCounts: {
        ...(base.entityCounts ?? {}),
        ...(sharePoint.count != null ? { sites: sharePoint.count } : {}),
      },
      scopes: [...(base.scopes ?? []), "sharepoint", "onedrive", "teams", "outlook"],
    });
  }
  return buildSafeConnectorMetadata(input.connector, { ...base, workspaceName });
}

async function fetchLatestConnection(input: {
  request: NextRequest;
  session: Awaited<ReturnType<typeof requireSession>>;
  orgId: string;
  provider: string;
}): Promise<ConnectionRecord | null> {
  try {
    const listUrl = new URL(`${getIntegrationCoreUrl()}/api/v1/connections`);
    listUrl.searchParams.set("organizationId", input.orgId);
    listUrl.searchParams.set("providerKey", input.provider);
    const response = await fetch(listUrl, {
      method: "GET",
      headers: buildControlPlaneHeaders(input.request, input.session),
      cache: "no-store",
      signal: AbortSignal.timeout(DISCOVERY_TIMEOUT_MS),
    });
    if (!response.ok) return null;
    const body = (await response.json().catch(() => null)) as
      | { data?: { connections?: unknown }; connections?: unknown }
      | null;
    const raw = Array.isArray(body?.data?.connections)
      ? body.data.connections
      : Array.isArray(body?.connections)
        ? body.connections
        : [];
    return raw.find((item): item is ConnectionRecord => Boolean(item && typeof item === "object")) ?? null;
  } catch {
    return null;
  }
}

function safeWorkspaceName(connection: ConnectionRecord | null | undefined): string | undefined {
  const workspaceId = stringValue(connection?.workspaceId) ?? stringValue(connection?.workspace_id);
  const providerLabel = stringValue(connection?.providerLabel) ?? stringValue(connection?.provider_label);
  const value = workspaceId ?? providerLabel;
  if (!value || value.length > 80 || value.includes("@")) return undefined;
  return value;
}

function providerDefaults(provider: string, sources: string[]): SafeMetadataInput {
  const sourceSet = new Set(sources.map((source) => source.toLowerCase()));
  switch (provider) {
    case "slack":
      return {
        workspaceName: "Slack workspace",
        entityCounts: { channels: 0 },
        sampleEntities: ["Public channels"],
        scopes: ["channels"],
      };
    case "github":
      return {
        workspaceName: "GitHub workspace",
        entityCounts: { repos: 0 },
        sampleEntities: ["Repositories", "README", "Issues"],
        scopes: ["repos", "readme", "issues", "wiki"],
      };
    case "notion":
      return {
        workspaceName: "Notion workspace",
        entityCounts: { pages: 0, databases: 0 },
        sampleEntities: ["Pages", "Databases"],
        scopes: ["pages", "databases"],
      };
    case "google":
    case "google-drive":
      return {
        workspaceName: "Google Drive",
        entityCounts: { documents: 0, folders: 0 },
        sampleEntities: ["Shared folders", "Documents"],
        scopes: ["google_drive", "documents"],
      };
    case "microsoft":
      return {
        workspaceName: "Microsoft 365 tenant",
        entityCounts: { sites: 0 },
        sampleEntities: [
          sourceSet.has("sharepoint") ? "SharePoint sites" : "Microsoft 365 workspace",
          sourceSet.has("teams") ? "Teams availability" : "OneDrive availability",
        ],
        scopes: sources,
      };
    default:
      return {
        workspaceName: providerLabel(provider),
        entityCounts: {},
        sampleEntities: [],
        scopes: sources,
      };
  }
}

async function fetchSharePointCount(orgId: string, userId: string): Promise<{ count?: number }> {
  try {
    const response = await fetch(`${getFinspoCoreUrl()}/api/v1/sharepoint/sites`, {
      method: "GET",
      headers: {
        Accept: "application/json",
        "X-API-Key": getInternalApiKey(),
        "x-internal-api-key": getInternalApiKey(),
        "X-Org-ID": orgId,
        "X-User-ID": userId,
      },
      cache: "no-store",
      signal: AbortSignal.timeout(DISCOVERY_TIMEOUT_MS),
    });
    if (!response.ok) return {};
    const body = (await response.json().catch(() => null)) as { data?: { count?: number; sites?: unknown[] } } | null;
    return {
      count:
        typeof body?.data?.count === "number"
          ? body.data.count
          : Array.isArray(body?.data?.sites)
            ? body.data.sites.length
            : undefined,
    };
  } catch {
    return {};
  }
}

async function seedDataPlaneSourceIntro(input: {
  metadata: SafeConnectorMetadata;
  connector: Pick<ConnectorPick, "id" | "label">;
  orgId: string;
  userId: string;
  provider: string;
}): Promise<{ status: "pending" | "ready" | "failed"; documentId?: string }> {
  try {
    const content = [
      `${input.connector.label} is connected to Verevon for onboarding.`,
      "This onboarding source map contains safe metadata only, not private message or document contents.",
      input.metadata.workspaceName ? `Workspace: ${input.metadata.workspaceName}.` : null,
      input.metadata.entityCounts
        ? `Counts: ${Object.entries(input.metadata.entityCounts).map(([key, value]) => `${key}=${value}`).join(", ")}.`
        : null,
      input.metadata.scopes?.length ? `Scopes: ${input.metadata.scopes.join(", ")}.` : null,
    ]
      .filter(Boolean)
      .join("\n");
    const response = await fetch(`${getDataPlaneDocumentsUrl()}/v1/documents`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Internal-Api-Key": getInternalApiKey(),
        "X-Org-ID": input.orgId,
        "X-User-Id": input.userId,
      },
      cache: "no-store",
      signal: AbortSignal.timeout(INGEST_TIMEOUT_MS),
      body: JSON.stringify({
        source: `onboarding:${input.provider}:${input.connector.id}`,
        type: "integration_source_metadata",
        title: `${input.connector.label} onboarding source map`,
        content,
        created_by: input.userId,
        idempotency_key: `onboarding-source:${input.orgId}:${input.connector.id}`,
        zdr_classification: "internal",
        metadata: {
          connector_id: input.connector.id,
          provider: input.provider,
          sensitivity: input.metadata.sensitivity,
          safe_metadata: input.metadata,
        },
      }),
    });
    if (!response.ok) return { status: "pending" };
    const body = (await response.json().catch(() => null)) as
      | { document_id?: unknown; documentId?: unknown; data?: { document_id?: unknown; documentId?: unknown } }
      | null;
    return {
      status: "ready",
      documentId:
        stringValue(body?.document_id) ??
        stringValue(body?.documentId) ??
        stringValue(body?.data?.document_id) ??
        stringValue(body?.data?.documentId),
    };
  } catch {
    return { status: "pending" };
  }
}

function normalizeProvider(value: string | undefined): string {
  const key = value?.trim().toLowerCase() ?? "";
  if (["microsoft365", "m365", "microsoft-365", "microsoft-graph"].includes(key)) return "microsoft";
  if (["gdrive", "google-drive", "google_workspace", "google-workspace", "gmail"].includes(key)) return "google";
  return key;
}

function providerLabel(provider: string): string {
  switch (provider) {
    case "slack":
      return "Slack";
    case "github":
      return "GitHub";
    case "notion":
      return "Notion";
    case "google":
    case "google-drive":
      return "Google Workspace";
    case "microsoft":
      return "Microsoft 365";
    default:
      return provider;
  }
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}
