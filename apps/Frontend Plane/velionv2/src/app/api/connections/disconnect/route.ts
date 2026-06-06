import { NextResponse, type NextRequest } from "next/server";

import {
  authErrorResponse,
  buildControlPlaneHeaders,
  readJsonOrNull,
  requireSession,
} from "@/app/api/_lib/control-plane-auth";
import {
  getIntegrationCoreUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";
import { providerKeysForConnectionIdentifiers } from "../_lib/provider-keys";

export const dynamic = "force-dynamic";

type ConnectionRecord = {
  id?: unknown;
  providerKey?: unknown;
  provider_key?: unknown;
  deletedAt?: unknown;
  deleted_at?: unknown;
};

type DisconnectPayload = {
  connectors?: unknown;
  providers?: unknown;
};

type DisconnectedConnection = {
  id: string;
  providerKey: string;
};

type FailedConnection = DisconnectedConnection & {
  status: number;
  message: string;
};

const MAX_DISCONNECT_IDENTIFIERS = 12;

/**
 * POST /api/connections/disconnect
 * Body: { connectors: string[] }
 *
 * Best-effort cleanup for onboarding plan downgrades. The client only sends
 * UI connector ids; this route resolves the caller's org and maps them to the
 * integration-core provider keys server-side so arbitrary connection ids or
 * provider keys cannot be deleted from the browser.
 */
export async function POST(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const body = (await request.json().catch(() => null)) as DisconnectPayload | null;
    const identifiers = connectionIdentifiersFromPayload(body);
    const providerKeys = providerKeysForConnectionIdentifiers(identifiers);

    if (providerKeys.length === 0) {
      return NextResponse.json(
        { disconnected: [], failed: [], skippedProviderKeys: [] },
        { status: identifiers.length > 0 ? 200 : 400 },
      );
    }

    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) {
      return NextResponse.json({ error: "No active organization found." }, { status: 409 });
    }

    const headers = buildControlPlaneHeaders(request, session);
    const listUrl = new URL(`${getIntegrationCoreUrl()}/api/v1/connections`);
    listUrl.searchParams.set("organizationId", orgId);

    let listResponse: Response;
    try {
      listResponse = await fetch(listUrl, {
        method: "GET",
        headers,
        cache: "no-store",
      });
    } catch {
      return NextResponse.json(
        { error: "Could not reach the integration service." },
        { status: 502 },
      );
    }

    const listBody = await readJsonOrNull(listResponse);
    if (!listResponse.ok) {
      return NextResponse.json(
        { error: connectionErrorMessage(listBody, "Could not load connected sources.") },
        { status: listResponse.status },
      );
    }

    const providerSet = new Set(providerKeys);
    const connections = connectionRecordsFromPayload(listBody).filter((connection) =>
      providerSet.has(connection.providerKey),
    );

    const deleteResults = await Promise.all(
      connections.map((connection) => deleteConnection(connection, headers)),
    );
    const disconnected = deleteResults.flatMap((result) =>
      result.ok ? [result.connection] : [],
    );
    const failed: FailedConnection[] = deleteResults.flatMap((result) =>
      result.ok ? [] : [{ ...result.connection, status: result.status, message: result.message }],
    );
    const seenProviderKeys = new Set(connections.map((connection) => connection.providerKey));
    const skippedProviderKeys = providerKeys.filter((providerKey) => !seenProviderKeys.has(providerKey));

    return NextResponse.json(
      { disconnected, failed, skippedProviderKeys },
      { status: failed.length > 0 ? 207 : 200 },
    );
  } catch (error) {
    return authErrorResponse(error);
  }
}

function connectionIdentifiersFromPayload(body: DisconnectPayload | null): string[] {
  const rawValues = [
    ...stringArrayFromUnknown(body?.connectors),
    ...stringArrayFromUnknown(body?.providers),
  ];
  return Array.from(new Set(rawValues)).slice(0, MAX_DISCONNECT_IDENTIFIERS);
}

function stringArrayFromUnknown(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .filter((item): item is string => typeof item === "string")
    .map((item) => item.trim())
    .filter(Boolean);
}

function connectionRecordsFromPayload(payload: unknown): DisconnectedConnection[] {
  const record = payload && typeof payload === "object" ? payload as {
    data?: { connections?: unknown };
    connections?: unknown;
  } : null;
  const rawConnections = Array.isArray(record?.data?.connections)
    ? record.data.connections
    : Array.isArray(record?.connections)
      ? record.connections
      : [];

  return rawConnections.flatMap((raw): DisconnectedConnection[] => {
    const connection = raw as ConnectionRecord;
    const id = typeof connection.id === "string" ? connection.id : null;
    const providerKey =
      typeof connection.providerKey === "string"
        ? connection.providerKey
        : typeof connection.provider_key === "string"
          ? connection.provider_key
          : null;
    const deleted = connection.deletedAt ?? connection.deleted_at;
    if (!id || !providerKey || deleted) return [];
    return [{ id, providerKey }];
  });
}

async function deleteConnection(
  connection: DisconnectedConnection,
  headers: Record<string, string>,
): Promise<
  | { ok: true; connection: DisconnectedConnection }
  | { ok: false; connection: DisconnectedConnection; status: number; message: string }
> {
  let response: Response;
  try {
    response = await fetch(
      `${getIntegrationCoreUrl()}/api/v1/connections/${encodeURIComponent(connection.id)}`,
      {
        method: "DELETE",
        headers,
        cache: "no-store",
      },
    );
  } catch {
    return {
      ok: false,
      connection,
      status: 502,
      message: "Could not reach the integration service.",
    };
  }

  if (response.ok || response.status === 404 || response.status === 410) {
    return { ok: true, connection };
  }

  const body = await readJsonOrNull(response);
  return {
    ok: false,
    connection,
    status: response.status,
    message: connectionErrorMessage(body, "Could not disconnect the source."),
  };
}

function connectionErrorMessage(payload: unknown, fallback: string): string {
  if (payload && typeof payload === "object" && "error" in payload) {
    const error = (payload as { error?: unknown }).error;
    if (typeof error === "string" && error.trim()) return error;
    if (error && typeof error === "object" && "message" in error) {
      const message = (error as { message?: unknown }).message;
      if (typeof message === "string" && message.trim()) return message;
    }
  }
  return fallback;
}
