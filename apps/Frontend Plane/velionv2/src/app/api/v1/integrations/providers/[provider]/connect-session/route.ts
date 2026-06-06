import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  readJsonOrNull,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { buildIntegrationCoreHeaders } from "@/app/api/v1/integrations/_lib/service-auth";
import { fail, ok } from "@/lib/api/envelope";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

const ALLOWED_PROVIDERS = new Set([
  "microsoft",
  "google",
  "google-drive",
  "slack",
  "github",
  "notion",
  "shopify",
  "stripe",
]);

type ConnectSessionPayload = {
  bundles?: unknown;
  providerContext?: unknown;
  selectedSources?: unknown;
  shop?: unknown;
  sources?: unknown;
};

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ provider: string }> },
) {
  try {
    const [session, { provider: rawProvider }] = await Promise.all([requireSession(request), params]);
    const provider = normalizeProvider(rawProvider);
    if (!ALLOWED_PROVIDERS.has(provider)) {
      return NextResponse.json(
        fail({ code: "provider_unsupported", message: "Unsupported provider." }),
        { status: 400 },
      );
    }

    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json(
        fail({ code: "org_required", message: "No active organization found." }),
        { status: 409 },
      );
    }

    const body = (await request.json().catch(() => null)) as ConnectSessionPayload | null;
    const response = await fetch(
      `${getIntegrationCoreUrl()}/api/v1/providers/${encodeURIComponent(provider)}/connect-session`,
      {
        method: "POST",
        headers: {
          ...buildIntegrationCoreHeaders(request, session, orgId),
          "Content-Type": "application/json",
        },
        cache: "no-store",
        body: JSON.stringify(connectSessionBody(body, orgId, session.user)),
      },
    );
    const payload = await readJsonOrNull(response);
    const sessionPayload = unwrapConnectSessionPayload(payload);

    if (!response.ok || !sessionPayload?.connectUrl) {
      return NextResponse.json(
        fail({
          code: "connect_session_failed",
          message: serviceErrorMessage(payload) ?? "Could not start provider authorization.",
        }),
        { status: response.ok ? 502 : response.status },
      );
    }

    return NextResponse.json(ok(sessionPayload));
  } catch (error) {
    return authErrorResponse(error);
  }
}

function connectSessionBody(
  body: ConnectSessionPayload | null,
  orgId: string,
  user: { id: string; email?: string | null },
) {
  return {
    organizationId: orgId,
    workspaceId: orgId,
    userId: user.id,
    userEmail: user.email ?? undefined,
    selectedSources: stringArray(body?.selectedSources ?? body?.sources),
    bundles: stringArray(body?.bundles),
    shop: typeof body?.shop === "string" ? body.shop : undefined,
    providerContext:
      body?.providerContext && typeof body.providerContext === "object" && !Array.isArray(body.providerContext)
        ? body.providerContext
        : undefined,
  };
}

function unwrapConnectSessionPayload(payload: unknown) {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const data = record.data && typeof record.data === "object"
    ? record.data as Record<string, unknown>
    : record;
  const connectUrl = typeof data.connectUrl === "string" ? data.connectUrl : null;
  if (!connectUrl) return null;
  return {
    connectUrl,
    authMode: typeof data.authMode === "string" ? data.authMode : undefined,
    sessionToken: typeof data.sessionToken === "string" ? data.sessionToken : undefined,
    expiresAt: typeof data.expiresAt === "string" ? data.expiresAt : undefined,
    providerConfigKey: typeof data.providerConfigKey === "string" ? data.providerConfigKey : undefined,
  };
}

function stringArray(value: unknown): string[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const values = value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim())
    .filter(Boolean)
    .slice(0, 24);
  return values.length ? values : undefined;
}

function normalizeProvider(value: string) {
  const key = value.trim().toLowerCase();
  if (["gdrive", "gmail", "google-drive", "google-workspace"].includes(key)) return "google";
  if (["m365", "microsoft365", "microsoft-365", "microsoft-graph"].includes(key)) return "microsoft";
  return key;
}

function serviceErrorMessage(payload: unknown): string | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as { error?: unknown; message?: unknown };
  if (typeof record.message === "string") return record.message;
  if (typeof record.error === "string") return record.error;
  if (record.error && typeof record.error === "object" && "message" in record.error) {
    const message = (record.error as { message?: unknown }).message;
    return typeof message === "string" ? message : null;
  }
  return null;
}
