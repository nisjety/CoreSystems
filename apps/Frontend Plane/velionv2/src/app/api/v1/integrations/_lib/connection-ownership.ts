import { NextResponse, type NextRequest } from "next/server";

import {
  readJsonOrNull,
  type ControlPlaneSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getIntegrationCoreUrl,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { fail } from "@/lib/api/envelope";

export type OwnedIntegrationConnection = {
  id: string;
  organizationId: string;
  providerKey: string;
  deletedAt: string | null;
};

type ConnectionPayload = {
  connection?: Record<string, unknown>;
  data?: {
    connection?: Record<string, unknown>;
  };
};

export async function loadOwnedIntegrationConnection(
  request: NextRequest,
  session: ControlPlaneSession,
  orgId: string,
  connectionId: string,
): Promise<
  | { connection: OwnedIntegrationConnection; response?: never }
  | { connection?: never; response: NextResponse }
> {
  const response = await fetch(
    `${getIntegrationCoreUrl()}/api/v1/connections/${encodeURIComponent(connectionId)}`,
    {
      method: "GET",
      headers: buildServiceHeaders(request, session, orgId),
      cache: "no-store",
    },
  );
  const payload = await readJsonOrNull(response) as ConnectionPayload | null;
  const connection = connectionFromPayload(payload);

  if (!response.ok || !connection) {
    return {
      response: NextResponse.json(
        fail({ code: "connection_not_found", message: "Connection could not be loaded." }),
        { status: response.ok ? 404 : response.status },
      ),
    };
  }

  if (connection.organizationId !== orgId) {
    return {
      response: NextResponse.json(
        fail({ code: "connection_forbidden", message: "Connection does not belong to the active organization." }),
        { status: 403 },
      ),
    };
  }

  if (connection.deletedAt) {
    return {
      response: NextResponse.json(
        fail({ code: "connection_not_found", message: "Connection was already removed." }),
        { status: 404 },
      ),
    };
  }

  return { connection };
}

function connectionFromPayload(payload: ConnectionPayload | null): OwnedIntegrationConnection | null {
  const raw = payload?.data?.connection ?? payload?.connection;
  if (!raw) return null;
  const id = stringValue(raw.id);
  const organizationId = stringValue(raw.organizationId) || stringValue(raw.organization_id);
  const providerKey = normalizeProvider(stringValue(raw.providerKey) || stringValue(raw.provider_key));
  if (!id || !organizationId || !providerKey) return null;
  return {
    id,
    organizationId,
    providerKey,
    deletedAt: stringValue(raw.deletedAt) || stringValue(raw.deleted_at) || null,
  };
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function normalizeProvider(value: string | null) {
  const key = value?.trim().toLowerCase() ?? "";
  if (["google-drive", "google-workspace", "gdrive", "gmail"].includes(key)) return "google";
  if (["microsoft-graph", "microsoft365", "m365", "microsoft-365"].includes(key)) return "microsoft";
  return key;
}
