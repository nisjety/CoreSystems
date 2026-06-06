import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  buildControlPlaneHeaders,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const dynamic = "force-dynamic";

const ALLOWED_PROVIDERS = new Set([
  "slack",
  "microsoft",
  "notion",
  "google",
  "google-drive",
  "github",
  "shopify",
  "stripe",
]);

type ConnectPayload = {
  authMode?: "direct-oauth";
  sessionToken?: string;
  connectUrl?: string;
  expiresAt?: string;
  providerConfigKey?: string;
  provider?: {
    configKey?: string;
  };
  error?: string | { message?: string };
};

function unwrapConnectPayload(payload: unknown): ConnectPayload | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as ConnectPayload & { data?: ConnectPayload };
  if (record.data && typeof record.data === "object") return record.data;
  return record;
}

/**
 * POST /api/connections/create
 * Body: { provider, sources }. Resolves org + user server-side and creates a
 * Velion direct OAuth connect session via integration-corev2. This is an
 * internal service call, so integration-core's public Bearer plan gate is
 * bypassed; onboarding plan enforcement happens at the paywall.
 */
export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as
      | { provider?: string; sources?: string[]; shop?: string; providerContext?: Record<string, string> }
      | null;

    const provider = body?.provider?.trim().toLowerCase();
    if (!provider || !ALLOWED_PROVIDERS.has(provider)) {
      return NextResponse.json({ error: "Unsupported provider." }, { status: 400 });
    }

    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json({ error: "No active organization found." }, { status: 409 });
    }

    // Compute service headers first so a config error (missing internal key)
    // surfaces distinctly from a network failure below.
    const headers = buildControlPlaneHeaders(request, session);

    let response: Response;
    try {
      response = await fetch(
        `${getIntegrationCoreUrl()}/api/v1/providers/${encodeURIComponent(provider)}/connect-session`,
        {
          method: "POST",
          headers,
          cache: "no-store",
          body: JSON.stringify({
            organizationId: orgId,
            workspaceId: orgId,
            userId: session.user.id,
            userEmail: session.user.email,
            selectedSources: Array.isArray(body?.sources) ? body?.sources : undefined,
            shop: typeof body?.shop === "string" ? body.shop : undefined,
            providerContext:
              body?.providerContext && typeof body.providerContext === "object"
                ? body.providerContext
                : undefined,
          }),
        },
      );
    } catch {
      return NextResponse.json(
        { error: "Could not reach the integration service. Try again shortly." },
        { status: 502 },
      );
    }

    const rawPayload = await response.json().catch(() => null);
    const payload = unwrapConnectPayload(rawPayload);

    if (!response.ok || !payload?.connectUrl) {
      const error = (rawPayload && typeof rawPayload === "object" && "error" in rawPayload)
        ? (rawPayload as { error?: ConnectPayload["error"] }).error
        : payload?.error;
      const message =
        (typeof error === "object" ? error?.message : error) ||
        "Could not start the connection.";
      return NextResponse.json({ error: message }, { status: response.ok ? 502 : response.status });
    }

    return NextResponse.json({
      connectUrl: payload.connectUrl,
      authMode: payload.authMode,
      sessionToken: payload.sessionToken,
      expiresAt: payload.expiresAt,
      providerConfigKey:
        payload.providerConfigKey ??
        payload.provider?.configKey ??
        providerConfigKeyFor(provider),
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}

function providerConfigKeyFor(provider: string): string {
  switch (provider) {
    case "microsoft":
      return "microsoft-graph";
    case "google":
    case "google-drive":
      return "google-workspace";
    default:
      return provider;
  }
}
