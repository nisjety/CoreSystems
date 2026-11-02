/**
 * Typed client for the xero-bff gateway.
 *
 * The browser only ever talks to /api/* (same-origin; Vite proxies it to the
 * Rust BFF in dev, a reverse proxy in prod). Refresh tokens and the Xero
 * client credential never reach this code — the BFF holds them.
 */

export interface XeroConnection {
  tenantId: string;
  tenantName: string;
  tenantType: string;
  /** RFC3339 timestamp of when the org was authorized. */
  connectedAt: string;
  scopes: string[];
}

export interface ConnectionsResponse {
  connections: XeroConnection[];
}

/** OAuth login is a full-page navigation (302 chain to Xero), never a fetch(). */
export const LOGIN_URL = "/api/auth/login";

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path, {
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  });
  if (!res.ok) {
    const detail = await res.text().catch(() => "");
    throw new Error(
      `${res.status} ${res.statusText}${detail ? ` — ${detail}` : ""}`,
    );
  }
  return (await res.json()) as T;
}

export function fetchConnections(): Promise<ConnectionsResponse> {
  return getJson<ConnectionsResponse>("/api/connections");
}

export async function logout(): Promise<void> {
  const res = await fetch("/api/auth/logout", {
    method: "POST",
    credentials: "same-origin",
  });
  if (!res.ok) {
    throw new Error(`logout failed: ${res.status} ${res.statusText}`);
  }
}

// ── On-demand sync ──────────────────────────────────────────────────────────

export type SyncStatus =
  | { state: "idle" }
  | { state: "running"; kind: string; started_at: string }
  | {
      state: "finished";
      kind: string;
      ok: boolean;
      summary: string;
      finished_at: string;
    };

export function fetchSyncStatus(): Promise<SyncStatus> {
  return getJson<SyncStatus>("/api/sync/status");
}

async function postAction(path: string, label: string): Promise<void> {
  const res = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { Accept: "application/json" },
  });
  if (res.status === 409) {
    throw new Error("A sync is already running — wait for it to finish.");
  }
  if (!res.ok) {
    let detail = await res.text().catch(() => "");
    try {
      const j = JSON.parse(detail) as { message?: string; error?: string };
      detail = j.message ?? j.error ?? detail;
    } catch {
      /* not JSON — fall back to the raw body text */
    }
    throw new Error(
      `${label} failed to start: ${res.status}${detail ? ` — ${detail}` : ""}`,
    );
  }
}

/** Start an incremental sync (recent changes). */
export function startSync(): Promise<void> {
  return postAction("/api/sync", "sync");
}

/**
 * Start a backfill. `from`/`to` are `YYYY-MM-DD`; omit them to let the server
 * default to the last 3 years → today.
 */
export function startBackfill(from?: string, to?: string): Promise<void> {
  const qs = new URLSearchParams();
  if (from) qs.set("from", from);
  if (to) qs.set("to", to);
  const q = qs.toString();
  return postAction(`/api/backfill${q ? `?${q}` : ""}`, "backfill");
}
